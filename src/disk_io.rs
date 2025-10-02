use log::warn;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;

static PHYSICAL_DISK_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(sd[a-z]+|hd[a-z]+|nvme\d+n\d+)$").expect("valid physical disk filter pattern")
});

#[derive(Serialize, Deserialize)]
pub struct DiskInfo {
    pub devices: String,
    pub read_ops: String,
    pub write_ops: String,
    pub read_time_ms: String,
    pub write_time_ms: String,
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

        let device = parts[2];
        if PHYSICAL_DISK_PATTERN.is_match(device) {
            disks_info_list.push(DiskInfo {
                devices: device.to_string(),
                read_ops: parts.get(5).unwrap_or(&"0").to_string(),
                write_ops: parts.get(9).unwrap_or(&"0").to_string(),
                read_time_ms: parts.get(12).unwrap_or(&"0").to_string(),
                write_time_ms: parts.get(14).unwrap_or(&"0").to_string(),
            });
        }
    }

    disks_info_list
}
