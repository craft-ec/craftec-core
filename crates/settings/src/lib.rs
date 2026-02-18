//! Craftec Settings
//!
//! Generic config file management for all Craftec services.
//! Each craft defines its own config type and uses `Settings<T>` to persist it.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;
use tracing::debug;

use craftec_keystore::default_config_dir_for;

#[derive(Error, Debug)]
pub enum SettingsError {
    #[error("Failed to read settings: {0}")]
    ReadError(String),
    #[error("Failed to write settings: {0}")]
    WriteError(String),
    #[error("Failed to parse settings: {0}")]
    ParseError(String),
    #[error("Failed to create directory: {0}")]
    CreateDirError(String),
}

pub type Result<T> = std::result::Result<T, SettingsError>;

/// Generic settings wrapper for any serializable config type.
///
/// Each craft defines its own config struct (e.g., `CraftOBJConfig`) and wraps it:
/// ```ignore
/// let settings: Settings<CraftOBJConfig> = Settings::load_or_default("craftobj", None)?;
/// ```
pub struct Settings<T> {
    pub config: T,
    path: PathBuf,
}

impl<T: Serialize + DeserializeOwned + Default> Settings<T> {
    /// Load settings from the default path for a service, or create defaults.
    pub fn load_or_default(service: &str, custom_path: Option<&Path>) -> Result<Self> {
        let path = match custom_path {
            Some(p) => p.to_path_buf(),
            None => default_settings_path(service),
        };

        if path.exists() {
            debug!("Loading settings from {}", path.display());
            let content = fs::read_to_string(&path)
                .map_err(|e| SettingsError::ReadError(e.to_string()))?;
            let config: T = serde_json::from_str(&content)
                .map_err(|e| SettingsError::ParseError(e.to_string()))?;
            Ok(Self { config, path })
        } else {
            debug!("Creating default settings at {}", path.display());
            let settings = Self {
                config: T::default(),
                path,
            };
            settings.save()?;
            Ok(settings)
        }
    }

    /// Save current settings to disk.
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| SettingsError::CreateDirError(e.to_string()))?;
        }
        let content = serde_json::to_string_pretty(&self.config)
            .map_err(|e| SettingsError::WriteError(e.to_string()))?;
        fs::write(&self.path, content)
            .map_err(|e| SettingsError::WriteError(e.to_string()))
    }

    /// Get the path where settings are stored.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Get the default settings file path for a service.
pub fn default_settings_path(service: &str) -> PathBuf {
    default_config_dir_for(service).join("settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, Default, PartialEq)]
    struct TestConfig {
        name: String,
        value: u32,
    }

    #[test]
    fn test_settings_load_or_default() {
        let dir = std::env::temp_dir().join("craftec-settings-test");
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("test-settings.json");

        // Create default
        let settings: Settings<TestConfig> =
            Settings::load_or_default("test", Some(&path)).unwrap();
        assert_eq!(settings.config, TestConfig::default());

        // Load existing
        let settings2: Settings<TestConfig> =
            Settings::load_or_default("test", Some(&path)).unwrap();
        assert_eq!(settings2.config, TestConfig::default());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_settings_save_and_load() {
        let dir = std::env::temp_dir().join("craftec-settings-test-save");
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("config.json");

        let mut settings: Settings<TestConfig> =
            Settings::load_or_default("test", Some(&path)).unwrap();
        settings.config.name = "modified".to_string();
        settings.config.value = 42;
        settings.save().unwrap();

        let loaded: Settings<TestConfig> =
            Settings::load_or_default("test", Some(&path)).unwrap();
        assert_eq!(loaded.config.name, "modified");
        assert_eq!(loaded.config.value, 42);

        let _ = fs::remove_dir_all(&dir);
    }
}
