use smallvec::SmallVec;
use solana_sdk::{pubkey::Pubkey, signature::Signature};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum T0Source {
    PoolCreateTx,
    FirstObservedPoolAccountUpdate,
    FirstObservedBuy,
    ReplayFixture,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxTimeSource {
    SlotIndex,
    BlockTime,
    LocalReceiverTimeDiagnosticOnly,
    ReplayFixture,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EconomicSpendSource {
    DecodedPumpInstruction,
    CurveRealSolDelta,
    SignerDeltaMinusKnownOverheads,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EconomicSpend {
    pub lamports: u64,
    pub source: EconomicSpendSource,
    pub confidence: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FeeTopologyFingerprint {
    pub external_fee_count: u8,
    pub internal_fee_count: u8,
    pub external_amount_pattern_hash: u16,
    pub has_wsol_self_flow: bool,
    pub has_create_ata_flow: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExecutionTemplateFingerprint {
    pub compute_budget_shape: u8,
    pub outer_program_sequence_hash: u16,
    pub inner_program_sequence_hash: u16,
    pub inner_instruction_count_bucket: u8,
    pub account_role_pattern_hash: u16,
    pub fee_topology_hash: u16,
    pub ata_wsol_shape: u8,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CapitalTemplateFingerprint {
    pub pre_balance_bucket: u16,
    pub residual_bucket: u16,
    pub overhead_bucket: u8,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
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
