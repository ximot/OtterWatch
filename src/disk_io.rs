use log::warn;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;

static PHYSICAL_DISK_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(sd[a-z]+|hd[a-z]+|nvme\d+n\d+)$").expect("valid physical disk filter pattern")
});

#[derive(Clone, Serialize, Deserialize)]
pub struct DiskInfo {
    pub device: String,
    pub read_ops: u64,
    pub write_ops: u64,
    pub read_time_ms: u64,
    pub write_time_ms: u64,
}

pub fn get_physical_disk_io_stats() -> Vec<DiskInfo> {
    let mut disks_info_list = Vec::new();

    let diskstats = match fs::read_to_string("/proc/diskstats") {
        Ok(content) => content,
        Err(err) => {
            warn!("Unable to read /proc/diskstats: {}", err);
            return disks_info_list;
        }
    };

    for line in diskstats.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() <= 13 {
            continue;
        }

        let Some(device) = parts.get(2) else {
            continue;
        };
        if PHYSICAL_DISK_PATTERN.is_match(device) {
            let read_ops = parts
                .get(3)
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            let write_ops = parts
                .get(7)
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            let read_time_ms = parts
                .get(6)
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            let write_time_ms = parts
                .get(10)
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);

            disks_info_list.push(DiskInfo {
                device: device.to_string(),
                read_ops,
                write_ops,
                read_time_ms,
                write_time_ms,
            });
        }
    }

    disks_info_list
}
