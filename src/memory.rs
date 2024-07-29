use std::{fs, io};

pub fn read_memory_info() -> (u64, u64, u64, u64, u64) {
    // DEBUG TIME
    use std::time::Instant;
    let now = Instant::now();

    let content = fs::read_to_string("/proc/meminfo").unwrap();
    let mut mem_total = 0;
    let mut mem_free = 0;
    let mut mem_aval = 0;
    let mut swap_total = 0;
    let mut swap_free = 0;

    for line in content.lines() {
        match line.split_whitespace().collect::<Vec<&str>>().as_slice() {
            ["MemTotal:", total, ..] => mem_total = total.parse().unwrap(),
            ["MemFree:", free, ..] => mem_free = free.parse().unwrap(),
            ["MemAvailable:", aval, ..] => mem_aval = aval.parse().unwrap(),
            ["SwapTotal:", total, ..] => swap_total = total.parse().unwrap(),
            ["SwapFree:", free, ..] => swap_free = free.parse().unwrap(),
            _ => {}
        }
    }

    let elapsed = now.elapsed();
    //println!("TIME READ MEMORY: {:.2?}", elapsed);

    (mem_total, mem_free, mem_aval, swap_total, swap_free)
}

pub fn read_process_swap_usage_with_names() -> io::Result<()> {
    // DEBUG TIME
    use std::time::Instant;
    let now = Instant::now();

    let mut swap_usage_per_process = Vec::new();
    for entry in fs::read_dir("/proc")? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            if let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() {
                let status_path = format!("/proc/{}/status", pid);
                if let Ok(contents) = fs::read_to_string(&status_path) {
                    let mut name = String::new();
                    let mut swap_usage: u64 = 0;
                    for line in contents.lines() {
                        if line.starts_with("Name:") {
                            name = line.split_whitespace().nth(1).unwrap_or("").to_string();
                        } else if line.starts_with("VmSwap:") {
                            swap_usage = line
                                .split_whitespace()
                                .nth(1)
                                .unwrap_or("0")
                                .parse()
                                .unwrap_or(0);
                        }
                    }
                    if swap_usage > 0 {
                        swap_usage_per_process.push((pid, name, swap_usage));
                    }
                }
            }
        }
    }
    swap_usage_per_process.sort_by(|a, b| b.2.cmp(&a.2));
    // for (pid, name, swap) in swap_usage_per_process.iter().take(10) {
    //     println!("PID: {}, Name: {}, Swap: {} kB", pid, name, swap);
    // }
    let elapsed = now.elapsed();
    //println!("TIME READ SWAP PROCESS USE: {:.2?}", elapsed);
    Ok(())
}
