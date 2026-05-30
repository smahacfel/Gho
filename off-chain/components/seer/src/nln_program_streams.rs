//! NLN Program Streams client for FSC v2 capture/evidence.
//!
//! This module intentionally stops at the transport/client boundary. It does
//! not feed Seer runtime events, does not call NLN RPC, and does not infer
//! coverage from offsets. Offsets are carried as diagnostic evidence only.

use crate::config::{ProgramStreamPayloadFormat, ProgramStreamsConfig};
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use futures::Stream;
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::pin::Pin;
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
    #[prost(string, tag = "3")]
    pub offset: String,
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
        offset_raw: response.offset.clone(),
        offset: response.offset.parse::<u64>().ok(),
        provider_ts_ms,
        recv_ts_ms,
        decode_ts_ms: now_ms(),
        payload_json,
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
            offset: "12345".to_string(),
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
            offset: "bad".to_string(),
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
}
