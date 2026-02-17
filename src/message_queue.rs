use chrono::Utc;
use log::{debug, error, warn};
use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{self, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// A disk-backed message queue for storing metrics when MQTT connection is unavailable.
/// Messages are stored as individual files with timestamp-based naming for ordering.
#[derive(Clone)]
pub struct MessageQueue {
    queue_dir: PathBuf,
    max_size_bytes: u64,
}

/// A message read from the queue
pub struct QueuedMessage {
    pub id: String,
    pub topic: String,
    pub payload: Vec<u8>,
    pub file_path: PathBuf,
}

impl MessageQueue {
    /// Creates a new message queue at the specified directory.
    pub fn new<P: AsRef<Path>>(queue_dir: P, max_size_mb: u64) -> io::Result<Self> {
        let queue_dir = queue_dir.as_ref().to_path_buf();
        fs::create_dir_all(&queue_dir)?;

        // Set restrictive permissions on queue directory
        #[cfg(unix)]
        {
            fs::set_permissions(&queue_dir, Permissions::from_mode(0o700))?;
        }

        Ok(Self {
            queue_dir,
            max_size_bytes: max_size_mb * 1024 * 1024,
        })
    }

    /// Enqueues a message to the disk queue.
    /// Returns Ok(true) if message was queued, Ok(false) if queue is full.
    pub fn enqueue(&self, topic: &str, payload: &[u8]) -> io::Result<bool> {
        // Check queue size before adding
        if self.current_size_bytes()? >= self.max_size_bytes {
            warn!(
                "Message queue is full (max {} MB), dropping oldest messages",
                self.max_size_bytes / (1024 * 1024)
            );
            self.remove_oldest_messages(1)?;
        }

        let timestamp_ms = Utc::now().timestamp_millis();
        let message_id = Uuid::new_v4().to_string();
        let filename = format!("{}_{}.msg", timestamp_ms, message_id);
        let temp_filename = format!(".{}.tmp", filename);

        let file_path = self.queue_dir.join(&filename);
        let temp_path = self.queue_dir.join(&temp_filename);

        // Write to temp file first (atomic write pattern)
        let mut temp_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&temp_path)?;

        // Write topic length (4 bytes) + topic + payload
        let topic_bytes = topic.as_bytes();
        let topic_len = topic_bytes.len() as u32;
        temp_file.write_all(&topic_len.to_le_bytes())?;
        temp_file.write_all(topic_bytes)?;
        temp_file.write_all(payload)?;
        temp_file.flush()?;

        // Set restrictive permissions
        #[cfg(unix)]
        {
            fs::set_permissions(&temp_path, Permissions::from_mode(0o600))?;
        }

        // Atomically rename to final path
        fs::rename(&temp_path, &file_path)?;

        debug!("Queued message {} ({} bytes)", message_id, payload.len());
        Ok(true)
    }

    /// Dequeues up to `count` messages from the queue (oldest first).
    /// Messages are not deleted until `mark_sent` is called.
    pub fn dequeue_batch(&self, count: usize) -> io::Result<Vec<QueuedMessage>> {
        let mut messages = Vec::new();
        let mut entries: Vec<_> = fs::read_dir(&self.queue_dir)?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .map(|ext| ext == "msg")
                    .unwrap_or(false)
            })
            .collect();

        // Sort by filename (which contains timestamp) for FIFO ordering
        entries.sort_by_key(|entry| entry.file_name());

        for entry in entries.into_iter().take(count) {
            let path = entry.path();
            match self.read_message(&path) {
                Ok(msg) => messages.push(msg),
                Err(err) => {
                    error!("Failed to read queued message {:?}: {}", path, err);
                    // Remove corrupted message
                    let _ = fs::remove_file(&path);
                }
            }
        }

        Ok(messages)
    }

    /// Marks messages as successfully sent and removes them from the queue.
    pub fn mark_sent(&self, messages: &[QueuedMessage]) -> io::Result<()> {
        for msg in messages {
            if let Err(err) = fs::remove_file(&msg.file_path) {
                warn!("Failed to remove sent message {:?}: {}", msg.file_path, err);
            } else {
                debug!("Removed sent message: {}", msg.id);
            }
        }
        Ok(())
    }

    /// Returns the number of messages in the queue.
    pub fn len(&self) -> io::Result<usize> {
        let count = fs::read_dir(&self.queue_dir)?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .map(|ext| ext == "msg")
                    .unwrap_or(false)
            })
            .count();
        Ok(count)
    }

    /// Returns true if the queue is empty.
    #[allow(dead_code)] // Public API for external use
    pub fn is_empty(&self) -> io::Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Returns queue statistics: (message_count, size_in_bytes)
    pub fn stats(&self) -> io::Result<(usize, u64)> {
        let mut count = 0usize;
        let mut total_size = 0u64;
        for entry in fs::read_dir(&self.queue_dir)? {
            let entry = entry?;
            if entry
                .path()
                .extension()
                .map(|ext| ext == "msg")
                .unwrap_or(false)
            {
                count += 1;
                total_size += entry.metadata()?.len();
            }
        }
        Ok((count, total_size))
    }

    /// Returns the current size of the queue in bytes.
    fn current_size_bytes(&self) -> io::Result<u64> {
        let mut total_size = 0u64;
        for entry in fs::read_dir(&self.queue_dir)? {
            let entry = entry?;
            if entry
                .path()
                .extension()
                .map(|ext| ext == "msg")
                .unwrap_or(false)
            {
                total_size += entry.metadata()?.len();
            }
        }
        Ok(total_size)
    }

    /// Removes the oldest messages to free up space.
    fn remove_oldest_messages(&self, count: usize) -> io::Result<()> {
        let mut entries: Vec<_> = fs::read_dir(&self.queue_dir)?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .map(|ext| ext == "msg")
                    .unwrap_or(false)
            })
            .collect();

        entries.sort_by_key(|entry| entry.file_name());

        for entry in entries.into_iter().take(count) {
            let path = entry.path();
            if let Err(err) = fs::remove_file(&path) {
                warn!("Failed to remove old message {:?}: {}", path, err);
            } else {
                debug!("Removed old message to free space: {:?}", path);
            }
        }

        Ok(())
    }

    /// Reads a message from a file.
    fn read_message(&self, path: &Path) -> io::Result<QueuedMessage> {
        let mut file = File::open(path)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;

        if buffer.len() < 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Message file too short",
            ));
        }

        // Read topic length
        let topic_len = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;

        if buffer.len() < 4 + topic_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid topic length in message",
            ));
        }

        let topic = String::from_utf8(buffer[4..4 + topic_len].to_vec())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid UTF-8 in topic"))?;

        let payload = buffer[4 + topic_len..].to_vec();

        // Extract message ID from filename
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.split('_').nth(1))
            .unwrap_or("unknown")
            .to_string();

        Ok(QueuedMessage {
            id,
            topic,
            payload,
            file_path: path.to_path_buf(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_enqueue_dequeue() {
        let temp_dir = tempdir().unwrap();
        let queue = MessageQueue::new(temp_dir.path(), 10).unwrap();

        // Enqueue a message
        let topic = "test/topic";
        let payload = b"test payload data";
        queue.enqueue(topic, payload).unwrap();

        // Verify queue has one message
        assert_eq!(queue.len().unwrap(), 1);

        // Dequeue message
        let messages = queue.dequeue_batch(10).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].topic, topic);
        assert_eq!(messages[0].payload, payload);

        // Mark as sent
        queue.mark_sent(&messages).unwrap();

        // Queue should be empty
        assert!(queue.is_empty().unwrap());
    }

    #[test]
    fn test_fifo_ordering() {
        let temp_dir = tempdir().unwrap();
        let queue = MessageQueue::new(temp_dir.path(), 10).unwrap();

        // Enqueue multiple messages
        for i in 0..5 {
            queue
                .enqueue("test/topic", format!("message {}", i).as_bytes())
                .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        // Dequeue should return in FIFO order
        let messages = queue.dequeue_batch(10).unwrap();
        assert_eq!(messages.len(), 5);
        for (i, msg) in messages.iter().enumerate() {
            assert_eq!(
                String::from_utf8(msg.payload.clone()).unwrap(),
                format!("message {}", i)
            );
        }
    }
}
