use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;

pub fn print_disk_io_stats(device: &str) {
    if let Ok(diskstats) = fs::read_to_string("/proc/diskstats") {
        for line in diskstats.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() > 3 && parts[2] == device {
                println!(
                    "Statystyki I/O dla urządzenia {}: Odczytów: {}, Zapisów: {}",
                    device, parts[5], parts[9]
                );
                break;
            }
        }
    } else {
        println!("Nie można odczytać /proc/diskstats");
    }
}

pub fn print_all_disk_io_stats() {
    let diskstats = match fs::read_to_string("/proc/diskstats") {
        Ok(content) => content,
        Err(e) => {
            println!("Nie można odczytać /proc/diskstats: {}", e);
            return;
        }
    };

    println!("Statystyki I/O dla wszystkich urządzeń:");
    for line in diskstats.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() > 13 {
            // Przykład zakłada format zgodny z typowymi wersjami jądra Linux
            let device = parts[2];
            let read_ops = parts[5];
            let write_ops = parts[9];
            let read_time_ms = parts[12]; // Czas spędzony na odczycie
            let write_time_ms = parts[14]; // Czas spędzony na zapisie

            println!("Urządzenie: {}, Odczytów: {}, Zapisów: {}, Czas odczytu: {} ms, Czas zapisu: {} ms",
                     device, read_ops, write_ops, read_time_ms, write_time_ms);
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct DiskInfo {
    pub devices: String,
    pub read_ops: String,
    pub write_ops: String,
    pub read_time_ms: String,
    pub write_time_ms: String,
}

pub fn get_physical_disk_io_stats() -> Vec<DiskInfo> {
    let mut disks_info_list = Vec::new();
    let diskstats = match fs::read_to_string("/proc/diskstats") {
        Ok(content) => content,
        Err(e) => {
            println!("Nie można odczytać /proc/diskstats: {}", e);
            return disks_info_list;
        }
    };

    for line in diskstats.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() > 13 {
            let device = parts[2];

            // Sprawdź, czy nazwa urządzenia pasuje do fizycznego dysku (ignoruj partycje)
            if Regex::new("^sd[a-z]$").unwrap().is_match(device)
                || Regex::new("^hd[a-z]$").unwrap().is_match(device)
                || Regex::new("^nvme[0-9]n[0-1]$").unwrap().is_match(device)
            {
                let read_ops = parts[5];
                let write_ops = parts[9];
                let read_time_ms = parts[12]; // Czas spędzony na odczycie
                let write_time_ms = parts[14]; // Czas spędzony na zapisie

                disks_info_list.push(DiskInfo {
                    devices: device.to_string(),
                    read_ops: read_ops.to_string(),
                    write_ops: write_ops.to_string(),
                    read_time_ms: read_time_ms.to_string(),
                    write_time_ms: write_time_ms.to_string(),
                });
            }
        }
    }
    return disks_info_list;
}

pub fn print_physical_disk_io_stats() {
    let diskstats = match fs::read_to_string("/proc/diskstats") {
        Ok(content) => content,
        Err(e) => {
            println!("Nie można odczytać /proc/diskstats: {}", e);
            return;
        }
    };

    println!("Statystyki I/O dla fizycznych dysków:");
    for line in diskstats.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() > 13 {
            let device = parts[2];

            // Sprawdź, czy nazwa urządzenia pasuje do fizycznego dysku (ignoruj partycje)
            if Regex::new("^sd[a-z]$").unwrap().is_match(device)
                || Regex::new("^hd[a-z]$").unwrap().is_match(device)
                || Regex::new("^nvme[0-9]n[0-1]$").unwrap().is_match(device)
            {
                let read_ops = parts[5];
                let write_ops = parts[9];
                let read_time_ms = parts[12]; // Czas spędzony na odczycie
                let write_time_ms = parts[14]; // Czas spędzony na zapisie

                println!("Urządzenie: {}, Odczytów: {}, Zapisów: {}, Czas odczytu: {} ms, Czas zapisu: {} ms",
                         device, read_ops, write_ops, read_time_ms, write_time_ms);
            }
        }
    }
}
