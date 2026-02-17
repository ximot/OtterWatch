use std::collections::HashMap;
use std::fs;
use std::io;

/// Detailed process information
#[derive(Clone, Debug, Default)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub state: String,
    pub ppid: u32,
    pub cpu_percent: f64,
    pub memory_rss_kib: u64,
    pub memory_vsz_kib: u64,
    pub threads: u64,
    pub user: String,
    pub cmdline: String,
    pub start_time: u64,
    // Internal tracking for CPU calculation
    #[allow(dead_code)] // Used for CPU tracking
    pub total_ticks: u64,
}

/// Simple process snapshot for internal tracking
#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessSnapshot {
    #[allow(dead_code)] // Used for CPU tracking
    pub total_ticks: u64,
    pub rss_pages: u64,
}

/// State for tracking CPU usage between samples
pub struct ProcessTracker {
    prev_samples: HashMap<u32, u64>,
    prev_total_cpu: u64,
}

impl ProcessTracker {
    pub fn new() -> Self {
        Self {
            prev_samples: HashMap::new(),
            prev_total_cpu: 0,
        }
    }

    /// Collects top N processes by CPU usage
    pub fn collect_top_processes(&mut self, top_n: usize) -> io::Result<Vec<ProcessInfo>> {
        let page_size = page_size_bytes();
        let clock_ticks = clock_ticks_per_second();

        // Read current total CPU time
        let current_total_cpu = read_total_cpu_time()?;
        let cpu_delta = current_total_cpu.saturating_sub(self.prev_total_cpu);

        let mut processes = Vec::new();
        let mut current_samples = HashMap::new();

        for entry in fs::read_dir("/proc")? {
            let entry = entry?;
            let filename = entry.file_name();
            let name_str = filename.to_string_lossy();

            // Only process numeric directories (PIDs)
            let pid: u32 = match name_str.parse() {
                Ok(p) => p,
                Err(_) => continue,
            };

            let proc_path = format!("/proc/{}", pid);

            // Read /proc/[pid]/stat
            let stat_path = format!("{}/stat", proc_path);
            let stat_content = match fs::read_to_string(&stat_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            // Parse stat file - handle process names with spaces/parentheses
            let (name, rest) = parse_stat_name(&stat_content);
            let stats: Vec<&str> = rest.split_whitespace().collect();

            if stats.len() < 20 {
                continue;
            }

            // Extract fields from stat (indices are offset by 2 due to name parsing)
            let state = stats.get(0).map(|s| s.to_string()).unwrap_or_default();
            let ppid: u32 = stats.get(1).and_then(|v| v.parse().ok()).unwrap_or(0);
            let utime: u64 = stats.get(11).and_then(|v| v.parse().ok()).unwrap_or(0);
            let stime: u64 = stats.get(12).and_then(|v| v.parse().ok()).unwrap_or(0);
            let num_threads: u64 = stats.get(17).and_then(|v| v.parse().ok()).unwrap_or(1);
            let starttime: u64 = stats.get(19).and_then(|v| v.parse().ok()).unwrap_or(0);
            let vsize: u64 = stats.get(20).and_then(|v| v.parse().ok()).unwrap_or(0);
            let rss_pages: u64 = stats.get(21).and_then(|v| v.parse().ok()).unwrap_or(0);

            let total_ticks = utime.saturating_add(stime);
            current_samples.insert(pid, total_ticks);

            // Calculate CPU percentage
            let prev_ticks = self.prev_samples.get(&pid).copied().unwrap_or(0);
            let proc_delta = total_ticks.saturating_sub(prev_ticks);

            let cpu_percent = if cpu_delta > 0 {
                (proc_delta as f64 / cpu_delta as f64) * 100.0 * num_cpus() as f64
            } else {
                0.0
            };

            // Read cmdline
            let cmdline_path = format!("{}/cmdline", proc_path);
            let cmdline = fs::read_to_string(&cmdline_path)
                .unwrap_or_default()
                .replace('\0', " ")
                .trim()
                .to_string();

            // Read owner (UID)
            let status_path = format!("{}/status", proc_path);
            let uid = read_uid_from_status(&status_path);
            let user = uid_to_username(uid);

            // Convert to KiB
            let memory_rss_kib = (rss_pages * page_size) / 1024;
            let memory_vsz_kib = vsize / 1024;

            processes.push(ProcessInfo {
                pid,
                name,
                state,
                ppid,
                cpu_percent,
                memory_rss_kib,
                memory_vsz_kib,
                threads: num_threads,
                user,
                cmdline,
                start_time: starttime / clock_ticks, // Convert to seconds since boot
                total_ticks,
            });
        }

        // Update tracking state
        self.prev_samples = current_samples;
        self.prev_total_cpu = current_total_cpu;

        // Sort by CPU usage (descending) and take top N
        processes.sort_by(|a, b| {
            b.cpu_percent
                .partial_cmp(&a.cpu_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        processes.truncate(top_n);

        Ok(processes)
    }
}

impl Default for ProcessTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse process name from stat file (handles names with spaces/parentheses)
fn parse_stat_name(stat_content: &str) -> (String, &str) {
    // Format: pid (name) state ppid ...
    // Name is enclosed in parentheses and may contain spaces
    let start = stat_content.find('(').unwrap_or(0) + 1;
    let end = stat_content.rfind(')').unwrap_or(stat_content.len());

    let name = stat_content[start..end].to_string();
    let rest = &stat_content[end + 1..].trim_start();

    (name, rest)
}

/// Read UID from /proc/[pid]/status
fn read_uid_from_status(path: &str) -> u32 {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return 0,
    };

    for line in content.lines() {
        if line.starts_with("Uid:") {
            return line
                .split_whitespace()
                .nth(1)
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
        }
    }
    0
}

/// Convert UID to username (simple implementation)
fn uid_to_username(uid: u32) -> String {
    // Try to read /etc/passwd for username lookup
    if let Ok(passwd) = fs::read_to_string("/etc/passwd") {
        for line in passwd.lines() {
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
    // Fallback to UID string
    uid.to_string()
}

/// Read total CPU time from /proc/stat
fn read_total_cpu_time() -> io::Result<u64> {
    let stat = fs::read_to_string("/proc/stat")?;

    for line in stat.lines() {
        if line.starts_with("cpu ") {
            let values: Vec<u64> = line
                .split_whitespace()
                .skip(1) // Skip "cpu"
                .filter_map(|v| v.parse().ok())
                .collect();

            return Ok(values.iter().sum());
        }
    }

    Ok(0)
}

/// Get number of CPU cores
fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(1)
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
