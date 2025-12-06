use dotenv::dotenv;
use std::env;
use std::path::PathBuf;
use crate::error::Result;

/// Configuration for the application.
#[derive(Debug, Clone)]
pub struct Config {
    /// The application name
    pub _app_name: String,
    /// The application version
    pub _app_version: String,
    /// Planning Center Online application ID
    pub pco_app_id: String,
    /// Planning Center Online secret
    pub pco_secret: String,
    /// Path to ProPresenter installation
    pub propresenter_path: Option<String>,
    /// How many days ahead to load services
    pub days_ahead: i64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            _app_name: env!("CARGO_PKG_NAME").to_string(),
            _app_version: env!("CARGO_PKG_VERSION").to_string(),
            pco_app_id: String::new(),
            pco_secret: String::new(),
            propresenter_path: None,
            days_ahead: 30,
        }
    }
}

impl Config {
    /// Load configuration from environment variables
    pub fn load() -> Result<Self> {
        // Try to load .env file if present
        dotenv().ok();
        
        let mut config = Config::default();
        
        // Try to load Planning Center credentials from environment
        if let Ok(app_id) = env::var("PCO_APP_ID") {
            config.pco_app_id = app_id;
        }
        
        if let Ok(secret) = env::var("PCO_SECRET") {
            config.pco_secret = secret;
        }
        
        // Try to load ProPresenter path from environment
        if let Ok(path) = env::var("PROPRESENTER_PATH") {
            config.propresenter_path = Some(path);
        } else {
            // Try to detect ProPresenter installation
            config.propresenter_path = detect_propresenter_path();
        }
        
        // Days ahead can be configured via environment
        if let Ok(days) = env::var("DAYS_AHEAD") {
            if let Ok(days) = days.parse::<i64>() {
                config.days_ahead = days;
            }
        }
        
        Ok(config)
    }
    
    /// Check if Planning Center is configured
    pub fn has_planning_center_credentials(&self) -> bool {
        !self.pco_app_id.is_empty() && !self.pco_secret.is_empty()
    }
}

/// Attempt to detect ProPresenter installation path
fn detect_propresenter_path() -> Option<String> {
    // Common installation paths for different platforms
    let paths = if cfg!(target_os = "macos") {
        vec![
            "/Applications/ProPresenter.app",
            "/Applications/ProPresenter 7.app",
        ]
    } else if cfg!(target_os = "windows") {
        vec![
            "C:\\Program Files\\Renewed Vision\\ProPresenter 7",
            "C:\\Program Files (x86)\\Renewed Vision\\ProPresenter 7",
        ]
    } else {
        // Linux or other platforms
        vec![]
    };
    
    // Check each path
    for path in paths {
        if PathBuf::from(path).exists() {
            return Some(path.to_string());
        }
    }
    
    None
} 