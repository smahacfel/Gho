use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use solana_sdk::{pubkey::Pubkey, signature::Signature};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum T0Source {
    PoolCreateTx,
    FirstObservedPoolAccountUpdate,
    FirstObservedBuy,
    ReplayFixture,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TxTimeSource {
    SlotIndex,
    BlockTime,
    LocalReceiverTimeDiagnosticOnly,
    ReplayFixture,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EconomicSpendSource {
    DecodedPumpInstruction,
    CurveRealSolDelta,
    SignerDeltaMinusKnownOverheads,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EconomicSpend {
    pub lamports: u64,
    pub source: EconomicSpendSource,
    pub confidence: f64,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct FeeTopologyFingerprint {
    pub external_fee_count: u8,
    pub internal_fee_count: u8,
    pub external_amount_pattern_hash: u16,
    pub has_wsol_self_flow: bool,
    pub has_create_ata_flow: bool,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct ExecutionTemplateFingerprint {
    pub compute_budget_shape: u8,
    pub outer_program_sequence_hash: u16,
    pub inner_program_sequence_hash: u16,
    pub inner_instruction_count_bucket: u8,
    pub account_role_pattern_hash: u16,
    pub fee_topology_hash: u16,
    pub ata_wsol_shape: u8,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct CapitalTemplateFingerprint {
    pub pre_balance_bucket: u16,
    pub residual_bucket: u16,
    pub overhead_bucket: u8,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct InfraFingerprint {
    pub account_role_pattern_hash: u16,
    pub outer_program_sequence_hash: u16,
    pub inner_program_sequence_hash: u16,
    pub outer_ix_count_bucket: u8,
    pub inner_ix_group_count_bucket: u8,
    pub compute_budget_shape: u8,
    pub fee_topology_hash: u16,
    pub ata_wsol_shape: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DevFingerprintMode {
    ComparablePureBuy,
    CreateTxSwapSliceOnly,
    NotComparable,
}

impl Default for DevFingerprintMode {
    fn default() -> Self {
        Self::NotComparable
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevFingerprintEvidence {
    pub mode: DevFingerprintMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<InfraFingerprint>,
    #[serde(default)]
    pub explicit_swap_slice: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DbiaBreakdown {
    pub account_role_similarity: f64,
    pub outer_program_similarity: f64,
    pub inner_program_similarity: f64,
    pub compute_budget_similarity: f64,
    pub fee_topology_similarity: f64,
    pub ata_wsol_similarity: f64,
    pub buyer_fingerprint_coverage: f64,
    pub dev_mode: DevFingerprintMode,
}

impl Default for DbiaBreakdown {
    fn default() -> Self {
        Self {
            account_role_similarity: 0.0,
            outer_program_similarity: 0.0,
            inner_program_similarity: 0.0,
            compute_budget_similarity: 0.0,
            fee_topology_similarity: 0.0,
            ata_wsol_similarity: 0.0,
            buyer_fingerprint_coverage: 0.0,
            dev_mode: DevFingerprintMode::NotComparable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeeTopologyCount {
    pub fingerprint: FeeTopologyFingerprint,
    pub count: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FtdiBreakdown {
    pub unique_buyer_count: u8,
    pub fingerprint_coverage: f64,
    pub missing_fingerprint_count: u8,
    #[serde(default, skip_serializing_if = "SmallVec::is_empty")]
    pub topology_counts: SmallVec<[FeeTopologyCount; 16]>,
}

impl Default for FtdiBreakdown {
    fn default() -> Self {
        Self {
            unique_buyer_count: 0,
            fingerprint_coverage: 0.0,
            missing_fingerprint_count: 0,
            topology_counts: SmallVec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SfdSourceCount {
    pub source: EconomicSpendSource,
    pub count: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SfdBreakdown {
    pub unique_buyer_count: u8,
    pub spend_coverage: f64,
    pub min_source_confidence: f64,
    pub mean_source_confidence: f64,
    pub skipped_outlier_count: u8,
    #[serde(default, skip_serializing_if = "SmallVec::is_empty")]
    pub spend_fractions: SmallVec<[f64; 16]>,
    #[serde(default, skip_serializing_if = "SmallVec::is_empty")]
    pub source_counts: SmallVec<[SfdSourceCount; 4]>,
}

impl Default for SfdBreakdown {
    fn default() -> Self {
        Self {
            unique_buyer_count: 0,
            spend_coverage: 0.0,
            min_source_confidence: 0.0,
            mean_source_confidence: 0.0,
            skipped_outlier_count: 0,
            spend_fractions: SmallVec::new(),
            source_counts: SmallVec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignerCrossPoolActivity {
    pub signer: Pubkey,
    pub other_pool_count: u8,
    #[serde(default)]
    pub current_pool_excluded: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_cutoff_slot: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_until_slot: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CpvSignerIntensity {
    pub signer: Pubkey,
    pub other_pool_count: u8,
    pub intensity: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CpvBreakdown {
    pub unique_buyer_count: u8,
    pub activity_coverage: f64,
    pub rolling_state_ready: bool,
    pub cutoff_proof_coverage: f64,
    pub current_pool_exclusion_coverage: f64,
    #[serde(default, skip_serializing_if = "SmallVec::is_empty")]
    pub signer_intensities: SmallVec<[CpvSignerIntensity; 16]>,
}

impl Default for CpvBreakdown {
    fn default() -> Self {
        Self {
            unique_buyer_count: 0,
            activity_coverage: 0.0,
            rolling_state_ready: false,
            cutoff_proof_coverage: 0.0,
            current_pool_exclusion_coverage: 0.0,
            signer_intensities: SmallVec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesBreakdown {
    pub sequence_buy_count: u8,
    pub eligible_pairs: u8,
    pub same_slot_burst_ratio: f64,
    #[serde(default, skip_serializing_if = "SmallVec::is_empty")]
    pub impacts: SmallVec<[f64; 16]>,
    #[serde(default, skip_serializing_if = "SmallVec::is_empty")]
    pub gap_after_slots: SmallVec<[f64; 16]>,
}

impl Default for DesBreakdown {
    fn default() -> Self {
        Self {
            sequence_buy_count: 0,
            eligible_pairs: 0,
            same_slot_burst_ratio: 0.0,
            impacts: SmallVec::new(),
            gap_after_slots: SmallVec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BseBreakdown {
    pub sequence_buy_count: u8,
    pub eligible_pairs: u8,
    pub tau_b_raw: Option<f64>,
    pub tau_b_abs: Option<f64>,
    pub economic_spend_coverage: f64,
    pub price_evidence_coverage: f64,
}

impl Default for BseBreakdown {
    fn default() -> Self {
        Self {
            sequence_buy_count: 0,
            eligible_pairs: 0,
            tau_b_raw: None,
            tau_b_abs: None,
            economic_spend_coverage: 0.0,
            price_evidence_coverage: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CucdBucketCount {
    pub bucket: u64,
    pub count: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CucdBreakdown {
    pub unique_buyer_count: u8,
    pub compute_unit_coverage: f64,
    pub cv: Option<f64>,
    pub robust_cv: Option<f64>,
    pub median_cu: Option<u64>,
    pub min_cu: Option<u64>,
    pub max_cu: Option<u64>,
    pub dominant_bucket_share: Option<f64>,
    pub cu_bucket_hhi_norm: Option<f64>,
    #[serde(default, skip_serializing_if = "SmallVec::is_empty")]
    pub bucket_counts: SmallVec<[CucdBucketCount; 16]>,
}

impl Default for CucdBreakdown {
    fn default() -> Self {
        Self {
            unique_buyer_count: 0,
            compute_unit_coverage: 0.0,
            cv: None,
            robust_cv: None,
            median_cu: None,
            min_cu: None,
            max_cu: None,
            dominant_bucket_share: None,
            cu_bucket_hhi_norm: None,
            bucket_counts: SmallVec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObservedBuyTx {
    pub signature: Signature,

    pub pool_id: Pubkey,
    pub mint: Pubkey,
    pub signer: Pubkey,

    pub slot: u64,
    pub slot_index: Option<u64>,

    pub tx_elapsed_ms_from_pool_create: Option<u64>,
    pub t0_source: Option<T0Source>,
    pub tx_time_source: Option<TxTimeSource>,

    pub is_success: bool,
    pub is_buy: bool,
    pub is_sell: bool,
    pub is_dev: bool,
    pub is_create_or_init_tx: bool,
    pub is_unknown_direction: bool,

    pub account_keys_resolved: SmallVec<[Pubkey; 64]>,

    pub outer_ix_count: Option<u8>,
    pub inner_ix_group_count: Option<u8>,

    pub fee_lamports: Option<u64>,

    pub pre_balance_signer: Option<u64>,
    pub post_balance_signer: Option<u64>,

    pub decoded_buy_sol_lamports: Option<u64>,
    pub curve_sol_delta_lamports: Option<u64>,
    pub economic_spent_lamports: Option<EconomicSpend>,

    pub tokens_received: Option<u64>,

    pub price_before: Option<f64>,
    pub price_after: Option<f64>,

    pub compute_units_consumed: Option<u64>,
    pub cost_units: Option<u64>,

    pub fee_topology_fp: Option<FeeTopologyFingerprint>,
    pub execution_template_fp: Option<ExecutionTemplateFingerprint>,
    pub capital_template_fp: Option<CapitalTemplateFingerprint>,
}

impl ObservedBuyTx {
    #[must_use]
    pub fn is_buyer_sample_candidate(&self) -> bool {
        self.is_success
            && self.is_buy
            && !self.is_sell
            && !self.is_unknown_direction
            && !(self.is_dev && self.is_create_or_init_tx)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CoordinationSampleFixture {
    pub txs: SmallVec<[ObservedBuyTx; 32]>,
}

impl CoordinationSampleFixture {
    #[must_use]
    pub fn new(txs: SmallVec<[ObservedBuyTx; 32]>) -> Self {
        Self { txs }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoordinationSampleSummary {
    pub total_txs_seen: u16,
    pub successful_buy_txs: u16,
    pub unique_buyers: u16,
    pub excluded_failed: u16,
    pub excluded_sell: u16,
    pub excluded_unknown_direction: u16,
    pub excluded_dev_create_or_init: u16,
    pub missing_slot_index_count: u16,
    pub missing_compute_units_count: u16,
    pub missing_balance_count: u16,
}
