//! Izolowany, kontrolowany benchmark Helius Sender vs Direct Jito vs NLN.
//!
//! Nie importuje ani nie uruchamia Ghost runtime. Używa jednak produkcyjnej
//! implementacji `LiveTxSender` dokładnie do submitu i confirmation Helius.
//! Bez `--execute` wykonuje Stage 0 (probe, saldo, konstrukcja i symulacja),
//! bez żadnego on-chain submitu. Sekrety przyjmuje wyłącznie przez environment.

use std::{
    collections::HashMap,
    env,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use futures::StreamExt;
use ghost_launcher::components::live_tx_sender::{
    resolve_live_sender_endpoint, sender_ping_endpoint, LiveTxSender, LiveTxSenderConfig,
    SenderTransactionSubmission,
};
use parking_lot::Mutex;
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
    transaction::{Transaction, VersionedTransaction},
};
use tokio::{sync::Barrier, task::JoinHandle, time::sleep};
use trigger::{config::BundleConfig, jito_client::JitoClient};
use yellowstone_grpc_client::GeyserGrpcClient;
use yellowstone_grpc_proto::prelude::{
    subscribe_update::UpdateOneof, CommitmentLevel, SubscribeRequest, SubscribeRequestFilterSlots,
    SubscribeRequestFilterTransactions,
};

const NLN_RPC_ENDPOINT: &str = "https://rpc.nln.clr3.org";
const DIRECT_JITO_ENDPOINT: &str = "https://frankfurt.mainnet.block-engine.jito.wtf";
const DIRECT_JITO_HTTP: &str = "https://frankfurt.mainnet.block-engine.jito.wtf/api/v1/bundles";
const MEMO_PROGRAM_ID: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";
const COMPUTE_UNIT_LIMIT: u32 = 50_000;
const COMPUTE_UNIT_PRICE_MICRO_LAMPORTS: u64 = 25_000;
const TIP_LAMPORTS: u64 = 1_000_000;
const BASE_FEE_MAX_LAMPORTS: u64 = 5_000;
const MAX_TRIPLETS: usize = 3;
const RECONCILIATION_TIMEOUT: Duration = Duration::from_secs(30);
const STATUS_POLL_INTERVAL: Duration = Duration::from_millis(250);
const BUNDLE_STATUS_INTERVAL: Duration = Duration::from_millis(1_050);

const API_KEY_ENV: &[&str] = &["NLN_BENCHMARK_API_KEY", "NLN_API_KEY", "GHOST_NLN_API_KEY"];
const PAYER_ENV: &str = "GHOST_TRIGGER_KEYPAIR_PATH";
const GRPC_ENDPOINT_ENV: &str = "GHOST_SEER_GRPC_ENDPOINT";
const GRPC_TOKEN_ENV: &str = "GHOST_SEER_GRPC_X_TOKEN";
const PRIORITY_RPC_ENV: &str = "GHOST_TRIGGER_RPC_URL";

#[derive(Debug)]
struct Args {
    execute: bool,
    max_triplets: usize,
}

impl Args {
    fn parse() -> Result<Self> {
        let mut execute = false;
        let mut max_triplets = None;
        let mut values = env::args().skip(1);
        while let Some(argument) = values.next() {
            match argument.as_str() {
                "--execute" => execute = true,
                "--max-triplets" => {
                    let raw = values
                        .next()
                        .context("--max-triplets wymaga liczby 1..=3")?;
                    let parsed: usize = raw
                        .parse()
                        .with_context(|| format!("nieprawidłowe --max-triplets: {raw}"))?;
                    if !(1..=MAX_TRIPLETS).contains(&parsed) {
                        bail!("--max-triplets musi należeć do 1..={MAX_TRIPLETS}");
                    }
                    max_triplets = Some(parsed);
                }
                "--help" | "-h" => {
                    println!(
                        "Usage: helius_sender_direct_jito_nln_benchmark [--execute --max-triplets 1..=3]\n\
                         env: NLN_BENCHMARK_API_KEY lub NLN_API_KEY, {PAYER_ENV}, \
                         {GRPC_ENDPOINT_ENV}, {GRPC_TOKEN_ENV}, {PRIORITY_RPC_ENV}.\n\
                         Bez --execute: Stage 0 bez on-chain submitu."
                    );
                    std::process::exit(0);
                }
                _ => bail!("nieznany argument: {argument}"),
            }
        }
        if execute && max_triplets.is_none() {
            bail!("live submit wymaga jawnego --max-triplets N");
        }
        Ok(Self {
            execute,
            max_triplets: max_triplets.unwrap_or(MAX_TRIPLETS),
        })
    }
}

#[derive(Debug, Clone)]
struct Environment {
    nln_api_key: String,
    payer_path: String,
    grpc_endpoint: String,
    grpc_token: String,
    priority_rpc_url: String,
}

impl Environment {
    fn read() -> Result<Self> {
        let nln_api_key = API_KEY_ENV
            .iter()
            .find_map(|name| env::var(name).ok().filter(|value| !value.trim().is_empty()))
            .with_context(|| format!("brak jednego z environment: {}", API_KEY_ENV.join(", ")))?;
        Ok(Self {
            nln_api_key,
            payer_path: required_env(PAYER_ENV)?,
            grpc_endpoint: required_env(GRPC_ENDPOINT_ENV)?,
            grpc_token: required_env(GRPC_TOKEN_ENV)?,
            priority_rpc_url: required_env(PRIORITY_RPC_ENV)?,
        })
    }
}

fn required_env(name: &str) -> Result<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .with_context(|| format!("brak {name}"))
}

#[derive(Debug, Serialize)]
struct StageZeroReport {
    stage: &'static str,
    on_chain_submit_performed: bool,
    payer_fingerprint: String,
    balance_lamports: u64,
    helius_endpoint: String,
    helius_ping_http_status: u16,
    nln_rpc_endpoint: &'static str,
    primary_observer_endpoint: String,
    direct_jito_tip_accounts: usize,
    nln_tip_accounts: usize,
    selected_helius_tip_account: String,
    selected_direct_jito_tip_account: String,
    selected_nln_tip_account: String,
    all_lane_tip_accounts_distinct: bool,
    empty_nln_bundle_rejected: bool,
    simulation_units_consumed: [Option<u64>; 3],
    compute_unit_limit: u32,
    compute_unit_price_micro_lamports: u64,
    tip_lamports: u64,
    maximum_cost_per_transaction_lamports: u64,
    hard_cap_lamports: u64,
    requested_triplets: usize,
    observer_ready: bool,
}

#[derive(Debug, Serialize)]
struct BenchmarkReport {
    benchmark: &'static str,
    tool_version: &'static str,
    stage_zero: StageZeroReport,
    execution_requested: bool,
    stopped_after_stage: String,
    balance_before_lamports: u64,
    balance_after_lamports: Option<u64>,
    expected_total_cost_lamports: Option<u64>,
    actual_balance_delta_lamports: Option<u64>,
    triplets: Vec<TripletReport>,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct TripletReport {
    triplet_id: usize,
    shared_blockhash_context_slot: u64,
    shared_last_valid_block_height: u64,
    submit_start_gap_ms: Option<f64>,
    pairing_quality: String,
    stage_one_balance_before_lamports: Option<u64>,
    stage_one_balance_after_lamports: Option<u64>,
    stage_one_balance_delta_matches_cost: Option<bool>,
    lanes: Vec<LaneRecord>,
}

#[derive(Debug, Serialize)]
struct LaneRecord {
    triplet_id: usize,
    lane: &'static str,
    payer_fingerprint: String,
    tip_account: String,
    tip_lamports: u64,
    cu_limit: u32,
    cu_price_micro_lamports: u64,
    local_signature: String,
    provider_returned_signature: Option<String>,
    bundle_id_or_none: Option<String>,
    submit_start_ns_relative: Option<u128>,
    submit_start_gap_ms: Option<f64>,
    ack_ms: Option<f64>,
    submit_to_processed_ms: Option<f64>,
    ack_to_processed_ms: Option<f64>,
    production_confirm_start_after_ack_ms: Option<f64>,
    production_confirm_fresh_connection_ms: Option<f64>,
    production_confirm_ms_or_none: Option<f64>,
    production_confirm_outcome: Option<String>,
    production_confirm_slot: Option<u64>,
    submit_observed_slot: Option<u64>,
    processed_slot: Option<u64>,
    landed_slot: Option<u64>,
    delta_slot: Option<i64>,
    confirmed: bool,
    finalized: bool,
    on_chain_err: Option<String>,
    provider_status: Option<String>,
    fee_lamports: Option<u64>,
    total_cost_lamports: Option<u64>,
    outcome: String,
    observer_error: Option<String>,
    submit_error: Option<String>,
    simulation_units_consumed: Option<u64>,
    #[serde(skip)]
    submit_at: Option<Instant>,
    #[serde(skip)]
    ack_at: Option<Instant>,
}

#[derive(Debug)]
struct PreparedTx {
    transaction: VersionedTransaction,
    bytes: Vec<u8>,
    signature: Signature,
    tip_account: Pubkey,
}

#[derive(Debug)]
struct LatestBlockhash {
    hash: Hash,
    context_slot: u64,
    last_valid_block_height: u64,
}

#[derive(Debug)]
struct ProductionConfirmation {
    start_after_ack_ms: f64,
    total_ms: f64,
    outcome: String,
    landed_slot: Option<u64>,
}

#[derive(Clone)]
struct NlnRpc {
    http: Client,
    api_key: String,
}

#[derive(Default)]
struct ObserverState {
    latest_slot: Option<u64>,
    tracked: HashMap<String, Option<(Instant, u64)>>,
    error: Option<String>,
}

#[derive(Clone)]
struct YellowstoneObserver {
    state: Arc<Mutex<ObserverState>>,
}

impl NlnRpc {
    fn new(api_key: String) -> Result<Self> {
        Ok(Self {
            http: Client::builder()
                .connect_timeout(Duration::from_secs(8))
                .timeout(Duration::from_secs(15))
                .tcp_nodelay(true)
                .build()
                .context("budowa persistentnego klienta HTTP NLN")?,
            api_key,
        })
    }

    async fn raw_call(&self, method: &str, params: Value) -> Result<Value> {
        let response = self
            .http
            .post(NLN_RPC_ENDPOINT)
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-api-key", &self.api_key)
            .json(
                &json!({"jsonrpc":"2.0", "id":"ghost-benchmark", "method":method, "params":params}),
            )
            .send()
            .await
            .with_context(|| format!("NLN transport {method}"))?;
        let status = response.status();
        let text = response.text().await.context("odczyt odpowiedzi NLN")?;
        let value: Value = serde_json::from_str(&text).context("NLN odpowiedział nie-JSON")?;
        if status != StatusCode::OK && value.get("error").is_none() {
            bail!(
                "NLN HTTP {} dla {method}: {}",
                status.as_u16(),
                compact_json(&value)
            );
        }
        Ok(value)
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let value = self.raw_call(method, params).await?;
        if value.get("error").is_some() {
            bail!("NLN {method}: {}", compact_json(&value));
        }
        Ok(value)
    }
}

fn compact_json(value: &Value) -> String {
    value.to_string().chars().take(360).collect()
}

fn compact_error(error: impl std::fmt::Display) -> String {
    error.to_string().chars().take(360).collect()
}

fn normalize_grpc_endpoint(raw: &str) -> String {
    let raw = raw.trim();
    if raw.starts_with("https://") || raw.starts_with("http://") {
        raw.to_owned()
    } else {
        format!("https://{raw}")
    }
}

fn fingerprint(pubkey: &Pubkey) -> String {
    let value = pubkey.to_string();
    format!("{}…{}", &value[..4], &value[value.len() - 4..])
}

fn priority_fee_upper_bound_lamports() -> u64 {
    (u64::from(COMPUTE_UNIT_LIMIT) * COMPUTE_UNIT_PRICE_MICRO_LAMPORTS + 999_999) / 1_000_000
}

fn maximum_cost_per_transaction() -> u64 {
    BASE_FEE_MAX_LAMPORTS + priority_fee_upper_bound_lamports() + TIP_LAMPORTS
}

fn elapsed_ms(later: Instant, earlier: Instant) -> f64 {
    if later >= earlier {
        later.duration_since(earlier).as_secs_f64() * 1_000.0
    } else {
        -(earlier.duration_since(later).as_secs_f64() * 1_000.0)
    }
}

fn relative_ns(point: Instant, origin: Instant) -> u128 {
    point.duration_since(origin).as_nanos()
}

async fn helius_ping(endpoint: &str) -> Result<u16> {
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(4))
        .build()
        .context("budowa niezależnego klienta Helius ping")?;
    let response = client
        .get(sender_ping_endpoint(endpoint))
        .send()
        .await
        .context("Helius /ping")?;
    Ok(response.status().as_u16())
}

async fn jito_tip_accounts(http: &Client) -> Result<Vec<Pubkey>> {
    let response = http
        .post(DIRECT_JITO_HTTP)
        .header(header::CONTENT_TYPE, "application/json")
        .json(&json!({"jsonrpc":"2.0", "id":"tip-probe", "method":"getTipAccounts", "params":[]}))
        .send()
        .await
        .context("Direct Jito getTipAccounts")?;
    let status = response.status();
    let body = response
        .text()
        .await
        .context("odczyt Direct Jito getTipAccounts")?;
    let value: Value = serde_json::from_str(&body).context("Direct Jito tip accounts nie-JSON")?;
    if status != StatusCode::OK || value.get("error").is_some() {
        bail!("Direct Jito getTipAccounts: {}", compact_json(&value));
    }
    value
        .get("result")
        .and_then(Value::as_array)
        .context("Direct Jito getTipAccounts bez result tablicy")?
        .iter()
        .filter_map(Value::as_str)
        .map(Pubkey::from_str)
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("nieprawidłowe konto tip Direct Jito")
}

async fn nln_tip_accounts(rpc: &NlnRpc) -> Result<Vec<Pubkey>> {
    rpc.call("getTipAccounts", json!([]))
        .await?
        .get("result")
        .and_then(Value::as_array)
        .context("NLN getTipAccounts bez result tablicy")?
        .iter()
        .filter_map(Value::as_str)
        .map(Pubkey::from_str)
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("nieprawidłowe konto tip NLN")
}

async fn latest_blockhash(rpc: &NlnRpc) -> Result<LatestBlockhash> {
    let value = rpc
        .call("getLatestBlockhash", json!([{"commitment":"processed"}]))
        .await?;
    let result = value
        .get("result")
        .context("getLatestBlockhash bez result")?;
    Ok(LatestBlockhash {
        hash: Hash::from_str(
            result
                .pointer("/value/blockhash")
                .and_then(Value::as_str)
                .context("getLatestBlockhash bez blockhash")?,
        )
        .context("nieprawidłowy recent blockhash")?,
        context_slot: result
            .pointer("/context/slot")
            .and_then(Value::as_u64)
            .context("getLatestBlockhash bez context.slot")?,
        last_valid_block_height: result
            .pointer("/value/lastValidBlockHeight")
            .and_then(Value::as_u64)
            .context("getLatestBlockhash bez lastValidBlockHeight")?,
    })
}

async fn balance(rpc: &NlnRpc, payer: &Pubkey) -> Result<u64> {
    rpc.call(
        "getBalance",
        json!([payer.to_string(), {"commitment":"confirmed"}]),
    )
    .await?
    .pointer("/result/value")
    .and_then(Value::as_u64)
    .context("getBalance bez result.value")
}

fn build_transaction(
    payer: &Keypair,
    lane_tag: &str,
    triplet_id: usize,
    tip_account: Pubkey,
    blockhash: Hash,
) -> Result<PreparedTx> {
    if lane_tag.len() != 3 {
        bail!("lane_tag musi mieć dokładnie 3 znaki dla parzystości rozmiaru Memo");
    }
    let memo = format!("GHOST_BENCH_20260723:{lane_tag}:T{triplet_id:02}");
    let memo_program = Pubkey::from_str(MEMO_PROGRAM_ID).context("Memo program id")?;
    let instructions = vec![
        ComputeBudgetInstruction::set_compute_unit_limit(COMPUTE_UNIT_LIMIT),
        ComputeBudgetInstruction::set_compute_unit_price(COMPUTE_UNIT_PRICE_MICRO_LAMPORTS),
        Instruction {
            program_id: memo_program,
            accounts: vec![AccountMeta::new_readonly(payer.pubkey(), true)],
            data: memo.into_bytes(),
        },
        system_instruction::transfer(&payer.pubkey(), &tip_account, TIP_LAMPORTS),
    ];
    let legacy = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &[payer],
        blockhash,
    );
    let signature = *legacy
        .signatures
        .first()
        .context("brak lokalnej signature")?;
    let transaction = VersionedTransaction::from(legacy);
    let bytes =
        bincode::serialize(&transaction).context("serializacja signed VersionedTransaction")?;
    Ok(PreparedTx {
        transaction,
        bytes,
        signature,
        tip_account,
    })
}

async fn simulate(rpc: &NlnRpc, prepared: &PreparedTx, lane: &str) -> Result<Option<u64>> {
    let value = rpc
        .call(
            "simulateTransaction",
            json!([
                BASE64.encode(&prepared.bytes),
                {"encoding":"base64", "sigVerify":true, "replaceRecentBlockhash":false, "commitment":"processed"}
            ]),
        )
        .await
        .with_context(|| format!("symulacja {lane}"))?;
    let result = value
        .pointer("/result/value")
        .context("simulateTransaction bez result.value")?;
    if let Some(error) = result.get("err").filter(|error| !error.is_null()) {
        bail!(
            "symulacja {lane} nie przeszła: {error}; logs={}",
            result.get("logs").unwrap_or(&Value::Null)
        );
    }
    Ok(result.get("unitsConsumed").and_then(Value::as_u64))
}

async fn simulate_triplet(
    rpc: &NlnRpc,
    helius: &PreparedTx,
    direct: &PreparedTx,
    nln: &PreparedTx,
    suffix: &str,
) -> Result<[Option<u64>; 3]> {
    // NLN egzekwuje osobny, niski limit dla simulateTransaction. Odstęp jest
    // wyłącznie częścią Stage 0/pre-submit i kończy się przed submit_started;
    // nie jest retry ani „ociepleniem” żadnego lane wysyłki.
    let helius_units = simulate(rpc, helius, &format!("HELIUS_SENDER_{suffix}")).await?;
    sleep(Duration::from_millis(650)).await;
    let direct_units = simulate(rpc, direct, &format!("DIRECT_JITO_GRPC_{suffix}")).await?;
    sleep(Duration::from_millis(650)).await;
    let nln_units = simulate(rpc, nln, &format!("NLN_SENDBUNDLE_{suffix}")).await?;
    Ok([helius_units, direct_units, nln_units])
}

fn observer_request(payer: &Pubkey) -> SubscribeRequest {
    let mut transactions = HashMap::new();
    transactions.insert(
        "benchmark_payer".to_owned(),
        SubscribeRequestFilterTransactions {
            vote: Some(false),
            failed: None,
            signature: None,
            account_include: vec![payer.to_string()],
            account_exclude: vec![],
            account_required: vec![],
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

impl YellowstoneObserver {
    async fn start(endpoint: &str, token: &str, payer: &Pubkey) -> Result<Self> {
        let mut client = GeyserGrpcClient::build_from_shared(normalize_grpc_endpoint(endpoint))
            .context("budowa Yellowstone observer endpoint")?
            .x_token(Some(token.to_owned()))
            .context("ustawienie Yellowstone x-token observera")?
            .http2_adaptive_window(true)
            .keep_alive_while_idle(true)
            .http2_keep_alive_interval(Duration::from_secs(10))
            .keep_alive_timeout(Duration::from_secs(5))
            .tcp_nodelay(true)
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .connect()
            .await
            .context("połączenie persistent Yellowstone observer")?;
        let mut stream = client
            .subscribe_once(observer_request(payer))
            .await
            .context("subskrypcja processed Yellowstone observer")?;
        let state = Arc::new(Mutex::new(ObserverState::default()));
        let task_state = Arc::clone(&state);
        tokio::spawn(async move {
            while let Some(message) = stream.next().await {
                match message {
                    Ok(update) => match update.update_oneof {
                        Some(UpdateOneof::Slot(slot)) => {
                            task_state.lock().latest_slot = Some(slot.slot)
                        }
                        Some(UpdateOneof::Transaction(transaction)) => {
                            let Some(info) = transaction.transaction else {
                                continue;
                            };
                            let signature = bs58::encode(info.signature).into_string();
                            if let Some(observation) = task_state.lock().tracked.get_mut(&signature)
                            {
                                if observation.is_none() {
                                    *observation = Some((Instant::now(), transaction.slot));
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

    fn latest_slot(&self) -> Option<u64> {
        self.state.lock().latest_slot
    }

    fn error(&self) -> Option<String> {
        self.state.lock().error.clone()
    }

    fn apply(&self, lane: &mut LaneRecord) {
        let state = self.state.lock();
        lane.observer_error = state.error.clone();
        let Some(Some((observed_at, slot))) = state.tracked.get(&lane.local_signature) else {
            return;
        };
        let Some(submit_at) = lane.submit_at else {
            return;
        };
        if lane.submit_to_processed_ms.is_none() {
            lane.submit_to_processed_ms = Some(elapsed_ms(*observed_at, submit_at));
            lane.processed_slot = Some(*slot);
        }
        if let Some(ack_at) = lane.ack_at {
            lane.ack_to_processed_ms = Some(elapsed_ms(*observed_at, ack_at));
        }
    }
}

async fn wait_for_observer_ready(observer: &YellowstoneObserver) -> Result<()> {
    let started = Instant::now();
    while observer.latest_slot().is_none() {
        if let Some(error) = observer.error() {
            bail!("observer Yellowstone zakończył się przed submitami: {error}");
        }
        if started.elapsed() > Duration::from_secs(6) {
            bail!("observer Yellowstone nie dostarczył processed slotu w 6 s");
        }
        sleep(Duration::from_millis(20)).await;
    }
    Ok(())
}

fn new_lane(
    triplet_id: usize,
    lane: &'static str,
    signature: Signature,
    tip_account: Pubkey,
    payer: &Pubkey,
) -> LaneRecord {
    LaneRecord {
        triplet_id,
        lane,
        payer_fingerprint: fingerprint(payer),
        tip_account: tip_account.to_string(),
        tip_lamports: TIP_LAMPORTS,
        cu_limit: COMPUTE_UNIT_LIMIT,
        cu_price_micro_lamports: COMPUTE_UNIT_PRICE_MICRO_LAMPORTS,
        local_signature: signature.to_string(),
        provider_returned_signature: None,
        bundle_id_or_none: None,
        submit_start_ns_relative: None,
        submit_start_gap_ms: None,
        ack_ms: None,
        submit_to_processed_ms: None,
        ack_to_processed_ms: None,
        production_confirm_start_after_ack_ms: None,
        production_confirm_fresh_connection_ms: None,
        production_confirm_ms_or_none: None,
        production_confirm_outcome: None,
        production_confirm_slot: None,
        submit_observed_slot: None,
        processed_slot: None,
        landed_slot: None,
        delta_slot: None,
        confirmed: false,
        finalized: false,
        on_chain_err: None,
        provider_status: None,
        fee_lamports: None,
        total_cost_lamports: None,
        outcome: "PREPARED".to_owned(),
        observer_error: None,
        submit_error: None,
        simulation_units_consumed: None,
        submit_at: None,
        ack_at: None,
    }
}

async fn submit_helius(
    mut lane: LaneRecord,
    tx: VersionedTransaction,
    sender: LiveTxSender,
    observer: YellowstoneObserver,
    barrier: Arc<Barrier>,
    run_origin: Instant,
) -> (LaneRecord, Option<JoinHandle<ProductionConfirmation>>) {
    barrier.wait().await;
    let started = Instant::now();
    lane.submit_at = Some(started);
    lane.submit_start_ns_relative = Some(relative_ns(started, run_origin));
    lane.submit_observed_slot = observer.latest_slot();
    let local_signature = lane.local_signature.clone();
    match sender.send_transaction(&tx).await {
        Ok(submission) if submission.signature.to_string() == local_signature => {
            let acked = Instant::now();
            lane.ack_at = Some(acked);
            lane.ack_ms = Some(elapsed_ms(acked, started));
            lane.provider_returned_signature = Some(submission.signature.to_string());
            lane.provider_status = Some("ACK_SIGNATURE_MATCH".to_owned());
            lane.outcome = "ACK_ONLY".to_owned();
            let sender_for_confirmation = sender.clone();
            let confirmation = tokio::spawn(async move {
                let confirmation_started = Instant::now();
                let start_after_ack_ms = elapsed_ms(confirmation_started, acked);
                let result = sender_for_confirmation
                    .confirm_submission(&SenderTransactionSubmission {
                        signature: submission.signature,
                    })
                    .await;
                let total_ms = confirmation_started.elapsed().as_secs_f64() * 1_000.0;
                match result {
                    Ok(confirmed) => ProductionConfirmation {
                        start_after_ack_ms,
                        total_ms,
                        outcome: "CONFIRMED".to_owned(),
                        landed_slot: confirmed.landed_slot,
                    },
                    Err(error) => ProductionConfirmation {
                        start_after_ack_ms,
                        total_ms,
                        outcome: format!("ERROR:{}", compact_error(error)),
                        landed_slot: None,
                    },
                }
            });
            (lane, Some(confirmation))
        }
        Ok(submission) => {
            lane.provider_returned_signature = Some(submission.signature.to_string());
            lane.outcome = "FAILED_SIGNATURE_MISMATCH".to_owned();
            lane.submit_error =
                Some("Helius ACK signature różni się od lokalnej signature".to_owned());
            (lane, None)
        }
        Err(error) => {
            lane.outcome = "SUBMIT_ERROR".to_owned();
            lane.submit_error = Some(compact_error(error));
            (lane, None)
        }
    }
}

async fn submit_direct_jito(
    mut lane: LaneRecord,
    tx: VersionedTransaction,
    client: &JitoClient,
    observer: YellowstoneObserver,
    barrier: Arc<Barrier>,
    run_origin: Instant,
) -> LaneRecord {
    barrier.wait().await;
    let started = Instant::now();
    lane.submit_at = Some(started);
    lane.submit_start_ns_relative = Some(relative_ns(started, run_origin));
    lane.submit_observed_slot = observer.latest_slot();
    match client.submit_bundle(vec![tx]).await {
        Ok(signature) if signature.to_string() == lane.local_signature => {
            let acked = Instant::now();
            lane.ack_at = Some(acked);
            lane.ack_ms = Some(elapsed_ms(acked, started));
            lane.provider_returned_signature = Some(signature.to_string());
            lane.provider_status = Some("ACK_LOCAL_SIGNATURE_FROM_TRIGGER_JITOCLIENT".to_owned());
            lane.outcome = "ACK_ONLY".to_owned();
        }
        Ok(signature) => {
            lane.provider_returned_signature = Some(signature.to_string());
            lane.outcome = "FAILED_SIGNATURE_MISMATCH".to_owned();
            lane.submit_error = Some("trigger::JitoClient zwrócił niezgodną signature".to_owned());
        }
        Err(error) => {
            lane.outcome = "SUBMIT_ERROR".to_owned();
            lane.submit_error = Some(compact_error(error));
        }
    }
    lane
}

async fn submit_nln(
    mut lane: LaneRecord,
    bytes: Vec<u8>,
    rpc: NlnRpc,
    observer: YellowstoneObserver,
    barrier: Arc<Barrier>,
    run_origin: Instant,
) -> LaneRecord {
    barrier.wait().await;
    let started = Instant::now();
    lane.submit_at = Some(started);
    lane.submit_start_ns_relative = Some(relative_ns(started, run_origin));
    lane.submit_observed_slot = observer.latest_slot();
    match rpc
        .call(
            "sendBundle",
            json!([[BASE64.encode(bytes)], {"encoding":"base64"}]),
        )
        .await
    {
        Ok(value) => match value
            .get("result")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        {
            Some(bundle_id) => {
                let acked = Instant::now();
                lane.ack_at = Some(acked);
                lane.ack_ms = Some(elapsed_ms(acked, started));
                lane.bundle_id_or_none = Some(bundle_id.to_owned());
                lane.provider_status = Some("ACK_BUNDLE_ID".to_owned());
                lane.outcome = "ACK_ONLY".to_owned();
            }
            None => {
                lane.outcome = "SUBMIT_ERROR".to_owned();
                lane.submit_error = Some("NLN sendBundle nie zwrócił bundle_id".to_owned());
            }
        },
        Err(error) => {
            lane.outcome = "SUBMIT_ERROR".to_owned();
            lane.submit_error = Some(compact_error(error));
        }
    }
    lane
}

fn apply_signature_status(lane: &mut LaneRecord, status: Option<&Value>) {
    let Some(status) = status.filter(|status| !status.is_null()) else {
        return;
    };
    if lane.landed_slot.is_none() {
        lane.landed_slot = status.get("slot").and_then(Value::as_u64);
    }
    if let Some(error) = status.get("err").filter(|error| !error.is_null()) {
        lane.on_chain_err = Some(error.to_string());
        lane.outcome = "ON_CHAIN_ERROR".to_owned();
        return;
    }
    match status.get("confirmationStatus").and_then(Value::as_str) {
        Some("finalized") => {
            lane.confirmed = true;
            lane.finalized = true;
            lane.outcome = "FINALIZED".to_owned();
        }
        Some("confirmed") => {
            lane.confirmed = true;
            if lane.outcome == "ACK_ONLY" {
                lane.outcome = "CONFIRMED".to_owned();
            }
        }
        _ => {}
    }
    lane.delta_slot = match (lane.landed_slot, lane.submit_observed_slot) {
        (Some(landed), Some(submitted)) => Some(landed as i64 - submitted as i64),
        _ => None,
    };
}

async fn update_nln_bundle_status(lane: &mut LaneRecord, rpc: &NlnRpc) {
    if lane.lane != "NLN_SENDBUNDLE" {
        return;
    }
    let Some(bundle_id) = lane.bundle_id_or_none.clone() else {
        return;
    };
    match rpc.call("getBundleStatuses", json!([[bundle_id]])).await {
        Ok(value) => {
            if let Some(status) = value
                .pointer("/result/value/0")
                .filter(|entry| !entry.is_null())
            {
                lane.provider_status = Some(format!(
                    "BUNDLE_STATUS:{}",
                    compact_json(status).chars().take(280).collect::<String>()
                ));
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
                .get_or_insert_with(|| format!("NLN bundle status: {}", compact_error(error)));
        }
    }
}

async fn populate_fee(lane: &mut LaneRecord, rpc: &NlnRpc) {
    if !lane.finalized || lane.fee_lamports.is_some() {
        return;
    }
    for _ in 0..5 {
        if let Ok(value) = rpc
            .call(
                "getTransaction",
                json!([lane.local_signature, {"encoding":"json", "commitment":"finalized", "maxSupportedTransactionVersion":0}]),
            )
            .await
        {
            if let Some(fee) = value.pointer("/result/meta/fee").and_then(Value::as_u64) {
                lane.fee_lamports = Some(fee);
                lane.total_cost_lamports = Some(fee.saturating_add(lane.tip_lamports));
                return;
            }
        }
        sleep(Duration::from_millis(250)).await;
    }
    lane.observer_error
        .get_or_insert_with(|| "nie odczytano final fee z getTransaction".to_owned());
}

async fn reconcile(
    lanes: &mut [LaneRecord],
    rpc: &NlnRpc,
    observer: &YellowstoneObserver,
) -> Result<()> {
    let deadline = Instant::now() + RECONCILIATION_TIMEOUT;
    let mut last_bundle_status = None;
    loop {
        for lane in lanes.iter_mut() {
            observer.apply(lane);
        }
        let signatures: Vec<String> = lanes
            .iter()
            .map(|lane| lane.local_signature.clone())
            .collect();
        match rpc
            .call(
                "getSignatureStatuses",
                json!([signatures, {"searchTransactionHistory":false}]),
            )
            .await
        {
            Ok(value) => {
                let statuses = value
                    .pointer("/result/value")
                    .and_then(Value::as_array)
                    .context("getSignatureStatuses bez result.value")?;
                for (lane, status) in lanes.iter_mut().zip(statuses.iter()) {
                    apply_signature_status(lane, Some(status));
                }
            }
            Err(error) => {
                for lane in lanes.iter_mut() {
                    lane.observer_error.get_or_insert_with(|| {
                        format!("signature status: {}", compact_error(&error))
                    });
                }
            }
        }
        if last_bundle_status
            .map(|at: Instant| at.elapsed() >= BUNDLE_STATUS_INTERVAL)
            .unwrap_or(true)
        {
            for lane in lanes.iter_mut() {
                update_nln_bundle_status(lane, rpc).await;
            }
            last_bundle_status = Some(Instant::now());
        }
        if lanes
            .iter()
            .all(|lane| lane.finalized || lane.on_chain_err.is_some())
        {
            break;
        }
        if Instant::now() >= deadline {
            for lane in lanes.iter_mut() {
                if !lane.finalized && lane.on_chain_err.is_none() {
                    lane.outcome = "UNKNOWN_AFTER_RECONCILIATION_TIMEOUT".to_owned();
                }
            }
            break;
        }
        sleep(STATUS_POLL_INTERVAL).await;
    }
    for lane in lanes.iter_mut() {
        observer.apply(lane);
        populate_fee(lane, rpc).await;
    }
    Ok(())
}

fn apply_confirmation(
    lane: &mut LaneRecord,
    confirmation: Result<ProductionConfirmation, tokio::task::JoinError>,
) {
    match confirmation {
        Ok(result) => {
            lane.production_confirm_start_after_ack_ms = Some(result.start_after_ack_ms);
            lane.production_confirm_ms_or_none = Some(result.total_ms);
            lane.production_confirm_outcome = Some(result.outcome);
            lane.production_confirm_slot = result.landed_slot;
            // `LiveTxSender::confirm_submission` nie eksponuje granicy połączenie → subscribe;
            // bez modyfikacji produkcyjnego kodu nie można uczciwie rozdzielić jej od całości.
            lane.production_confirm_fresh_connection_ms = None;
        }
        Err(error) => {
            lane.production_confirm_outcome = Some(format!("JOIN_ERROR:{}", compact_error(error)));
        }
    }
}

fn choose_tip_accounts(
    sender: &LiveTxSender,
    direct: &[Pubkey],
    nln: &[Pubkey],
    triplet_id: usize,
) -> Result<(Pubkey, Pubkey, Pubkey)> {
    let helius = sender.select_tip_account(format!("GHOST_HELIUS_BENCH:{triplet_id}").as_bytes());
    let direct_tip = *direct
        .get((triplet_id - 1) % direct.len())
        .context("pusta lista Direct Jito tip accounts")?;
    let nln_tip = nln
        .iter()
        .copied()
        .cycle()
        .skip(triplet_id % nln.len())
        .find(|candidate| *candidate != direct_tip && *candidate != helius)
        .context("brak odrębnego NLN tip account")?;
    if helius == direct_tip || helius == nln_tip || direct_tip == nln_tip {
        bail!("tip accounts lane nie są rozłączne");
    }
    Ok((helius, direct_tip, nln_tip))
}

async fn stage_zero(
    env: &Environment,
    rpc: &NlnRpc,
    payer: &Keypair,
    sender: &LiveTxSender,
    jito_http: &Client,
    requested_triplets: usize,
) -> Result<(StageZeroReport, Vec<Pubkey>, Vec<Pubkey>)> {
    let sender_endpoint = resolve_live_sender_endpoint();
    let ping_status = helius_ping(&sender_endpoint).await?;
    if !(200..300).contains(&ping_status) {
        bail!("Helius ping zwrócił HTTP {ping_status}");
    }
    let balance_lamports = balance(rpc, &payer.pubkey()).await?;
    let hard_cap = requested_triplets as u64 * 3 * maximum_cost_per_transaction();
    if balance_lamports < hard_cap {
        bail!("saldo {balance_lamports} < hard cap {hard_cap}; brak live submitu");
    }
    let direct_tips = jito_tip_accounts(jito_http).await?;
    let nln_tips = nln_tip_accounts(rpc).await?;
    if direct_tips.is_empty() || nln_tips.is_empty() {
        bail!("pusta lista tip accounts w Stage 0");
    }
    let empty = rpc
        .raw_call("sendBundle", json!([[], {"encoding":"base64"}]))
        .await?;
    let empty_nln_bundle_rejected = empty.get("error").is_some();
    if !empty_nln_bundle_rejected {
        bail!("NLN puste sendBundle nie zostało odrzucone walidacyjnie");
    }
    let latest = latest_blockhash(rpc).await?;
    let (helius_tip, direct_tip, nln_tip) =
        choose_tip_accounts(sender, &direct_tips, &nln_tips, 1)?;
    let helius = build_transaction(payer, "HLS", 0, helius_tip, latest.hash)?;
    let direct = build_transaction(payer, "JTO", 0, direct_tip, latest.hash)?;
    let nln = build_transaction(payer, "NLN", 0, nln_tip, latest.hash)?;
    let simulations = simulate_triplet(rpc, &helius, &direct, &nln, "STAGE_0").await?;
    Ok((
        StageZeroReport {
            stage: "STAGE_0_NO_CHAIN_SUBMIT",
            on_chain_submit_performed: false,
            payer_fingerprint: fingerprint(&payer.pubkey()),
            balance_lamports,
            helius_endpoint: sender_endpoint,
            helius_ping_http_status: ping_status,
            nln_rpc_endpoint: NLN_RPC_ENDPOINT,
            primary_observer_endpoint: normalize_grpc_endpoint(&env.grpc_endpoint),
            direct_jito_tip_accounts: direct_tips.len(),
            nln_tip_accounts: nln_tips.len(),
            selected_helius_tip_account: helius_tip.to_string(),
            selected_direct_jito_tip_account: direct_tip.to_string(),
            selected_nln_tip_account: nln_tip.to_string(),
            all_lane_tip_accounts_distinct: true,
            empty_nln_bundle_rejected,
            simulation_units_consumed: simulations,
            compute_unit_limit: COMPUTE_UNIT_LIMIT,
            compute_unit_price_micro_lamports: COMPUTE_UNIT_PRICE_MICRO_LAMPORTS,
            tip_lamports: TIP_LAMPORTS,
            maximum_cost_per_transaction_lamports: maximum_cost_per_transaction(),
            hard_cap_lamports: hard_cap,
            requested_triplets,
            observer_ready: false,
        },
        direct_tips,
        nln_tips,
    ))
}

async fn run_triplet(
    triplet_id: usize,
    payer: &Keypair,
    rpc: &NlnRpc,
    observer: &YellowstoneObserver,
    sender: &LiveTxSender,
    direct_jito: &JitoClient,
    direct_tips: &[Pubkey],
    nln_tips: &[Pubkey],
    run_origin: Instant,
) -> Result<TripletReport> {
    let latest = latest_blockhash(rpc).await?;
    let (helius_tip, direct_tip, nln_tip) =
        choose_tip_accounts(sender, direct_tips, nln_tips, triplet_id)?;
    let helius = build_transaction(payer, "HLS", triplet_id, helius_tip, latest.hash)?;
    let direct = build_transaction(payer, "JTO", triplet_id, direct_tip, latest.hash)?;
    let nln = build_transaction(payer, "NLN", triplet_id, nln_tip, latest.hash)?;
    let simulations = simulate_triplet(rpc, &helius, &direct, &nln, "PRE_SUBMIT").await?;
    observer.register(&helius.signature);
    observer.register(&direct.signature);
    observer.register(&nln.signature);
    let mut helius_lane = new_lane(
        triplet_id,
        "HELIUS_SENDER",
        helius.signature,
        helius.tip_account,
        &payer.pubkey(),
    );
    let mut direct_lane = new_lane(
        triplet_id,
        "DIRECT_JITO_GRPC",
        direct.signature,
        direct.tip_account,
        &payer.pubkey(),
    );
    let mut nln_lane = new_lane(
        triplet_id,
        "NLN_SENDBUNDLE",
        nln.signature,
        nln.tip_account,
        &payer.pubkey(),
    );
    helius_lane.simulation_units_consumed = simulations[0];
    direct_lane.simulation_units_consumed = simulations[1];
    nln_lane.simulation_units_consumed = simulations[2];
    let barrier = Arc::new(Barrier::new(3));
    let (helius_result, direct_result, nln_result) = tokio::join!(
        submit_helius(
            helius_lane,
            helius.transaction,
            sender.clone(),
            observer.clone(),
            Arc::clone(&barrier),
            run_origin,
        ),
        submit_direct_jito(
            direct_lane,
            direct.transaction,
            direct_jito,
            observer.clone(),
            Arc::clone(&barrier),
            run_origin,
        ),
        submit_nln(
            nln_lane,
            nln.bytes,
            rpc.clone(),
            observer.clone(),
            barrier,
            run_origin,
        ),
    );
    let (mut helius_lane, confirmation) = helius_result;
    let mut direct_lane = direct_result;
    let mut nln_lane = nln_result;
    let starts = [
        helius_lane.submit_at,
        direct_lane.submit_at,
        nln_lane.submit_at,
    ];
    let submit_start_gap_ms = starts
        .iter()
        .flatten()
        .copied()
        .min()
        .zip(starts.iter().flatten().copied().max())
        .map(|(first, last)| elapsed_ms(last, first));
    for lane in [&mut helius_lane, &mut direct_lane, &mut nln_lane] {
        lane.submit_start_gap_ms = submit_start_gap_ms;
    }
    let mut lanes = vec![helius_lane, direct_lane, nln_lane];
    reconcile(&mut lanes, rpc, observer).await?;
    if let Some(confirmation) = confirmation {
        apply_confirmation(&mut lanes[0], confirmation.await);
    }
    let pairing_quality = if submit_start_gap_ms.map(|gap| gap <= 10.0).unwrap_or(false) {
        "PAIRED_ELIGIBLE_GAP_LE_10MS".to_owned()
    } else {
        "DIAGNOSTIC_ONLY_GAP_OVER_10MS_OR_SUBMIT_FAILURE".to_owned()
    };
    Ok(TripletReport {
        triplet_id,
        shared_blockhash_context_slot: latest.context_slot,
        shared_last_valid_block_height: latest.last_valid_block_height,
        submit_start_gap_ms,
        pairing_quality,
        stage_one_balance_before_lamports: None,
        stage_one_balance_after_lamports: None,
        stage_one_balance_delta_matches_cost: None,
        lanes,
    })
}

fn triplet_is_safe_for_stage_two(triplet: &TripletReport, balance_matches: bool) -> bool {
    balance_matches
        && triplet.lanes.len() == 3
        && triplet.lanes.iter().all(|lane| {
            lane.finalized
                && lane.on_chain_err.is_none()
                && lane.fee_lamports.is_some()
                && lane.total_cost_lamports.is_some()
                && lane.submit_error.is_none()
                && (lane.lane != "HELIUS_SENDER"
                    || lane.provider_returned_signature.as_deref()
                        == Some(lane.local_signature.as_str()))
        })
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse()?;
    let env = Environment::read()?;
    let payer = read_keypair_file(&env.payer_path)
        .map_err(|error| anyhow!("nie można odczytać testowego keypaira z {PAYER_ENV}: {error}"))?;
    let rpc = NlnRpc::new(env.nln_api_key.clone())?;
    let sender = LiveTxSender::new(LiveTxSenderConfig::new(
        resolve_live_sender_endpoint(),
        env.priority_rpc_url.clone(),
        env.grpc_endpoint.clone(),
        env.grpc_token.clone(),
    ))
    .context("utworzenie persistent LiveTxSender")?;
    let jito_http = Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(15))
        .tcp_nodelay(true)
        .build()
        .context("budowa Direct Jito tip HTTP")?;
    let (mut stage_zero, direct_tips, nln_tips) =
        stage_zero(&env, &rpc, &payer, &sender, &jito_http, args.max_triplets).await?;
    let observer =
        YellowstoneObserver::start(&env.grpc_endpoint, &env.grpc_token, &payer.pubkey()).await?;
    wait_for_observer_ready(&observer).await?;
    stage_zero.observer_ready = true;
    if !args.execute {
        println!(
            "{}",
            serde_json::to_string_pretty(&BenchmarkReport {
                benchmark: "helius_sender_vs_direct_jito_vs_nln_v1",
                tool_version: "2026-07-23.1",
                stage_zero,
                execution_requested: false,
                stopped_after_stage: "STAGE_0_COMPLETE_NO_SUBMIT".to_owned(),
                balance_before_lamports: 0,
                balance_after_lamports: None,
                expected_total_cost_lamports: None,
                actual_balance_delta_lamports: None,
                triplets: vec![],
                notes: vec!["Bez --execute nie wysłano żadnej transakcji.".to_owned()],
            })?
        );
        return Ok(());
    }
    let direct_jito = JitoClient::new(DIRECT_JITO_ENDPOINT, BundleConfig::default());
    let run_origin = Instant::now();
    let balance_before = balance(&rpc, &payer.pubkey()).await?;
    let hard_cap = args.max_triplets as u64 * 3 * maximum_cost_per_transaction();
    if balance_before < hard_cap {
        bail!("ABORT przed Stage 1: saldo {balance_before} < hard cap {hard_cap}");
    }
    let mut triplets = Vec::new();
    let mut stopped_after_stage = "STAGE_2_COMPLETE".to_owned();
    let stage_one_before = balance_before;
    let mut stage_one_after = None;
    for triplet_id in 1..=args.max_triplets {
        let mut triplet = run_triplet(
            triplet_id,
            &payer,
            &rpc,
            &observer,
            &sender,
            &direct_jito,
            &direct_tips,
            &nln_tips,
            run_origin,
        )
        .await?;
        if triplet_id == 1 {
            let after = balance(&rpc, &payer.pubkey()).await?;
            let expected = triplet
                .lanes
                .iter()
                .map(|lane| lane.total_cost_lamports)
                .collect::<Option<Vec<_>>>()
                .map(|costs| costs.into_iter().sum::<u64>());
            let delta = stage_one_before.saturating_sub(after);
            let matches = expected.map(|cost| cost == delta).unwrap_or(false);
            triplet.stage_one_balance_before_lamports = Some(stage_one_before);
            triplet.stage_one_balance_after_lamports = Some(after);
            triplet.stage_one_balance_delta_matches_cost = Some(matches);
            stage_one_after = Some(after);
            let continue_stage_two = triplet_is_safe_for_stage_two(&triplet, matches);
            triplets.push(triplet);
            if !continue_stage_two {
                stopped_after_stage = "STAGE_1_STOP_INTEGRITY_FAILURE_OR_UNKNOWN".to_owned();
                break;
            }
        } else {
            triplets.push(triplet);
        }
    }
    let balance_after = balance(&rpc, &payer.pubkey()).await.ok();
    let expected_total_cost = triplets
        .iter()
        .flat_map(|triplet| triplet.lanes.iter().map(|lane| lane.total_cost_lamports))
        .collect::<Option<Vec<_>>>()
        .map(|costs| costs.into_iter().sum::<u64>());
    let actual_balance_delta = balance_after.map(|after| balance_before.saturating_sub(after));
    let report = BenchmarkReport {
        benchmark: "helius_sender_vs_direct_jito_vs_nln_v1",
        tool_version: "2026-07-23.1",
        stage_zero,
        execution_requested: true,
        stopped_after_stage,
        balance_before_lamports: balance_before,
        balance_after_lamports: balance_after,
        expected_total_cost_lamports: expected_total_cost,
        actual_balance_delta_lamports: actual_balance_delta,
        triplets,
        notes: vec![
            "Wspólny payer jest jawnym wyjątkiem zatwierdzonym przez operatora; może wprowadzać account write-lock contention między lane.".to_owned(),
            "ACK jest wyłącznie potwierdzeniem transportowym. Finalny wynik pochodzi z niezależnej reconciliation on-chain; processed oznacza czas pierwszej obserwacji w tym Yellowstone, nie czas wykonania validatora.".to_owned(),
            "Exact public trigger::JitoClient::submit_bundle zwraca local signature po ACK, lecz nie ujawnia Jito UUID; benchmark nie zastępuje go ręcznym transportem ani retry dla samego UUID.".to_owned(),
            "LiveTxSender::confirm_submission jest wywołany bez zmian. Implementacja nie eksponuje osobnego znacznika końca świeżego połączenia Yellowstone, dlatego ta składowa pozostaje null zamiast szacunku.".to_owned(),
        ],
    };
    let _ = stage_one_after;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_cost_is_exact_for_declared_upper_bound() {
        assert_eq!(priority_fee_upper_bound_lamports(), 1_250);
        assert_eq!(maximum_cost_per_transaction(), 1_006_250);
    }

    #[test]
    fn signature_status_absence_never_creates_success() {
        let payer = Pubkey::new_unique();
        let mut lane = new_lane(
            1,
            "NLN_SENDBUNDLE",
            Signature::new_unique(),
            Pubkey::new_unique(),
            &payer,
        );
        apply_signature_status(&mut lane, None);
        assert!(!lane.confirmed);
        assert!(!lane.finalized);
        assert_eq!(lane.outcome, "PREPARED");
    }
}
