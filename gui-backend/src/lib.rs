//! # GUI Backend - Ghost Control API
//!
//! REST API and WebSocket server for controlling and monitoring the Ghost trading system.
//!
//! ## Features
//!
//! - **REST API**: Status, portfolio, control commands, settings
//! - **WebSocket**: Live updates for portfolio changes and system status
//! - **Runtime Control**: Pause, Resume, Stop commands via atomic flags
//! - **Settings Management**: Configure SOL position size and Jito tips
//!
//! ## Usage
//!
//! ```ignore
//! use gui_backend::{GuiBackend, GuiBackendConfig, SystemMode};
//!
//! let config = GuiBackendConfig {
//!     port: 8800,
//!     enabled: true,
//! };
//!
//! let backend = GuiBackend::new(config);
//! let handle = tokio::spawn(async move {
//!     backend.run().await
//! });
//! ```

pub mod api;
pub mod config;
pub mod portfolio;
pub mod portfolio_bridge;
pub mod price_oracle;
pub mod process_control;
pub mod runtime_config;
pub mod server;
pub mod state;
pub mod ui_config;
pub mod websocket;

pub use config::GuiBackendConfig;
pub use portfolio::{PortfolioConfig, PortfolioState, PortfolioTracker, TokenPosition};
pub use portfolio_bridge::{PortfolioBridge, PortfolioBridgeConfig};
pub use price_oracle::{PoolAddressCache, PoolInfo, PriceOracle};
pub use process_control::{ProcessController, ProcessStatus};
pub use runtime_config::{
    create_shared_config, create_shared_config_from, RuntimeConfig, SharedRuntimeConfig,
};
pub use server::GuiBackend;
pub use state::{AppState, Portfolio, Position, Settings, SystemMode, SystemStatus};
pub use ui_config::{SystemConfigResponse, SystemConfigUpdateRequest, UiConfigStore, UiValue};
