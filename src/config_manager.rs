//! Configuration management module.
//!
//! Provides functionality for:
//! - Defining configuration schema with types, defaults, and validation rules
//! - Synchronizing settings.toml with the schema (adding missing parameters)
//! - Setting individual configuration values with validation
//! - Generating configuration schema for dashboard display

use crate::proto;
use std::fs;
use std::path::Path;
use toml_edit::{DocumentMut, Item};

/// Parameter type for configuration values
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    Int,
    Bool,
    String,
    #[allow(dead_code)] // Reserved for future use
    Float,
}

impl ParamType {
    fn as_str(&self) -> &'static str {
        match self {
            ParamType::Int => "int",
            ParamType::Bool => "bool",
            ParamType::String => "string",
            ParamType::Float => "float",
        }
    }
}

/// Definition of a configuration parameter
pub struct ConfigDefinition {
    pub key: &'static str,
    pub default: &'static str,
    pub param_type: ParamType,
    pub description: &'static str,
    pub min: Option<&'static str>,
    pub max: Option<&'static str>,
    pub added_in_version: &'static str,
}

/// Configuration schema - all known parameters with their metadata
pub const CONFIG_SCHEMA: &[ConfigDefinition] = &[
    // Collection settings
    ConfigDefinition {
        key: "interval_secs",
        default: "1",
        param_type: ParamType::Int,
        description: "Metrics collection interval (seconds)",
        min: Some("1"),
        max: Some("3600"),
        added_in_version: "0.1.0",
    },
    ConfigDefinition {
        key: "process_list_interval_secs",
        default: "10",
        param_type: ParamType::Int,
        description: "Process list collection interval (seconds)",
        min: Some("5"),
        max: Some("3600"),
        added_in_version: "0.1.0",
    },
    ConfigDefinition {
        key: "process_top_n",
        default: "25",
        param_type: ParamType::Int,
        description: "Number of top processes to collect",
        min: Some("1"),
        max: Some("100"),
        added_in_version: "0.1.0",
    },
    // Network settings
    ConfigDefinition {
        key: "listen_addr",
        default: "127.0.0.1:8080",
        param_type: ParamType::String,
        description: "HTTP API listen address",
        min: None,
        max: None,
        added_in_version: "0.1.0",
    },
    ConfigDefinition {
        key: "cors_allowed_origins",
        default: "*",
        param_type: ParamType::String,
        description: "CORS allowed origins (comma-separated)",
        min: None,
        max: None,
        added_in_version: "0.1.0",
    },
    // Storage settings
    ConfigDefinition {
        key: "db_file_name",
        default: "system_stats.db",
        param_type: ParamType::String,
        description: "Local storage directory name",
        min: None,
        max: None,
        added_in_version: "0.1.0",
    },
    ConfigDefinition {
        key: "db_save",
        default: "true",
        param_type: ParamType::Bool,
        description: "Enable local storage",
        min: None,
        max: None,
        added_in_version: "0.1.0",
    },
    ConfigDefinition {
        key: "db_history_days",
        default: "31",
        param_type: ParamType::Int,
        description: "Days of history to keep",
        min: Some("1"),
        max: Some("365"),
        added_in_version: "0.1.0",
    },
    ConfigDefinition {
        key: "exclude_interfaces",
        default: "",
        param_type: ParamType::String,
        description: "Network interfaces to exclude (comma-separated)",
        min: None,
        max: None,
        added_in_version: "0.1.0",
    },
    // MQTT settings
    ConfigDefinition {
        key: "mqtt_enabled",
        default: "false",
        param_type: ParamType::Bool,
        description: "Enable MQTT metrics push",
        min: None,
        max: None,
        added_in_version: "0.1.0",
    },
    ConfigDefinition {
        key: "mqtt_broker_addr",
        default: "tcp://localhost:1883",
        param_type: ParamType::String,
        description: "MQTT broker address",
        min: None,
        max: None,
        added_in_version: "0.1.0",
    },
    ConfigDefinition {
        key: "mqtt_topic_prefix",
        default: "otterwatch/metrics",
        param_type: ParamType::String,
        description: "MQTT topic prefix",
        min: None,
        max: None,
        added_in_version: "0.1.0",
    },
    ConfigDefinition {
        key: "mqtt_keepalive_secs",
        default: "30",
        param_type: ParamType::Int,
        description: "MQTT keepalive interval (seconds)",
        min: Some("10"),
        max: Some("600"),
        added_in_version: "0.1.0",
    },
    ConfigDefinition {
        key: "mqtt_retry_interval_secs",
        default: "5",
        param_type: ParamType::Int,
        description: "MQTT reconnection retry interval (seconds)",
        min: Some("1"),
        max: Some("60"),
        added_in_version: "0.1.0",
    },
    ConfigDefinition {
        key: "mqtt_queue_path",
        default: "mqtt_queue",
        param_type: ParamType::String,
        description: "Offline queue directory",
        min: None,
        max: None,
        added_in_version: "0.1.10",
    },
    ConfigDefinition {
        key: "mqtt_queue_max_size_mb",
        default: "100",
        param_type: ParamType::Int,
        description: "Maximum offline queue size (MB)",
        min: Some("10"),
        max: Some("10000"),
        added_in_version: "0.1.10",
    },
    // MQTT Bootstrap settings (v0.3.2)
    ConfigDefinition {
        key: "mqtt_bootstrap_url",
        default: "",
        param_type: ParamType::String,
        description: "Bootstrap URL for broker auto-discovery",
        min: None,
        max: None,
        added_in_version: "0.3.2",
    },
    ConfigDefinition {
        key: "mqtt_bootstrap_timeout_secs",
        default: "10",
        param_type: ParamType::Int,
        description: "Bootstrap request timeout (seconds)",
        min: Some("1"),
        max: Some("60"),
        added_in_version: "0.3.2",
    },
    // Agent settings
    ConfigDefinition {
        key: "agent_group",
        default: "",
        param_type: ParamType::String,
        description: "Agent group name for categorization",
        min: None,
        max: None,
        added_in_version: "0.1.0",
    },
    // Plugin settings (new in 0.2.0)
    ConfigDefinition {
        key: "plugins.plugin_interval_secs",
        default: "10",
        param_type: ParamType::Int,
        description: "Plugin metrics collection interval (seconds)",
        min: Some("5"),
        max: Some("3600"),
        added_in_version: "0.2.0",
    },
    ConfigDefinition {
        key: "plugins.collect_process_details",
        default: "false",
        param_type: ParamType::Bool,
        description: "Collect per-process details in plugins",
        min: None,
        max: None,
        added_in_version: "0.2.0",
    },
    ConfigDefinition {
        key: "plugins.nginx.enabled",
        default: "false",
        param_type: ParamType::Bool,
        description: "Enable nginx monitoring plugin",
        min: None,
        max: None,
        added_in_version: "0.2.0",
    },
    ConfigDefinition {
        key: "plugins.nginx.service_name",
        default: "nginx",
        param_type: ParamType::String,
        description: "Nginx systemd service name",
        min: None,
        max: None,
        added_in_version: "0.2.0",
    },
    ConfigDefinition {
        key: "plugins.tomcat.enabled",
        default: "false",
        param_type: ParamType::Bool,
        description: "Enable Tomcat monitoring plugin",
        min: None,
        max: None,
        added_in_version: "0.2.0",
    },
    ConfigDefinition {
        key: "plugins.tomcat.service_name",
        default: "tomcat",
        param_type: ParamType::String,
        description: "Tomcat systemd service name",
        min: None,
        max: None,
        added_in_version: "0.2.0",
    },
    ConfigDefinition {
        key: "plugins.self_monitor.enabled",
        default: "false",
        param_type: ParamType::Bool,
        description: "Enable enhanced self-monitoring plugin",
        min: None,
        max: None,
        added_in_version: "0.2.0",
    },
    ConfigDefinition {
        key: "plugins.self_monitor.collect_open_fds",
        default: "true",
        param_type: ParamType::Bool,
        description: "Collect open file descriptor count",
        min: None,
        max: None,
        added_in_version: "0.2.0",
    },
    ConfigDefinition {
        key: "plugins.self_monitor.collect_io_stats",
        default: "true",
        param_type: ParamType::Bool,
        description: "Collect I/O statistics for self",
        min: None,
        max: None,
        added_in_version: "0.2.0",
    },
];

/// Synchronize settings.toml with the configuration schema.
/// Adds missing parameters with their default values.
/// Returns a list of parameters that were added.
pub fn sync_config(config_path: &Path) -> Result<Vec<String>, String> {
    let settings_path = config_path.join("settings.toml");

    let content = fs::read_to_string(&settings_path)
        .map_err(|e| format!("Failed to read settings.toml: {}", e))?;

    let mut doc = content
        .parse::<DocumentMut>()
        .map_err(|e| format!("Failed to parse settings.toml: {}", e))?;

    let mut added = Vec::new();

    for def in CONFIG_SCHEMA {
        if !key_exists_in_doc(&doc, def.key) {
            set_nested_key(&mut doc, def.key, def.default, def.param_type);
            added.push(format!(
                "{} = {} ({})",
                def.key, def.default, def.description
            ));
        }
    }

    if !added.is_empty() {
        // Create backup before modifying
        let backup_path = settings_path.with_extension("toml.bak");
        let _ = fs::copy(&settings_path, &backup_path);

        fs::write(&settings_path, doc.to_string())
            .map_err(|e| format!("Failed to write settings.toml: {}", e))?;
    }

    Ok(added)
}

/// Set a single configuration value with validation.
pub fn set_config_value(config_path: &Path, key: &str, value: &str) -> Result<(), String> {
    // Find the parameter definition
    let def = CONFIG_SCHEMA
        .iter()
        .find(|d| d.key == key)
        .ok_or_else(|| format!("Unknown parameter: {}", key))?;

    // Validate the value
    validate_value(value, def)?;

    // Read and modify the config file
    let settings_path = config_path.join("settings.toml");
    let content = fs::read_to_string(&settings_path)
        .map_err(|e| format!("Failed to read settings.toml: {}", e))?;

    let mut doc = content
        .parse::<DocumentMut>()
        .map_err(|e| format!("Failed to parse settings.toml: {}", e))?;

    // Create backup before modifying
    let backup_path = settings_path.with_extension("toml.bak");
    let _ = fs::copy(&settings_path, &backup_path);

    set_nested_key(&mut doc, key, value, def.param_type);

    fs::write(&settings_path, doc.to_string())
        .map_err(|e| format!("Failed to write settings.toml: {}", e))?;

    Ok(())
}

/// Get the full configuration schema as a proto message.
pub fn get_config_schema() -> proto::ConfigSchema {
    let current_version = env!("CARGO_PKG_VERSION");

    let parameters: Vec<proto::ConfigParameter> = CONFIG_SCHEMA
        .iter()
        .map(|def| {
            let is_new = is_newer_version(def.added_in_version, "0.1.12");
            proto::ConfigParameter {
                key: def.key.to_string(),
                value: def.default.to_string(),
                param_type: def.param_type.as_str().to_string(),
                description: def.description.to_string(),
                default_value: Some(def.default.to_string()),
                min_value: def.min.map(|s| s.to_string()),
                max_value: def.max.map(|s| s.to_string()),
                is_new,
                added_in_version: def.added_in_version.to_string(),
            }
        })
        .collect();

    proto::ConfigSchema {
        parameters,
        agent_version: current_version.to_string(),
        config_version: "1.0".to_string(),
    }
}

/// Check if a key exists in the TOML document.
fn key_exists_in_doc(doc: &DocumentMut, key: &str) -> bool {
    let parts: Vec<&str> = key.split('.').collect();

    let mut current: &toml_edit::Item = doc.as_item();

    for part in parts {
        match current.get(part) {
            Some(item) => current = item,
            None => return false,
        }
    }

    !current.is_none()
}

/// Set a nested key in the TOML document.
fn set_nested_key(doc: &mut DocumentMut, key: &str, value: &str, param_type: ParamType) {
    let parts: Vec<&str> = key.split('.').collect();

    let toml_value = match param_type {
        ParamType::Int => toml_edit::value(value.parse::<i64>().unwrap_or(0)),
        ParamType::Bool => toml_edit::value(value.parse::<bool>().unwrap_or(false)),
        ParamType::Float => toml_edit::value(value.parse::<f64>().unwrap_or(0.0)),
        ParamType::String => toml_edit::value(value),
    };

    if parts.len() == 1 {
        // Simple key
        doc[key] = toml_value;
    } else {
        // Nested key - need to create intermediate tables
        let table_parts = &parts[..parts.len() - 1];
        let final_key = parts[parts.len() - 1];

        // Navigate/create intermediate tables
        let mut current = doc.as_table_mut();

        for &part in table_parts {
            if !current.contains_key(part) {
                current.insert(part, Item::Table(toml_edit::Table::new()));
            }
            current = current[part].as_table_mut().unwrap();
        }

        // Set the final value
        current[final_key] = toml_value;
    }
}

/// Validate a value against its parameter definition.
fn validate_value(value: &str, def: &ConfigDefinition) -> Result<(), String> {
    match def.param_type {
        ParamType::Int => {
            let v: i64 = value
                .parse()
                .map_err(|_| format!("{} must be an integer", def.key))?;

            if let Some(min) = def.min {
                let min_v: i64 = min.parse().unwrap();
                if v < min_v {
                    return Err(format!("{} minimum is {}", def.key, min));
                }
            }
            if let Some(max) = def.max {
                let max_v: i64 = max.parse().unwrap();
                if v > max_v {
                    return Err(format!("{} maximum is {}", def.key, max));
                }
            }
        }
        ParamType::Bool => {
            if value != "true" && value != "false" {
                return Err(format!("{} must be true or false", def.key));
            }
        }
        ParamType::Float => {
            value
                .parse::<f64>()
                .map_err(|_| format!("{} must be a number", def.key))?;
        }
        ParamType::String => {
            // Strings are always valid
        }
    }

    Ok(())
}

/// Check if version1 is newer than version2.
fn is_newer_version(version1: &str, version2: &str) -> bool {
    let v1: Vec<u32> = version1.split('.').filter_map(|s| s.parse().ok()).collect();
    let v2: Vec<u32> = version2.split('.').filter_map(|s| s.parse().ok()).collect();

    for i in 0..std::cmp::max(v1.len(), v2.len()) {
        let a = v1.get(i).copied().unwrap_or(0);
        let b = v2.get(i).copied().unwrap_or(0);
        if a > b {
            return true;
        }
        if a < b {
            return false;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_int() {
        let def = ConfigDefinition {
            key: "test",
            default: "10",
            param_type: ParamType::Int,
            description: "Test",
            min: Some("1"),
            max: Some("100"),
            added_in_version: "0.1.0",
        };

        assert!(validate_value("50", &def).is_ok());
        assert!(validate_value("0", &def).is_err());
        assert!(validate_value("101", &def).is_err());
        assert!(validate_value("abc", &def).is_err());
    }

    #[test]
    fn test_validate_bool() {
        let def = ConfigDefinition {
            key: "test",
            default: "true",
            param_type: ParamType::Bool,
            description: "Test",
            min: None,
            max: None,
            added_in_version: "0.1.0",
        };

        assert!(validate_value("true", &def).is_ok());
        assert!(validate_value("false", &def).is_ok());
        assert!(validate_value("yes", &def).is_err());
    }

    #[test]
    fn test_is_newer_version() {
        assert!(is_newer_version("0.2.0", "0.1.12"));
        assert!(is_newer_version("1.0.0", "0.9.9"));
        assert!(!is_newer_version("0.1.0", "0.1.12"));
        assert!(!is_newer_version("0.1.12", "0.1.12"));
    }

    #[test]
    fn test_get_config_schema() {
        let schema = get_config_schema();
        assert!(!schema.parameters.is_empty());
        assert!(!schema.agent_version.is_empty());
    }
}
