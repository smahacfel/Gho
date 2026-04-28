//! WebSocket handler for live updates

use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tracing::{debug, error, info};

use crate::state::{AppState, StateUpdate};

/// WebSocket upgrade handler
pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> Response {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

/// Handle WebSocket connection
async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();

    // Subscribe to state updates
    let mut rx = state.subscribe();

    info!("WebSocket client connected");

    // Send initial state
    let initial_status = state.get_status();
    let initial_portfolio = state.get_portfolio();
    let initial_settings = state.get_settings();

    if let Ok(msg) = serde_json::to_string(&StateUpdate::StatusUpdate {
        status: initial_status,
    }) {
        if sender.send(Message::Text(msg)).await.is_err() {
            error!("Failed to send initial status");
            return;
        }
    }

    if let Ok(msg) = serde_json::to_string(&StateUpdate::PortfolioUpdate {
        portfolio: initial_portfolio,
    }) {
        if sender.send(Message::Text(msg)).await.is_err() {
            error!("Failed to send initial portfolio");
            return;
        }
    }

    if let Ok(msg) = serde_json::to_string(&StateUpdate::SettingsUpdate {
        settings: initial_settings,
    }) {
        if sender.send(Message::Text(msg)).await.is_err() {
            error!("Failed to send initial settings");
            return;
        }
    }

    // Spawn task to handle incoming messages (for potential bidirectional communication)
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Close(_) => {
                    debug!("WebSocket client sent close message");
                    break;
                }
                Message::Ping(data) => {
                    debug!("Received ping, data: {:?}", data);
                    // Pong is sent automatically by axum
                }
                Message::Text(text) => {
                    debug!("Received text message: {}", text);
                    // Could handle client commands here in the future
                }
                _ => {}
            }
        }
    });

    // Spawn task to broadcast state updates
    let mut send_task = tokio::spawn(async move {
        while let Ok(update) = rx.recv().await {
            let msg = match serde_json::to_string(&update) {
                Ok(msg) => msg,
                Err(e) => {
                    error!("Failed to serialize update: {}", e);
                    continue;
                }
            };

            if sender.send(Message::Text(msg)).await.is_err() {
                debug!("Failed to send update, client likely disconnected");
                break;
            }
        }
    });

    // Wait for either task to complete
    tokio::select! {
        _ = &mut send_task => {
            recv_task.abort();
        }
        _ = &mut recv_task => {
            send_task.abort();
        }
    }

    info!("WebSocket client disconnected");
}
