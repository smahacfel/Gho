//! Main server implementation

use anyhow::Result;
use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::api::{
    control_ping_handler, get_control_status_handler, get_portfolio_handler, get_settings_handler,
    get_status_handler, get_system_config_handler, get_wallet_status_handler, health_handler,
    pause_handler, resume_handler, start_handler, stop_handler, stop_run_handler,
    update_settings_handler, update_system_config_handler,
};
use crate::config::GuiBackendConfig;
use crate::state::AppState;
use crate::websocket::websocket_handler;

/// GUI Backend server
pub struct GuiBackend {
    /// Server configuration
    config: GuiBackendConfig,

    /// Application state
    state: Arc<AppState>,
}

impl GuiBackend {
    /// Create new GUI backend server
    pub fn new(config: GuiBackendConfig) -> Self {
        Self {
            config,
            state: Arc::new(AppState::new()),
        }
    }

    /// Create new GUI backend with custom state
    pub fn with_state(config: GuiBackendConfig, state: Arc<AppState>) -> Self {
        Self { config, state }
    }

    /// Get reference to application state
    pub fn state(&self) -> Arc<AppState> {
        Arc::clone(&self.state)
    }

    /// Build the router with all endpoints
    fn build_router(&self) -> Router {
        // Create static file service
        let static_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("static");
        let serve_dir = ServeDir::new(static_dir);

        Router::new()
            // Health check
            .route("/health", get(health_handler))
            // Status and monitoring
            .route("/status", get(get_status_handler))
            .route("/portfolio", get(get_portfolio_handler))
            .route("/wallet/status", get(get_wallet_status_handler))
            // Settings
            .route("/settings", get(get_settings_handler))
            .route("/settings", post(update_settings_handler))
            // Control endpoints
            .route("/control/ping", get(control_ping_handler))
            .route("/control/status", get(get_control_status_handler))
            .route("/control/start", post(start_handler))
            .route("/control/pause", post(pause_handler))
            .route("/control/resume", post(resume_handler))
            .route("/control/stop", post(stop_handler))
            .route("/control/stop-run", post(stop_run_handler))
            // System config (root config.toml + ghost_brain_config.toml)
            .route("/config/system", get(get_system_config_handler))
            .route("/config/system", post(update_system_config_handler))
            // WebSocket
            .route("/ws", get(websocket_handler))
            // Static files (must be last to not override API routes)
            .nest_service("/static", serve_dir.clone())
            .fallback_service(serve_dir)
            // Add state
            .with_state(Arc::clone(&self.state))
            // Add CORS (allow all origins for development)
            .layer(
                CorsLayer::new()
                    .allow_origin(Any)
                    .allow_methods(Any)
                    .allow_headers(Any),
            )
            // Add tracing
            .layer(TraceLayer::new_for_http())
    }

    /// Run the server (blocking)
    pub async fn run(self) -> Result<()> {
        if !self.config.enabled {
            info!("GUI backend is disabled in configuration");
            return Ok(());
        }

        let addr = format!("{}:{}", self.config.bind_address, self.config.port);
        info!("Starting GUI backend server on {}", addr);

        let listener = tokio::net::TcpListener::bind(&addr).await?;
        let router = self.build_router();

        info!("GUI backend server ready");
        info!("  - Dashboard: http://{}", addr);
        info!("  - REST API: http://{}", addr);
        info!("  - WebSocket: ws://{}/ws", addr);
        info!("");
        info!("Available endpoints:");
        info!("  GET  /                    - Dashboard UI");
        info!("  GET  /health              - Health check");
        info!("  GET  /status              - System status");
        info!("  GET  /portfolio           - Portfolio state");
        info!("  GET  /wallet/status       - Wallet address + SOL balance");
        info!("  GET  /settings            - Current settings");
        info!("  POST /settings            - Update settings");
        info!("  POST /control/pause       - Pause system");
        info!("  POST /control/resume      - Resume system");
        info!("  POST /control/stop        - Stop system");
        info!("  POST /control/start       - Start launcher in tmux");
        info!("  POST /control/stop-run    - Stop launcher via tmux/pkill");
        info!("  GET  /config/system       - Read GUI-editable config values");
        info!("  POST /config/system       - Save GUI-editable config values");
        info!("  WS   /ws                  - WebSocket live updates");

        axum::serve(listener, router).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_creation() {
        let config = GuiBackendConfig::default();
        let backend = GuiBackend::new(config);

        // Should start in running mode
        assert!(backend.state.is_running());
    }

    #[test]
    fn test_router_builds() {
        let config = GuiBackendConfig::default();
        let backend = GuiBackend::new(config);
        let _router = backend.build_router();
        // If this doesn't panic, router was built successfully
    }
}
