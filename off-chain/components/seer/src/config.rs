//! Configuration for Seer module
//!
//! This module defines configuration options for the Seer component.

use crate::ipc::IpcChannelConfig;
use ghost_core::CurveFinality;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

/// Seer configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeerConfig {
    /// Connection mode: "websocket" or "grpc" (deprecated, use source_mode)
    pub connection_mode: ConnectionMode,

    /// Source mode: determines event source format (overrides connection_mode if set)
    pub source_mode: Option<SeerSourceMode>,

    /// Geyser/WebSocket RPC endpoint (used when connection_mode is WebSocket)
    pub geyser_endpoint: String,

    /// Yellowstone gRPC endpoint (used when connection_mode is gRPC)
    pub grpc_endpoint: String,

    /// Helius WebSocket endpoint (optional, for HeliusWebSocket mode)
    pub helius_endpoint: Option<String>,

    /// Fallback RPC endpoint for transaction fetching
    pub rpc_endpoint: String,

    /// Enable Yellowstone slot-gap RPC fallback via getTransaction.
    ///
    /// When disabled, gRPC slot gaps are logged but no manual RPC backfill
    /// worker is spawned.
    #[serde(default = "SeerConfig::default_grpc_manual_backfill_enabled")]
    pub grpc_manual_backfill_enabled: bool,

    /// gRPC client ID (optional, for identifying this client)
    pub grpc_client_id: Option<String>,

    /// gRPC authentication token (optional, for authenticated endpoints)
    pub grpc_auth_token: Option<String>,

    /// gRPC authentication metadata header name.
    #[serde(default = "SeerConfig::default_grpc_auth_header")]
    pub grpc_auth_header: String,

    /// Maximum reconnection attempts
    pub max_reconnect_attempts: u32,

    /// Initial delay between reconnection attempts (seconds) - used for exponential backoff
    pub reconnect_delay_secs: u64,

    /// Maximum delay between reconnection attempts (seconds) - caps exponential backoff
    pub max_reconnect_delay_secs: u64,

    /// Consecutive gRPC stalls required before a provider circuit opens.
    #[serde(default = "SeerConfig::default_grpc_max_stalls_before_open")]
    pub grpc_max_stalls_before_open: u32,

    /// Seconds without any gRPC message before the stream is treated as stalled.
    #[serde(default = "SeerConfig::default_grpc_stall_timeout_secs")]
    pub grpc_stall_timeout_secs: u64,

    /// Cooldown before an open provider circuit performs a half-open probe.
    #[serde(default = "SeerConfig::default_grpc_circuit_breaker_cooldown_ms")]
    pub grpc_circuit_breaker_cooldown_ms: u64,

    /// Enable verbose logging
    pub verbose: bool,

    /// Filter configuration
    pub filter: FilterConfig,

    /// Channel buffer size for candidate forwarding (deprecated, use ipc_config)
    pub channel_buffer_size: usize,

    /// IPC channel configuration for Seer→Trigger communication
    pub ipc_config: IpcChannelConfig,

    /// Prometheus metrics port
    pub metrics_port: u16,

    /// Queue utilization threshold (%) to enter ultrafast degraded mode
    pub ultrafast_enter_threshold: f64,

    /// Queue utilization threshold (%) to exit ultrafast degraded mode
    pub ultrafast_exit_threshold: f64,

    /// Commitment level for event sources (processed/mempool/confirmed/finalized)
    #[serde(default)]
    pub commitment: CommitmentLevel,

    /// Opt-in fallback to WebSocket if gRPC commitment is unsupported or fails.
    ///
    /// Disabled by default in production hardening mode.
    #[serde(default = "SeerConfig::default_grpc_fallback")]
    pub grpc_commitment_fallback_to_websocket: bool,

    /// PumpPortal WebSocket configuration
    #[serde(default)]
    pub pumpportal: PumpPortalConfig,

    /// Stream architecture mode for gRPC ingestion.
    #[serde(default)]
    pub stream_mode: StreamMode,

    /// Trade forwarding filter strategy.
    #[serde(default)]
    pub tx_filter_strategy: TxFilterStrategy,

    /// Optional dedicated funding-transfer ingest lane for FSC unlock rollout.
    ///
    /// Default is fail-closed: no extra funding lane is started and the existing
    /// `grpc_global_stream` contract remains filtered-only.
    #[serde(default)]
    pub funding_lane_mode: FundingLaneMode,

    /// Optional NLN Program Streams semantic event lane.
    ///
    /// PR-FSC1 only adds the inert config surface. No Program Streams client is
    /// started until a later implementation phase explicitly wires it.
    #[serde(default)]
    pub program_streams: ProgramStreamsConfig,

    /// TTL for watched pools in milliseconds (single_global mode).
    #[serde(default = "SeerConfig::default_watched_pools_ttl_ms")]
    pub watched_pools_ttl_ms: u64,

    /// Maximum number of watched pools retained in memory.
    #[serde(default = "SeerConfig::default_watched_pools_cap")]
    pub watched_pools_cap: usize,

    /// Debounce for repeated watch registrations (reserved for pooled_filtered mode).
    #[serde(default)]
    pub watch_debounce_ms: u64,

    /// Compatibility flag for the downstream canonical `AccountUpdate` relay.
    ///
    /// Production launcher startup derives the effective canonical ingest path
    /// from `AccountStateCore` enablement and does **not** treat this field as a
    /// primary production selector.
    ///
    /// `true` keeps the canonical relay active for direct/degraded harnesses.
    /// `false` is reserved for explicit degraded/test startup only.
    #[serde(default)]
    pub canonical_account_update_relay_enabled: bool,
}

/// Connection mode for Seer
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionMode {
    /// Use WebSocket/Geyser for event streaming
    WebSocket,
    /// Use Yellowstone gRPC for event streaming (recommended for mempool filtering)
    Grpc,
}

/// Source mode for Seer - determines how events are received
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SeerSourceMode {
    /// Geyser plugin via gRPC (Yellowstone) - for production/HFT
    GeyserGrpc,
    /// Geyser plugin via WebSocket - legacy mode
    GeyserWebSocket,
    /// Standard Helius/Solana RPC WebSocket - for testing/dry-run
    HeliusWebSocket,
    /// PumpPortal WebSocket - real-time Pump.fun data ingestion
    PumpPortalWs,
}

/// Stream architecture mode for gRPC ingestion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StreamMode {
    /// Exactly one active gRPC stream per process (SSOT mode).
    SingleGlobal,
    /// Optional future mode: pool-filtered stream orchestration.
    PooledFiltered,
}

impl Default for StreamMode {
    fn default() -> Self {
        StreamMode::SingleGlobal
    }
}

/// Trade forwarding filter strategy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TxFilterStrategy {
    /// Forward only trades from currently watched pools.
    PerPool,
    /// Forward all parsed trades.
    All,
}

impl Default for TxFilterStrategy {
    fn default() -> Self {
        TxFilterStrategy::PerPool
    }
}

/// Dedicated funding-transfer lane mode.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FundingLaneMode {
    /// Fail-closed default: keep only the legacy filtered `grpc_global_stream`.
    Disabled,
    /// Dedicated filtered funding lane scoped to Pump/PumpSwap transactions.
    PumpFiltered,
    /// Dedicated authoritative full-chain funding lane.
    FullChain,
}

impl Default for FundingLaneMode {
    fn default() -> Self {
        FundingLaneMode::Disabled
    }
}

impl FundingLaneMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            FundingLaneMode::Disabled => "disabled",
            FundingLaneMode::PumpFiltered => "pump_filtered",
            FundingLaneMode::FullChain => "full_chain",
        }
    }
}

/// Payload encoding requested from NLN Program Streams.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProgramStreamPayloadFormat {
    /// Base64-wrapped JSON payloads inside SubscribeResponse.
    #[serde(rename = "JSON", alias = "json")]
    Json,
}

impl Default for ProgramStreamPayloadFormat {
    fn default() -> Self {
        ProgramStreamPayloadFormat::Json
    }
}

impl ProgramStreamPayloadFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            ProgramStreamPayloadFormat::Json => "JSON",
        }
    }
}

/// Provider quota behavior for Program Streams subscription selection.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProgramStreamsQuotaPolicy {
    /// Preserve legacy behavior: selected optional topics may be dropped under
    /// provider stream limits.
    DropOptional,
    /// Fail before opening any stream if the configured topic set exceeds the
    /// provider quota.
    FailFast,
}

impl Default for ProgramStreamsQuotaPolicy {
    fn default() -> Self {
        Self::DropOptional
    }
}

/// NLN Program Streams configuration for FSC v2 capture/evidence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProgramStreamsConfig {
    /// Start the Program Streams lane. Defaults off until PR-FSC2 wires a client.
    #[serde(default)]
    pub enabled: bool,

    /// NLN Program Streams endpoint.
    #[serde(default = "ProgramStreamsConfig::default_endpoint")]
    pub endpoint: String,

    /// Metadata header used for NLN API key authentication.
    #[serde(default = "ProgramStreamsConfig::default_auth_header")]
    pub auth_header: String,

    /// Primary environment variable that contains the NLN API key.
    #[serde(default = "ProgramStreamsConfig::default_api_key_env")]
    pub api_key_env: String,

    /// Optional fallback environment variable for deployments using Ghost naming.
    #[serde(default = "ProgramStreamsConfig::default_api_key_env_fallback")]
    pub api_key_env_fallback: Option<String>,

    /// Optional NLN Event Streams policy metadata value. Runtime evidence
    /// profiles should restrict this to the explicitly enabled topics.
    #[serde(default)]
    pub eventstream_policy_header: Option<String>,

    /// Payload format requested from the Program Streams Subscribe API.
    #[serde(default)]
    pub format: ProgramStreamPayloadFormat,

    /// Maximum concurrent NLN Program Streams subscriptions.
    ///
    /// Default preserves the legacy three-topic capture lane. Rollout profiles
    /// with provider limits can lower this and rely on optional-topic dropping.
    #[serde(default = "ProgramStreamsConfig::default_max_streams")]
    pub max_streams: usize,

    /// How provider stream quotas should be enforced.
    #[serde(default)]
    pub quota_policy: ProgramStreamsQuotaPolicy,

    /// Explicit topic allowlist. When empty, the legacy topic fields below are
    /// used. When non-empty, only matching configured topics are subscribed.
    #[serde(default)]
    pub enabled_topics: Vec<String>,

    /// Optional topics known to this profile. These are never required for FSC
    /// capture and are useful for fail-fast quota audits.
    #[serde(default)]
    pub optional_topics: Vec<String>,

    /// Optional topics that must not be subscribed in the current profile.
    #[serde(default)]
    pub disabled_optional_topics: Vec<String>,

    /// Topics intentionally disabled in the current profile, including topics
    /// that are not optional in legacy profiles.
    #[serde(default)]
    pub disabled_streams: Vec<String>,

    /// Pump.fun create topic used for candidate birth artifacts.
    #[serde(default = "ProgramStreamsConfig::default_pumpfun_create_topic")]
    pub pumpfun_create_topic: String,

    /// Pump.fun trade topic used for early buyer flow.
    #[serde(default = "ProgramStreamsConfig::default_pumpfun_trade_topic")]
    pub pumpfun_trade_topic: String,

    /// Pump.fun decoded buy Program Stream used only as route evidence capture.
    #[serde(default = "ProgramStreamsConfig::default_pumpfun_buy_topic")]
    pub pumpfun_buy_topic: String,

    /// Pump.fun decoded buy_exact_sol_in Program Stream used only as route
    /// evidence capture.
    #[serde(default = "ProgramStreamsConfig::default_pumpfun_buy_exact_sol_in_topic")]
    pub pumpfun_buy_exact_sol_in_topic: String,

    /// Native SOL transfer topic used for FSC v2 funding index capture.
    #[serde(default = "ProgramStreamsConfig::default_system_transfers_topic")]
    pub system_transfers_topic: String,

    /// Optional artifact directory propagated by launch profiles for evidence
    /// rows that need a durable run scope label.
    #[serde(default)]
    pub artifact_capture_dir: Option<String>,

    /// TTL for NLN trade rows waiting for Ghost birth-lane pool identity.
    #[serde(default = "ProgramStreamsConfig::default_trade_resolver_ttl_ms")]
    pub trade_resolver_ttl_ms: u64,

    /// Per-mint cap for unresolved NLN trade buffering.
    #[serde(default = "ProgramStreamsConfig::default_trade_resolver_per_mint_cap")]
    pub trade_resolver_per_mint_cap: usize,

    /// Global cap for unresolved NLN trade buffering.
    #[serde(default = "ProgramStreamsConfig::default_trade_resolver_global_cap")]
    pub trade_resolver_global_cap: usize,

    /// TTL for NLN trade dedupe keys.
    #[serde(default = "ProgramStreamsConfig::default_trade_dedupe_ttl_ms")]
    pub trade_dedupe_ttl_ms: u64,

    /// Maximum retained NLN trade dedupe keys.
    #[serde(default = "ProgramStreamsConfig::default_trade_dedupe_max_entries")]
    pub trade_dedupe_max_entries: usize,

    /// TTL for NLN system transfer dedupe keys.
    #[serde(default = "ProgramStreamsConfig::default_transfer_dedupe_ttl_ms")]
    pub transfer_dedupe_ttl_ms: u64,

    /// Maximum retained NLN system transfer dedupe keys.
    #[serde(default = "ProgramStreamsConfig::default_transfer_dedupe_max_entries")]
    pub transfer_dedupe_max_entries: usize,
}

impl Default for ProgramStreamsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: Self::default_endpoint(),
            auth_header: Self::default_auth_header(),
            api_key_env: Self::default_api_key_env(),
            api_key_env_fallback: Self::default_api_key_env_fallback(),
            eventstream_policy_header: None,
            format: ProgramStreamPayloadFormat::default(),
            max_streams: Self::default_max_streams(),
            quota_policy: ProgramStreamsQuotaPolicy::default(),
            enabled_topics: Vec::new(),
            optional_topics: Vec::new(),
            disabled_optional_topics: Vec::new(),
            disabled_streams: Vec::new(),
            pumpfun_create_topic: Self::default_pumpfun_create_topic(),
            pumpfun_trade_topic: Self::default_pumpfun_trade_topic(),
            pumpfun_buy_topic: Self::default_pumpfun_buy_topic(),
            pumpfun_buy_exact_sol_in_topic: Self::default_pumpfun_buy_exact_sol_in_topic(),
            system_transfers_topic: Self::default_system_transfers_topic(),
            artifact_capture_dir: None,
            trade_resolver_ttl_ms: Self::default_trade_resolver_ttl_ms(),
            trade_resolver_per_mint_cap: Self::default_trade_resolver_per_mint_cap(),
            trade_resolver_global_cap: Self::default_trade_resolver_global_cap(),
            trade_dedupe_ttl_ms: Self::default_trade_dedupe_ttl_ms(),
            trade_dedupe_max_entries: Self::default_trade_dedupe_max_entries(),
            transfer_dedupe_ttl_ms: Self::default_transfer_dedupe_ttl_ms(),
            transfer_dedupe_max_entries: Self::default_transfer_dedupe_max_entries(),
        }
    }
}

impl ProgramStreamsConfig {
    pub fn default_endpoint() -> String {
        "stream-1.nln.clr3.org:443".to_string()
    }

    pub fn default_auth_header() -> String {
        "x-api-key".to_string()
    }

    pub fn default_api_key_env() -> String {
        "NLN_API_KEY".to_string()
    }

    pub fn default_api_key_env_fallback() -> Option<String> {
        Some("GHOST_NLN_API_KEY".to_string())
    }

    pub const fn default_max_streams() -> usize {
        3
    }

    pub fn default_pumpfun_create_topic() -> String {
        "prod.rpc.solana.pumpfun.create".to_string()
    }

    pub fn default_pumpfun_trade_topic() -> String {
        "prod.rpc.solana.pumpfun.trade".to_string()
    }

    pub fn default_pumpfun_buy_topic() -> String {
        "solana.pump_fun.buy".to_string()
    }

    pub fn default_pumpfun_buy_exact_sol_in_topic() -> String {
        "solana.pump_fun.buy_exact_sol_in".to_string()
    }

    pub fn default_system_transfers_topic() -> String {
        "prod.rpc.solana.system.transfers".to_string()
    }

    pub const fn default_trade_resolver_ttl_ms() -> u64 {
        30_000
    }

    pub const fn default_trade_resolver_per_mint_cap() -> usize {
        256
    }

    pub const fn default_trade_resolver_global_cap() -> usize {
        50_000
    }

    pub const fn default_trade_dedupe_ttl_ms() -> u64 {
        300_000
    }

    pub const fn default_trade_dedupe_max_entries() -> usize {
        250_000
    }

    pub const fn default_transfer_dedupe_ttl_ms() -> u64 {
        300_000
    }

    pub const fn default_transfer_dedupe_max_entries() -> usize {
        500_000
    }
}

/// Commitment level configuration for Seer connections
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CommitmentLevel {
    /// Earliest available (processed, equivalent to mempool/processed)
    #[serde(alias = "processed")]
    Mempool,
    /// Confirmed commitment
    Confirmed,
    /// Finalized commitment
    Finalized,
}

impl Default for CommitmentLevel {
    fn default() -> Self {
        CommitmentLevel::Mempool
    }
}

impl CommitmentLevel {
    /// Map commitment to Geyser/Yellowstone numeric value
    pub fn to_geyser_value(&self) -> i32 {
        match self {
            CommitmentLevel::Mempool => 0,   // processed/mempool
            CommitmentLevel::Confirmed => 1, // confirmed
            CommitmentLevel::Finalized => 2, // finalized
        }
    }

    /// Human-readable label for logging/metrics
    pub fn as_str(&self) -> &'static str {
        match self {
            CommitmentLevel::Mempool => "mempool",
            CommitmentLevel::Confirmed => "confirmed",
            CommitmentLevel::Finalized => "finalized",
        }
    }

    /// Map Seer connection commitment into ShadowLedger curve finality.
    pub const fn curve_finality(&self) -> CurveFinality {
        match self {
            CommitmentLevel::Mempool => CurveFinality::Speculative,
            CommitmentLevel::Confirmed => CurveFinality::Provisional,
            CommitmentLevel::Finalized => CurveFinality::Finalized,
        }
    }
}

impl Default for SeerConfig {
    fn default() -> Self {
        Self {
            connection_mode: ConnectionMode::Grpc, // Default to gRPC for better performance
            source_mode: None,                     // Derive from connection_mode if not set
            geyser_endpoint: "wss://api.mainnet-beta.solana.com".to_string(),
            grpc_endpoint: "http://grpc.mainnet.solana.com:10000".to_string(),
            helius_endpoint: None,
            rpc_endpoint: "https://api.mainnet-beta.solana.com".to_string(),
            grpc_manual_backfill_enabled: Self::default_grpc_manual_backfill_enabled(),
            grpc_client_id: None,
            grpc_auth_token: None,
            grpc_auth_header: Self::default_grpc_auth_header(),
            max_reconnect_attempts: 10,
            reconnect_delay_secs: 5,
            max_reconnect_delay_secs: 300, // 5 minutes max backoff
            grpc_max_stalls_before_open: Self::default_grpc_max_stalls_before_open(),
            grpc_stall_timeout_secs: Self::default_grpc_stall_timeout_secs(),
            grpc_circuit_breaker_cooldown_ms: Self::default_grpc_circuit_breaker_cooldown_ms(),
            verbose: false,
            filter: FilterConfig::default(),
            channel_buffer_size: 1000, // Kept for backward compatibility
            ipc_config: IpcChannelConfig::default(),
            metrics_port: 9090,
            ultrafast_enter_threshold: 80.0,
            ultrafast_exit_threshold: 50.0,
            commitment: CommitmentLevel::default(),
            grpc_commitment_fallback_to_websocket: Self::default_grpc_fallback(),
            pumpportal: PumpPortalConfig::default(),
            stream_mode: StreamMode::default(),
            tx_filter_strategy: TxFilterStrategy::default(),
            funding_lane_mode: FundingLaneMode::default(),
            program_streams: ProgramStreamsConfig::default(),
            watched_pools_ttl_ms: Self::default_watched_pools_ttl_ms(),
            watched_pools_cap: Self::default_watched_pools_cap(),
            watch_debounce_ms: 0,
            canonical_account_update_relay_enabled: true,
        }
    }
}

impl SeerConfig {
    /// Get the effective source mode, deriving from connection_mode if source_mode is None
    pub fn effective_source_mode(&self) -> SeerSourceMode {
        match &self.source_mode {
            Some(mode) => mode.clone(),
            None => {
                // Backward compatibility: derive from connection_mode
                match self.connection_mode {
                    ConnectionMode::WebSocket => SeerSourceMode::GeyserWebSocket,
                    ConnectionMode::Grpc => SeerSourceMode::GeyserGrpc,
                }
            }
        }
    }

    fn default_grpc_fallback() -> bool {
        false
    }

    fn default_grpc_manual_backfill_enabled() -> bool {
        true
    }

    pub fn default_grpc_auth_header() -> String {
        "x-token".to_string()
    }

    pub fn default_grpc_max_stalls_before_open() -> u32 {
        3
    }

    pub fn default_grpc_stall_timeout_secs() -> u64 {
        20
    }

    pub fn default_grpc_circuit_breaker_cooldown_ms() -> u64 {
        15_000
    }

    fn default_watched_pools_ttl_ms() -> u64 {
        120_000
    }

    fn default_watched_pools_cap() -> usize {
        32_768
    }
}

/// PumpPortal WebSocket configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PumpPortalConfig {
    /// PumpPortal WebSocket endpoint URL
    #[serde(default = "PumpPortalConfig::default_ws_url")]
    pub ws_url: String,

    /// Maximum number of active mints to track simultaneously
    #[serde(default = "PumpPortalConfig::default_max_active_mints")]
    pub max_active_mints: usize,

    /// Batch size for subscription requests (to avoid rate limiting)
    #[serde(default = "PumpPortalConfig::default_subscription_batch_size")]
    pub subscription_batch_size: usize,

    /// Base delay between reconnection attempts (seconds)
    #[serde(default = "PumpPortalConfig::default_reconnect_base_delay")]
    pub reconnect_base_delay_secs: u64,

    /// Maximum delay between reconnection attempts (seconds)
    #[serde(default = "PumpPortalConfig::default_reconnect_max_delay")]
    pub reconnect_max_delay_secs: u64,

    /// Time window for tracking stats per mint (seconds)
    #[serde(default = "PumpPortalConfig::default_stats_window_secs")]
    pub stats_window_secs: u64,
}

impl Default for PumpPortalConfig {
    fn default() -> Self {
        Self {
            ws_url: Self::default_ws_url(),
            max_active_mints: Self::default_max_active_mints(),
            subscription_batch_size: Self::default_subscription_batch_size(),
            reconnect_base_delay_secs: Self::default_reconnect_base_delay(),
            reconnect_max_delay_secs: Self::default_reconnect_max_delay(),
            stats_window_secs: Self::default_stats_window_secs(),
        }
    }
}

impl PumpPortalConfig {
    fn default_ws_url() -> String {
        "wss://pumpportal.fun/api/data".to_string()
    }

    fn default_max_active_mints() -> usize {
        1_000
    }

    fn default_subscription_batch_size() -> usize {
        10
    }

    fn default_reconnect_base_delay() -> u64 {
        5
    }

    fn default_reconnect_max_delay() -> u64 {
        300
    }

    fn default_stats_window_secs() -> u64 {
        900 // 15 minutes
    }
}

/// Filter configuration for event processing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterConfig {
    /// Enable Pump.fun pool detection
    pub enable_pumpfun: bool,

    /// Enable Bonk.fun pool detection
    pub enable_bonkfun: bool,

    /// Optional: Filter by specific quote mints (SOL, USDC, BONK)
    /// If empty, all quote mints are accepted
    pub allowed_quote_mints: Vec<String>,

    /// Optional: Minimum initial liquidity in SOL
    /// Pools with less initial liquidity will be filtered out
    pub min_initial_liquidity_sol: Option<f64>,
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            enable_pumpfun: true,
            enable_bonkfun: true,
            allowed_quote_mints: vec![
                // SOL
                "So11111111111111111111111111111111111111112".to_string(),
                // USDC
                "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(),
                // BONK
                "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263".to_string(),
            ],
            min_initial_liquidity_sol: None, // No minimum by default
        }
    }
}

impl FilterConfig {
    /// Check if a quote mint is allowed
    pub fn is_quote_mint_allowed(&self, mint: &Pubkey) -> bool {
        if self.allowed_quote_mints.is_empty() {
            return true; // Allow all if no filter specified
        }

        self.allowed_quote_mints
            .iter()
            .any(|allowed| Pubkey::from_str(allowed).ok().as_ref() == Some(mint))
    }

    /// Check if initial liquidity meets minimum requirement
    pub fn meets_liquidity_requirement(&self, liquidity_sol: Option<f64>) -> bool {
        match (self.min_initial_liquidity_sol, liquidity_sol) {
            (Some(min), Some(actual)) => actual >= min,
            (Some(_), None) => false, // Has requirement but no liquidity data
            (None, _) => true,        // No requirement
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SeerConfig::default();
        assert_eq!(config.connection_mode, ConnectionMode::Grpc);
        assert!(config.filter.enable_pumpfun);
        assert!(config.filter.enable_bonkfun);
        assert_eq!(config.filter.allowed_quote_mints.len(), 3);
        assert_eq!(config.commitment, CommitmentLevel::Mempool);
        assert!(!config.grpc_commitment_fallback_to_websocket);
        assert!(config.grpc_manual_backfill_enabled);
        assert_eq!(config.grpc_max_stalls_before_open, 3);
        assert_eq!(config.grpc_circuit_breaker_cooldown_ms, 15_000);
        assert_eq!(config.stream_mode, StreamMode::SingleGlobal);
        assert_eq!(config.tx_filter_strategy, TxFilterStrategy::PerPool);
        assert_eq!(config.funding_lane_mode, FundingLaneMode::Disabled);
        assert!(!config.program_streams.enabled);
        assert_eq!(config.program_streams.endpoint, "stream-1.nln.clr3.org:443");
        assert_eq!(
            config.program_streams.format,
            ProgramStreamPayloadFormat::Json
        );
        assert_eq!(config.program_streams.max_streams, 3);
        assert_eq!(
            config.program_streams.quota_policy,
            ProgramStreamsQuotaPolicy::DropOptional
        );
        assert!(config.program_streams.enabled_topics.is_empty());
        assert!(config.program_streams.optional_topics.is_empty());
        assert!(config.program_streams.disabled_optional_topics.is_empty());
        assert_eq!(config.watched_pools_ttl_ms, 120_000);
        assert_eq!(config.watched_pools_cap, 32_768);
    }

    #[test]
    fn test_quote_mint_filter() {
        let filter = FilterConfig::default();

        // SOL mint
        let sol_mint = Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap();
        assert!(filter.is_quote_mint_allowed(&sol_mint));

        // Random mint (not in allowed list)
        let random_mint = Pubkey::new_unique();
        assert!(!filter.is_quote_mint_allowed(&random_mint));

        // Empty filter allows all
        let empty_filter = FilterConfig {
            allowed_quote_mints: vec![],
            ..Default::default()
        };
        assert!(empty_filter.is_quote_mint_allowed(&random_mint));
    }

    #[test]
    fn test_liquidity_requirement() {
        let mut filter = FilterConfig::default();

        // No requirement
        assert!(filter.meets_liquidity_requirement(Some(1.0)));
        assert!(filter.meets_liquidity_requirement(None));

        // With requirement
        filter.min_initial_liquidity_sol = Some(5.0);
        assert!(filter.meets_liquidity_requirement(Some(10.0)));
        assert!(!filter.meets_liquidity_requirement(Some(3.0)));
        assert!(!filter.meets_liquidity_requirement(None));
    }

    #[test]
    fn test_effective_source_mode_with_explicit_source_mode() {
        let mut config = SeerConfig::default();

        // When source_mode is explicitly set, it should be used
        config.source_mode = Some(SeerSourceMode::PumpPortalWs);
        assert_eq!(config.effective_source_mode(), SeerSourceMode::PumpPortalWs);

        config.source_mode = Some(SeerSourceMode::HeliusWebSocket);
        assert_eq!(
            config.effective_source_mode(),
            SeerSourceMode::HeliusWebSocket
        );

        config.source_mode = Some(SeerSourceMode::GeyserWebSocket);
        assert_eq!(
            config.effective_source_mode(),
            SeerSourceMode::GeyserWebSocket
        );

        config.source_mode = Some(SeerSourceMode::GeyserGrpc);
        assert_eq!(config.effective_source_mode(), SeerSourceMode::GeyserGrpc);
    }

    #[test]
    fn test_effective_source_mode_fallback_to_connection_mode() {
        let mut config = SeerConfig::default();

        // When source_mode is None, should derive from connection_mode
        config.source_mode = None;
        config.connection_mode = ConnectionMode::Grpc;
        assert_eq!(config.effective_source_mode(), SeerSourceMode::GeyserGrpc);

        config.connection_mode = ConnectionMode::WebSocket;
        assert_eq!(
            config.effective_source_mode(),
            SeerSourceMode::GeyserWebSocket
        );
    }

    #[test]
    fn test_pumpportal_config_defaults() {
        let config = PumpPortalConfig::default();

        assert_eq!(config.ws_url, "wss://pumpportal.fun/api/data");
        assert_eq!(config.max_active_mints, 1_000);
        assert_eq!(config.subscription_batch_size, 10);
        assert_eq!(config.reconnect_base_delay_secs, 5);
        assert_eq!(config.reconnect_max_delay_secs, 300);
        assert_eq!(config.stats_window_secs, 900);
    }

    #[test]
    fn test_seer_source_mode_serialization() {
        // Test that SeerSourceMode serializes to snake_case
        assert_eq!(
            serde_json::to_string(&SeerSourceMode::PumpPortalWs).unwrap(),
            "\"pump_portal_ws\""
        );
        assert_eq!(
            serde_json::to_string(&SeerSourceMode::GeyserGrpc).unwrap(),
            "\"geyser_grpc\""
        );
        assert_eq!(
            serde_json::to_string(&SeerSourceMode::GeyserWebSocket).unwrap(),
            "\"geyser_web_socket\""
        );
        assert_eq!(
            serde_json::to_string(&SeerSourceMode::HeliusWebSocket).unwrap(),
            "\"helius_web_socket\""
        );
    }

    #[test]
    fn test_seer_source_mode_deserialization() {
        // Test that SeerSourceMode deserializes from snake_case
        assert_eq!(
            serde_json::from_str::<SeerSourceMode>("\"pump_portal_ws\"").unwrap(),
            SeerSourceMode::PumpPortalWs
        );
        assert_eq!(
            serde_json::from_str::<SeerSourceMode>("\"geyser_grpc\"").unwrap(),
            SeerSourceMode::GeyserGrpc
        );
        assert_eq!(
            serde_json::from_str::<SeerSourceMode>("\"geyser_web_socket\"").unwrap(),
            SeerSourceMode::GeyserWebSocket
        );
        assert_eq!(
            serde_json::from_str::<SeerSourceMode>("\"helius_web_socket\"").unwrap(),
            SeerSourceMode::HeliusWebSocket
        );
    }

    #[test]
    fn test_stream_mode_serialization() {
        assert_eq!(
            serde_json::to_string(&StreamMode::SingleGlobal).unwrap(),
            "\"single_global\""
        );
        assert_eq!(
            serde_json::to_string(&StreamMode::PooledFiltered).unwrap(),
            "\"pooled_filtered\""
        );
    }

    #[test]
    fn test_tx_filter_strategy_serialization() {
        assert_eq!(
            serde_json::to_string(&TxFilterStrategy::PerPool).unwrap(),
            "\"per_pool\""
        );
        assert_eq!(
            serde_json::to_string(&TxFilterStrategy::All).unwrap(),
            "\"all\""
        );
    }

    #[test]
    fn test_funding_lane_mode_serialization() {
        assert_eq!(
            serde_json::to_string(&FundingLaneMode::Disabled).unwrap(),
            "\"disabled\""
        );
        assert_eq!(
            serde_json::to_string(&FundingLaneMode::PumpFiltered).unwrap(),
            "\"pump_filtered\""
        );
        assert_eq!(
            serde_json::to_string(&FundingLaneMode::FullChain).unwrap(),
            "\"full_chain\""
        );
    }

    #[test]
    fn test_funding_lane_mode_deserialization() {
        assert_eq!(
            serde_json::from_str::<FundingLaneMode>("\"disabled\"").unwrap(),
            FundingLaneMode::Disabled
        );
        assert_eq!(
            serde_json::from_str::<FundingLaneMode>("\"pump_filtered\"").unwrap(),
            FundingLaneMode::PumpFiltered
        );
        assert_eq!(
            serde_json::from_str::<FundingLaneMode>("\"full_chain\"").unwrap(),
            FundingLaneMode::FullChain
        );
    }

    #[test]
    fn test_program_streams_defaults_are_inert() {
        let config = ProgramStreamsConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.endpoint, "stream-1.nln.clr3.org:443");
        assert_eq!(config.auth_header, "x-api-key");
        assert_eq!(config.api_key_env, "NLN_API_KEY");
        assert_eq!(
            config.api_key_env_fallback.as_deref(),
            Some("GHOST_NLN_API_KEY")
        );
        assert_eq!(config.format.as_str(), "JSON");
        assert_eq!(config.max_streams, 3);
        assert_eq!(config.quota_policy, ProgramStreamsQuotaPolicy::DropOptional);
        assert!(config.enabled_topics.is_empty());
        assert!(config.optional_topics.is_empty());
        assert!(config.disabled_optional_topics.is_empty());
        assert_eq!(config.trade_resolver_ttl_ms, 30_000);
        assert_eq!(config.trade_resolver_per_mint_cap, 256);
        assert_eq!(config.trade_resolver_global_cap, 50_000);
        assert_eq!(config.trade_dedupe_ttl_ms, 300_000);
        assert_eq!(config.transfer_dedupe_ttl_ms, 300_000);
        assert_eq!(
            config.pumpfun_create_topic,
            "prod.rpc.solana.pumpfun.create"
        );
        assert_eq!(config.pumpfun_trade_topic, "prod.rpc.solana.pumpfun.trade");
        assert_eq!(
            config.system_transfers_topic,
            "prod.rpc.solana.system.transfers"
        );
    }

    #[test]
    fn test_program_streams_deserializes_json_format() {
        let config: ProgramStreamsConfig = serde_json::from_str(
            r#"{
                "enabled": true,
                "format": "JSON",
                "endpoint": "stream-1.nln.clr3.org:443",
                "max_streams": 2,
                "quota_policy": "fail_fast",
                "enabled_topics": [
                    "prod.rpc.solana.system.transfers",
                    "prod.rpc.solana.pumpfun.trade"
                ],
                "optional_topics": [
                    "prod.rpc.solana.pumpfun.create",
                    "prod.rpc.solana.pumpfun.transaction"
                ],
                "disabled_optional_topics": [
                    "prod.rpc.solana.pumpfun.create"
                ],
                "trade_resolver_ttl_ms": 30000,
                "trade_resolver_per_mint_cap": 256,
                "trade_resolver_global_cap": 50000,
                "trade_dedupe_ttl_ms": 300000,
                "trade_dedupe_max_entries": 250000,
                "transfer_dedupe_ttl_ms": 300000,
                "transfer_dedupe_max_entries": 500000
            }"#,
        )
        .unwrap();
        assert!(config.enabled);
        assert_eq!(config.format, ProgramStreamPayloadFormat::Json);
        assert_eq!(config.max_streams, 2);
        assert_eq!(config.quota_policy, ProgramStreamsQuotaPolicy::FailFast);
        assert_eq!(
            config.enabled_topics,
            vec![
                "prod.rpc.solana.system.transfers".to_string(),
                "prod.rpc.solana.pumpfun.trade".to_string()
            ]
        );
        assert_eq!(
            config.optional_topics,
            vec![
                "prod.rpc.solana.pumpfun.create".to_string(),
                "prod.rpc.solana.pumpfun.transaction".to_string()
            ]
        );
        assert_eq!(
            config.disabled_optional_topics,
            vec!["prod.rpc.solana.pumpfun.create".to_string()]
        );
        assert_eq!(
            config.system_transfers_topic,
            "prod.rpc.solana.system.transfers"
        );
    }
}
