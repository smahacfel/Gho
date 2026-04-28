//! IWIM Veto Gate — post-Gatekeeper dev history verification.
//!
//! Acts as an independent "last veto" after the 10s Gatekeeper window completes.
//! Only invoked for candidates that passed Gatekeeper with BUY verdict.
//!
//! ## Architecture
//!
//! ```text
//! Gatekeeper 10s window → BUY verdict
//!     ↓
//! IWIM Veto Gate (300-500ms):
//!   1. Determine policy path (dev_known, gatekeeper_strength)
//!   2. Fetch dev wallet history (primary RPC → fallback RPC)
//!   3. Build IwimInput → iwim_analyze()
//!   4. Apply policy matrix → final verdict
//! ```
//!
//! ## Policy Matrix
//!
//! The policy matrix is deterministic and priority-ordered:
//! - Step 0: Only run for BUY candidates
//! - Step 1: dev_unknown handling (STRONG → BUY, BORDERLINE → REJECT)
//! - Step 2: fetch timeout/error (STRONG → BUY, BORDERLINE → REJECT)
//! - Step 3: iwim_quality LOW/NONE (STRONG → BUY, BORDERLINE → REJECT)
//! - Step 4: iwim_quality HIGH → honour VETO or confirm BUY

use std::time::{Duration, Instant};

use ghost_brain::config::{IwimFeedMode, IwimVetoGateConfig};
use ghost_brain::oracle::ultrafast::iwim::{iwim_analyze, IwimInput, IwimResult};
use seer::new_async_rpc_client;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use tracing::{debug, info, warn};

use super::gatekeeper::GatekeeperStrength;

// =============================================================================
// Types
// =============================================================================

/// IWIM fetch status (RPC interaction result).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IwimFetchStatus {
    /// Successfully fetched dev history within budget.
    Ok,
    /// Exceeded max_wait_ms across all attempts.
    Timeout,
    /// RPC error (parse failure, connection error, etc.).
    Error,
}

impl std::fmt::Display for IwimFetchStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ok => write!(f, "OK"),
            Self::Timeout => write!(f, "TIMEOUT"),
            Self::Error => write!(f, "ERROR"),
        }
    }
}

/// IWIM data quality classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IwimQuality {
    /// confidence >= min_confidence AND n_tx >= min_tx (mode-dependent).
    High,
    /// confidence < min_confidence OR n_tx < min_tx (but > 0).
    Low,
    /// n_tx == 0 (no data fetched or analyzed).
    None,
}

impl std::fmt::Display for IwimQuality {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::High => write!(f, "HIGH"),
            Self::Low => write!(f, "LOW"),
            Self::None => write!(f, "NONE"),
        }
    }
}

/// IWIM veto status (analysis result classification).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IwimStatus {
    /// No veto — dev history looks acceptable.
    Ok,
    /// Veto — dev history shows rug/sybil/scam pattern.
    Veto,
    /// Insufficient data / timeout / confidence too low.
    Unknown,
}

impl std::fmt::Display for IwimStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ok => write!(f, "OK"),
            Self::Veto => write!(f, "VETO"),
            Self::Unknown => write!(f, "UNKNOWN"),
        }
    }
}

/// Reason code for IWIM VETO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IwimVetoReason {
    /// rug_threat_score exceeded threshold.
    RugThreat,
    /// sybil_score exceeded threshold.
    SybilPattern,
    /// organic_score below floor.
    LowOrganic,
    /// Multiple veto conditions triggered.
    Combined(Vec<IwimVetoReason>),
}

impl std::fmt::Display for IwimVetoReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RugThreat => write!(f, "IWIM_RUG_THREAT"),
            Self::SybilPattern => write!(f, "IWIM_SYBIL_PATTERN"),
            Self::LowOrganic => write!(f, "IWIM_LOW_ORGANIC"),
            Self::Combined(reasons) => {
                let tags: Vec<String> = reasons.iter().map(|r| r.to_string()).collect();
                write!(f, "{}", tags.join("+"))
            }
        }
    }
}

/// Complete IWIM Veto Gate result for telemetry.
#[derive(Debug, Clone)]
pub struct IwimVetoResult {
    /// Whether IWIM gate was enabled and ran.
    pub enabled: bool,
    /// Feed mode used (PP/GRPC).
    pub mode: IwimFeedMode,
    /// RPC fetch status.
    pub fetch_status: IwimFetchStatus,
    /// Data quality classification.
    pub quality: IwimQuality,
    /// IWIM analysis confidence (0.0-1.0).
    pub confidence: f32,
    /// Number of TX analyzed by IWIM.
    pub n_tx_analyzed: usize,
    /// Number of TX requested.
    pub n_tx_requested: usize,
    /// Total IWIM latency (fetch + analyze) in ms.
    pub latency_ms: u64,
    /// Which RPC was used: "primary" or "fallback".
    pub rpc_used: String,
    /// IWIM veto status.
    pub status: IwimStatus,
    /// Veto reason code (only if status == Veto).
    pub veto_reason: Option<IwimVetoReason>,
    /// Raw IWIM scores for telemetry.
    pub raw_result: Option<IwimResult>,
    /// Gatekeeper strength used in policy decision.
    pub gatekeeper_strength: GatekeeperStrength,
    /// Was dev known?
    pub dev_known: bool,
}

impl Default for IwimVetoResult {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: IwimFeedMode::Pp,
            fetch_status: IwimFetchStatus::Error,
            quality: IwimQuality::None,
            confidence: 0.0,
            n_tx_analyzed: 0,
            n_tx_requested: 0,
            latency_ms: 0,
            rpc_used: String::new(),
            status: IwimStatus::Unknown,
            veto_reason: None,
            raw_result: None,
            gatekeeper_strength: GatekeeperStrength::Borderline,
            dev_known: false,
        }
    }
}

impl IwimVetoResult {
    /// One-line summary for log output.
    pub fn summary(&self) -> String {
        if !self.enabled {
            return "iwim=OFF".to_string();
        }
        format!(
            "iwim={} q={} fetch={} conf={:.2} n_tx={}/{} lat={}ms rpc={} str={} dev_k={}{}",
            self.status,
            self.quality,
            self.fetch_status,
            self.confidence,
            self.n_tx_analyzed,
            self.n_tx_requested,
            self.latency_ms,
            self.rpc_used,
            self.gatekeeper_strength,
            self.dev_known,
            self.veto_reason
                .as_ref()
                .map_or(String::new(), |r| format!(" reason={}", r)),
        )
    }
}

// =============================================================================
// Core Engine
// =============================================================================

/// Run the complete IWIM Veto Gate policy matrix.
///
/// This is the main entry point called from oracle_runtime after Gatekeeper BUY.
///
/// Returns `(should_buy, iwim_result)`:
/// - `should_buy = true` → proceed with BUY
/// - `should_buy = false` → REJECT (verdict_type from IwimVetoResult)
pub async fn run_iwim_veto_gate(
    config: &IwimVetoGateConfig,
    dev_pubkey: Option<&Pubkey>,
    pool_id: &Pubkey,
    gatekeeper_strength: GatekeeperStrength,
    rpc_client: Option<&Arc<RpcClient>>,
) -> (bool, IwimVetoResult) {
    let mut result = IwimVetoResult {
        enabled: config.enabled,
        mode: config.mode,
        gatekeeper_strength,
        dev_known: dev_pubkey.is_some(),
        ..Default::default()
    };

    if !config.enabled {
        result.status = IwimStatus::Ok;
        return (true, result);
    }

    let start = Instant::now();

    // ─── Step 1: dev_unknown handling ────────────────────────────────────
    let dev_key = match dev_pubkey {
        Some(key) => *key,
        None => {
            // dev_unknown
            result.status = IwimStatus::Unknown;
            result.latency_ms = start.elapsed().as_millis() as u64;

            match gatekeeper_strength {
                GatekeeperStrength::Strong => {
                    // STRONG + dev_unknown → BUY (already protected by dev_unknown strict soft cap)
                    info!(
                        pool = %pool_id,
                        "IWIM_VETO: dev_unknown + STRONG → BUY (skip IWIM, dev_unknown strict active)"
                    );
                    return (true, result);
                }
                GatekeeperStrength::Borderline => {
                    // BORDERLINE + dev_unknown → REJECT (highest risk vector)
                    info!(
                        pool = %pool_id,
                        "IWIM_VETO: dev_unknown + BORDERLINE → REJECT (DEV_UNKNOWN_BORDERLINE)"
                    );
                    result.veto_reason = Some(IwimVetoReason::LowOrganic);
                    return (false, result);
                }
            }
        }
    };

    // ─── Step 2: Fetch dev wallet history ────────────────────────────────
    let n_tx_requested = match config.mode {
        IwimFeedMode::Pp => config.min_tx_pp.max(30),
        IwimFeedMode::Grpc => config.min_tx_grpc.max(150),
    };
    result.n_tx_requested = n_tx_requested;

    let total_budget = Duration::from_millis(config.max_wait_ms);

    let mut rpc_attempts: Vec<(&str, Arc<RpcClient>)> = Vec::new();
    if !config.primary_rpc_url.is_empty() {
        rpc_attempts.push((
            "primary",
            Arc::new(new_async_rpc_client(config.primary_rpc_url.clone())),
        ));
    }
    if !config.fallback_rpc_url.is_empty() {
        rpc_attempts.push((
            "fallback",
            Arc::new(new_async_rpc_client(config.fallback_rpc_url.clone())),
        ));
    }
    if let Some(shared_rpc) = rpc_client.cloned() {
        rpc_attempts.push(("runtime", shared_rpc));
    }

    if rpc_attempts.is_empty() {
        result.fetch_status = IwimFetchStatus::Error;
        result.status = IwimStatus::Unknown;
        result.latency_ms = start.elapsed().as_millis() as u64;
        return apply_policy_timeout(pool_id, gatekeeper_strength, &mut result);
    }

    let mut saw_timeout = false;
    let mut saw_empty = false;
    let mut raw_tx_data: Option<Vec<Vec<u8>>> = None;

    for (label, rpc) in rpc_attempts {
        let remaining_budget = total_budget.saturating_sub(start.elapsed());
        if remaining_budget.is_zero() {
            saw_timeout = true;
            break;
        }

        result.rpc_used = label.to_string();
        match fetch_dev_signatures(&rpc, &dev_key, n_tx_requested, remaining_budget).await {
            Ok(data) if !data.is_empty() => {
                result.fetch_status = IwimFetchStatus::Ok;
                raw_tx_data = Some(data);
                break;
            }
            Ok(_empty) => {
                saw_empty = true;
                debug!(
                    pool = %pool_id,
                    rpc = label,
                    "IWIM_VETO: no dev history entries from RPC attempt"
                );
            }
            Err(e) => {
                if e.to_ascii_lowercase().contains("timeout") {
                    saw_timeout = true;
                }
                warn!(
                    pool = %pool_id,
                    rpc = label,
                    "IWIM_VETO: RPC attempt failed: {}",
                    e
                );
            }
        }
    }

    let raw_tx_data = match raw_tx_data {
        Some(data) => data,
        None if saw_empty => {
            result.fetch_status = IwimFetchStatus::Ok;
            result.n_tx_analyzed = 0;
            result.quality = IwimQuality::None;
            result.status = IwimStatus::Unknown;
            result.latency_ms = start.elapsed().as_millis() as u64;
            return apply_policy_no_data(pool_id, gatekeeper_strength, &mut result);
        }
        None if saw_timeout => {
            result.fetch_status = IwimFetchStatus::Timeout;
            result.status = IwimStatus::Unknown;
            result.latency_ms = start.elapsed().as_millis() as u64;
            return apply_policy_timeout(pool_id, gatekeeper_strength, &mut result);
        }
        None => {
            result.fetch_status = IwimFetchStatus::Error;
            result.status = IwimStatus::Unknown;
            result.latency_ms = start.elapsed().as_millis() as u64;
            return apply_policy_timeout(pool_id, gatekeeper_strength, &mut result);
        }
    };

    // ─── Step 3: Build IwimInput and analyze ─────────────────────────────
    let n_tx_fetched = raw_tx_data.len();

    // Build IwimInput — transactions are encoded as raw bytes.
    // From `getSignaturesForAddress` we get signatures + optional data.
    // We encode each signature as raw bytes for IWIM's classifier.
    let input = IwimInput {
        creator_pubkey: dev_key.to_bytes(),
        init_slot: None,      // we don't have the init slot in this context
        time_window_ms: 2000, // standard IWIM window
        transactions: raw_tx_data,
        init_timestamp_ms: None,
        synthetic: false,
        pool_id: Some(pool_id.to_string()),
    };

    let iwim_result = match iwim_analyze(&input) {
        Ok(r) => r,
        Err(e) => {
            warn!(
                pool = %pool_id,
                "IWIM_VETO: iwim_analyze error: {}",
                e
            );
            result.fetch_status = IwimFetchStatus::Error;
            result.status = IwimStatus::Unknown;
            result.latency_ms = start.elapsed().as_millis() as u64;
            return apply_policy_timeout(pool_id, gatekeeper_strength, &mut result);
        }
    };

    result.confidence = iwim_result.confidence;
    result.n_tx_analyzed = n_tx_fetched;
    result.raw_result = Some(iwim_result);
    result.latency_ms = start.elapsed().as_millis() as u64;

    // ─── Step 3b: Classify quality ───────────────────────────────────────
    let min_tx = match config.mode {
        IwimFeedMode::Pp => config.min_tx_pp,
        IwimFeedMode::Grpc => config.min_tx_grpc,
    };

    result.quality = if n_tx_fetched == 0 {
        IwimQuality::None
    } else if iwim_result.confidence >= config.min_confidence && n_tx_fetched >= min_tx {
        IwimQuality::High
    } else {
        IwimQuality::Low
    };

    // ─── Step 4: Apply policy matrix ─────────────────────────────────────

    match result.quality {
        IwimQuality::None => {
            result.status = IwimStatus::Unknown;
            return apply_policy_no_data(pool_id, gatekeeper_strength, &mut result);
        }
        IwimQuality::Low => {
            result.status = IwimStatus::Unknown;
            match gatekeeper_strength {
                GatekeeperStrength::Strong => {
                    info!(
                        pool = %pool_id,
                        "IWIM_VETO: LOW quality + STRONG → BUY (log iwim_quality=LOW)"
                    );
                    result.status = IwimStatus::Ok;
                    return (true, result);
                }
                GatekeeperStrength::Borderline => {
                    info!(
                        pool = %pool_id,
                        conf = iwim_result.confidence,
                        n_tx = n_tx_fetched,
                        "IWIM_VETO: LOW quality + BORDERLINE → REJECT"
                    );
                    return (false, result);
                }
            }
        }
        IwimQuality::High => {
            // Check for VETO conditions
            let mut veto_reasons: Vec<IwimVetoReason> = Vec::new();

            if iwim_result.rug_threat_score >= config.rug_threat_threshold {
                veto_reasons.push(IwimVetoReason::RugThreat);
            }
            if iwim_result.sybil_score >= config.sybil_threshold {
                veto_reasons.push(IwimVetoReason::SybilPattern);
            }
            if iwim_result.organic_score <= config.organic_floor {
                veto_reasons.push(IwimVetoReason::LowOrganic);
            }

            if !veto_reasons.is_empty() {
                let reason = if veto_reasons.len() == 1 {
                    veto_reasons.into_iter().next().unwrap()
                } else {
                    IwimVetoReason::Combined(veto_reasons)
                };

                info!(
                    pool = %pool_id,
                    rug = iwim_result.rug_threat_score,
                    sybil = iwim_result.sybil_score,
                    organic = iwim_result.organic_score,
                    conf = iwim_result.confidence,
                    reason = %reason,
                    "IWIM_VETO: HIGH quality → VETO ({})",
                    reason
                );

                result.status = IwimStatus::Veto;
                result.veto_reason = Some(reason);
                return (false, result);
            }

            // No veto conditions → BUY confirmed
            result.status = IwimStatus::Ok;
            info!(
                pool = %pool_id,
                rug = iwim_result.rug_threat_score,
                sybil = iwim_result.sybil_score,
                organic = iwim_result.organic_score,
                conf = iwim_result.confidence,
                "IWIM_VETO: HIGH quality → OK (BUY confirmed)"
            );
            return (true, result);
        }
    }
}

// =============================================================================
// Policy Helpers
// =============================================================================

/// Policy for timeout/error: STRONG → BUY, BORDERLINE → REJECT.
fn apply_policy_timeout(
    pool_id: &Pubkey,
    strength: GatekeeperStrength,
    result: &mut IwimVetoResult,
) -> (bool, IwimVetoResult) {
    match strength {
        GatekeeperStrength::Strong => {
            info!(
                pool = %pool_id,
                "IWIM_VETO: TIMEOUT/ERROR + STRONG → BUY (log iwim_status=UNKNOWN)"
            );
            result.status = IwimStatus::Unknown;
            (true, result.clone())
        }
        GatekeeperStrength::Borderline => {
            info!(
                pool = %pool_id,
                "IWIM_VETO: TIMEOUT/ERROR + BORDERLINE → REJECT (no bypass on timeout)"
            );
            result.status = IwimStatus::Unknown;
            (false, result.clone())
        }
    }
}

/// Policy for no data: same as timeout.
fn apply_policy_no_data(
    pool_id: &Pubkey,
    strength: GatekeeperStrength,
    result: &mut IwimVetoResult,
) -> (bool, IwimVetoResult) {
    apply_policy_timeout(pool_id, strength, result)
}

// =============================================================================
// RPC Fetch Layer
// =============================================================================

/// Fetch dev wallet's recent transaction signatures within a time budget.
///
/// Returns transaction placeholders containing real `block_time` when available.
/// Entries are ordered oldest→newest so IWIM sees a chronological sequence.
async fn fetch_dev_signatures(
    rpc: &RpcClient,
    dev_pubkey: &Pubkey,
    limit: usize,
    timeout: Duration,
) -> Result<Vec<Vec<u8>>, String> {
    use solana_client::rpc_client::GetConfirmedSignaturesForAddress2Config;

    let config = GetConfirmedSignaturesForAddress2Config {
        limit: Some(limit),
        ..Default::default()
    };

    let sigs = tokio::time::timeout(
        timeout,
        rpc.get_signatures_for_address_with_config(dev_pubkey, config),
    )
    .await
    .map_err(|_| "RPC timeout".to_string())?
    .map_err(|e| format!("RPC error: {}", e))?;

    // IWIM expects a chronological sequence. RPC returns newest-first.
    let tx_data: Vec<Vec<u8>> = sigs
        .iter()
        .rev()
        .filter_map(|sig_info| {
            let block_time = sig_info.block_time.unwrap_or(0);
            let placeholder = format!("JSON_TX_META_TIMESTAMP_{}", block_time);
            Some(placeholder.into_bytes())
        })
        .collect();

    Ok(tx_data)
}
