use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_GLOBAL_SIGNATURE_CAPACITY: usize = 500_000;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoverageAuditGlobalSignatureState {
    pub raw_received: bool,
    pub parse_candidate: bool,
    pub parse_miss: bool,
    pub mapping_missing: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mapping_missing_reason: Option<String>,
    pub raw_received_global_stream: bool,
    pub raw_received_pool_stream: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoverageAuditPoolSignatureState {
    pub seer_rx: bool,
    pub mapping_missing: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mapping_missing_reason: Option<String>,
    pub seer_emitted: bool,
    pub runtime_seen: bool,
    pub runtime_accepted: bool,
    pub seer_rx_global_stream: bool,
    pub seer_rx_pool_stream: bool,
    pub seer_emitted_global_stream: bool,
    pub seer_emitted_pool_stream: bool,
    pub runtime_filter_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_time_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_effective_time_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_fallback_class: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoverageAuditWindowDiagnostics {
    pub watch_registration_wall_ms: Option<u64>,
    pub watch_registration_source: Option<String>,
    pub registry_version_at_watch: Option<u64>,
    pub exact_curve_accounts_at_watch: Option<u64>,
    pub exact_pool_accounts_at_watch: Option<u64>,
    pub watched_mints_at_watch: Option<u64>,
    pub transport_resubs_sent_at_watch: Option<u64>,
    pub transport_msgs_spilled_at_watch: Option<u64>,
    pub transport_overflow_dropped_at_watch: Option<u64>,
    pub transport_slot_gaps_at_watch: Option<u64>,
    pub transport_last_msg_gap_ms_at_watch: Option<u64>,
    pub pool_task_backpressure_drop_count: u64,
    pub hot_pool_backpressure_drop_count: u64,
    pub canonical_update_count: u64,
    #[serde(default)]
    pub canonical_first_update_latency_ms: Option<u64>,
    #[serde(default)]
    pub live_account_update_count: u64,
    #[serde(default)]
    pub live_first_account_update_latency_ms: Option<u64>,
    #[serde(default)]
    pub account_update_runtime_seen_total: u64,
    #[serde(default)]
    pub account_update_runtime_accepted_total: u64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub account_update_runtime_seen_by_effective_time_source: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub account_update_runtime_seen_by_fallback_class: BTreeMap<String, u64>,
    pub timed_out_without_canonical_updates: bool,
    pub seer_account_updates_before_mapping_total: u64,
    pub seer_account_updates_pending_replay_total: u64,
    pub seer_account_updates_pending_overwrite_total: u64,
    pub seer_account_updates_pending_replay_send_failed_total: u64,
    pub seer_account_updates_pending_parse_failed_total: u64,
    #[serde(default)]
    pub seer_account_updates_pending_replay_max_dwell_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoverageAuditWatchRegistration {
    pub wall_ms: u64,
    pub source: String,
    pub registry_version: u64,
    pub exact_curve_accounts: u64,
    pub exact_pool_accounts: u64,
    pub watched_mints: u64,
    pub transport_resubs_sent: u64,
    pub transport_msgs_spilled: u64,
    pub transport_overflow_dropped: u64,
    pub transport_slot_gaps: u64,
    pub transport_last_msg_gap_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageAuditWindowState {
    pub window_id: String,
    pub pool_id: String,
    pub base_mint: Option<String>,
    pub t0_ms: u64,
    pub t_end_ms: u64,
    pub verdict: Option<String>,
    pub window_complete: bool,
    pub window_close_reason: Option<String>,
    pub signatures: HashMap<String, CoverageAuditPoolSignatureState>,
    pub diagnostics: CoverageAuditWindowDiagnostics,
}

impl CoverageAuditWindowState {
    fn new(pool_id: String, base_mint: Option<String>, t0_ms: u64, t_end_ms: u64) -> Self {
        Self {
            window_id: format!("{}:{}:{}", pool_id, t0_ms, t_end_ms),
            pool_id,
            base_mint,
            t0_ms,
            t_end_ms,
            verdict: None,
            window_complete: false,
            window_close_reason: None,
            signatures: HashMap::new(),
            diagnostics: CoverageAuditWindowDiagnostics::default(),
        }
    }

    fn signature_state_mut(&mut self, signature: &str) -> &mut CoverageAuditPoolSignatureState {
        self.signatures.entry(signature.to_string()).or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageAuditClosedWindow {
    pub window_id: String,
    pub pool_id: String,
    pub base_mint: Option<String>,
    pub t0_ms: u64,
    pub t_end_ms: u64,
    pub verdict: Option<String>,
    pub window_complete: bool,
    pub window_close_reason: Option<String>,
    pub signatures: HashMap<String, CoverageAuditPoolSignatureState>,
    pub diagnostics: CoverageAuditWindowDiagnostics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Ord, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum CoverageAuditReason {
    NotReceived,
    FailedTxFiltered,
    FilteredBeforeParse,
    ParseMiss,
    MappingMissing,
    NotForwarded,
    RuntimeFiltered,
    InvariantBroken,
}

impl CoverageAuditReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotReceived => "not_received",
            Self::FailedTxFiltered => "failed_tx_filtered",
            Self::FilteredBeforeParse => "filtered_before_parse",
            Self::ParseMiss => "parse_miss",
            Self::MappingMissing => "mapping_missing",
            Self::NotForwarded => "not_forwarded",
            Self::RuntimeFiltered => "runtime_filtered",
            Self::InvariantBroken => "invariant_broken",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Ord, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum CoverageAuditTimeoutClass {
    GenuineNoInterest,
    IngestMiss,
    FilterDrop,
    StaleOrLateArrival,
    WindowCloseTooEarly,
    InvariantBrokenBookkeeping,
    Unclassified,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoverageAuditMissingSignature {
    pub signature: String,
    pub reason: CoverageAuditReason,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw_sources: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seer_rx_sources: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seer_emitted_sources: Vec<String>,
    #[serde(default)]
    pub chain_truth_failed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain_truth_time_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mapping_missing_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_filter_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_time_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_effective_time_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_fallback_class: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoverageAuditInvariantSummary {
    pub emitted_without_rx: u64,
    pub runtime_accepted_without_emitted: u64,
    pub missing_reason_fallbacks: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoverageAuditTruthSignatureState {
    pub failed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CoverageAuditRecord {
    pub schema_version: u32,
    #[serde(default)]
    pub recorded_at_ms: u64,
    pub audit_type: String,
    pub audit_status: String,
    pub chain_truth_unavailable: bool,
    pub rpc_error: Option<String>,
    pub window_id: String,
    pub pool_id: String,
    pub base_mint: Option<String>,
    pub t0_ms: u64,
    pub t_end_ms: u64,
    pub window_ms: u64,
    pub verdict: Option<String>,
    pub window_complete: bool,
    pub window_close_reason: Option<String>,
    pub chain_truth_count: u64,
    pub chain_truth_failed_count: u64,
    pub seer_rx_count: u64,
    pub seer_emitted_count: u64,
    pub runtime_seen_count: u64,
    pub runtime_accepted_count: u64,
    pub missing_count: u64,
    pub truth_to_rx_pct: f64,
    pub truth_to_emit_pct: f64,
    pub truth_to_runtime_accept_pct: f64,
    pub counts_by_reason: BTreeMap<String, u64>,
    pub mapping_missing_by_reason: BTreeMap<String, u64>,
    pub raw_received_by_source: BTreeMap<String, u64>,
    pub seer_rx_by_source: BTreeMap<String, u64>,
    pub seer_emitted_by_source: BTreeMap<String, u64>,
    pub runtime_filtered_by_reason: BTreeMap<String, u64>,
    #[serde(default)]
    pub filtered_reason_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub duplicate_suppression_by_reason: BTreeMap<String, u64>,
    #[serde(default)]
    pub chain_truth_by_time_source: BTreeMap<String, u64>,
    #[serde(default)]
    pub runtime_seen_by_time_source: BTreeMap<String, u64>,
    #[serde(default)]
    pub runtime_seen_by_effective_time_source: BTreeMap<String, u64>,
    #[serde(default)]
    pub dominant_runtime_effective_time_source: Option<String>,
    #[serde(default)]
    pub runtime_seen_by_fallback_class: BTreeMap<String, u64>,
    #[serde(default)]
    pub timeout_primary_cause: Option<CoverageAuditTimeoutClass>,
    #[serde(default)]
    pub timeout_flags: Vec<CoverageAuditTimeoutClass>,
    pub missing_signatures: Vec<CoverageAuditMissingSignature>,
    pub invariants: CoverageAuditInvariantSummary,
    pub diagnostics: CoverageAuditWindowDiagnostics,
}

#[derive(Default)]
pub struct CoverageAuditRecorder {
    active_windows: DashMap<String, Arc<Mutex<CoverageAuditWindowState>>>,
    pending_pool_signatures:
        DashMap<String, Arc<Mutex<HashMap<String, CoverageAuditPoolSignatureState>>>>,
    pending_pool_diagnostics: DashMap<String, Arc<Mutex<CoverageAuditWindowDiagnostics>>>,
    pool_aliases: DashMap<String, String>,
    global_signatures: DashMap<String, CoverageAuditGlobalSignatureState>,
    global_signature_fifo: Mutex<VecDeque<String>>,
    global_signature_capacity: usize,
}

impl CoverageAuditRecorder {
    pub fn new() -> Self {
        Self {
            active_windows: DashMap::new(),
            pending_pool_signatures: DashMap::new(),
            pending_pool_diagnostics: DashMap::new(),
            pool_aliases: DashMap::new(),
            global_signatures: DashMap::new(),
            global_signature_fifo: Mutex::new(VecDeque::new()),
            global_signature_capacity: DEFAULT_GLOBAL_SIGNATURE_CAPACITY,
        }
    }

    pub fn register_pool_alias(&self, alias: &str, pool_id: &str) {
        if alias.is_empty() || pool_id.is_empty() || alias == pool_id {
            return;
        }

        let alias_key = alias.to_string();
        let pool_key = pool_id.to_string();
        self.pool_aliases
            .insert(alias_key.clone(), pool_key.clone());
        self.merge_pending_alias_state(&alias_key, &pool_key);
        if let Some(window) = self.active_windows.get(&pool_key) {
            let window = window.value().clone();
            self.merge_aliases_into_window(&pool_key, &window);
        }
    }

    pub fn open_window(
        &self,
        pool_id: impl Into<String>,
        base_mint: Option<String>,
        t0_ms: u64,
        t_end_ms: u64,
    ) -> String {
        let pool_id = pool_id.into();
        let window = Arc::new(Mutex::new(CoverageAuditWindowState::new(
            pool_id.clone(),
            base_mint,
            t0_ms,
            t_end_ms,
        )));

        if let Some((_, pending)) = self.pending_pool_signatures.remove(&pool_id) {
            let pending = pending
                .lock()
                .expect("pending coverage audit state poisoned")
                .clone();
            let mut guard = window.lock().expect("coverage audit window poisoned");
            guard.signatures.extend(pending);
        }
        if let Some((_, pending_diag)) = self.pending_pool_diagnostics.remove(&pool_id) {
            let pending_diag = pending_diag
                .lock()
                .expect("pending coverage audit diagnostics poisoned")
                .clone();
            let mut guard = window.lock().expect("coverage audit window poisoned");
            guard.diagnostics = pending_diag;
        }
        self.merge_aliases_into_window(&pool_id, &window);

        let window_id = window
            .lock()
            .expect("coverage audit window poisoned")
            .window_id
            .clone();
        self.active_windows.insert(pool_id, window);
        window_id
    }

    pub fn close_window(
        &self,
        pool_id: &str,
        verdict: Option<String>,
        window_complete: bool,
        window_close_reason: Option<String>,
    ) -> Option<CoverageAuditClosedWindow> {
        let resolved_pool_id = self.resolve_pool_id_owned(pool_id);
        let (_, window) = self.active_windows.remove(&resolved_pool_id)?;
        let mut guard = window.lock().expect("coverage audit window poisoned");
        guard.verdict = verdict;
        guard.window_complete = window_complete;
        guard.window_close_reason = window_close_reason;
        Some(CoverageAuditClosedWindow {
            window_id: guard.window_id.clone(),
            pool_id: guard.pool_id.clone(),
            base_mint: guard.base_mint.clone(),
            t0_ms: guard.t0_ms,
            t_end_ms: guard.t_end_ms,
            verdict: guard.verdict.clone(),
            window_complete: guard.window_complete,
            window_close_reason: guard.window_close_reason.clone(),
            signatures: guard.signatures.clone(),
            diagnostics: guard.diagnostics.clone(),
        })
    }

    pub fn set_window_base_mint(&self, pool_id: &str, base_mint: Option<String>) {
        let resolved_pool_id = self.resolve_pool_id_owned(pool_id);
        let Some(window) = self.active_windows.get(&resolved_pool_id) else {
            return;
        };
        let window = window.value().clone();
        let mut guard = window.lock().expect("coverage audit window poisoned");
        if guard.base_mint.is_none() {
            guard.base_mint = base_mint;
        }
    }

    pub fn record_raw_received(&self, signature: &str, source: &str) {
        self.update_global_signature(signature, |state| {
            state.raw_received = true;
            match source {
                "grpc_global_stream" => state.raw_received_global_stream = true,
                "grpc_pool_stream" => state.raw_received_pool_stream = true,
                _ => {}
            }
        });
    }

    pub fn record_parse_candidate(&self, signature: &str) {
        self.update_global_signature(signature, |state| {
            state.raw_received = true;
            state.parse_candidate = true;
        });
    }

    pub fn record_parse_miss(&self, signature: &str) {
        self.update_global_signature(signature, |state| {
            state.raw_received = true;
            state.parse_candidate = true;
            state.parse_miss = true;
        });
    }

    pub fn record_seer_rx(&self, pool_id: &str, signature: &str, source: &str) {
        self.update_pool_signature(pool_id, signature, |state| {
            state.seer_rx = true;
            match source {
                "grpc_global_stream" => state.seer_rx_global_stream = true,
                "grpc_pool_stream" => state.seer_rx_pool_stream = true,
                _ => {}
            }
        });
    }

    pub fn record_mapping_missing(&self, pool_id: &str, signature: &str) {
        self.record_mapping_missing_with_reason(pool_id, signature, None);
    }

    pub fn record_mapping_missing_with_reason(
        &self,
        pool_id: &str,
        signature: &str,
        reason: Option<&str>,
    ) {
        self.update_global_signature(signature, |state| {
            state.mapping_missing = true;
            if state.mapping_missing_reason.is_none() {
                state.mapping_missing_reason = reason.map(|value| value.to_string());
            }
        });
        self.update_pool_signature(pool_id, signature, |state| {
            state.seer_rx = true;
            state.mapping_missing = true;
            if state.mapping_missing_reason.is_none() {
                state.mapping_missing_reason = reason.map(|value| value.to_string());
            }
        });
    }

    pub fn record_seer_emitted(&self, pool_id: &str, signature: &str, source: &str) {
        self.update_pool_signature(pool_id, signature, |state| {
            state.seer_rx = true;
            state.seer_emitted = true;
            match source {
                "grpc_global_stream" => {
                    state.seer_rx_global_stream = true;
                    state.seer_emitted_global_stream = true;
                }
                "grpc_pool_stream" => {
                    state.seer_rx_pool_stream = true;
                    state.seer_emitted_pool_stream = true;
                }
                _ => {}
            }
        });
    }

    pub fn record_runtime_seen(&self, pool_id: &str, signature: &str) {
        self.update_pool_signature(pool_id, signature, |state| {
            state.runtime_seen = true;
        });
    }

    pub fn record_runtime_seen_with_source(&self, pool_id: &str, signature: &str, source: &str) {
        self.record_runtime_seen_with_detail(pool_id, signature, source, None);
    }

    pub fn record_runtime_seen_with_detail(
        &self,
        pool_id: &str,
        signature: &str,
        effective_source: &str,
        fallback_class: Option<&str>,
    ) {
        self.update_pool_signature(pool_id, signature, |state| {
            state.runtime_seen = true;
            state
                .runtime_time_source
                .get_or_insert_with(|| effective_source.to_string());
            state
                .runtime_effective_time_source
                .get_or_insert_with(|| effective_source.to_string());
            if let Some(fallback_class) = fallback_class {
                state
                    .runtime_fallback_class
                    .get_or_insert_with(|| fallback_class.to_string());
            }
        });
    }

    pub fn record_runtime_accepted(&self, pool_id: &str, signature: &str) {
        self.update_pool_signature(pool_id, signature, |state| {
            state.runtime_seen = true;
            state.runtime_accepted = true;
        });
    }

    pub fn record_runtime_filtered(&self, pool_id: &str, signature: &str, reason: &str) {
        self.update_pool_signature(pool_id, signature, |state| {
            state.runtime_seen = true;
            state
                .runtime_filter_reason
                .get_or_insert_with(|| reason.to_string());
        });
    }

    pub fn record_watch_registration(
        &self,
        pool_id: &str,
        registration: CoverageAuditWatchRegistration,
    ) {
        self.update_pool_diagnostics(pool_id, |diag| {
            let replace = diag.watch_registration_wall_ms.is_none()
                || diag
                    .watch_registration_wall_ms
                    .is_some_and(|existing| registration.wall_ms <= existing);
            if replace {
                diag.watch_registration_wall_ms = Some(registration.wall_ms);
                diag.watch_registration_source = Some(registration.source.clone());
                diag.registry_version_at_watch = Some(registration.registry_version);
                diag.exact_curve_accounts_at_watch = Some(registration.exact_curve_accounts);
                diag.exact_pool_accounts_at_watch = Some(registration.exact_pool_accounts);
                diag.watched_mints_at_watch = Some(registration.watched_mints);
                diag.transport_resubs_sent_at_watch = Some(registration.transport_resubs_sent);
                diag.transport_msgs_spilled_at_watch = Some(registration.transport_msgs_spilled);
                diag.transport_overflow_dropped_at_watch =
                    Some(registration.transport_overflow_dropped);
                diag.transport_slot_gaps_at_watch = Some(registration.transport_slot_gaps);
                diag.transport_last_msg_gap_ms_at_watch =
                    Some(registration.transport_last_msg_gap_ms);
            }
        });
    }

    pub fn record_pool_task_backpressure_drop(&self, pool_id: &str, is_hot: bool) {
        self.update_pool_diagnostics(pool_id, |diag| {
            diag.pool_task_backpressure_drop_count =
                diag.pool_task_backpressure_drop_count.saturating_add(1);
            if is_hot {
                diag.hot_pool_backpressure_drop_count =
                    diag.hot_pool_backpressure_drop_count.saturating_add(1);
            }
        });
    }

    pub fn record_canonical_update_observed(&self, pool_id: &str, latency_ms: Option<u64>) {
        self.update_pool_diagnostics(pool_id, |diag| {
            diag.canonical_update_count = diag.canonical_update_count.saturating_add(1);
            if diag.canonical_first_update_latency_ms.is_none() {
                diag.canonical_first_update_latency_ms = latency_ms;
            }
        });
    }

    pub fn record_live_account_update_observed(&self, pool_id: &str, latency_ms: Option<u64>) {
        self.update_pool_diagnostics(pool_id, |diag| {
            diag.live_account_update_count = diag.live_account_update_count.saturating_add(1);
            if diag.live_first_account_update_latency_ms.is_none() {
                diag.live_first_account_update_latency_ms = latency_ms;
            }
        });
    }

    pub fn record_account_update_runtime_seen(
        &self,
        pool_id: &str,
        effective_source: &str,
        fallback_class: Option<&str>,
        accepted: bool,
    ) {
        self.update_pool_diagnostics(pool_id, |diag| {
            diag.account_update_runtime_seen_total =
                diag.account_update_runtime_seen_total.saturating_add(1);
            *diag
                .account_update_runtime_seen_by_effective_time_source
                .entry(effective_source.to_string())
                .or_default() += 1;
            if let Some(fallback_class) = fallback_class {
                *diag
                    .account_update_runtime_seen_by_fallback_class
                    .entry(fallback_class.to_string())
                    .or_default() += 1;
            }
            if accepted {
                diag.account_update_runtime_accepted_total =
                    diag.account_update_runtime_accepted_total.saturating_add(1);
            }
        });
    }

    pub fn record_timeout_without_canonical_updates(&self, pool_id: &str) {
        self.update_pool_diagnostics(pool_id, |diag| {
            diag.timed_out_without_canonical_updates = true;
        });
    }

    pub fn record_seer_account_update_before_mapping(
        &self,
        pool_id: &str,
        overwritten_existing: bool,
    ) {
        self.update_pool_diagnostics(pool_id, |diag| {
            diag.seer_account_updates_before_mapping_total = diag
                .seer_account_updates_before_mapping_total
                .saturating_add(1);
            if overwritten_existing {
                diag.seer_account_updates_pending_overwrite_total = diag
                    .seer_account_updates_pending_overwrite_total
                    .saturating_add(1);
            }
        });
    }

    pub fn record_seer_account_update_pending_replay(
        &self,
        pool_id: &str,
        dwell_ms: Option<u64>,
        send_failed: bool,
        parse_failed: bool,
    ) {
        self.update_pool_diagnostics(pool_id, |diag| {
            diag.seer_account_updates_pending_replay_total = diag
                .seer_account_updates_pending_replay_total
                .saturating_add(1);
            if send_failed {
                diag.seer_account_updates_pending_replay_send_failed_total = diag
                    .seer_account_updates_pending_replay_send_failed_total
                    .saturating_add(1);
            }
            if parse_failed {
                diag.seer_account_updates_pending_parse_failed_total = diag
                    .seer_account_updates_pending_parse_failed_total
                    .saturating_add(1);
            }
            if let Some(dwell_ms) = dwell_ms {
                diag.seer_account_updates_pending_replay_max_dwell_ms = Some(
                    diag.seer_account_updates_pending_replay_max_dwell_ms
                        .map_or(dwell_ms, |existing| existing.max(dwell_ms)),
                );
            }
        });
    }

    pub fn global_signature_state(
        &self,
        signature: &str,
    ) -> Option<CoverageAuditGlobalSignatureState> {
        self.global_signatures
            .get(signature)
            .map(|entry| entry.clone())
    }

    pub fn build_record(
        &self,
        window: CoverageAuditClosedWindow,
        chain_truth_signatures: HashMap<String, CoverageAuditTruthSignatureState>,
        rpc_error: Option<String>,
    ) -> CoverageAuditRecord {
        let chain_truth_count = chain_truth_signatures.len() as u64;
        let chain_truth_failed_count = chain_truth_signatures
            .values()
            .filter(|state| state.failed)
            .count() as u64;
        let mut seer_rx_count = 0_u64;
        let mut seer_emitted_count = 0_u64;
        let mut runtime_seen_count = 0_u64;
        let mut runtime_accepted_count = 0_u64;
        let mut missing_signatures = Vec::new();
        let mut counts_by_reason: BTreeMap<String, u64> = BTreeMap::new();
        let mut mapping_missing_by_reason: BTreeMap<String, u64> = BTreeMap::new();
        let mut raw_received_by_source: BTreeMap<String, u64> = BTreeMap::new();
        let mut seer_rx_by_source: BTreeMap<String, u64> = BTreeMap::new();
        let mut seer_emitted_by_source: BTreeMap<String, u64> = BTreeMap::new();
        let mut runtime_filtered_by_reason: BTreeMap<String, u64> = BTreeMap::new();
        let mut chain_truth_by_time_source: BTreeMap<String, u64> = BTreeMap::new();
        let mut runtime_seen_by_time_source: BTreeMap<String, u64> = BTreeMap::new();
        let mut runtime_seen_by_effective_time_source: BTreeMap<String, u64> = BTreeMap::new();
        let mut runtime_seen_by_fallback_class: BTreeMap<String, u64> = BTreeMap::new();
        let mut emitted_without_rx = 0_u64;
        let mut runtime_accepted_without_emitted = 0_u64;

        let truth_signature_states = chain_truth_signatures.iter().map(|(signature, _)| {
            (
                signature,
                window
                    .signatures
                    .get(signature)
                    .cloned()
                    .unwrap_or_default(),
            )
        });

        for (_, state) in truth_signature_states {
            if state.seer_rx {
                seer_rx_count += 1;
            }
            if state.seer_emitted {
                seer_emitted_count += 1;
            }
            if state.runtime_seen {
                runtime_seen_count += 1;
            }
            if state.runtime_accepted {
                runtime_accepted_count += 1;
            }
            let runtime_effective_time_source = state
                .runtime_effective_time_source
                .clone()
                .or_else(|| state.runtime_time_source.clone());
            if let Some(source) = state.runtime_time_source.clone() {
                *runtime_seen_by_time_source.entry(source).or_insert(0) += 1;
            }
            if let Some(source) = runtime_effective_time_source {
                *runtime_seen_by_effective_time_source
                    .entry(source)
                    .or_insert(0) += 1;
            }
            if let Some(fallback_class) = state.runtime_fallback_class.clone() {
                *runtime_seen_by_fallback_class
                    .entry(fallback_class)
                    .or_insert(0) += 1;
            }
            if state.seer_emitted && !state.seer_rx {
                emitted_without_rx += 1;
            }
            if state.runtime_accepted && !state.seer_emitted {
                runtime_accepted_without_emitted += 1;
            }
        }

        let mut missing_reason_fallbacks = 0_u64;
        for (signature, truth_state) in &chain_truth_signatures {
            let pool_state = window
                .signatures
                .get(signature)
                .cloned()
                .unwrap_or_default();
            let global_state = self.global_signature_state(signature).unwrap_or_default();
            if let Some(source) = truth_state.time_source.clone() {
                *chain_truth_by_time_source.entry(source).or_insert(0) += 1;
            }
            for source in global_sources(&global_state) {
                *raw_received_by_source.entry(source).or_insert(0) += 1;
            }
            for source in pool_rx_sources(&pool_state) {
                *seer_rx_by_source.entry(source).or_insert(0) += 1;
            }
            for source in pool_emitted_sources(&pool_state) {
                *seer_emitted_by_source.entry(source).or_insert(0) += 1;
            }
            if pool_state.runtime_accepted {
                continue;
            }
            let reason = determine_missing_reason(&global_state, &pool_state, truth_state.failed);
            let runtime_filter_reason = normalized_runtime_filter_reason(&pool_state, reason);
            if reason == CoverageAuditReason::RuntimeFiltered {
                if let Some(reason) = runtime_filter_reason.clone() {
                    *runtime_filtered_by_reason.entry(reason).or_insert(0) += 1;
                }
            }
            let mapping_missing_reason = pool_state
                .mapping_missing_reason
                .clone()
                .or(global_state.mapping_missing_reason.clone());
            if reason == CoverageAuditReason::InvariantBroken {
                missing_reason_fallbacks += 1;
            }
            *counts_by_reason
                .entry(reason.as_str().to_string())
                .or_insert(0) += 1;
            if reason == CoverageAuditReason::MappingMissing {
                if let Some(mapping_reason) = mapping_missing_reason.clone() {
                    *mapping_missing_by_reason.entry(mapping_reason).or_insert(0) += 1;
                }
            }
            missing_signatures.push(CoverageAuditMissingSignature {
                signature: signature.clone(),
                reason,
                raw_sources: global_sources(&global_state),
                seer_rx_sources: pool_rx_sources(&pool_state),
                seer_emitted_sources: pool_emitted_sources(&pool_state),
                chain_truth_failed: truth_state.failed,
                chain_truth_time_source: truth_state.time_source.clone(),
                mapping_missing_reason,
                runtime_filter_reason,
                runtime_time_source: pool_state.runtime_time_source.clone(),
                runtime_effective_time_source: pool_state
                    .runtime_effective_time_source
                    .clone()
                    .or_else(|| pool_state.runtime_time_source.clone()),
                runtime_fallback_class: pool_state.runtime_fallback_class.clone(),
            });
        }
        missing_signatures.sort_by(|a, b| a.signature.cmp(&b.signature));

        let window_ms = window.t_end_ms.saturating_sub(window.t0_ms);
        let invariants = CoverageAuditInvariantSummary {
            emitted_without_rx,
            runtime_accepted_without_emitted,
            missing_reason_fallbacks,
        };
        let filtered_reason_keys = sorted_map_keys(&runtime_filtered_by_reason);
        let duplicate_suppression_by_reason =
            duplicate_suppression_breakdown(&runtime_filtered_by_reason);
        let dominant_runtime_effective_time_source =
            dominant_key(&runtime_seen_by_effective_time_source);
        let (timeout_primary_cause, timeout_flags) = classify_timeout_window(
            window.verdict.as_deref(),
            window.window_close_reason.as_deref(),
            window.window_complete,
            window_ms,
            chain_truth_count,
            seer_rx_count,
            seer_emitted_count,
            runtime_seen_count,
            runtime_accepted_count,
            &counts_by_reason,
            &runtime_filtered_by_reason,
            &runtime_seen_by_effective_time_source,
            &runtime_seen_by_fallback_class,
            &invariants,
            &window.diagnostics,
        );
        CoverageAuditRecord {
            schema_version: 5,
            recorded_at_ms: wall_clock_epoch_ms(),
            audit_type: "seer_runtime_coverage_audit".to_string(),
            audit_status: if rpc_error.is_some() {
                "rpc_error".to_string()
            } else {
                "ok".to_string()
            },
            chain_truth_unavailable: rpc_error.is_some(),
            rpc_error,
            window_id: window.window_id,
            pool_id: window.pool_id,
            base_mint: window.base_mint,
            t0_ms: window.t0_ms,
            t_end_ms: window.t_end_ms,
            window_ms,
            verdict: window.verdict,
            window_complete: window.window_complete,
            window_close_reason: window.window_close_reason,
            chain_truth_count,
            chain_truth_failed_count,
            seer_rx_count,
            seer_emitted_count,
            runtime_seen_count,
            runtime_accepted_count,
            missing_count: missing_signatures.len() as u64,
            truth_to_rx_pct: ratio(chain_truth_count, seer_rx_count),
            truth_to_emit_pct: ratio(chain_truth_count, seer_emitted_count),
            truth_to_runtime_accept_pct: ratio(chain_truth_count, runtime_accepted_count),
            counts_by_reason,
            mapping_missing_by_reason,
            raw_received_by_source,
            seer_rx_by_source,
            seer_emitted_by_source,
            runtime_filtered_by_reason,
            filtered_reason_keys,
            duplicate_suppression_by_reason,
            chain_truth_by_time_source,
            runtime_seen_by_time_source,
            runtime_seen_by_effective_time_source,
            dominant_runtime_effective_time_source,
            runtime_seen_by_fallback_class,
            timeout_primary_cause,
            timeout_flags,
            missing_signatures,
            invariants,
            diagnostics: window.diagnostics,
        }
    }

    fn update_global_signature<F>(&self, signature: &str, update: F)
    where
        F: FnOnce(&mut CoverageAuditGlobalSignatureState),
    {
        let mut state = self
            .global_signatures
            .get(signature)
            .map(|entry| entry.clone())
            .unwrap_or_default();
        update(&mut state);
        let existed = self
            .global_signatures
            .insert(signature.to_string(), state)
            .is_some();
        if !existed {
            self.touch_global_signature(signature);
        }
    }

    fn update_pool_signature<F>(&self, pool_id: &str, signature: &str, update: F)
    where
        F: FnOnce(&mut CoverageAuditPoolSignatureState),
    {
        let pool_id = self.resolve_pool_id_owned(pool_id);
        if let Some(window) = self.active_windows.get(&pool_id) {
            let window = window.value().clone();
            let mut guard = window.lock().expect("coverage audit window poisoned");
            update(guard.signature_state_mut(signature));
            return;
        }

        let entry = self
            .pending_pool_signatures
            .entry(pool_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(HashMap::new())))
            .clone();
        let mut guard = entry.lock().expect("pending coverage audit state poisoned");
        let state = guard.entry(signature.to_string()).or_default();
        update(state);
    }

    fn update_pool_diagnostics<F>(&self, pool_id: &str, update: F)
    where
        F: FnOnce(&mut CoverageAuditWindowDiagnostics),
    {
        let pool_id = self.resolve_pool_id_owned(pool_id);
        if let Some(window) = self.active_windows.get(&pool_id) {
            let window = window.value().clone();
            let mut guard = window.lock().expect("coverage audit window poisoned");
            update(&mut guard.diagnostics);
            return;
        }

        let entry = self
            .pending_pool_diagnostics
            .entry(pool_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(CoverageAuditWindowDiagnostics::default())))
            .clone();
        let mut guard = entry
            .lock()
            .expect("pending coverage audit diagnostics poisoned");
        update(&mut guard);
    }

    fn touch_global_signature(&self, signature: &str) {
        let mut fifo = self
            .global_signature_fifo
            .lock()
            .expect("coverage audit signature fifo poisoned");
        fifo.push_back(signature.to_string());
        while fifo.len() > self.global_signature_capacity {
            if let Some(oldest) = fifo.pop_front() {
                self.global_signatures.remove(&oldest);
            }
        }
    }

    fn resolve_pool_id_owned(&self, pool_id: &str) -> String {
        self.pool_aliases
            .get(pool_id)
            .map(|entry| entry.value().clone())
            .unwrap_or_else(|| pool_id.to_string())
    }

    fn merge_pending_alias_state(&self, alias: &str, pool_id: &str) {
        if let Some((_, alias_signatures)) = self.pending_pool_signatures.remove(alias) {
            let alias_state = alias_signatures
                .lock()
                .expect("pending coverage audit state poisoned")
                .clone();
            let target = self
                .pending_pool_signatures
                .entry(pool_id.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(HashMap::new())))
                .clone();
            let mut target_guard = target
                .lock()
                .expect("pending coverage audit state poisoned");
            for (signature, state) in alias_state {
                let dest = target_guard.entry(signature).or_default();
                merge_pool_signature_state(dest, &state);
            }
        }

        if let Some((_, alias_diag)) = self.pending_pool_diagnostics.remove(alias) {
            let alias_diag = alias_diag
                .lock()
                .expect("pending coverage audit diagnostics poisoned")
                .clone();
            let target = self
                .pending_pool_diagnostics
                .entry(pool_id.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(CoverageAuditWindowDiagnostics::default())))
                .clone();
            let mut target_guard = target
                .lock()
                .expect("pending coverage audit diagnostics poisoned");
            merge_window_diagnostics(&mut target_guard, &alias_diag);
        }
    }

    fn merge_aliases_into_window(
        &self,
        pool_id: &str,
        window: &Arc<Mutex<CoverageAuditWindowState>>,
    ) {
        let aliases: Vec<String> = self
            .pool_aliases
            .iter()
            .filter_map(|entry| {
                if entry.value() == pool_id {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect();

        for alias in aliases {
            if let Some((_, pending)) = self.pending_pool_signatures.remove(&alias) {
                let pending = pending
                    .lock()
                    .expect("pending coverage audit state poisoned")
                    .clone();
                let mut guard = window.lock().expect("coverage audit window poisoned");
                for (signature, state) in pending {
                    let dest = guard.signature_state_mut(&signature);
                    merge_pool_signature_state(dest, &state);
                }
            }
            if let Some((_, pending_diag)) = self.pending_pool_diagnostics.remove(&alias) {
                let pending_diag = pending_diag
                    .lock()
                    .expect("pending coverage audit diagnostics poisoned")
                    .clone();
                let mut guard = window.lock().expect("coverage audit window poisoned");
                merge_window_diagnostics(&mut guard.diagnostics, &pending_diag);
            }
        }
    }
}

fn wall_clock_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn merge_pool_signature_state(
    dest: &mut CoverageAuditPoolSignatureState,
    src: &CoverageAuditPoolSignatureState,
) {
    dest.seer_rx |= src.seer_rx;
    dest.mapping_missing |= src.mapping_missing;
    if dest.mapping_missing_reason.is_none() {
        dest.mapping_missing_reason = src.mapping_missing_reason.clone();
    }
    dest.seer_emitted |= src.seer_emitted;
    dest.runtime_seen |= src.runtime_seen;
    dest.runtime_accepted |= src.runtime_accepted;
    dest.seer_rx_global_stream |= src.seer_rx_global_stream;
    dest.seer_rx_pool_stream |= src.seer_rx_pool_stream;
    dest.seer_emitted_global_stream |= src.seer_emitted_global_stream;
    dest.seer_emitted_pool_stream |= src.seer_emitted_pool_stream;
    if dest.runtime_filter_reason.is_none() {
        dest.runtime_filter_reason = src.runtime_filter_reason.clone();
    }
}

fn merge_window_diagnostics(
    dest: &mut CoverageAuditWindowDiagnostics,
    src: &CoverageAuditWindowDiagnostics,
) {
    if dest.watch_registration_wall_ms.is_none() {
        dest.watch_registration_wall_ms = src.watch_registration_wall_ms;
    }
    if dest.watch_registration_source.is_none() {
        dest.watch_registration_source = src.watch_registration_source.clone();
    }
    if dest.registry_version_at_watch.is_none() {
        dest.registry_version_at_watch = src.registry_version_at_watch;
    }
    if dest.exact_curve_accounts_at_watch.is_none() {
        dest.exact_curve_accounts_at_watch = src.exact_curve_accounts_at_watch;
    }
    if dest.exact_pool_accounts_at_watch.is_none() {
        dest.exact_pool_accounts_at_watch = src.exact_pool_accounts_at_watch;
    }
    if dest.watched_mints_at_watch.is_none() {
        dest.watched_mints_at_watch = src.watched_mints_at_watch;
    }
    if dest.transport_resubs_sent_at_watch.is_none() {
        dest.transport_resubs_sent_at_watch = src.transport_resubs_sent_at_watch;
    }
    if dest.transport_msgs_spilled_at_watch.is_none() {
        dest.transport_msgs_spilled_at_watch = src.transport_msgs_spilled_at_watch;
    }
    if dest.transport_overflow_dropped_at_watch.is_none() {
        dest.transport_overflow_dropped_at_watch = src.transport_overflow_dropped_at_watch;
    }
    if dest.transport_slot_gaps_at_watch.is_none() {
        dest.transport_slot_gaps_at_watch = src.transport_slot_gaps_at_watch;
    }
    if dest.transport_last_msg_gap_ms_at_watch.is_none() {
        dest.transport_last_msg_gap_ms_at_watch = src.transport_last_msg_gap_ms_at_watch;
    }

    dest.pool_task_backpressure_drop_count = dest
        .pool_task_backpressure_drop_count
        .saturating_add(src.pool_task_backpressure_drop_count);
    dest.hot_pool_backpressure_drop_count = dest
        .hot_pool_backpressure_drop_count
        .saturating_add(src.hot_pool_backpressure_drop_count);
    dest.canonical_update_count = dest
        .canonical_update_count
        .saturating_add(src.canonical_update_count);
    if dest.canonical_first_update_latency_ms.is_none() {
        dest.canonical_first_update_latency_ms = src.canonical_first_update_latency_ms;
    } else if let Some(src_latency) = src.canonical_first_update_latency_ms {
        dest.canonical_first_update_latency_ms = Some(
            dest.canonical_first_update_latency_ms
                .map_or(src_latency, |existing| existing.min(src_latency)),
        );
    }
    dest.timed_out_without_canonical_updates |= src.timed_out_without_canonical_updates;
    dest.live_account_update_count = dest
        .live_account_update_count
        .saturating_add(src.live_account_update_count);
    if dest.live_first_account_update_latency_ms.is_none() {
        dest.live_first_account_update_latency_ms = src.live_first_account_update_latency_ms;
    } else if let Some(src_latency) = src.live_first_account_update_latency_ms {
        dest.live_first_account_update_latency_ms = Some(
            dest.live_first_account_update_latency_ms
                .map_or(src_latency, |existing| existing.min(src_latency)),
        );
    }
    dest.seer_account_updates_before_mapping_total = dest
        .seer_account_updates_before_mapping_total
        .saturating_add(src.seer_account_updates_before_mapping_total);
    dest.seer_account_updates_pending_replay_total = dest
        .seer_account_updates_pending_replay_total
        .saturating_add(src.seer_account_updates_pending_replay_total);
    dest.seer_account_updates_pending_overwrite_total = dest
        .seer_account_updates_pending_overwrite_total
        .saturating_add(src.seer_account_updates_pending_overwrite_total);
    dest.seer_account_updates_pending_replay_send_failed_total = dest
        .seer_account_updates_pending_replay_send_failed_total
        .saturating_add(src.seer_account_updates_pending_replay_send_failed_total);
    dest.seer_account_updates_pending_parse_failed_total = dest
        .seer_account_updates_pending_parse_failed_total
        .saturating_add(src.seer_account_updates_pending_parse_failed_total);
    if let Some(src_dwell) = src.seer_account_updates_pending_replay_max_dwell_ms {
        dest.seer_account_updates_pending_replay_max_dwell_ms = Some(
            dest.seer_account_updates_pending_replay_max_dwell_ms
                .map_or(src_dwell, |existing| existing.max(src_dwell)),
        );
    }
}

fn global_sources(global: &CoverageAuditGlobalSignatureState) -> Vec<String> {
    let mut out = Vec::new();
    if global.raw_received_global_stream {
        out.push("grpc_global_stream".to_string());
    }
    if global.raw_received_pool_stream {
        out.push("grpc_pool_stream".to_string());
    }
    out
}

fn pool_rx_sources(pool: &CoverageAuditPoolSignatureState) -> Vec<String> {
    let mut out = Vec::new();
    if pool.seer_rx_global_stream {
        out.push("grpc_global_stream".to_string());
    }
    if pool.seer_rx_pool_stream {
        out.push("grpc_pool_stream".to_string());
    }
    out
}

fn pool_emitted_sources(pool: &CoverageAuditPoolSignatureState) -> Vec<String> {
    let mut out = Vec::new();
    if pool.seer_emitted_global_stream {
        out.push("grpc_global_stream".to_string());
    }
    if pool.seer_emitted_pool_stream {
        out.push("grpc_pool_stream".to_string());
    }
    out
}

fn determine_missing_reason(
    global: &CoverageAuditGlobalSignatureState,
    pool: &CoverageAuditPoolSignatureState,
    chain_truth_failed: bool,
) -> CoverageAuditReason {
    if pool.runtime_accepted && !pool.seer_emitted {
        return CoverageAuditReason::InvariantBroken;
    }
    if pool.seer_emitted && !pool.seer_rx {
        return CoverageAuditReason::InvariantBroken;
    }
    if pool.seer_emitted {
        return CoverageAuditReason::RuntimeFiltered;
    }
    if pool.mapping_missing || global.mapping_missing {
        return CoverageAuditReason::MappingMissing;
    }
    if pool.seer_rx {
        return CoverageAuditReason::NotForwarded;
    }
    if global.parse_miss {
        return CoverageAuditReason::ParseMiss;
    }
    if global.raw_received && !global.parse_candidate {
        return CoverageAuditReason::FilteredBeforeParse;
    }
    if chain_truth_failed {
        return CoverageAuditReason::FailedTxFiltered;
    }
    if global.raw_received && global.parse_candidate {
        return CoverageAuditReason::InvariantBroken;
    }
    CoverageAuditReason::NotReceived
}

fn normalized_runtime_filter_reason(
    pool: &CoverageAuditPoolSignatureState,
    missing_reason: CoverageAuditReason,
) -> Option<String> {
    if missing_reason != CoverageAuditReason::RuntimeFiltered {
        return pool.runtime_filter_reason.clone();
    }
    pool.runtime_filter_reason.clone().or_else(|| {
        Some(if pool.runtime_seen {
            "runtime_filter_reason_missing".to_string()
        } else {
            "runtime_filter_reason_missing_before_runtime_seen".to_string()
        })
    })
}

fn sorted_map_keys(map: &BTreeMap<String, u64>) -> Vec<String> {
    map.keys().cloned().collect()
}

fn dominant_key(map: &BTreeMap<String, u64>) -> Option<String> {
    map.iter()
        .max_by(|(left_key, left_count), (right_key, right_count)| {
            left_count
                .cmp(right_count)
                .then_with(|| right_key.cmp(left_key))
        })
        .map(|(key, _)| key.clone())
}

fn duplicate_suppression_breakdown(
    runtime_filtered_by_reason: &BTreeMap<String, u64>,
) -> BTreeMap<String, u64> {
    runtime_filtered_by_reason
        .iter()
        .filter(|(reason, _)| reason.contains("duplicate") || reason.contains("dedup"))
        .map(|(reason, count)| (reason.clone(), *count))
        .collect()
}

fn is_timeout_verdict(verdict: Option<&str>, window_close_reason: Option<&str>) -> bool {
    verdict.is_some_and(|value| value.contains("TIMEOUT"))
        || window_close_reason == Some("GATEKEEPER_TIMEOUT")
}

fn is_fallback_effective_time_source(source: &str) -> bool {
    matches!(source, "wall_clock_fallback")
}

fn contains_fallback_effective_time_source(sources: &BTreeMap<String, u64>) -> bool {
    sources
        .keys()
        .any(|source| is_fallback_effective_time_source(source))
}

fn classify_timeout_window(
    verdict: Option<&str>,
    window_close_reason: Option<&str>,
    window_complete: bool,
    window_ms: u64,
    chain_truth_count: u64,
    seer_rx_count: u64,
    seer_emitted_count: u64,
    runtime_seen_count: u64,
    runtime_accepted_count: u64,
    counts_by_reason: &BTreeMap<String, u64>,
    runtime_filtered_by_reason: &BTreeMap<String, u64>,
    runtime_seen_by_effective_time_source: &BTreeMap<String, u64>,
    runtime_seen_by_fallback_class: &BTreeMap<String, u64>,
    invariants: &CoverageAuditInvariantSummary,
    diagnostics: &CoverageAuditWindowDiagnostics,
) -> (
    Option<CoverageAuditTimeoutClass>,
    Vec<CoverageAuditTimeoutClass>,
) {
    if !is_timeout_verdict(verdict, window_close_reason) {
        return (None, Vec::new());
    }

    let mut flags = Vec::new();
    if counts_by_reason
        .get(CoverageAuditReason::InvariantBroken.as_str())
        .copied()
        .unwrap_or(0)
        > 0
        || invariants.emitted_without_rx > 0
        || invariants.runtime_accepted_without_emitted > 0
        || invariants.missing_reason_fallbacks > 0
    {
        flags.push(CoverageAuditTimeoutClass::InvariantBrokenBookkeeping);
    }
    if !window_complete || window_close_reason == Some("GATEKEEPER_TIMEOUT") {
        flags.push(CoverageAuditTimeoutClass::WindowCloseTooEarly);
    }
    if chain_truth_count > 0 && seer_rx_count == 0 {
        flags.push(CoverageAuditTimeoutClass::IngestMiss);
    }
    if counts_by_reason
        .get(CoverageAuditReason::RuntimeFiltered.as_str())
        .copied()
        .unwrap_or(0)
        > 0
        || !runtime_filtered_by_reason.is_empty()
        || (seer_emitted_count > 0 && runtime_accepted_count < seer_emitted_count)
    {
        flags.push(CoverageAuditTimeoutClass::FilterDrop);
    }

    let has_stale_or_late_signal =
        contains_fallback_effective_time_source(runtime_seen_by_effective_time_source)
            || !runtime_seen_by_fallback_class.is_empty()
            || contains_fallback_effective_time_source(
                &diagnostics.account_update_runtime_seen_by_effective_time_source,
            )
            || !diagnostics
                .account_update_runtime_seen_by_fallback_class
                .is_empty()
            || diagnostics
                .canonical_first_update_latency_ms
                .is_some_and(|latency| latency > window_ms)
            || diagnostics
                .live_first_account_update_latency_ms
                .is_some_and(|latency| latency > window_ms)
            || (window_close_reason == Some("END_REACHED_BY_SWEEP")
                && chain_truth_count > seer_rx_count
                && seer_rx_count > 0);
    if has_stale_or_late_signal {
        flags.push(CoverageAuditTimeoutClass::StaleOrLateArrival);
    }
    if chain_truth_count == 0
        && seer_rx_count == 0
        && seer_emitted_count == 0
        && runtime_seen_count == 0
        && runtime_accepted_count == 0
    {
        flags.push(CoverageAuditTimeoutClass::GenuineNoInterest);
    }
    if flags.is_empty() {
        flags.push(CoverageAuditTimeoutClass::Unclassified);
    }

    (flags.first().copied(), flags)
}

fn ratio(denominator: u64, numerator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        (numerator as f64 / denominator as f64) * 100.0
    }
}

pub fn coverage_audit() -> &'static CoverageAuditRecorder {
    static INSTANCE: OnceLock<CoverageAuditRecorder> = OnceLock::new();
    INSTANCE.get_or_init(CoverageAuditRecorder::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn truth_state(failed: bool) -> CoverageAuditTruthSignatureState {
        CoverageAuditTruthSignatureState {
            failed,
            time_source: None,
        }
    }

    fn record_for_reason(
        global: CoverageAuditGlobalSignatureState,
        pool: CoverageAuditPoolSignatureState,
        chain_truth_failed: bool,
    ) -> CoverageAuditReason {
        determine_missing_reason(&global, &pool, chain_truth_failed)
    }

    #[test]
    fn reason_precedence_prefers_not_received_first() {
        assert_eq!(
            record_for_reason(
                CoverageAuditGlobalSignatureState::default(),
                CoverageAuditPoolSignatureState::default(),
                false,
            ),
            CoverageAuditReason::NotReceived
        );
    }

    #[test]
    fn reason_precedence_detects_failed_tx_filtered() {
        assert_eq!(
            record_for_reason(
                CoverageAuditGlobalSignatureState::default(),
                CoverageAuditPoolSignatureState::default(),
                true,
            ),
            CoverageAuditReason::FailedTxFiltered
        );
    }

    #[test]
    fn reason_precedence_detects_filtered_before_parse() {
        assert_eq!(
            record_for_reason(
                CoverageAuditGlobalSignatureState {
                    raw_received: true,
                    parse_candidate: false,
                    parse_miss: false,
                    ..CoverageAuditGlobalSignatureState::default()
                },
                CoverageAuditPoolSignatureState::default(),
                false,
            ),
            CoverageAuditReason::FilteredBeforeParse
        );
    }

    #[test]
    fn reason_precedence_detects_parse_miss() {
        assert_eq!(
            record_for_reason(
                CoverageAuditGlobalSignatureState {
                    raw_received: true,
                    parse_candidate: true,
                    parse_miss: true,
                    ..CoverageAuditGlobalSignatureState::default()
                },
                CoverageAuditPoolSignatureState::default(),
                false,
            ),
            CoverageAuditReason::ParseMiss
        );
    }

    #[test]
    fn reason_precedence_detects_mapping_missing() {
        assert_eq!(
            record_for_reason(
                CoverageAuditGlobalSignatureState {
                    raw_received: true,
                    parse_candidate: true,
                    parse_miss: false,
                    mapping_missing: true,
                    ..CoverageAuditGlobalSignatureState::default()
                },
                CoverageAuditPoolSignatureState {
                    seer_rx: true,
                    ..CoverageAuditPoolSignatureState::default()
                },
                false,
            ),
            CoverageAuditReason::MappingMissing
        );
    }

    #[test]
    fn reason_precedence_detects_not_forwarded() {
        assert_eq!(
            record_for_reason(
                CoverageAuditGlobalSignatureState {
                    raw_received: true,
                    parse_candidate: true,
                    parse_miss: false,
                    ..CoverageAuditGlobalSignatureState::default()
                },
                CoverageAuditPoolSignatureState {
                    seer_rx: true,
                    ..CoverageAuditPoolSignatureState::default()
                },
                false,
            ),
            CoverageAuditReason::NotForwarded
        );
    }

    #[test]
    fn reason_precedence_detects_runtime_filtered() {
        assert_eq!(
            record_for_reason(
                CoverageAuditGlobalSignatureState {
                    raw_received: true,
                    parse_candidate: true,
                    parse_miss: false,
                    ..CoverageAuditGlobalSignatureState::default()
                },
                CoverageAuditPoolSignatureState {
                    seer_rx: true,
                    seer_emitted: true,
                    runtime_seen: true,
                    runtime_accepted: false,
                    ..CoverageAuditPoolSignatureState::default()
                },
                false,
            ),
            CoverageAuditReason::RuntimeFiltered
        );
    }

    #[test]
    fn recorder_moves_pending_state_into_opened_window() {
        let recorder = CoverageAuditRecorder::new();
        recorder.record_seer_rx("pool-1", "sig-1", "grpc_global_stream");
        recorder.record_seer_emitted("pool-1", "sig-1", "grpc_global_stream");

        let window_id = recorder.open_window("pool-1", Some("mint-1".to_string()), 10, 20);
        let closed = recorder
            .close_window("pool-1", Some("BUY".to_string()), false, None)
            .expect("window must close");

        assert_eq!(closed.window_id, window_id);
        let state = closed.signatures.get("sig-1").expect("sig state missing");
        assert!(state.seer_rx);
        assert!(state.seer_emitted);
    }

    #[test]
    fn build_record_assigns_reason_per_missing_truth_signature() {
        let recorder = CoverageAuditRecorder::new();
        recorder.record_raw_received("sig-a", "grpc_global_stream");
        recorder.record_parse_candidate("sig-a");
        recorder.record_parse_miss("sig-a");
        recorder.record_seer_rx("pool-1", "sig-b", "grpc_pool_stream");
        recorder.record_mapping_missing_with_reason(
            "pool-1",
            "sig-b",
            Some("curve_mapping_missing"),
        );
        recorder.record_seer_rx("pool-1", "sig-c", "grpc_global_stream");
        recorder.record_seer_emitted("pool-1", "sig-d", "grpc_pool_stream");
        recorder.record_runtime_filtered("pool-1", "sig-d", "duplicate_tx_key");
        recorder.record_runtime_seen("pool-1", "sig-d");
        recorder.open_window("pool-1", Some("mint-1".to_string()), 100, 200);
        recorder.record_seer_rx("pool-1", "sig-b", "grpc_pool_stream");
        recorder.record_mapping_missing_with_reason(
            "pool-1",
            "sig-b",
            Some("curve_mapping_missing"),
        );
        recorder.record_seer_rx("pool-1", "sig-c", "grpc_global_stream");
        recorder.record_seer_emitted("pool-1", "sig-d", "grpc_pool_stream");
        recorder.record_runtime_filtered("pool-1", "sig-d", "duplicate_tx_key");
        recorder.record_runtime_seen("pool-1", "sig-d");
        let closed = recorder
            .close_window(
                "pool-1",
                Some("REJECT".to_string()),
                false,
                Some("POOL_REJECTED_EARLY".to_string()),
            )
            .expect("window must close");

        let truth = HashMap::from([
            ("sig-a".to_string(), truth_state(false)),
            ("sig-b".to_string(), truth_state(false)),
            ("sig-c".to_string(), truth_state(false)),
            ("sig-d".to_string(), truth_state(false)),
            ("sig-e".to_string(), truth_state(true)),
            ("sig-f".to_string(), truth_state(false)),
        ]);
        let record = recorder.build_record(closed, truth, None);
        assert_eq!(record.schema_version, 5);
        assert_eq!(record.chain_truth_count, 6);
        assert_eq!(record.chain_truth_failed_count, 1);
        assert_eq!(record.missing_count, 6);
        assert_eq!(record.counts_by_reason.get("parse_miss"), Some(&1));
        assert_eq!(record.counts_by_reason.get("mapping_missing"), Some(&1));
        assert_eq!(
            record
                .mapping_missing_by_reason
                .get("curve_mapping_missing"),
            Some(&1)
        );
        assert_eq!(record.counts_by_reason.get("not_forwarded"), Some(&1));
        assert_eq!(record.counts_by_reason.get("runtime_filtered"), Some(&1));
        assert_eq!(record.counts_by_reason.get("failed_tx_filtered"), Some(&1));
        assert_eq!(record.counts_by_reason.get("not_received"), Some(&1));
        assert_eq!(
            record.runtime_filtered_by_reason.get("duplicate_tx_key"),
            Some(&1)
        );
        assert_eq!(
            record.filtered_reason_keys,
            vec!["duplicate_tx_key".to_string()]
        );
        assert_eq!(
            record
                .duplicate_suppression_by_reason
                .get("duplicate_tx_key"),
            Some(&1)
        );
        assert_eq!(
            record.raw_received_by_source.get("grpc_global_stream"),
            Some(&1)
        );
        assert_eq!(record.seer_rx_by_source.get("grpc_pool_stream"), Some(&2));
        assert_eq!(
            record.seer_emitted_by_source.get("grpc_pool_stream"),
            Some(&1)
        );
        let sig_b = record
            .missing_signatures
            .iter()
            .find(|entry| entry.signature == "sig-b")
            .expect("sig-b missing");
        assert_eq!(
            sig_b.mapping_missing_reason.as_deref(),
            Some("curve_mapping_missing")
        );
        let sig_e = record
            .missing_signatures
            .iter()
            .find(|entry| entry.signature == "sig-e")
            .expect("sig-e missing");
        assert!(sig_e.chain_truth_failed, "sig-e should be marked failed");
        assert_eq!(sig_e.reason, CoverageAuditReason::FailedTxFiltered);
    }

    #[test]
    fn mapping_missing_reason_can_be_recovered_from_global_signature_state() {
        let recorder = CoverageAuditRecorder::new();
        recorder.record_raw_received("sig-z", "grpc_global_stream");
        recorder.record_parse_candidate("sig-z");
        recorder.record_mapping_missing_with_reason(
            "unknown-pool",
            "sig-z",
            Some("missing_pool_from_mint"),
        );
        let closed = recorder.close_window(
            "pool-1",
            Some("REJECT".to_string()),
            false,
            Some("POOL_REJECTED_EARLY".to_string()),
        );
        assert!(
            closed.is_none(),
            "closing unopened window should return None"
        );

        recorder.open_window("pool-1", Some("mint-1".to_string()), 100, 200);
        let closed = recorder
            .close_window(
                "pool-1",
                Some("REJECT".to_string()),
                false,
                Some("POOL_REJECTED_EARLY".to_string()),
            )
            .expect("window must close");
        let truth = HashMap::from([("sig-z".to_string(), truth_state(false))]);
        let record = recorder.build_record(closed, truth, None);
        assert_eq!(record.counts_by_reason.get("mapping_missing"), Some(&1));
        assert_eq!(
            record
                .mapping_missing_by_reason
                .get("missing_pool_from_mint"),
            Some(&1)
        );
        assert_eq!(
            record.missing_signatures[0]
                .mapping_missing_reason
                .as_deref(),
            Some("missing_pool_from_mint")
        );
    }

    #[test]
    fn recorder_keeps_watch_and_backpressure_diagnostics() {
        let recorder = CoverageAuditRecorder::new();
        recorder.record_watch_registration(
            "pool-1",
            CoverageAuditWatchRegistration {
                wall_ms: 1234,
                source: "create".to_string(),
                registry_version: 7,
                exact_curve_accounts: 11,
                exact_pool_accounts: 12,
                watched_mints: 13,
                transport_resubs_sent: 3,
                transport_msgs_spilled: 4,
                transport_overflow_dropped: 5,
                transport_slot_gaps: 6,
                transport_last_msg_gap_ms: 7,
            },
        );
        recorder.record_pool_task_backpressure_drop("pool-1", true);
        recorder.open_window("pool-1", Some("mint-1".to_string()), 100, 200);
        let closed = recorder
            .close_window("pool-1", Some("BUY".to_string()), true, None)
            .expect("window must close");
        let record = recorder.build_record(closed, HashMap::new(), None);
        assert_eq!(record.diagnostics.watch_registration_wall_ms, Some(1234));
        assert_eq!(
            record.diagnostics.watch_registration_source.as_deref(),
            Some("create")
        );
        assert_eq!(record.diagnostics.registry_version_at_watch, Some(7));
        assert_eq!(record.diagnostics.hot_pool_backpressure_drop_count, 1);
        assert_eq!(record.diagnostics.pool_task_backpressure_drop_count, 1);
    }

    #[test]
    fn recorder_keeps_phase3_canonical_ingest_diagnostics() {
        let recorder = CoverageAuditRecorder::new();
        recorder.record_canonical_update_observed("pool-1", Some(321));
        recorder.record_account_update_runtime_seen("pool-1", "ingress_wall", None, true);
        recorder.record_seer_account_update_before_mapping("pool-1", false);
        recorder.record_seer_account_update_before_mapping("pool-1", true);
        recorder.record_seer_account_update_pending_replay("pool-1", Some(77), false, false);
        recorder.record_seer_account_update_pending_replay("pool-1", Some(91), true, true);
        recorder.record_timeout_without_canonical_updates("pool-1");

        recorder.open_window("pool-1", Some("mint-1".to_string()), 100, 200);
        let closed = recorder
            .close_window("pool-1", Some("TIMEOUT".to_string()), true, None)
            .expect("window must close");
        let record = recorder.build_record(closed, HashMap::new(), None);

        assert_eq!(record.schema_version, 5);
        assert_eq!(record.diagnostics.canonical_update_count, 1);
        assert_eq!(
            record.diagnostics.canonical_first_update_latency_ms,
            Some(321)
        );
        assert_eq!(record.diagnostics.live_account_update_count, 0);
        assert_eq!(record.diagnostics.account_update_runtime_seen_total, 1);
        assert_eq!(record.diagnostics.account_update_runtime_accepted_total, 1);
        assert_eq!(
            record
                .diagnostics
                .account_update_runtime_seen_by_effective_time_source
                .get("ingress_wall"),
            Some(&1)
        );
        assert!(record.diagnostics.timed_out_without_canonical_updates);
        assert_eq!(
            record.diagnostics.seer_account_updates_before_mapping_total,
            2
        );
        assert_eq!(
            record
                .diagnostics
                .seer_account_updates_pending_overwrite_total,
            1
        );
        assert_eq!(
            record.diagnostics.seer_account_updates_pending_replay_total,
            2
        );
        assert_eq!(
            record
                .diagnostics
                .seer_account_updates_pending_replay_send_failed_total,
            1
        );
        assert_eq!(
            record
                .diagnostics
                .seer_account_updates_pending_parse_failed_total,
            1
        );
        assert_eq!(
            record
                .diagnostics
                .seer_account_updates_pending_replay_max_dwell_ms,
            Some(91)
        );
        assert_eq!(
            record.timeout_primary_cause,
            Some(CoverageAuditTimeoutClass::StaleOrLateArrival)
        );
        assert_eq!(
            record.timeout_flags,
            vec![
                CoverageAuditTimeoutClass::StaleOrLateArrival,
                CoverageAuditTimeoutClass::GenuineNoInterest,
            ]
        );
    }

    #[test]
    fn recorder_does_not_mark_timeout_stale_for_fresh_replay_alone() {
        let recorder = CoverageAuditRecorder::new();
        recorder.record_account_update_runtime_seen("pool-1", "ingress_wall", None, true);
        recorder.record_seer_account_update_pending_replay("pool-1", Some(77), false, false);
        recorder.open_window("pool-1", Some("mint-1".to_string()), 100, 200);
        let closed = recorder
            .close_window("pool-1", Some("TIMEOUT".to_string()), true, None)
            .expect("window must close");
        let record = recorder.build_record(closed, HashMap::new(), None);

        assert_eq!(
            record.timeout_primary_cause,
            Some(CoverageAuditTimeoutClass::GenuineNoInterest)
        );
        assert_eq!(
            record.timeout_flags,
            vec![CoverageAuditTimeoutClass::GenuineNoInterest]
        );
    }

    #[test]
    fn recorder_does_not_treat_arbitrary_fallback_substrings_as_fallback_sources() {
        let (timeout_primary_cause, timeout_flags) = classify_timeout_window(
            Some("TIMEOUT"),
            None,
            true,
            100,
            0,
            0,
            0,
            0,
            0,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::from([("not_a_real_fallback_source".to_string(), 1)]),
            &BTreeMap::new(),
            &CoverageAuditInvariantSummary {
                emitted_without_rx: 0,
                runtime_accepted_without_emitted: 0,
                missing_reason_fallbacks: 0,
            },
            &CoverageAuditWindowDiagnostics::default(),
        );

        assert_eq!(
            timeout_primary_cause,
            Some(CoverageAuditTimeoutClass::GenuineNoInterest)
        );
        assert_eq!(
            timeout_flags,
            vec![CoverageAuditTimeoutClass::GenuineNoInterest]
        );
    }

    #[test]
    fn recorder_tracks_runtime_effective_source_and_fallback_class() {
        let recorder = CoverageAuditRecorder::new();
        recorder.record_runtime_seen_with_detail("pool-1", "sig-chain", "chain_event", None);
        recorder.record_runtime_seen_with_detail(
            "pool-1",
            "sig-legacy",
            "wall_clock_fallback",
            Some("legacy_compat_rejected"),
        );

        recorder.open_window("pool-1", Some("mint-1".to_string()), 100, 200);
        let closed = recorder
            .close_window("pool-1", Some("TIMEOUT".to_string()), true, None)
            .expect("window must close");
        let record = recorder.build_record(
            closed,
            HashMap::from([
                (
                    "sig-chain".to_string(),
                    CoverageAuditTruthSignatureState::default(),
                ),
                (
                    "sig-legacy".to_string(),
                    CoverageAuditTruthSignatureState::default(),
                ),
            ]),
            None,
        );

        assert_eq!(
            record.runtime_seen_by_time_source.get("chain_event"),
            Some(&1)
        );
        assert_eq!(
            record
                .runtime_seen_by_effective_time_source
                .get("wall_clock_fallback"),
            Some(&1)
        );
        assert_eq!(
            record.dominant_runtime_effective_time_source.as_deref(),
            Some("chain_event")
        );
        assert_eq!(
            record
                .runtime_seen_by_fallback_class
                .get("legacy_compat_rejected"),
            Some(&1)
        );
        assert_eq!(
            record.missing_signatures[1]
                .runtime_effective_time_source
                .as_deref(),
            Some("wall_clock_fallback")
        );
        assert_eq!(
            record.missing_signatures[1]
                .runtime_fallback_class
                .as_deref(),
            Some("legacy_compat_rejected")
        );
    }

    #[test]
    fn recorder_merges_alias_pending_diagnostics_into_window() {
        let recorder = CoverageAuditRecorder::new();
        recorder.record_seer_account_update_before_mapping("curve-1", true);
        recorder.record_seer_account_update_pending_replay("curve-1", Some(55), false, false);
        recorder.record_canonical_update_observed("curve-1", Some(42));
        recorder.record_live_account_update_observed("curve-1", Some(44));
        recorder.register_pool_alias("curve-1", "pool-1");

        recorder.open_window("pool-1", Some("mint-1".to_string()), 100, 200);
        let closed = recorder
            .close_window("pool-1", Some("BUY".to_string()), true, None)
            .expect("window must close");
        let record = recorder.build_record(closed, HashMap::new(), None);

        assert_eq!(
            record.diagnostics.seer_account_updates_before_mapping_total,
            1
        );
        assert_eq!(
            record
                .diagnostics
                .seer_account_updates_pending_overwrite_total,
            1
        );
        assert_eq!(
            record.diagnostics.seer_account_updates_pending_replay_total,
            1
        );
        assert_eq!(
            record
                .diagnostics
                .seer_account_updates_pending_replay_max_dwell_ms,
            Some(55)
        );
        assert_eq!(record.diagnostics.canonical_update_count, 1);
        assert_eq!(
            record.diagnostics.canonical_first_update_latency_ms,
            Some(42)
        );
        assert_eq!(record.diagnostics.live_account_update_count, 1);
        assert_eq!(
            record.diagnostics.live_first_account_update_latency_ms,
            Some(44)
        );
    }

    #[test]
    fn build_record_ignores_non_truth_window_noise_for_counts_and_invariants() {
        let recorder = CoverageAuditRecorder::new();
        recorder.open_window("pool-1", Some("mint-1".to_string()), 100, 200);
        recorder.record_seer_rx("pool-1", "sig-truth", "grpc_global_stream");
        recorder.record_seer_emitted("pool-1", "sig-truth", "grpc_global_stream");
        recorder.record_runtime_seen("pool-1", "sig-truth");
        recorder.record_runtime_accepted("pool-1", "sig-truth");

        recorder.record_seer_emitted("pool-1", "sig-noise-emitted-only", "grpc_pool_stream");
        recorder.record_runtime_filtered("pool-1", "sig-noise-emitted-only", "duplicate_tx_key");
        recorder.record_runtime_seen("pool-1", "sig-noise-emitted-only");
        recorder.record_runtime_accepted("pool-1", "sig-noise-accepted-only");

        let closed = recorder
            .close_window("pool-1", Some("BUY".to_string()), true, None)
            .expect("window must close");

        let truth = HashMap::from([("sig-truth".to_string(), truth_state(false))]);
        let record = recorder.build_record(closed, truth, None);

        assert_eq!(record.chain_truth_count, 1);
        assert_eq!(record.seer_rx_count, 1);
        assert_eq!(record.seer_emitted_count, 1);
        assert_eq!(record.runtime_seen_count, 1);
        assert_eq!(record.runtime_accepted_count, 1);
        assert_eq!(record.truth_to_rx_pct, 100.0);
        assert_eq!(record.truth_to_emit_pct, 100.0);
        assert_eq!(record.truth_to_runtime_accept_pct, 100.0);
        assert_eq!(record.invariants.emitted_without_rx, 0);
        assert_eq!(record.invariants.runtime_accepted_without_emitted, 0);
    }

    #[test]
    fn runtime_filtered_without_explicit_reason_gets_fail_closed_bucket() {
        let recorder = CoverageAuditRecorder::new();
        recorder.open_window("pool-1", Some("mint-1".to_string()), 100, 200);
        recorder.record_seer_rx("pool-1", "sig-filtered", "grpc_pool_stream");
        recorder.record_seer_emitted("pool-1", "sig-filtered", "grpc_pool_stream");
        recorder.record_runtime_seen("pool-1", "sig-filtered");

        let closed = recorder
            .close_window(
                "pool-1",
                Some("TIMEOUT".to_string()),
                true,
                Some("END_REACHED".to_string()),
            )
            .expect("window must close");
        let record = recorder.build_record(
            closed,
            HashMap::from([("sig-filtered".to_string(), truth_state(false))]),
            None,
        );

        assert_eq!(record.counts_by_reason.get("runtime_filtered"), Some(&1));
        assert_eq!(
            record
                .runtime_filtered_by_reason
                .get("runtime_filter_reason_missing"),
            Some(&1)
        );
        assert_eq!(
            record.filtered_reason_keys,
            vec!["runtime_filter_reason_missing".to_string()]
        );
        assert_eq!(
            record.missing_signatures[0]
                .runtime_filter_reason
                .as_deref(),
            Some("runtime_filter_reason_missing")
        );
        assert_eq!(
            record.timeout_primary_cause,
            Some(CoverageAuditTimeoutClass::FilterDrop)
        );
    }

    #[test]
    fn timeout_taxonomy_marks_genuine_no_interest_windows() {
        let recorder = CoverageAuditRecorder::new();
        recorder.open_window("pool-1", Some("mint-1".to_string()), 100, 200);
        let closed = recorder
            .close_window(
                "pool-1",
                Some("TIMEOUT".to_string()),
                true,
                Some("END_REACHED".to_string()),
            )
            .expect("window must close");
        let record = recorder.build_record(closed, HashMap::new(), None);

        assert_eq!(
            record.timeout_primary_cause,
            Some(CoverageAuditTimeoutClass::GenuineNoInterest)
        );
        assert_eq!(
            record.timeout_flags,
            vec![CoverageAuditTimeoutClass::GenuineNoInterest]
        );
    }

    #[test]
    fn serialized_record_always_emits_required_coverage_contract_fields() {
        let recorder = CoverageAuditRecorder::new();
        recorder.open_window("pool-1", Some("mint-1".to_string()), 100, 200);
        let closed = recorder
            .close_window(
                "pool-1",
                Some("REJECT".to_string()),
                true,
                Some("POOL_REJECTED_EARLY".to_string()),
            )
            .expect("window must close");
        let record = recorder.build_record(closed, HashMap::new(), None);
        let json = serde_json::to_value(&record).expect("record should serialize");
        let object = json
            .as_object()
            .expect("coverage record should serialize to object");

        assert!(object.contains_key("timeout_primary_cause"));
        assert!(object.contains_key("timeout_flags"));
        assert!(object.contains_key("filtered_reason_keys"));
        assert!(object.contains_key("dominant_runtime_effective_time_source"));
        assert_eq!(
            object.get("timeout_primary_cause"),
            Some(&serde_json::Value::Null)
        );
        assert_eq!(
            object.get("timeout_flags"),
            Some(&serde_json::Value::Array(Vec::new()))
        );
        assert_eq!(
            object.get("filtered_reason_keys"),
            Some(&serde_json::Value::Array(Vec::new()))
        );
        assert_eq!(
            object.get("dominant_runtime_effective_time_source"),
            Some(&serde_json::Value::Null)
        );
    }

    #[test]
    fn timeout_taxonomy_marks_ingest_miss_windows() {
        let recorder = CoverageAuditRecorder::new();
        recorder.open_window("pool-1", Some("mint-1".to_string()), 100, 200);
        let closed = recorder
            .close_window(
                "pool-1",
                Some("TIMEOUT".to_string()),
                true,
                Some("END_REACHED_BY_SWEEP".to_string()),
            )
            .expect("window must close");
        let record = recorder.build_record(
            closed,
            HashMap::from([("sig-missed".to_string(), truth_state(false))]),
            None,
        );

        assert_eq!(
            record.timeout_primary_cause,
            Some(CoverageAuditTimeoutClass::IngestMiss)
        );
        assert!(record
            .timeout_flags
            .contains(&CoverageAuditTimeoutClass::IngestMiss));
    }
}
