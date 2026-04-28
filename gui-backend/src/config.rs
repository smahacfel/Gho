//! Configuration for GUI backend server

use serde::{Deserialize, Serialize};

/// Configuration for GUI backend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuiBackendConfig {
    /// Port to listen on (default: 8800)
    pub port: u16,

    /// Enable the GUI backend server
    pub enabled: bool,

    /// Bind address (default: "127.0.0.1" for localhost only)
    pub bind_address: String,
}

impl Default for GuiBackendConfig {
    fn default() -> Self {
        Self {
            port: 8800,
            enabled: false,
            bind_address: "127.0.0.1".to_string(),
        }
    }
}

impl GuiBackendConfig {
    /// Create new config with defaults
    pub fn new() -> Self {
        Self::default()
    }

    /// Set port
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Enable the backend
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Set bind address
    pub fn with_bind_address(mut self, address: String) -> Self {
        self.bind_address = address;
        self
    }
}
