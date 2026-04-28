use anyhow::{anyhow, bail, Context, Result};
use futures_util::{SinkExt, StreamExt};
use ghost_core::init_pool_parser::{extract_accounts, extract_trade_accounts, AmmType};
use rand::rngs::OsRng;
use rand::RngCore;
use seer::new_async_rpc_client;
use seer::types::RawInstruction;
use serde::Serialize;
use serde_json::Value;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_sdk::transaction::{Transaction, VersionedTransaction};
use solana_transaction_status::{
    EncodedConfirmedTransactionWithStatusMeta, EncodedTransaction, UiInstruction, UiMessage,
    UiParsedInstruction, UiTransactionEncoding,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::convert::TryInto;
use std::env;
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, Mutex, Semaphore};
use tokio::time::{sleep, Interval};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const DEFAULT_WINDOW_SECS: u64 = 360;
const DEFAULT_MAX_ACTIVE_MINTS: usize = 10;
const DEFAULT_RPC_RPS_LIMIT: u64 = 9;
const DEFAULT_MAX_RPC_CONCURRENCY: usize = 4;
const DEFAULT_WSS_RECONNECT_DELAY_SECS: u64 = 10;
const DEFAULT_WSS_PREFILTER_SUMMARY_INTERVAL_SECS: u64 = 30;
const NON_ZERO_SLOT_MASK: u64 = 1;
const PUMPFUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const PUMPFUN_CREATE_DISCRIMINATOR: [u8; 8] = [0xd6, 0x90, 0x4c, 0xec, 0x5f, 0x8b, 0x31, 0xb4];
const PUMPFUN_BUY_DISCRIMINATOR: [u8; 8] = [0x66, 0x06, 0x3d, 0x12, 0x01, 0xda, 0xeb, 0xea];
const PUMPFUN_SELL_DISCRIMINATOR: [u8; 8] = [0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad];
const RPC_MAX_RETRY_ATTEMPTS: u32 = 5;
const RPC_RETRY_BASE_DELAY_MS: u64 = 1000;
const RPC_RETRY_MAX_DELAY_MS: u64 = 16_000;
const RPC_RETRY_MAX_EXPONENT: u32 = 4;
const RPC_QUEUE_CAPACITY_MULTIPLIER: usize = 4;
const FINALIZER_POLL_INTERVAL_SECS: u64 = 2;

struct LogsNotification {
    signature: String,
    logs: Vec<String>,
    slot: u64,
    slot_missing: bool,
}

#[derive(Debug, Clone)]
struct RpcWorkItem {
    signature: String,
    slot: u64,
}

#[derive(Debug, Clone)]
struct CollectorConfig {
    helius_wss_url: String,
    helius_http_url: String,
    output_path: Option<PathBuf>,
    window_secs: u64,
    commitment: CommitmentLevel,
    max_rpc_concurrency: usize,
    max_active_mints: usize,
    rpc_rps_limit: u64,
    reconnect_delay_secs: u64,
    prefilter_summary_interval_secs: u64,
    emit_sample_records: bool,
}

impl CollectorConfig {
    fn from_env() -> Result<Self> {
        let helius_wss_url = required_env("HELIUS_WSS_URL")?.trim().to_string();
        if !helius_wss_url.starts_with("wss://") {
            bail!("HELIUS_WSS_URL must start with wss://");
        }

        let helius_http_url = required_env("HELIUS_HTTP_URL")?.trim().to_string();
        if !helius_http_url.starts_with("https://") {
            bail!("HELIUS_HTTP_URL must start with https://");
        }

        let output_path = env::var("OUTPUT_PATH")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);

        let window_secs = parse_env("WINDOW_SECS")?.unwrap_or(DEFAULT_WINDOW_SECS);
        if window_secs == 0 {
            bail!("WINDOW_SECS must be greater than 0");
        }

        let commitment = match env::var("COMMITMENT") {
            Ok(value) => CommitmentLevel::parse(&value)?,
            Err(env::VarError::NotPresent) => CommitmentLevel::Confirmed,
            Err(err) => return Err(err).context("Failed to read COMMITMENT"),
        };

        let max_rpc_concurrency =
            parse_env("MAX_RPC_CONCURRENCY")?.unwrap_or(DEFAULT_MAX_RPC_CONCURRENCY);
        if max_rpc_concurrency == 0 {
            bail!("MAX_RPC_CONCURRENCY must be greater than 0");
        }
        let max_active_mints = parse_env("MAX_ACTIVE_MINTS")?.unwrap_or(DEFAULT_MAX_ACTIVE_MINTS);
        if max_active_mints == 0 {
            bail!("MAX_ACTIVE_MINTS must be greater than 0");
        }
        let rpc_rps_limit = parse_env("RPC_RPS_LIMIT")?.unwrap_or(DEFAULT_RPC_RPS_LIMIT);
        if rpc_rps_limit == 0 {
            bail!("RPC_RPS_LIMIT must be greater than 0");
        }
        let reconnect_delay_secs =
            parse_env("WSS_RECONNECT_DELAY_SECS")?.unwrap_or(DEFAULT_WSS_RECONNECT_DELAY_SECS);
        if reconnect_delay_secs == 0 {
            bail!("WSS_RECONNECT_DELAY_SECS must be greater than 0");
        }
        let prefilter_summary_interval_secs = parse_env("WSS_PREFILTER_SUMMARY_INTERVAL_SECS")?
            .unwrap_or(DEFAULT_WSS_PREFILTER_SUMMARY_INTERVAL_SECS);
        if prefilter_summary_interval_secs == 0 {
            bail!("WSS_PREFILTER_SUMMARY_INTERVAL_SECS must be greater than 0");
        }
        let emit_sample_records = parse_env_bool("EMIT_SAMPLE_RECORDS")?.unwrap_or(false);

        Ok(Self {
            helius_wss_url,
            helius_http_url,
            output_path,
            window_secs,
            commitment,
            max_rpc_concurrency,
            max_active_mints,
            rpc_rps_limit,
            reconnect_delay_secs,
            prefilter_summary_interval_secs,
            emit_sample_records,
        })
    }
}

#[derive(Debug, Clone, Copy)]
enum CommitmentLevel {
    Processed,
    Confirmed,
    Finalized,
}

impl CommitmentLevel {
    fn parse(value: &str) -> Result<Self> {
        match value.trim().to_lowercase().as_str() {
            "processed" | "mempool" => Ok(Self::Processed),
            "confirmed" => Ok(Self::Confirmed),
            "finalized" => Ok(Self::Finalized),
            other => Err(anyhow!("Unsupported COMMITMENT value: {}", other)),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Processed => "processed",
            Self::Confirmed => "confirmed",
            Self::Finalized => "finalized",
        }
    }

    fn as_commitment_config(&self) -> CommitmentConfig {
        match self {
            Self::Processed => CommitmentConfig::processed(),
            Self::Confirmed => CommitmentConfig::confirmed(),
            Self::Finalized => CommitmentConfig::finalized(),
        }
    }
}

#[derive(Default)]
struct RpcFetchMetrics {
    attempted: AtomicU64,
    success: AtomicU64,
    failed: AtomicU64,
}

struct RpcFetchSnapshot {
    attempted: u64,
    success: u64,
    failed: u64,
}

struct PatientRpcFetcher {
    rpc: RpcClient,
    semaphore: Arc<Semaphore>,
    commitment: CommitmentConfig,
    metrics: Arc<RpcFetchMetrics>,
    rate_limiter: Arc<Mutex<Interval>>,
}

impl PatientRpcFetcher {
    fn new(config: &CollectorConfig) -> Self {
        let rate_interval = Duration::from_secs_f64(1.0 / config.rpc_rps_limit as f64);
        let mut limiter = tokio::time::interval(rate_interval);
        limiter.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        Self {
            rpc: new_async_rpc_client(config.helius_http_url.clone()),
            semaphore: Arc::new(Semaphore::new(config.max_rpc_concurrency)),
            commitment: config.commitment.as_commitment_config(),
            metrics: Arc::new(RpcFetchMetrics::default()),
            rate_limiter: Arc::new(Mutex::new(limiter)),
        }
    }

    async fn fetch_transaction(
        &self,
        signature: &Signature,
    ) -> Option<EncodedConfirmedTransactionWithStatusMeta> {
        let _permit = match self.semaphore.acquire().await {
            Ok(permit) => permit,
            Err(err) => {
                warn!("RPC getTransaction skipped; failed to acquire permit: {err}");
                return None;
            }
        };

        let attempted_total = self.metrics.attempted.fetch_add(1, Ordering::Relaxed) + 1;
        let tx_config = RpcTransactionConfig {
            encoding: Some(UiTransactionEncoding::JsonParsed),
            commitment: Some(self.commitment.clone()),
            max_supported_transaction_version: Some(0),
        };

        for attempt in 0..RPC_MAX_RETRY_ATTEMPTS {
            {
                let mut limiter = self.rate_limiter.lock().await;
                limiter.tick().await;
            }
            match self
                .rpc
                .get_transaction_with_config(signature, tx_config.clone())
                .await
            {
                Ok(tx) => {
                    let success_total = self.metrics.success.fetch_add(1, Ordering::Relaxed) + 1;
                    let failed_total = self.metrics.failed.load(Ordering::Relaxed);
                    debug!(
                        "RPC getTransaction success signature={} attempt={} attempted={} success={} failed={}",
                        signature,
                        attempt + 1,
                        attempted_total,
                        success_total,
                        failed_total
                    );
                    return Some(tx);
                }
                Err(err) => {
                    if attempt + 1 < RPC_MAX_RETRY_ATTEMPTS {
                        let delay_ms = rpc_retry_delay_ms(attempt);
                        warn!(
                            "RPC getTransaction retry {}/{} signature={} delay={}ms error={}",
                            attempt + 1,
                            RPC_MAX_RETRY_ATTEMPTS,
                            signature,
                            delay_ms,
                            err
                        );
                        sleep(Duration::from_millis(delay_ms)).await;
                    } else {
                        warn!(
                            "RPC getTransaction failed after retries signature={} error={}",
                            signature, err
                        );
                    }
                }
            }
        }

        let failed_total = self.metrics.failed.fetch_add(1, Ordering::Relaxed) + 1;
        let success_total = self.metrics.success.load(Ordering::Relaxed);
        warn!(
            "RPC getTransaction exhausted retries signature={} attempted={} success={} failed={}",
            signature, attempted_total, success_total, failed_total
        );
        None
    }

    fn snapshot_metrics(&self) -> RpcFetchSnapshot {
        RpcFetchSnapshot {
            attempted: self.metrics.attempted.load(Ordering::Relaxed),
            success: self.metrics.success.load(Ordering::Relaxed),
            failed: self.metrics.failed.load(Ordering::Relaxed),
        }
    }
}

struct NdjsonSink {
    file: Option<BufWriter<File>>,
}

impl NdjsonSink {
    fn new(output_path: Option<&Path>) -> Result<Self> {
        let file = match output_path {
            Some(path) => {
                let file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .with_context(|| format!("Failed to open output file {}", path.display()))?;
                Some(BufWriter::new(file))
            }
            None => None,
        };

        Ok(Self { file })
    }

    fn emit<T: Serialize>(&mut self, record: &T) -> Result<()> {
        let line = serde_json::to_string(record)?;
        {
            let mut stdout = io::stdout().lock();
            writeln!(stdout, "{line}")?;
            stdout.flush()?;
        }
        if let Some(file) = self.file.as_mut() {
            writeln!(file, "{line}")?;
            file.flush()?;
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct MintDetectedRecord {
    #[serde(rename = "type")]
    record_type: &'static str,
    mint: String,
    bonding_curve: String,
    signature: String,
    slot: u64,
    t0_unix_ms: i64,
    source: &'static str,
}

#[derive(Debug, Serialize)]
struct MintOutcome15mRecord {
    #[serde(rename = "type")]
    record_type: &'static str,
    mint: String,
    bonding_curve: String,
    t0_unix_ms: i64,
    t1_unix_ms: i64,
    tx_count_15m: u64,
    unique_traders_15m: u64,
    buy_volume_lamports_15m: u64,
    sell_volume_lamports_15m: u64,
    total_volume_lamports_15m: u64,
    first_trade_delay_ms: Option<u64>,
    completed_at_unix_ms: i64,
}

#[derive(Debug, Clone)]
struct PumpfunCreate {
    mint: Pubkey,
    bonding_curve: Pubkey,
    timestamp_ms: i64,
}

#[derive(Debug, Clone)]
struct PumpfunTrade {
    mint: Pubkey,
    bonding_curve: Pubkey,
    signer: Pubkey,
    amount: u64,
    max_sol_cost: u64,
    min_sol_output: u64,
    is_buy: bool,
    timestamp_ms: i64,
}

#[derive(Debug, Clone)]
enum PumpfunEvent {
    Create(PumpfunCreate),
    Trade(PumpfunTrade),
}

#[derive(Debug)]
struct MintState {
    mint: Pubkey,
    bonding_curve: Pubkey,
    t0_unix_ms: i64,
    t1_unix_ms: i64,
    tx_count: u64,
    buy_volume_lamports: u64,
    sell_volume_lamports: u64,
    unique_traders: HashSet<Pubkey>,
    first_trade_time: Option<i64>,
    seen_signatures: HashSet<String>,
}

impl MintState {
    fn new(create: &PumpfunCreate, window_ms: i64) -> Self {
        let t1_unix_ms = create
            .timestamp_ms
            .checked_add(window_ms)
            .unwrap_or_else(|| {
                warn!(
                    "Mint window overflow for mint={} bonding_curve={}",
                    create.mint, create.bonding_curve
                );
                create.timestamp_ms
            });
        Self {
            mint: create.mint,
            bonding_curve: create.bonding_curve,
            t0_unix_ms: create.timestamp_ms,
            t1_unix_ms,
            tx_count: 0,
            buy_volume_lamports: 0,
            sell_volume_lamports: 0,
            unique_traders: HashSet::new(),
            first_trade_time: None,
            seen_signatures: HashSet::new(),
        }
    }

    fn apply_trade(&mut self, trade: &PumpfunTrade, should_increment_tx_count: bool) {
        if should_increment_tx_count {
            self.tx_count = self.tx_count.saturating_add(1);
        }
        if trade.is_buy {
            self.buy_volume_lamports = self.buy_volume_lamports.saturating_add(trade.max_sol_cost);
        } else {
            self.sell_volume_lamports = self
                .sell_volume_lamports
                .saturating_add(trade.min_sol_output);
        }
        self.unique_traders.insert(trade.signer);
        if self.first_trade_time.is_none() {
            self.first_trade_time = Some(trade.timestamp_ms);
        }
    }

    fn total_volume_lamports(&self) -> u64 {
        self.buy_volume_lamports
            .saturating_add(self.sell_volume_lamports)
    }

    fn first_trade_delay_ms(&self) -> Option<u64> {
        self.first_trade_time
            .and_then(|first| first.checked_sub(self.t0_unix_ms))
            .and_then(|delay| u64::try_from(delay).ok())
    }

    fn outcome_record(&self, completed_at_unix_ms: i64) -> MintOutcome15mRecord {
        MintOutcome15mRecord {
            record_type: "mint_outcome_15m",
            mint: self.mint.to_string(),
            bonding_curve: self.bonding_curve.to_string(),
            t0_unix_ms: self.t0_unix_ms,
            t1_unix_ms: self.t1_unix_ms,
            tx_count_15m: self.tx_count,
            unique_traders_15m: self.unique_traders.len() as u64,
            buy_volume_lamports_15m: self.buy_volume_lamports,
            sell_volume_lamports_15m: self.sell_volume_lamports,
            total_volume_lamports_15m: self.total_volume_lamports(),
            first_trade_delay_ms: self.first_trade_delay_ms(),
            completed_at_unix_ms,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pumpfun_collector=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = CollectorConfig::from_env()?;
    info!(
        "Pump.fun collector configured: window_secs={}, commitment={}, max_rpc_concurrency={}, max_active_mints={}, rpc_rps_limit={}, reconnect_delay_secs={}, prefilter_summary_interval_secs={}",
        config.window_secs,
        config.commitment.as_str(),
        config.max_rpc_concurrency,
        config.max_active_mints,
        config.rpc_rps_limit,
        config.reconnect_delay_secs,
        config.prefilter_summary_interval_secs
    );
    info!("Helius WSS: {}", config.helius_wss_url);
    info!("Helius HTTP: {}", config.helius_http_url);
    if let Some(path) = &config.output_path {
        info!("NDJSON output file: {}", path.display());
    }

    let sink = Arc::new(Mutex::new(NdjsonSink::new(config.output_path.as_deref())?));
    if config.emit_sample_records {
        info!("EMIT_SAMPLE_RECORDS enabled; emitting sample records");
        let now_ms = unix_ms()?;
        let window_ms = window_ms(config.window_secs)?;
        let t1_unix_ms = now_ms
            .checked_add(window_ms)
            .ok_or_else(|| anyhow!("WINDOW_SECS causes t1_unix_ms overflow"))?;
        let signature = Signature::new_unique();
        let mut rng = OsRng;
        let slot = rng.next_u64() | NON_ZERO_SLOT_MASK;
        let tx_count_15m = rng.next_u64();
        let unique_traders_15m = rng.next_u64();
        let buy_volume_lamports_15m = rng.next_u64() >> 1;
        let sell_volume_lamports_15m = rng.next_u64() >> 1;
        let total_volume_lamports_15m = buy_volume_lamports_15m + sell_volume_lamports_15m;
        let window_ms_u64 = u64::try_from(window_ms)
            .map_err(|_| anyhow!("WINDOW_SECS too large to convert to milliseconds"))?;
        let first_trade_delay_ms = rng.next_u64() % window_ms_u64;
        let example_record = MintDetectedRecord {
            record_type: "mint_detected",
            mint: Pubkey::new_unique().to_string(),
            bonding_curve: Pubkey::new_unique().to_string(),
            signature: signature.to_string(),
            slot,
            t0_unix_ms: now_ms,
            source: "helius",
        };
        let outcome_record = MintOutcome15mRecord {
            record_type: "mint_outcome_15m",
            mint: example_record.mint.clone(),
            bonding_curve: example_record.bonding_curve.clone(),
            t0_unix_ms: now_ms,
            t1_unix_ms,
            tx_count_15m,
            unique_traders_15m,
            buy_volume_lamports_15m,
            sell_volume_lamports_15m,
            total_volume_lamports_15m,
            first_trade_delay_ms: Some(first_trade_delay_ms),
            completed_at_unix_ms: t1_unix_ms,
        };
        let mut sink_guard = sink.lock().await;
        sink_guard.emit(&example_record)?;
        sink_guard.emit(&outcome_record)?;
    }

    info!(
        "Starting Helius WSS log stream (commitment={})",
        config.commitment.as_str()
    );
    run_wss_loop(&config, Arc::clone(&sink)).await
}

async fn run_wss_loop(config: &CollectorConfig, sink: Arc<Mutex<NdjsonSink>>) -> Result<()> {
    let mut reconnect_count = 0u64;
    let mut dropped_count = 0u64;
    let mut passed_count = 0u64;
    let mut last_summary_at = Instant::now();
    let mut create_enqueued = 0u64;
    let mut trade_enqueued = 0u64;
    let mut create_dropped_full = 0u64;
    let mut create_dropped_closed = 0u64;
    let mut trade_dropped_full = 0u64;
    let mut trade_dropped_closed = 0u64;
    let mut last_rpc_summary_at = Instant::now();
    let window_ms = window_ms(config.window_secs)?;
    let mint_states = Arc::new(Mutex::new(HashMap::<Pubkey, MintState>::new()));
    let mint_queue = Arc::new(Mutex::new(VecDeque::new()));
    let mints_skipped_capacity = Arc::new(AtomicU64::new(0));
    let _finalizer_handle = spawn_finalizer_loop(
        Arc::clone(&mint_states),
        Arc::clone(&mint_queue),
        Arc::clone(&sink),
    );
    let rpc_fetcher = Arc::new(PatientRpcFetcher::new(config));
    let rpc_queue_capacity = config
        .max_rpc_concurrency
        .saturating_mul(RPC_QUEUE_CAPACITY_MULTIPLIER)
        .max(1);
    let (rpc_create_sender, rpc_create_receiver) = mpsc::channel::<RpcWorkItem>(rpc_queue_capacity);
    let (rpc_trade_sender, rpc_trade_receiver) = mpsc::channel::<RpcWorkItem>(rpc_queue_capacity);
    let rpc_create_receiver = Arc::new(Mutex::new(rpc_create_receiver));
    let rpc_trade_receiver = Arc::new(Mutex::new(rpc_trade_receiver));

    for _ in 0..config.max_rpc_concurrency {
        let rpc_fetcher = Arc::clone(&rpc_fetcher);
        let rpc_create_receiver = Arc::clone(&rpc_create_receiver);
        let rpc_trade_receiver = Arc::clone(&rpc_trade_receiver);
        let mint_states = Arc::clone(&mint_states);
        let mint_queue = Arc::clone(&mint_queue);
        let mints_skipped_capacity = Arc::clone(&mints_skipped_capacity);
        let sink = Arc::clone(&sink);
        let max_active_mints = config.max_active_mints;
        tokio::spawn(async move {
            loop {
                let work_item = tokio::select! {
                    biased;
                    item = async {
                        let mut receiver = rpc_create_receiver.lock().await;
                        receiver.recv().await
                    } => item,
                    item = async {
                        let mut receiver = rpc_trade_receiver.lock().await;
                        receiver.recv().await
                    } => item,
                };
                let Some(work_item) = work_item else {
                    break;
                };
                let signature = work_item.signature;
                let slot = work_item.slot;
                match Signature::from_str(&signature) {
                    Ok(sig) => match rpc_fetcher.fetch_transaction(&sig).await {
                        Some(tx) => {
                            let events = parse_pumpfun_transaction(&tx);
                            if !events.is_empty() {
                                if let Err(err) = handle_pumpfun_events(
                                    &signature,
                                    slot,
                                    &events,
                                    &mint_states,
                                    &mint_queue,
                                    &mints_skipped_capacity,
                                    max_active_mints,
                                    &sink,
                                    window_ms,
                                )
                                .await
                                {
                                    warn!(
                                        "Failed to process Pump.fun events signature={} error={}",
                                        signature, err
                                    );
                                }
                            }
                        }
                        None => {
                            warn!(
                                "RPC getTransaction returned no data for signature={}",
                                signature
                            );
                        }
                    },
                    Err(err) => warn!(
                        "Invalid signature from logsNotification: {} ({})",
                        signature, err
                    ),
                }
            }
        });
    }

    loop {
        if reconnect_count > 0 {
            info!("Reconnecting to Helius WSS (attempt={reconnect_count})");
        }

        let (ws_stream, _) = match connect_async(&config.helius_wss_url).await {
            Ok(stream) => stream,
            Err(err) => {
                reconnect_count = reconnect_count.saturating_add(1);
                warn!("WSS connection failed: {err}. reconnects={reconnect_count}");
                sleep(Duration::from_secs(config.reconnect_delay_secs)).await;
                continue;
            }
        };

        info!("Connected to Helius WSS");
        let (mut write, mut read) = ws_stream.split();
        let mut rng = OsRng;
        let subscribe_id = rng.next_u64();
        let subscribe_msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": subscribe_id,
            "method": "logsSubscribe",
            "params": [
                { "mentions": [PUMPFUN_PROGRAM_ID] },
                { "commitment": config.commitment.as_str() }
            ]
        });

        write
            .send(Message::Text(subscribe_msg.to_string()))
            .await
            .context("Failed to send logsSubscribe")?;

        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    let notification = match parse_logs_notification(&text) {
                        Ok(Some(notification)) => notification,
                        Ok(None) => continue,
                        Err(err) => {
                            warn!("Failed to parse logsNotification: {err}");
                            continue;
                        }
                    };
                    if notification.slot_missing {
                        warn!(
                            "logsNotification missing slot; using slot=0 (signature={})",
                            notification.signature
                        );
                    }

                    let (maybe_create, maybe_trade) = prefilter_log_flags(&notification.logs);
                    if !maybe_create && !maybe_trade {
                        dropped_count = dropped_count.saturating_add(1);
                        debug!(
                            "WSS prefilter dropped signature={} slot={} passed={} dropped={} reconnects={}",
                            notification.signature,
                            notification.slot,
                            passed_count,
                            dropped_count,
                            reconnect_count
                        );
                        log_prefilter_summary(
                            config,
                            passed_count,
                            dropped_count,
                            reconnect_count,
                            &mut last_summary_at,
                        );
                        if last_rpc_summary_at.elapsed()
                            >= Duration::from_secs(config.prefilter_summary_interval_secs)
                        {
                            log_rpc_summary(
                                config,
                                rpc_fetcher.as_ref(),
                                create_enqueued,
                                trade_enqueued,
                                create_dropped_full,
                                create_dropped_closed,
                                trade_dropped_full,
                                trade_dropped_closed,
                                mints_skipped_capacity.load(Ordering::Relaxed),
                                &mut last_rpc_summary_at,
                            );
                        }
                        continue;
                    }

                    passed_count = passed_count.saturating_add(1);
                    debug!(
                        "WSS prefilter passed signature={} slot={} maybe_create={} maybe_trade={} passed={} dropped={} reconnects={}",
                        notification.signature,
                        notification.slot,
                        maybe_create,
                        maybe_trade,
                        passed_count,
                        dropped_count,
                        reconnect_count
                    );
                    log_prefilter_summary(
                        config,
                        passed_count,
                        dropped_count,
                        reconnect_count,
                        &mut last_summary_at,
                    );
                    if maybe_create {
                        let work_item = RpcWorkItem {
                            signature: notification.signature.clone(),
                            slot: notification.slot,
                        };
                        match rpc_create_sender.try_send(work_item) {
                            Ok(()) => {
                                create_enqueued = create_enqueued.saturating_add(1);
                            }
                            Err(mpsc::error::TrySendError::Full(work_item)) => {
                                handle_rpc_queue_drop(
                                    work_item.signature,
                                    &mut create_dropped_full,
                                    "create",
                                    "full",
                                );
                            }
                            Err(mpsc::error::TrySendError::Closed(work_item)) => {
                                handle_rpc_queue_drop(
                                    work_item.signature,
                                    &mut create_dropped_closed,
                                    "create",
                                    "closed",
                                );
                            }
                        }
                    } else if maybe_trade {
                        let work_item = RpcWorkItem {
                            signature: notification.signature.clone(),
                            slot: notification.slot,
                        };
                        match rpc_trade_sender.try_send(work_item) {
                            Ok(()) => {
                                trade_enqueued = trade_enqueued.saturating_add(1);
                            }
                            Err(mpsc::error::TrySendError::Full(work_item)) => {
                                handle_rpc_queue_drop(
                                    work_item.signature,
                                    &mut trade_dropped_full,
                                    "trade",
                                    "full",
                                );
                            }
                            Err(mpsc::error::TrySendError::Closed(work_item)) => {
                                handle_rpc_queue_drop(
                                    work_item.signature,
                                    &mut trade_dropped_closed,
                                    "trade",
                                    "closed",
                                );
                            }
                        }
                    }
                    if last_rpc_summary_at.elapsed()
                        >= Duration::from_secs(config.prefilter_summary_interval_secs)
                    {
                        log_rpc_summary(
                            config,
                            rpc_fetcher.as_ref(),
                            create_enqueued,
                            trade_enqueued,
                            create_dropped_full,
                            create_dropped_closed,
                            trade_dropped_full,
                            trade_dropped_closed,
                            mints_skipped_capacity.load(Ordering::Relaxed),
                            &mut last_rpc_summary_at,
                        );
                    }
                }
                Ok(Message::Close(frame)) => {
                    warn!("WSS stream closed: {:?}", frame);
                    break;
                }
                Ok(Message::Ping(payload)) => {
                    write
                        .send(Message::Pong(payload))
                        .await
                        .context("Failed to respond to ping")?;
                }
                Ok(_) => {}
                Err(err) => {
                    warn!("WSS message error: {err}");
                    break;
                }
            }
        }

        reconnect_count = reconnect_count.saturating_add(1);
        warn!("WSS disconnected; reconnects={reconnect_count}");
        sleep(Duration::from_secs(config.reconnect_delay_secs)).await;
    }
}

fn window_ms(window_secs: u64) -> Result<i64> {
    i64::try_from(Duration::from_secs(window_secs).as_millis())
        .map_err(|_| anyhow!("WINDOW_SECS too large to convert to milliseconds"))
}

fn spawn_finalizer_loop(
    mint_states: Arc<Mutex<HashMap<Pubkey, MintState>>>,
    mint_queue: Arc<Mutex<VecDeque<Pubkey>>>,
    sink: Arc<Mutex<NdjsonSink>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(FINALIZER_POLL_INTERVAL_SECS)).await;
            let now_ms = match unix_ms() {
                Ok(ms) => ms,
                Err(err) => {
                    warn!("Finalizer skipped; failed to read system time: {err}");
                    continue;
                }
            };
            let (expired_states, expired_keys) = {
                let mut states = mint_states.lock().await;
                let expired_keys: Vec<Pubkey> = states
                    .iter()
                    .filter(|(_, state)| now_ms >= state.t1_unix_ms)
                    .map(|(key, _)| *key)
                    .collect();
                let mut expired_states = Vec::new();
                for key in &expired_keys {
                    if let Some(state) = states.remove(key) {
                        expired_states.push(state);
                    }
                }
                (expired_states, expired_keys)
            };
            if expired_states.is_empty() {
                continue;
            }
            if !expired_keys.is_empty() {
                let expired_set: HashSet<Pubkey> = expired_keys.into_iter().collect();
                let mut queue = mint_queue.lock().await;
                queue.retain(|key| !expired_set.contains(key));
            }
            let mut sink_guard = sink.lock().await;
            for state in expired_states {
                let record = state.outcome_record(now_ms);
                if let Err(err) = sink_guard.emit(&record) {
                    warn!(
                        "Failed to emit mint_outcome_15m mint={} bonding_curve={} error={}",
                        state.mint, state.bonding_curve, err
                    );
                }
            }
        }
    })
}

async fn handle_pumpfun_events(
    signature: &str,
    slot: u64,
    events: &[PumpfunEvent],
    mint_states: &Arc<Mutex<HashMap<Pubkey, MintState>>>,
    mint_queue: &Arc<Mutex<VecDeque<Pubkey>>>,
    mints_skipped_capacity: &Arc<AtomicU64>,
    max_active_mints: usize,
    sink: &Arc<Mutex<NdjsonSink>>,
    window_ms: i64,
) -> Result<()> {
    let signature = signature.to_string();
    let mut detected_records = Vec::new();
    let mut admitted_curves = Vec::new();
    {
        let mut states = mint_states.lock().await;
        for event in events {
            match event {
                PumpfunEvent::Create(create) => {
                    if states.contains_key(&create.bonding_curve) {
                        continue;
                    }
                    if states.len() >= max_active_mints {
                        mints_skipped_capacity.fetch_add(1, Ordering::Relaxed);
                        warn!(
                            "Skipping mint_detected due to MAX_ACTIVE_MINTS capacity mint={} bonding_curve={}",
                            create.mint, create.bonding_curve
                        );
                        continue;
                    }
                    let state = MintState::new(create, window_ms);
                    states.insert(create.bonding_curve, state);
                    admitted_curves.push(create.bonding_curve);
                    detected_records.push(MintDetectedRecord {
                        record_type: "mint_detected",
                        mint: create.mint.to_string(),
                        bonding_curve: create.bonding_curve.to_string(),
                        signature: signature.clone(),
                        slot,
                        t0_unix_ms: create.timestamp_ms,
                        source: "helius",
                    });
                }
                PumpfunEvent::Trade(trade) => {
                    let Some(state) = states.get_mut(&trade.bonding_curve) else {
                        warn!(
                            "Skipping trade for untracked mint signature={} mint={} bonding_curve={}",
                            signature,
                            trade.mint,
                            trade.bonding_curve
                        );
                        continue;
                    };
                    // Increment tx_count once per signature, but capture all trade volumes.
                    let should_increment_tx_count = state.seen_signatures.insert(signature.clone());
                    debug!(
                        "Processing Pump.fun trade signature={} mint={} bonding_curve={} signer={} is_buy={} amount={} max_sol_cost={} min_sol_output={} timestamp_ms={}",
                        signature,
                        trade.mint,
                        trade.bonding_curve,
                        trade.signer,
                        trade.is_buy,
                        trade.amount,
                        trade.max_sol_cost,
                        trade.min_sol_output,
                        trade.timestamp_ms
                    );
                    state.apply_trade(trade, should_increment_tx_count);
                }
            }
        }
    }
    if !admitted_curves.is_empty() {
        let mut queue = mint_queue.lock().await;
        for bonding_curve in admitted_curves {
            queue.push_back(bonding_curve);
        }
    }
    if !detected_records.is_empty() {
        let mut sink_guard = sink.lock().await;
        for record in detected_records {
            sink_guard.emit(&record)?;
        }
    }
    Ok(())
}

/// Parse Pump.fun Create/Buy/Sell instructions from a confirmed transaction.
/// Returns a list of parsed PumpfunEvent entries for the transaction.
fn parse_pumpfun_transaction(tx: &EncodedConfirmedTransactionWithStatusMeta) -> Vec<PumpfunEvent> {
    let Some((accounts, instructions)) = parse_ui_transaction(&tx.transaction.transaction) else {
        return Vec::new();
    };
    let timestamp_ms = transaction_unix_ms(tx.block_time);
    let program_id = match Pubkey::from_str(PUMPFUN_PROGRAM_ID) {
        Ok(program_id) => program_id,
        Err(err) => {
            warn!("Invalid Pump.fun program id: {err}");
            return Vec::new();
        }
    };
    let mut events = Vec::new();

    for instruction in instructions {
        if instruction.program_id != program_id {
            continue;
        }
        if instruction.data.len() < 8 {
            warn!(
                "Pump.fun instruction data too short: {} bytes",
                instruction.data.len()
            );
            continue;
        }
        let discriminator = &instruction.data[0..8];
        if discriminator == PUMPFUN_CREATE_DISCRIMINATOR {
            if let Some(create) =
                parse_pumpfun_create(&accounts, &instruction.account_indices, timestamp_ms)
            {
                events.push(PumpfunEvent::Create(create));
            }
        } else if discriminator == PUMPFUN_BUY_DISCRIMINATOR
            || discriminator == PUMPFUN_SELL_DISCRIMINATOR
        {
            let is_buy = discriminator == PUMPFUN_BUY_DISCRIMINATOR;
            if let Some(trade) = parse_pumpfun_trade(
                &accounts,
                &instruction.account_indices,
                &instruction.data,
                is_buy,
                timestamp_ms,
            ) {
                events.push(PumpfunEvent::Trade(trade));
            }
        }
    }

    events
}

fn parse_pumpfun_create(
    accounts: &[Pubkey],
    account_indices: &[u8],
    timestamp_ms: i64,
) -> Option<PumpfunCreate> {
    let extracted = match extract_accounts(
        accounts,
        account_indices,
        AmmType::PumpFun,
        false, // verbose logging
    ) {
        Ok(extracted) => extracted,
        Err(err) => {
            warn!("Pump.fun create instruction account extraction failed: {err}");
            return None;
        }
    };
    let mint = match extracted.mint {
        Some(mint) => mint,
        None => {
            warn!("Pump.fun create missing mint after extraction");
            return None;
        }
    };
    let bonding_curve = match extracted.bonding_curve {
        Some(bonding_curve) => bonding_curve,
        None => {
            warn!("Pump.fun create missing bonding curve after extraction");
            return None;
        }
    };

    Some(PumpfunCreate {
        mint,
        bonding_curve,
        timestamp_ms,
    })
}

fn parse_pumpfun_trade(
    accounts: &[Pubkey],
    account_indices: &[u8],
    data: &[u8],
    is_buy: bool,
    timestamp_ms: i64,
) -> Option<PumpfunTrade> {
    let (amount, max_sol_cost, min_sol_output) = parse_trade_params(data, is_buy)?;
    let extracted = match extract_trade_accounts(
        accounts,
        account_indices,
        AmmType::PumpFun,
        false, // no verbose logging
    ) {
        Ok(extracted) => extracted,
        Err(err) => {
            warn!("Pump.fun trade account extraction failed: {err}");
            return None;
        }
    };
    let mint = match extracted.mint {
        Some(mint) => mint,
        None => {
            warn!("Pump.fun trade missing mint after extraction");
            return None;
        }
    };
    let bonding_curve = match extracted.bonding_curve {
        Some(bonding_curve) => bonding_curve,
        None => {
            warn!("Pump.fun trade missing bonding curve after extraction");
            return None;
        }
    };
    let signer = match extracted.signer {
        Some(signer) => signer,
        None => {
            warn!("Pump.fun trade missing signer after extraction");
            return None;
        }
    };

    Some(PumpfunTrade {
        mint,
        bonding_curve,
        signer,
        amount,
        max_sol_cost,
        min_sol_output,
        is_buy,
        timestamp_ms,
    })
}

fn parse_trade_params(data: &[u8], is_buy: bool) -> Option<(u64, u64, u64)> {
    let amount = match read_u64_le(data, 8) {
        Some(amount) => amount,
        None => {
            warn!("Pump.fun trade missing amount at offset 8");
            return None;
        }
    };
    let price = match read_u64_le(data, 16) {
        Some(price) => price,
        None => {
            warn!("Pump.fun trade missing price at offset 16");
            return None;
        }
    };
    let (max_sol_cost, min_sol_output) = if is_buy { (price, 0) } else { (0, price) };

    Some((amount, max_sol_cost, min_sol_output))
}

/// Read a little-endian u64 from a byte slice at the given offset.
fn read_u64_le(data: &[u8], offset: usize) -> Option<u64> {
    let end = offset.checked_add(8)?;
    let bytes: [u8; 8] = data.get(offset..end)?.try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

fn transaction_unix_ms(block_time: Option<i64>) -> i64 {
    if let Some(seconds) = block_time {
        if let Some(ms) = seconds.checked_mul(1000) {
            return ms;
        }
        warn!("block_time overflow; falling back to SystemTime");
    }
    match unix_ms() {
        Ok(ms) => ms,
        Err(err) => {
            warn!("Failed to read system time for unix_ms: {err}");
            0
        }
    }
}

/// Parse a UI transaction into account keys and raw instructions.
/// Supports Json, JsonParsed, and Base64 encodings.
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
            warn!("Base64 transaction encoding may omit lookup table keys");
            use base64::{engine::general_purpose, Engine as _};
            let tx_data = match general_purpose::STANDARD.decode(data_str) {
                Ok(data) => data,
                Err(err) => {
                    warn!("Failed to decode Base64 transaction data: {}", err);
                    return None;
                }
            };

            match bincode::deserialize::<VersionedTransaction>(&tx_data) {
                Ok(versioned_tx) => {
                    let message = versioned_tx.message;
                    let static_keys = message.static_account_keys();
                    let accounts: Vec<Pubkey> = static_keys.iter().copied().collect();

                    let instructions: Vec<RawInstruction> = message
                        .instructions()
                        .iter()
                        .filter_map(|ix| {
                            let program_id = accounts.get(ix.program_id_index as usize)?.to_owned();
                            Some(RawInstruction {
                                program_id,
                                account_indices: ix.accounts.clone(),
                                data: ix.data.clone(),
                            })
                        })
                        .collect();

                    Some((accounts, instructions))
                }
                Err(_) => match bincode::deserialize::<Transaction>(&tx_data) {
                    Ok(tx) => {
                        let accounts = tx.message.account_keys.clone();

                        let instructions: Vec<RawInstruction> = tx
                            .message
                            .instructions
                            .iter()
                            .filter_map(|ix| {
                                let program_id =
                                    accounts.get(ix.program_id_index as usize)?.to_owned();
                                Some(RawInstruction {
                                    program_id,
                                    account_indices: ix.accounts.clone(),
                                    data: ix.data.clone(),
                                })
                            })
                            .collect();

                        Some((accounts, instructions))
                    }
                    Err(err) => {
                        warn!("Failed to deserialize transaction: {}", err);
                        None
                    }
                },
            }
        }
        other => {
            warn!("Unsupported transaction encoding format: {:?}", other);
            None
        }
    }
}

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
                            .and_then(|i| u8::try_from(i).ok())
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

fn parse_logs_notification(payload: &str) -> Result<Option<LogsNotification>> {
    let value: Value = match serde_json::from_str(payload) {
        Ok(value) => value,
        Err(err) => {
            debug!("Failed to parse WSS payload: {err}");
            return Ok(None);
        }
    };

    if value.get("method").and_then(|method| method.as_str()) != Some("logsNotification") {
        return Ok(None);
    }

    let params = match value.get("params").and_then(|p| p.get("result")) {
        Some(params) => params,
        None => {
            warn!("logsNotification missing params.result");
            return Ok(None);
        }
    };
    let value = match params.get("value") {
        Some(value) => value,
        None => {
            warn!("logsNotification missing params.result.value");
            return Ok(None);
        }
    };
    let signature = match value.get("signature").and_then(|s| s.as_str()) {
        Some(signature) => signature.to_string(),
        None => {
            warn!("logsNotification missing signature");
            return Ok(None);
        }
    };
    let logs_array = match value.get("logs").and_then(|l| l.as_array()) {
        Some(logs) => logs,
        None => {
            warn!(
                "logsNotification missing logs array (signature={})",
                signature
            );
            return Ok(None);
        }
    };
    let logs = logs_array
        .iter()
        .filter_map(|log| log.as_str().map(|log| log.to_string()))
        .collect::<Vec<_>>();
    let slot = params
        .get("context")
        .and_then(|context| context.get("slot"))
        .and_then(|slot| slot.as_u64())
        .or_else(|| value.get("slot").and_then(|slot| slot.as_u64()));
    let (slot, slot_missing) = match slot {
        Some(slot) => (slot, false),
        None => (0, true),
    };

    Ok(Some(LogsNotification {
        signature,
        logs,
        slot,
        slot_missing,
    }))
}

fn prefilter_log_flags(logs: &[String]) -> (bool, bool) {
    let mut maybe_create = false;
    let mut maybe_trade = false;

    for log in logs {
        if !maybe_create
            && (log.contains("Instruction: Create")
                || log.contains("InitializeMint")
                || log.contains("InitializeMint2"))
        {
            maybe_create = true;
        }
        if !maybe_trade
            && (log.contains("Instruction: Buy")
                || log.contains("Instruction: Sell")
                || log.contains("Instruction: Swap"))
        {
            maybe_trade = true;
        }
        if maybe_create && maybe_trade {
            break;
        }
    }

    (maybe_create, maybe_trade)
}

fn rpc_retry_delay_ms(attempt: u32) -> u64 {
    let capped_attempt = attempt.min(RPC_RETRY_MAX_EXPONENT);
    let delay = RPC_RETRY_BASE_DELAY_MS.saturating_mul(1_u64 << capped_attempt);
    delay.min(RPC_RETRY_MAX_DELAY_MS)
}

fn log_prefilter_summary(
    config: &CollectorConfig,
    passed_count: u64,
    dropped_count: u64,
    reconnect_count: u64,
    last_summary_at: &mut Instant,
) {
    let total = passed_count.saturating_add(dropped_count);
    if total == 0 {
        return;
    }
    if last_summary_at.elapsed() < Duration::from_secs(config.prefilter_summary_interval_secs) {
        return;
    }
    *last_summary_at = Instant::now();
    info!(
        "WSS prefilter summary passed={} dropped={} reconnects={}",
        passed_count, dropped_count, reconnect_count
    );
}

fn log_rpc_summary(
    config: &CollectorConfig,
    rpc_fetcher: &PatientRpcFetcher,
    create_enqueued: u64,
    trade_enqueued: u64,
    create_dropped_full: u64,
    create_dropped_closed: u64,
    trade_dropped_full: u64,
    trade_dropped_closed: u64,
    mints_skipped_capacity: u64,
    last_summary_at: &mut Instant,
) {
    let snapshot = rpc_fetcher.snapshot_metrics();
    let queue_total = create_enqueued
        .saturating_add(trade_enqueued)
        .saturating_add(create_dropped_full)
        .saturating_add(create_dropped_closed)
        .saturating_add(trade_dropped_full)
        .saturating_add(trade_dropped_closed)
        .saturating_add(mints_skipped_capacity);
    if snapshot.attempted == 0 && queue_total == 0 {
        return;
    }
    if last_summary_at.elapsed() < Duration::from_secs(config.prefilter_summary_interval_secs) {
        return;
    }
    *last_summary_at = Instant::now();
    info!(
        "RPC getTransaction summary attempted={} success={} failed={} create_enqueued={} trade_enqueued={} create_dropped_full={} create_dropped_closed={} trade_dropped_full={} trade_dropped_closed={} mints_skipped_capacity={}",
        snapshot.attempted,
        snapshot.success,
        snapshot.failed,
        create_enqueued,
        trade_enqueued,
        create_dropped_full,
        create_dropped_closed,
        trade_dropped_full,
        trade_dropped_closed,
        mints_skipped_capacity
    );
}

fn handle_rpc_queue_drop(
    signature: String,
    dropped_count: &mut u64,
    queue_name: &str,
    reason: &str,
) {
    *dropped_count = dropped_count.saturating_add(1);
    warn!(
        "RPC {} queue dropped signature={} reason={} dropped={}",
        queue_name, signature, reason, dropped_count
    );
}

fn required_env(key: &str) -> Result<String> {
    env::var(key).with_context(|| format!("Missing required env {key}"))
}

fn parse_env_bool(key: &str) -> Result<Option<bool>> {
    match env::var(key) {
        Ok(value) => {
            let normalized = value.trim().to_lowercase();
            match normalized.as_str() {
                "1" | "true" | "yes" => Ok(Some(true)),
                "0" | "false" | "no" => Ok(Some(false)),
                _ => bail!("Invalid {key} value: {value}"),
            }
        }
        Err(env::VarError::NotPresent) => Ok(None),
        Err(err) => Err(err).with_context(|| format!("Failed to read {key}")),
    }
}

fn parse_env<T>(key: &str) -> Result<Option<T>>
where
    T: std::str::FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    match env::var(key) {
        Ok(value) => {
            Ok(Some(value.trim().parse::<T>().with_context(|| {
                format!("Invalid {key} value: {value}")
            })?))
        }
        Err(env::VarError::NotPresent) => Ok(None),
        Err(err) => Err(err).with_context(|| format!("Failed to read {key}")),
    }
}

fn unix_ms() -> Result<i64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| anyhow!("System time before UNIX_EPOCH: {err}"))?;
    i64::try_from(duration.as_millis())
        .map_err(|_| anyhow!("System time exceeds i64 millisecond range"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rpc_retry_delay_ms_caps_exponent() {
        assert_eq!(rpc_retry_delay_ms(0), 1_000);
        assert_eq!(rpc_retry_delay_ms(1), 2_000);
        assert_eq!(rpc_retry_delay_ms(2), 4_000);
        assert_eq!(rpc_retry_delay_ms(3), 8_000);
        assert_eq!(rpc_retry_delay_ms(4), 16_000);
        assert_eq!(rpc_retry_delay_ms(10), 16_000);
    }

    #[test]
    fn test_parse_trade_params_buy_offsets() {
        let mut data = Vec::new();
        data.extend_from_slice(&PUMPFUN_BUY_DISCRIMINATOR);
        data.extend_from_slice(&1234u64.to_le_bytes());
        data.extend_from_slice(&5678u64.to_le_bytes());
        let (amount, max_sol_cost, min_sol_output) =
            parse_trade_params(&data, true).expect("trade params should parse");
        assert_eq!(amount, 1234);
        assert_eq!(max_sol_cost, 5678);
        assert_eq!(min_sol_output, 0);
    }

    #[test]
    fn test_parse_trade_params_sell_offsets() {
        let mut data = Vec::new();
        data.extend_from_slice(&PUMPFUN_SELL_DISCRIMINATOR);
        data.extend_from_slice(&999u64.to_le_bytes());
        data.extend_from_slice(&444u64.to_le_bytes());
        let (amount, max_sol_cost, min_sol_output) =
            parse_trade_params(&data, false).expect("trade params should parse");
        assert_eq!(amount, 999);
        assert_eq!(max_sol_cost, 0);
        assert_eq!(min_sol_output, 444);
    }

    #[test]
    fn test_mint_state_aggregation() {
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let t0_unix_ms = 1_700_000_000_000i64;
        let window_ms = 900_000i64;
        let create = PumpfunCreate {
            mint,
            bonding_curve,
            timestamp_ms: t0_unix_ms,
        };
        let mut state = MintState::new(&create, window_ms);

        let trade_one = PumpfunTrade {
            mint,
            bonding_curve,
            signer: Pubkey::new_unique(),
            amount: 10,
            max_sol_cost: 500,
            min_sol_output: 0,
            is_buy: true,
            timestamp_ms: t0_unix_ms + 5_000,
        };
        let trade_two = PumpfunTrade {
            mint,
            bonding_curve,
            signer: Pubkey::new_unique(),
            amount: 8,
            max_sol_cost: 0,
            min_sol_output: 250,
            is_buy: false,
            timestamp_ms: t0_unix_ms + 8_000,
        };

        state.apply_trade(&trade_one, true);
        state.apply_trade(&trade_two, true);

        assert_eq!(state.tx_count, 2);
        assert_eq!(state.unique_traders.len(), 2);
        assert_eq!(state.buy_volume_lamports, 500);
        assert_eq!(state.sell_volume_lamports, 250);
        assert_eq!(state.first_trade_delay_ms(), Some(5_000));

        let outcome = state.outcome_record(t0_unix_ms + window_ms + 1);
        assert_eq!(outcome.tx_count_15m, 2);
        assert_eq!(outcome.unique_traders_15m, 2);
        assert_eq!(outcome.total_volume_lamports_15m, 750);
        assert_eq!(outcome.first_trade_delay_ms, Some(5_000));
    }
}
