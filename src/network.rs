use log::warn;
use serde::{Deserialize, Serialize};
use std::fs;

pub fn get_network_io_stats(exclude_interfaces: &[String]) -> Vec<NetworkInterface> {
    let mut network_list = Vec::new();
    let content = match fs::read_to_string("/proc/net/dev") {
        Ok(c) => c,
        Err(err) => {
            warn!("Unable to read /proc/net/dev: {}", err);
            return network_list;
        }
    };
    for line in content.lines().skip(2) {
        // We skip the first two header lines
        let parts: Vec<&str> = line.split_whitespace().collect();
        let Some(interface_raw) = parts.first() else {
            continue;
        };
        let interface = interface_raw.trim_end_matches(':');

        if exclude_interfaces.iter().any(|iface| iface == interface) {
            continue;
        }

        let bytes_received = parts
            .get(1)
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let bytes_transmitted = parts
            .get(9)
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);

        network_list.push(NetworkInterface {
            interface_name: interface.to_string(),
            bytes_received,
            bytes_transmitted,
        });
    }
    network_list
}

#[derive(Clone, Serialize, Deserialize)]
pub struct NetworkInterface {
    pub interface_name: String,
    pub bytes_received: u64,
    pub bytes_transmitted: u64,
}
