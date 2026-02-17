use crate::message_queue::MessageQueue;
use crate::proto;
use arc_swap::ArcSwap;
use chrono::Utc;
use log::{debug, error, info, warn};
use prost::Message;
use rand::Rng;
use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time;

/// Command received from the server
#[derive(Debug, Clone)]
pub struct ReceivedCommand {
    pub command_id: String,
    pub command_type: proto::CommandType,
    pub set_group_value: String,
    pub update_url: String,
    pub update_checksum: String,
    // Config management fields
    pub config_key: String,
    pub config_value: String,
}

/// Channel sender for commands
pub type CommandSender = mpsc::UnboundedSender<ReceivedCommand>;

/// Configuration for the MQTT publisher
#[derive(Clone)]
pub struct MqttConfig {
    /// List of broker addresses for failover (e.g., ["tcp://broker1:1883", "tcp://broker2:1883"])
    pub broker_addrs: Vec<String>,
    pub client_id: String,
    pub api_key: String,
    pub topic_prefix: String,
    pub keepalive_secs: u64,
    pub retry_interval_secs: u64,
}

impl MqttConfig {
    /// Returns the broker address at the given index (wraps around)
    pub fn get_broker(&self, index: usize) -> &str {
        &self.broker_addrs[index % self.broker_addrs.len()]
    }

    /// Returns the number of configured brokers
    pub fn broker_count(&self) -> usize {
        self.broker_addrs.len()
    }
}

/// MQTT publisher that handles connection, publishing, and offline queueing
pub struct MqttPublisher {
    client: AsyncClient,
    config: MqttConfig,
    agent_id: String,
    queue: Arc<MessageQueue>,
    is_connected: Arc<AtomicBool>,
    /// Current broker index for failover
    current_broker_index: std::sync::atomic::AtomicUsize,
}

impl MqttPublisher {
    /// Creates a new MQTT publisher connecting to the first broker
    pub async fn new(
        config: MqttConfig,
        agent_id: String,
        queue: Arc<MessageQueue>,
    ) -> Result<(Self, EventLoop), rumqttc::ClientError> {
        Self::new_with_broker_index(config, agent_id, queue, 0).await
    }

    /// Creates a new MQTT publisher connecting to a specific broker by index
    pub async fn new_with_broker_index(
        config: MqttConfig,
        agent_id: String,
        queue: Arc<MessageQueue>,
        broker_index: usize,
    ) -> Result<(Self, EventLoop), rumqttc::ClientError> {
        let broker_addr = config.get_broker(broker_index);
        info!(
            "Creating MQTT client for broker {}/{}: {}",
            broker_index + 1,
            config.broker_count(),
            broker_addr
        );

        let mut mqtt_options = MqttOptions::new(
            &config.client_id,
            parse_broker_host(broker_addr),
            parse_broker_port(broker_addr),
        );

        mqtt_options.set_keep_alive(Duration::from_secs(config.keepalive_secs));
        mqtt_options.set_clean_session(true);
        // Allow larger packets (256KB) - needed for process lists and service metrics
        mqtt_options.set_max_packet_size(256 * 1024, 256 * 1024);

        // Set Last Will and Testament for offline detection
        let lwt_topic = format!("{}/{}/status", config.topic_prefix, agent_id);
        let lwt_payload = create_status_message(&agent_id, false);
        mqtt_options.set_last_will(rumqttc::LastWill {
            topic: lwt_topic,
            message: lwt_payload.into(),
            qos: QoS::AtLeastOnce,
            retain: true,
        });

        let (client, eventloop) = AsyncClient::new(mqtt_options, 100);

        let publisher = Self {
            client,
            config,
            agent_id,
            queue,
            is_connected: Arc::new(AtomicBool::new(false)),
            current_broker_index: std::sync::atomic::AtomicUsize::new(broker_index),
        };

        Ok((publisher, eventloop))
    }

    /// Returns the current broker index
    pub fn current_broker_index(&self) -> usize {
        self.current_broker_index.load(Ordering::Relaxed)
    }

    /// Returns the current broker address
    pub fn current_broker_addr(&self) -> &str {
        self.config.get_broker(self.current_broker_index())
    }

    /// Returns the number of configured brokers
    pub fn broker_count(&self) -> usize {
        self.config.broker_count()
    }

    /// Creates a new publisher connected to the next broker in the list
    /// Returns the new publisher, eventloop, and the new broker index
    pub async fn create_with_next_broker(
        &self,
    ) -> Result<(Self, EventLoop, usize), rumqttc::ClientError> {
        let next_index = (self.current_broker_index() + 1) % self.config.broker_count();
        let (publisher, eventloop) = Self::new_with_broker_index(
            self.config.clone(),
            self.agent_id.clone(),
            Arc::clone(&self.queue),
            next_index,
        )
        .await?;
        Ok((publisher, eventloop, next_index))
    }

    /// Returns the agent ID
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    /// Returns whether the client is currently connected
    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    /// Returns queue statistics: (message_count, size_in_bytes)
    pub fn queue_stats(&self) -> (u32, u64) {
        match self.queue.stats() {
            Ok((count, size)) => (count as u32, size),
            Err(_) => (0, 0),
        }
    }

    /// Publishes agent info message (called on startup and reconnection)
    pub async fn publish_agent_info(&self, info: &proto::AgentInfo) -> Result<(), MqttError> {
        let topic = format!("{}/{}/info", self.config.topic_prefix, self.agent_id);
        let payload = self.wrap_with_auth(proto::authenticated_message::Payload::AgentInfo(
            info.clone(),
        ));

        self.publish_message(&topic, &payload, true).await
    }

    /// Publishes a metrics snapshot with current queue stats included
    pub async fn publish_metrics(
        &self,
        snapshot: &proto::MetricsSnapshot,
    ) -> Result<(), MqttError> {
        // Add current queue stats to the snapshot
        let (queue_count, queue_bytes) = self.queue_stats();
        let mut snapshot_with_queue = snapshot.clone();
        snapshot_with_queue.queue_pending_count = queue_count;
        snapshot_with_queue.queue_pending_bytes = queue_bytes;

        let topic = format!("{}/{}/snapshot", self.config.topic_prefix, self.agent_id);
        let payload = self.wrap_with_auth(proto::authenticated_message::Payload::Metrics(
            snapshot_with_queue,
        ));

        self.publish_message(&topic, &payload, false).await
    }

    /// Publishes a process list
    pub async fn publish_process_list(
        &self,
        process_list: &proto::ProcessList,
    ) -> Result<(), MqttError> {
        let topic = format!("{}/{}/processes", self.config.topic_prefix, self.agent_id);
        let payload = self.wrap_with_auth(proto::authenticated_message::Payload::ProcessList(
            process_list.clone(),
        ));

        self.publish_message(&topic, &payload, false).await
    }

    /// Publishes service metrics from plugins
    pub async fn publish_service_metrics(
        &self,
        service_metrics: &proto::ServiceMetricsList,
    ) -> Result<(), MqttError> {
        let topic = format!("{}/{}/services", self.config.topic_prefix, self.agent_id);
        let payload = self.wrap_with_auth(proto::authenticated_message::Payload::ServiceMetrics(
            service_metrics.clone(),
        ));

        self.publish_message(&topic, &payload, false).await
    }

    /// Publishes online status
    pub async fn publish_online_status(&self) -> Result<(), MqttError> {
        let topic = format!("{}/{}/status", self.config.topic_prefix, self.agent_id);
        let payload = create_status_message(&self.agent_id, true);

        self.client
            .publish(&topic, QoS::AtLeastOnce, true, payload)
            .await
            .map_err(|e| MqttError::PublishError(e.to_string()))?;

        Ok(())
    }

    /// Subscribes to command topic
    pub async fn subscribe_commands(&self) -> Result<(), MqttError> {
        // Subscribe to: otterwatch/commands/{agent_id}/command
        let command_topic = format!(
            "{}/{}/command",
            self.config.topic_prefix.replace("/metrics", "/commands"),
            self.agent_id
        );

        self.client
            .subscribe(&command_topic, QoS::AtLeastOnce)
            .await
            .map_err(|e| MqttError::ConnectionError(e.to_string()))?;

        info!("Subscribed to command topic: {}", command_topic);
        Ok(())
    }

    /// Publishes a command response
    pub async fn publish_command_response(
        &self,
        command_id: &str,
        success: bool,
        message: &str,
    ) -> Result<(), MqttError> {
        self.publish_command_response_with_config(command_id, success, message, None)
            .await
    }

    /// Publishes a command response with optional config data
    pub async fn publish_command_response_with_config(
        &self,
        command_id: &str,
        success: bool,
        message: &str,
        config: Option<proto::AgentConfig>,
    ) -> Result<(), MqttError> {
        self.publish_command_response_full(command_id, success, message, config, None)
            .await
    }

    /// Publishes a command response with optional swap processes data
    pub async fn publish_command_response_with_swap_processes(
        &self,
        command_id: &str,
        success: bool,
        message: &str,
        swap_processes: Option<proto::SwapProcessList>,
    ) -> Result<(), MqttError> {
        self.publish_command_response_full(command_id, success, message, None, swap_processes)
            .await
    }

    /// Publishes a command response with all optional fields
    async fn publish_command_response_full(
        &self,
        command_id: &str,
        success: bool,
        message: &str,
        config: Option<proto::AgentConfig>,
        swap_processes: Option<proto::SwapProcessList>,
    ) -> Result<(), MqttError> {
        self.publish_command_response_extended(
            command_id,
            success,
            message,
            config,
            swap_processes,
            None,
            None,
        )
        .await
    }

    /// Publishes a command response with config schema
    pub async fn publish_command_response_with_schema(
        &self,
        command_id: &str,
        success: bool,
        message: &str,
        config_schema: Option<proto::ConfigSchema>,
    ) -> Result<(), MqttError> {
        self.publish_command_response_extended(
            command_id,
            success,
            message,
            None,
            None,
            config_schema,
            None,
        )
        .await
    }

    /// Publishes a command response with sync report
    pub async fn publish_command_response_with_sync_report(
        &self,
        command_id: &str,
        success: bool,
        message: &str,
        sync_report: Option<String>,
    ) -> Result<(), MqttError> {
        self.publish_command_response_extended(
            command_id,
            success,
            message,
            None,
            None,
            None,
            sync_report,
        )
        .await
    }

    /// Publishes a command response with all possible optional fields
    async fn publish_command_response_extended(
        &self,
        command_id: &str,
        success: bool,
        message: &str,
        config: Option<proto::AgentConfig>,
        swap_processes: Option<proto::SwapProcessList>,
        config_schema: Option<proto::ConfigSchema>,
        sync_report: Option<String>,
    ) -> Result<(), MqttError> {
        let topic = format!(
            "{}/{}",
            self.config.topic_prefix.replace("/metrics", "/responses"),
            self.agent_id
        );

        let response = proto::CommandResponse {
            command_id: command_id.to_string(),
            agent_id: self.agent_id.clone(),
            success,
            message: message.to_string(),
            timestamp: Some(prost_types::Timestamp {
                seconds: Utc::now().timestamp(),
                nanos: 0,
            }),
            config,
            swap_processes,
            config_schema,
            sync_report,
        };

        let payload = response.encode_to_vec();

        self.client
            .publish(&topic, QoS::AtLeastOnce, false, payload)
            .await
            .map_err(|e| MqttError::PublishError(e.to_string()))?;

        info!(
            "Published command response for {}: success={}",
            command_id, success
        );
        Ok(())
    }

    /// Sends a small batch of queued messages inline with the caller.
    /// This is designed to be called periodically from the metrics collection loop.
    /// Returns (sent_count, remaining_count).
    pub async fn send_queued_batch(&self, batch_size: usize) -> Result<(usize, usize), MqttError> {
        if !self.is_connected() {
            let remaining = self.queue.len().unwrap_or(0);
            return Ok((0, remaining));
        }

        let messages = self
            .queue
            .dequeue_batch(batch_size)
            .map_err(|e| MqttError::QueueError(e.to_string()))?;

        if messages.is_empty() {
            return Ok((0, 0));
        }

        let mut successfully_sent = Vec::new();

        for msg in &messages {
            match self
                .client
                .publish(&msg.topic, QoS::AtLeastOnce, false, msg.payload.clone())
                .await
            {
                Ok(_) => {
                    successfully_sent.push(msg);
                }
                Err(e) => {
                    warn!("Failed to send queued message: {}", e);
                    break;
                }
            }
        }

        let sent_count = successfully_sent.len();

        // Remove successfully sent messages from queue
        if !successfully_sent.is_empty() {
            let refs: Vec<_> = successfully_sent.into_iter().cloned().collect();
            self.queue
                .mark_sent(&refs)
                .map_err(|e| MqttError::QueueError(e.to_string()))?;
        }

        let remaining = self
            .queue
            .len()
            .map_err(|e| MqttError::QueueError(e.to_string()))?;

        if sent_count > 0 {
            info!(
                "Sent {} queued messages ({} remaining)",
                sent_count, remaining
            );
        }

        Ok((sent_count, remaining))
    }

    /// Drains a single message from the offline queue.
    /// This is designed to be called from the event loop with a timeout.
    /// Returns (1, remaining) if a message was sent, (0, remaining) otherwise.
    pub async fn drain_queue_single(&self) -> Result<(usize, usize), MqttError> {
        let messages = self
            .queue
            .dequeue_batch(1)
            .map_err(|e| MqttError::QueueError(e.to_string()))?;

        if messages.is_empty() {
            return Ok((0, 0));
        }

        let msg = &messages[0];
        match self
            .client
            .publish(&msg.topic, QoS::AtLeastOnce, false, msg.payload.clone())
            .await
        {
            Ok(_) => {
                // Mark as sent
                self.queue
                    .mark_sent(&messages)
                    .map_err(|e| MqttError::QueueError(e.to_string()))?;

                let remaining = self
                    .queue
                    .len()
                    .map_err(|e| MqttError::QueueError(e.to_string()))?;

                if remaining == 0 {
                    info!("Offline queue drained completely");
                } else if remaining % 100 == 0 {
                    info!("Queue drain progress: {} remaining", remaining);
                }

                Ok((1, remaining))
            }
            Err(e) => {
                warn!("Failed to send queued message: {}", e);
                let remaining = self.queue.len().unwrap_or(0);
                Ok((0, remaining))
            }
        }
    }

    /// Drains the offline queue by sending queued messages.
    /// Limited to MAX_DRAIN_PER_CYCLE messages per call to avoid blocking the event loop.
    /// This must return quickly so the event loop can poll() for keepalive.
    /// Returns the number of messages sent and remaining in queue.
    #[allow(dead_code)]
    pub async fn drain_queue(&self) -> Result<(usize, usize), MqttError> {
        // Keep these values low to avoid blocking event loop polling (keepalive)
        const MAX_DRAIN_PER_CYCLE: usize = 20; // Small batch per cycle
        const BATCH_SIZE: usize = 5; // Very small batches
        const DELAY_BETWEEN_BATCHES_MS: u64 = 10;

        // Get initial queue stats
        let (initial_count, initial_size) = self
            .queue
            .stats()
            .map_err(|e| MqttError::QueueError(e.to_string()))?;

        if initial_count == 0 {
            return Ok((0, 0));
        }

        info!(
            "Starting queue drain: {} messages ({:.2} MB) pending",
            initial_count,
            initial_size as f64 / (1024.0 * 1024.0)
        );

        let mut sent_count = 0;
        let mut batch_num = 0;

        loop {
            // Check if we've hit the per-cycle limit
            if sent_count >= MAX_DRAIN_PER_CYCLE {
                break;
            }

            let remaining_in_cycle = MAX_DRAIN_PER_CYCLE - sent_count;
            let batch_size = std::cmp::min(BATCH_SIZE, remaining_in_cycle);

            let messages = self
                .queue
                .dequeue_batch(batch_size)
                .map_err(|e| MqttError::QueueError(e.to_string()))?;

            if messages.is_empty() {
                break;
            }

            batch_num += 1;
            let mut successfully_sent = Vec::new();

            for msg in &messages {
                match self
                    .client
                    .publish(&msg.topic, QoS::AtLeastOnce, false, msg.payload.clone())
                    .await
                {
                    Ok(_) => {
                        successfully_sent.push(msg);
                        sent_count += 1;
                    }
                    Err(e) => {
                        warn!("Failed to send queued message: {}", e);
                        // Stop draining if we hit an error (likely disconnected)
                        break;
                    }
                }
            }

            // Remove successfully sent messages from queue
            let refs: Vec<_> = successfully_sent.into_iter().collect();
            self.queue
                .mark_sent(&refs.iter().map(|m| (*m).clone()).collect::<Vec<_>>())
                .map_err(|e| MqttError::QueueError(e.to_string()))?;

            // If we couldn't send all messages in batch, stop draining
            if refs.len() < messages.len() {
                break;
            }

            // Small delay between batches to not overwhelm the broker
            if batch_num % 5 == 0 {
                time::sleep(Duration::from_millis(DELAY_BETWEEN_BATCHES_MS)).await;
            }
        }

        // Get remaining count
        let remaining = self
            .queue
            .len()
            .map_err(|e| MqttError::QueueError(e.to_string()))?;

        if sent_count > 0 {
            if remaining > 0 {
                // Only log when we reach a milestone or queue is small
                if remaining % 50 == 0 || remaining < 10 {
                    info!(
                        "Queue drain progress: sent {} this cycle, {} remaining",
                        sent_count, remaining
                    );
                } else {
                    debug!("Drained {} messages ({} remaining)", sent_count, remaining);
                }
            } else {
                info!(
                    "Offline queue drained completely ({} messages sent)",
                    sent_count
                );
            }
        }

        Ok((sent_count, remaining))
    }

    /// Internal method to publish a message, with queueing if disconnected
    async fn publish_message(
        &self,
        topic: &str,
        payload: &[u8],
        retain: bool,
    ) -> Result<(), MqttError> {
        if self.is_connected() {
            match self
                .client
                .publish(topic, QoS::AtLeastOnce, retain, payload.to_vec())
                .await
            {
                Ok(_) => {
                    debug!("Published message to {}", topic);
                    Ok(())
                }
                Err(e) => {
                    warn!("Failed to publish, queueing message: {}", e);
                    self.queue_message(topic, payload)?;
                    Err(MqttError::PublishError(e.to_string()))
                }
            }
        } else {
            debug!("Not connected, queueing message for {}", topic);
            self.queue_message(topic, payload)?;
            Ok(())
        }
    }

    /// Queues a message for later delivery
    fn queue_message(&self, topic: &str, payload: &[u8]) -> Result<(), MqttError> {
        self.queue
            .enqueue(topic, payload)
            .map_err(|e| MqttError::QueueError(e.to_string()))?;
        Ok(())
    }

    /// Wraps a payload with authentication
    fn wrap_with_auth(&self, payload: proto::authenticated_message::Payload) -> Vec<u8> {
        let msg = proto::AuthenticatedMessage {
            api_key: self.config.api_key.clone(),
            payload: Some(payload),
        };
        msg.encode_to_vec()
    }

    /// Sets the connected state
    pub fn set_connected(&self, connected: bool) {
        self.is_connected.store(connected, Ordering::Relaxed);
    }
}

/// A shared MQTT publisher that can be atomically updated during failover.
/// This wrapper allows the MQTT event loop thread to update the publisher
/// when switching to a different broker, while other threads continue to
/// use the same SharedMqttPublisher instance.
pub struct SharedMqttPublisher {
    inner: ArcSwap<MqttPublisher>,
}

impl SharedMqttPublisher {
    /// Creates a new shared publisher from an existing publisher
    pub fn new(publisher: MqttPublisher) -> Self {
        Self {
            inner: ArcSwap::new(Arc::new(publisher)),
        }
    }

    /// Returns the current inner publisher
    pub fn load(&self) -> Arc<MqttPublisher> {
        self.inner.load_full()
    }

    /// Atomically swaps the inner publisher with a new one
    pub fn store(&self, publisher: MqttPublisher) {
        self.inner.store(Arc::new(publisher));
        info!("SharedMqttPublisher: inner publisher updated after failover");
    }

    /// Returns the agent ID
    pub fn agent_id(&self) -> String {
        self.inner.load().agent_id().to_string()
    }

    /// Returns whether the client is currently connected
    #[allow(dead_code)] // Public API
    pub fn is_connected(&self) -> bool {
        self.inner.load().is_connected()
    }

    /// Returns queue statistics: (message_count, size_in_bytes)
    #[allow(dead_code)] // Public API
    pub fn queue_stats(&self) -> (u32, u64) {
        self.inner.load().queue_stats()
    }

    /// Publishes agent info message
    #[allow(dead_code)] // Public API
    pub async fn publish_agent_info(&self, info: &proto::AgentInfo) -> Result<(), MqttError> {
        self.inner.load().publish_agent_info(info).await
    }

    /// Publishes a metrics snapshot
    pub async fn publish_metrics(
        &self,
        snapshot: &proto::MetricsSnapshot,
    ) -> Result<(), MqttError> {
        self.inner.load().publish_metrics(snapshot).await
    }

    /// Publishes a process list
    pub async fn publish_process_list(
        &self,
        process_list: &proto::ProcessList,
    ) -> Result<(), MqttError> {
        self.inner.load().publish_process_list(process_list).await
    }

    /// Publishes service metrics from plugins
    pub async fn publish_service_metrics(
        &self,
        service_metrics: &proto::ServiceMetricsList,
    ) -> Result<(), MqttError> {
        self.inner
            .load()
            .publish_service_metrics(service_metrics)
            .await
    }

    /// Publishes a command response
    pub async fn publish_command_response(
        &self,
        command_id: &str,
        success: bool,
        message: &str,
    ) -> Result<(), MqttError> {
        self.inner
            .load()
            .publish_command_response(command_id, success, message)
            .await
    }

    /// Publishes a command response with optional config data
    pub async fn publish_command_response_with_config(
        &self,
        command_id: &str,
        success: bool,
        message: &str,
        config: Option<proto::AgentConfig>,
    ) -> Result<(), MqttError> {
        self.inner
            .load()
            .publish_command_response_with_config(command_id, success, message, config)
            .await
    }

    /// Publishes a command response with optional swap processes data
    pub async fn publish_command_response_with_swap_processes(
        &self,
        command_id: &str,
        success: bool,
        message: &str,
        swap_processes: Option<proto::SwapProcessList>,
    ) -> Result<(), MqttError> {
        self.inner
            .load()
            .publish_command_response_with_swap_processes(
                command_id,
                success,
                message,
                swap_processes,
            )
            .await
    }

    /// Publishes a command response with config schema
    pub async fn publish_command_response_with_schema(
        &self,
        command_id: &str,
        success: bool,
        message: &str,
        config_schema: Option<proto::ConfigSchema>,
    ) -> Result<(), MqttError> {
        self.inner
            .load()
            .publish_command_response_with_schema(command_id, success, message, config_schema)
            .await
    }

    /// Publishes a command response with sync report
    pub async fn publish_command_response_with_sync_report(
        &self,
        command_id: &str,
        success: bool,
        message: &str,
        sync_report: Option<String>,
    ) -> Result<(), MqttError> {
        self.inner
            .load()
            .publish_command_response_with_sync_report(command_id, success, message, sync_report)
            .await
    }

    /// Sends a batch of queued messages
    pub async fn send_queued_batch(&self, batch_size: usize) -> Result<(usize, usize), MqttError> {
        self.inner.load().send_queued_batch(batch_size).await
    }
}

/// Result of the MQTT event loop
pub enum EventLoopResult {
    /// Loop exited normally (shouldn't happen)
    #[allow(dead_code)]
    Finished,
    /// Need to failover to next broker after too many connection failures
    FailoverNeeded,
}

/// Maximum consecutive connection failures before trying next broker
const MAX_FAILURES_BEFORE_FAILOVER: u64 = 5;

/// Spawns the MQTT event loop in a dedicated OS thread with its own tokio runtime.
/// This ensures the MQTT event loop is not affected by the main application's tokio
/// runtime being overloaded, which could cause keepalive timeouts on heavily loaded machines.
/// Supports broker failover when multiple brokers are configured.
///
/// The `shared_publisher` is atomically updated when failover occurs, so all other
/// parts of the application automatically use the new publisher.
pub fn spawn_mqtt_event_loop_in_dedicated_thread(
    shared_publisher: Arc<SharedMqttPublisher>,
    eventloop: EventLoop,
    agent_info: proto::AgentInfo,
    retry_interval_secs: u64,
    command_sender: Option<CommandSender>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("mqtt-event-loop".to_string())
        .spawn(move || {
            info!("MQTT event loop thread started (thread: {:?})", std::thread::current().id());

            // Create a dedicated single-threaded tokio runtime for MQTT
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    error!("Failed to create MQTT event loop runtime: {}", e);
                    return;
                }
            };

            info!("MQTT dedicated tokio runtime created successfully");

            // Run the event loop with failover support
            rt.block_on(async {
                // Get the initial publisher from the shared wrapper
                let mut current_publisher = shared_publisher.load();
                let mut current_eventloop = Some(eventloop);

                loop {
                    // Get the eventloop for this iteration
                    let eventloop = match current_eventloop.take() {
                        Some(el) => el,
                        None => {
                            // Need to create a new publisher/eventloop (after failover error)
                            error!("No eventloop available, attempting to recreate");
                            match current_publisher.create_with_next_broker().await {
                                Ok((new_pub, new_loop, _)) => {
                                    // Update both local and shared publisher
                                    shared_publisher.store(new_pub);
                                    current_publisher = shared_publisher.load();
                                    new_loop
                                }
                                Err(e) => {
                                    error!("Failed to recreate MQTT publisher: {}", e);
                                    time::sleep(Duration::from_secs(5)).await;
                                    continue;
                                }
                            }
                        }
                    };

                    let result = run_mqtt_event_loop(
                        Arc::clone(&current_publisher),
                        eventloop,
                        agent_info.clone(),
                        retry_interval_secs,
                        command_sender.clone(),
                    ).await;

                    match result {
                        EventLoopResult::Finished => {
                            warn!("MQTT event loop finished unexpectedly");
                            break;
                        }
                        EventLoopResult::FailoverNeeded => {
                            if current_publisher.broker_count() <= 1 {
                                warn!("Failover requested but only one broker configured, recreating connection");
                            } else {
                                info!("Failover: switching to next broker in the list");
                            }

                            match current_publisher.create_with_next_broker().await {
                                Ok((new_pub, new_loop, new_idx)) => {
                                    info!(
                                        "Failover: now using broker {}/{}: {}",
                                        new_idx + 1,
                                        new_pub.broker_count(),
                                        new_pub.current_broker_addr()
                                    );
                                    // CRITICAL: Update the shared publisher so all other tasks use the new client
                                    shared_publisher.store(new_pub);
                                    current_publisher = shared_publisher.load();
                                    current_eventloop = Some(new_loop);
                                }
                                Err(e) => {
                                    error!("Failed to create new MQTT publisher for failover: {}", e);
                                    time::sleep(Duration::from_secs(5)).await;
                                    // current_eventloop is None, will be recreated on next iteration
                                }
                            }
                        }
                    }
                }
            });

            warn!("MQTT event loop exited!");
        })
        .expect("Failed to spawn MQTT event loop thread")
}

/// Runs the MQTT event loop, handling reconnection and queue draining
/// Returns EventLoopResult::FailoverNeeded when too many consecutive connection failures occur
pub async fn run_mqtt_event_loop(
    publisher: Arc<MqttPublisher>,
    mut eventloop: EventLoop,
    agent_info: proto::AgentInfo,
    retry_interval_secs: u64,
    command_sender: Option<CommandSender>,
) -> EventLoopResult {
    let mut reconnect_attempts: u64 = 0;
    let mut poll_count: u64 = 0;
    let mut queue_remaining: usize = 0;
    let mut last_drain_poll: u64 = 0;

    // Drain one message every N polls if there are remaining messages
    // Keep this very low since we now send only 1 message at a time with timeout
    const DRAIN_INTERVAL_POLLS: u64 = 3;

    info!(
        "MQTT event loop starting for broker: {}",
        publisher.current_broker_addr()
    );

    loop {
        poll_count += 1;

        // Log heartbeat every 1000 polls (roughly every few seconds)
        if poll_count % 1000 == 0 {
            debug!("MQTT event loop heartbeat: {} polls processed", poll_count);
        }

        // Periodic queue drain if there are remaining messages
        // Use select! with timeout to ensure poll() is called frequently for keepalive
        if queue_remaining > 0
            && publisher.is_connected()
            && poll_count - last_drain_poll >= DRAIN_INTERVAL_POLLS
        {
            last_drain_poll = poll_count;

            // Drain with a short timeout to not block the event loop
            let drain_timeout = Duration::from_millis(100);
            match time::timeout(drain_timeout, publisher.drain_queue_single()).await {
                Ok(Ok((sent, remaining))) => {
                    queue_remaining = remaining;
                    if sent > 0 {
                        debug!("Periodic drain: sent {}, remaining {}", sent, remaining);
                    }
                }
                Ok(Err(e)) => {
                    warn!("Periodic drain failed: {}", e);
                }
                Err(_) => {
                    debug!("Drain timeout, will continue next cycle");
                }
            }
        }

        match eventloop.poll().await {
            Ok(Event::Incoming(Packet::ConnAck(_))) => {
                info!(
                    "Connected to MQTT broker: {}",
                    publisher.current_broker_addr()
                );
                reconnect_attempts = 0;

                publisher.set_connected(true);

                // Subscribe to commands
                if command_sender.is_some() {
                    if let Err(e) = publisher.subscribe_commands().await {
                        warn!("Failed to subscribe to commands: {}", e);
                    }
                }

                // Publish online status
                if let Err(e) = publisher.publish_online_status().await {
                    warn!("Failed to publish online status: {}", e);
                }

                // Publish agent info with current queue stats
                let (queue_count, queue_bytes) = publisher.queue_stats();
                let mut current_agent_info = agent_info.clone();
                current_agent_info.queue_pending_count = queue_count;
                current_agent_info.queue_pending_bytes = queue_bytes;

                if queue_count > 0 {
                    info!(
                        "Agent has {} pending messages ({:.2} MB) in offline queue",
                        queue_count,
                        queue_bytes as f64 / (1024.0 * 1024.0)
                    );
                }

                if let Err(e) = publisher.publish_agent_info(&current_agent_info).await {
                    warn!("Failed to publish agent info: {}", e);
                }

                // Set queue_remaining so periodic drain kicks in.
                // We DON'T drain immediately on connect because it can block the event loop
                // and cause keepalive timeouts (the broker expects PINGREQ).
                // The periodic drain (every DRAIN_INTERVAL_POLLS) handles it safely.
                queue_remaining = queue_count as usize;
                if queue_remaining > 0 {
                    info!(
                        "Queue drain deferred to periodic cycle ({} messages to send)",
                        queue_remaining
                    );
                }
            }
            Ok(Event::Incoming(Packet::Publish(msg))) => {
                // Handle incoming command
                if msg.topic.contains("/command") {
                    if let Some(ref sender) = command_sender {
                        match proto::Command::decode(msg.payload.as_ref()) {
                            Ok(cmd) => {
                                let cmd_type = cmd.command_type();
                                info!("Received command: {:?} (id: {})", cmd_type, cmd.command_id);
                                let received = ReceivedCommand {
                                    command_id: cmd.command_id,
                                    command_type: cmd_type,
                                    set_group_value: cmd.set_group_value,
                                    update_url: cmd.update_url,
                                    update_checksum: cmd.update_checksum,
                                    config_key: cmd.config_key,
                                    config_value: cmd.config_value,
                                };
                                if let Err(e) = sender.send(received) {
                                    error!("Failed to send command to handler: {}", e);
                                }
                            }
                            Err(e) => {
                                warn!("Failed to decode command: {}", e);
                            }
                        }
                    }
                }
            }
            Ok(Event::Incoming(Packet::PubAck(_))) => {
                debug!("Message acknowledged by broker");
            }
            Ok(Event::Incoming(Packet::PingResp)) => {
                debug!("MQTT keepalive ping response received");
            }
            Ok(Event::Outgoing(rumqttc::Outgoing::PingReq)) => {
                debug!("MQTT keepalive ping request sent");
            }
            Ok(event) => {
                // Other events - log at trace level for debugging
                debug!("MQTT event: {:?}", event);
            }
            Err(e) => {
                publisher.set_connected(false);

                reconnect_attempts += 1;
                warn!(
                    "MQTT connection error (attempt {}/{}): {} (broker: {})",
                    reconnect_attempts,
                    MAX_FAILURES_BEFORE_FAILOVER,
                    e,
                    publisher.current_broker_addr()
                );

                // Check if we should failover to next broker
                if reconnect_attempts >= MAX_FAILURES_BEFORE_FAILOVER {
                    warn!(
                        "Max reconnect attempts ({}) reached for broker {}, requesting failover",
                        MAX_FAILURES_BEFORE_FAILOVER,
                        publisher.current_broker_addr()
                    );
                    return EventLoopResult::FailoverNeeded;
                }

                // Exponential backoff with jitter to prevent thundering herd
                let base_delay = std::cmp::min(retry_interval_secs * reconnect_attempts, 60);
                let jitter = rand::thread_rng().gen_range(0..=base_delay / 2);
                let delay = base_delay + jitter;

                debug!(
                    "Reconnecting in {} seconds (base: {}, jitter: {})",
                    delay, base_delay, jitter
                );
                time::sleep(Duration::from_secs(delay)).await;
            }
        }
    }
}

/// Creates a status message (for LWT and online status)
fn create_status_message(agent_id: &str, online: bool) -> Vec<u8> {
    let status = proto::AgentStatus {
        agent_id: agent_id.to_string(),
        online,
        timestamp: Some(prost_types::Timestamp {
            seconds: Utc::now().timestamp(),
            nanos: 0,
        }),
    };
    status.encode_to_vec()
}

/// Parses the broker host from a connection string like "tcp://host:port"
fn parse_broker_host(addr: &str) -> String {
    addr.trim_start_matches("tcp://")
        .trim_start_matches("ssl://")
        .split(':')
        .next()
        .unwrap_or("localhost")
        .to_string()
}

/// Parses the broker port from a connection string like "tcp://host:port"
fn parse_broker_port(addr: &str) -> u16 {
    addr.trim_start_matches("tcp://")
        .trim_start_matches("ssl://")
        .split(':')
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(1883)
}

/// MQTT-related errors
#[derive(Debug)]
#[allow(dead_code)]
pub enum MqttError {
    PublishError(String),
    QueueError(String),
    ConnectionError(String),
}

impl std::fmt::Display for MqttError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MqttError::PublishError(msg) => write!(f, "MQTT publish error: {}", msg),
            MqttError::QueueError(msg) => write!(f, "Queue error: {}", msg),
            MqttError::ConnectionError(msg) => write!(f, "Connection error: {}", msg),
        }
    }
}

impl std::error::Error for MqttError {}

// Clone implementation for QueuedMessage (needed for mark_sent)
impl Clone for crate::message_queue::QueuedMessage {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            topic: self.topic.clone(),
            payload: self.payload.clone(),
            file_path: self.file_path.clone(),
        }
    }
}
