use std::fs::{self};
use std::io;

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
