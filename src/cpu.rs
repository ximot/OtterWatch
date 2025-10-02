use std::fs;
use std::time::Duration;

pub async fn read_cpu_stats() -> (f64, f64) {
    let (total1, idle1, iowait1) = match read_cpu_usage() {
        Some(times) => times,
        None => return (0.0, 0.0),
    };

    tokio::time::sleep(Duration::from_secs(1)).await;

    let (total2, idle2, iowait2) = match read_cpu_usage() {
        Some(times) => times,
        None => return (0.0, 0.0),
    };

    let total_diff = total2.saturating_sub(total1);
    if total_diff == 0 {
        return (0.0, 0.0);
    }

    let idle_diff = idle2.saturating_sub(idle1);
    let iowait_diff = iowait2.saturating_sub(iowait1);

    let cpu_usage = 100.0 * (total_diff.saturating_sub(idle_diff)) as f64 / total_diff as f64;
    let io_wait = 100.0 * iowait_diff as f64 / total_diff as f64;

    (cpu_usage.clamp(0.0, 100.0), io_wait.clamp(0.0, 100.0))
}

fn read_cpu_usage() -> Option<(u64, u64, u64)> {
    let content = fs::read_to_string("/proc/stat").ok()?;
    let line = content.lines().next()?;

    let mut total: u64 = 0;
    let mut idle: u64 = 0;
    let mut iowait: u64 = 0;

    for (idx, token) in line.split_whitespace().skip(1).enumerate() {
        let value: u64 = token.parse().ok()?;
        total = total.saturating_add(value);
        match idx {
            3 => idle = value,
            4 => iowait = value,
            _ => {}
        }
    }

    if total == 0 {
        return None;
    }

    Some((total, idle, iowait))
}
