//! Craftec App
//!
//! Unified initialization for all Craftec services: logging + keystore + settings.

use std::path::Path;

use craftec_crypto::SigningKeypair;
use craftec_logging::LogLevel;
use craftec_settings::{Settings, SettingsError};
use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;
use tracing::info;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Initialization failed: {0}")]
    InitError(String),
    #[error("Settings error: {0}")]
    SettingsError(#[from] SettingsError),
    #[error("Keystore error: {0}")]
    KeystoreError(#[from] craftec_keystore::KeystoreError),
}

/// Application type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppType {
    Cli,
    Desktop,
    Mobile,
    Daemon,
    Node,
}

impl AppType {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Cli => "CLI",
            Self::Desktop => "Desktop",
            Self::Mobile => "Mobile",
            Self::Daemon => "Daemon",
            Self::Node => "Node",
        }
    }
}

/// Initialized application context
pub struct App<T> {
    pub service: String,
    pub app_type: AppType,
    pub keypair: SigningKeypair,
    pub settings: Settings<T>,
}

/// Builder for constructing an App with configurable options.
pub struct AppBuilder<T> {
    service: String,
    app_type: AppType,
    log_level: LogLevel,
    skip_logging: bool,
    skip_banner: bool,
    config_path: Option<String>,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Serialize + DeserializeOwned + Default> AppBuilder<T> {
    pub fn new(service: &str) -> Self {
        Self {
            service: service.to_string(),
            app_type: AppType::Cli,
            log_level: LogLevel::Info,
            skip_logging: false,
            skip_banner: false,
            config_path: None,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn app_type(mut self, app_type: AppType) -> Self {
        self.app_type = app_type;
        self
    }

    pub fn log_level(mut self, level: LogLevel) -> Self {
        self.log_level = level;
        self
    }

    pub fn verbose(mut self, verbose: bool) -> Self {
        self.log_level = LogLevel::from_verbose(verbose);
        self
    }

    pub fn skip_logging(mut self) -> Self {
        self.skip_logging = true;
        self
    }

    pub fn skip_banner(mut self) -> Self {
        self.skip_banner = true;
        self
    }

    pub fn config_path(mut self, path: &str) -> Self {
        self.config_path = Some(path.to_string());
        self
    }

    pub fn build(self) -> Result<App<T>, AppError> {
        // Initialize logging
        if !self.skip_logging {
            let _ = craftec_logging::try_init(self.log_level);
        }

        // Load or generate keypair
        let key_path = craftec_keystore::default_key_path_for(&self.service);
        let keypair = craftec_keystore::load_or_generate_keypair(&key_path)?;

        // Load or create settings
        let config_path = self.config_path.as_deref().map(Path::new);
        let settings = Settings::load_or_default(&self.service, config_path)?;

        if !self.skip_banner {
            info!(
                "{} {} ({}) starting — pubkey: {}",
                self.service,
                env!("CARGO_PKG_VERSION"),
                self.app_type.name(),
                hex::encode(keypair.public_key_bytes()),
            );
        }

        Ok(App {
            service: self.service,
            app_type: self.app_type,
            keypair,
            settings,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, Default)]
    struct TestConfig {
        value: u32,
    }

    #[test]
    fn test_app_type_name() {
        assert_eq!(AppType::Cli.name(), "CLI");
        assert_eq!(AppType::Desktop.name(), "Desktop");
        assert_eq!(AppType::Daemon.name(), "Daemon");
    }

    #[test]
    fn test_app_builder() {
        let dir = std::env::temp_dir().join("craftec-app-test");
        let _ = std::fs::remove_dir_all(&dir);
        let config_path = dir.join("settings.json");

        let app: App<TestConfig> = AppBuilder::new("craftec-app-test")
            .app_type(AppType::Daemon)
            .skip_logging()
            .skip_banner()
            .config_path(config_path.to_str().unwrap())
            .build()
            .unwrap();

        assert_eq!(app.service, "craftec-app-test");
        assert_eq!(app.app_type, AppType::Daemon);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
