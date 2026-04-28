//! REST API handlers.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use solana_client::rpc_client::RpcClient;
use solana_sdk::signature::{read_keypair_file, Signer};
use std::sync::Arc;
use tracing::{info, warn};

use crate::process_control::{ProcessController, ProcessStatus};
use crate::state::{AppState, Portfolio, Settings, SystemMode, SystemStatus};
use crate::ui_config::{SystemConfigUpdateRequest, UiConfigStore};

/// API error response.
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

impl IntoResponse for ErrorResponse {
    fn into_response(self) -> Response {
        (StatusCode::BAD_REQUEST, Json(self)).into_response()
    }
}

/// Health check response.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
}

/// Generic API response for commands.
#[derive(Debug, Serialize)]
pub struct ApiMessageResponse {
    pub success: bool,
    pub message: String,
}

/// Wallet status response for GUI header.
#[derive(Debug, Serialize)]
pub struct WalletStatusResponse {
    pub wallet_address: String,
    pub sol_balance_lamports: u64,
    pub sol_balance_sol: f64,
}

/// Control response.
#[derive(Debug, Serialize)]
pub struct ControlResponse {
    pub success: bool,
    pub mode: SystemMode,
    pub message: String,
}

/// GET /health - Health check endpoint.
pub async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

/// GET /status - Get system status.
pub async fn get_status_handler(State(state): State<Arc<AppState>>) -> Json<SystemStatus> {
    Json(state.get_status())
}

/// GET /portfolio - Get portfolio state.
pub async fn get_portfolio_handler(State(state): State<Arc<AppState>>) -> Json<Portfolio> {
    Json(state.get_portfolio())
}

/// GET /wallet/status - Get currently configured wallet address and SOL balance.
pub async fn get_wallet_status_handler() -> Result<Json<WalletStatusResponse>, ErrorResponse> {
    let store = UiConfigStore::default();
    let ctx = store
        .load_wallet_runtime_context()
        .map_err(|e| ErrorResponse {
            error: format!("Failed to load wallet context: {}", e),
        })?;

    let keypair = read_keypair_file(&ctx.keypair_path).map_err(|e| ErrorResponse {
        error: format!("Failed to read keypair from {}: {}", ctx.keypair_path, e),
    })?;

    let wallet_address = keypair.pubkey().to_string();
    let rpc_url = ctx.rpc_url.clone();
    let wallet_address_for_rpc = wallet_address.clone();

    let balance_lamports = tokio::task::spawn_blocking(move || {
        let rpc_client = RpcClient::new(rpc_url);
        let pubkey = wallet_address_for_rpc.parse().map_err(|e| {
            anyhow::anyhow!(
                "Failed to parse wallet pubkey {}: {}",
                wallet_address_for_rpc,
                e
            )
        })?;
        rpc_client
            .get_balance(&pubkey)
            .map_err(|e| anyhow::anyhow!("Failed to fetch wallet balance: {}", e))
    })
    .await
    .map_err(|e| ErrorResponse {
        error: format!("Wallet balance task failed: {}", e),
    })?
    .map_err(|e| ErrorResponse {
        error: e.to_string(),
    })?;

    Ok(Json(WalletStatusResponse {
        wallet_address,
        sol_balance_lamports: balance_lamports,
        sol_balance_sol: balance_lamports as f64 / 1_000_000_000.0,
    }))
}

/// GET /settings - Get current in-memory settings.
pub async fn get_settings_handler(State(state): State<Arc<AppState>>) -> Json<Settings> {
    Json(state.get_settings())
}

/// Settings update request.
#[derive(Debug, Deserialize)]
pub struct UpdateSettingsRequest {
    pub position_size_lamports: Option<u64>,
    pub jito_tip_lamports: Option<u64>,
    pub max_slippage: Option<f64>,
    pub enable_jito: Option<bool>,
    pub auto_jito_tip: Option<bool>,
}

/// POST /settings - Update in-memory settings.
pub async fn update_settings_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateSettingsRequest>,
) -> Result<Json<Settings>, ErrorResponse> {
    let mut settings = state.get_settings();

    if let Some(size) = req.position_size_lamports {
        if size == 0 {
            return Err(ErrorResponse {
                error: "Position size must be greater than 0".to_string(),
            });
        }
        settings.position_size_lamports = size;
    }

    if let Some(tip) = req.jito_tip_lamports {
        settings.jito_tip_lamports = tip;
    }

    if let Some(slippage) = req.max_slippage {
        if !(0.0..=1.0).contains(&slippage) {
            return Err(ErrorResponse {
                error: "Slippage must be between 0.0 and 1.0".to_string(),
            });
        }
        settings.max_slippage = slippage;
    }

    if let Some(enable) = req.enable_jito {
        settings.enable_jito = enable;
    }

    if let Some(auto_tip) = req.auto_jito_tip {
        settings.auto_jito_tip = auto_tip;
    }

    state.update_settings(settings.clone());
    info!("In-memory settings updated via API");

    Ok(Json(settings))
}

/// GET /control/status - Process status for launcher/tmux.
pub async fn get_control_status_handler() -> Result<Json<ProcessStatus>, ErrorResponse> {
    let controller = ProcessController::default();
    controller
        .status()
        .await
        .map(Json)
        .map_err(|e| ErrorResponse {
            error: format!("Failed to read process status: {}", e),
        })
}

/// POST /control/start - Start launcher in tmux.
pub async fn start_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ControlResponse>, ErrorResponse> {
    let controller = ProcessController::default();
    let message = controller
        .start_launcher()
        .await
        .map_err(|e| ErrorResponse {
            error: format!("Failed to start launcher: {}", e),
        })?;

    state.set_mode(SystemMode::Running);
    info!("START requested via GUI: {}", message);

    Ok(Json(ControlResponse {
        success: true,
        mode: SystemMode::Running,
        message,
    }))
}

/// POST /control/pause - Pause state only (kept for compatibility).
pub async fn pause_handler(State(state): State<Arc<AppState>>) -> Json<ControlResponse> {
    let current_mode = state.get_mode();

    if current_mode == SystemMode::Stopped {
        warn!("Cannot pause: system is stopped");
        return Json(ControlResponse {
            success: false,
            mode: SystemMode::Stopped,
            message: "Cannot pause: system is stopped".to_string(),
        });
    }

    state.set_mode(SystemMode::Paused);
    info!("System paused via API");

    Json(ControlResponse {
        success: true,
        mode: SystemMode::Paused,
        message: "System paused successfully".to_string(),
    })
}

/// POST /control/resume - Resume/start launcher.
pub async fn resume_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ControlResponse>, ErrorResponse> {
    let controller = ProcessController::default();
    let message = controller
        .start_launcher()
        .await
        .map_err(|e| ErrorResponse {
            error: format!("Failed to resume/start launcher: {}", e),
        })?;

    state.set_mode(SystemMode::Running);
    info!("System resumed via API: {}", message);

    Ok(Json(ControlResponse {
        success: true,
        mode: SystemMode::Running,
        message,
    }))
}

/// POST /control/stop-run - Stop launcher process via tmux + pkill ghost.
pub async fn stop_run_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ControlResponse>, ErrorResponse> {
    let controller = ProcessController::default();
    let message = controller
        .stop_launcher()
        .await
        .map_err(|e| ErrorResponse {
            error: format!("Failed to stop launcher: {}", e),
        })?;

    state.set_mode(SystemMode::Stopped);
    warn!("STOP RUN requested via GUI: {}", message);

    Ok(Json(ControlResponse {
        success: true,
        mode: SystemMode::Stopped,
        message,
    }))
}

/// POST /control/stop - Legacy alias to stop launcher.
pub async fn stop_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ControlResponse>, ErrorResponse> {
    stop_run_handler(State(state)).await
}

/// GET /config/system - Fetch GUI-editable values from root config files.
pub async fn get_system_config_handler(
) -> Result<Json<crate::ui_config::SystemConfigResponse>, ErrorResponse> {
    let store = UiConfigStore::default();
    store
        .load_system_config()
        .map(Json)
        .map_err(|e| ErrorResponse {
            error: format!("Failed to load system config: {}", e),
        })
}

/// POST /config/system - Persist GUI-editable values to root config files.
pub async fn update_system_config_handler(
    Json(req): Json<SystemConfigUpdateRequest>,
) -> Result<Json<crate::ui_config::SystemConfigResponse>, ErrorResponse> {
    let store = UiConfigStore::default();
    store
        .save_system_config(req)
        .map(Json)
        .map_err(|e| ErrorResponse {
            error: format!("Failed to save system config: {}", e),
        })
}

/// GET /control/ping - lightweight control endpoint health.
pub async fn control_ping_handler() -> Json<ApiMessageResponse> {
    Json(ApiMessageResponse {
        success: true,
        message: "control api ready".to_string(),
    })
}
