//! Application configuration.
//!
//! Handles loading configuration from environment variables and .env files.

use dotenv::dotenv;
use std::env;

/// Configuration for the application.
#[derive(Clone, Default)]
pub struct Config {
    /// `Planning Center` Online application ID
    pub pco_app_id: String,
    /// `Planning Center` Online secret
    pub pco_secret: String,
}

/// Failure to read process configuration.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// A `.env` file exists but could not be read or parsed.
    #[error("failed to load .env ({source}); fix its syntax and permissions or remove the file")]
    DotEnv {
        /// Underlying dotenv failure.
        #[source]
        source: dotenv::Error,
    },
}

impl Config {
    /// Load configuration from environment variables
    pub fn load() -> Result<Self, ConfigError> {
        // A missing .env is normal; malformed or unreadable files are not.
        match dotenv() {
            Ok(_) => {}
            Err(dotenv::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => return Err(ConfigError::DotEnv { source }),
        }

        let mut config = Self::default();

        // Try to load Planning Center credentials from environment
        if let Ok(app_id) = env::var("PCO_APP_ID") {
            config.pco_app_id = app_id.trim().to_string();
        }

        if let Ok(secret) = env::var("PCO_SECRET") {
            config.pco_secret = secret.trim().to_string();
        }

        Ok(config)
    }
}
