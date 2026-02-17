//! Apache Tomcat monitoring plugin.
//!
//! Monitors the Tomcat Java application server using:
//! - cgroup v2/v1 for aggregate metrics (preferred)
//! - /proc filesystem for process-level details (fallback)
//!
//! Identifies Tomcat processes by looking for Java processes with
//! tomcat or catalina in their command line.

use super::cgroup;
use super::CgroupVersion;
use super::{DataSource, PluginConfig, PluginState, ProcessMetrics, ServiceMetrics, ServicePlugin};
use regex::Regex;
use std::fs;
use std::io;
use std::path::Path;

/// Plugin for monitoring Apache Tomcat
pub struct TomcatPlugin {
    /// Compiled regex patterns for process matching
    patterns: Vec<Regex>,
}

impl TomcatPlugin {
    pub fn new() -> Self {
        Self {
            patterns: vec![
                Regex::new(r"java.*tomcat").unwrap(),
                Regex::new(r"java.*catalina").unwrap(),
                Regex::new(r"org\.apache\.catalina\.startup\.Bootstrap").unwrap(),
            ],
        }
    }

    /// Try to collect metrics from cgroup
    fn collect_from_cgroup(
        &self,
        config: &PluginConfig,
        state: &mut PluginState,
        cgroup_path: &Path,
        version: CgroupVersion,
    ) -> io::Result<ServiceMetrics> {
        let stats = cgroup::read_cgroup_stats(cgroup_path, version)?;

        let now = std::time::Instant::now();
        let elapsed_secs = state
            .last_collection_time
            .map(|t| now.duration_since(t).as_secs_f64())
            .unwrap_or(1.0);
        state.last_collection_time = Some(now);

        let cpu_percent = state.calculate_cpu_percent(stats.cpu.usage_usec, elapsed_secs);

        // Get thread count from cgroup
        let thread_count = cgroup::get_cgroup_thread_count(cgroup_path).unwrap_or(0);

        Ok(ServiceMetrics {
            service_name: config.service_name.clone(),
            plugin_type: "tomcat".to_string(),
            is_running: true,
            cpu_usage_usec: stats.cpu.usage_usec,
            cpu_percent,
            cpu_user_usec: stats.cpu.user_usec,
            cpu_system_usec: stats.cpu.system_usec,
            memory_current_bytes: stats.memory.current_bytes,
            memory_swap_bytes: stats.memory.swap_bytes,
            memory_anon_bytes: stats.memory.anon_bytes,
            memory_file_bytes: stats.memory.file_bytes,
            disk_read_bytes: stats.io.read_bytes,
            disk_write_bytes: stats.io.write_bytes,
            disk_read_ops: stats.io.read_ios,
            disk_write_ops: stats.io.write_ios,
            net_rx_bytes: 0,
            net_tx_bytes: 0,
            process_count: stats.pids.current,
            thread_count,
            cgroup_version: Some(match version {
                CgroupVersion::V2 => 2,
                CgroupVersion::V1 => 1,
                CgroupVersion::None => 0,
            }),
            data_source: match version {
                CgroupVersion::V2 => DataSource::CgroupV2,
                CgroupVersion::V1 => DataSource::CgroupV1,
                CgroupVersion::None => DataSource::Unknown,
            },
            processes: vec![],
        })
    }

    /// Collect metrics by scanning /proc for Tomcat processes
    fn collect_from_proc(
        &self,
        config: &PluginConfig,
        state: &mut PluginState,
        collect_processes: bool,
    ) -> io::Result<ServiceMetrics> {
        let patterns = self.compile_patterns(config);
        let processes = scan_java_processes_by_pattern(&patterns)?;

        if processes.is_empty() {
            return Ok(ServiceMetrics::not_running(&config.service_name, "tomcat"));
        }

        let now = std::time::Instant::now();
        let elapsed_secs = state
            .last_collection_time
            .map(|t| now.duration_since(t).as_secs_f64())
            .unwrap_or(1.0);
        state.last_collection_time = Some(now);

        // Aggregate metrics across all Tomcat processes
        let mut total_cpu_ticks: u64 = 0;
        let mut total_memory: u64 = 0;
        let mut total_threads: u64 = 0;
        let mut total_io_read: u64 = 0;
        let mut total_io_write: u64 = 0;
        let mut process_details: Vec<ProcessMetrics> = vec![];

        let ticks_per_sec = get_clock_ticks_per_sec();

        for proc in &processes {
            total_cpu_ticks += proc.utime + proc.stime;
            total_memory += proc.rss_bytes;
            total_threads += proc.threads;
            total_io_read += proc.io_read_bytes;
            total_io_write += proc.io_write_bytes;

            if collect_processes {
                let prev_ticks = state.process_cpu_ticks.get(&proc.pid).copied().unwrap_or(0);
                let delta_ticks = (proc.utime + proc.stime).saturating_sub(prev_ticks);
                let cpu_percent = if elapsed_secs > 0.0 {
                    (delta_ticks as f64 / ticks_per_sec as f64) / elapsed_secs * 100.0
                } else {
                    0.0
                };
                state
                    .process_cpu_ticks
                    .insert(proc.pid, proc.utime + proc.stime);

                process_details.push(ProcessMetrics {
                    pid: proc.pid,
                    name: proc.name.clone(),
                    cpu_percent,
                    memory_bytes: proc.rss_bytes,
                    threads: proc.threads,
                });
            }
        }

        let cpu_usec = total_cpu_ticks * 1_000_000 / ticks_per_sec;
        let cpu_percent = state.calculate_cpu_percent(cpu_usec, elapsed_secs);

        Ok(ServiceMetrics {
            service_name: config.service_name.clone(),
            plugin_type: "tomcat".to_string(),
            is_running: true,
            cpu_usage_usec: cpu_usec,
            cpu_percent,
            cpu_user_usec: 0,
            cpu_system_usec: 0,
            memory_current_bytes: total_memory,
            memory_swap_bytes: 0,
            memory_anon_bytes: total_memory,
            memory_file_bytes: 0,
            disk_read_bytes: total_io_read,
            disk_write_bytes: total_io_write,
            disk_read_ops: 0,
            disk_write_ops: 0,
            net_rx_bytes: 0,
            net_tx_bytes: 0,
            process_count: processes.len() as u32,
            thread_count: total_threads,
            cgroup_version: None,
            data_source: DataSource::ProcFs,
            processes: process_details,
        })
    }

    fn compile_patterns(&self, config: &PluginConfig) -> Vec<Regex> {
        if config.process_patterns.is_empty() {
            self.patterns.clone()
        } else {
            config
                .process_patterns
                .iter()
                .filter_map(|p| Regex::new(p).ok())
                .collect()
        }
    }
}

impl Default for TomcatPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ServicePlugin for TomcatPlugin {
    fn name(&self) -> &'static str {
        "tomcat"
    }

    fn is_available(&self, config: &PluginConfig) -> bool {
        // Check if cgroup path exists
        if cgroup::cgroup_path_exists(&config.service_name).is_some() {
            return true;
        }

        // Check if any Tomcat processes are running
        let patterns = self.compile_patterns(config);
        if let Ok(procs) = scan_java_processes_by_pattern(&patterns) {
            return !procs.is_empty();
        }

        false
    }

    fn collect(
        &self,
        config: &PluginConfig,
        state: &mut PluginState,
        collect_processes: bool,
    ) -> io::Result<ServiceMetrics> {
        // Try cgroup first
        if let Some((path, version)) = cgroup::cgroup_path_exists(&config.service_name) {
            match self.collect_from_cgroup(config, state, &path, version) {
                Ok(mut metrics) => {
                    if collect_processes {
                        let patterns = self.compile_patterns(config);
                        if let Ok(procs) = scan_java_processes_by_pattern(&patterns) {
                            metrics.processes = procs
                                .iter()
                                .map(|p| ProcessMetrics {
                                    pid: p.pid,
                                    name: p.name.clone(),
                                    cpu_percent: 0.0,
                                    memory_bytes: p.rss_bytes,
                                    threads: p.threads,
                                })
                                .collect();
                        }
                    }
                    return Ok(metrics);
                }
                Err(_) => {}
            }
        }

        // Fallback to /proc scanning
        self.collect_from_proc(config, state, collect_processes)
    }
}

/// Process info from /proc scanning
#[derive(Debug)]
struct JavaProcInfo {
    pid: u32,
    name: String,
    utime: u64,
    stime: u64,
    rss_bytes: u64,
    threads: u64,
    io_read_bytes: u64,
    io_write_bytes: u64,
}

/// Scan /proc for Java processes matching Tomcat patterns
fn scan_java_processes_by_pattern(patterns: &[Regex]) -> io::Result<Vec<JavaProcInfo>> {
    let mut results = vec![];

    let proc_dir = fs::read_dir("/proc")?;
    let page_size = get_page_size();

    for entry in proc_dir.flatten() {
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();

        let pid: u32 = match name_str.parse() {
            Ok(p) => p,
            Err(_) => continue,
        };

        let proc_path = entry.path();

        // Read comm for process name
        let comm_path = proc_path.join("comm");
        let comm = match fs::read_to_string(&comm_path) {
            Ok(c) => c.trim().to_string(),
            Err(_) => continue,
        };

        // For Tomcat, we need to check cmdline since comm is just "java"
        let cmdline_path = proc_path.join("cmdline");
        let cmdline = fs::read_to_string(&cmdline_path)
            .unwrap_or_default()
            .replace('\0', " ")
            .trim()
            .to_string();

        // Only match Java processes with Tomcat-related arguments
        if !comm.contains("java") && !cmdline.starts_with("java") {
            continue;
        }

        // Check if cmdline matches any pattern
        let matches = patterns.iter().any(|p| p.is_match(&cmdline));
        if !matches {
            continue;
        }

        // Read stat for CPU and threads
        let stat_path = proc_path.join("stat");
        let stat_content = match fs::read_to_string(&stat_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let (utime, stime, threads) = parse_stat(&stat_content);

        // Read statm for memory
        let statm_path = proc_path.join("statm");
        let rss_bytes = match fs::read_to_string(&statm_path) {
            Ok(c) => {
                let fields: Vec<&str> = c.split_whitespace().collect();
                if fields.len() >= 2 {
                    fields[1].parse::<u64>().unwrap_or(0) * page_size
                } else {
                    0
                }
            }
            Err(_) => 0,
        };

        // Read io for disk I/O
        let io_path = proc_path.join("io");
        let (io_read, io_write) = match fs::read_to_string(&io_path) {
            Ok(c) => parse_io(&c),
            Err(_) => (0, 0),
        };

        // Use a more descriptive name for Java processes
        let display_name = if cmdline.contains("catalina") {
            "tomcat-catalina".to_string()
        } else if cmdline.contains("Bootstrap") {
            "tomcat-bootstrap".to_string()
        } else {
            "tomcat-java".to_string()
        };

        results.push(JavaProcInfo {
            pid,
            name: display_name,
            utime,
            stime,
            rss_bytes,
            threads,
            io_read_bytes: io_read,
            io_write_bytes: io_write,
        });
    }

    Ok(results)
}

fn parse_stat(content: &str) -> (u64, u64, u64) {
    if let Some(end_paren) = content.rfind(')') {
        let after_comm = &content[end_paren + 2..];
        let fields: Vec<&str> = after_comm.split_whitespace().collect();

        if fields.len() > 17 {
            let utime: u64 = fields[11].parse().unwrap_or(0);
            let stime: u64 = fields[12].parse().unwrap_or(0);
            let threads: u64 = fields[17].parse().unwrap_or(1);
            return (utime, stime, threads);
        }
    }
    (0, 0, 1)
}

fn parse_io(content: &str) -> (u64, u64) {
    let mut read_bytes = 0;
    let mut write_bytes = 0;

    for line in content.lines() {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() == 2 {
            let value: u64 = parts[1].trim().parse().unwrap_or(0);
            match parts[0].trim() {
                "read_bytes" => read_bytes = value,
                "write_bytes" => write_bytes = value,
                _ => {}
            }
        }
    }

    (read_bytes, write_bytes)
}

fn get_page_size() -> u64 {
    unsafe { libc::sysconf(libc::_SC_PAGESIZE) as u64 }
}

fn get_clock_ticks_per_sec() -> u64 {
    unsafe { libc::sysconf(libc::_SC_CLK_TCK) as u64 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tomcat_plugin_name() {
        let plugin = TomcatPlugin::new();
        assert_eq!(plugin.name(), "tomcat");
    }

    #[test]
    fn test_tomcat_patterns() {
        let plugin = TomcatPlugin::new();
        assert!(plugin
            .patterns
            .iter()
            .any(|p| p.is_match("java -jar catalina.jar")));
        assert!(plugin
            .patterns
            .iter()
            .any(|p| p.is_match("java -Dcatalina.home=/opt/tomcat")));
        assert!(!plugin
            .patterns
            .iter()
            .any(|p| p.is_match("java -jar myapp.jar")));
    }
}
