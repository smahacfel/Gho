use crate::events::PoolTransaction;
use ghost_core::tx_intelligence::types::{
    SybilResistanceFeatures, DBIA_INSUFFICIENT_BUYERS_REASON, DBIA_NO_DEV_BUY_REASON,
    DBIA_RAW_FINGERPRINT_UNAVAILABLE_REASON, DES_CURVE_DATA_UNAVAILABLE_REASON,
    DES_INSUFFICIENT_BUYS_REASON, DES_SLOT_ORDER_UNAVAILABLE_REASON, FTDI_INSUFFICIENT_BUYS_REASON,
    FTDI_RAW_FEE_TOPOLOGY_UNAVAILABLE_REASON, SFD_INSUFFICIENT_BUYS_REASON,
    SFD_PARTIAL_BALANCE_COVERAGE_REASON, SFD_POSTBALANCE_UNAVAILABLE_REASON,
    SFD_ZERO_PREBALANCE_SKIPPED_REASON,
};
use seer::types::ToolchainFingerprintInput;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};

const DBIA_ACCOUNT_KEYS_WEIGHT: f64 = 0.20;
const DBIA_OUTER_INSTRUCTION_WEIGHT: f64 = 0.25;
const DBIA_CU_LIMIT_WEIGHT: f64 = 0.05;
const DBIA_CU_PRICE_WEIGHT: f64 = 0.05;
const DBIA_INNER_GROUP_WEIGHT: f64 = 0.25;
const DBIA_FEE_TOPOLOGY_WEIGHT: f64 = 0.20;
const DES_SIGN_EPSILON: f64 = 1e-12;

#[derive(Debug, Clone, PartialEq)]
pub struct FtdiComputation {
    pub fee_topology_diversity_index: Option<f64>,
    pub degraded_reasons: Vec<String>,
    pub buy_sample_count: u64,
    pub signer_sample_count: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DbiaComputation {
    pub dev_buyer_infrastructure_affinity: Option<f64>,
    pub degraded_reasons: Vec<String>,
    pub buy_sample_count: u64,
    pub signer_sample_count: u64,
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
    Complete,
}

#[derive(Debug, Clone, Copy)]
struct SelectedSfdSample<'a> {
    tx: &'a PoolTransaction,
    coverage: SfdSampleCoverage,
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
        if self.account_keys_len != other.account_keys_len {
            distance += DBIA_ACCOUNT_KEYS_WEIGHT;
        }
        if self.outer_instruction_count != other.outer_instruction_count {
            distance += DBIA_OUTER_INSTRUCTION_WEIGHT;
        }
        if self.has_set_compute_unit_limit != other.has_set_compute_unit_limit {
            distance += DBIA_CU_LIMIT_WEIGHT;
        }
        if self.has_set_compute_unit_price != other.has_set_compute_unit_price {
            distance += DBIA_CU_PRICE_WEIGHT;
        }
        if self.inner_instruction_group_count != other.inner_instruction_group_count {
            distance += DBIA_INNER_GROUP_WEIGHT;
        }
        if self.fee_topology != other.fee_topology {
            distance += DBIA_FEE_TOPOLOGY_WEIGHT;
        }
        1.0 - distance
    }
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

fn unique_buyer_samples<'a>(buy_txs: &[&'a PoolTransaction]) -> Vec<&'a PoolTransaction> {
    let mut seen_signers = HashSet::<&str>::new();
    let mut unique_samples = Vec::new();
    for &tx in buy_txs {
        if seen_signers.insert(tx.signer.as_str()) {
            unique_samples.push(tx);
        }
    }
    unique_samples
}

fn sfd_sample_coverage(tx: &PoolTransaction) -> SfdSampleCoverage {
    match tx.signer_pre_balance_lamports {
        Some(0) => SfdSampleCoverage::ZeroPreBalance,
        Some(_) if tx.signer_post_balance_lamports.is_some() => SfdSampleCoverage::Complete,
        _ => SfdSampleCoverage::MissingRequiredBalance,
    }
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

fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        Some((sorted[mid - 1] + sorted[mid]) / 2.0)
    } else {
        Some(sorted[mid])
    }
}

fn ordered_buy_samples<'a>(buy_samples: &[SequencedBuyTx<'a>]) -> Option<Vec<OrderedBuyTx<'a>>> {
    let mut by_slot = BTreeMap::<u64, Vec<SequencedBuyTx<'a>>>::new();
    for sample in buy_samples {
        let slot = sample.tx.slot?;
        by_slot.entry(slot).or_default().push(*sample);
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

    Some(ordered)
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

fn kendall_tau(x_values: &[f64], y_values: &[f64]) -> f64 {
    let mut concordant = 0u64;
    let mut discordant = 0u64;

    for i in 0..x_values.len() {
        for k in (i + 1)..x_values.len() {
            let sign = (x_values[i] - x_values[k]) * (y_values[i] - y_values[k]);
            if sign > DES_SIGN_EPSILON {
                concordant += 1;
            } else if sign < -DES_SIGN_EPSILON {
                discordant += 1;
            }
        }
    }

    let comparable_pairs = concordant + discordant;
    if comparable_pairs == 0 {
        0.0
    } else {
        (concordant as f64 - discordant as f64) / comparable_pairs as f64
    }
}

pub fn compute_ftdi<'a>(
    transactions: impl IntoIterator<Item = &'a PoolTransaction>,
) -> FtdiComputation {
    let buy_txs = successful_buy_txs(transactions);
    compute_ftdi_from_buys(&buy_txs)
}

fn compute_ftdi_from_buys(buy_txs: &[&PoolTransaction]) -> FtdiComputation {
    let stats = buy_sample_stats(buy_txs);
    if stats.buy_sample_count < 3 {
        return FtdiComputation {
            fee_topology_diversity_index: None,
            degraded_reasons: vec![FTDI_INSUFFICIENT_BUYS_REASON.to_string()],
            buy_sample_count: stats.buy_sample_count,
            signer_sample_count: stats.signer_sample_count,
        };
    }

    let unique_samples = unique_buyer_samples(buy_txs);
    let mut unique_topologies = HashSet::<FeeTopology>::new();
    for tx in &unique_samples {
        let Some((external_fee_count, internal_fee_count)) =
            tx.toolchain_fingerprint.fee_topology()
        else {
            return FtdiComputation {
                fee_topology_diversity_index: None,
                degraded_reasons: vec![FTDI_RAW_FEE_TOPOLOGY_UNAVAILABLE_REASON.to_string()],
                buy_sample_count: stats.buy_sample_count,
                signer_sample_count: stats.signer_sample_count,
            };
        };

        unique_topologies.insert(FeeTopology {
            external_fee_count,
            internal_fee_count,
        });
    }

    FtdiComputation {
        fee_topology_diversity_index: (!unique_samples.is_empty())
            .then(|| unique_topologies.len() as f64 / unique_samples.len() as f64),
        degraded_reasons: Vec::new(),
        buy_sample_count: stats.buy_sample_count,
        signer_sample_count: stats.signer_sample_count,
    }
}

pub fn compute_dbia<'a>(
    transactions: impl IntoIterator<Item = &'a PoolTransaction>,
    dev_wallet: Option<&'a str>,
) -> DbiaComputation {
    let buy_txs = successful_buy_txs(transactions);
    compute_dbia_from_buys(&buy_txs, dev_wallet)
}

fn compute_dbia_from_buys<'a>(
    buy_txs: &[&'a PoolTransaction],
    dev_wallet: Option<&'a str>,
) -> DbiaComputation {
    let stats = buy_sample_stats(buy_txs);
    let unique_samples = unique_buyer_samples(buy_txs);
    let Some(dev_wallet) = resolve_dev_wallet(&unique_samples, dev_wallet) else {
        return DbiaComputation {
            dev_buyer_infrastructure_affinity: None,
            degraded_reasons: vec![DBIA_NO_DEV_BUY_REASON.to_string()],
            buy_sample_count: stats.buy_sample_count,
            signer_sample_count: stats.signer_sample_count,
        };
    };
    let Some(dev_tx) = unique_samples
        .iter()
        .copied()
        .find(|tx| tx.signer == dev_wallet)
    else {
        return DbiaComputation {
            dev_buyer_infrastructure_affinity: None,
            degraded_reasons: vec![DBIA_NO_DEV_BUY_REASON.to_string()],
            buy_sample_count: stats.buy_sample_count,
            signer_sample_count: stats.signer_sample_count,
        };
    };
    let Some(dev_fp) = InfrastructureFingerprint::from_input(&dev_tx.toolchain_fingerprint) else {
        return DbiaComputation {
            dev_buyer_infrastructure_affinity: None,
            degraded_reasons: vec![DBIA_RAW_FINGERPRINT_UNAVAILABLE_REASON.to_string()],
            buy_sample_count: stats.buy_sample_count,
            signer_sample_count: stats.signer_sample_count,
        };
    };

    let buyer_txs: Vec<&PoolTransaction> = unique_samples
        .into_iter()
        .filter(|tx| tx.signer != dev_wallet)
        .collect();
    if buyer_txs.len() < 2 {
        return DbiaComputation {
            dev_buyer_infrastructure_affinity: None,
            degraded_reasons: vec![DBIA_INSUFFICIENT_BUYERS_REASON.to_string()],
            buy_sample_count: stats.buy_sample_count,
            signer_sample_count: stats.signer_sample_count,
        };
    }

    let mut similarity_sum = 0.0;
    for tx in buyer_txs.iter().copied() {
        let Some(fingerprint) = InfrastructureFingerprint::from_input(&tx.toolchain_fingerprint)
        else {
            return DbiaComputation {
                dev_buyer_infrastructure_affinity: None,
                degraded_reasons: vec![DBIA_RAW_FINGERPRINT_UNAVAILABLE_REASON.to_string()],
                buy_sample_count: stats.buy_sample_count,
                signer_sample_count: stats.signer_sample_count,
            };
        };
        similarity_sum += dev_fp.similarity(&fingerprint);
    }

    DbiaComputation {
        dev_buyer_infrastructure_affinity: Some(similarity_sum / buyer_txs.len() as f64),
        degraded_reasons: Vec::new(),
        buy_sample_count: stats.buy_sample_count,
        signer_sample_count: stats.signer_sample_count,
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
    let mut spend_fractions = Vec::<f64>::new();

    for tx in &unique_samples {
        let Some(pre_balance) = tx.signer_pre_balance_lamports else {
            partial_balance_coverage = true;
            continue;
        };
        if pre_balance == 0 {
            zero_prebalance_skipped = true;
            continue;
        }
        let Some(post_balance) = tx.signer_post_balance_lamports else {
            partial_balance_coverage = true;
            continue;
        };

        let spent_lamports = pre_balance.saturating_sub(post_balance);
        spend_fractions.push(spent_lamports as f64 / pre_balance as f64);
    }

    if spend_fractions.len() < 3 {
        let mut reasons = Vec::new();
        if zero_prebalance_skipped {
            reasons.push(SFD_ZERO_PREBALANCE_SKIPPED_REASON.to_string());
        }
        if partial_balance_coverage {
            reasons.push(SFD_POSTBALANCE_UNAVAILABLE_REASON.to_string());
        }
        reasons.push(SFD_INSUFFICIENT_BUYS_REASON.to_string());
        return SfdComputation {
            spend_fraction_divergence: None,
            degraded_reasons: reasons,
            buy_sample_count: stats.buy_sample_count,
            signer_sample_count: stats.signer_sample_count,
        };
    }

    let median_fraction = median(&spend_fractions).expect("non-empty spend fractions");
    let deviations: Vec<f64> = spend_fractions
        .iter()
        .map(|value| (value - median_fraction).abs())
        .collect();
    let spend_fraction_divergence = median(&deviations);

    let mut degraded_reasons = Vec::new();
    if zero_prebalance_skipped {
        degraded_reasons.push(SFD_ZERO_PREBALANCE_SKIPPED_REASON.to_string());
    }
    if partial_balance_coverage {
        degraded_reasons.push(SFD_PARTIAL_BALANCE_COVERAGE_REASON.to_string());
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
    compute_des_from_transactions(&transactions)
}

fn compute_des_from_transactions(transactions: &[&PoolTransaction]) -> DesComputation {
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

    let Some(ordered_buy_txs) = ordered_buy_samples(&buy_samples) else {
        return DesComputation {
            demand_elasticity_score: None,
            degraded_reasons: vec![DES_SLOT_ORDER_UNAVAILABLE_REASON.to_string()],
            buy_sample_count: stats.buy_sample_count,
            signer_sample_count: stats.signer_sample_count,
        };
    };

    let mut prices = Vec::<f64>::with_capacity(ordered_buy_txs.len());
    for sample in &ordered_buy_txs {
        let Some(price) = curve_price(sample.tx) else {
            return DesComputation {
                demand_elasticity_score: None,
                degraded_reasons: vec![DES_CURVE_DATA_UNAVAILABLE_REASON.to_string()],
                buy_sample_count: stats.buy_sample_count,
                signer_sample_count: stats.signer_sample_count,
            };
        };
        prices.push(price);
    }

    let mut price_impacts = Vec::<f64>::with_capacity(ordered_buy_txs.len().saturating_sub(1));
    let mut timing_deltas = Vec::<f64>::with_capacity(ordered_buy_txs.len().saturating_sub(1));
    for index in 1..ordered_buy_txs.len() {
        let previous = ordered_buy_txs[index - 1];
        let current = ordered_buy_txs[index];
        price_impacts.push((prices[index] - prices[index - 1]) / prices[index - 1]);
        timing_deltas.push(inter_buy_delta(previous, current));
    }

    DesComputation {
        demand_elasticity_score: Some(kendall_tau(&price_impacts, &timing_deltas)),
        degraded_reasons: Vec::new(),
        buy_sample_count: stats.buy_sample_count,
        signer_sample_count: stats.signer_sample_count,
    }
}

pub fn compute_sybil_resistance<'a>(
    transactions: impl IntoIterator<Item = &'a PoolTransaction>,
    dev_wallet: Option<&'a str>,
) -> SybilResistanceFeatures {
    let transactions: Vec<&PoolTransaction> = transactions.into_iter().collect();
    let buy_txs = successful_buy_txs(transactions.iter().copied());
    let ftdi = compute_ftdi_from_buys(&buy_txs);
    let dbia = compute_dbia_from_buys(&buy_txs, dev_wallet);
    let sfd = compute_sfd_from_buys(&buy_txs);
    let des = compute_des_from_transactions(&transactions);

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
                dbia_fingerprint(20, 6, false, false, 5, (2, 1)),
            ),
            dbia_buy_tx(
                "buyer-b",
                "sig-b",
                false,
                dbia_fingerprint(18, 5, false, false, 4, (3, 1)),
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
        assert_approx_eq(result.spend_fraction_divergence.unwrap(), 0.02);
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
        assert_approx_eq(result.spend_fraction_divergence.unwrap(), 0.25);
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
            vec![SFD_PARTIAL_BALANCE_COVERAGE_REASON.to_string()]
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
        assert_approx_eq(ordered_result.demand_elasticity_score.unwrap(), 0.6);
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
        assert_approx_eq(result.demand_elasticity_score.unwrap(), 1.0);
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
