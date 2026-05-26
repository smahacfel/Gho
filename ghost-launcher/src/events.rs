//! Unified Memory Bus - Ghost Event System
//!
//! This module provides a unified event bus for communication between Ghost components.
//! Uses `tokio::sync::broadcast` for multi-consumer event distribution.
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────┐   GhostEvent   ┌───────────┐
//! │   Seer   │ ─────────────► │   Bus     │
//! └──────────┘                │ (10,240)  │
//!                             └─────┬─────┘
//!                                   │
//!                     ┌─────────────┼─────────────┐
//!                     ▼             ▼             ▼
//!               ┌──────────┐ ┌──────────┐ ┌──────────┐
//!               │ Trigger  │ │  Oracle  │ │ Metrics  │
//!               └──────────┘ └──────────┘ └──────────┘
//! ```
//!
//! ## Usage
//!
//! ```ignore
//! use ghost_launcher::events::{GhostEvent, create_event_bus, TradeResult};
//!
//! // Create the event bus
//! let (bus_tx, bus_rx) = create_event_bus();
//!
//! // Seer emits events
//! bus_tx.send(GhostEvent::NewPoolDetected(pool_data))?;
//!
//! // Trigger receives and acts
//! let event = bus_rx.recv().await?;
//! ```

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SimulationAccountManifestEntry {
    pub pubkey: String,
    pub role: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_authority_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_mismatch_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction_index: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_index: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_signer: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_writable: Option<bool>,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_expected: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buy_variant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_param_role: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SimulationAccountNotFoundCandidate {
    pub pubkey: String,
    pub role: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction_index: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_index: Option<u64>,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_fatality: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_exclusion_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShadowSimulationAccountDiagnostics {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_shadow_precheck_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_shadow_lifecycle_eligibility_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_feasibility_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_feasibility_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_resolution_terminal_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle_label_eligibility: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precheck_failure_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simulation_error_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simulation_error_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simulation_error_account_pubkey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simulation_error_account_role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simulation_error_account_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simulation_error_instruction_index: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simulation_error_account_index: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub simulation_error_account_candidates: Vec<SimulationAccountNotFoundCandidate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub simulation_error_account_candidates_raw: Vec<SimulationAccountNotFoundCandidate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub simulation_error_account_candidates_narrowed: Vec<SimulationAccountNotFoundCandidate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub simulation_error_account_candidates_excluded: Vec<SimulationAccountNotFoundCandidate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simulation_error_account_narrowing_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simulation_error_account_narrowing_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simulation_error_category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bonding_curve_v2_pubkey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bonding_curve_v2_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bonding_curve_v2_authority_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bonding_curve_v2_identity_authority_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bonding_curve_v2_mismatch_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bonding_curve_pubkey_from_diag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bonding_curve_pubkey_from_mfs: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bonding_curve_v2_seen_in_diag: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bonding_curve_v2_seen_in_mfs: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bonding_curve_v2_seen_in_account_state: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bonding_curve_ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bonding_curve_v2_rpc_load_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bonding_curve_v2_rpc_load_ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bonding_curve_v2_ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builder_required_curve_account_ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builder_required_curve_account_ready_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_bcv2_source_tx_signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_bcv2_source_slot: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_bcv2_source_slot_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_bcv2_source_instruction_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_bcv2_source_program_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_bcv2_source_discriminator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_bcv2_source_buy_variant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_bcv2_instruction_account_position: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_bcv2_message_account_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_bcv2_resolved_pubkey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_bcv2_loaded_address_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_bcv2_tx_success: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_bcv2_meta_err: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_bcv2_provenance_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_resolution_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_route_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_route_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_route_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_route_account_set_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected_route_account_set_roles: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_route_precheck_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_route_simulation_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_route_handoff_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_route_handoff_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_route_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_route_ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_route_not_ready_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_route_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_route_attempted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_route_ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_route_not_ready_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback_missing_roles: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback_missing_pubkeys: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback_account_sources: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback_simulation_load_account_set: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback_creatable_account_set: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback_required_precheck_account_set: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_failure_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_executable_route_account_set_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_buy_account_set_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_buy_curve_pubkey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_buy_curve_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_buy_curve_authority_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_buy_curve_rpc_load_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_buy_curve_rpc_load_ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_buy_curve_authority_readiness_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_buy_associated_bonding_curve_pubkey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_buy_associated_bonding_curve_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_buy_associated_bonding_curve_rpc_load_ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub legacy_buy_required_roles: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub legacy_buy_missing_roles: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub legacy_buy_missing_pubkeys: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_buy_route_ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_buy_route_not_ready_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_parity_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_request_built: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_buy_variant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_rpc_manifest_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_sender_manifest_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub working_builder_rpc_manifest_account_roles: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub working_builder_sender_manifest_account_roles: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_manifest_contains_bcv2: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_manifest_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_pubkey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_source_authority: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_rpc_load_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_rpc_load_ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_seen_in_observed_tx: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_seen_in_account_state: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_seen_in_mfs: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_seen_in_diag: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_readiness_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_precheck_pubkey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_builder_pubkey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_observed_pubkey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_pubkey_consistency_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_observed_slot: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_observed_tx_signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_precheck_context_slot: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_precheck_commitment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_precheck_attempt_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_precheck_latency_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_precheck_age_from_observed_slot: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_rpc_error_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_reconciliation_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_materialization_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_subscription_requested: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_account_update_received: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_account_update_mapped: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_rpc_fetch_ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_rpc_fetch_missing: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_rpc_fetch_owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_rpc_fetch_data_len: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_account_state_materialized: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_mfs_materialized: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_diag_materialized: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_account_state_lookup_performed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_account_state_seen: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_account_state_seen_slot: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_account_state_age_slots: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_account_state_owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_account_state_data_len: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_mfs_seen_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_diag_seen_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_local_coverage_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_evidence_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_evidence_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_evidence_ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_evidence_owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_evidence_data_len: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_evidence_slot: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_evidence_context_slot: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_evidence_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_evidence_conflict: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_execution_evidence_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_execution_evidence_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_execution_evidence_ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_execution_evidence_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_execution_evidence_conflict: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_execution_evidence_stale: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_execution_evidence_exact_pubkey_match: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_execution_evidence_age_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_execution_evidence_owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_execution_evidence_data_len: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_execution_evidence_slot: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_bcv2_execution_evidence_context_slot: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_creator_vault_pubkey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_creator_vault_source_authority: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_creator_vault_rpc_load_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_creator_vault_rpc_load_ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_creator_vault_seen_in_account_state: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_creator_vault_seen_in_mfs: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_creator_vault_seen_in_observed_tx: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_builder_creator_vault_readiness_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub working_builder_required_accounts: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub working_builder_creatable_accounts: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub working_builder_ephemeral_accounts: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub working_builder_missing_required_accounts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precheck_account_set_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prepared_request_account_set_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simulation_account_set_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precheck_account_set_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prepared_request_account_set_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simulation_account_set_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_set_match: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_set_mismatch_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accounts_only_in_precheck: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accounts_only_in_simulation: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_manifest_available: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_manifest_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub simulation_account_manifest: Vec<SimulationAccountManifestEntry>,
}

use crate::components::trigger::safety::PositionSlotId;
use tokio::sync::broadcast;

use ghost_core::{
    CurveFinality, EventSemanticEnvelope, EventTimeMetadata, ExecutionAccountEvidence,
};
use seer::ipc::{AccountUpdateReplayOrigin, FundingTransferProvenance};

// Re-export RawBytesMissingReason from seer for use in events
pub use seer::types::RawBytesMissingReason;
use seer::types::{TokenDelta, ToolchainFingerprintInput};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObservedAccountMetaProvenance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_tx_signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_slot: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_slot_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_instruction_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_program_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_discriminator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_buy_variant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction_account_position: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_account_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_pubkey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loaded_address_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_success: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta_err: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance_status: Option<String>,
}

/// Default buffer size for the event bus
///
/// Buffer size: 10,240 events
/// Rationale (Issue #156):
///   - Solana can produce 2-3k TPS during spam
///   - With 100-200 active pools, each pool generates ~10-30 events/sec
///   - Small buffer (16-64) causes RecvError::Lagged under load
///   - 10,240 provides ~5-10 second buffer at peak load
///   - Prevents SSMI entropy calculation gaps from lagged receiver
pub const EVENT_BUS_BUFFER_SIZE: usize = 10_240;

/// Represents the result of a trade execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeResult {
    /// Transaction signature
    pub signature: String,
    /// Token mint address
    pub mint: String,
    /// Amount of SOL spent/received
    pub sol_amount: f64,
    /// Amount of tokens bought/sold
    pub token_amount: f64,
    /// Entry price (SOL per token)
    pub entry_price: f64,
    /// Whether this was a buy (true) or sell (false)
    pub is_buy: bool,
    /// Slot when the transaction was confirmed
    pub slot: Option<u64>,
    /// PnL in SOL (for sells)
    pub pnl_sol: Option<f64>,
    /// Timestamp of the trade
    pub timestamp: u64,
}

/// Represents a newly detected pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedPool {
    /// Cross-source semantic envelope carried through canonical ingest.
    #[serde(default)]
    pub semantic: EventSemanticEnvelope,

    /// Pool AMM ID
    pub pool_amm_id: String,
    /// Base token mint
    pub base_mint: String,
    /// Quote token mint
    pub quote_mint: String,
    /// AMM program ID (pumpfun or bonkfun)
    pub amm_program: String,
    /// Bonding curve address
    pub bonding_curve: String,
    /// Creator/deployer wallet address
    pub creator: String,
    /// Slot when detected
    pub slot: Option<u64>,
    /// Detection timestamp in epoch-milliseconds
    pub timestamp_ms: u64,
    /// Explicit provenance for event/ingest time axes.
    #[serde(default)]
    pub event_time: EventTimeMetadata,
    /// Local wall-clock timestamp when the wrapper emitted NewPoolDetected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detected_wall_ts_ms: Option<u64>,
    /// Initial liquidity in SOL (if available)
    pub initial_liquidity_sol: Option<f64>,
    /// Transaction signature that created the pool
    pub signature: String,
}

/// Represents a pool transaction parsed from WebSocket/Geyser
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolTransaction {
    /// Cross-source semantic envelope carried through canonical ingest.
    #[serde(default)]
    pub semantic: EventSemanticEnvelope,

    /// Pool this transaction affects
    pub pool_amm_id: String,
    /// Slot number
    pub slot: Option<u64>,
    /// Stable event ordinal within the source transaction, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_ordinal: Option<u32>,
    /// Optional parser-side outer instruction index for execution provenance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outer_instruction_index: Option<u32>,
    /// Optional parser-side inner group index for execution provenance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inner_group_index: Option<u32>,
    /// Optional outer program id observed by the parser for this trade.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outer_program_id: Option<String>,
    /// Optional CPI stack height from the parser execution tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpi_stack_height: Option<u32>,
    /// Timestamp in milliseconds
    pub timestamp_ms: u64,
    /// Explicit provenance for event/ingest time axes.
    #[serde(default)]
    pub event_time: EventTimeMetadata,

    /// Monotonic arrival timestamp in milliseconds (assigned at WS reception)
    pub arrival_ts_ms: u64,
    /// Primary signer
    pub signer: String,
    /// True if this is a buy, false if sell
    pub is_buy: bool,
    /// Volume in SOL
    pub volume_sol: f64,

    /// Canonical SOL amount in lamports (Buy: SOL in, Sell: SOL out) if available.
    pub sol_amount_lamports: Option<u64>,

    /// Canonical token amount in base units (Buy: tokens out, Sell: tokens in) if available.
    pub token_amount_units: Option<u64>,
    /// Updated base reserve (if available)
    pub reserve_base: Option<f64>,
    /// Updated quote reserve (if available)
    pub reserve_quote: Option<f64>,
    /// Updated price (if available)
    pub price_quote: Option<f64>,
    /// True if this is a developer buy
    pub is_dev_buy: bool,
    /// Developer buy amount in lamports
    pub dev_buy_lamports: u64,
    /// Transaction signature
    pub signature: String,
    /// True when transaction succeeded (meta.err is None)
    #[serde(default = "default_tx_success")]
    pub success: bool,
    /// Parsed error code if transaction failed
    #[serde(default)]
    pub error_code: Option<String>,
    /// Compute units consumed (if available)
    #[serde(default)]
    pub compute_units_consumed: Option<u64>,
    /// Owner-resolved token deltas derived from tx token balance ownership.
    #[serde(default)]
    pub owner_token_deltas: Vec<TokenDelta>,
    /// MPCF payload bytes (aggregated instruction data for actor classification)
    pub mpcf_payload: Vec<u8>,
    /// Reason why MPCF payload is missing (if mpcf_payload is empty)
    pub mpcf_payload_missing_reason: RawBytesMissingReason,
    /// Token mint address (base token being traded)
    /// This is needed for WEST to track token-specific wallet states
    pub token_mint: Option<String>,

    /// Virtual tokens remaining in bonding curve.
    /// PumpPortal: `vTokensInBondingCurve`
    /// Yellowstone: computed from account state
    #[serde(default)]
    pub v_tokens_in_bonding_curve: Option<f64>,

    /// Virtual SOL remaining in bonding curve.
    /// PumpPortal: `vSolInBondingCurve`
    /// Yellowstone: computed from account state
    #[serde(default)]
    pub v_sol_in_bonding_curve: Option<f64>,

    /// Market cap in SOL as reported by data source.
    /// PumpPortal: `marketCapSol`
    /// Yellowstone: computed from reserves
    #[serde(default)]
    pub market_cap_sol: Option<f64>,

    /// Optional pool/global config account captured from the source instruction.
    #[serde(default)]
    pub global_config: Option<String>,

    /// Optional fee recipient captured from the source instruction.
    #[serde(default)]
    pub fee_recipient: Option<String>,

    /// Optional token program captured from the source instruction.
    #[serde(default)]
    pub token_program: Option<String>,

    /// Observed Pump.fun buy variant from the source instruction.
    #[serde(default)]
    pub buy_variant: Option<String>,

    /// Observed associated bonding curve token account from the source instruction.
    #[serde(default)]
    pub associated_bonding_curve: Option<String>,

    /// Observed route-specific bonding_curve_v2 account from the source instruction.
    ///
    /// This account is separate from the canonical bonding_curve/pool id and
    /// must come from transaction account metas or another authoritative route
    /// source before routed simulation treats it as execution-ready.
    #[serde(default)]
    pub bonding_curve_v2: Option<String>,

    /// Parser-side provenance for `bonding_curve_v2` when it was copied from
    /// an observed source transaction account meta.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bonding_curve_v2_provenance: Option<ObservedAccountMetaProvenance>,

    /// PumpPortal internal flag indicating unusual market conditions.
    /// Passed through for future analysis.
    #[serde(default)]
    pub is_mayhem_mode: Option<bool>,

    /// CU price in micro-lamports/CU from SetComputeUnitPrice instruction.
    #[serde(default)]
    pub cu_price_micro_lamports: Option<u64>,

    /// CU limit from SetComputeUnitLimit instruction.
    #[serde(default)]
    pub compute_unit_limit: Option<u32>,

    /// Total number of inner instructions across all top-level instructions.
    #[serde(default)]
    pub inner_ix_count: Option<u32>,

    /// Maximum CPI stack depth from inner instructions.
    #[serde(default)]
    pub cpi_depth: Option<u32>,

    /// Number of ATA-creation inner instructions.
    #[serde(default)]
    pub ata_create_count: Option<u32>,

    /// SOL pre-balance (lamports) of the signer before the transaction.
    #[serde(default)]
    pub signer_pre_balance_lamports: Option<u64>,

    /// SOL post-balance (lamports) of the signer after the transaction.
    #[serde(default)]
    pub signer_post_balance_lamports: Option<u64>,

    /// Deterministic Jito-tip detection derived from tx instructions.
    #[serde(default)]
    pub jito_tip_detected: Option<bool>,

    /// Parser-side raw infrastructure fingerprint used by FTDI/DBIA.
    #[serde(default, skip_serializing_if = "ToolchainFingerprintInput::is_empty")]
    pub toolchain_fingerprint: ToolchainFingerprintInput,

    /// Whether bonding curve data was successfully parsed from an AccountUpdate
    /// or received from a trusted data source (e.g. PumpPortal).
    /// Set explicitly by the parser — NOT derived from reserve values.
    #[serde(default)]
    pub curve_data_known: bool,

    /// Finality tier of the curve state used for this transaction.
    /// Defaults to `speculative` for backward-compatible deserialization.
    #[serde(default)]
    pub curve_finality: CurveFinality,
}

fn default_tx_success() -> bool {
    true
}

impl DetectedPool {
    pub fn effective_event_ts_ms(&self) -> Option<u64> {
        self.event_time.effective_event_ts_ms()
    }

    pub fn compat_event_ts_ms(&self) -> Option<u64> {
        self.event_time
            .compat_event_ts_ms((self.timestamp_ms > 0).then_some(self.timestamp_ms))
    }
}

impl PoolTransaction {
    pub fn effective_event_ts_ms(&self) -> Option<u64> {
        self.event_time.effective_event_ts_ms()
    }

    pub fn compat_event_ts_ms(&self) -> Option<u64> {
        self.event_time
            .compat_event_ts_ms((self.timestamp_ms > 0).then_some(self.timestamp_ms))
    }
}

/// Funding transfer observation carried on the launcher event bus.
///
/// `full_chain_coverage` remains the stable readiness bit used by downstream FSC
/// plumbing. `provenance` is additive and freezes the lane/replay contract so
/// filtered `grpc_global_stream` observations cannot be reinterpreted as an
/// authoritative funding plane.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundingTransferObserved {
    /// Cross-source semantic envelope carried through canonical ingest.
    #[serde(default)]
    pub semantic: EventSemanticEnvelope,

    /// Slot of the source transaction when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<u64>,

    /// Stable event ordinal within the source transaction when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_ordinal: Option<u32>,

    /// Optional parser-side outer instruction index for execution provenance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outer_instruction_index: Option<u32>,

    /// Optional parser-side inner group index for execution provenance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inner_group_index: Option<u32>,

    /// Optional CPI stack height from the parser execution tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpi_stack_height: Option<u32>,

    /// Explicit provenance for event/ingest time axes.
    #[serde(default)]
    pub event_time: EventTimeMetadata,

    /// Monotonic arrival timestamp captured at ingest time.
    #[serde(default)]
    pub arrival_ts_ms: u64,

    /// Source transaction signature.
    pub signature: String,

    /// Funding sender wallet.
    pub source_wallet: String,

    /// Funding recipient wallet.
    pub recipient_wallet: String,

    /// Transfer size in lamports.
    pub lamports: u64,

    /// Whether the upstream feed had chain-wide coverage for wallet funding provenance.
    ///
    /// `false` includes the current filtered `grpc_global_stream` lane and must
    /// not be treated as authoritative pre-buy funding coverage for FSC.
    #[serde(default)]
    pub full_chain_coverage: bool,

    /// Additive funding-lane provenance contract from Seer IPC.
    ///
    /// Omitted for the current default filtered contract to preserve legacy JSON
    /// shape across existing fixtures and log surfaces.
    #[serde(
        default,
        skip_serializing_if = "FundingTransferProvenance::is_legacy_default"
    )]
    pub provenance: FundingTransferProvenance,

    /// Wall-clock time when the producer emitted the event.
    pub detected_at: std::time::SystemTime,

    /// Monotonically increasing producer-side sequence number.
    pub sequence_number: u64,
}

impl FundingTransferObserved {
    pub fn effective_event_ts_ms(&self) -> Option<u64> {
        self.event_time.effective_event_ts_ms()
    }

    pub fn compat_event_ts_ms(&self) -> Option<u64> {
        self.event_time.compat_event_ts_ms(Some(self.arrival_ts_ms))
    }
}

/// Role-aware execution account evidence on the launcher event bus.
///
/// This event carries account-existence/loadability evidence for a concrete
/// execution account pubkey and role. It is intentionally separate from
/// canonical reserve `AccountUpdateEvent`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionAccountEvidenceEvent {
    pub evidence: ExecutionAccountEvidence,
    pub detected_at: std::time::SystemTime,
    pub sequence_number: u64,
}

/// On-chain AccountUpdate payload on the launcher event bus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountUpdateEvent {
    /// Cross-source semantic envelope carried through canonical ingest.
    #[serde(default)]
    pub semantic: EventSemanticEnvelope,
    /// Explicit provenance for event/ingest time axes.
    #[serde(default)]
    pub event_time: EventTimeMetadata,
    /// Resolved base mint — the key used by ReconciliationRuntime.
    pub base_mint: solana_sdk::pubkey::Pubkey,
    /// Bonding-curve account pubkey this update originated from.
    pub bonding_curve: solana_sdk::pubkey::Pubkey,
    /// Finality tier of the on-chain curve snapshot carried by this update.
    pub curve_finality: CurveFinality,
    /// Virtual SOL reserves from the on-chain bonding-curve account.
    pub sol_reserves: u64,
    /// Virtual token reserves from the on-chain bonding-curve account.
    pub token_reserves: u64,
    /// Curve completion flag (1 = graduated, 0 = active).
    pub complete: u8,
    /// Slot at which this AccountUpdate was observed.
    pub slot: u64,
    /// Optional Solana account write-version from Yellowstone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write_version: Option<u64>,
    /// Whether this update came live or from a pending pre-mapping replay path.
    #[serde(default)]
    pub replay_origin: AccountUpdateReplayOrigin,
    /// Dwell time in the pending pre-mapping buffer before replay, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_buffer_dwell_ms: Option<u64>,
    /// Wall-clock time when the producer emitted the event.
    pub detected_at: std::time::SystemTime,
    /// Monotonically increasing producer-side sequence number.
    pub sequence_number: u64,
}

/// Represents the result of oracle scoring for a pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolScoredEvent {
    /// Pool AMM ID
    pub pool_amm_id: String,
    /// Base token mint
    pub base_mint: String,
    /// Final score (0-100)
    pub score: f32,
    /// Whether the pool passed the threshold
    pub passed: bool,
    /// Risk level classification
    pub risk_level: String,
    /// Human-readable interpretation
    pub interpretation: String,
    /// Processing time in microseconds
    pub processing_time_us: u128,
    /// Individual component scores (for debugging)
    pub component_scores: serde_json::Value,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionJoinMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ab_record_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ab_record_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dispatch_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collection_plane: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe_plane: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_feature_snapshot_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_policy_config_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_plane: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollout_namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brain_config_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brain_config_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_decision_log_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_decision_row_offset: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_decision_row_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_v3_feature_snapshot_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_v3_policy_config_hash: Option<String>,
}

impl ExecutionJoinMetadata {
    pub fn is_empty(&self) -> bool {
        self.ab_record_id.is_none()
            && self.source_ab_record_id.is_none()
            && self.probe_id.is_none()
            && self.dispatch_source.is_none()
            && self.collection_plane.is_none()
            && self.probe_plane.is_none()
            && self.v3_feature_snapshot_hash.is_none()
            && self.v3_policy_config_hash.is_none()
            && self.decision_plane.is_none()
            && self.rollout_namespace.is_none()
            && self.run_id.is_none()
            && self.session_id.is_none()
            && self.brain_config_path.is_none()
            && self.brain_config_hash.is_none()
            && self.source_decision_log_path.is_none()
            && self.source_decision_row_offset.is_none()
            && self.source_decision_row_sha256.is_none()
            && self.source_v3_feature_snapshot_hash.is_none()
            && self.source_v3_policy_config_hash.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowBuySimulationEvent {
    #[serde(default, flatten)]
    pub join_metadata: ExecutionJoinMetadata,
    #[serde(default, flatten)]
    pub account_diagnostics: ShadowSimulationAccountDiagnostics,
    pub candidate_id: String,
    pub pool_amm_id: String,
    pub base_mint: String,
    pub mint: String,
    pub live_signature: Option<String>,
    pub payer_pubkey: String,
    pub payer_provenance: String,
    pub amount_lamports: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_token_amount_raw: Option<u64>,
    pub tip_lamports: u64,
    pub decision_ts_ms: u64,
    pub simulation_started_ts_ms: u64,
    pub simulation_finished_ts_ms: u64,
    pub latency_ms: u64,
    pub shadow_duration_ms: u64,
    pub rpc_slot: u64,
    pub retry_count: usize,
    pub used_sig_verify: bool,
    pub used_replace_recent_blockhash: bool,
    pub units_consumed: Option<u64>,
    pub logs: Vec<String>,
    pub return_data: Option<String>,
    pub err: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_detail_class: Option<String>,
}

pub fn build_execution_candidate_id(
    base_mint: impl AsRef<str>,
    pool_amm_id: impl AsRef<str>,
    trace_ref: impl AsRef<str>,
) -> String {
    format!(
        "{}_{}_{}",
        base_mint.as_ref(),
        pool_amm_id.as_ref(),
        trace_ref.as_ref()
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimePlane {
    CanonicalDecision,
    LegacyObservation,
    ShadowSimulation,
    PostBuyMonitoring,
}

impl RuntimePlane {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CanonicalDecision => "canonical_decision",
            Self::LegacyObservation => "legacy_observation",
            Self::ShadowSimulation => "shadow_simulation",
            Self::PostBuyMonitoring => "post_buy_monitoring",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyPathClassification {
    ObservabilityOnly,
    CompatibilityOnly,
    DisabledInProduction,
    DeprecatedWithRemovalDate,
}

impl LegacyPathClassification {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ObservabilityOnly => "observability_only",
            Self::CompatibilityOnly => "compatibility_only",
            Self::DisabledInProduction => "disabled_in_production",
            Self::DeprecatedWithRemovalDate => "deprecated_with_removal_date",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyPathDescriptor {
    pub path: &'static str,
    pub classification: LegacyPathClassification,
    pub runtime_plane: RuntimePlane,
    pub allows_authoritative_buy: bool,
    pub removal_date: Option<&'static str>,
}

impl LegacyPathDescriptor {
    pub const fn new(
        path: &'static str,
        classification: LegacyPathClassification,
        runtime_plane: RuntimePlane,
        allows_authoritative_buy: bool,
        removal_date: Option<&'static str>,
    ) -> Self {
        Self {
            path,
            classification,
            runtime_plane,
            allows_authoritative_buy,
            removal_date,
        }
    }
}

/// Unified event type for the Ghost event bus
///
/// All inter-component communication flows through these events.
#[derive(Debug, Clone)]
pub enum GhostEvent {
    /// A new pool was detected by Seer
    /// Uses Arc for zero-copy sharing across subscribers
    NewPoolDetected(Arc<DetectedPool>),

    /// A pool transaction was observed (for SnapshotEngine)
    PoolTransaction(Arc<PoolTransaction>),

    /// A funding transfer observation was observed on the bus.
    FundingTransferObserved(Arc<FundingTransferObserved>),

    /// Role-aware evidence for a concrete execution account.
    ExecutionAccountEvidence(Arc<ExecutionAccountEvidenceEvent>),

    /// A pool has been scored by the Oracle
    PoolScored(Arc<PoolScoredEvent>),

    /// A mint has been committed by GatekeeperRegistry
    GatekeeperCommitted {
        /// Pool AMM ID resolved from the canonical pool identity registry
        pool_amm_id: String,
        /// Base mint committed
        base_mint: String,
        /// Number of snapshots committed
        committed_count: usize,
        /// Pending-live merged count
        merged_pending_count: usize,
    },

    /// A transaction was sent to the network
    TransactionSent {
        /// Transaction signature
        signature: String,
        /// Slot when sent
        slot: Option<u64>,
        /// Type of transaction (buy, sell, etc.)
        tx_type: String,
    },

    /// A trade was executed and confirmed
    TradeExecuted(TradeResult),

    /// Raw GeyserEvent::Transaction (for Oracle Runtime gathering)
    /// Contains both real transactions from Geyser and synthetic from Shadow Ledger
    /// Used to extract timestamps, signers, and instruction_data for HyperOracle
    GeyserTransaction {
        /// Pool this transaction relates to
        pool_amm_id: String,
        /// The raw GeyserEvent::Transaction
        geyser_event: seer::types::GeyserEvent,
    },

    /// A live BUY transaction was successfully submitted — enriched event for PostBuyRuntime.
    /// Carries full context so downstream consumers do not need RPC lookups.
    PostBuySubmitted {
        /// Shared trace/correlation identifier for paper lifecycle and shadow compare logs.
        candidate_id: String,
        /// Pool AMM ID
        pool_amm_id: String,
        /// Base token mint
        base_mint: String,
        /// Transaction signature from the BUY transaction
        signature: String,
        /// Trade value in SOL
        amount_sol: f64,
        /// Inline live tip in lamports
        tip_lamports: u64,
        /// Execution lane (paper / live / single)
        lane: String,
        /// Epoch ID (monotonically increasing, set by caller)
        epoch_id: u64,
        /// Reserved runtime slot for active-position bulkhead tracking.
        position_slot_id: Option<PositionSlotId>,
        /// Whether the event comes from the live BUY pipeline or startup recovery.
        source: PostBuySource,
        /// Encoded token parameter from the live BUY request.
        /// - `routed_exact_sol_in`: minimum tokens out
        /// - `legacy_buy`: exact token amount requested
        ///
        /// Populated on live lane; None for paper/dry-run lanes.
        /// Used by the live sell path to arm the Revolver with correct token amount.
        min_tokens_out: Option<u64>,
        /// Canonical token quantity acquired by the entry in raw Pump token units (1e6 scale).
        ///
        /// Populated for shadow handoffs from executable curve simulation and for recovery/live
        /// paths only when the actual held quantity is already known.
        entry_token_amount_raw: Option<u64>,
        /// Confirmed slot where the BUY landed.
        /// Populated on live lane from Sender/Yellowstone confirmation; None for paper/dry-run lanes.
        buy_landed_slot: Option<u64>,
        /// Canonical creator pubkey string used to derive Pump.fun creator_vault for live SELL.
        creator_pubkey: Option<String>,
        /// Optional decision/shadow join metadata for audit-only correlation.
        join_metadata: ExecutionJoinMetadata,
    },

    /// A compare-only shadow buy simulation finished without sending a real TX.
    ShadowBuySimulated(Arc<ShadowBuySimulationEvent>),

    /// Generic custom event for extensibility
    Custom(String, serde_json::Value),

    /// On-chain AccountUpdate for a tracked pool, ready for reconciliation.
    ///
    /// Emitted by the Seer component once `base_mint` has been resolved and
    /// valid bonding-curve reserves have been extracted.
    ///
    /// Consumed by `start_oracle_runtime_task` which calls
    /// `OracleRuntime::process_account_update(...)` to drive corrective
    /// reconciliation. Shadow Ledger remains primary; this is corrective only.
    AccountUpdate(AccountUpdateEvent),
}

impl GhostEvent {
    /// Create a NewPoolDetected event from pool data
    pub fn new_pool_detected(pool: DetectedPool) -> Self {
        GhostEvent::NewPoolDetected(Arc::new(pool))
    }

    /// Create a PoolTransaction event
    pub fn pool_transaction(tx: PoolTransaction) -> Self {
        GhostEvent::PoolTransaction(Arc::new(tx))
    }

    /// Create a FundingTransferObserved event.
    pub fn funding_transfer_observed(transfer: FundingTransferObserved) -> Self {
        GhostEvent::FundingTransferObserved(Arc::new(transfer))
    }

    /// Create an ExecutionAccountEvidence event.
    pub fn execution_account_evidence(
        evidence: ExecutionAccountEvidence,
        detected_at: std::time::SystemTime,
        sequence_number: u64,
    ) -> Self {
        GhostEvent::ExecutionAccountEvidence(Arc::new(ExecutionAccountEvidenceEvent {
            evidence,
            detected_at,
            sequence_number,
        }))
    }

    /// Create a PoolScored event
    pub fn pool_scored(scored: PoolScoredEvent) -> Self {
        GhostEvent::PoolScored(Arc::new(scored))
    }

    /// Create a GatekeeperCommitted event
    pub fn gatekeeper_committed(
        pool_amm_id: impl Into<String>,
        base_mint: impl Into<String>,
        committed_count: usize,
        merged_pending_count: usize,
    ) -> Self {
        GhostEvent::GatekeeperCommitted {
            pool_amm_id: pool_amm_id.into(),
            base_mint: base_mint.into(),
            committed_count,
            merged_pending_count,
        }
    }

    /// Create a TransactionSent event
    pub fn transaction_sent(
        signature: impl Into<String>,
        slot: Option<u64>,
        tx_type: impl Into<String>,
    ) -> Self {
        GhostEvent::TransactionSent {
            signature: signature.into(),
            slot,
            tx_type: tx_type.into(),
        }
    }

    /// Create a TradeExecuted event
    pub fn trade_executed(result: TradeResult) -> Self {
        GhostEvent::TradeExecuted(result)
    }

    /// Create a GeyserTransaction event
    pub fn geyser_transaction(
        pool_amm_id: impl Into<String>,
        geyser_event: seer::types::GeyserEvent,
    ) -> Self {
        GhostEvent::GeyserTransaction {
            pool_amm_id: pool_amm_id.into(),
            geyser_event,
        }
    }

    /// Create a PostBuySubmitted event with full context for PostBuyRuntime.
    pub fn post_buy_submitted(
        pool_amm_id: impl Into<String>,
        base_mint: impl Into<String>,
        signature: impl Into<String>,
        amount_sol: f64,
        tip_lamports: u64,
        lane: impl Into<String>,
        epoch_id: u64,
        position_slot_id: Option<PositionSlotId>,
        source: PostBuySource,
        min_tokens_out: Option<u64>,
        entry_token_amount_raw: Option<u64>,
        buy_landed_slot: Option<u64>,
        creator_pubkey: Option<String>,
    ) -> Self {
        let pool_amm_id = pool_amm_id.into();
        let base_mint = base_mint.into();
        let signature = signature.into();
        GhostEvent::PostBuySubmitted {
            candidate_id: build_execution_candidate_id(&base_mint, &pool_amm_id, &signature),
            pool_amm_id,
            base_mint,
            signature,
            amount_sol,
            tip_lamports,
            lane: lane.into(),
            epoch_id,
            position_slot_id,
            source,
            min_tokens_out,
            entry_token_amount_raw,
            buy_landed_slot,
            creator_pubkey,
            join_metadata: ExecutionJoinMetadata::default(),
        }
    }

    pub fn with_execution_join_metadata(mut self, metadata: ExecutionJoinMetadata) -> Self {
        if let GhostEvent::PostBuySubmitted { join_metadata, .. } = &mut self {
            *join_metadata = metadata;
        }
        self
    }

    pub fn shadow_buy_simulated(event: ShadowBuySimulationEvent) -> Self {
        GhostEvent::ShadowBuySimulated(Arc::new(event))
    }

    /// Create a Custom event
    pub fn custom(event_type: impl Into<String>, data: serde_json::Value) -> Self {
        GhostEvent::Custom(event_type.into(), data)
    }

    pub fn runtime_plane(&self) -> Option<RuntimePlane> {
        match self {
            GhostEvent::PoolScored(_) => Some(RuntimePlane::LegacyObservation),
            GhostEvent::TransactionSent { .. } => Some(RuntimePlane::CanonicalDecision),
            GhostEvent::PostBuySubmitted { .. } => Some(RuntimePlane::PostBuyMonitoring),
            GhostEvent::ShadowBuySimulated(_) => Some(RuntimePlane::ShadowSimulation),
            _ => None,
        }
    }

    /// Get the event type as a string (for logging/metrics)
    pub fn event_type(&self) -> &'static str {
        match self {
            GhostEvent::NewPoolDetected(_) => "new_pool_detected",
            GhostEvent::PoolTransaction(_) => "pool_transaction",
            GhostEvent::FundingTransferObserved(_) => "funding_transfer_observed",
            GhostEvent::ExecutionAccountEvidence(_) => "execution_account_evidence",
            GhostEvent::PoolScored(_) => "pool_scored",
            GhostEvent::GatekeeperCommitted { .. } => "gatekeeper_committed",
            GhostEvent::TransactionSent { .. } => "transaction_sent",
            GhostEvent::PostBuySubmitted { .. } => "post_buy_submitted",
            GhostEvent::ShadowBuySimulated(_) => "shadow_buy_simulated",
            GhostEvent::TradeExecuted(_) => "trade_executed",
            GhostEvent::GeyserTransaction { .. } => "geyser_transaction",
            GhostEvent::Custom(_, _) => "custom",
            GhostEvent::AccountUpdate(_) => "account_update",
        }
    }
}

/// Event bus sender handle
pub type EventBusSender = broadcast::Sender<GhostEvent>;

/// Event bus receiver handle
pub type EventBusReceiver = broadcast::Receiver<GhostEvent>;

/// Create a new event bus with the default buffer size
///
/// Returns a tuple of (sender, receiver). The sender can be cloned
/// to allow multiple producers, and the receiver can be subscribed
/// to create additional consumers.
///
/// # Example
///
/// ```ignore
/// let (tx, rx) = create_event_bus();
///
/// // Clone sender for Seer
/// let seer_tx = tx.clone();
///
/// // Subscribe for Trigger
/// let trigger_rx = tx.subscribe();
/// ```
pub fn create_event_bus() -> (EventBusSender, EventBusReceiver) {
    broadcast::channel(EVENT_BUS_BUFFER_SIZE)
}

/// Create an event bus with a custom buffer size
pub fn create_event_bus_with_capacity(capacity: usize) -> (EventBusSender, EventBusReceiver) {
    broadcast::channel(capacity)
}

pub fn record_event_bus_receivers(sender: &EventBusSender) {
    crate::oracle_metrics::record_eventbus_active_receivers(sender.receiver_count());
}

/// P5: Eventbus backpressure with per-consumer alert threshold.
/// When skipped exceeds the threshold, emit a warning for on-call visibility.
const EVENTBUS_DROP_WARN_THRESHOLD: u64 = 100;
const EVENTBUS_DROP_WARN_WINDOW: Duration = Duration::from_secs(60);

#[derive(Debug, Default)]
struct EventBusLagWindowState {
    events: VecDeque<(Instant, u64)>,
    total_skipped: u64,
    threshold_exceeded: bool,
}

fn event_bus_lag_windows() -> &'static Mutex<HashMap<String, EventBusLagWindowState>> {
    static WINDOWS: OnceLock<Mutex<HashMap<String, EventBusLagWindowState>>> = OnceLock::new();
    WINDOWS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn update_event_bus_lag_window(consumer: &str, skipped: u64, now: Instant) -> Option<u64> {
    let mut windows = event_bus_lag_windows()
        .lock()
        .expect("event bus lag window mutex poisoned");
    let state = windows.entry(consumer.to_string()).or_default();

    while let Some((ts, skipped_count)) = state.events.front().copied() {
        if now.duration_since(ts) > EVENTBUS_DROP_WARN_WINDOW {
            state.events.pop_front();
            state.total_skipped = state.total_skipped.saturating_sub(skipped_count);
        } else {
            break;
        }
    }

    if skipped > 0 {
        state.events.push_back((now, skipped));
        state.total_skipped = state.total_skipped.saturating_add(skipped);
    }

    let threshold_exceeded = state.total_skipped > EVENTBUS_DROP_WARN_THRESHOLD;
    let should_warn = threshold_exceeded && !state.threshold_exceeded;
    state.threshold_exceeded = threshold_exceeded;

    should_warn.then_some(state.total_skipped)
}

pub fn record_event_bus_lag(consumer: &str, skipped: u64) {
    crate::oracle_metrics::record_eventbus_lag(consumer, skipped);
    if let Some(window_skipped) = update_event_bus_lag_window(consumer, skipped, Instant::now()) {
        tracing::warn!(
            consumer = consumer,
            skipped = skipped,
            skipped_last_60s = window_skipped,
            "eventbus_lag_threshold_exceeded"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_join_metadata_legacy_rows_still_parse() {
        let parsed: ExecutionJoinMetadata = serde_json::from_str("{}").unwrap();
        assert!(parsed.is_empty());
    }

    #[test]
    fn execution_join_metadata_probe_fields_roundtrip() {
        let metadata = ExecutionJoinMetadata {
            ab_record_id: Some("probe-ab".to_string()),
            source_ab_record_id: Some("source-ab".to_string()),
            probe_id: Some("probe-id".to_string()),
            dispatch_source: Some("counterfactual_shadow_probe".to_string()),
            collection_plane: Some("counterfactual_shadow_probe".to_string()),
            probe_plane: Some("p37_shadow_probe".to_string()),
            v3_feature_snapshot_hash: Some("feature-hash".to_string()),
            v3_policy_config_hash: Some("policy-hash".to_string()),
            decision_plane: Some("legacy_live".to_string()),
            rollout_namespace: Some("r15-smoke".to_string()),
            run_id: Some("r16-run".to_string()),
            session_id: Some("r16-session".to_string()),
            brain_config_path: Some("configs/rollout/brain-r16.toml".to_string()),
            brain_config_hash: Some("brain-hash".to_string()),
            source_decision_log_path: Some(
                "logs/decisions/gatekeeper_v2_decisions.jsonl".to_string(),
            ),
            source_decision_row_offset: Some(12),
            source_decision_row_sha256: Some("row-sha".to_string()),
            source_v3_feature_snapshot_hash: Some("feature-hash".to_string()),
            source_v3_policy_config_hash: Some("policy-hash".to_string()),
        };

        let json = serde_json::to_string(&metadata).unwrap();
        let parsed: ExecutionJoinMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed, metadata);
        assert!(!parsed.is_empty());
    }

    #[tokio::test]
    async fn test_event_bus_creation() {
        let (tx, _rx) = create_event_bus();
        assert_eq!(tx.receiver_count(), 1);
    }

    #[tokio::test]
    async fn test_new_pool_detected_event() {
        let (tx, mut rx) = create_event_bus();

        let pool = DetectedPool {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: "pool123".to_string(),
            base_mint: "mint123".to_string(),
            quote_mint: "So11111111111111111111111111111111111111112".to_string(),
            amm_program: "pumpfun".to_string(),
            bonding_curve: "curve123".to_string(),
            creator: "creator123".to_string(),
            slot: Some(12345),
            timestamp_ms: 1700000000000,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1700000000123),
            initial_liquidity_sol: Some(10.0),
            signature: "sig123".to_string(),
        };

        tx.send(GhostEvent::new_pool_detected(pool.clone()))
            .unwrap();

        let event = rx.recv().await.unwrap();
        assert_eq!(event.event_type(), "new_pool_detected");

        if let GhostEvent::NewPoolDetected(pool_arc) = event {
            assert_eq!(pool_arc.pool_amm_id, "pool123");
            assert_eq!(pool_arc.slot, Some(12345));
        } else {
            panic!("Expected NewPoolDetected event");
        }
    }

    #[tokio::test]
    async fn test_transaction_sent_event() {
        let (tx, mut rx) = create_event_bus();

        tx.send(GhostEvent::transaction_sent("sig456", Some(67890), "buy"))
            .unwrap();

        let event = rx.recv().await.unwrap();
        assert_eq!(event.event_type(), "transaction_sent");

        if let GhostEvent::TransactionSent {
            signature,
            slot,
            tx_type,
        } = event
        {
            assert_eq!(signature, "sig456");
            assert_eq!(slot, Some(67890));
            assert_eq!(tx_type, "buy");
        } else {
            panic!("Expected TransactionSent event");
        }
    }

    #[tokio::test]
    async fn test_funding_transfer_observed_event() {
        let (tx, mut rx) = create_event_bus();

        let transfer = FundingTransferObserved {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(42),
            event_ordinal: Some(5),
            outer_instruction_index: Some(1),
            inner_group_index: Some(1),
            cpi_stack_height: Some(2),
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 1_700_000_000_123,
            signature: "funding-sig".to_string(),
            source_wallet: "source-wallet".to_string(),
            recipient_wallet: "recipient-wallet".to_string(),
            lamports: 25_000_000,
            full_chain_coverage: true,
            provenance: FundingTransferProvenance::authoritative_full_feed_live(),
            detected_at: std::time::SystemTime::now(),
            sequence_number: 9,
        };

        tx.send(GhostEvent::funding_transfer_observed(transfer.clone()))
            .unwrap();

        let event = rx.recv().await.unwrap();
        assert_eq!(event.event_type(), "funding_transfer_observed");

        if let GhostEvent::FundingTransferObserved(observed) = event {
            assert_eq!(observed.signature, transfer.signature);
            assert_eq!(observed.source_wallet, transfer.source_wallet);
            assert_eq!(observed.recipient_wallet, transfer.recipient_wallet);
            assert_eq!(observed.lamports, transfer.lamports);
            assert_eq!(observed.full_chain_coverage, transfer.full_chain_coverage);
            assert_eq!(observed.provenance, transfer.provenance);
            assert_eq!(observed.arrival_ts_ms, transfer.arrival_ts_ms);
            assert_eq!(observed.event_ordinal, transfer.event_ordinal);
            assert_eq!(
                observed.outer_instruction_index,
                transfer.outer_instruction_index
            );
            assert_eq!(observed.inner_group_index, transfer.inner_group_index);
            assert_eq!(observed.cpi_stack_height, transfer.cpi_stack_height);
            assert_eq!(observed.sequence_number, 9);
        } else {
            panic!("Expected FundingTransferObserved event");
        }
    }

    #[tokio::test]
    async fn test_execution_account_evidence_event() {
        let (tx, mut rx) = create_event_bus();
        let evidence = ExecutionAccountEvidence {
            role: ghost_core::ExecutionAccountRole::BondingCurveV2,
            account_pubkey: solana_sdk::pubkey::Pubkey::new_unique(),
            base_mint: Some(solana_sdk::pubkey::Pubkey::new_unique()),
            pool_id: Some(solana_sdk::pubkey::Pubkey::new_unique()),
            canonical_bonding_curve: Some(solana_sdk::pubkey::Pubkey::new_unique()),
            source: ghost_core::ExecutionAccountEvidenceSource::RpcHydration,
            status: ghost_core::ExecutionAccountEvidenceStatus::RpcReady,
            slot: Some(42),
            context_slot: Some(43),
            write_version: Some(2),
            owner: Some(solana_sdk::pubkey::Pubkey::new_unique()),
            data_len: Some(256),
            tx_signature: Some("evidence-sig".to_string()),
            observed_instruction_index: Some(1),
            observed_account_position: Some(7),
            provenance_status: Some("route_compatible".to_string()),
            detected_at_ms: 1_700_000_000_000,
            received_at_ms: 1_700_000_000_050,
            evidence_ready: true,
            reason: None,
        };
        let detected_at = std::time::SystemTime::now();

        tx.send(GhostEvent::execution_account_evidence(
            evidence.clone(),
            detected_at,
            17,
        ))
        .unwrap();

        let event = rx.recv().await.unwrap();
        assert_eq!(event.event_type(), "execution_account_evidence");

        if let GhostEvent::ExecutionAccountEvidence(observed) = event {
            assert_eq!(observed.evidence, evidence);
            assert_eq!(observed.detected_at, detected_at);
            assert_eq!(observed.sequence_number, 17);
        } else {
            panic!("Expected ExecutionAccountEvidence event");
        }
    }

    #[test]
    fn account_update_event_schema_remains_canonical_reserve_update() {
        let update = AccountUpdateEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            event_time: ghost_core::EventTimeMetadata::default(),
            base_mint: solana_sdk::pubkey::Pubkey::new_unique(),
            bonding_curve: solana_sdk::pubkey::Pubkey::new_unique(),
            curve_finality: CurveFinality::Provisional,
            sol_reserves: 10,
            token_reserves: 20,
            complete: 0,
            slot: 42,
            write_version: Some(7),
            replay_origin: AccountUpdateReplayOrigin::Live,
            replay_buffer_dwell_ms: None,
            detected_at: std::time::SystemTime::now(),
            sequence_number: 3,
        };

        let serialized = serde_json::to_value(update).expect("serialize account update");
        let object = serialized
            .as_object()
            .expect("account update must serialize as object");

        assert!(object.contains_key("base_mint"));
        assert!(object.contains_key("bonding_curve"));
        assert!(!object.contains_key("evidence"));
        assert!(!object.contains_key("account_pubkey"));
        assert!(!object.contains_key("role"));
        assert!(!object.contains_key("bonding_curve_v2"));
    }

    #[test]
    fn funding_transfer_observed_default_filtered_serialization_omits_provenance() {
        let transfer = FundingTransferObserved {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(42),
            event_ordinal: None,
            outer_instruction_index: None,
            inner_group_index: None,
            cpi_stack_height: None,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 42_000,
            signature: "sig".to_string(),
            source_wallet: "source".to_string(),
            recipient_wallet: "recipient".to_string(),
            lamports: 1_000,
            full_chain_coverage: false,
            provenance: FundingTransferProvenance::filtered_grpc_global_stream_live(),
            detected_at: std::time::SystemTime::now(),
            sequence_number: 1,
        };

        let serialized =
            serde_json::to_value(&transfer).expect("serialize funding transfer observed");
        let object = serialized
            .as_object()
            .expect("funding transfer observed must serialize as object");
        assert!(
            !object.contains_key("provenance"),
            "default filtered provenance should stay omitted for legacy JSON shape"
        );
    }

    #[test]
    fn funding_transfer_observed_legacy_fixture_deserializes_with_filtered_defaults() {
        let mut serialized = serde_json::to_value(FundingTransferObserved {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(42),
            event_ordinal: None,
            outer_instruction_index: None,
            inner_group_index: None,
            cpi_stack_height: None,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 42_000,
            signature: "sig".to_string(),
            source_wallet: "source".to_string(),
            recipient_wallet: "recipient".to_string(),
            lamports: 1_000,
            full_chain_coverage: false,
            provenance: FundingTransferProvenance::filtered_grpc_global_stream_live(),
            detected_at: std::time::SystemTime::now(),
            sequence_number: 1,
        })
        .expect("serialize funding transfer observed");

        let object = serialized
            .as_object_mut()
            .expect("funding transfer observed must serialize as object");
        object.remove("provenance");

        let deserialized: FundingTransferObserved =
            serde_json::from_value(serialized).expect("legacy fixture should deserialize");
        assert!(!deserialized.full_chain_coverage);
        assert_eq!(
            deserialized.provenance,
            FundingTransferProvenance::filtered_grpc_global_stream_live()
        );
    }

    #[tokio::test]
    async fn test_trade_executed_event() {
        let (tx, mut rx) = create_event_bus();

        let result = TradeResult {
            signature: "sig789".to_string(),
            mint: "mint789".to_string(),
            sol_amount: 1.5,
            token_amount: 1000000.0,
            entry_price: 0.0000015,
            is_buy: true,
            slot: Some(99999),
            pnl_sol: None,
            timestamp: 1700000001,
        };

        tx.send(GhostEvent::trade_executed(result)).unwrap();

        let event = rx.recv().await.unwrap();
        assert_eq!(event.event_type(), "trade_executed");

        if let GhostEvent::TradeExecuted(trade) = event {
            assert_eq!(trade.signature, "sig789");
            assert!(trade.is_buy);
            assert_eq!(trade.sol_amount, 1.5);
        } else {
            panic!("Expected TradeExecuted event");
        }
    }

    #[tokio::test]
    async fn test_custom_event() {
        let (tx, mut rx) = create_event_bus();

        let data = serde_json::json!({
            "key": "value",
            "count": 42
        });

        tx.send(GhostEvent::custom("my_custom_event", data.clone()))
            .unwrap();

        let event = rx.recv().await.unwrap();
        assert_eq!(event.event_type(), "custom");

        if let GhostEvent::Custom(event_type, payload) = event {
            assert_eq!(event_type, "my_custom_event");
            assert_eq!(payload["count"], 42);
        } else {
            panic!("Expected Custom event");
        }
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let (tx, _rx) = create_event_bus();
        let mut rx1 = tx.subscribe();
        let mut rx2 = tx.subscribe();

        assert_eq!(tx.receiver_count(), 3); // Original + 2 subscribers

        tx.send(GhostEvent::transaction_sent("test", Some(1), "buy"))
            .unwrap();

        // Both receivers should get the event
        let event1 = rx1.recv().await.unwrap();
        let event2 = rx2.recv().await.unwrap();

        assert_eq!(event1.event_type(), "transaction_sent");
        assert_eq!(event2.event_type(), "transaction_sent");
    }

    #[test]
    fn test_runtime_plane_classification_for_execution_events() {
        let pool_scored = GhostEvent::pool_scored(PoolScoredEvent {
            pool_amm_id: "pool".to_string(),
            base_mint: "mint".to_string(),
            score: 91.0,
            passed: true,
            risk_level: "low".to_string(),
            interpretation: "legacy".to_string(),
            processing_time_us: 10,
            component_scores: serde_json::json!({}),
        });
        assert_eq!(
            pool_scored.runtime_plane(),
            Some(RuntimePlane::LegacyObservation)
        );

        let tx_sent = GhostEvent::transaction_sent("sig", Some(1), "buy");
        assert_eq!(
            tx_sent.runtime_plane(),
            Some(RuntimePlane::CanonicalDecision)
        );

        let post_buy = GhostEvent::post_buy_submitted(
            "pool",
            "mint",
            "sig",
            1.0,
            42,
            "live",
            1,
            None,
            PostBuySource::LiveBuy,
            None,
            None,
            None,
            None,
        );
        assert_eq!(
            post_buy.runtime_plane(),
            Some(RuntimePlane::PostBuyMonitoring)
        );
        if let GhostEvent::PostBuySubmitted { candidate_id, .. } = &post_buy {
            assert_eq!(candidate_id, "mint_pool_sig");
        } else {
            panic!("expected PostBuySubmitted");
        }

        let shadow = GhostEvent::shadow_buy_simulated(ShadowBuySimulationEvent {
            join_metadata: ExecutionJoinMetadata::default(),
            account_diagnostics: ShadowSimulationAccountDiagnostics::default(),
            candidate_id: build_execution_candidate_id("mint", "pool", "1000"),
            pool_amm_id: "pool".to_string(),
            base_mint: "mint".to_string(),
            mint: "mint".to_string(),
            live_signature: None,
            payer_pubkey: "payer".to_string(),
            payer_provenance: "configured".to_string(),
            amount_lamports: 1,
            entry_token_amount_raw: Some(1_000_000),
            tip_lamports: 1,
            decision_ts_ms: 1,
            simulation_started_ts_ms: 1,
            simulation_finished_ts_ms: 2,
            latency_ms: 1,
            shadow_duration_ms: 1,
            rpc_slot: 1,
            retry_count: 0,
            used_sig_verify: false,
            used_replace_recent_blockhash: true,
            units_consumed: None,
            logs: vec![],
            return_data: None,
            err: None,
            error_class: None,
            error_code: None,
            error_detail_class: None,
        });
        assert_eq!(shadow.runtime_plane(), Some(RuntimePlane::ShadowSimulation));
    }

    #[test]
    fn test_build_execution_candidate_id() {
        assert_eq!(
            build_execution_candidate_id("mint", "pool", "sig"),
            "mint_pool_sig"
        );
    }

    #[test]
    fn test_post_buy_and_shadow_share_candidate_id_contract() {
        let post_buy = GhostEvent::post_buy_submitted(
            "pool",
            "mint",
            "sig",
            1.0,
            42,
            "live",
            1,
            None,
            PostBuySource::LiveBuy,
            None,
            None,
            None,
            None,
        );
        let shadow_event = ShadowBuySimulationEvent {
            join_metadata: ExecutionJoinMetadata::default(),
            account_diagnostics: ShadowSimulationAccountDiagnostics::default(),
            candidate_id: build_execution_candidate_id("mint", "pool", "sig"),
            pool_amm_id: "pool".to_string(),
            base_mint: "mint".to_string(),
            mint: "mint".to_string(),
            live_signature: Some("sig".to_string()),
            payer_pubkey: "payer".to_string(),
            payer_provenance: "configured".to_string(),
            amount_lamports: 1,
            entry_token_amount_raw: Some(1_000_000),
            tip_lamports: 1,
            decision_ts_ms: 1,
            simulation_started_ts_ms: 1,
            simulation_finished_ts_ms: 2,
            latency_ms: 1,
            shadow_duration_ms: 1,
            rpc_slot: 1,
            retry_count: 0,
            used_sig_verify: false,
            used_replace_recent_blockhash: true,
            units_consumed: None,
            logs: vec![],
            return_data: None,
            err: None,
            error_class: None,
            error_code: None,
            error_detail_class: None,
        };
        let shadow_record =
            crate::components::trigger::shadow_run::ShadowBuySimulationRecord::from_event(
                crate::config::TriggerEntryMode::LiveAndShadow,
                &shadow_event,
            );

        if let GhostEvent::PostBuySubmitted { candidate_id, .. } = post_buy {
            assert_eq!(candidate_id, shadow_record.candidate_id);
        } else {
            panic!("expected PostBuySubmitted");
        }
    }

    #[test]
    fn test_legacy_path_descriptor_contract() {
        let descriptor = LegacyPathDescriptor::new(
            "trigger_pool_scored_observer",
            LegacyPathClassification::ObservabilityOnly,
            RuntimePlane::LegacyObservation,
            false,
            Some("2026-04-30"),
        );

        assert_eq!(descriptor.path, "trigger_pool_scored_observer");
        assert_eq!(
            descriptor.classification,
            LegacyPathClassification::ObservabilityOnly
        );
        assert_eq!(descriptor.runtime_plane, RuntimePlane::LegacyObservation);
        assert!(!descriptor.allows_authoritative_buy);
        assert_eq!(descriptor.removal_date, Some("2026-04-30"));
    }

    #[test]
    fn test_detected_pool_serialize() {
        let pool = DetectedPool {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: "pool".to_string(),
            base_mint: "mint".to_string(),
            quote_mint: "sol".to_string(),
            amm_program: "pumpfun".to_string(),
            bonding_curve: "curve".to_string(),
            creator: "creator".to_string(),
            slot: Some(100),
            timestamp_ms: 1700000000000,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1700000000123),
            initial_liquidity_sol: Some(5.0),
            signature: "sig".to_string(),
        };

        let json = serde_json::to_string(&pool).unwrap();
        assert!(json.contains("pool_amm_id"));
        assert!(json.contains("pumpfun"));
    }

    #[test]
    fn test_trade_result_serialize() {
        let result = TradeResult {
            signature: "sig".to_string(),
            mint: "mint".to_string(),
            sol_amount: 2.0,
            token_amount: 500000.0,
            entry_price: 0.000004,
            is_buy: false,
            slot: Some(200),
            pnl_sol: Some(0.5),
            timestamp: 12346,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("signature"));
        assert!(json.contains("pnl_sol"));
    }

    #[test]
    fn test_event_bus_lag_uses_rolling_60s_window() {
        let base = Instant::now();
        assert_eq!(update_event_bus_lag_window("oracle", 60, base), None);
        assert_eq!(
            update_event_bus_lag_window("oracle", 41, base + Duration::from_secs(10)),
            Some(101)
        );
        assert_eq!(
            update_event_bus_lag_window("oracle", 1, base + Duration::from_secs(20)),
            None
        );
        assert_eq!(
            update_event_bus_lag_window("oracle", 50, base + Duration::from_secs(71)),
            None
        );
        assert_eq!(
            update_event_bus_lag_window("oracle", 60, base + Duration::from_secs(72)),
            Some(111)
        );
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PostBuySource {
    LiveBuy,
    Recovery,
    CounterfactualShadowProbe,
}
