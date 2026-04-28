//! Jito Bundle Client
//!
//! This module provides integration with Jito MEV for bundle submission.
//! Implements bundle creation with dynamic tip calculation and redundancy policies.

use crate::config::{BundleConfig, RedundancyPolicy, TipConfig};
use crate::errors::{Result, TriggerError};
use crate::jito_protos::{bundle, packet, searcher};
use base64::{engine::general_purpose, Engine as _};
use jito_sdk_rust::JitoJsonRpcSDK;
use rand::Rng;
use reqwest::{StatusCode, Url};
use serde_json::{json, Value};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::RpcSimulateTransactionConfig;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    hash::Hash,
    message::{v0, VersionedMessage},
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer},
    system_instruction,
    transaction::VersionedTransaction,
};
use spl_associated_token_account::get_associated_token_address_with_program_id;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tonic::{
    metadata::MetadataValue,
    transport::{ClientTlsConfig, Endpoint},
    Code,
};
use tracing::{debug, error, info, warn};

/// Jito Bundle Client
///
/// Handles submission of transaction bundles to Jito for MEV extraction
/// and improved inclusion guarantees.
pub struct JitoClient {
    /// Jito RPC endpoint
    endpoint: String,
    /// Base URL passed to `jito-sdk-rust`.
    sdk_base_url: String,
    /// Bare gRPC authority used for SearcherService/SendBundle submission.
    grpc_endpoint: String,
    /// Bundle configuration
    bundle_config: BundleConfig,
    /// Opaque auth token/header value for gRPC `x-jito-auth` metadata.
    grpc_auth: Option<String>,
    /// UUID used exclusively by the REST status API.
    status_uuid: Option<String>,
    /// Dry-run mode (log without sending)
    dry_run: bool,
    /// RPC client for simulation (optional)
    rpc_client: Option<Arc<RpcClient>>,
}

/// Represents a Jito bundle with ordered transactions
#[derive(Debug, Clone)]
pub struct JitoBundle {
    /// Ordered transactions in the bundle (InitializePool TX first, then Ghost TXs)
    pub transactions: Vec<VersionedTransaction>,
    /// Tip amount in lamports
    pub tip_lamports: u64,
    /// Bundle ID (first transaction signature)
    pub bundle_id: Signature,
    /// Recent blockhash used
    pub recent_blockhash: Hash,
}

/// Bundle submission diagnostics
#[derive(Debug, Clone)]
pub struct BundleDiagnostics {
    /// Bundle ID
    pub bundle_id: Signature,
    /// Number of transactions in bundle
    pub transaction_count: usize,
    /// Tip amount assigned (in lamports)
    pub tip_lamports: u64,
    /// Tip percentage of transaction value
    pub tip_percent: f64,
    /// Priority factor used (0.0 - 1.0)
    pub priority_factor: f64,
    /// Redundancy count (N+X)
    pub redundancy_count: usize,
    /// Whether nonce staggered was applied
    pub nonce_staggered: bool,
    /// Detailed explanation
    pub explanation: String,
}

/// Successful Jito submission receipt used by bundle confirmation gating.
#[derive(Debug, Clone)]
pub struct JitoBundleSubmission {
    /// Signature of the first transaction in the submitted bundle.
    pub signature: Signature,
    /// Ordered signatures of every transaction in the submitted bundle.
    pub signatures: Vec<Signature>,
    /// Jito-assigned bundle UUID used for `getBundleStatuses` polling.
    pub bundle_uuid: String,
    /// Exact Jito gRPC endpoint that acknowledged `sendBundle` for this submission.
    pub submit_endpoint: String,
}

#[derive(Debug, Clone)]
struct JitoSubmitAck {
    bundle_uuid: String,
    submit_endpoint: String,
}

/// Result of a bundle that was both submitted and confirmed on-chain.
#[derive(Debug, Clone)]
pub struct JitoConfirmedBundle {
    /// Signature of the first transaction in the bundle.
    pub signature: Signature,
    /// Ordered signatures of every transaction in the confirmed bundle.
    pub signatures: Vec<Signature>,
    /// Jito bundle UUID that was confirmed.
    pub bundle_uuid: String,
    /// Landed slot reported by Jito, if available.
    pub landed_slot: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BalanceDeltaDirection {
    Increase,
    Decrease,
}

/// Jito mainnet tip accounts (as of 2024)
/// These are the official Jito MEV tip distribution accounts
pub const JITO_TIP_ACCOUNTS: &[&str] = &[
    "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5",
    "HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe",
    "Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY",
    "ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49",
    "DfXygSm4jCyNCybVYYK6DwvWqjKee8pbDmJGcLWNDXjh",
    "ADuUkR4vqLUMWXxW9gh6D6L8pMSawimctcNZ5pGwDcEt",
    "DttWaMuVvTiduZRnguLF7jNxTgiMBZ1hyAumKUiL2KRL",
    "3AVi9Tg9Uo68tJfuvoKvqKNWKkC5wPdSSdeBnizKZ6jT",
];

/// Canonical JSON-RPC path for Jito block-engine bundle operations.
pub const JITO_BUNDLES_JSONRPC_PATH: &str = "/api/v1/bundles";

/// Canonical base path expected by `jito-sdk-rust` before it appends operation suffixes.
const JITO_SDK_BASE_PATH: &str = "/api/v1";

/// Official public Jito block-engine mainnet hosts — ordered by preferred failover priority.
/// Failover sweeps this list top-to-bottom: Frankfurt → Amsterdam → London → Dublin →
/// New York → Singapore → Tokyo → Salt Lake City.
const JITO_PUBLIC_MAINNET_HOSTS: &[&str] = &[
    "frankfurt.mainnet.block-engine.jito.wtf",
    "amsterdam.mainnet.block-engine.jito.wtf",
    "london.mainnet.block-engine.jito.wtf",
    "dublin.mainnet.block-engine.jito.wtf",
    "ny.mainnet.block-engine.jito.wtf",
    "singapore.mainnet.block-engine.jito.wtf",
    "tokyo.mainnet.block-engine.jito.wtf",
    "slc.mainnet.block-engine.jito.wtf",
];

/// Live SendBundle gRPC rotation is intentionally pinned to the four EU block-engine hosts.
/// This keeps retry order deterministic for live trading: Frankfurt → Amsterdam → London → Dublin.
const JITO_GRPC_FAILOVER_HOSTS: &[&str] = &[
    "frankfurt.mainnet.block-engine.jito.wtf",
    "amsterdam.mainnet.block-engine.jito.wtf",
    "london.mainnet.block-engine.jito.wtf",
    "dublin.mainnet.block-engine.jito.wtf",
];

/// Load-balanced public alias that should trigger the full regional failover sweep.
const JITO_PUBLIC_MAINNET_ALIAS: &str = "mainnet.block-engine.jito.wtf";

/// Per-endpoint timeout for bundle submission requests.
const JITO_SEND_BUNDLE_TIMEOUT_MS: u64 = 1_200;

/// Wait roughly one slot before repeating a full failover round.
const JITO_FAILOVER_ROUND_BACKOFF_MS: u64 = 600;

#[derive(Debug, Clone, PartialEq, Eq)]
enum JitoSubmitAttemptError {
    RateLimited {
        endpoint: String,
        code: Option<i64>,
        message: String,
    },
    RetryableTransport {
        endpoint: String,
        message: String,
    },
    Fatal {
        endpoint: String,
        message: String,
    },
}

impl JitoSubmitAttemptError {
    fn into_trigger_error(self, rounds: usize, endpoint_count: usize) -> TriggerError {
        match self {
            Self::Fatal { message, .. } => TriggerError::JitoBundleError(message),
            Self::RateLimited {
                endpoint,
                code,
                message,
            } => TriggerError::JitoBundleError(format!(
                "Failed to submit bundle after {} round(s) across {} Jito endpoint(s); last rate-limited endpoint={} code={} message={}",
                rounds,
                endpoint_count,
                endpoint,
                code.map(|value| value.to_string()).unwrap_or_else(|| "none".to_string()),
                message
            )),
            Self::RetryableTransport { endpoint, message } => TriggerError::JitoBundleError(
                format!(
                    "Failed to submit bundle after {} round(s) across {} Jito endpoint(s); last retryable endpoint={} error={}",
                    rounds, endpoint_count, endpoint, message
                ),
            ),
        }
    }
}

/// Reachability classification for Jito endpoint probing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JitoEndpointProbeOutcome {
    TipAccounts {
        normalized_endpoint: String,
        tip_accounts: usize,
    },
    RateLimited {
        normalized_endpoint: String,
        code: Option<i64>,
        message: String,
    },
}

impl JitoEndpointProbeOutcome {
    #[must_use]
    pub fn normalized_endpoint(&self) -> &str {
        match self {
            Self::TipAccounts {
                normalized_endpoint,
                ..
            }
            | Self::RateLimited {
                normalized_endpoint,
                ..
            } => normalized_endpoint,
        }
    }
}

/// Normalize an operator-supplied Jito endpoint to the canonical bundle JSON-RPC path.
#[must_use]
pub fn normalize_jito_endpoint(endpoint: impl AsRef<str>) -> String {
    let trimmed = endpoint.as_ref().trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let candidate = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };

    let Ok(mut url) = Url::parse(&candidate) else {
        return candidate;
    };

    let path = url.path().trim_end_matches('/');
    if matches!(path, "" | "/" | "/api" | "/api/v1" | "/bundles") {
        url.set_path(JITO_BUNDLES_JSONRPC_PATH);
    }

    url.to_string()
}

fn normalize_jito_sdk_base_url(endpoint: impl AsRef<str>) -> String {
    let normalized_endpoint = normalize_jito_endpoint(endpoint);
    if normalized_endpoint.is_empty() {
        return String::new();
    }

    let Ok(mut url) = Url::parse(&normalized_endpoint) else {
        return normalized_endpoint;
    };

    let path = url.path().trim_end_matches('/');
    if path == JITO_BUNDLES_JSONRPC_PATH || matches!(path, "" | "/" | "/api" | "/api/v1") {
        url.set_path(JITO_SDK_BASE_PATH);
    }

    url.to_string()
}

fn normalize_jito_grpc_endpoint(endpoint: impl AsRef<str>) -> String {
    let trimmed = endpoint.as_ref().trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let candidate = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };

    let Ok(mut url) = Url::parse(&candidate) else {
        return candidate;
    };

    url.set_path("/");
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

fn build_public_jito_failover_endpoints(endpoint: impl AsRef<str>) -> Vec<String> {
    let primary = normalize_jito_endpoint(endpoint);
    if primary.is_empty() {
        return Vec::new();
    }

    let primary_host = Url::parse(&primary)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string));

    let mut endpoints = vec![primary.clone()];

    if matches!(
        primary_host.as_deref(),
        Some(host) if JITO_PUBLIC_MAINNET_HOSTS.contains(&host) || host == JITO_PUBLIC_MAINNET_ALIAS
    ) {
        for host in JITO_PUBLIC_MAINNET_HOSTS {
            let candidate = normalize_jito_endpoint(format!("https://{host}"));
            if !candidate.is_empty() && candidate != primary {
                endpoints.push(candidate);
            }
        }
    }

    endpoints
}

fn build_public_jito_failover_grpc_endpoints(endpoint: impl AsRef<str>) -> Vec<String> {
    let primary = normalize_jito_grpc_endpoint(endpoint);
    if primary.is_empty() {
        return Vec::new();
    }

    let primary_host = Url::parse(&primary)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string));

    if matches!(
        primary_host.as_deref(),
        Some(host) if JITO_PUBLIC_MAINNET_HOSTS.contains(&host) || host == JITO_PUBLIC_MAINNET_ALIAS
    ) {
        return JITO_GRPC_FAILOVER_HOSTS
            .iter()
            .map(|host| normalize_jito_grpc_endpoint(format!("https://{host}")))
            .collect();
    }

    vec![primary]
}

fn build_grpc_send_bundle_request(params: &Value) -> Result<searcher::SendBundleRequest> {
    let params_array = params.as_array().ok_or_else(|| {
        TriggerError::JitoBundleError(
            "Expected Jito bundle params array shaped like [[txs...], {encoding}]".to_string(),
        )
    })?;
    let serialized_txs = params_array
        .first()
        .and_then(Value::as_array)
        .ok_or_else(|| {
            TriggerError::JitoBundleError(
                "Missing Jito bundle transaction array in sendBundle params".to_string(),
            )
        })?;

    let encoding = params_array
        .get(1)
        .and_then(|value| value.get("encoding"))
        .and_then(Value::as_str)
        .unwrap_or("base64");
    if encoding != "base64" {
        return Err(TriggerError::JitoBundleError(format!(
            "Unsupported Jito bundle encoding for gRPC submit: {encoding}"
        )));
    }

    let mut packets = Vec::with_capacity(serialized_txs.len());
    for tx in serialized_txs {
        let encoded = tx.as_str().ok_or_else(|| {
            TriggerError::JitoBundleError(
                "Encountered non-string transaction payload in sendBundle params".to_string(),
            )
        })?;
        let bytes = general_purpose::STANDARD.decode(encoded).map_err(|e| {
            TriggerError::SerializationError(format!(
                "Failed to decode base64 bundle transaction for gRPC submit: {e}"
            ))
        })?;
        let packet_size = u64::try_from(bytes.len()).map_err(|_| {
            TriggerError::SerializationError(
                "Decoded bundle transaction exceeds supported packet size".to_string(),
            )
        })?;
        packets.push(packet::Packet {
            data: bytes,
            meta: Some(packet::Meta {
                size: packet_size,
                addr: String::new(),
                port: 0,
                flags: None,
                sender_stake: 0,
            }),
        });
    }

    if packets.is_empty() {
        return Err(TriggerError::JitoBundleError(
            "Cannot submit empty Jito bundle over gRPC".to_string(),
        ));
    }

    Ok(searcher::SendBundleRequest {
        bundle: Some(bundle::Bundle {
            header: None,
            packets,
        }),
    })
}

fn attach_jito_auth_metadata(
    endpoint: &str,
    uuid: Option<&str>,
    grpc_request: &mut tonic::Request<searcher::SendBundleRequest>,
) -> std::result::Result<(), JitoSubmitAttemptError> {
    let Some(uuid) = uuid.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };

    let metadata_value =
        MetadataValue::try_from(uuid).map_err(|e| JitoSubmitAttemptError::Fatal {
            endpoint: endpoint.to_string(),
            message: format!("Invalid UUID metadata for x-jito-auth: {e}"),
        })?;
    grpc_request
        .metadata_mut()
        .insert("x-jito-auth", metadata_value);

    Ok(())
}

fn classify_grpc_transport_message(endpoint: &str, message: String) -> JitoSubmitAttemptError {
    let lower = message.to_ascii_lowercase();
    if lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("deadline")
        || lower.contains("temporarily unavailable")
        || lower.contains("unavailable")
        || lower.contains("resource exhausted")
        || lower.contains("rate limit")
        || lower.contains("congest")
        || lower.contains("connection reset")
        || lower.contains("connection refused")
    {
        JitoSubmitAttemptError::RetryableTransport {
            endpoint: endpoint.to_string(),
            message,
        }
    } else {
        JitoSubmitAttemptError::Fatal {
            endpoint: endpoint.to_string(),
            message: format!("Jito gRPC SendBundle transport failed for {endpoint}: {message}"),
        }
    }
}

fn classify_grpc_status(endpoint: &str, status: tonic::Status) -> JitoSubmitAttemptError {
    let code = i64::from(status.code() as i32);
    let message = status.message().to_string();
    match status.code() {
        Code::ResourceExhausted => JitoSubmitAttemptError::RateLimited {
            endpoint: endpoint.to_string(),
            code: Some(code),
            message,
        },
        Code::Unavailable | Code::DeadlineExceeded | Code::Aborted | Code::Internal => {
            JitoSubmitAttemptError::RetryableTransport {
                endpoint: endpoint.to_string(),
                message: format!("grpc status {:?}: {}", status.code(), message),
            }
        }
        _ => JitoSubmitAttemptError::Fatal {
            endpoint: endpoint.to_string(),
            message: format!(
                "Jito gRPC SendBundle failed for {endpoint}: code={:?} message={}",
                status.code(),
                message
            ),
        },
    }
}

fn extract_jito_error_code_and_message(
    status: StatusCode,
    body: &Value,
    raw_body: &str,
) -> (Option<i64>, String) {
    let code = body
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(Value::as_i64);
    let message = body
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            let trimmed = raw_body.trim();
            if trimmed.is_empty() {
                format!("HTTP {} with empty response body", status)
            } else {
                trimmed.to_string()
            }
        });

    (code, message)
}

fn is_rate_limited_probe_response(status: StatusCode, body: &Value) -> bool {
    if status == StatusCode::TOO_MANY_REQUESTS {
        return true;
    }

    if matches!(
        body.get("error")
            .and_then(|error| error.get("code"))
            .and_then(Value::as_i64),
        Some(-32097)
    ) {
        return true;
    }

    body.get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .map(|message| {
            let normalized = message.to_ascii_lowercase();
            normalized.contains("rate limit")
                || normalized.contains("rate limited")
                || normalized.contains("congested")
        })
        .unwrap_or(false)
}

/// Probe a Jito endpoint for JSON-RPC reachability.
pub async fn probe_jito_endpoint(
    endpoint: &str,
    timeout: Duration,
) -> Result<JitoEndpointProbeOutcome> {
    let normalized_endpoint = normalize_jito_endpoint(endpoint);
    if normalized_endpoint.is_empty() {
        return Err(TriggerError::ConfigError(
            "Jito endpoint is empty after normalization".to_string(),
        ));
    }

    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| {
            TriggerError::NetworkError(format!("Failed to build Jito probe client: {e}"))
        })?;

    let response = client
        .post(&normalized_endpoint)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getTipAccounts",
            "params": []
        }))
        .send()
        .await
        .map_err(|e| {
            TriggerError::NetworkError(format!(
                "Jito getTipAccounts probe failed for {normalized_endpoint}: {e}"
            ))
        })?;

    let status = response.status();
    let body_text = response.text().await.map_err(|e| {
        TriggerError::NetworkError(format!(
            "Failed to read Jito probe response body for {normalized_endpoint}: {e}"
        ))
    })?;
    let body_json = serde_json::from_str::<Value>(&body_text).unwrap_or(Value::Null);

    if let Some(tips) = body_json.get("result").and_then(Value::as_array) {
        if !tips.is_empty() {
            return Ok(JitoEndpointProbeOutcome::TipAccounts {
                normalized_endpoint,
                tip_accounts: tips.len(),
            });
        }
    }

    if is_rate_limited_probe_response(status, &body_json) {
        let code = body_json
            .get("error")
            .and_then(|error| error.get("code"))
            .and_then(Value::as_i64);
        let message = body_json
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("HTTP {} without explicit JSON-RPC message", status));
        return Ok(JitoEndpointProbeOutcome::RateLimited {
            normalized_endpoint,
            code,
            message,
        });
    }

    if let Some(error) = body_json.get("error") {
        let code = error
            .get("code")
            .and_then(Value::as_i64)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("missing error message");
        return Err(TriggerError::JitoBundleError(format!(
            "Jito getTipAccounts probe returned JSON-RPC error code {code} for {normalized_endpoint}: {message}"
        )));
    }

    Err(TriggerError::JitoBundleError(format!(
        "Jito getTipAccounts probe returned unexpected HTTP {} for {}: {}",
        status,
        normalized_endpoint,
        body_text.trim()
    )))
}

// IRONCLAD TRANSACTION PROTOCOL CONSTANTS

/// Maximum time allowed for blockhash fetch (200ms)
/// If blockhash fetch takes longer, it's considered stale on arrival
const MAX_BLOCKHASH_FETCH_MS: u128 = 200;

/// Maximum time to wait for bundle confirmation (1500ms)
/// After this timeout, transaction is marked as FAILED with no retries
const BUNDLE_CONFIRMATION_TIMEOUT_MS: u64 = 1500;

/// Fail-closed window for reconciling non-accepted Jito bundle status with actual on-chain landing.
///
/// Live BUY capacity must stay reserved until we can either prove bundle failure or observe the
/// full submitted bundle on-chain. The observed production race landed ~38s after the initial
/// `Rejected` response, so this window intentionally remains much longer than the fast Jito poll.
const FAIL_CLOSED_ONCHAIN_RECONCILIATION_TIMEOUT_MS: u64 = 60_000;

/// Poll cadence while reconciling submitted bundle signatures on-chain.
const ONCHAIN_RECONCILIATION_POLL_INTERVAL_MS: u64 = 200;

/// Maximum retries for bundle submission (network errors only, not simulation errors)
const MAX_BUNDLE_RETRIES: usize = 3;

/// Maximum time allowed for transaction simulation (100ms)
/// If simulation takes longer, skip the transaction (market is moving too fast)
const MAX_SIMULATION_TIME_MS: u128 = 100;

/// Maximum compute units for a simple swap (400k)
/// Higher values may indicate honeypot with loops
const MAX_COMPUTE_UNITS_SIMPLE_SWAP: u64 = 400_000;

// Error detection patterns for simulation logs
/// Critical error patterns that indicate transaction failure
const CRITICAL_ERROR_PATTERNS: &[&str] = &[
    "insufficient funds",
    "insufficient lamports",
    "custom program error",
    "Error: ",
    "Failed to ",
    "Transaction failed",
];

/// Error patterns to ignore (false positives)
const IGNORED_ERROR_PATTERNS: &[&str] =
    &["no error", "NoError", "error code: 0", "ErrorCode::Success"];

impl JitoClient {
    /// Create a new Jito client
    ///
    /// # Arguments
    /// * `endpoint` - Jito RPC endpoint URL
    /// * `bundle_config` - Bundle building configuration
    pub fn new(endpoint: impl Into<String>, bundle_config: BundleConfig) -> Self {
        Self::new_with_credentials(endpoint, None, None, bundle_config)
    }

    pub fn new_with_status_uuid(
        endpoint: impl Into<String>,
        status_uuid: Option<String>,
        bundle_config: BundleConfig,
    ) -> Self {
        Self::new_with_credentials(endpoint, None, status_uuid, bundle_config)
    }

    pub fn new_with_auth(
        endpoint: impl Into<String>,
        grpc_auth: String,
        status_uuid: Option<String>,
        bundle_config: BundleConfig,
    ) -> Self {
        Self::new_with_credentials(endpoint, Some(grpc_auth), status_uuid, bundle_config)
    }

    /// Legacy constructor kept only for migration/test compatibility.
    pub fn new_with_uuid(
        endpoint: impl Into<String>,
        uuid: String,
        bundle_config: BundleConfig,
    ) -> Self {
        Self::new_with_credentials(endpoint, Some(uuid), None, bundle_config)
    }

    fn new_with_credentials(
        endpoint: impl Into<String>,
        grpc_auth: Option<String>,
        status_uuid: Option<String>,
        bundle_config: BundleConfig,
    ) -> Self {
        let raw_endpoint = endpoint.into();
        let endpoint = normalize_jito_endpoint(&raw_endpoint);
        let sdk_base_url = normalize_jito_sdk_base_url(&raw_endpoint);
        let grpc_endpoint = normalize_jito_grpc_endpoint(&raw_endpoint);

        Self {
            endpoint,
            sdk_base_url,
            grpc_endpoint,
            bundle_config,
            grpc_auth,
            status_uuid,
            dry_run: false,
            rpc_client: None,
        }
    }

    /// Set RPC client for transaction simulation (IRONCLAD PROTOCOL)
    ///
    /// When set, transactions will be simulated before submission to detect honeypots
    pub fn with_rpc_client(mut self, rpc_client: Arc<RpcClient>) -> Self {
        self.rpc_client = Some(rpc_client);
        self
    }

    /// Enable or disable dry-run mode
    ///
    /// In dry-run mode, bundles are logged but not actually submitted
    pub fn set_dry_run(&mut self, dry_run: bool) {
        self.dry_run = dry_run;
        if dry_run {
            info!("Jito client in DRY-RUN mode - bundles will be logged but not sent");
        }
    }

    /// Returns the normalized Jito bundle endpoint used by this client.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Returns whether the client is in dry-run mode.
    pub fn is_dry_run(&self) -> bool {
        self.dry_run
    }

    async fn send_bundle_request_to_endpoint(
        &self,
        endpoint: &str,
        params: &Value,
    ) -> std::result::Result<JitoSubmitAck, JitoSubmitAttemptError> {
        let request = build_grpc_send_bundle_request(params).map_err(|err| {
            JitoSubmitAttemptError::Fatal {
                endpoint: endpoint.to_string(),
                message: err.to_string(),
            }
        })?;

        tracing::debug!(
            endpoint = endpoint,
            packet_count = request.bundle.as_ref().map(|b| b.packets.len()).unwrap_or(0),
            packet_sizes = ?request
                .bundle
                .as_ref()
                .map(|b| b.packets.iter().map(|p| p.data.len()).collect::<Vec<_>>())
                .unwrap_or_default(),
            "gRPC SendBundleRequest before submit"
        );

        let tonic_endpoint = Endpoint::from_shared(endpoint.to_string()).map_err(|e| {
            JitoSubmitAttemptError::Fatal {
                endpoint: endpoint.to_string(),
                message: format!("Invalid Jito gRPC endpoint {endpoint}: {e}"),
            }
        })?;

        let tonic_endpoint = if Url::parse(endpoint)
            .ok()
            .map(|url| url.scheme().eq_ignore_ascii_case("https"))
            .unwrap_or(false)
        {
            tonic_endpoint
                .tls_config(ClientTlsConfig::new())
                .map_err(|e| JitoSubmitAttemptError::Fatal {
                    endpoint: endpoint.to_string(),
                    message: format!(
                        "Failed to configure TLS for Jito gRPC endpoint {endpoint}: {e}"
                    ),
                })?
        } else {
            tonic_endpoint
        };

        let channel = tonic_endpoint
            .tcp_nodelay(true)
            .connect_timeout(Duration::from_millis(JITO_SEND_BUNDLE_TIMEOUT_MS))
            .timeout(Duration::from_millis(JITO_SEND_BUNDLE_TIMEOUT_MS))
            .connect()
            .await
            .map_err(|e| classify_grpc_transport_message(endpoint, e.to_string()))?;

        let mut client = searcher::searcher_service_client::SearcherServiceClient::new(channel);
        let mut grpc_request = tonic::Request::new(request);
        attach_jito_auth_metadata(endpoint, self.grpc_auth.as_deref(), &mut grpc_request)?;
        if self.grpc_auth.is_some() {
            debug!(
                endpoint = endpoint,
                "Attached x-jito-auth metadata to Jito gRPC sendBundle request"
            );
        }
        let response = client
            .send_bundle(grpc_request)
            .await
            .map_err(|status| classify_grpc_status(endpoint, status))?;

        let bundle_uuid = response.into_inner().uuid;
        if bundle_uuid.is_empty() {
            return Err(JitoSubmitAttemptError::Fatal {
                endpoint: endpoint.to_string(),
                message: format!("Jito gRPC SendBundle returned empty UUID for {endpoint}"),
            });
        }

        Ok(JitoSubmitAck {
            bundle_uuid,
            submit_endpoint: endpoint.to_string(),
        })
    }

    async fn submit_bundle_across_endpoints(
        &self,
        params: &Value,
        endpoints: &[String],
    ) -> Result<JitoSubmitAck> {
        let endpoint_count = endpoints.len().max(1);
        let mut last_error: Option<JitoSubmitAttemptError> = None;

        for round in 0..MAX_BUNDLE_RETRIES {
            for endpoint in endpoints {
                match self.send_bundle_request_to_endpoint(endpoint, params).await {
                    Ok(ack) => {
                        if self.bundle_config.enable_diagnostics {
                            info!(
                                endpoint = endpoint,
                                round = round + 1,
                                transport = "grpc",
                                bundle_uuid = ack.bundle_uuid.as_str(),
                                "Jito bundle submission ACK received; awaiting status confirmation"
                            );
                        }
                        return Ok(ack);
                    }
                    Err(JitoSubmitAttemptError::RateLimited {
                        endpoint,
                        code,
                        message,
                    }) => {
                        warn!(
                            endpoint = endpoint,
                            round = round + 1,
                            transport = "grpc",
                            code = code.map(|value| value.to_string()).unwrap_or_else(|| "none".to_string()),
                            "Jito gRPC endpoint rate-limited/congested during SendBundle; rotating endpoint: {}",
                            message
                        );
                        last_error = Some(JitoSubmitAttemptError::RateLimited {
                            endpoint,
                            code,
                            message,
                        });
                    }
                    Err(JitoSubmitAttemptError::RetryableTransport { endpoint, message }) => {
                        warn!(
                            endpoint = endpoint,
                            round = round + 1,
                            transport = "grpc",
                            "Retryable Jito gRPC transport failure during SendBundle; rotating endpoint: {}",
                            message
                        );
                        last_error =
                            Some(JitoSubmitAttemptError::RetryableTransport { endpoint, message });
                    }
                    Err(JitoSubmitAttemptError::Fatal { message, .. }) => {
                        return Err(TriggerError::JitoBundleError(message));
                    }
                }
            }

            if round + 1 < MAX_BUNDLE_RETRIES {
                let backoff_ms = JITO_FAILOVER_ROUND_BACKOFF_MS * (1u64 << round);
                warn!(
                    round = round + 1,
                    endpoint_count,
                    backoff_ms,
                    "Jito failover round exhausted; backing off before retry round"
                );
                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
            }
        }

        Err(last_error
            .unwrap_or_else(|| JitoSubmitAttemptError::Fatal {
                endpoint: self.endpoint.clone(),
                message: "No Jito endpoints available for bundle submission".to_string(),
            })
            .into_trigger_error(MAX_BUNDLE_RETRIES, endpoint_count))
    }

    async fn submit_bundle_request_with_failover(&self, params: &Value) -> Result<JitoSubmitAck> {
        let endpoints = build_public_jito_failover_grpc_endpoints(&self.grpc_endpoint);
        self.submit_bundle_across_endpoints(params, &endpoints)
            .await
    }

    /// Get a random Jito tip account for MEV payment.
    ///
    /// Selects randomly from the official Jito mainnet tip distribution accounts.
    pub fn random_tip_account() -> Result<Pubkey> {
        let random_index = rand::thread_rng().gen_range(0..JITO_TIP_ACCOUNTS.len());
        let tip_account_str = JITO_TIP_ACCOUNTS[random_index];

        Pubkey::from_str(tip_account_str).map_err(|e| {
            TriggerError::JitoBundleError(format!("Invalid tip account pubkey: {}", e))
        })
    }

    // ========================================================================
    // IRONCLAD TRANSACTION PROTOCOL - TTL & Simulation
    // ========================================================================

    /// Fetch fresh blockhash with TTL enforcement (PART A: TTL Guard)
    ///
    /// Measures blockhash fetch latency and aborts if > 200ms (blockhash is stale on arrival)
    ///
    /// # Returns
    /// * `Ok(Hash)` - Fresh blockhash fetched within time limit
    /// * `Err(TriggerError::StaleBlockhash)` - Fetch took too long, blockhash is stale
    pub async fn get_fresh_blockhash(&self) -> Result<Hash> {
        let rpc = self.rpc_client.as_ref().ok_or_else(|| {
            TriggerError::ConfigError("RPC client not configured for blockhash fetch".to_string())
        })?;

        let start = Instant::now();

        let (blockhash, _) = rpc
            .get_latest_blockhash_with_commitment(CommitmentConfig::confirmed())
            .await
            .map_err(|e| TriggerError::NetworkError(format!("Failed to fetch blockhash: {}", e)))?;

        let fetch_time = start.elapsed();

        if fetch_time.as_millis() > MAX_BLOCKHASH_FETCH_MS {
            warn!(
                "⚠️ IRONCLAD ABORT: Blockhash fetch took {}ms > {}ms limit - blockhash is STALE",
                fetch_time.as_millis(),
                MAX_BLOCKHASH_FETCH_MS
            );
            return Err(TriggerError::StaleBlockhash(format!(
                "Blockhash fetch took {}ms, exceeds {}ms limit",
                fetch_time.as_millis(),
                MAX_BLOCKHASH_FETCH_MS
            )));
        }

        debug!(
            "✅ Fresh blockhash fetched in {}ms: {}",
            fetch_time.as_millis(),
            blockhash
        );

        Ok(blockhash)
    }

    /// Simulate transaction pre-flight (PART B: Simulation Pre-Flight)
    ///
    /// Simulates transaction before sending to detect honeypots, insufficient funds, and other issues.
    /// Aborts if simulation fails any validation check or takes > 100ms.
    ///
    /// # Arguments
    /// * `tx` - Transaction to simulate
    ///
    /// # Returns
    /// * `Ok(())` - Simulation passed all checks
    /// * `Err(TriggerError::SimulationFailed)` - Simulation failed validation
    /// * `Err(TriggerError::TransactionAborted)` - Simulation took too long
    pub async fn simulate_transaction_preflight(&self, tx: &VersionedTransaction) -> Result<()> {
        let rpc = self.rpc_client.as_ref().ok_or_else(|| {
            TriggerError::ConfigError("RPC client not configured for simulation".to_string())
        })?;

        let start = Instant::now();

        // Simulate with `processed` commitment so freshly-created Token-2022 mints
        // are visible (they may not yet be finalized when a BUY fires).
        let simulation_result = rpc
            .simulate_transaction_with_config(
                tx,
                RpcSimulateTransactionConfig {
                    sig_verify: false,
                    replace_recent_blockhash: true,
                    commitment: Some(CommitmentConfig::processed()),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| TriggerError::NetworkError(format!("Simulation RPC error: {}", e)))?;

        let sim_time = start.elapsed();

        // Check latency limit (100ms)
        if sim_time.as_millis() > MAX_SIMULATION_TIME_MS {
            warn!(
                "⚠️ IRONCLAD SKIP: Simulation took {}ms > {}ms limit - market moving too fast",
                sim_time.as_millis(),
                MAX_SIMULATION_TIME_MS
            );
            return Err(TriggerError::TransactionAborted(format!(
                "Simulation took {}ms, exceeds {}ms limit",
                sim_time.as_millis(),
                MAX_SIMULATION_TIME_MS
            )));
        }

        // Check for simulation error
        if let Some(err) = simulation_result.value.err {
            warn!("⚠️ IRONCLAD ABORT: Simulation failed with error: {:?}", err);
            if let Some(logs) = &simulation_result.value.logs {
                for log in logs {
                    warn!("  SIM LOG: {}", log);
                }
            }
            return Err(TriggerError::SimulationFailed(format!(
                "Transaction simulation error: {:?}",
                err
            )));
        }

        // Check logs for errors
        if let Some(logs) = &simulation_result.value.logs {
            for log in logs {
                let log_lower = log.to_lowercase();

                // First check if this is an ignored pattern (false positive)
                let is_ignored = IGNORED_ERROR_PATTERNS
                    .iter()
                    .any(|pattern| log_lower.contains(&pattern.to_lowercase()));

                if is_ignored {
                    continue;
                }

                // Check for critical error patterns
                for pattern in CRITICAL_ERROR_PATTERNS {
                    if log_lower.contains(&pattern.to_lowercase()) {
                        warn!("⚠️ IRONCLAD ABORT: Critical error in logs: {}", log);
                        return Err(TriggerError::SimulationFailed(format!(
                            "Critical error detected in logs: {}",
                            pattern
                        )));
                    }
                }
            }
        }

        // Check compute units (honeypot detection)
        // Note: This uses a fixed threshold for simple swaps. In production,
        // consider making this configurable per transaction type.
        if let Some(units_consumed) = simulation_result.value.units_consumed {
            if units_consumed > MAX_COMPUTE_UNITS_SIMPLE_SWAP {
                warn!(
                    "⚠️ IRONCLAD ABORT: CU consumption {}k > {}k - possible honeypot with loops",
                    units_consumed / 1000,
                    MAX_COMPUTE_UNITS_SIMPLE_SWAP / 1000
                );
                return Err(TriggerError::SimulationFailed(format!(
                    "Excessive compute units: {} > {} - possible honeypot",
                    units_consumed, MAX_COMPUTE_UNITS_SIMPLE_SWAP
                )));
            }

            debug!(
                "✅ Simulation passed: {}ms, {} CU",
                sim_time.as_millis(),
                units_consumed
            );
        }

        Ok(())
    }

    /// Build a Jito bundle from InitializePool TX and Ghost TXs
    ///
    /// # Arguments
    /// * `init_pool_tx` - The InitializePool transaction (must be first in bundle)
    /// * `ghost_txs` - Array of Ghost transactions to execute after pool initialization
    /// * `transaction_value` - Estimated total transaction value in lamports (for tip calculation)
    /// * `priority` - Priority factor for tip calculation (0.0 = base, 1.0 = dynamic max)
    /// * `recent_blockhash` - Recent blockhash for the bundle
    /// * `tip_payer` - Optional keypair to pay the MEV tip (if provided, a tip transaction is added to the bundle)
    ///
    /// # Returns
    /// * Constructed bundle ready for submission
    pub fn build_bundle(
        &self,
        init_pool_tx: VersionedTransaction,
        ghost_txs: Vec<VersionedTransaction>,
        transaction_value: u64,
        priority: f64,
        recent_blockhash: Hash,
        tip_payer: Option<&Keypair>,
    ) -> Result<JitoBundle> {
        // Validate inputs
        if ghost_txs.is_empty() {
            return Err(TriggerError::JitoBundleError(
                "Cannot create bundle without Ghost transactions".to_string(),
            ));
        }

        // Calculate tip
        let tip_lamports = self
            .bundle_config
            .tip_config
            .calculate_tip(transaction_value, priority);

        // Build ordered transaction list: InitializePool first, then Ghost TXs
        let mut transactions = vec![init_pool_tx];
        transactions.extend(ghost_txs);

        // Add tip transaction if payer is provided (Point 4: Paying MEV tips to validators)
        if let Some(payer) = tip_payer {
            if tip_lamports > 0 {
                let tip_tx = self.create_tip_transaction(payer, tip_lamports, recent_blockhash)?;
                transactions.push(tip_tx);

                if self.bundle_config.enable_diagnostics {
                    info!(
                        "Added tip transaction to bundle: {} lamports to random Jito validator",
                        tip_lamports
                    );
                }
            }
        }

        // Get bundle ID from first transaction
        let bundle_id = transactions[0].signatures[0];

        if self.bundle_config.enable_diagnostics {
            info!(
                "Built Jito bundle: id={}, tx_count={}, tip={} lamports ({:.2}%)",
                bundle_id,
                transactions.len(),
                tip_lamports,
                (tip_lamports as f64 / transaction_value as f64) * 100.0
            );
        }

        Ok(JitoBundle {
            transactions,
            tip_lamports,
            bundle_id,
            recent_blockhash,
        })
    }

    /// Build a Jito bundle with TipGuard++ protection
    ///
    /// This method applies TipGuard safety limits to the tip calculation before
    /// building the bundle. It prevents excessive tip amounts during gas wars.
    ///
    /// # Arguments
    /// * `init_pool_tx` - InitializePool transaction (from detected event)
    /// * `ghost_txs` - Ghost buy transactions (1 or more)
    /// * `transaction_value` - Estimated value of the transaction in lamports
    /// * `priority` - Priority factor (0.0 = base, 1.0 = dynamic max)
    /// * `recent_blockhash` - Recent blockhash for tip transaction
    /// * `tip_payer` - Optional keypair to pay the tip
    /// * `tip_guard_config` - TipGuard configuration for safety limits
    ///
    /// # Returns
    /// * `JitoBundle` with safe tip amount
    ///
    /// # Safety Limits
    /// 1. Absolute cap: Never exceed configured max tip (default: 0.04 SOL)
    /// 2. Ratio cap: Never exceed percentage of trade value (default: 40%)
    /// 3. Fallback: Use conservative default on calculation errors
    ///
    /// # Example
    /// ```ignore
    /// use trigger::safety::TipGuardConfig;
    ///
    /// let tip_config = TipGuardConfig::default();
    /// let bundle = client.build_bundle_with_tip_guard(
    ///     init_tx,
    ///     ghost_txs,
    ///     transaction_value,
    ///     0.8, // High priority
    ///     recent_blockhash,
    ///     Some(&payer),
    ///     &tip_config,
    /// )?;
    /// ```
    pub fn build_bundle_with_tip_guard(
        &self,
        init_pool_tx: VersionedTransaction,
        ghost_txs: Vec<VersionedTransaction>,
        transaction_value: u64,
        priority: f64,
        recent_blockhash: Hash,
        tip_payer: Option<&Keypair>,
        tip_guard_config: &crate::safety::TipGuardConfig,
    ) -> Result<JitoBundle> {
        use crate::safety::calculate_safe_tip;

        // Validate inputs
        if ghost_txs.is_empty() {
            return Err(TriggerError::JitoBundleError(
                "Cannot create bundle without Ghost transactions".to_string(),
            ));
        }

        // Calculate raw tip using existing logic
        let raw_tip_lamports = self
            .bundle_config
            .tip_config
            .calculate_tip(transaction_value, priority);

        // Convert to SOL for safety calculations
        let raw_tip_sol = raw_tip_lamports as f64 / 1_000_000_000.0;
        let trade_value_sol = transaction_value as f64 / 1_000_000_000.0;

        // TIPGUARD: Apply safety limits
        let safe_tip_sol = calculate_safe_tip(raw_tip_sol, trade_value_sol, tip_guard_config);
        let safe_tip_lamports = (safe_tip_sol * 1_000_000_000.0) as u64;

        // Log if tip was reduced
        if safe_tip_lamports < raw_tip_lamports {
            warn!(
                "🛡️ TIPGUARD: Reduced tip from {} SOL ({} lamports) to {} SOL ({} lamports) - Protecting capital!",
                raw_tip_sol, raw_tip_lamports, safe_tip_sol, safe_tip_lamports
            );
        } else {
            info!(
                "🛡️ TIPGUARD: Tip {} SOL ({} lamports) within safe limits",
                safe_tip_sol, safe_tip_lamports
            );
        }

        // Build ordered transaction list: InitializePool first, then Ghost TXs
        let mut transactions = vec![init_pool_tx];
        transactions.extend(ghost_txs);

        // Add tip transaction if payer is provided
        if let Some(payer) = tip_payer {
            if safe_tip_lamports > 0 {
                let tip_tx =
                    self.create_tip_transaction(payer, safe_tip_lamports, recent_blockhash)?;
                transactions.push(tip_tx);

                if self.bundle_config.enable_diagnostics {
                    info!(
                        "Added safe tip transaction to bundle: {} lamports to random Jito validator",
                        safe_tip_lamports
                    );
                }
            }
        }

        // Get bundle ID from first transaction
        let bundle_id = transactions[0].signatures[0];

        if self.bundle_config.enable_diagnostics {
            info!(
                "Built Jito bundle with TipGuard: id={}, tx_count={}, tip={} lamports ({:.2}%)",
                bundle_id,
                transactions.len(),
                safe_tip_lamports,
                (safe_tip_lamports as f64 / transaction_value as f64) * 100.0
            );
        }

        Ok(JitoBundle {
            transactions,
            tip_lamports: safe_tip_lamports,
            bundle_id,
            recent_blockhash,
        })
    }

    /// Create a tip payment transaction for Jito validators
    ///
    /// # Arguments
    /// * `payer` - The keypair that will pay the tip
    /// * `tip_lamports` - Amount to tip in lamports
    /// * `recent_blockhash` - Recent blockhash for the transaction
    ///
    /// # Returns
    /// * Signed versioned transaction with tip payment
    pub fn create_tip_transaction(
        &self,
        payer: &Keypair,
        tip_lamports: u64,
        recent_blockhash: Hash,
    ) -> Result<VersionedTransaction> {
        // Get random Jito tip account
        let tip_account = Self::random_tip_account()?;

        if self.bundle_config.enable_diagnostics {
            debug!(
                "Creating tip transaction: {} lamports to {}",
                tip_lamports, tip_account
            );
        }

        // Create system transfer instruction
        let tip_instruction =
            system_instruction::transfer(&payer.pubkey(), &tip_account, tip_lamports);

        // Build versioned message
        let message =
            v0::Message::try_compile(&payer.pubkey(), &[tip_instruction], &[], recent_blockhash)
                .map_err(|e| {
                    TriggerError::TransactionBuildFailed(format!(
                        "Failed to compile tip transaction message: {}",
                        e
                    ))
                })?;

        // Create and sign transaction
        let versioned_tx = VersionedTransaction::try_new(VersionedMessage::V0(message), &[payer])
            .map_err(|e| {
            TriggerError::TransactionBuildFailed(format!("Failed to create tip transaction: {}", e))
        })?;

        Ok(versioned_tx)
    }

    /// Build bundle diagnostics for logging and analysis
    ///
    /// # Arguments
    /// * `bundle` - The bundle to analyze
    /// * `transaction_value` - Transaction value used for tip calculation
    /// * `priority` - Priority factor used
    ///
    /// # Returns
    /// * Detailed diagnostics information
    pub fn create_diagnostics(
        &self,
        bundle: &JitoBundle,
        transaction_value: u64,
        priority: f64,
    ) -> BundleDiagnostics {
        let tip_percent = if transaction_value > 0 {
            (bundle.tip_lamports as f64 / transaction_value as f64) * 100.0
        } else {
            0.0
        };

        let explanation = format!(
            "Bundle {} contains {} transaction(s). Tip: {} lamports ({:.2}% of {} lamports value). \
             Priority factor: {:.2}. Redundancy: N+{} (will submit {} bundles). \
             Nonce staggering: {}. Tip range: {:.1}%-{:.1}% (capped at {:.1}%).",
            bundle.bundle_id,
            bundle.transactions.len(),
            bundle.tip_lamports,
            tip_percent,
            transaction_value,
            priority,
            self.bundle_config.redundancy_policy.bundle_count() - 1,
            self.bundle_config.redundancy_policy.bundle_count(),
            if self.bundle_config.stagger_nonce { "enabled" } else { "disabled" },
            self.bundle_config.tip_config.base_tip_percent * 100.0,
            self.bundle_config.tip_config.dynamic_tip_percent * 100.0,
            self.bundle_config.tip_config.max_tip_percent * 100.0,
        );

        BundleDiagnostics {
            bundle_id: bundle.bundle_id,
            transaction_count: bundle.transactions.len(),
            tip_lamports: bundle.tip_lamports,
            tip_percent,
            priority_factor: priority,
            redundancy_count: self.bundle_config.redundancy_policy.bundle_count(),
            nonce_staggered: self.bundle_config.stagger_nonce,
            explanation,
        }
    }

    /// Log bundle diagnostics
    ///
    /// # Arguments
    /// * `diagnostics` - The diagnostics to log
    pub fn log_diagnostics(&self, diagnostics: &BundleDiagnostics) {
        if self.bundle_config.enable_diagnostics {
            info!("=== Bundle Diagnostics ===");
            info!("  Bundle ID: {}", diagnostics.bundle_id);
            info!("  Transaction Count: {}", diagnostics.transaction_count);
            info!(
                "  Tip: {} lamports ({:.2}%)",
                diagnostics.tip_lamports, diagnostics.tip_percent
            );
            info!("  Priority Factor: {:.2}", diagnostics.priority_factor);
            info!(
                "  Redundancy: N+{} ({} bundles)",
                diagnostics.redundancy_count - 1,
                diagnostics.redundancy_count
            );
            info!("  Nonce Staggered: {}", diagnostics.nonce_staggered);
            info!("  Explanation: {}", diagnostics.explanation);
            info!("=========================");
        }
    }

    /// Submit a bundle with redundancy
    ///
    /// # Arguments
    /// * `bundle` - The bundle to submit
    ///
    /// # Returns
    /// * Bundle ID (signature of first transaction)
    pub async fn submit_bundle_with_redundancy(&self, bundle: JitoBundle) -> Result<Signature> {
        let redundancy_count = self.bundle_config.redundancy_policy.bundle_count();

        info!(
            "Submitting bundle {} with redundancy N+{} ({} submissions)",
            bundle.bundle_id,
            redundancy_count - 1,
            redundancy_count
        );

        if self.dry_run {
            info!(
                "DRY-RUN: Would submit bundle {} with {} transactions",
                bundle.bundle_id,
                bundle.transactions.len()
            );
            return Ok(bundle.bundle_id);
        }

        // Track submission metrics
        let submission_start = Instant::now();
        let mut submit_acks = Vec::new();
        let mut last_error = None;

        // Submit bundle multiple times based on redundancy policy
        for i in 0..redundancy_count {
            debug!("Bundle submission {}/{}", i + 1, redundancy_count);

            // Measure latency for this submission
            let submit_start = Instant::now();

            match self.submit_single_bundle(&bundle).await {
                Ok(ack) => {
                    let latency = submit_start.elapsed();
                    info!(
                        "Bundle {} submission {}/{} acknowledged by Jito submit transport (UUID: {}, endpoint: {}, latency: {:.2}ms)",
                        bundle.bundle_id,
                        i + 1,
                        redundancy_count,
                        ack.bundle_uuid,
                        ack.submit_endpoint,
                        latency.as_secs_f64() * 1000.0
                    );
                    submit_acks.push(ack);

                    // Log if latency exceeds target
                    if latency.as_millis() > 5 {
                        warn!(
                            "Bundle submission latency {:.2}ms exceeds 5ms target",
                            latency.as_secs_f64() * 1000.0
                        );
                    }
                }
                Err(e) => {
                    let latency = submit_start.elapsed();
                    error!(
                        "Bundle {} submission {}/{} failed: {} (latency: {:.2}ms)",
                        bundle.bundle_id,
                        i + 1,
                        redundancy_count,
                        e,
                        latency.as_secs_f64() * 1000.0
                    );
                    last_error = Some(e);
                }
            }

            // Optional: stagger submissions slightly if configured
            if self.bundle_config.stagger_nonce && i < redundancy_count - 1 {
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }
        }

        let total_latency = submission_start.elapsed();
        info!(
            "Bundle {} submit ACKs across gRPC transport: attempts={}, acknowledgements={}, total latency: {:.2}ms, avg: {:.2}ms)",
            bundle.bundle_id,
            redundancy_count,
            submit_acks.len(),
            total_latency.as_secs_f64() * 1000.0,
            (total_latency.as_secs_f64() * 1000.0) / redundancy_count as f64
        );

        if submit_acks.is_empty() {
            return Err(last_error.unwrap_or_else(|| {
                TriggerError::JitoBundleError("All bundle submissions failed".to_string())
            }));
        }

        info!(
            "Bundle {} received at least one Jito submit ACK; acceptance is not checked in submit_bundle_with_redundancy()",
            bundle.bundle_id
        );
        Ok(bundle.bundle_id)
    }

    /// Submit a bundle with redundancy and return both signature and bundle UUID.
    pub async fn submit_bundle_with_redundancy_receipt(
        &self,
        bundle: JitoBundle,
    ) -> Result<JitoBundleSubmission> {
        let redundancy_count = self.bundle_config.redundancy_policy.bundle_count();
        let signatures = Self::bundle_transaction_signatures(&bundle.transactions);

        info!(
            "Submitting bundle {} with redundancy N+{} ({} submissions)",
            bundle.bundle_id,
            redundancy_count - 1,
            redundancy_count
        );

        if self.dry_run {
            info!(
                "DRY-RUN: Would submit bundle {} with {} transactions",
                bundle.bundle_id,
                bundle.transactions.len()
            );
            return Ok(JitoBundleSubmission {
                signature: bundle.bundle_id,
                signatures,
                bundle_uuid: format!("dry-run-{}", bundle.bundle_id),
                submit_endpoint: self.grpc_endpoint.clone(),
            });
        }

        let submission_start = Instant::now();
        let mut first_success: Option<JitoSubmitAck> = None;
        let mut success_count = 0usize;
        let mut last_error = None;

        for i in 0..redundancy_count {
            debug!("Bundle submission {}/{}", i + 1, redundancy_count);

            let submit_start = Instant::now();

            match self.submit_single_bundle(&bundle).await {
                Ok(ack) => {
                    let latency = submit_start.elapsed();
                    info!(
                        "Bundle {} submission {}/{} successful (UUID: {}, latency: {:.2}ms)",
                        bundle.bundle_id,
                        i + 1,
                        redundancy_count,
                        ack.bundle_uuid,
                        latency.as_secs_f64() * 1000.0
                    );

                    if first_success.is_none() {
                        first_success = Some(ack);
                    }
                    success_count += 1;

                    if latency.as_millis() > 5 {
                        warn!(
                            "Bundle submission latency {:.2}ms exceeds 5ms target",
                            latency.as_secs_f64() * 1000.0
                        );
                    }
                }
                Err(e) => {
                    let latency = submit_start.elapsed();
                    error!(
                        "Bundle {} submission {}/{} failed: {} (latency: {:.2}ms)",
                        bundle.bundle_id,
                        i + 1,
                        redundancy_count,
                        e,
                        latency.as_secs_f64() * 1000.0
                    );
                    last_error = Some(e);
                }
            }

            if self.bundle_config.stagger_nonce && i < redundancy_count - 1 {
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }
        }

        let total_latency = submission_start.elapsed();
        info!(
            "Bundle {} submit ACKs across gRPC transport: attempts={}, acknowledgements={}, total latency: {:.2}ms, avg: {:.2}ms)",
            bundle.bundle_id,
            redundancy_count,
            success_count,
            total_latency.as_secs_f64() * 1000.0,
            (total_latency.as_secs_f64() * 1000.0) / redundancy_count as f64
        );

        match first_success {
            Some(ack) => Ok(JitoBundleSubmission {
                signature: bundle.bundle_id,
                signatures,
                bundle_uuid: ack.bundle_uuid,
                submit_endpoint: ack.submit_endpoint,
            }),
            None => Err(last_error.unwrap_or_else(|| {
                TriggerError::JitoBundleError("All bundle submissions failed".to_string())
            })),
        }
    }

    /// Submit bundle with IRONCLAD protocol (TTL + Simulation)
    ///
    /// This method enforces the IRONCLAD TRANSACTION PROTOCOL:
    /// 1. Simulates all transactions in the bundle before submission
    /// 2. Enforces TTL timeout of 1500ms for bundle confirmation
    /// 3. No retries for transactions within bundle (max retries = 0)
    /// 4. Max 2 retries for bundle submission (network errors only)
    ///
    /// # Arguments
    /// * `bundle` - The bundle to submit with IRONCLAD protocol
    ///
    /// # Returns
    /// * `Ok(Signature)` - Bundle received at least one submit ACK
    /// * `Err(TriggerError::SimulationFailed)` - Simulation failed validation
    /// * `Err(TriggerError::TtlViolation)` - Bundle confirmation timeout exceeded
    pub async fn submit_bundle_ironclad(&self, bundle: JitoBundle) -> Result<Signature> {
        info!(
            "🛡️ IRONCLAD: Submitting bundle {} (TTL: {}ms, Simulation: ON)",
            bundle.bundle_id, BUNDLE_CONFIRMATION_TIMEOUT_MS
        );

        // PART B: Pre-flight simulation for each transaction
        if self.rpc_client.is_some() {
            debug!("🛡️ IRONCLAD: Running pre-flight simulation checks");

            for (idx, tx) in bundle.transactions.iter().enumerate() {
                debug!(
                    "Simulating transaction {}/{}",
                    idx + 1,
                    bundle.transactions.len()
                );

                // Simulate transaction - will abort if honeypot or other issues detected
                self.simulate_transaction_preflight(tx).await?;
            }

            info!("✅ IRONCLAD: All transactions passed simulation checks");
        } else {
            warn!("⚠️ IRONCLAD: Simulation skipped (RPC client not configured)");
        }

        // PART A: TTL enforcement - wrap submission in timeout
        let timeout_duration = Duration::from_millis(BUNDLE_CONFIRMATION_TIMEOUT_MS);

        let submission_start = Instant::now();

        let result =
            tokio::time::timeout(timeout_duration, self.submit_bundle_with_redundancy(bundle))
                .await;

        let submission_time = submission_start.elapsed();

        match result {
            Ok(Ok(signature)) => {
                info!(
                    "✅ IRONCLAD SUCCESS: Bundle {} submitted in {}ms",
                    signature,
                    submission_time.as_millis()
                );
                Ok(signature)
            }
            Ok(Err(e)) => {
                error!("❌ IRONCLAD FAILED: Bundle submission error: {}", e);
                Err(e)
            }
            Err(_timeout_err) => {
                warn!(
                    "⚠️ IRONCLAD TTL VIOLATION: Bundle submission exceeded {}ms timeout",
                    BUNDLE_CONFIRMATION_TIMEOUT_MS
                );
                Err(TriggerError::TtlViolation(format!(
                    "Bundle submission timeout: exceeded {}ms TTL",
                    BUNDLE_CONFIRMATION_TIMEOUT_MS
                )))
            }
        }
    }

    /// Submit a single bundle to Jito
    ///
    /// # Arguments
    /// * `bundle` - The bundle to submit
    ///
    /// # Returns
    /// * Bundle UUID from Jito
    async fn submit_single_bundle(&self, bundle: &JitoBundle) -> Result<JitoSubmitAck> {
        // Serialize transactions to base64
        let serialized_txs: Vec<String> = bundle
            .transactions
            .iter()
            .map(|tx| {
                bincode::serialize(tx)
                    .map(|bytes| general_purpose::STANDARD.encode(bytes))
                    .map_err(|e| {
                        TriggerError::SerializationError(format!(
                            "Failed to serialize transaction: {}",
                            e
                        ))
                    })
            })
            .collect::<Result<Vec<_>>>()?;

        // Prepare bundle for submission
        let transactions_json = json!(serialized_txs);
        let params = json!([
            transactions_json,
            {
                "encoding": "base64"
            }
        ]);

        if self.bundle_config.enable_diagnostics {
            debug!(
                "Submitting bundle to Jito gRPC endpoint: {}",
                self.grpc_endpoint
            );
            debug!("Jito JSON status base URL: {}", self.sdk_base_url);
            debug!("Bundle contains {} transactions", bundle.transactions.len());
        }

        self.submit_bundle_request_with_failover(&params).await
    }

    /// Submit a bundle of transactions
    ///
    /// # Arguments
    /// * `transactions` - Vector of transactions to bundle together
    ///
    /// # Returns
    /// * Bundle ID or first transaction signature
    pub async fn submit_bundle(
        &self,
        transactions: Vec<VersionedTransaction>,
    ) -> Result<Signature> {
        info!(
            "Submitting bundle with {} transactions to Jito via gRPC submit transport",
            transactions.len()
        );
        debug!("Jito gRPC endpoint: {}", self.grpc_endpoint);
        debug!("Jito JSON status endpoint: {}", self.endpoint);

        if transactions.is_empty() {
            return Err(TriggerError::JitoBundleError(
                "Cannot submit empty bundle".to_string(),
            ));
        }

        let bundle_signature = transactions[0].signatures[0];

        if self.dry_run {
            info!(
                "DRY-RUN: Would submit bundle with signature: {}",
                bundle_signature
            );
            return Ok(bundle_signature);
        }

        // Serialize transactions to base64
        let serialized_txs: Vec<String> = transactions
            .iter()
            .map(|tx| {
                bincode::serialize(tx)
                    .map(|bytes| general_purpose::STANDARD.encode(bytes))
                    .map_err(|e| {
                        TriggerError::SerializationError(format!(
                            "Failed to serialize transaction: {}",
                            e
                        ))
                    })
            })
            .collect::<Result<Vec<_>>>()?;

        // Prepare bundle for submission
        let transactions_json = json!(serialized_txs);
        let params = json!([
            transactions_json,
            {
                "encoding": "base64"
            }
        ]);

        let ack = self.submit_bundle_request_with_failover(&params).await?;

        info!(
            "Jito bundle submit ACK received (transport=gRPC, signature={}, bundle_uuid={}, endpoint={}); submit_bundle() does not wait for acceptance",
            bundle_signature, ack.bundle_uuid, ack.submit_endpoint
        );

        Ok(bundle_signature)
    }

    /// Submit a bundle of transactions and block until Jito confirms landing.
    pub async fn submit_bundle_and_confirm(
        &self,
        transactions: Vec<VersionedTransaction>,
    ) -> Result<JitoConfirmedBundle> {
        info!(
            "Submitting bundle with {} transactions to Jito via gRPC and waiting for status confirmation",
            transactions.len()
        );
        debug!("Jito gRPC endpoint: {}", self.grpc_endpoint);
        debug!("Jito JSON status endpoint: {}", self.endpoint);

        if transactions.is_empty() {
            return Err(TriggerError::JitoBundleError(
                "Cannot submit empty bundle".to_string(),
            ));
        }

        let signatures = Self::bundle_transaction_signatures(&transactions);
        let signature = signatures[0];

        if self.dry_run {
            info!("DRY-RUN: Would submit bundle with signature: {}", signature);
            return Ok(JitoConfirmedBundle {
                signature,
                signatures: signatures.clone(),
                bundle_uuid: format!("dry-run-{}", signature),
                landed_slot: None,
            });
        }

        let serialized_txs: Vec<String> = transactions
            .iter()
            .map(|tx| {
                bincode::serialize(tx)
                    .map(|bytes| general_purpose::STANDARD.encode(bytes))
                    .map_err(|e| {
                        TriggerError::SerializationError(format!(
                            "Failed to serialize transaction: {}",
                            e
                        ))
                    })
            })
            .collect::<Result<Vec<_>>>()?;

        let params = json!([
            serialized_txs,
            {
                "encoding": "base64"
            }
        ]);

        let ack = self.submit_bundle_request_with_failover(&params).await?;

        info!(
            "Jito bundle submit ACK received (transport=gRPC, signature={}, bundle_uuid={}, endpoint={}); polling for acceptance",
            signature, ack.bundle_uuid, ack.submit_endpoint
        );

        let status = self
            .wait_for_bundle_status_with_timeout(
                &ack.bundle_uuid,
                &ack.submit_endpoint,
                Duration::from_millis(BUNDLE_CONFIRMATION_TIMEOUT_MS),
                Duration::from_millis(200),
            )
            .await?;

        match status.status {
            BundleState::Accepted => {
                info!(
                    "Jito bundle accepted by status poll: signature={} bundle_uuid={} landed_slot={:?}",
                    signature, ack.bundle_uuid, status.landed_slot
                );
                Ok(JitoConfirmedBundle {
                    signature,
                    signatures: signatures.clone(),
                    bundle_uuid: ack.bundle_uuid.clone(),
                    landed_slot: status.landed_slot,
                })
            }
            BundleState::Rejected | BundleState::Expired => {
                match self
                    .reconcile_bundle_with_chain(
                        &ack.bundle_uuid,
                        status.status,
                        &signatures,
                        Duration::from_millis(FAIL_CLOSED_ONCHAIN_RECONCILIATION_TIMEOUT_MS),
                    )
                    .await?
                {
                    OnchainBundleReconciliation::Confirmed(landed_slot) => {
                        return Ok(JitoConfirmedBundle {
                            signature,
                            signatures: signatures.clone(),
                            bundle_uuid: ack.bundle_uuid.clone(),
                            landed_slot,
                        });
                    }
                    OnchainBundleReconciliation::Failed => {
                        warn!(
                            "Jito bundle not accepted after submit ACK and on-chain reconciliation observed a definitive bundle failure: signature={} bundle_uuid={} bundle_id={} status={:?} reason={:?}",
                            signature, ack.bundle_uuid, status.bundle_id, status.status, status.reason
                        );
                        Err(TriggerError::JitoBundleError(format!(
                            "Bundle {} was not accepted by Jito ({})",
                            ack.bundle_uuid,
                            Self::format_bundle_status_summary(&status)
                        )))
                    }
                    OnchainBundleReconciliation::Uncertain => {
                        warn!(
                            "Jito bundle landing remained uncertain after fail-closed on-chain reconciliation: signature={} bundle_uuid={} bundle_id={} status={:?} reason={:?} timeout_ms={}",
                            signature,
                            ack.bundle_uuid,
                            status.bundle_id,
                            status.status,
                            status.reason,
                            FAIL_CLOSED_ONCHAIN_RECONCILIATION_TIMEOUT_MS
                        );
                        Err(TriggerError::UncertainBundleLanding(format!(
                            "Bundle {} remained uncertain after {:?} and {}ms of on-chain reconciliation",
                            ack.bundle_uuid,
                            status.status,
                            FAIL_CLOSED_ONCHAIN_RECONCILIATION_TIMEOUT_MS
                        )))
                    }
                }
            }
            BundleState::Pending => {
                match self
                    .reconcile_bundle_with_chain(
                        &ack.bundle_uuid,
                        status.status,
                        &signatures,
                        Duration::from_millis(FAIL_CLOSED_ONCHAIN_RECONCILIATION_TIMEOUT_MS),
                    )
                    .await?
                {
                    OnchainBundleReconciliation::Confirmed(landed_slot) => {
                        warn!(
                            signature = %signature,
                            bundle_uuid = %ack.bundle_uuid,
                            ?landed_slot,
                            "Jito bundle status was Pending at poll timeout but on-chain reconciliation found all signatures — treating as confirmed"
                        );
                        return Ok(JitoConfirmedBundle {
                            signature,
                            signatures: signatures.clone(),
                            bundle_uuid: ack.bundle_uuid.clone(),
                            landed_slot,
                        });
                    }
                    OnchainBundleReconciliation::Failed => {
                        warn!(
                            "Jito bundle remained pending after confirmation timeout and on-chain reconciliation observed a definitive bundle failure: signature={} bundle_uuid={} timeout_ms={}",
                            signature, ack.bundle_uuid, BUNDLE_CONFIRMATION_TIMEOUT_MS
                        );
                        Err(TriggerError::JitoBundleError(format!(
                            "Bundle {} remained pending after {}ms",
                            ack.bundle_uuid, BUNDLE_CONFIRMATION_TIMEOUT_MS
                        )))
                    }
                    OnchainBundleReconciliation::Uncertain => {
                        warn!(
                            "Jito bundle remained pending and landing stayed uncertain after fail-closed on-chain reconciliation: signature={} bundle_uuid={} timeout_ms={} reconcile_timeout_ms={}",
                            signature,
                            ack.bundle_uuid,
                            BUNDLE_CONFIRMATION_TIMEOUT_MS,
                            FAIL_CLOSED_ONCHAIN_RECONCILIATION_TIMEOUT_MS
                        );
                        Err(TriggerError::UncertainBundleLanding(format!(
                            "Bundle {} remained pending after {}ms and uncertain after {}ms of on-chain reconciliation",
                            ack.bundle_uuid,
                            BUNDLE_CONFIRMATION_TIMEOUT_MS,
                            FAIL_CLOSED_ONCHAIN_RECONCILIATION_TIMEOUT_MS
                        )))
                    }
                }
            }
        }
    }

    pub async fn submit_bundle_and_confirm_with_balance(
        &self,
        transactions: Vec<VersionedTransaction>,
        owner: &Pubkey,
        mint: &Pubkey,
        baseline_balance: u64,
        direction: BalanceDeltaDirection,
    ) -> Result<JitoConfirmedBundle> {
        if transactions.is_empty() {
            return Err(TriggerError::JitoBundleError(
                "Cannot submit empty bundle".to_string(),
            ));
        }

        let signatures = Self::bundle_transaction_signatures(&transactions);
        let primary_signature = signatures[0];
        let serialized_txs: Vec<String> = transactions
            .iter()
            .map(|tx| {
                bincode::serialize(tx)
                    .map(|bytes| general_purpose::STANDARD.encode(bytes))
                    .map_err(|e| {
                        TriggerError::SerializationError(format!(
                            "Failed to serialize transaction: {}",
                            e
                        ))
                    })
            })
            .collect::<Result<Vec<_>>>()?;
        let params = json!([serialized_txs, { "encoding": "base64" }]);
        let ack = self.submit_bundle_request_with_failover(&params).await?;
        self.confirm_bundle_submission_with_balance(
            JitoBundleSubmission {
                signature: primary_signature,
                signatures,
                bundle_uuid: ack.bundle_uuid,
                submit_endpoint: ack.submit_endpoint,
            },
            owner,
            mint,
            baseline_balance,
            direction,
        )
        .await
    }

    /// Submit a single transaction as a bundle.
    ///
    /// Returns a submission receipt containing both the signature and bundle UUID.
    pub async fn submit_single_transaction(
        &self,
        transaction: VersionedTransaction,
    ) -> Result<JitoBundleSubmission> {
        let signature = transaction.signatures[0];

        if self.dry_run {
            info!("DRY-RUN: Would submit bundle with signature: {}", signature);
            return Ok(JitoBundleSubmission {
                signature,
                signatures: vec![signature],
                bundle_uuid: format!("dry-run-{}", signature),
                submit_endpoint: self.grpc_endpoint.clone(),
            });
        }

        let serialized_txs: Vec<String> = vec![transaction]
            .iter()
            .map(|tx| {
                bincode::serialize(tx)
                    .map(|bytes| general_purpose::STANDARD.encode(bytes))
                    .map_err(|e| {
                        TriggerError::SerializationError(format!(
                            "Failed to serialize transaction: {}",
                            e
                        ))
                    })
            })
            .collect::<Result<Vec<_>>>()?;

        let params = json!([
            serialized_txs,
            {
                "encoding": "base64"
            }
        ]);

        let ack = self.submit_bundle_request_with_failover(&params).await?;

        info!(
            "Jito bundle submit ACK received (transport=gRPC, signature={}, bundle_uuid={}, endpoint={}); polling for acceptance",
            signature, ack.bundle_uuid, ack.submit_endpoint
        );

        Ok(JitoBundleSubmission {
            signature,
            signatures: vec![signature],
            bundle_uuid: ack.bundle_uuid,
            submit_endpoint: ack.submit_endpoint,
        })
    }

    /// Confirm a previously submitted bundle using every submitted transaction signature.
    ///
    /// This is split out so callers can record an explicit submitted state before
    /// blocking on Jito acceptance / on-chain reconciliation.
    pub async fn confirm_bundle_submission(
        &self,
        submission: JitoBundleSubmission,
    ) -> Result<JitoConfirmedBundle> {
        let JitoBundleSubmission {
            signature,
            signatures,
            bundle_uuid,
            submit_endpoint,
        } = submission;

        if signatures.is_empty() {
            return Err(TriggerError::JitoBundleError(
                "Bundle submission receipt did not contain any transaction signatures".to_string(),
            ));
        }

        let primary_signature = signatures.first().copied().unwrap_or(signature);

        if self.dry_run {
            return Ok(JitoConfirmedBundle {
                signature: primary_signature,
                signatures,
                bundle_uuid,
                landed_slot: None,
            });
        }

        let status = self
            .wait_for_bundle_status_with_timeout(
                &bundle_uuid,
                &submit_endpoint,
                Duration::from_millis(BUNDLE_CONFIRMATION_TIMEOUT_MS),
                Duration::from_millis(200),
            )
            .await?;

        match status.status {
            BundleState::Accepted => {
                info!(
                    signature = %primary_signature,
                    bundle_uuid = %bundle_uuid,
                    bundle_id = %status.bundle_id,
                    landed_slot = ?status.landed_slot,
                    tracked_signatures = ?signatures,
                    "Jito bundle accepted by status poll"
                );
                Ok(JitoConfirmedBundle {
                    signature: primary_signature,
                    signatures: signatures.clone(),
                    bundle_uuid: bundle_uuid.clone(),
                    landed_slot: status.landed_slot,
                })
            }
            BundleState::Rejected | BundleState::Expired => {
                match self
                    .reconcile_bundle_with_chain(
                        &bundle_uuid,
                        status.status,
                        &signatures,
                        Duration::from_millis(FAIL_CLOSED_ONCHAIN_RECONCILIATION_TIMEOUT_MS),
                    )
                    .await?
                {
                    OnchainBundleReconciliation::Confirmed(landed_slot) => {
                        return Ok(JitoConfirmedBundle {
                            signature: primary_signature,
                            signatures: signatures.clone(),
                            bundle_uuid: bundle_uuid.clone(),
                            landed_slot,
                        });
                    }
                    OnchainBundleReconciliation::Failed => {
                        warn!(
                            signature = %primary_signature,
                            bundle_uuid = %bundle_uuid,
                            bundle_id = %status.bundle_id,
                            status = ?status.status,
                            reason = ?status.reason,
                            tracked_signatures = ?signatures,
                            "Jito bundle not accepted after submit ACK and on-chain reconciliation observed a definitive bundle failure"
                        );
                        Err(TriggerError::JitoBundleError(format!(
                            "Bundle {} was not accepted by Jito ({})",
                            bundle_uuid,
                            Self::format_bundle_status_summary(&status)
                        )))
                    }
                    OnchainBundleReconciliation::Uncertain => {
                        warn!(
                            signature = %primary_signature,
                            bundle_uuid = %bundle_uuid,
                            bundle_id = %status.bundle_id,
                            status = ?status.status,
                            reason = ?status.reason,
                            timeout_ms = FAIL_CLOSED_ONCHAIN_RECONCILIATION_TIMEOUT_MS,
                            tracked_signatures = ?signatures,
                            "Jito bundle landing remained uncertain after fail-closed on-chain reconciliation"
                        );
                        Err(TriggerError::UncertainBundleLanding(format!(
                            "Bundle {} remained uncertain after {:?} and {}ms of on-chain reconciliation",
                            bundle_uuid,
                            status.status,
                            FAIL_CLOSED_ONCHAIN_RECONCILIATION_TIMEOUT_MS
                        )))
                    }
                }
            }
            BundleState::Pending => {
                match self
                    .reconcile_bundle_with_chain(
                        &bundle_uuid,
                        status.status,
                        &signatures,
                        Duration::from_millis(FAIL_CLOSED_ONCHAIN_RECONCILIATION_TIMEOUT_MS),
                    )
                    .await?
                {
                    OnchainBundleReconciliation::Confirmed(landed_slot) => {
                        return Ok(JitoConfirmedBundle {
                            signature: primary_signature,
                            signatures: signatures.clone(),
                            bundle_uuid: bundle_uuid.clone(),
                            landed_slot,
                        });
                    }
                    OnchainBundleReconciliation::Failed => {
                        warn!(
                            signature = %primary_signature,
                            bundle_uuid = %bundle_uuid,
                            bundle_id = %status.bundle_id,
                            reason = ?status.reason,
                            timeout_ms = BUNDLE_CONFIRMATION_TIMEOUT_MS,
                            tracked_signatures = ?signatures,
                            "Jito bundle remained pending after confirmation timeout and on-chain reconciliation observed a definitive bundle failure"
                        );
                        Err(TriggerError::JitoBundleError(format!(
                            "Bundle {} remained pending after {}ms",
                            bundle_uuid, BUNDLE_CONFIRMATION_TIMEOUT_MS
                        )))
                    }
                    OnchainBundleReconciliation::Uncertain => {
                        warn!(
                            signature = %primary_signature,
                            bundle_uuid = %bundle_uuid,
                            bundle_id = %status.bundle_id,
                            reason = ?status.reason,
                            timeout_ms = BUNDLE_CONFIRMATION_TIMEOUT_MS,
                            reconcile_timeout_ms = FAIL_CLOSED_ONCHAIN_RECONCILIATION_TIMEOUT_MS,
                            tracked_signatures = ?signatures,
                            "Jito bundle remained pending and landing stayed uncertain after fail-closed on-chain reconciliation"
                        );
                        Err(TriggerError::UncertainBundleLanding(format!(
                            "Bundle {} remained pending after {}ms and uncertain after {}ms of on-chain reconciliation",
                            bundle_uuid,
                            BUNDLE_CONFIRMATION_TIMEOUT_MS,
                            FAIL_CLOSED_ONCHAIN_RECONCILIATION_TIMEOUT_MS
                        )))
                    }
                }
            }
        }
    }

    pub async fn confirm_bundle_submission_with_balance(
        &self,
        submission: JitoBundleSubmission,
        owner: &Pubkey,
        mint: &Pubkey,
        baseline_balance: u64,
        direction: BalanceDeltaDirection,
    ) -> Result<JitoConfirmedBundle> {
        let JitoBundleSubmission {
            signature,
            signatures,
            bundle_uuid,
            submit_endpoint,
        } = submission;

        if signatures.is_empty() {
            return Err(TriggerError::JitoBundleError(
                "Bundle submission receipt did not contain any transaction signatures".to_string(),
            ));
        }

        let primary_signature = signatures.first().copied().unwrap_or(signature);
        let status_future = self.wait_for_bundle_status_with_timeout(
            &bundle_uuid,
            &submit_endpoint,
            Duration::from_millis(BUNDLE_CONFIRMATION_TIMEOUT_MS),
            Duration::from_millis(200),
        );
        let balance_future = self.wait_for_balance_delta(
            owner,
            mint,
            baseline_balance,
            direction,
            Duration::from_millis(BUNDLE_CONFIRMATION_TIMEOUT_MS),
            Duration::from_millis(200),
        );
        tokio::pin!(status_future);
        tokio::pin!(balance_future);

        let mut last_status: Option<Result<BundleStatus>> = None;
        let mut balance_observed = None;
        loop {
            tokio::select! {
                status = &mut status_future, if last_status.is_none() => {
                    match status {
                        Ok(bundle_status) if bundle_status.status == BundleState::Accepted => {
                            return Ok(JitoConfirmedBundle {
                                signature: primary_signature,
                                signatures: signatures.clone(),
                                bundle_uuid: bundle_uuid.clone(),
                                landed_slot: bundle_status.landed_slot,
                            });
                        }
                        other => {
                            last_status = Some(other);
                        }
                    }
                }
                balance = &mut balance_future, if balance_observed.is_none() => {
                    match balance {
                        Ok(true) => {
                            return Ok(JitoConfirmedBundle {
                                signature: primary_signature,
                                signatures: signatures.clone(),
                                bundle_uuid: bundle_uuid.clone(),
                                landed_slot: None,
                            });
                        }
                        Ok(false) => balance_observed = Some(Ok(false)),
                        Err(error) => balance_observed = Some(Err(error)),
                    }
                }
            }

            if last_status.is_some() && balance_observed.is_some() {
                break;
            }
        }

        match (last_status, balance_observed) {
            (Some(Ok(status)), Some(Ok(false))) => match status.status {
                BundleState::Rejected | BundleState::Expired => Err(TriggerError::JitoBundleError(
                    format!("Bundle {} failed Jito confirmation ({})", bundle_uuid, Self::format_bundle_status_summary(&status)),
                )),
                BundleState::Pending => Err(TriggerError::JitoBundleError(format!(
                    "Bundle {} remained pending without balance delta after {}ms",
                    bundle_uuid, BUNDLE_CONFIRMATION_TIMEOUT_MS
                ))),
                BundleState::Accepted => unreachable!("accepted status returns early"),
            },
            (Some(Err(status_error)), Some(Ok(false))) => Err(status_error),
            (Some(Ok(status)), Some(Err(balance_error))) => match status.status {
                BundleState::Rejected | BundleState::Expired => Err(TriggerError::JitoBundleError(
                    format!("Bundle {} failed Jito confirmation ({})", bundle_uuid, Self::format_bundle_status_summary(&status)),
                )),
                BundleState::Pending => Err(TriggerError::JitoBundleError(format!(
                    "Bundle {} remained pending and balance delta observation failed: {}",
                    bundle_uuid, balance_error
                ))),
                BundleState::Accepted => unreachable!("accepted status returns early"),
            },
            (Some(Err(status_error)), Some(Err(balance_error))) => Err(TriggerError::JitoBundleError(
                format!(
                    "Jito status confirmation failed and balance delta was not observed for bundle {}: status_error={} balance_error={}",
                    bundle_uuid, status_error, balance_error
                ),
            )),
            _ => Err(TriggerError::JitoBundleError(format!(
                "bundle {} confirmation ended without terminal signal",
                bundle_uuid
            ))),
        }
    }

    /// Confirm a previously submitted single-transaction bundle.
    ///
    /// This is split out so callers can record an explicit submitted state before
    /// blocking on Jito acceptance / on-chain reconciliation.
    pub async fn confirm_single_transaction_submission(
        &self,
        submission: JitoBundleSubmission,
    ) -> Result<JitoConfirmedBundle> {
        if submission.signatures.len() != 1 {
            return Err(TriggerError::ConfigError(format!(
                "confirm_single_transaction_submission requires exactly one tracked signature, got {}",
                submission.signatures.len()
            )));
        }

        self.confirm_bundle_submission(submission).await
    }

    /// Submit a single transaction bundle and block until Jito confirms landing.
    pub async fn submit_single_transaction_and_confirm(
        &self,
        transaction: VersionedTransaction,
    ) -> Result<JitoConfirmedBundle> {
        let submission = self.submit_single_transaction(transaction).await?;
        self.confirm_single_transaction_submission(submission).await
    }

    /// Check if a bundle should be used for this transaction
    ///
    /// Based on configuration and transaction characteristics
    pub fn should_use_bundle(&self, _transaction: &VersionedTransaction) -> bool {
        // For now, always use bundle when client is configured
        // In production, this could be more sophisticated:
        // - Only bundle high-value swaps
        // - Only bundle after Raydium migration
        // - Only bundle during high congestion
        true
    }

    /// Get bundle status
    ///
    /// # Arguments
    /// * `bundle_id` - The bundle ID to check
    ///
    /// # Returns
    /// * Bundle status information
    pub async fn get_bundle_status(&self, bundle_id: &Signature) -> Result<BundleStatus> {
        debug!("Checking bundle status for: {}", bundle_id);

        if self.dry_run {
            info!("DRY-RUN: Would check status for bundle: {}", bundle_id);
            return Ok(BundleStatus {
                bundle_id: bundle_id.to_string(),
                status: BundleState::Pending,
                landed_slot: None,
                reason: None,
            });
        }

        // Note: For bundle status, we need the bundle UUID, not the signature
        // In a production system, you would store the mapping from signature to UUID
        // For now, we'll return a pending status as a fallback
        warn!("Bundle status checking requires bundle UUID mapping - returning pending status");

        Ok(BundleStatus {
            bundle_id: bundle_id.to_string(),
            status: BundleState::Pending,
            landed_slot: None,
            reason: None,
        })
    }

    /// Get bundle status by UUID
    ///
    /// # Arguments
    /// * `bundle_uuid` - The Jito bundle UUID to check
    ///
    /// # Returns
    /// * Bundle status information
    pub async fn get_bundle_status_by_uuid(
        &self,
        bundle_uuid: &str,
        submit_endpoint: &str,
    ) -> Result<BundleStatus> {
        let status_base_url = self.status_sdk_base_url(submit_endpoint);
        debug!(
            bundle_uuid = bundle_uuid,
            submit_endpoint = submit_endpoint,
            status_base_url = status_base_url,
            "Checking bundle status for UUID via ACK-matched Jito host"
        );

        if self.dry_run {
            info!(
                "DRY-RUN: Would check status for bundle UUID: {}",
                bundle_uuid
            );
            return Ok(BundleStatus {
                bundle_id: bundle_uuid.to_string(),
                status: BundleState::Pending,
                landed_slot: None,
                reason: None,
            });
        }

        let status_sdk = self.status_sdk(&status_base_url)?;

        // First try in-flight status
        let response = status_sdk
            .get_in_flight_bundle_statuses(vec![bundle_uuid.to_string()])
            .await
            .map_err(|e| {
                TriggerError::JitoBundleError(format!(
                    "Failed to get in-flight bundle status: {}",
                    e
                ))
            })?;

        // Parse in-flight status
        if let Some(result) = response.get("result") {
            if let Some(value) = result.get("value") {
                if let Some(statuses) = value.as_array() {
                    if let Some(bundle_status) = statuses.first() {
                        if let Some(status_str) =
                            bundle_status.get("status").and_then(|s| s.as_str())
                        {
                            let bundle_id =
                                Self::extract_bundle_status_id(bundle_uuid, bundle_status);
                            let reason = Self::extract_bundle_status_reason(bundle_status);
                            match status_str {
                                "Landed" => {
                                    // Check final status for slot info
                                    return self
                                        .get_final_bundle_status(bundle_uuid, submit_endpoint)
                                        .await;
                                }
                                "Pending" => {
                                    return Ok(BundleStatus {
                                        bundle_id,
                                        status: BundleState::Pending,
                                        landed_slot: None,
                                        reason,
                                    });
                                }
                                "Failed" | "Rejected" => {
                                    if reason.is_none() {
                                        warn!(
                                            bundle_uuid = %bundle_uuid,
                                            bundle_id = %bundle_id,
                                            raw_status = %bundle_status,
                                            "Jito rejected in-flight bundle status did not include a structured reason field"
                                        );
                                    }
                                    return Ok(BundleStatus {
                                        bundle_id,
                                        status: BundleState::Rejected,
                                        landed_slot: None,
                                        reason,
                                    });
                                }
                                "Invalid" => {
                                    if reason.is_none() {
                                        warn!(
                                            bundle_uuid = %bundle_uuid,
                                            bundle_id = %bundle_id,
                                            raw_status = %bundle_status,
                                            "Jito invalid in-flight bundle status did not include a structured reason field"
                                        );
                                    }
                                    return Ok(BundleStatus {
                                        bundle_id,
                                        status: BundleState::Rejected,
                                        landed_slot: None,
                                        reason,
                                    });
                                }
                                "Expired" => {
                                    return Ok(BundleStatus {
                                        bundle_id,
                                        status: BundleState::Expired,
                                        landed_slot: None,
                                        reason,
                                    });
                                }
                                _ => {
                                    warn!("Unknown bundle status: {}", status_str);
                                }
                            }
                        }
                    }
                }
            }
        }

        // If in-flight didn't work, try final status
        self.get_final_bundle_status(bundle_uuid, submit_endpoint)
            .await
    }

    /// Get final bundle status (for landed bundles)
    async fn get_final_bundle_status(
        &self,
        bundle_uuid: &str,
        submit_endpoint: &str,
    ) -> Result<BundleStatus> {
        let status_base_url = self.status_sdk_base_url(submit_endpoint);
        let status_sdk = self.status_sdk(&status_base_url)?;

        let response = status_sdk
            .get_bundle_statuses(vec![bundle_uuid.to_string()])
            .await
            .map_err(|e| {
                TriggerError::JitoBundleError(format!("Failed to get bundle status: {}", e))
            })?;

        if let Some(result) = response.get("result") {
            if let Some(value) = result.get("value") {
                if let Some(statuses) = value.as_array() {
                    if let Some(bundle_status) = statuses.first() {
                        let bundle_id = Self::extract_bundle_status_id(bundle_uuid, bundle_status);
                        let reason = Self::extract_bundle_status_reason(bundle_status);
                        let confirmation_status = bundle_status
                            .get("confirmation_status")
                            .and_then(|s| s.as_str());

                        let slot = bundle_status.get("slot").and_then(|s| s.as_u64());

                        let state = match confirmation_status {
                            Some("confirmed") | Some("finalized") => BundleState::Accepted,
                            Some("failed") | Some("invalid") | Some("rejected") => {
                                BundleState::Rejected
                            }
                            Some("expired") => BundleState::Expired,
                            _ => BundleState::Pending,
                        };

                        if matches!(state, BundleState::Rejected | BundleState::Expired)
                            && reason.is_none()
                        {
                            warn!(
                                bundle_uuid = %bundle_uuid,
                                bundle_id = %bundle_id,
                                raw_status = %bundle_status,
                                "Jito final bundle status did not include a structured reason field"
                            );
                        }

                        return Ok(BundleStatus {
                            bundle_id,
                            status: state,
                            landed_slot: slot,
                            reason,
                        });
                    }
                }
            }
        }

        Ok(BundleStatus {
            bundle_id: bundle_uuid.to_string(),
            status: BundleState::Pending,
            landed_slot: None,
            reason: None,
        })
    }

    fn format_bundle_status_summary(status: &BundleStatus) -> String {
        match &status.reason {
            Some(reason) => format!("status: {:?}, reason: {}", status.status, reason),
            None => format!("status: {:?}", status.status),
        }
    }

    fn extract_bundle_status_id(bundle_uuid: &str, bundle_status: &Value) -> String {
        bundle_status
            .get("bundle_id")
            .or_else(|| bundle_status.get("bundleId"))
            .or_else(|| bundle_status.get("uuid"))
            .and_then(|value| match value {
                Value::String(text) if !text.trim().is_empty() => Some(text.to_string()),
                Value::Number(number) => Some(number.to_string()),
                _ => None,
            })
            .unwrap_or_else(|| bundle_uuid.to_string())
    }

    fn extract_bundle_status_reason(bundle_status: &Value) -> Option<String> {
        for key in ["reason", "rejection_reason", "rejected_reason"] {
            if let Some(reason) = Self::stringify_bundle_status_value(bundle_status.get(key)) {
                return Some(reason);
            }
        }

        if let Some(error_value) = bundle_status
            .get("error")
            .or_else(|| bundle_status.get("err"))
        {
            if let Some(message) = error_value
                .get("message")
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
            {
                return Some(message.to_string());
            }
            if let Some(reason) = Self::stringify_bundle_status_value(Some(error_value)) {
                return Some(reason);
            }
        }

        if let Some(retryable) = Self::stringify_bundle_status_value(bundle_status.get("retryable"))
        {
            return Some(format!("retryable={retryable}"));
        }

        None
    }

    fn stringify_bundle_status_value(value: Option<&Value>) -> Option<String> {
        match value? {
            Value::Null => None,
            Value::String(text) => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }
            Value::Bool(flag) => Some(flag.to_string()),
            Value::Number(number) => Some(number.to_string()),
            Value::Array(items) if !items.is_empty() => {
                Some(Value::Array(items.clone()).to_string())
            }
            Value::Object(map) if !map.is_empty() => Some(Value::Object(map.clone()).to_string()),
            _ => None,
        }
    }

    fn status_sdk_base_url(&self, submit_endpoint: &str) -> String {
        let candidate = submit_endpoint.trim();
        if candidate.is_empty() {
            self.sdk_base_url.clone()
        } else {
            normalize_jito_sdk_base_url(candidate)
        }
    }

    fn status_sdk(&self, status_base_url: &str) -> Result<JitoJsonRpcSDK> {
        let status_uuid = self.status_uuid.as_ref().cloned().ok_or_else(|| {
            TriggerError::ConfigError(
                "Jito status polling requires status_uuid but none is configured".to_string(),
            )
        })?;
        Ok(JitoJsonRpcSDK::new(status_base_url, Some(status_uuid)))
    }

    pub async fn current_wallet_token_balance(&self, owner: &Pubkey, mint: &Pubkey) -> Result<u64> {
        let rpc_client = self.rpc_client.as_ref().ok_or_else(|| {
            TriggerError::ConfigError(
                "balance-delta fallback requires rpc_client but none is configured".to_string(),
            )
        })?;
        self.fetch_wallet_token_balance(rpc_client, owner, mint)
            .await
    }

    async fn fetch_wallet_token_balance(
        &self,
        rpc_client: &Arc<RpcClient>,
        owner: &Pubkey,
        mint: &Pubkey,
    ) -> Result<u64> {
        let mut total_balance = 0u64;
        for program_id_str in [
            "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
            "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
        ] {
            let program_id = Pubkey::from_str(program_id_str).map_err(|error| {
                TriggerError::ConfigError(format!(
                    "invalid token program id constant {}: {}",
                    program_id_str, error
                ))
            })?;
            let ata = get_associated_token_address_with_program_id(owner, mint, &program_id);
            match rpc_client.get_token_account_balance(&ata).await {
                Ok(response) => {
                    total_balance = total_balance
                        .saturating_add(response.amount.parse::<u64>().unwrap_or_default());
                }
                Err(error) if Self::is_missing_token_account_error(&error.to_string()) => {}
                Err(error) => {
                    return Err(TriggerError::ClientError(error));
                }
            }
        }
        Ok(total_balance)
    }

    async fn wait_for_balance_delta(
        &self,
        owner: &Pubkey,
        mint: &Pubkey,
        baseline_balance: u64,
        direction: BalanceDeltaDirection,
        timeout: Duration,
        poll_interval: Duration,
    ) -> Result<bool> {
        let rpc_client = self.rpc_client.as_ref().ok_or_else(|| {
            TriggerError::ConfigError(
                "balance-delta fallback requires rpc_client but none is configured".to_string(),
            )
        })?;
        let deadline = Instant::now() + timeout;
        loop {
            match self
                .fetch_wallet_token_balance(rpc_client, owner, mint)
                .await
            {
                Ok(observed_balance) => {
                    let satisfied = match direction {
                        BalanceDeltaDirection::Increase => observed_balance > baseline_balance,
                        BalanceDeltaDirection::Decrease => observed_balance < baseline_balance,
                    };
                    if satisfied {
                        return Ok(true);
                    }
                }
                Err(error) => {
                    warn!(
                        owner = %owner,
                        mint = %mint,
                        error = %error,
                        "balance-delta fallback failed to query wallet token balance — retrying"
                    );
                }
            }

            if Instant::now() >= deadline {
                return Ok(false);
            }
            tokio::time::sleep(poll_interval).await;
        }
    }

    fn is_missing_token_account_error(message: &str) -> bool {
        message.contains("AccountNotFound")
            || message.contains("could not find account")
            || message.contains("Invalid param: could not find account")
    }

    /// Wait for bundle status with timeout and retry
    ///
    /// Polls the bundle status until it's accepted, rejected, or timeout is reached.
    /// This implements the confirmation tracking for submitted bundles.
    ///
    /// # Arguments
    /// * `bundle_uuid` - The Jito bundle UUID to track
    ///
    /// # Returns
    /// * Final bundle status or error if timeout/failed
    pub async fn wait_for_bundle_status(
        &self,
        bundle_uuid: &str,
        submit_endpoint: &str,
    ) -> Result<BundleStatus> {
        let max_attempts = 30;
        let poll_interval = Duration::from_secs(2);

        info!(
            "Waiting for bundle status: {} via submit endpoint {}",
            bundle_uuid, submit_endpoint
        );

        for attempt in 0..max_attempts {
            let status = self
                .get_bundle_status_by_uuid(bundle_uuid, submit_endpoint)
                .await?;

            match status.status {
                BundleState::Accepted => {
                    info!(
                        bundle_uuid = %bundle_uuid,
                        bundle_id = %status.bundle_id,
                        landed_slot = ?status.landed_slot,
                        "Bundle accepted and landed"
                    );
                    return Ok(status);
                }
                BundleState::Rejected => {
                    warn!(
                        bundle_uuid = %bundle_uuid,
                        bundle_id = %status.bundle_id,
                        reason = ?status.reason,
                        "Bundle was rejected"
                    );
                    return Err(TriggerError::JitoBundleError(format!(
                        "Bundle {} was rejected by validators ({})",
                        bundle_uuid,
                        Self::format_bundle_status_summary(&status)
                    )));
                }
                BundleState::Expired => {
                    warn!(
                        bundle_uuid = %bundle_uuid,
                        bundle_id = %status.bundle_id,
                        reason = ?status.reason,
                        "Bundle expired without being included"
                    );
                    return Err(TriggerError::JitoBundleError(format!(
                        "Bundle {} expired ({})",
                        bundle_uuid,
                        Self::format_bundle_status_summary(&status)
                    )));
                }
                BundleState::Pending => {
                    if attempt % 5 == 0 {
                        debug!(
                            "Bundle {} still pending (attempt {}/{})",
                            bundle_uuid,
                            attempt + 1,
                            max_attempts
                        );
                    }
                    tokio::time::sleep(poll_interval).await;
                }
            }
        }

        Err(TriggerError::JitoBundleError(format!(
            "Timeout waiting for bundle {} status after {} seconds",
            bundle_uuid,
            max_attempts * poll_interval.as_secs()
        )))
    }

    /// Poll bundle status with a caller-provided timeout and poll interval.
    ///
    /// Returns the last observed status. If the timeout is hit while the bundle is
    /// still pending, `BundleState::Pending` is returned and the caller decides how
    /// to handle that outcome.
    pub async fn wait_for_bundle_status_with_timeout(
        &self,
        bundle_uuid: &str,
        submit_endpoint: &str,
        timeout: Duration,
        poll_interval: Duration,
    ) -> Result<BundleStatus> {
        let deadline = Instant::now() + timeout;

        info!(
            "Waiting for bundle status: {} via submit endpoint {} (timeout={}ms, poll={}ms)",
            bundle_uuid,
            submit_endpoint,
            timeout.as_millis(),
            poll_interval.as_millis()
        );

        loop {
            let status = match self
                .get_bundle_status_by_uuid(bundle_uuid, submit_endpoint)
                .await
            {
                Ok(s) => s,
                Err(err) => {
                    return Err(TriggerError::ConfigError(format!(
                        "Jito status API failed for bundle {}: {}",
                        bundle_uuid, err
                    )));
                }
            };

            match status.status {
                BundleState::Accepted | BundleState::Rejected | BundleState::Expired => {
                    return Ok(status);
                }
                BundleState::Pending => {
                    if Instant::now() >= deadline {
                        return Ok(status);
                    }
                    tokio::time::sleep(poll_interval).await;
                }
            }
        }
    }

    /// Get bundle configuration
    pub fn bundle_config(&self) -> &BundleConfig {
        &self.bundle_config
    }

    fn bundle_transaction_signatures(transactions: &[VersionedTransaction]) -> Vec<Signature> {
        transactions.iter().map(|tx| tx.signatures[0]).collect()
    }

    async fn reconcile_bundle_with_chain(
        &self,
        bundle_uuid: &str,
        bundle_status: BundleState,
        signatures: &[Signature],
        timeout: Duration,
    ) -> Result<OnchainBundleReconciliation> {
        let Some(rpc_client) = self.rpc_client.as_ref() else {
            warn!(
                bundle_uuid,
                ?bundle_status,
                "Jito bundle status was non-accepted, but on-chain reconciliation is unavailable because rpc_client is not configured"
            );
            return Ok(OnchainBundleReconciliation::Uncertain);
        };

        let observation = self
            .wait_for_signature_observation(
                rpc_client,
                signatures,
                timeout,
                Duration::from_millis(ONCHAIN_RECONCILIATION_POLL_INTERVAL_MS),
            )
            .await?;

        match classify_bundle_reconciliation(bundle_status, &observation) {
            OnchainBundleReconciliation::Confirmed(landed_slot) => {
                warn!(
                    bundle_uuid,
                    ?bundle_status,
                    landed_signatures = ?observation.landed_signatures,
                    landed_slot = ?landed_slot,
                    "Jito status poll disagreed with chain reality; all submitted bundle transactions landed on-chain, treating bundle as confirmed"
                );
                Ok(OnchainBundleReconciliation::Confirmed(landed_slot))
            }
            OnchainBundleReconciliation::Failed => {
                warn!(
                    bundle_uuid,
                    ?bundle_status,
                    landed_signatures = ?observation.landed_signatures,
                    failed_signatures = ?observation.failed_signatures,
                    missing_signatures = ?observation.missing_signatures,
                    "On-chain reconciliation treated the bundle as definitively failed"
                );
                Ok(OnchainBundleReconciliation::Failed)
            }
            OnchainBundleReconciliation::Uncertain => {
                if observation.has_any_observation() {
                    warn!(
                        bundle_uuid,
                        ?bundle_status,
                        landed_signatures = ?observation.landed_signatures,
                        missing_signatures = ?observation.missing_signatures,
                        failed_signatures = ?observation.failed_signatures,
                        "Jito status poll disagreed partially with chain reality; bundle transactions were only partially observed on-chain"
                    );
                } else {
                    warn!(
                        bundle_uuid,
                        ?bundle_status,
                        signatures = ?signatures,
                        timeout_ms = timeout.as_millis(),
                        "Jito bundle status was non-accepted, and on-chain reconciliation did not observe any submitted signatures before the fail-closed deadline"
                    );
                }
                Ok(OnchainBundleReconciliation::Uncertain)
            }
        }
    }

    async fn wait_for_signature_observation(
        &self,
        rpc_client: &Arc<RpcClient>,
        signatures: &[Signature],
        timeout: Duration,
        poll_interval: Duration,
    ) -> Result<OnchainBundleObservation> {
        let deadline = Instant::now() + timeout;

        loop {
            let statuses = match rpc_client
                .get_signature_statuses_with_history(signatures)
                .await
            {
                Ok(s) => s,
                Err(err) => {
                    warn!(
                        signatures = ?signatures,
                        error = %err,
                        "on-chain reconciliation: getSignatureStatuses RPC call failed — retrying"
                    );
                    if Instant::now() >= deadline {
                        return Ok(OnchainBundleObservation {
                            landed_signatures: vec![],
                            failed_signatures: vec![],
                            missing_signatures: signatures.to_vec(),
                            landed_slot: None,
                        });
                    }
                    tokio::time::sleep(poll_interval).await;
                    continue;
                }
            };

            let observation = summarize_onchain_bundle_observation(signatures, &statuses.value);

            if matches!(
                classify_onchain_bundle_observation(&observation),
                OnchainBundleReconciliation::Confirmed(_) | OnchainBundleReconciliation::Failed
            ) || Instant::now() >= deadline
            {
                return Ok(observation);
            }

            tokio::time::sleep(poll_interval).await;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OnchainBundleReconciliation {
    Confirmed(Option<u64>),
    Failed,
    Uncertain,
}

fn classify_onchain_bundle_observation(
    observation: &OnchainBundleObservation,
) -> OnchainBundleReconciliation {
    if observation.all_landed_successfully() {
        OnchainBundleReconciliation::Confirmed(observation.landed_slot)
    } else if observation.has_failed_signatures() {
        OnchainBundleReconciliation::Failed
    } else {
        OnchainBundleReconciliation::Uncertain
    }
}

fn classify_bundle_reconciliation(
    bundle_status: BundleState,
    observation: &OnchainBundleObservation,
) -> OnchainBundleReconciliation {
    match classify_onchain_bundle_observation(observation) {
        OnchainBundleReconciliation::Uncertain
            if !observation.has_any_observation()
                && matches!(bundle_status, BundleState::Rejected | BundleState::Expired) =>
        {
            OnchainBundleReconciliation::Failed
        }
        verdict => verdict,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OnchainBundleObservation {
    landed_signatures: Vec<Signature>,
    missing_signatures: Vec<Signature>,
    failed_signatures: Vec<(Signature, String)>,
    landed_slot: Option<u64>,
}

impl OnchainBundleObservation {
    fn all_landed_successfully(&self) -> bool {
        self.missing_signatures.is_empty() && self.failed_signatures.is_empty()
    }

    fn has_any_observation(&self) -> bool {
        !self.landed_signatures.is_empty() || !self.failed_signatures.is_empty()
    }

    fn has_failed_signatures(&self) -> bool {
        !self.failed_signatures.is_empty()
    }
}

fn summarize_onchain_bundle_observation<T>(
    signatures: &[Signature],
    statuses: &[Option<T>],
) -> OnchainBundleObservation
where
    T: OnchainBundleStatusView,
{
    let mut landed_signatures = Vec::new();
    let mut missing_signatures = Vec::new();
    let mut failed_signatures = Vec::new();
    let mut landed_slot = None;

    for (signature, status) in signatures.iter().zip(statuses.iter()) {
        match status {
            Some(status) if status.err_string().is_none() => {
                landed_signatures.push(*signature);
                landed_slot = Some(
                    landed_slot.map_or(status.slot(), |current: u64| current.max(status.slot())),
                );
            }
            Some(status) => {
                failed_signatures.push((
                    *signature,
                    status
                        .err_string()
                        .unwrap_or_else(|| "unknown transaction error".to_string()),
                ));
            }
            None => missing_signatures.push(*signature),
        }
    }

    OnchainBundleObservation {
        landed_signatures,
        missing_signatures,
        failed_signatures,
        landed_slot,
    }
}

trait OnchainBundleStatusView {
    fn slot(&self) -> u64;
    fn err_string(&self) -> Option<String>;
}

impl OnchainBundleStatusView for solana_transaction_status::TransactionStatus {
    fn slot(&self) -> u64 {
        self.slot
    }

    fn err_string(&self) -> Option<String> {
        self.err.as_ref().map(|err| format!("{err:?}"))
    }
}

/// Bundle status information
#[derive(Debug, Clone)]
pub struct BundleStatus {
    /// Jito bundle identifier / UUID associated with the returned status payload.
    pub bundle_id: String,
    /// Current status
    pub status: BundleState,
    /// Slot where bundle landed (if accepted)
    pub landed_slot: Option<u64>,
    /// Rejection / failure detail returned by Jito, if present.
    pub reason: Option<String>,
}

/// Bundle state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleState {
    /// Bundle is pending
    Pending,
    /// Bundle was accepted and included
    Accepted,
    /// Bundle was rejected
    Rejected,
    /// Bundle expired without being included
    Expired,
}

/// Builder for Jito client configuration
pub struct JitoClientBuilder {
    endpoint: Option<String>,
    grpc_auth: Option<String>,
    status_uuid: Option<String>,
    bundle_config: BundleConfig,
    dry_run: bool,
    rpc_client: Option<Arc<RpcClient>>,
}

impl JitoClientBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            endpoint: None,
            grpc_auth: None,
            status_uuid: None,
            bundle_config: BundleConfig::default(),
            dry_run: false,
            rpc_client: None,
        }
    }

    /// Set Jito endpoint
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    /// Set opaque gRPC auth metadata value.
    pub fn with_grpc_auth(mut self, grpc_auth: impl Into<String>) -> Self {
        self.grpc_auth = Some(grpc_auth.into());
        self
    }

    /// Set REST status UUID.
    pub fn with_status_uuid(mut self, status_uuid: impl Into<String>) -> Self {
        self.status_uuid = Some(status_uuid.into());
        self
    }

    /// Legacy alias preserved for migration/test compatibility.
    pub fn with_uuid(mut self, uuid: impl Into<String>) -> Self {
        self.grpc_auth = Some(uuid.into());
        self
    }

    /// Set RPC client for simulation (IRONCLAD protocol)
    pub fn with_rpc_client(mut self, rpc_client: Arc<RpcClient>) -> Self {
        self.rpc_client = Some(rpc_client);
        self
    }

    /// Set bundle configuration
    pub fn with_bundle_config(mut self, config: BundleConfig) -> Self {
        self.bundle_config = config;
        self
    }

    /// Set redundancy policy
    pub fn with_redundancy_policy(mut self, policy: RedundancyPolicy) -> Self {
        self.bundle_config.redundancy_policy = policy;
        self
    }

    /// Set tip configuration
    pub fn with_tip_config(mut self, config: TipConfig) -> Self {
        self.bundle_config.tip_config = config;
        self
    }

    /// Enable or disable diagnostics
    pub fn with_diagnostics(mut self, enable: bool) -> Self {
        self.bundle_config.enable_diagnostics = enable;
        self
    }

    /// Enable or disable dry-run mode
    pub fn with_dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }

    /// Build the Jito client
    pub fn build(self) -> Result<JitoClient> {
        let endpoint = self.endpoint.unwrap_or_else(|| {
            // Default Jito endpoint
            format!(
                "https://mainnet.block-engine.jito.wtf{}",
                JITO_BUNDLES_JSONRPC_PATH
            )
        });

        let mut client = match self.grpc_auth {
            Some(grpc_auth) => {
                JitoClient::new_with_auth(endpoint, grpc_auth, self.status_uuid, self.bundle_config)
            }
            None => {
                JitoClient::new_with_status_uuid(endpoint, self.status_uuid, self.bundle_config)
            }
        };

        client.set_dry_run(self.dry_run);

        if let Some(rpc) = self.rpc_client {
            client.rpc_client = Some(rpc);
        }

        Ok(client)
    }
}

impl Default for JitoClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::signature::Keypair;
    use solana_sdk::signer::Signer;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tonic::{Code, Status};

    #[test]
    fn test_jito_client_creation() {
        let config = BundleConfig::default();
        let client = JitoClient::new("https://test.jito.wtf/api/v1", config);
        assert_eq!(client.endpoint, "https://test.jito.wtf/api/v1/bundles");
        assert_eq!(client.sdk_base_url, "https://test.jito.wtf/api/v1");
        assert_eq!(client.grpc_endpoint, "https://test.jito.wtf/");
        assert!(!client.dry_run);
    }

    #[test]
    fn test_jito_client_with_uuid() {
        let config = BundleConfig::default();
        let client = JitoClient::new_with_uuid(
            "https://test.jito.wtf/api/v1",
            "test-uuid-123".to_string(),
            config,
        );
        assert_eq!(client.endpoint, "https://test.jito.wtf/api/v1/bundles");
        assert_eq!(client.sdk_base_url, "https://test.jito.wtf/api/v1");
        assert_eq!(client.grpc_endpoint, "https://test.jito.wtf/");
        assert_eq!(client.grpc_auth, Some("test-uuid-123".to_string()));
        assert_eq!(client.status_uuid, None);
    }

    #[test]
    fn test_normalize_jito_endpoint_promotes_public_host_to_bundle_path() {
        assert_eq!(
            normalize_jito_endpoint("https://frankfurt.mainnet.block-engine.jito.wtf"),
            "https://frankfurt.mainnet.block-engine.jito.wtf/api/v1/bundles"
        );
        assert_eq!(
            normalize_jito_endpoint("frankfurt.mainnet.block-engine.jito.wtf"),
            "https://frankfurt.mainnet.block-engine.jito.wtf/api/v1/bundles"
        );
        assert_eq!(
            normalize_jito_endpoint("https://test.jito.wtf/api/v1"),
            "https://test.jito.wtf/api/v1/bundles"
        );
    }

    #[test]
    fn test_normalize_jito_sdk_base_url_strips_bundle_suffix_for_sdk() {
        assert_eq!(
            normalize_jito_sdk_base_url("https://frankfurt.mainnet.block-engine.jito.wtf"),
            "https://frankfurt.mainnet.block-engine.jito.wtf/api/v1"
        );
        assert_eq!(
            normalize_jito_sdk_base_url("https://test.jito.wtf/api/v1/bundles"),
            "https://test.jito.wtf/api/v1"
        );
    }

    #[test]
    fn test_normalize_jito_grpc_endpoint_strips_paths() {
        assert_eq!(
            normalize_jito_grpc_endpoint(
                "https://frankfurt.mainnet.block-engine.jito.wtf/api/v1/bundles"
            ),
            "https://frankfurt.mainnet.block-engine.jito.wtf/"
        );
        assert_eq!(
            normalize_jito_grpc_endpoint("frankfurt.mainnet.block-engine.jito.wtf"),
            "https://frankfurt.mainnet.block-engine.jito.wtf/"
        );
    }

    #[test]
    fn test_build_public_jito_failover_endpoints_orders_requested_public_host_first() {
        let endpoints =
            build_public_jito_failover_endpoints("https://frankfurt.mainnet.block-engine.jito.wtf");

        assert_eq!(
            endpoints.first().map(String::as_str),
            Some("https://frankfurt.mainnet.block-engine.jito.wtf/api/v1/bundles")
        );
        assert!(endpoints.iter().any(|endpoint| {
            endpoint == "https://amsterdam.mainnet.block-engine.jito.wtf/api/v1/bundles"
        }));
        assert!(endpoints.iter().any(|endpoint| {
            endpoint == "https://tokyo.mainnet.block-engine.jito.wtf/api/v1/bundles"
        }));
    }

    #[test]
    fn test_build_public_jito_failover_endpoints_expands_mainnet_alias() {
        let endpoints =
            build_public_jito_failover_endpoints("https://mainnet.block-engine.jito.wtf");

        assert_eq!(
            endpoints.first().map(String::as_str),
            Some("https://mainnet.block-engine.jito.wtf/api/v1/bundles")
        );
        assert!(endpoints.iter().any(|endpoint| {
            endpoint == "https://frankfurt.mainnet.block-engine.jito.wtf/api/v1/bundles"
        }));
        assert!(endpoints.iter().any(|endpoint| {
            endpoint == "https://slc.mainnet.block-engine.jito.wtf/api/v1/bundles"
        }));
    }

    #[test]
    fn test_build_public_jito_failover_endpoints_keeps_custom_host_singleton() {
        let endpoints = build_public_jito_failover_endpoints("https://private.example.invalid");
        assert_eq!(
            endpoints,
            vec!["https://private.example.invalid/api/v1/bundles".to_string()]
        );
    }

    #[test]
    fn test_build_public_jito_failover_grpc_endpoints_uses_fixed_four_host_sequence() {
        let endpoints = build_public_jito_failover_grpc_endpoints(
            "https://amsterdam.mainnet.block-engine.jito.wtf/api/v1/bundles",
        );

        assert_eq!(
            endpoints,
            vec![
                "https://frankfurt.mainnet.block-engine.jito.wtf/".to_string(),
                "https://amsterdam.mainnet.block-engine.jito.wtf/".to_string(),
                "https://london.mainnet.block-engine.jito.wtf/".to_string(),
                "https://dublin.mainnet.block-engine.jito.wtf/".to_string(),
            ]
        );
    }

    #[test]
    fn test_build_grpc_send_bundle_request_decodes_packets() {
        let params = json!([
            ["ZmFrZS10eA==", "AQID"],
            {"encoding":"base64"}
        ]);

        let request = build_grpc_send_bundle_request(&params).expect("grpc request");
        let bundle = request.bundle.expect("bundle");
        assert_eq!(bundle.packets.len(), 2);
        assert_eq!(bundle.packets[0].data, b"fake-tx");
        assert_eq!(bundle.packets[1].data, vec![1, 2, 3]);
        assert_eq!(
            bundle.packets[0].meta.as_ref().map(|meta| meta.size),
            Some(7)
        );
        assert_eq!(
            bundle.packets[1].meta.as_ref().map(|meta| meta.size),
            Some(3)
        );
    }

    #[test]
    fn test_build_grpc_send_bundle_request_rejects_non_base64_encoding() {
        let params = json!([
            ["ZmFrZS10eA=="],
            {"encoding":"base58"}
        ]);

        let err = build_grpc_send_bundle_request(&params).expect_err("non-base64 should fail");
        assert!(err
            .to_string()
            .contains("Unsupported Jito bundle encoding for gRPC submit"));
    }

    #[test]
    fn test_attach_jito_auth_metadata_inserts_x_jito_auth_header() {
        let params = json!([
            ["ZmFrZS10eA=="],
            {"encoding":"base64"}
        ]);
        let request = build_grpc_send_bundle_request(&params).expect("grpc request");
        let mut grpc_request = tonic::Request::new(request);

        attach_jito_auth_metadata(
            "https://frankfurt.mainnet.block-engine.jito.wtf/",
            Some("test-uuid-123"),
            &mut grpc_request,
        )
        .expect("metadata should be inserted");

        assert_eq!(
            grpc_request
                .metadata()
                .get("x-jito-auth")
                .and_then(|value| value.to_str().ok()),
            Some("test-uuid-123")
        );
    }

    #[test]
    fn test_attach_jito_auth_metadata_rejects_invalid_header_value() {
        let params = json!([
            ["ZmFrZS10eA=="],
            {"encoding":"base64"}
        ]);
        let request = build_grpc_send_bundle_request(&params).expect("grpc request");
        let mut grpc_request = tonic::Request::new(request);

        let err = attach_jito_auth_metadata(
            "https://frankfurt.mainnet.block-engine.jito.wtf/",
            Some("bad\nuuid"),
            &mut grpc_request,
        )
        .expect_err("invalid metadata must fail closed");

        assert!(matches!(
            err,
            JitoSubmitAttemptError::Fatal { message, .. }
                if message.contains("Invalid UUID metadata for x-jito-auth")
        ));
    }

    #[test]
    fn test_classify_grpc_status_maps_resource_exhausted_to_rate_limited() {
        let err = classify_grpc_status(
            "https://amsterdam.mainnet.block-engine.jito.wtf/",
            Status::new(Code::ResourceExhausted, "rate limited"),
        );
        assert!(matches!(
            err,
            JitoSubmitAttemptError::RateLimited {
                code: Some(value),
                ..
            } if value == Code::ResourceExhausted as i64
        ));
    }

    #[test]
    fn test_classify_grpc_status_maps_unavailable_to_retryable() {
        let err = classify_grpc_status(
            "https://amsterdam.mainnet.block-engine.jito.wtf/",
            Status::new(Code::Unavailable, "backend unavailable"),
        );
        assert!(matches!(
            err,
            JitoSubmitAttemptError::RetryableTransport { .. }
        ));
    }

    #[test]
    fn test_builder() {
        let client = JitoClientBuilder::new()
            .with_endpoint("https://test.jito.wtf/api/v1")
            .with_uuid("test-uuid")
            .with_redundancy_policy(RedundancyPolicy::NPlusFive)
            .with_diagnostics(true)
            .with_dry_run(true)
            .build();

        assert!(client.is_ok());
        let client = client.unwrap();
        assert_eq!(
            client.bundle_config.redundancy_policy,
            RedundancyPolicy::NPlusFive
        );
        assert!(client.bundle_config.enable_diagnostics);
        assert!(client.dry_run);
        assert_eq!(client.grpc_auth, Some("test-uuid".to_string()));
        assert_eq!(client.status_uuid, None);
        assert_eq!(client.endpoint, "https://test.jito.wtf/api/v1/bundles");
        assert_eq!(client.sdk_base_url, "https://test.jito.wtf/api/v1");
    }

    #[test]
    fn test_tip_calculation() {
        let config = BundleConfig::default();
        let client = JitoClient::new("https://test.jito.wtf", config);

        // Test base tip (priority = 0.0)
        let tx_value = 1_000_000_000; // 1 SOL
        let base_tip = client.bundle_config.tip_config.calculate_tip(tx_value, 0.0);
        assert_eq!(base_tip, 20_000_000); // 2% of 1 SOL

        // Test dynamic tip (priority = 1.0)
        let dynamic_tip = client.bundle_config.tip_config.calculate_tip(tx_value, 1.0);
        assert_eq!(dynamic_tip, 50_000_000); // 5% of 1 SOL
    }

    #[test]
    fn test_build_bundle() {
        let config = BundleConfig::default();
        let client = JitoClient::new("https://test.jito.wtf/api/v1", config);

        let init_pool_tx = create_dummy_transaction();
        let ghost_txs = vec![create_dummy_transaction(), create_dummy_transaction()];

        let bundle = client.build_bundle(
            init_pool_tx,
            ghost_txs,
            1_000_000_000, // 1 SOL value
            0.5,           // Medium priority
            solana_sdk::hash::Hash::default(),
            None, // No tip payer for this test
        );

        assert!(bundle.is_ok());
        let bundle = bundle.unwrap();
        assert_eq!(bundle.transactions.len(), 3); // 1 init + 2 ghost (no tip tx)
        assert_eq!(bundle.tip_lamports, 35_000_000); // 3.5% of 1 SOL
    }

    #[test]
    fn test_build_bundle_empty_ghost_txs() {
        let config = BundleConfig::default();
        let client = JitoClient::new("https://test.jito.wtf/api/v1", config);

        let init_pool_tx = create_dummy_transaction();
        let ghost_txs = vec![];

        let bundle = client.build_bundle(
            init_pool_tx,
            ghost_txs,
            1_000_000_000,
            0.5,
            solana_sdk::hash::Hash::default(),
            None,
        );

        assert!(bundle.is_err());
    }

    #[test]
    fn test_diagnostics_creation() {
        let config = BundleConfig::default();
        let client = JitoClient::new("https://test.jito.wtf/api/v1", config);

        let init_pool_tx = create_dummy_transaction();
        let ghost_txs = vec![create_dummy_transaction()];

        let bundle = client
            .build_bundle(
                init_pool_tx,
                ghost_txs,
                1_000_000_000,
                0.5,
                solana_sdk::hash::Hash::default(),
                None,
            )
            .unwrap();

        let diagnostics = client.create_diagnostics(&bundle, 1_000_000_000, 0.5);

        assert_eq!(diagnostics.transaction_count, 2);
        assert_eq!(diagnostics.tip_lamports, 35_000_000);
        assert_eq!(diagnostics.priority_factor, 0.5);
        assert_eq!(diagnostics.redundancy_count, 4); // N+3 default
        assert!(diagnostics.nonce_staggered);
        assert!(!diagnostics.explanation.is_empty());
    }

    #[test]
    fn test_should_use_bundle() {
        let config = BundleConfig::default();
        let client = JitoClient::new("https://test.jito.wtf/api/v1", config);

        let dummy_tx = create_dummy_transaction();
        assert!(client.should_use_bundle(&dummy_tx));
    }

    #[test]
    fn test_dry_run_mode() {
        let config = BundleConfig::default();
        let mut client = JitoClient::new("https://test.jito.wtf/api/v1", config);

        assert!(!client.dry_run);
        client.set_dry_run(true);
        assert!(client.dry_run);
        client.set_dry_run(false);
        assert!(!client.dry_run);
    }

    #[test]
    fn test_build_bundle_with_tip_transaction() {
        let config = BundleConfig::default();
        let client = JitoClient::new("https://test.jito.wtf/api/v1", config);

        let tip_payer = Keypair::new();
        let init_pool_tx = create_dummy_transaction();
        let ghost_txs = vec![create_dummy_transaction(), create_dummy_transaction()];

        let bundle = client.build_bundle(
            init_pool_tx,
            ghost_txs,
            1_000_000_000, // 1 SOL value
            0.5,           // Medium priority
            solana_sdk::hash::Hash::default(),
            Some(&tip_payer), // Include tip payer
        );

        assert!(bundle.is_ok());
        let bundle = bundle.unwrap();
        // Should have 4 transactions: 1 init + 2 ghost + 1 tip
        assert_eq!(bundle.transactions.len(), 4);
        assert_eq!(bundle.tip_lamports, 35_000_000); // 3.5% of 1 SOL
    }

    #[test]
    fn test_submit_bundle_with_redundancy_receipt_tracks_all_bundle_signatures_in_dry_run() {
        let client = JitoClientBuilder::new()
            .with_endpoint("https://test.jito.wtf/api/v1")
            .with_dry_run(true)
            .build()
            .expect("dry-run client");

        let tx1 = create_dummy_transaction();
        let tx2 = create_dummy_transaction();
        let transactions = vec![tx1, tx2];
        let signatures = JitoClient::bundle_transaction_signatures(&transactions);
        let bundle = JitoBundle {
            transactions,
            tip_lamports: 123,
            bundle_id: signatures[0],
            recent_blockhash: Hash::default(),
        };

        let submission = block_on_test_future(client.submit_bundle_with_redundancy_receipt(bundle))
            .expect("dry-run submission");

        assert_eq!(submission.signature, signatures[0]);
        assert_eq!(submission.signatures, signatures);
        assert_eq!(submission.submit_endpoint, "https://test.jito.wtf/");
    }

    #[test]
    fn test_confirm_bundle_submission_preserves_all_tracked_signatures_in_dry_run() {
        let client = JitoClientBuilder::new()
            .with_endpoint("https://test.jito.wtf/api/v1")
            .with_dry_run(true)
            .build()
            .expect("dry-run client");
        let signatures = vec![Signature::new_unique(), Signature::new_unique()];

        let confirmed =
            block_on_test_future(client.confirm_bundle_submission(JitoBundleSubmission {
                signature: signatures[0],
                signatures: signatures.clone(),
                bundle_uuid: "dry-run-bundle".to_string(),
                submit_endpoint: "https://test.jito.wtf/".to_string(),
            }))
            .expect("dry-run confirm");

        assert_eq!(confirmed.signature, signatures[0]);
        assert_eq!(confirmed.signatures, signatures);
        assert_eq!(confirmed.bundle_uuid, "dry-run-bundle");
        assert_eq!(confirmed.landed_slot, None);
    }

    #[test]
    fn test_confirm_single_transaction_submission_rejects_multi_signature_receipt() {
        let client = JitoClientBuilder::new()
            .with_endpoint("https://test.jito.wtf/api/v1")
            .with_dry_run(true)
            .build()
            .expect("dry-run client");

        let err = block_on_test_future(client.confirm_single_transaction_submission(
            JitoBundleSubmission {
                signature: Signature::new_unique(),
                signatures: vec![Signature::new_unique(), Signature::new_unique()],
                bundle_uuid: "dry-run-bundle".to_string(),
                submit_endpoint: "https://test.jito.wtf/".to_string(),
            },
        ))
        .expect_err("multi-signature receipt should be rejected");

        assert!(matches!(
            err,
            TriggerError::ConfigError(message)
                if message.contains("requires exactly one tracked signature")
        ));
    }

    #[tokio::test]
    async fn test_confirm_bundle_submission_rejected_bundle_keeps_tip_signature_offchain() {
        let bundle_id = Signature::new_unique();
        let tip_signature = Signature::new_unique();
        let bundle_uuid = "bundle-rejected-no-tip";
        let rejection_reason = "winning_batch_bid_too_low";
        let (jito_url, _jito_requests) =
            spawn_failed_bundle_status_server(bundle_uuid, rejection_reason).await;
        let (rpc_url, rpc_requests) = spawn_missing_signature_status_rpc_server().await;
        let rpc_client = Arc::new(RpcClient::new(rpc_url));
        let client = JitoClientBuilder::new()
            .with_endpoint(jito_url.clone())
            .with_rpc_client(rpc_client)
            .build()
            .expect("mock client");

        let status = client
            .get_bundle_status_by_uuid(bundle_uuid, &jito_url)
            .await
            .expect("status lookup");
        assert_eq!(status.bundle_id, bundle_uuid);
        assert_eq!(status.status, BundleState::Rejected);
        assert_eq!(status.reason.as_deref(), Some(rejection_reason));

        let err = client
            .confirm_bundle_submission(JitoBundleSubmission {
                signature: bundle_id,
                signatures: vec![bundle_id, tip_signature],
                bundle_uuid: bundle_uuid.to_string(),
                submit_endpoint: jito_url.clone(),
            })
            .await
            .expect_err("rejected bundle should fail closed");

        assert!(matches!(
            err,
            TriggerError::JitoBundleError(message)
                if message.contains(bundle_uuid)
                    && message.contains("Rejected")
                    && message.contains(rejection_reason)
        ));

        let captured_requests = rpc_requests.lock().expect("rpc request lock").join("\n");
        assert!(
            captured_requests.contains(&bundle_id.to_string()),
            "on-chain reconciliation must check the rejected bundle's sell signature"
        );
        assert!(
            captured_requests.contains(&tip_signature.to_string()),
            "on-chain reconciliation must check the rejected bundle's tip signature"
        );

        println!(
            "bundle_id={} bundle_uuid={} status=Rejected sell_signature_observed_on_chain=false tip_signature_observed_on_chain=false tip_transfer_from_bundle=false",
            bundle_id, bundle_uuid
        );
    }

    #[tokio::test]
    async fn test_get_bundle_status_by_uuid_uses_submit_endpoint_host_for_polling() {
        let bundle_uuid = "bundle-host-match";
        let wrong_server_bundle_uuid = bundle_uuid.to_string();
        let (wrong_url, wrong_requests) = spawn_mock_json_server(Arc::new(move |request| {
            if request.contains("getInflightBundleStatuses") {
                format!(
                    "{{\"jsonrpc\":\"2.0\",\"result\":{{\"context\":{{\"slot\":1}},\"value\":[{{\"bundle_id\":\"{}\",\"status\":\"Pending\",\"landed_slot\":null}}]}},\"id\":1}}",
                    wrong_server_bundle_uuid
                )
            } else if request.contains("\"getVersion\"") {
                "{\"jsonrpc\":\"2.0\",\"result\":{\"solana-core\":\"1.18.26\",\"feature-set\":1},\"id\":1}".to_string()
            } else {
                "{\"jsonrpc\":\"2.0\",\"result\":{\"value\":[]},\"id\":1}".to_string()
            }
        }))
        .await;
        let (right_url, right_requests) =
            spawn_failed_bundle_status_server(bundle_uuid, "regional-host-match").await;

        let client = JitoClientBuilder::new()
            .with_endpoint(wrong_url.clone())
            .build()
            .expect("mock client");

        let status = client
            .get_bundle_status_by_uuid(bundle_uuid, &right_url)
            .await
            .expect("status lookup via submit endpoint");

        assert_eq!(status.bundle_id, bundle_uuid);
        assert_eq!(status.status, BundleState::Rejected);
        assert_eq!(status.reason.as_deref(), Some("regional-host-match"));

        let wrong_captured = wrong_requests
            .lock()
            .expect("wrong request lock")
            .join("\n");
        assert!(
            !wrong_captured.contains("getInflightBundleStatuses"),
            "status polling must not hit the client's default host when submit host is provided"
        );

        let right_captured = right_requests
            .lock()
            .expect("right request lock")
            .join("\n");
        assert!(
            right_captured.contains("POST /api/v1/getInflightBundleStatuses HTTP/1.1"),
            "status polling must hit the submit host's inflight endpoint"
        );
    }

    #[test]
    fn test_get_random_tip_account() {
        // Test that we can get a random tip account
        let tip_account = JitoClient::random_tip_account();
        assert!(tip_account.is_ok());

        // Test multiple calls return valid accounts
        for _ in 0..10 {
            let account = JitoClient::random_tip_account();
            assert!(account.is_ok());
        }
    }

    fn create_dummy_transaction() -> VersionedTransaction {
        use solana_sdk::message::{v0, VersionedMessage};

        let payer = Keypair::new();
        let message =
            v0::Message::try_compile(&payer.pubkey(), &[], &[], solana_sdk::hash::Hash::default())
                .unwrap();

        VersionedTransaction::try_new(VersionedMessage::V0(message), &[&payer]).unwrap()
    }

    fn block_on_test_future<T>(future: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime")
            .block_on(future)
    }

    async fn spawn_mock_json_server(
        responder: Arc<dyn Fn(&str) -> String + Send + Sync + 'static>,
    ) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let addr = listener.local_addr().expect("mock addr");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_task = Arc::clone(&requests);
        let responder_task = Arc::clone(&responder);

        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mut buffer = vec![0u8; 16_384];
                let n = match stream.read(&mut buffer).await {
                    Ok(n) if n > 0 => n,
                    _ => continue,
                };
                let request = String::from_utf8_lossy(&buffer[..n]).to_string();
                requests_task
                    .lock()
                    .expect("mock request lock")
                    .push(request.clone());
                let body = responder_task(&request);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });

        (format!("http://{}", addr), requests)
    }

    async fn spawn_failed_bundle_status_server(
        bundle_uuid: &str,
        reason: &str,
    ) -> (String, Arc<Mutex<Vec<String>>>) {
        let bundle_uuid = bundle_uuid.to_string();
        let reason = reason.to_string();
        spawn_mock_json_server(Arc::new(move |request| {
            if request.contains("getInflightBundleStatuses") {
                format!(
                    "{{\"jsonrpc\":\"2.0\",\"result\":{{\"context\":{{\"slot\":1}},\"value\":[{{\"bundle_id\":\"{}\",\"status\":\"Failed\",\"landed_slot\":null,\"reason\":\"{}\"}}]}},\"id\":1}}",
                    bundle_uuid, reason
                )
            } else if request.contains("\"getVersion\"") {
                "{\"jsonrpc\":\"2.0\",\"result\":{\"solana-core\":\"1.18.26\",\"feature-set\":1},\"id\":1}".to_string()
            } else {
                "{\"jsonrpc\":\"2.0\",\"result\":{\"value\":[]},\"id\":1}".to_string()
            }
        }))
        .await
    }

    async fn spawn_missing_signature_status_rpc_server() -> (String, Arc<Mutex<Vec<String>>>) {
        spawn_mock_json_server(Arc::new(|request| {
            if request.contains("\"getSignatureStatuses\"") {
                "{\"jsonrpc\":\"2.0\",\"result\":{\"context\":{\"slot\":1},\"value\":[null,null]},\"id\":1}".to_string()
            } else if request.contains("\"getVersion\"") {
                "{\"jsonrpc\":\"2.0\",\"result\":{\"solana-core\":\"1.18.26\",\"feature-set\":1},\"id\":1}".to_string()
            } else {
                "{\"jsonrpc\":\"2.0\",\"result\":\"ok\",\"id\":1}".to_string()
            }
        }))
        .await
    }

    // IRONCLAD PROTOCOL TESTS

    #[test]
    fn test_ironclad_constants() {
        // Verify IRONCLAD protocol constants are set correctly
        assert_eq!(MAX_BLOCKHASH_FETCH_MS, 200);
        assert_eq!(BUNDLE_CONFIRMATION_TIMEOUT_MS, 1500);
        assert!(
            FAIL_CLOSED_ONCHAIN_RECONCILIATION_TIMEOUT_MS > BUNDLE_CONFIRMATION_TIMEOUT_MS,
            "fail-closed reconciliation window must outlive the fast Jito confirmation poll"
        );
        assert_eq!(MAX_BUNDLE_RETRIES, 3);
        assert_eq!(MAX_SIMULATION_TIME_MS, 100);
        assert_eq!(MAX_COMPUTE_UNITS_SIMPLE_SWAP, 400_000);
    }

    #[test]
    fn test_jito_client_with_rpc_client() {
        let config = BundleConfig::default();
        let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));

        let client = JitoClient::new("https://test.jito.wtf/api/v1", config)
            .with_rpc_client(rpc_client.clone());

        assert!(client.rpc_client.is_some());
        assert_eq!(client.endpoint, "https://test.jito.wtf/api/v1/bundles");
    }

    #[test]
    fn test_builder_with_rpc_client() {
        let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));

        let client = JitoClientBuilder::new()
            .with_endpoint("https://test.jito.wtf/api/v1")
            .with_rpc_client(rpc_client)
            .with_diagnostics(true)
            .build();

        assert!(client.is_ok());
        let client = client.unwrap();
        assert!(client.rpc_client.is_some());
        assert!(client.bundle_config.enable_diagnostics);
        assert_eq!(client.endpoint, "https://test.jito.wtf/api/v1/bundles");
        assert_eq!(client.sdk_base_url, "https://test.jito.wtf/api/v1");
    }

    #[test]
    fn test_bundle_retry_count() {
        assert_eq!(MAX_BUNDLE_RETRIES, 3);
    }

    #[derive(Clone)]
    struct FakeOnchainStatus {
        slot: u64,
        err: Option<&'static str>,
    }

    impl OnchainBundleStatusView for FakeOnchainStatus {
        fn slot(&self) -> u64 {
            self.slot
        }

        fn err_string(&self) -> Option<String> {
            self.err.map(str::to_string)
        }
    }

    #[test]
    fn test_summarize_onchain_bundle_observation_all_landed() {
        let signatures = vec![Signature::new_unique(), Signature::new_unique()];
        let statuses = vec![
            Some(FakeOnchainStatus {
                slot: 11,
                err: None,
            }),
            Some(FakeOnchainStatus {
                slot: 12,
                err: None,
            }),
        ];

        let observation = summarize_onchain_bundle_observation(&signatures, &statuses);

        assert!(observation.all_landed_successfully());
        assert_eq!(observation.landed_signatures, signatures);
        assert!(observation.missing_signatures.is_empty());
        assert!(observation.failed_signatures.is_empty());
        assert_eq!(observation.landed_slot, Some(12));
        assert_eq!(
            classify_onchain_bundle_observation(&observation),
            OnchainBundleReconciliation::Confirmed(Some(12))
        );
    }

    #[test]
    fn test_summarize_onchain_bundle_observation_detects_partial_landing() {
        let signatures = vec![Signature::new_unique(), Signature::new_unique()];
        let statuses = vec![
            Some(FakeOnchainStatus {
                slot: 42,
                err: None,
            }),
            None,
        ];

        let observation = summarize_onchain_bundle_observation(&signatures, &statuses);

        assert!(!observation.all_landed_successfully());
        assert_eq!(observation.landed_signatures, vec![signatures[0]]);
        assert_eq!(observation.missing_signatures, vec![signatures[1]]);
        assert!(observation.failed_signatures.is_empty());
        assert_eq!(observation.landed_slot, Some(42));
        assert_eq!(
            classify_onchain_bundle_observation(&observation),
            OnchainBundleReconciliation::Uncertain
        );
    }

    #[test]
    fn test_summarize_onchain_bundle_observation_detects_failed_signature() {
        let signatures = vec![Signature::new_unique()];
        let statuses = vec![Some(FakeOnchainStatus {
            slot: 7,
            err: Some("InstructionError"),
        })];

        let observation = summarize_onchain_bundle_observation(&signatures, &statuses);

        assert!(!observation.all_landed_successfully());
        assert!(observation.landed_signatures.is_empty());
        assert!(observation.missing_signatures.is_empty());
        assert_eq!(
            observation.failed_signatures,
            vec![(signatures[0], "InstructionError".to_string())]
        );
        assert_eq!(observation.landed_slot, None);
        assert_eq!(
            classify_onchain_bundle_observation(&observation),
            OnchainBundleReconciliation::Failed
        );
    }

    #[test]
    fn test_classify_bundle_reconciliation_rejected_without_onchain_observation_is_failed() {
        let signatures = vec![Signature::new_unique(), Signature::new_unique()];
        let statuses: Vec<Option<FakeOnchainStatus>> = vec![None, None];

        let observation = summarize_onchain_bundle_observation(&signatures, &statuses);

        assert_eq!(
            classify_onchain_bundle_observation(&observation),
            OnchainBundleReconciliation::Uncertain
        );
        assert_eq!(
            classify_bundle_reconciliation(BundleState::Rejected, &observation),
            OnchainBundleReconciliation::Failed
        );
        assert_eq!(
            classify_bundle_reconciliation(BundleState::Expired, &observation),
            OnchainBundleReconciliation::Failed
        );
        assert_eq!(
            classify_bundle_reconciliation(BundleState::Pending, &observation),
            OnchainBundleReconciliation::Uncertain
        );
    }
}
