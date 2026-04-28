//! Endpoint server for exposing metrics and health checks

use anyhow::Result;
use tokio::net::TcpListener;

/// Start the endpoint server
pub async fn endpoint_server(port: u16) -> Result<()> {
    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).await?;

    tracing::info!("Metrics endpoint listening on {}", addr);

    // Simple HTTP server for metrics
    loop {
        match listener.accept().await {
            Ok((mut socket, _addr)) => {
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};

                    let mut buf = [0; 1024];
                    match socket.read(&mut buf).await {
                        Ok(_) => {
                            let response = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nMetrics endpoint";
                            let _ = socket.write_all(response.as_bytes()).await;
                        }
                        Err(e) => {
                            tracing::error!("Failed to read from socket: {}", e);
                        }
                    }
                });
            }
            Err(e) => {
                tracing::error!("Failed to accept connection: {}", e);
            }
        }
    }
}
