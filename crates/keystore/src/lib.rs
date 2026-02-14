//! Craftec Keystore
//!
//! File-based key persistence with platform-aware paths.
//! Configurable service name for per-craft keystore directories.

use std::fs;
use std::path::{Path, PathBuf};

use craftec_crypto::SigningKeypair;
use thiserror::Error;
use tracing::{debug, info};

#[derive(Error, Debug)]
pub enum KeystoreError {
    #[error("Failed to read key file: {0}")]
    ReadError(String),
    #[error("Failed to write key file: {0}")]
    WriteError(String),
    #[error("Invalid key format")]
    InvalidFormat,
    #[error("Failed to create directory: {0}")]
    CreateDirError(String),
}

pub type Result<T> = std::result::Result<T, KeystoreError>;

/// Load or generate a signing keypair from a file path.
///
/// If the file exists, loads the 32-byte secret key.
/// If not, generates a new keypair and saves it.
pub fn load_or_generate_keypair(path: &Path) -> Result<SigningKeypair> {
    if path.exists() {
        debug!("Loading keypair from {}", path.display());
        let bytes = fs::read(path).map_err(|e| KeystoreError::ReadError(e.to_string()))?;
        if bytes.len() != 32 {
            return Err(KeystoreError::InvalidFormat);
        }
        let mut secret = [0u8; 32];
        secret.copy_from_slice(&bytes);
        Ok(SigningKeypair::from_secret_bytes(&secret))
    } else {
        info!("Generating new keypair at {}", path.display());
        let keypair = SigningKeypair::generate();
        save_keypair_bytes(path, &keypair.secret_key_bytes())?;
        Ok(keypair)
    }
}

/// Save raw keypair bytes to a file, creating parent directories as needed.
pub fn save_keypair_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| KeystoreError::CreateDirError(e.to_string()))?;
    }
    fs::write(path, bytes).map_err(|e| KeystoreError::WriteError(e.to_string()))
}

/// Get the default keystore directory for a given service name.
///
/// - macOS: `~/Library/Application Support/{ServiceName}/keys`
/// - Linux: `~/.local/share/{service_name}/keys`
/// - Windows: `%APPDATA%\{ServiceName}\keys`
pub fn default_keystore_dir_for(service: &str) -> PathBuf {
    let base = data_dir(service);
    base.join("keys")
}

/// Get the default config directory for a given service name.
///
/// - macOS: `~/Library/Application Support/{ServiceName}`
/// - Linux: `~/.config/{service_name}`
/// - Windows: `%APPDATA%\{ServiceName}`
pub fn default_config_dir_for(service: &str) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home_dir().join("Library").join("Application Support").join(capitalize(service))
    }
    #[cfg(target_os = "linux")]
    {
        let xdg = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home_dir().join(".config"));
        xdg.join(service.to_lowercase())
    }
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home_dir().join("AppData").join("Roaming"));
        appdata.join(capitalize(service))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        home_dir().join(format!(".{}", service.to_lowercase()))
    }
}

/// Get the default data directory for a given service name.
pub fn data_dir(service: &str) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home_dir().join("Library").join("Application Support").join(capitalize(service))
    }
    #[cfg(target_os = "linux")]
    {
        let xdg = std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home_dir().join(".local").join("share"));
        xdg.join(service.to_lowercase())
    }
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home_dir().join("AppData").join("Roaming"));
        appdata.join(capitalize(service))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        home_dir().join(format!(".{}", service.to_lowercase()))
    }
}

/// Get the default key file path for a service.
pub fn default_key_path_for(service: &str) -> PathBuf {
    default_keystore_dir_for(service).join("signing.key")
}

/// Expand `~` in paths to the home directory.
pub fn expand_path(path: &str) -> PathBuf {
    if path.starts_with('~') {
        let home = home_dir();
        home.join(&path[2..])
    } else {
        PathBuf::from(path)
    }
}

fn home_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::env::var("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("C:\\Users\\default"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp"))
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + &chars.as_str().to_lowercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_generate_and_load_keypair() {
        let dir = std::env::temp_dir().join("craftec-keystore-test");
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("test.key");

        // Generate
        let kp1 = load_or_generate_keypair(&path).unwrap();
        let pubkey1 = kp1.public_key_bytes();

        // Load
        let kp2 = load_or_generate_keypair(&path).unwrap();
        assert_eq!(kp2.public_key_bytes(), pubkey1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_invalid_key_format() {
        let dir = std::env::temp_dir().join("craftec-keystore-test-invalid");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.key");
        fs::write(&path, b"too short").unwrap();

        let result = load_or_generate_keypair(&path);
        assert!(matches!(result, Err(KeystoreError::InvalidFormat)));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_default_paths() {
        let keystore_dir = default_keystore_dir_for("datacraft");
        assert!(keystore_dir.to_string_lossy().contains("keys"));

        let config_dir = default_config_dir_for("datacraft");
        assert!(!config_dir.to_string_lossy().is_empty());

        let key_path = default_key_path_for("datacraft");
        assert!(key_path.to_string_lossy().contains("signing.key"));
    }

    #[test]
    fn test_expand_path() {
        let expanded = expand_path("~/test");
        assert!(!expanded.to_string_lossy().starts_with('~'));
    }
}
