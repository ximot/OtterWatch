use log::warn;
use std::{fs, io};

pub fn read_memory_info() -> (u64, u64, u64, u64, u64) {
    let content = match fs::read_to_string("/proc/meminfo") {
        Ok(content) => content,
        Err(err) => {
            warn!("Unable to read /proc/meminfo: {}", err);
            return (0, 0, 0, 0, 0);
        }
    };
    let mut mem_total = 0;
    let mut mem_free = 0;
    let mut mem_aval = 0;
    let mut swap_total = 0;
    let mut swap_free = 0;

    for line in content.lines() {
        let mut parts = line.split_whitespace();
        let key = parts.next().unwrap_or("");
        let value = parts.next().unwrap_or("0");
        let parsed = value.parse::<u64>().unwrap_or(0);

        match key {
            "MemTotal:" => mem_total = parsed,
            "MemFree:" => mem_free = parsed,
            "MemAvailable:" => mem_aval = parsed,
            "SwapTotal:" => swap_total = parsed,
            "SwapFree:" => swap_free = parsed,
            _ => {}
        }
    }

    (mem_total, mem_free, mem_aval, swap_total, swap_free)
}

pub fn read_process_swap_usage_with_names() -> io::Result<()> {
    let mut swap_usage_per_process = Vec::new();
    for entry in fs::read_dir("/proc")? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }

        let pid = match entry.file_name().to_string_lossy().parse::<u32>() {
            Ok(pid) => pid,
            Err(_) => continue,
        };

        let status_path = format!("/proc/{}/status", pid);
        let contents = match fs::read_to_string(&status_path) {
            Ok(contents) => contents,
            Err(err) => {
                warn!("Unable to read {}: {}", status_path, err);
                continue;
            }
        };

        let mut name = String::new();
        let mut swap_usage: u64 = 0;

        for line in contents.lines() {
            if let Some(value) = line.strip_prefix("Name:") {
                name = value.split_whitespace().next().unwrap_or("").to_string();
            } else if let Some(value) = line.strip_prefix("VmSwap:") {
                swap_usage = value
                    .split_whitespace()
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
            }
        }

        if swap_usage > 0 {
            swap_usage_per_process.push((pid, name, swap_usage));
        }
    }
    swap_usage_per_process.sort_by(|a, b| b.2.cmp(&a.2));
    Ok(())
}
