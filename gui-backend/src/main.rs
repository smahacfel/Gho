use gui_backend::{GuiBackend, GuiBackendConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let port = std::env::var("GUI_BACKEND_PORT")
        .ok()
        .and_then(|raw| raw.parse::<u16>().ok())
        .unwrap_or(8800);
    let bind_address =
        std::env::var("GUI_BACKEND_BIND_ADDRESS").unwrap_or_else(|_| "0.0.0.0".to_string());

    let backend = GuiBackend::new(GuiBackendConfig {
        port,
        enabled: true,
        bind_address,
    });

    backend.run().await
}
