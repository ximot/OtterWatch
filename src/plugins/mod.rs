//! Plugin system for monitoring specialized services.
//!
//! This module provides a trait-based plugin architecture for collecting metrics
//! from services like nginx, tomcat, and the agent itself. Plugins can use either
//! cgroup (v2/v1) for aggregate metrics or /proc for process-level details.

pub mod cgroup;
pub mod nginx;
pub mod self_monitor;
pub mod tomcat;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;

/// Cgroup version detected on the system
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CgroupVersion {
    V2,
    V1,
    None,
}

/// Configuration for a single plugin
#[derive(Debug, Clone, Deserialize, Default)]
pub struct PluginConfig {
    pub enabled: bool,
    #[serde(default)]
    pub service_name: String,
    #[serde(default)]
    pub process_patterns: Vec<String>,
    // Self-monitor specific
    #[serde(default)]
    pub collect_open_fds: bool,
    #[serde(default)]
    pub collect_io_stats: bool,
}

/// Plugin settings from settings.toml
#[derive(Debug, Clone, Deserialize)]
pub struct PluginSettings {
    #[serde(default = "default_plugin_interval")]
    pub plugin_interval_secs: u64,
    #[serde(default)]
    pub collect_process_details: bool,
    #[serde(default)]
    pub nginx: PluginConfig,
    #[serde(default)]
    pub tomcat: PluginConfig,
    #[serde(default = "default_self_config")]
    pub self_monitor: PluginConfig,
}

impl Default for PluginSettings {
    fn default() -> Self {
        Self {
            plugin_interval_secs: default_plugin_interval(),
            collect_process_details: false,
            nginx: PluginConfig::default(),
            tomcat: PluginConfig::default(),
            self_monitor: default_self_config(),
        }
    }
}

fn default_plugin_interval() -> u64 {
    10
}

fn default_self_config() -> PluginConfig {
    PluginConfig {
        enabled: false,
        service_name: "otterwatch".to_string(),
        process_patterns: vec![],
        collect_open_fds: true,
        collect_io_stats: true,
    }
}

impl PluginSettings {
    /// Check if any plugin is enabled
    pub fn has_enabled_plugins(&self) -> bool {
        self.nginx.enabled || self.tomcat.enabled || self.self_monitor.enabled
    }

    /// Convert to a map of plugin name to config
    pub fn to_configs(&self) -> HashMap<String, PluginConfig> {
        let mut configs = HashMap::new();

        if self.nginx.enabled {
            let mut nginx_config = self.nginx.clone();
            if nginx_config.service_name.is_empty() {
                nginx_config.service_name = "nginx".to_string();
            }
            if nginx_config.process_patterns.is_empty() {
                nginx_config.process_patterns = vec!["^nginx:".to_string()];
            }
            configs.insert("nginx".to_string(), nginx_config);
        }

        if self.tomcat.enabled {
            let mut tomcat_config = self.tomcat.clone();
            if tomcat_config.service_name.is_empty() {
                tomcat_config.service_name = "tomcat".to_string();
            }
            if tomcat_config.process_patterns.is_empty() {
                tomcat_config.process_patterns =
                    vec!["java.*tomcat".to_string(), "java.*catalina".to_string()];
            }
            configs.insert("tomcat".to_string(), tomcat_config);
        }

        if self.self_monitor.enabled {
            let mut self_config = self.self_monitor.clone();
            if self_config.service_name.is_empty() {
                self_config.service_name = "otterwatch".to_string();
            }
            configs.insert("self".to_string(), self_config);
        }

        configs
    }
}

/// Per-process metrics within a service
#[derive(Debug, Clone, Serialize, Default)]
pub struct ProcessMetrics {
    pub pid: u32,
    pub name: String,
    pub cpu_percent: f64,
    pub memory_bytes: u64,
    pub threads: u64,
}

/// Aggregated metrics for a service
#[derive(Debug, Clone, Serialize, Default)]
pub struct ServiceMetrics {
    pub service_name: String,
    pub plugin_type: String,
    pub is_running: bool,

    // CPU metrics
    pub cpu_usage_usec: u64,
    pub cpu_percent: f64,
    pub cpu_user_usec: u64,
    pub cpu_system_usec: u64,

    // Memory metrics
    pub memory_current_bytes: u64,
    pub memory_swap_bytes: u64,
    pub memory_anon_bytes: u64,
    pub memory_file_bytes: u64,

    // Disk I/O metrics
    pub disk_read_bytes: u64,
    pub disk_write_bytes: u64,
    pub disk_read_ops: u64,
    pub disk_write_ops: u64,

    // Network I/O metrics (requires network namespace tracking)
    pub net_rx_bytes: u64,
    pub net_tx_bytes: u64,

    // Process/thread counts
    pub process_count: u32,
    pub thread_count: u64,

    // Data source info
    pub cgroup_version: Option<u8>,
    pub data_source: DataSource,

    // Per-process details (optional)
    pub processes: Vec<ProcessMetrics>,
}

/// Where the metrics were collected from
#[derive(Debug, Clone, Copy, Serialize, Default, PartialEq, Eq)]
pub enum DataSource {
    #[default]
    Unknown,
    CgroupV2,
    CgroupV1,
    ProcFs,
}

impl ServiceMetrics {
    /// Create empty metrics for a service that's not running
    pub fn not_running(service_name: &str, plugin_type: &str) -> Self {
        Self {
            service_name: service_name.to_string(),
            plugin_type: plugin_type.to_string(),
            is_running: false,
            ..Default::default()
        }
    }

    /// Convert to Protocol Buffer ServiceMetrics message
    #[allow(dead_code)] // Public API for external use
    pub fn to_proto(&self) -> crate::proto::ServiceMetrics {
        crate::proto::ServiceMetrics {
            service_name: self.service_name.clone(),
            plugin_type: self.plugin_type.clone(),
            is_running: self.is_running,
            cpu_usage_usec: self.cpu_usage_usec,
            cpu_percent: self.cpu_percent,
            cpu_user_usec: self.cpu_user_usec,
            cpu_system_usec: self.cpu_system_usec,
            memory_current_bytes: self.memory_current_bytes,
            memory_swap_bytes: self.memory_swap_bytes,
            memory_anon_bytes: self.memory_anon_bytes,
            memory_file_bytes: self.memory_file_bytes,
            disk_read_bytes: self.disk_read_bytes,
            disk_write_bytes: self.disk_write_bytes,
            disk_read_ops: self.disk_read_ops,
            disk_write_ops: self.disk_write_ops,
            net_rx_bytes: self.net_rx_bytes,
            net_tx_bytes: self.net_tx_bytes,
            process_count: self.process_count,
            thread_count: self.thread_count,
            cgroup_version: self.cgroup_version.map(|v| v as u32),
            data_source: match self.data_source {
                DataSource::Unknown => 0,
                DataSource::CgroupV2 => 1,
                DataSource::CgroupV1 => 2,
                DataSource::ProcFs => 3,
            },
            processes: self
                .processes
                .iter()
                .map(|p| crate::proto::ServiceProcessInfo {
                    pid: p.pid,
                    name: p.name.clone(),
                    cpu_percent: p.cpu_percent,
                    memory_bytes: p.memory_bytes,
                    threads: p.threads,
                })
                .collect(),
        }
    }
}

/// State maintained between collection cycles for delta calculations
#[derive(Debug, Default)]
pub struct PluginState {
    pub prev_cpu_usec: u64,
    #[allow(dead_code)] // Reserved for future delta calculations
    pub prev_disk_read_bytes: u64,
    #[allow(dead_code)] // Reserved for future delta calculations
    pub prev_disk_write_bytes: u64,
    #[allow(dead_code)] // Reserved for future delta calculations
    pub prev_net_rx_bytes: u64,
    #[allow(dead_code)] // Reserved for future delta calculations
    pub prev_net_tx_bytes: u64,
    pub last_collection_time: Option<std::time::Instant>,
    /// Per-process CPU ticks for delta calculation
    pub process_cpu_ticks: HashMap<u32, u64>,
}

impl PluginState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Calculate CPU percentage based on elapsed time
    pub fn calculate_cpu_percent(&mut self, current_usec: u64, elapsed_secs: f64) -> f64 {
        if elapsed_secs <= 0.0 || self.prev_cpu_usec == 0 {
            self.prev_cpu_usec = current_usec;
            return 0.0;
        }

        let delta_usec = current_usec.saturating_sub(self.prev_cpu_usec);
        self.prev_cpu_usec = current_usec;

        // CPU usage percentage = (delta_usec / 1_000_000) / elapsed_secs * 100
        (delta_usec as f64 / 1_000_000.0) / elapsed_secs * 100.0
    }
}

/// Trait that all plugins must implement
#[allow(dead_code)] // Trait methods may not be called directly but are part of the interface
pub trait ServicePlugin: Send + Sync {
    /// Plugin identifier (e.g., "nginx", "tomcat", "self")
    fn name(&self) -> &'static str;

    /// Check if the service can be monitored (e.g., cgroup path exists)
    fn is_available(&self, config: &PluginConfig) -> bool;

    /// Collect metrics for the service
    fn collect(
        &self,
        config: &PluginConfig,
        state: &mut PluginState,
        collect_processes: bool,
    ) -> io::Result<ServiceMetrics>;
}

/// Registry of all available plugins
#[allow(dead_code)] // Public API for future use
pub struct PluginRegistry {
    plugins: HashMap<String, Box<dyn ServicePlugin>>,
}

#[allow(dead_code)] // Public API for future use
impl PluginRegistry {
    /// Create a new registry with all built-in plugins
    pub fn new() -> Self {
        let mut plugins: HashMap<String, Box<dyn ServicePlugin>> = HashMap::new();

        plugins.insert("nginx".to_string(), Box::new(nginx::NginxPlugin::new()));
        plugins.insert("tomcat".to_string(), Box::new(tomcat::TomcatPlugin::new()));
        plugins.insert(
            "self".to_string(),
            Box::new(self_monitor::SelfMonitorPlugin::new()),
        );

        Self { plugins }
    }

    /// Get a plugin by name
    pub fn get(&self, name: &str) -> Option<&dyn ServicePlugin> {
        self.plugins.get(name).map(|p| p.as_ref())
    }

    /// List all registered plugin names
    pub fn list_plugins(&self) -> Vec<&str> {
        self.plugins.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_settings_default() {
        let settings = PluginSettings::default();
        assert!(!settings.has_enabled_plugins());
        assert_eq!(settings.plugin_interval_secs, 10);
    }

    #[test]
    fn test_plugin_settings_to_configs() {
        let mut settings = PluginSettings::default();
        settings.nginx.enabled = true;
        settings.nginx.service_name = "my-nginx".to_string();

        let configs = settings.to_configs();
        assert!(configs.contains_key("nginx"));
        assert_eq!(configs["nginx"].service_name, "my-nginx");
    }

    #[test]
    fn test_plugin_state_cpu_calculation() {
        let mut state = PluginState::new();

        // First call - no delta
        let cpu1 = state.calculate_cpu_percent(1_000_000, 1.0);
        assert_eq!(cpu1, 0.0);

        // Second call - 1 second of CPU time in 1 second elapsed = 100%
        let cpu2 = state.calculate_cpu_percent(2_000_000, 1.0);
        assert!((cpu2 - 100.0).abs() < 0.001);

        // Third call - 0.5 second of CPU time in 1 second elapsed = 50%
        let cpu3 = state.calculate_cpu_percent(2_500_000, 1.0);
        assert!((cpu3 - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_service_metrics_not_running() {
        let metrics = ServiceMetrics::not_running("test-service", "test-plugin");
        assert!(!metrics.is_running);
        assert_eq!(metrics.service_name, "test-service");
        assert_eq!(metrics.plugin_type, "test-plugin");
    }

    #[test]
    fn test_plugin_registry() {
        let registry = PluginRegistry::new();
        assert!(registry.get("nginx").is_some());
        assert!(registry.get("tomcat").is_some());
        assert!(registry.get("self").is_some());
        assert!(registry.get("nonexistent").is_none());
    }
}
