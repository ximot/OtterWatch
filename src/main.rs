mod agent_id;
mod app_config;
mod bootstrap;
mod config_manager;
mod console_ui;
mod cpu;
mod disk_io;
mod memory;
mod message_queue;
mod mqtt_client;
mod network;
mod osinfo;
mod plugins;
mod pressure;
mod process;
mod proto;
mod self_update;
mod storage;

use crate::mqtt_client::ReceivedCommand;
use crate::osinfo::get_os_info_api;
use crate::pressure::PressureSnapshot;
use crate::storage::{TimeseriesSnapshot, TimeseriesStorage};
use actix_cors::Cors;
use actix_web::{web, App, HttpResponse, HttpServer};
use chrono::Utc;
use log::{error, info, warn};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use tokio::time;

#[derive(Debug, Deserialize)]
struct Settings {
    interval_secs: u64,
    listen_addr: String,
    db_file_name: String,
    db_save: bool,
    db_history_days: u64,
    exclude_interfaces: String,
    // Security settings
    #[serde(default = "default_cors_origins")]
    cors_allowed_origins: String,
    #[serde(default)]
    #[allow(dead_code)] // Reserved for future HTTP API authentication
    http_api_key: String,
    #[serde(default)]
    #[allow(dead_code)] // Reserved for future HTTP API authentication
    http_require_auth: bool,
    // MQTT settings
    #[serde(default)]
    mqtt_enabled: bool,
    #[serde(default = "default_mqtt_broker")]
    mqtt_broker_addr: String,
    /// List of MQTT broker addresses for failover (takes priority over mqtt_broker_addr)
    #[serde(default)]
    mqtt_broker_addrs: Vec<String>,
    /// Bootstrap URL for auto-discovery of broker addresses from server
    #[serde(default)]
    mqtt_bootstrap_url: String,
    /// Timeout for bootstrap request in seconds
    #[serde(default = "default_mqtt_bootstrap_timeout")]
    mqtt_bootstrap_timeout_secs: u64,
    #[serde(default)]
    mqtt_client_id: Option<String>,
    #[serde(default)]
    mqtt_api_key: String,
    #[serde(default = "default_mqtt_topic_prefix")]
    mqtt_topic_prefix: String,
    #[serde(default = "default_mqtt_keepalive")]
    mqtt_keepalive_secs: u64,
    #[serde(default = "default_mqtt_retry_interval")]
    mqtt_retry_interval_secs: u64,
    #[serde(default = "default_mqtt_queue_path")]
    mqtt_queue_path: String,
    #[serde(default = "default_mqtt_queue_max_size")]
    mqtt_queue_max_size_mb: u64,
    // Process list settings
    #[serde(default = "default_process_interval")]
    process_list_interval_secs: u64,
    #[serde(default = "default_process_top_n")]
    process_top_n: usize,
    // Agent grouping
    #[serde(default)]
    agent_group: String,
    // Plugin settings
    #[serde(default)]
    plugins: plugins::PluginSettings,
}

fn default_cors_origins() -> String {
    "*".to_string()
}

fn default_mqtt_broker() -> String {
    "tcp://localhost:1883".to_string()
}

fn default_mqtt_topic_prefix() -> String {
    "otterwatch/metrics".to_string()
}

fn default_mqtt_keepalive() -> u64 {
    30
}

fn default_mqtt_retry_interval() -> u64 {
    5
}

fn default_mqtt_queue_path() -> String {
    "mqtt_queue".to_string()
}

fn default_mqtt_queue_max_size() -> u64 {
    100
}

fn default_process_interval() -> u64 {
    10 // Collect process list every 10 seconds
}

fn default_process_top_n() -> usize {
    25 // Top 25 processes by CPU
}

fn default_mqtt_bootstrap_timeout() -> u64 {
    10 // 10 seconds timeout for bootstrap request
}

#[derive(Serialize, Clone)]
pub(crate) struct SystemStats {
    cpu_usage: f64,
    used_memory: u64,
    available_memory: u64,
    total_memory: u64,
    swap_usage: u64,
    swap_total: u64,
}

impl SystemStats {
    fn new() -> Self {
        Self {
            cpu_usage: 0f64,
            total_memory: 0,
            used_memory: 0,
            available_memory: 0,
            swap_usage: 0,
            swap_total: 0,
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct OperationTimings {
    pub(crate) cpu_read: Option<Duration>,
    pub(crate) memory_read: Option<Duration>,
    pub(crate) disk_read: Option<Duration>,
    pub(crate) network_read: Option<Duration>,
    pub(crate) db_write: Option<Duration>,
    pub(crate) cycle: Option<Duration>,
}

#[derive(Clone, Default)]
pub(crate) struct ProgramUsageStats {
    pub(crate) cpu_percent: f64,
    pub(crate) memory_bytes: u64,
}

/// Handles incoming commands from the server
async fn handle_commands(
    mut command_rx: mpsc::UnboundedReceiver<ReceivedCommand>,
    mqtt_publisher: Arc<mqtt_client::SharedMqttPublisher>,
    config_path: std::path::PathBuf,
) {
    while let Some(cmd) = command_rx.recv().await {
        info!(
            "Processing command: {:?} (id: {})",
            cmd.command_type, cmd.command_id
        );

        let (success, message) = match cmd.command_type {
            proto::CommandType::Ping => (true, "pong".to_string()),
            proto::CommandType::ReloadConfig => {
                // Reload configuration file
                match app_config::load_config() {
                    Ok(_new_config) => {
                        // Note: Full reload would require restarting tasks
                        // For now, we just validate the config is readable
                        (
                            true,
                            "Configuration validated successfully. Full reload requires restart."
                                .to_string(),
                        )
                    }
                    Err(e) => (false, format!("Failed to load configuration: {}", e)),
                }
            }
            proto::CommandType::Restart => {
                // Request process restart
                // In production, this would be handled by systemd or a wrapper script
                info!("Restart requested - scheduling shutdown");
                let _ = mqtt_publisher
                    .publish_command_response(&cmd.command_id, true, "Restart initiated")
                    .await;

                // Give time for response to be sent
                tokio::time::sleep(Duration::from_millis(500)).await;

                // Exit with special code that wrapper/systemd can detect
                std::process::exit(42);
            }
            proto::CommandType::SetGroup => {
                // Update group in settings file
                let group = if cmd.set_group_value.is_empty() {
                    None
                } else {
                    Some(cmd.set_group_value.clone())
                };

                match update_config_group(&config_path, group.as_deref()) {
                    Ok(_) => {
                        let msg = match &group {
                            Some(g) => format!(
                                "Group set to '{}'. Restart agent for change to take effect.",
                                g
                            ),
                            None => "Group cleared. Restart agent for change to take effect."
                                .to_string(),
                        };
                        (true, msg)
                    }
                    Err(e) => (false, format!("Failed to update group: {}", e)),
                }
            }
            proto::CommandType::Update => {
                if cmd.update_url.is_empty() {
                    (false, "Update URL not provided".to_string())
                } else {
                    info!("Starting update from: {}", cmd.update_url);

                    // Download and verify the update
                    let result =
                        self_update::download_update(&cmd.update_url, &cmd.update_checksum).await;

                    if result.success {
                        // Send success response before exiting
                        let _ = mqtt_publisher
                            .publish_command_response(
                                &cmd.command_id,
                                true,
                                &format!(
                                    "Update downloaded. Restarting to apply. {}",
                                    result.message
                                ),
                            )
                            .await;

                        // Give time for response to be sent
                        tokio::time::sleep(Duration::from_millis(500)).await;

                        // Exit with update code - wrapper script will replace binary and restart
                        info!(
                            "Update downloaded successfully, exiting with code {}",
                            self_update::EXIT_CODE_UPDATE
                        );
                        std::process::exit(self_update::EXIT_CODE_UPDATE);
                    } else {
                        (false, result.message)
                    }
                }
            }
            proto::CommandType::GetConfig => {
                // Load current configuration and send it back
                match app_config::load_config() {
                    Ok(config) => {
                        let agent_config = proto::AgentConfig {
                            interval_secs: config.interval_secs,
                            process_list_interval_secs: config.process_list_interval_secs,
                            process_top_n: config.process_top_n as u32,
                            listen_addr: config.listen_addr.clone(),
                            mqtt_broker_addr: config.mqtt_broker_addr.clone(),
                            mqtt_topic_prefix: config.mqtt_topic_prefix.clone(),
                            mqtt_enabled: config.mqtt_enabled,
                            mqtt_bootstrap_url: config.mqtt_bootstrap_url.clone(),
                            mqtt_bootstrap_timeout_secs: config.mqtt_bootstrap_timeout_secs,
                            db_file_name: config.db_file_name.clone(),
                            db_save: config.db_save,
                            db_history_days: config.db_history_days,
                            agent_group: config.agent_group.clone(),
                            exclude_interfaces: config.exclude_interfaces.clone(),
                            config_path: config_path
                                .join("settings.toml")
                                .to_string_lossy()
                                .to_string(),
                            data_directory: config.db_file_name.clone(),
                            // Plugin settings
                            plugin_interval_secs: config.plugins.plugin_interval_secs,
                            plugins_collect_process_details: config.plugins.collect_process_details,
                            plugins_nginx_enabled: config.plugins.nginx.enabled,
                            plugins_nginx_service_name: config.plugins.nginx.service_name.clone(),
                            plugins_tomcat_enabled: config.plugins.tomcat.enabled,
                            plugins_tomcat_service_name: config.plugins.tomcat.service_name.clone(),
                            plugins_self_monitor_enabled: config.plugins.self_monitor.enabled,
                            plugins_self_monitor_collect_open_fds: config
                                .plugins
                                .self_monitor
                                .collect_open_fds,
                            plugins_self_monitor_collect_io_stats: config
                                .plugins
                                .self_monitor
                                .collect_io_stats,
                        };

                        // Send response with config
                        if let Err(e) = mqtt_publisher
                            .publish_command_response_with_config(
                                &cmd.command_id,
                                true,
                                "Configuration retrieved",
                                Some(agent_config),
                            )
                            .await
                        {
                            error!("Failed to send config response: {}", e);
                        }
                        continue; // Skip the normal response sending below
                    }
                    Err(e) => (false, format!("Failed to load configuration: {}", e)),
                }
            }
            proto::CommandType::GetSwapProcesses => {
                // Use spawn_blocking for the I/O-heavy /proc reading
                // This runs in tokio's blocking thread pool, avoiding interference with async tasks
                let swap_result = tokio::task::spawn_blocking(memory::read_swap_processes).await;

                // Limit to top 50 processes to avoid exceeding MQTT packet size limits
                const MAX_SWAP_PROCESSES: usize = 50;

                let (success, message, swap_list) = match swap_result {
                    Ok(Ok(processes)) => {
                        let total_swap: u64 = processes.iter().map(|p| p.swap_kib).sum();
                        let total_count = processes.len();

                        // Take only top N processes (already sorted by swap usage descending)
                        let limited_processes: Vec<_> =
                            processes.into_iter().take(MAX_SWAP_PROCESSES).collect();
                        let returned_count = limited_processes.len();

                        let list = proto::SwapProcessList {
                            timestamp: Some(prost_types::Timestamp {
                                seconds: Utc::now().timestamp(),
                                nanos: 0,
                            }),
                            processes: limited_processes
                                .into_iter()
                                .map(|p| proto::SwapProcessInfo {
                                    pid: p.pid,
                                    name: p.name,
                                    swap_kib: p.swap_kib,
                                    cmdline: p.cmdline.chars().take(200).collect(), // Limit cmdline length
                                    user: p.user,
                                })
                                .collect(),
                            total_swap_kib: total_swap,
                        };

                        let msg = if total_count > returned_count {
                            format!(
                                "Found {} processes using {} KiB swap (showing top {})",
                                total_count, total_swap, returned_count
                            )
                        } else {
                            format!(
                                "Found {} processes using {} KiB swap",
                                total_count, total_swap
                            )
                        };

                        (true, msg, Some(list))
                    }
                    Ok(Err(e)) => (false, format!("Failed to read swap processes: {}", e), None),
                    Err(e) => (false, format!("Task failed: {}", e), None),
                };

                // Send response using the same MQTT client (no separate runtime)
                if let Err(e) = mqtt_publisher
                    .publish_command_response_with_swap_processes(
                        &cmd.command_id,
                        success,
                        &message,
                        swap_list,
                    )
                    .await
                {
                    error!("Failed to send swap processes response: {}", e);
                }
                continue;
            }
            proto::CommandType::SetConfig => {
                // Set a single config parameter
                if cmd.config_key.is_empty() {
                    (false, "Missing config_key parameter".to_string())
                } else {
                    match config_manager::set_config_value(
                        &config_path,
                        &cmd.config_key,
                        &cmd.config_value,
                    ) {
                        Ok(_) => (
                            true,
                            format!("Set {} = {}", cmd.config_key, cmd.config_value),
                        ),
                        Err(e) => (false, e),
                    }
                }
            }
            proto::CommandType::SyncConfig => {
                // Synchronize config with schema (add missing parameters)
                match config_manager::sync_config(&config_path) {
                    Ok(added) => {
                        if added.is_empty() {
                            (true, "Configuration is up to date".to_string())
                        } else {
                            let report =
                                format!("Added {} parameters:\n{}", added.len(), added.join("\n"));
                            if let Err(e) = mqtt_publisher
                                .publish_command_response_with_sync_report(
                                    &cmd.command_id,
                                    true,
                                    &report,
                                    Some(report.clone()),
                                )
                                .await
                            {
                                error!("Failed to send sync config response: {}", e);
                            }
                            continue;
                        }
                    }
                    Err(e) => (false, format!("Sync failed: {}", e)),
                }
            }
            proto::CommandType::GetConfigSchema => {
                // Get full config schema with metadata
                let schema = config_manager::get_config_schema();
                if let Err(e) = mqtt_publisher
                    .publish_command_response_with_schema(
                        &cmd.command_id,
                        true,
                        "Configuration schema",
                        Some(schema),
                    )
                    .await
                {
                    error!("Failed to send config schema response: {}", e);
                }
                continue;
            }
            proto::CommandType::Reconnect => {
                // Reconnect to MQTT broker (re-run bootstrap for new broker assignment)
                info!("Reconnect requested - will restart with new bootstrap");
                let _ = mqtt_publisher
                    .publish_command_response(&cmd.command_id, true, "Reconnect initiated")
                    .await;

                // Give time for response to be sent
                tokio::time::sleep(Duration::from_millis(500)).await;

                // Exit with code 44 - wrapper will restart and agent will re-bootstrap
                std::process::exit(44);
            }
            proto::CommandType::Unspecified => (false, "Unknown command type".to_string()),
        };

        // Send response
        if let Err(e) = mqtt_publisher
            .publish_command_response(&cmd.command_id, success, &message)
            .await
        {
            error!("Failed to send command response: {}", e);
        }
    }
}

/// Updates the agent_group in the config file
fn update_config_group(config_path: &std::path::Path, group: Option<&str>) -> Result<(), String> {
    let settings_path = config_path.join("settings.toml");

    // Read current config
    let content = std::fs::read_to_string(&settings_path)
        .map_err(|e| format!("Failed to read settings.toml: {}", e))?;

    // Parse and update
    let mut doc = content
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("Failed to parse settings.toml: {}", e))?;

    match group {
        Some(g) => {
            doc["agent_group"] = toml_edit::value(g);
        }
        None => {
            doc["agent_group"] = toml_edit::value("");
        }
    }

    // Write back
    std::fs::write(&settings_path, doc.to_string())
        .map_err(|e| format!("Failed to write settings.toml: {}", e))?;

    Ok(())
}

/// Collects and publishes process list to MQTT
async fn collect_and_publish_processes(
    interval_secs: u64,
    top_n: usize,
    mqtt_publisher: Arc<mqtt_client::SharedMqttPublisher>,
) {
    let mut interval = time::interval(Duration::from_secs(interval_secs));
    let mut process_tracker = process::ProcessTracker::new();

    // First tick initializes the tracker (CPU percentages will be 0)
    interval.tick().await;
    let _ = process_tracker.collect_top_processes(top_n);

    loop {
        interval.tick().await;

        match process_tracker.collect_top_processes(top_n) {
            Ok(processes) => {
                // Convert to proto ProcessList
                let process_list = proto::ProcessList {
                    timestamp: Some(prost_types::Timestamp {
                        seconds: Utc::now().timestamp(),
                        nanos: 0,
                    }),
                    agent_id: mqtt_publisher.agent_id().to_string(),
                    processes: processes
                        .into_iter()
                        .map(|p| proto::ProcessInfo {
                            pid: p.pid,
                            name: p.name,
                            state: p.state,
                            ppid: p.ppid,
                            cpu_percent: p.cpu_percent,
                            memory_rss_kib: p.memory_rss_kib,
                            memory_vsz_kib: p.memory_vsz_kib,
                            threads: p.threads,
                            user: p.user,
                            cmdline: p.cmdline,
                            start_time: p.start_time,
                        })
                        .collect(),
                };

                if let Err(e) = mqtt_publisher.publish_process_list(&process_list).await {
                    warn!("Failed to publish process list to MQTT: {}", e);
                }
            }
            Err(e) => {
                warn!("Failed to collect process list: {}", e);
            }
        }
    }
}

/// Collects and publishes plugin metrics to MQTT
async fn collect_and_publish_plugin_metrics(
    plugin_settings: plugins::PluginSettings,
    mqtt_publisher: Arc<mqtt_client::SharedMqttPublisher>,
) {
    use plugins::{PluginState, ServicePlugin};
    use std::collections::HashMap;

    let mut interval = time::interval(Duration::from_secs(plugin_settings.plugin_interval_secs));
    let collect_processes = plugin_settings.collect_process_details;
    let configs = plugin_settings.to_configs();

    // Create plugin instances
    let registry: Vec<Box<dyn ServicePlugin>> = vec![
        Box::new(plugins::nginx::NginxPlugin::new()),
        Box::new(plugins::tomcat::TomcatPlugin::new()),
        Box::new(plugins::self_monitor::SelfMonitorPlugin::new()),
    ];

    // State for each plugin (for CPU delta calculations)
    let mut states: HashMap<String, PluginState> = HashMap::new();

    // First tick - wait before collecting
    interval.tick().await;

    loop {
        interval.tick().await;

        let mut services = Vec::new();

        for plugin in &registry {
            let name = plugin.name();
            if let Some(config) = configs.get(name) {
                if !config.enabled {
                    continue;
                }

                let state = states
                    .entry(name.to_string())
                    .or_insert_with(PluginState::new);

                match plugin.collect(config, state, collect_processes) {
                    Ok(metrics) => {
                        // Convert to proto ServiceMetrics
                        let proto_metrics = proto::ServiceMetrics {
                            service_name: metrics.service_name,
                            plugin_type: metrics.plugin_type,
                            is_running: metrics.is_running,
                            cpu_usage_usec: metrics.cpu_usage_usec,
                            cpu_percent: metrics.cpu_percent,
                            cpu_user_usec: metrics.cpu_user_usec,
                            cpu_system_usec: metrics.cpu_system_usec,
                            memory_current_bytes: metrics.memory_current_bytes,
                            memory_swap_bytes: metrics.memory_swap_bytes,
                            memory_anon_bytes: metrics.memory_anon_bytes,
                            memory_file_bytes: metrics.memory_file_bytes,
                            disk_read_bytes: metrics.disk_read_bytes,
                            disk_write_bytes: metrics.disk_write_bytes,
                            disk_read_ops: metrics.disk_read_ops,
                            disk_write_ops: metrics.disk_write_ops,
                            net_rx_bytes: metrics.net_rx_bytes,
                            net_tx_bytes: metrics.net_tx_bytes,
                            process_count: metrics.process_count,
                            thread_count: metrics.thread_count,
                            cgroup_version: metrics.cgroup_version.map(|v| v as u32),
                            data_source: match metrics.data_source {
                                plugins::DataSource::CgroupV2 => 1, // 1=cgroupv2
                                plugins::DataSource::CgroupV1 => 2, // 2=cgroupv1
                                plugins::DataSource::ProcFs => 3,   // 3=procfs
                                plugins::DataSource::Unknown => 0,  // 0=unknown
                            },
                            processes: metrics
                                .processes
                                .into_iter()
                                .map(|p| proto::ServiceProcessInfo {
                                    pid: p.pid,
                                    name: p.name,
                                    cpu_percent: p.cpu_percent,
                                    memory_bytes: p.memory_bytes,
                                    threads: p.threads,
                                })
                                .collect(),
                        };
                        services.push(proto_metrics);
                    }
                    Err(e) => {
                        warn!("Failed to collect {} metrics: {}", name, e);
                    }
                }
            }
        }

        if !services.is_empty() {
            let list = proto::ServiceMetricsList {
                timestamp: Some(prost_types::Timestamp {
                    seconds: Utc::now().timestamp(),
                    nanos: 0,
                }),
                agent_id: mqtt_publisher.agent_id().to_string(),
                services,
            };

            if let Err(e) = mqtt_publisher.publish_service_metrics(&list).await {
                warn!("Failed to publish service metrics: {}", e);
            }
        }
    }
}

async fn clean_history_data(days_old: u64, storage: Arc<TimeseriesStorage>) {
    const TIMER_TRIGGER: u64 = 86400;
    let mut interval = time::interval(Duration::from_secs(TIMER_TRIGGER));

    loop {
        interval.tick().await;
        let storage = Arc::clone(&storage);
        let cleanup_result =
            tokio::task::spawn_blocking(move || storage.prune_older_than_days(days_old)).await;

        match cleanup_result {
            Ok(Ok(())) => println!("Historic data has been truncated! (over {} days)", days_old),
            Ok(Err(err)) => error!("Failed to prune history files: {}", err),
            Err(join_err) => error!("Failed to execute history cleanup task: {}", join_err),
        }
    }
}

async fn collect_and_save_stats(
    interval_secs: u64,
    db_save: bool,
    excluded_interfaces: Arc<Vec<String>>,
    storage: Option<Arc<TimeseriesStorage>>,
    mqtt_publisher: Option<Arc<mqtt_client::SharedMqttPublisher>>,
) {
    let mut interval = time::interval(Duration::from_secs(interval_secs));
    let ticks_per_second = process::clock_ticks_per_second();
    let page_size = process::page_size_bytes();
    let mut prev_process_ticks: Option<u64> = None;

    // Queue drain settings: every N metric sends, send a batch of queued messages
    const QUEUE_DRAIN_INTERVAL: u64 = 10; // Every 10 metric sends
    const QUEUE_DRAIN_BATCH_SIZE: usize = 10; // Send 10 queued messages per batch
    let mut send_counter: u64 = 0;

    loop {
        interval.tick().await;

        let loop_start = Instant::now();

        let cpu_measure_start = Instant::now();
        let cpu_usage = cpu::read_cpu_stats().await;
        let cpu_duration = cpu_measure_start.elapsed();

        let memory_measure_start = Instant::now();
        let (mem_total, _mem_free, mem_avail, swap_total, swap_free) = memory::read_memory_info();
        let memory_duration = memory_measure_start.elapsed();

        let mut disk_duration = None;
        let mut network_duration = None;
        let mut db_write_duration = None;

        let mem_used = mem_total.saturating_sub(mem_avail);
        {
            let mut data = GLOBAL_DATA.write().await;
            data.cpu_usage = cpu_usage.0;
            data.used_memory = mem_used;
            data.total_memory = mem_total;
            data.swap_usage = swap_total.saturating_sub(swap_free);
            data.swap_total = swap_total;
            data.available_memory = mem_avail;
        }

        // Collect disk and network info if db_save is enabled OR mqtt is enabled
        let collect_detailed = db_save || mqtt_publisher.is_some();

        let (disk_info, network_info) = if collect_detailed {
            let disk_start = Instant::now();
            let disk_info = disk_io::get_physical_disk_io_stats();
            disk_duration = Some(disk_start.elapsed());

            let network_start = Instant::now();
            let network_info = network::get_network_io_stats(excluded_interfaces.as_ref());
            network_duration = Some(network_start.elapsed());

            (disk_info, network_info)
        } else {
            (Vec::new(), Vec::new())
        };

        // Create snapshot for storage and/or MQTT
        let snapshot = if collect_detailed {
            Some(TimeseriesSnapshot::new(
                Utc::now(),
                cpu_usage,
                mem_used,
                mem_avail,
                mem_total,
                swap_free,
                swap_total,
                disk_info.clone(),
                network_info.clone(),
            ))
        } else {
            None
        };

        // Save to local storage if enabled
        if db_save {
            if let (Some(storage), Some(_)) = (storage.as_ref(), &snapshot) {
                let storage = Arc::clone(storage);
                let snapshot_clone = TimeseriesSnapshot::new(
                    Utc::now(),
                    cpu_usage,
                    mem_used,
                    mem_avail,
                    mem_total,
                    swap_free,
                    swap_total,
                    disk_info,
                    network_info,
                );

                let db_start = Instant::now();
                let write_result =
                    tokio::task::spawn_blocking(move || storage.append_snapshot(snapshot_clone))
                        .await;
                db_write_duration = Some(db_start.elapsed());

                match write_result {
                    Ok(Ok(())) => {}
                    Ok(Err(err)) => error!("Failed to persist system stats: {}", err),
                    Err(join_err) => error!("Storage writer task panicked: {}", join_err),
                }
            }
        }

        // Publish to MQTT if enabled
        if let (Some(publisher), Some(snapshot)) = (&mqtt_publisher, snapshot) {
            let proto_snapshot = snapshot.to_proto(&publisher.agent_id());
            if let Err(e) = publisher.publish_metrics(&proto_snapshot).await {
                warn!("Failed to publish metrics to MQTT: {}", e);
            }

            // Periodically drain queued messages
            send_counter += 1;
            if send_counter % QUEUE_DRAIN_INTERVAL == 0 {
                if let Err(e) = publisher.send_queued_batch(QUEUE_DRAIN_BATCH_SIZE).await {
                    warn!("Failed to send queued batch: {}", e);
                }
            }
        }

        let cycle_duration = loop_start.elapsed();

        {
            let mut timings = GLOBAL_TIMINGS.write().await;
            timings.cpu_read = Some(cpu_duration);
            timings.memory_read = Some(memory_duration);
            timings.disk_read = disk_duration;
            timings.network_read = network_duration;
            timings.db_write = db_write_duration;
            timings.cycle = Some(cycle_duration);
        }

        if let Ok(snapshot) = process::read_self_snapshot() {
            let cpu_percent = if let Some(prev_ticks) = prev_process_ticks {
                if cycle_duration.as_secs_f64() > 0.0 && ticks_per_second > 0 {
                    let delta_ticks = snapshot.total_ticks.saturating_sub(prev_ticks);
                    (delta_ticks as f64 / ticks_per_second as f64) / cycle_duration.as_secs_f64()
                        * 100.0
                } else {
                    0.0
                }
            } else {
                0.0
            };
            prev_process_ticks = Some(snapshot.total_ticks);
            let rss_bytes = snapshot.rss_pages.saturating_mul(page_size);

            let mut program_stats = GLOBAL_PROGRAM_STATS.write().await;
            program_stats.cpu_percent = cpu_percent;
            program_stats.memory_bytes = rss_bytes;
        }

        let pressure_snapshot = pressure::read_pressure_snapshot();
        {
            let mut pressure_data = GLOBAL_PRESSURE.write().await;
            *pressure_data = pressure_snapshot;
        }

        if let Err(err) = memory::read_process_swap_usage_with_names() {
            warn!("Failed to read process swap usage: {}", err);
        }
    }
}

async fn system_stats() -> HttpResponse {
    let stats = GLOBAL_DATA.read().await.clone();
    let payload = serde_json::json!({
        "cpu_usage_percent": stats.cpu_usage,
        "memory_used_kib": stats.used_memory,
        "memory_total_kib": stats.total_memory,
        "memory_available_kib": stats.available_memory,
        "swap_used_kib": stats.swap_usage,
        "swap_total_kib": stats.swap_total,
    });

    HttpResponse::Ok()
        .content_type("application/json")
        .body(payload.to_string())
}

async fn system_info() -> HttpResponse {
    match get_os_info_api() {
        Ok(body) => HttpResponse::Ok()
            .content_type("application/json")
            .body(body),
        Err(err) => {
            error!("Failed to serialize OS info: {}", err);
            HttpResponse::InternalServerError().finish()
        }
    }
}

static GLOBAL_DATA: Lazy<Arc<RwLock<SystemStats>>> =
    Lazy::new(|| Arc::new(RwLock::new(SystemStats::new())));

static GLOBAL_TIMINGS: Lazy<Arc<RwLock<OperationTimings>>> =
    Lazy::new(|| Arc::new(RwLock::new(OperationTimings::default())));

static GLOBAL_PROGRAM_STATS: Lazy<Arc<RwLock<ProgramUsageStats>>> =
    Lazy::new(|| Arc::new(RwLock::new(ProgramUsageStats::default())));

static GLOBAL_PRESSURE: Lazy<Arc<RwLock<PressureSnapshot>>> =
    Lazy::new(|| Arc::new(RwLock::new(PressureSnapshot::default())));

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Initialize logger - use RUST_LOG env var to control level (default: info)
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let use_gui = std::env::args().any(|arg| arg == "--gui");

    let mut config = app_config::load_config().map_err(|err| {
        error!("Failed to load configuration from file: {}", err);
        io::Error::new(io::ErrorKind::Other, err.to_string())
    })?;

    let data_root = std::path::PathBuf::from(&config.db_file_name);

    // Config directory is current working directory where settings.toml is located
    let config_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

    // Auto-sync configuration at startup (add missing parameters after agent update)
    match config_manager::sync_config(&config_dir) {
        Ok(added) if !added.is_empty() => {
            info!(
                "Configuration synchronized, added {} new parameters:",
                added.len()
            );
            for param in &added {
                info!("  + {}", param);
            }
            // Reload configuration with new default values
            config = app_config::load_config().map_err(|err| {
                error!("Failed to reload configuration: {}", err);
                io::Error::new(io::ErrorKind::Other, err.to_string())
            })?;
        }
        Ok(_) => {
            info!("Configuration is up to date");
        }
        Err(e) => {
            warn!("Configuration sync failed: {}", e);
        }
    }

    let storage = if config.db_save {
        match TimeseriesStorage::new(&data_root) {
            Ok(storage) => Some(Arc::new(storage)),
            Err(err) => {
                error!(
                    "Failed to initialize time-series storage at {}: {}",
                    data_root.display(),
                    err
                );
                return Err(io::Error::new(io::ErrorKind::Other, err.to_string()));
            }
        }
    } else {
        if let Err(err) = std::fs::create_dir_all(&data_root) {
            warn!(
                "Unable to prepare data directory {}: {}",
                data_root.display(),
                err
            );
        }
        None
    };

    if let Err(err) = osinfo::show_and_save_os_info(&data_root) {
        warn!("Failed to persist OS information: {}", err);
    }

    let exclude_interfaces = Arc::new(
        config
            .exclude_interfaces
            .split(',')
            .map(|s| s.trim().to_string())
            .collect::<Vec<String>>(),
    );

    // Initialize MQTT if enabled
    let mqtt_publisher = if config.mqtt_enabled {
        if config.mqtt_api_key.is_empty() {
            warn!("MQTT enabled but no API key configured. Set mqtt_api_key or APP_MQTT_API_KEY");
        }

        // Get or create agent ID
        let agent_id = match agent_id::get_or_create_agent_id(&data_root) {
            Ok(id) => id,
            Err(err) => {
                error!("Failed to get/create agent ID: {}", err);
                return Err(io::Error::new(io::ErrorKind::Other, err.to_string()));
            }
        };

        // Initialize message queue for offline storage
        let queue_path = data_root.join(&config.mqtt_queue_path);
        let queue =
            match message_queue::MessageQueue::new(&queue_path, config.mqtt_queue_max_size_mb) {
                Ok(q) => Arc::new(q),
                Err(err) => {
                    error!("Failed to initialize MQTT message queue: {}", err);
                    return Err(io::Error::new(io::ErrorKind::Other, err.to_string()));
                }
            };

        // Build broker list from config or bootstrap
        // Priority: bootstrap (if configured) -> mqtt_broker_addrs -> mqtt_broker_addr
        let broker_addrs = if !config.mqtt_bootstrap_url.is_empty() {
            // Use bootstrap to fetch broker configuration from server
            info!(
                "Bootstrap enabled, fetching broker configuration from {}",
                config.mqtt_bootstrap_url
            );
            bootstrap::fetch_or_fallback(
                &config.mqtt_bootstrap_url,
                &config.mqtt_api_key,
                &config.agent_group,
                config.mqtt_bootstrap_timeout_secs,
                &config.mqtt_broker_addr,
                &config.mqtt_broker_addrs,
            )
            .await
        } else if config.mqtt_broker_addrs.is_empty() {
            vec![config.mqtt_broker_addr.clone()]
        } else {
            config.mqtt_broker_addrs.clone()
        };

        if broker_addrs.len() > 1 {
            info!(
                "MQTT failover enabled with {} brokers: {:?}",
                broker_addrs.len(),
                broker_addrs
            );
        }

        // Create MQTT config
        let mqtt_config = mqtt_client::MqttConfig {
            broker_addrs,
            client_id: config
                .mqtt_client_id
                .clone()
                .unwrap_or_else(|| format!("otterwatch-{}", &agent_id[..8])),
            api_key: config.mqtt_api_key.clone(),
            topic_prefix: config.mqtt_topic_prefix.clone(),
            keepalive_secs: config.mqtt_keepalive_secs,
            retry_interval_secs: config.mqtt_retry_interval_secs,
        };

        // Get OS info for agent registration
        let os_info = osinfo::get_os_info_struct();

        // Check if running as root
        let is_root = unsafe { libc::getuid() == 0 };
        if is_root {
            info!("Agent running as root (full system access)");
        } else {
            info!("Agent running as regular user (limited process visibility)");
        }

        let agent_info = proto::AgentInfo {
            agent_id: agent_id.clone(),
            hostname: os_info.hostname,
            os_name: os_info.os_name,
            kernel_version: os_info.kernel_version,
            agent_version: env!("CARGO_PKG_VERSION").to_string(),
            cpu_cores: os_info.cpu_cores as u32,
            cpu_name: os_info.cpu_name,
            agent_group: config.agent_group.clone(),
            is_root,
            queue_pending_count: 0, // Will be updated by MQTT event loop
            queue_pending_bytes: 0,
        };

        // Create command channel for receiving server commands
        let (command_tx, command_rx) = mpsc::unbounded_channel::<ReceivedCommand>();

        // Create MQTT publisher
        match mqtt_client::MqttPublisher::new(mqtt_config.clone(), agent_id, queue).await {
            Ok((publisher, eventloop)) => {
                // Wrap publisher in SharedMqttPublisher for atomic updates during failover
                let shared_publisher = Arc::new(mqtt_client::SharedMqttPublisher::new(publisher));

                // Spawn MQTT event loop in a dedicated OS thread with its own tokio runtime
                // This ensures MQTT keepalives are always processed, even on heavily loaded machines
                let publisher_for_loop = Arc::clone(&shared_publisher);
                let _mqtt_thread = mqtt_client::spawn_mqtt_event_loop_in_dedicated_thread(
                    publisher_for_loop,
                    eventloop,
                    agent_info,
                    mqtt_config.retry_interval_secs,
                    Some(command_tx),
                );

                // Spawn command handler
                let publisher_for_commands = Arc::clone(&shared_publisher);
                let config_path_for_commands = config_dir.clone();
                tokio::spawn(handle_commands(
                    command_rx,
                    publisher_for_commands,
                    config_path_for_commands,
                ));

                if mqtt_config.broker_addrs.len() > 1 {
                    println!(
                        "MQTT enabled with {} brokers (failover): {}",
                        mqtt_config.broker_addrs.len(),
                        mqtt_config.broker_addrs.join(", ")
                    );
                } else {
                    println!(
                        "MQTT enabled, connecting to {}",
                        mqtt_config.broker_addrs[0]
                    );
                }
                Some(shared_publisher)
            }
            Err(err) => {
                error!("Failed to create MQTT publisher: {}", err);
                return Err(io::Error::new(io::ErrorKind::Other, err.to_string()));
            }
        }
    } else {
        None
    };

    // Spawn process collection task if MQTT is enabled
    if let Some(ref publisher) = mqtt_publisher {
        let publisher_for_processes = Arc::clone(publisher);
        tokio::spawn(collect_and_publish_processes(
            config.process_list_interval_secs,
            config.process_top_n,
            publisher_for_processes,
        ));
    }

    // Spawn plugin collection task if MQTT is enabled and plugins are configured
    if let Some(ref publisher) = mqtt_publisher {
        if config.plugins.has_enabled_plugins() {
            info!(
                "Starting plugin collection (interval: {}s, plugins: nginx={}, tomcat={}, self={})",
                config.plugins.plugin_interval_secs,
                config.plugins.nginx.enabled,
                config.plugins.tomcat.enabled,
                config.plugins.self_monitor.enabled,
            );
            let publisher_for_plugins = Arc::clone(publisher);
            tokio::spawn(collect_and_publish_plugin_metrics(
                config.plugins.clone(),
                publisher_for_plugins,
            ));
        }
    }

    let storage_for_collection = storage.clone();
    tokio::spawn(collect_and_save_stats(
        config.interval_secs,
        config.db_save,
        Arc::clone(&exclude_interfaces),
        storage_for_collection,
        mqtt_publisher,
    ));

    if use_gui {
        println!("Starting console view (--gui enabled)");
        console_ui::spawn_console_view(
            Arc::clone(&GLOBAL_DATA),
            Arc::clone(&GLOBAL_TIMINGS),
            Arc::clone(&GLOBAL_PROGRAM_STATS),
            Arc::clone(&GLOBAL_PRESSURE),
        );
    }

    if let Some(storage_for_cleanup) = storage {
        tokio::spawn(clean_history_data(
            config.db_history_days,
            storage_for_cleanup,
        ));
    }

    let listen_addr = config.listen_addr.clone();
    let cors_origins = config.cors_allowed_origins.clone();

    HttpServer::new(move || {
        let cors = build_cors_config(&cors_origins);
        App::new()
            .wrap(cors)
            .route("/system-stats", web::get().to(system_stats))
            .route("/system-info", web::get().to(system_info))
    })
    .bind(&listen_addr)?
    .run()
    .await
}

fn build_cors_config(allowed_origins: &str) -> Cors {
    if allowed_origins == "*" {
        // Permissive mode - allow all origins (not recommended for production)
        Cors::permissive()
    } else {
        // Parse comma-separated origins and configure restrictive CORS
        let origins: Vec<&str> = allowed_origins
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        let mut cors = Cors::default()
            .allowed_methods(vec!["GET", "OPTIONS"])
            .allowed_headers(vec![
                actix_web::http::header::CONTENT_TYPE,
                actix_web::http::header::ACCEPT,
            ])
            .max_age(3600);

        for origin in origins {
            cors = cors.allowed_origin(origin);
        }

        cors
    }
}
