use log::warn;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone)]
pub struct OSInfo {
    pub hostname: String,
    pub os_name: String,
    pub kernel_version: String,
    pub start_time: String,
    pub cpu_name: String,
    pub cpu_cores: usize,
}

fn get_os_info() -> OSInfo {
    // Distribution name and version
    let os_release = fs::read_to_string("/etc/os-release").unwrap_or_else(|err| {
        warn!("Unable to read /etc/os-release: {}", err);
        String::new()
    });
    let os_name = os_release
        .lines()
        .find(|line| line.starts_with("PRETTY_NAME"))
        .and_then(|line| line.split_once("="))
        .map(|(_, value)| value.trim_matches('"'))
        .unwrap_or("Unknown system");

    // Kernel version
    let kernel_version = fs::read_to_string("/proc/version")
        .unwrap_or_else(|err| {
            warn!("Unable to read /proc/version: {}", err);
            String::new()
        })
        .split_whitespace()
        .nth(2)
        .unwrap_or("Unknown kernel version")
        .to_string();

    // System start-up time
    let uptime_seconds = fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|content| content.split_whitespace().next().map(str::to_owned))
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or_default();
    let start_time = chrono::Utc::now() - chrono::Duration::seconds(uptime_seconds as i64);

    // Number of CPU cores
    let cpu_info = fs::read_to_string("/proc/cpuinfo").unwrap_or_else(|err| {
        warn!("Unable to read /proc/cpuinfo: {}", err);
        String::new()
    });
    let cpu_cores = cpu_info
        .lines()
        .filter(|line| line.starts_with("processor"))
        .count();

    // Processor name
    let cpu_name = cpu_info
        .lines()
        .find(|line| line.starts_with("model name"))
        .and_then(|line| line.split_once(":"))
        .map(|(_, value)| value.trim())
        .unwrap_or("Nieznany procesor");

    // Hostname
    let hostname = fs::read_to_string("/etc/hostname")
        .unwrap_or_else(|err| {
            warn!("Unable to read /etc/hostname: {}", err);
            String::new()
        })
        .trim()
        .to_string();

    OSInfo {
        hostname,
        os_name: os_name.to_string(),
        kernel_version,
        start_time: start_time.to_rfc3339(),
        cpu_name: cpu_name.to_string(),
        cpu_cores,
    }
}

pub fn save_os_info_to_file(root: &Path) -> io::Result<()> {
    fs::create_dir_all(root)?;
    let os_info = get_os_info();
    let path = root.join("system_info.json");
    let mut file = File::create(&path)?;
    serde_json::to_writer_pretty(&mut file, &os_info)?;
    file.write_all(b"\n")?;
    Ok(())
}

pub fn show_os_info() {
    let os_info = get_os_info();
    println!("Hostname: {}", os_info.hostname);
    println!("System: {}", os_info.os_name);
    println!("Kernel version: {}", os_info.kernel_version);
    println!("System start-up time {}", os_info.start_time);
    println!("Number of CPU cores: {}", os_info.cpu_cores);
    println!("Processor name: {}", os_info.cpu_name);
}

pub fn get_os_info_api() -> serde_json::Result<String> {
    let os_info = get_os_info();
    serde_json::to_string(&os_info)
}

/// Returns OS info as a struct (used for MQTT agent registration)
pub fn get_os_info_struct() -> OSInfo {
    get_os_info()
}

pub fn show_and_save_os_info(root: &Path) -> io::Result<()> {
    let result = save_os_info_to_file(root);
    show_os_info();
    result
}
