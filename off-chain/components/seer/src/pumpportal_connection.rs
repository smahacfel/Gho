//! PumpPortal WebSocket connection module for real-time Pump.fun data ingestion
//!
//! This module implements a WebSocket client for PumpPortal's public API,
//! providing real-time access to Pump.fun token creation and trading events.
//!
//! Features:
//! - Single persistent WebSocket connection
//! - Dynamic subscription management for token trades
//! - In-memory statistics tracking per mint
//! - Exponential backoff reconnection
//! - Event mapping to Seer's GeyserEvent format

use crate::config::PumpPortalConfig;
use crate::errors::{SeerError, SeerResult};
use crate::metrics::SeerMetrics;
use crate::types::{
    GeyserEvent, InitializePoolEvent, RawBytesMissingReason, RawInstruction, SyntheticPayload,
    TradeEvent,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use std::collections::{HashMap, HashSet, VecDeque};
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tokio::time::sleep;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, warn};

/// Pump.fun program ID
const PUMPFUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

/// Pump.fun Global State Account (MUST be blacklisted - this is NOT a token)
/// If this address appears as a "mint", the payload is invalid
const PUMPFUN_GLOBAL_STATE: &str = "TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM";

/// SOL mint address (native token)
const SOL_MINT: &str = "So11111111111111111111111111111111111111112";

/// Lazy-initialized Pubkey constant for global state (for efficient comparison)
static PUMPFUN_GLOBAL_STATE_PUBKEY: std::sync::OnceLock<Pubkey> = std::sync::OnceLock::new();

/// Get the Pump.fun global state pubkey (initialized once)
fn get_pumpfun_global_state_pubkey() -> &'static Pubkey {
    PUMPFUN_GLOBAL_STATE_PUBKEY.get_or_init(|| {
        Pubkey::from_str(PUMPFUN_GLOBAL_STATE).expect("PUMPFUN_GLOBAL_STATE must be valid Pubkey")
    })
}

/// Lamports per SOL (1 SOL = 10^9 lamports)
const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

/// Pump.fun tokens use 6 decimal places (1 token = 10^6 base units)
const PUMPFUN_TOKEN_BASE_UNITS: u64 = 1_000_000;
const PUMPFUN_TOKEN_BASE_UNITS_F64: f64 = 1_000_000.0;

/// Custom deserializer that accepts both integer and float values and converts to lamports
fn deserialize_optional_float_to_lamports<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};

    struct OptionalFloatToLamportsVisitor;

    impl<'de> Visitor<'de> for OptionalFloatToLamportsVisitor {
        type Value = Option<u64>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an optional integer or float")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            deserializer
                .deserialize_any(FloatToLamportsVisitor)
                .map(Some)
        }
    }

    struct FloatToLamportsVisitor;

    impl<'de> Visitor<'de> for FloatToLamportsVisitor {
        type Value = u64;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an integer or float")
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            value.try_into().map_err(de::Error::custom)
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value)
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            // Convert SOL (float) to lamports (u64)
            // Check bounds before multiplication to avoid precision issues
            if value < 0.0 {
                return Err(de::Error::custom("negative values not allowed"));
            }
            if value > (u64::MAX as f64 / LAMPORTS_PER_SOL) {
                return Err(de::Error::custom("value too large"));
            }
            let lamports = (value * LAMPORTS_PER_SOL).round();
            Ok(lamports as u64)
        }
    }

    deserializer.deserialize_option(OptionalFloatToLamportsVisitor)
}

/// Custom deserializer that accepts both integer and float token amounts
/// Does NOT multiply by LAMPORTS_PER_SOL - just rounds to nearest integer
fn deserialize_optional_float_to_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};

    struct OptionalFloatToU64Visitor;

    impl<'de> Visitor<'de> for OptionalFloatToU64Visitor {
        type Value = Option<u64>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an optional integer or float")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            deserializer.deserialize_any(FloatToU64Visitor).map(Some)
        }
    }

    struct FloatToU64Visitor;

    impl<'de> Visitor<'de> for FloatToU64Visitor {
        type Value = u64;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an integer or float")
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            value.try_into().map_err(de::Error::custom)
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value)
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            // Round float to nearest integer (for token amounts)
            // Check bounds before rounding
            if value < 0.0 {
                return Err(de::Error::custom("negative values not allowed"));
            }
            if value > u64::MAX as f64 {
                return Err(de::Error::custom("value too large"));
            }
            let rounded = value.round();
            Ok(rounded as u64)
        }
    }

    deserializer.deserialize_option(OptionalFloatToU64Visitor)
}

/// PumpPortal WebSocket message types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method")]
enum PumpPortalRequest {
    #[serde(rename = "subscribeNewToken")]
    SubscribeNewToken,
    #[serde(rename = "subscribeTokenTrade")]
    SubscribeTokenTrade { keys: Vec<String> },
    #[serde(rename = "unsubscribeTokenTrade")]
    UnsubscribeTokenTrade { keys: Vec<String> },
}

/// PumpPortal new token event payload
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewTokenPayload {
    signature: String,
    mint: String,
    #[serde(
        default,
        alias = "bondingCurveKey",
        alias = "bondingCurve",
        alias = "bonding_curve"
    )]
    bonding_curve: Option<String>,
    #[serde(default)]
    trader_public_key: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    symbol: Option<String>,
    #[serde(default)]
    uri: Option<String>,
    #[serde(default)]
    timestamp: Option<i64>,
    #[serde(
        default,
        alias = "initialBuy",
        deserialize_with = "deserialize_optional_float_to_u64"
    )]
    initial_buy: Option<u64>,
    #[serde(default)]
    pool: Option<String>,
    #[serde(
        default,
        alias = "virtualSolReserves",
        deserialize_with = "deserialize_optional_float_to_lamports"
    )]
    virtual_sol_reserves: Option<u64>,
    #[serde(
        default,
        alias = "virtualTokenReserves",
        deserialize_with = "deserialize_optional_float_to_u64"
    )]
    virtual_token_reserves: Option<u64>,
    #[serde(default)]
    tx_type: Option<String>, // "create" for new token events
    /// Virtual tokens remaining in bonding curve (raw f64 from PumpPortal).
    #[serde(default)]
    v_tokens_in_bonding_curve: Option<f64>,
    /// Virtual SOL remaining in bonding curve (raw f64 from PumpPortal).
    #[serde(default)]
    v_sol_in_bonding_curve: Option<f64>,
    /// Market cap in SOL as reported by PumpPortal.
    #[serde(default)]
    market_cap_sol: Option<f64>,
    /// PumpPortal internal flag for unusual market conditions.
    #[serde(default)]
    is_mayhem_mode: Option<bool>,
    /// SOL amount for create events (raw f64 from PumpPortal).
    #[serde(default)]
    sol_amount: Option<f64>,
}

/// PumpPortal trade event payload
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TradePayload {
    signature: String,
    mint: String,
    #[serde(
        default,
        alias = "bondingCurveKey",
        alias = "bondingCurve",
        alias = "bonding_curve"
    )]
    bonding_curve: Option<String>,
    #[serde(default)]
    trader_public_key: Option<String>,
    #[serde(default)]
    tx_type: Option<String>, // "buy" or "sell"
    #[serde(
        default,
        alias = "solAmount",
        deserialize_with = "deserialize_optional_float_to_lamports"
    )]
    sol_amount: Option<u64>,
    #[serde(
        default,
        alias = "tokenAmount",
        deserialize_with = "deserialize_optional_float_to_u64"
    )]
    token_amount: Option<u64>,
    #[serde(default)]
    timestamp: Option<i64>,
    #[serde(default)]
    new_market_cap_sol: Option<f64>,
    #[serde(default)]
    pool: Option<String>,
    #[serde(
        default,
        alias = "virtualSolReserves",
        deserialize_with = "deserialize_optional_float_to_lamports"
    )]
    virtual_sol_reserves: Option<u64>,
    #[serde(
        default,
        alias = "virtualTokenReserves",
        deserialize_with = "deserialize_optional_float_to_u64"
    )]
    virtual_token_reserves: Option<u64>,
    /// Virtual tokens remaining in bonding curve (raw f64 from PumpPortal).
    #[serde(default)]
    v_tokens_in_bonding_curve: Option<f64>,
    /// Virtual SOL remaining in bonding curve (raw f64 from PumpPortal).
    #[serde(default)]
    v_sol_in_bonding_curve: Option<f64>,
    /// Market cap in SOL as reported by PumpPortal.
    #[serde(default)]
    market_cap_sol: Option<f64>,
    /// PumpPortal internal flag for unusual market conditions.
    #[serde(default)]
    is_mayhem_mode: Option<bool>,
}

/// PumpPortal event wrapper
#[derive(Debug, Clone, Deserialize)]
struct PumpPortalEvent {
    #[serde(flatten)]
    data: serde_json::Value,
}

/// Statistics for a single mint
#[derive(Debug, Clone)]
struct MintStats {
    mint: Pubkey,
    bonding_curve: Pubkey,
    first_seen: Instant,
    tx_count: u64,
    buy_volume_lamports: u64,
    sell_volume_lamports: u64,
    unique_traders: HashSet<Pubkey>,
    last_trade_time: Option<Instant>,
}

impl MintStats {
    fn new(mint: Pubkey, bonding_curve: Pubkey) -> Self {
        Self {
            mint,
            bonding_curve,
            first_seen: Instant::now(),
            tx_count: 0,
            buy_volume_lamports: 0,
            sell_volume_lamports: 0,
            unique_traders: HashSet::new(),
            last_trade_time: None,
        }
    }

    fn apply_trade(&mut self, is_buy: bool, sol_amount: u64, trader: Pubkey) {
        self.tx_count = self.tx_count.saturating_add(1);
        if is_buy {
            self.buy_volume_lamports = self.buy_volume_lamports.saturating_add(sol_amount);
        } else {
            self.sell_volume_lamports = self.sell_volume_lamports.saturating_add(sol_amount);
        }
        self.unique_traders.insert(trader);
        self.last_trade_time = Some(Instant::now());
    }

    fn total_volume_lamports(&self) -> u64 {
        self.buy_volume_lamports
            .saturating_add(self.sell_volume_lamports)
    }

    fn is_expired(&self, window_secs: u64) -> bool {
        self.first_seen.elapsed().as_secs() > window_secs
    }
}

/// PumpPortal WebSocket connection manager
pub struct PumpPortalConnection {
    config: PumpPortalConfig,
    metrics: Arc<SeerMetrics>,
    pumpfun_program_id: Pubkey,
    mint_stats: Arc<Mutex<HashMap<Pubkey, MintStats>>>,
    active_mints: Arc<Mutex<VecDeque<Pubkey>>>,
}

impl PumpPortalConnection {
    /// Create a new PumpPortal connection manager
    pub fn new(config: PumpPortalConfig, metrics: Arc<SeerMetrics>) -> SeerResult<Self> {
        let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID)
            .map_err(|e| SeerError::ConfigError(format!("Invalid Pump.fun program ID: {}", e)))?;

        info!("Initializing PumpPortal connection manager");
        info!("  Endpoint: {}", config.ws_url);
        info!("  Max active mints: {}", config.max_active_mints);
        info!(
            "  Subscription batch size: {}",
            config.subscription_batch_size
        );
        info!("  Stats window: {}s", config.stats_window_secs);

        Ok(Self {
            config,
            metrics,
            pumpfun_program_id,
            mint_stats: Arc::new(Mutex::new(HashMap::new())),
            active_mints: Arc::new(Mutex::new(VecDeque::new())),
        })
    }

    /// Connect to PumpPortal WebSocket and return an event stream
    pub async fn connect(
        &self,
    ) -> SeerResult<Pin<Box<dyn futures_util::Stream<Item = SeerResult<GeyserEvent>> + Send>>> {
        let config = self.config.clone();
        let metrics = Arc::clone(&self.metrics);
        let mint_stats = Arc::clone(&self.mint_stats);
        let active_mints = Arc::clone(&self.active_mints);
        let pumpfun_program_id = self.pumpfun_program_id;

        let stream = async_stream::stream! {
            let mut reconnect_count = 0u32;
            let mut current_delay = config.reconnect_base_delay_secs;

            loop {
                if reconnect_count > 0 {
                    info!("Reconnecting to PumpPortal (attempt {})", reconnect_count);
                    sleep(Duration::from_secs(current_delay)).await;

                    // Exponential backoff
                    current_delay = std::cmp::min(
                        current_delay * 2,
                        config.reconnect_max_delay_secs
                    );
                }

                match Self::connect_and_stream(
                    &config,
                    &metrics,
                    &mint_stats,
                    &active_mints,
                    pumpfun_program_id,
                ).await {
                    Ok(mut stream) => {
                        info!("Successfully connected to PumpPortal");
                        current_delay = config.reconnect_base_delay_secs; // Reset backoff
                        reconnect_count = 0;

                        while let Some(event) = stream.next().await {
                            let event: SeerResult<GeyserEvent> = event;
                            yield event;
                        }

                        warn!("PumpPortal stream ended unexpectedly");
                    }
                    Err(e) => {
                        error!("Failed to connect to PumpPortal: {}", e);
                        metrics.websocket_reconnections
                            .with_label_values(&["pumpportal_failed"])
                            .inc();
                    }
                }

                reconnect_count = reconnect_count.saturating_add(1);
            }
        };

        Ok(Box::pin(stream))
    }

    /// Internal method to establish connection and stream events
    async fn connect_and_stream(
        config: &PumpPortalConfig,
        metrics: &Arc<SeerMetrics>,
        mint_stats: &Arc<Mutex<HashMap<Pubkey, MintStats>>>,
        active_mints: &Arc<Mutex<VecDeque<Pubkey>>>,
        pumpfun_program_id: Pubkey,
    ) -> SeerResult<Pin<Box<dyn futures_util::Stream<Item = SeerResult<GeyserEvent>> + Send>>> {
        info!("Connecting to PumpPortal WebSocket: {}", config.ws_url);

        let (ws_stream, _) = connect_async(&config.ws_url)
            .await
            .map_err(|e| SeerError::WebSocketError(format!("Connection failed: {}", e)))?;

        info!("PumpPortal WebSocket connected");

        let (mut write, mut read) = ws_stream.split();

        // Subscribe to new token events
        let subscribe_new_token = PumpPortalRequest::SubscribeNewToken;
        let subscribe_msg = serde_json::to_string(&subscribe_new_token).map_err(|e| {
            SeerError::WebSocketError(format!("Failed to serialize subscription: {}", e))
        })?;

        write
            .send(Message::Text(subscribe_msg))
            .await
            .map_err(|e| {
                SeerError::WebSocketError(format!("Failed to send subscription: {}", e))
            })?;

        info!("Subscribed to PumpPortal new token events");

        let config = config.clone();
        let metrics = Arc::clone(metrics);
        let mint_stats = Arc::clone(mint_stats);
        let active_mints = Arc::clone(active_mints);
        let write = Arc::new(Mutex::new(write));

        let stream = async_stream::stream! {
            let mut pending_subscriptions: Vec<String> = Vec::new();
            let mut last_subscription_time = Instant::now();
            let subscription_interval = Duration::from_millis(500); // Rate limit subscriptions

            while let Some(msg) = read.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        debug!("Received PumpPortal message: {}", text);

                        match Self::parse_event(&text) {
                            Ok(Some(event)) => {
                                metrics.geyser_events_received
                                    .with_label_values(&["pumpportal"])
                                    .inc();

                                // Handle event and generate GeyserEvent(s)
                                match Self::handle_pumpportal_event(
                                    event,
                                    &mint_stats,
                                    &active_mints,
                                    &config,
                                    pumpfun_program_id,
                                    &mut pending_subscriptions,
                                ).await {
                                    Ok(events) => {
                                        for geyser_event in events {
                                            yield Ok(geyser_event);
                                        }
                                    }
                                    Err(e) => {
                                        warn!("Failed to handle PumpPortal event: {}", e);
                                    }
                                }

                                // Process pending subscriptions in batches
                                if !pending_subscriptions.is_empty()
                                    && last_subscription_time.elapsed() >= subscription_interval
                                {
                                    let batch: Vec<String> = pending_subscriptions
                                        .drain(..std::cmp::min(
                                            config.subscription_batch_size,
                                            pending_subscriptions.len()
                                        ))
                                        .collect();

                                    if !batch.is_empty() {
                                        if let Err(e) = Self::send_trade_subscriptions(
                                            &write,
                                            batch,
                                        ).await {
                                            warn!("Failed to send trade subscriptions: {}", e);
                                        }
                                        last_subscription_time = Instant::now();
                                    }
                                }
                            }
                            Ok(None) => {
                                // Not a relevant event or control message
                            }
                            Err(e) => {
                                warn!("Failed to parse PumpPortal event: {}", e);
                            }
                        }
                    }
                    Ok(Message::Close(frame)) => {
                        warn!("PumpPortal WebSocket closed: {:?}", frame);
                        break;
                    }
                    Ok(Message::Ping(payload)) => {
                        let mut writer = write.lock().await;
                        if let Err(e) = writer.send(Message::Pong(payload)).await {
                            warn!("Failed to send pong: {}", e);
                        }
                    }
                    Ok(_) => {
                        // Other message types (binary, pong, etc.)
                    }
                    Err(e) => {
                        error!("PumpPortal WebSocket error: {}", e);
                        break;
                    }
                }
            }

            warn!("PumpPortal WebSocket stream ended");
        };

        Ok(Box::pin(stream))
    }

    /// Parse a PumpPortal event from JSON text
    fn parse_event(text: &str) -> SeerResult<Option<PumpPortalEvent>> {
        // Try to parse as JSON
        let value: serde_json::Value = serde_json::from_str(text)
            .map_err(|e| SeerError::ParseError(format!("Invalid JSON: {}", e)))?;

        // Check if this is an event we care about
        if value.get("mint").is_some() {
            Ok(Some(PumpPortalEvent { data: value }))
        } else {
            // Not an event (might be a subscription acknowledgment or other control message)
            Ok(None)
        }
    }

    /// Handle a PumpPortal event and generate GeyserEvent(s) if applicable
    async fn handle_pumpportal_event(
        event: PumpPortalEvent,
        mint_stats: &Arc<Mutex<HashMap<Pubkey, MintStats>>>,
        active_mints: &Arc<Mutex<VecDeque<Pubkey>>>,
        config: &PumpPortalConfig,
        pumpfun_program_id: Pubkey,
        pending_subscriptions: &mut Vec<String>,
    ) -> SeerResult<Vec<GeyserEvent>> {
        // Check if there's a txType field to determine event type
        if let Some(tx_type_value) = event.data.get("txType") {
            if let Some(tx_type_str) = tx_type_value.as_str() {
                match tx_type_str {
                    "buy" | "sell" => {
                        // Trade event
                        debug!("pumpportal_event=trade type={}", tx_type_str);
                        let trade = serde_json::from_value::<TradePayload>(event.data.clone())
                            .map_err(|e| {
                                SeerError::ParseError(format!("Invalid trade payload: {}", e))
                            })?;
                        let result: Option<GeyserEvent> =
                            Self::handle_trade(trade, mint_stats, pumpfun_program_id).await?;
                        return Ok(result.into_iter().collect());
                    }
                    "create" => {
                        // Token creation event - handle as new_token
                        debug!("pumpportal_event=create");
                        let new_token = serde_json::from_value::<NewTokenPayload>(
                            event.data.clone(),
                        )
                        .map_err(|e| {
                            SeerError::ParseError(format!("Invalid new token payload: {}", e))
                        })?;
                        return Self::handle_new_token(
                            new_token,
                            mint_stats,
                            active_mints,
                            config,
                            pumpfun_program_id,
                            pending_subscriptions,
                        )
                        .await;
                    }
                    unknown => {
                        // Unknown txType - log and ignore
                        debug!("Unknown txType from PumpPortal: {}", unknown);
                        return Ok(vec![]);
                    }
                }
            }
        }

        // No txType field - assume it's a legacy new token event
        debug!("pumpportal_event=new_token");
        let new_token = serde_json::from_value::<NewTokenPayload>(event.data.clone())
            .map_err(|e| SeerError::ParseError(format!("Invalid new token payload: {}", e)))?;
        return Self::handle_new_token(
            new_token,
            mint_stats,
            active_mints,
            config,
            pumpfun_program_id,
            pending_subscriptions,
        )
        .await;
    }

    /// Handle a new token event
    async fn handle_new_token(
        new_token: NewTokenPayload,
        mint_stats: &Arc<Mutex<HashMap<Pubkey, MintStats>>>,
        active_mints: &Arc<Mutex<VecDeque<Pubkey>>>,
        config: &PumpPortalConfig,
        pumpfun_program_id: Pubkey,
        pending_subscriptions: &mut Vec<String>,
    ) -> SeerResult<Vec<GeyserEvent>> {
        if matches!(new_token.pool.as_deref(), Some("bonk")) {
            warn!(
                "Dropping Bonk.fun PumpPortal create event (pool id missing in payload) mint={} signature={}",
                new_token.mint,
                new_token.signature
            );
            return Ok(vec![]);
        }
        let mint = Pubkey::from_str(&new_token.mint)
            .map_err(|e| SeerError::ParseError(format!("Invalid mint pubkey: {}", e)))?;

        // Pre-serialize payload once for error logging (avoid duplication)
        let payload_json_for_error =
            || serde_json::to_string(&new_token).unwrap_or_else(|_| "N/A".to_string());

        // Get global state pubkey for efficient comparison
        let global_state_pubkey = get_pumpfun_global_state_pubkey();

        // [BUG FIX] Invariant 1: base_mint must NEVER be the Pump.fun program ID
        // This would cause OracleRuntime to reject the pool and SnapshotEngine to bootstrap under wrong key
        if mint == pumpfun_program_id {
            error!(
                "🚨 REJECTED: PumpPortal 'mint' field equals Pump.fun program ID - invalid payload | \
                 signature={} | mint={} | bonding_curve={:?} | payload={:?}",
                new_token.signature,
                new_token.mint,
                new_token.bonding_curve,
                payload_json_for_error()
            );
            return Ok(vec![]); // Drop event - do not bootstrap or emit NewPoolDetected
        }

        // [BUG FIX] Invariant 2: base_mint must NEVER be the Pump.fun Global State address
        // This is NOT a token mint, it's a program state account
        if mint == *global_state_pubkey {
            error!(
                "🚨 REJECTED: PumpPortal 'mint' field equals Pump.fun Global State - invalid payload | \
                 signature={} | mint={} | bonding_curve={:?} | payload={:?}",
                new_token.signature,
                new_token.mint,
                new_token.bonding_curve,
                payload_json_for_error()
            );
            return Ok(vec![]);
        }

        let bonding_curve = match &new_token.bonding_curve {
            Some(bc_str) => {
                let bc = Pubkey::from_str(bc_str).map_err(|e| {
                    SeerError::ParseError(format!("Invalid bonding curve pubkey: {}", e))
                })?;

                // [BUG FIX] Invariant 3: bonding_curve must not be the program ID
                if bc == pumpfun_program_id {
                    error!(
                        "🚨 REJECTED: PumpPortal 'bonding_curve' field equals Pump.fun program ID - invalid payload | \
                         signature={} | mint={} | bonding_curve={} | payload={:?}",
                        new_token.signature,
                        new_token.mint,
                        bc_str,
                        payload_json_for_error()
                    );
                    return Ok(vec![]);
                }

                // [BUG FIX] Invariant 4: bonding_curve must not be the global state address
                if bc == *global_state_pubkey {
                    error!(
                        "🚨 REJECTED: PumpPortal 'bonding_curve' field equals Pump.fun Global State - invalid payload | \
                         signature={} | mint={} | bonding_curve={} | payload={:?}",
                        new_token.signature,
                        new_token.mint,
                        bc_str,
                        payload_json_for_error()
                    );
                    return Ok(vec![]);
                }
                bc
            }
            None => {
                // When bonding_curve field is missing, use mint as fallback for backward compatibility
                // This maintains compatibility with legacy PumpPortal payloads that may not include bondingCurveKey
                // This is safe since we validated mint != program_id and mint != global_state above
                warn!(
                    "⚠️  PumpPortal payload missing bonding_curve field, using mint as fallback | \
                     signature={} | mint={}",
                    new_token.signature, new_token.mint
                );
                mint
            }
        };

        info!(
            "New Pump.fun token detected: mint={}, bonding_curve={}",
            mint, bonding_curve
        );

        // Clean up expired mints
        Self::cleanup_expired_mints(mint_stats, active_mints, config.stats_window_secs).await;

        // Check capacity
        let mut stats_guard = mint_stats.lock().await;
        let mut active_guard = active_mints.lock().await;

        if stats_guard.len() >= config.max_active_mints {
            // Remove oldest mint if at capacity
            if let Some(oldest_mint) = active_guard.pop_front() {
                stats_guard.remove(&oldest_mint);
                warn!(
                    "Removed oldest mint to make room: {} (capacity: {})",
                    oldest_mint, config.max_active_mints
                );
            }
        }

        // Add new mint to tracking
        let stats = MintStats::new(mint, bonding_curve);
        stats_guard.insert(mint, stats);
        active_guard.push_back(mint);

        drop(stats_guard);
        drop(active_guard);

        // Add to pending trade subscriptions
        pending_subscriptions.push(new_token.mint.clone());

        // Generate InitializePoolEvent
        let signature = match Signature::from_str(&new_token.signature) {
            Ok(sig) => sig,
            Err(e) => {
                warn!(
                    "Invalid signature in new token event: {} (error: {})",
                    new_token.signature, e
                );
                Signature::new_unique()
            }
        };

        let timestamp_ms = match new_token.timestamp {
            Some(ts) if ts >= 0 => ts,
            Some(ts) => {
                warn!(
                    "Negative timestamp in new token event: {}, using current time",
                    ts
                );
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
                    .min(i64::MAX as u128) as i64
            }
            None => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(i64::MAX as u128) as i64,
        };

        let creator = new_token
            .trader_public_key
            .as_ref()
            .and_then(|s| Pubkey::from_str(s).ok())
            .unwrap_or_else(Pubkey::new_unique);

        // Create InitializePoolEvent
        // Fix 3: Prefer vSolInBondingCurve (f64 from PumpPortal) over legacy virtual_sol_reserves
        // Convert f64 SOL to lamports (u64) for the InitializePoolEvent
        let initial_virtual_sol_reserves = new_token
            .v_sol_in_bonding_curve
            .map(|sol| (sol * LAMPORTS_PER_SOL) as u64)
            .or(new_token.virtual_sol_reserves);
        let initial_virtual_token_reserves = new_token
            .v_tokens_in_bonding_curve
            .map(|tokens| (tokens * PUMPFUN_TOKEN_BASE_UNITS_F64) as u64)
            .or(new_token.virtual_token_reserves);

        let pool_event = InitializePoolEvent {
            slot: None,
            event_ts_ms: Some(timestamp_ms as u64),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature,
            amm_program_id: pumpfun_program_id,
            pool_amm_id: bonding_curve,
            base_mint: mint,
            quote_mint: Pubkey::from_str(SOL_MINT).unwrap(), // SOL
            bonding_curve,
            creator,
            initial_virtual_token_reserves,
            initial_virtual_sol_reserves,
            initial_real_token_reserves: None,
            initial_real_sol_reserves: None,
            token_total_supply: None,
            block_time: Some(timestamp_ms / 1000),
            raw_data: vec![],
        };

        let mut events = vec![Self::pool_event_to_geyser_event(pool_event)];

        // Fix 1 & 2: Also emit a TradeEvent (dev buy) for create events
        // This ensures the PoolTransaction with is_dev_buy=true, sol_amount_lamports,
        // and token_amount_units (from initialBuy) is emitted after NewPoolDetected.
        let sol_amount_lamports = new_token
            .sol_amount
            .map(|sol| (sol * LAMPORTS_PER_SOL) as u64)
            .unwrap_or(0);

        // initialBuy is deserialized as u64, but represents token units.
        // Pump.fun tokens have 6 decimals, so multiply by PUMPFUN_TOKEN_BASE_UNITS for base units.
        let token_amount_units = new_token
            .initial_buy
            .map(|ib| ib.saturating_mul(PUMPFUN_TOKEN_BASE_UNITS));

        let trade_event = TradeEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: None,
            signature,
            event_ordinal: Some(0),
            provenance: None,
            timestamp_ms: timestamp_ms as u64,
            arrival_ts_ms: crate::types::arrival_time_ms(),
            event_time: ghost_core::EventTimeMetadata::default(),
            pool_amm_id: bonding_curve,
            mint,
            signer: creator,
            is_buy: true,
            is_dev_buy: true,
            amount: token_amount_units.unwrap_or(0),
            max_sol_cost: sol_amount_lamports,
            min_sol_output: 0,
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            v_tokens_in_bonding_curve: new_token.v_tokens_in_bonding_curve,
            v_sol_in_bonding_curve: new_token.v_sol_in_bonding_curve,
            market_cap_sol: new_token.market_cap_sol,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
            is_mayhem_mode: new_token.is_mayhem_mode,
            cu_price_micro_lamports: None,
            compute_unit_limit: None,
            inner_ix_count: None,
            cpi_depth: None,
            ata_create_count: None,
            signer_pre_balance_lamports: None,
            signer_post_balance_lamports: None,
            jito_tip_detected: None,
            toolchain_fingerprint: crate::types::ToolchainFingerprintInput::default(),
            // PumpPortal always sends real bonding curve data when reserves are present
            curve_data_known: new_token.v_tokens_in_bonding_curve.is_some()
                && new_token.v_sol_in_bonding_curve.is_some(),
            curve_finality: ghost_core::CurveFinality::from_curve_data_known(
                new_token.v_tokens_in_bonding_curve.is_some()
                    && new_token.v_sol_in_bonding_curve.is_some(),
            ),
            is_pumpswap: false,
        };

        events.push(Self::trade_event_to_geyser_event(
            trade_event,
            pumpfun_program_id,
        ));

        Ok(events)
    }

    /// Handle a trade event
    async fn handle_trade(
        trade: TradePayload,
        mint_stats: &Arc<Mutex<HashMap<Pubkey, MintStats>>>,
        pumpfun_program_id: Pubkey,
    ) -> SeerResult<Option<GeyserEvent>> {
        if matches!(trade.pool.as_deref(), Some("bonk")) {
            warn!(
                "Dropping Bonk.fun PumpPortal trade event (pool id missing in payload) mint={} signature={}",
                trade.mint,
                trade.signature
            );
            return Ok(None);
        }
        let mint = Pubkey::from_str(&trade.mint)
            .map_err(|e| SeerError::ParseError(format!("Invalid mint pubkey: {}", e)))?;

        let trader = trade
            .trader_public_key
            .as_ref()
            .and_then(|s| Pubkey::from_str(s).ok())
            .unwrap_or_else(Pubkey::new_unique);

        let is_buy = trade.tx_type.as_deref() == Some("buy");
        let sol_amount = trade.sol_amount.unwrap_or(0);

        let mut bonding_curve = match &trade.bonding_curve {
            Some(bc_str) => Pubkey::from_str(bc_str).map_err(|e| {
                SeerError::ParseError(format!("Invalid bonding curve pubkey: {}", e))
            })?,
            None => mint,
        };

        // Update stats
        let mut stats_guard = mint_stats.lock().await;
        if let Some(stats) = stats_guard.get_mut(&mint) {
            // Always use the canonical bonding_curve from MintStats to keep pool_amm_id
            // consistent with the earlier NewPoolDetected event. If the trade payload
            // contains a different bondingCurveKey, log it but do not remap.
            if let Some(ref bc_str) = trade.bonding_curve {
                if let Ok(bc) = Pubkey::from_str(bc_str) {
                    if bc != stats.bonding_curve {
                        warn!(
                            "Trade bonding_curve differs from tracked value; keeping tracked | mint={} trade_bc={} tracked_bc={}",
                            mint,
                            bc,
                            stats.bonding_curve
                        );
                    }
                }
            }
            bonding_curve = stats.bonding_curve;
            stats.apply_trade(is_buy, sol_amount, trader);

            debug!(
                "Trade: mint={}, is_buy={}, sol_amount={}, tx_count={}, volume={}",
                mint,
                is_buy,
                sol_amount,
                stats.tx_count,
                stats.total_volume_lamports()
            );
        } else {
            debug!("Received trade for untracked mint: {}", mint);
            // Ignore trades for mints we're not tracking
            return Ok(None);
        }
        drop(stats_guard);

        // Generate TradeEvent
        let signature = match Signature::from_str(&trade.signature) {
            Ok(sig) => sig,
            Err(e) => {
                warn!(
                    "Invalid signature in trade event: {} (error: {})",
                    trade.signature, e
                );
                Signature::new_unique()
            }
        };

        let timestamp_ms = match trade.timestamp {
            Some(ts) if ts >= 0 => ts as u64,
            Some(ts) => {
                warn!(
                    "Negative timestamp in trade event: {}, using current time",
                    ts
                );
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
                    .min(u64::MAX as u128) as u64
            }
            None => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(u64::MAX as u128) as u64,
        };

        let trade_event = TradeEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: None,
            signature,
            event_ordinal: Some(0),
            provenance: None,
            timestamp_ms,
            arrival_ts_ms: crate::types::arrival_time_ms(),
            event_time: ghost_core::EventTimeMetadata::default(),
            pool_amm_id: bonding_curve,
            mint,
            signer: trader,
            is_buy,
            is_dev_buy: false,
            amount: trade.token_amount.unwrap_or(0),
            max_sol_cost: if is_buy { sol_amount } else { 0 },
            min_sol_output: if !is_buy { sol_amount } else { 0 },
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![], // PumpPortal doesn't provide raw instruction data
            mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            v_tokens_in_bonding_curve: trade.v_tokens_in_bonding_curve,
            v_sol_in_bonding_curve: trade.v_sol_in_bonding_curve,
            market_cap_sol: trade.market_cap_sol.or(trade.new_market_cap_sol),
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
            is_mayhem_mode: trade.is_mayhem_mode,
            cu_price_micro_lamports: None,
            compute_unit_limit: None,
            inner_ix_count: None,
            cpi_depth: None,
            ata_create_count: None,
            signer_pre_balance_lamports: None,
            signer_post_balance_lamports: None,
            jito_tip_detected: None,
            toolchain_fingerprint: crate::types::ToolchainFingerprintInput::default(),
            // PumpPortal always sends real bonding curve data when reserves are present
            curve_data_known: trade.v_tokens_in_bonding_curve.is_some()
                && trade.v_sol_in_bonding_curve.is_some(),
            curve_finality: ghost_core::CurveFinality::from_curve_data_known(
                trade.v_tokens_in_bonding_curve.is_some() && trade.v_sol_in_bonding_curve.is_some(),
            ),
            is_pumpswap: false,
        };

        Ok(Some(Self::trade_event_to_geyser_event(
            trade_event,
            pumpfun_program_id,
        )))
    }

    /// Convert InitializePoolEvent to GeyserEvent::Transaction
    fn pool_event_to_geyser_event(event: InitializePoolEvent) -> GeyserEvent {
        let payload = match bincode::serialize(&SyntheticPayload::InitializePool(event.clone())) {
            Ok(bytes) => bytes,
            Err(e) => {
                warn!(
                    "Failed to serialize SyntheticPayload::InitializePool: {}",
                    e
                );
                vec![]
            }
        };

        // Create a synthetic transaction event for pool initialization
        let arrival_ts_ms = crate::types::arrival_time_ms();
        let ingress_wall_ts_ms = crate::types::ingress_epoch_ms();
        GeyserEvent::Transaction {
            slot: event.slot,
            event_ts_ms: crate::types::event_ts_from_block_time(event.block_time),
            arrival_ts_ms: Some(arrival_ts_ms),
            event_time: ghost_core::EventTimeMetadata::new(
                None,
                Some(ingress_wall_ts_ms),
                Some(arrival_ts_ms),
            ),
            signature: event.signature,
            accounts: vec![
                event.amm_program_id,
                event.pool_amm_id,
                event.base_mint,
                event.quote_mint,
                event.bonding_curve,
                event.creator,
            ],
            instructions: vec![RawInstruction {
                program_id: event.amm_program_id,
                account_indices: vec![0, 1, 2, 3, 4, 5],
                data: payload,
            }],
            logs: vec![
                format!("Program {} invoke [1]", event.amm_program_id),
                "Program log: Instruction: Create".to_string(),
                "Program log: InitializeMint2".to_string(),
            ],
            block_time: event.block_time,
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: true, // PumpPortal events are synthetic (no raw transaction bytes)
            source: "pumpportal".to_string(),
            mpcf_payload_bytes: None, // PumpPortal doesn't provide raw bytes
            mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            inner_instructions: vec![],
            pre_token_balances: vec![],
            post_token_balances: vec![],
        }
    }

    /// Convert TradeEvent to GeyserEvent::Transaction
    fn trade_event_to_geyser_event(event: TradeEvent, program_id: Pubkey) -> GeyserEvent {
        let payload = match bincode::serialize(&SyntheticPayload::Trade(event.clone())) {
            Ok(bytes) => bytes,
            Err(e) => {
                warn!("Failed to serialize SyntheticPayload::Trade: {}", e);
                vec![]
            }
        };

        let instruction_log = if event.is_buy {
            "Program log: Instruction: Buy"
        } else {
            "Program log: Instruction: Sell"
        };

        let arrival_ts_ms = crate::types::arrival_time_ms();
        let ingress_wall_ts_ms = crate::types::ingress_epoch_ms();
        GeyserEvent::Transaction {
            slot: event.slot,
            event_ts_ms: Some(event.timestamp_ms),
            arrival_ts_ms: Some(arrival_ts_ms),
            event_time: ghost_core::EventTimeMetadata::new(
                None,
                Some(ingress_wall_ts_ms),
                Some(arrival_ts_ms),
            ),
            signature: event.signature,
            accounts: vec![program_id, event.pool_amm_id, event.mint, event.signer],
            instructions: vec![RawInstruction {
                program_id,
                account_indices: vec![0, 1, 2, 3],
                data: payload,
            }],
            logs: vec![
                format!("Program {} invoke [1]", program_id),
                instruction_log.to_string(),
            ],
            block_time: Some((event.timestamp_ms / 1000) as i64),
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: event.success,
            error_code: event.error_code.clone(),
            compute_units_consumed: event.compute_units_consumed,
            synthetic: true, // PumpPortal events are synthetic (no raw transaction bytes)
            source: "pumpportal".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            inner_instructions: vec![],
            pre_token_balances: vec![],
            post_token_balances: vec![],
        }
    }

    /// Send trade subscriptions for a batch of mints
    async fn send_trade_subscriptions(
        write: &Arc<
            Mutex<
                futures_util::stream::SplitSink<
                    tokio_tungstenite::WebSocketStream<
                        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
                    >,
                    Message,
                >,
            >,
        >,
        mints: Vec<String>,
    ) -> SeerResult<()> {
        let request = PumpPortalRequest::SubscribeTokenTrade {
            keys: mints.clone(),
        };
        let msg = serde_json::to_string(&request)
            .map_err(|e| SeerError::WebSocketError(format!("Failed to serialize: {}", e)))?;

        let mut writer = write.lock().await;
        writer
            .send(Message::Text(msg))
            .await
            .map_err(|e| SeerError::WebSocketError(format!("Failed to send: {}", e)))?;

        info!("Subscribed to {} token trades", mints.len());
        Ok(())
    }

    /// Clean up expired mints from tracking
    async fn cleanup_expired_mints(
        mint_stats: &Arc<Mutex<HashMap<Pubkey, MintStats>>>,
        active_mints: &Arc<Mutex<VecDeque<Pubkey>>>,
        window_secs: u64,
    ) {
        let mut stats_guard = mint_stats.lock().await;
        let mut active_guard = active_mints.lock().await;

        let expired: Vec<Pubkey> = stats_guard
            .iter()
            .filter(|(_, stats)| stats.is_expired(window_secs))
            .map(|(mint, _)| *mint)
            .collect();

        for mint in &expired {
            stats_guard.remove(mint);
            debug!("Removed expired mint: {}", mint);
        }

        // Use HashSet for O(n) retain instead of O(n²) with Vec::contains
        let expired_set: HashSet<Pubkey> = expired.into_iter().collect();
        active_guard.retain(|m| !expired_set.contains(m));

        if !expired_set.is_empty() {
            info!("Cleaned up {} expired mints", expired_set.len());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PumpPortalConfig;
    use serde_json::json;
    use std::collections::{HashMap, VecDeque};
    use std::str::FromStr;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[test]
    fn test_mint_stats_tracking() {
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let mut stats = MintStats::new(mint, bonding_curve);

        assert_eq!(stats.tx_count, 0);
        assert_eq!(stats.buy_volume_lamports, 0);
        assert_eq!(stats.sell_volume_lamports, 0);

        let trader1 = Pubkey::new_unique();
        stats.apply_trade(true, 1000, trader1);
        assert_eq!(stats.tx_count, 1);
        assert_eq!(stats.buy_volume_lamports, 1000);
        assert_eq!(stats.unique_traders.len(), 1);

        let trader2 = Pubkey::new_unique();
        stats.apply_trade(false, 500, trader2);
        assert_eq!(stats.tx_count, 2);
        assert_eq!(stats.sell_volume_lamports, 500);
        assert_eq!(stats.unique_traders.len(), 2);
        assert_eq!(stats.total_volume_lamports(), 1500);
    }

    #[test]
    fn test_new_token_payload_parsing() {
        let json = r#"{
            "signature": "5J8...",
            "mint": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
            "bondingCurve": "8xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsV",
            "traderPublicKey": "9xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsW",
            "timestamp": 1699900000000
        }"#;

        let payload: Result<NewTokenPayload, _> = serde_json::from_str(json);
        assert!(payload.is_ok());

        let payload = payload.unwrap();
        assert_eq!(payload.signature, "5J8...");
        assert_eq!(payload.mint, "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU");
    }

    #[test]
    fn test_trade_payload_parsing() {
        let json = r#"{
            "signature": "5J8...",
            "mint": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
            "txType": "buy",
            "solAmount": 1000000,
            "timestamp": 1699900000000
        }"#;

        let payload: Result<TradePayload, _> = serde_json::from_str(json);
        assert!(payload.is_ok());

        let payload = payload.unwrap();
        assert_eq!(payload.tx_type, Some("buy".to_string()));
        assert_eq!(payload.sol_amount, Some(1000000));
    }

    #[test]
    fn test_no_timestamp_slot_estimator_present() {
        let source = include_str!("pumpportal_connection.rs");
        let needle = ["estimate", "_slot_from_timestamp_ms"].concat();
        assert!(
            !source.contains(&needle),
            "PumpPortal must not include timestamp->slot estimator"
        );
    }

    #[tokio::test]
    async fn test_pumpportal_no_slot() {
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        let mint_stats = Arc::new(Mutex::new(HashMap::new()));
        mint_stats
            .lock()
            .await
            .insert(mint, MintStats::new(mint, bonding_curve));

        let event = PumpPortalEvent {
            data: json!({
                "signature": "sig_no_slot",
                "mint": mint.to_string(),
                "bondingCurveKey": bonding_curve.to_string(),
                "txType": "buy",
                "solAmount": 1.0,
                "tokenAmount": 10,
                "timestamp": 1234567890
            }),
        };

        let active_mints = Arc::new(Mutex::new(VecDeque::new()));
        let config = PumpPortalConfig::default();
        let mut pending_subscriptions = Vec::new();
        let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID).unwrap();

        let result = PumpPortalConnection::handle_pumpportal_event(
            event,
            &mint_stats,
            &active_mints,
            &config,
            pumpfun_program_id,
            &mut pending_subscriptions,
        )
        .await
        .expect("trade event should parse");
        let result = result.into_iter().last().expect("expected trade event");

        match result {
            GeyserEvent::Transaction {
                slot, instructions, ..
            } => {
                assert!(slot.is_none(), "PumpPortal GeyserEvent slot must be None");
                let synthetic = instructions
                    .iter()
                    .find_map(|ix| bincode::deserialize::<SyntheticPayload>(&ix.data).ok());
                match synthetic {
                    Some(SyntheticPayload::Trade(trade)) => {
                        assert!(
                            trade.slot.is_none(),
                            "PumpPortal TradeEvent slot must be None"
                        );
                    }
                    other => panic!("Expected SyntheticPayload::Trade, got {:?}", other),
                }
            }
            other => panic!("Expected GeyserEvent::Transaction, got {:?}", other),
        }
    }

    #[test]
    fn test_bonding_curve_key_alias_new_token() {
        let bonding_curve = Pubkey::new_unique().to_string();
        let payload: NewTokenPayload = serde_json::from_value(json!({
            "signature": "sig",
            "mint": Pubkey::new_unique().to_string(),
            "bondingCurveKey": bonding_curve.clone()
        }))
        .expect("new token payload should parse");

        assert_eq!(
            payload.bonding_curve.as_deref(),
            Some(bonding_curve.as_str())
        );
    }

    #[test]
    fn test_bonding_curve_key_alias_trade() {
        let bonding_curve = Pubkey::new_unique().to_string();
        let payload: TradePayload = serde_json::from_value(json!({
            "signature": "sig",
            "mint": Pubkey::new_unique().to_string(),
            "bondingCurveKey": bonding_curve.clone(),
            "txType": "buy"
        }))
        .expect("trade payload should parse");

        assert_eq!(
            payload.bonding_curve.as_deref(),
            Some(bonding_curve.as_str())
        );
    }

    #[tokio::test]
    async fn test_trade_payload_with_tx_type_does_not_create_pool() {
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let event = PumpPortalEvent {
            data: json!({
                "signature": "sig",
                "mint": mint.to_string(),
                "bondingCurveKey": bonding_curve.to_string(),
                "txType": "buy",
                "solAmount": 10,
                "tokenAmount": 5
            }),
        };

        let mint_stats = Arc::new(Mutex::new(HashMap::new()));
        let active_mints = Arc::new(Mutex::new(VecDeque::new()));
        let config = PumpPortalConfig::default();
        let mut pending_subscriptions = Vec::new();
        let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID).unwrap();

        let result = PumpPortalConnection::handle_pumpportal_event(
            event,
            &mint_stats,
            &active_mints,
            &config,
            pumpfun_program_id,
            &mut pending_subscriptions,
        )
        .await
        .expect("trade event should parse");

        assert!(result.is_empty());
        assert!(mint_stats.lock().await.is_empty());
        assert!(pending_subscriptions.is_empty());
    }

    #[tokio::test]
    async fn test_txtype_create_routes_to_new_token() {
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let event = PumpPortalEvent {
            data: json!({
                "signature": "sig123",
                "mint": mint.to_string(),
                "bondingCurveKey": bonding_curve.to_string(),
                "txType": "create",
                "solAmount": 0.5,
                "initialBuy": 1.2
            }),
        };

        let mint_stats = Arc::new(Mutex::new(HashMap::new()));
        let active_mints = Arc::new(Mutex::new(VecDeque::new()));
        let config = PumpPortalConfig::default();
        let mut pending_subscriptions = Vec::new();
        let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID).unwrap();

        let result = PumpPortalConnection::handle_pumpportal_event(
            event,
            &mint_stats,
            &active_mints,
            &config,
            pumpfun_program_id,
            &mut pending_subscriptions,
        )
        .await
        .expect("create event should parse");

        // Should generate an event (InitializePoolEvent)
        assert!(!result.is_empty());
        // Should add mint to tracking
        assert_eq!(mint_stats.lock().await.len(), 1);
        // Should add to pending subscriptions
        assert_eq!(pending_subscriptions.len(), 1);
    }

    #[tokio::test]
    async fn test_txtype_buy_with_float_sol_amount() {
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        // First add the mint to tracking (simulating it was created earlier)
        let mint_stats = Arc::new(Mutex::new(HashMap::new()));
        mint_stats
            .lock()
            .await
            .insert(mint, MintStats::new(mint, bonding_curve));

        let event = PumpPortalEvent {
            data: json!({
                "signature": "sig456",
                "mint": mint.to_string(),
                "bondingCurveKey": bonding_curve.to_string(),
                "txType": "buy",
                "solAmount": 0.716049381,
                "tokenAmount": 1000.5,
                "traderPublicKey": Pubkey::new_unique().to_string()
            }),
        };

        let active_mints = Arc::new(Mutex::new(VecDeque::new()));
        let config = PumpPortalConfig::default();
        let mut pending_subscriptions = Vec::new();
        let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID).unwrap();

        let result = PumpPortalConnection::handle_pumpportal_event(
            event,
            &mint_stats,
            &active_mints,
            &config,
            pumpfun_program_id,
            &mut pending_subscriptions,
        )
        .await
        .expect("buy event with float solAmount should parse");

        // Should generate a trade event
        assert!(!result.is_empty());
    }

    #[tokio::test]
    async fn test_txtype_sell_with_float_sol_amount() {
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        // First add the mint to tracking
        let mint_stats = Arc::new(Mutex::new(HashMap::new()));
        mint_stats
            .lock()
            .await
            .insert(mint, MintStats::new(mint, bonding_curve));

        let event = PumpPortalEvent {
            data: json!({
                "signature": "sig789",
                "mint": mint.to_string(),
                "bondingCurveKey": bonding_curve.to_string(),
                "txType": "sell",
                "solAmount": 2.5,
                "tokenAmount": 5000.0
            }),
        };

        let active_mints = Arc::new(Mutex::new(VecDeque::new()));
        let config = PumpPortalConfig::default();
        let mut pending_subscriptions = Vec::new();
        let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID).unwrap();

        let result = PumpPortalConnection::handle_pumpportal_event(
            event,
            &mint_stats,
            &active_mints,
            &config,
            pumpfun_program_id,
            &mut pending_subscriptions,
        )
        .await
        .expect("sell event with float solAmount should parse");

        // Should generate a trade event
        assert!(!result.is_empty());
    }

    #[tokio::test]
    async fn test_missing_txtype_routes_to_new_token() {
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let event = PumpPortalEvent {
            data: json!({
                "signature": "sig_no_txtype",
                "mint": mint.to_string(),
                "bondingCurveKey": bonding_curve.to_string(),
                "initialBuy": 100
            }),
        };

        let mint_stats = Arc::new(Mutex::new(HashMap::new()));
        let active_mints = Arc::new(Mutex::new(VecDeque::new()));
        let config = PumpPortalConfig::default();
        let mut pending_subscriptions = Vec::new();
        let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID).unwrap();

        let result = PumpPortalConnection::handle_pumpportal_event(
            event,
            &mint_stats,
            &active_mints,
            &config,
            pumpfun_program_id,
            &mut pending_subscriptions,
        )
        .await
        .expect("event without txType should parse as new_token");

        // Should generate an event
        assert!(!result.is_empty());
        // Should add mint to tracking
        assert_eq!(mint_stats.lock().await.len(), 1);
    }

    #[tokio::test]
    async fn test_unknown_txtype_ignored() {
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let event = PumpPortalEvent {
            data: json!({
                "signature": "sig_unknown",
                "mint": mint.to_string(),
                "bondingCurveKey": bonding_curve.to_string(),
                "txType": "unknown_type"
            }),
        };

        let mint_stats = Arc::new(Mutex::new(HashMap::new()));
        let active_mints = Arc::new(Mutex::new(VecDeque::new()));
        let config = PumpPortalConfig::default();
        let mut pending_subscriptions = Vec::new();
        let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID).unwrap();

        let result = PumpPortalConnection::handle_pumpportal_event(
            event,
            &mint_stats,
            &active_mints,
            &config,
            pumpfun_program_id,
            &mut pending_subscriptions,
        )
        .await
        .expect("event with unknown txType should not error");

        // Should return None (ignored)
        assert!(result.is_empty());
        // Should not add mint to tracking
        assert!(mint_stats.lock().await.is_empty());
        assert!(pending_subscriptions.is_empty());
    }

    #[test]
    fn test_float_sol_to_lamports_conversion() {
        // Test that float solAmount values are properly converted to lamports
        let json_with_float = r#"{
            "signature": "sig",
            "mint": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
            "txType": "buy",
            "solAmount": 0.716049381,
            "tokenAmount": 1000.5
        }"#;

        let payload: Result<TradePayload, _> = serde_json::from_str(json_with_float);
        assert!(payload.is_ok(), "Should parse float solAmount");

        let payload = payload.unwrap();
        // 0.716049381 SOL = 716049381 lamports
        assert_eq!(payload.sol_amount, Some(716049381));
        // tokenAmount should be rounded to 1001, NOT multiplied by LAMPORTS_PER_SOL
        assert_eq!(payload.token_amount, Some(1001));
    }

    #[test]
    fn test_integer_sol_amount_still_works() {
        // Test that integer solAmount values still work
        let json_with_int = r#"{
            "signature": "sig",
            "mint": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
            "txType": "buy",
            "solAmount": 1000000000,
            "tokenAmount": 1000
        }"#;

        let payload: Result<TradePayload, _> = serde_json::from_str(json_with_int);
        assert!(payload.is_ok(), "Should parse integer solAmount");

        let payload = payload.unwrap();
        assert_eq!(payload.sol_amount, Some(1000000000));
        assert_eq!(payload.token_amount, Some(1000));
    }

    #[test]
    fn test_token_amount_not_converted_to_lamports() {
        // Verify that tokenAmount is NOT multiplied by LAMPORTS_PER_SOL
        let json = r#"{
            "signature": "sig",
            "mint": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
            "txType": "buy",
            "tokenAmount": 5.0
        }"#;

        let payload: Result<TradePayload, _> = serde_json::from_str(json);
        assert!(payload.is_ok());

        let payload = payload.unwrap();
        // tokenAmount 5.0 should become 5, NOT 5000000000
        assert_eq!(payload.token_amount, Some(5));
    }

    // =========================================================================
    // BUG FIX TESTS: Prevent base_mint from being set to Pump.fun program ID
    // =========================================================================

    #[tokio::test]
    async fn test_rejects_mint_equals_program_id() {
        // Regression test: Ensure parser rejects when mint field contains program ID
        let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID).unwrap();
        let valid_bonding_curve = Pubkey::new_unique();

        let event = PumpPortalEvent {
            data: json!({
                "signature": "test_sig",
                "mint": PUMPFUN_PROGRAM_ID, // BUG: mint is program ID!
                "bondingCurveKey": valid_bonding_curve.to_string(),
                "txType": "create",
                "timestamp": 1234567890
            }),
        };

        let mint_stats = Arc::new(Mutex::new(HashMap::new()));
        let active_mints = Arc::new(Mutex::new(VecDeque::new()));
        let config = PumpPortalConfig::default();
        let mut pending_subscriptions = Vec::new();

        let result = PumpPortalConnection::handle_pumpportal_event(
            event,
            &mint_stats,
            &active_mints,
            &config,
            pumpfun_program_id,
            &mut pending_subscriptions,
        )
        .await
        .expect("should not error but return None");

        // Should reject (return None) - do not bootstrap, do not emit
        assert!(
            result.is_empty(),
            "Event with mint==program_id should be rejected"
        );
        assert!(
            mint_stats.lock().await.is_empty(),
            "Should not track invalid mint"
        );
        assert!(
            pending_subscriptions.is_empty(),
            "Should not subscribe to invalid mint"
        );
    }

    #[tokio::test]
    async fn test_rejects_mint_equals_global_state() {
        // Regression test: Ensure parser rejects when mint field contains global state address
        let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID).unwrap();
        let valid_bonding_curve = Pubkey::new_unique();

        let event = PumpPortalEvent {
            data: json!({
                "signature": "test_sig",
                "mint": PUMPFUN_GLOBAL_STATE, // BUG: mint is global state!
                "bondingCurveKey": valid_bonding_curve.to_string(),
                "txType": "create",
                "timestamp": 1234567890
            }),
        };

        let mint_stats = Arc::new(Mutex::new(HashMap::new()));
        let active_mints = Arc::new(Mutex::new(VecDeque::new()));
        let config = PumpPortalConfig::default();
        let mut pending_subscriptions = Vec::new();

        let result = PumpPortalConnection::handle_pumpportal_event(
            event,
            &mint_stats,
            &active_mints,
            &config,
            pumpfun_program_id,
            &mut pending_subscriptions,
        )
        .await
        .expect("should not error but return None");

        // Should reject (return None)
        assert!(
            result.is_empty(),
            "Event with mint==global_state should be rejected"
        );
        assert!(
            mint_stats.lock().await.is_empty(),
            "Should not track invalid mint"
        );
        assert!(
            pending_subscriptions.is_empty(),
            "Should not subscribe to invalid mint"
        );
    }

    #[tokio::test]
    async fn test_rejects_bonding_curve_equals_program_id() {
        // Regression test: Ensure parser rejects when bondingCurveKey field contains program ID
        let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID).unwrap();
        let valid_mint = Pubkey::new_unique();

        let event = PumpPortalEvent {
            data: json!({
                "signature": "test_sig",
                "mint": valid_mint.to_string(),
                "bondingCurveKey": PUMPFUN_PROGRAM_ID, // BUG: bonding curve is program ID!
                "txType": "create",
                "timestamp": 1234567890
            }),
        };

        let mint_stats = Arc::new(Mutex::new(HashMap::new()));
        let active_mints = Arc::new(Mutex::new(VecDeque::new()));
        let config = PumpPortalConfig::default();
        let mut pending_subscriptions = Vec::new();

        let result = PumpPortalConnection::handle_pumpportal_event(
            event,
            &mint_stats,
            &active_mints,
            &config,
            pumpfun_program_id,
            &mut pending_subscriptions,
        )
        .await
        .expect("should not error but return None");

        // Should reject (return None)
        assert!(
            result.is_empty(),
            "Event with bonding_curve==program_id should be rejected"
        );
        assert!(
            mint_stats.lock().await.is_empty(),
            "Should not track invalid bonding curve"
        );
        assert!(
            pending_subscriptions.is_empty(),
            "Should not subscribe to invalid bonding curve"
        );
    }

    #[tokio::test]
    async fn test_rejects_bonding_curve_equals_global_state() {
        // Regression test: Ensure parser rejects when bondingCurveKey field contains global state
        let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID).unwrap();
        let valid_mint = Pubkey::new_unique();

        let event = PumpPortalEvent {
            data: json!({
                "signature": "test_sig",
                "mint": valid_mint.to_string(),
                "bondingCurveKey": PUMPFUN_GLOBAL_STATE, // BUG: bonding curve is global state!
                "txType": "create",
                "timestamp": 1234567890
            }),
        };

        let mint_stats = Arc::new(Mutex::new(HashMap::new()));
        let active_mints = Arc::new(Mutex::new(VecDeque::new()));
        let config = PumpPortalConfig::default();
        let mut pending_subscriptions = Vec::new();

        let result = PumpPortalConnection::handle_pumpportal_event(
            event,
            &mint_stats,
            &active_mints,
            &config,
            pumpfun_program_id,
            &mut pending_subscriptions,
        )
        .await
        .expect("should not error but return None");

        // Should reject (return None)
        assert!(
            result.is_empty(),
            "Event with bonding_curve==global_state should be rejected"
        );
        assert!(
            mint_stats.lock().await.is_empty(),
            "Should not track invalid bonding curve"
        );
        assert!(
            pending_subscriptions.is_empty(),
            "Should not subscribe to invalid bonding curve"
        );
    }

    #[tokio::test]
    async fn test_accepts_valid_create_event() {
        // Positive test: Ensure parser accepts valid create event
        let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID).unwrap();
        let valid_mint = Pubkey::new_unique();
        let valid_bonding_curve = Pubkey::new_unique();

        let event = PumpPortalEvent {
            data: json!({
                "signature": "valid_sig",
                "mint": valid_mint.to_string(),
                "bondingCurveKey": valid_bonding_curve.to_string(),
                "txType": "create",
                "timestamp": 1234567890,
                "virtualSolReserves": 30.0,
                "virtualTokenReserves": 1_000_000_000.0
            }),
        };

        let mint_stats = Arc::new(Mutex::new(HashMap::new()));
        let active_mints = Arc::new(Mutex::new(VecDeque::new()));
        let config = PumpPortalConfig::default();
        let mut pending_subscriptions = Vec::new();

        let result = PumpPortalConnection::handle_pumpportal_event(
            event,
            &mint_stats,
            &active_mints,
            &config,
            pumpfun_program_id,
            &mut pending_subscriptions,
        )
        .await
        .expect("valid event should parse");

        // Should accept and process
        assert!(!result.is_empty(), "Valid create event should be accepted");

        // Verify GeyserEvent was created with correct base_mint
        if let Some(GeyserEvent::Transaction { accounts, .. }) = result.first() {
            // accounts[0] = amm_program_id
            // accounts[1] = pool_amm_id (bonding_curve)
            // accounts[2] = base_mint (valid_mint)
            // accounts[3] = quote_mint (SOL)
            // accounts[4] = bonding_curve
            // accounts[5] = creator
            assert_eq!(accounts.len(), 6, "GeyserEvent should have 6 accounts");
            assert_eq!(
                accounts[0], pumpfun_program_id,
                "accounts[0] should be program_id"
            );
            assert_eq!(
                accounts[1], valid_bonding_curve,
                "accounts[1] (pool_amm_id) should be bonding_curve"
            );
            assert_eq!(
                accounts[2], valid_mint,
                "accounts[2] (base_mint) should be token mint"
            );
            assert_eq!(
                accounts[4], valid_bonding_curve,
                "accounts[4] should be bonding_curve"
            );
        } else {
            panic!("Expected GeyserEvent::Transaction");
        }

        // Should track mint and add to pending subscriptions
        assert_eq!(mint_stats.lock().await.len(), 1, "Should track valid mint");
        assert_eq!(
            pending_subscriptions.len(),
            1,
            "Should add to pending subscriptions"
        );
    }

    #[tokio::test]
    async fn test_missing_bonding_curve_uses_mint_fallback() {
        // Test fallback behavior when bondingCurveKey is missing
        let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID).unwrap();
        let valid_mint = Pubkey::new_unique();

        let event = PumpPortalEvent {
            data: json!({
                "signature": "fallback_sig",
                "mint": valid_mint.to_string(),
                // bondingCurveKey is missing
                "txType": "create",
                "timestamp": 1234567890
            }),
        };

        let mint_stats = Arc::new(Mutex::new(HashMap::new()));
        let active_mints = Arc::new(Mutex::new(VecDeque::new()));
        let config = PumpPortalConfig::default();
        let mut pending_subscriptions = Vec::new();

        let result = PumpPortalConnection::handle_pumpportal_event(
            event,
            &mint_stats,
            &active_mints,
            &config,
            pumpfun_program_id,
            &mut pending_subscriptions,
        )
        .await
        .expect("event with missing bondingCurveKey should use fallback");

        // Should accept with fallback (bonding_curve = mint)
        assert!(
            !result.is_empty(),
            "Event with missing bondingCurveKey should use mint as fallback"
        );

        // Verify GeyserEvent was created correctly
        if let Some(GeyserEvent::Transaction { accounts, .. }) = result.first() {
            assert_eq!(
                accounts[1], valid_mint,
                "pool_amm_id should fall back to mint"
            );
            assert_eq!(accounts[2], valid_mint, "base_mint should be mint");
            assert_eq!(
                accounts[4], valid_mint,
                "bonding_curve should fall back to mint"
            );
        } else {
            panic!("Expected GeyserEvent::Transaction");
        }
    }

    // =========================================================================
    // CROSS-FIELD INVARIANT TESTS (Issue #19 follow-up)
    // =========================================================================

    #[tokio::test]
    async fn test_field_mapping_correctness() {
        // Positive test: Verify correct 1:1 mapping of fields
        // base_mint comes from payload.mint
        // bonding_curve comes from payload.bondingCurveKey
        // pool_amm_id should be bonding_curve (for Pump.fun)

        let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID).unwrap();
        let token_mint = Pubkey::new_unique();
        let bonding_curve_address = Pubkey::new_unique();

        let event = PumpPortalEvent {
            data: json!({
                "signature": "field_mapping_test",
                "mint": token_mint.to_string(),
                "bondingCurveKey": bonding_curve_address.to_string(),
                "txType": "create",
                "timestamp": 1234567890
            }),
        };

        let mint_stats = Arc::new(Mutex::new(HashMap::new()));
        let active_mints = Arc::new(Mutex::new(VecDeque::new()));
        let config = PumpPortalConfig::default();
        let mut pending_subscriptions = Vec::new();

        let result = PumpPortalConnection::handle_pumpportal_event(
            event,
            &mint_stats,
            &active_mints,
            &config,
            pumpfun_program_id,
            &mut pending_subscriptions,
        )
        .await
        .expect("valid event should parse");

        assert!(!result.is_empty(), "Valid event should be accepted");

        if let Some(GeyserEvent::Transaction { accounts, .. }) = result.first() {
            // Verify field mapping:
            // accounts[0] = amm_program_id (PUMPFUN_PROGRAM_ID)
            // accounts[1] = pool_amm_id (bonding_curve_address for Pump.fun)
            // accounts[2] = base_mint (token_mint)
            // accounts[3] = quote_mint (SOL)
            // accounts[4] = bonding_curve (bonding_curve_address)
            // accounts[5] = creator

            assert_eq!(
                accounts[0], pumpfun_program_id,
                "accounts[0] should be program_id"
            );
            assert_eq!(
                accounts[1], bonding_curve_address,
                "accounts[1] (pool_amm_id) should be bonding_curve"
            );
            assert_eq!(
                accounts[2], token_mint,
                "accounts[2] (base_mint) should be token mint from payload.mint"
            );
            assert_eq!(
                accounts[4], bonding_curve_address,
                "accounts[4] (bonding_curve) should match bondingCurveKey"
            );

            // Cross-field validation: ensure no field swaps
            assert_ne!(
                accounts[2], accounts[0],
                "base_mint must not equal amm_program"
            );
            assert_ne!(
                accounts[2], accounts[1],
                "base_mint should not equal pool_amm_id in normal case"
            );
        } else {
            panic!("Expected GeyserEvent::Transaction");
        }
    }

    // =========================================================================
    // Etap 5: PumpPortal Parser Tests — vTokensInBondingCurve, vSolInBondingCurve,
    // marketCapSol, txType:create mapping
    // =========================================================================

    /// Helper: extract TradeEvent from GeyserEvent::Transaction's synthetic payload
    fn extract_trade_event(event: &GeyserEvent) -> Option<TradeEvent> {
        match event {
            GeyserEvent::Transaction { instructions, .. } => instructions.iter().find_map(|ix| {
                match bincode::deserialize::<SyntheticPayload>(&ix.data) {
                    Ok(SyntheticPayload::Trade(trade)) => Some(trade),
                    _ => None,
                }
            }),
            _ => None,
        }
    }

    // --- Test 1: test_parse_create_event ---
    #[test]
    fn test_parse_create_event() {
        let json = r#"{
            "signature": "4uVn123",
            "mint": "658ZHoFdR6e9V8ie9RhUpgaND6NWK6dF7vjr7WF3pump",
            "traderPublicKey": "DVKg1YfyxNzvQuccJ2HfDcAXh3TcFouccBLjnfy4aehT",
            "txType": "create",
            "initialBuy": 17376518,
            "solAmount": 0.493827159,
            "bondingCurveKey": "9Vi6i2o3ebHQku6Y9re6YmqzT8ebf3QTwbzyjempS7Ux",
            "vTokensInBondingCurve": 1055623481.83309,
            "vSolInBondingCurve": 30.49382715899998,
            "marketCapSol": 28.88702997213311,
            "name": "ArmouredJesus",
            "symbol": "JESUS",
            "uri": "https://ipfs.io/ipfs/test",
            "pool": "pump"
        }"#;

        let payload: NewTokenPayload = serde_json::from_str(json).unwrap();

        assert_eq!(payload.signature, "4uVn123");
        assert_eq!(payload.mint, "658ZHoFdR6e9V8ie9RhUpgaND6NWK6dF7vjr7WF3pump");
        assert_eq!(payload.tx_type, Some("create".to_string()));
        assert_eq!(
            payload.trader_public_key,
            Some("DVKg1YfyxNzvQuccJ2HfDcAXh3TcFouccBLjnfy4aehT".to_string())
        );
        assert!((payload.v_tokens_in_bonding_curve.unwrap() - 1055623481.83309).abs() < 1e-3); // large value, lower precision
        assert!((payload.v_sol_in_bonding_curve.unwrap() - 30.49382715899998).abs() < 1e-10);
        assert!((payload.market_cap_sol.unwrap() - 28.88702997213311).abs() < 1e-10);
        assert!(payload.initial_buy.is_some());
        assert!((payload.sol_amount.unwrap() - 0.493827159).abs() < 1e-10);
    }

    // --- Test 2: test_parse_buy_event ---
    #[test]
    fn test_parse_buy_event() {
        let json = r#"{
            "signature": "sig_buy",
            "mint": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
            "txType": "buy",
            "solAmount": 1500000000,
            "tokenAmount": 5000000,
            "vTokensInBondingCurve": 1000000000.0,
            "vSolInBondingCurve": 31.5,
            "marketCapSol": 30.0
        }"#;

        let payload: TradePayload = serde_json::from_str(json).unwrap();

        assert_eq!(payload.tx_type, Some("buy".to_string()));
        assert!((payload.v_tokens_in_bonding_curve.unwrap() - 1000000000.0).abs() < 1e-3); // large value, lower precision
        assert!((payload.v_sol_in_bonding_curve.unwrap() - 31.5).abs() < 1e-10);
        assert!((payload.market_cap_sol.unwrap() - 30.0).abs() < 1e-10);
    }

    // --- Test 3: test_parse_sell_event ---
    #[test]
    fn test_parse_sell_event() {
        let json = r#"{
            "signature": "sig_sell",
            "mint": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
            "txType": "sell",
            "solAmount": 800000000,
            "tokenAmount": 3000000,
            "vTokensInBondingCurve": 1020000000.0,
            "vSolInBondingCurve": 29.2
        }"#;

        let payload: TradePayload = serde_json::from_str(json).unwrap();

        assert_eq!(payload.tx_type, Some("sell".to_string()));
        assert!((payload.v_tokens_in_bonding_curve.unwrap() - 1020000000.0).abs() < 1e-3); // large value, lower precision
        assert!((payload.v_sol_in_bonding_curve.unwrap() - 29.2).abs() < 1e-10);
    }

    // --- Test 4: test_parse_missing_reserves ---
    #[test]
    fn test_parse_missing_reserves() {
        let json = r#"{
            "signature": "sig_no_reserves",
            "mint": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
            "txType": "buy",
            "solAmount": 1000000
        }"#;

        let payload: TradePayload = serde_json::from_str(json).unwrap();

        assert!(payload.v_tokens_in_bonding_curve.is_none());
        assert!(payload.v_sol_in_bonding_curve.is_none());
        assert!(payload.market_cap_sol.is_none());
    }

    // --- Test 5: test_parse_missing_initial_buy ---
    #[test]
    fn test_parse_missing_initial_buy() {
        let json = r#"{
            "signature": "sig_no_initial_buy",
            "mint": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
            "bondingCurveKey": "9Vi6i2o3ebHQku6Y9re6YmqzT8ebf3QTwbzyjempS7Ux",
            "txType": "create",
            "vTokensInBondingCurve": 1073000000.0,
            "vSolInBondingCurve": 30.0,
            "marketCapSol": 28.0
        }"#;

        let payload: NewTokenPayload = serde_json::from_str(json).unwrap();

        assert!(payload.initial_buy.is_none());
        assert!(payload.v_tokens_in_bonding_curve.is_some());
        assert!(payload.v_sol_in_bonding_curve.is_some());
        assert!(payload.market_cap_sol.is_some());
    }

    // --- Test 6: test_parse_zero_sol_amount ---
    #[test]
    fn test_parse_zero_sol_amount() {
        let json = r#"{
            "signature": "sig_zero",
            "mint": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
            "txType": "buy",
            "solAmount": 0,
            "tokenAmount": 0
        }"#;

        let payload: TradePayload = serde_json::from_str(json).unwrap();

        assert_eq!(payload.sol_amount, Some(0));
    }

    // --- Test 7: test_parse_mayhem_mode ---
    #[test]
    fn test_parse_mayhem_mode() {
        let json_trade = r#"{
            "signature": "sig_mayhem",
            "mint": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
            "txType": "buy",
            "solAmount": 1000000,
            "isMayhemMode": true
        }"#;

        let payload: TradePayload = serde_json::from_str(json_trade).unwrap();
        assert_eq!(payload.is_mayhem_mode, Some(true));

        let json_create = r#"{
            "signature": "sig_mayhem_create",
            "mint": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
            "txType": "create",
            "isMayhemMode": false
        }"#;

        let payload: NewTokenPayload = serde_json::from_str(json_create).unwrap();
        assert_eq!(payload.is_mayhem_mode, Some(false));
    }

    // --- Test 8: test_create_to_detected_pool ---
    #[tokio::test]
    async fn test_create_to_detected_pool() {
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let creator = Pubkey::new_unique();

        let event = PumpPortalEvent {
            data: json!({
                "signature": "sig_create_pool",
                "mint": mint.to_string(),
                "bondingCurveKey": bonding_curve.to_string(),
                "txType": "create",
                "traderPublicKey": creator.to_string(),
                "vTokensInBondingCurve": 1073000000.0,
                "vSolInBondingCurve": 30.0,
                "marketCapSol": 28.0,
                "initialBuy": 100
            }),
        };

        let mint_stats = Arc::new(Mutex::new(HashMap::new()));
        let active_mints = Arc::new(Mutex::new(VecDeque::new()));
        let config = PumpPortalConfig::default();
        let mut pending_subscriptions = Vec::new();
        let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID).unwrap();

        let result = PumpPortalConnection::handle_pumpportal_event(
            event,
            &mint_stats,
            &active_mints,
            &config,
            pumpfun_program_id,
            &mut pending_subscriptions,
        )
        .await
        .expect("create event should parse");

        assert!(!result.is_empty());

        if let Some(GeyserEvent::Transaction { accounts, .. }) = result.first() {
            // creator should be in accounts[5]
            assert_eq!(accounts[5], creator, "creator must be traderPublicKey");
            // base_mint should be in accounts[2]
            assert_eq!(accounts[2], mint, "base_mint must be mint from payload");
            // bonding_curve should be in accounts[4]
            assert_eq!(
                accounts[4], bonding_curve,
                "bonding_curve must match bondingCurveKey"
            );
        } else {
            panic!("Expected GeyserEvent::Transaction");
        }
    }

    // --- Test 9: test_buy_not_detected_pool ---
    #[tokio::test]
    async fn test_buy_not_detected_pool() {
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        let event = PumpPortalEvent {
            data: json!({
                "signature": "sig_buy_no_pool",
                "mint": mint.to_string(),
                "bondingCurveKey": bonding_curve.to_string(),
                "txType": "buy",
                "solAmount": 1000000,
                "tokenAmount": 5000
            }),
        };

        let mint_stats = Arc::new(Mutex::new(HashMap::new()));
        let active_mints = Arc::new(Mutex::new(VecDeque::new()));
        let config = PumpPortalConfig::default();
        let mut pending_subscriptions = Vec::new();
        let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID).unwrap();

        let result = PumpPortalConnection::handle_pumpportal_event(
            event,
            &mint_stats,
            &active_mints,
            &config,
            pumpfun_program_id,
            &mut pending_subscriptions,
        )
        .await
        .expect("buy event should parse");

        // Buy event for untracked mint should return None (no pool detection)
        assert!(result.is_empty());
        // No pending subscriptions should be added
        assert!(pending_subscriptions.is_empty());
    }

    // --- Test 10: test_create_to_pool_transaction ---
    #[tokio::test]
    async fn test_create_to_pool_transaction() {
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let creator = Pubkey::new_unique();

        let event = PumpPortalEvent {
            data: json!({
                "signature": "sig_create_tx",
                "mint": mint.to_string(),
                "bondingCurveKey": bonding_curve.to_string(),
                "txType": "create",
                "traderPublicKey": creator.to_string(),
                "solAmount": 0.5,
                "initialBuy": 17376518,
                "vTokensInBondingCurve": 1055623481.83,
                "vSolInBondingCurve": 30.49,
                "marketCapSol": 28.88
            }),
        };

        let mint_stats = Arc::new(Mutex::new(HashMap::new()));
        let active_mints = Arc::new(Mutex::new(VecDeque::new()));
        let config = PumpPortalConfig::default();
        let mut pending_subscriptions = Vec::new();
        let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID).unwrap();

        let result = PumpPortalConnection::handle_pumpportal_event(
            event,
            &mint_stats,
            &active_mints,
            &config,
            pumpfun_program_id,
            &mut pending_subscriptions,
        )
        .await
        .expect("create event should parse");
        let result = result.into_iter().next().expect("should return event");
        if let GeyserEvent::Transaction { instructions, .. } = &result {
            // Verify the pool_event synthetic payload
            let synthetic = instructions
                .iter()
                .find_map(|ix| bincode::deserialize::<SyntheticPayload>(&ix.data).ok());
            assert!(synthetic.is_some(), "Should contain SyntheticPayload");
            match synthetic.unwrap() {
                SyntheticPayload::InitializePool(pool) => {
                    assert_eq!(pool.base_mint, mint);
                    assert_eq!(pool.creator, creator);
                    assert_eq!(pool.bonding_curve, bonding_curve);
                    assert_eq!(pool.pool_amm_id, bonding_curve);
                }
                _ => panic!("Expected SyntheticPayload::InitializePool"),
            }
        } else {
            panic!("Expected GeyserEvent::Transaction");
        }
    }

    // --- Test 11: test_sol_amount_to_lamports_precision ---
    #[tokio::test]
    async fn test_sol_amount_to_lamports_precision() {
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        let mint_stats = Arc::new(Mutex::new(HashMap::new()));
        mint_stats
            .lock()
            .await
            .insert(mint, MintStats::new(mint, bonding_curve));

        let event = PumpPortalEvent {
            data: json!({
                "signature": "sig_precision",
                "mint": mint.to_string(),
                "bondingCurveKey": bonding_curve.to_string(),
                "txType": "buy",
                "solAmount": 0.493827159,
                "tokenAmount": 1000
            }),
        };

        let active_mints = Arc::new(Mutex::new(VecDeque::new()));
        let config = PumpPortalConfig::default();
        let mut pending_subscriptions = Vec::new();
        let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID).unwrap();

        let result = PumpPortalConnection::handle_pumpportal_event(
            event,
            &mint_stats,
            &active_mints,
            &config,
            pumpfun_program_id,
            &mut pending_subscriptions,
        )
        .await
        .expect("event should parse");
        let result = result.into_iter().last().expect("should return event");

        let trade = extract_trade_event(&result).expect("should contain TradeEvent");
        // solAmount 0.493827159 * 1e9 = 493827159 lamports
        assert_eq!(trade.max_sol_cost, 493827159);
    }

    // --- Test 12: test_initial_buy_to_token_units ---
    #[test]
    fn test_initial_buy_to_token_units() {
        // initialBuy field in NewTokenPayload is deserialized via deserialize_optional_float_to_u64
        let json = r#"{
            "signature": "sig_ib",
            "mint": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
            "txType": "create",
            "initialBuy": 17376518.16691
        }"#;

        let payload: NewTokenPayload = serde_json::from_str(json).unwrap();
        // initialBuy is stored as u64 — floor of the float value
        assert!(payload.initial_buy.is_some());
        let initial_buy = payload.initial_buy.unwrap();
        // The deserialize_optional_float_to_u64 converts the float to u64
        assert!(initial_buy > 0);
    }

    // --- Test 13: test_price_derived_from_reserves ---
    #[tokio::test]
    async fn test_price_derived_from_reserves() {
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        let mint_stats = Arc::new(Mutex::new(HashMap::new()));
        mint_stats
            .lock()
            .await
            .insert(mint, MintStats::new(mint, bonding_curve));

        let event = PumpPortalEvent {
            data: json!({
                "signature": "sig_price",
                "mint": mint.to_string(),
                "bondingCurveKey": bonding_curve.to_string(),
                "txType": "buy",
                "solAmount": 1000000,
                "tokenAmount": 5000,
                "vTokensInBondingCurve": 1055623481.83,
                "vSolInBondingCurve": 30.49
            }),
        };

        let active_mints = Arc::new(Mutex::new(VecDeque::new()));
        let config = PumpPortalConfig::default();
        let mut pending_subscriptions = Vec::new();
        let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID).unwrap();

        let result = PumpPortalConnection::handle_pumpportal_event(
            event,
            &mint_stats,
            &active_mints,
            &config,
            pumpfun_program_id,
            &mut pending_subscriptions,
        )
        .await
        .expect("event should parse");
        let result = result.into_iter().last().expect("should return event");

        let trade = extract_trade_event(&result).expect("should contain TradeEvent");

        // Verify reserves are carried through
        assert!(trade.v_tokens_in_bonding_curve.is_some());
        assert!(trade.v_sol_in_bonding_curve.is_some());
        assert!((trade.v_tokens_in_bonding_curve.unwrap() - 1055623481.83).abs() < 1e-1); // large value, lower precision
        assert!((trade.v_sol_in_bonding_curve.unwrap() - 30.49).abs() < 1e-1);
    }

    // --- Test 14: test_price_none_when_no_reserves ---
    #[tokio::test]
    async fn test_price_none_when_no_reserves() {
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        let mint_stats = Arc::new(Mutex::new(HashMap::new()));
        mint_stats
            .lock()
            .await
            .insert(mint, MintStats::new(mint, bonding_curve));

        let event = PumpPortalEvent {
            data: json!({
                "signature": "sig_no_reserves_trade",
                "mint": mint.to_string(),
                "bondingCurveKey": bonding_curve.to_string(),
                "txType": "buy",
                "solAmount": 1000000,
                "tokenAmount": 5000
            }),
        };

        let active_mints = Arc::new(Mutex::new(VecDeque::new()));
        let config = PumpPortalConfig::default();
        let mut pending_subscriptions = Vec::new();
        let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID).unwrap();

        let result = PumpPortalConnection::handle_pumpportal_event(
            event,
            &mint_stats,
            &active_mints,
            &config,
            pumpfun_program_id,
            &mut pending_subscriptions,
        )
        .await
        .expect("event should parse");
        let result = result.into_iter().last().expect("should return event");

        let trade = extract_trade_event(&result).expect("should contain TradeEvent");

        assert!(trade.v_tokens_in_bonding_curve.is_none());
        assert!(trade.v_sol_in_bonding_curve.is_none());
        assert!(trade.market_cap_sol.is_none());
    }

    // --- Test 15: test_price_none_when_zero_tokens ---
    #[tokio::test]
    async fn test_price_none_when_zero_tokens() {
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        let mint_stats = Arc::new(Mutex::new(HashMap::new()));
        mint_stats
            .lock()
            .await
            .insert(mint, MintStats::new(mint, bonding_curve));

        let event = PumpPortalEvent {
            data: json!({
                "signature": "sig_zero_tokens_trade",
                "mint": mint.to_string(),
                "bondingCurveKey": bonding_curve.to_string(),
                "txType": "buy",
                "solAmount": 1000000,
                "tokenAmount": 5000,
                "vTokensInBondingCurve": 0.0,
                "vSolInBondingCurve": 30.0
            }),
        };

        let active_mints = Arc::new(Mutex::new(VecDeque::new()));
        let config = PumpPortalConfig::default();
        let mut pending_subscriptions = Vec::new();
        let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID).unwrap();

        let result = PumpPortalConnection::handle_pumpportal_event(
            event,
            &mint_stats,
            &active_mints,
            &config,
            pumpfun_program_id,
            &mut pending_subscriptions,
        )
        .await
        .expect("event should parse");
        let result = result.into_iter().last().expect("should return event");

        let trade = extract_trade_event(&result).expect("should contain TradeEvent");

        // vTokens = 0.0, so division by zero guard should prevent price computation
        // The reserves should still be present
        assert_eq!(trade.v_tokens_in_bonding_curve, Some(0.0));
        assert!(trade.v_sol_in_bonding_curve.is_some());
    }

    // --- Test 16: test_create_emits_both_events ---
    #[tokio::test]
    async fn test_create_emits_both_events() {
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        let event = PumpPortalEvent {
            data: json!({
                "signature": "sig_both",
                "mint": mint.to_string(),
                "bondingCurveKey": bonding_curve.to_string(),
                "txType": "create",
                "traderPublicKey": Pubkey::new_unique().to_string(),
                "vTokensInBondingCurve": 1073000000.0,
                "vSolInBondingCurve": 30.0,
                "marketCapSol": 28.0
            }),
        };

        let mint_stats = Arc::new(Mutex::new(HashMap::new()));
        let active_mints = Arc::new(Mutex::new(VecDeque::new()));
        let config = PumpPortalConfig::default();
        let mut pending_subscriptions = Vec::new();
        let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID).unwrap();

        let result = PumpPortalConnection::handle_pumpportal_event(
            event,
            &mint_stats,
            &active_mints,
            &config,
            pumpfun_program_id,
            &mut pending_subscriptions,
        )
        .await
        .expect("create event should parse");

        // Create event should return BOTH InitializePool AND TradeEvent
        assert_eq!(
            result.len(),
            2,
            "Create event should emit 2 events (InitializePool + TradeEvent)"
        );

        // First event should be InitializePool
        let ge = result.first().expect("should have at least one event");
        match ge {
            GeyserEvent::Transaction { instructions, .. } => {
                let synthetic = instructions
                    .iter()
                    .find_map(|ix| bincode::deserialize::<SyntheticPayload>(&ix.data).ok());
                assert!(
                    matches!(synthetic, Some(SyntheticPayload::InitializePool(_))),
                    "First event should be InitializePool"
                );
            }
            _ => panic!("Expected GeyserEvent::Transaction"),
        }

        // Second event should be Trade (dev buy)
        let trade_ge = result.last().expect("should have second event");
        match trade_ge {
            GeyserEvent::Transaction { instructions, .. } => {
                let synthetic = instructions
                    .iter()
                    .find_map(|ix| bincode::deserialize::<SyntheticPayload>(&ix.data).ok());
                match synthetic {
                    Some(SyntheticPayload::Trade(trade)) => {
                        assert!(trade.is_buy, "Dev buy should be is_buy=true");
                        assert!(trade.is_dev_buy, "Dev buy should have is_dev_buy=true");
                    }
                    _ => panic!("Second event should be SyntheticPayload::Trade"),
                }
            }
            _ => panic!("Expected GeyserEvent::Transaction"),
        }
    }

    // --- Test 17: test_create_pool_detected_before_tx ---
    #[tokio::test]
    async fn test_create_pool_detected_before_tx() {
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let creator = Pubkey::new_unique();

        let event = PumpPortalEvent {
            data: json!({
                "signature": "sig_order",
                "mint": mint.to_string(),
                "bondingCurveKey": bonding_curve.to_string(),
                "txType": "create",
                "traderPublicKey": creator.to_string(),
                "vSolInBondingCurve": 30.0,
                "vTokensInBondingCurve": 1073000000.0
            }),
        };

        let mint_stats = Arc::new(Mutex::new(HashMap::new()));
        let active_mints = Arc::new(Mutex::new(VecDeque::new()));
        let config = PumpPortalConfig::default();
        let mut pending_subscriptions = Vec::new();
        let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID).unwrap();

        let result = PumpPortalConnection::handle_pumpportal_event(
            event,
            &mint_stats,
            &active_mints,
            &config,
            pumpfun_program_id,
            &mut pending_subscriptions,
        )
        .await
        .expect("create event should parse");
        let result = result.into_iter().next().expect("should return event");

        if let GeyserEvent::Transaction { instructions, .. } = &result {
            let synthetic = instructions
                .iter()
                .find_map(|ix| bincode::deserialize::<SyntheticPayload>(&ix.data).ok());
            match synthetic {
                Some(SyntheticPayload::InitializePool(pool)) => {
                    assert_eq!(pool.pool_amm_id, bonding_curve);
                    assert_eq!(pool.base_mint, mint);
                    assert_eq!(pool.creator, creator);
                }
                _ => panic!("Expected SyntheticPayload::InitializePool"),
            }
        } else {
            panic!("Expected GeyserEvent::Transaction");
        }
    }

    // --- Test 18: test_buy_emits_only_tx ---
    #[tokio::test]
    async fn test_buy_emits_only_tx() {
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        let mint_stats = Arc::new(Mutex::new(HashMap::new()));
        mint_stats
            .lock()
            .await
            .insert(mint, MintStats::new(mint, bonding_curve));

        let event = PumpPortalEvent {
            data: json!({
                "signature": "sig_buy_only",
                "mint": mint.to_string(),
                "bondingCurveKey": bonding_curve.to_string(),
                "txType": "buy",
                "solAmount": 1000000,
                "tokenAmount": 5000,
                "vTokensInBondingCurve": 1000000000.0,
                "vSolInBondingCurve": 31.0,
                "marketCapSol": 30.0
            }),
        };

        let active_mints = Arc::new(Mutex::new(VecDeque::new()));
        let config = PumpPortalConfig::default();
        let mut pending_subscriptions = Vec::new();
        let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID).unwrap();

        let result = PumpPortalConnection::handle_pumpportal_event(
            event,
            &mint_stats,
            &active_mints,
            &config,
            pumpfun_program_id,
            &mut pending_subscriptions,
        )
        .await
        .expect("buy event should parse");
        let result = result.into_iter().last().expect("should return event");

        // Buy event should produce a Trade (not InitializePool)
        let trade = extract_trade_event(&result);
        assert!(
            trade.is_some(),
            "Buy should produce SyntheticPayload::Trade"
        );
        assert!(trade.unwrap().is_buy);
    }

    // --- Test 19: test_sell_emits_only_tx ---
    #[tokio::test]
    async fn test_sell_emits_only_tx() {
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        let mint_stats = Arc::new(Mutex::new(HashMap::new()));
        mint_stats
            .lock()
            .await
            .insert(mint, MintStats::new(mint, bonding_curve));

        let event = PumpPortalEvent {
            data: json!({
                "signature": "sig_sell_only",
                "mint": mint.to_string(),
                "bondingCurveKey": bonding_curve.to_string(),
                "txType": "sell",
                "solAmount": 500000,
                "tokenAmount": 2000,
                "vTokensInBondingCurve": 1010000000.0,
                "vSolInBondingCurve": 29.5
            }),
        };

        let active_mints = Arc::new(Mutex::new(VecDeque::new()));
        let config = PumpPortalConfig::default();
        let mut pending_subscriptions = Vec::new();
        let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID).unwrap();

        let result = PumpPortalConnection::handle_pumpportal_event(
            event,
            &mint_stats,
            &active_mints,
            &config,
            pumpfun_program_id,
            &mut pending_subscriptions,
        )
        .await
        .expect("sell event should parse");
        let result = result.into_iter().last().expect("should return event");

        // Sell event should produce a Trade (not InitializePool)
        let trade = extract_trade_event(&result);
        assert!(
            trade.is_some(),
            "Sell should produce SyntheticPayload::Trade"
        );
        assert!(!trade.unwrap().is_buy);
    }

    // --- Test 20: test_real_pumpportal_json_roundtrip ---
    #[test]
    fn test_real_pumpportal_json_roundtrip() {
        let raw_json = r#"{
            "signature":"4uVnTxFDXzMbSFS5e6b5nydMsM2LXUZsdvQBaucnZ9k9LQyz7FjrzfFDq8MGAaabJZa4csNz883ATngq4HmBW4ij",
            "mint":"658ZHoFdR6e9V8ie9RhUpgaND6NWK6dF7vjr7WF3pump",
            "traderPublicKey":"DVKg1YfyxNzvQuccJ2HfDcAXh3TcFouccBLjnfy4aehT",
            "txType":"create",
            "initialBuy":17376518,
            "solAmount":0.493827159,
            "bondingCurveKey":"9Vi6i2o3ebHQku6Y9re6YmqzT8ebf3QTwbzyjempS7Ux",
            "vTokensInBondingCurve":1055623481.83309,
            "vSolInBondingCurve":30.49382715899998,
            "marketCapSol":28.88702997213311,
            "name":"ArmouredJesus",
            "symbol":"JESUS",
            "uri":"https://ipfs.io/ipfs/bafkreichfxaudppdevygnbxk6qxxcpjczpshn7n4xchknytgrjyuweyqjq",
            "isMayhemMode":false,
            "pool":"pump"
        }"#;

        // Test NewTokenPayload deserialization (since txType=create routes here)
        let msg: NewTokenPayload = serde_json::from_str(raw_json).unwrap();

        assert_eq!(msg.signature, "4uVnTxFDXzMbSFS5e6b5nydMsM2LXUZsdvQBaucnZ9k9LQyz7FjrzfFDq8MGAaabJZa4csNz883ATngq4HmBW4ij");
        assert_eq!(msg.tx_type, Some("create".to_string()));
        assert_eq!(
            msg.trader_public_key,
            Some("DVKg1YfyxNzvQuccJ2HfDcAXh3TcFouccBLjnfy4aehT".to_string())
        );
        assert!((msg.sol_amount.unwrap() - 0.493827159).abs() < 1e-10);
        assert!((msg.v_tokens_in_bonding_curve.unwrap() - 1055623481.83309).abs() < 1e-3); // large value, lower precision
        assert!((msg.v_sol_in_bonding_curve.unwrap() - 30.49382715899998).abs() < 1e-10);
        assert!((msg.market_cap_sol.unwrap() - 28.88702997213311).abs() < 1e-10);
        assert!(msg.initial_buy.is_some());
        assert_eq!(msg.is_mayhem_mode, Some(false));
        assert_eq!(msg.name, Some("ArmouredJesus".to_string()));
        assert_eq!(msg.symbol, Some("JESUS".to_string()));
        assert_eq!(msg.pool, Some("pump".to_string()));
        assert_eq!(
            msg.bonding_curve,
            Some("9Vi6i2o3ebHQku6Y9re6YmqzT8ebf3QTwbzyjempS7Ux".to_string())
        );

        // Also test as TradePayload (since the JSON can be parsed as either)
        let trade_msg: TradePayload = serde_json::from_str(raw_json).unwrap();
        assert!((trade_msg.v_tokens_in_bonding_curve.unwrap() - 1055623481.83309).abs() < 1e-3); // large value, lower precision
        assert!((trade_msg.v_sol_in_bonding_curve.unwrap() - 30.49382715899998).abs() < 1e-10);
        assert!((trade_msg.market_cap_sol.unwrap() - 28.88702997213311).abs() < 1e-10);
        assert_eq!(trade_msg.is_mayhem_mode, Some(false));
    }
}
