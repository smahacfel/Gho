//! Bounded, append-only prospective evidence for RUG SCALP V2.
//!
//! This module is deliberately not an execution adapter and not a position
//! manager.  It observes already-canonical trades and the existing isolated
//! RUG hand-off, materialises typed Pump quote trajectories, and emits durable
//! evidence for later latency-freeze replay.  It has no API that can submit,
//! register, close, or otherwise mutate a position.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use ghost_core::market_state::BondingCurve;
use ghost_core::PumpQuoteV1;

use crate::events::{DetectedPool, PoolTransaction};
use crate::rug_scalp_v2::{
    pump_reserves, reserves_after_buy, RugScalpCanonicalStateV2, RugScalpEntryAssessmentV2,
    RugScalpEntryIntentV2, RugScalpNotionalQuoteV2, RugScalpPumpQuoteContractV1,
    RugScalpReasonCodeV2, RugScalpRuntimeFeeAuthorityManifestV1, RugScalpV2Config,
    RUG_SCALP_ENTRY_ROUTE, RUG_SCALP_EXIT_ROUTE,
};

const VALIDATION_TAPE_SCHEMA_VERSION: u16 = 1;
const MAX_VALIDATION_TAPE_EVENTS: usize = 256;
const MAX_VALIDATION_TAPE_SLOTS: u64 = 10; // eight hold slots plus STRESS_2 settlement.

/// Additive configuration for the one prospective validation sidecar.
///
/// The default is inert. `technical_capture` permits collecting latency before
/// its p90 slot profile is frozen; it never changes the RUG signal or PM exit
/// profile and it never enables a live lane.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RugScalpValidationTapeConfigV1 {
    pub enabled: bool,
    pub technical_capture: bool,
    pub log_path: String,
    pub run_id: String,
    pub code_hash: String,
}

impl Default for RugScalpValidationTapeConfigV1 {
    fn default() -> Self {
        Self {
            enabled: false,
            technical_capture: false,
            log_path: "logs/rug_scalp_v2/rug_scalp_validation_tape_v1.jsonl".to_string(),
            run_id: String::new(),
            code_hash: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RugScalpValidationRunContextV1 {
    pub run_id: String,
    pub config_hash: String,
    pub code_hash: String,
    pub binary_hash: String,
    pub runtime_fee_authority: Option<RugScalpRuntimeFeeAuthorityManifestV1>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RugScalpCanonicalOrderKeyV1 {
    pub slot: u64,
    pub tx_index: u32,
    pub event_ordinal: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RugScalpValidationCompletenessV1 {
    Complete,
    DataGap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RugScalpValidationTerminalStatusV1 {
    NonEvaluableFeeAuthority,
    DataInvalidated,
    EntryFailed,
    EntryUnknown,
    PositionClosed,
    ExitUnavailable,
    BoundedWindowComplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RugScalpValidationLatencyStageV1 {
    SignalReceived,
    QuoteMaterialized,
    EntryBuildCompleted,
    EntrySubmitReady,
    EntryModelledOrReachableLanding,
    ExitDecision,
    ExitQuoteMaterialized,
    ExitBuildCompleted,
    ExitModelledOrReachableLanding,
}

#[derive(Debug, Clone, Serialize)]
pub struct RugScalpValidationLatencyObservationV1 {
    pub stage: RugScalpValidationLatencyStageV1,
    pub observed_ingress_ms: u64,
    pub observed_slot: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RugScalpValidationNotionalStateV1 {
    pub notional_lamports: u64,
    pub buy_v2_entry_quote: Option<PumpQuoteV1>,
    pub legacy_sell_quote_for_fresh_entry: Option<PumpQuoteV1>,
    pub fresh_entry_total_debit_lamports: Option<u64>,
    pub fresh_entry_executable_wallet_credit_lamports: Option<u64>,
    pub fresh_entry_net_pnl_lamports: Option<i64>,
    pub tracked_entry_token_amount_raw: Option<u64>,
    pub tracked_entry_total_debit_lamports: Option<u64>,
    pub tracked_legacy_sell_quote: Option<PumpQuoteV1>,
    pub tracked_executable_wallet_credit_lamports: Option<u64>,
    pub tracked_net_pnl_lamports: Option<i64>,
    pub tracked_r_net_bps: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RugScalpValidationStateV1 {
    pub canonical_order_key: RugScalpCanonicalOrderKeyV1,
    pub state_slot: u64,
    pub ingress_timestamp_ms: u64,
    pub primary: RugScalpValidationNotionalStateV1,
    pub sensitivity: RugScalpValidationNotionalStateV1,
    pub reserve_drain_bps: Option<u32>,
    pub primary_executable_value_change_bps: Option<i32>,
    pub successful_buy_count_in_slot: u32,
    pub successful_sell_fact: bool,
    pub data_completeness: RugScalpValidationCompletenessV1,
}

#[derive(Debug, Clone, Serialize)]
pub struct RugScalpValidationAttemptIdentityV1 {
    pub run_id: String,
    pub candidate_id: String,
    pub mint: String,
    pub signal_id: String,
    pub position_id: Option<String>,
    pub signal_canonical_order_key: RugScalpCanonicalOrderKeyV1,
    pub config_hash: String,
    pub code_hash: String,
    pub binary_hash: String,
    pub entry_route_id: String,
    pub exit_route_id: String,
    pub buy_v2_fee_schedule_id: String,
    pub legacy_sell_fee_schedule_id: String,
    pub primary_notional_lamports: u64,
    pub sensitivity_notional_lamports: u64,
    /// Exact typed BuyV2 settlement at the accepted signal state. The later
    /// state rows must never substitute this frozen quote for their own
    /// canonical re-quotes.
    pub primary_signal_buy_v2_quote: Option<RugScalpNotionalQuoteV2>,
    pub sensitivity_signal_buy_v2_quote: Option<RugScalpNotionalQuoteV2>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RugScalpValidationTerminalV1 {
    pub status: RugScalpValidationTerminalStatusV1,
    pub terminal_reason: String,
    pub terminal_ingress_ms: u64,
    pub terminal_slot: Option<u64>,
    pub pm_owned_net_pnl_lamports: Option<i64>,
    pub state_count: usize,
    pub data_gap_seen: bool,
    pub first_target_slot: Option<u64>,
    pub first_hard_stop_slot: Option<u64>,
    pub first_material_sell_slot: Option<u64>,
    pub first_flow_stop_slot: Option<u64>,
    pub same_slot_dump_wins: bool,
    pub primary_mfe_lamports: Option<i64>,
    pub primary_mae_lamports: Option<i64>,
    pub time_to_target_ms: Option<u64>,
    pub time_to_dump_ms: Option<u64>,
}

/// Every record is append-only. A candidate has exactly one `attempt_terminal`
/// record; state and latency rows are evidence feeding that bounded terminal.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "record_type", rename_all = "snake_case")]
pub enum RugScalpValidationTapeRecordV1 {
    RunManifest {
        schema_version: u16,
        run: RugScalpValidationRunContextV1,
    },
    Birth {
        schema_version: u16,
        run_id: String,
        pool_id: String,
        mint: String,
        birth_slot: Option<u64>,
        ingress_timestamp_ms: u64,
    },
    AttemptStarted {
        schema_version: u16,
        attempt: RugScalpValidationAttemptIdentityV1,
        signal_ingress_ms: u64,
    },
    State {
        schema_version: u16,
        attempt: RugScalpValidationAttemptIdentityV1,
        state: RugScalpValidationStateV1,
    },
    Latency {
        schema_version: u16,
        attempt: RugScalpValidationAttemptIdentityV1,
        observation: RugScalpValidationLatencyObservationV1,
    },
    AttemptTerminal {
        schema_version: u16,
        attempt: RugScalpValidationAttemptIdentityV1,
        terminal: RugScalpValidationTerminalV1,
    },
}

#[derive(Debug, Clone)]
pub enum RugScalpValidationTapeEventV1 {
    EntryStage {
        candidate_id: String,
        stage: RugScalpValidationLatencyStageV1,
        observed_ingress_ms: u64,
        observed_slot: Option<u64>,
    },
    EntryTerminal {
        candidate_id: String,
        status: RugScalpValidationTerminalStatusV1,
        reason: String,
        observed_ingress_ms: u64,
    },
    PmTerminal {
        candidate_id: String,
        position_id: String,
        status: RugScalpValidationTerminalStatusV1,
        reason: String,
        exit_landed_slot: Option<u64>,
        pm_owned_net_pnl_lamports: Option<i64>,
        observed_ingress_ms: u64,
    },
}

/// A bounded, explicitly lossy only-with-invalidation hand-off from detached
/// shadow/PM tasks to the single tape owner in the Oracle event loop.
#[derive(Debug, Clone)]
pub struct RugScalpValidationTapeBusV1 {
    queue: Arc<Mutex<VecDeque<RugScalpValidationTapeEventV1>>>,
    overflowed: Arc<AtomicBool>,
}

impl RugScalpValidationTapeBusV1 {
    pub fn new() -> Self {
        Self {
            queue: Arc::new(Mutex::new(VecDeque::with_capacity(
                MAX_VALIDATION_TAPE_EVENTS,
            ))),
            overflowed: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn emit(&self, event: RugScalpValidationTapeEventV1) {
        let mut queue = self.queue.lock();
        if queue.len() >= MAX_VALIDATION_TAPE_EVENTS {
            self.overflowed.store(true, Ordering::Release);
            return;
        }
        queue.push_back(event);
    }

    fn drain(&self) -> (bool, Vec<RugScalpValidationTapeEventV1>) {
        let overflowed = self.overflowed.swap(false, Ordering::AcqRel);
        let mut queue = self.queue.lock();
        (overflowed, queue.drain(..).collect())
    }
}

impl Default for RugScalpValidationTapeBusV1 {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
struct ValidationEntryV1 {
    token_amount_raw: u64,
    total_debit_lamports: u64,
    opened_ingress_ms: u64,
    opened_slot: u64,
}

#[derive(Debug, Clone, Copy)]
struct SlotFlowV1 {
    slot: u64,
    successful_buy_count: u32,
    reserve_start: u64,
    primary_value_start: Option<u64>,
}

#[derive(Debug)]
struct ActiveAttemptV1 {
    identity: RugScalpValidationAttemptIdentityV1,
    signal_ingress_ms: u64,
    signal_slot: u64,
    primary_signal_entry: ValidationEntryV1,
    sensitivity_signal_entry: ValidationEntryV1,
    primary_modelled_entry: Option<ValidationEntryV1>,
    last_order: Option<RugScalpCanonicalOrderKeyV1>,
    seen_signatures: HashSet<String>,
    last_curve: Option<BondingCurve>,
    current_slot: Option<SlotFlowV1>,
    empty_complete_slots: u8,
    state_count: usize,
    data_gap_seen: bool,
    first_target_slot: Option<u64>,
    first_target_ingress_ms: Option<u64>,
    first_hard_stop_slot: Option<u64>,
    first_material_sell_slot: Option<u64>,
    first_material_sell_ingress_ms: Option<u64>,
    first_flow_stop_slot: Option<u64>,
    same_slot_dump_wins: bool,
    primary_mfe_lamports: Option<i64>,
    primary_mae_lamports: Option<i64>,
}

/// The only mutable owner of validation trajectory state. It is called from
/// the Oracle event loop; it never communicates with Trigger or PM directly.
#[derive(Debug)]
pub struct RugScalpValidationTapeV1 {
    config: RugScalpValidationTapeConfigV1,
    context: RugScalpValidationRunContextV1,
    quote_contract: Option<RugScalpPumpQuoteContractV1>,
    attempts_by_candidate: HashMap<String, ActiveAttemptV1>,
    candidate_by_mint: HashMap<String, String>,
}

impl RugScalpValidationTapeV1 {
    pub fn new(config: &RugScalpV2Config, context: RugScalpValidationRunContextV1) -> Self {
        Self {
            config: config.validation_tape.clone(),
            context,
            quote_contract: config
                .pump_quote_authority
                .as_ref()
                .and_then(|authority| authority.materialize().ok()),
            attempts_by_candidate: HashMap::new(),
            candidate_by_mint: HashMap::new(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    /// The manifest is emitted once before the first birth/assessment. It
    /// binds every later schedule ID to the verified runtime account snapshot
    /// without promoting the tape into an authority owner.
    pub fn run_manifest_record(&self) -> RugScalpValidationTapeRecordV1 {
        RugScalpValidationTapeRecordV1::RunManifest {
            schema_version: VALIDATION_TAPE_SCHEMA_VERSION,
            run: self.context.clone(),
        }
    }

    /// Logs the prospective denominator without participating in reducer
    /// state, admission, or position management.
    pub fn on_birth(
        &self,
        pool: &DetectedPool,
        ingress_timestamp_ms: u64,
    ) -> Vec<RugScalpValidationTapeRecordV1> {
        self.enabled()
            .then(|| RugScalpValidationTapeRecordV1::Birth {
                schema_version: VALIDATION_TAPE_SCHEMA_VERSION,
                run_id: self.context.run_id.clone(),
                pool_id: pool.pool_amm_id.clone(),
                mint: pool.base_mint.clone(),
                birth_slot: pool.slot,
                ingress_timestamp_ms,
            })
            .into_iter()
            .collect()
    }

    /// A source/disconnect gap invalidates every still-open attempt. It does
    /// not synthesize an empty slot or guess a terminal market state.
    pub fn mark_stream_gap(&mut self, reason: &str) -> Vec<RugScalpValidationTapeRecordV1> {
        let ids: Vec<String> = self.attempts_by_candidate.keys().cloned().collect();
        let now = current_time_ms();
        ids.into_iter()
            .flat_map(|candidate_id| {
                self.finish_candidate(
                    candidate_id,
                    RugScalpValidationTerminalStatusV1::DataInvalidated,
                    reason.to_string(),
                    None,
                    None,
                    now,
                )
            })
            .collect()
    }

    /// Records the only non-evaluable outcome that can be known from an
    /// assessment itself. Accepted attempts start only from the accompanying
    /// entry intent, preserving its exact canonical trade order key.
    pub fn on_assessment(
        &mut self,
        assessment: &RugScalpEntryAssessmentV2,
        signal_order: RugScalpCanonicalOrderKeyV1,
    ) -> Vec<RugScalpValidationTapeRecordV1> {
        if !self.enabled() {
            return Vec::new();
        }
        if matches!(
            assessment.reason,
            RugScalpReasonCodeV2::QuoteMathUnavailable
        ) {
            return self.non_evaluable_fee_authority(assessment, signal_order);
        }
        Vec::new()
    }

    /// Begins one append-only validation attempt after the existing reducer
    /// has accepted a signal. The tape receives the canonical order key from
    /// the originating trade; it never fabricates tx/event ordering.
    pub fn on_accepted_intent(
        &mut self,
        intent: &RugScalpEntryIntentV2,
        signal_order: RugScalpCanonicalOrderKeyV1,
    ) -> Vec<RugScalpValidationTapeRecordV1> {
        if !self.enabled()
            || self
                .attempts_by_candidate
                .contains_key(&intent.candidate_id)
        {
            return Vec::new();
        }
        let assessment = &intent.assessment;
        let Some(primary) = assessment.primary_quote.as_ref() else {
            return self.non_evaluable_fee_authority(assessment, signal_order);
        };
        let Some(sensitivity) = assessment.sensitivity_quote.as_ref() else {
            return self.non_evaluable_fee_authority(assessment, signal_order);
        };

        let primary_entry = ValidationEntryV1 {
            token_amount_raw: primary.entry_token_amount_raw,
            total_debit_lamports: primary
                .entry_wallet_debit_lamports
                .saturating_add(primary.entry_transaction_cost_lamports),
            opened_ingress_ms: assessment.signal_ingress_ms,
            opened_slot: signal_order.slot,
        };
        let sensitivity_entry = ValidationEntryV1 {
            token_amount_raw: sensitivity.entry_token_amount_raw,
            total_debit_lamports: sensitivity
                .entry_wallet_debit_lamports
                .saturating_add(sensitivity.entry_transaction_cost_lamports),
            opened_ingress_ms: assessment.signal_ingress_ms,
            opened_slot: signal_order.slot,
        };
        let identity = RugScalpValidationAttemptIdentityV1 {
            run_id: self.context.run_id.clone(),
            candidate_id: intent.candidate_id.clone(),
            mint: assessment.mint.clone(),
            signal_id: format!(
                "{}:{}:{}:{}",
                assessment.mint,
                signal_order.slot,
                signal_order.tx_index,
                signal_order.event_ordinal
            ),
            position_id: None,
            signal_canonical_order_key: signal_order,
            config_hash: self.context.config_hash.clone(),
            code_hash: self.context.code_hash.clone(),
            binary_hash: self.context.binary_hash.clone(),
            entry_route_id: primary.entry_route_id.clone(),
            exit_route_id: primary.exit_route_id.clone(),
            buy_v2_fee_schedule_id: primary.entry_fee_schedule_id.clone(),
            legacy_sell_fee_schedule_id: primary.exit_fee_schedule_id.clone(),
            primary_notional_lamports: primary.notional_lamports,
            sensitivity_notional_lamports: sensitivity.notional_lamports,
            primary_signal_buy_v2_quote: Some(primary.clone()),
            sensitivity_signal_buy_v2_quote: Some(sensitivity.clone()),
        };
        let candidate_id = intent.candidate_id.clone();
        self.candidate_by_mint
            .insert(assessment.mint.clone(), candidate_id.clone());
        self.attempts_by_candidate.insert(
            candidate_id,
            ActiveAttemptV1 {
                identity: identity.clone(),
                signal_ingress_ms: assessment.signal_ingress_ms,
                signal_slot: signal_order.slot,
                primary_signal_entry: primary_entry,
                sensitivity_signal_entry: sensitivity_entry,
                primary_modelled_entry: None,
                last_order: Some(signal_order),
                seen_signatures: HashSet::new(),
                last_curve: None,
                current_slot: None,
                empty_complete_slots: 0,
                state_count: 0,
                data_gap_seen: false,
                first_target_slot: None,
                first_target_ingress_ms: None,
                first_hard_stop_slot: None,
                first_material_sell_slot: None,
                first_material_sell_ingress_ms: None,
                first_flow_stop_slot: None,
                same_slot_dump_wins: false,
                primary_mfe_lamports: None,
                primary_mae_lamports: None,
            },
        );
        vec![
            RugScalpValidationTapeRecordV1::AttemptStarted {
                schema_version: VALIDATION_TAPE_SCHEMA_VERSION,
                attempt: identity.clone(),
                signal_ingress_ms: assessment.signal_ingress_ms,
            },
            RugScalpValidationTapeRecordV1::Latency {
                schema_version: VALIDATION_TAPE_SCHEMA_VERSION,
                attempt: identity.clone(),
                observation: RugScalpValidationLatencyObservationV1 {
                    stage: RugScalpValidationLatencyStageV1::SignalReceived,
                    observed_ingress_ms: assessment.signal_ingress_ms,
                    observed_slot: Some(signal_order.slot),
                },
            },
            RugScalpValidationTapeRecordV1::Latency {
                schema_version: VALIDATION_TAPE_SCHEMA_VERSION,
                attempt: identity,
                observation: RugScalpValidationLatencyObservationV1 {
                    stage: RugScalpValidationLatencyStageV1::QuoteMaterialized,
                    observed_ingress_ms: assessment.signal_ingress_ms,
                    observed_slot: Some(signal_order.slot),
                },
            },
        ]
    }

    pub fn observe_trade(
        &mut self,
        tx: &PoolTransaction,
        ingress_timestamp_ms: u64,
        canonical: RugScalpCanonicalStateV2,
        curve: Option<BondingCurve>,
    ) -> Vec<RugScalpValidationTapeRecordV1> {
        let Some(mint) = tx.token_mint.as_ref() else {
            return Vec::new();
        };
        let Some(candidate_id) = self.candidate_by_mint.get(mint).cloned() else {
            return Vec::new();
        };
        let Some(mut attempt) = self.attempts_by_candidate.remove(&candidate_id) else {
            return Vec::new();
        };
        let records =
            self.observe_attempt_trade(&mut attempt, tx, ingress_timestamp_ms, canonical, curve);
        let terminal_emitted = records.iter().any(|record| {
            matches!(
                record,
                RugScalpValidationTapeRecordV1::AttemptTerminal { .. }
            )
        });
        if !terminal_emitted {
            self.candidate_by_mint
                .insert(mint.clone(), candidate_id.clone());
            self.attempts_by_candidate.insert(candidate_id, attempt);
        }
        records
    }

    pub fn drain_external_events(
        &mut self,
        bus: &RugScalpValidationTapeBusV1,
    ) -> Vec<RugScalpValidationTapeRecordV1> {
        let (overflowed, events) = bus.drain();
        let mut records = Vec::new();
        if overflowed {
            let ids: Vec<String> = self.attempts_by_candidate.keys().cloned().collect();
            for candidate_id in ids {
                if let Some(attempt) = self.attempts_by_candidate.remove(&candidate_id) {
                    self.candidate_by_mint.remove(&attempt.identity.mint);
                    records.push(self.terminal_record(
                        attempt,
                        RugScalpValidationTerminalStatusV1::DataInvalidated,
                        "validation_telemetry_bus_overflow".to_string(),
                        None,
                        None,
                        current_time_ms(),
                    ));
                }
            }
        }
        for event in events {
            records.extend(self.apply_external_event(event));
        }
        records
    }

    fn apply_external_event(
        &mut self,
        event: RugScalpValidationTapeEventV1,
    ) -> Vec<RugScalpValidationTapeRecordV1> {
        match event {
            RugScalpValidationTapeEventV1::EntryStage {
                candidate_id,
                stage,
                observed_ingress_ms,
                observed_slot,
            } => self
                .attempts_by_candidate
                .get(&candidate_id)
                .map(|attempt| {
                    vec![RugScalpValidationTapeRecordV1::Latency {
                        schema_version: VALIDATION_TAPE_SCHEMA_VERSION,
                        attempt: attempt.identity.clone(),
                        observation: RugScalpValidationLatencyObservationV1 {
                            stage,
                            observed_ingress_ms,
                            observed_slot,
                        },
                    }]
                })
                .unwrap_or_default(),
            RugScalpValidationTapeEventV1::EntryTerminal {
                candidate_id,
                status,
                reason,
                observed_ingress_ms,
            } => self.finish_candidate(
                candidate_id,
                status,
                reason,
                None,
                None,
                observed_ingress_ms,
            ),
            RugScalpValidationTapeEventV1::PmTerminal {
                candidate_id,
                position_id,
                status,
                reason,
                exit_landed_slot,
                pm_owned_net_pnl_lamports,
                observed_ingress_ms,
            } => {
                if let Some(attempt) = self.attempts_by_candidate.get_mut(&candidate_id) {
                    attempt.identity.position_id = Some(position_id);
                }
                self.finish_candidate(
                    candidate_id,
                    status,
                    reason,
                    exit_landed_slot,
                    pm_owned_net_pnl_lamports,
                    observed_ingress_ms,
                )
            }
        }
    }

    /// PM ACK is evidence only: it binds the already-existing primary modelled
    /// fill to the tape and never registers a position itself.
    pub fn bind_modelled_entry(
        &mut self,
        candidate_id: &str,
        position_id: String,
        entry_token_amount_raw: u64,
        entry_total_debit_lamports: u64,
        entry_slot: u64,
        observed_ingress_ms: u64,
    ) -> Vec<RugScalpValidationTapeRecordV1> {
        let Some(attempt) = self.attempts_by_candidate.get_mut(candidate_id) else {
            return Vec::new();
        };
        attempt.identity.position_id = Some(position_id);
        attempt.primary_modelled_entry = Some(ValidationEntryV1 {
            token_amount_raw: entry_token_amount_raw,
            total_debit_lamports: entry_total_debit_lamports,
            opened_ingress_ms: observed_ingress_ms,
            opened_slot: entry_slot,
        });
        vec![RugScalpValidationTapeRecordV1::Latency {
            schema_version: VALIDATION_TAPE_SCHEMA_VERSION,
            attempt: attempt.identity.clone(),
            observation: RugScalpValidationLatencyObservationV1 {
                stage: RugScalpValidationLatencyStageV1::EntryModelledOrReachableLanding,
                observed_ingress_ms,
                observed_slot: Some(entry_slot),
            },
        }]
    }

    /// Enforces the five-second bound even when a dead pool emits no later
    /// trade.  This is a local observer timeout only: it cannot synthesize a
    /// slot completion, an empty-flow fact, an exit intent, or a PM outcome.
    pub fn on_clock(&mut self, ingress_timestamp_ms: u64) -> Vec<RugScalpValidationTapeRecordV1> {
        let due: Vec<String> = self
            .attempts_by_candidate
            .iter()
            .filter_map(|(candidate_id, attempt)| {
                attempt.primary_modelled_entry.as_ref().and_then(|entry| {
                    (ingress_timestamp_ms.saturating_sub(entry.opened_ingress_ms) >= 5_000)
                        .then(|| candidate_id.clone())
                })
            })
            .collect();
        due.into_iter()
            .flat_map(|candidate_id| {
                self.finish_candidate(
                    candidate_id,
                    RugScalpValidationTerminalStatusV1::BoundedWindowComplete,
                    "validation_window_complete_wall_clock".to_string(),
                    None,
                    None,
                    ingress_timestamp_ms,
                )
            })
            .collect()
    }

    fn observe_attempt_trade(
        &self,
        attempt: &mut ActiveAttemptV1,
        tx: &PoolTransaction,
        ingress_timestamp_ms: u64,
        canonical: RugScalpCanonicalStateV2,
        curve: Option<BondingCurve>,
    ) -> Vec<RugScalpValidationTapeRecordV1> {
        let Some(order) = order_from_trade(tx) else {
            attempt.data_gap_seen = true;
            return vec![self.terminal_from_attempt(
                attempt,
                RugScalpValidationTerminalStatusV1::DataInvalidated,
                "missing_canonical_order_key".to_string(),
                None,
                None,
                ingress_timestamp_ms,
            )];
        };
        if !canonical.state_clean || !canonical.ordering_known || canonical.accepted_window_has_gap
        {
            attempt.data_gap_seen = true;
            return vec![self.terminal_from_attempt(
                attempt,
                RugScalpValidationTerminalStatusV1::DataInvalidated,
                "canonical_state_or_ordering_gap".to_string(),
                Some(order.slot),
                None,
                ingress_timestamp_ms,
            )];
        }
        if !attempt.seen_signatures.insert(tx.signature.clone()) {
            return Vec::new();
        }
        if attempt.last_order.is_some_and(|previous| order <= previous) {
            attempt.data_gap_seen = true;
            return vec![self.terminal_from_attempt(
                attempt,
                RugScalpValidationTerminalStatusV1::DataInvalidated,
                "reordered_canonical_trade".to_string(),
                Some(order.slot),
                None,
                ingress_timestamp_ms,
            )];
        }
        if attempt
            .last_order
            .is_some_and(|previous| order.slot > previous.slot.saturating_add(1))
        {
            attempt.data_gap_seen = true;
            return vec![self.terminal_from_attempt(
                attempt,
                RugScalpValidationTerminalStatusV1::DataInvalidated,
                "canonical_slot_gap".to_string(),
                Some(order.slot),
                None,
                ingress_timestamp_ms,
            )];
        }
        let Some(curve) = curve else {
            attempt.data_gap_seen = true;
            return vec![self.terminal_from_attempt(
                attempt,
                RugScalpValidationTerminalStatusV1::DataInvalidated,
                "canonical_curve_unavailable".to_string(),
                Some(order.slot),
                None,
                ingress_timestamp_ms,
            )];
        };
        let Some(reserves) = pump_reserves(curve) else {
            attempt.data_gap_seen = true;
            return vec![self.terminal_from_attempt(
                attempt,
                RugScalpValidationTerminalStatusV1::DataInvalidated,
                "canonical_curve_inactive".to_string(),
                Some(order.slot),
                None,
                ingress_timestamp_ms,
            )];
        };
        let Some(contract) = self.quote_contract.as_ref() else {
            return vec![self.terminal_from_attempt(
                attempt,
                RugScalpValidationTerminalStatusV1::NonEvaluableFeeAuthority,
                "runtime_fee_authority_unavailable".to_string(),
                Some(order.slot),
                None,
                ingress_timestamp_ms,
            )];
        };

        let mut records = Vec::new();
        if let Some(previous_slot) = attempt.current_slot {
            if order.slot > previous_slot.slot {
                self.close_slot(attempt, previous_slot, ingress_timestamp_ms);
                attempt.current_slot = None;
            }
        }
        let primary_before = attempt
            .primary_modelled_entry
            .as_ref()
            .or(Some(&attempt.primary_signal_entry))
            .and_then(|entry| {
                contract
                    .executable_exit_value_lamports(order.slot, reserves, entry.token_amount_raw)
                    .ok()
                    .map(|(_, value)| value)
            });
        let slot_flow = attempt.current_slot.get_or_insert(SlotFlowV1 {
            slot: order.slot,
            successful_buy_count: 0,
            reserve_start: reserves.virtual_quote_reserves,
            primary_value_start: primary_before,
        });
        if tx.success && tx.is_buy {
            slot_flow.successful_buy_count = slot_flow.successful_buy_count.saturating_add(1);
        }

        let primary = validation_notional_state(
            contract,
            order.slot,
            reserves,
            attempt.identity.primary_notional_lamports,
            attempt
                .primary_modelled_entry
                .as_ref()
                .or(Some(&attempt.primary_signal_entry)),
        );
        let sensitivity = validation_notional_state(
            contract,
            order.slot,
            reserves,
            attempt.identity.sensitivity_notional_lamports,
            Some(&attempt.sensitivity_signal_entry),
        );
        let reserve_drain_bps = drain_bps(slot_flow.reserve_start, reserves.virtual_quote_reserves);
        let primary_value_change_bps = slot_flow
            .primary_value_start
            .zip(primary.tracked_executable_wallet_credit_lamports)
            .and_then(|(before, after)| signed_change_bps(before, after));
        if let Some(pnl) = primary.tracked_net_pnl_lamports {
            attempt.primary_mfe_lamports =
                Some(attempt.primary_mfe_lamports.map_or(pnl, |max| max.max(pnl)));
            attempt.primary_mae_lamports =
                Some(attempt.primary_mae_lamports.map_or(pnl, |min| min.min(pnl)));
            if primary.tracked_r_net_bps.is_some_and(|bps| bps >= 1_000)
                && attempt.first_target_slot.is_none()
            {
                attempt.first_target_slot = Some(order.slot);
                attempt.first_target_ingress_ms = Some(ingress_timestamp_ms);
            }
            if primary.tracked_r_net_bps.is_some_and(|bps| bps <= -500)
                && attempt.first_hard_stop_slot.is_none()
            {
                attempt.first_hard_stop_slot = Some(order.slot);
            }
        }
        let state = RugScalpValidationStateV1 {
            canonical_order_key: order,
            state_slot: order.slot,
            ingress_timestamp_ms,
            primary,
            sensitivity,
            reserve_drain_bps,
            primary_executable_value_change_bps: primary_value_change_bps,
            successful_buy_count_in_slot: slot_flow.successful_buy_count,
            successful_sell_fact: tx.success && !tx.is_buy,
            data_completeness: RugScalpValidationCompletenessV1::Complete,
        };
        attempt.state_count = attempt.state_count.saturating_add(1);
        attempt.last_order = Some(order);
        attempt.last_curve = Some(curve);
        records.push(RugScalpValidationTapeRecordV1::State {
            schema_version: VALIDATION_TAPE_SCHEMA_VERSION,
            attempt: attempt.identity.clone(),
            state,
        });

        if let Some(entry) = attempt.primary_modelled_entry.as_ref() {
            if order.slot >= entry.opened_slot.saturating_add(MAX_VALIDATION_TAPE_SLOTS)
                || ingress_timestamp_ms.saturating_sub(entry.opened_ingress_ms) >= 5_000
            {
                records.push(self.terminal_from_attempt(
                    attempt,
                    RugScalpValidationTerminalStatusV1::BoundedWindowComplete,
                    "validation_window_complete".to_string(),
                    Some(order.slot),
                    None,
                    ingress_timestamp_ms,
                ));
            }
        }
        records
    }

    fn close_slot(&self, attempt: &mut ActiveAttemptV1, slot: SlotFlowV1, ingress_ms: u64) {
        if slot.successful_buy_count == 0 {
            attempt.empty_complete_slots = attempt.empty_complete_slots.saturating_add(1);
            if attempt.empty_complete_slots >= 2 && attempt.first_flow_stop_slot.is_none() {
                attempt.first_flow_stop_slot = Some(slot.slot);
            }
        } else {
            attempt.empty_complete_slots = 0;
        }
        let Some(last_curve) = attempt.last_curve else {
            return;
        };
        let Some(reserves) = pump_reserves(last_curve) else {
            return;
        };
        let Some(contract) = self.quote_contract.as_ref() else {
            return;
        };
        let primary = attempt
            .primary_modelled_entry
            .as_ref()
            .or(Some(&attempt.primary_signal_entry));
        let value_after = primary.and_then(|entry| {
            contract
                .executable_exit_value_lamports(slot.slot, reserves, entry.token_amount_raw)
                .ok()
                .map(|(_, value)| value)
        });
        let material = drain_bps(slot.reserve_start, reserves.virtual_quote_reserves)
            .is_some_and(|bps| bps >= 500)
            || slot
                .primary_value_start
                .zip(value_after)
                .and_then(|(before, after)| signed_change_bps(before, after))
                .is_some_and(|bps| bps <= -1_500);
        if material && attempt.first_material_sell_slot.is_none() {
            attempt.first_material_sell_slot = Some(slot.slot);
            attempt.first_material_sell_ingress_ms = Some(ingress_ms);
            if attempt.first_target_slot == Some(slot.slot) {
                attempt.same_slot_dump_wins = true;
            }
        }
    }

    fn non_evaluable_fee_authority(
        &self,
        assessment: &RugScalpEntryAssessmentV2,
        signal_order: RugScalpCanonicalOrderKeyV1,
    ) -> Vec<RugScalpValidationTapeRecordV1> {
        let identity = non_evaluable_identity(&self.context, assessment, signal_order);
        vec![
            RugScalpValidationTapeRecordV1::AttemptStarted {
                schema_version: VALIDATION_TAPE_SCHEMA_VERSION,
                attempt: identity.clone(),
                signal_ingress_ms: assessment.signal_ingress_ms,
            },
            RugScalpValidationTapeRecordV1::AttemptTerminal {
                schema_version: VALIDATION_TAPE_SCHEMA_VERSION,
                attempt: identity,
                terminal: RugScalpValidationTerminalV1 {
                    status: RugScalpValidationTerminalStatusV1::NonEvaluableFeeAuthority,
                    terminal_reason: "runtime_fee_authority_unavailable".to_string(),
                    terminal_ingress_ms: assessment.signal_ingress_ms,
                    terminal_slot: Some(signal_order.slot),
                    pm_owned_net_pnl_lamports: None,
                    state_count: 0,
                    data_gap_seen: false,
                    first_target_slot: None,
                    first_hard_stop_slot: None,
                    first_material_sell_slot: None,
                    first_flow_stop_slot: None,
                    same_slot_dump_wins: false,
                    primary_mfe_lamports: None,
                    primary_mae_lamports: None,
                    time_to_target_ms: None,
                    time_to_dump_ms: None,
                },
            },
        ]
    }

    /// Records one terminal validation disposition for an existing attempt.
    /// This is evidence-only and cannot issue an execution or PM command.
    pub fn finish_candidate(
        &mut self,
        candidate_id: String,
        status: RugScalpValidationTerminalStatusV1,
        reason: String,
        terminal_slot: Option<u64>,
        pm_owned_net_pnl_lamports: Option<i64>,
        terminal_ingress_ms: u64,
    ) -> Vec<RugScalpValidationTapeRecordV1> {
        let Some(attempt) = self.attempts_by_candidate.remove(&candidate_id) else {
            return Vec::new();
        };
        self.candidate_by_mint.remove(&attempt.identity.mint);
        vec![self.terminal_record(
            attempt,
            status,
            reason,
            terminal_slot,
            pm_owned_net_pnl_lamports,
            terminal_ingress_ms,
        )]
    }

    fn terminal_from_attempt(
        &self,
        attempt: &ActiveAttemptV1,
        status: RugScalpValidationTerminalStatusV1,
        reason: String,
        terminal_slot: Option<u64>,
        pm_owned_net_pnl_lamports: Option<i64>,
        terminal_ingress_ms: u64,
    ) -> RugScalpValidationTapeRecordV1 {
        self.terminal_record(
            attempt.clone_for_terminal(),
            status,
            reason,
            terminal_slot,
            pm_owned_net_pnl_lamports,
            terminal_ingress_ms,
        )
    }

    fn terminal_record(
        &self,
        attempt: ActiveAttemptV1,
        status: RugScalpValidationTerminalStatusV1,
        reason: String,
        terminal_slot: Option<u64>,
        pm_owned_net_pnl_lamports: Option<i64>,
        terminal_ingress_ms: u64,
    ) -> RugScalpValidationTapeRecordV1 {
        let entry_ingress = attempt
            .primary_modelled_entry
            .as_ref()
            .map(|entry| entry.opened_ingress_ms)
            .unwrap_or(attempt.signal_ingress_ms);
        RugScalpValidationTapeRecordV1::AttemptTerminal {
            schema_version: VALIDATION_TAPE_SCHEMA_VERSION,
            attempt: attempt.identity,
            terminal: RugScalpValidationTerminalV1 {
                status,
                terminal_reason: reason,
                terminal_ingress_ms,
                terminal_slot,
                pm_owned_net_pnl_lamports,
                state_count: attempt.state_count,
                data_gap_seen: attempt.data_gap_seen,
                first_target_slot: attempt.first_target_slot,
                first_hard_stop_slot: attempt.first_hard_stop_slot,
                first_material_sell_slot: attempt.first_material_sell_slot,
                first_flow_stop_slot: attempt.first_flow_stop_slot,
                same_slot_dump_wins: attempt.same_slot_dump_wins,
                primary_mfe_lamports: attempt.primary_mfe_lamports,
                primary_mae_lamports: attempt.primary_mae_lamports,
                time_to_target_ms: attempt
                    .first_target_ingress_ms
                    .map(|target| target.saturating_sub(entry_ingress)),
                time_to_dump_ms: attempt
                    .first_material_sell_ingress_ms
                    .map(|dump| dump.saturating_sub(entry_ingress)),
            },
        }
    }
}

impl ActiveAttemptV1 {
    fn clone_for_terminal(&self) -> Self {
        Self {
            identity: self.identity.clone(),
            signal_ingress_ms: self.signal_ingress_ms,
            signal_slot: self.signal_slot,
            primary_signal_entry: self.primary_signal_entry.clone(),
            sensitivity_signal_entry: self.sensitivity_signal_entry.clone(),
            primary_modelled_entry: self.primary_modelled_entry.clone(),
            last_order: self.last_order,
            seen_signatures: self.seen_signatures.clone(),
            last_curve: self.last_curve,
            current_slot: self.current_slot,
            empty_complete_slots: self.empty_complete_slots,
            state_count: self.state_count,
            data_gap_seen: self.data_gap_seen,
            first_target_slot: self.first_target_slot,
            first_target_ingress_ms: self.first_target_ingress_ms,
            first_hard_stop_slot: self.first_hard_stop_slot,
            first_material_sell_slot: self.first_material_sell_slot,
            first_material_sell_ingress_ms: self.first_material_sell_ingress_ms,
            first_flow_stop_slot: self.first_flow_stop_slot,
            same_slot_dump_wins: self.same_slot_dump_wins,
            primary_mfe_lamports: self.primary_mfe_lamports,
            primary_mae_lamports: self.primary_mae_lamports,
        }
    }
}

fn validation_notional_state(
    contract: &RugScalpPumpQuoteContractV1,
    slot: u64,
    reserves: ghost_core::PumpReserveState,
    notional_lamports: u64,
    tracked_entry: Option<&ValidationEntryV1>,
) -> RugScalpValidationNotionalStateV1 {
    let entry_quote = contract
        .quote_buy_v2_under_wallet_cap(slot, reserves, notional_lamports)
        .ok();
    let fresh = entry_quote.as_ref().and_then(|quote| {
        let post_entry = reserves_after_buy(reserves, quote);
        contract
            .executable_exit_value_lamports(slot, post_entry, quote.token_amount)
            .ok()
            .map(|(sell, credit)| {
                let total_debit = quote
                    .program_settlement
                    .wallet_debit_or_credit
                    .saturating_add(
                        contract
                            .entry_transaction_cost_lamports()
                            .unwrap_or_default(),
                    );
                (sell, total_debit, credit)
            })
    });
    let tracked = tracked_entry.and_then(|entry| {
        contract
            .executable_exit_value_lamports(slot, reserves, entry.token_amount_raw)
            .ok()
            .map(|(sell, credit)| (sell, credit, entry.total_debit_lamports))
    });
    let fresh_entry_total_debit_lamports = fresh.as_ref().map(|(_, debit, _)| *debit);
    let fresh_entry_executable_wallet_credit_lamports =
        fresh.as_ref().map(|(_, _, credit)| *credit);
    let fresh_entry_net_pnl_lamports = fresh
        .as_ref()
        .and_then(|(_, debit, credit)| signed_pnl(*credit, *debit));
    let (
        tracked_legacy_sell_quote,
        tracked_executable_wallet_credit_lamports,
        tracked_entry_total_debit_lamports,
    ) = match tracked {
        Some((sell, credit, debit)) => (Some(sell), Some(credit), Some(debit)),
        None => (None, None, None),
    };
    let tracked_net_pnl_lamports = tracked_executable_wallet_credit_lamports
        .zip(tracked_entry_total_debit_lamports)
        .and_then(|(credit, debit)| signed_pnl(credit, debit));
    let tracked_r_net_bps = tracked_net_pnl_lamports
        .zip(tracked_entry_total_debit_lamports)
        .and_then(|(pnl, debit)| r_net_bps(pnl, debit));
    RugScalpValidationNotionalStateV1 {
        notional_lamports,
        buy_v2_entry_quote: entry_quote,
        legacy_sell_quote_for_fresh_entry: fresh.as_ref().map(|(sell, _, _)| sell.clone()),
        fresh_entry_total_debit_lamports,
        fresh_entry_executable_wallet_credit_lamports,
        fresh_entry_net_pnl_lamports,
        tracked_entry_token_amount_raw: tracked_entry.map(|entry| entry.token_amount_raw),
        tracked_entry_total_debit_lamports,
        tracked_legacy_sell_quote,
        tracked_executable_wallet_credit_lamports,
        tracked_net_pnl_lamports,
        tracked_r_net_bps,
    }
}

fn order_from_trade(tx: &PoolTransaction) -> Option<RugScalpCanonicalOrderKeyV1> {
    Some(RugScalpCanonicalOrderKeyV1 {
        slot: tx.slot?,
        tx_index: tx.tx_index?,
        event_ordinal: tx.event_ordinal?,
    })
}

fn non_evaluable_identity(
    context: &RugScalpValidationRunContextV1,
    assessment: &RugScalpEntryAssessmentV2,
    signal_order: RugScalpCanonicalOrderKeyV1,
) -> RugScalpValidationAttemptIdentityV1 {
    RugScalpValidationAttemptIdentityV1 {
        run_id: context.run_id.clone(),
        candidate_id: format!("non-evaluable:{}:{}", assessment.mint, signal_order.slot),
        mint: assessment.mint.clone(),
        signal_id: format!("{}:{}", assessment.mint, signal_order.slot),
        position_id: None,
        signal_canonical_order_key: signal_order,
        config_hash: context.config_hash.clone(),
        code_hash: context.code_hash.clone(),
        binary_hash: context.binary_hash.clone(),
        entry_route_id: RUG_SCALP_ENTRY_ROUTE.as_str().to_string(),
        exit_route_id: RUG_SCALP_EXIT_ROUTE.as_str().to_string(),
        buy_v2_fee_schedule_id: String::new(),
        legacy_sell_fee_schedule_id: String::new(),
        primary_notional_lamports: 100_000_000,
        sensitivity_notional_lamports: 200_000_000,
        primary_signal_buy_v2_quote: None,
        sensitivity_signal_buy_v2_quote: None,
    }
}

fn signed_pnl(credit: u64, debit: u64) -> Option<i64> {
    let credit = i64::try_from(credit).ok()?;
    let debit = i64::try_from(debit).ok()?;
    Some(credit.saturating_sub(debit))
}

fn r_net_bps(pnl: i64, debit: u64) -> Option<i32> {
    let debit = i128::from(debit);
    (debit > 0).then(|| ((i128::from(pnl) * 10_000) / debit) as i32)
}

fn signed_change_bps(before: u64, after: u64) -> Option<i32> {
    (before > 0).then(|| {
        let delta = i128::from(after).saturating_sub(i128::from(before));
        ((delta * 10_000) / i128::from(before)) as i32
    })
}

fn drain_bps(before: u64, after: u64) -> Option<u32> {
    (before > after && before > 0)
        .then(|| ((u128::from(before - after) * 10_000) / u128::from(before)) as u32)
}

fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rug_scalp_v2::{RugScalpPumpFeeScheduleV1, RugScalpPumpQuoteAuthorityV1};
    use ghost_core::{market_state::BondingCurve, TransactionCosts};
    use ghost_core::{
        FeeRounding, ProgramFeeRule, ProgramFeeSchedule, ProgramFeeScheduleEvidenceV1,
        PumpRouteVariant,
    };

    fn signal_order() -> RugScalpCanonicalOrderKeyV1 {
        RugScalpCanonicalOrderKeyV1 {
            slot: 12,
            tx_index: 4,
            event_ordinal: 0,
        }
    }

    fn schedule(route: PumpRouteVariant, id: &str) -> RugScalpPumpFeeScheduleV1 {
        RugScalpPumpFeeScheduleV1 {
            route_variant: route,
            schedule: ProgramFeeSchedule {
                fee_schedule_id: id.to_string(),
                effective_slot: 1,
                evidence: ProgramFeeScheduleEvidenceV1::OnChainConfig {
                    config_pubkey: "fee-config".to_string(),
                    owner_program: "fee-program".to_string(),
                    account_data_hash: format!("hash-{id}"),
                    observed_slot: 1,
                },
                rules: vec![ProgramFeeRule {
                    component_id: "protocol".to_string(),
                    numerator: 1,
                    denominator: 100,
                    rounding: FeeRounding::Ceil,
                }],
            },
        }
    }

    fn config() -> RugScalpV2Config {
        let mut config = RugScalpV2Config::default();
        config.enabled = true;
        config.validation_tape.enabled = true;
        config.validation_tape.technical_capture = true;
        config.pump_quote_authority = Some(RugScalpPumpQuoteAuthorityV1 {
            schedules: vec![
                schedule(PumpRouteVariant::BuyV2, "buy-v2"),
                schedule(PumpRouteVariant::LegacySell, "legacy-sell"),
            ],
            entry_transaction_costs: TransactionCosts::default(),
            exit_transaction_costs: TransactionCosts::default(),
        });
        config
    }

    fn context() -> RugScalpValidationRunContextV1 {
        RugScalpValidationRunContextV1 {
            run_id: "run-1".to_string(),
            config_hash: "config-hash".to_string(),
            code_hash: "code-hash".to_string(),
            binary_hash: "binary-hash".to_string(),
            runtime_fee_authority: None,
        }
    }

    fn assessment() -> (RugScalpEntryAssessmentV2, RugScalpEntryIntentV2) {
        let mut reducer = crate::rug_scalp_v2::RugScalpRuntimeAdapterV2::new(config());
        let pool = crate::events::DetectedPool {
            semantic: Default::default(),
            pool_amm_id: "pool".to_string(),
            base_mint: solana_sdk::pubkey::Pubkey::new_unique().to_string(),
            quote_mint: "SOL".to_string(),
            amm_program: "pumpfun".to_string(),
            bonding_curve: "curve".to_string(),
            creator: solana_sdk::pubkey::Pubkey::new_unique().to_string(),
            slot: Some(10),
            tx_index: Some(0),
            timestamp_ms: 1_000,
            event_time: Default::default(),
            detected_wall_ts_ms: Some(1_000),
            initial_liquidity_sol: None,
            signature: "birth".to_string(),
        };
        reducer.on_birth(&pool, 1_000);
        let curve = BondingCurve {
            discriminator: 0,
            virtual_token_reserves: 1_073_000_000_000_000,
            virtual_sol_reserves: 30_000_000_000,
            real_token_reserves: 793_100_000_000_000,
            real_sol_reserves: 0,
            token_total_supply: 1_000_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };
        let mut intent = None;
        let mut accepted = None;
        for (slot, index, signer) in [
            (11, 1, "a"),
            (11, 2, "b"),
            (12, 1, "c"),
            (12, 2, "d"),
            (12, 3, "e"),
            (12, 4, "f"),
        ] {
            let tx = PoolTransaction {
                semantic: Default::default(),
                pool_amm_id: "pool".to_string(),
                signature: format!("{slot}-{index}"),
                token_mint: Some(pool.base_mint.clone()),
                slot: Some(slot),
                tx_index: Some(index),
                event_ordinal: Some(0),
                outer_instruction_index: None,
                inner_group_index: None,
                outer_program_id: None,
                cpi_stack_height: None,
                timestamp_ms: 1_000 + index as u64,
                event_time: Default::default(),
                arrival_ts_ms: 1_000 + index as u64,
                signer: signer.to_string(),
                is_buy: true,
                success: true,
                sol_amount_lamports: Some(5_000_000_000),
                volume_sol: 5.0,
                token_amount_units: Some(1),
                reserve_base: None,
                reserve_quote: None,
                price_quote: None,
                is_dev_buy: false,
                dev_buy_lamports: 0,
                error_code: None,
                compute_units_consumed: None,
                owner_token_deltas: vec![],
                mpcf_payload: vec![],
                mpcf_payload_missing_reason: Default::default(),
                v_tokens_in_bonding_curve: None,
                v_sol_in_bonding_curve: None,
                virtual_sol_reserves: None,
                virtual_token_reserves: None,
                real_sol_reserves: None,
                real_token_reserves: None,
                complete: None,
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
                toolchain_fingerprint: Default::default(),
                curve_data_known: true,
                curve_finality: Default::default(),
            };
            for action in reducer.on_trade(
                &tx,
                1_000 + index as u64,
                RugScalpCanonicalStateV2 {
                    state_clean: true,
                    ordering_known: true,
                    accepted_window_has_gap: false,
                },
                Some(curve),
            ) {
                match action {
                    crate::rug_scalp_v2::RugScalpRuntimeActionV2::Assessment(value) => {
                        if value.is_candidate() {
                            accepted = Some(value);
                        }
                    }
                    crate::rug_scalp_v2::RugScalpRuntimeActionV2::EntryIntent(value) => {
                        intent = Some(value)
                    }
                    _ => {}
                }
            }
        }
        (
            accepted.expect("accepted assessment"),
            intent.expect("one intent"),
        )
    }

    fn trade(mint: String, slot: u64, index: u32, is_buy: bool) -> PoolTransaction {
        PoolTransaction {
            semantic: Default::default(),
            pool_amm_id: "pool".to_string(),
            signature: format!("state-{slot}-{index}"),
            token_mint: Some(mint),
            slot: Some(slot),
            tx_index: Some(index),
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 2_000 + index as u64,
            event_time: Default::default(),
            arrival_ts_ms: 2_000 + index as u64,
            signer: "observer".to_string(),
            is_buy,
            success: true,
            sol_amount_lamports: Some(1),
            volume_sol: 0.0,
            token_amount_units: Some(1),
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: Default::default(),
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            virtual_sol_reserves: None,
            virtual_token_reserves: None,
            real_sol_reserves: None,
            real_token_reserves: None,
            complete: None,
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
            toolchain_fingerprint: Default::default(),
            curve_data_known: true,
            curve_finality: Default::default(),
        }
    }

    fn curve(virtual_quote_reserves: u64) -> BondingCurve {
        BondingCurve {
            discriminator: 0,
            virtual_token_reserves: 1_073_000_000_000_000,
            virtual_sol_reserves: virtual_quote_reserves,
            real_token_reserves: 793_100_000_000_000,
            real_sol_reserves: 0,
            token_total_supply: 1_000_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        }
    }

    #[test]
    fn consecutive_canonical_states_materialize_typed_legacy_sell_requotes() {
        let (assessment, intent) = assessment();
        let mut tape = RugScalpValidationTapeV1::new(&config(), context());
        let started = tape.on_accepted_intent(&intent, signal_order());
        let signal_quote = started
            .into_iter()
            .find_map(|row| match row {
                RugScalpValidationTapeRecordV1::AttemptStarted { attempt, .. } => {
                    attempt.primary_signal_buy_v2_quote
                }
                _ => None,
            })
            .expect("signal-state primary quote is retained by the tape");
        assert_eq!(signal_quote.notional_lamports, 100_000_000);
        let first = tape.observe_trade(
            &trade(assessment.mint.clone(), 13, 1, true),
            2_000,
            RugScalpCanonicalStateV2 {
                state_clean: true,
                ordering_known: true,
                accepted_window_has_gap: false,
            },
            Some(curve(10_500_000_000)),
        );
        let second = tape.observe_trade(
            &trade(assessment.mint.clone(), 13, 2, true),
            2_001,
            RugScalpCanonicalStateV2 {
                state_clean: true,
                ordering_known: true,
                accepted_window_has_gap: false,
            },
            Some(curve(10_600_000_000)),
        );
        for record in first.into_iter().chain(second) {
            if let RugScalpValidationTapeRecordV1::State { state, .. } = record {
                assert!(state.primary.legacy_sell_quote_for_fresh_entry.is_some());
                assert!(state
                    .sensitivity
                    .legacy_sell_quote_for_fresh_entry
                    .is_some());
            }
        }
    }

    #[test]
    fn states_hold_fresh_quotes_not_the_signal_quote() {
        let (assessment, intent) = assessment();
        let signal_tokens = assessment
            .primary_quote
            .as_ref()
            .unwrap()
            .entry_token_amount_raw;
        let mut tape = RugScalpValidationTapeV1::new(&config(), context());
        tape.on_accepted_intent(&intent, signal_order());
        let rows = tape.observe_trade(
            &trade(assessment.mint.clone(), 13, 1, true),
            2_000,
            RugScalpCanonicalStateV2 {
                state_clean: true,
                ordering_known: true,
                accepted_window_has_gap: false,
            },
            Some(curve(20_000_000_000)),
        );
        let state = rows
            .into_iter()
            .find_map(|row| match row {
                RugScalpValidationTapeRecordV1::State { state, .. } => Some(state),
                _ => None,
            })
            .unwrap();
        assert_ne!(
            state.primary.buy_v2_entry_quote.unwrap().token_amount,
            signal_tokens
        );
    }

    #[test]
    fn sensitivity_never_creates_a_lifecycle_identity() {
        let (assessment, intent) = assessment();
        let mut tape = RugScalpValidationTapeV1::new(&config(), context());
        let rows = tape.on_accepted_intent(&intent, signal_order());
        let started = rows
            .into_iter()
            .find_map(|row| match row {
                RugScalpValidationTapeRecordV1::AttemptStarted { attempt, .. } => Some(attempt),
                _ => None,
            })
            .unwrap();
        assert_eq!(started.position_id, None);
        assert_eq!(started.sensitivity_notional_lamports, 200_000_000);
    }

    #[test]
    fn canonical_gap_terminates_as_data_invalidated() {
        let (assessment, intent) = assessment();
        let mut tape = RugScalpValidationTapeV1::new(&config(), context());
        tape.on_accepted_intent(&intent, signal_order());
        let rows = tape.observe_trade(
            &trade(assessment.mint.clone(), 13, 1, true),
            2_000,
            RugScalpCanonicalStateV2 {
                state_clean: false,
                ordering_known: true,
                accepted_window_has_gap: false,
            },
            Some(curve(10_500_000_000)),
        );
        assert!(
            matches!(rows.as_slice(), [RugScalpValidationTapeRecordV1::AttemptTerminal { terminal, .. }]
            if matches!(terminal.status, RugScalpValidationTerminalStatusV1::DataInvalidated))
        );
    }

    #[test]
    fn missing_fee_authority_is_non_evaluable_and_has_no_entry() {
        let mut missing = config();
        missing.pump_quote_authority = None;
        let mut tape = RugScalpValidationTapeV1::new(&missing, context());
        let mut assessment = assessment().0;
        assessment.primary_quote = None;
        assessment.sensitivity_quote = None;
        assessment.reason = RugScalpReasonCodeV2::QuoteMathUnavailable;
        let rows = tape.on_assessment(&assessment, signal_order());
        assert!(rows.iter().any(|row| matches!(row, RugScalpValidationTapeRecordV1::AttemptTerminal { terminal, .. }
            if matches!(terminal.status, RugScalpValidationTerminalStatusV1::NonEvaluableFeeAuthority))));
    }

    #[test]
    fn same_slot_material_dump_overrides_target() {
        let (assessment, intent) = assessment();
        let mut tape = RugScalpValidationTapeV1::new(&config(), context());
        tape.on_accepted_intent(&intent, signal_order());
        tape.bind_modelled_entry(
            &intent.candidate_id,
            "position".to_string(),
            1,
            1,
            12,
            1_900,
        );
        let _ = tape.observe_trade(
            &trade(assessment.mint.clone(), 13, 1, true),
            2_000,
            RugScalpCanonicalStateV2 {
                state_clean: true,
                ordering_known: true,
                accepted_window_has_gap: false,
            },
            Some(curve(20_000_000_000)),
        );
        tape.attempts_by_candidate
            .get_mut(&intent.candidate_id)
            .expect("active validation attempt")
            .first_target_slot = Some(13);
        let _ = tape.observe_trade(
            &trade(assessment.mint.clone(), 13, 2, false),
            2_050,
            RugScalpCanonicalStateV2 {
                state_clean: true,
                ordering_known: true,
                accepted_window_has_gap: false,
            },
            Some(curve(10_000_000_000)),
        );
        let _ = tape.observe_trade(
            &trade(assessment.mint.clone(), 14, 1, false),
            2_100,
            RugScalpCanonicalStateV2 {
                state_clean: true,
                ordering_known: true,
                accepted_window_has_gap: false,
            },
            Some(curve(9_000_000_000)),
        );
        let terminal = tape.finish_candidate(
            intent.candidate_id.clone(),
            RugScalpValidationTerminalStatusV1::PositionClosed,
            "pm_closed".to_string(),
            Some(14),
            None,
            2_200,
        );
        let dump_wins = terminal.into_iter().any(|record| {
            matches!(record,
                RugScalpValidationTapeRecordV1::AttemptTerminal { terminal, .. }
                if terminal.same_slot_dump_wins
                    && terminal.first_material_sell_slot == Some(13))
        });
        assert!(dump_wins, "same-slot material dump must override target");
    }

    #[test]
    fn tape_only_emits_records_and_has_no_execution_action() {
        let (assessment, intent) = assessment();
        let mut tape = RugScalpValidationTapeV1::new(&config(), context());
        let rows = tape.on_accepted_intent(&intent, signal_order());
        assert!(rows.iter().all(|row| matches!(
            row,
            RugScalpValidationTapeRecordV1::AttemptStarted { .. }
                | RugScalpValidationTapeRecordV1::Latency { .. }
        )));
    }

    #[test]
    fn exactly_one_terminal_record_is_emitted_per_attempt() {
        let (assessment, intent) = assessment();
        let mut tape = RugScalpValidationTapeV1::new(&config(), context());
        tape.on_accepted_intent(&intent, signal_order());
        tape.bind_modelled_entry(
            &intent.candidate_id,
            "position".to_string(),
            intent.expected_entry_token_amount_raw,
            intent.entry_total_debit_lamports,
            signal_order().slot,
            2_000,
        );
        assert_eq!(tape.on_clock(7_000).len(), 1);
        assert!(tape.on_clock(7_001).is_empty());
    }
}
