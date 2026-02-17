//! Cgroup v2/v1 detection and metric reading utilities.
//!
//! Provides functions for:
//! - Detecting cgroup version (v2, v1, or none)
//! - Finding cgroup paths for systemd services
//! - Reading CPU, memory, and I/O statistics from cgroup files

use super::CgroupVersion;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Base paths for cgroup filesystems
const CGROUP_V2_BASE: &str = "/sys/fs/cgroup";
const CGROUP_V1_BASE: &str = "/sys/fs/cgroup";

/// Detect the cgroup version available on the system
pub fn detect_cgroup_version() -> CgroupVersion {
    // Check for cgroup v2 unified hierarchy
    if Path::new("/sys/fs/cgroup/cgroup.controllers").exists() {
        return CgroupVersion::V2;
    }

    // Check for cgroup v1 (legacy)
    if Path::new("/sys/fs/cgroup/cpu/cpu.stat").exists() || Path::new("/sys/fs/cgroup/cpu").is_dir()
    {
        return CgroupVersion::V1;
    }

    CgroupVersion::None
}

/// CPU statistics from cgroup
#[derive(Debug, Default, Clone)]
pub struct CgroupCpuStats {
    pub usage_usec: u64,
    pub user_usec: u64,
    pub system_usec: u64,
    pub nr_periods: u64,
    pub nr_throttled: u64,
    pub throttled_usec: u64,
}

/// Memory statistics from cgroup
#[derive(Debug, Default, Clone)]
pub struct CgroupMemoryStats {
    pub current_bytes: u64,
    pub swap_bytes: u64,
    pub anon_bytes: u64,
    pub file_bytes: u64,
    pub kernel_bytes: u64,
    pub shmem_bytes: u64,
}

/// I/O statistics from cgroup (aggregated across all devices)
#[derive(Debug, Default, Clone)]
pub struct CgroupIoStats {
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub read_ios: u64,
    pub write_ios: u64,
}

/// PIDs/process count from cgroup
#[derive(Debug, Default, Clone)]
pub struct CgroupPidsStats {
    pub current: u32,
    pub max: Option<u64>,
}

/// All cgroup stats combined
#[derive(Debug, Default, Clone)]
pub struct CgroupStats {
    pub cpu: CgroupCpuStats,
    pub memory: CgroupMemoryStats,
    pub io: CgroupIoStats,
    pub pids: CgroupPidsStats,
}

/// Get the cgroup path for a systemd service
pub fn get_service_cgroup_path(service_name: &str, version: CgroupVersion) -> Option<PathBuf> {
    let service_dir = if service_name.ends_with(".service") {
        service_name.to_string()
    } else {
        format!("{}.service", service_name)
    };

    match version {
        CgroupVersion::V2 => {
            let path = PathBuf::from(CGROUP_V2_BASE)
                .join("system.slice")
                .join(&service_dir);
            if path.exists() {
                return Some(path);
            }

            // Also check user.slice for user services
            let user_path = PathBuf::from(CGROUP_V2_BASE)
                .join("user.slice")
                .join(&service_dir);
            if user_path.exists() {
                return Some(user_path);
            }

            None
        }
        CgroupVersion::V1 => {
            // For cgroup v1, we need to check multiple controllers
            // CPU controller path is typically the main one
            let path = PathBuf::from(CGROUP_V1_BASE)
                .join("cpu")
                .join("system.slice")
                .join(&service_dir);
            if path.exists() {
                Some(path)
            } else {
                None
            }
        }
        CgroupVersion::None => None,
    }
}

/// Check if a cgroup path exists and is accessible
pub fn cgroup_path_exists(service_name: &str) -> Option<(PathBuf, CgroupVersion)> {
    let version = detect_cgroup_version();
    if let Some(path) = get_service_cgroup_path(service_name, version) {
        return Some((path, version));
    }
    None
}

/// Read all cgroup stats for a service
pub fn read_cgroup_stats(cgroup_path: &Path, version: CgroupVersion) -> io::Result<CgroupStats> {
    let mut stats = CgroupStats::default();

    match version {
        CgroupVersion::V2 => {
            stats.cpu = read_cpu_stat_v2(cgroup_path)?;
            stats.memory = read_memory_stats_v2(cgroup_path)?;
            stats.io = read_io_stats_v2(cgroup_path)?;
            stats.pids = read_pids_stats_v2(cgroup_path)?;
        }
        CgroupVersion::V1 => {
            stats.cpu = read_cpu_stat_v1(cgroup_path)?;
            stats.memory = read_memory_stats_v1(cgroup_path)?;
            // I/O stats in v1 are in a different controller path
            stats.io = read_io_stats_v1(cgroup_path)?;
            stats.pids = read_pids_stats_v1(cgroup_path)?;
        }
        CgroupVersion::None => {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "No cgroup support detected",
            ));
        }
    }

    Ok(stats)
}

// ============================================
// Cgroup v2 reading functions
// ============================================

/// Read CPU stats from cgroup v2
fn read_cpu_stat_v2(cgroup_path: &Path) -> io::Result<CgroupCpuStats> {
    let content = fs::read_to_string(cgroup_path.join("cpu.stat"))?;
    let mut stats = CgroupCpuStats::default();

    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let value: u64 = parts[1].parse().unwrap_or(0);
            match parts[0] {
                "usage_usec" => stats.usage_usec = value,
                "user_usec" => stats.user_usec = value,
                "system_usec" => stats.system_usec = value,
                "nr_periods" => stats.nr_periods = value,
                "nr_throttled" => stats.nr_throttled = value,
                "throttled_usec" => stats.throttled_usec = value,
                _ => {}
            }
        }
    }

    Ok(stats)
}

/// Read memory stats from cgroup v2
fn read_memory_stats_v2(cgroup_path: &Path) -> io::Result<CgroupMemoryStats> {
    let mut stats = CgroupMemoryStats::default();

    // Read current memory usage
    if let Ok(content) = fs::read_to_string(cgroup_path.join("memory.current")) {
        stats.current_bytes = content.trim().parse().unwrap_or(0);
    }

    // Read swap usage
    if let Ok(content) = fs::read_to_string(cgroup_path.join("memory.swap.current")) {
        stats.swap_bytes = content.trim().parse().unwrap_or(0);
    }

    // Read detailed memory stats
    if let Ok(content) = fs::read_to_string(cgroup_path.join("memory.stat")) {
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let value: u64 = parts[1].parse().unwrap_or(0);
                match parts[0] {
                    "anon" => stats.anon_bytes = value,
                    "file" => stats.file_bytes = value,
                    "kernel" => stats.kernel_bytes = value,
                    "shmem" => stats.shmem_bytes = value,
                    _ => {}
                }
            }
        }
    }

    Ok(stats)
}

/// Read I/O stats from cgroup v2
fn read_io_stats_v2(cgroup_path: &Path) -> io::Result<CgroupIoStats> {
    let mut stats = CgroupIoStats::default();

    // io.stat format: "major:minor rbytes=X wbytes=Y rios=Z wios=W ..."
    if let Ok(content) = fs::read_to_string(cgroup_path.join("io.stat")) {
        for line in content.lines() {
            let pairs: HashMap<&str, u64> = line
                .split_whitespace()
                .skip(1) // Skip device major:minor
                .filter_map(|pair| {
                    let mut parts = pair.split('=');
                    match (parts.next(), parts.next()) {
                        (Some(key), Some(value)) => value.parse::<u64>().ok().map(|v| (key, v)),
                        _ => None,
                    }
                })
                .collect();

            stats.read_bytes += pairs.get("rbytes").copied().unwrap_or(0);
            stats.write_bytes += pairs.get("wbytes").copied().unwrap_or(0);
            stats.read_ios += pairs.get("rios").copied().unwrap_or(0);
            stats.write_ios += pairs.get("wios").copied().unwrap_or(0);
        }
    }

    Ok(stats)
}

/// Read PIDs stats from cgroup v2
fn read_pids_stats_v2(cgroup_path: &Path) -> io::Result<CgroupPidsStats> {
    let mut stats = CgroupPidsStats::default();

    if let Ok(content) = fs::read_to_string(cgroup_path.join("pids.current")) {
        stats.current = content.trim().parse().unwrap_or(0);
    }

    if let Ok(content) = fs::read_to_string(cgroup_path.join("pids.max")) {
        let trimmed = content.trim();
        if trimmed != "max" {
            stats.max = trimmed.parse().ok();
        }
    }

    Ok(stats)
}

// ============================================
// Cgroup v1 reading functions
// ============================================

/// Read CPU stats from cgroup v1
fn read_cpu_stat_v1(cgroup_path: &Path) -> io::Result<CgroupCpuStats> {
    let mut stats = CgroupCpuStats::default();

    // cpu.stat in v1 has different format
    if let Ok(content) = fs::read_to_string(cgroup_path.join("cpu.stat")) {
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let value: u64 = parts[1].parse().unwrap_or(0);
                match parts[0] {
                    "nr_periods" => stats.nr_periods = value,
                    "nr_throttled" => stats.nr_throttled = value,
                    "throttled_time" => stats.throttled_usec = value / 1000, // ns to us
                    _ => {}
                }
            }
        }
    }

    // cpuacct.usage gives total CPU time in nanoseconds
    let cpuacct_path = cgroup_path.parent().and_then(|p| p.parent()).map(|p| {
        p.join("cpuacct")
            .join("system.slice")
            .join(cgroup_path.file_name().unwrap_or_default())
    });

    if let Some(ref path) = cpuacct_path {
        if let Ok(content) = fs::read_to_string(path.join("cpuacct.usage")) {
            let ns: u64 = content.trim().parse().unwrap_or(0);
            stats.usage_usec = ns / 1000;
        }

        if let Ok(content) = fs::read_to_string(path.join("cpuacct.stat")) {
            for line in content.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let jiffies: u64 = parts[1].parse().unwrap_or(0);
                    // Convert jiffies to microseconds (assume 100 Hz)
                    let usec = jiffies * 10000;
                    match parts[0] {
                        "user" => stats.user_usec = usec,
                        "system" => stats.system_usec = usec,
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(stats)
}

/// Read memory stats from cgroup v1
fn read_memory_stats_v1(cgroup_path: &Path) -> io::Result<CgroupMemoryStats> {
    let mut stats = CgroupMemoryStats::default();

    // Memory controller might be in a different path
    let memory_path = cgroup_path.parent().and_then(|p| p.parent()).map(|p| {
        p.join("memory")
            .join("system.slice")
            .join(cgroup_path.file_name().unwrap_or_default())
    });

    let path: &Path = memory_path
        .as_ref()
        .filter(|p| p.exists())
        .map(|p| p.as_path())
        .unwrap_or(cgroup_path);

    if let Ok(content) = fs::read_to_string(path.join("memory.usage_in_bytes")) {
        stats.current_bytes = content.trim().parse().unwrap_or(0);
    }

    if let Ok(content) = fs::read_to_string(path.join("memory.memsw.usage_in_bytes")) {
        let memsw: u64 = content.trim().parse().unwrap_or(0);
        stats.swap_bytes = memsw.saturating_sub(stats.current_bytes);
    }

    if let Ok(content) = fs::read_to_string(path.join("memory.stat")) {
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let value: u64 = parts[1].parse().unwrap_or(0);
                match parts[0] {
                    "total_rss" | "rss" => stats.anon_bytes = value,
                    "total_cache" | "cache" => stats.file_bytes = value,
                    "total_shmem" | "shmem" => stats.shmem_bytes = value,
                    _ => {}
                }
            }
        }
    }

    Ok(stats)
}

/// Read I/O stats from cgroup v1
fn read_io_stats_v1(cgroup_path: &Path) -> io::Result<CgroupIoStats> {
    let mut stats = CgroupIoStats::default();

    // blkio controller
    let blkio_path = cgroup_path.parent().and_then(|p| p.parent()).map(|p| {
        p.join("blkio")
            .join("system.slice")
            .join(cgroup_path.file_name().unwrap_or_default())
    });

    let path: &Path = blkio_path
        .as_ref()
        .filter(|p| p.exists())
        .map(|p| p.as_path())
        .unwrap_or(cgroup_path);

    // Read bytes
    if let Ok(content) = fs::read_to_string(path.join("blkio.throttle.io_service_bytes")) {
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let value: u64 = parts[2].parse().unwrap_or(0);
                match parts[1] {
                    "Read" => stats.read_bytes += value,
                    "Write" => stats.write_bytes += value,
                    _ => {}
                }
            }
        }
    }

    // Read I/O operations
    if let Ok(content) = fs::read_to_string(path.join("blkio.throttle.io_serviced")) {
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let value: u64 = parts[2].parse().unwrap_or(0);
                match parts[1] {
                    "Read" => stats.read_ios += value,
                    "Write" => stats.write_ios += value,
                    _ => {}
                }
            }
        }
    }

    Ok(stats)
}

/// Read PIDs stats from cgroup v1
fn read_pids_stats_v1(cgroup_path: &Path) -> io::Result<CgroupPidsStats> {
    let mut stats = CgroupPidsStats::default();

    // Count processes from cgroup.procs
    if let Ok(content) = fs::read_to_string(cgroup_path.join("cgroup.procs")) {
        stats.current = content.lines().count() as u32;
    }

    Ok(stats)
}

/// Get list of PIDs in a cgroup
#[allow(dead_code)] // Public API for external use
pub fn get_cgroup_pids(cgroup_path: &Path) -> io::Result<Vec<u32>> {
    let content = fs::read_to_string(cgroup_path.join("cgroup.procs"))?;
    Ok(content
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect())
}

/// Get thread count from cgroup
pub fn get_cgroup_thread_count(cgroup_path: &Path) -> io::Result<u64> {
    let content = fs::read_to_string(cgroup_path.join("cgroup.threads"))?;
    Ok(content.lines().count() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_cgroup_version() {
        let version = detect_cgroup_version();
        // Should detect some version on a Linux system
        println!("Detected cgroup version: {:?}", version);
    }

    #[test]
    fn test_service_cgroup_path() {
        let version = detect_cgroup_version();
        if version != CgroupVersion::None {
            // Try to find NetworkManager or sshd (common services)
            let nm_path = get_service_cgroup_path("NetworkManager", version);
            let sshd_path = get_service_cgroup_path("sshd", version);
            println!("NetworkManager cgroup: {:?}", nm_path);
            println!("sshd cgroup: {:?}", sshd_path);
        }
    }

    #[test]
    fn test_cgroup_path_exists() {
        if let Some((path, version)) = cgroup_path_exists("NetworkManager") {
            println!("Found NetworkManager at {:?} (version {:?})", path, version);

            // Try to read stats
            if let Ok(stats) = read_cgroup_stats(&path, version) {
                println!("CPU usage: {} usec", stats.cpu.usage_usec);
                println!("Memory: {} bytes", stats.memory.current_bytes);
                println!("PIDs: {}", stats.pids.current);
            }
        }
    }
}
