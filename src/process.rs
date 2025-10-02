use libc;
use std::fs::{self};
use std::io;

#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessSnapshot {
    pub total_ticks: u64,
    pub rss_pages: u64,
}

pub fn read_self_snapshot() -> io::Result<ProcessSnapshot> {
    let stat_content = fs::read_to_string("/proc/self/stat")?;
    let stats: Vec<&str> = stat_content.split_whitespace().collect();
    if stats.len() <= 24 {
        return Ok(ProcessSnapshot::default());
    }

    let utime: u64 = stats.get(13).and_then(|v| v.parse().ok()).unwrap_or(0);
    let stime: u64 = stats.get(14).and_then(|v| v.parse().ok()).unwrap_or(0);

    let statm_content = fs::read_to_string("/proc/self/statm")?;
    let rss_pages = statm_content
        .split_whitespace()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    Ok(ProcessSnapshot {
        total_ticks: utime.saturating_add(stime),
        rss_pages,
    })
}

pub fn clock_ticks_per_second() -> u64 {
    unsafe {
        let ticks = libc::sysconf(libc::_SC_CLK_TCK);
        if ticks > 0 {
            ticks as u64
        } else {
            100
        }
    }
}

pub fn page_size_bytes() -> u64 {
    unsafe {
        let page = libc::sysconf(libc::_SC_PAGESIZE);
        if page > 0 {
            page as u64
        } else {
            4096
        }
    }
}

pub fn find_top_processes() -> io::Result<()> {
    let mut cpu_usage_vec = Vec::new();

    for entry in fs::read_dir("/proc")? {
        let entry = entry?;
        if let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() {
            if let Ok(stat) = fs::read_to_string(format!("/proc/{}/stat", pid)) {
                let stats: Vec<&str> = stat.split_whitespace().collect();
                if stats.len() > 13 {
                    let utime: u64 = stats[13].parse().unwrap_or(0);
                    let stime: u64 = stats[14].parse().unwrap_or(0);
                    let pname: String = stats[1].to_owned();
                    let total_time = utime + stime;
                    cpu_usage_vec.push((pid, total_time, pname));
                }
            }
        }
    }

    // Sorting of processes by total CPU time and selection of the first 10
    cpu_usage_vec.sort_by(|a, b| b.1.cmp(&a.1));
    let top_processes = cpu_usage_vec.into_iter().take(10);

    for (pid, cpu_usage, pname) in top_processes {
        println!(
            "PID: {}, ProcName: {}, CPU Usage: {}",
            pid, pname, cpu_usage
        );
    }

    Ok(())
}
