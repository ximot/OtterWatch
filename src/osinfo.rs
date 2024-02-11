use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Serialize, Deserialize)]
struct OSInfo {
    hostname: String,
    os_name: String,
    kernel_version: String,
    start_time: String,
    cpu_name: String,
    cpu_cores: usize,
}

fn get_os_info() -> OSInfo {
    // Distribution name and version
    let os_release = fs::read_to_string("/etc/os-release").unwrap();
    let os_name = os_release
        .lines()
        .find(|line| line.starts_with("PRETTY_NAME"))
        .and_then(|line| line.split_once("="))
        .map(|(_, value)| value.trim_matches('"'))
        .unwrap_or("Unknown system");

    // Kernel version
    let kernel_version = fs::read_to_string("/proc/version")
        .unwrap()
        .split_whitespace()
        .nth(2)
        .unwrap_or("Unknown kernel version")
        .to_string();

    // System start-up time
    let uptime_seconds = fs::read_to_string("/proc/uptime")
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse::<f64>()
        .unwrap();
    let start_time = chrono::Utc::now() - chrono::Duration::seconds(uptime_seconds as i64);

    // Number of CPU cores
    let cpu_info = fs::read_to_string("/proc/cpuinfo").unwrap();
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
        .unwrap()
        .trim()
        .to_string();

    let mut osinfo: OSInfo = OSInfo {
        hostname: "".to_string(),
        os_name: "".to_string(),
        kernel_version: "".to_string(),
        start_time: "".to_string(),
        cpu_name: "".to_string(),
        cpu_cores,
    };

    osinfo.os_name = os_name.parse().unwrap();
    osinfo.kernel_version = kernel_version.parse().unwrap();
    osinfo.start_time = start_time.to_rfc3339();
    osinfo.cpu_name = cpu_name.parse().unwrap();
    osinfo.cpu_cores = cpu_cores;
    osinfo.hostname = hostname;

    return osinfo;
}

pub fn save_os_info_to_db(db_file_name: &String) {
    let conn = Connection::open(db_file_name).expect("DB connection failed!");

    let os_info = get_os_info();

    conn.execute(
        "INSERT INTO system (os_name, kernel_version, boot_time, cpu_name, cpu_cores, hostname) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![os_info.os_name, os_info.kernel_version, os_info.start_time, os_info.cpu_name, os_info.cpu_cores, os_info.hostname],
    ).expect("Failed to insert stats");

    conn.close().unwrap();
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

pub fn show_and_save_os_info_to_db(db_file_name: &String) {
    save_os_info_to_db(db_file_name);
    show_os_info();
}
