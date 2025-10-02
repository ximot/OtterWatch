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
        let interface = parts[0].trim_end_matches(':'); // Nazwa interfejsu

        if exclude_interfaces.iter().any(|iface| iface == interface) {
            continue;
        }

        let bytes_received = parts.get(1).unwrap_or(&"0");
        let bytes_transmitted = parts.get(9).unwrap_or(&"0");

        network_list.push(NetworkInterface {
            interface_name: interface.to_string(),
            bytes_received: bytes_received.to_string(),
            bytes_transmitted: bytes_transmitted.to_string(),
        });
    }
    network_list
}

#[derive(Serialize, Deserialize)]
pub struct NetworkInterface {
    pub interface_name: String,
    pub bytes_received: String,
    pub bytes_transmitted: String,
}
