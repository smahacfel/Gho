//! Kontrolowany benchmark v2: Direct Jito gRPC vs NLN `sendBundle`.
//!
//! To narzędzie jest całkowicie poza runtime Ghosta. Nie importuje Oracle,
//! Gatekeepera, Event Busa ani sendera Ghosta. Wysyła wyłącznie testowy bundle
//! z jedną transakcją: ComputeBudget + Memo + inline transfer minimalnego tipu.
//! Bez `--execute` wykonuje wyłącznie etap 0 (autoryzacja, tip accounts,
//! kontrakt `sendBundle`, koszt maksymalny) i nic nie publikuje on-chain.
//!
//! Przykład etapu 0:
//! ```text
//! NLN_BENCHMARK_API_KEY=... JITO_PROBE_KEYPAIR_PATH=/path/test.json \
//! BENCH_MAX_TOTAL_LAMPORTS=50000 \
//! cargo run -p seer --example nln_jito_submission_benchmark -- --max-pairs 3
//! ```
//!
//! Wysłanie wymaga dodatkowo `--execute`. Klucze nigdy nie są wypisywane ani
//! zapisywane przez narzędzie.

use std::{
    collections::HashMap,
    env,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use futures::StreamExt;
use parking_lot::Mutex;
use prost::Message;
use reqwest::{header, Client, StatusCode};
use serde::Serialize;
use serde_json::{json, Value};
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signature, Signer},
    system_instruction,
    transaction::Transaction,
};
use tokio::time::sleep;
use tonic::{
    codegen::http::uri::PathAndQuery,
    metadata::{Ascii, AsciiMetadataValue, MetadataKey, MetadataValue},
    service::Interceptor,
    transport::{Channel, ClientTlsConfig, Endpoint},
    Request, Status,
};
use tonic_health::pb::health_client::HealthClient;
use yellowstone_grpc_client::GeyserGrpcClient;
use yellowstone_grpc_proto::prelude::{
    geyser_client::GeyserClient, subscribe_update::UpdateOneof, CommitmentLevel, SubscribeRequest,
    SubscribeRequestFilterSlots, SubscribeRequestFilterTransactions,
};

const NLN_RPC_ENDPOINT: &str = "https://rpc.nln.clr3.org";
const NLN_GRPC_ENDPOINT: &str = "https://grpc.nln.clr3.org:443";
const JITO_FRANKFURT_GRPC: &str = "https://frankfurt.mainnet.block-engine.jito.wtf";
const JITO_FRANKFURT_HTTP: &str = "https://frankfurt.mainnet.block-engine.jito.wtf/api/v1/bundles";
const JITO_EU_GRPC_FAILOVER: &[&str] = &[
    "https://frankfurt.mainnet.block-engine.jito.wtf",
    "https://amsterdam.mainnet.block-engine.jito.wtf",
    "https://london.mainnet.block-engine.jito.wtf",
    "https://dublin.mainnet.block-engine.jito.wtf",
];
const NLN_API_KEY_ENV: &str = "NLN_BENCHMARK_API_KEY";
const KEYPAIR_ENV: &str = "JITO_PROBE_KEYPAIR_PATH";
const BUDGET_ENV: &str = "BENCH_MAX_TOTAL_LAMPORTS";
const JITO_AUTH_ENV: &str = "JITO_BENCHMARK_GRPC_AUTH";
const MEMO_PROGRAM_ID: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";

// Jito dokumentuje minimum 1_000 lamportów dla bundle. Celowo nie używamy
// produkcyjnego capu 0.002 SOL: jest to benchmark transportu, nie aukcja MEV.
const MIN_TIP_LAMPORTS: u64 = 1_000;
const MAX_EXPLICIT_TIP_LAMPORTS: u64 = 4_000_000;
// Symulacja kontrolna wykazała, że sam podpisany Memo zużywa ok. 19,7k CU;
// 50k pozostawia mały, lecz wystarczający margines dla transferu tipa.
const COMPUTE_UNIT_LIMIT: u32 = 50_000;
const COMPUTE_UNIT_PRICE_MICRO_LAMPORTS: u64 = 1_000;
const BASE_FEE_MAX_LAMPORTS: u64 = 5_000;
const STATUS_POLL_INTERVAL: Duration = Duration::from_millis(200);
const RECONCILIATION_TIMEOUT: Duration = Duration::from_secs(30);
const JITO_STATUS_MIN_INTERVAL: Duration = Duration::from_millis(1_050);
const JITO_GRPC_TIMEOUT: Duration = Duration::from_millis(1_200);

#[derive(Debug)]
struct Args {
    execute: bool,
    max_pairs: Option<usize>,
    tip_lamports: u64,
}

impl Args {
    fn parse() -> Result<Self> {
        let mut execute = false;
        let mut max_pairs = None;
        let mut tip_lamports = MIN_TIP_LAMPORTS;
        let mut values = env::args().skip(1);
        while let Some(argument) = values.next() {
            match argument.as_str() {
                "--execute" => execute = true,
                "--max-pairs" => {
                    let value = values.next().context("--max-pairs wymaga liczby 1..=5")?;
                    let parsed: usize = value
                        .parse()
                        .with_context(|| format!("nieprawidłowe --max-pairs: {value}"))?;
                    if !(1..=5).contains(&parsed) {
                        bail!("--max-pairs musi być w przedziale 1..=5");
                    }
                    max_pairs = Some(parsed);
                }
                "--tip-lamports" => {
                    let value = values.next().context("--tip-lamports wymaga liczby")?;
                    tip_lamports = value
                        .parse()
                        .with_context(|| format!("nieprawidłowe --tip-lamports: {value}"))?;
                }
                "--help" | "-h" => {
                    println!(
                        "Usage: nln_jito_submission_benchmark [--execute] --max-pairs 1..5 [--tip-lamports N]\n\
                         Wymaga env: NLN_BENCHMARK_API_KEY, JITO_PROBE_KEYPAIR_PATH, \
                         BENCH_MAX_TOTAL_LAMPORTS. Bez --execute: tylko Stage 0."
                    );
                    std::process::exit(0);
                }
                _ => bail!("nieznany argument: {argument}"),
            }
        }
        if execute && max_pairs.is_none() {
            bail!("wysłanie wymaga jawnego --max-pairs N");
        }
        if !(MIN_TIP_LAMPORTS..=MAX_EXPLICIT_TIP_LAMPORTS).contains(&tip_lamports) {
            bail!(
                "--tip-lamports musi należeć do {}..={}",
                MIN_TIP_LAMPORTS,
                MAX_EXPLICIT_TIP_LAMPORTS
            );
        }
        Ok(Self {
            execute,
            max_pairs,
            tip_lamports,
        })
    }
}

#[derive(Debug, Serialize)]
struct StageZeroReport {
    stage: &'static str,
    nln_rpc_endpoint: &'static str,
    nln_grpc_endpoint: &'static str,
    direct_jito_transport: &'static str,
    nln_send_bundle_contract: &'static str,
    nln_tip_accounts: usize,
    direct_jito_tip_accounts: usize,
    tip_account_sets_match: bool,
    empty_send_bundle_rejected_without_submit: bool,
    max_pairs_requested: Option<usize>,
    configured_tip_lamports: u64,
    maximum_cost_per_transaction_lamports: u64,
    maximum_cost_for_requested_run_lamports: Option<u64>,
    declared_budget_lamports: u64,
    execute_requested: bool,
    no_on_chain_submit_performed: bool,
}

#[derive(Debug, Serialize)]
struct RunReport {
    benchmark: &'static str,
    tool_version: &'static str,
    started_unix_ms: u128,
    stage_zero: StageZeroReport,
    execution_scope: ExecutionScope,
    wallet: WalletReport,
    pairs: Vec<PairReport>,
    summaries: Vec<LaneSummary>,
    stopped_after_stage: String,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ExecutionScope {
    lanes: [&'static str; 2],
    direct_jito: &'static str,
    nln: &'static str,
    transaction_shape: &'static str,
    observer: &'static str,
    retry_rule: &'static str,
    primary_metric: &'static str,
    configured_tip_lamports: u64,
    active_ghost_runtime_note: &'static str,
}

#[derive(Debug, Serialize)]
struct WalletReport {
    fingerprint: String,
    balance_before_lamports: u64,
    balance_after_lamports: Option<u64>,
}

#[derive(Debug, Serialize)]
struct PairReport {
    pair: usize,
    shared_blockhash_context_slot: u64,
    shared_last_valid_block_height: u64,
    tip_account: String,
    submit_order: [&'static str; 2],
    submit_start_gap_ms: Option<f64>,
    pairing_quality: String,
    lanes: [LaneReport; 2],
}

#[derive(Debug, Serialize)]
struct LaneReport {
    lane: &'static str,
    submission_protocol: &'static str,
    signature: String,
    bundle_id: Option<String>,
    bundle_status: Option<String>,
    provider_submit_endpoint: Option<String>,
    provider_ack: bool,
    provider_ack_after_submit_ms: Option<f64>,
    submit_to_grpc_processed_ms: Option<f64>,
    ack_to_grpc_processed_ms: Option<f64>,
    submit_to_first_landed_seen_ms: Option<f64>,
    submit_to_finalized_seen_ms: Option<f64>,
    submit_slot_from_yellowstone: Option<u64>,
    grpc_processed_slot: Option<u64>,
    landed_slot: Option<u64>,
    finalized_slot: Option<u64>,
    transaction_fee_lamports: Option<u64>,
    tip_lamports: u64,
    priority_fee_upper_bound_lamports: u64,
    final_state: String,
    submit_error: Option<String>,
    observer_error: Option<String>,
    #[serde(skip)]
    submit_at: Option<Instant>,
    #[serde(skip)]
    ack_at: Option<Instant>,
}

#[derive(Debug, Serialize)]
struct LaneSummary {
    lane: &'static str,
    samples: usize,
    acked: usize,
    grpc_processed: usize,
    finalized: usize,
    ack_ms: Option<Stat>,
    submit_to_grpc_processed_ms: Option<Stat>,
    ack_to_grpc_processed_ms: Option<Stat>,
    submit_to_landed_seen_ms: Option<Stat>,
    submit_to_finalized_seen_ms: Option<Stat>,
}

#[derive(Debug, Serialize)]
struct Stat {
    samples: usize,
    min_ms: f64,
    mean_ms: f64,
    median_ms: f64,
    max_ms: f64,
    range_ms: f64,
}

#[derive(Debug)]
struct LatestBlockhash {
    hash: Hash,
    context_slot: u64,
    last_valid_block_height: u64,
}

struct NlnRpc {
    http: Client,
    api_key: String,
}

struct JitoStatus {
    http: Client,
    last_request: Option<Instant>,
}

#[derive(Default)]
struct ObserverState {
    latest_slot: Option<u64>,
    tracked: HashMap<String, Option<(Instant, u64)>>,
    error: Option<String>,
}

struct YellowstoneObserver {
    state: Arc<Mutex<ObserverState>>,
}

#[derive(Clone)]
struct NlnApiKeyInterceptor {
    header: MetadataKey<Ascii>,
    value: AsciiMetadataValue,
}

impl NlnApiKeyInterceptor {
    fn new(api_key: &str) -> Result<Self> {
        Ok(Self {
            header: MetadataKey::from_bytes(b"x-api-key").context("nazwa x-api-key")?,
            value: AsciiMetadataValue::try_from(api_key).context("wartość x-api-key")?,
        })
    }
}

impl Interceptor for NlnApiKeyInterceptor {
    fn call(&mut self, mut request: Request<()>) -> std::result::Result<Request<()>, Status> {
        request
            .metadata_mut()
            .insert(self.header.clone(), self.value.clone());
        Ok(request)
    }
}

// Minimalne typy protobuf wymagane dokładnie przez SearcherService/SendBundle.
// Są zgodne z protokołami używanymi przez trigger::jito_client, lecz utrzymane
// lokalnie, aby benchmark nie zmieniał publicznego API Triggera.
#[derive(Clone, PartialEq, Message)]
struct PacketMeta {
    #[prost(uint64, tag = "1")]
    size: u64,
    #[prost(string, tag = "2")]
    addr: String,
    #[prost(uint32, tag = "3")]
    port: u32,
    #[prost(message, optional, tag = "4")]
    flags: Option<PacketFlags>,
    #[prost(uint64, tag = "5")]
    sender_stake: u64,
}

#[derive(Clone, PartialEq, Message)]
struct PacketFlags {
    #[prost(bool, tag = "1")]
    discard: bool,
    #[prost(bool, tag = "2")]
    forwarded: bool,
    #[prost(bool, tag = "3")]
    repair: bool,
    #[prost(bool, tag = "4")]
    simple_vote_tx: bool,
    #[prost(bool, tag = "5")]
    tracer_packet: bool,
    #[prost(bool, tag = "6")]
    from_staked_node: bool,
}

#[derive(Clone, PartialEq, Message)]
struct Packet {
    #[prost(bytes = "vec", tag = "1")]
    data: Vec<u8>,
    #[prost(message, optional, tag = "2")]
    meta: Option<PacketMeta>,
}

#[derive(Clone, PartialEq, Message)]
struct Bundle {
    #[prost(message, repeated, tag = "3")]
    packets: Vec<Packet>,
}

#[derive(Clone, PartialEq, Message)]
struct SendBundleRequest {
    #[prost(message, optional, tag = "1")]
    bundle: Option<Bundle>,
}

#[derive(Clone, PartialEq, Message)]
struct SendBundleResponse {
    #[prost(string, tag = "1")]
    uuid: String,
}

#[derive(Clone, PartialEq, Message)]
struct GetTipAccountsRequest {}

#[derive(Clone, PartialEq, Message)]
struct GetTipAccountsResponse {
    #[prost(string, repeated, tag = "1")]
    accounts: Vec<String>,
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn wallet_fingerprint(pubkey: &Pubkey) -> String {
    let value = pubkey.to_string();
    format!("{}…{}", &value[..4], &value[value.len() - 4..])
}

fn priority_fee_upper_bound_lamports() -> u64 {
    (u64::from(COMPUTE_UNIT_LIMIT) * COMPUTE_UNIT_PRICE_MICRO_LAMPORTS).div_ceil(1_000_000)
}

fn maximum_cost_per_transaction(tip_lamports: u64) -> u64 {
    BASE_FEE_MAX_LAMPORTS + priority_fee_upper_bound_lamports() + tip_lamports
}

fn compact_error(error: impl std::fmt::Display) -> String {
    error.to_string().chars().take(360).collect()
}

fn stat(values: impl Iterator<Item = f64>) -> Option<Stat> {
    let mut values: Vec<f64> = values.filter(|value| value.is_finite()).collect();
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let len = values.len();
    let median = if len % 2 == 0 {
        (values[len / 2 - 1] + values[len / 2]) / 2.0
    } else {
        values[len / 2]
    };
    let min = values[0];
    let max = values[len - 1];
    Some(Stat {
        samples: len,
        min_ms: min,
        mean_ms: values.iter().sum::<f64>() / len as f64,
        median_ms: median,
        max_ms: max,
        range_ms: max - min,
    })
}

impl NlnRpc {
    fn new(api_key: String) -> Result<Self> {
        Ok(Self {
            http: Client::builder()
                .connect_timeout(Duration::from_secs(8))
                .timeout(Duration::from_secs(15))
                .tcp_nodelay(true)
                .build()
                .context("budowa persistent HTTP client NLN")?,
            api_key,
        })
    }

    async fn raw_call(&self, method: &str, params: Value) -> Result<Value> {
        let response = self
            .http
            .post(NLN_RPC_ENDPOINT)
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-api-key", &self.api_key)
            .json(&json!({"jsonrpc":"2.0", "id":1, "method":method, "params":params}))
            .send()
            .await
            .with_context(|| format!("NLN transport {method}"))?;
        let status = response.status();
        let text = response.text().await.context("odczyt NLN JSON-RPC")?;
        let value: Value = serde_json::from_str(&text).context("NLN zwrócił nie-JSON")?;
        // JSON-RPC walidacja `sendBundle` może prawidłowo używać HTTP 400 wraz
        // z ciałem JSON-RPC error. Stage 0 musi móc ten kontrakt odczytać bez
        // potraktowania go jako transportowej awarii.
        if status != StatusCode::OK && value.get("error").is_none() {
            bail!(
                "NLN HTTP {} dla {method}: {}",
                status.as_u16(),
                compact_json_error(&value)
            );
        }
        Ok(value)
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let value = self.raw_call(method, params).await?;
        if value.get("error").is_some() {
            bail!("NLN {method}: {}", compact_json_error(&value));
        }
        Ok(value)
    }
}

impl JitoStatus {
    fn new() -> Result<Self> {
        Ok(Self {
            http: Client::builder()
                .connect_timeout(Duration::from_secs(8))
                .timeout(Duration::from_secs(15))
                .tcp_nodelay(true)
                .build()
                .context("budowa persistent Jito status client")?,
            last_request: None,
        })
    }

    async fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        if let Some(last) = self.last_request {
            let elapsed = last.elapsed();
            if elapsed < JITO_STATUS_MIN_INTERVAL {
                sleep(JITO_STATUS_MIN_INTERVAL - elapsed).await;
            }
        }
        self.last_request = Some(Instant::now());
        let response = self
            .http
            .post(JITO_FRANKFURT_HTTP)
            .header(header::CONTENT_TYPE, "application/json")
            .json(&json!({"jsonrpc":"2.0", "id":1, "method":method, "params":params}))
            .send()
            .await
            .with_context(|| format!("Jito Frankfurt HTTP {method}"))?;
        let status = response.status();
        let text = response.text().await.context("odczyt Jito status JSON")?;
        let value: Value = serde_json::from_str(&text).context("Jito status zwrócił nie-JSON")?;
        if status != StatusCode::OK || value.get("error").is_some() {
            bail!("Jito status {method}: {}", compact_json_error(&value));
        }
        Ok(value)
    }
}

fn compact_json_error(value: &Value) -> String {
    value
        .get("error")
        .map(|error| error.to_string())
        .unwrap_or_else(|| value.to_string())
        .chars()
        .take(360)
        .collect()
}

async fn jito_channel(endpoint: &str) -> Result<Channel> {
    let endpoint = Endpoint::from_shared(endpoint.to_owned())
        .context("nieprawidłowy Jito gRPC endpoint")?
        .tls_config(ClientTlsConfig::new())
        .context("TLS Jito gRPC")?
        .tcp_nodelay(true)
        .connect_timeout(JITO_GRPC_TIMEOUT)
        .timeout(JITO_GRPC_TIMEOUT);
    endpoint.connect().await.context("połączenie Jito gRPC")
}

fn authenticated_request<T>(message: T, auth: Option<&str>) -> Result<Request<T>> {
    let mut request = Request::new(message);
    if let Some(auth) = auth.map(str::trim).filter(|value| !value.is_empty()) {
        request.metadata_mut().insert(
            "x-jito-auth",
            MetadataValue::try_from(auth).context("nieprawidłowe x-jito-auth")?,
        );
    }
    Ok(request)
}

async fn jito_grpc_tip_accounts(auth: Option<&str>) -> Result<Vec<String>> {
    let channel = jito_channel(JITO_FRANKFURT_GRPC).await?;
    let mut client = tonic::client::Grpc::new(channel);
    client
        .ready()
        .await
        .map_err(|error| anyhow!("Jito gRPC GetTipAccounts client not ready: {error}"))?;
    let response = client
        .unary(
            authenticated_request(GetTipAccountsRequest {}, auth)?,
            PathAndQuery::from_static("/searcher.SearcherService/GetTipAccounts"),
            tonic::codec::ProstCodec::<GetTipAccountsRequest, GetTipAccountsResponse>::default(),
        )
        .await
        .context("Jito gRPC GetTipAccounts")?;
    Ok(response.into_inner().accounts)
}

async fn direct_jito_send_bundle(
    serialized: Vec<u8>,
    auth: Option<&str>,
) -> Result<(String, String)> {
    let request = SendBundleRequest {
        bundle: Some(Bundle {
            packets: vec![Packet {
                meta: Some(PacketMeta {
                    size: serialized.len() as u64,
                    addr: String::new(),
                    port: 0,
                    flags: None,
                    sender_stake: 0,
                }),
                data: serialized,
            }],
        }),
    };

    // Ten sam porządek Frankfurt → Amsterdam → London → Dublin i trzy rundy,
    // co aktualny trigger::JitoClient. Przy zwykłym ACK kończy się na pierwszej
    // próbie; ewentualny failover zostaje jawnie zapisany w raporcie.
    let mut last_error = None;
    for round in 0..3 {
        for endpoint in JITO_EU_GRPC_FAILOVER {
            match jito_channel(endpoint).await {
                Ok(channel) => {
                    let mut client = tonic::client::Grpc::new(channel);
                    if let Err(error) = client.ready().await {
                        last_error = Some(format!("{endpoint}: client not ready: {error}"));
                        continue;
                    }
                    match client
                        .unary(
                            authenticated_request(request.clone(), auth)?,
                            PathAndQuery::from_static("/searcher.SearcherService/SendBundle"),
                            tonic::codec::ProstCodec::<SendBundleRequest, SendBundleResponse>::default(),
                        )
                        .await
                    {
                        Ok(response) => {
                            let bundle_id = response.into_inner().uuid;
                            if bundle_id.is_empty() {
                                bail!("Jito gRPC zwróciło pusty bundle UUID");
                            }
                            return Ok((bundle_id, (*endpoint).to_owned()));
                        }
                        Err(error) => last_error = Some(format!("{endpoint}: {error}")),
                    }
                }
                Err(error) => last_error = Some(format!("{endpoint}: {error:#}")),
            }
        }
        if round < 2 {
            sleep(Duration::from_millis(600_u64 << round)).await;
        }
    }
    bail!(
        "Direct Jito gRPC bez ACK po failover: {}",
        last_error.unwrap_or_default()
    )
}

fn observer_request(payer: &Pubkey) -> SubscribeRequest {
    let mut transactions = HashMap::new();
    transactions.insert(
        "test_payer_transactions".to_owned(),
        SubscribeRequestFilterTransactions {
            vote: Some(false),
            failed: None,
            account_include: vec![payer.to_string()],
            account_exclude: vec![],
            account_required: vec![],
            signature: None,
        },
    );
    let mut slots = HashMap::new();
    slots.insert(
        "processed_slots".to_owned(),
        SubscribeRequestFilterSlots {
            filter_by_commitment: Some(true),
        },
    );
    SubscribeRequest {
        accounts: HashMap::new(),
        slots,
        transactions,
        transactions_status: HashMap::new(),
        blocks: HashMap::new(),
        blocks_meta: HashMap::new(),
        entry: HashMap::new(),
        commitment: Some(CommitmentLevel::Processed as i32),
        accounts_data_slice: vec![],
        ping: None,
    }
}

async fn nln_yellowstone_client(api_key: &str) -> Result<GeyserGrpcClient<NlnApiKeyInterceptor>> {
    let endpoint = Endpoint::from_shared(NLN_GRPC_ENDPOINT.to_owned())
        .context("budowa NLN Yellowstone endpoint")?
        .http2_adaptive_window(true)
        .initial_connection_window_size(1 << 26)
        .initial_stream_window_size(1 << 25)
        .keep_alive_while_idle(true)
        .http2_keep_alive_interval(Duration::from_secs(10))
        .keep_alive_timeout(Duration::from_secs(5))
        .tcp_nodelay(true)
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(30));
    let channel = endpoint
        .connect()
        .await
        .context("połączenie NLN Yellowstone")?;
    let interceptor = NlnApiKeyInterceptor::new(api_key)?;
    let health = HealthClient::with_interceptor(channel.clone(), interceptor.clone());
    let geyser = GeyserClient::with_interceptor(channel, interceptor);
    Ok(GeyserGrpcClient::new(health, geyser))
}

impl YellowstoneObserver {
    async fn start(api_key: &str, payer: &Pubkey) -> Result<Self> {
        let mut client = nln_yellowstone_client(api_key).await?;
        let mut stream = client
            .subscribe_once(observer_request(payer))
            .await
            .context("subskrypcja Yellowstone observer")?;
        let state = Arc::new(Mutex::new(ObserverState::default()));
        let task_state = Arc::clone(&state);
        tokio::spawn(async move {
            while let Some(next) = stream.next().await {
                match next {
                    Ok(update) => match update.update_oneof {
                        Some(UpdateOneof::Slot(slot)) => {
                            task_state.lock().latest_slot = Some(slot.slot);
                        }
                        Some(UpdateOneof::Transaction(transaction)) => {
                            let Some(info) = transaction.transaction else {
                                continue;
                            };
                            let signature = bs58::encode(info.signature).into_string();
                            let mut state = task_state.lock();
                            if let Some(observed) = state.tracked.get_mut(&signature) {
                                if observed.is_none() {
                                    *observed = Some((Instant::now(), transaction.slot));
                                }
                            }
                        }
                        _ => {}
                    },
                    Err(error) => {
                        task_state.lock().error = Some(compact_error(error));
                        break;
                    }
                }
            }
        });
        Ok(Self { state })
    }

    fn register(&self, signature: &Signature) {
        self.state
            .lock()
            .tracked
            .insert(signature.to_string(), None);
    }

    fn submit_slot(&self) -> Option<u64> {
        self.state.lock().latest_slot
    }

    fn stream_error(&self) -> Option<String> {
        self.state.lock().error.clone()
    }

    fn apply(&self, lane: &mut LaneReport) {
        let state = self.state.lock();
        lane.observer_error = state.error.clone();
        let Some(Some((observed_at, slot))) = state.tracked.get(&lane.signature) else {
            return;
        };
        let Some(submit_at) = lane.submit_at else {
            return;
        };
        if lane.submit_to_grpc_processed_ms.is_none() {
            lane.submit_to_grpc_processed_ms = Some(signed_ms(*observed_at, submit_at));
            lane.grpc_processed_slot = Some(*slot);
        }
        if let Some(ack_at) = lane.ack_at {
            lane.ack_to_grpc_processed_ms = Some(signed_ms(*observed_at, ack_at));
        }
        if lane.final_state == "ACK_ONLY" {
            lane.final_state = "PROCESSED".to_owned();
        }
    }
}

fn signed_ms(later: Instant, earlier: Instant) -> f64 {
    if later >= earlier {
        later.duration_since(earlier).as_secs_f64() * 1_000.0
    } else {
        -(earlier.duration_since(later).as_secs_f64() * 1_000.0)
    }
}

async fn wait_for_observer_slot(observer: &YellowstoneObserver) -> Result<()> {
    let started = Instant::now();
    while observer.submit_slot().is_none() {
        if let Some(error) = observer.stream_error() {
            bail!("Yellowstone observer zakończył się przed benchmarkiem: {error}");
        }
        if started.elapsed() > Duration::from_secs(6) {
            bail!("Yellowstone observer nie dostarczył slotu przed benchmarkiem");
        }
        sleep(Duration::from_millis(20)).await;
    }
    Ok(())
}

async fn latest_blockhash(rpc: &NlnRpc) -> Result<LatestBlockhash> {
    let value = rpc
        .call("getLatestBlockhash", json!([{"commitment":"processed"}]))
        .await?;
    let result = value
        .get("result")
        .context("brak result getLatestBlockhash")?;
    Ok(LatestBlockhash {
        hash: Hash::from_str(
            result
                .pointer("/value/blockhash")
                .and_then(Value::as_str)
                .context("brak blockhash")?,
        )
        .context("nieprawidłowy blockhash")?,
        context_slot: result
            .pointer("/context/slot")
            .and_then(Value::as_u64)
            .context("brak blockhash context slot")?,
        last_valid_block_height: result
            .pointer("/value/lastValidBlockHeight")
            .and_then(Value::as_u64)
            .context("brak lastValidBlockHeight")?,
    })
}

async fn balance(rpc: &NlnRpc, payer: &Pubkey) -> Result<u64> {
    let value = rpc
        .call(
            "getBalance",
            json!([payer.to_string(), {"commitment":"confirmed"}]),
        )
        .await?;
    value
        .pointer("/result/value")
        .and_then(Value::as_u64)
        .context("brak result.value getBalance")
}

fn build_transaction(
    payer: &Keypair,
    memo_lane: &str,
    pair: usize,
    tip_account: &Pubkey,
    tip_lamports: u64,
    blockhash: Hash,
) -> Result<(Transaction, Signature)> {
    let memo_program = Pubkey::from_str(MEMO_PROGRAM_ID).context("Memo program id")?;
    // JTO i NLN mają identyczną długość, więc nie powodują różnicy rozmiaru TX.
    let memo = format!("NLN_BENCH_V2:{memo_lane}:PAIR_{pair:02}");
    let instructions = vec![
        ComputeBudgetInstruction::set_compute_unit_limit(COMPUTE_UNIT_LIMIT),
        ComputeBudgetInstruction::set_compute_unit_price(COMPUTE_UNIT_PRICE_MICRO_LAMPORTS),
        Instruction {
            program_id: memo_program,
            accounts: vec![AccountMeta::new_readonly(payer.pubkey(), true)],
            data: memo.into_bytes(),
        },
        system_instruction::transfer(&payer.pubkey(), tip_account, tip_lamports),
    ];
    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &[payer],
        blockhash,
    );
    let signature = transaction
        .signatures
        .first()
        .copied()
        .context("brak podpisu TX")?;
    Ok((transaction, signature))
}

fn serialized(transaction: &Transaction) -> Result<Vec<u8>> {
    bincode::serialize(transaction).context("serializacja VersionedTransaction")
}

async fn simulate_serialized_transaction(
    rpc: &NlnRpc,
    bytes: &[u8],
    label: &str,
) -> Result<Option<u64>> {
    let value = rpc
        .call(
            "simulateTransaction",
            json!([
                BASE64.encode(bytes),
                {
                    "encoding":"base64",
                    "sigVerify":true,
                    "replaceRecentBlockhash":false,
                    "commitment":"processed"
                }
            ]),
        )
        .await
        .with_context(|| format!("Stage 0 simulateTransaction {label}"))?;
    let result = value
        .pointer("/result/value")
        .context("simulateTransaction bez result.value")?;
    if let Some(error) = result.get("err").filter(|error| !error.is_null()) {
        let logs = result
            .get("logs")
            .map(|logs| logs.to_string())
            .unwrap_or_default();
        bail!("Stage 0 simulation {label} failed: {error}; logs={logs}");
    }
    Ok(result.get("unitsConsumed").and_then(Value::as_u64))
}

fn new_lane(
    lane: &'static str,
    signature: Signature,
    slot: Option<u64>,
    tip_lamports: u64,
) -> LaneReport {
    LaneReport {
        lane,
        submission_protocol: if lane == "DIRECT_JITO_GRPC" {
            "SearcherService/SendBundle over fresh TLS gRPC channel; same EU failover order as trigger::JitoClient"
        } else {
            "NLN authenticated JSON-RPC sendBundle over persistent warmed HTTPS client"
        },
        signature: signature.to_string(),
        bundle_id: None,
        bundle_status: None,
        provider_submit_endpoint: None,
        provider_ack: false,
        provider_ack_after_submit_ms: None,
        submit_to_grpc_processed_ms: None,
        ack_to_grpc_processed_ms: None,
        submit_to_first_landed_seen_ms: None,
        submit_to_finalized_seen_ms: None,
        submit_slot_from_yellowstone: slot,
        grpc_processed_slot: None,
        landed_slot: None,
        finalized_slot: None,
        transaction_fee_lamports: None,
        tip_lamports,
        priority_fee_upper_bound_lamports: priority_fee_upper_bound_lamports(),
        final_state: "PREPARED".to_owned(),
        submit_error: None,
        observer_error: None,
        submit_at: None,
        ack_at: None,
    }
}

async fn submit_direct(
    lane: &mut LaneReport,
    bytes: Vec<u8>,
    observer: &YellowstoneObserver,
    auth: Option<&str>,
) {
    lane.submit_slot_from_yellowstone = observer.submit_slot();
    let started = Instant::now();
    lane.submit_at = Some(started);
    match direct_jito_send_bundle(bytes, auth).await {
        Ok((bundle_id, endpoint)) => {
            let ack = Instant::now();
            lane.provider_ack = true;
            lane.bundle_id = Some(bundle_id);
            lane.provider_submit_endpoint = Some(endpoint);
            lane.provider_ack_after_submit_ms = Some(signed_ms(ack, started));
            lane.ack_at = Some(ack);
            lane.final_state = "ACK_ONLY".to_owned();
        }
        Err(error) => {
            lane.final_state = "FAILED".to_owned();
            lane.submit_error = Some(compact_error(error));
        }
    }
}

async fn submit_nln(
    lane: &mut LaneReport,
    bytes: Vec<u8>,
    observer: &YellowstoneObserver,
    rpc: &NlnRpc,
) {
    lane.submit_slot_from_yellowstone = observer.submit_slot();
    let started = Instant::now();
    lane.submit_at = Some(started);
    match rpc
        .call(
            "sendBundle",
            json!([[BASE64.encode(bytes)], {"encoding":"base64"}]),
        )
        .await
    {
        Ok(value) => match value.get("result").and_then(Value::as_str) {
            Some(bundle_id) if !bundle_id.is_empty() => {
                let ack = Instant::now();
                lane.provider_ack = true;
                lane.bundle_id = Some(bundle_id.to_owned());
                lane.provider_submit_endpoint = Some(NLN_RPC_ENDPOINT.to_owned());
                lane.provider_ack_after_submit_ms = Some(signed_ms(ack, started));
                lane.ack_at = Some(ack);
                lane.final_state = "ACK_ONLY".to_owned();
            }
            _ => {
                lane.final_state = "FAILED".to_owned();
                lane.submit_error = Some("NLN sendBundle nie zwrócił bundle_id".to_owned());
            }
        },
        Err(error) => {
            lane.final_state = "FAILED".to_owned();
            lane.submit_error = Some(compact_error(error));
        }
    }
}

fn apply_signature_status(lane: &mut LaneReport, status: Option<&Value>) {
    let Some(status) = status.filter(|value| !value.is_null()) else {
        return;
    };
    let now = Instant::now();
    let Some(submit_at) = lane.submit_at else {
        return;
    };
    if lane.submit_to_first_landed_seen_ms.is_none() {
        lane.submit_to_first_landed_seen_ms = Some(signed_ms(now, submit_at));
        lane.landed_slot = status.get("slot").and_then(Value::as_u64);
    }
    if let Some(error) = status.get("err").filter(|error| !error.is_null()) {
        lane.final_state = "FAILED".to_owned();
        lane.submit_error.get_or_insert_with(|| error.to_string());
        return;
    }
    match status.get("confirmationStatus").and_then(Value::as_str) {
        Some("finalized") => {
            lane.finalized_slot = status.get("slot").and_then(Value::as_u64);
            if lane.submit_to_finalized_seen_ms.is_none() {
                lane.submit_to_finalized_seen_ms = Some(signed_ms(now, submit_at));
            }
            lane.final_state = "FINALIZED".to_owned();
        }
        Some("confirmed") if lane.final_state != "FINALIZED" => {
            lane.final_state = "CONFIRMED".to_owned();
        }
        Some("processed") if lane.final_state == "ACK_ONLY" => {
            lane.final_state = "PROCESSED".to_owned();
        }
        _ => {}
    }
}

async fn update_bundle_status(lane: &mut LaneReport, rpc: &NlnRpc, jito_status: &mut JitoStatus) {
    let Some(bundle_id) = lane.bundle_id.clone() else {
        return;
    };
    let response = if lane.lane == "DIRECT_JITO_GRPC" {
        jito_status
            .call("getBundleStatuses", json!([[bundle_id]]))
            .await
    } else {
        rpc.call("getBundleStatuses", json!([[bundle_id]])).await
    };
    match response {
        Ok(value) => {
            if let Some(status) = value
                .pointer("/result/value/0")
                .filter(|value| !value.is_null())
            {
                lane.bundle_status = Some(status.to_string().chars().take(480).collect());
                if lane.landed_slot.is_none() {
                    lane.landed_slot = status
                        .get("slot")
                        .or_else(|| status.get("landed_slot"))
                        .and_then(Value::as_u64);
                }
            }
        }
        Err(error) => {
            lane.observer_error
                .get_or_insert_with(|| format!("bundle status: {}", compact_error(error)));
        }
    }
}

async fn set_fee_if_finalized(lane: &mut LaneReport, rpc: &NlnRpc) {
    if lane.final_state != "FINALIZED" || lane.transaction_fee_lamports.is_some() {
        return;
    }
    let Ok(value) = rpc
        .call(
            "getTransaction",
            json!([lane.signature, {"encoding":"json", "commitment":"finalized", "maxSupportedTransactionVersion":0}]),
        )
        .await
    else {
        return;
    };
    lane.transaction_fee_lamports = value.pointer("/result/meta/fee").and_then(Value::as_u64);
}

async fn reconcile_pair(
    pair: &mut PairReport,
    rpc: &NlnRpc,
    jito_status: &mut JitoStatus,
    observer: &YellowstoneObserver,
) -> Result<()> {
    let deadline = Instant::now() + RECONCILIATION_TIMEOUT;
    let mut last_bundle_lookup = None;
    loop {
        for lane in &mut pair.lanes {
            observer.apply(lane);
        }
        let signatures: Vec<String> = pair
            .lanes
            .iter()
            .map(|lane| lane.signature.clone())
            .collect();
        let result = match rpc
            .call(
                "getSignatureStatuses",
                json!([signatures, {"searchTransactionHistory":false}]),
            )
            .await
        {
            Ok(result) => result,
            Err(error) => {
                let diagnostic = format!(
                    "signature status (benchmark stopped): {}",
                    compact_error(error)
                );
                for lane in &mut pair.lanes {
                    lane.observer_error
                        .get_or_insert_with(|| diagnostic.clone());
                    if !matches!(lane.final_state.as_str(), "FINALIZED" | "FAILED") {
                        lane.final_state = "UNKNOWN".to_owned();
                    }
                }
                break;
            }
        };
        let statuses = result
            .pointer("/result/value")
            .and_then(Value::as_array)
            .context("getSignatureStatuses bez result.value")?;
        for (lane, status) in pair.lanes.iter_mut().zip(statuses.iter()) {
            apply_signature_status(lane, Some(status));
        }

        let due_bundle = last_bundle_lookup
            .map(|last: Instant| last.elapsed() >= JITO_STATUS_MIN_INTERVAL)
            .unwrap_or(true);
        if due_bundle {
            for lane in &mut pair.lanes {
                update_bundle_status(lane, rpc, jito_status).await;
            }
            last_bundle_lookup = Some(Instant::now());
        }

        if pair
            .lanes
            .iter()
            .all(|lane| matches!(lane.final_state.as_str(), "FINALIZED" | "FAILED"))
        {
            break;
        }
        if Instant::now() >= deadline {
            for lane in &mut pair.lanes {
                if !matches!(lane.final_state.as_str(), "FINALIZED" | "FAILED") {
                    lane.final_state = "UNKNOWN".to_owned();
                }
            }
            break;
        }
        sleep(STATUS_POLL_INTERVAL).await;
    }
    for lane in &mut pair.lanes {
        observer.apply(lane);
        set_fee_if_finalized(lane, rpc).await;
    }
    Ok(())
}

fn stage_one_valid(pair: &PairReport) -> bool {
    pair.lanes.iter().all(|lane| {
        lane.provider_ack
            && lane.bundle_id.is_some()
            && lane.final_state == "FINALIZED"
            && lane.submit_to_grpc_processed_ms.is_some()
    })
}

fn summaries(pairs: &[PairReport]) -> Vec<LaneSummary> {
    ["DIRECT_JITO_GRPC", "NLN_SENDBUNDLE"]
        .into_iter()
        .map(|lane_name| {
            let lanes: Vec<&LaneReport> = pairs
                .iter()
                .flat_map(|pair| pair.lanes.iter())
                .filter(|lane| lane.lane == lane_name)
                .collect();
            LaneSummary {
                lane: lane_name,
                samples: lanes.len(),
                acked: lanes.iter().filter(|lane| lane.provider_ack).count(),
                grpc_processed: lanes
                    .iter()
                    .filter(|lane| lane.submit_to_grpc_processed_ms.is_some())
                    .count(),
                finalized: lanes
                    .iter()
                    .filter(|lane| lane.final_state == "FINALIZED")
                    .count(),
                ack_ms: stat(
                    lanes
                        .iter()
                        .filter_map(|lane| lane.provider_ack_after_submit_ms),
                ),
                submit_to_grpc_processed_ms: stat(
                    lanes
                        .iter()
                        .filter_map(|lane| lane.submit_to_grpc_processed_ms),
                ),
                ack_to_grpc_processed_ms: stat(
                    lanes
                        .iter()
                        .filter_map(|lane| lane.ack_to_grpc_processed_ms),
                ),
                submit_to_landed_seen_ms: stat(
                    lanes
                        .iter()
                        .filter_map(|lane| lane.submit_to_first_landed_seen_ms),
                ),
                submit_to_finalized_seen_ms: stat(
                    lanes
                        .iter()
                        .filter_map(|lane| lane.submit_to_finalized_seen_ms),
                ),
            }
        })
        .collect()
}

async fn stage_zero(
    rpc: &NlnRpc,
    auth: Option<&str>,
    args: &Args,
    budget: u64,
) -> Result<StageZeroReport> {
    let nln_tips = rpc.call("getTipAccounts", json!([])).await?;
    let nln_tips: Vec<String> = nln_tips
        .get("result")
        .and_then(Value::as_array)
        .context("NLN getTipAccounts bez tablicy")?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let direct_tips = jito_grpc_tip_accounts(auth).await?;
    let mut nln_sorted = nln_tips.clone();
    let mut direct_sorted = direct_tips.clone();
    nln_sorted.sort();
    direct_sorted.sort();
    let empty_submit = rpc
        .raw_call("sendBundle", json!([[], {"encoding":"base64"}]))
        .await?;
    let empty_rejected = empty_submit
        .pointer("/error/message")
        .and_then(Value::as_str)
        .map(|message| message.contains("no transactions"))
        .unwrap_or(false);
    if !empty_rejected {
        bail!("Stage 0: NLN sendBundle nie zweryfikował pustego bundle oczekiwanym błędem");
    }
    let max_run = args
        .max_pairs
        .map(|pairs| pairs as u64 * 2 * maximum_cost_per_transaction(args.tip_lamports));
    Ok(StageZeroReport {
        stage: "STAGE_0_NO_CHAIN_SUBMIT",
        nln_rpc_endpoint: NLN_RPC_ENDPOINT,
        nln_grpc_endpoint: NLN_GRPC_ENDPOINT,
        direct_jito_transport: "Jito SearcherService/SendBundle, TLS gRPC, fresh channel per submit",
        nln_send_bundle_contract: "POST rpc.nln.clr3.org + x-api-key, JSON-RPC method sendBundle, params [[base64 signed tx], {encoding:base64}], bundle_id result",
        nln_tip_accounts: nln_tips.len(),
        direct_jito_tip_accounts: direct_tips.len(),
        tip_account_sets_match: nln_sorted == direct_sorted,
        empty_send_bundle_rejected_without_submit: true,
        max_pairs_requested: args.max_pairs,
        configured_tip_lamports: args.tip_lamports,
        maximum_cost_per_transaction_lamports: maximum_cost_per_transaction(args.tip_lamports),
        maximum_cost_for_requested_run_lamports: max_run,
        declared_budget_lamports: budget,
        execute_requested: args.execute,
        no_on_chain_submit_performed: !args.execute,
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse()?;
    let budget: u64 = env::var(BUDGET_ENV)
        .with_context(|| format!("brak obowiązkowego {BUDGET_ENV}"))?
        .parse()
        .with_context(|| format!("{BUDGET_ENV} musi być liczbą lamportów"))?;
    if budget == 0 {
        bail!("{BUDGET_ENV} musi być dodatni");
    }
    let api_key = env::var(NLN_API_KEY_ENV).with_context(|| format!("brak {NLN_API_KEY_ENV}"))?;
    let keypair_path = env::var(KEYPAIR_ENV).with_context(|| format!("brak {KEYPAIR_ENV}"))?;
    let payer = read_keypair_file(&keypair_path)
        .map_err(|error| anyhow!("nie można odczytać testowego keypaira: {error}"))?;
    let jito_auth = env::var(JITO_AUTH_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty());
    let rpc = NlnRpc::new(api_key.clone())?;
    let mut jito_status = JitoStatus::new()?;
    let stage_zero = stage_zero(&rpc, jito_auth.as_deref(), &args, budget).await?;
    if !stage_zero.tip_account_sets_match {
        bail!("Stage 0: zestawy kont tip Direct Jito i NLN są różne; nie porównuję lane");
    }
    if !args.execute {
        println!("{}", serde_json::to_string_pretty(&stage_zero)?);
        return Ok(());
    }
    let max_pairs = args.max_pairs.context("--max-pairs")?;
    let max_cost = stage_zero
        .maximum_cost_for_requested_run_lamports
        .context("maxymalny koszt runu")?;
    if max_cost > budget {
        bail!("ABORT: maksymalny koszt {max_cost} przekracza {BUDGET_ENV}={budget}");
    }
    let balance_before = balance(&rpc, &payer.pubkey()).await?;
    if balance_before < max_cost {
        bail!("ABORT: saldo test walleta {balance_before} < maksymalny koszt {max_cost}");
    }

    let observer = YellowstoneObserver::start(&api_key, &payer.pubkey()).await?;
    wait_for_observer_slot(&observer).await?;
    let tip_accounts = rpc.call("getTipAccounts", json!([])).await?;
    let tip_accounts: Vec<Pubkey> = tip_accounts
        .get("result")
        .and_then(Value::as_array)
        .context("brak result getTipAccounts")?
        .iter()
        .filter_map(Value::as_str)
        .map(Pubkey::from_str)
        .collect::<std::result::Result<_, _>>()
        .context("nieprawidłowe konto tip")?;
    if tip_accounts.is_empty() {
        bail!("NLN getTipAccounts zwrócił pustą listę");
    }

    let started_unix_ms = now_unix_ms();
    let mut pairs = Vec::new();
    let mut stopped_after_stage = "STAGE_2_COMPLETE".to_owned();
    for pair_number in 1..=max_pairs {
        let latest = latest_blockhash(&rpc).await?;
        let tip_account = tip_accounts[(pair_number - 1) % tip_accounts.len()];
        let (jito_tx, jito_signature) = build_transaction(
            &payer,
            "JTO",
            pair_number,
            &tip_account,
            args.tip_lamports,
            latest.hash,
        )?;
        let (nln_tx, nln_signature) = build_transaction(
            &payer,
            "NLN",
            pair_number,
            &tip_account,
            args.tip_lamports,
            latest.hash,
        )?;
        let jito_bytes = serialized(&jito_tx)?;
        let nln_bytes = serialized(&nln_tx)?;
        // Dry-run bezpośrednio przed pierwszym dozwolonym submitiem nowego
        // blockhasha. Wynik jest poza oknem metryki i nie może zostać
        // potraktowany jako landing.
        simulate_serialized_transaction(&rpc, &jito_bytes, "DIRECT_JITO_GRPC").await?;
        simulate_serialized_transaction(&rpc, &nln_bytes, "NLN_SENDBUNDLE").await?;
        observer.register(&jito_signature);
        observer.register(&nln_signature);
        let order = if pair_number % 2 == 1 {
            ["DIRECT_JITO_GRPC", "NLN_SENDBUNDLE"]
        } else {
            ["NLN_SENDBUNDLE", "DIRECT_JITO_GRPC"]
        };
        let mut jito_lane = new_lane(
            "DIRECT_JITO_GRPC",
            jito_signature,
            observer.submit_slot(),
            args.tip_lamports,
        );
        let mut nln_lane = new_lane(
            "NLN_SENDBUNDLE",
            nln_signature,
            observer.submit_slot(),
            args.tip_lamports,
        );
        if order[0] == "DIRECT_JITO_GRPC" {
            tokio::join!(
                submit_direct(&mut jito_lane, jito_bytes, &observer, jito_auth.as_deref()),
                async {
                    sleep(Duration::from_millis(5)).await;
                    submit_nln(&mut nln_lane, nln_bytes, &observer, &rpc).await;
                }
            );
        } else {
            tokio::join!(
                submit_nln(&mut nln_lane, nln_bytes, &observer, &rpc),
                async {
                    sleep(Duration::from_millis(5)).await;
                    submit_direct(&mut jito_lane, jito_bytes, &observer, jito_auth.as_deref())
                        .await;
                }
            );
        }
        let starts = [jito_lane.submit_at, nln_lane.submit_at];
        let gap = match (starts[0], starts[1]) {
            (Some(first), Some(second)) => Some(signed_ms(second, first).abs()),
            _ => None,
        };
        let mut pair = PairReport {
            pair: pair_number,
            shared_blockhash_context_slot: latest.context_slot,
            shared_last_valid_block_height: latest.last_valid_block_height,
            tip_account: tip_account.to_string(),
            submit_order: order,
            submit_start_gap_ms: gap,
            pairing_quality: if gap.map(|value| value <= 25.0).unwrap_or(false) {
                "OK".to_owned()
            } else {
                "WEAK_GAP_OVER_25MS_OR_SUBMIT_FAILURE".to_owned()
            },
            lanes: [jito_lane, nln_lane],
        };
        reconcile_pair(&mut pair, &rpc, &mut jito_status, &observer).await?;
        let stage_one_ok = pair_number != 1 || stage_one_valid(&pair);
        pairs.push(pair);
        if !stage_one_ok {
            stopped_after_stage = "STAGE_1_STOP_INVALID_OR_UNCERTAIN".to_owned();
            break;
        }
    }
    let balance_after = balance(&rpc, &payer.pubkey()).await.ok();
    let report = RunReport {
        benchmark: "direct_jito_grpc_vs_nln_sendbundle_latency_v2",
        tool_version: "2026-07-23.0",
        started_unix_ms,
        stage_zero,
        execution_scope: ExecutionScope {
            lanes: ["DIRECT_JITO_GRPC", "NLN_SENDBUNDLE"],
            direct_jito: "Fresh TLS gRPC channel per submit and the exact Frankfurt→Amsterdam→London→Dublin failover ordering from trigger::JitoClient. No new Ghost runtime wiring.",
            nln: "Authenticated NLN JSON-RPC sendBundle; no sendTransaction/sendRawTransaction fallback.",
            transaction_shape: "One signed test transaction per lane: 50k CU limit, 1k micro-lamports/CU, same explicitly configured inline Jito tip, same payer/blockhash/tip account, equal-length lane Memo.",
            observer: "One persistent processed Yellowstone stream, payer-filtered and locally restricted to the six-or-fewer registered test signatures; it records receipt Instant and slot only.",
            retry_rule: "No application-level re-submit after ACK. Direct Jito preserves its production transport failover only if no ACK is received; any such path is visible by submit endpoint/error.",
            primary_metric: "submit_started → first matching Yellowstone processed transaction; RPC/status polling is reconciliation only.",
            configured_tip_lamports: args.tip_lamports,
            active_ghost_runtime_note: "Current ghost-launcher BUY/SELL code is Sender-only; this measures the retained trigger::JitoClient-compatible Direct Jito transport, not an active Ghost BUY/SELL lane.",
        },
        wallet: WalletReport {
            fingerprint: wallet_fingerprint(&payer.pubkey()),
            balance_before_lamports: balance_before,
            balance_after_lamports: balance_after,
        },
        summaries: summaries(&pairs),
        pairs,
        stopped_after_stage,
        notes: vec![
            "T0 chain → local Yellowstone cannot be measured without a provider-created source timestamp; stream receipt is the first locally observable processed event.".to_owned(),
            "ACK proves provider receipt, not landing. Inflight Invalid is not treated as terminal failure; only chain status/error and bounded reconciliation classify the attempt.".to_owned(),
            "n <= 5: the report exposes raw trials plus min/mean/median/max/range, never p95/p99.".to_owned(),
        ],
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_ceiling_is_small_and_exact() {
        assert_eq!(priority_fee_upper_bound_lamports(), 50);
        assert_eq!(maximum_cost_per_transaction(MIN_TIP_LAMPORTS), 6_050);
    }

    #[test]
    fn summary_uses_median_not_percentiles() {
        let result = stat([1.0, 4.0, 2.0].into_iter()).expect("stat");
        assert_eq!(result.median_ms, 2.0);
        assert_eq!(result.range_ms, 3.0);
    }

    #[test]
    fn final_status_is_not_created_from_ack() {
        let signature = Signature::new_unique();
        let mut lane = new_lane("NLN_SENDBUNDLE", signature, Some(1), MIN_TIP_LAMPORTS);
        lane.submit_at = Some(Instant::now());
        lane.final_state = "ACK_ONLY".to_owned();
        apply_signature_status(&mut lane, None);
        assert_eq!(lane.final_state, "ACK_ONLY");
    }
}
