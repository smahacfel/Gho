use std::{
    collections::HashMap,
    str::FromStr,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use futures::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use solana_sdk::{pubkey::Pubkey, signature::Signature, transaction::VersionedTransaction};
use thiserror::Error;
use tokio::sync::{oneshot, watch, Mutex};
use tracing::warn;
use yellowstone_grpc_client::GeyserGrpcClient;
use yellowstone_grpc_proto::prelude::{
    subscribe_update::UpdateOneof, CommitmentLevel, SubscribeRequest,
    SubscribeRequestFilterTransactions,
};

pub const HELIUS_SENDER_ENDPOINT: &str = "http://fra-sender.helius-rpc.com/fast";
pub const HELIUS_SENDER_ENDPOINT_ENV: &str = "GHOST_HELIUS_SENDER_ENDPOINT";
pub const HELIUS_SENDER_MIN_TIP_LAMPORTS: u64 = 200_000;
pub const HELIUS_SENDER_BUY_BASELINE_TIP_LAMPORTS: u64 = 1_000_000;
pub const HELIUS_PRIORITY_FEE_FALLBACK_MICRO_LAMPORTS: u64 = 25_000;

const HELIUS_SENDER_CONFIRM_TIMEOUT_MS: u64 = 12_000;
const HELIUS_SENDER_SUBMIT_TIMEOUT_MS: u64 = 1_500;
const HELIUS_PRIORITY_FEE_TIMEOUT_MS: u64 = 1_200;
const PRIORITY_FEE_CACHE_TTL_MS: u64 = 350;
const PRIORITY_FEE_INFLIGHT_JOIN_MAX_TOTAL_MS: u64 = 110;
const TIP_FLOOR_TIMEOUT_MS: u64 = 500;
const TIP_FLOOR_CACHE_TTL_MS: u64 = 350;
const TIP_FLOOR_STALE_LAST_GOOD_MAX_AGE_MS: u64 = 900;
const TIP_FLOOR_INFLIGHT_JOIN_MAX_TOTAL_MS: u64 = 180;
const TIP_FLOOR_BUFFER_MULTIPLIER: f64 = 1.8;
const TIP_FLOOR_SOURCE_ENDPOINT: &str = "https://bundles.jito.wtf/api/v1/bundles/tip_floor";
const DEFAULT_PRIORITY_LEVEL: &str = "High";
const DEFAULT_PRIORITY_TRANSACTION_ENCODING: &str = "base64";

const HELIUS_SENDER_TIP_ACCOUNTS: &[&str] = &[
    "4ACfpUFoaSD9bfPdeu6DBt89gB6ENTeHBXCAi87NhDEE",
    "D2L6yPZ2FmmmTKPgzaMKdhu6EWZcTpLy1Vhx8uvZe7NZ",
    "9bnz4RShgq1hAnLnZbP8kbgBg1kEmcJBYQq3gQbmnSta",
    "5VY91ws6B2hMmBFRsXkoAAdsPHBJwRfBht4DXox3xkwn",
    "2nyhqdwKcJZR2vcqCyrYsaPVdAnFoJjiksCXJ7hfEYgD",
    "2q5pghRs6arqVjRvT5gfgWfWcHWmw1ZuCzphgd5KfWGJ",
    "wyvPkWjVZz1M8fHQnMMCDTQDbkManefNNhweYk5WkcF",
    "3KCKozbAaF75qEU33jtzozcJ29yJuaLJTy2jFdzUY8bT",
    "4vieeGHPYPG2MmyPRcYjdiDmmhN3ww7hsFNap8pVN3Ey",
    "4TQLFNWK8AovT1gFvda5jfw2oJeRMKEmw7aH6MGBJ3or",
];

#[derive(Debug, Clone)]
pub struct LiveTxSenderConfig {
    pub sender_endpoint: String,
    pub priority_fee_rpc_url: String,
    pub tip_floor_endpoint: String,
    pub yellowstone_grpc_endpoint: String,
    pub yellowstone_x_token: String,
}

#[derive(Clone)]
pub struct LiveTxSender {
    http: Client,
    config: LiveTxSenderConfig,
    tip_floor_cache: Arc<RwLock<Option<CachedTipFloor>>>,
    priority_fee_cache: Arc<RwLock<HashMap<PriorityFeeCacheKey, CachedPriorityFee>>>,
    tip_floor_inflight: Arc<Mutex<Option<InflightRefresh<TipFloorResolution>>>>,
    priority_fee_inflight:
        Arc<Mutex<HashMap<PriorityFeeCacheKey, InflightRefresh<PriorityFeeEstimate>>>>,
}

#[derive(Debug, Clone)]
pub struct SenderTransactionSubmission {
    pub signature: Signature,
}

#[derive(Debug, Clone)]
pub struct SenderConfirmedTransaction {
    pub signature: Signature,
    pub landed_slot: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TipFloorResolutionTelemetry {
    pub cache_hit: bool,
    pub cache_age_ms: u64,
    pub fetch_latency_ms: u64,
    pub cache_mode: &'static str,
    pub source: &'static str,
    pub inflight_join_result: &'static str,
    pub inflight_wait_ms: u64,
}

impl Default for TipFloorResolutionTelemetry {
    fn default() -> Self {
        Self {
            cache_hit: false,
            cache_age_ms: 0,
            fetch_latency_ms: 0,
            cache_mode: "not_collected",
            source: "not_collected",
            inflight_join_result: "not_attempted",
            inflight_wait_ms: 0,
        }
    }
}

impl TipFloorResolutionTelemetry {
    fn sender_fixed_tip() -> Self {
        Self {
            cache_hit: false,
            cache_age_ms: 0,
            fetch_latency_ms: 0,
            cache_mode: "fixed_baseline",
            source: "sender_fixed_tip",
            inflight_join_result: "disabled",
            inflight_wait_ms: 0,
        }
    }

    fn record_metrics(&self) {
        let cache_result = if self.source == "sender_fixed_tip" {
            "skipped"
        } else if self.cache_hit {
            "hit"
        } else {
            "miss"
        };
        metrics::counter!(
            "tip_floor_cache_hit",
            1u64,
            "result" => cache_result,
            "source" => self.source
        );
        metrics::histogram!(
            "tip_floor_cache_age_ms",
            self.cache_age_ms as f64,
            "source" => self.source
        );
        metrics::histogram!(
            "tip_floor_fetch_latency_ms",
            self.fetch_latency_ms as f64,
            "source" => self.source
        );
        metrics::counter!(
            "tip_floor_cache_mode_total",
            1u64,
            "mode" => self.cache_mode,
            "source" => self.source
        );
        metrics::counter!(
            "tip_floor_inflight_join_total",
            1u64,
            "result" => self.inflight_join_result,
            "source" => self.source
        );
        metrics::histogram!(
            "tip_floor_inflight_wait_ms",
            self.inflight_wait_ms as f64,
            "result" => self.inflight_join_result,
            "source" => self.source
        );
    }
}

#[derive(Debug, Clone)]
pub struct BuyTipResolution {
    pub tip_lamports: u64,
    pub telemetry: TipFloorResolutionTelemetry,
}

impl BuyTipResolution {
    fn sender_fixed_tip() -> Self {
        Self {
            tip_lamports: select_buy_tip_lamports(None),
            telemetry: TipFloorResolutionTelemetry::sender_fixed_tip(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorityFeeEstimateTelemetry {
    pub cache_hit: bool,
    pub cache_age_ms: u64,
    pub fetch_latency_ms: u64,
    pub cache_mode: &'static str,
    pub source: &'static str,
    pub inflight_join_result: &'static str,
    pub inflight_wait_ms: u64,
}

impl Default for PriorityFeeEstimateTelemetry {
    fn default() -> Self {
        Self {
            cache_hit: false,
            cache_age_ms: 0,
            fetch_latency_ms: 0,
            cache_mode: "not_collected",
            source: "not_collected",
            inflight_join_result: "not_attempted",
            inflight_wait_ms: 0,
        }
    }
}

impl PriorityFeeEstimateTelemetry {
    fn record_buy_metrics(&self) {
        metrics::counter!(
            "priority_fee_cache_hit",
            1u64,
            "result" => if self.cache_hit { "hit" } else { "miss" },
            "source" => self.source
        );
        metrics::histogram!(
            "priority_fee_cache_age_ms",
            self.cache_age_ms as f64,
            "source" => self.source
        );
        metrics::histogram!(
            "priority_fee_fetch_latency_ms",
            self.fetch_latency_ms as f64,
            "source" => self.source
        );
        metrics::counter!(
            "priority_fee_cache_mode_total",
            1u64,
            "mode" => self.cache_mode,
            "source" => self.source
        );
        metrics::counter!(
            "priority_fee_inflight_join_total",
            1u64,
            "result" => self.inflight_join_result,
            "source" => self.source
        );
        metrics::histogram!(
            "priority_fee_inflight_wait_ms",
            self.inflight_wait_ms as f64,
            "result" => self.inflight_join_result,
            "source" => self.source
        );
    }
}

#[derive(Debug, Clone)]
pub struct PriorityFeeEstimate {
    pub micro_lamports: u64,
    pub telemetry: PriorityFeeEstimateTelemetry,
}

impl PriorityFeeEstimate {
    fn fresh_cache(micro_lamports: u64, cache_age_ms: u64) -> Self {
        Self {
            micro_lamports,
            telemetry: PriorityFeeEstimateTelemetry {
                cache_hit: true,
                cache_age_ms,
                fetch_latency_ms: 0,
                cache_mode: "fresh_cache",
                source: "priority_fee_cache",
                ..PriorityFeeEstimateTelemetry::default()
            },
        }
    }

    fn refreshed(micro_lamports: u64, fetch_latency_ms: u64, source: &'static str) -> Self {
        Self {
            micro_lamports,
            telemetry: PriorityFeeEstimateTelemetry {
                fetch_latency_ms,
                cache_mode: "miss_refresh",
                source,
                ..PriorityFeeEstimateTelemetry::default()
            },
        }
    }

    fn fallback(fetch_latency_ms: u64, source: &'static str) -> Self {
        Self {
            micro_lamports: HELIUS_PRIORITY_FEE_FALLBACK_MICRO_LAMPORTS,
            telemetry: PriorityFeeEstimateTelemetry {
                fetch_latency_ms,
                cache_mode: "miss_refresh_failed",
                source,
                ..PriorityFeeEstimateTelemetry::default()
            },
        }
    }

    fn with_inflight_join(mut self, result: &'static str, wait_ms: u64) -> Self {
        self.telemetry.inflight_join_result = result;
        self.telemetry.inflight_wait_ms = wait_ms;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PriorityFeeCacheKey {
    tx_kind: &'static str,
    buy_variant: &'static str,
    token_program: Pubkey,
    ata_missing_pre_submit: bool,
    has_inline_tip: bool,
}

impl PriorityFeeCacheKey {
    pub fn buy(
        buy_variant: &'static str,
        token_program: Pubkey,
        ata_missing_pre_submit: bool,
        has_inline_tip: bool,
    ) -> Self {
        Self {
            tx_kind: "buy",
            buy_variant,
            token_program,
            ata_missing_pre_submit,
            has_inline_tip,
        }
    }
}

#[derive(Debug, Error)]
pub enum LiveTxSenderError {
    #[error("Helius Sender submission failed: {message}")]
    Submit { message: String },
    #[error("Yellowstone confirmation timed out for signature {signature} after {timeout_ms}ms")]
    ConfirmationTimeout {
        signature: Signature,
        timeout_ms: u64,
    },
    #[error("Yellowstone confirmation transport failed for signature {signature}: {message}")]
    ConfirmationTransport {
        signature: Signature,
        message: String,
    },
    #[error("Yellowstone observed failed transaction {signature} at slot {slot}")]
    ConfirmationRejected { signature: Signature, slot: u64 },
}

#[derive(Debug, Clone, Deserialize)]
struct TipFloorEntry {
    #[serde(default)]
    landed_tips_75th_percentile: Option<f64>,
    #[serde(default)]
    landed_tips_50th_percentile: Option<f64>,
    #[serde(default)]
    landed_tips_25th_percentile: Option<f64>,
}

#[derive(Debug, Clone)]
struct CachedTipFloor {
    lamports: u64,
    fetched_at: Instant,
}

impl CachedTipFloor {
    fn age_ms(&self) -> u64 {
        elapsed_ms(self.fetched_at)
    }
}

#[derive(Debug, Clone)]
struct CachedPriorityFee {
    micro_lamports: u64,
    fetched_at: Instant,
}

impl CachedPriorityFee {
    fn age_ms(&self) -> u64 {
        elapsed_ms(self.fetched_at)
    }
}

#[derive(Clone)]
struct InflightRefresh<T: Clone> {
    started_at: Instant,
    receiver: watch::Receiver<Option<T>>,
}

#[derive(Clone, Copy)]
enum RefreshIntent {
    HotPath,
    Background,
}

impl RefreshIntent {
    fn max_total_inflight_ms(self, kind: &'static str) -> Option<u64> {
        match (self, kind) {
            (Self::HotPath, "tip_floor") => Some(TIP_FLOOR_INFLIGHT_JOIN_MAX_TOTAL_MS),
            (Self::HotPath, "priority_fee") => Some(PRIORITY_FEE_INFLIGHT_JOIN_MAX_TOTAL_MS),
            (Self::Background, _) => None,
            _ => None,
        }
    }
}

enum InflightJoinOutcome<T> {
    Joined(T),
    NotFound,
    TimedOut,
}

enum PriorityFeeBackgroundLaunch {
    Cached(PriorityFeeEstimate),
    Join(InflightRefresh<PriorityFeeEstimate>),
    Start {
        started_at: Instant,
        sender: watch::Sender<Option<PriorityFeeEstimate>>,
    },
}

#[derive(Debug, Clone)]
struct TipFloorResolution {
    floor_lamports: Option<u64>,
    telemetry: TipFloorResolutionTelemetry,
}

impl TipFloorResolution {
    fn new(
        floor_lamports: Option<u64>,
        cache_hit: bool,
        cache_age_ms: u64,
        fetch_latency_ms: u64,
        cache_mode: &'static str,
        source: &'static str,
    ) -> Self {
        Self {
            floor_lamports,
            telemetry: TipFloorResolutionTelemetry {
                cache_hit,
                cache_age_ms,
                fetch_latency_ms,
                cache_mode,
                source,
                ..TipFloorResolutionTelemetry::default()
            },
        }
    }

    fn fresh_cache(lamports: u64, cache_age_ms: u64) -> Self {
        Self::new(
            Some(lamports),
            true,
            cache_age_ms,
            0,
            "fresh_cache",
            "tip_floor_cache",
        )
    }

    fn refreshed(lamports: u64, fetch_latency_ms: u64) -> Self {
        Self::new(
            Some(lamports),
            false,
            0,
            fetch_latency_ms,
            "miss_refresh",
            "jito_tip_floor",
        )
    }

    fn stale_last_good(lamports: u64, cache_age_ms: u64, fetch_latency_ms: u64) -> Self {
        Self::new(
            Some(lamports),
            false,
            cache_age_ms,
            fetch_latency_ms,
            "stale_last_good",
            "stale_last_good",
        )
    }

    fn failed(fetch_latency_ms: u64, source: &'static str) -> Self {
        Self::new(
            None,
            false,
            0,
            fetch_latency_ms,
            "miss_refresh_failed",
            source,
        )
    }

    fn with_inflight_join(mut self, result: &'static str, wait_ms: u64) -> Self {
        self.telemetry.inflight_join_result = result;
        self.telemetry.inflight_wait_ms = wait_ms;
        self
    }
}

impl LiveTxSenderConfig {
    pub fn new(
        sender_endpoint: impl Into<String>,
        priority_fee_rpc_url: impl Into<String>,
        yellowstone_grpc_endpoint: impl Into<String>,
        yellowstone_x_token: impl Into<String>,
    ) -> Self {
        Self {
            sender_endpoint: sender_endpoint.into(),
            priority_fee_rpc_url: priority_fee_rpc_url.into(),
            tip_floor_endpoint: TIP_FLOOR_SOURCE_ENDPOINT.to_string(),
            yellowstone_grpc_endpoint: yellowstone_grpc_endpoint.into(),
            yellowstone_x_token: yellowstone_x_token.into(),
        }
    }

    pub fn with_tip_floor_endpoint(mut self, tip_floor_endpoint: impl Into<String>) -> Self {
        self.tip_floor_endpoint = tip_floor_endpoint.into();
        self
    }
}

impl LiveTxSender {
    pub fn new(config: LiveTxSenderConfig) -> Result<Self> {
        let http = Client::builder()
            .connect_timeout(Duration::from_millis(300))
            .timeout(Duration::from_millis(HELIUS_SENDER_SUBMIT_TIMEOUT_MS))
            .build()
            .context("build live tx sender http client")?;
        Ok(Self {
            http,
            config,
            tip_floor_cache: Arc::new(RwLock::new(None)),
            priority_fee_cache: Arc::new(RwLock::new(HashMap::new())),
            tip_floor_inflight: Arc::new(Mutex::new(None)),
            priority_fee_inflight: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn sender_endpoint(&self) -> &str {
        &self.config.sender_endpoint
    }

    pub async fn estimate_priority_fee_micro_lamports(
        &self,
        transaction: &VersionedTransaction,
    ) -> u64 {
        self.fetch_priority_fee_uncached(transaction)
            .await
            .micro_lamports
    }

    pub async fn estimate_buy_priority_fee_micro_lamports_with_telemetry(
        &self,
        transaction: &VersionedTransaction,
        cache_key: Option<&PriorityFeeCacheKey>,
    ) -> PriorityFeeEstimate {
        let estimate = self
            .estimate_buy_priority_fee_internal(transaction, cache_key, RefreshIntent::HotPath)
            .await;
        estimate.telemetry.record_buy_metrics();
        estimate
    }

    pub fn get_cached_buy_priority_fee(
        &self,
        cache_key: &PriorityFeeCacheKey,
    ) -> Option<PriorityFeeEstimate> {
        let cached = self
            .priority_fee_cache
            .read()
            .ok()
            .and_then(|guard| guard.get(cache_key).cloned())?;
        let cache_age_ms = cached.age_ms();
        (cache_age_ms <= PRIORITY_FEE_CACHE_TTL_MS)
            .then(|| PriorityFeeEstimate::fresh_cache(cached.micro_lamports, cache_age_ms))
    }

    fn store_cached_buy_priority_fee(&self, cache_key: &PriorityFeeCacheKey, micro_lamports: u64) {
        if let Ok(mut guard) = self.priority_fee_cache.write() {
            guard.insert(
                cache_key.clone(),
                CachedPriorityFee {
                    micro_lamports,
                    fetched_at: Instant::now(),
                },
            );
        }
    }

    async fn estimate_buy_priority_fee_internal(
        &self,
        transaction: &VersionedTransaction,
        cache_key: Option<&PriorityFeeCacheKey>,
        intent: RefreshIntent,
    ) -> PriorityFeeEstimate {
        if let Some(cache_key) = cache_key {
            if let Some(cached_estimate) = self.get_cached_buy_priority_fee(cache_key) {
                return cached_estimate;
            }
            match self
                .maybe_join_priority_fee_inflight(cache_key, intent)
                .await
            {
                InflightJoinOutcome::Joined(inflight_estimate) => return inflight_estimate,
                InflightJoinOutcome::TimedOut => {
                    let estimate = self.fetch_priority_fee_uncached(transaction).await;
                    if estimate.telemetry.source == "helius_rpc" {
                        self.store_cached_buy_priority_fee(cache_key, estimate.micro_lamports);
                    }
                    return estimate.with_inflight_join("timed_out_fallback", 0);
                }
                InflightJoinOutcome::NotFound => {}
            }
        }
        self.start_or_fetch_priority_fee(transaction, cache_key)
            .await
    }

    async fn start_or_fetch_priority_fee(
        &self,
        transaction: &VersionedTransaction,
        cache_key: Option<&PriorityFeeCacheKey>,
    ) -> PriorityFeeEstimate {
        let Some(cache_key) = cache_key else {
            return self.fetch_priority_fee_uncached(transaction).await;
        };

        let started_at = Instant::now();
        let (tx, rx) = watch::channel(None::<PriorityFeeEstimate>);
        let should_fallback_direct = {
            let mut guard = self.priority_fee_inflight.lock().await;
            if let Some(cached_estimate) = self.get_cached_buy_priority_fee(cache_key) {
                return cached_estimate;
            }
            if guard.contains_key(cache_key) {
                true
            } else {
                guard.insert(
                    cache_key.clone(),
                    InflightRefresh {
                        started_at,
                        receiver: rx,
                    },
                );
                false
            }
        };
        if should_fallback_direct {
            let estimate = self.fetch_priority_fee_uncached(transaction).await;
            if estimate.telemetry.source == "helius_rpc" {
                self.store_cached_buy_priority_fee(cache_key, estimate.micro_lamports);
            }
            return estimate.with_inflight_join("timed_out_fallback", 0);
        }

        let estimate = self.fetch_priority_fee_uncached(transaction).await;
        if estimate.telemetry.source == "helius_rpc" {
            self.store_cached_buy_priority_fee(cache_key, estimate.micro_lamports);
        }
        let _ = tx.send(Some(estimate.clone()));
        let mut guard = self.priority_fee_inflight.lock().await;
        if guard
            .get(cache_key)
            .map(|inflight| inflight.started_at == started_at)
            .unwrap_or(false)
        {
            guard.remove(cache_key);
        }
        estimate
    }

    async fn maybe_join_priority_fee_inflight(
        &self,
        cache_key: &PriorityFeeCacheKey,
        intent: RefreshIntent,
    ) -> InflightJoinOutcome<PriorityFeeEstimate> {
        let inflight = self
            .priority_fee_inflight
            .lock()
            .await
            .get(cache_key)
            .cloned();
        let Some(inflight) = inflight else {
            return InflightJoinOutcome::NotFound;
        };
        self.await_priority_fee_inflight(inflight, intent).await
    }

    async fn await_priority_fee_inflight(
        &self,
        inflight: InflightRefresh<PriorityFeeEstimate>,
        intent: RefreshIntent,
    ) -> InflightJoinOutcome<PriorityFeeEstimate> {
        let mut receiver = inflight.receiver.clone();
        if let Some(estimate) = receiver.borrow().clone() {
            return InflightJoinOutcome::Joined(estimate.with_inflight_join("joined", 0));
        }

        let wait_started_at = Instant::now();
        let joined = async {
            loop {
                if receiver.changed().await.is_err() {
                    return None;
                }
                if let Some(estimate) = receiver.borrow().clone() {
                    return Some(estimate);
                }
            }
        };
        let joined = if let Some(max_total_ms) = intent.max_total_inflight_ms("priority_fee") {
            let elapsed_before_join_ms = elapsed_ms(inflight.started_at);
            let remaining_wait_ms = max_total_ms.saturating_sub(elapsed_before_join_ms);
            if remaining_wait_ms == 0 {
                return InflightJoinOutcome::TimedOut;
            }
            tokio::time::timeout(Duration::from_millis(remaining_wait_ms), joined)
                .await
                .ok()
                .flatten()
        } else {
            joined.await
        };
        match joined {
            Some(estimate) => InflightJoinOutcome::Joined(
                estimate.with_inflight_join("joined", elapsed_ms(wait_started_at)),
            ),
            None => InflightJoinOutcome::TimedOut,
        }
    }

    async fn fetch_priority_fee_uncached(
        &self,
        transaction: &VersionedTransaction,
    ) -> PriorityFeeEstimate {
        let fetch_started_at = Instant::now();
        let encoded = match serialize_transaction_base64(transaction) {
            Ok(encoded) => encoded,
            Err(err) => {
                warn!(
                    error = %err,
                    fallback_micro_lamports = HELIUS_PRIORITY_FEE_FALLBACK_MICRO_LAMPORTS,
                    "priority fee estimate fell back because transaction serialization failed"
                );
                return PriorityFeeEstimate::fallback(0, "fallback_serialize");
            }
        };

        let response = self
            .http
            .post(&self.config.priority_fee_rpc_url)
            .timeout(Duration::from_millis(HELIUS_PRIORITY_FEE_TIMEOUT_MS))
            .json(&json!({
                "jsonrpc": "2.0",
                "id": "ghost-live-priority-fee",
                "method": "getPriorityFeeEstimate",
                "params": [{
                    "transaction": encoded,
                    "options": {
                        "transactionEncoding": DEFAULT_PRIORITY_TRANSACTION_ENCODING,
                        "priorityLevel": DEFAULT_PRIORITY_LEVEL,
                    }
                }]
            }))
            .send()
            .await;

        match response {
            Ok(resp) => {
                let status = resp.status();
                let body = match resp.text().await {
                    Ok(body) => body,
                    Err(err) => {
                        warn!(
                            error = %err,
                            fallback_micro_lamports = HELIUS_PRIORITY_FEE_FALLBACK_MICRO_LAMPORTS,
                            "priority fee estimate fell back because response body could not be read"
                        );
                        return PriorityFeeEstimate::fallback(
                            elapsed_ms(fetch_started_at),
                            "fallback_body_read",
                        );
                    }
                };

                if !status.is_success() {
                    warn!(
                        status = %status,
                        body,
                        fallback_micro_lamports = HELIUS_PRIORITY_FEE_FALLBACK_MICRO_LAMPORTS,
                        "priority fee estimate fell back because Helius RPC returned non-success"
                    );
                    return PriorityFeeEstimate::fallback(
                        elapsed_ms(fetch_started_at),
                        "fallback_http_status",
                    );
                }

                match parse_priority_fee_micro_lamports_response(&body) {
                    Some(priority_fee) if priority_fee > 0 => PriorityFeeEstimate::refreshed(
                        priority_fee,
                        elapsed_ms(fetch_started_at),
                        "helius_rpc",
                    ),
                    _ => {
                        warn!(
                            body,
                            fallback_micro_lamports = HELIUS_PRIORITY_FEE_FALLBACK_MICRO_LAMPORTS,
                            "priority fee estimate fell back because response did not contain a usable value"
                        );
                        PriorityFeeEstimate::fallback(
                            elapsed_ms(fetch_started_at),
                            "fallback_response",
                        )
                    }
                }
            }
            Err(err) => {
                warn!(
                    error = %err,
                    fallback_micro_lamports = HELIUS_PRIORITY_FEE_FALLBACK_MICRO_LAMPORTS,
                    "priority fee estimate fell back because Helius RPC request failed"
                );
                PriorityFeeEstimate::fallback(elapsed_ms(fetch_started_at), "fallback_request")
            }
        }
    }

    pub async fn raise_tip_to_dynamic_floor(&self, requested_lamports: u64) -> u64 {
        requested_lamports.max(HELIUS_SENDER_MIN_TIP_LAMPORTS)
    }

    pub async fn resolve_buy_tip_lamports(&self) -> u64 {
        self.resolve_buy_tip_lamports_with_telemetry()
            .await
            .tip_lamports
    }

    pub async fn prewarm_buy_tip_floor_with_telemetry(&self) -> BuyTipResolution {
        BuyTipResolution::sender_fixed_tip()
    }

    pub(crate) async fn start_buy_tip_floor_prewarm_with_telemetry(
        &self,
    ) -> oneshot::Receiver<BuyTipResolution> {
        let (result_tx, result_rx) = oneshot::channel();
        let _ = result_tx.send(BuyTipResolution::sender_fixed_tip());
        result_rx
    }

    pub async fn resolve_buy_tip_lamports_with_telemetry(&self) -> BuyTipResolution {
        let resolution = BuyTipResolution::sender_fixed_tip();
        resolution.telemetry.record_metrics();
        resolution
    }

    pub fn select_tip_account(&self, seed_material: &[u8]) -> Pubkey {
        select_sender_tip_account(seed_material)
    }

    pub async fn prewarm_buy_priority_fee_cache_with_telemetry(
        &self,
        transaction: &VersionedTransaction,
        cache_key: &PriorityFeeCacheKey,
    ) -> PriorityFeeEstimate {
        self.estimate_buy_priority_fee_internal(
            transaction,
            Some(cache_key),
            RefreshIntent::Background,
        )
        .await
    }

    pub(crate) async fn start_buy_priority_fee_prewarm_with_telemetry(
        &self,
        transaction: VersionedTransaction,
        cache_key: PriorityFeeCacheKey,
    ) -> oneshot::Receiver<PriorityFeeEstimate> {
        let (result_tx, result_rx) = oneshot::channel();
        match self.begin_priority_fee_background_launch(&cache_key).await {
            PriorityFeeBackgroundLaunch::Cached(estimate) => {
                let _ = result_tx.send(estimate);
            }
            PriorityFeeBackgroundLaunch::Join(inflight) => {
                let sender = self.clone();
                tokio::spawn(async move {
                    let estimate = match sender
                        .await_priority_fee_inflight(inflight, RefreshIntent::Background)
                        .await
                    {
                        InflightJoinOutcome::Joined(estimate) => estimate,
                        InflightJoinOutcome::NotFound | InflightJoinOutcome::TimedOut => {
                            sender
                                .estimate_buy_priority_fee_internal(
                                    &transaction,
                                    Some(&cache_key),
                                    RefreshIntent::Background,
                                )
                                .await
                        }
                    };
                    let _ = result_tx.send(estimate);
                });
            }
            PriorityFeeBackgroundLaunch::Start {
                started_at,
                sender: inflight_sender,
            } => {
                let sender = self.clone();
                tokio::spawn(async move {
                    let estimate = sender.fetch_priority_fee_uncached(&transaction).await;
                    if estimate.telemetry.source == "helius_rpc" {
                        sender.store_cached_buy_priority_fee(&cache_key, estimate.micro_lamports);
                    }
                    let _ = inflight_sender.send(Some(estimate.clone()));
                    let mut guard = sender.priority_fee_inflight.lock().await;
                    if guard
                        .get(&cache_key)
                        .map(|inflight| inflight.started_at == started_at)
                        .unwrap_or(false)
                    {
                        guard.remove(&cache_key);
                    }
                    let _ = result_tx.send(estimate);
                });
            }
        }
        result_rx
    }

    async fn begin_priority_fee_background_launch(
        &self,
        cache_key: &PriorityFeeCacheKey,
    ) -> PriorityFeeBackgroundLaunch {
        if let Some(cached_estimate) = self.get_cached_buy_priority_fee(cache_key) {
            return PriorityFeeBackgroundLaunch::Cached(cached_estimate);
        }

        let started_at = Instant::now();
        let (tx, rx) = watch::channel(None::<PriorityFeeEstimate>);
        let mut guard = self.priority_fee_inflight.lock().await;
        if let Some(cached_estimate) = self.get_cached_buy_priority_fee(cache_key) {
            return PriorityFeeBackgroundLaunch::Cached(cached_estimate);
        }
        if let Some(inflight) = guard.get(cache_key).cloned() {
            return PriorityFeeBackgroundLaunch::Join(inflight);
        }
        guard.insert(
            cache_key.clone(),
            InflightRefresh {
                started_at,
                receiver: rx,
            },
        );
        PriorityFeeBackgroundLaunch::Start {
            started_at,
            sender: tx,
        }
    }

    pub async fn send_transaction(
        &self,
        transaction: &VersionedTransaction,
    ) -> Result<SenderTransactionSubmission, LiveTxSenderError> {
        #[cfg(test)]
        if let Some(mock_result) = self.try_mock_send_transaction(transaction) {
            return mock_result;
        }

        let encoded =
            serialize_transaction_base64(transaction).map_err(|err| LiveTxSenderError::Submit {
                message: format!("serialize transaction: {err}"),
            })?;

        let response = self
            .http
            .post(&self.config.sender_endpoint)
            .timeout(Duration::from_millis(HELIUS_SENDER_SUBMIT_TIMEOUT_MS))
            .json(&json!({
                "jsonrpc": "2.0",
                "id": "ghost-live-sender",
                "method": "sendTransaction",
                "params": [
                    encoded,
                    {
                        "encoding": "base64",
                        "skipPreflight": true,
                        "maxRetries": 0
                    }
                ]
            }))
            .send()
            .await
            .map_err(|err| LiveTxSenderError::Submit {
                message: format!("request error: {err}"),
            })?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|err| LiveTxSenderError::Submit {
                message: format!("failed to read response body: {err}"),
            })?;

        if !status.is_success() {
            return Err(LiveTxSenderError::Submit {
                message: format!("HTTP {status}: {body}"),
            });
        }

        let value: Value =
            serde_json::from_str(&body).map_err(|err| LiveTxSenderError::Submit {
                message: format!("invalid JSON response: {err}; body={body}"),
            })?;

        if let Some(error) = value.get("error") {
            return Err(LiveTxSenderError::Submit {
                message: error.to_string(),
            });
        }

        let signature_str = value.get("result").and_then(Value::as_str).ok_or_else(|| {
            LiveTxSenderError::Submit {
                message: format!("response missing result field: {value}"),
            }
        })?;
        let signature =
            Signature::from_str(signature_str).map_err(|err| LiveTxSenderError::Submit {
                message: format!("response returned invalid signature {signature_str}: {err}"),
            })?;

        Ok(SenderTransactionSubmission { signature })
    }

    pub async fn confirm_submission(
        &self,
        submission: &SenderTransactionSubmission,
    ) -> Result<SenderConfirmedTransaction, LiveTxSenderError> {
        self.confirm_submission_with_timeout(submission, HELIUS_SENDER_CONFIRM_TIMEOUT_MS)
            .await
    }

    pub async fn confirm_submission_with_timeout(
        &self,
        submission: &SenderTransactionSubmission,
        timeout_ms: u64,
    ) -> Result<SenderConfirmedTransaction, LiveTxSenderError> {
        #[cfg(test)]
        if let Some(mock_result) = self.try_mock_confirm_submission(submission) {
            return mock_result;
        }

        let mut tx_status_filters = HashMap::new();
        tx_status_filters.insert(
            "sender_signature".to_string(),
            SubscribeRequestFilterTransactions {
                vote: Some(false),
                failed: None,
                signature: Some(submission.signature.to_string()),
                account_include: vec![],
                account_exclude: vec![],
                account_required: vec![],
            },
        );

        let request = SubscribeRequest {
            accounts: HashMap::new(),
            slots: HashMap::new(),
            transactions: HashMap::new(),
            transactions_status: tx_status_filters,
            blocks: HashMap::new(),
            blocks_meta: HashMap::new(),
            entry: HashMap::new(),
            commitment: Some(CommitmentLevel::Confirmed as i32),
            accounts_data_slice: vec![],
            ping: None,
        };

        let endpoint = normalize_grpc_endpoint(&self.config.yellowstone_grpc_endpoint);
        let x_token = self.config.yellowstone_x_token.trim().to_string();
        let signature = submission.signature;

        let confirm_future = async {
            let mut client = GeyserGrpcClient::build_from_shared(endpoint.clone())
                .map_err(|err| LiveTxSenderError::ConfirmationTransport {
                    signature,
                    message: format!("build Yellowstone client for {endpoint}: {err}"),
                })?
                .x_token(Some(x_token))
                .map_err(|err| LiveTxSenderError::ConfirmationTransport {
                    signature,
                    message: format!("apply Yellowstone x-token for {endpoint}: {err}"),
                })?
                .http2_adaptive_window(true)
                .keep_alive_while_idle(true)
                .http2_keep_alive_interval(Duration::from_secs(10))
                .keep_alive_timeout(Duration::from_secs(5))
                .tcp_nodelay(true)
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(15))
                .connect()
                .await
                .map_err(|err| LiveTxSenderError::ConfirmationTransport {
                    signature,
                    message: format!("connect Yellowstone gRPC {endpoint}: {err}"),
                })?;

            let (_, mut stream) =
                client
                    .subscribe_with_request(Some(request))
                    .await
                    .map_err(|err| LiveTxSenderError::ConfirmationTransport {
                        signature,
                        message: format!("subscribe_with_request failed: {err}"),
                    })?;

            while let Some(message) = stream.next().await {
                let update = message.map_err(|err| LiveTxSenderError::ConfirmationTransport {
                    signature,
                    message: format!("stream receive failed: {err}"),
                })?;

                match update.update_oneof {
                    Some(UpdateOneof::TransactionStatus(status)) => {
                        if status.err.is_some() {
                            return Err(LiveTxSenderError::ConfirmationRejected {
                                signature,
                                slot: status.slot,
                            });
                        }

                        return Ok(SenderConfirmedTransaction {
                            signature,
                            landed_slot: Some(status.slot),
                        });
                    }
                    Some(UpdateOneof::Ping(_)) | Some(UpdateOneof::Pong(_)) => continue,
                    _ => continue,
                }
            }

            Err(LiveTxSenderError::ConfirmationTransport {
                signature,
                message: "Yellowstone stream closed before confirmation update arrived".to_string(),
            })
        };

        tokio::time::timeout(Duration::from_millis(timeout_ms), confirm_future)
            .await
            .map_err(|_| LiveTxSenderError::ConfirmationTimeout {
                signature,
                timeout_ms,
            })?
    }

    fn get_cached_tip_floor(&self) -> Option<CachedTipFloor> {
        self.tip_floor_cache
            .read()
            .ok()
            .and_then(|guard| guard.clone())
    }

    fn store_cached_tip_floor(&self, lamports: u64) {
        if let Ok(mut guard) = self.tip_floor_cache.write() {
            *guard = Some(CachedTipFloor {
                lamports,
                fetched_at: Instant::now(),
            });
        }
    }

    fn fresh_cached_tip_floor_resolution(&self) -> Option<TipFloorResolution> {
        let cached_tip_floor = self.get_cached_tip_floor()?;
        let cache_age_ms = cached_tip_floor.age_ms();
        (cache_age_ms <= TIP_FLOOR_CACHE_TTL_MS)
            .then(|| TipFloorResolution::fresh_cache(cached_tip_floor.lamports, cache_age_ms))
    }

    async fn resolve_tip_floor_resolution_from_endpoint(
        &self,
        endpoint: &str,
        intent: RefreshIntent,
    ) -> TipFloorResolution {
        if let Some(cached_resolution) = self.fresh_cached_tip_floor_resolution() {
            return cached_resolution;
        }

        match self.maybe_join_tip_floor_inflight(intent).await {
            InflightJoinOutcome::Joined(inflight_resolution) => return inflight_resolution,
            InflightJoinOutcome::TimedOut => {
                let fetched_resolution = self.fetch_tip_floor_resolution_uncached(endpoint).await;
                return self
                    .materialize_tip_floor_resolution(fetched_resolution, endpoint)
                    .with_inflight_join("timed_out_fallback", 0);
            }
            InflightJoinOutcome::NotFound => {}
        }

        let started_at = Instant::now();
        let (tx, rx) = watch::channel(None::<TipFloorResolution>);
        let should_fallback_direct = {
            let mut guard = self.tip_floor_inflight.lock().await;
            if let Some(cached_resolution) = self.fresh_cached_tip_floor_resolution() {
                return cached_resolution;
            }
            if guard.is_some() {
                true
            } else {
                *guard = Some(InflightRefresh {
                    started_at,
                    receiver: rx,
                });
                false
            }
        };
        if should_fallback_direct {
            let fetched_resolution = self.fetch_tip_floor_resolution_uncached(endpoint).await;
            return self
                .materialize_tip_floor_resolution(fetched_resolution, endpoint)
                .with_inflight_join("timed_out_fallback", 0);
        }

        let fetched_resolution = self.fetch_tip_floor_resolution_uncached(endpoint).await;
        if let Some(floor_lamports) = fetched_resolution.floor_lamports {
            self.store_cached_tip_floor(floor_lamports);
        }
        let resolved = self.materialize_tip_floor_resolution(fetched_resolution, endpoint);
        let _ = tx.send(Some(resolved.clone()));
        let mut guard = self.tip_floor_inflight.lock().await;
        if guard
            .as_ref()
            .map(|inflight| inflight.started_at == started_at)
            .unwrap_or(false)
        {
            *guard = None;
        }
        resolved
    }

    fn materialize_tip_floor_resolution(
        &self,
        fetched_resolution: TipFloorResolution,
        endpoint: &str,
    ) -> TipFloorResolution {
        if let Some(floor_lamports) = fetched_resolution.floor_lamports {
            return TipFloorResolution::refreshed(
                floor_lamports,
                fetched_resolution.telemetry.fetch_latency_ms,
            );
        }

        if let Some(cached_tip_floor) = self.get_cached_tip_floor() {
            let cache_age_ms = cached_tip_floor.age_ms();
            if cache_age_ms <= TIP_FLOOR_STALE_LAST_GOOD_MAX_AGE_MS {
                warn!(
                    endpoint,
                    cache_mode = "stale_last_good",
                    cache_age_ms,
                    fetch_latency_ms = fetched_resolution.telemetry.fetch_latency_ms,
                    failed_source = fetched_resolution.telemetry.source,
                    "tip floor refresh failed; using stale last known good"
                );
                return TipFloorResolution::stale_last_good(
                    cached_tip_floor.lamports,
                    cache_age_ms,
                    fetched_resolution.telemetry.fetch_latency_ms,
                );
            }
        }

        fetched_resolution
    }

    async fn maybe_join_tip_floor_inflight(
        &self,
        intent: RefreshIntent,
    ) -> InflightJoinOutcome<TipFloorResolution> {
        let inflight = self.tip_floor_inflight.lock().await.clone();
        let Some(inflight) = inflight else {
            return InflightJoinOutcome::NotFound;
        };
        self.await_tip_floor_inflight(inflight, intent).await
    }

    async fn await_tip_floor_inflight(
        &self,
        inflight: InflightRefresh<TipFloorResolution>,
        intent: RefreshIntent,
    ) -> InflightJoinOutcome<TipFloorResolution> {
        let mut receiver = inflight.receiver.clone();
        if let Some(resolution) = receiver.borrow().clone() {
            return InflightJoinOutcome::Joined(resolution.with_inflight_join("joined", 0));
        }

        let wait_started_at = Instant::now();
        let joined = async {
            loop {
                if receiver.changed().await.is_err() {
                    return None;
                }
                if let Some(resolution) = receiver.borrow().clone() {
                    return Some(resolution);
                }
            }
        };
        let joined = if let Some(max_total_ms) = intent.max_total_inflight_ms("tip_floor") {
            let elapsed_before_join_ms = elapsed_ms(inflight.started_at);
            let remaining_wait_ms = max_total_ms.saturating_sub(elapsed_before_join_ms);
            if remaining_wait_ms == 0 {
                return InflightJoinOutcome::TimedOut;
            }
            tokio::time::timeout(Duration::from_millis(remaining_wait_ms), joined)
                .await
                .ok()
                .flatten()
        } else {
            joined.await
        };
        match joined {
            Some(resolution) => InflightJoinOutcome::Joined(
                resolution.with_inflight_join("joined", elapsed_ms(wait_started_at)),
            ),
            None => InflightJoinOutcome::TimedOut,
        }
    }

    async fn fetch_tip_floor_resolution_uncached(&self, endpoint: &str) -> TipFloorResolution {
        let fetch_started_at = Instant::now();
        let response = match self
            .http
            .get(endpoint)
            .timeout(Duration::from_millis(TIP_FLOOR_TIMEOUT_MS))
            .send()
            .await
        {
            Ok(response) => response,
            Err(err) => {
                warn!(
                    error = %err,
                    endpoint,
                    "tip floor fetch fell back because Jito endpoint request failed"
                );
                return TipFloorResolution::failed(
                    elapsed_ms(fetch_started_at),
                    "fallback_request",
                );
            }
        };

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable body>".to_string());
            warn!(
                status = %status,
                endpoint,
                body,
                "tip floor fetch fell back because Jito endpoint returned non-success"
            );
            return TipFloorResolution::failed(
                elapsed_ms(fetch_started_at),
                "fallback_http_status",
            );
        }

        let floors = match response.json::<Vec<TipFloorEntry>>().await {
            Ok(floors) => floors,
            Err(err) => {
                warn!(
                    error = %err,
                    endpoint,
                    "tip floor fetch fell back because Jito response could not be decoded"
                );
                return TipFloorResolution::failed(elapsed_ms(fetch_started_at), "fallback_json");
            }
        };

        let floor_lamports = select_tip_floor_lamports(&floors);
        if let Some(floor_lamports) = floor_lamports {
            return TipFloorResolution::refreshed(floor_lamports, elapsed_ms(fetch_started_at));
        }

        warn!(
            endpoint,
            "tip floor fetch fell back because Jito response did not contain a usable floor"
        );
        TipFloorResolution::failed(elapsed_ms(fetch_started_at), "fallback_response")
    }

    #[cfg(test)]
    pub(crate) fn cached_tip_floor_lamports(&self) -> Option<u64> {
        self.get_cached_tip_floor().map(|cached| cached.lamports)
    }

    #[cfg(test)]
    fn try_mock_send_transaction(
        &self,
        transaction: &VersionedTransaction,
    ) -> Option<Result<SenderTransactionSubmission, LiveTxSenderError>> {
        match self.config.sender_endpoint.as_str() {
            "test://sender-success" => Some(Ok(SenderTransactionSubmission {
                signature: transaction.signatures[0],
            })),
            "test://sender-submit-fail" => Some(Err(LiveTxSenderError::Submit {
                message: "mock sender submit failure".to_string(),
            })),
            _ => None,
        }
    }

    #[cfg(test)]
    fn try_mock_confirm_submission(
        &self,
        submission: &SenderTransactionSubmission,
    ) -> Option<Result<SenderConfirmedTransaction, LiveTxSenderError>> {
        match self.config.yellowstone_grpc_endpoint.as_str() {
            "test://yellowstone-confirmed" => Some(Ok(SenderConfirmedTransaction {
                signature: submission.signature,
                landed_slot: Some(777),
            })),
            "test://yellowstone-resource-exhausted" => {
                Some(Err(LiveTxSenderError::ConfirmationTransport {
                    signature: submission.signature,
                    message: "subscribe_with_request failed: status: ResourceExhausted, message: Concurrent Yellowstone Geyser stream limit reached".to_string(),
                }))
            }
            "test://yellowstone-rejected" => Some(Err(LiveTxSenderError::ConfirmationRejected {
                signature: submission.signature,
                slot: 777,
            })),
            _ => None,
        }
    }
}

fn parse_priority_fee_micro_lamports_response(body: &str) -> Option<u64> {
    let value: Value = serde_json::from_str(body).ok()?;
    parse_priority_fee_micro_lamports_value(value.get("result")?.get("priorityFeeEstimate")?)
}

fn parse_priority_fee_micro_lamports_value(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => {
            if let Some(priority_fee) = number.as_u64() {
                return Some(priority_fee);
            }

            number
                .as_f64()
                .filter(|priority_fee| {
                    priority_fee.is_finite()
                        && *priority_fee > 0.0
                        && *priority_fee <= u64::MAX as f64
                })
                .map(|priority_fee| priority_fee.ceil() as u64)
        }
        Value::String(raw_priority_fee) => raw_priority_fee
            .parse::<f64>()
            .ok()
            .filter(|priority_fee| {
                priority_fee.is_finite() && *priority_fee > 0.0 && *priority_fee <= u64::MAX as f64
            })
            .map(|priority_fee| priority_fee.ceil() as u64),
        _ => None,
    }
}

pub fn resolve_live_sender_endpoint() -> String {
    std::env::var(HELIUS_SENDER_ENDPOINT_ENV)
        .ok()
        .map(|raw| raw.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| HELIUS_SENDER_ENDPOINT.to_string())
}

pub fn sender_ping_endpoint(sender_endpoint: &str) -> String {
    if let Some(prefix) = sender_endpoint.strip_suffix("/fast") {
        return format!("{prefix}/ping");
    }

    format!("{}/ping", sender_endpoint.trim_end_matches('/'))
}

pub fn select_sender_tip_account(seed_material: &[u8]) -> Pubkey {
    let digest = blake3::hash(seed_material);
    let mut seed = [0u8; 8];
    seed.copy_from_slice(&digest.as_bytes()[..8]);
    let index = (u64::from_le_bytes(seed) as usize) % HELIUS_SENDER_TIP_ACCOUNTS.len();
    Pubkey::from_str(HELIUS_SENDER_TIP_ACCOUNTS[index]).expect("valid Helius Sender tip account")
}

fn select_tip_floor_lamports(floors: &[TipFloorEntry]) -> Option<u64> {
    let tip_sol = floors.iter().find_map(|entry| {
        entry
            .landed_tips_75th_percentile
            .or(entry.landed_tips_50th_percentile)
            .or(entry.landed_tips_25th_percentile)
    })?;
    if !tip_sol.is_finite() || tip_sol <= 0.0 {
        return None;
    }

    let lamports = (tip_sol * 1_000_000_000.0 * TIP_FLOOR_BUFFER_MULTIPLIER).ceil() as u64;
    Some(lamports.max(HELIUS_SENDER_MIN_TIP_LAMPORTS))
}

fn select_buy_tip_lamports(dynamic_floor_lamports: Option<u64>) -> u64 {
    match dynamic_floor_lamports {
        Some(dynamic_floor) if dynamic_floor > HELIUS_SENDER_BUY_BASELINE_TIP_LAMPORTS => {
            dynamic_floor.saturating_mul(2)
        }
        _ => HELIUS_SENDER_BUY_BASELINE_TIP_LAMPORTS,
    }
}

fn elapsed_ms(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis().min(u64::MAX as u128) as u64
}

fn normalize_grpc_endpoint(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    }
}

fn serialize_transaction_base64(transaction: &VersionedTransaction) -> Result<String> {
    let wire_bytes = bincode::serialize(transaction).context("serialize versioned transaction")?;
    Ok(BASE64_STANDARD.encode(wire_bytes))
}

pub async fn probe_sender_endpoint(sender_endpoint: &str) -> Result<()> {
    let ping_endpoint = sender_ping_endpoint(sender_endpoint);
    let status = Client::builder()
        .connect_timeout(Duration::from_millis(300))
        .timeout(Duration::from_secs(3))
        .build()
        .context("build sender probe client")?
        .get(&ping_endpoint)
        .send()
        .await
        .with_context(|| format!("Sender ping failed for {ping_endpoint}"))?
        .error_for_status()
        .with_context(|| format!("Sender ping returned non-success for {ping_endpoint}"))?;
    let _ = status;
    Ok(())
}

pub async fn probe_priority_fee_rpc(priority_fee_rpc_url: &str) -> Result<String> {
    let response = Client::builder()
        .connect_timeout(Duration::from_millis(300))
        .timeout(Duration::from_secs(3))
        .build()
        .context("build Helius priority fee probe client")?
        .post(priority_fee_rpc_url)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": "ghost-live-rpc-probe",
            "method": "getVersion"
        }))
        .send()
        .await
        .with_context(|| format!("Helius RPC getVersion failed for {priority_fee_rpc_url}"))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .context("read Helius RPC probe body")?;
    if !status.is_success() {
        return Err(anyhow!(
            "Helius RPC getVersion returned HTTP {status} for {priority_fee_rpc_url}: {body}"
        ));
    }

    let value: Value =
        serde_json::from_str(&body).context("parse Helius RPC getVersion JSON response")?;
    if let Some(error) = value.get("error") {
        return Err(anyhow!(
            "Helius RPC getVersion returned JSON-RPC error for {priority_fee_rpc_url}: {error}"
        ));
    }

    Ok(value
        .get("result")
        .and_then(|result| result.get("solana-core"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        parse_priority_fee_micro_lamports_value, select_buy_tip_lamports,
        select_sender_tip_account, select_tip_floor_lamports, serialize_transaction_base64,
        LiveTxSender, LiveTxSenderConfig, PriorityFeeCacheKey, DEFAULT_PRIORITY_LEVEL,
        DEFAULT_PRIORITY_TRANSACTION_ENCODING, HELIUS_SENDER_BUY_BASELINE_TIP_LAMPORTS,
        HELIUS_SENDER_MIN_TIP_LAMPORTS, PRIORITY_FEE_CACHE_TTL_MS,
        PRIORITY_FEE_INFLIGHT_JOIN_MAX_TOTAL_MS, TIP_FLOOR_CACHE_TTL_MS,
        TIP_FLOOR_STALE_LAST_GOOD_MAX_AGE_MS,
    };
    use serde_json::{json, Value};
    use solana_sdk::{
        hash::Hash,
        message::{v0, VersionedMessage},
        pubkey::Pubkey,
        signature::Keypair,
        signer::Signer,
        system_instruction,
        transaction::VersionedTransaction,
    };
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::oneshot;

    #[test]
    fn test_select_tip_floor_lamports_applies_buffer_and_minimum() {
        let lamports = select_tip_floor_lamports(&[super::TipFloorEntry {
            landed_tips_75th_percentile: Some(0.0003),
            landed_tips_50th_percentile: None,
            landed_tips_25th_percentile: None,
        }])
        .expect("tip floor");

        assert_eq!(lamports, 540_000);
    }

    #[test]
    fn test_select_tip_floor_lamports_enforces_sender_minimum() {
        let lamports = select_tip_floor_lamports(&[super::TipFloorEntry {
            landed_tips_75th_percentile: Some(0.00001),
            landed_tips_50th_percentile: None,
            landed_tips_25th_percentile: None,
        }])
        .expect("tip floor");

        assert_eq!(lamports, HELIUS_SENDER_MIN_TIP_LAMPORTS);
    }

    #[test]
    fn test_select_buy_tip_lamports_uses_sender_baseline_below_hype_floor() {
        assert_eq!(
            select_buy_tip_lamports(Some(540_000)),
            HELIUS_SENDER_BUY_BASELINE_TIP_LAMPORTS
        );
        assert_eq!(
            select_buy_tip_lamports(None),
            HELIUS_SENDER_BUY_BASELINE_TIP_LAMPORTS
        );
    }

    #[test]
    fn test_select_buy_tip_lamports_doubles_hype_floor() {
        let dynamic_floor = HELIUS_SENDER_BUY_BASELINE_TIP_LAMPORTS + 100_000;
        assert_eq!(
            select_buy_tip_lamports(Some(dynamic_floor)),
            dynamic_floor.saturating_mul(2)
        );
    }

    #[test]
    fn test_select_sender_tip_account_is_stable() {
        let tip_account = select_sender_tip_account(b"ghost-test-tip-seed");

        assert_eq!(
            tip_account.to_string(),
            "2nyhqdwKcJZR2vcqCyrYsaPVdAnFoJjiksCXJ7hfEYgD"
        );
    }

    fn build_test_versioned_transaction() -> VersionedTransaction {
        let payer = Keypair::new();
        let recipient = Keypair::new();
        let recent_blockhash = Hash::new_unique();
        let transfer_ix = system_instruction::transfer(&payer.pubkey(), &recipient.pubkey(), 1_000);
        let message =
            v0::Message::try_compile(&payer.pubkey(), &[transfer_ix], &[], recent_blockhash)
                .expect("compile test message");
        VersionedTransaction::try_new(VersionedMessage::V0(message), &[&payer])
            .expect("sign test tx")
    }

    async fn spawn_priority_fee_capture_server(
        response_body: &'static str,
    ) -> (String, oneshot::Receiver<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind priority fee capture server");
        let addr = listener.local_addr().expect("priority fee capture addr");
        let (request_tx, request_rx) = oneshot::channel();

        tokio::spawn(async move {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let mut buffer = vec![0u8; 16_384];
            let n = match stream.read(&mut buffer).await {
                Ok(n) if n > 0 => n,
                _ => return,
            };
            let request = String::from_utf8_lossy(&buffer[..n]).to_string();
            let _ = request_tx.send(request);

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = stream.shutdown().await;
        });

        (format!("http://{}", addr), request_rx)
    }

    async fn spawn_priority_fee_server(
        responses: Vec<(u16, &'static str)>,
    ) -> (String, Arc<AtomicUsize>) {
        spawn_priority_fee_server_with_delay(
            responses
                .into_iter()
                .map(|(status, body)| (status, body, 0))
                .collect(),
        )
        .await
    }

    async fn spawn_priority_fee_server_with_delay(
        responses: Vec<(u16, &'static str, u64)>,
    ) -> (String, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind priority fee server");
        let addr = listener.local_addr().expect("priority fee addr");
        let request_count = Arc::new(AtomicUsize::new(0));
        let responses = Arc::new(responses);

        tokio::spawn({
            let request_count = Arc::clone(&request_count);
            let responses = Arc::clone(&responses);
            async move {
                loop {
                    let Ok((mut stream, _)) = listener.accept().await else {
                        return;
                    };
                    let mut buffer = vec![0u8; 16_384];
                    let n = match stream.read(&mut buffer).await {
                        Ok(n) if n > 0 => n,
                        _ => continue,
                    };
                    let _request = String::from_utf8_lossy(&buffer[..n]).to_string();
                    let request_index = request_count.fetch_add(1, Ordering::Relaxed);
                    let (status_code, response_body, response_delay_ms) = responses
                        .get(request_index)
                        .copied()
                        .unwrap_or_else(|| *responses.last().expect("last priority fee response"));
                    let status_text = if status_code == 200 {
                        "OK"
                    } else {
                        "Internal Server Error"
                    };
                    if response_delay_ms > 0 {
                        tokio::time::sleep(Duration::from_millis(response_delay_ms)).await;
                    }

                    let response = format!(
                        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                        status_code,
                        status_text,
                        response_body.len(),
                        response_body
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.shutdown().await;
                }
            }
        });

        (format!("http://{}", addr), request_count)
    }

    fn test_buy_priority_fee_cache_key(
        token_program: Pubkey,
        ata_missing_pre_submit: bool,
        has_inline_tip: bool,
    ) -> PriorityFeeCacheKey {
        PriorityFeeCacheKey::buy(
            "routed_exact_sol_in",
            token_program,
            ata_missing_pre_submit,
            has_inline_tip,
        )
    }

    async fn spawn_tip_floor_server(
        responses: Vec<(u16, &'static str)>,
    ) -> (String, Arc<AtomicUsize>) {
        spawn_tip_floor_server_with_delay(
            responses
                .into_iter()
                .map(|(status, body)| (status, body, 0))
                .collect(),
        )
        .await
    }

    async fn spawn_tip_floor_server_with_delay(
        responses: Vec<(u16, &'static str, u64)>,
    ) -> (String, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind tip floor server");
        let addr = listener.local_addr().expect("tip floor addr");
        let request_count = Arc::new(AtomicUsize::new(0));
        let responses = Arc::new(responses);

        tokio::spawn({
            let request_count = Arc::clone(&request_count);
            let responses = Arc::clone(&responses);
            async move {
                loop {
                    let Ok((mut stream, _)) = listener.accept().await else {
                        return;
                    };
                    let mut buffer = vec![0u8; 16_384];
                    let n = match stream.read(&mut buffer).await {
                        Ok(n) if n > 0 => n,
                        _ => continue,
                    };
                    let _request = String::from_utf8_lossy(&buffer[..n]).to_string();
                    let request_index = request_count.fetch_add(1, Ordering::Relaxed);
                    let (status_code, response_body, response_delay_ms) = responses
                        .get(request_index)
                        .copied()
                        .unwrap_or_else(|| *responses.last().expect("last tip floor response"));
                    let status_text = if status_code == 200 {
                        "OK"
                    } else {
                        "Internal Server Error"
                    };
                    if response_delay_ms > 0 {
                        tokio::time::sleep(Duration::from_millis(response_delay_ms)).await;
                    }

                    let response = format!(
                        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                        status_code,
                        status_text,
                        response_body.len(),
                        response_body
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.shutdown().await;
                }
            }
        });

        (format!("http://{}", addr), request_count)
    }

    #[tokio::test]
    async fn test_estimate_priority_fee_uses_base64_encoding_option() {
        let response_body =
            "{\"jsonrpc\":\"2.0\",\"result\":{\"priorityFeeEstimate\":42000},\"id\":\"ghost-live-priority-fee\"}";
        let (priority_fee_rpc_url, request_rx) =
            spawn_priority_fee_capture_server(response_body).await;
        let sender = LiveTxSender::new(LiveTxSenderConfig::new(
            "http://127.0.0.1:1/fast",
            priority_fee_rpc_url,
            "http://127.0.0.1:1",
            "test-yellowstone-token",
        ))
        .expect("test live tx sender");
        let transaction = build_test_versioned_transaction();

        let estimate = sender
            .estimate_priority_fee_micro_lamports(&transaction)
            .await;

        assert_eq!(estimate, 42_000);

        let request = request_rx.await.expect("captured priority fee request");
        let body = request
            .split("\r\n\r\n")
            .nth(1)
            .expect("http request body must exist");
        let json: Value = serde_json::from_str(body).expect("request body must be valid json");
        let serialized_transaction =
            serialize_transaction_base64(&transaction).expect("serialized transaction");
        let params = json
            .get("params")
            .and_then(Value::as_array)
            .expect("params array");
        let options = params[0].get("options").expect("options object");
        assert_eq!(
            params[0].get("transaction").and_then(Value::as_str),
            Some(serialized_transaction.as_str())
        );
        assert_eq!(
            options.get("transactionEncoding").and_then(Value::as_str),
            Some(DEFAULT_PRIORITY_TRANSACTION_ENCODING)
        );
        assert_eq!(
            options.get("priorityLevel").and_then(Value::as_str),
            Some(DEFAULT_PRIORITY_LEVEL)
        );
    }

    #[tokio::test]
    async fn test_estimate_buy_priority_fee_with_telemetry_reports_rpc_source() {
        let response_body =
            "{\"jsonrpc\":\"2.0\",\"result\":{\"priorityFeeEstimate\":42000},\"id\":\"ghost-live-priority-fee\"}";
        let (priority_fee_rpc_url, _request_rx) =
            spawn_priority_fee_capture_server(response_body).await;
        let sender = LiveTxSender::new(LiveTxSenderConfig::new(
            "http://127.0.0.1:1/fast",
            priority_fee_rpc_url,
            "http://127.0.0.1:1",
            "test-yellowstone-token",
        ))
        .expect("test live tx sender");
        let transaction = build_test_versioned_transaction();

        let estimate = sender
            .estimate_buy_priority_fee_micro_lamports_with_telemetry(&transaction, None)
            .await;

        assert_eq!(estimate.micro_lamports, 42_000);
        assert_eq!(estimate.telemetry.source, "helius_rpc");
        assert!(!estimate.telemetry.cache_hit);
        assert_eq!(estimate.telemetry.cache_age_ms, 0);
        assert_eq!(estimate.telemetry.cache_mode, "miss_refresh");
    }

    #[tokio::test]
    async fn test_buy_priority_fee_cache_miss_fetches_and_stores() {
        let response_body =
            "{\"jsonrpc\":\"2.0\",\"result\":{\"priorityFeeEstimate\":42000},\"id\":\"ghost-live-priority-fee\"}";
        let (priority_fee_rpc_url, request_count) =
            spawn_priority_fee_server(vec![(200, response_body)]).await;
        let sender = LiveTxSender::new(LiveTxSenderConfig::new(
            "http://127.0.0.1:1/fast",
            priority_fee_rpc_url,
            "http://127.0.0.1:1",
            "test-yellowstone-token",
        ))
        .expect("test live tx sender");
        let transaction = build_test_versioned_transaction();
        let cache_key = test_buy_priority_fee_cache_key(Pubkey::new_unique(), true, true);

        let first_estimate = sender
            .estimate_buy_priority_fee_micro_lamports_with_telemetry(&transaction, Some(&cache_key))
            .await;
        let cached_estimate = sender
            .get_cached_buy_priority_fee(&cache_key)
            .expect("cached estimate must be stored");

        assert_eq!(first_estimate.micro_lamports, 42_000);
        assert_eq!(first_estimate.telemetry.source, "helius_rpc");
        assert_eq!(first_estimate.telemetry.cache_mode, "miss_refresh");
        assert!(!first_estimate.telemetry.cache_hit);
        assert_eq!(cached_estimate.micro_lamports, 42_000);
        assert_eq!(cached_estimate.telemetry.source, "priority_fee_cache");
        assert_eq!(cached_estimate.telemetry.cache_mode, "fresh_cache");
        assert!(cached_estimate.telemetry.cache_hit);
        assert_eq!(request_count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_buy_priority_fee_cache_hit_is_shared_across_sender_clone() {
        let response_body =
            "{\"jsonrpc\":\"2.0\",\"result\":{\"priorityFeeEstimate\":42000},\"id\":\"ghost-live-priority-fee\"}";
        let (priority_fee_rpc_url, request_count) =
            spawn_priority_fee_server(vec![(200, response_body)]).await;
        let sender = LiveTxSender::new(LiveTxSenderConfig::new(
            "http://127.0.0.1:1/fast",
            priority_fee_rpc_url,
            "http://127.0.0.1:1",
            "test-yellowstone-token",
        ))
        .expect("test live tx sender");
        let sender_clone = sender.clone();
        let transaction = build_test_versioned_transaction();
        let cache_key = test_buy_priority_fee_cache_key(Pubkey::new_unique(), false, true);

        let first_estimate = sender
            .estimate_buy_priority_fee_micro_lamports_with_telemetry(&transaction, Some(&cache_key))
            .await;
        let second_estimate = sender_clone
            .estimate_buy_priority_fee_micro_lamports_with_telemetry(&transaction, Some(&cache_key))
            .await;

        assert_eq!(first_estimate.telemetry.source, "helius_rpc");
        assert_eq!(second_estimate.micro_lamports, 42_000);
        assert_eq!(second_estimate.telemetry.source, "priority_fee_cache");
        assert_eq!(second_estimate.telemetry.cache_mode, "fresh_cache");
        assert!(second_estimate.telemetry.cache_hit);
        assert_eq!(request_count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_buy_priority_fee_cache_is_keyed_by_buy_class() {
        let first_response =
            "{\"jsonrpc\":\"2.0\",\"result\":{\"priorityFeeEstimate\":42000},\"id\":\"ghost-live-priority-fee\"}";
        let second_response =
            "{\"jsonrpc\":\"2.0\",\"result\":{\"priorityFeeEstimate\":55000},\"id\":\"ghost-live-priority-fee\"}";
        let (priority_fee_rpc_url, request_count) =
            spawn_priority_fee_server(vec![(200, first_response), (200, second_response)]).await;
        let sender = LiveTxSender::new(LiveTxSenderConfig::new(
            "http://127.0.0.1:1/fast",
            priority_fee_rpc_url,
            "http://127.0.0.1:1",
            "test-yellowstone-token",
        ))
        .expect("test live tx sender");
        let transaction = build_test_versioned_transaction();
        let token_program = Pubkey::new_unique();
        let missing_ata_key = test_buy_priority_fee_cache_key(token_program, true, true);
        let existing_ata_key = test_buy_priority_fee_cache_key(token_program, false, true);

        let first_estimate = sender
            .estimate_buy_priority_fee_micro_lamports_with_telemetry(
                &transaction,
                Some(&missing_ata_key),
            )
            .await;
        let second_estimate = sender
            .estimate_buy_priority_fee_micro_lamports_with_telemetry(
                &transaction,
                Some(&existing_ata_key),
            )
            .await;
        let first_cached = sender
            .estimate_buy_priority_fee_micro_lamports_with_telemetry(
                &transaction,
                Some(&missing_ata_key),
            )
            .await;

        assert_eq!(first_estimate.micro_lamports, 42_000);
        assert_eq!(second_estimate.micro_lamports, 55_000);
        assert_eq!(second_estimate.telemetry.source, "helius_rpc");
        assert_eq!(first_cached.micro_lamports, 42_000);
        assert_eq!(first_cached.telemetry.source, "priority_fee_cache");
        assert!(first_cached.telemetry.cache_hit);
        assert_eq!(request_count.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn test_buy_priority_fee_cache_stale_entry_refreshes_on_success() {
        let first_response =
            "{\"jsonrpc\":\"2.0\",\"result\":{\"priorityFeeEstimate\":42000},\"id\":\"ghost-live-priority-fee\"}";
        let second_response =
            "{\"jsonrpc\":\"2.0\",\"result\":{\"priorityFeeEstimate\":61000},\"id\":\"ghost-live-priority-fee\"}";
        let (priority_fee_rpc_url, request_count) =
            spawn_priority_fee_server(vec![(200, first_response), (200, second_response)]).await;
        let sender = LiveTxSender::new(LiveTxSenderConfig::new(
            "http://127.0.0.1:1/fast",
            priority_fee_rpc_url,
            "http://127.0.0.1:1",
            "test-yellowstone-token",
        ))
        .expect("test live tx sender");
        let transaction = build_test_versioned_transaction();
        let cache_key = test_buy_priority_fee_cache_key(Pubkey::new_unique(), true, true);

        let first_estimate = sender
            .estimate_buy_priority_fee_micro_lamports_with_telemetry(&transaction, Some(&cache_key))
            .await;
        tokio::time::sleep(Duration::from_millis(PRIORITY_FEE_CACHE_TTL_MS + 25)).await;
        let refreshed_estimate = sender
            .estimate_buy_priority_fee_micro_lamports_with_telemetry(&transaction, Some(&cache_key))
            .await;

        assert_eq!(first_estimate.micro_lamports, 42_000);
        assert_eq!(refreshed_estimate.micro_lamports, 61_000);
        assert_eq!(refreshed_estimate.telemetry.source, "helius_rpc");
        assert_eq!(refreshed_estimate.telemetry.cache_mode, "miss_refresh");
        assert!(!refreshed_estimate.telemetry.cache_hit);
        assert_eq!(request_count.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn test_hot_path_priority_fee_joins_inflight_prewarm_for_same_buy_class() {
        let response_body =
            "{\"jsonrpc\":\"2.0\",\"result\":{\"priorityFeeEstimate\":42000},\"id\":\"ghost-live-priority-fee\"}";
        let (priority_fee_rpc_url, request_count) =
            spawn_priority_fee_server_with_delay(vec![(200, response_body, 80)]).await;
        let sender = LiveTxSender::new(LiveTxSenderConfig::new(
            "http://127.0.0.1:1/fast",
            priority_fee_rpc_url,
            "http://127.0.0.1:1",
            "test-yellowstone-token",
        ))
        .expect("test live tx sender");
        let prewarm_sender = sender.clone();
        let prewarm_tx = build_test_versioned_transaction();
        let hot_path_tx = build_test_versioned_transaction();
        let cache_key = test_buy_priority_fee_cache_key(Pubkey::new_unique(), false, true);
        let prewarm_key = cache_key.clone();

        let prewarm_task = tokio::spawn(async move {
            prewarm_sender
                .prewarm_buy_priority_fee_cache_with_telemetry(&prewarm_tx, &prewarm_key)
                .await
        });
        tokio::time::sleep(Duration::from_millis(20)).await;

        let joined_estimate = sender
            .estimate_buy_priority_fee_micro_lamports_with_telemetry(&hot_path_tx, Some(&cache_key))
            .await;
        let prewarm_estimate = prewarm_task.await.expect("prewarm task");

        assert_eq!(joined_estimate.micro_lamports, 42_000);
        assert_eq!(joined_estimate.telemetry.inflight_join_result, "joined");
        assert!(joined_estimate.telemetry.inflight_wait_ms > 0);
        assert_eq!(prewarm_estimate.micro_lamports, 42_000);
        assert_eq!(request_count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_hot_path_priority_fee_inflight_timeout_falls_back_without_locking_cache_key() {
        let response_body =
            "{\"jsonrpc\":\"2.0\",\"result\":{\"priorityFeeEstimate\":42000},\"id\":\"ghost-live-priority-fee\"}";
        let (priority_fee_rpc_url, request_count) = spawn_priority_fee_server_with_delay(vec![
            (
                200,
                response_body,
                PRIORITY_FEE_INFLIGHT_JOIN_MAX_TOTAL_MS + 80,
            ),
            (200, response_body, 0),
        ])
        .await;
        let sender = LiveTxSender::new(LiveTxSenderConfig::new(
            "http://127.0.0.1:1/fast",
            priority_fee_rpc_url,
            "http://127.0.0.1:1",
            "test-yellowstone-token",
        ))
        .expect("test live tx sender");
        let prewarm_sender = sender.clone();
        let prewarm_tx = build_test_versioned_transaction();
        let hot_path_tx = build_test_versioned_transaction();
        let cache_key = test_buy_priority_fee_cache_key(Pubkey::new_unique(), true, true);
        let prewarm_key = cache_key.clone();

        let prewarm_task = tokio::spawn(async move {
            prewarm_sender
                .prewarm_buy_priority_fee_cache_with_telemetry(&prewarm_tx, &prewarm_key)
                .await
        });
        tokio::time::sleep(Duration::from_millis(20)).await;

        let fallback_estimate = sender
            .estimate_buy_priority_fee_micro_lamports_with_telemetry(&hot_path_tx, Some(&cache_key))
            .await;
        let _ = prewarm_task.await.expect("prewarm task");

        assert_eq!(fallback_estimate.micro_lamports, 42_000);
        assert_eq!(
            fallback_estimate.telemetry.inflight_join_result,
            "timed_out_fallback"
        );
        assert_eq!(request_count.load(Ordering::Relaxed), 2);
        assert_eq!(
            sender
                .get_cached_buy_priority_fee(&cache_key)
                .expect("cache should remain usable after timeout fallback")
                .micro_lamports,
            42_000
        );
    }

    #[tokio::test]
    async fn test_tip_floor_cache_miss_fetches_and_stores() {
        let response_body =
            "[{\"landed_tips_75th_percentile\":0.0003,\"landed_tips_50th_percentile\":null,\"landed_tips_25th_percentile\":null}]";
        let (endpoint, request_count) = spawn_tip_floor_server(vec![(200, response_body)]).await;
        let sender = LiveTxSender::new(LiveTxSenderConfig::new(
            "http://127.0.0.1:1/fast",
            "http://127.0.0.1:1",
            "http://127.0.0.1:1",
            "test-yellowstone-token",
        ))
        .expect("test live tx sender");

        let resolution = sender
            .resolve_tip_floor_resolution_from_endpoint(&endpoint, super::RefreshIntent::Background)
            .await;

        assert_eq!(resolution.floor_lamports, Some(540_000));
        assert_eq!(resolution.telemetry.source, "jito_tip_floor");
        assert!(!resolution.telemetry.cache_hit);
        assert_eq!(resolution.telemetry.cache_age_ms, 0);
        assert_eq!(resolution.telemetry.cache_mode, "miss_refresh");
        assert_eq!(sender.cached_tip_floor_lamports(), Some(540_000));
        assert_eq!(request_count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_tip_floor_cache_hit_is_shared_across_sender_clone() {
        let response_body =
            "[{\"landed_tips_75th_percentile\":0.0003,\"landed_tips_50th_percentile\":null,\"landed_tips_25th_percentile\":null}]";
        let (endpoint, request_count) = spawn_tip_floor_server(vec![(200, response_body)]).await;
        let sender = LiveTxSender::new(LiveTxSenderConfig::new(
            "http://127.0.0.1:1/fast",
            "http://127.0.0.1:1",
            "http://127.0.0.1:1",
            "test-yellowstone-token",
        ))
        .expect("test live tx sender");
        let sender_clone = sender.clone();

        let first_resolution = sender
            .resolve_tip_floor_resolution_from_endpoint(&endpoint, super::RefreshIntent::Background)
            .await;
        let second_resolution = sender_clone
            .resolve_tip_floor_resolution_from_endpoint(&endpoint, super::RefreshIntent::Background)
            .await;

        assert_eq!(first_resolution.floor_lamports, Some(540_000));
        assert_eq!(second_resolution.floor_lamports, Some(540_000));
        assert!(second_resolution.telemetry.cache_hit);
        assert_eq!(second_resolution.telemetry.source, "tip_floor_cache");
        assert_eq!(second_resolution.telemetry.cache_mode, "fresh_cache");
        assert_eq!(request_count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_tip_floor_stale_entry_refreshes_on_success() {
        let first_response =
            "[{\"landed_tips_75th_percentile\":0.0003,\"landed_tips_50th_percentile\":null,\"landed_tips_25th_percentile\":null}]";
        let second_response =
            "[{\"landed_tips_75th_percentile\":0.0004,\"landed_tips_50th_percentile\":null,\"landed_tips_25th_percentile\":null}]";
        let (endpoint, request_count) =
            spawn_tip_floor_server(vec![(200, first_response), (200, second_response)]).await;
        let sender = LiveTxSender::new(LiveTxSenderConfig::new(
            "http://127.0.0.1:1/fast",
            "http://127.0.0.1:1",
            "http://127.0.0.1:1",
            "test-yellowstone-token",
        ))
        .expect("test live tx sender");

        let first_resolution = sender
            .resolve_tip_floor_resolution_from_endpoint(&endpoint, super::RefreshIntent::Background)
            .await;
        tokio::time::sleep(Duration::from_millis(TIP_FLOOR_CACHE_TTL_MS + 25)).await;
        let second_resolution = sender
            .resolve_tip_floor_resolution_from_endpoint(&endpoint, super::RefreshIntent::Background)
            .await;

        assert_eq!(first_resolution.floor_lamports, Some(540_000));
        assert_eq!(second_resolution.floor_lamports, Some(720_000));
        assert_eq!(second_resolution.telemetry.source, "jito_tip_floor");
        assert_eq!(second_resolution.telemetry.cache_mode, "miss_refresh");
        assert!(!second_resolution.telemetry.cache_hit);
        assert_eq!(sender.cached_tip_floor_lamports(), Some(720_000));
        assert_eq!(request_count.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn test_buy_tip_resolution_uses_fixed_sender_tip_without_tip_floor_fetch() {
        let response_body =
            "[{\"landed_tips_75th_percentile\":0.0003,\"landed_tips_50th_percentile\":null,\"landed_tips_25th_percentile\":null}]";
        let (endpoint, request_count) =
            spawn_tip_floor_server_with_delay(vec![(200, response_body, 120)]).await;
        let sender = LiveTxSender::new(
            LiveTxSenderConfig::new(
                "http://127.0.0.1:1/fast",
                "http://127.0.0.1:1",
                "http://127.0.0.1:1",
                "test-yellowstone-token",
            )
            .with_tip_floor_endpoint(endpoint),
        )
        .expect("test live tx sender");
        let prewarm_sender = sender.clone();

        let prewarm_task =
            tokio::spawn(
                async move { prewarm_sender.prewarm_buy_tip_floor_with_telemetry().await },
            );
        let resolved = sender.resolve_buy_tip_lamports_with_telemetry().await;
        let prewarm_resolution = prewarm_task.await.expect("prewarm task");

        assert_eq!(
            resolved.tip_lamports,
            HELIUS_SENDER_BUY_BASELINE_TIP_LAMPORTS
        );
        assert_eq!(resolved.telemetry.source, "sender_fixed_tip");
        assert_eq!(resolved.telemetry.cache_mode, "fixed_baseline");
        assert_eq!(resolved.telemetry.fetch_latency_ms, 0);
        assert_eq!(resolved.telemetry.inflight_join_result, "disabled");
        assert_eq!(prewarm_resolution.tip_lamports, 1_000_000);
        assert_eq!(prewarm_resolution.telemetry.source, "sender_fixed_tip");
        assert_eq!(request_count.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn test_raise_tip_to_dynamic_floor_skips_tip_floor_fetch() {
        let response_body =
            "[{\"landed_tips_75th_percentile\":0.0003,\"landed_tips_50th_percentile\":null,\"landed_tips_25th_percentile\":null}]";
        let (endpoint, request_count) = spawn_tip_floor_server(vec![(200, response_body)]).await;
        let sender = LiveTxSender::new(
            LiveTxSenderConfig::new(
                "http://127.0.0.1:1/fast",
                "http://127.0.0.1:1",
                "http://127.0.0.1:1",
                "test-yellowstone-token",
            )
            .with_tip_floor_endpoint(endpoint),
        )
        .expect("test live tx sender");

        let raised_tip = sender.raise_tip_to_dynamic_floor(1_000_000).await;

        assert_eq!(raised_tip, 1_000_000);
        assert_eq!(request_count.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn test_tip_floor_stale_last_good_used_only_on_fetch_failure() {
        let success_response =
            "[{\"landed_tips_75th_percentile\":0.0003,\"landed_tips_50th_percentile\":null,\"landed_tips_25th_percentile\":null}]";
        let failure_response = "{\"error\":\"upstream unavailable\"}";
        let (endpoint, request_count) =
            spawn_tip_floor_server(vec![(200, success_response), (500, failure_response)]).await;
        let sender = LiveTxSender::new(LiveTxSenderConfig::new(
            "http://127.0.0.1:1/fast",
            "http://127.0.0.1:1",
            "http://127.0.0.1:1",
            "test-yellowstone-token",
        ))
        .expect("test live tx sender");

        let first_resolution = sender
            .resolve_tip_floor_resolution_from_endpoint(&endpoint, super::RefreshIntent::Background)
            .await;
        tokio::time::sleep(Duration::from_millis(TIP_FLOOR_CACHE_TTL_MS + 25)).await;
        let stale_last_good_resolution = sender
            .resolve_tip_floor_resolution_from_endpoint(&endpoint, super::RefreshIntent::Background)
            .await;
        tokio::time::sleep(Duration::from_millis(
            TIP_FLOOR_STALE_LAST_GOOD_MAX_AGE_MS - TIP_FLOOR_CACHE_TTL_MS + 50,
        ))
        .await;
        let expired_resolution = sender
            .resolve_tip_floor_resolution_from_endpoint(&endpoint, super::RefreshIntent::Background)
            .await;

        assert_eq!(first_resolution.floor_lamports, Some(540_000));
        assert_eq!(stale_last_good_resolution.floor_lamports, Some(540_000));
        assert_eq!(
            stale_last_good_resolution.telemetry.source,
            "stale_last_good"
        );
        assert_eq!(
            stale_last_good_resolution.telemetry.cache_mode,
            "stale_last_good"
        );
        assert!(!stale_last_good_resolution.telemetry.cache_hit);
        assert!(stale_last_good_resolution.telemetry.cache_age_ms > TIP_FLOOR_CACHE_TTL_MS);

        assert_eq!(expired_resolution.floor_lamports, None);
        assert_eq!(expired_resolution.telemetry.source, "fallback_http_status");
        assert_eq!(
            expired_resolution.telemetry.cache_mode,
            "miss_refresh_failed"
        );
        assert_eq!(request_count.load(Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn test_estimate_priority_fee_accepts_decimal_response_value() {
        let response_body =
            "{\"jsonrpc\":\"2.0\",\"result\":{\"priorityFeeEstimate\":360000.0},\"id\":\"ghost-live-priority-fee\"}";
        let (priority_fee_rpc_url, _request_rx) =
            spawn_priority_fee_capture_server(response_body).await;
        let sender = LiveTxSender::new(LiveTxSenderConfig::new(
            "http://127.0.0.1:1/fast",
            priority_fee_rpc_url,
            "http://127.0.0.1:1",
            "test-yellowstone-token",
        ))
        .expect("test live tx sender");
        let transaction = build_test_versioned_transaction();

        let estimate = sender
            .estimate_priority_fee_micro_lamports(&transaction)
            .await;

        assert_eq!(estimate, 360_000);
    }

    #[test]
    fn test_parse_priority_fee_micro_lamports_value_rounds_up_fractional_numbers() {
        assert_eq!(
            parse_priority_fee_micro_lamports_value(&json!(42.1)),
            Some(43)
        );
        assert_eq!(
            parse_priority_fee_micro_lamports_value(&json!("360000.0")),
            Some(360_000)
        );
        assert_eq!(parse_priority_fee_micro_lamports_value(&json!(0.0)), None);
    }
}
