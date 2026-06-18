use crate::events::PoolTransaction;
use ghost_brain::config::GatekeeperV2Config;
use ghost_core::features::coordination::stats::{kendall_tau_b, weighted_mad};
use ghost_core::tx_intelligence::types::{
    SybilResistanceFeatures, DBIA_INSUFFICIENT_BUYERS_REASON, DBIA_NO_DEV_BUY_REASON,
    DBIA_PARTIAL_FINGERPRINT_COVERAGE, DBIA_RAW_FINGERPRINT_UNAVAILABLE_REASON,
    DES_CURVE_DATA_UNAVAILABLE_REASON, DES_INSUFFICIENT_BUYS_REASON, DES_NO_COMPARABLE_PAIRS,
    DES_PARTIAL_SEQUENCE_COVERAGE, DES_SLOT_ORDER_UNAVAILABLE_REASON,
    FTDI_INSUFFICIENT_BUYS_REASON, FTDI_PARTIAL_FEE_TOPOLOGY_COVERAGE,
    FTDI_RAW_FEE_TOPOLOGY_UNAVAILABLE_REASON, SFD_BUY_AMOUNT_UNAVAILABLE,
    SFD_INSUFFICIENT_BUYS_REASON, SFD_NEGATIVE_BALANCE_DELTA_SKIPPED,
    SFD_PARTIAL_BALANCE_COVERAGE_REASON, SFD_POSTBALANCE_UNAVAILABLE_REASON,
    SFD_ZERO_PREBALANCE_SKIPPED_REASON,
};
use seer::types::ToolchainFingerprintInput;
use std::collections::{BTreeMap, HashMap, HashSet};

const DBIA_ACCOUNT_KEYS_WEIGHT: f64 = 0.20;
const DBIA_OUTER_INSTRUCTION_WEIGHT: f64 = 0.25;
const DBIA_CU_LIMIT_WEIGHT: f64 = 0.05;
const DBIA_CU_PRICE_WEIGHT: f64 = 0.05;
const DBIA_INNER_GROUP_WEIGHT: f64 = 0.25;
const DBIA_FEE_TOPOLOGY_WEIGHT: f64 = 0.20;
const DBIA_ACCOUNT_KEYS_SCALE: f64 = 8.0;
const DBIA_OUTER_INSTRUCTION_SCALE: f64 = 4.0;
const DBIA_INNER_GROUP_SCALE: f64 = 4.0;
const DBIA_FEE_TOPOLOGY_SCALE: f64 = 3.0;
const LAMPORTS_PER_SOL_F64: f64 = 1_000_000_000.0;

#[derive(Debug, Clone, PartialEq)]
struct SybilMetricQualityConfig {
    min_toolchain_metric_coverage: f64,
    min_des_valid_sequence_coverage: f64,
    cpv_min_observed_window_ratio: f64,
    fsc_require_clean_v2_for_actionability: bool,
    fsc_require_coverage_window_for_actionability: bool,
}

impl SybilMetricQualityConfig {
    fn from_gatekeeper_config(config: &GatekeeperV2Config) -> Self {
        Self {
            min_toolchain_metric_coverage: config.min_toolchain_metric_coverage,
            min_des_valid_sequence_coverage: config.min_des_valid_sequence_coverage,
            cpv_min_observed_window_ratio: config.cpv_min_observed_window_ratio,
            fsc_require_clean_v2_for_actionability: config.fsc_require_clean_v2_for_actionability,
            fsc_require_coverage_window_for_actionability: config
                .fsc_require_coverage_window_for_actionability,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FtdiComputation {
    pub fee_topology_diversity_index: Option<f64>,
    pub degraded_reasons: Vec<String>,
    pub buy_sample_count: u64,
    pub signer_sample_count: u64,
    pub toolchain_fingerprint_coverage: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DbiaComputation {
    pub dev_buyer_infrastructure_affinity: Option<f64>,
    pub degraded_reasons: Vec<String>,
    pub buy_sample_count: u64,
    pub signer_sample_count: u64,
    pub toolchain_fingerprint_coverage: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SfdComputation {
    pub spend_fraction_divergence: Option<f64>,
    pub degraded_reasons: Vec<String>,
    pub buy_sample_count: u64,
    pub signer_sample_count: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesComputation {
    pub demand_elasticity_score: Option<f64>,
    pub degraded_reasons: Vec<String>,
    pub buy_sample_count: u64,
    pub signer_sample_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BuySampleStats {
    buy_sample_count: u64,
    signer_sample_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FeeTopology {
    external_fee_count: u32,
    internal_fee_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InfrastructureFingerprint {
    account_keys_len: u32,
    outer_instruction_count: u32,
    has_set_compute_unit_limit: bool,
    has_set_compute_unit_price: bool,
    inner_instruction_group_count: u32,
    fee_topology: FeeTopology,
}

#[derive(Debug, Clone, Copy)]
struct SequencedBuyTx<'a> {
    tx: &'a PoolTransaction,
    buffer_index: usize,
}

#[derive(Debug, Clone, Copy)]
struct OrderedBuyTx<'a> {
    tx: &'a PoolTransaction,
    slot: u64,
    intra_slot_rank: usize,
    slot_group_size: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SfdSampleCoverage {
    MissingRequiredBalance,
    ZeroPreBalance,
    MissingBuyAmount,
    MissingFallbackPostBalance,
    NegativeBalanceDelta,
    FallbackSpend,
    PrimarySpend,
}

#[derive(Debug, Clone, Copy)]
struct SelectedSfdSample<'a> {
    tx: &'a PoolTransaction,
    coverage: SfdSampleCoverage,
}

#[derive(Debug, Clone, Copy)]
struct SfdSpendSample {
    spend_fraction: f64,
    weight: f64,
    used_buy_amount_fallback: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolchainMetricKind {
    Ftdi,
    Dbia,
}

#[derive(Debug, Clone, Copy)]
struct SelectedToolchainSample<'a> {
    tx: &'a PoolTransaction,
    usable: bool,
}

impl InfrastructureFingerprint {
    fn from_input(input: &ToolchainFingerprintInput) -> Option<Self> {
        Some(Self {
            account_keys_len: input.account_keys_len?,
            outer_instruction_count: input.outer_instruction_count?,
            has_set_compute_unit_limit: input.has_set_compute_unit_limit?,
            has_set_compute_unit_price: input.has_set_compute_unit_price?,
            inner_instruction_group_count: input.inner_instruction_group_count?,
            fee_topology: FeeTopology {
                external_fee_count: input.external_fee_transfer_count?,
                internal_fee_count: input.internal_fee_transfer_count?,
            },
        })
    }

    fn similarity(&self, other: &Self) -> f64 {
        let mut distance = 0.0;
        distance += normalized_u32_distance(
            self.account_keys_len,
            other.account_keys_len,
            DBIA_ACCOUNT_KEYS_SCALE,
        ) * DBIA_ACCOUNT_KEYS_WEIGHT;
        distance += normalized_u32_distance(
            self.outer_instruction_count,
            other.outer_instruction_count,
            DBIA_OUTER_INSTRUCTION_SCALE,
        ) * DBIA_OUTER_INSTRUCTION_WEIGHT;
        if self.has_set_compute_unit_limit != other.has_set_compute_unit_limit {
            distance += DBIA_CU_LIMIT_WEIGHT;
        }
        if self.has_set_compute_unit_price != other.has_set_compute_unit_price {
            distance += DBIA_CU_PRICE_WEIGHT;
        }
        distance += normalized_u32_distance(
            self.inner_instruction_group_count,
            other.inner_instruction_group_count,
            DBIA_INNER_GROUP_SCALE,
        ) * DBIA_INNER_GROUP_WEIGHT;
        distance += self.fee_topology.distance(&other.fee_topology) * DBIA_FEE_TOPOLOGY_WEIGHT;
        (1.0 - distance).clamp(0.0, 1.0)
    }
}

impl FeeTopology {
    fn distance(&self, other: &Self) -> f64 {
        let external = normalized_u32_distance(
            self.external_fee_count,
            other.external_fee_count,
            DBIA_FEE_TOPOLOGY_SCALE,
        );
        let internal = normalized_u32_distance(
            self.internal_fee_count,
            other.internal_fee_count,
            DBIA_FEE_TOPOLOGY_SCALE,
        );
        (external + internal) / 2.0
    }
}

fn normalized_u32_distance(left: u32, right: u32, scale: f64) -> f64 {
    (left.abs_diff(right) as f64 / scale).min(1.0)
}

fn successful_buy_txs<'a>(
    transactions: impl IntoIterator<Item = &'a PoolTransaction>,
) -> Vec<&'a PoolTransaction> {
    transactions
        .into_iter()
        .filter(|tx| tx.is_buy && tx.success)
        .collect()
}

fn successful_buy_samples<'a>(transactions: &[&'a PoolTransaction]) -> Vec<SequencedBuyTx<'a>> {
    transactions
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(buffer_index, tx)| {
            (tx.is_buy && tx.success).then_some(SequencedBuyTx { tx, buffer_index })
        })
        .collect()
}

fn buy_sample_stats(buy_txs: &[&PoolTransaction]) -> BuySampleStats {
    BuySampleStats {
        buy_sample_count: buy_txs.len() as u64,
        signer_sample_count: buy_txs
            .iter()
            .map(|tx| tx.signer.as_str())
            .collect::<HashSet<_>>()
            .len() as u64,
    }
}

fn toolchain_sample_usable(tx: &PoolTransaction, metric_kind: ToolchainMetricKind) -> bool {
    match metric_kind {
        ToolchainMetricKind::Ftdi => tx.toolchain_fingerprint.fee_topology().is_some(),
        ToolchainMetricKind::Dbia => {
            InfrastructureFingerprint::from_input(&tx.toolchain_fingerprint).is_some()
        }
    }
}

fn best_toolchain_sample_per_signer<'a>(
    buy_txs: &[&'a PoolTransaction],
    metric_kind: ToolchainMetricKind,
) -> Vec<&'a PoolTransaction> {
    let mut signer_order = Vec::<String>::new();
    let mut selected = HashMap::<String, SelectedToolchainSample<'a>>::new();

    for &tx in buy_txs {
        let signer = tx.signer.clone();
        let usable = toolchain_sample_usable(tx, metric_kind);
        match selected.get_mut(&signer) {
            Some(best) => {
                if usable && !best.usable {
                    *best = SelectedToolchainSample { tx, usable };
                }
            }
            None => {
                signer_order.push(signer.clone());
                selected.insert(signer, SelectedToolchainSample { tx, usable });
            }
        }
    }

    signer_order
        .into_iter()
        .filter_map(|signer| selected.get(&signer).map(|sample| sample.tx))
        .collect()
}

fn toolchain_coverage(usable_count: usize, total_count: usize) -> Option<f64> {
    (total_count > 0).then_some(usable_count as f64 / total_count as f64)
}

fn sfd_sample_coverage(tx: &PoolTransaction) -> SfdSampleCoverage {
    let Some(pre_balance) = tx.signer_pre_balance_lamports else {
        return SfdSampleCoverage::MissingRequiredBalance;
    };
    if pre_balance == 0 {
        return SfdSampleCoverage::ZeroPreBalance;
    }
    if tx
        .signer_post_balance_lamports
        .is_some_and(|post_balance| post_balance > pre_balance)
    {
        return SfdSampleCoverage::NegativeBalanceDelta;
    }
    if tx.sol_amount_lamports.is_some_and(|amount| amount > 0) {
        return SfdSampleCoverage::PrimarySpend;
    }
    if tx.volume_sol.is_finite() && tx.volume_sol > 0.0 {
        if tx.signer_post_balance_lamports.is_some() {
            SfdSampleCoverage::FallbackSpend
        } else {
            SfdSampleCoverage::MissingFallbackPostBalance
        }
    } else {
        SfdSampleCoverage::MissingBuyAmount
    }
}

fn sfd_spend_sample(tx: &PoolTransaction) -> Option<SfdSpendSample> {
    let pre_balance = tx.signer_pre_balance_lamports?;
    if pre_balance == 0 {
        return None;
    }
    if tx
        .signer_post_balance_lamports
        .is_some_and(|post_balance| post_balance > pre_balance)
    {
        return None;
    }

    if let Some(amount_lamports) = tx.sol_amount_lamports.filter(|amount| *amount > 0) {
        let spend_fraction = amount_lamports as f64 / pre_balance as f64;
        let buy_amount_sol = amount_lamports as f64 / LAMPORTS_PER_SOL_F64;
        let weight = buy_amount_sol.sqrt();
        return (spend_fraction.is_finite() && weight.is_finite() && weight > 0.0).then_some(
            SfdSpendSample {
                spend_fraction,
                weight,
                used_buy_amount_fallback: false,
            },
        );
    }

    if tx.volume_sol.is_finite() && tx.volume_sol > 0.0 {
        let post_balance = tx.signer_post_balance_lamports?;
        let spent_lamports = pre_balance.saturating_sub(post_balance);
        let spend_fraction = spent_lamports as f64 / pre_balance as f64;
        let weight = tx.volume_sol.sqrt();
        return (spend_fraction.is_finite() && weight.is_finite() && weight > 0.0).then_some(
            SfdSpendSample {
                spend_fraction,
                weight,
                used_buy_amount_fallback: true,
            },
        );
    }

    None
}

fn selected_sfd_samples<'a>(buy_txs: &[&'a PoolTransaction]) -> Vec<&'a PoolTransaction> {
    let mut signer_order = Vec::<String>::new();
    let mut selected = HashMap::<String, SelectedSfdSample<'a>>::new();

    for &tx in buy_txs {
        let signer = tx.signer.clone();
        let coverage = sfd_sample_coverage(tx);
        match selected.get_mut(&signer) {
            Some(best) => {
                if coverage > best.coverage {
                    *best = SelectedSfdSample { tx, coverage };
                }
            }
            None => {
                signer_order.push(signer.clone());
                selected.insert(signer, SelectedSfdSample { tx, coverage });
            }
        }
    }

    signer_order
        .into_iter()
        .filter_map(|signer| selected.get(&signer).map(|sample| sample.tx))
        .collect()
}

fn resolve_dev_wallet<'a>(
    buy_txs: &[&'a PoolTransaction],
    explicit_dev_wallet: Option<&'a str>,
) -> Option<&'a str> {
    explicit_dev_wallet.or_else(|| {
        buy_txs
            .iter()
            .find(|tx| tx.is_dev_buy)
            .map(|tx| tx.signer.as_str())
    })
}

fn ordered_buy_samples<'a>(buy_samples: &[SequencedBuyTx<'a>]) -> Vec<OrderedBuyTx<'a>> {
    let mut by_slot = BTreeMap::<u64, Vec<SequencedBuyTx<'a>>>::new();
    for sample in buy_samples {
        if let Some(slot) = sample.tx.slot {
            by_slot.entry(slot).or_default().push(*sample);
        }
    }

    let mut ordered = Vec::with_capacity(buy_samples.len());
    for (slot, mut slot_samples) in by_slot {
        let use_event_ordinal = slot_samples
            .iter()
            .all(|sample| sample.tx.event_ordinal.is_some());
        if use_event_ordinal {
            slot_samples.sort_by_key(|sample| {
                (
                    sample.tx.event_ordinal.unwrap_or_default(),
                    sample.buffer_index,
                )
            });
        } else {
            slot_samples.sort_by_key(|sample| sample.buffer_index);
        }

        let slot_group_size = slot_samples.len();
        for (intra_slot_rank, sample) in slot_samples.into_iter().enumerate() {
            ordered.push(OrderedBuyTx {
                tx: sample.tx,
                slot,
                intra_slot_rank,
                slot_group_size,
            });
        }
    }

    ordered
}

fn curve_price(tx: &PoolTransaction) -> Option<f64> {
    if !tx.curve_data_known {
        return None;
    }

    let v_sol = tx.v_sol_in_bonding_curve?;
    let v_tokens = tx.v_tokens_in_bonding_curve?;
    if !v_sol.is_finite() || !v_tokens.is_finite() || v_tokens <= 0.0 {
        return None;
    }

    let price = v_sol / v_tokens;
    (price.is_finite() && price > 0.0).then_some(price)
}

fn inter_buy_delta(previous: OrderedBuyTx<'_>, current: OrderedBuyTx<'_>) -> f64 {
    if current.slot != previous.slot {
        return current.slot.saturating_sub(previous.slot) as f64;
    }

    (current
        .intra_slot_rank
        .saturating_sub(previous.intra_slot_rank) as f64)
        / current.slot_group_size as f64
}

fn add_degraded_reason(reasons: &mut Vec<String>, reason: &str) {
    if !reasons.iter().any(|existing| existing == reason) {
        reasons.push(reason.to_string());
    }
}

fn des_unavailable_reasons(missing_slot: bool, missing_curve: bool) -> Vec<String> {
    let mut reasons = Vec::new();
    if missing_slot {
        add_degraded_reason(&mut reasons, DES_SLOT_ORDER_UNAVAILABLE_REASON);
    }
    if missing_curve {
        add_degraded_reason(&mut reasons, DES_CURVE_DATA_UNAVAILABLE_REASON);
    }
    if reasons.is_empty() {
        add_degraded_reason(&mut reasons, DES_INSUFFICIENT_BUYS_REASON);
    }
    reasons
}

fn des_valid_segments<'a>(
    buy_samples: &[SequencedBuyTx<'a>],
) -> (Vec<Vec<(OrderedBuyTx<'a>, f64)>>, bool, bool) {
    let mut missing_slot = false;
    let mut missing_curve = false;
    let mut slot_runs = Vec::<Vec<SequencedBuyTx<'a>>>::new();
    let mut current_run = Vec::<SequencedBuyTx<'a>>::new();

    for sample in buy_samples {
        if sample.tx.slot.is_some() {
            current_run.push(*sample);
        } else {
            missing_slot = true;
            if !current_run.is_empty() {
                slot_runs.push(std::mem::take(&mut current_run));
            }
        }
    }
    if !current_run.is_empty() {
        slot_runs.push(current_run);
    }

    let mut segments = Vec::<Vec<(OrderedBuyTx<'a>, f64)>>::new();
    for run in slot_runs {
        let mut current_segment = Vec::<(OrderedBuyTx<'a>, f64)>::new();
        for sample in ordered_buy_samples(&run) {
            if let Some(price) = curve_price(sample.tx) {
                current_segment.push((sample, price));
            } else {
                missing_curve = true;
                if !current_segment.is_empty() {
                    segments.push(std::mem::take(&mut current_segment));
                }
            }
        }
        if !current_segment.is_empty() {
            segments.push(current_segment);
        }
    }

    (segments, missing_slot, missing_curve)
}

pub fn compute_ftdi<'a>(
    transactions: impl IntoIterator<Item = &'a PoolTransaction>,
) -> FtdiComputation {
    let buy_txs = successful_buy_txs(transactions);
    let quality = SybilMetricQualityConfig::from_gatekeeper_config(&GatekeeperV2Config::default());
    compute_ftdi_from_buys(&buy_txs, &quality)
}

fn compute_ftdi_from_buys(
    buy_txs: &[&PoolTransaction],
    quality: &SybilMetricQualityConfig,
) -> FtdiComputation {
    let stats = buy_sample_stats(buy_txs);
    if stats.buy_sample_count < 3 {
        return FtdiComputation {
            fee_topology_diversity_index: None,
            degraded_reasons: vec![FTDI_INSUFFICIENT_BUYS_REASON.to_string()],
            buy_sample_count: stats.buy_sample_count,
            signer_sample_count: stats.signer_sample_count,
            toolchain_fingerprint_coverage: None,
        };
    }

    let unique_samples = best_toolchain_sample_per_signer(buy_txs, ToolchainMetricKind::Ftdi);
    if unique_samples.len() < 3 {
        return FtdiComputation {
            fee_topology_diversity_index: None,
            degraded_reasons: vec![FTDI_INSUFFICIENT_BUYS_REASON.to_string()],
            buy_sample_count: stats.buy_sample_count,
            signer_sample_count: stats.signer_sample_count,
            toolchain_fingerprint_coverage: None,
        };
    }

    let mut unique_topologies = HashSet::<FeeTopology>::new();
    let mut usable_count = 0usize;
    for tx in &unique_samples {
        let Some((external_fee_count, internal_fee_count)) =
            tx.toolchain_fingerprint.fee_topology()
        else {
            continue;
        };

        usable_count += 1;
        unique_topologies.insert(FeeTopology {
            external_fee_count,
            internal_fee_count,
        });
    }

    let coverage = toolchain_coverage(usable_count, unique_samples.len());
    if usable_count < 3
        || coverage.map_or(true, |value| value < quality.min_toolchain_metric_coverage)
    {
        return FtdiComputation {
            fee_topology_diversity_index: None,
            degraded_reasons: vec![FTDI_RAW_FEE_TOPOLOGY_UNAVAILABLE_REASON.to_string()],
            buy_sample_count: stats.buy_sample_count,
            signer_sample_count: stats.signer_sample_count,
            toolchain_fingerprint_coverage: coverage,
        };
    }

    let mut degraded_reasons = Vec::new();
    if usable_count < unique_samples.len() {
        degraded_reasons.push(FTDI_PARTIAL_FEE_TOPOLOGY_COVERAGE.to_string());
    }

    FtdiComputation {
        fee_topology_diversity_index: Some(unique_topologies.len() as f64 / usable_count as f64),
        degraded_reasons,
        buy_sample_count: stats.buy_sample_count,
        signer_sample_count: stats.signer_sample_count,
        toolchain_fingerprint_coverage: coverage,
    }
}

pub fn compute_dbia<'a>(
    transactions: impl IntoIterator<Item = &'a PoolTransaction>,
    dev_wallet: Option<&'a str>,
) -> DbiaComputation {
    let buy_txs = successful_buy_txs(transactions);
    let quality = SybilMetricQualityConfig::from_gatekeeper_config(&GatekeeperV2Config::default());
    compute_dbia_from_buys(&buy_txs, dev_wallet, &quality)
}

fn compute_dbia_from_buys<'a>(
    buy_txs: &[&'a PoolTransaction],
    dev_wallet: Option<&'a str>,
    quality: &SybilMetricQualityConfig,
) -> DbiaComputation {
    let stats = buy_sample_stats(buy_txs);
    let selected_samples = best_toolchain_sample_per_signer(buy_txs, ToolchainMetricKind::Dbia);
    let Some(dev_wallet) = resolve_dev_wallet(buy_txs, dev_wallet) else {
        return DbiaComputation {
            dev_buyer_infrastructure_affinity: None,
            degraded_reasons: vec![DBIA_NO_DEV_BUY_REASON.to_string()],
            buy_sample_count: stats.buy_sample_count,
            signer_sample_count: stats.signer_sample_count,
            toolchain_fingerprint_coverage: None,
        };
    };
    let Some(dev_tx) = selected_samples
        .iter()
        .copied()
        .find(|tx| tx.signer == dev_wallet)
    else {
        return DbiaComputation {
            dev_buyer_infrastructure_affinity: None,
            degraded_reasons: vec![DBIA_NO_DEV_BUY_REASON.to_string()],
            buy_sample_count: stats.buy_sample_count,
            signer_sample_count: stats.signer_sample_count,
            toolchain_fingerprint_coverage: None,
        };
    };

    let non_dev_total = selected_samples
        .iter()
        .filter(|tx| tx.signer != dev_wallet)
        .count();
    if non_dev_total < 2 {
        return DbiaComputation {
            dev_buyer_infrastructure_affinity: None,
            degraded_reasons: vec![DBIA_INSUFFICIENT_BUYERS_REASON.to_string()],
            buy_sample_count: stats.buy_sample_count,
            signer_sample_count: stats.signer_sample_count,
            toolchain_fingerprint_coverage: None,
        };
    }

    let usable_count = selected_samples
        .iter()
        .filter(|tx| InfrastructureFingerprint::from_input(&tx.toolchain_fingerprint).is_some())
        .count();
    let coverage = toolchain_coverage(usable_count, selected_samples.len());

    let Some(dev_fp) = InfrastructureFingerprint::from_input(&dev_tx.toolchain_fingerprint) else {
        return DbiaComputation {
            dev_buyer_infrastructure_affinity: None,
            degraded_reasons: vec![DBIA_RAW_FINGERPRINT_UNAVAILABLE_REASON.to_string()],
            buy_sample_count: stats.buy_sample_count,
            signer_sample_count: stats.signer_sample_count,
            toolchain_fingerprint_coverage: coverage,
        };
    };

    let buyer_fingerprints: Vec<InfrastructureFingerprint> = selected_samples
        .iter()
        .filter(|tx| tx.signer != dev_wallet)
        .filter_map(|tx| InfrastructureFingerprint::from_input(&tx.toolchain_fingerprint))
        .collect();

    if buyer_fingerprints.len() < 2
        || coverage.map_or(true, |value| value < quality.min_toolchain_metric_coverage)
    {
        return DbiaComputation {
            dev_buyer_infrastructure_affinity: None,
            degraded_reasons: vec![DBIA_RAW_FINGERPRINT_UNAVAILABLE_REASON.to_string()],
            buy_sample_count: stats.buy_sample_count,
            signer_sample_count: stats.signer_sample_count,
            toolchain_fingerprint_coverage: coverage,
        };
    }

    let mut similarity_sum = 0.0;
    for fingerprint in &buyer_fingerprints {
        similarity_sum += dev_fp.similarity(fingerprint);
    }

    let mut degraded_reasons = Vec::new();
    if usable_count < selected_samples.len() {
        degraded_reasons.push(DBIA_PARTIAL_FINGERPRINT_COVERAGE.to_string());
    }

    DbiaComputation {
        dev_buyer_infrastructure_affinity: Some(similarity_sum / buyer_fingerprints.len() as f64),
        degraded_reasons,
        buy_sample_count: stats.buy_sample_count,
        signer_sample_count: stats.signer_sample_count,
        toolchain_fingerprint_coverage: coverage,
    }
}

pub fn compute_sfd<'a>(
    transactions: impl IntoIterator<Item = &'a PoolTransaction>,
) -> SfdComputation {
    let buy_txs = successful_buy_txs(transactions);
    compute_sfd_from_buys(&buy_txs)
}

fn compute_sfd_from_buys(buy_txs: &[&PoolTransaction]) -> SfdComputation {
    let stats = buy_sample_stats(buy_txs);
    if stats.buy_sample_count < 3 {
        return SfdComputation {
            spend_fraction_divergence: None,
            degraded_reasons: vec![SFD_INSUFFICIENT_BUYS_REASON.to_string()],
            buy_sample_count: stats.buy_sample_count,
            signer_sample_count: stats.signer_sample_count,
        };
    }

    let unique_samples = selected_sfd_samples(buy_txs);
    let mut zero_prebalance_skipped = false;
    let mut partial_balance_coverage = false;
    let mut postbalance_unavailable = false;
    let mut negative_balance_delta_skipped = false;
    let mut buy_amount_unavailable = false;
    let mut spend_samples = Vec::<(f64, f64)>::new();

    for tx in &unique_samples {
        match sfd_spend_sample(tx) {
            Some(sample) => {
                if sample.used_buy_amount_fallback {
                    buy_amount_unavailable = true;
                }
                spend_samples.push((sample.spend_fraction, sample.weight));
            }
            None => match sfd_sample_coverage(tx) {
                SfdSampleCoverage::MissingRequiredBalance => {
                    partial_balance_coverage = true;
                    postbalance_unavailable = true;
                }
                SfdSampleCoverage::ZeroPreBalance => {
                    zero_prebalance_skipped = true;
                }
                SfdSampleCoverage::MissingBuyAmount => {
                    buy_amount_unavailable = true;
                }
                SfdSampleCoverage::MissingFallbackPostBalance => {
                    buy_amount_unavailable = true;
                    partial_balance_coverage = true;
                    postbalance_unavailable = true;
                }
                SfdSampleCoverage::NegativeBalanceDelta => {
                    negative_balance_delta_skipped = true;
                }
                SfdSampleCoverage::FallbackSpend | SfdSampleCoverage::PrimarySpend => {}
            },
        }
    }

    if spend_samples.len() < 3 {
        let mut reasons = Vec::new();
        if zero_prebalance_skipped {
            add_degraded_reason(&mut reasons, SFD_ZERO_PREBALANCE_SKIPPED_REASON);
        }
        if negative_balance_delta_skipped {
            add_degraded_reason(&mut reasons, SFD_NEGATIVE_BALANCE_DELTA_SKIPPED);
        }
        if buy_amount_unavailable {
            add_degraded_reason(&mut reasons, SFD_BUY_AMOUNT_UNAVAILABLE);
        }
        if postbalance_unavailable {
            add_degraded_reason(&mut reasons, SFD_POSTBALANCE_UNAVAILABLE_REASON);
        }
        add_degraded_reason(&mut reasons, SFD_INSUFFICIENT_BUYS_REASON);
        return SfdComputation {
            spend_fraction_divergence: None,
            degraded_reasons: reasons,
            buy_sample_count: stats.buy_sample_count,
            signer_sample_count: stats.signer_sample_count,
        };
    }

    let spend_fraction_divergence = weighted_mad(&spend_samples);

    let mut degraded_reasons = Vec::new();
    if zero_prebalance_skipped {
        add_degraded_reason(&mut degraded_reasons, SFD_ZERO_PREBALANCE_SKIPPED_REASON);
    }
    if negative_balance_delta_skipped {
        add_degraded_reason(&mut degraded_reasons, SFD_NEGATIVE_BALANCE_DELTA_SKIPPED);
    }
    if buy_amount_unavailable {
        add_degraded_reason(&mut degraded_reasons, SFD_BUY_AMOUNT_UNAVAILABLE);
    }
    if partial_balance_coverage {
        add_degraded_reason(&mut degraded_reasons, SFD_PARTIAL_BALANCE_COVERAGE_REASON);
    }

    SfdComputation {
        spend_fraction_divergence,
        degraded_reasons,
        buy_sample_count: stats.buy_sample_count,
        signer_sample_count: stats.signer_sample_count,
    }
}

pub fn compute_des<'a>(
    transactions: impl IntoIterator<Item = &'a PoolTransaction>,
) -> DesComputation {
    let transactions: Vec<&PoolTransaction> = transactions.into_iter().collect();
    let quality = SybilMetricQualityConfig::from_gatekeeper_config(&GatekeeperV2Config::default());
    compute_des_from_transactions(&transactions, &quality)
}

fn compute_des_from_transactions(
    transactions: &[&PoolTransaction],
    quality: &SybilMetricQualityConfig,
) -> DesComputation {
    let buy_samples = successful_buy_samples(transactions);
    let buy_txs: Vec<&PoolTransaction> = buy_samples.iter().map(|sample| sample.tx).collect();
    let stats = buy_sample_stats(&buy_txs);

    if stats.buy_sample_count < 4 {
        return DesComputation {
            demand_elasticity_score: None,
            degraded_reasons: vec![DES_INSUFFICIENT_BUYS_REASON.to_string()],
            buy_sample_count: stats.buy_sample_count,
            signer_sample_count: stats.signer_sample_count,
        };
    }

    let (segments, missing_slot, missing_curve) = des_valid_segments(&buy_samples);
    let mut selected_segment: Option<Vec<(OrderedBuyTx<'_>, f64)>> = None;
    for segment in segments {
        if segment.len() < 4 {
            continue;
        }
        if selected_segment
            .as_ref()
            .map_or(true, |current| segment.len() > current.len())
        {
            selected_segment = Some(segment);
        }
    }

    let Some(selected_segment) = selected_segment else {
        return DesComputation {
            demand_elasticity_score: None,
            degraded_reasons: des_unavailable_reasons(missing_slot, missing_curve),
            buy_sample_count: stats.buy_sample_count,
            signer_sample_count: stats.signer_sample_count,
        };
    };

    let coverage = selected_segment.len() as f64 / stats.buy_sample_count as f64;
    if coverage < quality.min_des_valid_sequence_coverage {
        return DesComputation {
            demand_elasticity_score: None,
            degraded_reasons: des_unavailable_reasons(missing_slot, missing_curve),
            buy_sample_count: stats.buy_sample_count,
            signer_sample_count: stats.signer_sample_count,
        };
    }

    let mut price_impacts = Vec::<f64>::with_capacity(selected_segment.len().saturating_sub(1));
    let mut timing_deltas = Vec::<f64>::with_capacity(selected_segment.len().saturating_sub(1));
    for index in 1..selected_segment.len() {
        let (previous, previous_price) = selected_segment[index - 1];
        let (current, current_price) = selected_segment[index];
        price_impacts.push((current_price - previous_price) / previous_price);
        timing_deltas.push(inter_buy_delta(previous, current));
    }

    let Some(demand_elasticity_score) = kendall_tau_b(&price_impacts, &timing_deltas) else {
        return DesComputation {
            demand_elasticity_score: None,
            degraded_reasons: vec![DES_NO_COMPARABLE_PAIRS.to_string()],
            buy_sample_count: stats.buy_sample_count,
            signer_sample_count: stats.signer_sample_count,
        };
    };

    let mut degraded_reasons = Vec::new();
    if selected_segment.len() < stats.buy_sample_count as usize {
        add_degraded_reason(&mut degraded_reasons, DES_PARTIAL_SEQUENCE_COVERAGE);
    }

    DesComputation {
        demand_elasticity_score: Some(demand_elasticity_score),
        degraded_reasons,
        buy_sample_count: stats.buy_sample_count,
        signer_sample_count: stats.signer_sample_count,
    }
}

pub fn compute_sybil_resistance<'a>(
    transactions: impl IntoIterator<Item = &'a PoolTransaction>,
    dev_wallet: Option<&'a str>,
) -> SybilResistanceFeatures {
    compute_sybil_resistance_with_config(transactions, dev_wallet, &GatekeeperV2Config::default())
}

pub fn compute_sybil_resistance_with_config<'a>(
    transactions: impl IntoIterator<Item = &'a PoolTransaction>,
    dev_wallet: Option<&'a str>,
    config: &GatekeeperV2Config,
) -> SybilResistanceFeatures {
    let transactions: Vec<&PoolTransaction> = transactions.into_iter().collect();
    let buy_txs = successful_buy_txs(transactions.iter().copied());
    let quality = SybilMetricQualityConfig::from_gatekeeper_config(config);
    let ftdi = compute_ftdi_from_buys(&buy_txs, &quality);
    let dbia = compute_dbia_from_buys(&buy_txs, dev_wallet, &quality);
    let sfd = compute_sfd_from_buys(&buy_txs);
    let des = compute_des_from_transactions(&transactions, &quality);

    let mut degraded_reasons = Vec::<String>::new();
    for reason in ftdi
        .degraded_reasons
        .iter()
        .chain(dbia.degraded_reasons.iter())
        .chain(sfd.degraded_reasons.iter())
        .chain(des.degraded_reasons.iter())
    {
        if !degraded_reasons.contains(reason) {
            degraded_reasons.push(reason.clone());
        }
    }

    SybilResistanceFeatures {
        fee_topology_diversity_index: ftdi.fee_topology_diversity_index,
        dev_buyer_infrastructure_affinity: dbia.dev_buyer_infrastructure_affinity,
        spend_fraction_divergence: sfd.spend_fraction_divergence,
        demand_elasticity_score: des.demand_elasticity_score,
        degraded_reasons,
        buy_sample_count: ftdi.buy_sample_count,
        signer_sample_count: ftdi.signer_sample_count,
        toolchain_fingerprint_coverage: match (
            ftdi.toolchain_fingerprint_coverage,
            dbia.toolchain_fingerprint_coverage,
        ) {
            (Some(ftdi_coverage), Some(dbia_coverage)) => Some(ftdi_coverage.min(dbia_coverage)),
            (Some(coverage), None) | (None, Some(coverage)) => Some(coverage),
            (None, None) => None,
        },
        ..SybilResistanceFeatures::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{PoolTransaction, RawBytesMissingReason};
    use ghost_core::{CurveFinality, EventSemanticEnvelope, EventTimeMetadata};

    fn buy_tx(
        signer: &str,
        signature: &str,
        toolchain_fingerprint: ToolchainFingerprintInput,
    ) -> PoolTransaction {
        PoolTransaction {
            semantic: EventSemanticEnvelope::default(),
            pool_amm_id: "pool-1".to_string(),
            slot: Some(1),
            event_ordinal: Some(0),
            tx_index: None,
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 1_000,
            event_time: EventTimeMetadata::default(),
            arrival_ts_ms: 1_000,
            signer: signer.to_string(),
            is_buy: true,
            volume_sol: 1.0,
            sol_amount_lamports: Some(1_000_000_000),
            token_amount_units: Some(1_000_000),
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: signature.to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
            token_mint: None,
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
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
            toolchain_fingerprint,
            curve_data_known: false,
            curve_finality: CurveFinality::Speculative,
        }
    }

    fn ftdi_fingerprint(topology: Option<(u32, u32)>) -> ToolchainFingerprintInput {
        ToolchainFingerprintInput {
            external_fee_transfer_count: topology.map(|value| value.0),
            internal_fee_transfer_count: topology.map(|value| value.1),
            ..ToolchainFingerprintInput::default()
        }
    }

    fn dbia_fingerprint(
        account_keys_len: u32,
        outer_instruction_count: u32,
        has_set_compute_unit_limit: bool,
        has_set_compute_unit_price: bool,
        inner_instruction_group_count: u32,
        fee_topology: (u32, u32),
    ) -> ToolchainFingerprintInput {
        ToolchainFingerprintInput {
            account_keys_len: Some(account_keys_len),
            outer_instruction_count: Some(outer_instruction_count),
            inner_instruction_group_count: Some(inner_instruction_group_count),
            has_set_compute_unit_limit: Some(has_set_compute_unit_limit),
            has_set_compute_unit_price: Some(has_set_compute_unit_price),
            external_fee_transfer_count: Some(fee_topology.0),
            internal_fee_transfer_count: Some(fee_topology.1),
            filtered_wsol_self_transfer_count: Some(0),
        }
    }

    fn dbia_buy_tx(
        signer: &str,
        signature: &str,
        is_dev_buy: bool,
        toolchain_fingerprint: ToolchainFingerprintInput,
    ) -> PoolTransaction {
        let mut tx = buy_tx(signer, signature, toolchain_fingerprint);
        tx.is_dev_buy = is_dev_buy;
        tx
    }

    fn sfd_buy_tx(
        signer: &str,
        signature: &str,
        pre_balance: Option<u64>,
        post_balance: Option<u64>,
    ) -> PoolTransaction {
        let mut tx = buy_tx(signer, signature, ToolchainFingerprintInput::default());
        tx.signer_pre_balance_lamports = pre_balance;
        tx.signer_post_balance_lamports = post_balance;
        tx.sol_amount_lamports = pre_balance
            .zip(post_balance)
            .and_then(|(pre, post)| pre.checked_sub(post));
        tx
    }

    fn sfd_buy_tx_with_amount(
        signer: &str,
        signature: &str,
        pre_balance: Option<u64>,
        post_balance: Option<u64>,
        sol_amount_lamports: Option<u64>,
        volume_sol: f64,
    ) -> PoolTransaction {
        let mut tx = sfd_buy_tx(signer, signature, pre_balance, post_balance);
        tx.sol_amount_lamports = sol_amount_lamports;
        tx.volume_sol = volume_sol;
        tx
    }

    fn des_buy_tx(
        signer: &str,
        signature: &str,
        slot: Option<u64>,
        event_ordinal: Option<u32>,
        v_sol: Option<f64>,
        v_tokens: Option<f64>,
    ) -> PoolTransaction {
        let mut tx = buy_tx(
            signer,
            signature,
            dbia_fingerprint(12, 3, true, true, 2, (0, 0)),
        );
        tx.slot = slot;
        tx.event_ordinal = event_ordinal;
        tx.v_sol_in_bonding_curve = v_sol;
        tx.v_tokens_in_bonding_curve = v_tokens;
        tx.market_cap_sol = match (v_sol, v_tokens) {
            (Some(sol), Some(tokens)) if tokens > 0.0 => Some((sol / tokens) * 1_000_000_000.0),
            _ => None,
        };
        tx.curve_data_known = v_sol.is_some() && v_tokens.is_some();
        tx.signer_pre_balance_lamports = Some(100);
        tx.signer_post_balance_lamports = Some(90);
        tx
    }

    fn assert_approx_eq(left: f64, right: f64) {
        assert!(
            (left - right).abs() <= 1e-9,
            "left={left} right={right} diff={}",
            (left - right).abs()
        );
    }

    #[test]
    fn sybil_metric_quality_config_from_gatekeeper_v2_config() {
        let config = GatekeeperV2Config {
            min_toolchain_metric_coverage: 0.81,
            min_des_valid_sequence_coverage: 0.82,
            cpv_min_observed_window_ratio: 0.99,
            fsc_require_clean_v2_for_actionability: false,
            fsc_require_coverage_window_for_actionability: false,
            ..GatekeeperV2Config::default()
        };

        let quality = SybilMetricQualityConfig::from_gatekeeper_config(&config);
        assert_eq!(quality.min_toolchain_metric_coverage, 0.81);
        assert_eq!(quality.min_des_valid_sequence_coverage, 0.82);
        assert_eq!(quality.cpv_min_observed_window_ratio, 0.99);
        assert!(!quality.fsc_require_clean_v2_for_actionability);
        assert!(!quality.fsc_require_coverage_window_for_actionability);
    }

    #[test]
    fn mixed_toolchain_yields_higher_ftdi_than_homogeneous_batch() {
        let homogeneous = vec![
            buy_tx("a", "sig-a", ftdi_fingerprint(Some((0, 0)))),
            buy_tx("b", "sig-b", ftdi_fingerprint(Some((0, 0)))),
            buy_tx("c", "sig-c", ftdi_fingerprint(Some((0, 0)))),
        ];
        let mixed = vec![
            buy_tx("a", "sig-a", ftdi_fingerprint(Some((0, 0)))),
            buy_tx("b", "sig-b", ftdi_fingerprint(Some((1, 0)))),
            buy_tx("c", "sig-c", ftdi_fingerprint(Some((2, 0)))),
        ];

        let homogeneous_ftdi = compute_ftdi(homogeneous.iter());
        let mixed_ftdi = compute_ftdi(mixed.iter());

        assert_eq!(
            homogeneous_ftdi.fee_topology_diversity_index,
            Some(1.0 / 3.0)
        );
        assert_eq!(mixed_ftdi.fee_topology_diversity_index, Some(1.0));
        assert!(
            mixed_ftdi.fee_topology_diversity_index.unwrap()
                > homogeneous_ftdi.fee_topology_diversity_index.unwrap()
        );
    }

    #[test]
    fn insufficient_buys_returns_none_and_reason() {
        let txs = vec![
            buy_tx("a", "sig-a", ftdi_fingerprint(Some((0, 0)))),
            buy_tx("b", "sig-b", ftdi_fingerprint(Some((1, 0)))),
        ];

        let result = compute_ftdi(txs.iter());

        assert_eq!(result.fee_topology_diversity_index, None);
        assert_eq!(
            result.degraded_reasons,
            vec![FTDI_INSUFFICIENT_BUYS_REASON.to_string()]
        );
        assert_eq!(result.buy_sample_count, 2);
        assert_eq!(result.signer_sample_count, 2);
    }

    #[test]
    fn missing_raw_fee_topology_returns_none_and_reason() {
        let txs = vec![
            buy_tx("a", "sig-a", ftdi_fingerprint(Some((0, 0)))),
            buy_tx("b", "sig-b", ftdi_fingerprint(None)),
            buy_tx("c", "sig-c", ftdi_fingerprint(Some((2, 0)))),
        ];

        let result = compute_ftdi(txs.iter());

        assert_eq!(result.fee_topology_diversity_index, None);
        assert_eq!(
            result.degraded_reasons,
            vec![FTDI_RAW_FEE_TOPOLOGY_UNAVAILABLE_REASON.to_string()]
        );
    }

    #[test]
    fn ftdi_uses_later_complete_toolchain_sample_for_same_signer() {
        let txs = vec![
            buy_tx("a", "sig-a-missing", ftdi_fingerprint(None)),
            buy_tx("a", "sig-a-complete", ftdi_fingerprint(Some((0, 0)))),
            buy_tx("b", "sig-b", ftdi_fingerprint(Some((1, 0)))),
            buy_tx("c", "sig-c", ftdi_fingerprint(Some((2, 0)))),
        ];

        let result = compute_ftdi(txs.iter());

        assert_eq!(result.fee_topology_diversity_index, Some(1.0));
        assert!(result.degraded_reasons.is_empty());
        assert_eq!(result.buy_sample_count, 4);
        assert_eq!(result.signer_sample_count, 3);
        assert_eq!(result.toolchain_fingerprint_coverage, Some(1.0));
    }

    #[test]
    fn ftdi_partial_fee_topology_coverage_materializes_above_threshold() {
        let txs = vec![
            buy_tx("a", "sig-a", ftdi_fingerprint(Some((0, 0)))),
            buy_tx("b", "sig-b", ftdi_fingerprint(Some((1, 0)))),
            buy_tx("c", "sig-c", ftdi_fingerprint(Some((2, 0)))),
            buy_tx("d", "sig-d", ftdi_fingerprint(Some((3, 0)))),
            buy_tx("e", "sig-e-missing", ftdi_fingerprint(None)),
        ];

        let result = compute_ftdi(txs.iter());

        assert_eq!(result.fee_topology_diversity_index, Some(1.0));
        assert_eq!(
            result.degraded_reasons,
            vec![FTDI_PARTIAL_FEE_TOPOLOGY_COVERAGE.to_string()]
        );
        assert_eq!(result.toolchain_fingerprint_coverage, Some(0.8));
    }

    #[test]
    fn dbia_requires_dev_buy_in_window() {
        let txs = vec![
            dbia_buy_tx(
                "buyer-a",
                "sig-a",
                false,
                dbia_fingerprint(12, 3, true, true, 2, (0, 0)),
            ),
            dbia_buy_tx(
                "buyer-b",
                "sig-b",
                false,
                dbia_fingerprint(12, 3, true, true, 2, (0, 0)),
            ),
            dbia_buy_tx(
                "buyer-c",
                "sig-c",
                false,
                dbia_fingerprint(14, 4, false, false, 3, (1, 0)),
            ),
        ];

        let result = compute_dbia(txs.iter(), Some("dev"));

        assert_eq!(result.dev_buyer_infrastructure_affinity, None);
        assert_eq!(
            result.degraded_reasons,
            vec![DBIA_NO_DEV_BUY_REASON.to_string()]
        );
    }

    #[test]
    fn dbia_requires_two_non_dev_buyers() {
        let txs = vec![
            dbia_buy_tx(
                "dev",
                "sig-dev",
                true,
                dbia_fingerprint(12, 3, true, true, 2, (0, 0)),
            ),
            dbia_buy_tx(
                "buyer-a",
                "sig-a",
                false,
                dbia_fingerprint(12, 3, true, true, 2, (0, 0)),
            ),
        ];

        let result = compute_dbia(txs.iter(), Some("dev"));

        assert_eq!(result.dev_buyer_infrastructure_affinity, None);
        assert_eq!(
            result.degraded_reasons,
            vec![DBIA_INSUFFICIENT_BUYERS_REASON.to_string()]
        );
    }

    #[test]
    fn dbia_identical_fingerprints_yield_similarity_one() {
        let shared = dbia_fingerprint(12, 3, true, true, 2, (0, 0));
        let txs = vec![
            dbia_buy_tx("dev", "sig-dev", true, shared.clone()),
            dbia_buy_tx("buyer-a", "sig-a", false, shared.clone()),
            dbia_buy_tx("buyer-b", "sig-b", false, shared),
        ];

        let result = compute_dbia(txs.iter(), None);

        assert_eq!(result.dev_buyer_infrastructure_affinity, Some(1.0));
        assert!(result.degraded_reasons.is_empty());
        assert_eq!(result.buy_sample_count, 3);
        assert_eq!(result.signer_sample_count, 3);
        assert_eq!(result.toolchain_fingerprint_coverage, Some(1.0));
    }

    #[test]
    fn dbia_uses_later_complete_toolchain_sample_for_same_signer() {
        let shared = dbia_fingerprint(12, 3, true, true, 2, (0, 0));
        let txs = vec![
            dbia_buy_tx("dev", "sig-dev", true, shared.clone()),
            dbia_buy_tx(
                "buyer-a",
                "sig-a-missing",
                false,
                ToolchainFingerprintInput::default(),
            ),
            dbia_buy_tx("buyer-a", "sig-a-complete", false, shared.clone()),
            dbia_buy_tx("buyer-b", "sig-b", false, shared),
        ];

        let result = compute_dbia(txs.iter(), Some("dev"));

        assert_eq!(result.dev_buyer_infrastructure_affinity, Some(1.0));
        assert!(result.degraded_reasons.is_empty());
        assert_eq!(result.buy_sample_count, 4);
        assert_eq!(result.signer_sample_count, 3);
        assert_eq!(result.toolchain_fingerprint_coverage, Some(1.0));
    }

    #[test]
    fn dbia_uses_later_complete_dev_toolchain_sample() {
        let shared = dbia_fingerprint(12, 3, true, true, 2, (0, 0));
        let txs = vec![
            dbia_buy_tx(
                "dev",
                "sig-dev-missing",
                true,
                ToolchainFingerprintInput::default(),
            ),
            dbia_buy_tx("dev", "sig-dev-complete", true, shared.clone()),
            dbia_buy_tx("buyer-a", "sig-a", false, shared.clone()),
            dbia_buy_tx("buyer-b", "sig-b", false, shared),
        ];

        let result = compute_dbia(txs.iter(), Some("dev"));

        assert_eq!(result.dev_buyer_infrastructure_affinity, Some(1.0));
        assert!(result.degraded_reasons.is_empty());
        assert_eq!(result.buy_sample_count, 4);
        assert_eq!(result.signer_sample_count, 3);
        assert_eq!(result.toolchain_fingerprint_coverage, Some(1.0));
    }

    #[test]
    fn dbia_partial_fingerprint_coverage_materializes_above_threshold() {
        let shared = dbia_fingerprint(12, 3, true, true, 2, (0, 0));
        let txs = vec![
            dbia_buy_tx("dev", "sig-dev", true, shared.clone()),
            dbia_buy_tx("buyer-a", "sig-a", false, shared.clone()),
            dbia_buy_tx("buyer-b", "sig-b", false, shared.clone()),
            dbia_buy_tx("buyer-c", "sig-c", false, shared),
            dbia_buy_tx(
                "buyer-d",
                "sig-d-missing",
                false,
                ToolchainFingerprintInput::default(),
            ),
        ];

        let result = compute_dbia(txs.iter(), Some("dev"));

        assert_eq!(result.dev_buyer_infrastructure_affinity, Some(1.0));
        assert_eq!(
            result.degraded_reasons,
            vec![DBIA_PARTIAL_FINGERPRINT_COVERAGE.to_string()]
        );
        assert_eq!(result.toolchain_fingerprint_coverage, Some(0.8));
    }

    #[test]
    fn dbia_small_numeric_delta_uses_scaled_distance() {
        let txs = vec![
            dbia_buy_tx(
                "dev",
                "sig-dev",
                true,
                dbia_fingerprint(12, 7, true, true, 2, (0, 0)),
            ),
            dbia_buy_tx(
                "buyer-a",
                "sig-a",
                false,
                dbia_fingerprint(12, 8, true, true, 2, (0, 0)),
            ),
            dbia_buy_tx(
                "buyer-b",
                "sig-b",
                false,
                dbia_fingerprint(12, 8, true, true, 2, (0, 0)),
            ),
        ];

        let result = compute_dbia(txs.iter(), Some("dev"));

        assert_approx_eq(result.dev_buyer_infrastructure_affinity.unwrap(), 0.9375);
        assert!(result.degraded_reasons.is_empty());
    }

    #[test]
    fn dbia_distinct_fingerprints_yield_similarity_zero() {
        let txs = vec![
            dbia_buy_tx(
                "dev",
                "sig-dev",
                true,
                dbia_fingerprint(12, 3, true, true, 2, (0, 0)),
            ),
            dbia_buy_tx(
                "buyer-a",
                "sig-a",
                false,
                dbia_fingerprint(30, 10, false, false, 8, (3, 3)),
            ),
            dbia_buy_tx(
                "buyer-b",
                "sig-b",
                false,
                dbia_fingerprint(28, 9, false, false, 7, (4, 4)),
            ),
        ];

        let result = compute_dbia(txs.iter(), Some("dev"));

        assert_eq!(result.dev_buyer_infrastructure_affinity, Some(0.0));
        assert!(result.degraded_reasons.is_empty());
    }

    #[test]
    fn dbia_missing_raw_fingerprint_returns_none_and_reason() {
        let txs = vec![
            dbia_buy_tx(
                "dev",
                "sig-dev",
                true,
                dbia_fingerprint(12, 3, true, true, 2, (0, 0)),
            ),
            dbia_buy_tx(
                "buyer-a",
                "sig-a",
                false,
                ToolchainFingerprintInput::default(),
            ),
            dbia_buy_tx(
                "buyer-b",
                "sig-b",
                false,
                dbia_fingerprint(12, 3, true, true, 2, (0, 0)),
            ),
        ];

        let result = compute_dbia(txs.iter(), Some("dev"));

        assert_eq!(result.dev_buyer_infrastructure_affinity, None);
        assert_eq!(
            result.degraded_reasons,
            vec![DBIA_RAW_FINGERPRINT_UNAVAILABLE_REASON.to_string()]
        );
        assert_approx_eq(result.toolchain_fingerprint_coverage.unwrap(), 2.0 / 3.0);
    }

    #[test]
    fn sfd_cabal_example_yields_low_mad() {
        let txs = vec![
            sfd_buy_tx("a", "sig-a", Some(100), Some(12)),
            sfd_buy_tx("b", "sig-b", Some(100), Some(9)),
            sfd_buy_tx("c", "sig-c", Some(100), Some(14)),
            sfd_buy_tx("d", "sig-d", Some(100), Some(7)),
            sfd_buy_tx("e", "sig-e", Some(100), Some(11)),
        ];

        let result = compute_sfd(txs.iter());

        assert!(result.degraded_reasons.is_empty());
        assert_eq!(result.buy_sample_count, 5);
        assert_eq!(result.signer_sample_count, 5);
        assert!(result.spend_fraction_divergence.unwrap() < 0.05);
    }

    #[test]
    fn sfd_organic_example_yields_high_mad() {
        let txs = vec![
            sfd_buy_tx("a", "sig-a", Some(100), Some(99)),
            sfd_buy_tx("b", "sig-b", Some(100), Some(17)),
            sfd_buy_tx("c", "sig-c", Some(100), Some(80)),
            sfd_buy_tx("d", "sig-d", Some(100), Some(55)),
            sfd_buy_tx("e", "sig-e", Some(100), Some(38)),
        ];

        let result = compute_sfd(txs.iter());

        assert!(result.degraded_reasons.is_empty());
        assert!(result.spend_fraction_divergence.unwrap() > 0.15);
    }

    #[test]
    fn sfd_zero_prebalance_is_skipped_without_panicking() {
        let txs = vec![
            sfd_buy_tx("a", "sig-a", Some(0), Some(0)),
            sfd_buy_tx("b", "sig-b", Some(100), Some(10)),
            sfd_buy_tx("c", "sig-c", Some(100), Some(10)),
            sfd_buy_tx("d", "sig-d", Some(100), Some(10)),
        ];

        let result = compute_sfd(txs.iter());

        assert_eq!(result.spend_fraction_divergence, Some(0.0));
        assert_eq!(
            result.degraded_reasons,
            vec![SFD_ZERO_PREBALANCE_SKIPPED_REASON.to_string()]
        );
    }

    #[test]
    fn sfd_missing_postbalance_returns_none_and_reason() {
        let txs = vec![
            sfd_buy_tx("a", "sig-a", Some(100), Some(10)),
            sfd_buy_tx("b", "sig-b", Some(100), None),
            sfd_buy_tx("c", "sig-c", Some(100), Some(20)),
        ];

        let result = compute_sfd(txs.iter());

        assert_eq!(result.spend_fraction_divergence, None);
        assert_eq!(
            result.degraded_reasons,
            vec![
                SFD_BUY_AMOUNT_UNAVAILABLE.to_string(),
                SFD_POSTBALANCE_UNAVAILABLE_REASON.to_string(),
                SFD_INSUFFICIENT_BUYS_REASON.to_string()
            ]
        );
    }

    #[test]
    fn sfd_prefers_best_balance_sample_per_signer() {
        let txs = vec![
            sfd_buy_tx("a", "sig-a-missing", Some(100), None),
            sfd_buy_tx("a", "sig-a-complete", Some(100), Some(10)),
            sfd_buy_tx("b", "sig-b", Some(100), Some(10)),
            sfd_buy_tx("c", "sig-c", Some(100), Some(10)),
        ];

        let result = compute_sfd(txs.iter());

        assert_eq!(result.spend_fraction_divergence, Some(0.0));
        assert!(result.degraded_reasons.is_empty());
    }

    #[test]
    fn sfd_partial_balance_coverage_still_materializes_when_three_usable_samples_remain() {
        let txs = vec![
            sfd_buy_tx("a", "sig-a", Some(100), Some(10)),
            sfd_buy_tx("b", "sig-b", Some(100), Some(10)),
            sfd_buy_tx("c", "sig-c", Some(100), Some(10)),
            sfd_buy_tx("d", "sig-d", Some(100), None),
        ];

        let result = compute_sfd(txs.iter());

        assert_eq!(result.spend_fraction_divergence, Some(0.0));
        assert_eq!(
            result.degraded_reasons,
            vec![
                SFD_BUY_AMOUNT_UNAVAILABLE.to_string(),
                SFD_PARTIAL_BALANCE_COVERAGE_REASON.to_string()
            ]
        );
    }

    #[test]
    fn sfd_weighted_mad_is_not_dominated_by_dust_spam() {
        let txs = vec![
            sfd_buy_tx_with_amount(
                "dust-a",
                "sig-dust-a",
                Some(1_000),
                Some(999),
                Some(1),
                0.01,
            ),
            sfd_buy_tx_with_amount(
                "dust-b",
                "sig-dust-b",
                Some(1_000),
                Some(999),
                Some(1),
                0.01,
            ),
            sfd_buy_tx_with_amount(
                "dust-c",
                "sig-dust-c",
                Some(1_000),
                Some(999),
                Some(1),
                0.01,
            ),
            sfd_buy_tx_with_amount(
                "dust-d",
                "sig-dust-d",
                Some(1_000),
                Some(999),
                Some(1),
                0.01,
            ),
            sfd_buy_tx_with_amount(
                "dust-e",
                "sig-dust-e",
                Some(1_000),
                Some(999),
                Some(1),
                0.01,
            ),
            sfd_buy_tx_with_amount(
                "dust-f",
                "sig-dust-f",
                Some(1_000),
                Some(999),
                Some(1),
                0.01,
            ),
            sfd_buy_tx_with_amount(
                "large-a",
                "sig-large-a",
                Some(1_000),
                Some(600),
                Some(400),
                1.0,
            ),
            sfd_buy_tx_with_amount(
                "large-b",
                "sig-large-b",
                Some(1_000),
                Some(400),
                Some(600),
                1.0,
            ),
            sfd_buy_tx_with_amount(
                "large-c",
                "sig-large-c",
                Some(1_000),
                Some(200),
                Some(800),
                1.0,
            ),
        ];

        let result = compute_sfd(txs.iter());

        assert!(result.degraded_reasons.is_empty());
        assert!(
            result.spend_fraction_divergence.unwrap() > 0.15,
            "weighted SFD should stay controlled by economically meaningful buys"
        );
    }

    #[test]
    fn sfd_negative_balance_delta_is_skipped_instead_of_zero_spend() {
        let txs = vec![
            sfd_buy_tx_with_amount("a", "sig-a", Some(100), Some(110), Some(10), 0.1),
            sfd_buy_tx("b", "sig-b", Some(100), Some(90)),
            sfd_buy_tx("c", "sig-c", Some(100), Some(80)),
            sfd_buy_tx("d", "sig-d", Some(100), Some(70)),
        ];

        let result = compute_sfd(txs.iter());

        assert!(result.spend_fraction_divergence.is_some());
        assert!(result
            .degraded_reasons
            .contains(&SFD_NEGATIVE_BALANCE_DELTA_SKIPPED.to_string()));
    }

    #[test]
    fn sfd_missing_buy_amount_falls_back_to_balance_delta_with_reason() {
        let txs = vec![
            sfd_buy_tx_with_amount("a", "sig-a", Some(100), Some(90), None, 0.1),
            sfd_buy_tx_with_amount("b", "sig-b", Some(100), Some(80), None, 0.2),
            sfd_buy_tx_with_amount("c", "sig-c", Some(100), Some(60), None, 0.4),
        ];

        let result = compute_sfd(txs.iter());

        assert!(result.spend_fraction_divergence.is_some());
        assert_eq!(
            result.degraded_reasons,
            vec![SFD_BUY_AMOUNT_UNAVAILABLE.to_string()]
        );
    }

    #[test]
    fn des_increasing_price_impacts_with_longer_pauses_yield_positive_tau() {
        let txs = vec![
            des_buy_tx("a", "sig-a", Some(1), Some(0), Some(10.0), Some(1.0)),
            des_buy_tx("b", "sig-b", Some(2), Some(0), Some(11.0), Some(1.0)),
            des_buy_tx("c", "sig-c", Some(4), Some(0), Some(13.2), Some(1.0)),
            des_buy_tx("d", "sig-d", Some(7), Some(0), Some(17.16), Some(1.0)),
        ];

        let result = compute_des(txs.iter());

        assert_eq!(result.buy_sample_count, 4);
        assert_eq!(result.signer_sample_count, 4);
        assert!(result.degraded_reasons.is_empty());
        assert_approx_eq(result.demand_elasticity_score.unwrap(), 1.0);
    }

    #[test]
    fn des_independent_price_impacts_and_timing_yield_neutral_tau() {
        let txs = vec![
            des_buy_tx("a", "sig-a", Some(1), Some(0), Some(10.0), Some(1.0)),
            des_buy_tx("b", "sig-b", Some(2), Some(0), Some(11.0), Some(1.0)),
            des_buy_tx("c", "sig-c", Some(4), Some(0), Some(13.2), Some(1.0)),
            des_buy_tx("d", "sig-d", Some(7), Some(0), Some(17.16), Some(1.0)),
            des_buy_tx("e", "sig-e", Some(7), Some(1), Some(24.024), Some(1.0)),
        ];

        let result = compute_des(txs.iter());

        assert!(result.degraded_reasons.is_empty());
        assert_approx_eq(result.demand_elasticity_score.unwrap(), 0.0);
    }

    #[test]
    fn des_same_slot_ordering_is_deterministic_when_event_ordinal_exists() {
        let ordered = vec![
            des_buy_tx("a", "sig-a", Some(1), Some(0), Some(10.0), Some(1.0)),
            des_buy_tx("b", "sig-b", Some(1), Some(1), Some(11.0), Some(1.0)),
            des_buy_tx("c", "sig-c", Some(2), Some(0), Some(13.2), Some(1.0)),
            des_buy_tx("d", "sig-d", Some(2), Some(1), Some(17.16), Some(1.0)),
            des_buy_tx("e", "sig-e", Some(4), Some(0), Some(24.024), Some(1.0)),
        ];
        let permuted = vec![
            ordered[3].clone(),
            ordered[0].clone(),
            ordered[4].clone(),
            ordered[1].clone(),
            ordered[2].clone(),
        ];

        let ordered_result = compute_des(ordered.iter());
        let permuted_result = compute_des(permuted.iter());

        assert_eq!(ordered_result.degraded_reasons, Vec::<String>::new());
        assert_eq!(
            ordered_result.demand_elasticity_score,
            permuted_result.demand_elasticity_score
        );
        let score = ordered_result.demand_elasticity_score.unwrap();
        assert!(score > 0.5);
        assert!(score < 0.6);
    }

    #[test]
    fn des_same_slot_fallback_uses_stable_buffer_order() {
        let txs = vec![
            des_buy_tx("a", "sig-a", Some(1), None, Some(10.0), Some(1.0)),
            des_buy_tx("b", "sig-b", Some(1), None, Some(11.0), Some(1.0)),
            des_buy_tx("c", "sig-c", Some(1), None, Some(13.2), Some(1.0)),
            des_buy_tx("d", "sig-d", Some(2), None, Some(17.16), Some(1.0)),
        ];

        let result = compute_des(txs.iter());

        assert!(result.degraded_reasons.is_empty());
        assert!(result.demand_elasticity_score.unwrap() > 0.8);
    }

    #[test]
    fn des_partial_sequence_coverage_materializes_from_longest_valid_segment() {
        let txs = vec![
            des_buy_tx("invalid", "sig-invalid", Some(1), Some(0), None, Some(1.0)),
            des_buy_tx("a", "sig-a", Some(2), Some(0), Some(10.0), Some(1.0)),
            des_buy_tx("b", "sig-b", Some(3), Some(0), Some(11.0), Some(1.0)),
            des_buy_tx("c", "sig-c", Some(5), Some(0), Some(13.2), Some(1.0)),
            des_buy_tx("d", "sig-d", Some(8), Some(0), Some(17.16), Some(1.0)),
        ];

        let result = compute_des(txs.iter());

        assert_approx_eq(result.demand_elasticity_score.unwrap(), 1.0);
        assert_eq!(
            result.degraded_reasons,
            vec![DES_PARTIAL_SEQUENCE_COVERAGE.to_string()]
        );
    }

    #[test]
    fn des_invalid_sample_without_valid_segment_returns_none() {
        let txs = vec![
            des_buy_tx("a", "sig-a", Some(1), Some(0), Some(10.0), Some(1.0)),
            des_buy_tx("b", "sig-b", Some(2), Some(0), Some(11.0), Some(1.0)),
            des_buy_tx("invalid", "sig-invalid", Some(3), Some(0), None, Some(1.0)),
            des_buy_tx("c", "sig-c", Some(4), Some(0), Some(13.2), Some(1.0)),
            des_buy_tx("d", "sig-d", Some(7), Some(0), Some(17.16), Some(1.0)),
        ];

        let result = compute_des(txs.iter());

        assert_eq!(result.demand_elasticity_score, None);
        assert_eq!(
            result.degraded_reasons,
            vec![DES_CURVE_DATA_UNAVAILABLE_REASON.to_string()]
        );
    }

    #[test]
    fn des_ties_without_comparable_pairs_return_none() {
        let txs = vec![
            des_buy_tx("a", "sig-a", Some(1), Some(0), Some(10.0), Some(1.0)),
            des_buy_tx("b", "sig-b", Some(2), Some(0), Some(11.0), Some(1.0)),
            des_buy_tx("c", "sig-c", Some(3), Some(0), Some(12.1), Some(1.0)),
            des_buy_tx("d", "sig-d", Some(4), Some(0), Some(13.31), Some(1.0)),
        ];

        let result = compute_des(txs.iter());

        assert_eq!(result.demand_elasticity_score, None);
        assert_eq!(
            result.degraded_reasons,
            vec![DES_NO_COMPARABLE_PAIRS.to_string()]
        );
    }

    #[test]
    fn des_missing_curve_data_returns_none_and_reason() {
        let txs = vec![
            des_buy_tx("a", "sig-a", Some(1), Some(0), Some(10.0), Some(1.0)),
            des_buy_tx("b", "sig-b", Some(2), Some(0), Some(11.0), Some(1.0)),
            des_buy_tx("c", "sig-c", Some(4), Some(0), None, Some(1.0)),
            des_buy_tx("d", "sig-d", Some(7), Some(0), Some(17.16), Some(1.0)),
        ];

        let result = compute_des(txs.iter());

        assert_eq!(result.demand_elasticity_score, None);
        assert_eq!(
            result.degraded_reasons,
            vec![DES_CURVE_DATA_UNAVAILABLE_REASON.to_string()]
        );
    }

    #[test]
    fn des_missing_slot_returns_none_and_reason() {
        let txs = vec![
            des_buy_tx("a", "sig-a", Some(1), Some(0), Some(10.0), Some(1.0)),
            des_buy_tx("b", "sig-b", None, Some(0), Some(11.0), Some(1.0)),
            des_buy_tx("c", "sig-c", Some(4), Some(0), Some(13.2), Some(1.0)),
            des_buy_tx("d", "sig-d", Some(7), Some(0), Some(17.16), Some(1.0)),
        ];

        let result = compute_des(txs.iter());

        assert_eq!(result.demand_elasticity_score, None);
        assert_eq!(
            result.degraded_reasons,
            vec![DES_SLOT_ORDER_UNAVAILABLE_REASON.to_string()]
        );
    }
}
