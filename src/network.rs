use serde::{Deserialize, Serialize};
use std::fs;

pub fn print_network_io_stats(exclude_interfaces: Vec<String>) {
    let content = match fs::read_to_string("/proc/net/dev") {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Nie można odczytać /proc/net/dev: {}", e);
            return;
        }
    };

    println!("Statystyki sieciowe:");
    for line in content.lines().skip(2) {
        // Pomijamy pierwsze dwie linie nagłówków
        let parts: Vec<&str> = line.split_whitespace().collect();
        let interface = parts[0].trim_end_matches(':'); // Nazwa interfejsu

        if !exclude_interfaces.contains(&interface.to_string()) {
            let bytes_received = parts[1]; // Bajty otrzymane
            let bytes_transmitted = parts[9]; // Bajty wysłane

            println!(
                "Interfejs: {}, Odebrane bajty: {}, Wysłane bajty: {}",
                interface, bytes_received, bytes_transmitted
            );
        }
    }
}

pub fn get_network_io_stats(exclude_interfaces: Vec<String>) -> Vec<NetworkInterface> {
    let mut network_list = Vec::new();
    let content = match fs::read_to_string("/proc/net/dev") {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Nie można odczytać /proc/net/dev: {}", e);
            return network_list;
        }
    };
    for line in content.lines().skip(2) {
        // Pomijamy pierwsze dwie linie nagłówków
        let parts: Vec<&str> = line.split_whitespace().collect();
        let interface = parts[0].trim_end_matches(':'); // Nazwa interfejsu

        if !exclude_interfaces.contains(&interface.to_string()) {
            let bytes_received = parts[1]; // Bajty otrzymane
            let bytes_transmitted = parts[9]; // Bajty wysłane

            network_list.push(NetworkInterface {
                interface_name: interface.to_string(),
                bytes_received: bytes_received.to_string(),
                bytes_transmitted: bytes_transmitted.to_string(),
            })
        }
    }

    network_list
}

#[derive(Serialize, Deserialize)]
pub struct NetworkInterface {
    pub interface_name: String,
    pub bytes_received: String,
    pub bytes_transmitted: String,
}
