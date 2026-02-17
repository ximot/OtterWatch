//! Bootstrap module for auto-discovery of MQTT broker configuration
//!
//! This module allows agents to fetch their assigned MQTT broker from a central server
//! based on their agent_group. This enables automatic broker discovery and load balancing.

use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Request sent to the bootstrap endpoint
#[derive(Debug, Serialize)]
pub struct BootstrapRequest {
    /// Agent's group for broker assignment
    pub agent_group: Option<String>,
    /// API key for authentication
    pub api_key: String,
}

/// Response from the bootstrap endpoint
#[derive(Debug, Deserialize)]
pub struct BootstrapResponse {
    /// Whether the request was successful
    pub success: bool,
    /// Bootstrap data (present if success is true)
    pub data: Option<BootstrapData>,
    /// Error message (present if success is false)
    pub error: Option<String>,
}

/// Bootstrap data containing broker configuration
#[derive(Debug, Deserialize)]
pub struct BootstrapData {
    /// Primary broker address for this agent's group
    pub primary_broker: String,
    /// All available brokers for failover
    pub all_brokers: Vec<String>,
}

/// Fetches broker configuration from the bootstrap server
///
/// # Arguments
/// * `bootstrap_url` - URL of the bootstrap endpoint (e.g., "http://server:8080/api/cluster/bootstrap")
/// * `api_key` - API key for authentication
/// * `agent_group` - Agent's group for broker assignment
/// * `timeout_secs` - Request timeout in seconds
///
/// # Returns
/// * `Ok(Vec<String>)` - List of broker addresses (primary first, then others for failover)
/// * `Err(String)` - Error message if bootstrap failed
pub async fn fetch_broker_config(
    bootstrap_url: &str,
    api_key: &str,
    agent_group: &str,
    timeout_secs: u64,
) -> Result<Vec<String>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let request = BootstrapRequest {
        agent_group: if agent_group.is_empty() {
            None
        } else {
            Some(agent_group.to_string())
        },
        api_key: api_key.to_string(),
    };

    info!("Fetching broker configuration from {}", bootstrap_url);

    let response = client
        .post(bootstrap_url)
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("Bootstrap request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Bootstrap server returned HTTP {}",
            response.status()
        ));
    }

    let bootstrap_response: BootstrapResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse bootstrap response: {}", e))?;

    if !bootstrap_response.success {
        return Err(bootstrap_response
            .error
            .unwrap_or_else(|| "Unknown error".to_string()));
    }

    let data = bootstrap_response
        .data
        .ok_or_else(|| "Bootstrap response missing data".to_string())?;

    info!(
        "Bootstrap: primary broker '{}' for group '{}'",
        data.primary_broker,
        if agent_group.is_empty() {
            "default"
        } else {
            agent_group
        }
    );

    // Build broker list: primary first, then others for failover
    let mut broker_addrs = vec![data.primary_broker.clone()];
    for broker in data.all_brokers {
        if !broker_addrs.contains(&broker) {
            broker_addrs.push(broker);
        }
    }

    if broker_addrs.len() > 1 {
        info!(
            "Bootstrap: {} brokers available for failover",
            broker_addrs.len()
        );
    }

    Ok(broker_addrs)
}

/// Attempts to fetch broker config with fallback to configured brokers
///
/// # Arguments
/// * `bootstrap_url` - URL of the bootstrap endpoint
/// * `api_key` - API key for authentication
/// * `agent_group` - Agent's group for broker assignment
/// * `timeout_secs` - Request timeout in seconds
/// * `fallback_broker_addr` - Single broker address to use if bootstrap fails
/// * `fallback_broker_addrs` - List of broker addresses to use if bootstrap fails
///
/// # Returns
/// List of broker addresses (either from bootstrap or fallback)
pub async fn fetch_or_fallback(
    bootstrap_url: &str,
    api_key: &str,
    agent_group: &str,
    timeout_secs: u64,
    fallback_broker_addr: &str,
    fallback_broker_addrs: &[String],
) -> Vec<String> {
    match fetch_broker_config(bootstrap_url, api_key, agent_group, timeout_secs).await {
        Ok(addrs) => addrs,
        Err(e) => {
            warn!("Bootstrap failed: {}. Using fallback brokers.", e);
            if fallback_broker_addrs.is_empty() {
                vec![fallback_broker_addr.to_string()]
            } else {
                fallback_broker_addrs.to_vec()
            }
        }
    }
}
