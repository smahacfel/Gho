//! Seer component wrapper

use crate::config::{
    redact_endpoint_for_logs, ProgramStreamsQuotaPolicy as LauncherProgramStreamsQuotaPolicy,
    SeerCommitment, SeerComponentConfig,
};
use crate::events::{
    AccountUpdateEvent, DetectedPool, EventBusSender, FundingTransferObserved, GhostEvent,
};
use anyhow::Result;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use futures::StreamExt;
use ghost_brain::oracle::{InitPoolEvent, SnapshotEngine};
use ghost_core::health::RuntimeHealth;
use ghost_core::shadow_ledger::ShadowLedger;
use ghost_core::{ExecutionAccountRole, TimestampQuality, Wal};
use metrics::increment_counter;
use seer::{
    config::{
        ConnectionMode, FilterConfig, FundingLaneMode, ProgramStreamPayloadFormat,
        ProgramStreamsConfig, ProgramStreamsQuotaPolicy as SeerProgramStreamsQuotaPolicy,
        PumpPortalConfig, SeerConfig, SeerSourceMode, StreamMode, TxFilterStrategy,
    },
    ipc::{create_ipc_channel, BackpressurePolicy, FundingLaneRuntimeHealth, IpcChannelConfig},
    nln_program_streams::{
        normalize_nln_event, NlnEvent, NlnFundingTransferCoverage, NlnProgramStreamMessage,
        NlnProgramStreamsClient, NlnPumpFunCreateEvent, NlnPumpFunTradeEvent,
        NlnSubscribeLoopOptions, NlnTransferEvent,
    },
    Seer,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::env;
use std::hash::Hash;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
// SystemTime is used transitively via event.detected_at.elapsed()
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use std::time::{Duration, Instant};
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::mpsc;
use tokio::sync::{broadcast, watch};
use tracing::{debug, error, info, warn};

const SYSTEM_PROGRAM_ID: &str = "11111111111111111111111111111111";
const PUMPSWAP_PROGRAM_ID_STR: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
const COMPUTE_BUDGET_PROGRAM_ID: &str = "ComputeBudget111111111111111111111111111111";
const ASSOCIATED_TOKEN_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
const SESSION_POOL_TRADE_BUFFER_TTL: Duration = Duration::from_millis(10);
const SESSION_POOL_TRADE_BUFFER_PER_POOL_CAP: usize = 64;
const SESSION_POOL_TRADE_BUFFER_GLOBAL_CAP: usize = 2_048;
const SESSION_ACCOUNT_UPDATE_BUFFER_TTL: Duration = Duration::from_secs(2);
const SESSION_ACCOUNT_UPDATE_BUFFER_PER_KEY_CAP: usize = 8;
const SESSION_ACCOUNT_UPDATE_BUFFER_GLOBAL_CAP: usize = 4_096;
const SESSION_POOL_REGISTRY_FALLBACK_TTL: Duration = Duration::from_secs(30 * 60);
const SESSION_POOL_REGISTRY_FALLBACK_CAP: usize = 16_384;
const SESSION_POOL_BRIDGE_PRUNE_INTERVAL: Duration = Duration::from_millis(250);
const NLN_ARTIFACT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
struct NlnArtifactCaptureConfig {
    enabled: bool,
    capture_dir: PathBuf,
    queue_capacity: usize,
    flush_interval: Duration,
    transfer_sample_rate: u32,
}

impl NlnArtifactCaptureConfig {
    fn from_launcher(config: &crate::config::SeerProgramStreamsComponentConfig) -> Self {
        let capture_dir = config
            .artifact_capture_dir
            .as_ref()
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("logs/nln_capture/default"));
        let flush_ms = config.artifact_flush_interval_ms.max(250);
        Self {
            enabled: config.artifact_capture_enabled,
            capture_dir,
            queue_capacity: config.artifact_queue_capacity.max(1_024),
            flush_interval: Duration::from_millis(flush_ms),
            transfer_sample_rate: config.artifact_transfer_sample_rate.max(1),
        }
    }
}

#[derive(Debug)]
enum NlnArtifactRecord {
    PumpFunCreateRaw(Value),
    PumpFunTradeRaw(Value),
    PumpFunBuyRaw(Value),
    PumpFunBuyExactSolInRaw(Value),
    SystemTransfersRaw(Value),
    NormalizationError(Value),
    CandidateBirth(Value),
    RouteManifestEvidenceCandidate(Value),
    FundingEvent(Value),
}

#[derive(Clone)]
struct NlnArtifactWriter {
    tx: mpsc::Sender<NlnArtifactRecord>,
    transfer_sample_rate: u32,
}

impl NlnArtifactWriter {
    async fn send_lossless(&self, record: NlnArtifactRecord, label: &'static str) -> bool {
        match self.tx.send(record).await {
            Ok(()) => {
                metrics::counter!(
                    "seer_nln_program_streams_artifact_records_sent_total",
                    1,
                    "label" => label
                );
                true
            }
            Err(_) => {
                metrics::counter!(
                    "seer_nln_program_streams_artifact_writer_closed_total",
                    1,
                    "label" => label
                );
                warn!(
                    label = %label,
                    "Seer: NLN artifact writer closed; required artifact record could not be persisted"
                );
                false
            }
        }
    }

    fn should_capture_transfer(&self, event: &NlnTransferEvent) -> bool {
        let rate = self.transfer_sample_rate;
        if rate <= 1 {
            return true;
        }
        let parts = vec![
            event.signature.clone(),
            event
                .tx_index
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            event
                .instruction_index
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            event.from_wallet.to_string(),
            event.to_wallet.to_string(),
            event.amount_lamports.to_string(),
        ];
        nln_stable_hash_u64(&parts) % u64::from(rate) == 0
    }

    fn should_capture_raw_transfer_message(&self, message: &NlnProgramStreamMessage) -> bool {
        let rate = self.transfer_sample_rate;
        if rate <= 1 {
            return true;
        }
        let body = message.payload_json.as_object();
        let value = |keys: &[&str]| -> String {
            keys.iter()
                .find_map(|key| body.and_then(|object| object.get(*key)))
                .and_then(json_scalar_string)
                .unwrap_or_else(|| "none".to_string())
        };
        let parts = vec![
            value(&["signature", "tx_signature"]),
            value(&["tx_index", "txIndex"]),
            value(&[
                "instruction_index",
                "instructionIndex",
                "outer_instruction_index",
            ]),
            value(&["from_wallet", "fromWallet", "source_wallet", "from"]),
            value(&["to_wallet", "toWallet", "recipient_wallet", "to"]),
            value(&["amount", "amount_lamports", "lamports"]),
            message.partition.to_string(),
            message.offset_raw.clone(),
        ];
        nln_stable_hash_u64(&parts) % u64::from(rate) == 0
    }
}

fn json_scalar_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) if !value.is_empty() => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Object(object) => object.get("value").and_then(json_scalar_string),
        _ => None,
    }
}

fn json_account_pubkey_string(value: &Value) -> Option<String> {
    let raw = json_scalar_string(value)?;
    if let Ok(bytes) = BASE64_STANDARD.decode(raw.as_bytes()) {
        if let Ok(bytes) = <[u8; 32]>::try_from(bytes.as_slice()) {
            return Some(Pubkey::new_from_array(bytes).to_string());
        }
    }
    Some(raw)
}

fn json_scalar_value(value: &Value) -> Value {
    match value {
        Value::Object(object) => object
            .get("value")
            .map(json_scalar_value)
            .unwrap_or(Value::Null),
        Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null => value.clone(),
        _ => Value::Null,
    }
}

fn resolve_program_streams_endpoint(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(var_name) = trimmed
        .strip_prefix("${")
        .and_then(|value| value.strip_suffix('}'))
    {
        if let Ok(value) = env::var(var_name) {
            let value = value.trim();
            if !value.is_empty() {
                return value.to_string();
            }
        }
    }
    trimmed.to_string()
}

fn nln_program_stream_run_scope(config: &ProgramStreamsConfig) -> Option<String> {
    config
        .artifact_capture_dir
        .as_deref()
        .and_then(|dir| std::path::Path::new(dir).file_name())
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}

fn nln_artifact_raw_row(
    message: &NlnProgramStreamMessage,
    config: &ProgramStreamsConfig,
    capture_version: &'static str,
) -> Value {
    json!({
        "schema_version": NLN_ARTIFACT_SCHEMA_VERSION,
        "provider": "NLN",
        "stream_kind": "program_stream",
        "run_scope": nln_program_stream_run_scope(config),
        "capture_version": capture_version,
        "source_endpoint": config.endpoint,
        "payload_format": config.format.as_str(),
        "parse_status": "raw_captured",
        "received_at_ms": message.recv_ts_ms,
        "topic": message.topic,
        "partition": message.partition,
        "offset_raw": message.offset_raw,
        "offset": message.offset,
        "provider_ts_ms": message.provider_ts_ms,
        "recv_ts_ms": message.recv_ts_ms,
        "recv_ts_ns": message.recv_ts_ns,
        "decode_ts_ms": message.decode_ts_ms,
        "payload_json": message.payload_json,
    })
}

fn nln_value_at_path<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    let object = value.as_object()?;
    keys.iter().find_map(|key| object.get(*key))
}

fn nln_nested_value<'a>(payload: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    nln_value_at_path(payload, keys).or_else(|| {
        ["payload", "data", "event", "accounts", "args"]
            .iter()
            .find_map(|container| {
                payload
                    .get(*container)
                    .and_then(|value| nln_value_at_path(value, keys))
            })
    })
}

fn nln_nested_scalar_string(payload: &Value, keys: &[&str]) -> Option<String> {
    nln_nested_value(payload, keys).and_then(json_scalar_string)
}

fn nln_nested_account_pubkey_string(payload: &Value, keys: &[&str]) -> Option<String> {
    nln_nested_value(payload, keys).and_then(json_account_pubkey_string)
}

fn nln_nested_u64(payload: &Value, keys: &[&str]) -> Option<u64> {
    nln_nested_value(payload, keys).and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_str().and_then(|value| value.parse::<u64>().ok()))
    })
}

fn nln_route_account_role_value(payload: &Value, role: &str, aliases: &[&str]) -> Value {
    json!({
        "role": role,
        "pubkey": nln_nested_account_pubkey_string(payload, aliases),
        "source": "nln_program_stream_named_account",
    })
}

fn nln_route_named_accounts(payload: &Value) -> Vec<Value> {
    [
        ("global", &["global", "global_config", "globalConfig"][..]),
        ("mint", &["mint", "base_mint", "baseMint"][..]),
        ("bonding_curve", &["bonding_curve", "bondingCurve"][..]),
        (
            "associated_bonding_curve",
            &["associated_bonding_curve", "associatedBondingCurve"][..],
        ),
        (
            "associated_user",
            &[
                "associated_user",
                "associatedUser",
                "associated_token_account",
            ][..],
        ),
        ("user", &["user", "buyer"][..]),
        ("fee_recipient", &["fee_recipient", "feeRecipient"][..]),
        ("creator_vault", &["creator_vault", "creatorVault"][..]),
        ("token_program", &["token_program", "tokenProgram"][..]),
        ("system_program", &["system_program", "systemProgram"][..]),
        (
            "event_authority",
            &["event_authority", "eventAuthority"][..],
        ),
        (
            "global_volume_accumulator",
            &["global_volume_accumulator", "globalVolumeAccumulator"][..],
        ),
        (
            "user_volume_accumulator",
            &["user_volume_accumulator", "userVolumeAccumulator"][..],
        ),
        ("fee_config", &["fee_config", "feeConfig"][..]),
        ("fee_program", &["fee_program", "feeProgram"][..]),
        ("program", &["program", "program_id", "programId"][..]),
    ]
    .into_iter()
    .map(|(role, aliases)| nln_route_account_role_value(payload, role, aliases))
    .collect()
}

fn nln_route_remaining_accounts(payload: &Value) -> Vec<Value> {
    let Some(value) = nln_nested_value(payload, &["remaining_accounts", "remainingAccounts"])
    else {
        return Vec::new();
    };
    let Some(accounts) = value.as_array() else {
        return Vec::new();
    };
    accounts
        .iter()
        .enumerate()
        .map(|(index, account)| {
            json!({
                "index": index,
                "pubkey": json_account_pubkey_string(account),
                "source": "nln_program_stream_remaining_account",
            })
        })
        .collect()
}

fn nln_route_args(payload: &Value, topic_kind: NlnProgramStreamCaptureTopic) -> Vec<Value> {
    let fields: &[(&str, &[&str])] = match topic_kind {
        NlnProgramStreamCaptureTopic::PumpFunBuy => &[
            ("amount", &["amount"][..]),
            ("max_sol_cost", &["max_sol_cost", "maxSolCost"][..]),
            ("track_volume", &["track_volume", "trackVolume"][..]),
        ],
        NlnProgramStreamCaptureTopic::PumpFunBuyExactSolIn => &[
            (
                "spendable_sol_in",
                &["spendable_sol_in", "spendableSolIn"][..],
            ),
            ("min_tokens_out", &["min_tokens_out", "minTokensOut"][..]),
            ("track_volume", &["track_volume", "trackVolume"][..]),
        ],
        _ => &[],
    };
    fields
        .iter()
        .map(|(name, aliases)| {
            json!({
                "name": name,
                "value": nln_nested_value(payload, aliases)
                    .map(json_scalar_value)
                    .unwrap_or(Value::Null),
            })
        })
        .collect()
}

fn nln_route_kind_for_topic(topic_kind: NlnProgramStreamCaptureTopic) -> Option<&'static str> {
    match topic_kind {
        NlnProgramStreamCaptureTopic::PumpFunBuy => Some("legacy_buy"),
        NlnProgramStreamCaptureTopic::PumpFunBuyExactSolIn => Some("routed_exact_sol_in"),
        _ => None,
    }
}

fn nln_route_manifest_evidence_candidate_row(
    message: &NlnProgramStreamMessage,
    topic_kind: NlnProgramStreamCaptureTopic,
    config: &ProgramStreamsConfig,
) -> Option<Value> {
    let route_kind = nln_route_kind_for_topic(topic_kind)?;
    let payload = &message.payload_json;
    let signature =
        nln_nested_scalar_string(payload, &["signature", "tx_signature", "txSignature"]);
    let slot = nln_nested_u64(payload, &["slot"]);
    let tx_index = nln_nested_u64(payload, &["tx_index", "txIndex"]);
    let ix_index = nln_nested_u64(
        payload,
        &[
            "ix_index",
            "ixIndex",
            "instruction_index",
            "instructionIndex",
            "outer_instruction_index",
        ],
    );
    let named_accounts = nln_route_named_accounts(payload);
    let remaining_accounts = nln_route_remaining_accounts(payload);
    let args = nln_route_args(payload, topic_kind);
    let args_hash = nln_stable_hash(&[serde_json::to_string(&args).unwrap_or_default()]);
    let named_accounts_value = json!(named_accounts);
    let mut manifest_layout = BTreeMap::new();
    manifest_layout.insert("schema_version", json!(1));
    manifest_layout.insert("route_kind", json!(route_kind));
    manifest_layout.insert("program_id", json!("pump_fun"));
    manifest_layout.insert("named_accounts", named_accounts_value.clone());
    manifest_layout.insert("remaining_accounts", json!(remaining_accounts.clone()));
    manifest_layout.insert(
        "remaining_accounts_status",
        if remaining_accounts.is_empty() {
            json!("unknown_until_raw_grpc_join")
        } else {
            json!("program_stream_tail_observed")
        },
    );
    let account_manifest_hash =
        nln_stable_hash(&[serde_json::to_string(&manifest_layout).unwrap_or_default()]);
    let instruction_evidence = json!({
        "schema_version": 1,
        "source": "nln_program_stream",
        "source_endpoint": config.endpoint,
        "topic": message.topic,
        "route_kind": route_kind,
        "signature": signature,
        "slot": slot,
        "tx_index": tx_index,
        "ix_index": ix_index,
        "named_accounts": named_accounts_value,
        "args_hash": args_hash,
        "args": args,
        "account_manifest_hash": account_manifest_hash,
    });
    let instruction_evidence_hash =
        nln_stable_hash(&[serde_json::to_string(&instruction_evidence).unwrap_or_default()]);
    let strong_join_keys_present =
        signature.is_some() && slot.is_some() && tx_index.is_some() && ix_index.is_some();
    let join_status = if strong_join_keys_present {
        "pending_raw_grpc_join"
    } else {
        "degraded_missing_tx_or_instruction_index"
    };
    let manifest_status = if strong_join_keys_present {
        "pending_join"
    } else {
        "degraded_pending_join"
    };
    let tail_evidence_status = if remaining_accounts.is_empty() {
        "unknown_until_raw_grpc_join"
    } else {
        "program_stream_tail_observed_unverified"
    };
    let remaining_account_count = remaining_accounts.len();

    let mut row = serde_json::Map::new();
    row.insert("schema_version".to_string(), json!(1));
    row.insert(
        "artifact".to_string(),
        json!("route_manifest_evidence_candidate_v1"),
    );
    row.insert("stream_kind".to_string(), json!("program_stream"));
    row.insert("source".to_string(), json!("nln_program_stream"));
    row.insert("source_endpoint".to_string(), json!(config.endpoint));
    row.insert(
        "run_scope".to_string(),
        json!(nln_program_stream_run_scope(config)),
    );
    row.insert(
        "capture_version".to_string(),
        json!("nln_program_route_evidence_candidate_v1"),
    );
    row.insert("received_at_ms".to_string(), json!(message.recv_ts_ms));
    row.insert("topic".to_string(), json!(message.topic));
    row.insert("topic_kind".to_string(), json!(topic_kind.label()));
    row.insert("route_kind".to_string(), json!(route_kind));
    row.insert("signature".to_string(), json!(signature));
    row.insert("slot".to_string(), json!(slot));
    row.insert("tx_index".to_string(), json!(tx_index));
    row.insert("ix_index".to_string(), json!(ix_index));
    row.insert("parse_status".to_string(), json!("OK"));
    row.insert("named_accounts".to_string(), named_accounts_value);
    row.insert(
        "global".to_string(),
        json!(nln_nested_account_pubkey_string(
            payload,
            &["global", "global_config", "globalConfig"]
        )),
    );
    row.insert(
        "mint".to_string(),
        json!(nln_nested_account_pubkey_string(
            payload,
            &["mint", "base_mint", "baseMint"]
        )),
    );
    row.insert(
        "bonding_curve".to_string(),
        json!(nln_nested_account_pubkey_string(
            payload,
            &["bonding_curve", "bondingCurve"]
        )),
    );
    row.insert(
        "associated_bonding_curve".to_string(),
        json!(nln_nested_account_pubkey_string(
            payload,
            &["associated_bonding_curve", "associatedBondingCurve"]
        )),
    );
    row.insert(
        "associated_user".to_string(),
        json!(nln_nested_account_pubkey_string(
            payload,
            &[
                "associated_user",
                "associatedUser",
                "associated_token_account"
            ]
        )),
    );
    row.insert(
        "user".to_string(),
        json!(nln_nested_account_pubkey_string(
            payload,
            &["user", "buyer"]
        )),
    );
    row.insert(
        "fee_recipient".to_string(),
        json!(nln_nested_account_pubkey_string(
            payload,
            &["fee_recipient", "feeRecipient"]
        )),
    );
    row.insert(
        "creator_vault".to_string(),
        json!(nln_nested_account_pubkey_string(
            payload,
            &["creator_vault", "creatorVault"]
        )),
    );
    row.insert(
        "token_program".to_string(),
        json!(nln_nested_account_pubkey_string(
            payload,
            &["token_program", "tokenProgram"]
        )),
    );
    row.insert(
        "system_program".to_string(),
        json!(nln_nested_account_pubkey_string(
            payload,
            &["system_program", "systemProgram"]
        )),
    );
    row.insert(
        "event_authority".to_string(),
        json!(nln_nested_account_pubkey_string(
            payload,
            &["event_authority", "eventAuthority"]
        )),
    );
    row.insert(
        "global_volume_accumulator".to_string(),
        json!(nln_nested_account_pubkey_string(
            payload,
            &["global_volume_accumulator", "globalVolumeAccumulator"]
        )),
    );
    row.insert(
        "user_volume_accumulator".to_string(),
        json!(nln_nested_account_pubkey_string(
            payload,
            &["user_volume_accumulator", "userVolumeAccumulator"]
        )),
    );
    row.insert(
        "fee_config".to_string(),
        json!(nln_nested_account_pubkey_string(
            payload,
            &["fee_config", "feeConfig"]
        )),
    );
    row.insert(
        "fee_program".to_string(),
        json!(nln_nested_account_pubkey_string(
            payload,
            &["fee_program", "feeProgram"]
        )),
    );
    row.insert(
        "program".to_string(),
        json!(nln_nested_account_pubkey_string(
            payload,
            &["program", "program_id", "programId"]
        )),
    );
    row.insert("args".to_string(), instruction_evidence["args"].clone());
    row.insert("args_hash".to_string(), json!(args_hash));
    row.insert(
        "instruction_evidence_hash".to_string(),
        json!(instruction_evidence_hash),
    );
    row.insert(
        "account_manifest_hash".to_string(),
        json!(account_manifest_hash),
    );
    row.insert("remaining_accounts".to_string(), json!(remaining_accounts));
    row.insert(
        "remaining_accounts_count".to_string(),
        json!(remaining_account_count),
    );
    row.insert(
        "remaining_account_count".to_string(),
        json!(remaining_account_count),
    );
    row.insert(
        "has_legacy_tail".to_string(),
        json!(remaining_account_count == 2),
    );
    row.insert(
        "manifest_hash_notes".to_string(),
        json!({
            "account_manifest_hash_excludes_dynamic_args": true,
            "instruction_evidence_hash_includes_args_hash": true
        }),
    );
    row.insert("join_status".to_string(), json!(join_status));
    row.insert("manifest_status".to_string(), json!(manifest_status));
    row.insert(
        "tail_evidence_status".to_string(),
        json!(tail_evidence_status),
    );
    row.insert(
        "program_stream_complete_is_executable".to_string(),
        json!(false),
    );
    row.insert("degraded_join_can_complete".to_string(), json!(false));
    row.insert("can_unlock_execution".to_string(), json!(false));

    Some(Value::Object(row))
}

fn nln_payload_scalar_string(message: &NlnProgramStreamMessage, keys: &[&str]) -> Option<String> {
    let object = message.payload_json.as_object()?;
    keys.iter()
        .find_map(|key| object.get(*key))
        .and_then(json_scalar_string)
}

fn nln_normalization_error_row(
    message: &NlnProgramStreamMessage,
    topic_kind: NlnProgramStreamCaptureTopic,
    error: &anyhow::Error,
) -> Value {
    let raw_payload_hash = nln_stable_hash(&[message.payload_json.to_string()]);
    json!({
        "schema_version": NLN_ARTIFACT_SCHEMA_VERSION,
        "artifact": "nln_normalization_errors_v1",
        "provider": "NLN",
        "topic": message.topic,
        "topic_kind": topic_kind.label(),
        "partition": message.partition,
        "offset_raw": message.offset_raw,
        "offset": message.offset,
        "signature": nln_payload_scalar_string(message, &["signature", "tx_signature"]),
        "slot": nln_payload_scalar_string(message, &["slot"]),
        "tx_index": nln_payload_scalar_string(message, &["tx_index", "txIndex"]),
        "instruction_index": nln_payload_scalar_string(message, &[
            "instruction_index",
            "instructionIndex",
            "outer_instruction_index"
        ]),
        "provider_ts_ms": message.provider_ts_ms,
        "recv_ts_ms": message.recv_ts_ms,
        "recv_ts_ns": message.recv_ts_ns,
        "decode_ts_ms": message.decode_ts_ms,
        "error": error.to_string(),
        "raw_payload_hash": raw_payload_hash,
    })
}

fn add_nln_artifact_sampling(row: &mut Value, transfer_sample_rate: u32) {
    if let Value::Object(map) = row {
        map.insert(
            "artifact_transfer_sample_rate".to_string(),
            json!(transfer_sample_rate),
        );
        map.insert(
            "artifact_sampled".to_string(),
            json!(transfer_sample_rate > 1),
        );
    }
}

fn nln_effective_event_ts_ms(
    provider_ts_ms: Option<i64>,
    block_time: Option<i64>,
    recv_ts_ms: u64,
) -> u64 {
    if let Some(block_time) = block_time.and_then(|value| (value >= 0).then_some(value as u64)) {
        return block_time.saturating_mul(1_000);
    }
    if let Some(provider_ts_ms) =
        provider_ts_ms.and_then(|value| (value >= 0).then_some(value as u64))
    {
        return provider_ts_ms;
    }
    recv_ts_ms
}

fn unix_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

fn nln_stable_hash(parts: &[String]) -> String {
    format!("fnv64:{:016x}", nln_stable_hash_u64(parts))
}

fn nln_stable_hash_u64(parts: &[String]) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash = FNV_OFFSET;
    for part in parts {
        for byte in part.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn nln_candidate_birth_artifact_row(event: &NlnPumpFunCreateEvent) -> Value {
    let bonding_curve = event.bonding_curve.map(|value| value.to_string());
    let birth_ts_ms = nln_effective_event_ts_ms(
        event.meta.provider_ts_ms,
        event.block_time,
        event.meta.recv_ts_ms,
    );
    let candidate_id = format!(
        "nln:{}:{}:{}",
        event.mint,
        bonding_curve.as_deref().unwrap_or("missing_bonding_curve"),
        event.signature
    );
    let mut missing = Vec::new();
    if bonding_curve.is_none() {
        missing.push("bonding_curve");
    }

    json!({
        "selector_schema_version": NLN_ARTIFACT_SCHEMA_VERSION,
        "artifact": "nln_candidate_birth_v1",
        "candidate_id": candidate_id,
        "candidate_id_source": "nln_mint_bonding_curve_signature",
        "candidate_birth_status": if missing.is_empty() { "ok" } else { "universe_incomplete" },
        "candidate_identity_missing_fields": missing,
        "cohort": "pumpfun_bonding_curve_sol_v1",
        "cohort_in_scope": missing.is_empty(),
        "provider": "NLN",
        "source_topic": event.meta.topic,
        "source_kind": "nln_program_stream",
        "source_partition": event.meta.partition,
        "source_offset": event.meta.offset,
        "source_offset_raw": event.meta.offset_raw,
        "signature": event.signature,
        "slot": event.slot,
        "tx_index": event.tx_index,
        "birth_ts_ms": birth_ts_ms,
        "base_mint": event.mint.to_string(),
        "mint_id": event.mint.to_string(),
        "pool_id": bonding_curve.clone(),
        "bonding_curve": bonding_curve,
        "creator": event.creator.to_string(),
        "quote_mint": "SOL",
        "quote_mint_source": "verified_nln_pumpfun_create_topic",
    })
}

fn nln_funding_event_artifact_row(event: &NlnTransferEvent) -> Value {
    let missing = [
        ("tx_index", event.tx_index.is_none()),
        ("instruction_index", event.instruction_index.is_none()),
    ]
    .into_iter()
    .filter_map(|(field, is_missing)| is_missing.then_some(field))
    .collect::<Vec<_>>();
    let parts = vec![
        event.signature.clone(),
        event
            .tx_index
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".to_string()),
        event
            .instruction_index
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".to_string()),
        event.from_wallet.to_string(),
        event.to_wallet.to_string(),
        event.amount_lamports.to_string(),
    ];
    let event_id = nln_stable_hash(&parts);
    let event_ts_ms =
        nln_effective_event_ts_ms(event.meta.provider_ts_ms, None, event.meta.recv_ts_ms);

    json!({
        "selector_schema_version": NLN_ARTIFACT_SCHEMA_VERSION,
        "artifact": "funding_events_v1",
        "funding_event_status": if missing.is_empty() { "ok" } else { "incomplete" },
        "missing_fields": missing,
        "provider": "NLN",
        "source_topic": event.meta.topic,
        "source_kind": "nln_program_stream",
        "source_partition": event.meta.partition,
        "source_offset": event.meta.offset,
        "source_offset_raw": event.meta.offset_raw,
        "event_id": event_id,
        "signature": event.signature,
        "slot": event.slot,
        "tx_index": event.tx_index,
        "instruction_index": event.instruction_index,
        "event_ts_ms": event_ts_ms,
        "from_wallet": event.from_wallet.to_string(),
        "to_wallet": event.to_wallet.to_string(),
        "amount_lamports": event.amount_lamports,
        "token_address": event.token_address,
        "asset": "native_sol",
        "event_order_key": [
            event.slot.to_string(),
            event.tx_index.map(|value| value.to_string()).unwrap_or_else(|| "none".to_string()),
            event.instruction_index.map(|value| value.to_string()).unwrap_or_else(|| "none".to_string()),
            event.signature.clone()
        ],
    })
}

async fn write_nln_artifact_line(
    writer: &mut Option<BufWriter<tokio::fs::File>>,
    row: &Value,
    label: &'static str,
) {
    let Some(inner) = writer.as_mut() else {
        return;
    };
    let Ok(mut line) = serde_json::to_string(row) else {
        metrics::counter!(
            "seer_nln_program_streams_artifact_serialize_errors_total",
            1,
            "label" => label
        );
        return;
    };
    line.push('\n');
    if let Err(err) = inner.write_all(line.as_bytes()).await {
        warn!(
            label,
            error = %err,
            "Seer: disabling NLN artifact writer after write failure"
        );
        ::metrics::gauge!("nln_artifact_capture_available", 0.0, "label" => label);
        ::metrics::counter!(
            "nln_artifact_capture_degraded",
            1,
            "label" => label,
            "reason" => "write_failure"
        );
        *writer = None;
    } else {
        metrics::counter!(
            "seer_nln_program_streams_artifact_rows_written_total",
            1,
            "label" => label
        );
    }
}

async fn flush_nln_artifact_writer(
    writer: &mut Option<BufWriter<tokio::fs::File>>,
    label: &'static str,
) {
    let Some(inner) = writer.as_mut() else {
        return;
    };
    if let Err(err) = inner.flush().await {
        warn!(
            label,
            error = %err,
            "Seer: disabling NLN artifact writer after flush failure"
        );
        ::metrics::gauge!("nln_artifact_capture_available", 0.0, "label" => label);
        ::metrics::counter!(
            "nln_artifact_capture_degraded",
            1,
            "label" => label,
            "reason" => "flush_failure"
        );
        *writer = None;
    }
}

async fn open_nln_artifact_file(
    path: PathBuf,
    label: &'static str,
) -> Option<BufWriter<tokio::fs::File>> {
    match tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await
    {
        Ok(file) => {
            ::metrics::gauge!("nln_artifact_capture_available", 1.0, "label" => label);
            Some(BufWriter::new(file))
        }
        Err(err) => {
            warn!(
                path = %path.display(),
                error = %err,
                "Seer: NLN artifact file could not be opened"
            );
            ::metrics::gauge!("nln_artifact_capture_available", 0.0, "label" => label);
            ::metrics::counter!(
                "nln_artifact_capture_degraded",
                1,
                "label" => label,
                "reason" => "open_failure"
            );
            None
        }
    }
}

fn spawn_nln_artifact_writer(config: NlnArtifactCaptureConfig) -> Option<NlnArtifactWriter> {
    if !config.enabled {
        return None;
    }

    let transfer_sample_rate = config.transfer_sample_rate;
    let (tx, mut rx) = mpsc::channel(config.queue_capacity);
    tokio::spawn(async move {
        if let Err(err) = tokio::fs::create_dir_all(&config.capture_dir).await {
            warn!(
                dir = %config.capture_dir.display(),
                error = %err,
                "Seer: NLN artifact capture directory unavailable; artifact writes disabled"
            );
            ::metrics::counter!(
                "nln_artifact_capture_degraded",
                1,
                "label" => "capture_dir",
                "reason" => "directory_unavailable"
            );
            return;
        }

        let mut create_raw = open_nln_artifact_file(
            config.capture_dir.join("pumpfun_create_raw_v1.jsonl"),
            "pumpfun_create_raw_v1",
        )
        .await;
        let mut trade_raw = open_nln_artifact_file(
            config.capture_dir.join("pumpfun_trade_raw_v1.jsonl"),
            "pumpfun_trade_raw_v1",
        )
        .await;
        let mut buy_raw = open_nln_artifact_file(
            config.capture_dir.join("nln_pumpfun_buy_raw_v1.jsonl"),
            "nln_pumpfun_buy_raw_v1",
        )
        .await;
        let mut buy_exact_sol_in_raw = open_nln_artifact_file(
            config
                .capture_dir
                .join("nln_pumpfun_buy_exact_sol_in_raw_v1.jsonl"),
            "nln_pumpfun_buy_exact_sol_in_raw_v1",
        )
        .await;
        let mut transfers_raw = open_nln_artifact_file(
            config.capture_dir.join("system_transfers_raw_v1.jsonl"),
            "system_transfers_raw_v1",
        )
        .await;
        let mut normalization_errors = open_nln_artifact_file(
            config.capture_dir.join("nln_normalization_errors_v1.jsonl"),
            "nln_normalization_errors_v1",
        )
        .await;
        let mut candidate_birth = open_nln_artifact_file(
            config.capture_dir.join("nln_candidate_birth_v1.jsonl"),
            "nln_candidate_birth_v1",
        )
        .await;
        let mut route_manifest_candidates = open_nln_artifact_file(
            config
                .capture_dir
                .join("route_manifest_evidence_candidates_v1.jsonl"),
            "route_manifest_evidence_candidates_v1",
        )
        .await;
        let mut funding_events = open_nln_artifact_file(
            config.capture_dir.join("funding_events_v1.jsonl"),
            "funding_events_v1",
        )
        .await;
        let mut flush = tokio::time::interval(config.flush_interval);

        info!(
            dir = %config.capture_dir.display(),
            queue_capacity = config.queue_capacity,
            flush_interval_ms = config.flush_interval.as_millis(),
            transfer_sample_rate = config.transfer_sample_rate,
            "Seer: NLN PR8 artifact capture writer started"
        );

        loop {
            tokio::select! {
                maybe_record = rx.recv() => {
                    let Some(record) = maybe_record else {
                        break;
                    };
                    match record {
                        NlnArtifactRecord::PumpFunCreateRaw(row) => {
                            write_nln_artifact_line(&mut create_raw, &row, "pumpfun_create_raw_v1").await;
                        }
                        NlnArtifactRecord::PumpFunTradeRaw(row) => {
                            write_nln_artifact_line(&mut trade_raw, &row, "pumpfun_trade_raw_v1").await;
                        }
                        NlnArtifactRecord::PumpFunBuyRaw(row) => {
                            write_nln_artifact_line(&mut buy_raw, &row, "nln_pumpfun_buy_raw_v1").await;
                        }
                        NlnArtifactRecord::PumpFunBuyExactSolInRaw(row) => {
                            write_nln_artifact_line(&mut buy_exact_sol_in_raw, &row, "nln_pumpfun_buy_exact_sol_in_raw_v1").await;
                        }
                        NlnArtifactRecord::SystemTransfersRaw(row) => {
                            write_nln_artifact_line(&mut transfers_raw, &row, "system_transfers_raw_v1").await;
                        }
                        NlnArtifactRecord::NormalizationError(row) => {
                            write_nln_artifact_line(&mut normalization_errors, &row, "nln_normalization_errors_v1").await;
                        }
                        NlnArtifactRecord::CandidateBirth(row) => {
                            write_nln_artifact_line(&mut candidate_birth, &row, "nln_candidate_birth_v1").await;
                        }
                        NlnArtifactRecord::RouteManifestEvidenceCandidate(row) => {
                            write_nln_artifact_line(&mut route_manifest_candidates, &row, "route_manifest_evidence_candidates_v1").await;
                        }
                        NlnArtifactRecord::FundingEvent(row) => {
                            write_nln_artifact_line(&mut funding_events, &row, "funding_events_v1").await;
                        }
                    }
                }
                _ = flush.tick() => {
                    flush_nln_artifact_writer(&mut create_raw, "pumpfun_create_raw_v1").await;
                    flush_nln_artifact_writer(&mut trade_raw, "pumpfun_trade_raw_v1").await;
                    flush_nln_artifact_writer(&mut buy_raw, "nln_pumpfun_buy_raw_v1").await;
                    flush_nln_artifact_writer(&mut buy_exact_sol_in_raw, "nln_pumpfun_buy_exact_sol_in_raw_v1").await;
                    flush_nln_artifact_writer(&mut transfers_raw, "system_transfers_raw_v1").await;
                    flush_nln_artifact_writer(&mut normalization_errors, "nln_normalization_errors_v1").await;
                    flush_nln_artifact_writer(&mut candidate_birth, "nln_candidate_birth_v1").await;
                    flush_nln_artifact_writer(&mut route_manifest_candidates, "route_manifest_evidence_candidates_v1").await;
                    flush_nln_artifact_writer(&mut funding_events, "funding_events_v1").await;
                }
            }
        }

        flush_nln_artifact_writer(&mut create_raw, "pumpfun_create_raw_v1").await;
        flush_nln_artifact_writer(&mut trade_raw, "pumpfun_trade_raw_v1").await;
        flush_nln_artifact_writer(&mut buy_raw, "nln_pumpfun_buy_raw_v1").await;
        flush_nln_artifact_writer(
            &mut buy_exact_sol_in_raw,
            "nln_pumpfun_buy_exact_sol_in_raw_v1",
        )
        .await;
        flush_nln_artifact_writer(&mut transfers_raw, "system_transfers_raw_v1").await;
        flush_nln_artifact_writer(&mut normalization_errors, "nln_normalization_errors_v1").await;
        flush_nln_artifact_writer(&mut candidate_birth, "nln_candidate_birth_v1").await;
        flush_nln_artifact_writer(
            &mut route_manifest_candidates,
            "route_manifest_evidence_candidates_v1",
        )
        .await;
        flush_nln_artifact_writer(&mut funding_events, "funding_events_v1").await;
        info!("Seer: NLN PR8 artifact capture writer stopped");
    });

    Some(NlnArtifactWriter {
        tx,
        transfer_sample_rate,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NlnProgramStreamCaptureTopic {
    PumpFunCreate,
    PumpFunTrade,
    PumpFunBuy,
    PumpFunBuyExactSolIn,
    SystemTransfers,
}

impl NlnProgramStreamCaptureTopic {
    const fn label(self) -> &'static str {
        match self {
            Self::PumpFunCreate => "pumpfun_create",
            Self::PumpFunTrade => "pumpfun_trade",
            Self::PumpFunBuy => "pumpfun_buy",
            Self::PumpFunBuyExactSolIn => "pumpfun_buy_exact_sol_in",
            Self::SystemTransfers => "system_transfers",
        }
    }

    const fn is_route_evidence(self) -> bool {
        matches!(self, Self::PumpFunBuy | Self::PumpFunBuyExactSolIn)
    }

    const fn is_enhanced_legacy(self) -> bool {
        !self.is_route_evidence()
    }
}

#[derive(Debug, Clone)]
struct NlnProgramStreamSubscription {
    topic: String,
    topic_kind: NlnProgramStreamCaptureTopic,
    required_for_fsc: bool,
    priority: u8,
}

#[derive(Debug, Clone)]
struct NlnProgramStreamsSelection {
    requested_topic_count: usize,
    allowed_stream_count: usize,
    subscriptions: Vec<NlnProgramStreamSubscription>,
    dropped_optional_topics: Vec<String>,
    required_topics_exceed_limit: bool,
    quota_policy_violation: bool,
    fail_reasons: Vec<String>,
}

fn select_nln_program_stream_subscriptions(
    config: &ProgramStreamsConfig,
) -> NlnProgramStreamsSelection {
    let enabled_topics: HashSet<String> = config
        .enabled_topics
        .iter()
        .map(|topic| topic.trim())
        .filter(|topic| !topic.is_empty())
        .map(str::to_owned)
        .collect();
    let optional_topics: HashSet<String> = config
        .optional_topics
        .iter()
        .chain(config.disabled_optional_topics.iter())
        .map(|topic| topic.trim())
        .filter(|topic| !topic.is_empty())
        .map(str::to_owned)
        .collect();
    let disabled_optional_topics: HashSet<String> = config
        .disabled_optional_topics
        .iter()
        .map(|topic| topic.trim())
        .filter(|topic| !topic.is_empty())
        .map(str::to_owned)
        .collect();
    let disabled_streams: HashSet<String> = config
        .disabled_streams
        .iter()
        .map(|topic| topic.trim())
        .filter(|topic| !topic.is_empty())
        .map(str::to_owned)
        .collect();

    let known_candidate_topics: HashSet<String> = [
        config.system_transfers_topic.trim(),
        config.pumpfun_trade_topic.trim(),
        config.pumpfun_create_topic.trim(),
        config.pumpfun_buy_topic.trim(),
        config.pumpfun_buy_exact_sol_in_topic.trim(),
    ]
    .into_iter()
    .filter(|topic| !topic.is_empty())
    .map(str::to_owned)
    .collect();
    let route_evidence_topics: HashSet<String> = [
        config.pumpfun_buy_topic.trim(),
        config.pumpfun_buy_exact_sol_in_topic.trim(),
    ]
    .into_iter()
    .filter(|topic| !topic.is_empty())
    .map(str::to_owned)
    .collect();
    let legacy_enhanced_topics: HashSet<String> = [
        config.pumpfun_trade_topic.trim(),
        config.system_transfers_topic.trim(),
    ]
    .into_iter()
    .filter(|topic| !topic.is_empty())
    .map(str::to_owned)
    .collect();
    let mut fail_reasons = Vec::new();
    if config.quota_policy == SeerProgramStreamsQuotaPolicy::FailFast {
        if enabled_topics.len() > config.max_streams {
            fail_reasons.push("enabled_topics_exceed_max_streams".to_string());
        }
        let enabled_optional_topics: Vec<String> = enabled_topics
            .iter()
            .filter(|topic| optional_topics.contains(*topic))
            .cloned()
            .collect();
        if !enabled_optional_topics.is_empty() {
            fail_reasons.push(format!(
                "optional_topics_enabled:{}",
                enabled_optional_topics.join(",")
            ));
        }
        let unknown_enabled_topics: Vec<String> = enabled_topics
            .iter()
            .filter(|topic| !known_candidate_topics.contains(*topic))
            .cloned()
            .collect();
        if !unknown_enabled_topics.is_empty() {
            fail_reasons.push(format!(
                "unknown_enabled_topics:{}",
                unknown_enabled_topics.join(",")
            ));
        }
        let enabled_disabled_streams: Vec<String> = enabled_topics
            .iter()
            .filter(|topic| disabled_streams.contains(*topic))
            .cloned()
            .collect();
        if !enabled_disabled_streams.is_empty() {
            fail_reasons.push(format!(
                "disabled_streams_enabled:{}",
                enabled_disabled_streams.join(",")
            ));
        }
        let route_evidence_enabled = enabled_topics
            .iter()
            .any(|topic| route_evidence_topics.contains(topic));
        if route_evidence_enabled {
            let active_known_topic_count = enabled_topics
                .iter()
                .filter(|topic| known_candidate_topics.contains(*topic))
                .count();
            if active_known_topic_count > 2 {
                fail_reasons.push("route_evidence_active_parsed_streams_exceed_two".to_string());
            }
            let mixed_enhanced_topics: Vec<String> = enabled_topics
                .iter()
                .filter(|topic| legacy_enhanced_topics.contains(*topic))
                .cloned()
                .collect();
            if !mixed_enhanced_topics.is_empty() {
                fail_reasons.push(format!(
                    "route_evidence_profile_forbids_enhanced_streams:{}",
                    mixed_enhanced_topics.join(",")
                ));
            }
        }
    }

    let mut seen_topics = HashSet::new();
    let mut candidates = Vec::new();
    let mut configured_candidates = vec![
        (
            config.system_transfers_topic.trim(),
            NlnProgramStreamCaptureTopic::SystemTransfers,
            true,
            0,
        ),
        (
            config.pumpfun_trade_topic.trim(),
            NlnProgramStreamCaptureTopic::PumpFunTrade,
            true,
            1,
        ),
        (
            config.pumpfun_create_topic.trim(),
            NlnProgramStreamCaptureTopic::PumpFunCreate,
            false,
            2,
        ),
    ];
    if !enabled_topics.is_empty() {
        configured_candidates.extend([
            (
                config.pumpfun_buy_topic.trim(),
                NlnProgramStreamCaptureTopic::PumpFunBuy,
                false,
                0,
            ),
            (
                config.pumpfun_buy_exact_sol_in_topic.trim(),
                NlnProgramStreamCaptureTopic::PumpFunBuyExactSolIn,
                false,
                1,
            ),
        ]);
    }

    for (topic, topic_kind, required_for_fsc, priority) in configured_candidates {
        if topic.is_empty() {
            continue;
        }
        if !enabled_topics.is_empty() && !enabled_topics.contains(topic) {
            continue;
        }
        if !seen_topics.insert(topic.to_owned()) {
            continue;
        }
        candidates.push(NlnProgramStreamSubscription {
            topic: topic.to_owned(),
            topic_kind,
            required_for_fsc,
            priority,
        });
    }

    let requested_topic_count = candidates.len();
    let mut dropped_optional_topics = Vec::new();
    let mut selected = Vec::new();
    for candidate in candidates {
        if !candidate.required_for_fsc && disabled_optional_topics.contains(&candidate.topic) {
            dropped_optional_topics.push(candidate.topic);
            continue;
        }
        selected.push(candidate);
    }

    selected.sort_by_key(|candidate| candidate.priority);
    let allowed_stream_count = config.max_streams;
    let required_count = selected
        .iter()
        .filter(|candidate| candidate.required_for_fsc)
        .count();
    let required_topics_exceed_limit = required_count > allowed_stream_count;
    let selected_topics_exceed_limit = selected.len() > allowed_stream_count;
    if config.quota_policy == SeerProgramStreamsQuotaPolicy::FailFast
        && selected_topics_exceed_limit
    {
        fail_reasons.push("selected_topics_exceed_max_streams".to_string());
    }
    let quota_policy_violation = !fail_reasons.is_empty();

    if required_topics_exceed_limit || quota_policy_violation {
        NlnProgramStreamsSelection {
            requested_topic_count,
            allowed_stream_count,
            subscriptions: Vec::new(),
            dropped_optional_topics,
            required_topics_exceed_limit,
            quota_policy_violation,
            fail_reasons,
        }
    } else {
        while selected.len() > allowed_stream_count {
            let Some(drop_index) = selected
                .iter()
                .rposition(|candidate| !candidate.required_for_fsc)
            else {
                break;
            };
            dropped_optional_topics.push(selected.remove(drop_index).topic);
        }
        NlnProgramStreamsSelection {
            requested_topic_count,
            allowed_stream_count,
            subscriptions: selected,
            dropped_optional_topics,
            required_topics_exceed_limit,
            quota_policy_violation,
            fail_reasons,
        }
    }
}

async fn write_nln_program_stream_run_manifest(
    capture_dir: &std::path::Path,
    config: &ProgramStreamsConfig,
    selection: &NlnProgramStreamsSelection,
) {
    if let Err(err) = tokio::fs::create_dir_all(capture_dir).await {
        warn!(
            dir = %capture_dir.display(),
            error = %err,
            "Seer: NLN Program Streams manifest directory unavailable"
        );
        return;
    }

    let active_program_streams = selection
        .subscriptions
        .iter()
        .filter(|subscription| subscription.topic_kind.is_route_evidence())
        .count();
    let active_enhanced_streams = selection
        .subscriptions
        .iter()
        .filter(|subscription| subscription.topic_kind.is_enhanced_legacy())
        .count();
    let active_topics: Vec<Value> = selection
        .subscriptions
        .iter()
        .map(|subscription| {
            json!({
                "topic": subscription.topic,
                "topic_kind": subscription.topic_kind.label(),
                "route_evidence_stream": subscription.topic_kind.is_route_evidence(),
            })
        })
        .collect();
    let expected_missing_artifacts = if active_enhanced_streams == 0 {
        json!([
            {
                "path": "pumpfun_trade_raw_v1.jsonl",
                "status": "intentionally_disabled"
            },
            {
                "path": "system_transfers_raw_v1.jsonl",
                "status": "intentionally_disabled"
            },
            {
                "path": "funding_events_v1.jsonl",
                "status": "intentionally_disabled"
            }
        ])
    } else {
        json!([])
    };
    let manifest = json!({
        "schema_version": 1,
        "artifact": "nln_program_stream_run_manifest_v1",
        "source_mode": "program_stream_route_evidence_capture_only",
        "active_program_streams": active_program_streams,
        "active_enhanced_streams": active_enhanced_streams,
        "active_raw_grpc_streams": 1,
        "active_topics": active_topics,
        "disabled_streams": config.disabled_streams.clone(),
        "disabled_optional_topics": config.disabled_optional_topics.clone(),
        "expected_missing_artifacts": expected_missing_artifacts,
        "stream_limit_policy": {
            "max_active_parsed_streams": 2,
            "active_program_streams_plus_enhanced_streams": active_program_streams + active_enhanced_streams,
            "status": if active_program_streams + active_enhanced_streams <= 2 { "PASS" } else { "FAIL" }
        },
        "canonical_pool_birth_source": "yellowstone_geyser_grpc",
        "canonical_account_state_source": "yellowstone_geyser_grpc",
        "nln_program_streams_are_ssot": false,
        "program_stream_candidate_can_unlock_execution": false,
    });
    let path = capture_dir.join("nln_program_stream_run_manifest_v1.json");
    match serde_json::to_vec_pretty(&manifest) {
        Ok(bytes) => {
            if let Err(err) = tokio::fs::write(&path, bytes).await {
                warn!(
                    path = %path.display(),
                    error = %err,
                    "Seer: NLN Program Streams manifest write failed"
                );
            }
        }
        Err(err) => {
            warn!(
                error = %err,
                "Seer: NLN Program Streams manifest serialization failed"
            );
        }
    }
}

async fn capture_raw_nln_message(
    writer: &NlnArtifactWriter,
    topic_kind: NlnProgramStreamCaptureTopic,
    message: &NlnProgramStreamMessage,
    raw_row: &Value,
    force: bool,
) -> bool {
    match topic_kind {
        NlnProgramStreamCaptureTopic::PumpFunCreate => {
            writer
                .send_lossless(
                    NlnArtifactRecord::PumpFunCreateRaw(raw_row.clone()),
                    "pumpfun_create_raw_v1",
                )
                .await
        }
        NlnProgramStreamCaptureTopic::PumpFunTrade => {
            writer
                .send_lossless(
                    NlnArtifactRecord::PumpFunTradeRaw(raw_row.clone()),
                    "pumpfun_trade_raw_v1",
                )
                .await
        }
        NlnProgramStreamCaptureTopic::PumpFunBuy => {
            writer
                .send_lossless(
                    NlnArtifactRecord::PumpFunBuyRaw(raw_row.clone()),
                    "nln_pumpfun_buy_raw_v1",
                )
                .await
        }
        NlnProgramStreamCaptureTopic::PumpFunBuyExactSolIn => {
            writer
                .send_lossless(
                    NlnArtifactRecord::PumpFunBuyExactSolInRaw(raw_row.clone()),
                    "nln_pumpfun_buy_exact_sol_in_raw_v1",
                )
                .await
        }
        NlnProgramStreamCaptureTopic::SystemTransfers => {
            if force || writer.should_capture_raw_transfer_message(message) {
                let mut row = raw_row.clone();
                add_nln_artifact_sampling(&mut row, writer.transfer_sample_rate);
                writer
                    .send_lossless(
                        NlnArtifactRecord::SystemTransfersRaw(row),
                        "system_transfers_raw_v1",
                    )
                    .await
            } else {
                false
            }
        }
    }
}

async fn run_nln_program_streams_topic_capture(
    client: NlnProgramStreamsClient,
    config: ProgramStreamsConfig,
    topic: String,
    topic_kind: NlnProgramStreamCaptureTopic,
    event_bus_tx: Option<EventBusSender>,
    health: Option<Arc<RuntimeHealth>>,
    artifact_writer: Option<NlnArtifactWriter>,
    trade_resolver: Option<Arc<Mutex<NlnTradePoolIdentityResolver>>>,
    authoritative_funding_stream_tx: Option<watch::Sender<bool>>,
) {
    let options = NlnSubscribeLoopOptions {
        max_reconnects: None,
        stall_timeout: Some(Duration::from_secs(30)),
        ..NlnSubscribeLoopOptions::default()
    };
    let stats = client.stats();
    let mut last_reconnect_total = stats.snapshot().reconnects;
    let mut lane_health = FundingLaneRuntimeHealth::default();
    let mut stream = client.subscribe_json_with_reconnect(topic.clone(), options);
    let mut sequence_number = 0u64;
    let mut native_transfer_count = 0u64;
    let mut non_native_transfer_count = 0u64;
    let mut create_count = 0u64;
    let mut trade_count = 0u64;
    let mut decode_error_count = 0u64;
    let mut first_message_logged = false;
    let mut transfer_dedupe = BoundedTtlSet::new(
        Duration::from_millis(config.transfer_dedupe_ttl_ms.max(1)),
        config.transfer_dedupe_max_entries,
    );
    ::metrics::gauge!("nln_stream_connected", 1.0, "topic" => topic.clone());
    if matches!(topic_kind, NlnProgramStreamCaptureTopic::SystemTransfers) {
        if let Some(tx) = authoritative_funding_stream_tx.as_ref() {
            let _ = tx.send(false);
        }
    }

    while let Some(next) = stream.next().await {
        let message = match next {
            Ok(message) => message,
            Err(err) => {
                decode_error_count = decode_error_count.saturating_add(1);
                warn!(
                    topic = %topic,
                    kind = topic_kind.label(),
                    decode_error_count,
                    error = %err,
                    "Seer: NLN Program Streams message decode failed"
                );
                ::metrics::counter!("nln_decode_errors", 1, "topic" => topic.clone());
                ::metrics::increment_counter!("nln_decode_errors_total", "topic" => topic.clone());
                continue;
            }
        };
        if !first_message_logged {
            first_message_logged = true;
            info!(
                topic = %topic,
                kind = topic_kind.label(),
                partition = message.partition,
                offset = %message.offset_raw,
                has_artifact_writer = artifact_writer.is_some(),
                "Seer: NLN Program Streams first message received"
            );
        }
        let stats_snapshot = stats.snapshot();
        if stats_snapshot.reconnects > last_reconnect_total {
            let delta = stats_snapshot
                .reconnects
                .saturating_sub(last_reconnect_total);
            last_reconnect_total = stats_snapshot.reconnects;
            lane_health.stream_epoch = lane_health.stream_epoch.saturating_add(delta);
            lane_health.gap_suspected = true;
            lane_health.last_reconnect_ts_ms = Some(unix_now_ms());
            ::metrics::counter!("nln_reconnect_count", delta, "topic" => topic.clone());
            ::metrics::counter!("nln_reconnect_count_total", delta, "topic" => topic.clone());
            if matches!(topic_kind, NlnProgramStreamCaptureTopic::SystemTransfers) {
                if let Some(tx) = authoritative_funding_stream_tx.as_ref() {
                    let _ = tx.send(false);
                }
            }
        }

        let raw_row = artifact_writer
            .as_ref()
            .map(|_| nln_artifact_raw_row(&message, &config, "nln_program_stream_raw_v1"));
        let mut raw_captured = false;
        if let (Some(writer), Some(row)) = (artifact_writer.as_ref(), raw_row.as_ref()) {
            raw_captured = capture_raw_nln_message(writer, topic_kind, &message, row, false).await;
        }

        if matches!(
            topic_kind,
            NlnProgramStreamCaptureTopic::PumpFunBuy
                | NlnProgramStreamCaptureTopic::PumpFunBuyExactSolIn
        ) {
            if let (Some(writer), Some(candidate)) = (
                artifact_writer.as_ref(),
                nln_route_manifest_evidence_candidate_row(&message, topic_kind, &config),
            ) {
                writer
                    .send_lossless(
                        NlnArtifactRecord::RouteManifestEvidenceCandidate(candidate),
                        "route_manifest_evidence_candidates_v1",
                    )
                    .await;
            }
            ::metrics::counter!(
                "nln_program_route_evidence_captured_total",
                1,
                "topic_kind" => topic_kind.label()
            );
            match topic_kind {
                NlnProgramStreamCaptureTopic::PumpFunBuy => {
                    ::metrics::counter!("NLN_PROGRAM_BUY_CAPTURED", 1);
                }
                NlnProgramStreamCaptureTopic::PumpFunBuyExactSolIn => {
                    ::metrics::counter!("NLN_PROGRAM_BUY_EXACT_SOL_IN_CAPTURED", 1);
                }
                _ => {}
            }
            continue;
        }

        let event = match normalize_nln_event(&message, &config) {
            Ok(event) => event,
            Err(err) => {
                decode_error_count = decode_error_count.saturating_add(1);
                if let Some(writer) = artifact_writer.as_ref() {
                    if let Some(row) = raw_row.as_ref() {
                        if !raw_captured {
                            capture_raw_nln_message(writer, topic_kind, &message, row, true).await;
                        }
                    }
                    writer
                        .send_lossless(
                            NlnArtifactRecord::NormalizationError(nln_normalization_error_row(
                                &message, topic_kind, &err,
                            )),
                            "nln_normalization_errors_v1",
                        )
                        .await;
                }
                warn!(
                    topic = %message.topic,
                    kind = topic_kind.label(),
                    partition = message.partition,
                    offset = %message.offset_raw,
                    decode_error_count,
                    error = %err,
                    "Seer: NLN Program Streams event normalization failed"
                );
                ::metrics::counter!("nln_decode_errors", 1, "topic" => topic.clone());
                ::metrics::increment_counter!("nln_decode_errors_total", "topic" => topic.clone());
                continue;
            }
        };

        match (topic_kind, event) {
            (NlnProgramStreamCaptureTopic::PumpFunCreate, NlnEvent::PumpFunCreate(create)) => {
                create_count = create_count.saturating_add(1);
                if let Some(writer) = artifact_writer.as_ref() {
                    writer
                        .send_lossless(
                            NlnArtifactRecord::CandidateBirth(nln_candidate_birth_artifact_row(
                                &create,
                            )),
                            "nln_candidate_birth_v1",
                        )
                        .await;
                }
                if create_count == 1 || create_count % 1_000 == 0 {
                    info!(
                        topic = %topic,
                        create_count,
                        "Seer: NLN Program Streams pumpfun.create events captured"
                    );
                }
            }
            (NlnProgramStreamCaptureTopic::PumpFunTrade, NlnEvent::PumpFunTrade(trade)) => {
                trade_count = trade_count.saturating_add(1);
                ::metrics::counter!("nln_trade_rows", 1);
                increment_counter!("nln_trade_rows_total");
                ::metrics::counter!("nln_events_received", 1, "topic" => topic.clone());
                ::metrics::increment_counter!("nln_events_received_total", "topic" => topic.clone());
                if trade.side.is_buy() {
                    ::metrics::counter!("nln_trade_buy_rows", 1);
                    increment_counter!("nln_trade_buy_rows_total");
                }
                if let (Some(tx), Some(resolver)) = (event_bus_tx.as_ref(), trade_resolver.as_ref())
                {
                    let now = Instant::now();
                    let resolve = match resolver.lock() {
                        Ok(mut guard) => Some(guard.resolve_or_buffer(trade.clone(), now)),
                        Err(err) => {
                            warn!(
                                topic = %topic,
                                error = %err,
                                "Seer: NLN trade pool identity resolver lock poisoned"
                            );
                            None
                        }
                    };
                    if let Some(resolve) = resolve {
                        if resolve.expired_count > 0 {
                            ::metrics::counter!(
                                "nln_trade_unresolved_after_ttl",
                                resolve.expired_count as u64
                            );
                        }
                        if resolve.evicted_per_mint > 0 {
                            ::metrics::counter!(
                                "nln_trade_resolver_evicted",
                                resolve.evicted_per_mint as u64,
                                "reason" => "per_mint_cap"
                            );
                            ::metrics::counter!(
                                "nln_trade_resolver_evicted_total",
                                resolve.evicted_per_mint as u64,
                                "reason" => "per_mint_cap"
                            );
                        }
                        if resolve.evicted_global > 0 {
                            ::metrics::counter!(
                                "nln_trade_resolver_evicted",
                                resolve.evicted_global as u64,
                                "reason" => "global_cap"
                            );
                            ::metrics::counter!(
                                "nln_trade_resolver_evicted_total",
                                resolve.evicted_global as u64,
                                "reason" => "global_cap"
                            );
                        }
                        match resolve.decision {
                            NlnTradeResolveDecision::ForwardNow { pool_amm_id } => {
                                let trade_event = trade.to_trade_event(pool_amm_id);
                                emit_pool_transaction_to_event_bus(
                                    tx,
                                    &trade_event,
                                    health.as_ref(),
                                    false,
                                );
                                ::metrics::counter!("nln_trade_resolved_to_pool", 1);
                                ::metrics::counter!("nln_trade_forwarded_pool_transaction", 1);
                                increment_counter!("nln_trade_resolved_to_pool_total");
                                increment_counter!("nln_trade_forwarded_pool_transaction_total");
                                info!(
                                    topic = %topic,
                                    mint = %trade.mint,
                                    pool = %pool_amm_id,
                                    signature = %trade.signature,
                                    slot = trade.slot,
                                    side = ?trade.side,
                                    resolver_action = "forward_now",
                                    "Seer: NLN pumpfun.trade forwarded to PoolTransaction"
                                );
                            }
                            NlnTradeResolveDecision::Buffered => {
                                ::metrics::counter!("nln_trade_buffered", 1);
                                increment_counter!("nln_trade_buffered_total");
                            }
                            NlnTradeResolveDecision::Duplicate => {
                                ::metrics::counter!("nln_trade_deduped", 1);
                                increment_counter!("nln_trade_deduped_total");
                            }
                            NlnTradeResolveDecision::IdentityCollision => {
                                ::metrics::counter!("nln_trade_pool_identity_collision", 1);
                                increment_counter!("nln_trade_pool_identity_collision_total");
                                warn!(
                                    topic = %topic,
                                    mint = %trade.mint,
                                    signature = %trade.signature,
                                    slot = trade.slot,
                                    resolver_action = "identity_collision",
                                    "Seer: NLN pumpfun.trade not forwarded due to mint/pool identity collision"
                                );
                            }
                        }
                    }
                }
                if trade_count == 1 || trade_count % 1_000 == 0 {
                    info!(
                        topic = %topic,
                        trade_count,
                        "Seer: NLN Program Streams pumpfun.trade events captured"
                    );
                }
            }
            (NlnProgramStreamCaptureTopic::SystemTransfers, NlnEvent::Transfer(transfer)) => {
                ::metrics::counter!("nln_events_received", 1, "topic" => topic.clone());
                ::metrics::increment_counter!("nln_events_received_total", "topic" => topic.clone());
                if !transfer_dedupe.insert_new(
                    NlnTransferDedupeKey::from_transfer(&transfer),
                    Instant::now(),
                ) {
                    ::metrics::counter!("nln_transfer_deduped", 1);
                    increment_counter!("nln_transfer_deduped_total");
                    continue;
                }
                let Some(transfer_event) = transfer
                    .to_native_sol_funding_transfer_event(NlnFundingTransferCoverage::CaptureOnly)
                else {
                    non_native_transfer_count = non_native_transfer_count.saturating_add(1);
                    continue;
                };

                native_transfer_count = native_transfer_count.saturating_add(1);
                sequence_number = sequence_number.saturating_add(1);
                if let Some(tx) = authoritative_funding_stream_tx.as_ref() {
                    // This marks live native-SOL lane evidence, not FundingSourceIndex store acceptance.
                    let _ = tx.send(true);
                }

                if let Some(writer) = artifact_writer
                    .as_ref()
                    .filter(|writer| writer.should_capture_transfer(&transfer))
                {
                    let mut funding_row = nln_funding_event_artifact_row(&transfer);
                    add_nln_artifact_sampling(&mut funding_row, writer.transfer_sample_rate);
                    writer
                        .send_lossless(
                            NlnArtifactRecord::FundingEvent(funding_row),
                            "funding_events_v1",
                        )
                        .await;
                }

                if let Some(ref tx) = event_bus_tx {
                    let detected = seer::ipc::DetectedFundingTransferEvent {
                        transfer: transfer_event,
                        lane_health,
                        detected_at: std::time::SystemTime::now(),
                        sequence_number,
                        priority: seer::ipc::EventPriority::High,
                    };
                    emit_funding_transfer_to_event_bus(tx, &detected, health.as_ref());
                }

                if native_transfer_count == 1 || native_transfer_count % 1_000 == 0 {
                    info!(
                        topic = %topic,
                        native_transfer_count,
                        non_native_transfer_count,
                        sequence_number,
                        "Seer: NLN Program Streams native SOL funding transfers captured"
                    );
                }
            }
            (_, other) => {
                debug!(
                    topic = %topic,
                    kind = topic_kind.label(),
                    event = ?other,
                    "Seer: NLN Program Streams capture ignored unexpected event kind"
                );
            }
        }
    }

    warn!(
        topic = %topic,
        kind = topic_kind.label(),
        native_transfer_count,
        non_native_transfer_count,
        create_count,
        trade_count,
        decode_error_count,
        "Seer: NLN Program Streams topic capture lane exited"
    );
    ::metrics::gauge!("nln_stream_connected", 0.0, "topic" => topic.clone());
    if matches!(topic_kind, NlnProgramStreamCaptureTopic::SystemTransfers) {
        if let Some(tx) = authoritative_funding_stream_tx.as_ref() {
            let _ = tx.send(false);
        }
    }
}

fn map_launcher_commitment(commitment: SeerCommitment) -> seer::config::CommitmentLevel {
    match commitment {
        SeerCommitment::Processed => seer::config::CommitmentLevel::Mempool,
        SeerCommitment::Confirmed => seer::config::CommitmentLevel::Confirmed,
        SeerCommitment::Finalized => seer::config::CommitmentLevel::Finalized,
    }
}

fn sanitize_detected_creator(creator: Pubkey) -> String {
    let creator_str = creator.to_string();
    if !creator.is_on_curve()
        || creator == Pubkey::default()
        || creator_str == SYSTEM_PROGRAM_ID
        || creator_str == TOKEN_PROGRAM_ID
        || creator_str == TOKEN_2022_PROGRAM_ID
        || creator_str == COMPUTE_BUDGET_PROGRAM_ID
        || creator_str == ASSOCIATED_TOKEN_PROGRAM_ID
        || creator_str.starts_with("Sysvar")
    {
        "unknown".to_string()
    } else {
        creator_str
    }
}

fn trade_has_forwardable_identity(trade: &seer::types::TradeEvent) -> bool {
    trade.pool_amm_id != Pubkey::default() && trade.mint != Pubkey::default()
}

fn route_compatible_trade_bcv2_context(
    trade: &seer::types::TradeEvent,
) -> Option<SessionBcv2Context> {
    if !trade.success || trade.pool_amm_id == Pubkey::default() || trade.mint == Pubkey::default() {
        return None;
    }
    let bcv2 = trade
        .bonding_curve_v2
        .filter(|value| *value != Pubkey::default())?;
    let provenance = trade.bonding_curve_v2_provenance.as_ref()?;
    if provenance.provenance_status.as_deref() != Some("route_compatible") {
        return None;
    }

    Some(SessionBcv2Context {
        account_pubkey: bcv2,
        base_mint: Some(trade.mint),
        pool_id: Some(trade.pool_amm_id),
        canonical_bonding_curve: None,
    })
}

#[derive(Clone)]
struct BufferedSessionTrade {
    trade: seer::types::TradeEvent,
    buffered_at: Instant,
    dedupe_key: BufferedSessionTradeKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct BufferedSessionTradeKey {
    pool: Pubkey,
    signature: String,
    event_ordinal: Option<u32>,
}

impl BufferedSessionTradeKey {
    fn from_trade(trade: &seer::types::TradeEvent) -> Self {
        Self {
            pool: trade.pool_amm_id,
            signature: trade.signature.to_string(),
            event_ordinal: trade.event_ordinal,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct NlnTradeDedupeKey {
    signature: String,
    tx_index: Option<u32>,
    instruction_index: Option<u32>,
    mint: Pubkey,
    user: Pubkey,
    ix_name: Option<String>,
    sol_amount_lamports: u64,
}

impl NlnTradeDedupeKey {
    fn from_trade(trade: &NlnPumpFunTradeEvent) -> Self {
        Self {
            signature: trade.signature.to_string(),
            tx_index: trade.tx_index,
            instruction_index: trade.meta.instruction_index,
            mint: trade.mint,
            user: trade.user,
            ix_name: trade.ix_name.clone(),
            sol_amount_lamports: trade.sol_amount_lamports,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct NlnTransferDedupeKey {
    signature: String,
    instruction_index: Option<u32>,
    from_wallet: Pubkey,
    to_wallet: Pubkey,
    amount_lamports: u64,
    token_address: String,
}

impl NlnTransferDedupeKey {
    fn from_transfer(transfer: &NlnTransferEvent) -> Self {
        Self {
            signature: transfer.signature.clone(),
            instruction_index: transfer.instruction_index,
            from_wallet: transfer.from_wallet,
            to_wallet: transfer.to_wallet,
            amount_lamports: transfer.amount_lamports,
            token_address: transfer.token_address.clone(),
        }
    }
}

#[derive(Debug, Clone)]
struct BoundedTtlSet<K>
where
    K: Clone + Eq + Hash,
{
    entries: HashSet<K>,
    order: VecDeque<(Instant, K)>,
    ttl: Duration,
    cap: usize,
}

impl<K> BoundedTtlSet<K>
where
    K: Clone + Eq + Hash,
{
    fn new(ttl: Duration, cap: usize) -> Self {
        Self {
            entries: HashSet::new(),
            order: VecDeque::new(),
            ttl,
            cap: cap.max(1),
        }
    }

    fn insert_new(&mut self, key: K, now: Instant) -> bool {
        self.prune(now);
        if self.entries.contains(&key) {
            return false;
        }
        while self.entries.len() >= self.cap {
            if let Some((_, oldest)) = self.order.pop_front() {
                self.entries.remove(&oldest);
            } else {
                break;
            }
        }
        self.entries.insert(key.clone());
        self.order.push_back((now, key));
        true
    }

    fn prune(&mut self, now: Instant) -> usize {
        let mut expired = 0usize;
        while let Some((seen_at, key)) = self.order.front().cloned() {
            if now.duration_since(seen_at) <= self.ttl {
                break;
            }
            self.order.pop_front();
            if self.entries.remove(&key) {
                expired += 1;
            }
        }
        expired
    }
}

#[derive(Debug, Clone, Copy)]
struct NlnPoolIdentity {
    pool_amm_id: Pubkey,
    base_mint: Pubkey,
    bonding_curve: Pubkey,
    first_seen_ts_ms: u64,
}

#[derive(Clone)]
struct BufferedNlnTrade {
    trade: NlnPumpFunTradeEvent,
    buffered_at: Instant,
}

#[derive(Debug, Default)]
struct NlnTradeResolverRegisterResult {
    replay_ready: Vec<NlnPumpFunTradeEvent>,
    expired_count: usize,
    collision: bool,
}

#[derive(Debug)]
enum NlnTradeResolveDecision {
    ForwardNow { pool_amm_id: Pubkey },
    Buffered,
    Duplicate,
    IdentityCollision,
}

#[derive(Debug)]
struct NlnTradeResolveResult {
    decision: NlnTradeResolveDecision,
    expired_count: usize,
    evicted_per_mint: usize,
    evicted_global: usize,
}

struct NlnTradePoolIdentityResolver {
    mint_to_pool: HashMap<Pubkey, NlnPoolIdentity>,
    mint_order: VecDeque<Pubkey>,
    collided_mints: HashSet<Pubkey>,
    pending_by_mint: HashMap<Pubkey, VecDeque<BufferedNlnTrade>>,
    pending_total: usize,
    seen_trade_keys: BoundedTtlSet<NlnTradeDedupeKey>,
    ttl: Duration,
    per_mint_cap: usize,
    global_cap: usize,
    identity_cap: usize,
}

impl NlnTradePoolIdentityResolver {
    fn from_program_streams_config(config: &ProgramStreamsConfig) -> Self {
        Self {
            mint_to_pool: HashMap::new(),
            mint_order: VecDeque::new(),
            collided_mints: HashSet::new(),
            pending_by_mint: HashMap::new(),
            pending_total: 0,
            seen_trade_keys: BoundedTtlSet::new(
                Duration::from_millis(config.trade_dedupe_ttl_ms.max(1)),
                config.trade_dedupe_max_entries,
            ),
            ttl: Duration::from_millis(config.trade_resolver_ttl_ms.max(1)),
            per_mint_cap: config.trade_resolver_per_mint_cap.max(1),
            global_cap: config.trade_resolver_global_cap.max(1),
            identity_cap: SESSION_POOL_REGISTRY_FALLBACK_CAP,
        }
    }

    fn register_candidate(
        &mut self,
        candidate: &seer::types::CandidatePool,
        now: Instant,
    ) -> NlnTradeResolverRegisterResult {
        let expired_count = self.prune_expired(now);
        let identity = NlnPoolIdentity {
            pool_amm_id: candidate.pool_amm_id,
            base_mint: candidate.base_mint,
            bonding_curve: candidate.bonding_curve,
            first_seen_ts_ms: candidate.compat_event_ts_ms().unwrap_or_else(unix_now_ms),
        };

        if let Some(existing) = self.mint_to_pool.get(&candidate.base_mint).copied() {
            if existing.pool_amm_id != identity.pool_amm_id
                || existing.bonding_curve != identity.bonding_curve
            {
                self.collided_mints.insert(candidate.base_mint);
                self.mint_to_pool.remove(&candidate.base_mint);
                self.pending_total = self.pending_total.saturating_sub(
                    self.pending_by_mint
                        .remove(&candidate.base_mint)
                        .map_or(0, |queue| queue.len()),
                );
                ::metrics::counter!("nln_trade_pool_identity_collision", 1);
                increment_counter!("nln_trade_pool_identity_collision_total");
                return NlnTradeResolverRegisterResult {
                    replay_ready: Vec::new(),
                    expired_count,
                    collision: true,
                };
            }
        }

        if self.collided_mints.contains(&candidate.base_mint) {
            return NlnTradeResolverRegisterResult {
                replay_ready: Vec::new(),
                expired_count,
                collision: true,
            };
        }

        if !self.mint_to_pool.contains_key(&candidate.base_mint) {
            self.mint_order.push_back(candidate.base_mint);
        }
        self.mint_to_pool.insert(candidate.base_mint, identity);
        while self.mint_to_pool.len() > self.identity_cap {
            if let Some(oldest) = self.mint_order.pop_front() {
                self.mint_to_pool.remove(&oldest);
            } else {
                break;
            }
        }

        let mut replay_ready = Vec::new();
        if let Some(mut queue) = self.pending_by_mint.remove(&candidate.base_mint) {
            while let Some(buffered) = queue.pop_front() {
                self.pending_total = self.pending_total.saturating_sub(1);
                if now.duration_since(buffered.buffered_at) <= self.ttl {
                    replay_ready.push(buffered.trade);
                }
            }
        }
        if !replay_ready.is_empty() {
            ::metrics::counter!("nln_trade_resolved_to_pool", replay_ready.len() as u64);
            ::metrics::counter!(
                "nln_trade_resolved_to_pool_total",
                replay_ready.len() as u64
            );
        }
        NlnTradeResolverRegisterResult {
            replay_ready,
            expired_count,
            collision: false,
        }
    }

    fn resolve_or_buffer(
        &mut self,
        trade: NlnPumpFunTradeEvent,
        now: Instant,
    ) -> NlnTradeResolveResult {
        let expired_count = self.prune_expired(now);
        if !self
            .seen_trade_keys
            .insert_new(NlnTradeDedupeKey::from_trade(&trade), now)
        {
            return NlnTradeResolveResult {
                decision: NlnTradeResolveDecision::Duplicate,
                expired_count,
                evicted_per_mint: 0,
                evicted_global: 0,
            };
        }

        if self.collided_mints.contains(&trade.mint) {
            return NlnTradeResolveResult {
                decision: NlnTradeResolveDecision::IdentityCollision,
                expired_count,
                evicted_per_mint: 0,
                evicted_global: 0,
            };
        }

        if let Some(identity) = self.mint_to_pool.get(&trade.mint).copied() {
            debug_assert_eq!(identity.base_mint, trade.mint);
            let _identity_age_ms = unix_now_ms().saturating_sub(identity.first_seen_ts_ms);
            return NlnTradeResolveResult {
                decision: NlnTradeResolveDecision::ForwardNow {
                    pool_amm_id: identity.pool_amm_id,
                },
                expired_count,
                evicted_per_mint: 0,
                evicted_global: 0,
            };
        }

        let mut evicted_per_mint = 0usize;
        let mut evicted_global = 0usize;
        while self.pending_total >= self.global_cap {
            if self.evict_oldest_pending().is_some() {
                evicted_global += 1;
            } else {
                break;
            }
        }
        let queue = self.pending_by_mint.entry(trade.mint).or_default();
        while queue.len() >= self.per_mint_cap {
            if queue.pop_front().is_some() {
                self.pending_total = self.pending_total.saturating_sub(1);
                evicted_per_mint += 1;
            } else {
                break;
            }
        }
        queue.push_back(BufferedNlnTrade {
            trade,
            buffered_at: now,
        });
        self.pending_total += 1;
        NlnTradeResolveResult {
            decision: NlnTradeResolveDecision::Buffered,
            expired_count,
            evicted_per_mint,
            evicted_global,
        }
    }

    fn prune_expired(&mut self, now: Instant) -> usize {
        self.seen_trade_keys.prune(now);
        let mut expired = 0usize;
        let mut empty_mints = Vec::new();
        for (mint, queue) in self.pending_by_mint.iter_mut() {
            while matches!(queue.front(), Some(front) if now.duration_since(front.buffered_at) > self.ttl)
            {
                if queue.pop_front().is_some() {
                    self.pending_total = self.pending_total.saturating_sub(1);
                    expired += 1;
                }
            }
            if queue.is_empty() {
                empty_mints.push(*mint);
            }
        }
        for mint in empty_mints {
            self.pending_by_mint.remove(&mint);
        }
        if expired > 0 {
            ::metrics::counter!("nln_trade_unresolved_after_ttl", expired as u64);
            ::metrics::counter!("nln_trade_unresolved_after_ttl_total", expired as u64);
        }
        expired
    }

    fn evict_oldest_pending(&mut self) -> Option<NlnPumpFunTradeEvent> {
        let oldest_mint = self
            .pending_by_mint
            .iter()
            .filter_map(|(mint, queue)| queue.front().map(|front| (*mint, front.buffered_at)))
            .min_by_key(|(_, buffered_at)| *buffered_at)
            .map(|(mint, _)| mint)?;
        let queue = self.pending_by_mint.get_mut(&oldest_mint)?;
        let removed = queue.pop_front();
        if queue.is_empty() {
            self.pending_by_mint.remove(&oldest_mint);
        }
        if removed.is_some() {
            self.pending_total = self.pending_total.saturating_sub(1);
        }
        removed.map(|buffered| buffered.trade)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionTradeDecision {
    /// Pool is registered in this session — trade forwarded immediately.
    ForwardNow,
    /// Pool not in this session registry — trade silently discarded.
    SilentDrop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionTradeIngressResult {
    pub decision: SessionTradeDecision,
    pub expired_count: usize,
    pub evicted_per_pool: usize,
    pub evicted_global: usize,
}

pub struct SessionTradeFlushResult {
    pub replay_ready: Vec<seer::types::TradeEvent>,
    pub expired_count: usize,
    pub expired_detected_pools: usize,
    pub evicted_detected_pools: usize,
}

#[derive(Clone)]
struct BufferedSessionAccountUpdate {
    update: seer::ipc::DetectedAccountUpdateEvent,
    buffered_at: Instant,
}

#[derive(Clone)]
struct BufferedSessionExecutionAccountEvidence {
    event: seer::ipc::DetectedExecutionAccountEvidenceEvent,
    buffered_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SessionDetectedKeyRole {
    Pool,
    BaseMint,
    BondingCurve,
    BondingCurveV2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SessionDetectedKey {
    role: SessionDetectedKeyRole,
    pubkey: Pubkey,
}

impl SessionDetectedKey {
    fn new(role: SessionDetectedKeyRole, pubkey: Pubkey) -> Self {
        Self { role, pubkey }
    }

    fn pool(pubkey: Pubkey) -> Self {
        Self::new(SessionDetectedKeyRole::Pool, pubkey)
    }

    fn base_mint(pubkey: Pubkey) -> Self {
        Self::new(SessionDetectedKeyRole::BaseMint, pubkey)
    }

    fn bonding_curve(pubkey: Pubkey) -> Self {
        Self::new(SessionDetectedKeyRole::BondingCurve, pubkey)
    }

    fn bonding_curve_v2(pubkey: Pubkey) -> Self {
        Self::new(SessionDetectedKeyRole::BondingCurveV2, pubkey)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SessionBcv2Context {
    account_pubkey: Pubkey,
    base_mint: Option<Pubkey>,
    pool_id: Option<Pubkey>,
    canonical_bonding_curve: Option<Pubkey>,
}

impl SessionBcv2Context {
    fn from_evidence(evidence: &ghost_core::ExecutionAccountEvidence) -> Option<Self> {
        (evidence.role == ExecutionAccountRole::BondingCurveV2
            && evidence.account_pubkey != Pubkey::default())
        .then_some(Self {
            account_pubkey: evidence.account_pubkey,
            base_mint: evidence
                .base_mint
                .filter(|value| *value != Pubkey::default()),
            pool_id: evidence.pool_id.filter(|value| *value != Pubkey::default()),
            canonical_bonding_curve: evidence
                .canonical_bonding_curve
                .filter(|value| *value != Pubkey::default()),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionExecutionAccountEvidenceDecision {
    ForwardNow,
    BufferedUntilPoolDetected,
    SilentDrop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionExecutionAccountEvidenceIngressResult {
    pub decision: SessionExecutionAccountEvidenceDecision,
    pub expired_count: usize,
    pub expired_detected_keys: usize,
    pub expired_evidence_count: usize,
    pub evicted_per_key: usize,
    pub evicted_global: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionAccountUpdateDecision {
    ForwardNow,
    BufferedUntilPoolDetected,
    SilentDrop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionAccountUpdateIngressResult {
    pub decision: SessionAccountUpdateDecision,
    pub expired_count: usize,
    pub expired_detected_keys: usize,
    pub evicted_per_key: usize,
    pub evicted_global: usize,
}

pub struct SessionAccountUpdateFlushResult {
    pub replay_ready: Vec<seer::ipc::DetectedAccountUpdateEvent>,
    pub replay_ready_evidence: Vec<seer::ipc::DetectedExecutionAccountEvidenceEvent>,
    pub expired_count: usize,
    pub expired_evidence_count: usize,
    pub expired_detected_keys: usize,
}

#[derive(Debug, Clone)]
struct SessionAccountUpdateLivenessResult {
    expired_count: usize,
    expired_evidence_count: usize,
    expired_detected_keys: usize,
    replay_ready_evidence: Vec<seer::ipc::DetectedExecutionAccountEvidenceEvent>,
}

pub struct SessionAccountUpdateBridge {
    detected_keys: HashMap<SessionDetectedKey, Instant>,
    detected_key_order: VecDeque<SessionDetectedKey>,
    pending_updates: HashMap<SessionDetectedKey, VecDeque<BufferedSessionAccountUpdate>>,
    pending_execution_evidence:
        HashMap<SessionDetectedKey, VecDeque<BufferedSessionExecutionAccountEvidence>>,
    pending_total: usize,
    pending_execution_evidence_total: usize,
    bcv2_contexts_by_pubkey: HashMap<Pubkey, SessionBcv2Context>,
    bcv2_by_base_mint: HashMap<Pubkey, HashSet<Pubkey>>,
    bcv2_by_pool: HashMap<Pubkey, HashSet<Pubkey>>,
    bcv2_by_canonical_bonding_curve: HashMap<Pubkey, HashSet<Pubkey>>,
    ttl: Duration,
    per_key_cap: usize,
    global_cap: usize,
    detected_key_ttl: Duration,
    detected_key_cap: usize,
}

impl Default for SessionAccountUpdateBridge {
    fn default() -> Self {
        Self::new(
            SESSION_ACCOUNT_UPDATE_BUFFER_TTL,
            SESSION_ACCOUNT_UPDATE_BUFFER_PER_KEY_CAP,
            SESSION_ACCOUNT_UPDATE_BUFFER_GLOBAL_CAP,
            SESSION_POOL_REGISTRY_FALLBACK_TTL,
            SESSION_POOL_REGISTRY_FALLBACK_CAP,
        )
    }
}

impl SessionAccountUpdateBridge {
    pub fn new(
        ttl: Duration,
        per_key_cap: usize,
        global_cap: usize,
        detected_key_ttl: Duration,
        detected_key_cap: usize,
    ) -> Self {
        Self {
            detected_keys: HashMap::new(),
            detected_key_order: VecDeque::new(),
            pending_updates: HashMap::new(),
            pending_execution_evidence: HashMap::new(),
            pending_total: 0,
            pending_execution_evidence_total: 0,
            bcv2_contexts_by_pubkey: HashMap::new(),
            bcv2_by_base_mint: HashMap::new(),
            bcv2_by_pool: HashMap::new(),
            bcv2_by_canonical_bonding_curve: HashMap::new(),
            ttl,
            per_key_cap: per_key_cap.max(1),
            global_cap: global_cap.max(1),
            detected_key_ttl,
            detected_key_cap: detected_key_cap.max(1),
        }
    }

    fn from_runtime_config(detected_key_ttl: Duration, detected_key_cap: usize) -> Self {
        Self::new(
            SESSION_ACCOUNT_UPDATE_BUFFER_TTL,
            SESSION_ACCOUNT_UPDATE_BUFFER_PER_KEY_CAP,
            SESSION_ACCOUNT_UPDATE_BUFFER_GLOBAL_CAP,
            detected_key_ttl,
            detected_key_cap,
        )
    }

    pub fn register_detected_pool(
        &mut self,
        candidate: &seer::types::CandidatePool,
        now: Instant,
    ) -> SessionAccountUpdateFlushResult {
        let liveness = self.refresh_detected_keys(
            [
                SessionDetectedKey::pool(candidate.pool_amm_id),
                SessionDetectedKey::bonding_curve(candidate.bonding_curve),
                SessionDetectedKey::base_mint(candidate.base_mint),
            ],
            now,
        );

        let mut replay_ready = Vec::new();
        let mut flush_keys = Vec::new();
        for key in [
            SessionDetectedKey::pool(candidate.pool_amm_id),
            SessionDetectedKey::bonding_curve(candidate.bonding_curve),
            SessionDetectedKey::base_mint(candidate.base_mint),
        ] {
            if key.pubkey != Pubkey::default() && !flush_keys.contains(&key) {
                flush_keys.push(key);
            }
        }

        for key in flush_keys {
            if let Some(mut queue) = self.pending_updates.remove(&key) {
                while let Some(buffered) = queue.pop_front() {
                    self.pending_total = self.pending_total.saturating_sub(1);
                    if now.duration_since(buffered.buffered_at) <= self.ttl {
                        replay_ready.push(buffered.update);
                    }
                }
            }
        }

        replay_ready.sort_by_key(|update| {
            (
                update.slot,
                update.write_version.unwrap_or(u64::MAX),
                update.sequence_number,
            )
        });

        let mut replay_ready_evidence = liveness.replay_ready_evidence;
        let mut expired_evidence_count = liveness.expired_evidence_count;
        for bcv2 in self.related_bcv2_for_detected_pool(candidate) {
            if let Some(context) = self.bcv2_contexts_by_pubkey.get(&bcv2).copied() {
                self.register_bcv2_key(context, now, "pool_detected");
            } else {
                self.mark_detected_key(SessionDetectedKey::bonding_curve_v2(bcv2), now);
            }
            let (mut flushed, expired) =
                self.flush_pending_bcv2_evidence(bcv2, now, "pool_detected");
            replay_ready_evidence.append(&mut flushed);
            expired_evidence_count += expired;
        }
        replay_ready_evidence.sort_by_key(|event| event.sequence_number);

        SessionAccountUpdateFlushResult {
            replay_ready,
            replay_ready_evidence,
            expired_count: liveness.expired_count,
            expired_evidence_count,
            expired_detected_keys: liveness.expired_detected_keys,
        }
    }

    fn refresh_from_trade(
        &mut self,
        trade: &seer::types::TradeEvent,
        now: Instant,
    ) -> SessionAccountUpdateLivenessResult {
        let mut liveness = self.refresh_detected_keys(
            [
                SessionDetectedKey::pool(trade.pool_amm_id),
                SessionDetectedKey::base_mint(trade.mint),
            ],
            now,
        );
        if let Some(context) = route_compatible_trade_bcv2_context(trade) {
            self.record_bcv2_context(context);
            self.register_bcv2_key(context, now, "route_compatible_trade");
            let (mut flushed, expired) = self.flush_pending_bcv2_evidence(
                context.account_pubkey,
                now,
                "route_compatible_trade",
            );
            liveness.replay_ready_evidence.append(&mut flushed);
            liveness.expired_evidence_count += expired;
            liveness
                .replay_ready_evidence
                .sort_by_key(|event| event.sequence_number);
        }
        liveness
    }

    pub fn ingest_account_update(
        &mut self,
        update: &seer::ipc::DetectedAccountUpdateEvent,
        now: Instant,
    ) -> SessionAccountUpdateIngressResult {
        let (expired_count, expired_detected_keys, _) = self.prune_expired(now);

        let bonding_curve_key = SessionDetectedKey::bonding_curve(update.bonding_curve);
        let base_mint_key = SessionDetectedKey::base_mint(update.base_mint);
        if self.detected_keys.contains_key(&bonding_curve_key)
            || self.detected_keys.contains_key(&base_mint_key)
        {
            self.mark_detected_keys([bonding_curve_key, base_mint_key], now);
            return SessionAccountUpdateIngressResult {
                decision: SessionAccountUpdateDecision::ForwardNow,
                expired_count,
                expired_detected_keys,
                evicted_per_key: 0,
                evicted_global: 0,
            };
        }

        let key = if update.bonding_curve != Pubkey::default() {
            bonding_curve_key
        } else if update.base_mint != Pubkey::default() {
            base_mint_key
        } else {
            return SessionAccountUpdateIngressResult {
                decision: SessionAccountUpdateDecision::SilentDrop,
                expired_count,
                expired_detected_keys,
                evicted_per_key: 0,
                evicted_global: 0,
            };
        };

        let mut evicted_per_key = 0;
        let mut evicted_global = 0;

        while self.pending_total >= self.global_cap {
            if self.evict_oldest_pending_update().is_some() {
                evicted_global += 1;
            } else {
                break;
            }
        }

        let queue = self.pending_updates.entry(key).or_default();
        while queue.len() >= self.per_key_cap {
            if queue.pop_front().is_some() {
                self.pending_total = self.pending_total.saturating_sub(1);
                evicted_per_key += 1;
            } else {
                break;
            }
        }

        queue.push_back(BufferedSessionAccountUpdate {
            update: update.clone(),
            buffered_at: now,
        });
        self.pending_total += 1;

        SessionAccountUpdateIngressResult {
            decision: SessionAccountUpdateDecision::BufferedUntilPoolDetected,
            expired_count,
            expired_detected_keys,
            evicted_per_key,
            evicted_global,
        }
    }

    pub fn ingest_execution_account_evidence(
        &mut self,
        event: &seer::ipc::DetectedExecutionAccountEvidenceEvent,
        now: Instant,
    ) -> SessionExecutionAccountEvidenceIngressResult {
        let (expired_count, expired_detected_keys, expired_evidence_count) =
            self.prune_expired(now);

        if event.evidence.role != ExecutionAccountRole::BondingCurveV2 {
            return SessionExecutionAccountEvidenceIngressResult {
                decision: SessionExecutionAccountEvidenceDecision::ForwardNow,
                expired_count,
                expired_detected_keys,
                expired_evidence_count,
                evicted_per_key: 0,
                evicted_global: 0,
            };
        }

        let Some(context) = SessionBcv2Context::from_evidence(&event.evidence) else {
            return SessionExecutionAccountEvidenceIngressResult {
                decision: SessionExecutionAccountEvidenceDecision::SilentDrop,
                expired_count,
                expired_detected_keys,
                expired_evidence_count,
                evicted_per_key: 0,
                evicted_global: 0,
            };
        };

        self.record_bcv2_context(context);
        let bcv2_key = SessionDetectedKey::bonding_curve_v2(context.account_pubkey);
        if self.detected_keys.contains_key(&bcv2_key)
            || self.bcv2_context_has_detected_session(&context)
        {
            self.register_bcv2_key(context, now, "execution_account_evidence");
            return SessionExecutionAccountEvidenceIngressResult {
                decision: SessionExecutionAccountEvidenceDecision::ForwardNow,
                expired_count,
                expired_detected_keys,
                expired_evidence_count,
                evicted_per_key: 0,
                evicted_global: 0,
            };
        }

        let mut evicted_per_key = 0;
        let mut evicted_global = 0;

        while self.pending_execution_evidence_total >= self.global_cap {
            if self.evict_oldest_pending_evidence().is_some() {
                evicted_global += 1;
            } else {
                break;
            }
        }

        let queue = self.pending_execution_evidence.entry(bcv2_key).or_default();
        while queue.len() >= self.per_key_cap {
            if queue.pop_front().is_some() {
                self.pending_execution_evidence_total =
                    self.pending_execution_evidence_total.saturating_sub(1);
                evicted_per_key += 1;
            } else {
                break;
            }
        }

        queue.push_back(BufferedSessionExecutionAccountEvidence {
            event: event.clone(),
            buffered_at: now,
        });
        self.pending_execution_evidence_total += 1;
        increment_counter!("seer_bridge_session_bcv2_evidence_buffered_total");

        SessionExecutionAccountEvidenceIngressResult {
            decision: SessionExecutionAccountEvidenceDecision::BufferedUntilPoolDetected,
            expired_count,
            expired_detected_keys,
            expired_evidence_count,
            evicted_per_key,
            evicted_global,
        }
    }

    fn prune_expired(&mut self, now: Instant) -> (usize, usize, usize) {
        let mut expired = 0;
        let mut empty_keys = Vec::new();

        for (key, queue) in self.pending_updates.iter_mut() {
            while matches!(queue.front(), Some(front) if now.duration_since(front.buffered_at) > self.ttl)
            {
                if queue.pop_front().is_some() {
                    self.pending_total = self.pending_total.saturating_sub(1);
                    expired += 1;
                }
            }

            if queue.is_empty() {
                empty_keys.push(*key);
            }
        }

        for key in empty_keys {
            self.pending_updates.remove(&key);
        }

        let mut expired_evidence = 0;
        let mut empty_evidence_keys = Vec::new();

        for (key, queue) in self.pending_execution_evidence.iter_mut() {
            while matches!(queue.front(), Some(front) if now.duration_since(front.buffered_at) > self.ttl)
            {
                if queue.pop_front().is_some() {
                    self.pending_execution_evidence_total =
                        self.pending_execution_evidence_total.saturating_sub(1);
                    expired_evidence += 1;
                }
            }

            if queue.is_empty() {
                empty_evidence_keys.push(*key);
            }
        }

        for key in empty_evidence_keys {
            self.pending_execution_evidence.remove(&key);
        }

        if expired_evidence > 0 {
            ::metrics::counter!(
                "seer_bridge_session_bcv2_pending_expired_total",
                expired_evidence as u64
            );
            tracing::info!(count = expired_evidence, "BCV2_SESSION_PENDING_EXPIRED");
        }

        let mut expired_detected_keys = 0;
        while let Some(key) = self.detected_key_order.front().copied() {
            let is_expired = self
                .detected_keys
                .get(&key)
                .map(|last_seen| now.duration_since(*last_seen) > self.detected_key_ttl)
                .unwrap_or(true);
            if !is_expired {
                break;
            }

            self.detected_key_order.pop_front();
            if self.detected_keys.remove(&key).is_some() {
                expired_detected_keys += 1;
            }
        }

        (expired, expired_detected_keys, expired_evidence)
    }

    fn mark_detected_key(&mut self, key: SessionDetectedKey, now: Instant) -> bool {
        if key.pubkey == Pubkey::default() {
            return false;
        }
        let is_new = !self.detected_keys.contains_key(&key);
        if !self.detected_keys.contains_key(&key) {
            self.detected_key_order.push_back(key);
        }
        self.detected_keys.insert(key, now);
        is_new
    }

    fn mark_detected_keys<I>(&mut self, keys: I, now: Instant)
    where
        I: IntoIterator<Item = SessionDetectedKey>,
    {
        let mut protected_keys = HashSet::new();
        for key in keys {
            if key.pubkey != Pubkey::default() {
                self.mark_detected_key(key, now);
                protected_keys.insert(key);
            }
        }

        while self.detected_keys.len() > self.detected_key_cap {
            if self.evict_oldest_detected_key(&protected_keys).is_none() {
                break;
            }
        }
    }

    fn refresh_detected_keys<I>(
        &mut self,
        keys: I,
        now: Instant,
    ) -> SessionAccountUpdateLivenessResult
    where
        I: IntoIterator<Item = SessionDetectedKey>,
    {
        let (expired_count, expired_detected_keys, expired_evidence_count) =
            self.prune_expired(now);
        self.mark_detected_keys(keys, now);
        SessionAccountUpdateLivenessResult {
            expired_count,
            expired_evidence_count,
            expired_detected_keys,
            replay_ready_evidence: Vec::new(),
        }
    }

    fn evict_oldest_pending_update(&mut self) -> Option<seer::ipc::DetectedAccountUpdateEvent> {
        let oldest_key = self
            .pending_updates
            .iter()
            .filter_map(|(key, queue)| queue.front().map(|front| (*key, front.buffered_at)))
            .min_by_key(|(_, buffered_at)| *buffered_at)
            .map(|(key, _)| key)?;

        let removed = {
            let queue = self.pending_updates.get_mut(&oldest_key)?;
            let removed = queue.pop_front();
            let emptied = queue.is_empty();
            (removed, emptied)
        };

        if removed.0.is_some() {
            self.pending_total = self.pending_total.saturating_sub(1);
        }
        if removed.1 {
            self.pending_updates.remove(&oldest_key);
        }

        removed.0.map(|buffered| buffered.update)
    }

    fn evict_oldest_pending_evidence(
        &mut self,
    ) -> Option<seer::ipc::DetectedExecutionAccountEvidenceEvent> {
        let oldest_key = self
            .pending_execution_evidence
            .iter()
            .filter_map(|(key, queue)| queue.front().map(|front| (*key, front.buffered_at)))
            .min_by_key(|(_, buffered_at)| *buffered_at)
            .map(|(key, _)| key)?;

        let removed = {
            let queue = self.pending_execution_evidence.get_mut(&oldest_key)?;
            let removed = queue.pop_front();
            let emptied = queue.is_empty();
            (removed, emptied)
        };

        if removed.0.is_some() {
            self.pending_execution_evidence_total =
                self.pending_execution_evidence_total.saturating_sub(1);
        }
        if removed.1 {
            self.pending_execution_evidence.remove(&oldest_key);
        }

        removed.0.map(|buffered| buffered.event)
    }

    fn record_bcv2_context(&mut self, context: SessionBcv2Context) {
        if context.account_pubkey == Pubkey::default() {
            return;
        }

        self.bcv2_contexts_by_pubkey
            .entry(context.account_pubkey)
            .and_modify(|existing| {
                if existing.base_mint.is_none() {
                    existing.base_mint = context.base_mint;
                }
                if existing.pool_id.is_none() {
                    existing.pool_id = context.pool_id;
                }
                if existing.canonical_bonding_curve.is_none() {
                    existing.canonical_bonding_curve = context.canonical_bonding_curve;
                }
            })
            .or_insert(context);

        if let Some(base_mint) = context.base_mint {
            self.bcv2_by_base_mint
                .entry(base_mint)
                .or_default()
                .insert(context.account_pubkey);
        }
        if let Some(pool_id) = context.pool_id {
            self.bcv2_by_pool
                .entry(pool_id)
                .or_default()
                .insert(context.account_pubkey);
        }
        if let Some(canonical_bonding_curve) = context.canonical_bonding_curve {
            self.bcv2_by_canonical_bonding_curve
                .entry(canonical_bonding_curve)
                .or_default()
                .insert(context.account_pubkey);
        }
    }

    fn bcv2_context_has_detected_session(&self, context: &SessionBcv2Context) -> bool {
        self.detected_keys
            .contains_key(&SessionDetectedKey::bonding_curve_v2(
                context.account_pubkey,
            ))
            || context
                .pool_id
                .map(|pool| {
                    self.detected_keys
                        .contains_key(&SessionDetectedKey::pool(pool))
                })
                .unwrap_or(false)
            || context
                .base_mint
                .map(|base_mint| {
                    self.detected_keys
                        .contains_key(&SessionDetectedKey::base_mint(base_mint))
                })
                .unwrap_or(false)
            || context
                .canonical_bonding_curve
                .map(|bonding_curve| {
                    self.detected_keys
                        .contains_key(&SessionDetectedKey::bonding_curve(bonding_curve))
                })
                .unwrap_or(false)
    }

    fn register_bcv2_key(
        &mut self,
        context: SessionBcv2Context,
        now: Instant,
        trigger: &'static str,
    ) -> bool {
        let inserted = self.mark_detected_key(
            SessionDetectedKey::bonding_curve_v2(context.account_pubkey),
            now,
        );
        if inserted {
            increment_counter!("seer_bridge_session_bcv2_key_registered_total");
            tracing::info!(
                account_pubkey = %context.account_pubkey,
                base_mint = ?context.base_mint,
                pool_id = ?context.pool_id,
                trigger,
                "BCV2_SESSION_KEY_REGISTERED"
            );
        }
        inserted
    }

    fn related_bcv2_for_detected_pool(
        &self,
        candidate: &seer::types::CandidatePool,
    ) -> Vec<Pubkey> {
        let mut related = HashSet::new();
        if let Some(values) = self.bcv2_by_pool.get(&candidate.pool_amm_id) {
            related.extend(values.iter().copied());
        }
        if let Some(values) = self.bcv2_by_base_mint.get(&candidate.base_mint) {
            related.extend(values.iter().copied());
        }
        if let Some(values) = self
            .bcv2_by_canonical_bonding_curve
            .get(&candidate.bonding_curve)
        {
            related.extend(values.iter().copied());
        }
        let mut related = related.into_iter().collect::<Vec<_>>();
        related.sort();
        related
    }

    fn flush_pending_bcv2_evidence(
        &mut self,
        account_pubkey: Pubkey,
        now: Instant,
        trigger: &'static str,
    ) -> (Vec<seer::ipc::DetectedExecutionAccountEvidenceEvent>, usize) {
        let key = SessionDetectedKey::bonding_curve_v2(account_pubkey);
        let mut replay_ready = Vec::new();
        let mut expired = 0;

        if let Some(mut queue) = self.pending_execution_evidence.remove(&key) {
            while let Some(buffered) = queue.pop_front() {
                self.pending_execution_evidence_total =
                    self.pending_execution_evidence_total.saturating_sub(1);
                if now.duration_since(buffered.buffered_at) <= self.ttl {
                    replay_ready.push(buffered.event);
                } else {
                    expired += 1;
                }
            }
        }

        if !replay_ready.is_empty() {
            ::metrics::counter!(
                "seer_bridge_session_bcv2_pending_flushed_total",
                replay_ready.len() as u64
            );
            tracing::info!(
                account_pubkey = %account_pubkey,
                count = replay_ready.len(),
                trigger,
                "BCV2_SESSION_PENDING_FLUSHED"
            );
        }
        if expired > 0 {
            ::metrics::counter!(
                "seer_bridge_session_bcv2_pending_expired_total",
                expired as u64
            );
            tracing::info!(
                account_pubkey = %account_pubkey,
                count = expired,
                trigger,
                "BCV2_SESSION_PENDING_EXPIRED"
            );
        }

        replay_ready.sort_by_key(|event| event.sequence_number);
        (replay_ready, expired)
    }

    fn evict_oldest_detected_key(
        &mut self,
        protected_keys: &HashSet<SessionDetectedKey>,
    ) -> Option<SessionDetectedKey> {
        let mut deferred = VecDeque::new();
        let mut evicted = None;

        while let Some(key) = self.detected_key_order.pop_front() {
            if !self.detected_keys.contains_key(&key) {
                continue;
            }
            if protected_keys.contains(&key) {
                deferred.push_back(key);
                continue;
            }

            self.detected_keys.remove(&key);
            evicted = Some(key);
            break;
        }

        while let Some(key) = deferred.pop_front() {
            self.detected_key_order.push_back(key);
        }

        evicted
    }

    #[cfg(test)]
    fn pending_total(&self) -> usize {
        self.pending_total
    }

    #[cfg(test)]
    fn pending_execution_evidence_total(&self) -> usize {
        self.pending_execution_evidence_total
    }

    #[cfg(test)]
    fn has_detected_bcv2_key(&self, pubkey: Pubkey) -> bool {
        self.detected_keys
            .contains_key(&SessionDetectedKey::bonding_curve_v2(pubkey))
    }
}

pub struct SessionPoolTradeBridge {
    detected_pools: HashMap<Pubkey, Instant>,
    detected_pool_order: VecDeque<Pubkey>,
    pending_trades: HashMap<Pubkey, VecDeque<BufferedSessionTrade>>,
    pending_trade_keys: HashSet<BufferedSessionTradeKey>,
    pending_total: usize,
    ttl: Duration,
    per_pool_cap: usize,
    global_cap: usize,
    detected_pool_ttl: Duration,
    detected_pool_cap: usize,
}

impl Default for SessionPoolTradeBridge {
    fn default() -> Self {
        Self::new(
            SESSION_POOL_TRADE_BUFFER_TTL,
            SESSION_POOL_TRADE_BUFFER_PER_POOL_CAP,
            SESSION_POOL_TRADE_BUFFER_GLOBAL_CAP,
            SESSION_POOL_REGISTRY_FALLBACK_TTL,
            SESSION_POOL_REGISTRY_FALLBACK_CAP,
        )
    }
}

impl SessionPoolTradeBridge {
    pub fn new(
        ttl: Duration,
        per_pool_cap: usize,
        global_cap: usize,
        detected_pool_ttl: Duration,
        detected_pool_cap: usize,
    ) -> Self {
        Self {
            detected_pools: HashMap::new(),
            detected_pool_order: VecDeque::new(),
            pending_trades: HashMap::new(),
            pending_trade_keys: HashSet::new(),
            pending_total: 0,
            ttl,
            per_pool_cap: per_pool_cap.max(1),
            global_cap: global_cap.max(1),
            detected_pool_ttl,
            detected_pool_cap: detected_pool_cap.max(1),
        }
    }

    fn from_runtime_config(
        pending_ttl: Duration,
        detected_pool_ttl: Duration,
        detected_pool_cap: usize,
    ) -> Self {
        Self::new(
            pending_ttl,
            SESSION_POOL_TRADE_BUFFER_PER_POOL_CAP,
            SESSION_POOL_TRADE_BUFFER_GLOBAL_CAP,
            detected_pool_ttl,
            detected_pool_cap,
        )
    }

    pub fn register_detected_pool(
        &mut self,
        pool: Pubkey,
        now: Instant,
    ) -> SessionTradeFlushResult {
        let (expired_count, expired_detected_pools) = self.prune_expired(now);
        self.mark_detected_pool(pool, now);

        let mut evicted_detected_pools = 0;
        while self.detected_pools.len() > self.detected_pool_cap {
            if self.evict_oldest_detected_pool(pool).is_some() {
                evicted_detected_pools += 1;
            } else {
                break;
            }
        }

        let mut replay_ready = Vec::new();
        if let Some(mut queue) = self.pending_trades.remove(&pool) {
            while let Some(buffered) = queue.pop_front() {
                self.pending_total = self.pending_total.saturating_sub(1);
                self.pending_trade_keys.remove(&buffered.dedupe_key);
                if now.duration_since(buffered.buffered_at) <= self.ttl {
                    replay_ready.push(buffered.trade);
                }
            }
        }

        SessionTradeFlushResult {
            replay_ready,
            expired_count,
            expired_detected_pools,
            evicted_detected_pools,
        }
    }

    pub fn ingest_trade(
        &mut self,
        trade: &seer::types::TradeEvent,
        now: Instant,
    ) -> SessionTradeIngressResult {
        let (expired_count, _) = self.prune_expired(now);
        if self.detected_pools.contains_key(&trade.pool_amm_id) {
            self.mark_detected_pool(trade.pool_amm_id, now);
            return SessionTradeIngressResult {
                decision: SessionTradeDecision::ForwardNow,
                expired_count,
                evicted_per_pool: 0,
                evicted_global: 0,
            };
        }

        if trade.pool_amm_id == Pubkey::default() || trade.mint == Pubkey::default() {
            return SessionTradeIngressResult {
                decision: SessionTradeDecision::SilentDrop,
                expired_count,
                evicted_per_pool: 0,
                evicted_global: 0,
            };
        }
        SessionTradeIngressResult {
            decision: SessionTradeDecision::SilentDrop,
            expired_count,
            evicted_per_pool: 0,
            evicted_global: 0,
        }
    }

    fn prune_expired(&mut self, now: Instant) -> (usize, usize) {
        let mut expired = 0;
        let mut expired_detected_pools = 0;
        let mut empty_pools = Vec::new();

        for (pool, queue) in self.pending_trades.iter_mut() {
            while matches!(queue.front(), Some(front) if now.duration_since(front.buffered_at) > self.ttl)
            {
                if let Some(removed) = queue.pop_front() {
                    self.pending_total = self.pending_total.saturating_sub(1);
                    self.pending_trade_keys.remove(&removed.dedupe_key);
                    expired += 1;
                }
            }

            if queue.is_empty() {
                empty_pools.push(*pool);
            }
        }

        for pool in empty_pools {
            self.pending_trades.remove(&pool);
        }

        while let Some(pool) = self.detected_pool_order.front().copied() {
            let is_expired = self
                .detected_pools
                .get(&pool)
                .map(|last_seen| now.duration_since(*last_seen) > self.detected_pool_ttl)
                .unwrap_or(true);

            if !is_expired {
                break;
            }

            self.detected_pool_order.pop_front();
            if self.detected_pools.remove(&pool).is_some() {
                expired_detected_pools += 1;
            }
        }

        (expired, expired_detected_pools)
    }

    fn evict_oldest_pending_trade(&mut self) -> Option<seer::types::TradeEvent> {
        let oldest_pool = self
            .pending_trades
            .iter()
            .filter_map(|(pool, queue)| queue.front().map(|front| (*pool, front.buffered_at)))
            .min_by_key(|(_, buffered_at)| *buffered_at)
            .map(|(pool, _)| pool)?;

        let removed = {
            let queue = self.pending_trades.get_mut(&oldest_pool)?;
            let removed = queue.pop_front();
            let emptied = queue.is_empty();
            (removed, emptied)
        };

        if let Some(ref removed_trade) = removed.0 {
            self.pending_total = self.pending_total.saturating_sub(1);
            self.pending_trade_keys.remove(&removed_trade.dedupe_key);
        }
        if removed.1 {
            self.pending_trades.remove(&oldest_pool);
        }

        removed.0.map(|buffered| buffered.trade)
    }

    fn mark_detected_pool(&mut self, pool: Pubkey, now: Instant) {
        if !self.detected_pools.contains_key(&pool) {
            self.detected_pool_order.push_back(pool);
        }
        self.detected_pools.insert(pool, now);
    }

    fn evict_oldest_detected_pool(&mut self, protected_pool: Pubkey) -> Option<Pubkey> {
        let mut deferred = VecDeque::new();
        let mut evicted = None;

        while let Some(pool) = self.detected_pool_order.pop_front() {
            if !self.detected_pools.contains_key(&pool) {
                continue;
            }
            if pool == protected_pool {
                deferred.push_back(pool);
                continue;
            }

            self.detected_pools.remove(&pool);
            evicted = Some(pool);
            break;
        }

        while let Some(pool) = deferred.pop_front() {
            self.detected_pool_order.push_back(pool);
        }

        evicted
    }

    #[cfg(test)]
    fn pending_total(&self) -> usize {
        self.pending_total
    }

    #[cfg(test)]
    fn detected_total(&self) -> usize {
        self.detected_pools.len()
    }
}

fn record_session_buffer_expired(count: usize) {
    if count == 0 {
        return;
    }
    ::metrics::counter!("seer_bridge_session_pool_expired_total", count as u64);
}

fn record_session_detected_pool_expired(count: usize) {
    if count == 0 {
        return;
    }
    ::metrics::counter!(
        "seer_bridge_session_pool_registry_expired_total",
        count as u64
    );
}

fn record_session_detected_pool_evicted(count: usize) {
    if count == 0 {
        return;
    }
    ::metrics::counter!(
        "seer_bridge_session_pool_registry_evicted_total",
        count as u64
    );
}

fn record_session_account_update_expired(count: usize) {
    if count == 0 {
        return;
    }
    ::metrics::counter!(
        "seer_bridge_session_account_update_expired_total",
        count as u64
    );
}

fn record_session_account_update_detected_key_expired(count: usize) {
    if count == 0 {
        return;
    }
    ::metrics::counter!(
        "seer_bridge_session_account_update_registry_expired_total",
        count as u64
    );
}

fn record_session_account_update_evictions(per_key: usize, global: usize) {
    if per_key > 0 {
        ::metrics::counter!(
            "seer_bridge_session_account_update_rejected_total",
            per_key as u64,
            "reason" => "per_key_cap"
        );
    }
    if global > 0 {
        ::metrics::counter!(
            "seer_bridge_session_account_update_rejected_total",
            global as u64,
            "reason" => "global_cap"
        );
    }
}

fn record_session_buffer_evictions(per_pool: usize, global: usize) {
    if per_pool > 0 {
        ::metrics::counter!(
            "seer_bridge_session_pool_rejected_total",
            per_pool as u64,
            "reason" => "per_pool_cap"
        );
    }
    if global > 0 {
        ::metrics::counter!(
            "seer_bridge_session_pool_rejected_total",
            global as u64,
            "reason" => "global_cap"
        );
    }
}

fn session_bridge_prune_interval(ttl: Duration, detected_pool_ttl: Duration) -> Duration {
    let min_window = ttl.min(detected_pool_ttl);
    min_window
        .min(SESSION_POOL_BRIDGE_PRUNE_INTERVAL)
        .max(Duration::from_millis(50))
}

fn detected_pool_from_candidate(
    candidate: &seer::types::CandidatePool,
    detected_ms: u64,
) -> DetectedPool {
    DetectedPool {
        semantic: if candidate.effective_event_ts_ms().is_some() {
            candidate.semantic
        } else {
            candidate
                .semantic
                .with_timestamp_quality(TimestampQuality::WallClock)
        },
        pool_amm_id: candidate.pool_amm_id.to_string(),
        base_mint: candidate.base_mint.to_string(),
        quote_mint: candidate.quote_mint.to_string(),
        amm_program: candidate.amm_program_id.to_string(),
        bonding_curve: candidate.bonding_curve.to_string(),
        creator: sanitize_detected_creator(candidate.creator),
        slot: candidate.slot,
        tx_index: candidate.tx_index,
        timestamp_ms: candidate.compat_event_ts_ms().unwrap_or(detected_ms),
        event_time: candidate.event_time,
        detected_wall_ts_ms: Some(detected_ms),
        initial_liquidity_sol: candidate.initial_liquidity_sol,
        signature: candidate.signature.clone(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DetectionClockSummary {
    compat_event_ts_ms: u64,
    effective_event_ts_ms: Option<u64>,
    chain_event_ts_ms: Option<u64>,
    has_explicit_event_time: bool,
    ingest_latency_ms: u64,
}

fn detection_clock_summary(
    candidate: &seer::types::CandidatePool,
    detected_ms: u64,
) -> DetectionClockSummary {
    let effective_event_ts_ms = candidate.effective_event_ts_ms();
    let compat_event_ts_ms = candidate.compat_event_ts_ms().unwrap_or(detected_ms);
    DetectionClockSummary {
        compat_event_ts_ms,
        effective_event_ts_ms,
        chain_event_ts_ms: candidate.event_time.chain_event_ts_ms,
        has_explicit_event_time: effective_event_ts_ms.is_some(),
        ingest_latency_ms: detected_ms.saturating_sub(effective_event_ts_ms.unwrap_or(detected_ms)),
    }
}

fn process_trade_event_for_session_gate(
    tx: &EventBusSender,
    session_trade_bridge: &mut SessionPoolTradeBridge,
    trade: &seer::types::TradeEvent,
    health: Option<&Arc<RuntimeHealth>>,
    now: Instant,
) -> SessionTradeIngressResult {
    let gating_result = session_trade_bridge.ingest_trade(trade, now);
    record_session_buffer_expired(gating_result.expired_count);
    record_session_buffer_evictions(gating_result.evicted_per_pool, gating_result.evicted_global);

    match gating_result.decision {
        SessionTradeDecision::ForwardNow => {
            emit_pool_transaction_to_event_bus(tx, trade, health, false);
        }
        SessionTradeDecision::SilentDrop => {
            // Pool not yet registered in this session — discarded without event bus emission.
            increment_counter!("seer_bridge_session_pool_silent_drop_total");
        }
    }

    gating_result
}

fn process_pool_detected_event_for_session_gate(
    tx: &EventBusSender,
    session_trade_bridge: &mut SessionPoolTradeBridge,
    candidate: &seer::types::CandidatePool,
    health: Option<&Arc<RuntimeHealth>>,
    now: Instant,
    detected_ms: u64,
) -> SessionTradeFlushResult {
    let detected_pool = detected_pool_from_candidate(candidate, detected_ms);

    info!(
        "Seer: 🚀 Emitting NewPoolDetected: pool_amm_id={}, base_mint={}, slot={:?}, amm_program={}",
        detected_pool.pool_amm_id,
        detected_pool.base_mint,
        detected_pool.slot,
        detected_pool.amm_program
    );

    if let Err(e) = tx.send(GhostEvent::new_pool_detected(detected_pool.clone())) {
        error!("Seer: ❌ Failed to emit NewPoolDetected event: {}", e);
    } else {
        if let Some(health) = health {
            health.mark_bus_event();
        }
        info!(
            "Seer: ✅ Event emitted to Event Bus for new pool: pool={}, receivers={}",
            detected_pool.pool_amm_id,
            tx.receiver_count()
        );
    }

    let flush_result = session_trade_bridge.register_detected_pool(candidate.pool_amm_id, now);
    record_session_buffer_expired(flush_result.expired_count);
    record_session_detected_pool_expired(flush_result.expired_detected_pools);
    record_session_detected_pool_evicted(flush_result.evicted_detected_pools);

    if !flush_result.replay_ready.is_empty() {
        ::metrics::counter!(
            "seer_bridge_session_pool_replayed_total",
            flush_result.replay_ready.len() as u64
        );
        info!(
            "Seer: ♻️ Replaying {} session-buffered trades after PoolDetected for pool={}",
            flush_result.replay_ready.len(),
            candidate.pool_amm_id
        );
        for trade in &flush_result.replay_ready {
            emit_pool_transaction_to_event_bus(tx, trade, health, true);
        }
    }

    flush_result
}

fn emit_pool_transaction_to_event_bus(
    tx: &EventBusSender,
    trade: &seer::types::TradeEvent,
    health: Option<&Arc<RuntimeHealth>>,
    replayed_from_session_buffer: bool,
) {
    let pool_tx = trade_event_to_pool_transaction(trade);

    info!(
        "Seer: 🚀 Emitting PoolTransaction: {} pool={} volume={:.4} SOL replayed_from_session_buffer={}",
        if trade.is_buy { "BUY" } else { "SELL" },
        pool_tx.pool_amm_id,
        pool_tx.volume_sol,
        replayed_from_session_buffer
    );

    if let Err(e) = tx.send(GhostEvent::pool_transaction(pool_tx)) {
        error!("Seer: ❌ Failed to emit PoolTransaction event: {}", e);
    } else {
        if let Some(health) = health {
            health.mark_bus_event();
        }
        info!(
            "Seer: ✅ PoolTransaction ZOSTAŁA PRZEKAZANA DO MAGISTRALI ZDARZEŃ: receivers={} replayed_from_session_buffer={}",
            tx.receiver_count(),
            replayed_from_session_buffer
        );
    }
}

fn emit_account_update_to_event_bus(
    tx: &EventBusSender,
    update: &seer::ipc::DetectedAccountUpdateEvent,
    health: Option<&Arc<RuntimeHealth>>,
    replayed_from_session_buffer: bool,
) {
    tracing::info!(
        base_mint = %update.base_mint,
        bonding_curve = %update.bonding_curve,
        slot = update.slot,
        sol_reserves = update.sol_reserves,
        token_reserves = update.token_reserves,
        complete = update.complete,
        curve_finality = %update.curve_finality.as_str(),
        replayed_from_session_buffer,
        "DIAG_ACCOUNT_UPDATE_RELAY"
    );
    let ghost_event = GhostEvent::AccountUpdate(AccountUpdateEvent {
        semantic: update.semantic,
        event_time: update.event_time,
        base_mint: update.base_mint,
        bonding_curve: update.bonding_curve,
        curve_finality: update.curve_finality,
        sol_reserves: update.sol_reserves,
        token_reserves: update.token_reserves,
        complete: update.complete,
        slot: update.slot,
        write_version: update.write_version,
        replay_origin: update.replay_origin,
        replay_buffer_dwell_ms: update.replay_buffer_dwell_ms,
        detected_at: update.detected_at,
        sequence_number: update.sequence_number,
    });
    if let Err(e) = tx.send(ghost_event) {
        tracing::debug!(
            "Seer: AccountUpdate event not delivered (no receivers or lag): {}",
            e
        );
        return;
    }
    if let Some(health) = health {
        health.mark_bus_event();
    }
}

fn emit_funding_transfer_to_event_bus(
    tx: &EventBusSender,
    funding_event: &seer::ipc::DetectedFundingTransferEvent,
    health: Option<&Arc<RuntimeHealth>>,
) {
    let ghost_event = GhostEvent::funding_transfer_observed(FundingTransferObserved {
        semantic: funding_event.transfer.semantic.clone(),
        slot: funding_event.transfer.slot,
        event_ordinal: funding_event.transfer.event_ordinal,
        tx_index: funding_event.transfer.tx_index,
        outer_instruction_index: funding_event.transfer.outer_instruction_index,
        inner_group_index: funding_event.transfer.inner_group_index,
        cpi_stack_height: funding_event.transfer.cpi_stack_height,
        event_time: funding_event.transfer.event_time.clone(),
        arrival_ts_ms: funding_event.transfer.arrival_ts_ms,
        signature: funding_event.transfer.signature.clone(),
        source_wallet: funding_event.transfer.source_wallet.clone(),
        recipient_wallet: funding_event.transfer.recipient_wallet.clone(),
        lamports: funding_event.transfer.lamports,
        full_chain_coverage: funding_event.transfer.full_chain_coverage,
        provenance: funding_event.transfer.provenance,
        lane_health: funding_event.lane_health,
        detected_at: funding_event.detected_at.clone(),
        sequence_number: funding_event.sequence_number,
    });
    if let Err(e) = tx.send(ghost_event) {
        tracing::debug!(
            "Seer: FundingTransfer event not delivered (no receivers or lag): {}",
            e
        );
        return;
    }
    if let Some(health) = health {
        health.mark_bus_event();
    }
}

fn emit_execution_account_evidence_to_event_bus(
    tx: &EventBusSender,
    evidence_event: &seer::ipc::DetectedExecutionAccountEvidenceEvent,
    health: Option<&Arc<RuntimeHealth>>,
) {
    tracing::info!(
        role = %evidence_event.evidence.role.label(),
        account_pubkey = %evidence_event.evidence.account_pubkey,
        source = %evidence_event.evidence.source.as_str(),
        status = %evidence_event.evidence.status.as_str(),
        evidence_ready = evidence_event.evidence.evidence_ready,
        sequence_number = evidence_event.sequence_number,
        "DIAG_EXECUTION_ACCOUNT_EVIDENCE_RELAY"
    );

    let ghost_event = GhostEvent::execution_account_evidence(
        evidence_event.evidence.clone(),
        evidence_event.detected_at,
        evidence_event.sequence_number,
    );
    if let Err(e) = tx.send(ghost_event) {
        tracing::debug!(
            "Seer: ExecutionAccountEvidence event not delivered (no receivers or lag): {}",
            e
        );
        return;
    }
    if let Some(health) = health {
        health.mark_bus_event();
    }
}

fn spawn_nln_program_streams_capture(
    config: ProgramStreamsConfig,
    artifact_config: NlnArtifactCaptureConfig,
    event_bus_tx: Option<EventBusSender>,
    health: Option<Arc<RuntimeHealth>>,
    authoritative_funding_stream_tx: Option<watch::Sender<bool>>,
    trade_resolver: Arc<Mutex<NlnTradePoolIdentityResolver>>,
) -> Option<tokio::task::JoinHandle<()>> {
    if !config.enabled {
        return None;
    }

    Some(tokio::spawn(async move {
        let endpoint = redact_endpoint_for_logs(&config.endpoint);
        let artifact_capture_enabled = artifact_config.enabled;
        let artifact_capture_dir = artifact_config.capture_dir.clone();
        let artifact_writer = spawn_nln_artifact_writer(artifact_config);
        let selection = select_nln_program_stream_subscriptions(&config);
        let started_topics: Vec<String> = selection
            .subscriptions
            .iter()
            .map(|subscription| subscription.topic.clone())
            .collect();
        let started_topic_kinds: Vec<&'static str> = selection
            .subscriptions
            .iter()
            .map(|subscription| subscription.topic_kind.label())
            .collect();
        info!(
            endpoint = %endpoint,
            format = config.format.as_str(),
            requested_topic_count = selection.requested_topic_count,
            allowed_stream_count = selection.allowed_stream_count,
            started_topic_count = selection.subscriptions.len(),
            started_topics = ?started_topics,
            started_topic_kinds = ?started_topic_kinds,
            dropped_optional_topics = ?selection.dropped_optional_topics,
            "Seer: starting NLN Program Streams FSC capture lane"
        );
        if artifact_capture_enabled {
            write_nln_program_stream_run_manifest(&artifact_capture_dir, &config, &selection).await;
        }
        if selection.required_topics_exceed_limit || selection.quota_policy_violation {
            error!(
                endpoint = %endpoint,
                requested_topic_count = selection.requested_topic_count,
                allowed_stream_count = selection.allowed_stream_count,
                required_topics_exceed_limit = selection.required_topics_exceed_limit,
                quota_policy_violation = selection.quota_policy_violation,
                fail_reasons = ?selection.fail_reasons,
                "Seer: NLN Program Streams topic selection failed before connect"
            );
            if let Some(tx) = authoritative_funding_stream_tx.as_ref() {
                let _ = tx.send(false);
            }
            return;
        }
        if selection.subscriptions.is_empty() {
            error!(
                endpoint = %endpoint,
                allowed_stream_count = selection.allowed_stream_count,
                "Seer: NLN Program Streams capture lane has no selected topics"
            );
            if let Some(tx) = authoritative_funding_stream_tx.as_ref() {
                let _ = tx.send(false);
            }
            return;
        }

        let client = match NlnProgramStreamsClient::connect(config.clone()).await {
            Ok(client) => client,
            Err(err) => {
                error!(
                    endpoint = %endpoint,
                    error = %err,
                    "Seer: NLN Program Streams FSC capture connection failed"
                );
                if let Some(tx) = authoritative_funding_stream_tx.as_ref() {
                    let _ = tx.send(false);
                }
                return;
            }
        };

        match client.clone().list_topics().await {
            Ok(topics) => {
                let topic_count = topics.len();
                let has_transfers_topic = topics
                    .iter()
                    .any(|topic| topic.topic == config.system_transfers_topic);
                let listed_topics: HashSet<&str> =
                    topics.iter().map(|topic| topic.topic.as_str()).collect();
                let missing_selected_topics: Vec<&str> = selection
                    .subscriptions
                    .iter()
                    .map(|subscription| subscription.topic.as_str())
                    .filter(|topic| !listed_topics.contains(topic))
                    .collect();
                info!(
                    endpoint = %endpoint,
                    topic_count,
                    has_transfers_topic,
                    missing_selected_topics = ?missing_selected_topics,
                    "Seer: NLN Program Streams ListTopics completed"
                );
            }
            Err(err) => {
                warn!(
                    endpoint = %endpoint,
                    error = %err,
                    "Seer: NLN Program Streams ListTopics failed; continuing with configured topic"
                );
            }
        }

        let mut handles = Vec::with_capacity(selection.subscriptions.len());
        for subscription in selection.subscriptions {
            let is_funding_topic = matches!(
                subscription.topic_kind,
                NlnProgramStreamCaptureTopic::SystemTransfers
            );
            let is_trade_topic = matches!(
                subscription.topic_kind,
                NlnProgramStreamCaptureTopic::PumpFunTrade
            );
            let topic_event_bus_tx = if is_funding_topic || is_trade_topic {
                event_bus_tx.clone()
            } else {
                None
            };
            let topic_health = if is_funding_topic {
                health.clone()
            } else {
                None
            };
            let topic_authoritative_funding_stream_tx = is_funding_topic
                .then(|| authoritative_funding_stream_tx.clone())
                .flatten();
            handles.push(tokio::spawn(run_nln_program_streams_topic_capture(
                client.clone(),
                config.clone(),
                subscription.topic,
                subscription.topic_kind,
                topic_event_bus_tx,
                topic_health,
                artifact_writer.clone(),
                is_trade_topic.then(|| trade_resolver.clone()),
                topic_authoritative_funding_stream_tx,
            )));
        }

        for handle in handles {
            let _ = handle.await;
        }

        warn!("Seer: NLN Program Streams FSC capture lane exited");
        if let Some(tx) = authoritative_funding_stream_tx.as_ref() {
            let _ = tx.send(false);
        }
    }))
}

/// Run the Seer component
pub async fn run(
    config: SeerComponentConfig,
    mut shutdown_rx: broadcast::Receiver<()>,
    event_bus_tx: Option<EventBusSender>,
    snapshot_engine: Option<Arc<SnapshotEngine>>,
    shadow_ledger: Option<Arc<ShadowLedger>>,
    wal: Option<Arc<Wal>>,
    paradox_tx: Option<
        tokio::sync::oneshot::Sender<
            tokio::sync::watch::Receiver<seer::paradox_sensor::ParadoxState>,
        >,
    >,
    health: Option<Arc<RuntimeHealth>>,
    authoritative_funding_stream_tx: Option<watch::Sender<bool>>,
    canonical_account_update_relay_enabled: bool,
) -> Result<()> {
    info!("Seer: Initializing component");

    if snapshot_engine.is_some() {
        info!("Seer: 📸 SnapshotEngine integration enabled");
    }

    // Convert launcher config to Seer config
    let program_streams_artifact_config =
        NlnArtifactCaptureConfig::from_launcher(&config.program_streams);

    // Determine source mode first, checking both specific source_mode and legacy connection_mode
    let derived_source_mode = if let Some(mode) = &config.source_mode {
        match mode.to_lowercase().as_str() {
            "grpc" => Some(SeerSourceMode::GeyserGrpc),
            "geyser_grpc" => Some(SeerSourceMode::GeyserGrpc),
            "websocket" | "ws" => Some(SeerSourceMode::GeyserWebSocket),
            "geyser_websocket" => Some(SeerSourceMode::GeyserWebSocket),
            "helius_websocket" => Some(SeerSourceMode::HeliusWebSocket),
            "pump_portal_ws" => Some(SeerSourceMode::PumpPortalWs),
            _ => {
                warn!(
                    "Unknown source_mode '{}', will derive from connection_mode",
                    mode
                );
                None
            }
        }
    } else {
        // Fallback to inferring from connection_mode for backward compatibility
        match config.connection_mode.to_lowercase().as_str() {
            "helius_websocket" => Some(SeerSourceMode::HeliusWebSocket),
            _ => None,
        }
    };
    let funding_lane_mode = match config.funding_lane_mode.to_lowercase().as_str() {
        "disabled" => FundingLaneMode::Disabled,
        "pump_filtered" => FundingLaneMode::PumpFiltered,
        "full_chain" => FundingLaneMode::FullChain,
        other => {
            warn!(
                "Unknown seer funding_lane_mode='{}' — defaulting to disabled",
                other
            );
            FundingLaneMode::Disabled
        }
    };
    let program_streams_format = match config.program_streams.format.to_ascii_uppercase().as_str() {
        "JSON" => ProgramStreamPayloadFormat::Json,
        other => {
            warn!(
                "Unknown seer.program_streams.format='{}' - defaulting to JSON",
                other
            );
            ProgramStreamPayloadFormat::Json
        }
    };

    let seer_config = SeerConfig {
        connection_mode: match config.connection_mode.to_lowercase().as_str() {
            "websocket" | "ws" | "helius_websocket" => ConnectionMode::WebSocket,
            "grpc" | "g" => ConnectionMode::Grpc,
            _ => ConnectionMode::Grpc,
        },
        source_mode: derived_source_mode,
        geyser_endpoint: config.geyser_endpoint,
        grpc_endpoint: config.grpc_endpoint,
        helius_endpoint: config.helius_endpoint,
        rpc_endpoint: config.rpc_endpoint,
        grpc_manual_backfill_enabled: config.grpc_manual_backfill_enabled,
        grpc_client_id: config.grpc_client_id,
        // Use grpc_x_token if provided, otherwise fall back to grpc_auth_token
        // grpc_x_token is the preferred way to authenticate with Yellowstone
        grpc_auth_token: config.grpc_x_token.or(config.grpc_auth_token),
        grpc_auth_header: config.grpc_auth_header,
        max_reconnect_attempts: 10,
        reconnect_delay_secs: 5,
        max_reconnect_delay_secs: 300,
        grpc_max_stalls_before_open: config.grpc_max_stalls_before_open,
        grpc_stall_timeout_secs: config.grpc_stall_timeout_secs,
        grpc_circuit_breaker_cooldown_ms: config.grpc_circuit_breaker_cooldown_ms,
        verbose: false,
        filter: FilterConfig {
            enable_pumpfun: config.enable_pumpfun,
            enable_bonkfun: config.enable_bonkfun,
            allowed_quote_mints: Vec::new(),
            min_initial_liquidity_sol: None,
        },
        channel_buffer_size: config.ipc_buffer_size,
        ipc_config: IpcChannelConfig {
            buffer_size: config.ipc_buffer_size,
            backpressure_policy: match config.ipc_backpressure_policy.to_lowercase().as_str() {
                "block" => BackpressurePolicy::Block,
                "dropoldest" | "drop_oldest" => BackpressurePolicy::DropOldest,
                "dropnew" | "drop_new" => BackpressurePolicy::DropNew,
                "dropbypriority" | "drop_by_priority" => BackpressurePolicy::DropByPriority,
                _ => BackpressurePolicy::Block,
            },
            log_drops: true,
            log_overflows: true,
            warning_threshold_percent: 80.0,
        },
        metrics_port: config.metrics_port,
        ultrafast_enter_threshold: 80.0,
        ultrafast_exit_threshold: 50.0,
        commitment: map_launcher_commitment(config.commitment),
        grpc_commitment_fallback_to_websocket: config.grpc_commitment_fallback_to_websocket,
        stream_mode: match config.stream_mode.to_lowercase().as_str() {
            "pooled_filtered" => StreamMode::PooledFiltered,
            _ => StreamMode::SingleGlobal,
        },
        tx_filter_strategy: match config.tx_filter_strategy.to_lowercase().as_str() {
            "all" => TxFilterStrategy::All,
            _ => TxFilterStrategy::PerPool,
        },
        funding_lane_mode,
        program_streams: ProgramStreamsConfig {
            enabled: config.program_streams.enabled,
            endpoint: resolve_program_streams_endpoint(&config.program_streams.endpoint),
            auth_header: config.program_streams.auth_header.clone(),
            api_key_env: config.program_streams.api_key_env.clone(),
            api_key_env_fallback: config.program_streams.api_key_env_fallback.clone(),
            eventstream_policy_header: config.program_streams.eventstream_policy_header.clone(),
            format: program_streams_format,
            max_streams: config.program_streams.max_streams,
            quota_policy: match config.program_streams.quota_policy {
                LauncherProgramStreamsQuotaPolicy::DropOptional => {
                    SeerProgramStreamsQuotaPolicy::DropOptional
                }
                LauncherProgramStreamsQuotaPolicy::FailFast => {
                    SeerProgramStreamsQuotaPolicy::FailFast
                }
            },
            enabled_topics: config.program_streams.enabled_topics.clone(),
            optional_topics: config.program_streams.optional_topics.clone(),
            disabled_optional_topics: config.program_streams.disabled_optional_topics.clone(),
            disabled_streams: config.program_streams.disabled_streams.clone(),
            pumpfun_create_topic: config.program_streams.pumpfun_create_topic.clone(),
            pumpfun_trade_topic: config.program_streams.pumpfun_trade_topic.clone(),
            pumpfun_buy_topic: config.program_streams.pumpfun_buy_topic.clone(),
            pumpfun_buy_exact_sol_in_topic: config
                .program_streams
                .pumpfun_buy_exact_sol_in_topic
                .clone(),
            system_transfers_topic: config.program_streams.system_transfers_topic.clone(),
            artifact_capture_dir: config.program_streams.artifact_capture_dir.clone(),
            trade_resolver_ttl_ms: config.program_streams.trade_resolver_ttl_ms,
            trade_resolver_per_mint_cap: config.program_streams.trade_resolver_per_mint_cap,
            trade_resolver_global_cap: config.program_streams.trade_resolver_global_cap,
            trade_dedupe_ttl_ms: config.program_streams.trade_dedupe_ttl_ms,
            trade_dedupe_max_entries: config.program_streams.trade_dedupe_max_entries,
            transfer_dedupe_ttl_ms: config.program_streams.transfer_dedupe_ttl_ms,
            transfer_dedupe_max_entries: config.program_streams.transfer_dedupe_max_entries,
        },
        watched_pools_ttl_ms: config.watched_pools_ttl_ms,
        watched_pools_cap: config.watched_pools_cap,
        watch_debounce_ms: config.watch_debounce_ms,
        canonical_account_update_relay_enabled,
        pumpportal: PumpPortalConfig {
            ws_url: config.pumpportal.ws_url.clone(),
            max_active_mints: config.pumpportal.max_active_mints,
            subscription_batch_size: config.pumpportal.subscription_batch_size,
            reconnect_base_delay_secs: config.pumpportal.reconnect_base_delay_secs,
            reconnect_max_delay_secs: config.pumpportal.reconnect_max_delay_secs,
            stats_window_secs: config.pumpportal.stats_window_secs,
        },
    };
    let program_streams_capture_config = seer_config.program_streams.clone();
    let nln_trade_pool_resolver = Arc::new(Mutex::new(
        NlnTradePoolIdentityResolver::from_program_streams_config(&program_streams_capture_config),
    ));

    info!("Seer: Configuration loaded");
    info!(
        "  Effective source mode: {:?}",
        seer_config.effective_source_mode()
    );
    info!(
        "  gRPC endpoint: {}",
        redact_endpoint_for_logs(&seer_config.grpc_endpoint)
    );
    info!(
        "  RPC endpoint: {}",
        redact_endpoint_for_logs(&seer_config.rpc_endpoint)
    );
    info!(
        "  grpc_manual_backfill_enabled: {}",
        seer_config.grpc_manual_backfill_enabled
    );
    info!(
        "  grpc_stall_timeout_secs: {}",
        seer_config.grpc_stall_timeout_secs
    );
    info!(
        "  grpc_commitment_fallback_to_websocket: {}",
        seer_config.grpc_commitment_fallback_to_websocket
    );
    info!("  stream_mode: {:?}", seer_config.stream_mode);
    info!("  tx_filter_strategy: {:?}", seer_config.tx_filter_strategy);
    info!(
        "  funding_lane_mode: {}",
        seer_config.funding_lane_mode.as_str()
    );
    info!("  commitment: {}", seer_config.commitment.as_str());
    info!(
        "  watched_pools: ttl_ms={} cap={} debounce_ms={}",
        seer_config.watched_pools_ttl_ms,
        seer_config.watched_pools_cap,
        seer_config.watch_debounce_ms
    );

    // Log PumpPortal config when in PumpPortal mode
    if matches!(
        seer_config.effective_source_mode(),
        seer::config::SeerSourceMode::PumpPortalWs
    ) {
        info!("  PumpPortal WS URL: {}", seer_config.pumpportal.ws_url);
        info!(
            "  PumpPortal max active mints: {}",
            seer_config.pumpportal.max_active_mints
        );
        info!(
            "  PumpPortal subscription batch size: {}",
            seer_config.pumpportal.subscription_batch_size
        );
    }
    info!(
        "  gRPC auth: {}",
        if seer_config.grpc_auth_token.is_some() {
            "ENABLED (will be sent with every streaming message)"
        } else {
            "DISABLED"
        }
    );

    // Create IPC channel for candidate forwarding
    let (ipc_sender, mut ipc_receiver, ipc_metrics) =
        create_ipc_channel(seer_config.ipc_config.clone());

    // Create Seer instance (optionally with ShadowLedger for live curve updates)
    let mut seer_instance = match shadow_ledger {
        Some(ledger) => {
            Seer::new_with_ipc_and_shadow_ledger(seer_config.clone(), ipc_sender, ledger)
        }
        None => Seer::new_with_ipc(seer_config.clone(), ipc_sender),
    };

    // Wire RuntimeHealth into Seer → GrpcConnection for gRPC heartbeats
    if let Some(ref h) = health {
        seer_instance.set_health(Arc::clone(h));
    }

    if let Some(wal) = wal {
        seer_instance = seer_instance.with_wal(wal);
    }

    let nln_authoritative_funding_stream_tx = authoritative_funding_stream_tx.clone();
    if let Some(tx) = authoritative_funding_stream_tx {
        if seer_instance.set_authoritative_funding_stream_availability_sender(tx) {
            info!(
                "Seer: FSC authoritative funding availability signal wired (funding_lane_mode={})",
                seer_config.funding_lane_mode.as_str()
            );
        } else {
            info!(
                "Seer: FSC authoritative funding availability remains fail-closed (funding_lane_mode={})",
                seer_config.funding_lane_mode.as_str()
            );
        }
    }

    let seer = Arc::new(seer_instance);

    // Get Paradox Sensor state receiver and send it back to caller if requested
    if let Some(paradox_rx) = seer.paradox_state_receiver() {
        info!("Seer: 🔮 Paradox Sensor state receiver available for HFT detection");
        if let Some(tx) = paradox_tx {
            let _ = tx.send(paradox_rx);
            info!("Seer: 🔮 Paradox Sensor state sent to OracleRuntime");
        }
    } else {
        warn!("Seer: 🔮 Paradox Sensor state receiver is None - HFT detection disabled");
    }

    // Start Seer event loop
    let seer_handle = {
        let seer = Arc::clone(&seer);
        tokio::spawn(async move {
            loop {
                info!("Seer: Starting event processing loop");
                match Arc::clone(&seer).run().await {
                    Ok(()) => {
                        info!("Seer: Event loop ended normally");
                        break;
                    }
                    Err(e) => {
                        error!(
                            "Seer: Error in event loop: {}. Restarting in 10 seconds...",
                            e
                        );
                        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                    }
                }
            }
        })
    };

    let nln_program_streams_handle = spawn_nln_program_streams_capture(
        program_streams_capture_config,
        program_streams_artifact_config,
        event_bus_tx.clone(),
        health.clone(),
        nln_authoritative_funding_stream_tx,
        nln_trade_pool_resolver.clone(),
    );

    // Process IPC events and emit to event bus
    let health_ipc = health.clone();
    let nln_trade_pool_resolver_ipc = nln_trade_pool_resolver.clone();
    let ipc_handle = tokio::spawn(async move {
        let detected_pool_ttl = Duration::from_millis(seer_config.watched_pools_ttl_ms.max(1));
        let detected_pool_cap = seer_config.watched_pools_cap.max(1);
        let mut session_trade_bridge = SessionPoolTradeBridge::from_runtime_config(
            SESSION_POOL_TRADE_BUFFER_TTL,
            detected_pool_ttl,
            detected_pool_cap,
        );
        let mut session_account_update_bridge =
            SessionAccountUpdateBridge::from_runtime_config(detected_pool_ttl, detected_pool_cap);
        let mut prune_interval = tokio::time::interval(session_bridge_prune_interval(
            SESSION_POOL_TRADE_BUFFER_TTL,
            detected_pool_ttl,
        ));
        info!("Seer: Starting IPC event processing");
        info!("Seer: IPC receiver task is now listening for pool detection events from Seer core");

        loop {
            let seer_event = tokio::select! {
                _ = prune_interval.tick() => {
                    let (expired_pending, expired_detected) =
                        session_trade_bridge.prune_expired(Instant::now());
                    record_session_buffer_expired(expired_pending);
                    record_session_detected_pool_expired(expired_detected);
                    let (expired_updates, expired_update_keys, _expired_evidence) =
                        session_account_update_bridge.prune_expired(Instant::now());
                    record_session_account_update_expired(expired_updates);
                    record_session_account_update_detected_key_expired(expired_update_keys);
                    continue;
                }
                maybe_event = ipc_receiver.recv() => match maybe_event {
                    Some(event) => event,
                    None => break,
                }
            };

            // Mark IPC heartbeat on every received event
            if let Some(ref h) = health_ipc {
                h.mark_ipc_event();
            }

            match seer_event {
                seer::ipc::SeerEvent::PoolDetected(event) => {
                    let candidate = &event.candidate;

                    info!(
                        "Seer: Pool detected via IPC - pool={}, amm={}, priority={:?}",
                        candidate.pool_amm_id, candidate.amm_program_id, event.priority
                    );

                    // Use event.detected_at for true IPC latency
                    // (time from IPC event creation to consumption), not
                    // candidate.timestamp which is in seconds and gives
                    // false ~500-600ms readings.
                    let ipc_latency_ms = event
                        .detected_at
                        .elapsed()
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    let detected_ms = event
                        .detected_at
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    let clock_summary = detection_clock_summary(&candidate, detected_ms);

                    info!(
                        "Seer: 🕒 Pool detection latency={}ms pool={} base_mint={} slot={:?} compat_event_ts_ms={} effective_event_ts_ms={:?} chain_event_ts_ms={:?} detected_wall_ts_ms={} has_explicit_event_time={} ingest_latency_ms={} source=ipc",
                        ipc_latency_ms,
                        candidate.pool_amm_id,
                        candidate.base_mint,
                        candidate.slot,
                        clock_summary.compat_event_ts_ms,
                        clock_summary.effective_event_ts_ms,
                        clock_summary.chain_event_ts_ms,
                        detected_ms,
                        clock_summary.has_explicit_event_time,
                        clock_summary.ingest_latency_ms
                    );

                    // [LAST-GATE VALIDATION] Final invariant checks before emission/bootstrap
                    // This is the last line of defense against invalid data propagating downstream
                    let pumpfun_global_state_str = "TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM";

                    let base_mint_str = candidate.base_mint.to_string();
                    let bonding_curve_str = candidate.bonding_curve.to_string();
                    let amm_program_str = candidate.amm_program_id.to_string();
                    let pool_amm_id_str = candidate.pool_amm_id.to_string();

                    // Invariant 1: base_mint must NEVER be the program ID
                    if is_known_pump_fun_program_id(&base_mint_str) {
                        error!(
                            "🚨 LAST-GATE REJECT: base_mint equals Pump.fun program ID | \
                             source=seer_ipc | signature={} | pool={} | base_mint={} | \
                             bonding_curve={} | amm_program={} | DROPPED",
                            candidate.signature,
                            pool_amm_id_str,
                            base_mint_str,
                            bonding_curve_str,
                            amm_program_str
                        );
                        continue; // Skip this pool - do not emit, do not bootstrap
                    }

                    // Invariant 2: base_mint must NEVER be the global state address
                    if base_mint_str == pumpfun_global_state_str {
                        error!(
                            "🚨 LAST-GATE REJECT: base_mint equals Pump.fun global state | \
                             source=seer_ipc | signature={} | pool={} | base_mint={} | \
                             bonding_curve={} | amm_program={} | DROPPED",
                            candidate.signature,
                            pool_amm_id_str,
                            base_mint_str,
                            bonding_curve_str,
                            amm_program_str
                        );
                        continue;
                    }

                    // Invariant 3: bonding_curve must NEVER be the program ID
                    if is_known_pump_fun_program_id(&bonding_curve_str) {
                        error!(
                            "🚨 LAST-GATE REJECT: bonding_curve equals Pump.fun program ID | \
                             source=seer_ipc | signature={} | pool={} | base_mint={} | \
                             bonding_curve={} | amm_program={} | DROPPED",
                            candidate.signature,
                            pool_amm_id_str,
                            base_mint_str,
                            bonding_curve_str,
                            amm_program_str
                        );
                        continue;
                    }

                    // Invariant 4: bonding_curve must NEVER be the global state address
                    if bonding_curve_str == pumpfun_global_state_str {
                        error!(
                            "🚨 LAST-GATE REJECT: bonding_curve equals Pump.fun global state | \
                             source=seer_ipc | signature={} | pool={} | base_mint={} | \
                             bonding_curve={} | amm_program={} | DROPPED",
                            candidate.signature,
                            pool_amm_id_str,
                            base_mint_str,
                            bonding_curve_str,
                            amm_program_str
                        );
                        continue;
                    }

                    // Invariant 5: base_mint must NEVER equal amm_program (field swap detection)
                    if base_mint_str == amm_program_str {
                        error!(
                            "🚨 LAST-GATE REJECT: base_mint equals amm_program (field swap) | \
                             source=seer_ipc | signature={} | pool={} | base_mint={} | \
                             bonding_curve={} | amm_program={} | DROPPED",
                            candidate.signature,
                            pool_amm_id_str,
                            base_mint_str,
                            bonding_curve_str,
                            amm_program_str
                        );
                        continue;
                    }

                    // Invariant 6: base_mint should not equal bonding_curve (fallback bug detection)
                    // This is usually a sign of incorrect fallback logic
                    if base_mint_str == bonding_curve_str {
                        warn!(
                            "⚠️  LAST-GATE WARNING: base_mint equals bonding_curve (possible fallback) | \
                             source=seer_ipc | signature={} | pool={} | base_mint={} | \
                             bonding_curve={} | amm_program={} | ALLOWED",
                            candidate.signature, pool_amm_id_str, base_mint_str,
                            bonding_curve_str, amm_program_str
                        );
                        // Allow but log - this can be legitimate in some edge cases
                    }

                    // Bootstrap SnapshotEngine only after final invariants pass.
                    if let Some(ref engine) = snapshot_engine {
                        engine.track_pool(candidate.pool_amm_id);
                        let init_event = InitPoolEvent {
                            pool_amm_id: candidate.pool_amm_id,
                            base_mint: candidate.base_mint,
                            quote_mint: candidate.quote_mint,
                            slot: candidate.slot,
                            timestamp_ms: clock_summary.compat_event_ts_ms,
                            initial_liquidity_sol: candidate.initial_liquidity_sol.unwrap_or(0.0),
                            initial_reserve_base: 0.0,
                            initial_reserve_quote: candidate.initial_liquidity_sol.unwrap_or(0.0),
                            initial_price_quote: 0.0,
                        };

                        engine.handle_initialize_pool_event(&init_event);
                        info!(
                            "Seer: 📸 Bootstrapped SnapshotEngine for pool={}",
                            candidate.pool_amm_id
                        );
                    }

                    // Emit to unified event bus if available
                    if let Some(ref tx) = event_bus_tx {
                        let now = Instant::now();
                        process_pool_detected_event_for_session_gate(
                            tx,
                            &mut session_trade_bridge,
                            candidate,
                            health_ipc.as_ref(),
                            now,
                            detected_ms,
                        );
                        let nln_replay_ready = match nln_trade_pool_resolver_ipc.lock() {
                            Ok(mut resolver) => {
                                let result = resolver.register_candidate(candidate, now);
                                if result.expired_count > 0 {
                                    ::metrics::counter!(
                                        "nln_trade_unresolved_after_ttl",
                                        result.expired_count as u64
                                    );
                                }
                                if result.collision {
                                    warn!(
                                        mint = %candidate.base_mint,
                                        pool = %candidate.pool_amm_id,
                                        bonding_curve = %candidate.bonding_curve,
                                        "Seer: NLN trade pool identity collision; trades for mint will not be forwarded"
                                    );
                                }
                                result.replay_ready
                            }
                            Err(err) => {
                                warn!(
                                    error = %err,
                                    "Seer: NLN trade pool identity resolver lock poisoned"
                                );
                                Vec::new()
                            }
                        };
                        for nln_trade in nln_replay_ready {
                            let trade_event = nln_trade.to_trade_event(candidate.pool_amm_id);
                            emit_pool_transaction_to_event_bus(
                                tx,
                                &trade_event,
                                health_ipc.as_ref(),
                                true,
                            );
                            ::metrics::counter!("nln_trade_forwarded_pool_transaction", 1);
                            increment_counter!("nln_trade_forwarded_pool_transaction_total");
                            info!(
                                mint = %nln_trade.mint,
                                pool = %candidate.pool_amm_id,
                                signature = %nln_trade.signature,
                                slot = nln_trade.slot,
                                side = ?nln_trade.side,
                                resolver_action = "replay_after_candidate",
                                "Seer: NLN pumpfun.trade replayed to PoolTransaction after Ghost birth"
                            );
                        }
                        let flush = session_account_update_bridge
                            .register_detected_pool(candidate, Instant::now());
                        record_session_account_update_expired(flush.expired_count);
                        record_session_account_update_detected_key_expired(
                            flush.expired_detected_keys,
                        );
                        if !flush.replay_ready.is_empty() {
                            ::metrics::counter!(
                                "seer_bridge_session_account_update_replayed_total",
                                flush.replay_ready.len() as u64
                            );
                            for update in &flush.replay_ready {
                                emit_account_update_to_event_bus(
                                    tx,
                                    update,
                                    health_ipc.as_ref(),
                                    true,
                                );
                            }
                        }
                        for evidence in &flush.replay_ready_evidence {
                            emit_execution_account_evidence_to_event_bus(
                                tx,
                                evidence,
                                health_ipc.as_ref(),
                            );
                        }
                    } else {
                        let flush_result = session_trade_bridge
                            .register_detected_pool(candidate.pool_amm_id, Instant::now());
                        if let Ok(mut resolver) = nln_trade_pool_resolver_ipc.lock() {
                            let _ = resolver.register_candidate(candidate, Instant::now());
                        }
                        record_session_buffer_expired(flush_result.expired_count);
                        record_session_detected_pool_expired(flush_result.expired_detected_pools);
                        record_session_detected_pool_evicted(flush_result.evicted_detected_pools);
                        let flush = session_account_update_bridge
                            .register_detected_pool(candidate, Instant::now());
                        record_session_account_update_expired(flush.expired_count);
                        record_session_account_update_detected_key_expired(
                            flush.expired_detected_keys,
                        );
                    }

                    // Log metrics periodically
                    if event.sequence_number % 100 == 0 {
                        let drop_rate = ipc_metrics.calculate_drop_rate();
                        let queue_util = ipc_metrics.calculate_queue_utilization(10000);
                        info!(
                            "Seer: IPC metrics - queue_util={:.1}%, drop_rate={:.2}%",
                            queue_util, drop_rate
                        );
                    }
                }

                seer::ipc::SeerEvent::Trade(trade_event) => {
                    let trade = &trade_event.trade;

                    if !trade_has_forwardable_identity(trade) {
                        warn!(
                            "Seer: dropping unresolved trade before Event Bus bridge sig={} pool={} mint={} event_ordinal={:?}",
                            trade.signature,
                            trade.pool_amm_id,
                            trade.mint,
                            trade.event_ordinal
                        );
                        continue;
                    }

                    // --- Canonical Bridge: Seer TradeEvent → Shadow Ledger PoolTransaction ---
                    // This is the single, explicit adapter that maps Seer parsed trade semantics
                    // into the canonical PoolTransaction input model consumed by the Shadow Ledger
                    // runtime flow (Gatekeeper pre-commit / LivePipeline post-commit).
                    //
                    // Seer is the canonical transaction PRODUCER.
                    // Shadow Ledger (via Gatekeeper + LivePipeline) is the authoritative curve-state CONSUMER.
                    //
                    // NOTE: Log only forwarded trades (ForwardNow). Pools born before session
                    // startup are SilentDrop — logging before the gate check would spam hundreds
                    // of thousands of INFO lines per minute for pools we will never observe.
                    if let Some(ref tx) = event_bus_tx {
                        let now = Instant::now();
                        let gate = process_trade_event_for_session_gate(
                            tx,
                            &mut session_trade_bridge,
                            trade,
                            health_ipc.as_ref(),
                            now,
                        );
                        if gate.decision == SessionTradeDecision::ForwardNow {
                            let liveness =
                                session_account_update_bridge.refresh_from_trade(trade, now);
                            record_session_account_update_expired(liveness.expired_count);
                            record_session_account_update_detected_key_expired(
                                liveness.expired_detected_keys,
                            );
                            for evidence in &liveness.replay_ready_evidence {
                                emit_execution_account_evidence_to_event_bus(
                                    tx,
                                    evidence,
                                    health_ipc.as_ref(),
                                );
                            }
                            let ipc_volume_sol = if trade.is_buy {
                                trade.max_sol_cost as f64 / 1_000_000_000.0
                            } else {
                                trade.min_sol_output as f64 / 1_000_000_000.0
                            };
                            info!(
                                "Seer: 🔄 Trade detected via IPC - {} on pool={}, mint={}, sol_volume={:.6} SOL, token_amount={:.6}, signer={}",
                                if trade.is_buy { "BUY" } else { "SELL" },
                                trade.pool_amm_id,
                                trade.mint,
                                ipc_volume_sol,
                                trade.amount as f64 / 1_000_000.0,
                                trade.signer
                            );
                        }
                    }
                }

                seer::ipc::SeerEvent::FundingTransfer(funding_event) => {
                    if let Some(ref tx) = event_bus_tx {
                        emit_funding_transfer_to_event_bus(tx, &funding_event, health_ipc.as_ref());
                    }
                }

                seer::ipc::SeerEvent::ExecutionAccountEvidence(evidence_event) => {
                    if let Some(ref tx) = event_bus_tx {
                        let ingress = session_account_update_bridge
                            .ingest_execution_account_evidence(&evidence_event, Instant::now());
                        record_session_account_update_expired(ingress.expired_count);
                        record_session_account_update_detected_key_expired(
                            ingress.expired_detected_keys,
                        );
                        match ingress.decision {
                            SessionExecutionAccountEvidenceDecision::ForwardNow => {
                                emit_execution_account_evidence_to_event_bus(
                                    tx,
                                    &evidence_event,
                                    health_ipc.as_ref(),
                                );
                            }
                            SessionExecutionAccountEvidenceDecision::BufferedUntilPoolDetected => {}
                            SessionExecutionAccountEvidenceDecision::SilentDrop => {
                                increment_counter!(
                                    "seer_bridge_session_bcv2_evidence_silent_drop_total"
                                );
                            }
                        }
                    }
                }

                // ── Live AccountUpdate canonical ingest wiring ────────────────
                // This boolean is the launcher-derived effective runtime state,
                // not a primary production config selector. When true, Seer
                // forwards canonical reserve snapshots to OracleRuntime so
                // AccountStateCore remains hydrated in real time.
                //
                // When false, we are in explicit degraded/test compatibility
                // startup and the canonical AccountUpdate relay is intentionally
                // suppressed end-to-end.
                seer::ipc::SeerEvent::AccountUpdate(au) => {
                    if canonical_account_update_relay_enabled {
                        if let Some(ref tx) = event_bus_tx {
                            let ingress = session_account_update_bridge
                                .ingest_account_update(&au, Instant::now());
                            record_session_account_update_expired(ingress.expired_count);
                            record_session_account_update_detected_key_expired(
                                ingress.expired_detected_keys,
                            );
                            record_session_account_update_evictions(
                                ingress.evicted_per_key,
                                ingress.evicted_global,
                            );
                            match ingress.decision {
                                SessionAccountUpdateDecision::ForwardNow => {
                                    emit_account_update_to_event_bus(
                                        tx,
                                        &au,
                                        health_ipc.as_ref(),
                                        false,
                                    );
                                }
                                SessionAccountUpdateDecision::BufferedUntilPoolDetected => {
                                    ::metrics::counter!(
                                        "seer_bridge_session_account_update_buffered_total",
                                        1u64
                                    );
                                }
                                SessionAccountUpdateDecision::SilentDrop => {
                                    increment_counter!(
                                        "seer_bridge_session_account_update_silent_drop_total"
                                    );
                                }
                            }
                        }
                    }
                    // degraded/test compatibility: silently drop — no
                    // ShadowLedger writes happen in Seer, so there is no local
                    // reconciliation side effect.
                }
            }
        }

        warn!("Seer: IPC receiver task has exited - no more pool events will be processed!");
        warn!("Seer: This usually means the Seer core component has stopped or the IPC channel closed");
    });

    // Wait for shutdown signal
    let _ = shutdown_rx.recv().await;
    info!("Seer: Shutdown signal received");

    // Cancel tasks
    seer_handle.abort();
    ipc_handle.abort();
    if let Some(handle) = nln_program_streams_handle {
        handle.abort();
    }

    info!("Seer: Component stopped");
    Ok(())
}

/// Canonical bridge: maps a Seer-parsed `TradeEvent` into the `PoolTransaction` input
/// model consumed by the Shadow Ledger runtime flow.
///
/// This is the **single, explicit adapter** between Seer's ingress/parser role and the
/// Shadow Ledger's authoritative curve-state evolution path (Gatekeeper pre-commit +
/// LivePipeline post-commit).
///
/// ## Semantic contract
/// - Seer is the canonical **transaction producer**: parse, dedup, ordering metadata,
///   mint/pool/curve mapping, and event production.
/// - Shadow Ledger is the authoritative **curve-state machine**: it consumes the
///   `PoolTransaction` produced here via `forward_approved_tx_to_commit_or_live_pipeline`.
///
/// ## Fields preserved
/// | `TradeEvent` source        | `PoolTransaction` field          |
/// |----------------------------|----------------------------------|
/// | `mint`                     | `token_mint`                     |
/// | `event_ordinal`            | `event_ordinal`                  |
/// | `provenance.*`             | execution provenance optionals   |
/// | `timestamp_ms`, `slot`     | `timestamp_ms`, `slot`           |
/// | `arrival_ts_ms`            | `arrival_ts_ms`                  |
/// | `is_buy`                   | `is_buy`                         |
/// | `max_sol_cost`/`min_sol_output` | `sol_amount_lamports`       |
/// | `amount`                   | `token_amount_units`             |
/// | `signer`                   | `signer`                         |
/// | `is_dev_buy`               | `is_dev_buy`, `dev_buy_lamports` |
/// | `v_tokens_*`, `v_sol_*`    | `reserve_base`, `reserve_quote`  |
/// | `signature`                | `signature`                      |
pub fn trade_event_to_pool_transaction(
    trade: &seer::types::TradeEvent,
) -> crate::events::PoolTransaction {
    let sol_amount_lamports = if trade.is_buy {
        trade.max_sol_cost
    } else {
        trade.min_sol_output
    };
    let volume_sol = sol_amount_lamports as f64 / 1_000_000_000.0;

    crate::events::PoolTransaction {
        semantic: trade.semantic,
        pool_amm_id: trade.pool_amm_id.to_string(),
        slot: trade.slot,
        event_ordinal: trade.event_ordinal,
        tx_index: trade.tx_index,
        outer_instruction_index: trade
            .provenance
            .as_ref()
            .and_then(|value| value.outer_instruction_index),
        inner_group_index: trade
            .provenance
            .as_ref()
            .and_then(|value| value.inner_group_index),
        outer_program_id: trade
            .provenance
            .as_ref()
            .and_then(|value| value.outer_program_id.clone()),
        cpi_stack_height: trade
            .provenance
            .as_ref()
            .and_then(|value| value.stack_height),
        timestamp_ms: trade.timestamp_ms,
        event_time: trade.event_time,
        arrival_ts_ms: trade.arrival_ts_ms,
        signer: trade.signer.to_string(),
        is_buy: trade.is_buy,
        volume_sol,
        sol_amount_lamports: Some(sol_amount_lamports),
        token_amount_units: Some(trade.amount),
        reserve_base: trade.v_tokens_in_bonding_curve,
        reserve_quote: trade.v_sol_in_bonding_curve,
        price_quote: match (
            trade.v_tokens_in_bonding_curve,
            trade.v_sol_in_bonding_curve,
        ) {
            (Some(tokens), Some(sol)) if tokens > 0.0 => Some(sol / tokens),
            _ => None,
        },
        is_dev_buy: trade.is_dev_buy,
        dev_buy_lamports: if trade.is_dev_buy {
            sol_amount_lamports
        } else {
            0
        },
        signature: trade.signature.to_string(),
        success: trade.success,
        error_code: trade.error_code.clone(),
        compute_units_consumed: trade.compute_units_consumed,
        owner_token_deltas: trade.owner_token_deltas.clone(),
        mpcf_payload: trade.mpcf_payload.clone(),
        mpcf_payload_missing_reason: trade.mpcf_payload_missing_reason,
        token_mint: (trade.mint != Pubkey::default()).then(|| trade.mint.to_string()),
        v_tokens_in_bonding_curve: trade.v_tokens_in_bonding_curve,
        v_sol_in_bonding_curve: trade.v_sol_in_bonding_curve,
        market_cap_sol: trade.market_cap_sol,
        global_config: trade.global_config.map(|value| value.to_string()),
        fee_recipient: trade.fee_recipient.map(|value| value.to_string()),
        token_program: trade.token_program.map(|value| value.to_string()),
        buy_variant: trade.buy_variant.clone(),
        associated_bonding_curve: trade
            .associated_bonding_curve
            .map(|value| value.to_string()),
        creator_vault: trade.creator_vault.map(|value| value.to_string()),
        bonding_curve_v2: trade.bonding_curve_v2.map(|value| value.to_string()),
        bonding_curve_v2_provenance: trade.bonding_curve_v2_provenance.as_ref().map(|value| {
            crate::events::ObservedAccountMetaProvenance {
                source_tx_signature: value.source_tx_signature.clone(),
                source_slot: value.source_slot,
                source_slot_index: value.source_slot_index,
                source_instruction_index: value.source_instruction_index,
                source_program_id: value.source_program_id.clone(),
                source_discriminator: value.source_discriminator.clone(),
                source_buy_variant: value.source_buy_variant.clone(),
                instruction_account_position: value.instruction_account_position,
                message_account_index: value.message_account_index,
                resolved_pubkey: value.resolved_pubkey.clone(),
                loaded_address_source: value.loaded_address_source.clone(),
                tx_success: value.tx_success,
                meta_err: value.meta_err.clone(),
                provenance_status: value.provenance_status.clone(),
            }
        }),
        buy_remaining_accounts: trade
            .buy_remaining_accounts
            .iter()
            .map(ToString::to_string)
            .collect(),
        is_mayhem_mode: trade.is_mayhem_mode,
        cu_price_micro_lamports: trade.cu_price_micro_lamports,
        compute_unit_limit: trade.compute_unit_limit,
        inner_ix_count: trade.inner_ix_count,
        cpi_depth: trade.cpi_depth,
        ata_create_count: trade.ata_create_count,
        signer_pre_balance_lamports: trade.signer_pre_balance_lamports,
        signer_post_balance_lamports: trade.signer_post_balance_lamports,
        jito_tip_detected: trade.jito_tip_detected,
        toolchain_fingerprint: trade.toolchain_fingerprint.clone(),
        curve_data_known: trade.curve_data_known,
        curve_finality: trade.curve_finality,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        detected_pool_from_candidate, detection_clock_summary,
        emit_execution_account_evidence_to_event_bus, emit_funding_transfer_to_event_bus,
        nln_normalization_error_row, nln_route_manifest_evidence_candidate_row,
        process_pool_detected_event_for_session_gate, process_trade_event_for_session_gate,
        pumpswap_program_id, select_nln_program_stream_subscriptions,
        trade_event_to_pool_transaction, trade_has_forwardable_identity,
        NlnProgramStreamCaptureTopic, NlnTradePoolIdentityResolver, NlnTradeResolveDecision,
        SessionAccountUpdateBridge, SessionAccountUpdateDecision, SessionBcv2Context,
        SessionExecutionAccountEvidenceDecision, SessionPoolTradeBridge, SessionTradeDecision,
        TOKEN_PROGRAM_ID,
    };
    use crate::events::{create_event_bus, GhostEvent};
    use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
    use ghost_core::CurveFinality;
    use seer::config::{ProgramStreamsConfig, ProgramStreamsQuotaPolicy};
    use seer::ipc::{
        AccountUpdateReplayOrigin, DetectedAccountUpdateEvent,
        DetectedExecutionAccountEvidenceEvent, DetectedFundingTransferEvent, DetectedPoolEvent,
        DetectedTradeEvent, EventPriority, FundingTransferEvent, SeerEvent,
    };
    use seer::nln_program_streams::{
        NlnIngestMeta, NlnProgramStreamMessage, NlnPumpFunTradeEvent, PumpFunTradeSide,
    };
    use seer::types::{
        CandidatePool, InstructionProvenance, ObservedAccountMetaProvenance, RawBytesMissingReason,
        TradeEvent,
    };
    use serde_json::{json, Value};
    use solana_sdk::{pubkey::Pubkey, signature::Signature};
    use std::str::FromStr;
    use std::time::{Duration, Instant, SystemTime};

    fn make_candidate(pool: Pubkey, mint: Pubkey) -> CandidatePool {
        CandidatePool {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(11),
            tx_index: None,
            event_ts_ms: Some(11_000),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: Signature::new_unique().to_string(),
            amm_program_id: Pubkey::new_unique(),
            pool_amm_id: pool,
            base_mint: mint,
            quote_mint: Pubkey::new_unique(),
            bonding_curve: Pubkey::new_unique(),
            creator: Pubkey::new_unique(),
            timestamp: 11,
            bonding_curve_progress: Some(1.0),
            initial_liquidity_sol: Some(1.5),
            token_total_supply: Some(1_000_000),
            block_time: Some(11),
        }
    }

    fn make_nln_trade(mint: Pubkey, user: Pubkey, signature: Signature) -> NlnPumpFunTradeEvent {
        NlnPumpFunTradeEvent {
            meta: NlnIngestMeta {
                provider: "NLN".to_string(),
                topic: ProgramStreamsConfig::default_pumpfun_trade_topic(),
                partition: 0,
                offset_raw: "1".to_string(),
                offset: Some(1),
                provider_ts_ms: Some(1_700_000_000_000),
                recv_ts_ms: 1_700_000_000_010,
                recv_ts_ns: 1_700_000_000_010_000_000,
                decode_ts_ms: 1_700_000_000_011,
                slot: Some(42),
                signature: Some(signature.to_string()),
                tx_index: Some(3),
                instruction_index: Some(2),
            },
            signature,
            tx_index: Some(3),
            slot: 42,
            mint,
            user,
            creator: None,
            ix_name: Some("buy".to_string()),
            side: PumpFunTradeSide::Buy,
            sol_amount_lamports: 10_000_000,
            token_amount_units: 1_000,
            block_time: Some(1_700_000_000),
            virtual_sol_reserves: Some(30_000_000_000),
            virtual_token_reserves: Some(1_000_000_000_000),
            real_sol_reserves: None,
            real_token_reserves: None,
        }
    }

    async fn recv_only_event(rx: &mut tokio::sync::broadcast::Receiver<GhostEvent>) -> GhostEvent {
        tokio::time::timeout(Duration::from_millis(50), rx.recv())
            .await
            .expect("timed out waiting for event")
            .expect("event bus closed")
    }

    #[test]
    fn test_program_stream_selection_preserves_legacy_three_topics_by_default() {
        let config = ProgramStreamsConfig::default();
        let selection = select_nln_program_stream_subscriptions(&config);

        assert_eq!(selection.allowed_stream_count, 3);
        assert_eq!(selection.requested_topic_count, 3);
        assert!(!selection.required_topics_exceed_limit);
        assert!(!selection.quota_policy_violation);
        assert!(selection.dropped_optional_topics.is_empty());
        assert_eq!(selection.subscriptions.len(), 3);
        assert_eq!(
            selection
                .subscriptions
                .iter()
                .map(|subscription| subscription.topic_kind)
                .collect::<Vec<_>>(),
            vec![
                NlnProgramStreamCaptureTopic::SystemTransfers,
                NlnProgramStreamCaptureTopic::PumpFunTrade,
                NlnProgramStreamCaptureTopic::PumpFunCreate,
            ]
        );
    }

    #[test]
    fn test_program_stream_selection_drops_optional_create_under_two_stream_limit() {
        let config = ProgramStreamsConfig {
            enabled: true,
            max_streams: 2,
            ..ProgramStreamsConfig::default()
        };
        let selection = select_nln_program_stream_subscriptions(&config);

        assert_eq!(selection.allowed_stream_count, 2);
        assert_eq!(selection.requested_topic_count, 3);
        assert!(!selection.quota_policy_violation);
        assert_eq!(
            selection.dropped_optional_topics,
            vec!["prod.rpc.solana.pumpfun.create".to_string()]
        );
        assert_eq!(
            selection
                .subscriptions
                .iter()
                .map(|subscription| subscription.topic_kind)
                .collect::<Vec<_>>(),
            vec![
                NlnProgramStreamCaptureTopic::SystemTransfers,
                NlnProgramStreamCaptureTopic::PumpFunTrade,
            ]
        );
    }

    #[test]
    fn test_program_stream_selection_honors_explicit_fsc_topic_allowlist() {
        let config = ProgramStreamsConfig {
            enabled: true,
            max_streams: 2,
            enabled_topics: vec![
                "prod.rpc.solana.system.transfers".to_string(),
                "prod.rpc.solana.pumpfun.trade".to_string(),
            ],
            disabled_optional_topics: vec!["prod.rpc.solana.pumpfun.create".to_string()],
            ..ProgramStreamsConfig::default()
        };
        let selection = select_nln_program_stream_subscriptions(&config);

        assert_eq!(selection.requested_topic_count, 2);
        assert!(!selection.quota_policy_violation);
        assert!(selection.dropped_optional_topics.is_empty());
        assert_eq!(selection.subscriptions.len(), 2);
        assert!(selection.subscriptions.iter().all(|subscription| {
            subscription.topic_kind != NlnProgramStreamCaptureTopic::PumpFunCreate
        }));
    }

    #[test]
    fn test_program_stream_selection_accepts_route_evidence_topics_only() {
        let config = ProgramStreamsConfig {
            enabled: true,
            max_streams: 2,
            quota_policy: ProgramStreamsQuotaPolicy::FailFast,
            enabled_topics: vec![
                ProgramStreamsConfig::default_pumpfun_buy_topic(),
                ProgramStreamsConfig::default_pumpfun_buy_exact_sol_in_topic(),
            ],
            disabled_streams: vec![
                ProgramStreamsConfig::default_pumpfun_trade_topic(),
                ProgramStreamsConfig::default_system_transfers_topic(),
            ],
            ..ProgramStreamsConfig::default()
        };
        let selection = select_nln_program_stream_subscriptions(&config);

        assert!(!selection.quota_policy_violation);
        assert_eq!(selection.requested_topic_count, 2);
        assert_eq!(
            selection
                .subscriptions
                .iter()
                .map(|subscription| subscription.topic_kind)
                .collect::<Vec<_>>(),
            vec![
                NlnProgramStreamCaptureTopic::PumpFunBuy,
                NlnProgramStreamCaptureTopic::PumpFunBuyExactSolIn,
            ]
        );
    }

    #[test]
    fn test_route_evidence_profile_fails_when_legacy_enhanced_stream_is_enabled() {
        let config = ProgramStreamsConfig {
            enabled: true,
            max_streams: 3,
            quota_policy: ProgramStreamsQuotaPolicy::FailFast,
            enabled_topics: vec![
                ProgramStreamsConfig::default_pumpfun_buy_topic(),
                ProgramStreamsConfig::default_pumpfun_buy_exact_sol_in_topic(),
                ProgramStreamsConfig::default_pumpfun_trade_topic(),
            ],
            ..ProgramStreamsConfig::default()
        };
        let selection = select_nln_program_stream_subscriptions(&config);

        assert!(selection.quota_policy_violation);
        assert!(selection.subscriptions.is_empty());
        assert!(selection
            .fail_reasons
            .iter()
            .any(|reason| reason == "route_evidence_active_parsed_streams_exceed_two"));
        assert!(selection
            .fail_reasons
            .iter()
            .any(|reason| reason.starts_with("route_evidence_profile_forbids_enhanced_streams:")));
    }

    fn make_nln_route_evidence_message(
        amount: u64,
        include_ix_index: bool,
    ) -> NlnProgramStreamMessage {
        let mut payload = json!({
            "signature": "5S1gnatureRouteEvidence",
            "slot": 12345,
            "tx_index": 7,
            "mint": "Mint111111111111111111111111111111111111111",
            "bonding_curve": "Curve11111111111111111111111111111111111111",
            "associated_bonding_curve": "AssocCurve11111111111111111111111111111111",
            "user": "User111111111111111111111111111111111111111",
            "fee_recipient": "Fee111111111111111111111111111111111111111",
            "creator_vault": "Vault11111111111111111111111111111111111111",
            "token_program": TOKEN_PROGRAM_ID,
            "amount": amount,
            "max_sol_cost": 20_000_000u64,
            "track_volume": true
        });
        if include_ix_index {
            payload["instruction_index"] = json!(3);
        }
        NlnProgramStreamMessage {
            topic: ProgramStreamsConfig::default_pumpfun_buy_topic(),
            partition: 0,
            offset_raw: "42".to_string(),
            offset: Some(42),
            provider_ts_ms: Some(1_780_000_000_000),
            recv_ts_ms: 1_780_000_000_001,
            recv_ts_ns: 1_780_000_000_001_000_000,
            decode_ts_ms: 1_780_000_000_002,
            payload_json: payload,
        }
    }

    fn encoded_pubkey_value(pubkey: Pubkey) -> Value {
        json!({
            "value": BASE64_STANDARD.encode(pubkey.to_bytes()),
        })
    }

    fn make_nln_route_evidence_message_with_value_accounts(
        include_ix_index: bool,
    ) -> (NlnProgramStreamMessage, Pubkey, Pubkey, Pubkey) {
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let remaining_0 = Pubkey::new_unique();
        let remaining_1 = Pubkey::new_unique();
        let mut payload = json!({
            "slot": 12345,
            "tx_index": 7,
            "accounts": {
                "global": encoded_pubkey_value(Pubkey::new_unique()),
                "mint": encoded_pubkey_value(mint),
                "bonding_curve": encoded_pubkey_value(bonding_curve),
                "associated_bonding_curve": encoded_pubkey_value(Pubkey::new_unique()),
                "associated_user": encoded_pubkey_value(Pubkey::new_unique()),
                "user": encoded_pubkey_value(Pubkey::new_unique()),
                "fee_recipient": encoded_pubkey_value(Pubkey::new_unique()),
                "creator_vault": encoded_pubkey_value(Pubkey::new_unique()),
                "system_program": encoded_pubkey_value(Pubkey::default()),
                "token_program": encoded_pubkey_value(Pubkey::from_str(TOKEN_PROGRAM_ID).unwrap()),
                "event_authority": encoded_pubkey_value(Pubkey::new_unique()),
                "program": encoded_pubkey_value(Pubkey::new_unique()),
                "global_volume_accumulator": encoded_pubkey_value(Pubkey::new_unique()),
                "user_volume_accumulator": encoded_pubkey_value(Pubkey::new_unique()),
                "fee_config": encoded_pubkey_value(Pubkey::new_unique()),
                "fee_program": encoded_pubkey_value(Pubkey::new_unique()),
                "remaining_accounts": [
                    encoded_pubkey_value(remaining_0),
                    encoded_pubkey_value(remaining_1),
                ],
            },
            "args": {
                "amount": {"value": "68105046256"},
                "max_sol_cost": {"value": "24975697"},
                "track_volume": {"value": true}
            }
        });
        if include_ix_index {
            payload["instruction_index"] = json!(3);
        }
        let message = NlnProgramStreamMessage {
            topic: ProgramStreamsConfig::default_pumpfun_buy_topic(),
            partition: 0,
            offset_raw: "42".to_string(),
            offset: Some(42),
            provider_ts_ms: Some(1_780_000_000_000),
            recv_ts_ms: 1_780_000_000_001,
            recv_ts_ns: 1_780_000_000_001_000_000,
            decode_ts_ms: 1_780_000_000_002,
            payload_json: payload,
        };
        (message, mint, bonding_curve, remaining_0)
    }

    #[test]
    fn test_route_evidence_candidate_hashes_keep_args_out_of_account_manifest_hash() {
        let config = ProgramStreamsConfig {
            endpoint: "events.nln.clr3.org:443".to_string(),
            artifact_capture_dir: Some(
                "logs/nln_capture/shadow-burnin-v3-selector-dataset-r12-simcov-evidence"
                    .to_string(),
            ),
            ..ProgramStreamsConfig::default()
        };
        let first = nln_route_manifest_evidence_candidate_row(
            &make_nln_route_evidence_message(1_000, true),
            NlnProgramStreamCaptureTopic::PumpFunBuy,
            &config,
        )
        .expect("route evidence row");
        let second = nln_route_manifest_evidence_candidate_row(
            &make_nln_route_evidence_message(2_000, true),
            NlnProgramStreamCaptureTopic::PumpFunBuy,
            &config,
        )
        .expect("route evidence row");

        assert_eq!(first["can_unlock_execution"], json!(false));
        assert_eq!(first["program_stream_complete_is_executable"], json!(false));
        assert_eq!(first["manifest_status"], json!("pending_join"));
        assert_eq!(
            first["account_manifest_hash"],
            second["account_manifest_hash"]
        );
        assert_ne!(first["args_hash"], second["args_hash"]);
        assert_ne!(
            first["instruction_evidence_hash"],
            second["instruction_evidence_hash"]
        );
    }

    #[test]
    fn test_route_evidence_candidate_without_instruction_index_is_degraded_only() {
        let config = ProgramStreamsConfig::default();
        let row = nln_route_manifest_evidence_candidate_row(
            &make_nln_route_evidence_message(1_000, false),
            NlnProgramStreamCaptureTopic::PumpFunBuy,
            &config,
        )
        .expect("route evidence row");

        assert_eq!(
            row["join_status"],
            json!("degraded_missing_tx_or_instruction_index")
        );
        assert_eq!(row["can_unlock_execution"], json!(false));
        assert_eq!(row["degraded_join_can_complete"], json!(false));
    }

    #[test]
    fn test_route_evidence_candidate_decodes_program_stream_value_accounts_and_tail() {
        let config = ProgramStreamsConfig::default();
        let (message, mint, bonding_curve, remaining_0) =
            make_nln_route_evidence_message_with_value_accounts(true);
        let row = nln_route_manifest_evidence_candidate_row(
            &message,
            NlnProgramStreamCaptureTopic::PumpFunBuy,
            &config,
        )
        .expect("route evidence row");

        assert_eq!(row["parse_status"], json!("OK"));
        assert_eq!(row["route_kind"], json!("legacy_buy"));
        assert_eq!(row["mint"], json!(mint.to_string()));
        assert_eq!(row["bonding_curve"], json!(bonding_curve.to_string()));
        for role in [
            "global",
            "mint",
            "bonding_curve",
            "associated_bonding_curve",
            "associated_user",
            "user",
            "fee_recipient",
            "creator_vault",
            "system_program",
            "token_program",
            "event_authority",
            "program",
            "global_volume_accumulator",
            "user_volume_accumulator",
            "fee_config",
            "fee_program",
        ] {
            assert!(
                row[role].as_str().is_some(),
                "expected named account role {role} to be extracted"
            );
        }
        assert_eq!(row["args"][0]["name"], json!("amount"));
        assert_eq!(row["args"][0]["value"], json!("68105046256"));
        assert_eq!(row["args"][1]["name"], json!("max_sol_cost"));
        assert_eq!(row["args"][1]["value"], json!("24975697"));
        assert_eq!(row["args"][2]["name"], json!("track_volume"));
        assert_eq!(row["args"][2]["value"], json!(true));
        assert!(row["args_hash"].as_str().is_some());
        assert_eq!(row["remaining_account_count"], json!(2));
        assert_eq!(row["remaining_accounts_count"], json!(2));
        assert_eq!(row["has_legacy_tail"], json!(true));
        assert_eq!(
            row["tail_evidence_status"],
            json!("program_stream_tail_observed_unverified")
        );
        assert_eq!(
            row["remaining_accounts"][0]["pubkey"],
            json!(remaining_0.to_string())
        );
        assert_eq!(row["manifest_status"], json!("degraded_pending_join"));
        assert_eq!(
            row["join_status"],
            json!("degraded_missing_tx_or_instruction_index")
        );
        assert_eq!(row["can_unlock_execution"], json!(false));
        assert_eq!(row["program_stream_complete_is_executable"], json!(false));

        let mut changed_args = message.clone();
        changed_args.payload_json["args"]["amount"] = json!({"value": "999"});
        changed_args.payload_json["args"]["max_sol_cost"] = json!({"value": "123"});
        let changed_row = nln_route_manifest_evidence_candidate_row(
            &changed_args,
            NlnProgramStreamCaptureTopic::PumpFunBuy,
            &config,
        )
        .expect("route evidence row");
        assert_eq!(
            row["account_manifest_hash"],
            changed_row["account_manifest_hash"]
        );
        assert_ne!(row["args_hash"], changed_row["args_hash"]);
        assert_ne!(
            row["instruction_evidence_hash"],
            changed_row["instruction_evidence_hash"]
        );
    }

    #[test]
    fn test_route_evidence_candidate_decodes_buy_exact_sol_in_value_args() {
        let config = ProgramStreamsConfig::default();
        let (mut message, _mint, _bonding_curve, _remaining_0) =
            make_nln_route_evidence_message_with_value_accounts(true);
        message.topic = ProgramStreamsConfig::default_pumpfun_buy_exact_sol_in_topic();
        message.payload_json["args"] = json!({
            "spendable_sol_in": {"value": "1000000"},
            "min_tokens_out": {"value": "250000"},
            "track_volume": {"value": false}
        });
        let row = nln_route_manifest_evidence_candidate_row(
            &message,
            NlnProgramStreamCaptureTopic::PumpFunBuyExactSolIn,
            &config,
        )
        .expect("route evidence row");

        assert_eq!(row["parse_status"], json!("OK"));
        assert_eq!(row["route_kind"], json!("routed_exact_sol_in"));
        assert_eq!(row["args"][0]["name"], json!("spendable_sol_in"));
        assert_eq!(row["args"][0]["value"], json!("1000000"));
        assert_eq!(row["args"][1]["name"], json!("min_tokens_out"));
        assert_eq!(row["args"][1]["value"], json!("250000"));
        assert_eq!(row["args"][2]["name"], json!("track_volume"));
        assert_eq!(row["args"][2]["value"], json!(false));
        assert!(row["args_hash"].as_str().is_some());
        assert_eq!(row["remaining_accounts_count"], json!(2));
        assert_eq!(row["has_legacy_tail"], json!(true));
        assert_eq!(row["can_unlock_execution"], json!(false));
    }

    #[test]
    fn test_program_stream_selection_fails_closed_when_required_topics_exceed_limit() {
        let config = ProgramStreamsConfig {
            enabled: true,
            max_streams: 1,
            ..ProgramStreamsConfig::default()
        };
        let selection = select_nln_program_stream_subscriptions(&config);

        assert!(selection.required_topics_exceed_limit);
        assert!(selection.subscriptions.is_empty());
    }

    #[test]
    fn test_program_stream_selection_fail_fast_rejects_enabled_topic_over_quota() {
        let config = ProgramStreamsConfig {
            enabled: true,
            max_streams: 2,
            quota_policy: ProgramStreamsQuotaPolicy::FailFast,
            enabled_topics: vec![
                "prod.rpc.solana.system.transfers".to_string(),
                "prod.rpc.solana.pumpfun.trade".to_string(),
                "prod.rpc.solana.pumpfun.create".to_string(),
            ],
            optional_topics: vec!["prod.rpc.solana.pumpfun.create".to_string()],
            ..ProgramStreamsConfig::default()
        };
        let selection = select_nln_program_stream_subscriptions(&config);

        assert!(selection.quota_policy_violation);
        assert!(selection.subscriptions.is_empty());
        assert!(selection
            .fail_reasons
            .iter()
            .any(|reason| reason == "enabled_topics_exceed_max_streams"));
        assert!(selection
            .fail_reasons
            .iter()
            .any(|reason| reason.starts_with("optional_topics_enabled:")));
    }

    #[test]
    fn test_program_stream_selection_fail_fast_rejects_unknown_enabled_topic() {
        let config = ProgramStreamsConfig {
            enabled: true,
            max_streams: 2,
            quota_policy: ProgramStreamsQuotaPolicy::FailFast,
            enabled_topics: vec![
                "prod.rpc.solana.system.transfers".to_string(),
                "prod.rpc.solana.pumpfun.trade".to_string(),
                "prod.rpc.solana.pumpfun.transaction".to_string(),
            ],
            optional_topics: vec!["prod.rpc.solana.pumpfun.transaction".to_string()],
            ..ProgramStreamsConfig::default()
        };
        let selection = select_nln_program_stream_subscriptions(&config);

        assert!(selection.quota_policy_violation);
        assert!(selection.subscriptions.is_empty());
        assert!(selection
            .fail_reasons
            .iter()
            .any(|reason| reason.starts_with("unknown_enabled_topics:")));
    }

    #[test]
    fn test_nln_trade_resolver_buffers_until_candidate_then_replays() {
        let mut resolver =
            NlnTradePoolIdentityResolver::from_program_streams_config(&ProgramStreamsConfig {
                trade_resolver_ttl_ms: 1_000,
                ..ProgramStreamsConfig::default()
            });
        let mint = Pubkey::new_unique();
        let pool = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let trade = make_nln_trade(mint, user, Signature::new_unique());
        let now = Instant::now();

        let resolve = resolver.resolve_or_buffer(trade, now);
        assert!(matches!(
            resolve.decision,
            NlnTradeResolveDecision::Buffered
        ));

        let candidate = make_candidate(pool, mint);
        let replay = resolver.register_candidate(&candidate, now + Duration::from_millis(10));

        assert!(!replay.collision);
        assert_eq!(replay.replay_ready.len(), 1);
        let forwarded = replay.replay_ready[0].to_trade_event(pool);
        assert_eq!(forwarded.pool_amm_id, pool);
        assert_eq!(forwarded.mint, mint);
        assert!(forwarded.bonding_curve_v2.is_none());
    }

    #[test]
    fn test_nln_trade_resolver_dedupes_trade_keys() {
        let mut resolver =
            NlnTradePoolIdentityResolver::from_program_streams_config(&ProgramStreamsConfig {
                trade_resolver_ttl_ms: 1_000,
                ..ProgramStreamsConfig::default()
            });
        let mint = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let signature = Signature::new_unique();
        let now = Instant::now();

        let first = resolver.resolve_or_buffer(make_nln_trade(mint, user, signature), now);
        let second = resolver.resolve_or_buffer(make_nln_trade(mint, user, signature), now);

        assert!(matches!(first.decision, NlnTradeResolveDecision::Buffered));
        assert!(matches!(
            second.decision,
            NlnTradeResolveDecision::Duplicate
        ));
    }

    #[test]
    fn test_nln_trade_resolver_fails_closed_on_mint_pool_collision() {
        let mut resolver =
            NlnTradePoolIdentityResolver::from_program_streams_config(&ProgramStreamsConfig {
                trade_resolver_ttl_ms: 1_000,
                ..ProgramStreamsConfig::default()
            });
        let mint = Pubkey::new_unique();
        let first = make_candidate(Pubkey::new_unique(), mint);
        let mut second = make_candidate(Pubkey::new_unique(), mint);
        second.bonding_curve = Pubkey::new_unique();
        let now = Instant::now();

        let first_result = resolver.register_candidate(&first, now);
        let collision = resolver.register_candidate(&second, now + Duration::from_millis(1));
        let trade_result = resolver.resolve_or_buffer(
            make_nln_trade(mint, Pubkey::new_unique(), Signature::new_unique()),
            now + Duration::from_millis(2),
        );

        assert!(!first_result.collision);
        assert!(collision.collision);
        assert!(matches!(
            trade_result.decision,
            NlnTradeResolveDecision::IdentityCollision
        ));
    }

    #[test]
    fn test_nln_normalization_error_row_preserves_join_keys_and_payload_hash() {
        let message = NlnProgramStreamMessage {
            topic: "prod.rpc.solana.system.transfers".to_string(),
            partition: 7,
            offset_raw: "42".to_string(),
            offset: Some(42),
            provider_ts_ms: Some(1_700_000_000_001),
            recv_ts_ms: 1_700_000_000_111,
            recv_ts_ns: 1_700_000_000_111_000_000,
            decode_ts_ms: 1_700_000_000_112,
            payload_json: serde_json::json!({
                "signature": "sig-test",
                "slot": "321",
                "tx_index": 3,
                "instruction_index": "2",
                "from_wallet": "from-test",
                "to_wallet": "to-test",
                "amount": "10000000"
            }),
        };
        let err = anyhow::anyhow!("bad transfer payload");

        let row = nln_normalization_error_row(
            &message,
            NlnProgramStreamCaptureTopic::SystemTransfers,
            &err,
        );

        assert_eq!(row["artifact"], "nln_normalization_errors_v1");
        assert_eq!(row["signature"], "sig-test");
        assert_eq!(row["slot"], "321");
        assert_eq!(row["tx_index"], "3");
        assert_eq!(row["instruction_index"], "2");
        assert_eq!(row["error"], "bad transfer payload");
        assert!(row["raw_payload_hash"]
            .as_str()
            .expect("payload hash")
            .starts_with("fnv64:"));
    }

    fn make_trade(pool: Pubkey, mint: Pubkey) -> TradeEvent {
        TradeEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(1),
            signature: Signature::new_unique(),
            event_ordinal: Some(7),
            tx_index: None,
            provenance: None,
            timestamp_ms: 1_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 1_010,
            pool_amm_id: pool,
            mint,
            signer: Pubkey::new_unique(),
            is_buy: true,
            is_dev_buy: false,
            amount: 42,
            max_sol_cost: 1_000_000_000,
            min_sol_output: 0,
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            v_tokens_in_bonding_curve: Some(10.0),
            v_sol_in_bonding_curve: Some(1.0),
            market_cap_sol: None,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
            creator_vault: None,
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
            toolchain_fingerprint: seer::types::ToolchainFingerprintInput::default(),
            curve_data_known: true,
            curve_finality: ghost_core::CurveFinality::Provisional,
            is_pumpswap: false,
        }
    }

    fn make_funding_transfer() -> FundingTransferEvent {
        FundingTransferEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(22),
            event_ordinal: Some(4),
            tx_index: None,
            outer_instruction_index: Some(1),
            inner_group_index: Some(1),
            cpi_stack_height: Some(2),
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 22_010,
            signature: "funding-sig".to_string(),
            source_wallet: Pubkey::new_unique().to_string(),
            recipient_wallet: Pubkey::new_unique().to_string(),
            lamports: 50_000_000,
            full_chain_coverage: false,
            provenance: seer::ipc::FundingTransferProvenance::filtered_grpc_global_stream_live(),
        }
    }

    fn make_execution_account_evidence() -> ghost_core::ExecutionAccountEvidence {
        ghost_core::ExecutionAccountEvidence {
            role: ghost_core::ExecutionAccountRole::BondingCurveV2,
            account_pubkey: Pubkey::new_unique(),
            base_mint: Some(Pubkey::new_unique()),
            pool_id: Some(Pubkey::new_unique()),
            canonical_bonding_curve: Some(Pubkey::new_unique()),
            source: ghost_core::ExecutionAccountEvidenceSource::RpcHydration,
            status: ghost_core::ExecutionAccountEvidenceStatus::RpcReady,
            slot: Some(42),
            context_slot: Some(43),
            write_version: Some(7),
            owner: Some(Pubkey::new_unique()),
            data_len: Some(256),
            tx_signature: Some("evidence-sig".to_string()),
            observed_instruction_index: Some(1),
            observed_account_position: Some(9),
            provenance_status: Some("route_compatible".to_string()),
            detected_at_ms: 22_000,
            received_at_ms: 22_010,
            evidence_ready: true,
            reason: None,
        }
    }

    fn make_bcv2_execution_account_evidence_event(
        bcv2: Pubkey,
        base_mint: Option<Pubkey>,
        pool_id: Option<Pubkey>,
        source: ghost_core::ExecutionAccountEvidenceSource,
        status: ghost_core::ExecutionAccountEvidenceStatus,
    ) -> DetectedExecutionAccountEvidenceEvent {
        DetectedExecutionAccountEvidenceEvent {
            evidence: ghost_core::ExecutionAccountEvidence {
                role: ghost_core::ExecutionAccountRole::BondingCurveV2,
                account_pubkey: bcv2,
                base_mint,
                pool_id,
                canonical_bonding_curve: None,
                source,
                status,
                slot: Some(42),
                context_slot: Some(43),
                write_version: Some(7),
                owner: Some(Pubkey::new_unique()),
                data_len: Some(256),
                tx_signature: Some("evidence-sig".to_string()),
                observed_instruction_index: Some(1),
                observed_account_position: Some(9),
                provenance_status: Some("route_compatible".to_string()),
                detected_at_ms: 22_000,
                received_at_ms: 22_010,
                evidence_ready: true,
                reason: None,
            },
            detected_at: SystemTime::now(),
            sequence_number: 11,
            priority: EventPriority::High,
        }
    }

    fn make_route_compatible_bcv2_trade(pool: Pubkey, mint: Pubkey, bcv2: Pubkey) -> TradeEvent {
        let mut trade = make_trade(pool, mint);
        trade.bonding_curve_v2 = Some(bcv2);
        trade.bonding_curve_v2_provenance = Some(ObservedAccountMetaProvenance {
            source_tx_signature: Some(trade.signature.to_string()),
            source_slot: trade.slot,
            source_instruction_index: Some(3),
            instruction_account_position: Some(16),
            resolved_pubkey: Some(bcv2.to_string()),
            tx_success: Some(true),
            provenance_status: Some("route_compatible".to_string()),
            ..Default::default()
        });
        trade
    }

    fn make_non_route_compatible_bcv2_trade(
        pool: Pubkey,
        mint: Pubkey,
        bcv2: Pubkey,
    ) -> TradeEvent {
        let mut trade = make_route_compatible_bcv2_trade(pool, mint, bcv2);
        if let Some(provenance) = trade.bonding_curve_v2_provenance.as_mut() {
            provenance.provenance_status = Some("message_index_resolution_failed".to_string());
        }
        trade
    }

    fn make_account_update(base_mint: Pubkey, bonding_curve: Pubkey) -> DetectedAccountUpdateEvent {
        DetectedAccountUpdateEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            event_time: ghost_core::EventTimeMetadata::default(),
            base_mint,
            bonding_curve,
            curve_finality: CurveFinality::Provisional,
            sol_reserves: 10,
            token_reserves: 20,
            complete: 0,
            slot: 42,
            write_version: Some(7),
            replay_origin: AccountUpdateReplayOrigin::Live,
            replay_buffer_dwell_ms: None,
            detected_at: SystemTime::now(),
            sequence_number: 1,
        }
    }

    #[test]
    fn canonical_account_update_decode_supports_pumpswap_pool_layout() {
        let base_mint = Pubkey::new_unique();
        let pool_state = seer::binary_parser::AmmPoolState {
            pool_bump: 1,
            index: 9,
            creator: Pubkey::new_unique().to_bytes(),
            base_mint: base_mint.to_bytes(),
            quote_mint: Pubkey::from_str("So11111111111111111111111111111111111111112")
                .expect("valid wsol mint")
                .to_bytes(),
            lp_mint: Pubkey::new_unique().to_bytes(),
            pool_base_token_account: Pubkey::new_unique().to_bytes(),
            pool_quote_token_account: Pubkey::new_unique().to_bytes(),
            base_amount: 777,
            quote_amount: 888,
        };
        let mut data = seer::binary_parser::DISC_AMM_POOL.to_vec();
        data.push(pool_state.pool_bump);
        data.extend_from_slice(&pool_state.index.to_le_bytes());
        data.extend_from_slice(&pool_state.creator);
        data.extend_from_slice(&pool_state.base_mint);
        data.extend_from_slice(&pool_state.quote_mint);
        data.extend_from_slice(&pool_state.lp_mint);
        data.extend_from_slice(&pool_state.pool_base_token_account);
        data.extend_from_slice(&pool_state.pool_quote_token_account);
        data.extend_from_slice(&pool_state.base_amount.to_le_bytes());
        data.extend_from_slice(&pool_state.quote_amount.to_le_bytes());

        let payload = seer::decode_canonical_account_update(*pumpswap_program_id(), &data)
            .expect("pumpswap AMM pool must decode as canonical account update");
        assert_eq!(payload.sol_reserves(), 888);
        assert_eq!(payload.token_reserves(), 777);
        assert_eq!(payload.complete(), 1);
    }

    #[test]
    fn canonical_account_update_decode_is_data_driven_even_for_unknown_owner() {
        let base_mint = Pubkey::new_unique();
        let pool_state = seer::binary_parser::AmmPoolState {
            pool_bump: 1,
            index: 5,
            creator: Pubkey::new_unique().to_bytes(),
            base_mint: base_mint.to_bytes(),
            quote_mint: Pubkey::from_str("So11111111111111111111111111111111111111112")
                .expect("valid wsol mint")
                .to_bytes(),
            lp_mint: Pubkey::new_unique().to_bytes(),
            pool_base_token_account: Pubkey::new_unique().to_bytes(),
            pool_quote_token_account: Pubkey::new_unique().to_bytes(),
            base_amount: 10,
            quote_amount: 20,
        };
        let mut data = seer::binary_parser::DISC_AMM_POOL.to_vec();
        data.push(pool_state.pool_bump);
        data.extend_from_slice(&pool_state.index.to_le_bytes());
        data.extend_from_slice(&pool_state.creator);
        data.extend_from_slice(&pool_state.base_mint);
        data.extend_from_slice(&pool_state.quote_mint);
        data.extend_from_slice(&pool_state.lp_mint);
        data.extend_from_slice(&pool_state.pool_base_token_account);
        data.extend_from_slice(&pool_state.pool_quote_token_account);
        data.extend_from_slice(&pool_state.base_amount.to_le_bytes());
        data.extend_from_slice(&pool_state.quote_amount.to_le_bytes());

        let unknown_owner = Pubkey::new_unique();
        assert_ne!(unknown_owner, *pumpswap_program_id());
        let payload = seer::decode_canonical_account_update(unknown_owner, &data)
            .expect("layout decoding remains data-driven");
        assert_eq!(payload.sol_reserves(), 20);
        assert_eq!(payload.token_reserves(), 10);
    }

    #[test]
    fn trade_event_to_pool_transaction_preserves_failed_status() {
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let mut trade = make_trade(pool, mint);
        trade.success = false;
        trade.error_code = Some("InstructionError(Custom(1))".to_string());

        let pool_tx = trade_event_to_pool_transaction(&trade);

        assert!(!pool_tx.success);
        assert_eq!(
            pool_tx.error_code.as_deref(),
            Some("InstructionError(Custom(1))")
        );
    }

    #[test]
    fn trade_with_default_mint_is_not_forwardable() {
        let trade = make_trade(Pubkey::new_unique(), Pubkey::default());
        assert!(!trade_has_forwardable_identity(&trade));
    }

    #[test]
    fn trade_with_resolved_identity_is_forwardable() {
        let trade = make_trade(Pubkey::new_unique(), Pubkey::new_unique());
        assert!(trade_has_forwardable_identity(&trade));
    }

    #[test]
    fn bridge_preserves_event_ordinal() {
        let trade = make_trade(Pubkey::new_unique(), Pubkey::new_unique());
        let tx = trade_event_to_pool_transaction(&trade);
        assert_eq!(tx.event_ordinal, trade.event_ordinal);
    }

    #[test]
    fn bridge_preserves_tx_index() {
        let mut trade = make_trade(Pubkey::new_unique(), Pubkey::new_unique());
        trade.tx_index = Some(123);

        let tx = trade_event_to_pool_transaction(&trade);

        assert_eq!(tx.tx_index, Some(123));
    }

    #[test]
    fn bridge_preserves_toolchain_fingerprint() {
        let mut trade = make_trade(Pubkey::new_unique(), Pubkey::new_unique());
        trade.toolchain_fingerprint = seer::types::ToolchainFingerprintInput {
            account_keys_len: Some(18),
            outer_instruction_count: Some(3),
            inner_instruction_group_count: Some(2),
            has_set_compute_unit_limit: Some(true),
            has_set_compute_unit_price: Some(true),
            internal_fee_transfer_count: Some(0),
            external_fee_transfer_count: Some(2),
            filtered_wsol_self_transfer_count: Some(1),
        };

        let tx = trade_event_to_pool_transaction(&trade);

        assert_eq!(tx.toolchain_fingerprint, trade.toolchain_fingerprint);
    }

    #[test]
    fn bridge_preserves_signer_post_balance() {
        let mut trade = make_trade(Pubkey::new_unique(), Pubkey::new_unique());
        trade.signer_pre_balance_lamports = Some(5_000_000_000);
        trade.signer_post_balance_lamports = Some(4_100_000_000);

        let tx = trade_event_to_pool_transaction(&trade);

        assert_eq!(
            tx.signer_pre_balance_lamports,
            trade.signer_pre_balance_lamports
        );
        assert_eq!(
            tx.signer_post_balance_lamports,
            trade.signer_post_balance_lamports
        );
    }

    #[test]
    fn bridge_preserves_provenance_when_enabled() {
        let mut trade = make_trade(Pubkey::new_unique(), Pubkey::new_unique());
        trade.provenance = Some(InstructionProvenance {
            outer_instruction_index: Some(4),
            inner_group_index: Some(2),
            outer_program_id: Some("outer-program".to_string()),
            invoked_program_id: "invoked-program".to_string(),
            stack_height: Some(3),
            from_cpi: true,
        });

        let tx = trade_event_to_pool_transaction(&trade);

        assert_eq!(tx.outer_instruction_index, Some(4));
        assert_eq!(tx.inner_group_index, Some(2));
        assert_eq!(tx.outer_program_id.as_deref(), Some("outer-program"));
        assert_eq!(tx.cpi_stack_height, Some(3));
    }

    #[test]
    fn bridge_omits_default_mint_identity() {
        let trade = make_trade(Pubkey::new_unique(), Pubkey::default());
        let tx = trade_event_to_pool_transaction(&trade);
        assert!(tx.token_mint.is_none());
    }

    #[test]
    fn bridge_preserves_trade_semantics_and_mpcf_payload() {
        let mut trade = make_trade(Pubkey::new_unique(), Pubkey::new_unique());
        trade.semantic = ghost_core::EventSemanticEnvelope::new(
            ghost_core::SourceKind::PumpPortal,
            ghost_core::EventTruthKind::Synthetic,
            ghost_core::SlotQuality::Absent,
            ghost_core::TimestampQuality::Adapter,
            ghost_core::EventCompleteness::Partial,
        );
        trade.mpcf_payload = vec![9, 8, 7];
        trade.mpcf_payload_missing_reason = RawBytesMissingReason::NotMissing;

        let tx = trade_event_to_pool_transaction(&trade);

        assert_eq!(tx.semantic, trade.semantic);
        assert_eq!(tx.mpcf_payload, trade.mpcf_payload);
        assert_eq!(
            tx.mpcf_payload_missing_reason,
            RawBytesMissingReason::NotMissing
        );
    }

    #[test]
    fn detected_pool_fallback_downgrades_timestamp_quality_to_wall_clock() {
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let mut candidate = make_candidate(pool, mint);
        candidate.event_ts_ms = None;
        candidate.semantic = ghost_core::EventSemanticEnvelope::new(
            ghost_core::SourceKind::Grpc,
            ghost_core::EventTruthKind::RawChain,
            ghost_core::SlotQuality::Present,
            ghost_core::TimestampQuality::Chain,
            ghost_core::EventCompleteness::Full,
        );

        let detected = detected_pool_from_candidate(&candidate, 77_000);

        assert_eq!(detected.timestamp_ms, 77_000);
        assert_eq!(
            detected.semantic.timestamp_quality,
            ghost_core::TimestampQuality::WallClock
        );
        assert_eq!(
            detected.semantic.completeness,
            ghost_core::EventCompleteness::Partial
        );
    }

    #[test]
    fn detection_clock_summary_prefers_explicit_ingress_time_for_latency() {
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let mut candidate = make_candidate(pool, mint);
        candidate.event_time = ghost_core::EventTimeMetadata::new(None, Some(66_000), None);
        candidate.event_ts_ms = Some(55_000);

        let summary = detection_clock_summary(&candidate, 77_000);

        assert_eq!(summary.compat_event_ts_ms, 66_000);
        assert_eq!(summary.effective_event_ts_ms, Some(66_000));
        assert_eq!(summary.chain_event_ts_ms, None);
        assert!(summary.has_explicit_event_time);
        assert_eq!(summary.ingest_latency_ms, 11_000);
    }

    #[test]
    fn detection_clock_summary_ignores_legacy_only_timestamp_for_latency() {
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let mut candidate = make_candidate(pool, mint);
        candidate.event_time = ghost_core::EventTimeMetadata::default();
        candidate.event_ts_ms = Some(66_000);

        let summary = detection_clock_summary(&candidate, 77_000);

        assert_eq!(summary.compat_event_ts_ms, 66_000);
        assert_eq!(summary.effective_event_ts_ms, None);
        assert_eq!(summary.chain_event_ts_ms, None);
        assert!(!summary.has_explicit_event_time);
        assert_eq!(summary.ingest_latency_ms, 0);
    }

    #[test]
    fn detected_pool_legacy_timestamp_downgrades_timestamp_quality_to_wall_clock() {
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let mut candidate = make_candidate(pool, mint);
        candidate.event_time = ghost_core::EventTimeMetadata::default();
        candidate.event_ts_ms = Some(66_000);
        candidate.semantic = ghost_core::EventSemanticEnvelope::new(
            ghost_core::SourceKind::Grpc,
            ghost_core::EventTruthKind::RawChain,
            ghost_core::SlotQuality::Present,
            ghost_core::TimestampQuality::Chain,
            ghost_core::EventCompleteness::Full,
        );

        let detected = detected_pool_from_candidate(&candidate, 77_000);

        assert_eq!(detected.timestamp_ms, 66_000);
        assert_eq!(
            detected.semantic.timestamp_quality,
            ghost_core::TimestampQuality::WallClock
        );
        assert_eq!(
            detected.semantic.completeness,
            ghost_core::EventCompleteness::Partial
        );
    }

    #[test]
    fn detected_pool_explicit_chain_event_time_preserves_chain_quality() {
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let mut candidate = make_candidate(pool, mint);
        candidate.event_ts_ms = None;
        candidate.event_time = ghost_core::EventTimeMetadata::new(Some(66_000), None, None);
        candidate.semantic = ghost_core::EventSemanticEnvelope::new(
            ghost_core::SourceKind::Grpc,
            ghost_core::EventTruthKind::RawChain,
            ghost_core::SlotQuality::Present,
            ghost_core::TimestampQuality::Chain,
            ghost_core::EventCompleteness::Full,
        );

        let detected = detected_pool_from_candidate(&candidate, 77_000);

        assert_eq!(detected.timestamp_ms, 66_000);
        assert_eq!(
            detected.semantic.timestamp_quality,
            ghost_core::TimestampQuality::Chain
        );
        assert_eq!(
            detected.semantic.completeness,
            ghost_core::EventCompleteness::Full
        );
    }

    #[test]
    fn session_bridge_silently_drops_unknown_pool_trade() {
        let ttl = Duration::from_millis(100);
        let mut bridge = SessionPoolTradeBridge::new(ttl, 4, 16, Duration::from_secs(60), 32);
        let pool = Pubkey::new_unique();
        let trade = make_trade(pool, Pubkey::new_unique());
        let now = Instant::now();

        // Unknown pool → SilentDrop.
        let ingress = bridge.ingest_trade(&trade, now);
        assert_eq!(ingress.decision, SessionTradeDecision::SilentDrop);
        assert_eq!(bridge.pending_total(), 0);

        // Subsequent trade for now-registered pool → ForwardNow.
        let flush = bridge.register_detected_pool(pool, now + Duration::from_millis(50));
        assert_eq!(flush.expired_count, 0);
        assert!(flush.replay_ready.is_empty());
        let ingress2 = bridge.ingest_trade(&trade, now + Duration::from_millis(51));
        assert_eq!(ingress2.decision, SessionTradeDecision::ForwardNow);
    }

    #[test]
    fn session_bridge_silently_drops_unknown_pool_regardless_of_timing() {
        let ttl = Duration::from_millis(1); // extremely short TTL
        let mut bridge = SessionPoolTradeBridge::new(ttl, 4, 16, Duration::from_secs(60), 32);
        let pool = Pubkey::new_unique();
        let trade = make_trade(pool, Pubkey::new_unique());
        let now = Instant::now();

        let ingress = bridge.ingest_trade(&trade, now);
        assert_eq!(ingress.decision, SessionTradeDecision::SilentDrop);

        // TTL elapsed before PoolDetected → still no replay because unknown pool was never buffered.
        let flush = bridge.register_detected_pool(pool, now + Duration::from_millis(500));
        assert_eq!(flush.expired_count, 0);
        assert!(flush.replay_ready.is_empty());
    }

    #[test]
    fn session_bridge_forwards_immediately_after_pool_detected() {
        let ttl = Duration::from_millis(100);
        let mut bridge = SessionPoolTradeBridge::new(ttl, 4, 16, Duration::from_secs(60), 32);
        let pool = Pubkey::new_unique();
        let trade = make_trade(pool, Pubkey::new_unique());
        let now = Instant::now();

        let flush = bridge.register_detected_pool(pool, now);
        assert!(flush.replay_ready.is_empty());

        let ingress = bridge.ingest_trade(&trade, now + Duration::from_millis(1));
        assert_eq!(ingress.decision, SessionTradeDecision::ForwardNow);
    }

    #[test]
    fn session_bridge_forwards_trades_after_pool_detected_registration() {
        // Production path: PoolDetected arrives first, then trades.
        let ttl = Duration::from_millis(100);
        let mut bridge = SessionPoolTradeBridge::new(ttl, 4, 16, Duration::from_secs(60), 32);
        let pool = Pubkey::new_unique();
        let trade = make_trade(pool, Pubkey::new_unique());
        let now = Instant::now();

        // Trade before registration → SilentDrop.
        let pre = bridge.ingest_trade(&trade, now);
        assert_eq!(pre.decision, SessionTradeDecision::SilentDrop);

        // PoolDetected registers the pool, but no prior trade is replayed.
        let flush = bridge.register_detected_pool(pool, now + Duration::from_millis(1));
        assert!(flush.replay_ready.is_empty());

        // All subsequent trades → ForwardNow.
        let first = bridge.ingest_trade(&trade, now + Duration::from_millis(2));
        assert_eq!(first.decision, SessionTradeDecision::ForwardNow);

        let second = bridge.ingest_trade(&trade, now + Duration::from_millis(3));
        assert_eq!(second.decision, SessionTradeDecision::ForwardNow);
        assert_eq!(bridge.pending_total(), 0);
    }

    #[test]
    fn session_bridge_prunes_detected_pool_registry_after_ttl() {
        let ttl = Duration::from_millis(100);
        let registry_ttl = Duration::from_millis(20);
        let mut bridge = SessionPoolTradeBridge::new(ttl, 4, 16, registry_ttl, 32);
        let pool = Pubkey::new_unique();
        let now = Instant::now();

        let flush = bridge.register_detected_pool(pool, now);
        assert_eq!(flush.expired_detected_pools, 0);
        assert_eq!(bridge.detected_total(), 1);

        let (_expired_pending, expired_detected) =
            bridge.prune_expired(now + Duration::from_millis(25));
        assert_eq!(expired_detected, 1);
        assert_eq!(bridge.detected_total(), 0);
    }

    #[test]
    fn session_account_update_bridge_buffers_until_pool_detected() {
        let ttl = Duration::from_millis(100);
        let mut bridge = SessionAccountUpdateBridge::new(ttl, 4, 16, Duration::from_secs(60), 32);
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let mut candidate = make_candidate(pool, mint);
        candidate.bonding_curve = curve;
        let update = make_account_update(mint, curve);
        let now = Instant::now();

        let ingress = bridge.ingest_account_update(&update, now);
        assert_eq!(
            ingress.decision,
            SessionAccountUpdateDecision::BufferedUntilPoolDetected
        );
        assert_eq!(bridge.pending_total(), 1);

        let flush = bridge.register_detected_pool(&candidate, now + Duration::from_millis(1));
        assert_eq!(flush.expired_count, 0);
        assert_eq!(flush.replay_ready.len(), 1);
        assert_eq!(flush.replay_ready[0].base_mint, mint);
        assert_eq!(bridge.pending_total(), 0);
    }

    #[test]
    fn session_account_update_bridge_expires_unknown_updates() {
        let ttl = Duration::from_millis(5);
        let mut bridge = SessionAccountUpdateBridge::new(ttl, 4, 16, Duration::from_secs(60), 32);
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let mut candidate = make_candidate(pool, mint);
        candidate.bonding_curve = curve;
        let update = make_account_update(mint, curve);
        let now = Instant::now();

        let ingress = bridge.ingest_account_update(&update, now);
        assert_eq!(
            ingress.decision,
            SessionAccountUpdateDecision::BufferedUntilPoolDetected
        );

        let flush = bridge.register_detected_pool(&candidate, now + Duration::from_millis(20));
        assert_eq!(flush.replay_ready.len(), 0);
        assert_eq!(flush.expired_count, 1);
    }

    #[test]
    fn session_account_update_bridge_refreshes_detected_keys_on_forward_now() {
        let ttl = Duration::from_millis(100);
        let detected_key_ttl = Duration::from_millis(10);
        let mut bridge = SessionAccountUpdateBridge::new(ttl, 4, 16, detected_key_ttl, 32);
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let mut candidate = make_candidate(pool, mint);
        candidate.bonding_curve = curve;
        let update = make_account_update(mint, curve);
        let now = Instant::now();

        let flush = bridge.register_detected_pool(&candidate, now);
        assert_eq!(flush.expired_detected_keys, 0);

        let first = bridge.ingest_account_update(&update, now + Duration::from_millis(8));
        assert_eq!(first.decision, SessionAccountUpdateDecision::ForwardNow);

        let second = bridge.ingest_account_update(&update, now + Duration::from_millis(15));
        assert_eq!(second.decision, SessionAccountUpdateDecision::ForwardNow);
    }

    #[test]
    fn session_account_update_bridge_refreshes_detected_keys_from_trade_activity() {
        let ttl = Duration::from_millis(100);
        let detected_key_ttl = Duration::from_millis(10);
        let mut bridge = SessionAccountUpdateBridge::new(ttl, 4, 16, detected_key_ttl, 32);
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let mut candidate = make_candidate(pool, mint);
        candidate.bonding_curve = curve;
        let trade = make_trade(pool, mint);
        let update = make_account_update(mint, curve);
        let now = Instant::now();

        let flush = bridge.register_detected_pool(&candidate, now);
        assert_eq!(flush.expired_detected_keys, 0);

        let keepalive = bridge.refresh_from_trade(&trade, now + Duration::from_millis(8));
        assert_eq!(keepalive.expired_detected_keys, 0);

        let ingress = bridge.ingest_account_update(&update, now + Duration::from_millis(15));
        assert_eq!(ingress.decision, SessionAccountUpdateDecision::ForwardNow);
    }

    #[test]
    fn session_bridge_buffers_bcv2_evidence_until_pool_detected() {
        let mut bridge = SessionAccountUpdateBridge::new(
            Duration::from_millis(100),
            4,
            16,
            Duration::from_secs(60),
            32,
        );
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let bcv2 = Pubkey::new_unique();
        let evidence = make_bcv2_execution_account_evidence_event(
            bcv2,
            Some(mint),
            Some(pool),
            ghost_core::ExecutionAccountEvidenceSource::RpcHydration,
            ghost_core::ExecutionAccountEvidenceStatus::RpcReady,
        );

        let ingress = bridge.ingest_execution_account_evidence(&evidence, Instant::now());

        assert_eq!(
            ingress.decision,
            SessionExecutionAccountEvidenceDecision::BufferedUntilPoolDetected
        );
        assert_eq!(bridge.pending_execution_evidence_total(), 1);
        assert!(!bridge.has_detected_bcv2_key(bcv2));
    }

    #[test]
    fn session_bridge_pool_detected_flushes_bcv2_evidence_by_base_mint_context() {
        let mut bridge = SessionAccountUpdateBridge::new(
            Duration::from_millis(100),
            4,
            16,
            Duration::from_secs(60),
            32,
        );
        let candidate_pool = Pubkey::new_unique();
        let other_pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let bcv2 = Pubkey::new_unique();
        let candidate = make_candidate(candidate_pool, mint);
        let evidence = make_bcv2_execution_account_evidence_event(
            bcv2,
            Some(mint),
            Some(other_pool),
            ghost_core::ExecutionAccountEvidenceSource::ObservedTxMeta,
            ghost_core::ExecutionAccountEvidenceStatus::DiscoveryHint,
        );
        let now = Instant::now();
        assert_eq!(
            bridge
                .ingest_execution_account_evidence(&evidence, now)
                .decision,
            SessionExecutionAccountEvidenceDecision::BufferedUntilPoolDetected
        );

        let flush = bridge.register_detected_pool(&candidate, now + Duration::from_millis(1));

        assert_eq!(flush.replay_ready_evidence.len(), 1);
        assert_eq!(flush.replay_ready_evidence[0].evidence.account_pubkey, bcv2);
        assert_eq!(flush.expired_evidence_count, 0);
        assert_eq!(bridge.pending_execution_evidence_total(), 0);
        assert!(bridge.has_detected_bcv2_key(bcv2));
    }

    #[test]
    fn session_bridge_pool_detected_flushes_bcv2_evidence_by_pool_context() {
        let mut bridge = SessionAccountUpdateBridge::new(
            Duration::from_millis(100),
            4,
            16,
            Duration::from_secs(60),
            32,
        );
        let pool = Pubkey::new_unique();
        let evidence_mint = Pubkey::new_unique();
        let candidate_mint = Pubkey::new_unique();
        let bcv2 = Pubkey::new_unique();
        let candidate = make_candidate(pool, candidate_mint);
        let evidence = make_bcv2_execution_account_evidence_event(
            bcv2,
            Some(evidence_mint),
            Some(pool),
            ghost_core::ExecutionAccountEvidenceSource::YellowstoneAccountUpdate,
            ghost_core::ExecutionAccountEvidenceStatus::AccountUpdateReceived,
        );
        let now = Instant::now();
        assert_eq!(
            bridge
                .ingest_execution_account_evidence(&evidence, now)
                .decision,
            SessionExecutionAccountEvidenceDecision::BufferedUntilPoolDetected
        );

        let flush = bridge.register_detected_pool(&candidate, now + Duration::from_millis(1));

        assert_eq!(flush.replay_ready_evidence.len(), 1);
        assert_eq!(flush.replay_ready_evidence[0].evidence.account_pubkey, bcv2);
        assert!(bridge.has_detected_bcv2_key(bcv2));
    }

    #[test]
    fn session_bridge_refresh_from_route_compatible_trade_registers_bcv2_and_flushes_pending() {
        let mut bridge = SessionAccountUpdateBridge::new(
            Duration::from_millis(100),
            4,
            16,
            Duration::from_secs(60),
            32,
        );
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let bcv2 = Pubkey::new_unique();
        let candidate = make_candidate(pool, mint);
        let evidence = make_bcv2_execution_account_evidence_event(
            bcv2,
            None,
            None,
            ghost_core::ExecutionAccountEvidenceSource::RpcHydration,
            ghost_core::ExecutionAccountEvidenceStatus::RpcReady,
        );
        let now = Instant::now();
        bridge.register_detected_pool(&candidate, now);
        assert_eq!(
            bridge
                .ingest_execution_account_evidence(&evidence, now + Duration::from_millis(1))
                .decision,
            SessionExecutionAccountEvidenceDecision::BufferedUntilPoolDetected
        );

        let trade = make_route_compatible_bcv2_trade(pool, mint, bcv2);
        let liveness = bridge.refresh_from_trade(&trade, now + Duration::from_millis(2));

        assert_eq!(liveness.replay_ready_evidence.len(), 1);
        assert_eq!(
            liveness.replay_ready_evidence[0].evidence.account_pubkey,
            bcv2
        );
        assert_eq!(bridge.pending_execution_evidence_total(), 0);
        assert!(bridge.has_detected_bcv2_key(bcv2));
    }

    #[test]
    fn session_bridge_non_route_compatible_bcv2_trade_does_not_register_session_key() {
        let mut bridge = SessionAccountUpdateBridge::new(
            Duration::from_millis(100),
            4,
            16,
            Duration::from_secs(60),
            32,
        );
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let bcv2 = Pubkey::new_unique();
        let candidate = make_candidate(pool, mint);
        let evidence = make_bcv2_execution_account_evidence_event(
            bcv2,
            None,
            None,
            ghost_core::ExecutionAccountEvidenceSource::RpcHydration,
            ghost_core::ExecutionAccountEvidenceStatus::RpcReady,
        );
        let now = Instant::now();
        bridge.register_detected_pool(&candidate, now);
        bridge.ingest_execution_account_evidence(&evidence, now + Duration::from_millis(1));

        let trade = make_non_route_compatible_bcv2_trade(pool, mint, bcv2);
        let liveness = bridge.refresh_from_trade(&trade, now + Duration::from_millis(2));

        assert!(liveness.replay_ready_evidence.is_empty());
        assert_eq!(bridge.pending_execution_evidence_total(), 1);
        assert!(!bridge.has_detected_bcv2_key(bcv2));
    }

    #[test]
    fn session_bridge_missing_bcv2_provenance_does_not_register_session_key() {
        let mut bridge = SessionAccountUpdateBridge::new(
            Duration::from_millis(100),
            4,
            16,
            Duration::from_secs(60),
            32,
        );
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let bcv2 = Pubkey::new_unique();
        let candidate = make_candidate(pool, mint);
        let evidence = make_bcv2_execution_account_evidence_event(
            bcv2,
            None,
            None,
            ghost_core::ExecutionAccountEvidenceSource::RpcHydration,
            ghost_core::ExecutionAccountEvidenceStatus::RpcReady,
        );
        let now = Instant::now();
        bridge.register_detected_pool(&candidate, now);
        bridge.ingest_execution_account_evidence(&evidence, now + Duration::from_millis(1));
        let mut trade = make_trade(pool, mint);
        trade.bonding_curve_v2 = Some(bcv2);

        let liveness = bridge.refresh_from_trade(&trade, now + Duration::from_millis(2));

        assert!(liveness.replay_ready_evidence.is_empty());
        assert_eq!(bridge.pending_execution_evidence_total(), 1);
        assert!(!bridge.has_detected_bcv2_key(bcv2));
    }

    #[test]
    fn session_bridge_canonical_account_update_semantics_remain_classic_only() {
        let mut bridge = SessionAccountUpdateBridge::new(
            Duration::from_millis(100),
            4,
            16,
            Duration::from_secs(60),
            32,
        );
        let bcv2 = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let context = SessionBcv2Context {
            account_pubkey: bcv2,
            base_mint: None,
            pool_id: None,
            canonical_bonding_curve: None,
        };
        bridge.register_bcv2_key(context, Instant::now(), "test");
        let update = make_account_update(base_mint, bcv2);

        let ingress = bridge.ingest_account_update(&update, Instant::now());

        assert_eq!(
            ingress.decision,
            SessionAccountUpdateDecision::BufferedUntilPoolDetected
        );
        assert_eq!(bridge.pending_total(), 1);
    }

    #[tokio::test]
    async fn seer_trade_without_pool_detected_is_silently_dropped() {
        // Trade for an unknown pool must not hit the event bus immediately; it is
        // discarded by the launcher-side session gate until PoolDetected arrives.
        let (tx, mut rx) = create_event_bus();
        let pool = Pubkey::new_unique();
        let trade = make_trade(pool, Pubkey::new_unique());
        let trade_event = SeerEvent::Trade(DetectedTradeEvent {
            trade: trade.clone(),
            detected_at: SystemTime::now(),
            sequence_number: 1,
            priority: EventPriority::Normal,
        });
        let mut bridge = SessionPoolTradeBridge::new(
            Duration::from_millis(100),
            4,
            16,
            Duration::from_secs(60),
            32,
        );

        match trade_event {
            SeerEvent::Trade(event) => {
                let gating = process_trade_event_for_session_gate(
                    &tx,
                    &mut bridge,
                    &event.trade,
                    None,
                    Instant::now(),
                );
                assert_eq!(gating.decision, SessionTradeDecision::SilentDrop);
            }
            _ => unreachable!(),
        }

        // Nothing emitted — dropped trade must not produce any event bus message.
        let timeout_result = tokio::time::timeout(Duration::from_millis(50), rx.recv()).await;
        assert!(
            timeout_result.is_err(),
            "No event should be emitted for an unknown-pool trade"
        );
    }

    #[tokio::test]
    async fn seer_trade_before_pool_detected_does_not_replay_after_registration() {
        let (tx, mut rx) = create_event_bus();
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let trade = make_trade(pool, mint);
        let candidate = make_candidate(pool, mint);
        let mut bridge = SessionPoolTradeBridge::new(
            Duration::from_millis(100),
            4,
            16,
            Duration::from_secs(60),
            32,
        );

        let gating =
            process_trade_event_for_session_gate(&tx, &mut bridge, &trade, None, Instant::now());
        assert_eq!(gating.decision, SessionTradeDecision::SilentDrop);

        let detected_ms = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let flush = process_pool_detected_event_for_session_gate(
            &tx,
            &mut bridge,
            &candidate,
            None,
            Instant::now() + Duration::from_millis(1),
            detected_ms,
        );
        assert!(flush.replay_ready.is_empty());

        let first = recv_only_event(&mut rx).await;
        match first {
            GhostEvent::NewPoolDetected(pool_event) => {
                assert_eq!(pool_event.pool_amm_id, pool.to_string());
                assert_eq!(pool_event.base_mint, mint.to_string());
            }
            other => panic!("expected NewPoolDetected, got {}", other.event_type()),
        }

        assert!(tokio::time::timeout(Duration::from_millis(50), rx.recv())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn seer_pool_detected_then_trade_emits_new_pool_detected_then_pool_transaction() {
        // Production path (seer FIFO guarantee): PoolDetected always precedes Trade on
        // the IPC channel for newly created pools. This test verifies the canonical flow:
        // 1. PoolDetected → bridge registers pool + event bus receives NewPoolDetected
        // 2. Trade → bridge returns ForwardNow + event bus receives PoolTransaction
        let (tx, mut rx) = create_event_bus();
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let trade = make_trade(pool, mint);
        let candidate = make_candidate(pool, mint);
        let pool_event = SeerEvent::PoolDetected(DetectedPoolEvent {
            candidate: candidate.clone(),
            detected_at: SystemTime::now(),
            sequence_number: 1,
            priority: EventPriority::Normal,
        });
        let trade_event = SeerEvent::Trade(DetectedTradeEvent {
            trade: trade.clone(),
            detected_at: SystemTime::now(),
            sequence_number: 2,
            priority: EventPriority::Normal,
        });
        let mut bridge = SessionPoolTradeBridge::new(
            Duration::from_millis(100),
            4,
            16,
            Duration::from_secs(60),
            32,
        );

        // Step 1: PoolDetected → registers pool, emits NewPoolDetected.
        match pool_event {
            SeerEvent::PoolDetected(event) => {
                let detected_ms = event
                    .detected_at
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                let flush = process_pool_detected_event_for_session_gate(
                    &tx,
                    &mut bridge,
                    &event.candidate,
                    None,
                    Instant::now(),
                    detected_ms,
                );
                assert!(flush.replay_ready.is_empty());
            }
            _ => unreachable!(),
        }

        // Step 2: Trade for now-registered pool → ForwardNow.
        match trade_event {
            SeerEvent::Trade(event) => {
                let gating = process_trade_event_for_session_gate(
                    &tx,
                    &mut bridge,
                    &event.trade,
                    None,
                    Instant::now(),
                );
                assert_eq!(gating.decision, SessionTradeDecision::ForwardNow);
            }
            _ => unreachable!(),
        }

        // First event: NewPoolDetected.
        let first = recv_only_event(&mut rx).await;
        match first {
            GhostEvent::NewPoolDetected(pool_event) => {
                let expected = detected_pool_from_candidate(
                    &candidate,
                    pool_event
                        .detected_wall_ts_ms
                        .expect("missing detected wall ts"),
                );
                assert_eq!(pool_event.pool_amm_id, expected.pool_amm_id);
                assert_eq!(pool_event.base_mint, expected.base_mint);
            }
            other => panic!("expected NewPoolDetected, got {}", other.event_type()),
        }

        // Second event: PoolTransaction.
        let second = recv_only_event(&mut rx).await;
        match second {
            GhostEvent::PoolTransaction(pool_tx) => {
                assert_eq!(pool_tx.pool_amm_id, pool.to_string());
                assert_eq!(pool_tx.signature, trade.signature.to_string());
            }
            other => panic!("expected PoolTransaction, got {}", other.event_type()),
        }

        assert!(tokio::time::timeout(Duration::from_millis(50), rx.recv())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn seer_funding_transfer_emits_funding_transfer_observed() {
        let (tx, mut rx) = create_event_bus();
        let mut transfer = make_funding_transfer();
        transfer.tx_index = Some(77);
        let funding = SeerEvent::FundingTransfer(DetectedFundingTransferEvent {
            transfer,
            lane_health: seer::ipc::FundingLaneRuntimeHealth::default(),
            detected_at: SystemTime::now(),
            sequence_number: 7,
            priority: EventPriority::High,
        });

        match funding {
            SeerEvent::FundingTransfer(event) => {
                emit_funding_transfer_to_event_bus(&tx, &event, None);
            }
            _ => unreachable!(),
        }

        let received = recv_only_event(&mut rx).await;
        match received {
            GhostEvent::FundingTransferObserved(observed) => {
                assert_eq!(observed.signature, "funding-sig");
                assert_eq!(observed.lamports, 50_000_000);
                assert_eq!(observed.event_ordinal, Some(4));
                assert_eq!(observed.tx_index, Some(77));
                assert_eq!(observed.outer_instruction_index, Some(1));
                assert_eq!(observed.inner_group_index, Some(1));
                assert_eq!(observed.cpi_stack_height, Some(2));
                assert_eq!(observed.arrival_ts_ms, 22_010);
                assert_eq!(observed.sequence_number, 7);
                assert!(!observed.full_chain_coverage);
                assert_eq!(
                    observed.provenance,
                    seer::ipc::FundingTransferProvenance::filtered_grpc_global_stream_live()
                );
            }
            other => panic!(
                "expected FundingTransferObserved, got {}",
                other.event_type()
            ),
        }
    }

    #[tokio::test]
    async fn seer_execution_account_evidence_before_pool_detected_replays_after_pool_detected() {
        let (tx, mut rx) = create_event_bus();
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let bcv2 = Pubkey::new_unique();
        let candidate = make_candidate(pool, mint);
        let evidence_event = make_bcv2_execution_account_evidence_event(
            bcv2,
            Some(mint),
            Some(pool),
            ghost_core::ExecutionAccountEvidenceSource::ObservedTxMeta,
            ghost_core::ExecutionAccountEvidenceStatus::DiscoveryHint,
        );
        let mut trade_bridge = SessionPoolTradeBridge::new(
            Duration::from_millis(100),
            4,
            16,
            Duration::from_secs(60),
            32,
        );
        let mut account_bridge = SessionAccountUpdateBridge::new(
            Duration::from_millis(100),
            4,
            16,
            Duration::from_secs(60),
            32,
        );
        let now = Instant::now();

        let ingress = account_bridge.ingest_execution_account_evidence(&evidence_event, now);
        assert_eq!(
            ingress.decision,
            SessionExecutionAccountEvidenceDecision::BufferedUntilPoolDetected
        );
        assert!(tokio::time::timeout(Duration::from_millis(50), rx.recv())
            .await
            .is_err());

        let detected_ms = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        process_pool_detected_event_for_session_gate(
            &tx,
            &mut trade_bridge,
            &candidate,
            None,
            now + Duration::from_millis(1),
            detected_ms,
        );
        let flush =
            account_bridge.register_detected_pool(&candidate, now + Duration::from_millis(1));
        assert_eq!(flush.replay_ready_evidence.len(), 1);
        for evidence in &flush.replay_ready_evidence {
            emit_execution_account_evidence_to_event_bus(&tx, evidence, None);
        }

        let first = recv_only_event(&mut rx).await;
        match first {
            GhostEvent::NewPoolDetected(pool_event) => {
                assert_eq!(pool_event.pool_amm_id, pool.to_string());
            }
            other => panic!("expected NewPoolDetected, got {}", other.event_type()),
        }

        let second = recv_only_event(&mut rx).await;
        match second {
            GhostEvent::ExecutionAccountEvidence(observed) => {
                assert_eq!(observed.evidence.account_pubkey, bcv2);
                assert_eq!(
                    observed.evidence.role,
                    ghost_core::ExecutionAccountRole::BondingCurveV2
                );
            }
            GhostEvent::AccountUpdate(_) => {
                panic!("ExecutionAccountEvidence replay must not route through AccountUpdate")
            }
            other => panic!(
                "expected ExecutionAccountEvidence, got {}",
                other.event_type()
            ),
        }
    }

    #[tokio::test]
    async fn seer_execution_account_evidence_emits_ghost_event_without_account_update_path() {
        let (tx, mut rx) = create_event_bus();
        let evidence = make_execution_account_evidence();
        let evidence_event =
            SeerEvent::ExecutionAccountEvidence(DetectedExecutionAccountEvidenceEvent {
                evidence: evidence.clone(),
                detected_at: SystemTime::now(),
                sequence_number: 11,
                priority: EventPriority::High,
            });

        match evidence_event {
            SeerEvent::ExecutionAccountEvidence(event) => {
                emit_execution_account_evidence_to_event_bus(&tx, &event, None);
            }
            _ => unreachable!(),
        }

        let received = recv_only_event(&mut rx).await;
        match received {
            GhostEvent::ExecutionAccountEvidence(observed) => {
                assert_eq!(observed.evidence, evidence);
                assert_eq!(observed.sequence_number, 11);
                assert_eq!(
                    observed.evidence.role,
                    ghost_core::ExecutionAccountRole::BondingCurveV2
                );
            }
            GhostEvent::AccountUpdate(_) => {
                panic!("ExecutionAccountEvidence must not be routed through AccountUpdate")
            }
            other => panic!(
                "expected ExecutionAccountEvidence, got {}",
                other.event_type()
            ),
        }
    }
}

const PUMP_FUN_PROGRAM_ID_STR: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

fn is_known_pump_fun_program_id(value: &str) -> bool {
    value == PUMP_FUN_PROGRAM_ID_STR
}

fn pumpswap_program_id() -> &'static Pubkey {
    use std::sync::OnceLock;
    static PK: OnceLock<Pubkey> = OnceLock::new();
    PK.get_or_init(|| Pubkey::from_str(PUMPSWAP_PROGRAM_ID_STR).expect("valid pumpswap program ID"))
}
