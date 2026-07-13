//! Application configuration.
//!
//! Handles loading configuration from environment variables and .env files.

use crate::error::{Error, Result};
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

impl Config {
    /// Load configuration from environment variables
    pub fn load() -> Result<Self> {
        // A missing .env is normal; malformed or unreadable files are not.
        match dotenv() {
            Ok(_) => {}
            Err(dotenv::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(Error::config(
                    format!("failed to load .env: {error}"),
                    "Fix the .env syntax/permissions or remove the file",
                ));
            }
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

    /// Check if `Planning Center` is configured
    pub const fn has_planning_center_credentials(&self) -> bool {
        !self.pco_app_id.is_empty() && !self.pco_secret.is_empty()
    }
}
