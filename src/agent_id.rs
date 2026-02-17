use log::{info, warn};
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::Path;
use uuid::Uuid;

const AGENT_ID_FILENAME: &str = "agent_id";
const MACHINE_ID_PATH: &str = "/etc/machine-id";
const SYSTEM_AGENT_ID_PATH: &str = "/etc/otterwatch/agent_id";

/// Gets the existing agent ID or creates a new one if it doesn't exist.
///
/// The agent ID is determined in the following order:
/// 1. Check /etc/otterwatch/agent_id (system-wide, survives reinstalls)
/// 2. Check data_dir/agent_id (backwards compatibility)
/// 3. Generate deterministic ID from /etc/machine-id (Linux standard)
/// 4. Generate random UUID as last resort
///
/// The ID is then saved to the data directory for quick subsequent reads.
pub fn get_or_create_agent_id(data_dir: &Path) -> io::Result<String> {
    let local_id_file = data_dir.join(AGENT_ID_FILENAME);

    // 1. Try system-wide agent ID first (highest priority)
    if let Some(id) = try_read_agent_id(Path::new(SYSTEM_AGENT_ID_PATH)) {
        info!(
            "Using system agent ID from {}: {}",
            SYSTEM_AGENT_ID_PATH, id
        );
        // Also save to local file for consistency
        let _ = save_agent_id(&local_id_file, &id);
        return Ok(id);
    }

    // 2. Try local data directory (backwards compatibility)
    if let Some(id) = try_read_agent_id(&local_id_file) {
        info!(
            "Using existing agent ID from {}: {}",
            local_id_file.display(),
            id
        );
        return Ok(id);
    }

    // 3. Generate deterministic ID from machine-id
    let new_id = if let Some(machine_id) = read_machine_id() {
        generate_deterministic_uuid(&machine_id)
    } else {
        // 4. Fallback to random UUID
        warn!("No machine-id found, generating random agent ID");
        Uuid::new_v4().to_string()
    };

    // Save to local file
    save_agent_id(&local_id_file, &new_id)?;

    // Try to save to system-wide location (may fail without root)
    if let Err(e) = save_system_agent_id(&new_id) {
        info!("Could not save system agent ID (needs root): {}", e);
    }

    info!("Generated new agent ID: {}", new_id);
    Ok(new_id)
}

/// Try to read agent ID from a file, returning None if invalid or not found
fn try_read_agent_id(path: &Path) -> Option<String> {
    if !path.exists() {
        return None;
    }

    match fs::read_to_string(path) {
        Ok(content) => {
            let agent_id = content.trim().to_string();
            if Uuid::parse_str(&agent_id).is_ok() {
                Some(agent_id)
            } else {
                warn!("Invalid agent ID in {}", path.display());
                None
            }
        }
        Err(err) => {
            warn!("Failed to read agent ID from {}: {}", path.display(), err);
            None
        }
    }
}

/// Read /etc/machine-id (standard Linux machine identifier)
fn read_machine_id() -> Option<String> {
    fs::read_to_string(MACHINE_ID_PATH)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Generate a deterministic UUID v5 from machine-id
fn generate_deterministic_uuid(machine_id: &str) -> String {
    // Use SHA-256 to create a deterministic hash from machine-id + namespace
    let mut hasher = Sha256::new();
    hasher.update(b"otterwatch-agent-");
    hasher.update(machine_id.as_bytes());
    let hash = hasher.finalize();

    // Convert first 16 bytes to UUID format (version 5 style)
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash[..16]);

    // Set version (4 bits) and variant (2 bits) to make it a valid UUID
    bytes[6] = (bytes[6] & 0x0f) | 0x50; // Version 5
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // Variant RFC 4122

    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        u16::from_be_bytes([bytes[4], bytes[5]]),
        u16::from_be_bytes([bytes[6], bytes[7]]),
        u16::from_be_bytes([bytes[8], bytes[9]]),
        u64::from_be_bytes([
            0, 0, bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
        ])
    )
}

/// Save agent ID to a file with proper permissions
fn save_agent_id(path: &Path, id: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(path, id)?;

    #[cfg(unix)]
    {
        use std::fs::Permissions;
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, Permissions::from_mode(0o600))?;
    }

    Ok(())
}

/// Try to save agent ID to system-wide location
fn save_system_agent_id(id: &str) -> io::Result<()> {
    let path = Path::new(SYSTEM_AGENT_ID_PATH);
    save_agent_id(path, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_create_new_agent_id() {
        let temp_dir = tempdir().unwrap();
        let agent_id = get_or_create_agent_id(temp_dir.path()).unwrap();

        // Verify it's a valid UUID
        assert!(Uuid::parse_str(&agent_id).is_ok());

        // Verify file was created in temp dir
        let id_file = temp_dir.path().join(AGENT_ID_FILENAME);
        assert!(id_file.exists());

        // Verify content matches
        let stored_id = fs::read_to_string(&id_file).unwrap();
        assert_eq!(stored_id.trim(), agent_id);
    }

    #[test]
    fn test_reuse_existing_agent_id() {
        let temp_dir = tempdir().unwrap();

        // Create first agent ID
        let first_id = get_or_create_agent_id(temp_dir.path()).unwrap();

        // Get agent ID again - should be the same
        let second_id = get_or_create_agent_id(temp_dir.path()).unwrap();

        assert_eq!(first_id, second_id);
    }

    #[test]
    fn test_deterministic_uuid_generation() {
        // Same machine-id should always produce the same UUID
        let machine_id = "abc123def456";
        let uuid1 = generate_deterministic_uuid(machine_id);
        let uuid2 = generate_deterministic_uuid(machine_id);

        assert_eq!(uuid1, uuid2);
        assert!(Uuid::parse_str(&uuid1).is_ok());
    }

    #[test]
    fn test_different_machine_ids_produce_different_uuids() {
        let uuid1 = generate_deterministic_uuid("machine1");
        let uuid2 = generate_deterministic_uuid("machine2");

        assert_ne!(uuid1, uuid2);
    }
}
