//! Krótki, izolowany benchmark transportu NLN Yellowstone gRPC i HTTP JSON-RPC.
//!
//! Uruchomienie (klucz pozostaje poza repo):
//! `NLN_BENCHMARK_API_KEY=... cargo run -p seer --example nln_latency_benchmark`
//!
//! Narzędzie nie uruchamia Ghosta, nie buduje transakcji, niczego nie wysyła na
//! łańcuch i nie zapisuje sekretu ani odpowiedzi providera na dysk. Wynik JSON
//! trafia wyłącznie na stdout, aby operator mógł zarchiwizować wyłącznie raport
//! z zagregowanymi metrykami.

use std::{
    collections::{BTreeSet, HashMap},
    env,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, Context, Result};
use futures::StreamExt;
use serde::Serialize;
use serde_json::{json, Value};
use tokio::time::sleep;
use tonic::{
    metadata::{Ascii, AsciiMetadataValue, MetadataKey},
    service::Interceptor,
    transport::Endpoint,
    Request, Status,
};
use tonic_health::pb::health_client::HealthClient;
use yellowstone_grpc_client::GeyserGrpcClient;
use yellowstone_grpc_proto::prelude::{
    geyser_client::GeyserClient, subscribe_update::UpdateOneof, CommitmentLevel, SubscribeRequest,
    SubscribeRequestFilterTransactions,
};

const GRPC_ENDPOINT: &str = "https://grpc.nln.clr3.org:443";
const RPC_ENDPOINT: &str = "https://rpc.nln.clr3.org";
const AUTH_HEADER: &str = "x-api-key";
const PUMP_FUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

// Celowo poniżej kilku wywołań HTTP/s. Benchmark latency nie może sam wywołać
// throttlingu klucza i następnie raportować jego kolejki jako czasu odpowiedzi.
const WARM_PING_SAMPLES: usize = 7;
const COLD_CONNECT_PING_SAMPLES: usize = 2;
const WARM_RPC_SAMPLES: usize = 6;
const COLD_RPC_SAMPLES: usize = 2;
const SLOT_PARITY_SAMPLES: usize = 5;
const INTER_SAMPLE_PAUSE: Duration = Duration::from_secs(1);
const DEFAULT_STREAM_EVENT_TARGET: usize = 12;
const DEFAULT_STREAM_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone)]
struct ApiKeyInterceptor {
    header: MetadataKey<Ascii>,
    value: AsciiMetadataValue,
}

impl ApiKeyInterceptor {
    fn new(api_key: &str) -> Result<Self> {
        let header = MetadataKey::<Ascii>::from_bytes(AUTH_HEADER.as_bytes())
            .map_err(|err| anyhow!("nieprawidłowa nazwa nagłówka auth: {err}"))?;
        let value = AsciiMetadataValue::try_from(api_key)
            .context("klucz API nie może zostać zapisany jako metadata gRPC")?;
        Ok(Self { header, value })
    }
}

impl Interceptor for ApiKeyInterceptor {
    fn call(&mut self, mut request: Request<()>) -> std::result::Result<Request<()>, Status> {
        request
            .metadata_mut()
            .insert(self.header.clone(), self.value.clone());
        Ok(request)
    }
}

#[derive(Debug, Serialize)]
struct Distribution {
    samples: usize,
    min_ms: f64,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    max_ms: f64,
    mean_ms: f64,
}

#[derive(Debug, Serialize)]
struct GrpcStreamReport {
    request_to_stream_ready_ms: f64,
    request_to_first_pumpfun_transaction_ms: Option<f64>,
    collected_pumpfun_transactions: usize,
    target_pumpfun_transactions: usize,
    unique_slots: usize,
    elapsed_ms: f64,
    interarrival_ms: Option<Distribution>,
    terminated_by: &'static str,
}

#[derive(Debug, Serialize)]
struct SlotParity {
    samples: usize,
    grpc_get_slot: Distribution,
    rpc_get_slot: Distribution,
    grpc_minus_rpc_slot_min: i64,
    grpc_minus_rpc_slot_p50: i64,
    grpc_minus_rpc_slot_max: i64,
}

#[derive(Debug, Serialize)]
struct RpcReport {
    cold_connect_plus_get_slot: Distribution,
    warm_get_slot: Distribution,
    warm_get_latest_blockhash: Distribution,
}

#[derive(Debug, Serialize)]
struct BenchmarkReport {
    benchmark: &'static str,
    tool_version: &'static str,
    started_unix_ms: u128,
    endpoints: Endpoints,
    protocol: Protocol,
    scope: Scope,
    grpc: GrpcReport,
    rpc: RpcReport,
    slot_parity: SlotParity,
    pumpfun_stream: GrpcStreamReport,
}

#[derive(Debug, Serialize)]
struct Endpoints {
    grpc: &'static str,
    rpc: &'static str,
}

#[derive(Debug, Serialize)]
struct Protocol {
    grpc_auth_header: &'static str,
    grpc_commitment: &'static str,
    pumpfun_transaction_filter: &'static str,
    rpc_auth_header: &'static str,
}

#[derive(Debug, Serialize)]
struct Scope {
    measures: Vec<&'static str>,
    explicitly_does_not_measure: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct GrpcReport {
    cold_connect_plus_ping: Distribution,
    warm_ping: Distribution,
    get_version: String,
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn distribution(samples: &[f64]) -> Result<Distribution> {
    if samples.is_empty() {
        return Err(anyhow!("brak próbek do agregacji"));
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let nth = |percentile: f64| {
        let index = ((sorted.len() as f64 * percentile).ceil() as usize)
            .saturating_sub(1)
            .min(sorted.len().saturating_sub(1));
        sorted[index]
    };
    let max_ms = *sorted
        .last()
        .ok_or_else(|| anyhow!("brak maksimum dla niepustej próbki"))?;
    Ok(Distribution {
        samples: sorted.len(),
        min_ms: sorted[0],
        p50_ms: nth(0.50),
        p95_ms: nth(0.95),
        p99_ms: nth(0.99),
        max_ms,
        mean_ms: sorted.iter().sum::<f64>() / sorted.len() as f64,
    })
}

fn percentile_i64(samples: &[i64], percentile: f64) -> Result<i64> {
    if samples.is_empty() {
        return Err(anyhow!("brak próbek slotów do agregacji"));
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let index = ((sorted.len() as f64 * percentile).ceil() as usize)
        .saturating_sub(1)
        .min(sorted.len().saturating_sub(1));
    Ok(sorted[index])
}

async fn grpc_client(api_key: &str) -> Result<GeyserGrpcClient<ApiKeyInterceptor>> {
    // Ustawienia transportowe odpowiadają aktywnemu konstruktorowi Seera; nie
    // wykorzystujemy helpera `x_token`, bo NLN wymaga obecnie `x-api-key`.
    let endpoint = Endpoint::from_shared(GRPC_ENDPOINT.to_owned())?
        .http2_adaptive_window(true)
        .initial_connection_window_size(1 << 26)
        .initial_stream_window_size(1 << 25)
        .keep_alive_while_idle(true)
        .http2_keep_alive_interval(Duration::from_secs(10))
        .keep_alive_timeout(Duration::from_secs(5))
        .tcp_nodelay(true)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30));
    let channel = endpoint
        .connect()
        .await
        .context("TLS/HTTP2 connect do NLN gRPC")?;
    let interceptor = ApiKeyInterceptor::new(api_key)?;
    let health = HealthClient::with_interceptor(channel.clone(), interceptor.clone());
    let geyser = GeyserClient::with_interceptor(channel, interceptor);
    Ok(GeyserGrpcClient::new(health, geyser))
}

fn pumpfun_transaction_request() -> SubscribeRequest {
    let mut transactions = HashMap::new();
    transactions.insert(
        "pumpfun_tx".to_owned(),
        SubscribeRequestFilterTransactions {
            vote: Some(false),
            failed: Some(false),
            account_include: vec![PUMP_FUN_PROGRAM_ID.to_owned()],
            account_exclude: vec![],
            account_required: vec![],
            signature: None,
        },
    );
    SubscribeRequest {
        accounts: HashMap::new(),
        slots: HashMap::new(),
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

async fn warm_grpc_ping(client: &mut GeyserGrpcClient<ApiKeyInterceptor>) -> Result<Distribution> {
    let mut timings = Vec::with_capacity(WARM_PING_SAMPLES);
    for sequence in 0..WARM_PING_SAMPLES {
        let started = Instant::now();
        let pong = client
            .ping(sequence as i32 + 1)
            .await
            .with_context(|| format!("gRPC Ping nr {}", sequence + 1))?;
        if pong.count != sequence as i32 + 1 {
            return Err(anyhow!(
                "gRPC Ping zwrócił inny count: otrzymano {}, oczekiwano {}",
                pong.count,
                sequence + 1
            ));
        }
        timings.push(started.elapsed().as_secs_f64() * 1_000.0);
        sleep(INTER_SAMPLE_PAUSE).await;
    }
    distribution(&timings)
}

async fn cold_grpc_connect_ping(api_key: &str) -> Result<Distribution> {
    let mut timings = Vec::with_capacity(COLD_CONNECT_PING_SAMPLES);
    for sequence in 0..COLD_CONNECT_PING_SAMPLES {
        let started = Instant::now();
        let mut client = grpc_client(api_key).await?;
        let pong = client
            .ping(10_000 + sequence as i32)
            .await
            .with_context(|| format!("cold gRPC Ping nr {}", sequence + 1))?;
        if pong.count != 10_000 + sequence as i32 {
            return Err(anyhow!("cold gRPC Ping zwrócił niepoprawny count"));
        }
        timings.push(started.elapsed().as_secs_f64() * 1_000.0);
        sleep(INTER_SAMPLE_PAUSE).await;
    }
    distribution(&timings)
}

async fn pumpfun_stream_sample(
    api_key: &str,
    target: usize,
    timeout: Duration,
) -> Result<GrpcStreamReport> {
    let mut client = grpc_client(api_key).await?;
    let started = Instant::now();
    let mut stream = client
        .subscribe_once(pumpfun_transaction_request())
        .await
        .context("otwarcie subskrypcji Pump.fun")?;
    let stream_ready_ms = started.elapsed().as_secs_f64() * 1_000.0;

    let mut first_at: Option<Instant> = None;
    let mut previous_at: Option<Instant> = None;
    let mut interarrival = Vec::new();
    let mut slots = BTreeSet::new();
    let mut received = 0usize;
    let terminated_by;

    loop {
        if received >= target {
            terminated_by = "target_reached";
            break;
        }
        let elapsed = started.elapsed();
        if elapsed >= timeout {
            terminated_by = "stream_timeout";
            break;
        }
        let remaining = timeout.saturating_sub(elapsed);
        let next = tokio::time::timeout(remaining, stream.next()).await;
        let Some(message) = next
            .map_err(|_| anyhow!("timeout odbioru gRPC Pump.fun"))?
            .ok_or_else(|| anyhow!("strumień Pump.fun zakończył się bez komunikatu końcowego"))?
            .context("błąd komunikatu w strumieniu Pump.fun")?
            .update_oneof
        else {
            continue;
        };
        if let UpdateOneof::Transaction(transaction) = message {
            let now = Instant::now();
            first_at.get_or_insert(now);
            if let Some(previous) = previous_at.replace(now) {
                interarrival.push(now.duration_since(previous).as_secs_f64() * 1_000.0);
            }
            slots.insert(transaction.slot);
            received += 1;
        }
    }

    Ok(GrpcStreamReport {
        request_to_stream_ready_ms: stream_ready_ms,
        request_to_first_pumpfun_transaction_ms: first_at
            .map(|instant| instant.duration_since(started).as_secs_f64() * 1_000.0),
        collected_pumpfun_transactions: received,
        target_pumpfun_transactions: target,
        unique_slots: slots.len(),
        elapsed_ms: started.elapsed().as_secs_f64() * 1_000.0,
        interarrival_ms: (!interarrival.is_empty())
            .then(|| distribution(&interarrival))
            .transpose()?,
        terminated_by,
    })
}

fn rpc_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .tcp_nodelay(true)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(15))
        .pool_max_idle_per_host(1)
        .build()
        .context("budowa klienta HTTP RPC")
}

async fn rpc_call(
    client: &reqwest::Client,
    api_key: &str,
    method: &str,
    params: Value,
) -> Result<Value> {
    let response = client
        .post(RPC_ENDPOINT)
        .header(AUTH_HEADER, api_key)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        }))
        .send()
        .await
        .with_context(|| format!("HTTP RPC {method}"))?
        .error_for_status()
        .with_context(|| format!("status HTTP RPC {method}"))?;
    let payload: Value = response
        .json()
        .await
        .with_context(|| format!("JSON odpowiedzi RPC {method}"))?;
    if let Some(error) = payload.get("error") {
        return Err(anyhow!("RPC {method} zwrócił error: {error}"));
    }
    payload
        .get("result")
        .cloned()
        .ok_or_else(|| anyhow!("RPC {method} nie zwrócił pola result"))
}

async fn rpc_get_slot(client: &reqwest::Client, api_key: &str) -> Result<u64> {
    rpc_call(
        client,
        api_key,
        "getSlot",
        json!([{ "commitment": "processed" }]),
    )
    .await?
    .as_u64()
    .ok_or_else(|| anyhow!("RPC getSlot result nie jest u64"))
}

async fn warm_rpc_get_slot(client: &reqwest::Client, api_key: &str) -> Result<Distribution> {
    let mut timings = Vec::with_capacity(WARM_RPC_SAMPLES);
    for _ in 0..WARM_RPC_SAMPLES {
        let started = Instant::now();
        let _slot = rpc_get_slot(client, api_key).await?;
        timings.push(started.elapsed().as_secs_f64() * 1_000.0);
        sleep(INTER_SAMPLE_PAUSE).await;
    }
    distribution(&timings)
}

async fn warm_rpc_latest_blockhash(
    client: &reqwest::Client,
    api_key: &str,
) -> Result<Distribution> {
    let mut timings = Vec::with_capacity(WARM_RPC_SAMPLES);
    for _ in 0..WARM_RPC_SAMPLES {
        let started = Instant::now();
        let value = rpc_call(
            client,
            api_key,
            "getLatestBlockhash",
            json!([{ "commitment": "processed" }]),
        )
        .await?;
        if value
            .get("value")
            .and_then(|value| value.get("blockhash"))
            .and_then(Value::as_str)
            .is_none()
        {
            return Err(anyhow!("RPC getLatestBlockhash bez result.value.blockhash"));
        }
        timings.push(started.elapsed().as_secs_f64() * 1_000.0);
        sleep(INTER_SAMPLE_PAUSE).await;
    }
    distribution(&timings)
}

async fn cold_rpc_connect_get_slot(api_key: &str) -> Result<Distribution> {
    let mut timings = Vec::with_capacity(COLD_RPC_SAMPLES);
    for _ in 0..COLD_RPC_SAMPLES {
        let started = Instant::now();
        let client = rpc_client()?;
        let _slot = rpc_get_slot(&client, api_key).await?;
        timings.push(started.elapsed().as_secs_f64() * 1_000.0);
        sleep(INTER_SAMPLE_PAUSE).await;
    }
    distribution(&timings)
}

async fn slot_parity(
    grpc: &mut GeyserGrpcClient<ApiKeyInterceptor>,
    rpc: &reqwest::Client,
    api_key: &str,
) -> Result<SlotParity> {
    let mut grpc_timings = Vec::with_capacity(SLOT_PARITY_SAMPLES);
    let mut rpc_timings = Vec::with_capacity(SLOT_PARITY_SAMPLES);
    let mut deltas = Vec::with_capacity(SLOT_PARITY_SAMPLES);
    for _ in 0..SLOT_PARITY_SAMPLES {
        let grpc_timed = async {
            let started = Instant::now();
            let result = grpc
                .get_slot(Some(CommitmentLevel::Processed))
                .await
                .context("gRPC GetSlot")?;
            Result::<(u64, f64)>::Ok((result.slot, started.elapsed().as_secs_f64() * 1_000.0))
        };
        let rpc_timed = async {
            let started = Instant::now();
            let slot = rpc_get_slot(rpc, api_key).await?;
            Result::<(u64, f64)>::Ok((slot, started.elapsed().as_secs_f64() * 1_000.0))
        };
        let (grpc_result, rpc_result) = tokio::join!(grpc_timed, rpc_timed);
        let (grpc_slot, grpc_ms) = grpc_result?;
        let (rpc_slot, rpc_ms) = rpc_result?;
        grpc_timings.push(grpc_ms);
        rpc_timings.push(rpc_ms);
        deltas.push(grpc_slot as i64 - rpc_slot as i64);
        sleep(INTER_SAMPLE_PAUSE).await;
    }
    let grpc_minus_rpc_slot_min = *deltas
        .iter()
        .min()
        .ok_or_else(|| anyhow!("brak minimum różnicy slotów"))?;
    let grpc_minus_rpc_slot_max = *deltas
        .iter()
        .max()
        .ok_or_else(|| anyhow!("brak maksimum różnicy slotów"))?;
    Ok(SlotParity {
        samples: deltas.len(),
        grpc_get_slot: distribution(&grpc_timings)?,
        rpc_get_slot: distribution(&rpc_timings)?,
        grpc_minus_rpc_slot_min,
        grpc_minus_rpc_slot_p50: percentile_i64(&deltas, 0.50)?,
        grpc_minus_rpc_slot_max,
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let api_key = env::var("NLN_BENCHMARK_API_KEY").context(
        "ustaw NLN_BENCHMARK_API_KEY poza repo; benchmark nie przyjmuje klucza jako argumentu",
    )?;
    if api_key.trim().is_empty() {
        return Err(anyhow!("NLN_BENCHMARK_API_KEY jest puste"));
    }

    let mut args = env::args().skip(1);
    let mut stream_target = DEFAULT_STREAM_EVENT_TARGET;
    let mut stream_timeout = DEFAULT_STREAM_TIMEOUT;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--stream-events" => {
                stream_target = args
                    .next()
                    .context("--stream-events wymaga liczby")?
                    .parse()
                    .context("--stream-events musi być dodatnią liczbą")?;
            }
            "--stream-timeout-secs" => {
                let secs: u64 = args
                    .next()
                    .context("--stream-timeout-secs wymaga liczby")?
                    .parse()
                    .context("--stream-timeout-secs musi być liczbą")?;
                stream_timeout = Duration::from_secs(secs);
            }
            "--help" | "-h" => {
                println!(
                    "użycie: NLN_BENCHMARK_API_KEY=... cargo run -p seer --example nln_latency_benchmark -- [--stream-events N] [--stream-timeout-secs N]"
                );
                return Ok(());
            }
            other => return Err(anyhow!("nieznany argument: {other}")),
        }
    }
    if stream_target == 0 || stream_timeout.is_zero() {
        return Err(anyhow!("parametry streamu muszą być dodatnie"));
    }

    let started_unix_ms = now_unix_ms();
    let cold_grpc = cold_grpc_connect_ping(&api_key).await?;
    let mut grpc = grpc_client(&api_key).await?;
    let version = grpc.get_version().await.context("gRPC GetVersion")?.version;
    let warm_grpc = warm_grpc_ping(&mut grpc).await?;

    let rpc = rpc_client()?;
    let cold_rpc = cold_rpc_connect_get_slot(&api_key).await?;
    let warm_rpc_slot = warm_rpc_get_slot(&rpc, &api_key).await?;
    let warm_rpc_blockhash = warm_rpc_latest_blockhash(&rpc, &api_key).await?;
    let parity = slot_parity(&mut grpc, &rpc, &api_key).await?;
    let stream = pumpfun_stream_sample(&api_key, stream_target, stream_timeout).await?;

    let report = BenchmarkReport {
        benchmark: "NLN gRPC/RPC short-latency benchmark",
        tool_version: "1.0.0",
        started_unix_ms,
        endpoints: Endpoints {
            grpc: GRPC_ENDPOINT,
            rpc: RPC_ENDPOINT,
        },
        protocol: Protocol {
            grpc_auth_header: AUTH_HEADER,
            grpc_commitment: "processed",
            pumpfun_transaction_filter: PUMP_FUN_PROGRAM_ID,
            rpc_auth_header: AUTH_HEADER,
        },
        scope: Scope {
            measures: vec![
                "gRPC TLS/HTTP2 connect plus authenticated Ping RTT",
                "gRPC warm authenticated Ping RTT",
                "gRPC Pump.fun processed transaction stream setup and receipt continuity",
                "HTTP JSON-RPC connect plus getSlot RTT",
                "HTTP JSON-RPC warm getSlot and getLatestBlockhash RTT",
                "same-commitment GetSlot latency and gRPC-minus-RPC slot delta",
            ],
            explicitly_does_not_measure: vec![
                "absolute on-chain-to-client one-way latency (proto v1.14 has no provider transmit timestamp)",
                "parser, bounded queue, Event Bus, Gatekeeper, or bot end-to-end latency",
                "Jito Block Engine receipt, bundle forwarding, inclusion, or landing latency",
                "transaction submission; this benchmark does not send transactions or bundles",
            ],
        },
        grpc: GrpcReport {
            cold_connect_plus_ping: cold_grpc,
            warm_ping: warm_grpc,
            get_version: version,
        },
        rpc: RpcReport {
            cold_connect_plus_get_slot: cold_rpc,
            warm_get_slot: warm_rpc_slot,
            warm_get_latest_blockhash: warm_rpc_blockhash,
        },
        slot_parity: parity,
        pumpfun_stream: stream,
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
