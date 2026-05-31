//! NLN Program Streams client for FSC v2 capture/evidence.
//!
//! This module intentionally stops at the transport/client boundary. It does
//! not feed Seer runtime events, does not call NLN RPC, and does not infer
//! coverage from offsets. Offsets are carried as diagnostic evidence only.

use crate::config::{ProgramStreamPayloadFormat, ProgramStreamsConfig};
use crate::grpc_connection::PUMP_FUN_PROGRAM_ID;
use crate::ipc::{FundingTransferCoverageClass, FundingTransferEvent, FundingTransferProvenance};
use crate::types::{CandidatePool, RawBytesMissingReason, ToolchainFingerprintInput, TradeEvent};
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use futures::Stream;
use ghost_core::{
    CurveFinality, EventCompleteness, EventSemanticEnvelope, EventTimeMetadata, EventTruthKind,
    SlotQuality, SourceKind, TimestampQuality,
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::pin::Pin;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::{sleep, timeout};
use tonic::client::Grpc;
use tonic::codec::{ProstCodec, Streaming};
use tonic::codegen::http;
use tonic::metadata::{Ascii, AsciiMetadataValue, MetadataKey};
use tonic::transport::{Channel, Endpoint};
use tonic::{Request, Response, Status};
use tracing::{debug, warn};

const LIST_TOPICS_PATH: &str = "/nln.stream.v1.StreamService/ListTopics";
const SUBSCRIBE_PATH: &str = "/nln.stream.v1.StreamService/Subscribe";
const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
const LAMPORTS_PER_SOL_F64: f64 = 1_000_000_000.0;

/// Empty request for `nln.stream.v1.StreamService/ListTopics`.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ListTopicsRequest {}

/// Topic metadata returned by `ListTopics`.
///
/// The field names are local Rust names; protobuf compatibility depends on
/// field numbers. The provider spec exposes topic identity plus optional proto
/// message type metadata.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TopicInfo {
    #[prost(string, tag = "1")]
    pub topic: String,
    #[prost(string, tag = "2")]
    pub proto_message_type: String,
}

/// Response for `ListTopics`.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ListTopicsResponse {
    #[prost(message, repeated, tag = "1")]
    pub topics: Vec<TopicInfo>,
}

/// NLN output format. PR-FSC2 supports JSON mode only.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, Serialize,
)]
#[repr(i32)]
pub enum OutputFormat {
    Unspecified = 0,
    Json = 1,
    Proto = 2,
}

/// Request for `Subscribe`.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SubscribeRequest {
    #[prost(string, tag = "1")]
    pub topic: String,
    #[prost(enumeration = "OutputFormat", tag = "2")]
    pub format: i32,
}

/// Streaming response wrapper for NLN Program Streams.
///
/// `payload` is decoded as JSON bytes first, then as a base64-wrapped JSON
/// payload. This keeps the client compatible with both tonic/protobuf byte
/// behavior and the provider examples that surface payload as base64 text.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SubscribeResponse {
    #[prost(string, tag = "1")]
    pub topic: String,
    #[prost(uint32, tag = "2")]
    pub partition: u32,
    #[prost(uint64, tag = "3")]
    pub offset: u64,
    #[prost(int64, tag = "4")]
    pub timestamp_ms: i64,
    #[prost(bytes = "vec", tag = "5")]
    pub payload: Vec<u8>,
}

/// A decoded Program Streams message with ingest metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NlnProgramStreamMessage {
    pub topic: String,
    pub partition: u32,
    /// Raw provider offset. Diagnostic-only; do not infer drops from gaps.
    pub offset_raw: String,
    /// Parsed offset when it is decimal. Diagnostic-only.
    pub offset: Option<u64>,
    pub provider_ts_ms: Option<i64>,
    pub recv_ts_ms: u64,
    pub decode_ts_ms: u64,
    pub payload_json: Value,
}

/// Topic info normalized for callers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NlnTopicInfo {
    pub topic: String,
    pub proto_message_type: Option<String>,
}

/// Normalized ingest metadata attached to decoded NLN payloads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NlnIngestMeta {
    pub provider: String,
    pub topic: String,
    pub partition: u32,
    /// Raw provider offset. Diagnostic-only; do not infer drops from gaps.
    pub offset_raw: String,
    /// Parsed provider offset when decimal. Diagnostic-only.
    pub offset: Option<u64>,
    pub provider_ts_ms: Option<i64>,
    pub recv_ts_ms: u64,
    pub decode_ts_ms: u64,
    pub slot: Option<u64>,
    pub signature: Option<String>,
    pub tx_index: Option<u32>,
    pub instruction_index: Option<u32>,
}

impl NlnIngestMeta {
    fn from_message(
        message: &NlnProgramStreamMessage,
        slot: Option<u64>,
        signature: Option<String>,
        tx_index: Option<u32>,
        instruction_index: Option<u32>,
    ) -> Self {
        Self {
            provider: "NLN".to_string(),
            topic: message.topic.clone(),
            partition: message.partition,
            offset_raw: message.offset_raw.clone(),
            offset: message.offset,
            provider_ts_ms: message.provider_ts_ms,
            recv_ts_ms: message.recv_ts_ms,
            decode_ts_ms: message.decode_ts_ms,
            slot,
            signature,
            tx_index,
            instruction_index,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NlnEvent {
    PumpFunCreate(NlnPumpFunCreateEvent),
    PumpFunTrade(NlnPumpFunTradeEvent),
    Transfer(NlnTransferEvent),
    Unknown { meta: NlnIngestMeta, raw: Value },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PumpFunTradeSide {
    Buy,
    Sell,
}

impl PumpFunTradeSide {
    pub const fn is_buy(self) -> bool {
        matches!(self, Self::Buy)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransferAsset {
    NativeSol,
    WrappedSol,
    SplToken,
    Unknown,
}

/// Coverage contract for converting NLN transfer entries into primary FSC events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NlnFundingTransferCoverage {
    CaptureOnly,
    HealthyDedicatedFullChain,
}

impl NlnFundingTransferCoverage {
    const fn full_chain_coverage(self) -> bool {
        matches!(self, Self::HealthyDedicatedFullChain)
    }

    const fn provenance(self) -> FundingTransferProvenance {
        match self {
            Self::CaptureOnly => FundingTransferProvenance::nln_program_streams_live(
                FundingTransferCoverageClass::FilteredObservations,
            ),
            Self::HealthyDedicatedFullChain => FundingTransferProvenance::nln_program_streams_live(
                FundingTransferCoverageClass::FullChainCoverage,
            ),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NlnPumpFunCreateEvent {
    pub meta: NlnIngestMeta,
    pub signature: String,
    pub tx_index: Option<u32>,
    pub slot: u64,
    pub mint: solana_sdk::pubkey::Pubkey,
    pub creator: solana_sdk::pubkey::Pubkey,
    pub bonding_curve: Option<solana_sdk::pubkey::Pubkey>,
    pub block_time: Option<i64>,
    pub virtual_sol_reserves: Option<u64>,
    pub virtual_token_reserves: Option<u64>,
    pub real_sol_reserves: Option<u64>,
    pub real_token_reserves: Option<u64>,
}

impl NlnPumpFunCreateEvent {
    pub fn to_candidate_pool(&self) -> Result<CandidatePool> {
        let bonding_curve = self
            .bonding_curve
            .context("NLN pumpfun.create missing bonding_curve; cannot build CandidatePool")?;
        let quote_mint =
            solana_sdk::pubkey::Pubkey::from_str(WSOL_MINT).context("invalid WSOL mint")?;
        let amm_program_id = solana_sdk::pubkey::Pubkey::from_str(PUMP_FUN_PROGRAM_ID)
            .context("invalid pump.fun program id")?;
        let event_time = event_time_from_meta(&self.meta, self.block_time);
        let event_ts_ms = event_time.effective_event_ts_ms();
        let timestamp = self
            .block_time
            .and_then(|value| (value >= 0).then_some(value as u64))
            .or_else(|| event_ts_ms.map(|value| value / 1000))
            .unwrap_or_else(|| self.meta.recv_ts_ms / 1000);

        Ok(CandidatePool {
            semantic: nln_semantic(self.meta.slot, event_time),
            slot: Some(self.slot),
            tx_index: self.tx_index,
            event_ts_ms,
            event_time,
            signature: self.signature.clone(),
            amm_program_id,
            pool_amm_id: bonding_curve,
            base_mint: self.mint,
            quote_mint,
            bonding_curve,
            creator: self.creator,
            timestamp,
            bonding_curve_progress: None,
            initial_liquidity_sol: self
                .virtual_sol_reserves
                .or(self.real_sol_reserves)
                .map(lamports_to_sol),
            token_total_supply: None,
            block_time: self.block_time,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NlnPumpFunTradeEvent {
    pub meta: NlnIngestMeta,
    pub signature: solana_sdk::signature::Signature,
    pub tx_index: u32,
    pub slot: u64,
    pub mint: solana_sdk::pubkey::Pubkey,
    pub user: solana_sdk::pubkey::Pubkey,
    pub creator: Option<solana_sdk::pubkey::Pubkey>,
    pub side: PumpFunTradeSide,
    pub sol_amount_lamports: u64,
    pub token_amount_units: u64,
    pub block_time: Option<i64>,
    pub virtual_sol_reserves: Option<u64>,
    pub virtual_token_reserves: Option<u64>,
    pub real_sol_reserves: Option<u64>,
    pub real_token_reserves: Option<u64>,
}

impl NlnPumpFunTradeEvent {
    /// Convert into the existing Seer trade boundary after the caller resolves
    /// mint -> canonical pool/bonding-curve identity.
    pub fn to_trade_event(&self, pool_amm_id: solana_sdk::pubkey::Pubkey) -> TradeEvent {
        let event_time = event_time_from_meta(&self.meta, self.block_time);
        let timestamp_ms = event_time
            .effective_event_ts_ms()
            .unwrap_or(self.meta.recv_ts_ms);
        let is_buy = self.side.is_buy();

        TradeEvent {
            semantic: nln_semantic(Some(self.slot), event_time),
            slot: Some(self.slot),
            signature: self.signature,
            event_ordinal: None,
            tx_index: Some(self.tx_index),
            provenance: None,
            timestamp_ms,
            arrival_ts_ms: self.meta.recv_ts_ms,
            event_time,
            pool_amm_id,
            mint: self.mint,
            signer: self.user,
            is_buy,
            is_dev_buy: false,
            amount: self.token_amount_units,
            max_sol_cost: if is_buy { self.sol_amount_lamports } else { 0 },
            min_sol_output: if is_buy { 0 } else { self.sol_amount_lamports },
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            v_tokens_in_bonding_curve: self.virtual_token_reserves.map(|value| value as f64),
            v_sol_in_bonding_curve: self.virtual_sol_reserves.map(lamports_to_sol),
            market_cap_sol: None,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: Some(match self.side {
                PumpFunTradeSide::Buy => "nln_pumpfun_buy".to_string(),
                PumpFunTradeSide::Sell => "nln_pumpfun_sell".to_string(),
            }),
            associated_bonding_curve: None,
            bonding_curve_v2: None,
            bonding_curve_v2_provenance: None,
            buy_remaining_accounts: vec![],
            is_mayhem_mode: None,
            cu_price_micro_lamports: None,
            compute_unit_limit: None,
            inner_ix_count: None,
            cpi_depth: None,
            ata_create_count: None,
            signer_pre_balance_lamports: None,
            signer_post_balance_lamports: None,
            jito_tip_detected: None,
            toolchain_fingerprint: ToolchainFingerprintInput::default(),
            curve_data_known: self.virtual_sol_reserves.is_some()
                || self.virtual_token_reserves.is_some()
                || self.real_sol_reserves.is_some()
                || self.real_token_reserves.is_some(),
            curve_finality: CurveFinality::Speculative,
            is_pumpswap: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NlnTransferEvent {
    pub meta: NlnIngestMeta,
    pub signature: String,
    pub tx_index: Option<u32>,
    pub instruction_index: Option<u32>,
    pub slot: u64,
    pub from_wallet: solana_sdk::pubkey::Pubkey,
    pub to_wallet: solana_sdk::pubkey::Pubkey,
    pub amount_lamports: u64,
    pub token_address: String,
}

impl NlnTransferEvent {
    pub fn asset(&self) -> TransferAsset {
        match self.token_address.as_str() {
            "solana" => TransferAsset::NativeSol,
            WSOL_MINT => TransferAsset::WrappedSol,
            "" => TransferAsset::Unknown,
            _ => TransferAsset::SplToken,
        }
    }

    /// Convert only native SOL transfers into the primary FSC funding boundary.
    pub fn to_native_sol_funding_transfer_event(
        &self,
        coverage: NlnFundingTransferCoverage,
    ) -> Option<FundingTransferEvent> {
        if self.asset() != TransferAsset::NativeSol {
            return None;
        }

        let event_time = event_time_from_meta(&self.meta, None);
        Some(FundingTransferEvent {
            semantic: nln_semantic(Some(self.slot), event_time),
            slot: Some(self.slot),
            event_ordinal: None,
            tx_index: self.tx_index,
            outer_instruction_index: self.instruction_index,
            inner_group_index: None,
            cpi_stack_height: None,
            event_time,
            arrival_ts_ms: self.meta.recv_ts_ms,
            signature: self.signature.clone(),
            source_wallet: self.from_wallet.to_string(),
            recipient_wallet: self.to_wallet.to_string(),
            lamports: self.amount_lamports,
            full_chain_coverage: coverage.full_chain_coverage(),
            provenance: coverage.provenance(),
        })
    }
}

/// Subscribe loop options. Defaults are bounded and inert until explicitly used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NlnSubscribeLoopOptions {
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    pub max_reconnects: Option<u32>,
    pub stall_timeout: Option<Duration>,
}

impl Default for NlnSubscribeLoopOptions {
    fn default() -> Self {
        Self {
            initial_backoff: Duration::from_millis(200),
            max_backoff: Duration::from_secs(5),
            max_reconnects: Some(10),
            stall_timeout: None,
        }
    }
}

/// Atomic counters for the PR-FSC2 client boundary.
#[derive(Debug, Default)]
pub struct NlnProgramStreamsStats {
    list_topics_requests: AtomicU64,
    subscribe_requests: AtomicU64,
    messages_received: AtomicU64,
    payload_decode_errors: AtomicU64,
    json_decode_errors: AtomicU64,
    reconnects: AtomicU64,
    stalls: AtomicU64,
    last_message_recv_ts_ms: AtomicU64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NlnProgramStreamsStatsSnapshot {
    pub list_topics_requests: u64,
    pub subscribe_requests: u64,
    pub messages_received: u64,
    pub payload_decode_errors: u64,
    pub json_decode_errors: u64,
    pub reconnects: u64,
    pub stalls: u64,
    pub last_message_recv_ts_ms: u64,
}

impl NlnProgramStreamsStats {
    pub fn snapshot(&self) -> NlnProgramStreamsStatsSnapshot {
        NlnProgramStreamsStatsSnapshot {
            list_topics_requests: self.list_topics_requests.load(Ordering::Relaxed),
            subscribe_requests: self.subscribe_requests.load(Ordering::Relaxed),
            messages_received: self.messages_received.load(Ordering::Relaxed),
            payload_decode_errors: self.payload_decode_errors.load(Ordering::Relaxed),
            json_decode_errors: self.json_decode_errors.load(Ordering::Relaxed),
            reconnects: self.reconnects.load(Ordering::Relaxed),
            stalls: self.stalls.load(Ordering::Relaxed),
            last_message_recv_ts_ms: self.last_message_recv_ts_ms.load(Ordering::Relaxed),
        }
    }

    fn record_list_topics_request(&self) {
        self.list_topics_requests.fetch_add(1, Ordering::Relaxed);
        metrics::counter!("seer_nln_program_streams_list_topics_requests_total", 1);
    }

    fn record_subscribe_request(&self, topic: &str) {
        self.subscribe_requests.fetch_add(1, Ordering::Relaxed);
        metrics::counter!(
            "seer_nln_program_streams_subscribe_requests_total",
            1,
            "topic" => topic.to_string()
        );
    }

    fn record_message(&self, topic: &str, recv_ts_ms: u64, provider_ts_ms: Option<i64>) {
        self.messages_received.fetch_add(1, Ordering::Relaxed);
        self.last_message_recv_ts_ms
            .store(recv_ts_ms, Ordering::Relaxed);
        metrics::counter!(
            "seer_nln_program_streams_messages_total",
            1,
            "topic" => topic.to_string()
        );

        if let Some(provider_ts_ms) = provider_ts_ms {
            if provider_ts_ms >= 0 && recv_ts_ms >= provider_ts_ms as u64 {
                metrics::histogram!(
                    "seer_nln_program_streams_transport_latency_ms",
                    (recv_ts_ms - provider_ts_ms as u64) as f64,
                    "topic" => topic.to_string()
                );
            }
        }
    }

    fn record_payload_decode_error(&self, topic: &str) {
        self.payload_decode_errors.fetch_add(1, Ordering::Relaxed);
        metrics::counter!(
            "seer_nln_program_streams_payload_decode_errors_total",
            1,
            "topic" => topic.to_string()
        );
    }

    fn record_json_decode_error(&self, topic: &str) {
        self.json_decode_errors.fetch_add(1, Ordering::Relaxed);
        metrics::counter!(
            "seer_nln_program_streams_json_decode_errors_total",
            1,
            "topic" => topic.to_string()
        );
    }

    pub fn record_reconnect(&self, topic: &str) {
        self.reconnects.fetch_add(1, Ordering::Relaxed);
        metrics::counter!(
            "seer_nln_program_streams_reconnects_total",
            1,
            "topic" => topic.to_string()
        );
    }

    pub fn record_stall(&self, topic: &str) {
        self.stalls.fetch_add(1, Ordering::Relaxed);
        metrics::counter!(
            "seer_nln_program_streams_stalls_total",
            1,
            "topic" => topic.to_string()
        );
    }
}

/// Minimal tonic client for `nln.stream.v1.StreamService`.
#[derive(Debug, Clone)]
struct StreamServiceClient {
    inner: Grpc<Channel>,
}

impl StreamServiceClient {
    async fn connect(endpoint: String) -> Result<Self> {
        let channel = Endpoint::from_shared(endpoint)
            .context("invalid NLN Program Streams endpoint")?
            .connect()
            .await
            .context("failed to connect to NLN Program Streams endpoint")?;
        Ok(Self {
            inner: Grpc::new(channel),
        })
    }

    async fn list_topics(
        &mut self,
        request: Request<ListTopicsRequest>,
    ) -> std::result::Result<Response<ListTopicsResponse>, Status> {
        self.inner
            .ready()
            .await
            .map_err(|err| Status::unknown(format!("NLN ListTopics service not ready: {err}")))?;
        let path = http::uri::PathAndQuery::from_static(LIST_TOPICS_PATH);
        self.inner.unary(request, path, ProstCodec::default()).await
    }

    async fn subscribe(
        &mut self,
        request: Request<SubscribeRequest>,
    ) -> std::result::Result<Response<Streaming<SubscribeResponse>>, Status> {
        self.inner
            .ready()
            .await
            .map_err(|err| Status::unknown(format!("NLN Subscribe service not ready: {err}")))?;
        let path = http::uri::PathAndQuery::from_static(SUBSCRIBE_PATH);
        self.inner
            .server_streaming(request, path, ProstCodec::default())
            .await
    }
}

/// NLN Program Streams JSON-mode client.
#[derive(Debug, Clone)]
pub struct NlnProgramStreamsClient {
    config: ProgramStreamsConfig,
    inner: StreamServiceClient,
    auth_header: MetadataKey<Ascii>,
    api_key: AsciiMetadataValue,
    stats: Arc<NlnProgramStreamsStats>,
}

impl NlnProgramStreamsClient {
    pub async fn connect(config: ProgramStreamsConfig) -> Result<Self> {
        Self::connect_with_stats(config, Arc::new(NlnProgramStreamsStats::default())).await
    }

    pub async fn connect_with_stats(
        config: ProgramStreamsConfig,
        stats: Arc<NlnProgramStreamsStats>,
    ) -> Result<Self> {
        ensure_json_mode(config.format)?;

        let endpoint = normalize_endpoint_uri(&config.endpoint)?;
        let auth_header = MetadataKey::<Ascii>::from_bytes(config.auth_header.as_bytes())
            .with_context(|| format!("invalid NLN auth header '{}'", config.auth_header))?;
        let api_key = resolve_api_key(&config)?;
        let api_key = AsciiMetadataValue::try_from(api_key.as_str())
            .context("NLN API key contains non-ASCII metadata bytes")?;
        let inner = StreamServiceClient::connect(endpoint).await?;

        Ok(Self {
            config,
            inner,
            auth_header,
            api_key,
            stats,
        })
    }

    pub fn stats(&self) -> Arc<NlnProgramStreamsStats> {
        Arc::clone(&self.stats)
    }

    pub fn config(&self) -> &ProgramStreamsConfig {
        &self.config
    }

    pub async fn list_topics(&mut self) -> Result<Vec<NlnTopicInfo>> {
        self.stats.record_list_topics_request();
        let mut request = Request::new(ListTopicsRequest {});
        self.attach_auth(&mut request);
        let response = self
            .inner
            .list_topics(request)
            .await
            .context("NLN ListTopics request failed")?
            .into_inner();

        Ok(response
            .topics
            .into_iter()
            .map(|topic| NlnTopicInfo {
                topic: topic.topic,
                proto_message_type: non_empty(topic.proto_message_type),
            })
            .collect())
    }

    pub async fn subscribe_raw(
        &mut self,
        topic: impl Into<String>,
    ) -> Result<Streaming<SubscribeResponse>> {
        let topic = topic.into();
        self.stats.record_subscribe_request(&topic);
        let mut request = Request::new(SubscribeRequest {
            topic,
            format: OutputFormat::Json as i32,
        });
        self.attach_auth(&mut request);
        Ok(self
            .inner
            .subscribe(request)
            .await
            .context("NLN Subscribe request failed")?
            .into_inner())
    }

    pub fn decode_subscribe_response(
        &self,
        response: &SubscribeResponse,
    ) -> Result<NlnProgramStreamMessage> {
        decode_subscribe_response(response, self.stats.as_ref())
    }

    pub fn record_reconnect(&self, topic: &str) {
        self.stats.record_reconnect(topic);
    }

    pub fn record_stall(&self, topic: &str) {
        self.stats.record_stall(topic);
    }

    pub fn subscribe_json_with_reconnect(
        self,
        topic: impl Into<String>,
        options: NlnSubscribeLoopOptions,
    ) -> Pin<Box<dyn Stream<Item = Result<NlnProgramStreamMessage>> + Send>> {
        let topic = topic.into();
        Box::pin(async_stream::try_stream! {
            let mut client = self;
            let mut reconnect_attempts = 0u32;
            let mut backoff = options.initial_backoff;

            loop {
                if reconnect_attempts > 0 {
                    if options
                        .max_reconnects
                        .is_some_and(|max_reconnects| reconnect_attempts > max_reconnects)
                    {
                        Err(anyhow!(
                            "NLN Program Streams reconnect limit exceeded for topic '{}'",
                            topic
                        ))?;
                    }

                    client.record_reconnect(&topic);
                    let sleep_for = jitter_backoff(backoff);
                    debug!(
                        topic = %topic,
                        reconnect_attempts,
                        backoff_ms = sleep_for.as_millis(),
                        "reconnecting NLN Program Streams subscription"
                    );
                    sleep(sleep_for).await;
                    backoff = next_backoff(backoff, options.max_backoff);
                }

                let mut stream = client.subscribe_raw(topic.clone()).await?;

                loop {
                    let next_message = if let Some(stall_timeout) = options.stall_timeout {
                        match timeout(stall_timeout, stream.message()).await {
                            Ok(result) => result,
                            Err(_) => {
                                client.record_stall(&topic);
                                warn!(
                                    topic = %topic,
                                    stall_timeout_ms = stall_timeout.as_millis(),
                                    "NLN Program Streams subscription stalled"
                                );
                                break;
                            }
                        }
                    } else {
                        stream.message().await
                    };

                    match next_message {
                        Ok(Some(response)) => {
                            reconnect_attempts = 0;
                            backoff = options.initial_backoff;
                            yield client.decode_subscribe_response(&response)?;
                        }
                        Ok(None) => {
                            warn!(topic = %topic, "NLN Program Streams subscription ended");
                            break;
                        }
                        Err(err) => {
                            warn!(topic = %topic, error = %err, "NLN Program Streams receive error");
                            break;
                        }
                    }
                }

                reconnect_attempts = reconnect_attempts.saturating_add(1);
            }
        })
    }

    fn attach_auth<T>(&self, request: &mut Request<T>) {
        request
            .metadata_mut()
            .insert(self.auth_header.clone(), self.api_key.clone());
    }
}

pub fn decode_subscribe_response(
    response: &SubscribeResponse,
    stats: &NlnProgramStreamsStats,
) -> Result<NlnProgramStreamMessage> {
    let recv_ts_ms = now_ms();
    let provider_ts_ms = (response.timestamp_ms > 0).then_some(response.timestamp_ms);
    stats.record_message(&response.topic, recv_ts_ms, provider_ts_ms);

    let payload_json = match decode_json_payload(&response.payload) {
        Ok(payload) => payload,
        Err(err) => {
            stats.record_payload_decode_error(&response.topic);
            stats.record_json_decode_error(&response.topic);
            return Err(err).with_context(|| {
                format!(
                    "failed to decode NLN Program Streams payload for topic '{}'",
                    response.topic
                )
            });
        }
    };

    Ok(NlnProgramStreamMessage {
        topic: response.topic.clone(),
        partition: response.partition,
        offset_raw: response.offset.to_string(),
        offset: Some(response.offset),
        provider_ts_ms,
        recv_ts_ms,
        decode_ts_ms: now_ms(),
        payload_json,
    })
}

pub fn normalize_nln_event(
    message: &NlnProgramStreamMessage,
    config: &ProgramStreamsConfig,
) -> Result<NlnEvent> {
    if message.topic == config.pumpfun_create_topic {
        return parse_pumpfun_create(message).map(NlnEvent::PumpFunCreate);
    }
    if message.topic == config.pumpfun_trade_topic {
        return parse_pumpfun_trade(message).map(NlnEvent::PumpFunTrade);
    }
    if message.topic == config.system_transfers_topic {
        return parse_system_transfer(message).map(NlnEvent::Transfer);
    }

    Ok(NlnEvent::Unknown {
        meta: NlnIngestMeta::from_message(message, None, None, None, None),
        raw: message.payload_json.clone(),
    })
}

pub fn parse_pumpfun_create(message: &NlnProgramStreamMessage) -> Result<NlnPumpFunCreateEvent> {
    let object = payload_object(message)?;
    let signature = required_string(object, &["signature"])?;
    let tx_index = optional_u32(object, &["tx_index", "txIndex"])?;
    let slot = required_u64(object, &["slot"])?;
    let mint = required_pubkey(object, &["mint"])?;
    let creator = required_pubkey(object, &["creator", "user"])?;
    let bonding_curve = optional_pubkey(
        object,
        &[
            "bonding_curve",
            "bondingCurve",
            "pool_amm_id",
            "pool",
            "bondingCurveAddress",
        ],
    )?;
    let block_time = optional_i64(object, &["block_time", "blockTime", "timestamp"])?;
    let meta = NlnIngestMeta::from_message(
        message,
        Some(slot),
        Some(signature.clone()),
        tx_index,
        optional_u32(object, &["instruction_index", "instructionIndex"])?,
    );

    Ok(NlnPumpFunCreateEvent {
        meta,
        signature,
        tx_index,
        slot,
        mint,
        creator,
        bonding_curve,
        block_time,
        virtual_sol_reserves: optional_u64(
            object,
            &["virtual_sol_reserves", "virtualSolReserves"],
        )?,
        virtual_token_reserves: optional_u64(
            object,
            &["virtual_token_reserves", "virtualTokenReserves"],
        )?,
        real_sol_reserves: optional_u64(object, &["real_sol_reserves", "realSolReserves"])?,
        real_token_reserves: optional_u64(object, &["real_token_reserves", "realTokenReserves"])?,
    })
}

pub fn parse_pumpfun_trade(message: &NlnProgramStreamMessage) -> Result<NlnPumpFunTradeEvent> {
    let object = payload_object(message)?;
    let signature_text = required_string(object, &["signature"])?;
    let signature = solana_sdk::signature::Signature::from_str(&signature_text)
        .with_context(|| format!("invalid NLN pumpfun.trade signature '{signature_text}'"))?;
    let tx_index = required_u32(object, &["tx_index", "txIndex"])?;
    let slot = required_u64(object, &["slot"])?;
    let mint = required_pubkey(object, &["mint"])?;
    let user = required_pubkey(object, &["user", "buyer", "wallet"])?;
    let side = parse_trade_side(&required_string(object, &["ix_name", "ixName", "side"])?)
        .context("invalid NLN pumpfun.trade ix_name")?;
    let meta = NlnIngestMeta::from_message(
        message,
        Some(slot),
        Some(signature_text),
        Some(tx_index),
        optional_u32(object, &["instruction_index", "instructionIndex"])?,
    );

    Ok(NlnPumpFunTradeEvent {
        meta,
        signature,
        tx_index,
        slot,
        mint,
        user,
        creator: optional_pubkey(object, &["creator"])?,
        side,
        sol_amount_lamports: required_u64(object, &["sol_amount", "solAmount"])?,
        token_amount_units: required_u64(object, &["token_amount", "tokenAmount"])?,
        block_time: optional_i64(object, &["block_time", "blockTime", "timestamp"])?,
        virtual_sol_reserves: optional_u64(
            object,
            &["virtual_sol_reserves", "virtualSolReserves"],
        )?,
        virtual_token_reserves: optional_u64(
            object,
            &["virtual_token_reserves", "virtualTokenReserves"],
        )?,
        real_sol_reserves: optional_u64(object, &["real_sol_reserves", "realSolReserves"])?,
        real_token_reserves: optional_u64(object, &["real_token_reserves", "realTokenReserves"])?,
    })
}

pub fn parse_system_transfer(message: &NlnProgramStreamMessage) -> Result<NlnTransferEvent> {
    let object = payload_object(message)?;
    let signature = required_string(object, &["signature"])?;
    let tx_index = optional_u32(object, &["tx_index", "txIndex"])?;
    let instruction_index = optional_u32(object, &["instruction_index", "instructionIndex"])?;
    let slot = required_u64(object, &["slot"])?;
    let meta = NlnIngestMeta::from_message(
        message,
        Some(slot),
        Some(signature.clone()),
        tx_index,
        instruction_index,
    );

    Ok(NlnTransferEvent {
        meta,
        signature,
        tx_index,
        instruction_index,
        slot,
        from_wallet: required_pubkey(object, &["from_wallet", "fromWallet", "from"])?,
        to_wallet: required_pubkey(object, &["to_wallet", "toWallet", "to"])?,
        amount_lamports: required_u64(object, &["amount", "lamports", "amount_lamports"])?,
        token_address: required_string(object, &["token_address", "tokenAddress"])?,
    })
}

pub fn decode_json_payload(payload: &[u8]) -> Result<Value> {
    if payload.is_empty() {
        bail!("NLN Program Streams payload is empty");
    }

    if let Ok(value) = serde_json::from_slice::<Value>(payload) {
        return Ok(value);
    }

    let payload_text = std::str::from_utf8(payload)
        .context("NLN Program Streams payload is neither JSON bytes nor UTF-8 base64")?;
    let decoded = general_purpose::STANDARD
        .decode(payload_text.trim())
        .context("NLN Program Streams payload base64 decode failed")?;
    serde_json::from_slice::<Value>(&decoded)
        .context("NLN Program Streams decoded payload is not valid JSON")
}

pub fn normalize_endpoint_uri(endpoint: &str) -> Result<String> {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() {
        bail!("NLN Program Streams endpoint is empty");
    }

    if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        Ok(endpoint.to_string())
    } else {
        Ok(format!("https://{endpoint}"))
    }
}

pub fn resolve_api_key(config: &ProgramStreamsConfig) -> Result<String> {
    if let Some(value) = read_non_empty_env(&config.api_key_env) {
        return Ok(value);
    }

    if let Some(fallback_env) = config.api_key_env_fallback.as_deref() {
        if let Some(value) = read_non_empty_env(fallback_env) {
            return Ok(value);
        }
    }

    match config.api_key_env_fallback.as_deref() {
        Some(fallback_env) => bail!(
            "NLN API key not found in '{}' or fallback '{}'",
            config.api_key_env,
            fallback_env
        ),
        None => bail!("NLN API key not found in '{}'", config.api_key_env),
    }
}

fn ensure_json_mode(format: ProgramStreamPayloadFormat) -> Result<()> {
    match format {
        ProgramStreamPayloadFormat::Json => Ok(()),
    }
}

fn read_non_empty_env(name: &str) -> Option<String> {
    if name.trim().is_empty() {
        return None;
    }
    std::env::var(name).ok().and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn next_backoff(current: Duration, max: Duration) -> Duration {
    std::cmp::min(current.saturating_mul(2), max)
}

fn jitter_backoff(base: Duration) -> Duration {
    let max_jitter_ms = base.as_millis().min(250) as u64;
    if max_jitter_ms == 0 {
        return base;
    }
    base + Duration::from_millis(rand::thread_rng().gen_range(0..=max_jitter_ms))
}

fn payload_object(message: &NlnProgramStreamMessage) -> Result<&Map<String, Value>> {
    message.payload_json.as_object().with_context(|| {
        format!(
            "NLN Program Streams payload for topic '{}' is not a JSON object",
            message.topic
        )
    })
}

fn parse_trade_side(value: &str) -> Result<PumpFunTradeSide> {
    match value.trim().to_ascii_lowercase().as_str() {
        "buy" => Ok(PumpFunTradeSide::Buy),
        "sell" => Ok(PumpFunTradeSide::Sell),
        other => bail!("unsupported pumpfun trade side '{other}'"),
    }
}

fn event_time_from_meta(meta: &NlnIngestMeta, block_time: Option<i64>) -> EventTimeMetadata {
    EventTimeMetadata::new(
        block_time.and_then(|value| {
            if value >= 0 {
                Some((value as u64).saturating_mul(1000))
            } else {
                None
            }
        }),
        Some(meta.recv_ts_ms),
        None,
    )
}

fn nln_semantic(slot: Option<u64>, event_time: EventTimeMetadata) -> EventSemanticEnvelope {
    let timestamp_quality = if event_time.chain_event_ts_ms.is_some() {
        TimestampQuality::Chain
    } else {
        TimestampQuality::WallClock
    };
    let slot_quality = if slot.is_some() {
        SlotQuality::Present
    } else {
        SlotQuality::Absent
    };
    let completeness =
        if slot_quality == SlotQuality::Present && timestamp_quality == TimestampQuality::Chain {
            EventCompleteness::Full
        } else {
            EventCompleteness::Partial
        };

    EventSemanticEnvelope::new(
        SourceKind::Grpc,
        EventTruthKind::AdaptedChain,
        slot_quality,
        timestamp_quality,
        completeness,
    )
}

fn lamports_to_sol(lamports: u64) -> f64 {
    lamports as f64 / LAMPORTS_PER_SOL_F64
}

fn get_field<'a>(object: &'a Map<String, Value>, names: &[&str]) -> Option<&'a Value> {
    names.iter().find_map(|name| object.get(*name))
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.trim().to_string()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
    .filter(|value| !value.is_empty())
}

fn required_string(object: &Map<String, Value>, names: &[&str]) -> Result<String> {
    get_field(object, names)
        .and_then(value_to_string)
        .with_context(|| format!("missing required NLN field {}", names.join("|")))
}

fn optional_string(object: &Map<String, Value>, names: &[&str]) -> Result<Option<String>> {
    match get_field(object, names) {
        Some(Value::Null) | None => Ok(None),
        Some(value) => value_to_string(value)
            .map(Some)
            .with_context(|| format!("invalid string NLN field {}", names.join("|"))),
    }
}

fn required_u64(object: &Map<String, Value>, names: &[&str]) -> Result<u64> {
    optional_u64(object, names)?
        .with_context(|| format!("missing required numeric NLN field {}", names.join("|")))
}

fn optional_u64(object: &Map<String, Value>, names: &[&str]) -> Result<Option<u64>> {
    let Some(value) = get_field(object, names) else {
        return Ok(None);
    };
    match value {
        Value::Null => Ok(None),
        Value::Number(number) => number
            .as_u64()
            .map(Some)
            .with_context(|| format!("invalid unsigned numeric NLN field {}", names.join("|"))),
        Value::String(text) => text
            .trim()
            .parse::<u64>()
            .map(Some)
            .with_context(|| format!("invalid unsigned string NLN field {}", names.join("|"))),
        _ => bail!("invalid numeric NLN field {}", names.join("|")),
    }
}

fn required_u32(object: &Map<String, Value>, names: &[&str]) -> Result<u32> {
    let value = required_u64(object, names)?;
    u32::try_from(value).with_context(|| format!("NLN field {} exceeds u32", names.join("|")))
}

fn optional_u32(object: &Map<String, Value>, names: &[&str]) -> Result<Option<u32>> {
    optional_u64(object, names)?
        .map(|value| {
            u32::try_from(value)
                .with_context(|| format!("NLN field {} exceeds u32", names.join("|")))
        })
        .transpose()
}

fn optional_i64(object: &Map<String, Value>, names: &[&str]) -> Result<Option<i64>> {
    let Some(value) = get_field(object, names) else {
        return Ok(None);
    };
    match value {
        Value::Null => Ok(None),
        Value::Number(number) => number
            .as_i64()
            .map(Some)
            .with_context(|| format!("invalid signed numeric NLN field {}", names.join("|"))),
        Value::String(text) => text
            .trim()
            .parse::<i64>()
            .map(Some)
            .with_context(|| format!("invalid signed string NLN field {}", names.join("|"))),
        _ => bail!("invalid signed NLN field {}", names.join("|")),
    }
}

fn required_pubkey(
    object: &Map<String, Value>,
    names: &[&str],
) -> Result<solana_sdk::pubkey::Pubkey> {
    let value = required_string(object, names)?;
    solana_sdk::pubkey::Pubkey::from_str(&value)
        .with_context(|| format!("invalid pubkey in NLN field {}", names.join("|")))
}

fn optional_pubkey(
    object: &Map<String, Value>,
    names: &[&str],
) -> Result<Option<solana_sdk::pubkey::Pubkey>> {
    optional_string(object, names)?
        .map(|value| {
            solana_sdk::pubkey::Pubkey::from_str(&value)
                .with_context(|| format!("invalid pubkey in NLN field {}", names.join("|")))
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn default_config_with_env(env_name: &str, fallback: Option<String>) -> ProgramStreamsConfig {
        ProgramStreamsConfig {
            api_key_env: env_name.to_string(),
            api_key_env_fallback: fallback,
            ..ProgramStreamsConfig::default()
        }
    }

    fn decoded_message(topic: String, payload_json: Value) -> NlnProgramStreamMessage {
        NlnProgramStreamMessage {
            topic,
            partition: 0,
            offset_raw: "42".to_string(),
            offset: Some(42),
            provider_ts_ms: Some(1_700_000_000_000),
            recv_ts_ms: 1_700_000_000_010,
            decode_ts_ms: 1_700_000_000_011,
            payload_json,
        }
    }

    #[test]
    fn normalizes_endpoint_without_scheme_to_https() {
        assert_eq!(
            normalize_endpoint_uri("stream-1.nln.clr3.org:443").unwrap(),
            "https://stream-1.nln.clr3.org:443"
        );
        assert_eq!(
            normalize_endpoint_uri("https://stream-1.nln.clr3.org:443").unwrap(),
            "https://stream-1.nln.clr3.org:443"
        );
    }

    #[test]
    fn resolve_api_key_uses_primary_env() {
        let primary = "GHOST_TEST_NLN_PRIMARY_PR2";
        let fallback = "GHOST_TEST_NLN_FALLBACK_PR2";
        std::env::set_var(primary, " primary-key ");
        std::env::set_var(fallback, "fallback-key");

        let config = default_config_with_env(primary, Some(fallback.to_string()));
        assert_eq!(resolve_api_key(&config).unwrap(), "primary-key");

        std::env::remove_var(primary);
        std::env::remove_var(fallback);
    }

    #[test]
    fn resolve_api_key_uses_fallback_env() {
        let primary = "GHOST_TEST_NLN_MISSING_PRIMARY_PR2";
        let fallback = "GHOST_TEST_NLN_PRESENT_FALLBACK_PR2";
        std::env::remove_var(primary);
        std::env::set_var(fallback, "fallback-key");

        let config = default_config_with_env(primary, Some(fallback.to_string()));
        assert_eq!(resolve_api_key(&config).unwrap(), "fallback-key");

        std::env::remove_var(fallback);
    }

    #[test]
    fn decode_json_payload_accepts_raw_json_bytes() {
        let payload = br#"{"topic":"ok","slot":42}"#;
        assert_eq!(
            decode_json_payload(payload).unwrap(),
            json!({"topic": "ok", "slot": 42})
        );
    }

    #[test]
    fn decode_json_payload_accepts_base64_wrapped_json() {
        let json_payload = br#"{"signature":"abc","amount":"1000"}"#;
        let payload = general_purpose::STANDARD.encode(json_payload);
        assert_eq!(
            decode_json_payload(payload.as_bytes()).unwrap(),
            json!({"signature": "abc", "amount": "1000"})
        );
    }

    #[test]
    fn decode_response_preserves_offset_as_diagnostic() {
        let stats = NlnProgramStreamsStats::default();
        let response = SubscribeResponse {
            topic: "prod.rpc.solana.system.transfers".to_string(),
            partition: 3,
            offset: 12345,
            timestamp_ms: 1,
            payload: br#"{"from_wallet":"a","to_wallet":"b"}"#.to_vec(),
        };

        let decoded = decode_subscribe_response(&response, &stats).unwrap();
        assert_eq!(decoded.offset_raw, "12345");
        assert_eq!(decoded.offset, Some(12345));
        assert_eq!(decoded.partition, 3);
        assert_eq!(decoded.payload_json["from_wallet"], "a");
        assert_eq!(stats.snapshot().messages_received, 1);
    }

    #[test]
    fn decode_error_is_metered() {
        let stats = NlnProgramStreamsStats::default();
        let response = SubscribeResponse {
            topic: "prod.rpc.solana.pumpfun.trade".to_string(),
            partition: 0,
            offset: 0,
            timestamp_ms: 0,
            payload: b"not-json-not-base64".to_vec(),
        };

        assert!(decode_subscribe_response(&response, &stats).is_err());
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.messages_received, 1);
        assert_eq!(snapshot.payload_decode_errors, 1);
        assert_eq!(snapshot.json_decode_errors, 1);
    }

    #[test]
    fn subscribe_request_uses_json_output_format_enum() {
        let request = SubscribeRequest {
            topic: ProgramStreamsConfig::default_system_transfers_topic(),
            format: OutputFormat::Json as i32,
        };

        assert_eq!(request.format, OutputFormat::Json as i32);
        assert_eq!(
            OutputFormat::try_from(request.format),
            Ok(OutputFormat::Json)
        );
    }

    #[test]
    fn stats_records_reconnects_and_stalls() {
        let stats = NlnProgramStreamsStats::default();
        stats.record_reconnect("topic-a");
        stats.record_stall("topic-a");
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.reconnects, 1);
        assert_eq!(snapshot.stalls, 1);
    }

    #[test]
    fn backoff_is_bounded_by_maximum() {
        assert_eq!(
            next_backoff(Duration::from_secs(4), Duration::from_secs(5)),
            Duration::from_secs(5)
        );
    }

    #[test]
    fn empty_endpoint_is_rejected() {
        assert!(normalize_endpoint_uri(" ").is_err());
    }

    #[test]
    fn missing_api_key_reports_env_names_only() {
        let primary = "GHOST_TEST_NLN_NO_PRIMARY_PR2";
        let fallback = "GHOST_TEST_NLN_NO_FALLBACK_PR2";
        std::env::remove_var(primary);
        std::env::remove_var(fallback);
        let config = default_config_with_env(primary, Some(fallback.to_string()));

        let err = resolve_api_key(&config).unwrap_err().to_string();
        assert!(err.contains(primary));
        assert!(err.contains(fallback));
    }

    #[test]
    fn normalizes_native_sol_transfer_and_preserves_tx_index() {
        let from_wallet = solana_sdk::pubkey::Pubkey::new_unique();
        let to_wallet = solana_sdk::pubkey::Pubkey::new_unique();
        let message = decoded_message(
            ProgramStreamsConfig::default_system_transfers_topic(),
            json!({
                "signature": "transfer-sig",
                "tx_index": "408",
                "slot": "422819679",
                "from_wallet": from_wallet.to_string(),
                "to_wallet": to_wallet.to_string(),
                "amount": "1000",
                "token_address": "solana",
                "instruction_index": 2
            }),
        );

        let transfer = parse_system_transfer(&message).unwrap();
        assert_eq!(transfer.tx_index, Some(408));
        assert_eq!(transfer.instruction_index, Some(2));
        assert_eq!(transfer.asset(), TransferAsset::NativeSol);

        let event = transfer
            .to_native_sol_funding_transfer_event(NlnFundingTransferCoverage::CaptureOnly)
            .expect("native SOL transfer should convert to FSC funding event");
        assert_eq!(event.tx_index, Some(408));
        assert_eq!(event.outer_instruction_index, Some(2));
        assert!(!event.full_chain_coverage);
        assert_eq!(
            event.provenance,
            FundingTransferProvenance::nln_program_streams_live(
                FundingTransferCoverageClass::FilteredObservations
            )
        );

        let full = transfer
            .to_native_sol_funding_transfer_event(
                NlnFundingTransferCoverage::HealthyDedicatedFullChain,
            )
            .expect("native SOL transfer should convert to FSC funding event");
        assert!(full.full_chain_coverage);
        assert_eq!(
            full.provenance.coverage_class,
            FundingTransferCoverageClass::FullChainCoverage
        );
    }

    #[test]
    fn transfer_order_fields_are_optional_for_capture_lane() {
        let from_wallet = solana_sdk::pubkey::Pubkey::new_unique();
        let to_wallet = solana_sdk::pubkey::Pubkey::new_unique();
        let message = decoded_message(
            ProgramStreamsConfig::default_system_transfers_topic(),
            json!({
                "signature": "transfer-sig",
                "slot": "422819679",
                "from_wallet": from_wallet.to_string(),
                "to_wallet": to_wallet.to_string(),
                "amount": "1000",
                "token_address": "solana"
            }),
        );

        let transfer = parse_system_transfer(&message).unwrap();
        assert_eq!(transfer.tx_index, None);
        assert_eq!(transfer.instruction_index, None);

        let event = transfer
            .to_native_sol_funding_transfer_event(NlnFundingTransferCoverage::CaptureOnly)
            .expect("native SOL transfer should still enter capture lane");
        assert_eq!(event.tx_index, None);
        assert_eq!(event.outer_instruction_index, None);
    }

    #[test]
    fn non_native_transfer_is_not_primary_fsc_input() {
        let from_wallet = solana_sdk::pubkey::Pubkey::new_unique();
        let to_wallet = solana_sdk::pubkey::Pubkey::new_unique();
        let message = decoded_message(
            ProgramStreamsConfig::default_system_transfers_topic(),
            json!({
                "signature": "transfer-sig",
                "tx_index": "1",
                "slot": "2",
                "from_wallet": from_wallet.to_string(),
                "to_wallet": to_wallet.to_string(),
                "amount": "1000",
                "token_address": WSOL_MINT,
                "instruction_index": 3
            }),
        );

        let transfer = parse_system_transfer(&message).unwrap();
        assert_eq!(transfer.asset(), TransferAsset::WrappedSol);
        assert!(transfer
            .to_native_sol_funding_transfer_event(NlnFundingTransferCoverage::CaptureOnly)
            .is_none());
    }

    #[test]
    fn normalizes_trade_user_as_buyer_and_builds_trade_event() {
        let signature = solana_sdk::signature::Signature::new_unique();
        let mint = solana_sdk::pubkey::Pubkey::new_unique();
        let user = solana_sdk::pubkey::Pubkey::new_unique();
        let creator = solana_sdk::pubkey::Pubkey::new_unique();
        let pool = solana_sdk::pubkey::Pubkey::new_unique();
        let message = decoded_message(
            ProgramStreamsConfig::default_pumpfun_trade_topic(),
            json!({
                "signature": signature.to_string(),
                "tx_index": "70",
                "mint": mint.to_string(),
                "sol_amount": "8418446",
                "token_amount": "330719451263",
                "user": user.to_string(),
                "creator": creator.to_string(),
                "ix_name": "buy",
                "slot": "422817405",
                "block_time": "1780009808",
                "virtual_sol_reserves": "26551096335",
                "virtual_token_reserves": "1043392990434519"
            }),
        );

        let trade = parse_pumpfun_trade(&message).unwrap();
        assert_eq!(trade.user, user);
        assert_eq!(trade.side, PumpFunTradeSide::Buy);
        assert_eq!(trade.tx_index, 70);

        let trade_event = trade.to_trade_event(pool);
        assert_eq!(trade_event.signer, user);
        assert!(trade_event.is_buy);
        assert_eq!(trade_event.max_sol_cost, 8_418_446);
        assert_eq!(trade_event.min_sol_output, 0);
        assert_eq!(trade_event.tx_index, Some(70));
        assert_eq!(trade_event.event_ordinal, None);
    }

    #[test]
    fn normalizes_create_with_provenance_to_candidate_pool() {
        let mint = solana_sdk::pubkey::Pubkey::new_unique();
        let creator = solana_sdk::pubkey::Pubkey::new_unique();
        let bonding_curve = solana_sdk::pubkey::Pubkey::new_unique();
        let message = decoded_message(
            ProgramStreamsConfig::default_pumpfun_create_topic(),
            json!({
                "signature": "create-sig",
                "tx_index": "9",
                "slot": "422817000",
                "mint": mint.to_string(),
                "creator": creator.to_string(),
                "bonding_curve": bonding_curve.to_string(),
                "block_time": "1780009700",
                "virtual_sol_reserves": "10000000"
            }),
        );

        let create = parse_pumpfun_create(&message).unwrap();
        assert_eq!(create.tx_index, Some(9));
        assert_eq!(create.meta.tx_index, Some(9));
        let candidate = create.to_candidate_pool().unwrap();
        assert_eq!(candidate.tx_index, Some(9));
        assert_eq!(candidate.base_mint, mint);
        assert_eq!(candidate.creator, creator);
        assert_eq!(candidate.bonding_curve, bonding_curve);
        assert_eq!(candidate.pool_amm_id, bonding_curve);
        assert_eq!(candidate.block_time, Some(1_780_009_700));
    }

    #[test]
    fn dispatches_known_topics_and_preserves_unknown_payloads() {
        let config = ProgramStreamsConfig::default();
        let transfer_message = decoded_message(
            config.system_transfers_topic.clone(),
            json!({
                "signature": "transfer-sig",
                "tx_index": "1",
                "slot": "2",
                "from_wallet": solana_sdk::pubkey::Pubkey::new_unique().to_string(),
                "to_wallet": solana_sdk::pubkey::Pubkey::new_unique().to_string(),
                "amount": "1000",
                "token_address": "solana",
                "instruction_index": 0
            }),
        );
        assert!(matches!(
            normalize_nln_event(&transfer_message, &config).unwrap(),
            NlnEvent::Transfer(_)
        ));

        let unknown = decoded_message("prod.rpc.solana.unknown".to_string(), json!({"x": 1}));
        assert!(matches!(
            normalize_nln_event(&unknown, &config).unwrap(),
            NlnEvent::Unknown { .. }
        ));
    }
}
