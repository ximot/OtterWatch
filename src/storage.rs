use crate::disk_io::DiskInfo;
use crate::network::NetworkInterface;
use chrono::{DateTime, Duration as ChronoDuration, NaiveDate, Utc};
use log::warn;
use serde::Serialize;
use std::fs::{self, OpenOptions, Permissions};
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const TIMESERIES_FILE_PREFIX: &str = "stats-";
const TIMESERIES_FILE_EXTENSION: &str = "jsonl";

#[derive(Serialize)]
pub struct TimeseriesSnapshot {
    timestamp: DateTime<Utc>,
    cpu_usage: f64,
    cpu_io_wait: f64,
    used_memory: u64,
    avail_memory: u64,
    total_memory: u64,
    swap_free_memory: u64,
    swap_total_memory: u64,
    disks: Vec<DiskInfo>,
    network: Vec<NetworkInterface>,
}

impl TimeseriesSnapshot {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        timestamp: DateTime<Utc>,
        cpu_usage: (f64, f64),
        mem_used: u64,
        mem_avail: u64,
        mem_total: u64,
        swap_free: u64,
        swap_total: u64,
        disks: Vec<DiskInfo>,
        network: Vec<NetworkInterface>,
    ) -> Self {
        Self {
            timestamp,
            cpu_usage: cpu_usage.0,
            cpu_io_wait: cpu_usage.1,
            used_memory: mem_used,
            avail_memory: mem_avail,
            total_memory: mem_total,
            swap_free_memory: swap_free,
            swap_total_memory: swap_total,
            disks,
            network,
        }
    }

    /// Converts this snapshot to a Protocol Buffer MetricsSnapshot
    pub fn to_proto(&self, agent_id: &str) -> crate::proto::MetricsSnapshot {
        crate::proto::MetricsSnapshot {
            timestamp: Some(prost_types::Timestamp {
                seconds: self.timestamp.timestamp(),
                nanos: self.timestamp.timestamp_subsec_nanos() as i32,
            }),
            agent_id: agent_id.to_string(),
            cpu_usage_percent: self.cpu_usage,
            cpu_io_wait_percent: self.cpu_io_wait,
            memory_used_kib: self.used_memory,
            memory_available_kib: self.avail_memory,
            memory_total_kib: self.total_memory,
            swap_free_kib: self.swap_free_memory,
            swap_total_kib: self.swap_total_memory,
            disks: self
                .disks
                .iter()
                .map(|d| crate::proto::DiskMetrics {
                    device: d.device.clone(),
                    read_ops: d.read_ops,
                    write_ops: d.write_ops,
                    read_time_ms: d.read_time_ms,
                    write_time_ms: d.write_time_ms,
                })
                .collect(),
            network: self
                .network
                .iter()
                .map(|n| crate::proto::NetworkMetrics {
                    interface_name: n.interface_name.clone(),
                    bytes_received: n.bytes_received,
                    bytes_transmitted: n.bytes_transmitted,
                })
                .collect(),
            pressure: None, // TODO: Add pressure metrics conversion
            // Queue stats are set by publish_metrics in mqtt_client.rs
            queue_pending_count: 0,
            queue_pending_bytes: 0,
        }
    }
}

#[derive(Clone)]
pub struct TimeseriesStorage {
    root: PathBuf,
}

impl TimeseriesStorage {
    pub fn new<P: AsRef<Path>>(root: P) -> io::Result<Self> {
        let root_ref = root.as_ref();
        if root_ref.is_file() {
            let backup = root_ref.with_extension("sqlite.bak");
            warn!(
                "Existing file {} detected; renaming to {}",
                root_ref.display(),
                backup.display()
            );
            fs::rename(root_ref, &backup)?;
        }

        let root = root_ref.to_path_buf();
        fs::create_dir_all(&root)?;

        // Set restrictive permissions on data directory (owner only)
        #[cfg(unix)]
        {
            fs::set_permissions(&root, Permissions::from_mode(0o700))?;
        }

        Ok(Self { root })
    }

    pub fn append_snapshot(&self, snapshot: TimeseriesSnapshot) -> io::Result<()> {
        let file_path = self.daily_file_path(snapshot.timestamp);
        let file_exists = file_path.exists();

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)?;

        // Set restrictive permissions on new files (owner only read/write)
        #[cfg(unix)]
        if !file_exists {
            fs::set_permissions(&file_path, Permissions::from_mode(0o600))?;
        }

        serde_json::to_writer(&mut file, &snapshot)?;
        file.write_all(b"\n")?;
        file.flush()?;
        Ok(())
    }

    pub fn prune_older_than_days(&self, days_to_keep: u64) -> io::Result<()> {
        let cutoff_date = Utc::now()
            .checked_sub_signed(ChronoDuration::days(days_to_keep as i64))
            .map(|ts| ts.date_naive())
            .unwrap_or_else(|| NaiveDate::from_ymd_opt(1970, 1, 1).unwrap());

        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            if !matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some(TIMESERIES_FILE_EXTENSION)
            ) {
                continue;
            }

            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };

            if let Some(date_part) = stem.strip_prefix(TIMESERIES_FILE_PREFIX) {
                if let Ok(file_date) = NaiveDate::parse_from_str(date_part, "%Y-%m-%d") {
                    if file_date < cutoff_date {
                        let _ = fs::remove_file(&path);
                    }
                }
            }
        }

        Ok(())
    }

    fn daily_file_path(&self, timestamp: DateTime<Utc>) -> PathBuf {
        let file_name = format!(
            "{}{}.{}",
            TIMESERIES_FILE_PREFIX,
            timestamp.format("%Y-%m-%d"),
            TIMESERIES_FILE_EXTENSION
        );
        self.root.join(file_name)
    }
}
