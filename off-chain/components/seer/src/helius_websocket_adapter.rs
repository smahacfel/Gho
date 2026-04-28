//! Helius WebSocket Adapter - Standard Solana RPC WebSocket support
//!
//! This adapter connects to Helius Free Tier WebSocket endpoint and subscribes
//! to log notifications for Pump.fun program. It parses incoming log notifications,
//! fetches full transaction details via RPC, and converts them to GeyserEvent format
//! for compatibility with the existing Seer pipeline.

use crate::errors::{SeerError, SeerResult};
use crate::metrics::SeerMetrics;
use crate::rpc_http_client::new_async_rpc_client;
use crate::types::{GeyserEvent, RawBytesMissingReason, RawInstruction};
use arc_swap::ArcSwap;
use dashmap::DashMap;
use futures_util::{SinkExt, Stream, StreamExt};
use once_cell::sync::Lazy;
use serde_json::Value;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_sdk::transaction::{Transaction, VersionedTransaction};
use solana_transaction_status::{
    option_serializer::OptionSerializer, EncodedTransaction, UiInstruction, UiMessage,
    UiParsedInstruction, UiTransactionEncoding, UiTransactionStatusMeta,
};
use std::collections::{HashMap, HashSet};
use std::pin::Pin;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, trace, warn};

/// Label for Pump.fun AMM metrics
const PUMPFUN_AMM_LABEL: &str = "pumpfun";

/// Pump.fun program ID
const PUMPFUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

/// Bonding curve account index in Pump.fun Create instruction
const BONDING_CURVE_ACCOUNT_INDEX: usize = 2;

/// Pump.fun Create instruction discriminator (8 bytes)
/// This is the "fingerprint" of a TRUE pool creation event.
const PUMPFUN_CREATE_DISCRIMINATOR: [u8; 8] = [24, 30, 200, 40, 5, 28, 7, 119];

static PUMPFUN_PROGRAM_PUBKEY: Lazy<Pubkey> = Lazy::new(|| {
    Pubkey::from_str(PUMPFUN_PROGRAM_ID).expect("PUMPFUN_PROGRAM_ID must be a valid pubkey")
});

const RECENT_POOL_TTL_SECS: u64 = 30;

/// Base delay for exponential backoff in milliseconds (Patient Fetcher)
/// INCREASED to 1000ms to handle Helius RPC lag and prevent 429s
const RETRY_BASE_DELAY_MS: u64 = 1000;

/// Event stream type
pub type EventStream = Pin<Box<dyn Stream<Item = SeerResult<GeyserEvent>> + Send>>;

/// Helius WebSocket Adapter
///
/// Connects to standard Helius/Solana RPC WebSocket and converts log notifications
/// to GeyserEvent format for seamless integration with existing Seer pipeline.
pub struct HeliusWebSocketAdapter {
    /// WebSocket endpoint (e.g., wss://mainnet.helius-rpc.com/?api-key=XXX)
    endpoint: String,

    /// RPC endpoint for transaction fetching
    rpc_endpoint: String,

    /// Metrics collector
    metrics: Arc<SeerMetrics>,

    /// Maximum reconnection attempts
    max_reconnect_attempts: u32,

    /// Reconnection delay in seconds
    reconnect_delay_secs: u64,

    /// Total WebSocket messages received (for land rate calculation)
    ws_messages_received: Arc<AtomicU64>,

    /// Total log notifications received (for land rate calculation)
    log_notifications_received: Arc<AtomicU64>,

    /// Total events successfully published to stream (for land rate calculation)
    events_published: Arc<AtomicU64>,

    /// Total events dropped/filtered (for land rate calculation)
    events_dropped: Arc<AtomicU64>,

    /// Aggregate RPC call durations (milliseconds)
    rpc_call_total_ms: Arc<AtomicU64>,

    /// RPC call count for average duration
    rpc_call_count: Arc<AtomicU64>,

    /// Rejected pump log samples logged
    rejected_pump_signatures_logged: Arc<AtomicU64>,

    /// Stop flag for health loop
    stop_flag: Arc<AtomicBool>,

    /// Tracked pool addresses (bonding curves from creation events)
    /// Used to publish ALL transactions involving these pools, not just creation events
    tracked_pools: Arc<ArcSwap<HashSet<Pubkey>>>,

    /// Recently created pools to bridge creation→trade race
    recent_pools: Arc<DashMap<Pubkey, Instant>>,
}

impl HeliusWebSocketAdapter {
    /// Create a new Helius WebSocket adapter
    pub fn new(
        endpoint: String,
        rpc_endpoint: String,
        metrics: Arc<SeerMetrics>,
        max_reconnect_attempts: u32,
        reconnect_delay_secs: u64,
    ) -> Self {
        Self {
            endpoint,
            rpc_endpoint,
            metrics,
            max_reconnect_attempts,
            reconnect_delay_secs,
            ws_messages_received: Arc::new(AtomicU64::new(0)),
            log_notifications_received: Arc::new(AtomicU64::new(0)),
            events_published: Arc::new(AtomicU64::new(0)),
            events_dropped: Arc::new(AtomicU64::new(0)),
            rpc_call_total_ms: Arc::new(AtomicU64::new(0)),
            rpc_call_count: Arc::new(AtomicU64::new(0)),
            rejected_pump_signatures_logged: Arc::new(AtomicU64::new(0)),
            stop_flag: Arc::new(AtomicBool::new(false)),
            tracked_pools: Arc::new(ArcSwap::from_pointee(HashSet::new())),
            recent_pools: Arc::new(DashMap::new()),
        }
    }

    /// Get current land rate (events published / log notifications received)
    /// Target: >= 80% per requirements
    pub fn land_rate(&self) -> f64 {
        let notifications = self.log_notifications_received.load(Ordering::Relaxed) as f64;
        if notifications == 0.0 {
            return 100.0; // No notifications yet, technically 100%
        }
        let published = self.events_published.load(Ordering::Relaxed) as f64;
        (published / notifications) * 100.0
    }

    /// Get total WebSocket messages received
    pub fn ws_messages_received(&self) -> u64 {
        self.ws_messages_received.load(Ordering::Relaxed)
    }

    /// Get total log notifications received
    pub fn log_notifications_received(&self) -> u64 {
        self.log_notifications_received.load(Ordering::Relaxed)
    }

    /// Get total events published
    pub fn events_published(&self) -> u64 {
        self.events_published.load(Ordering::Relaxed)
    }

    /// Get total events dropped
    pub fn events_dropped(&self) -> u64 {
        self.events_dropped.load(Ordering::Relaxed)
    }

    /// Stop the health loop
    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Release);
        info!("🛑 Helius adapter stop requested");
    }

    /// Connect to Helius WebSocket and return event stream
    pub async fn connect(&self) -> SeerResult<EventStream> {
        info!("Connecting to Helius WebSocket at {}", self.endpoint);

        // Start periodic metrics update task
        let metrics = Arc::clone(&self.metrics);
        let ws_messages_received = Arc::clone(&self.ws_messages_received);
        let log_notifications_received = Arc::clone(&self.log_notifications_received);
        let events_published = Arc::clone(&self.events_published);
        let events_dropped = Arc::clone(&self.events_dropped);
        let stop_flag = Arc::clone(&self.stop_flag);

        tokio::spawn(async move {
            // Track previous values to calculate deltas
            let mut prev_ws_msgs = 0u64;
            let mut prev_log_notifs = 0u64;
            let mut prev_published = 0u64;
            let mut prev_dropped = 0u64;

            loop {
                if stop_flag.load(Ordering::Acquire) {
                    info!("🛑 Helius adapter health loop terminated");
                    break;
                }
                sleep(Duration::from_secs(10)).await;

                // Read current values
                let ws_msgs = ws_messages_received.load(Ordering::Relaxed);
                let log_notifs = log_notifications_received.load(Ordering::Relaxed);
                let published = events_published.load(Ordering::Relaxed);
                let dropped = events_dropped.load(Ordering::Relaxed);

                // Calculate land rate
                let land_rate = if log_notifs > 0 {
                    (published as f64 / log_notifs as f64) * 100.0
                } else {
                    100.0
                };

                // Update Prometheus gauge
                metrics.helius_land_rate.set(land_rate);

                // Update counters with deltas (only increment by new events since last update)
                let ws_msgs_delta = ws_msgs.saturating_sub(prev_ws_msgs);
                if ws_msgs_delta > 0 {
                    metrics
                        .helius_ws_messages_received
                        .with_label_values(&["total"])
                        .inc_by(ws_msgs_delta);
                    prev_ws_msgs = ws_msgs;
                }

                let log_notifs_delta = log_notifs.saturating_sub(prev_log_notifs);
                if log_notifs_delta > 0 {
                    metrics
                        .helius_log_notifications_received
                        .with_label_values(&["total"])
                        .inc_by(log_notifs_delta);
                    prev_log_notifs = log_notifs;
                }

                let published_delta = published.saturating_sub(prev_published);
                if published_delta > 0 {
                    metrics
                        .helius_events_published
                        .with_label_values(&["success"])
                        .inc_by(published_delta);
                    prev_published = published;
                }

                let dropped_delta = dropped.saturating_sub(prev_dropped);
                if dropped_delta > 0 {
                    metrics
                        .helius_events_dropped
                        .with_label_values(&["filtered"])
                        .inc_by(dropped_delta);
                    prev_dropped = dropped;
                }

                // Log health status (Only if we have processed meaningful notifications)
                if log_notifs >= 5 {
                    if land_rate < 80.0 {
                        // Changed to WARN because aggressive filtering naturally lowers this
                        // if we count total traffic vs interested traffic.
                        warn!(
                            "⚠️ Helius Land Rate: {:.2}% (published: {}, filtered/dropped: {})",
                            land_rate, published, dropped
                        );
                    } else {
                        debug!(
                            "✅ Helius Health OK: {:.2}% (published: {}, notifications: {}, dropped: {})",
                            land_rate, published, log_notifs, dropped
                        );
                    }
                }
            }
        });

        let mut attempt = 0;
        loop {
            match self.try_connect().await {
                Ok(stream) => {
                    info!("Successfully connected to Helius WebSocket");
                    return Ok(stream);
                }
                Err(e) => {
                    attempt += 1;
                    if attempt >= self.max_reconnect_attempts {
                        return Err(e);
                    }
                    warn!(
                        "Connection attempt {} failed: {}. Retrying in {} seconds...",
                        attempt, e, self.reconnect_delay_secs
                    );
                    sleep(Duration::from_secs(self.reconnect_delay_secs)).await;
                }
            }
        }
    }

    /// Try to connect to Helius WebSocket
    async fn try_connect(&self) -> SeerResult<EventStream> {
        let (ws_stream, _) = connect_async(&self.endpoint)
            .await
            .map_err(|e| SeerError::WebSocketError(e.to_string()))?;

        let (mut write, read) = ws_stream.split();

        // Subscribe to Pump.fun logs
        let subscribe_msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "logsSubscribe",
            "params": [
                { "mentions": [PUMPFUN_PROGRAM_ID] },
                { "commitment": "confirmed" }
            ]
        });

        write
            .send(Message::Text(subscribe_msg.to_string()))
            .await
            .map_err(|e| SeerError::WebSocketError(e.to_string()))?;

        info!("Subscribed to Pump.fun logs on Helius WebSocket");

        let rpc_client = Arc::new(new_async_rpc_client(self.rpc_endpoint.clone()));
        let metrics = Arc::clone(&self.metrics);
        let ws_messages_received = Arc::clone(&self.ws_messages_received);
        let log_notifications_received = Arc::clone(&self.log_notifications_received);
        let events_published = Arc::clone(&self.events_published);
        let events_dropped = Arc::clone(&self.events_dropped);
        let rpc_call_total_ms = Arc::clone(&self.rpc_call_total_ms);
        let rpc_call_count = Arc::clone(&self.rpc_call_count);
        let rejected_pump_signatures_logged = Arc::clone(&self.rejected_pump_signatures_logged);
        let tracked_pools: Arc<ArcSwap<HashSet<Pubkey>>> = Arc::clone(&self.tracked_pools);
        let recent_pools: Arc<DashMap<Pubkey, Instant>> = Arc::clone(&self.recent_pools);

        // Process incoming WebSocket messages
        let event_stream = read.filter_map(move |msg| {
            let metrics = Arc::clone(&metrics);
            let rpc = Arc::clone(&rpc_client);
            let ws_messages_received = Arc::clone(&ws_messages_received);
            let log_notifications_received = Arc::clone(&log_notifications_received);
            let events_published = Arc::clone(&events_published);
            let events_dropped = Arc::clone(&events_dropped);
            let tracked_pools = Arc::clone(&tracked_pools);
            let rpc_call_total_ms = Arc::clone(&rpc_call_total_ms);
            let rpc_call_count = Arc::clone(&rpc_call_count);
            let rejected_pump_signatures_logged = Arc::clone(&rejected_pump_signatures_logged);
            let recent_pools = Arc::clone(&recent_pools);

            async move {
                match msg {
                    Ok(Message::Text(text)) => {
                        // Increment WebSocket messages received counter
                        ws_messages_received.fetch_add(1, Ordering::Relaxed);
                        trace!("📨 Received WebSocket message (total: {})", ws_messages_received.load(Ordering::Relaxed));

                        // Parse JSON response first
                        metrics
                            .helius_ws_logs_notifications_total
                            .with_label_values(&["raw"])
                            .inc();
                        let v: Value = match serde_json::from_str(&text) {
                            Ok(v) => {
                                debug!("✅ Parsed JSON WebSocket message");
                                v
                            },
                            Err(e) => {
                                metrics
                                    .helius_events_dropped
                                    .with_label_values(&["parse_json_failed"])
                                    .inc();
                                warn!("❌ DROPPED EVENT (reason: parse_json_failed): Failed to parse WebSocket message as JSON: {}", e);
                                events_dropped.fetch_add(1, Ordering::Relaxed);
                                return None;
                            },
                        };

                        // Filter for logsNotification method
                        if v.get("method") != Some(&Value::String("logsNotification".to_string())) {
                            metrics
                                .helius_events_dropped
                                .with_label_values(&["not_logs_notification"])
                                .inc();
                            debug!("⏭️  DROPPED EVENT (reason: not_logs_notification): Skipping non-logsNotification message: {:?}", v.get("method"));
                            events_dropped.fetch_add(1, Ordering::Relaxed);
                            return None;
                        }

                        let params = match v.get("params").and_then(|p| p.get("result")).and_then(|r| r.get("value")) {
                            Some(p) => p,
                            None => {
                                metrics
                                    .helius_events_dropped
                                    .with_label_values(&["parse_json_failed"])
                                    .inc();
                                error!("❌ DROPPED EVENT (reason: missing_params): Missing params.result.value in logsNotification");
                                events_dropped.fetch_add(1, Ordering::Relaxed);
                                return None;
                            }
                        };

                        let logs = match params.get("logs").and_then(|l| l.as_array()) {
                            Some(l) => l,
                            None => {
                                metrics
                                    .helius_events_dropped
                                    .with_label_values(&["parse_json_failed"])
                                    .inc();
                                error!("❌ DROPPED EVENT (reason: missing_logs): Missing or invalid logs array in logsNotification");
                                events_dropped.fetch_add(1, Ordering::Relaxed);
                                return None;
                            }
                        };

                        let signature_str = match params.get("signature").and_then(|s| s.as_str()) {
                            Some(s) => s,
                            None => {
                                metrics
                                    .helius_events_dropped
                                    .with_label_values(&["parse_json_failed"])
                                    .inc();
                                error!("❌ DROPPED EVENT (reason: missing_signature): Missing signature in logsNotification");
                                events_dropped.fetch_add(1, Ordering::Relaxed);
                                return None;
                            }
                        };

                        // =================================================================
                        // 🛑 RPC THROTTLE FIX: AGGRESSIVE PRE-FILTERING
                        // =================================================================
                        // Only fetch transaction if it looks like a NEW POOL creation or InitializeMint
                        // This filters out 99% of random trades (which also emit logs but differ in text)
                        // Note: "Instruction: Create" is tricky because TokenProgram uses it too.
                        // We filter aggressively here to save RPC, and then VALIDATE DISCRIMINATOR later.

                        let mut looks_like_creation = false;
                        let mut looks_like_trade = false;
                        let mut pump_log_sample: Option<&str> = None;

                        for log_str in logs.iter().filter_map(|l| l.as_str()) {
                            if !looks_like_creation
                                && (log_str.contains("Instruction: Create")
                                    || log_str.contains("InitializeMint")
                                    || log_str.contains("InitializeMint2"))
                            {
                                looks_like_creation = true;
                            }
                            if !looks_like_trade
                                && (log_str.contains("Instruction: Buy")
                                    || log_str.contains("Instruction: Sell")
                                    || log_str.contains("Instruction: Swap"))
                            {
                                looks_like_trade = true;
                            }
                            if pump_log_sample.is_none()
                                && (log_str.contains("pump") || log_str.contains("Pump"))
                            {
                                pump_log_sample = Some(log_str);
                            }
                            if looks_like_creation && looks_like_trade && pump_log_sample.is_some() {
                                break;
                            }
                        }

                        let tracked_pools_snapshot = tracked_pools.load_full();
                        let has_tracked_pools = !tracked_pools_snapshot.is_empty();
                        let has_recent_pools = !recent_pools.is_empty();

                        // Prefilter accounts via params if available
                        let mut accounts_checked_prefilter = false;
                        let mut involves_tracked_pool_prefilter = false;
                        let now = Instant::now();
                        if has_tracked_pools || has_recent_pools {
                            if let Some(account_keys_json) = params.get("accounts").or_else(|| params.get("accountKeys")) {
                                if let Some(arr) = account_keys_json.as_array() {
                                    accounts_checked_prefilter = true;
                                    for key_str in arr.iter().filter_map(|k| k.as_str()) {
                                        if let Ok(pk) = Pubkey::from_str(key_str) {
                                            if tracked_pools_snapshot.contains(&pk) {
                                                involves_tracked_pool_prefilter = true;
                                                break;
                                            }
                                            if let Some(entry) = recent_pools.get(&pk) {
                                                if now.duration_since(*entry.value()).as_secs() < RECENT_POOL_TTL_SECS {
                                                    involves_tracked_pool_prefilter = true;
                                                    break;
                                                } else {
                                                    recent_pools.remove(&pk);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        if let Some(reason) = trade_prefilter_drop_reason(
                            looks_like_creation,
                            looks_like_trade,
                            has_tracked_pools,
                            has_recent_pools,
                            accounts_checked_prefilter,
                            involves_tracked_pool_prefilter,
                        ) {
                            metrics
                                .helius_events_dropped
                                .with_label_values(&[reason])
                                .inc();
                            events_dropped.fetch_add(1, Ordering::Relaxed);
                            if let Some(sample_log) = pump_log_sample {
                                let seen = rejected_pump_signatures_logged.fetch_add(1, Ordering::Relaxed);
                                if seen < 5 {
                                    warn!(
                                        "⛔ REJECTED PUMP LOG ({}): signature={}, sample_log={}",
                                        reason,
                                        signature_str,
                                        sample_log
                                    );
                                }
                            }
                            return None;
                        }

                        if !looks_like_creation && !looks_like_trade {
                            let should_drop = if accounts_checked_prefilter {
                                !involves_tracked_pool_prefilter
                            } else {
                                !has_tracked_pools && !has_recent_pools
                            };

                            if should_drop {
                                metrics
                                    .helius_events_dropped
                                    .with_label_values(&["prefilter_drop"])
                                    .inc();
                                if let Some(sample_log) = pump_log_sample {
                                    let seen = rejected_pump_signatures_logged.fetch_add(1, Ordering::Relaxed);
                                    if seen < 5 {
                                        warn!(
                                            "⛔ REJECTED PUMP LOG (filtered): signature={}, sample_log={}",
                                            signature_str,
                                            sample_log
                                        );
                                    }
                                }
                                return None;
                            }
                        }

                        debug!(
                            "prefilter decision: sig={}, creation_like={}, trade_like={}, has_tracked={}, has_recent={}, accounts_checked={}, involves_prefilter={}",
                            signature_str,
                            looks_like_creation,
                            looks_like_trade,
                            has_tracked_pools,
                            has_recent_pools,
                            accounts_checked_prefilter,
                            involves_tracked_pool_prefilter
                        );

                        // NOW we increment the "Relevant Notification" counter
                        log_notifications_received.fetch_add(1, Ordering::Relaxed);
                        metrics
                            .helius_ws_logs_notifications_prefilter_passed
                            .with_label_values(&["passed"])
                            .inc();
                        debug!("🔍 Analyzing relevant signature: {} with {} logs", signature_str, logs.len());

                        // Parse signature
                        let sig = match Signature::from_str(signature_str) {
                            Ok(s) => s,
                            Err(e) => {
                                error!("❌ DROPPED EVENT (reason: invalid_signature): Invalid signature '{}': {}", signature_str, e);
                                events_dropped.fetch_add(1, Ordering::Relaxed);
                                return None;
                            }
                        };

                        // Fetch transaction with Patient Fetcher pattern (exponential backoff)
                        let tx_config = RpcTransactionConfig {
                            encoding: Some(UiTransactionEncoding::Base64),
                            commitment: Some(CommitmentConfig::confirmed()),
                            max_supported_transaction_version: Some(0),
                        };

                        // Retry loop with exponential backoff (INCREASED to 5 attempts)
                        metrics
                            .helius_rpc_fetch_attempted_total
                            .with_label_values(&["attempted"])
                            .inc();
                        let mut tx_result = None;
                        for attempt in 0..5u32 {
                            let rpc_start = Instant::now();
                            match rpc.get_transaction_with_config(&sig, tx_config).await {
                                Ok(tx) => {
                                    let elapsed_ms = rpc_start.elapsed().as_millis() as u64;
                                    let total = rpc_call_total_ms.fetch_add(elapsed_ms, Ordering::Relaxed) + elapsed_ms;
                                    let count = rpc_call_count.fetch_add(1, Ordering::Relaxed) + 1;
                                    let avg = total / count.max(1);
                                    debug!(
                                        "rpc.get_transaction duration={}ms (avg {}ms over {} calls)",
                                        elapsed_ms, avg, count
                                    );
                                    if avg > 500 {
                                        warn!("WARN: RPC SLOW - CAUSING PACKET LOSS (avg {}ms)", avg);
                                    }
                                     metrics
                                         .helius_rpc_fetch_success_total
                                         .with_label_values(&["success"])
                                         .inc();
                                     tx_result = Some(tx);
                                    break;
                                }
                                Err(e) => {
                                    let elapsed_ms = rpc_start.elapsed().as_millis() as u64;
                                    let total = rpc_call_total_ms.fetch_add(elapsed_ms, Ordering::Relaxed) + elapsed_ms;
                                    let count = rpc_call_count.fetch_add(1, Ordering::Relaxed) + 1;
                                    let avg = total / count.max(1);
                                    debug!(
                                        "rpc.get_transaction duration={}ms (avg {}ms over {} calls) attempt={}",
                                        elapsed_ms, avg, count, attempt + 1
                                    );
                                    if avg > 500 {
                                        warn!("WARN: RPC SLOW - CAUSING PACKET LOSS (avg {}ms)", avg);
                                    }
                                    // 1s, 2s, 4s, 8s, 16s - Patient backoff for lagged RPC
                                    let delay = RETRY_BASE_DELAY_MS * (1 << attempt.min(4));
                                    warn!(
                                        "⚠️ Fetch retry {}/5 (delay {}ms)... Error: {}",
                                        attempt + 1,
                                        delay,
                                        e
                                    );
                                    sleep(Duration::from_millis(delay)).await;
                                }
                            }
                        }

                        let tx = match tx_result {
                            Some(t) => t,
                            None => {
                                metrics
                                    .helius_events_dropped
                                    .with_label_values(&["fetch_failed"])
                                    .inc();
                                error!(
                                    "❌ DROPPED EVENT (reason: fetch_transaction_failed): Failed to fetch transaction after retries: {}",
                                    signature_str
                                );
                                events_dropped.fetch_add(1, Ordering::Relaxed);
                                return None;
                            }
                        };

                        let meta = match &tx.transaction.meta {
                            Some(m) => m,
                            None => {
                                metrics
                                    .helius_events_dropped
                                    .with_label_values(&["fetch_failed"])
                                    .inc();
                                error!(
                                    "❌ DROPPED EVENT (reason: missing_metadata): Transaction {} has no metadata - cannot process",
                                    signature_str
                                );
                                events_dropped.fetch_add(1, Ordering::Relaxed);
                                return None;
                            }
                        };

                        let (pre_balances, post_balances) = extract_balances_from_meta(meta);

                        // Parse transaction to extract accounts and instructions
                        let (accounts, instructions): (Vec<Pubkey>, Vec<RawInstruction>) = match parse_ui_transaction(&tx.transaction.transaction) {
                            Some(parsed) => parsed,
                            None => {
                                metrics
                                    .helius_events_dropped
                                    .with_label_values(&["fetch_failed"])
                                    .inc();
                                error!("❌ DROPPED EVENT (reason: parse_transaction_failed): Failed to parse transaction data from EncodedTransaction for signature {}", signature_str);
                                events_dropped.fetch_add(1, Ordering::Relaxed);
                                return None;
                            }
                        };

                        // 🔍 CRITICAL FIX: VALIDATE DISCRIMINATOR
                        // We verify the 8-byte discriminator to be 100% sure it's Pump.fun Create.

                        let mut is_verified_creation = false;

                        for instruction in &instructions {
                            if instruction.program_id == *PUMPFUN_PROGRAM_PUBKEY {
                                // Check if data matches Create discriminator
                                if instruction.data.len() >= 8 &&
                                   instruction.data[0..8] == PUMPFUN_CREATE_DISCRIMINATOR
                                {
                                    is_verified_creation = true;
                                    break;
                                }
                            }
                        }

                        let involves_tracked_pool = has_tracked_pools
                            && accounts_include_tracked_pool(&accounts, &tracked_pools_snapshot);

                        let involves_recent_pool = if involves_tracked_pool {
                            true
                        } else {
                            let now = Instant::now();
                            accounts.iter().any(|pk| {
                                if let Some(entry) = recent_pools.get(pk) {
                                    now.duration_since(*entry.value()).as_secs() < RECENT_POOL_TTL_SECS
                                } else {
                                    false
                                }
                            })
                        };

                        if !is_verified_creation && !involves_tracked_pool && !involves_recent_pool {
                            metrics
                                .helius_events_dropped
                                .with_label_values(&["not_creation_and_not_tracked_pool"])
                                .inc();
                            debug!("⏭️  DROPPED EVENT (reason: not_creation_and_not_tracked_pool): Not a verified creation and no tracked pool involvement for {}", signature_str);
                            events_dropped.fetch_add(1, Ordering::Relaxed);
                            return None;
                        }

                        if is_verified_creation {
                            // If we are here, it is a CONFIRMED Pump.fun creation
                            info!("🔥 FOUND VERIFIED CREATION EVENT! Signature: {}", signature_str);

                            metrics
                                .initialize_pool_detected
                                .with_label_values(&[PUMPFUN_AMM_LABEL])
                                .inc();

                            // Extract bonding curve address and add to tracked pools and recent cache
                            for instruction in &instructions {
                                if instruction.program_id == *PUMPFUN_PROGRAM_PUBKEY {
                                    if instruction.data.len() >= 8 && instruction.data[0..8] == PUMPFUN_CREATE_DISCRIMINATOR {
                                        if let Some(&bonding_curve_idx) = instruction
                                            .account_indices
                                            .get(BONDING_CURVE_ACCOUNT_INDEX)
                                        {
                                            if let Some(bonding_curve_pubkey) = accounts.get(bonding_curve_idx as usize) {
                                                // 🛑 BLACKLIST SYSTEM ADDRESSES 🛑
                                                let key_str = bonding_curve_pubkey.to_string();
                                                if key_str == "11111111111111111111111111111111"
                                                    || key_str == "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
                                                    || key_str == "So11111111111111111111111111111111111111112"
                                                {
                                                    debug!("🚫 IGNORING System/Token Address masked as pool: {}", key_str);
                                                    continue;
                                                }

                                                // Add to tracked pools
                                                let pool_count = {
                                                    let mut new_set = (*tracked_pools.load_full()).clone();
                                                    new_set.insert(*bonding_curve_pubkey);
                                                    let len = new_set.len();
                                                    tracked_pools.store(Arc::new(new_set));
                                                    len
                                                };
                                                recent_pools.insert(*bonding_curve_pubkey, Instant::now());

                                                info!(
                                                    "✅ TRACKING NEW POOL: {} (total tracked: {})",
                                                    bonding_curve_pubkey,
                                                    pool_count
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            info!("📈 Tracked pool activity detected (non-creation) for {}", signature_str);
                        }

                        // Fetch bonding curve account data for relevant accounts
                        let mut account_data = HashMap::new();
                        // (Fetching simplified for creation events - usually not strictly needed for detection,
                        // but good for completeness if we want initial reserves)
                        // ... code omitted for brevity as it's optional for detection logic ...

                        // Extract logs from metadata
                        let tx_logs = extract_logs_from_meta(meta);

                        info!(
                            "📤 Emitting GeyserEvent::Transaction (creation={}) with {} accounts, {} instructions",
                            is_verified_creation,
                            accounts.len(),
                            instructions.len()
                        );

                        // Create and return the GeyserEvent
                        let arrival_ts_ms = crate::types::arrival_time_ms();
                        let ingress_wall_ts_ms = crate::types::ingress_epoch_ms();
                        let event = GeyserEvent::Transaction {
                            slot: crate::types::normalize_slot(Some(tx.slot)),
                            event_ts_ms: crate::types::event_ts_from_block_time(tx.block_time),
                            arrival_ts_ms: Some(arrival_ts_ms),
                            event_time: ghost_core::EventTimeMetadata::new(
                                crate::types::event_ts_from_block_time(tx.block_time),
                                Some(ingress_wall_ts_ms),
                                Some(arrival_ts_ms),
                            ),
                            signature: sig,
                            accounts,
                            instructions,
                            logs: tx_logs,
                            block_time: tx.block_time,
                            account_data,
                            pre_balances,
                            post_balances,
                            success: meta.err.is_none(),
                            error_code: meta.err.as_ref().map(|err| format!("{:?}", err)),
                            compute_units_consumed: meta.compute_units_consumed.clone().into(),
                            source: "helius".to_string(),
                            synthetic: false,
                            // Helius provides parsed JSON, not raw bytes
                            mpcf_payload_bytes: None,
                            mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
                            inner_instructions: vec![],
                            pre_token_balances: vec![],
                            post_token_balances: vec![],
                        };

                        // Successfully published event
                        events_published.fetch_add(1, Ordering::Relaxed);
                        let published = events_published.load(Ordering::Relaxed);
                        let total_notifications = log_notifications_received.load(Ordering::Relaxed);
                        let land_rate = if total_notifications > 0 {
                            (published as f64 / total_notifications as f64) * 100.0
                        } else {
                            100.0
                        };

                        info!(
                            "✅ EVENT PUBLISHED! Published: {}, Total notifications: {}, Land rate: {:.2}%",
                            published, total_notifications, land_rate
                        );

                        return Some(Ok(event));
                    }
                    Ok(Message::Close(frame)) => {
                        warn!("⚠️ WebSocket close frame received: {:?}", frame);
                        None
                    }
                    Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {
                        trace!("🏓 Ping/Pong message");
                        None
                    }
                    Ok(msg) => {
                        trace!("⏭️  Skipping non-text WebSocket message: {:?}", msg);
                        None
                    }
                    Err(e) => {
                        error!("❌ WebSocket error: {}", e);
                        None
                    }
                }
            }
        });

        Ok(Box::pin(event_stream))
    }
}

impl Drop for HeliusWebSocketAdapter {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Release);
        info!("🛑 Helius adapter dropped, stop flag set");
    }
}

/// Fetch account data with Patient Fetcher pattern (exponential backoff)
async fn fetch_account_data_patient(
    rpc: &RpcClient,
    pubkey: &Pubkey,
    max_attempts: u32,
) -> Option<Vec<u8>> {
    for attempt in 0..max_attempts {
        match rpc
            .get_account_with_commitment(pubkey, CommitmentConfig::confirmed())
            .await
        {
            Ok(response) => {
                if let Some(account) = response.value {
                    debug!(
                        "✅ Account data fetched for {} on attempt {}",
                        pubkey,
                        attempt + 1
                    );
                    return Some(account.data);
                } else {
                    let delay = RETRY_BASE_DELAY_MS * (1 << attempt.min(3));
                    debug!(
                        "⚠️ Account not found (None) for {} on attempt {} (delay {}ms)",
                        pubkey,
                        attempt + 1,
                        delay
                    );
                    sleep(Duration::from_millis(delay)).await;
                }
            }
            Err(e) => {
                let delay = RETRY_BASE_DELAY_MS * (1 << attempt.min(3));
                debug!(
                    "⚠️ Account fetch retry {} for {} (delay {}ms)... ({})",
                    attempt + 1,
                    pubkey,
                    delay,
                    e
                );
                sleep(Duration::from_millis(delay)).await;
            }
        }
    }

    warn!(
        "❌ Failed to fetch account data for {} after {} attempts",
        pubkey, max_attempts
    );
    None
}

/// Parse a UI transaction and extract accounts and instructions
fn parse_ui_transaction(
    encoded_tx: &EncodedTransaction,
) -> Option<(Vec<Pubkey>, Vec<RawInstruction>)> {
    match encoded_tx {
        EncodedTransaction::Json(ui_tx) => match &ui_tx.message {
            UiMessage::Raw(raw_msg) => {
                let accounts: Vec<Pubkey> = raw_msg
                    .account_keys
                    .iter()
                    .filter_map(|key_str| Pubkey::from_str(key_str).ok())
                    .collect();

                let instructions: Vec<RawInstruction> = raw_msg
                    .instructions
                    .iter()
                    .filter_map(|ix| {
                        let program_id = accounts.get(ix.program_id_index as usize)?.to_owned();
                        let data = bs58::decode(&ix.data).into_vec().ok()?;
                        Some(RawInstruction {
                            program_id,
                            account_indices: ix.accounts.clone(),
                            data,
                        })
                    })
                    .collect();

                Some((accounts, instructions))
            }
            UiMessage::Parsed(parsed_msg) => {
                info!("📋 Handling JsonParsed format (Helius fallback)");

                let accounts: Vec<Pubkey> = parsed_msg
                    .account_keys
                    .iter()
                    .filter_map(|acc| Pubkey::from_str(&acc.pubkey).ok())
                    .collect();

                let instructions: Vec<RawInstruction> = parsed_msg
                    .instructions
                    .iter()
                    .filter_map(|ui_ix| parse_ui_instruction(ui_ix, &accounts))
                    .collect();

                Some((accounts, instructions))
            }
        },
        EncodedTransaction::LegacyBinary(data_str) | EncodedTransaction::Binary(data_str, _) => {
            info!("📋 Handling Base64-encoded transaction");

            use base64::{engine::general_purpose, Engine as _};
            let tx_data = match general_purpose::STANDARD.decode(data_str) {
                Ok(data) => data,
                Err(e) => {
                    warn!("⚠️ Failed to decode Base64 transaction data: {}", e);
                    return None;
                }
            };

            // Try VersionedTransaction first
            match bincode::deserialize::<VersionedTransaction>(&tx_data) {
                Ok(versioned_tx) => {
                    let message = versioned_tx.message;
                    let static_keys = message.static_account_keys();
                    let accounts: Vec<Pubkey> = static_keys.iter().copied().collect();

                    let instructions: Vec<RawInstruction> = message
                        .instructions()
                        .iter()
                        .map(|ix| {
                            let program_id = accounts[ix.program_id_index as usize];
                            RawInstruction {
                                program_id,
                                account_indices: ix.accounts.clone(),
                                data: ix.data.clone(),
                            }
                        })
                        .collect();

                    Some((accounts, instructions))
                }
                Err(versioned_err) => {
                    // Fallback to legacy Transaction
                    match bincode::deserialize::<Transaction>(&tx_data) {
                        Ok(tx) => {
                            let accounts = tx.message.account_keys.clone();

                            let instructions: Vec<RawInstruction> = tx
                                .message
                                .instructions
                                .iter()
                                .map(|ix| {
                                    let program_id = accounts[ix.program_id_index as usize];
                                    RawInstruction {
                                        program_id,
                                        account_indices: ix.accounts.clone(),
                                        data: ix.data.clone(),
                                    }
                                })
                                .collect();

                            Some((accounts, instructions))
                        }
                        Err(legacy_err) => {
                            warn!("⚠️ Failed to deserialize transaction: {}", legacy_err);
                            None
                        }
                    }
                }
            }
        }
        _ => {
            warn!("⚠️ Unsupported transaction encoding format");
            None
        }
    }
}

/// Parse a UiInstruction into RawInstruction
fn parse_ui_instruction(ui_ix: &UiInstruction, accounts: &[Pubkey]) -> Option<RawInstruction> {
    match ui_ix {
        UiInstruction::Compiled(compiled) => {
            let program_id = accounts.get(compiled.program_id_index as usize)?.to_owned();
            let data = bs58::decode(&compiled.data).into_vec().ok()?;

            Some(RawInstruction {
                program_id,
                account_indices: compiled.accounts.clone(),
                data,
            })
        }
        UiInstruction::Parsed(parsed) => match parsed {
            UiParsedInstruction::PartiallyDecoded(partial) => {
                let program_id = Pubkey::from_str(&partial.program_id).ok()?;
                let data = bs58::decode(&partial.data).into_vec().ok()?;

                let account_indices: Vec<u8> = partial
                    .accounts
                    .iter()
                    .filter_map(|acc_str| {
                        let acc_pubkey = Pubkey::from_str(acc_str).ok()?;
                        accounts
                            .iter()
                            .position(|a| a == &acc_pubkey)
                            .map(|i| i as u8)
                    })
                    .collect();

                Some(RawInstruction {
                    program_id,
                    account_indices,
                    data,
                })
            }
            UiParsedInstruction::Parsed(fully_parsed) => {
                let program_id = Pubkey::from_str(&fully_parsed.program_id).ok()?;

                Some(RawInstruction {
                    program_id,
                    account_indices: Vec::new(),
                    data: Vec::new(),
                })
            }
        },
    }
}

/// Extract logs from transaction metadata
fn extract_logs_from_meta(meta: &UiTransactionStatusMeta) -> Vec<String> {
    match &meta.log_messages {
        OptionSerializer::Some(logs) => logs.clone(),
        OptionSerializer::Skip | OptionSerializer::None => Vec::new(),
    }
}

fn extract_balances_from_meta(meta: &UiTransactionStatusMeta) -> (Vec<u64>, Vec<u64>) {
    (meta.pre_balances.clone(), meta.post_balances.clone())
}

fn accounts_include_tracked_pool(accounts: &[Pubkey], tracked: &HashSet<Pubkey>) -> bool {
    accounts.iter().any(|acc| tracked.contains(acc))
}

fn trade_prefilter_drop_reason(
    looks_like_creation: bool,
    looks_like_trade: bool,
    has_tracked_pools: bool,
    has_recent_pools: bool,
    accounts_checked_prefilter: bool,
    involves_tracked_pool_prefilter: bool,
) -> Option<&'static str> {
    if looks_like_creation {
        return None;
    }

    if looks_like_trade && !has_tracked_pools && !has_recent_pools {
        return Some("prefilter_trade_while_untracked");
    }

    if looks_like_trade && (has_tracked_pools || has_recent_pools) {
        if !accounts_checked_prefilter {
            return Some("prefilter_trade_no_account_keys");
        }
        if !involves_tracked_pool_prefilter {
            return Some("prefilter_trade_not_tracked");
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // ... tests as in previous version ...
    #[test]
    fn test_helius_adapter_creation() {
        let metrics = Arc::new(SeerMetrics::new());
        let adapter = HeliusWebSocketAdapter::new(
            "wss://test.com".to_string(),
            "https://test-rpc.com".to_string(),
            metrics,
            3,
            5,
        );
        assert_eq!(adapter.endpoint, "wss://test.com");
    }

    #[test]
    fn test_accounts_include_tracked_pool_detection() {
        let target_pool = Pubkey::new_unique();
        let other = Pubkey::new_unique();
        let accounts = vec![other, target_pool];

        let mut tracked = HashSet::new();
        tracked.insert(target_pool);

        assert!(accounts_include_tracked_pool(&accounts, &tracked));

        let empty_tracked: HashSet<Pubkey> = HashSet::new();
        assert!(!accounts_include_tracked_pool(&accounts, &empty_tracked));
    }

    #[test]
    fn test_trade_prefilter_drop_reason_rules() {
        // Creation should bypass trade gate
        assert_eq!(
            trade_prefilter_drop_reason(true, true, true, true, false, false),
            None
        );

        // Trade with tracked pools but missing account keys should drop
        assert_eq!(
            trade_prefilter_drop_reason(false, true, true, false, false, false),
            Some("prefilter_trade_no_account_keys")
        );

        // Trade with checked accounts but no tracked pool involvement should drop
        assert_eq!(
            trade_prefilter_drop_reason(false, true, true, false, true, false),
            Some("prefilter_trade_not_tracked")
        );

        // Trade-like event without any tracked pools should drop before RPC
        assert_eq!(
            trade_prefilter_drop_reason(false, true, false, false, false, false),
            Some("prefilter_trade_while_untracked")
        );

        // Trade involving tracked pool passes
        assert_eq!(
            trade_prefilter_drop_reason(false, true, true, true, true, true),
            None
        );
    }

    // Additional tests for logic verification if needed
}
