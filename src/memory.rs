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

/// Information about a process using swap memory
#[derive(Debug, Clone)]
pub struct SwapProcessInfo {
    pub pid: u32,
    pub name: String,
    pub swap_kib: u64,
    pub cmdline: String,
    pub user: String,
}

/// Reads all processes using swap and returns them sorted by swap usage (descending)
pub fn read_swap_processes() -> io::Result<Vec<SwapProcessInfo>> {
    let mut swap_processes = Vec::new();

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
            Err(_) => continue, // Process may have exited
        };

        let mut name = String::new();
        let mut swap_usage: u64 = 0;
        let mut uid: u32 = 0;

        for line in contents.lines() {
            if let Some(value) = line.strip_prefix("Name:") {
                name = value.split_whitespace().next().unwrap_or("").to_string();
            } else if let Some(value) = line.strip_prefix("VmSwap:") {
                swap_usage = value
                    .split_whitespace()
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
            } else if let Some(value) = line.strip_prefix("Uid:") {
                // First value is real UID
                uid = value
                    .split_whitespace()
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
            }
        }

        if swap_usage > 0 {
            // Try to read cmdline
            let cmdline_path = format!("/proc/{}/cmdline", pid);
            let cmdline = fs::read_to_string(&cmdline_path)
                .map(|c| c.replace('\0', " ").trim().to_string())
                .unwrap_or_default();

            // Try to resolve username from UID
            let user = get_username_from_uid(uid);

            swap_processes.push(SwapProcessInfo {
                pid,
                name,
                swap_kib: swap_usage,
                cmdline,
                user,
            });
        }
    }

    // Sort by swap usage descending
    swap_processes.sort_by(|a, b| b.swap_kib.cmp(&a.swap_kib));

    Ok(swap_processes)
}

/// Get username from UID by reading /etc/passwd
fn get_username_from_uid(uid: u32) -> String {
    // Try to read /etc/passwd and find username
    if let Ok(contents) = fs::read_to_string("/etc/passwd") {
        for line in contents.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 3 {
                if let Ok(line_uid) = parts[2].parse::<u32>() {
                    if line_uid == uid {
                        return parts[0].to_string();
                    }
                }
            }
        }
    }
    // Fall back to UID string
    uid.to_string()
}

#[allow(dead_code)]
pub fn read_process_swap_usage_with_names() -> io::Result<()> {
    let _ = read_swap_processes()?;
    Ok(())
}
