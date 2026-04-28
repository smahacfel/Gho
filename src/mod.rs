//! Streaming data sources for real-time Solana monitoring
//!
//! Supports both WebSocket (free tier) and Geyser gRPC (premium)

#[cfg(feature = "ws-stream")]
pub mod websocket_stream;

#[cfg(feature = "geyser-stream")]
pub mod geyser_stream;

use async_trait::async_trait;
use solana_sdk::pubkey::Pubkey;
use tokio::sync::mpsc;

/// Unified streaming interface
#[async_trait]
pub trait StreamProvider: Send + Sync {
    /// Connect to the streaming source
    async fn connect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Subscribe to program updates
    async fn subscribe_program(
        &mut self,
        program_id: &Pubkey,
        tx: mpsc::UnboundedSender<StreamUpdate>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Disconnect and cleanup
    async fn disconnect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

/// Streaming update event
#[derive(Debug, Clone)]
pub enum StreamUpdate {
    ProgramUpdate {
        pubkey: Pubkey,
        data: Vec<u8>,
        slot: u64,
    },
    Transaction {
        signature: String,
        slot: u64,
        error: Option<String>,
    },
}

/// Create streaming provider based on configuration
pub fn create_stream_provider(config: &StreamConfig) -> Box<dyn StreamProvider> {
    match config.mode.as_str() {
        #[cfg(feature = "ws-stream")]
        "websocket" => Box::new(WebSocketStreamProvider::new(config.websocket_url.clone())),
        #[cfg(feature = "geyser-stream")]
        "geyser" => Box::new(geyser_stream::GeyserStreamProvider::new(config)),
        _ => panic!("Invalid streaming mode: {}", config.mode),
    }
}

#[derive(Debug, Clone)]
pub struct StreamConfig {
    pub mode: String,
    pub websocket_url: String,
    pub commitment: String,
    /// Geyser/Yellowstone gRPC endpoint URL
    pub geyser_endpoint: Option<String>,
    /// Geyser authentication token (if required)
    pub geyser_auth_token: Option<String>,
}

#[cfg(feature = "ws-stream")]
struct WebSocketStreamProvider {
    stream: websocket_stream::WebSocketStream,
    client: Option<std::sync::Arc<solana_client::nonblocking::pubsub_client::PubsubClient>>,
}

#[cfg(feature = "ws-stream")]
impl WebSocketStreamProvider {
    fn new(ws_url: String) -> Self {
        Self {
            stream: websocket_stream::WebSocketStream::new(ws_url),
            client: None,
        }
    }
}

#[cfg(feature = "ws-stream")]
#[async_trait]
impl StreamProvider for WebSocketStreamProvider {
    async fn connect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let client = self.stream.connect().await?;
        self.client = Some(client);
        Ok(())
    }

    async fn subscribe_program(
        &mut self,
        program_id: &Pubkey,
        tx: mpsc::UnboundedSender<StreamUpdate>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let client = self.client.as_ref().ok_or("Not connected")?.clone();
        let (program_tx, mut program_rx) = mpsc::unbounded_channel();

        // Subscribe to program updates
        self.stream.subscribe_program(client, program_id, program_tx).await?;

        // Forward updates to the unified channel
        tokio::spawn(async move {
            while let Some(update) = program_rx.recv().await {
                let stream_update = StreamUpdate::ProgramUpdate {
                    pubkey: update.pubkey,
                    data: update.account_data,
                    slot: update.slot,
                };
                if tx.send(stream_update).is_err() {
                    break;
                }
            }
        });

        Ok(())
    }

    async fn disconnect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // WebSocket client handles cleanup automatically on drop
        self.client = None;
        Ok(())
    }
}
