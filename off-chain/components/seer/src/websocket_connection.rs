//! WebSocket/Geyser connection module
//!
//! # DEPRECATED AS PRIMARY STREAM
//!
//! `WebSocketConnection::connect_geyser()` is a LEGACY stream that works via:
//!   1. `logsSubscribe` → detects creation events by log string matching
//!   2. Per-event RPC `getTransaction` call — adds 300-2000ms latency per event
//!
//! This is NEVER acceptable for the primary stream. The canonical stream is
//! `GrpcConnection` (Yellowstone gRPC) which delivers raw tx bytes at sub-5ms latency.
//!
//! `WebSocketConnection` is kept **only** as an explicit legacy fallback when gRPC is unavailable
//! (config: `grpc_commitment_fallback_to_websocket = true`). It is disabled by default and in
//! production it should never be active.
//!
//! The public utility functions (`parse_ui_transaction_with_meta`, `extract_logs_from_meta`,
//! `extract_balances_from_meta`) are still used by `lib.rs` for the RPC backfill path
//! and the curve_resolve path. These are fine to keep.
use crate::config::CommitmentLevel;
use crate::errors::{SeerError, SeerResult};
use crate::metrics::SeerMetrics;
use crate::paradox_sensor::ParadoxSensor;
use crate::rpc_http_client::new_async_rpc_client;
use crate::types::{
    GeyserEvent, InnerInstructionGroup, InnerIx, RawBytesMissingReason, RawInstruction,
    RawTokenBalance,
};
use futures_util::{SinkExt, Stream, StreamExt};
use ghost_core::shadow_ledger::ShadowLedger;
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
use std::collections::HashMap;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, info, warn};

/// Label for Pump.fun AMM metrics
const PUMPFUN_AMM_LABEL: &str = "pumpfun";

/// Pump.fun program ID (6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P)
const PUMPFUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

/// Bonding curve account index in Pump.fun Create instruction
const BONDING_CURVE_ACCOUNT_INDEX: usize = 2;

/// Base delay for exponential backoff in milliseconds (Patient Fetcher)
/// Starts at 500ms as per issue requirements
const RETRY_BASE_DELAY_MS: u64 = 500;

/// Parse a UI transaction and extract accounts and instructions.
///
/// Handles Json, JsonParsed, and Base64 encodings. Helius may return different formats
/// based on RPC node configuration.
///
/// Returns (accounts, instructions) tuple extracted from the transaction.
#[allow(dead_code)]
pub(crate) fn parse_ui_transaction(
    encoded_tx: &EncodedTransaction,
) -> Option<(Vec<Pubkey>, Vec<RawInstruction>)> {
    parse_ui_transaction_with_meta(encoded_tx, None)
}

/// Parse a UI transaction and extract accounts and instructions, using meta for ALT keys when available.
pub fn parse_ui_transaction_with_meta(
    encoded_tx: &EncodedTransaction,
    meta: Option<&UiTransactionStatusMeta>,
) -> Option<(Vec<Pubkey>, Vec<RawInstruction>)> {
    match encoded_tx {
        EncodedTransaction::Json(ui_tx) => {
            match &ui_tx.message {
                UiMessage::Raw(raw_msg) => {
                    // Parse account keys from strings
                    let accounts: Vec<Pubkey> = raw_msg
                        .account_keys
                        .iter()
                        .filter_map(|key_str| Pubkey::from_str(key_str).ok())
                        .collect();

                    // Parse instructions
                    let instructions: Vec<RawInstruction> = raw_msg
                        .instructions
                        .iter()
                        .filter_map(|ix| {
                            // Get the program ID from account_keys using program_id_index
                            let program_id = accounts.get(ix.program_id_index as usize)?.to_owned();

                            // Decode instruction data from base58
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
                    // Fallback for Helius Free Tier which may return parsed format
                    // even when Json encoding is requested
                    info!("📋 Handling JsonParsed format (Helius fallback)");

                    // Parse account keys from ParsedAccount structures
                    let accounts: Vec<Pubkey> = parsed_msg
                        .account_keys
                        .iter()
                        .filter_map(|acc| Pubkey::from_str(&acc.pubkey).ok())
                        .collect();

                    // Parse instructions from UiInstruction enum
                    let instructions: Vec<RawInstruction> = parsed_msg
                        .instructions
                        .iter()
                        .filter_map(|ui_ix| parse_ui_instruction(ui_ix, &accounts))
                        .collect();

                    info!(
                        "📋 Parsed {} accounts and {} instructions from JsonParsed format",
                        accounts.len(),
                        instructions.len()
                    );

                    Some((accounts, instructions))
                }
            }
        }
        EncodedTransaction::LegacyBinary(data_str) | EncodedTransaction::Binary(data_str, _) => {
            // Handle Base64-encoded transaction
            info!("📋 Handling Base64-encoded transaction");

            // Decode base64 data using base64 v0.21 API
            use base64::{engine::general_purpose, Engine as _};
            let tx_data = match general_purpose::STANDARD.decode(data_str) {
                Ok(data) => data,
                Err(e) => {
                    warn!("⚠️ Failed to decode Base64 transaction data: {}", e);
                    return None;
                }
            };

            // Try to deserialize as VersionedTransaction first (modern format)
            // If that fails, fall back to legacy Transaction
            match bincode::deserialize::<VersionedTransaction>(&tx_data) {
                Ok(versioned_tx) => {
                    // Extract accounts and instructions from versioned transaction
                    let message = versioned_tx.message;

                    // Get static account keys from the message
                    let static_keys = message.static_account_keys();

                    // For v0 transactions, indices can point into loaded ALT addresses.
                    // Use meta.loadedAddresses when available to rebuild full key list.
                    let mut accounts: Vec<Pubkey> = static_keys.iter().copied().collect();
                    if let Some(meta) = meta {
                        append_loaded_addresses(&mut accounts, meta);
                    }

                    // Extract instructions
                    let instructions: Vec<RawInstruction> = message
                        .instructions()
                        .iter()
                        .filter_map(|ix| {
                            let Some(program_id) =
                                accounts.get(ix.program_id_index as usize).copied()
                            else {
                                warn!(
                                    "⚠️ Base64 VersionedTransaction program_id_index OOB: {} >= {}",
                                    ix.program_id_index,
                                    accounts.len()
                                );
                                return None;
                            };
                            Some(RawInstruction {
                                program_id,
                                account_indices: ix.accounts.clone(),
                                data: ix.data.clone(),
                            })
                        })
                        .collect();

                    info!("📋 Parsed {} accounts and {} instructions from Base64 VersionedTransaction",
                        accounts.len(), instructions.len());

                    Some((accounts, instructions))
                }
                Err(versioned_err) => {
                    // Fall back to legacy Transaction format
                    match bincode::deserialize::<Transaction>(&tx_data) {
                        Ok(tx) => {
                            // Extract accounts from transaction
                            let accounts = tx.message.account_keys.clone();

                            // Extract instructions
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

                            info!("📋 Parsed {} accounts and {} instructions from Base64 legacy Transaction",
                                accounts.len(), instructions.len());

                            Some((accounts, instructions))
                        }
                        Err(legacy_err) => {
                            warn!(
                                "⚠️ Failed to deserialize transaction as VersionedTransaction: {}",
                                versioned_err
                            );
                            warn!(
                                "⚠️ Failed to deserialize transaction as legacy Transaction: {}",
                                legacy_err
                            );
                            warn!("⚠️ Transaction data length: {} bytes", tx_data.len());
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

/// Extend `accounts` with ALT-loaded addresses from transaction metadata.
fn append_loaded_addresses(accounts: &mut Vec<Pubkey>, meta: &UiTransactionStatusMeta) {
    let solana_transaction_status::option_serializer::OptionSerializer::Some(loaded) =
        &meta.loaded_addresses
    else {
        return;
    };

    let addrs = loaded
        .writable
        .iter()
        .chain(loaded.readonly.iter())
        .filter_map(|key| Pubkey::from_str(key).ok());
    accounts.extend(addrs);
}

/// Parse a UiInstruction into RawInstruction.
///
/// Handles both Compiled and Parsed variants of UiInstruction.
fn parse_ui_instruction(ui_ix: &UiInstruction, accounts: &[Pubkey]) -> Option<RawInstruction> {
    match ui_ix {
        UiInstruction::Compiled(compiled) => {
            // Compiled instruction has program_id_index and raw data
            let program_id = accounts.get(compiled.program_id_index as usize)?.to_owned();
            let data = bs58::decode(&compiled.data).into_vec().ok()?;

            Some(RawInstruction {
                program_id,
                account_indices: compiled.accounts.clone(),
                data,
            })
        }
        UiInstruction::Parsed(parsed) => {
            match parsed {
                UiParsedInstruction::PartiallyDecoded(partial) => {
                    // PartiallyDecoded has program_id as string and data as base58
                    let program_id = Pubkey::from_str(&partial.program_id).ok()?;
                    let data = bs58::decode(&partial.data).into_vec().ok()?;

                    // Convert account strings to indices
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
                    // Fully parsed instruction - extract program_id but data is already parsed
                    // We can't easily reconstruct raw instruction data from parsed format,
                    // but we can still emit the event with accounts for pool detection
                    let program_id = Pubkey::from_str(&fully_parsed.program_id).ok()?;

                    // For fully parsed instructions, we don't have raw data
                    // Return empty data - the accounts are the primary data needed for pool detection
                    Some(RawInstruction {
                        program_id,
                        account_indices: Vec::new(),
                        data: Vec::new(),
                    })
                }
            }
        }
    }
}

/// Extract logs from transaction metadata.
pub fn extract_logs_from_meta(meta: &UiTransactionStatusMeta) -> Vec<String> {
    match &meta.log_messages {
        OptionSerializer::Some(logs) => logs.clone(),
        OptionSerializer::Skip | OptionSerializer::None => Vec::new(),
    }
}

/// Extract pre/post balances from transaction metadata.
pub fn extract_balances_from_meta(meta: &UiTransactionStatusMeta) -> (Vec<u64>, Vec<u64>) {
    (meta.pre_balances.clone(), meta.post_balances.clone())
}

fn option_serializer_string(value: &OptionSerializer<String>) -> Option<String> {
    match value {
        OptionSerializer::Some(value) if !value.is_empty() => Some(value.clone()),
        _ => None,
    }
}

fn decode_ui_instruction(
    ui_ix: &UiInstruction,
    accounts: &[Pubkey],
) -> Option<(u8, Vec<u8>, Vec<u8>, Option<u32>)> {
    match ui_ix {
        UiInstruction::Compiled(compiled) => Some((
            compiled.program_id_index,
            compiled.accounts.clone(),
            bs58::decode(&compiled.data).into_vec().ok()?,
            compiled.stack_height,
        )),
        UiInstruction::Parsed(parsed) => match parsed {
            UiParsedInstruction::PartiallyDecoded(partial) => {
                let program_id = Pubkey::from_str(&partial.program_id).ok()?;
                let program_id_index = accounts
                    .iter()
                    .position(|account| *account == program_id)
                    .map(|idx| idx as u8)?;
                let account_indices: Vec<u8> = partial
                    .accounts
                    .iter()
                    .filter_map(|account| {
                        let pubkey = Pubkey::from_str(account).ok()?;
                        accounts
                            .iter()
                            .position(|candidate| *candidate == pubkey)
                            .map(|idx| idx as u8)
                    })
                    .collect();
                Some((
                    program_id_index,
                    account_indices,
                    bs58::decode(&partial.data).into_vec().ok()?,
                    partial.stack_height,
                ))
            }
            UiParsedInstruction::Parsed(fully_parsed) => {
                let program_id = Pubkey::from_str(&fully_parsed.program_id).ok()?;
                let program_id_index = accounts
                    .iter()
                    .position(|account| *account == program_id)
                    .map(|idx| idx as u8)?;
                Some((
                    program_id_index,
                    Vec::new(),
                    Vec::new(),
                    fully_parsed.stack_height,
                ))
            }
        },
    }
}

pub fn extract_inner_instructions_from_meta(
    meta: &UiTransactionStatusMeta,
    accounts: &[Pubkey],
) -> Vec<InnerInstructionGroup> {
    let OptionSerializer::Some(groups) = &meta.inner_instructions else {
        return Vec::new();
    };

    groups
        .iter()
        .map(|group| InnerInstructionGroup {
            index: u32::from(group.index),
            instructions: group
                .instructions
                .iter()
                .filter_map(|instruction| {
                    let (program_id_index, account_indices, data, stack_height) =
                        decode_ui_instruction(instruction, accounts)?;
                    Some(InnerIx {
                        program_id_index,
                        accounts: account_indices,
                        data,
                        stack_height,
                    })
                })
                .collect(),
        })
        .collect()
}

fn convert_token_balances(
    balances: &[solana_transaction_status::UiTransactionTokenBalance],
) -> Vec<RawTokenBalance> {
    balances
        .iter()
        .map(|balance| RawTokenBalance {
            account_index: u32::from(balance.account_index),
            mint: balance.mint.clone(),
            owner: option_serializer_string(&balance.owner),
            amount: balance.ui_token_amount.amount.parse::<u64>().unwrap_or(0),
        })
        .collect()
}

pub fn extract_token_balances_from_meta(
    meta: &UiTransactionStatusMeta,
) -> (Vec<RawTokenBalance>, Vec<RawTokenBalance>) {
    let pre = match &meta.pre_token_balances {
        OptionSerializer::Some(balances) => convert_token_balances(balances),
        _ => Vec::new(),
    };
    let post = match &meta.post_token_balances {
        OptionSerializer::Some(balances) => convert_token_balances(balances),
        _ => Vec::new(),
    };
    (pre, post)
}

pub type EventStream = Pin<Box<dyn Stream<Item = SeerResult<GeyserEvent>> + Send>>;

pub struct WebSocketConnection {
    endpoint: String,
    rpc_endpoint: String,
    metrics: Arc<SeerMetrics>,
    max_reconnect_attempts: u32,
    reconnect_delay_secs: u64,
    verbose: bool,
    shadow_ledger: Option<Arc<ShadowLedger>>,
    paradox_sensor: Option<Arc<ParadoxSensor>>,
    commitment: CommitmentLevel,
}

impl WebSocketConnection {
    pub fn new(
        endpoint: String,
        rpc_endpoint: String,
        metrics: Arc<SeerMetrics>,
        max_reconnect_attempts: u32,
        reconnect_delay_secs: u64,
        verbose: bool,
        commitment: CommitmentLevel,
    ) -> Self {
        Self {
            endpoint,
            rpc_endpoint,
            metrics,
            max_reconnect_attempts,
            reconnect_delay_secs,
            verbose,
            shadow_ledger: None,
            paradox_sensor: None,
            commitment,
        }
    }

    pub fn new_with_shadow_ledger(
        endpoint: String,
        rpc_endpoint: String,
        metrics: Arc<SeerMetrics>,
        max_reconnect_attempts: u32,
        reconnect_delay_secs: u64,
        verbose: bool,
        shadow_ledger: Arc<ShadowLedger>,
        commitment: CommitmentLevel,
    ) -> Self {
        Self {
            endpoint,
            rpc_endpoint,
            metrics,
            max_reconnect_attempts,
            reconnect_delay_secs,
            verbose,
            shadow_ledger: Some(shadow_ledger),
            paradox_sensor: None,
            commitment,
        }
    }

    /// Set the Paradox Sensor for this connection
    ///
    /// This allows the WebSocket to report network pulses to the sensor
    /// for telemetry analysis.
    pub fn with_paradox_sensor(mut self, sensor: Arc<ParadoxSensor>) -> Self {
        self.paradox_sensor = Some(sensor);
        self
    }

    /// Fetch account data with Patient Fetcher pattern (exponential backoff)
    ///
    /// This implements the "Patient Fetcher" strategy required by the issue:
    /// - Retries with exponential backoff: 500ms → 1s → 2s
    /// - Handles RPC indexing delays where account might not be immediately available
    async fn fetch_account_data_patient(
        rpc: &RpcClient,
        pubkey: &Pubkey,
        max_attempts: u32,
    ) -> Option<Vec<u8>> {
        for attempt in 0..max_attempts {
            match rpc.get_account(pubkey).await {
                Ok(account) => {
                    debug!(
                        "✅ Account data fetched for {} on attempt {}",
                        pubkey,
                        attempt + 1
                    );
                    return Some(account.data);
                }
                Err(e) => {
                    // Exponential backoff: 500ms, 1000ms, 2000ms
                    // Cap attempt at 10 to prevent overflow
                    let delay = RETRY_BASE_DELAY_MS * (1 << attempt.min(10));
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

    pub async fn connect_geyser(&self) -> SeerResult<EventStream> {
        // DEPRECATION WARNING: This WebSocket stream uses logsNotification + per-event
        // RPC getTransaction fetches. Latency is 300-2000ms per event.
        // Use GrpcConnection in production. This path only exists as gRPC fallback.
        warn!(
            "WebSocketConnection::connect_geyser() activated — this is the LEGACY stream. \
             Latency will be 300-2000ms per event. Use GeyserGrpc in production."
        );
        info!("Connecting to Geyser WS at {}", self.endpoint);
        let mut attempt = 0;
        loop {
            match self.try_connect().await {
                Ok(stream) => {
                    info!("Successfully connected to Geyser");
                    return Ok(stream);
                }
                Err(e) => {
                    attempt += 1;
                    if attempt >= self.max_reconnect_attempts {
                        return Err(e);
                    }
                    sleep(Duration::from_secs(self.reconnect_delay_secs)).await;
                }
            }
        }
    }

    async fn try_connect(&self) -> SeerResult<EventStream> {
        let (ws_stream, _) = connect_async(&self.endpoint)
            .await
            .map_err(|e| SeerError::WebSocketError(e.to_string()))?;

        let (mut write, read) = ws_stream.split();

        // Subskrypcja Pump.fun (Filter: ID Programu)
        let subscribe_msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "logsSubscribe",
            "params": [
                { "mentions": ["6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"] },
                { "commitment": "confirmed" }
            ]
        });

        write
            .send(Message::Text(subscribe_msg.to_string()))
            .await
            .map_err(|e| SeerError::WebSocketError(e.to_string()))?;

        let rpc_client = Arc::new(new_async_rpc_client(self.rpc_endpoint.clone()));
        let metrics = Arc::clone(&self.metrics);
        let _verbose = self.verbose;
        let paradox_sensor = self.paradox_sensor.clone();
        let rpc_commitment = match self.commitment {
            CommitmentLevel::Mempool => CommitmentConfig::processed(),
            CommitmentLevel::Confirmed => CommitmentConfig::confirmed(),
            CommitmentLevel::Finalized => CommitmentConfig::finalized(),
        };

        let event_stream = read.filter_map(move |msg| {
            let metrics = Arc::clone(&metrics);
            let rpc = Arc::clone(&rpc_client);
            let sensor = paradox_sensor.clone();

            async move {
                match msg {
                    Ok(Message::Text(text)) => {
                        // [PARADOX SENSOR HOOK]
                        // Record network pulse immediately upon message arrival
                        // This measures the "physical" arrival time before any processing
                        if let Some(ref sensor) = sensor {
                            sensor.record_pulse(text.len());
                        }

                        // Wykrywanie "Create" w tekście
                        if text.contains("Create") || text.contains("Initialize") {
                             info!("⚡ POTENTIAL GEM DETECTED (Log): {}", text.chars().take(100).collect::<String>());
                        }

                        let v: Value = match serde_json::from_str(&text) {
                            Ok(v) => v,
                            Err(_) => return None,
                        };

                        if v.get("method") != Some(&Value::String("logsNotification".to_string())) {
                            return None;
                        }

                        let params = v.get("params")?.get("result")?.get("value")?;
                        let logs = params.get("logs")?.as_array()?;
                        let signature_str = params.get("signature")?.as_str()?;

                        let logs_str = serde_json::to_string(logs).unwrap_or_default();
                        let is_creation = logs_str.contains("Instruction: Create") || logs_str.contains("InitializeMint");

                        if is_creation {
                            info!("🔥 FOUND CREATION LOG! Signature: {}. Fetching details...", signature_str);

                            let sig = Signature::from_str(signature_str).ok()?;

                            // Patient Fetcher: Use Base64 encoding for reliable binary data
                            // Use 'confirmed' commitment to find transaction immediately
                            let tx_config = RpcTransactionConfig {
                                encoding: Some(UiTransactionEncoding::Base64),
                                commitment: Some(rpc_commitment.clone()),
                                max_supported_transaction_version: Some(0),
                            };

                            // Retry loop with exponential backoff (500ms -> 1s -> 2s)
                            // Patient Fetcher: Gives RPC more time on later retries
                            for attempt in 0..3u32 {
                                match rpc.get_transaction_with_config(&sig, tx_config).await {
                                    Ok(tx) => {
                                        if let Some(meta) = &tx.transaction.meta {
                                            info!("✅ TRANSACTION FETCHED SUCCESSFULLY! Proceeding to analysis...");

                                            let (pre_balances, post_balances) =
                                                extract_balances_from_meta(meta);

                                            // Parse the transaction to extract accounts and instructions
                                            if let Some((accounts, instructions)) = parse_ui_transaction_with_meta(
                                                &tx.transaction.transaction,
                                                Some(meta),
                                            ) {
                                                // Extract logs and metadata-rich transaction context.
                                                let tx_logs = extract_logs_from_meta(meta);
                                                let inner_instructions =
                                                    extract_inner_instructions_from_meta(
                                                        meta, &accounts,
                                                    );
                                                let (pre_token_balances, post_token_balances) =
                                                    extract_token_balances_from_meta(meta);

                                                // Record metric for pool detection
                                                metrics.initialize_pool_detected.with_label_values(&[PUMPFUN_AMM_LABEL]).inc();

                                                // Fetch bonding curve account data
                                                // For Pump.fun Create instruction, bonding curve is at BONDING_CURVE_ACCOUNT_INDEX
                                                let mut account_data = HashMap::new();

                                                // Find the bonding curve account by looking for Pump.fun program instructions
                                                for instruction in &instructions {
                                                    if instruction.program_id.to_string() == PUMPFUN_PROGRAM_ID {
                                                        // Bonding curve is at account index 2 for Pump.fun Create
                                                        if let Some(&bonding_curve_idx) = instruction.account_indices.get(BONDING_CURVE_ACCOUNT_INDEX) {
                                                            if let Some(bonding_curve_pubkey) = accounts.get(bonding_curve_idx as usize) {
                                                                info!("🔍 Fetching bonding curve account data for {}", bonding_curve_pubkey);

                                                                // Patient Fetcher: Fetch bonding curve account data
                                                                if let Some(bc_data) = Self::fetch_account_data_patient(
                                                                    &rpc,
                                                                    bonding_curve_pubkey,
                                                                    3, // 3 attempts with backoff
                                                                ).await {
                                                                    info!("✅ Bonding curve account data fetched: {} bytes", bc_data.len());
                                                                    account_data.insert(*bonding_curve_pubkey, bc_data);
                                                                } else {
                                                                    warn!("⚠️ Failed to fetch bonding curve account data for {}", bonding_curve_pubkey);
                                                                }
                                                            }
                                                        }
                                                    }
                                                }

                                                info!("📤 Emitting GeyserEvent::Transaction with {} accounts, {} instructions, {} account_data entries",
                                                    accounts.len(), instructions.len(), account_data.len());

                                                // Create and return the GeyserEvent
                                                let success = meta.err.is_none();
                                                let error_code = meta.err.as_ref().map(|err| format!("{:?}", err));
                                                let compute_units_consumed = Option::<u64>::from(meta.compute_units_consumed.clone());

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
                                                    success,
                                                    error_code,
                                                    compute_units_consumed,
                                                    synthetic: false,  // WebSocket events are real blockchain events
                                                    source: "websocket".to_string(),
                                                    // WebSocket provides parsed JSON, not raw bytes
                                                    mpcf_payload_bytes: None,
                                                    mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
                                                    inner_instructions,
                                                    pre_token_balances,
                                                    post_token_balances,
                                                };

                                                return Some(Ok(event));
                                            } else {
                                                warn!("⚠️ Failed to parse transaction data from EncodedTransaction");
                                            }
                                        } else {
                                            warn!("⚠️ Transaction {} has no metadata - cannot process", signature_str);
                                        }
                                        break;
                                    },
                                    Err(e) => {
                                        // Exponential backoff: 500ms, 1000ms, 2000ms
                                        // Cap attempt at 10 to prevent overflow
                                        let delay = RETRY_BASE_DELAY_MS * (1 << attempt.min(10));
                                        warn!("⚠️ Fetch retry {} (delay {}ms)... ({})", attempt + 1, delay, e);
                                        sleep(Duration::from_millis(delay)).await;
                                    }
                                }
                            }
                            warn!("❌ Failed to fetch transaction after retries: {}", signature_str);
                        }
                        None
                    }
                    _ => None,
                }
            }
        });

        Ok(Box::pin(event_stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose, Engine as _};
    use solana_sdk::hash::Hash;
    use solana_sdk::instruction::CompiledInstruction;
    use solana_sdk::message::{v0, Message, MessageHeader, VersionedMessage};
    use solana_sdk::system_instruction;
    use solana_sdk::transaction::{Transaction, VersionedTransaction};
    use solana_transaction_status::{
        option_serializer::OptionSerializer, TransactionBinaryEncoding, UiCompiledInstruction,
        UiInnerInstructions, UiInstruction, UiLoadedAddresses, UiTransactionTokenBalance,
    };

    #[test]
    fn test_parse_versioned_transaction_base64() {
        // Create a simple versioned transaction
        let payer = Pubkey::new_unique();
        let to = Pubkey::new_unique();

        let instruction = system_instruction::transfer(&payer, &to, 1000);
        let message = Message::new(&[instruction], Some(&payer));
        let versioned_message = VersionedMessage::Legacy(message);
        let versioned_tx = VersionedTransaction {
            signatures: vec![],
            message: versioned_message,
        };

        // Serialize and encode as base64
        let serialized = bincode::serialize(&versioned_tx).unwrap();
        let base64_encoded = general_purpose::STANDARD.encode(&serialized);

        // Create EncodedTransaction::Binary
        let encoded_tx =
            EncodedTransaction::Binary(base64_encoded, TransactionBinaryEncoding::Base64);

        // Parse it
        let result = parse_ui_transaction(&encoded_tx);

        // Verify parsing succeeded
        assert!(
            result.is_some(),
            "Should successfully parse VersionedTransaction"
        );
        let (accounts, instructions) = result.unwrap();

        // Should have at least the payer and recipient accounts
        assert!(accounts.len() >= 2, "Should have at least 2 accounts");

        // Should have at least one instruction
        assert_eq!(instructions.len(), 1, "Should have 1 instruction");
    }

    #[test]
    fn test_parse_legacy_transaction_base64() {
        // Create a simple legacy transaction
        let payer = Pubkey::new_unique();
        let to = Pubkey::new_unique();

        let instruction = system_instruction::transfer(&payer, &to, 1000);
        let message = Message::new(&[instruction], Some(&payer));
        let legacy_tx = Transaction {
            signatures: vec![],
            message,
        };

        // Serialize and encode as base64
        let serialized = bincode::serialize(&legacy_tx).unwrap();
        let base64_encoded = general_purpose::STANDARD.encode(&serialized);

        // Create EncodedTransaction::LegacyBinary
        let encoded_tx = EncodedTransaction::LegacyBinary(base64_encoded);

        // Parse it
        let result = parse_ui_transaction(&encoded_tx);

        // Verify parsing succeeded
        assert!(
            result.is_some(),
            "Should successfully parse legacy Transaction"
        );
        let (accounts, instructions) = result.unwrap();

        // Should have at least the payer and recipient accounts
        assert!(accounts.len() >= 2, "Should have at least 2 accounts");

        // Should have at least one instruction
        assert_eq!(instructions.len(), 1, "Should have 1 instruction");
    }

    #[test]
    fn test_parse_invalid_base64() {
        // Create an invalid base64 string
        let encoded_tx = EncodedTransaction::Binary(
            "!!!invalid base64!!!".to_string(),
            TransactionBinaryEncoding::Base64,
        );

        // Parse should fail gracefully
        let result = parse_ui_transaction(&encoded_tx);
        assert!(result.is_none(), "Should fail gracefully on invalid base64");
    }

    #[test]
    fn test_parse_versioned_transaction_uses_loaded_addresses_from_meta() {
        let payer = Pubkey::new_unique();
        let program = Pubkey::new_unique();
        let looked_up = Pubkey::new_unique();

        let message = v0::Message {
            header: MessageHeader {
                num_required_signatures: 1,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: 1,
            },
            account_keys: vec![payer, program],
            recent_blockhash: Hash::default(),
            instructions: vec![CompiledInstruction {
                program_id_index: 1,
                accounts: vec![0, 2], // index 2 points into loaded addresses
                data: vec![1, 2, 3],
            }],
            address_table_lookups: vec![],
        };

        let versioned_tx = VersionedTransaction {
            signatures: vec![],
            message: VersionedMessage::V0(message),
        };

        let serialized = bincode::serialize(&versioned_tx).unwrap();
        let base64_encoded = general_purpose::STANDARD.encode(&serialized);
        let encoded_tx =
            EncodedTransaction::Binary(base64_encoded, TransactionBinaryEncoding::Base64);

        let meta = UiTransactionStatusMeta {
            err: None,
            status: Ok(()),
            fee: 0,
            pre_balances: vec![],
            post_balances: vec![],
            inner_instructions: OptionSerializer::Skip,
            log_messages: OptionSerializer::Skip,
            pre_token_balances: OptionSerializer::Skip,
            post_token_balances: OptionSerializer::Skip,
            rewards: OptionSerializer::Skip,
            loaded_addresses: OptionSerializer::Some(UiLoadedAddresses {
                writable: vec![looked_up.to_string()],
                readonly: vec![],
            }),
            return_data: OptionSerializer::Skip,
            compute_units_consumed: OptionSerializer::Skip,
        };

        let result = parse_ui_transaction_with_meta(&encoded_tx, Some(&meta));
        assert!(result.is_some());

        let (accounts, instructions) = result.unwrap();
        assert_eq!(accounts.len(), 3);
        assert_eq!(accounts[2], looked_up);
        assert_eq!(instructions.len(), 1);
        assert_eq!(instructions[0].account_indices, vec![0, 2]);
    }

    #[test]
    fn test_extract_token_balances_from_meta_preserves_owner() {
        let owner = Pubkey::new_unique();
        let owner_str = owner.to_string();
        let mint = Pubkey::new_unique();
        let token_balance: UiTransactionTokenBalance = serde_json::from_value(serde_json::json!({
            "accountIndex": 3,
            "mint": mint.to_string(),
            "uiTokenAmount": {
                "uiAmount": 1.25,
                "decimals": 6,
                "amount": "1250000",
                "uiAmountString": "1.25"
            },
            "owner": owner_str.clone()
        }))
        .expect("valid token balance json");
        let meta = UiTransactionStatusMeta {
            err: None,
            status: Ok(()),
            fee: 0,
            pre_balances: vec![],
            post_balances: vec![],
            inner_instructions: OptionSerializer::Skip,
            log_messages: OptionSerializer::Skip,
            pre_token_balances: OptionSerializer::Some(vec![token_balance]),
            post_token_balances: OptionSerializer::Skip,
            rewards: OptionSerializer::Skip,
            loaded_addresses: OptionSerializer::Skip,
            return_data: OptionSerializer::Skip,
            compute_units_consumed: OptionSerializer::Skip,
        };

        let (pre, post) = extract_token_balances_from_meta(&meta);
        assert!(post.is_empty());
        assert_eq!(pre.len(), 1);
        assert_eq!(pre[0].account_index, 3);
        assert_eq!(pre[0].mint, mint.to_string());
        assert_eq!(pre[0].owner.as_deref(), Some(owner_str.as_str()));
        assert_eq!(pre[0].amount, 1_250_000);
    }

    #[test]
    fn test_extract_inner_instructions_from_meta_preserves_compiled_ix() {
        let program = Pubkey::new_unique();
        let account = Pubkey::new_unique();
        let accounts = vec![account, program];
        let meta = UiTransactionStatusMeta {
            err: None,
            status: Ok(()),
            fee: 0,
            pre_balances: vec![],
            post_balances: vec![],
            inner_instructions: OptionSerializer::Some(vec![UiInnerInstructions {
                index: 7,
                instructions: vec![UiInstruction::Compiled(UiCompiledInstruction {
                    program_id_index: 1,
                    accounts: vec![0],
                    data: bs58::encode([1_u8, 2, 3]).into_string(),
                    stack_height: Some(4),
                })],
            }]),
            log_messages: OptionSerializer::Skip,
            pre_token_balances: OptionSerializer::Skip,
            post_token_balances: OptionSerializer::Skip,
            rewards: OptionSerializer::Skip,
            loaded_addresses: OptionSerializer::Skip,
            return_data: OptionSerializer::Skip,
            compute_units_consumed: OptionSerializer::Skip,
        };

        let groups = extract_inner_instructions_from_meta(&meta, &accounts);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].index, 7);
        assert_eq!(groups[0].instructions.len(), 1);
        assert_eq!(groups[0].instructions[0].program_id_index, 1);
        assert_eq!(groups[0].instructions[0].accounts, vec![0]);
        assert_eq!(groups[0].instructions[0].data, vec![1, 2, 3]);
        assert_eq!(groups[0].instructions[0].stack_height, Some(4));
    }
}
