//! Self-monitoring plugin for the OtterWatch agent itself.
//!
//! Provides enhanced metrics about the agent's own resource usage:
//! - CPU and memory usage
//! - Open file descriptors
//! - I/O statistics
//! - Thread count

use super::{DataSource, PluginConfig, PluginState, ProcessMetrics, ServiceMetrics, ServicePlugin};
use std::fs;
use std::io;
use std::path::Path;

/// Plugin for monitoring the OtterWatch agent itself
pub struct SelfMonitorPlugin;

impl SelfMonitorPlugin {
    pub fn new() -> Self {
        Self
    }

    /// Read the agent's own process stats from /proc/self
    fn read_self_stats(&self, config: &PluginConfig) -> io::Result<SelfStats> {
        let mut stats = SelfStats::default();

        // Read /proc/self/stat for CPU ticks and threads
        if let Ok(content) = fs::read_to_string("/proc/self/stat") {
            // Format: pid (comm) state ppid ... utime stime ... num_threads ...
            // Fields are space-separated, but comm can contain spaces and is in parens
            if let Some(end_paren) = content.rfind(')') {
                let after_comm = &content[end_paren + 2..];
                let fields: Vec<&str> = after_comm.split_whitespace().collect();

                // utime is field 11 (index 11 after comm)
                // stime is field 12
                // num_threads is field 17
                if fields.len() > 17 {
                    stats.utime_ticks = fields[11].parse().unwrap_or(0);
                    stats.stime_ticks = fields[12].parse().unwrap_or(0);
                    stats.num_threads = fields[17].parse().unwrap_or(1);
                }
            }
        }

        // Read /proc/self/statm for memory
        if let Ok(content) = fs::read_to_string("/proc/self/statm") {
            let fields: Vec<&str> = content.split_whitespace().collect();
            if fields.len() >= 2 {
                let page_size = get_page_size();
                stats.vsize_pages = fields[0].parse().unwrap_or(0);
                stats.rss_pages = fields[1].parse().unwrap_or(0);
                stats.memory_bytes = stats.rss_pages * page_size;
            }
        }

        // Read /proc/self/io for I/O stats if enabled and accessible
        if config.collect_io_stats {
            if let Ok(content) = fs::read_to_string("/proc/self/io") {
                for line in content.lines() {
                    let parts: Vec<&str> = line.split(':').collect();
                    if parts.len() == 2 {
                        let value: u64 = parts[1].trim().parse().unwrap_or(0);
                        match parts[0].trim() {
                            "read_bytes" => stats.io_read_bytes = value,
                            "write_bytes" => stats.io_write_bytes = value,
                            "rchar" => stats.io_rchar = value,
                            "wchar" => stats.io_wchar = value,
                            _ => {}
                        }
                    }
                }
            }
        }

        // Count open file descriptors if enabled
        if config.collect_open_fds {
            if let Ok(entries) = fs::read_dir("/proc/self/fd") {
                stats.open_fds = entries.count() as u32;
            }
        }

        Ok(stats)
    }
}

impl Default for SelfMonitorPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Default)]
struct SelfStats {
    utime_ticks: u64,
    stime_ticks: u64,
    num_threads: u64,
    vsize_pages: u64,
    rss_pages: u64,
    memory_bytes: u64,
    io_read_bytes: u64,
    io_write_bytes: u64,
    io_rchar: u64,
    io_wchar: u64,
    open_fds: u32,
}

impl ServicePlugin for SelfMonitorPlugin {
    fn name(&self) -> &'static str {
        "self"
    }

    fn is_available(&self, _config: &PluginConfig) -> bool {
        // Always available - we can always read /proc/self
        Path::new("/proc/self/stat").exists()
    }

    fn collect(
        &self,
        config: &PluginConfig,
        state: &mut PluginState,
        _collect_processes: bool,
    ) -> io::Result<ServiceMetrics> {
        let stats = self.read_self_stats(config)?;

        // Calculate elapsed time since last collection
        let now = std::time::Instant::now();
        let elapsed_secs = state
            .last_collection_time
            .map(|t| now.duration_since(t).as_secs_f64())
            .unwrap_or(1.0);
        state.last_collection_time = Some(now);

        // Convert ticks to microseconds (assuming 100 Hz = 10000 us per tick)
        let ticks_per_sec = get_clock_ticks_per_sec();
        let total_ticks = stats.utime_ticks + stats.stime_ticks;
        let cpu_usec = total_ticks * 1_000_000 / ticks_per_sec;

        // Calculate CPU percentage using delta
        let cpu_percent = state.calculate_cpu_percent(cpu_usec, elapsed_secs);

        let mut metrics = ServiceMetrics {
            service_name: config.service_name.clone(),
            plugin_type: "self".to_string(),
            is_running: true,
            cpu_usage_usec: cpu_usec,
            cpu_percent,
            cpu_user_usec: stats.utime_ticks * 1_000_000 / ticks_per_sec,
            cpu_system_usec: stats.stime_ticks * 1_000_000 / ticks_per_sec,
            memory_current_bytes: stats.memory_bytes,
            memory_swap_bytes: 0, // Would need to parse smaps for accurate swap
            memory_anon_bytes: stats.memory_bytes, // RSS is mostly anonymous for this agent
            memory_file_bytes: 0,
            disk_read_bytes: stats.io_read_bytes,
            disk_write_bytes: stats.io_write_bytes,
            disk_read_ops: 0, // Not tracked at process level
            disk_write_ops: 0,
            net_rx_bytes: 0, // Not tracked at process level
            net_tx_bytes: 0,
            process_count: 1,
            thread_count: stats.num_threads,
            cgroup_version: None,
            data_source: DataSource::ProcFs,
            processes: vec![],
        };

        // Add self process details
        let pid = std::process::id();
        metrics.processes.push(ProcessMetrics {
            pid,
            name: "otterwatch".to_string(),
            cpu_percent,
            memory_bytes: stats.memory_bytes,
            threads: stats.num_threads,
        });

        Ok(metrics)
    }
}

/// Get system page size in bytes
fn get_page_size() -> u64 {
    // SAFETY: sysconf is safe to call
    unsafe { libc::sysconf(libc::_SC_PAGESIZE) as u64 }
}

/// Get clock ticks per second (usually 100 on Linux)
fn get_clock_ticks_per_sec() -> u64 {
    // SAFETY: sysconf is safe to call
    unsafe { libc::sysconf(libc::_SC_CLK_TCK) as u64 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_self_monitor_available() {
        let plugin = SelfMonitorPlugin::new();
        let config = PluginConfig::default();
        assert!(plugin.is_available(&config));
    }

    #[test]
    fn test_self_monitor_collect() {
        let plugin = SelfMonitorPlugin::new();
        let config = PluginConfig {
            enabled: true,
            service_name: "otterwatch".to_string(),
            collect_open_fds: true,
            collect_io_stats: true,
            ..Default::default()
        };
        let mut state = PluginState::new();

        let metrics = plugin.collect(&config, &mut state, true).unwrap();
        assert!(metrics.is_running);
        assert_eq!(metrics.process_count, 1);
        assert!(metrics.memory_current_bytes > 0);
        assert!(metrics.thread_count > 0);
    }
}
