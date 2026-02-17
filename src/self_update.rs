//! Self-update module for OtterWatch agent
//!
//! Downloads new binary from server, verifies SHA256 checksum,
//! backs up current binary, and prepares for restart.

use log::{error, info, warn};
use sha2::{Digest, Sha256};
use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Exit code for update - wrapper script should replace binary and restart
pub const EXIT_CODE_UPDATE: i32 = 43;

/// Exit code for restart without update
#[allow(dead_code)] // Used by wrapper script
pub const EXIT_CODE_RESTART: i32 = 42;

/// Result of update download
#[derive(Debug)]
pub struct UpdateResult {
    pub success: bool,
    pub message: String,
    #[allow(dead_code)] // Info for wrapper script
    pub new_binary_path: Option<PathBuf>,
}

/// Downloads and verifies an update binary
pub async fn download_update(url: &str, expected_checksum: &str) -> UpdateResult {
    info!("Starting update download from: {}", url);

    // Get current executable path
    let current_exe = match env::current_exe() {
        Ok(path) => path,
        Err(e) => {
            return UpdateResult {
                success: false,
                message: format!("Failed to get current executable path: {}", e),
                new_binary_path: None,
            };
        }
    };

    // Determine paths
    let exe_dir = current_exe.parent().unwrap_or(Path::new("/tmp"));
    let backup_path = exe_dir.join("otterwatch.backup");
    let new_binary_path = exe_dir.join("otterwatch.new");

    // Download binary
    info!("Downloading update binary...");
    let binary_data = match download_binary(url).await {
        Ok(data) => data,
        Err(e) => {
            return UpdateResult {
                success: false,
                message: format!("Failed to download binary: {}", e),
                new_binary_path: None,
            };
        }
    };

    info!("Downloaded {} bytes", binary_data.len());

    // Verify checksum
    let actual_checksum = calculate_sha256(&binary_data);
    if !expected_checksum.is_empty() && actual_checksum != expected_checksum.to_lowercase() {
        error!(
            "Checksum mismatch! Expected: {}, Got: {}",
            expected_checksum, actual_checksum
        );
        return UpdateResult {
            success: false,
            message: format!(
                "Checksum verification failed. Expected: {}, Got: {}",
                expected_checksum, actual_checksum
            ),
            new_binary_path: None,
        };
    }

    info!("Checksum verified: {}", actual_checksum);

    // Backup current binary
    info!("Creating backup of current binary...");
    if let Err(e) = fs::copy(&current_exe, &backup_path) {
        warn!("Failed to create backup (continuing anyway): {}", e);
    } else {
        info!("Backup created at: {:?}", backup_path);
    }

    // Write new binary to temp location
    info!("Writing new binary to: {:?}", new_binary_path);
    if let Err(e) = write_binary(&new_binary_path, &binary_data) {
        return UpdateResult {
            success: false,
            message: format!("Failed to write new binary: {}", e),
            new_binary_path: None,
        };
    }

    // Set executable permissions
    if let Err(e) = fs::set_permissions(&new_binary_path, fs::Permissions::from_mode(0o755)) {
        warn!("Failed to set executable permissions: {}", e);
    }

    info!("Update download complete. Ready for restart.");

    UpdateResult {
        success: true,
        message: format!(
            "Update downloaded and verified. New binary at: {:?}",
            new_binary_path
        ),
        new_binary_path: Some(new_binary_path),
    }
}

/// Downloads binary data from URL
async fn download_binary(url: &str) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Server returned error status: {}",
            response.status()
        ));
    }

    // Get checksum from header if available
    if let Some(checksum) = response.headers().get("x-checksum-sha256") {
        if let Ok(checksum_str) = checksum.to_str() {
            info!("Server provided checksum: {}", checksum_str);
        }
    }

    response
        .bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| format!("Failed to read response body: {}", e))
}

/// Calculates SHA256 hash of data
fn calculate_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Writes binary data to file
fn write_binary(path: &Path, data: &[u8]) -> Result<(), String> {
    let mut file = File::create(path).map_err(|e| format!("Failed to create file: {}", e))?;

    file.write_all(data)
        .map_err(|e| format!("Failed to write data: {}", e))?;

    file.sync_all()
        .map_err(|e| format!("Failed to sync file: {}", e))?;

    Ok(())
}

/// Verifies the current binary matches expected checksum (for rollback verification)
#[allow(dead_code)] // Public API for wrapper script
pub fn verify_current_binary(expected_checksum: &str) -> bool {
    let current_exe = match env::current_exe() {
        Ok(path) => path,
        Err(_) => return false,
    };

    let mut file = match File::open(&current_exe) {
        Ok(f) => f,
        Err(_) => return false,
    };

    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let bytes_read = match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => return false,
        };
        hasher.update(&buffer[..bytes_read]);
    }

    let actual = format!("{:x}", hasher.finalize());
    actual == expected_checksum.to_lowercase()
}

/// Gets the path to the backup binary
#[allow(dead_code)] // Public API for wrapper script
pub fn get_backup_path() -> Option<PathBuf> {
    let current_exe = env::current_exe().ok()?;
    let exe_dir = current_exe.parent()?;
    let backup_path = exe_dir.join("otterwatch.backup");

    if backup_path.exists() {
        Some(backup_path)
    } else {
        None
    }
}

/// Gets the path to the new binary (for wrapper script)
#[allow(dead_code)] // Public API for wrapper script
pub fn get_new_binary_path() -> Option<PathBuf> {
    let current_exe = env::current_exe().ok()?;
    let exe_dir = current_exe.parent()?;
    let new_path = exe_dir.join("otterwatch.new");

    if new_path.exists() {
        Some(new_path)
    } else {
        None
    }
}
