//! GUI Backend component wrapper

use crate::config::GuiBackendComponentConfig;
use anyhow::Result;
use gui_backend::{GuiBackend, GuiBackendConfig};
use tokio::sync::broadcast;
use tracing::info;

/// Run the GUI Backend component
pub async fn run(
    config: GuiBackendComponentConfig,
    mut shutdown_rx: broadcast::Receiver<()>,
) -> Result<()> {
    info!("GUI Backend: Initializing component");
    info!("  Bind address: {}", config.bind_address);
    info!("  Port: {}", config.port);

    // Create GUI Backend configuration
    let backend_config = GuiBackendConfig {
        port: config.port,
        enabled: true,
        bind_address: config.bind_address.clone(),
    };

    // Create and start backend
    let backend = GuiBackend::new(backend_config);

    info!(
        "GUI Backend: Server available at http://{}:{}",
        config.bind_address, config.port
    );

    // Run backend in a separate task
    let backend_handle = tokio::spawn(async move {
        if let Err(e) = backend.run().await {
            tracing::error!("GUI Backend: Server error: {}", e);
        }
    });

    // Wait for shutdown signal
    let _ = shutdown_rx.recv().await;
    info!("GUI Backend: Shutdown signal received");

    // Cancel backend task
    backend_handle.abort();

    info!("GUI Backend: Component stopped");
    Ok(())
}
