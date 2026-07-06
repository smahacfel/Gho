//! Shadow V2 L1 deterministic execution simulation engine.
//!
//! This module is decision-inert. It wraps the canonical formula source from
//! `ghost-core::shadow_v2_price` and separates deterministic simulation
//! readiness from research-grade provenance readiness.

use ghost_core::{
    quote_constant_product, ShadowV2PoolPhase, ShadowV2PriceError, ShadowV2Quote,
    ShadowV2QuoteSide, ShadowV2Reserves, SHADOW_V2_BPS_DENOMINATOR,
    SHADOW_V2_PRICE_FORMULA_VERSION,
};
use serde::{Deserialize, Serialize};

use super::shadow_v2::{
    chain_order_tuple_for_execution, EventOrderKey, FillStatus, PoolStateSampleV2, PoolStateSource,
    TemporalClass,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ShadowV2ExecutionSide {
    Buy,
    Sell,
}

impl ShadowV2ExecutionSide {
    pub const fn quote_side(self) -> ShadowV2QuoteSide {
        match self {
            Self::Buy => ShadowV2QuoteSide::Buy,
            Self::Sell => ShadowV2QuoteSide::Sell,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Buy => "BUY",
            Self::Sell => "SELL",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ShadowV2BoundaryKind {
    EntryBefore,
    EntryAfterDerived,
    ExitBefore,
    ExitAfterDerived,
}

impl ShadowV2BoundaryKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::EntryBefore => "ENTRY_BEFORE",
            Self::EntryAfterDerived => "ENTRY_AFTER_DERIVED",
            Self::ExitBefore => "EXIT_BEFORE",
            Self::ExitAfterDerived => "EXIT_AFTER_DERIVED",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ShadowV2ExecutionLabelGrade {
    DiagnosticSim,
    ResearchCandidate,
    LiveConfirmed,
}

impl ShadowV2ExecutionLabelGrade {
    pub const fn label(self) -> &'static str {
        match self {
            Self::DiagnosticSim => "DIAGNOSTIC_SIM",
            Self::ResearchCandidate => "RESEARCH_CANDIDATE",
            Self::LiveConfirmed => "LIVE_CONFIRMED",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ShadowV2NoFillReason {
    MinOutNotMet,
    ZeroOutput,
    InsufficientReserves,
    PoolCompleteOrMigrated,
    UnsupportedPoolPhase,
    StalePoolState,
    OrderingAmbiguity,
    TokenAmountMissingForSell,
}

impl ShadowV2NoFillReason {
    pub const fn label(self) -> &'static str {
        match self {
            Self::MinOutNotMet => "NO_FILL_MIN_OUT_NOT_MET",
            Self::ZeroOutput => "NO_FILL_ZERO_OUTPUT",
            Self::InsufficientReserves => "NO_FILL_INSUFFICIENT_RESERVES",
            Self::PoolCompleteOrMigrated => "NO_FILL_POOL_COMPLETE_OR_MIGRATED",
            Self::UnsupportedPoolPhase => "NO_FILL_UNSUPPORTED_POOL_PHASE",
            Self::StalePoolState => "NO_FILL_STALE_POOL_STATE",
            Self::OrderingAmbiguity => "NO_FILL_ORDERING_AMBIGUITY",
            Self::TokenAmountMissingForSell => "NO_FILL_TOKEN_AMOUNT_MISSING_FOR_SELL",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ShadowV2BlockedReason {
    PoolStateMissing,
    PoolStateIncomplete,
    PoolStateHashMissing,
    PoolStateStalenessUnknownOrReversed,
    OrderingAmbiguity,
    TokenAmountMissing,
    FeeModelMissing,
    FormulaUnsupported,
    UnsupportedPoolPhase,
    MissingTokenDecimals,
    MissingLamportsNormalization,
}

impl ShadowV2BlockedReason {
    pub const fn label(self) -> &'static str {
        match self {
            Self::PoolStateMissing => "BLOCKED_POOL_STATE_MISSING",
            Self::PoolStateIncomplete => "BLOCKED_POOL_STATE_INCOMPLETE",
            Self::PoolStateHashMissing => "POOL_STATE_ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME",
            Self::PoolStateStalenessUnknownOrReversed => "POOL_STATE_STALENESS_UNKNOWN_OR_REVERSED",
            Self::OrderingAmbiguity => "BLOCKED_ORDERING_AMBIGUITY",
            Self::TokenAmountMissing => "BLOCKED_TOKEN_AMOUNT_MISSING",
            Self::FeeModelMissing => "BLOCKED_FEE_MODEL_MISSING",
            Self::FormulaUnsupported => "BLOCKED_FORMULA_UNSUPPORTED",
            Self::UnsupportedPoolPhase => "BLOCKED_UNSUPPORTED_POOL_PHASE",
            Self::MissingTokenDecimals => "TOKEN_DECIMALS_MISSING",
            Self::MissingLamportsNormalization => "SOL_LAMPORTS_NORMALIZATION_MISSING",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowV2DerivedPoolState {
    pub source: ShadowV2BoundaryKind,
    pub source_pool_state_event_id: String,
    pub source_formula_version: String,
    pub post_sol_reserves_lamports: u64,
    pub post_token_reserves_raw: u64,
    pub token_decimals: u8,
    pub sol_lamports: u64,
}

impl ShadowV2DerivedPoolState {
    pub fn ref_label(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}",
            self.source.label(),
            self.source_pool_state_event_id,
            self.post_sol_reserves_lamports,
            self.post_token_reserves_raw,
            self.source_formula_version
        )
    }
}

#[derive(Debug, Clone)]
pub struct ShadowV2ExecutionInput<'a> {
    pub side: ShadowV2ExecutionSide,
    pub pool_phase: ShadowV2PoolPhase,
    pub pool_state_before: Option<&'a PoolStateSampleV2>,
    pub boundary_kind: ShadowV2BoundaryKind,
    pub event_order_key: EventOrderKey,
    pub input_amount_raw: Option<u64>,
    pub min_out_raw: Option<u64>,
    pub fee_bps: Option<u16>,
    pub slippage_tolerance_bps: Option<u16>,
    pub model_version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowV2ExecutionOutcome {
    pub side: ShadowV2ExecutionSide,
    pub fill_status: FillStatus,
    pub execution_simulation_ready: bool,
    pub research_provenance_ready: bool,
    pub execution_label_grade: ShadowV2ExecutionLabelGrade,
    pub provenance_ready: bool,
    pub provenance_blockers: Vec<String>,
    pub blocked_reasons: Vec<String>,
    pub no_fill_reason: Option<ShadowV2NoFillReason>,
    pub fail_reason: Option<String>,
    pub fill_price: Option<f64>,
    pub fill_price_source: Option<String>,
    pub fill_amount_sol: Option<f64>,
    pub fill_amount_tokens: Option<f64>,
    pub output_amount_raw: Option<u64>,
    pub expected_output_raw: Option<u64>,
    pub min_out_raw: Option<u64>,
    pub slippage_tolerance_bps: Option<i32>,
    pub deterministic_price_impact_bps: Option<i32>,
    pub realized_slippage_bps: Option<i32>,
    pub quote_fill_divergence_bps: Option<i32>,
    pub own_impact_bps: Option<i32>,
    pub fee_bps: Option<i32>,
    pub fee_amount_raw: Option<u64>,
    pub pool_state_before_ref: Option<String>,
    pub pool_state_after_derived: Option<ShadowV2DerivedPoolState>,
    pub pool_state_after_source: Option<String>,
    pub reconstruction_status: String,
    pub quality: String,
    pub limitations: Vec<String>,
    pub model_version: String,
}

pub struct ShadowV2FillEngine;

impl ShadowV2FillEngine {
    pub fn simulate(input: ShadowV2ExecutionInput<'_>) -> ShadowV2ExecutionOutcome {
        let mut context = ShadowV2ExecutionContext::from_input(input);
        context.simulate()
    }
}

struct ShadowV2ExecutionContext<'a> {
    input: ShadowV2ExecutionInput<'a>,
    provenance_blockers: Vec<String>,
    blocked_reasons: Vec<String>,
}

impl<'a> ShadowV2ExecutionContext<'a> {
    fn from_input(input: ShadowV2ExecutionInput<'a>) -> Self {
        Self {
            input,
            provenance_blockers: Vec::new(),
            blocked_reasons: Vec::new(),
        }
    }

    fn simulate(&mut self) -> ShadowV2ExecutionOutcome {
        let pool_state = match self.input.pool_state_before {
            Some(pool_state) => pool_state,
            None => {
                self.block(ShadowV2BlockedReason::PoolStateMissing);
                return self.blocked_outcome();
            }
        };
        self.collect_provenance_blockers(pool_state);
        self.collect_execution_blockers(pool_state);
        if !self.blocked_reasons.is_empty() {
            return self.blocked_outcome();
        }

        let amount = self.input.input_amount_raw.unwrap_or_default();
        let fee_bps = self.input.fee_bps.unwrap_or_default();
        let slippage_bps = self.input.slippage_tolerance_bps.unwrap_or_default();
        let reserves = match self.reserves_from_pool_state(pool_state) {
            Some(reserves) => reserves,
            None => {
                self.block(ShadowV2BlockedReason::PoolStateIncomplete);
                return self.blocked_outcome();
            }
        };

        match quote_constant_product(
            self.input.pool_phase,
            self.input.side.quote_side(),
            reserves,
            amount,
            fee_bps,
            slippage_bps,
        ) {
            Ok(quote) => self.outcome_from_quote(pool_state, reserves, quote),
            Err(ShadowV2PriceError::OutputWouldBeZero) => {
                self.no_fill_outcome(ShadowV2NoFillReason::ZeroOutput, None, None)
            }
            Err(ShadowV2PriceError::MissingOrZeroSolReserves)
            | Err(ShadowV2PriceError::MissingOrZeroTokenReserves) => {
                self.no_fill_outcome(ShadowV2NoFillReason::InsufficientReserves, None, None)
            }
            Err(error) => {
                self.block(ShadowV2BlockedReason::FormulaUnsupported);
                self.blocked_reasons.push(format!("FORMULA_ERROR={error}"));
                self.blocked_outcome()
            }
        }
    }

    fn collect_execution_blockers(&mut self, pool_state: &PoolStateSampleV2) {
        if self.input.input_amount_raw.unwrap_or_default() == 0 {
            match self.input.side {
                ShadowV2ExecutionSide::Buy => self.block(ShadowV2BlockedReason::TokenAmountMissing),
                ShadowV2ExecutionSide::Sell => {
                    self.no_fill_blocker(ShadowV2NoFillReason::TokenAmountMissingForSell)
                }
            }
        }
        if self.input.fee_bps.is_none() || self.input.slippage_tolerance_bps.is_none() {
            self.block(ShadowV2BlockedReason::FeeModelMissing);
        }
        if self
            .input
            .fee_bps
            .is_some_and(|fee_bps| fee_bps as u64 > SHADOW_V2_BPS_DENOMINATOR)
            || self
                .input
                .slippage_tolerance_bps
                .is_some_and(|slippage_bps| slippage_bps as u64 > SHADOW_V2_BPS_DENOMINATOR)
        {
            self.block(ShadowV2BlockedReason::FeeModelMissing);
        }
        if pool_state.token_decimals.is_none() {
            self.block(ShadowV2BlockedReason::MissingTokenDecimals);
        }
        if pool_state.sol_lamports.is_none() {
            self.block(ShadowV2BlockedReason::MissingLamportsNormalization);
        }
        if !self.has_reserve_pair_for_phase(pool_state) {
            self.block(ShadowV2BlockedReason::PoolStateIncomplete);
        }
        if !self.temporal_class_allowed(pool_state.envelope.temporal_class) {
            self.block(ShadowV2BlockedReason::OrderingAmbiguity);
            self.blocked_reasons.push(format!(
                "{}_POOL_STATE_TEMPORAL_CLASS_NOT_ALLOWED={:?}",
                self.input.side.label(),
                pool_state.envelope.temporal_class
            ));
        }
        for blocker in self.ordering_blockers(pool_state) {
            self.blocked_reasons.push(blocker);
            self.block(ShadowV2BlockedReason::OrderingAmbiguity);
        }
    }

    fn collect_provenance_blockers(&mut self, pool_state: &PoolStateSampleV2) {
        for blocker in pool_state.research_blockers() {
            self.provenance_label(normalize_pool_state_research_blocker(&blocker));
        }
        let account_state_source_proof = has_account_state_source_proof(pool_state);
        let simulated_fill_boundary_proof =
            self.has_account_state_boundary_proof_for_simulated_fill(pool_state);
        if pool_state
            .event_order_key
            .has_explicit_unknown_chain_order()
            && !account_state_source_proof
        {
            self.provenance_label(ShadowV2BlockedReason::OrderingAmbiguity.label().to_string());
            self.provenance_blockers.extend(
                pool_state
                    .event_order_key
                    .ambiguity_labels()
                    .into_iter()
                    .map(|label| format!("POOL_STATE_{label}")),
            );
        }
        if self
            .input
            .event_order_key
            .has_explicit_unknown_chain_order()
            && !simulated_fill_boundary_proof
        {
            self.provenance_label(ShadowV2BlockedReason::OrderingAmbiguity.label().to_string());
            self.provenance_blockers.extend(
                self.input
                    .event_order_key
                    .ambiguity_labels()
                    .into_iter()
                    .map(|label| format!("FILL_EVENT_{label}")),
            );
        }
        if self.pool_state_same_slot_ambiguous_with_fill(pool_state)
            && !simulated_fill_boundary_proof
        {
            self.provenance_label(format!(
                "{}_POOL_STATE_SAME_SLOT_ORDER_AMBIGUOUS",
                self.fill_prefix()
            ));
        }
    }

    fn ordering_blockers(&self, pool_state: &PoolStateSampleV2) -> Vec<String> {
        let mut blockers = Vec::new();
        let pool_order = &pool_state.event_order_key;
        let fill_order = &self.input.event_order_key;
        let prefix = self.fill_prefix();
        if fill_order.observed_at_wall_ms == 0 {
            blockers.push(format!("{prefix}_EVENT_ORDER_OBSERVED_AT_WALL_MS_MISSING"));
        }
        if fill_order.slot.is_unknown() || pool_order.slot.is_unknown() {
            blockers.push(format!("{prefix}_EVENT_ORDER_SLOT_UNKNOWN"));
        }
        if pool_order.event_seq_in_process >= fill_order.event_seq_in_process {
            blockers.push(format!(
                "{prefix}_POOL_STATE_NOT_STRICTLY_BEFORE_FILL_BOUNDARY"
            ));
        }
        match (pool_order.slot.as_known(), fill_order.slot.as_known()) {
            (Some(pool_slot), Some(fill_slot)) if pool_slot > fill_slot => {
                blockers.push(format!("{prefix}_POOL_STATE_AFTER_FILL_BOUNDARY"));
            }
            (Some(pool_slot), Some(fill_slot)) if pool_slot == fill_slot => {
                if !pool_order.same_slot_ambiguous_with(fill_order) {
                    if let (Some(pool_tuple), Some(fill_tuple)) = (
                        chain_order_tuple_for_execution(pool_order),
                        chain_order_tuple_for_execution(fill_order),
                    ) {
                        if pool_tuple >= fill_tuple {
                            blockers.push(format!("{prefix}_POOL_STATE_AFTER_FILL_BOUNDARY"));
                        }
                    }
                }
            }
            _ => {}
        }
        blockers
    }

    fn pool_state_same_slot_ambiguous_with_fill(&self, pool_state: &PoolStateSampleV2) -> bool {
        pool_state
            .event_order_key
            .same_slot_ambiguous_with(&self.input.event_order_key)
    }

    fn has_account_state_boundary_proof_for_simulated_fill(
        &self,
        pool_state: &PoolStateSampleV2,
    ) -> bool {
        matches!(
            self.input.boundary_kind,
            ShadowV2BoundaryKind::EntryBefore | ShadowV2BoundaryKind::ExitBefore
        ) && has_account_state_source_proof(pool_state)
            && pool_state.event_order_key.slot.as_known().is_some()
            && self.input.event_order_key.slot.as_known().is_some()
            && pool_state.event_order_key.event_seq_in_process
                < self.input.event_order_key.event_seq_in_process
    }

    fn fill_prefix(&self) -> &'static str {
        match self.input.side {
            ShadowV2ExecutionSide::Buy => "ENTRY_FILL",
            ShadowV2ExecutionSide::Sell => "EXIT_FILL",
        }
    }

    fn temporal_class_allowed(&self, temporal_class: TemporalClass) -> bool {
        match self.input.side {
            ShadowV2ExecutionSide::Buy => matches!(
                temporal_class,
                TemporalClass::PreDecision | TemporalClass::AtDecision | TemporalClass::PostEntry
            ),
            ShadowV2ExecutionSide::Sell => temporal_class == TemporalClass::PostEntry,
        }
    }

    fn has_reserve_pair_for_phase(&self, pool_state: &PoolStateSampleV2) -> bool {
        match self.input.pool_phase {
            ShadowV2PoolPhase::BondingCurve => matches!(
                (pool_state.virtual_sol_reserves, pool_state.virtual_token_reserves),
                (Some(sol), Some(tokens)) if sol > 0 && tokens > 0
            ),
            ShadowV2PoolPhase::Amm => matches!(
                (pool_state.real_sol_reserves, pool_state.real_token_reserves),
                (Some(sol), Some(tokens)) if sol > 0 && tokens > 0
            ),
        }
    }

    fn reserves_from_pool_state(&self, pool_state: &PoolStateSampleV2) -> Option<ShadowV2Reserves> {
        let (sol_reserves, token_reserves) = match self.input.pool_phase {
            ShadowV2PoolPhase::BondingCurve => (
                pool_state.virtual_sol_reserves?,
                pool_state.virtual_token_reserves?,
            ),
            ShadowV2PoolPhase::Amm => (
                pool_state.real_sol_reserves?,
                pool_state.real_token_reserves?,
            ),
        };
        Some(ShadowV2Reserves::new(
            sol_reserves,
            token_reserves,
            pool_state.token_decimals?,
            pool_state.sol_lamports?,
        ))
    }

    fn outcome_from_quote(
        &mut self,
        pool_state: &PoolStateSampleV2,
        reserves: ShadowV2Reserves,
        quote: ShadowV2Quote,
    ) -> ShadowV2ExecutionOutcome {
        let effective_min_out = self.input.min_out_raw.unwrap_or(quote.min_output_amount);
        if quote.expected_output_amount < effective_min_out {
            return self.no_fill_outcome(
                ShadowV2NoFillReason::MinOutNotMet,
                Some(quote.expected_output_amount),
                Some(effective_min_out),
            );
        }
        let fill_amount_sol = match self.input.side {
            ShadowV2ExecutionSide::Buy => {
                Some(quote.input_amount as f64 / reserves.sol_lamports as f64)
            }
            ShadowV2ExecutionSide::Sell => {
                Some(quote.expected_output_amount as f64 / reserves.sol_lamports as f64)
            }
        };
        let fill_amount_tokens = match self.input.side {
            ShadowV2ExecutionSide::Buy => Some(normalized_token_amount(
                quote.expected_output_amount,
                reserves.token_decimals,
            )),
            ShadowV2ExecutionSide::Sell => Some(normalized_token_amount(
                quote.input_amount,
                reserves.token_decimals,
            )),
        };
        let research_ready = self.provenance_blockers.is_empty();
        let grade = if research_ready {
            ShadowV2ExecutionLabelGrade::ResearchCandidate
        } else {
            ShadowV2ExecutionLabelGrade::DiagnosticSim
        };
        let quality = if research_ready {
            format!(
                "L1_{}_EXECUTION_SIM_RESEARCH_CANDIDATE",
                self.input.side.label()
            )
        } else {
            format!("L1_{}_EXECUTION_SIM_DIAGNOSTIC", self.input.side.label())
        };
        let mut limitations = vec![
            format!("L1_EXECUTION_LABEL_GRADE={}", grade.label()),
            format!(
                "EXECUTION_BOUNDARY_KIND={}",
                self.input.boundary_kind.label()
            ),
            "FILL_MODEL_STATIC_NOT_LIVE_CONFIRMED".to_string(),
            "SLIPPAGE_TOLERANCE_BPS_IS_CONFIGURED_NOT_REALIZED".to_string(),
            "REALIZED_SLIPPAGE_BPS_UNAVAILABLE_IN_L1".to_string(),
            "QUOTE_FILL_DIVERGENCE_UNAVAILABLE_IN_L1".to_string(),
            "POOL_STATE_AFTER_IS_DETERMINISTIC_DERIVED_STATE_NOT_OBSERVED_ACCOUNT".to_string(),
            format!(
                "{}_FILL_POOL_PHASE={:?}",
                self.input.side.label(),
                self.input.pool_phase
            ),
            format!("FILL_FORMULA_VERSION={SHADOW_V2_PRICE_FORMULA_VERSION}"),
        ];
        match self.input.side {
            ShadowV2ExecutionSide::Buy => {
                limitations.push("NO_LIVE_LANDING_OR_FAILED_TX_TELEMETRY".to_string());
            }
            ShadowV2ExecutionSide::Sell => {
                limitations.push("NO_LIVE_EXIT_TRANSACTION_OR_FAILED_TX_TELEMETRY".to_string());
                limitations.push("STATIC_EXIT_FILL_DOES_NOT_ENABLE_ACTIVE_CLOSE".to_string());
            }
        }
        limitations.extend(self.provenance_blockers.clone());

        ShadowV2ExecutionOutcome {
            side: self.input.side,
            fill_status: FillStatus::Filled,
            execution_simulation_ready: true,
            research_provenance_ready: research_ready,
            execution_label_grade: grade,
            provenance_ready: research_ready,
            provenance_blockers: sorted_unique(self.provenance_blockers.clone()),
            blocked_reasons: Vec::new(),
            no_fill_reason: None,
            fail_reason: None,
            fill_price: Some(quote.fill_price_sol_per_token),
            fill_price_source: Some(quote.price_source_label().to_string()),
            fill_amount_sol,
            fill_amount_tokens,
            output_amount_raw: Some(quote.expected_output_amount),
            expected_output_raw: Some(quote.expected_output_amount),
            min_out_raw: Some(effective_min_out),
            slippage_tolerance_bps: Some(quote.slippage_tolerance_bps as i32),
            deterministic_price_impact_bps: Some(quote.own_impact_bps),
            realized_slippage_bps: None,
            quote_fill_divergence_bps: None,
            own_impact_bps: Some(quote.own_impact_bps),
            fee_bps: Some(quote.fee_bps as i32),
            fee_amount_raw: Some(quote.fee_amount_lamports),
            pool_state_before_ref: Some(pool_state.envelope.event_id.clone()),
            pool_state_after_derived: Some(ShadowV2DerivedPoolState {
                source: match self.input.side {
                    ShadowV2ExecutionSide::Buy => ShadowV2BoundaryKind::EntryAfterDerived,
                    ShadowV2ExecutionSide::Sell => ShadowV2BoundaryKind::ExitAfterDerived,
                },
                source_pool_state_event_id: pool_state.envelope.event_id.clone(),
                source_formula_version: quote.formula_version.clone(),
                post_sol_reserves_lamports: quote.post_sol_reserves_lamports,
                post_token_reserves_raw: quote.post_token_reserves_raw,
                token_decimals: reserves.token_decimals,
                sol_lamports: reserves.sol_lamports,
            }),
            pool_state_after_source: Some("DETERMINISTIC_DERIVED".to_string()),
            reconstruction_status: format!(
                "{}_FILL_RECONSTRUCTED_BY_L1_EXECUTION_ENGINE",
                self.input.side.label()
            ),
            quality,
            limitations: sorted_unique(limitations),
            model_version: self.input.model_version.clone(),
        }
    }

    fn no_fill_outcome(
        &self,
        reason: ShadowV2NoFillReason,
        expected_output_raw: Option<u64>,
        min_out_raw: Option<u64>,
    ) -> ShadowV2ExecutionOutcome {
        let mut limitations = vec![
            format!(
                "EXECUTION_BOUNDARY_KIND={}",
                self.input.boundary_kind.label()
            ),
            reason.label().to_string(),
            "NO_FILL_IS_DETERMINISTIC_SIM_RESULT_NOT_LIVE_TRANSACTION".to_string(),
            "FILL_PRICE_UNAVAILABLE_BECAUSE_FILL_DID_NOT_OCCUR".to_string(),
        ];
        limitations.extend(self.provenance_blockers.clone());
        let research_ready = self.provenance_blockers.is_empty();
        ShadowV2ExecutionOutcome {
            side: self.input.side,
            fill_status: FillStatus::NoFill,
            execution_simulation_ready: true,
            research_provenance_ready: research_ready,
            execution_label_grade: if research_ready {
                ShadowV2ExecutionLabelGrade::ResearchCandidate
            } else {
                ShadowV2ExecutionLabelGrade::DiagnosticSim
            },
            provenance_ready: research_ready,
            provenance_blockers: sorted_unique(self.provenance_blockers.clone()),
            blocked_reasons: Vec::new(),
            no_fill_reason: Some(reason),
            fail_reason: None,
            fill_price: None,
            fill_price_source: None,
            fill_amount_sol: None,
            fill_amount_tokens: None,
            output_amount_raw: None,
            expected_output_raw,
            min_out_raw,
            slippage_tolerance_bps: self.input.slippage_tolerance_bps.map(i32::from),
            deterministic_price_impact_bps: None,
            realized_slippage_bps: None,
            quote_fill_divergence_bps: None,
            own_impact_bps: None,
            fee_bps: self.input.fee_bps.map(i32::from),
            fee_amount_raw: None,
            pool_state_before_ref: self
                .input
                .pool_state_before
                .map(|pool_state| pool_state.envelope.event_id.clone()),
            pool_state_after_derived: None,
            pool_state_after_source: None,
            reconstruction_status: format!("{}_FILL_NO_FILL_BY_L1_ENGINE", self.input.side.label()),
            quality: "L1_EXECUTION_SIM_NO_FILL".to_string(),
            limitations: sorted_unique(limitations),
            model_version: self.input.model_version.clone(),
        }
    }

    fn blocked_outcome(&self) -> ShadowV2ExecutionOutcome {
        let mut limitations = vec![
            format!(
                "EXECUTION_BOUNDARY_KIND={}",
                self.input.boundary_kind.label()
            ),
            "BLOCKED_BY_DATA_L1_EXECUTION_INPUTS_INCOMPLETE".to_string(),
            "NO_EXECUTABLE_FILL_LABEL_EMITTED".to_string(),
        ];
        limitations.extend(self.provenance_blockers.clone());
        limitations.extend(self.blocked_reasons.clone());
        ShadowV2ExecutionOutcome {
            side: self.input.side,
            fill_status: FillStatus::BlockedByData,
            execution_simulation_ready: false,
            research_provenance_ready: false,
            execution_label_grade: ShadowV2ExecutionLabelGrade::DiagnosticSim,
            provenance_ready: false,
            provenance_blockers: sorted_unique(self.provenance_blockers.clone()),
            blocked_reasons: sorted_unique(self.blocked_reasons.clone()),
            no_fill_reason: None,
            fail_reason: None,
            fill_price: None,
            fill_price_source: None,
            fill_amount_sol: None,
            fill_amount_tokens: None,
            output_amount_raw: None,
            expected_output_raw: None,
            min_out_raw: self.input.min_out_raw,
            slippage_tolerance_bps: self.input.slippage_tolerance_bps.map(i32::from),
            deterministic_price_impact_bps: None,
            realized_slippage_bps: None,
            quote_fill_divergence_bps: None,
            own_impact_bps: None,
            fee_bps: self.input.fee_bps.map(i32::from),
            fee_amount_raw: None,
            pool_state_before_ref: self
                .input
                .pool_state_before
                .map(|pool_state| pool_state.envelope.event_id.clone()),
            pool_state_after_derived: None,
            pool_state_after_source: None,
            reconstruction_status: format!(
                "{}_FILL_BLOCKED_BY_L1_EXECUTION_ENGINE",
                self.input.side.label()
            ),
            quality: "BLOCKED_BY_DATA".to_string(),
            limitations: sorted_unique(limitations),
            model_version: self.input.model_version.clone(),
        }
    }

    fn block(&mut self, reason: ShadowV2BlockedReason) {
        self.blocked_reasons.push(reason.label().to_string());
    }

    fn provenance(&mut self, reason: ShadowV2BlockedReason) {
        self.provenance_label(reason.label().to_string());
    }

    fn provenance_label(&mut self, label: String) {
        if !self.provenance_blockers.contains(&label) {
            self.provenance_blockers.push(label);
        }
    }

    fn no_fill_blocker(&mut self, reason: ShadowV2NoFillReason) {
        self.blocked_reasons.push(reason.label().to_string());
    }
}

fn normalized_token_amount(raw_tokens: u64, token_decimals: u8) -> f64 {
    raw_tokens as f64 / 10_f64.powi(token_decimals as i32)
}

fn sorted_unique(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values.dedup();
    values
}

fn normalize_pool_state_research_blocker(blocker: &str) -> String {
    match blocker {
        "POOL_STATE_ACCOUNT_DATA_HASH_MISSING" => ShadowV2BlockedReason::PoolStateHashMissing
            .label()
            .to_string(),
        "POOL_STATE_STALENESS_MS_MISSING_OR_REVERSED"
        | "POOL_STATE_STALENESS_SLOTS_MISSING_OR_REVERSED" => {
            ShadowV2BlockedReason::PoolStateStalenessUnknownOrReversed
                .label()
                .to_string()
        }
        _ => blocker.to_string(),
    }
}

fn has_account_state_source_proof(pool_state: &PoolStateSampleV2) -> bool {
    pool_state.source == PoolStateSource::AccountStateCore
        && pool_state.has_complete_account_state_source_proof()
}
