use crate::events::PoolTransaction;
pub use ghost_core::tx_intelligence::types::DesEvidenceV2 as DesComputation;
use ghost_core::tx_intelligence::types::{
    DesDefinitionV2, DesIntervalUnitV2, DesPriceSourceV2,
    DES_COMPARISON_DEFINITION_MISMATCH_REASON, DES_INSUFFICIENT_TRIPLES_REASON,
    DES_NO_COMPARABLE_PAIRS_REASON, DES_PRICE_DOMAIN_MISMATCH_REASON,
};
use ghost_core::tx_intelligence::types::{
    SybilResistanceFeatures, DBIA_INSUFFICIENT_BUYERS_REASON, DBIA_NO_DEV_BUY_REASON,
    DBIA_RAW_FINGERPRINT_UNAVAILABLE_REASON, DES_CURVE_DATA_UNAVAILABLE_REASON,
    DES_INSUFFICIENT_BUYS_REASON, DES_SLOT_ORDER_UNAVAILABLE_REASON, FTDI_INSUFFICIENT_BUYS_REASON,
    FTDI_RAW_FEE_TOPOLOGY_UNAVAILABLE_REASON, SFD_INSUFFICIENT_BUYS_REASON,
    SFD_INVALID_BALANCE_PAIR_REASON, SFD_PARTIAL_BALANCE_COVERAGE_REASON,
    SFD_POSTBALANCE_UNAVAILABLE_REASON, SFD_ZERO_PREBALANCE_SKIPPED_REASON,
};
use seer::types::ToolchainFingerprintInput;
use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};

const DBIA_ACCOUNT_KEYS_WEIGHT: f64 = 0.20;
const DBIA_OUTER_INSTRUCTION_WEIGHT: f64 = 0.25;
const DBIA_CU_LIMIT_WEIGHT: f64 = 0.05;
const DBIA_CU_PRICE_WEIGHT: f64 = 0.05;
const DBIA_INNER_GROUP_WEIGHT: f64 = 0.25;
const DBIA_FEE_TOPOLOGY_WEIGHT: f64 = 0.20;
pub(crate) const MIN_DIAGNOSTIC_SAMPLE_COUNT: usize = 2;
pub(crate) const MIN_CLEAN_BUY_SAMPLE_COUNT: u64 = 3;
pub(crate) const MIN_CLEAN_UNIQUE_BUYER_SAMPLE_COUNT_V2: u64 = 3;
const MIN_CLEAN_DBIA_BUYER_COUNT: usize = 2;
const MIN_CLEAN_DES_TRIPLE_COUNT: u64 = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct FtdiComputation {
    /// Gini–Simpson: 1 - coordination_hhi; nowa definicja z M2.
    pub fee_topology_diversity_index: Option<f64>,
    pub unique_topology_count: u64,
    pub coordination_hhi: Option<f64>,
    pub legacy_buy_tx_actionable: bool,
    pub unique_buyer_actionable_v2: bool,
    pub degraded_reasons: Vec<String>,
    pub buy_sample_count: u64,
    /// Wszyscy kwalifikowani signerzy potwierdzonych BUY.
    pub signer_sample_count: u64,
    /// Signerzy z kompletną cechą i jednoznacznym reprezentantem.
    pub represented_signer_count: u64,
}

impl FtdiComputation {
    pub fn evidence_v2(&self) -> ghost_core::tx_intelligence::types::FtdiEvidenceV2 {
        ghost_core::tx_intelligence::types::FtdiEvidenceV2 {
            definition: ghost_core::tx_intelligence::types::FtdiDefinitionV2::GiniSimpson,
            fee_topology_diversity_index: self.fee_topology_diversity_index,
            coordination_hhi: self.coordination_hhi,
            unique_topology_count: self.unique_topology_count,
            buy_sample_count: self.buy_sample_count,
            signer_sample_count: self.signer_sample_count,
            represented_signer_count: self.represented_signer_count,
            degraded_reasons: self.degraded_reasons.clone(),
        }
    }

    /// Pełna jakość nowego FTDI Gini–Simpson; nie historyczna flaga K/N.
    pub fn has_full_quality(&self) -> bool {
        self.fee_topology_diversity_index.is_some()
            && self.represented_signer_count >= MIN_CLEAN_UNIQUE_BUYER_SAMPLE_COUNT_V2
            && self.represented_signer_count == self.signer_sample_count
            && self.degraded_reasons.is_empty()
    }

    /// Historyczny widok K/N pozostaje dla zapisów i kontraktów V1. Współczesne
    /// porównania M6 korzystają z osobnego evidence V2 i jawnych progów Gini–Simpson.
    pub(crate) fn legacy_contract_v1(&self) -> FtdiLegacyContractV1 {
        let value = self
            .fee_topology_diversity_index
            .filter(|_| self.represented_signer_count >= MIN_DIAGNOSTIC_SAMPLE_COUNT as u64)
            .map(|_| self.unique_topology_count as f64 / self.signer_sample_count as f64);
        let mut degraded_reasons = self.degraded_reasons.clone();
        if self.buy_sample_count >= MIN_CLEAN_BUY_SAMPLE_COUNT
            && self.represented_signer_count >= MIN_DIAGNOSTIC_SAMPLE_COUNT as u64
        {
            degraded_reasons.retain(|reason| reason != FTDI_INSUFFICIENT_BUYS_REASON);
        }
        FtdiLegacyContractV1 {
            fee_topology_diversity_index: value,
            unique_topology_count: if value.is_some() {
                self.unique_topology_count
            } else {
                0
            },
            coordination_hhi: self.coordination_hhi.filter(|_| value.is_some()),
            legacy_buy_tx_actionable: self.legacy_buy_tx_actionable,
            unique_buyer_actionable_v2: self.unique_buyer_actionable_v2,
            degraded_reasons,
            buy_sample_count: self.buy_sample_count,
            signer_sample_count: self.signer_sample_count,
            input_complete: self.has_complete_input(),
        }
    }

    pub(crate) fn has_complete_input(&self) -> bool {
        !self
            .degraded_reasons
            .iter()
            .any(|reason| reason.starts_with("FTDI_INPUT_"))
            && self.represented_signer_count == self.signer_sample_count
    }

    fn gate_input_quality(&mut self) {
        if !self.has_complete_input() {
            self.legacy_buy_tx_actionable = false;
            self.unique_buyer_actionable_v2 = false;
        }
    }
}

/// Historyczny kontrakt K/N; typ oddzielony od nowego wyniku FTDI.
pub(crate) struct FtdiLegacyContractV1 {
    pub fee_topology_diversity_index: Option<f64>,
    pub unique_topology_count: u64,
    pub coordination_hhi: Option<f64>,
    pub legacy_buy_tx_actionable: bool,
    pub unique_buyer_actionable_v2: bool,
    pub degraded_reasons: Vec<String>,
    pub buy_sample_count: u64,
    pub signer_sample_count: u64,
    pub input_complete: bool,
}

// Te same obiekty są wynikiem kalkulatora i trwałym evidence; bez drugiej ścieżki obliczeń.
pub use ghost_core::tx_intelligence::types::{
    DbiaEvidenceV1 as DbiaComputation, SfdEvidenceV1 as SfdComputation,
};

#[derive(Debug, Clone, PartialEq)]
pub struct SybilResistanceComputationV1 {
    pub features: SybilResistanceFeatures,
    pub ftdi: FtdiComputation,
    pub dbia: DbiaComputation,
    pub sfd: SfdComputation,
    pub des: DesComputation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BuySampleStats {
    buy_sample_count: u64,
    signer_sample_count: u64,
}

impl SybilResistanceComputationV1 {
    pub(crate) fn mark_view_history_unavailable(&mut self) {
        for metric in ["FTDI", "DBIA", "SFD", "DES"] {
            let reason = format!("{metric}_INPUT_VIEW_HISTORY_UNAVAILABLE");
            if !self.features.degraded_reasons.contains(&reason) {
                self.features.degraded_reasons.push(reason.clone());
            }
            let target = match metric {
                "FTDI" => Some(&mut self.ftdi.degraded_reasons),
                "DBIA" => Some(&mut self.dbia.degraded_reasons),
                "SFD" => Some(&mut self.sfd.degraded_reasons),
                "DES" => Some(&mut self.des.degraded_reasons),
                _ => None,
            };
            if let Some(reasons) = target {
                if !reasons.contains(&reason) {
                    reasons.push(reason);
                }
            }
        }
        self.ftdi.gate_input_quality();
        self.features.fee_topology_diversity_v2 = Some(self.ftdi.evidence_v2());
        self.features.dbia_evidence_v1 = Some(self.dbia.clone());
        self.features.sfd_evidence_v1 = Some(self.sfd.clone());
        self.features.demand_elasticity_v2 = Some(self.des.clone());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
}

#[derive(Debug, Clone, Copy)]
struct OrderedBuyTx<'a> {
    tx: &'a PoolTransaction,
    slot: u64,
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

pub(crate) type SybilEventKey = (ghost_core::metric_contracts::StableEventIdentityV1, u32);

pub(crate) fn sybil_event_key(tx: &PoolTransaction) -> Option<SybilEventKey> {
    Some((
        ghost_core::metric_contracts::StableEventIdentityV1::try_from_signature(
            tx.semantic.source_kind.as_str(),
            &tx.signature,
        )
        .ok()?,
        tx.event_ordinal?,
    ))
}

/// Porównanie informacji wejściowych, bez czasu redostawy i nieużywanego payloadu.
pub(crate) fn same_sybil_view(a: &PoolTransaction, b: &PoolTransaction) -> bool {
    (
        a.is_buy,
        &a.signer,
        &a.pool_amm_id,
        a.is_dev_buy,
        a.metadata_availability,
        a.success,
    ) == (
        b.is_buy,
        &b.signer,
        &b.pool_amm_id,
        b.is_dev_buy,
        b.metadata_availability,
        b.success,
    ) && (
        a.slot,
        a.tx_index,
        a.outer_instruction_index,
        &a.inner_instruction_path,
        a.inner_group_index,
        &a.outer_program_id,
    ) == (
        b.slot,
        b.tx_index,
        b.outer_instruction_index,
        &b.inner_instruction_path,
        b.inner_group_index,
        &b.outer_program_id,
    ) && (
        a.virtual_sol_reserves,
        a.virtual_token_reserves,
        &a.token_mint,
    ) == (
        b.virtual_sol_reserves,
        b.virtual_token_reserves,
        &b.token_mint,
    ) && a.toolchain_fingerprint == b.toolchain_fingerprint
        && (
            a.signer_pre_balance_lamports,
            a.signer_post_balance_lamports,
        ) == (
            b.signer_pre_balance_lamports,
            b.signer_post_balance_lamports,
        )
        && (
            a.curve_data_known,
            a.v_sol_in_bonding_curve.map(f64::to_bits),
            a.v_tokens_in_bonding_curve.map(f64::to_bits),
            a.price_quote.map(f64::to_bits),
        ) == (
            b.curve_data_known,
            b.v_sol_in_bonding_curve.map(f64::to_bits),
            b.v_tokens_in_bonding_curve.map(f64::to_bits),
            b.price_quote.map(f64::to_bits),
        )
}

fn consistent_option<T: Eq>(values: impl IntoIterator<Item = Option<T>>) -> (Option<T>, bool) {
    let mut selected = None;
    for value in values.into_iter().flatten() {
        if selected.as_ref().is_some_and(|old| old != &value) {
            return (None, true);
        }
        selected = Some(value);
    }
    (selected, false)
}

/// Projekcja jednego eventu na bieżący cutoff; raw widoki pozostają osobne.
fn resolve_event_views<'a>(
    views: &[&'a PoolTransaction],
) -> (Option<Cow<'a, PoolTransaction>>, [bool; 4]) {
    let first = views[0];
    let mut conflicts = [false; 4];
    if views.iter().any(|tx| {
        tx.signer != first.signer
            || tx.pool_amm_id != first.pool_amm_id
            || tx.is_buy != first.is_buy
    }) {
        return (None, [true; 4]);
    }
    let known_success = views.iter().any(|tx| tx.is_confirmed_success());
    let known_failure = views
        .iter()
        .any(|tx| tx.metadata_availability.status_known && !tx.success);
    if known_success && known_failure {
        return (None, [true; 4]);
    }
    if !known_success || !first.is_buy {
        return (None, conflicts);
    }
    if views.iter().all(|tx| same_sybil_view(first, tx)) {
        return (Some(Cow::Borrowed(first)), conflicts);
    }
    let mut tx = first.clone();
    let mut order_conflict = false;
    macro_rules! coordinate {
        ($field:ident) => {{
            let (value, conflict) = consistent_option(views.iter().map(|v| v.$field.clone()));
            tx.$field = value;
            order_conflict |= conflict;
        }};
    }
    coordinate!(slot);
    coordinate!(tx_index);
    coordinate!(outer_instruction_index);
    coordinate!(inner_instruction_path);
    coordinate!(inner_group_index);
    coordinate!(outer_program_id);
    if order_conflict {
        return (None, [true; 4]);
    }
    tx.success = true;
    tx.metadata_availability.status_known = true;
    tx.metadata_availability.inner_instructions_known = views
        .iter()
        .any(|v| v.metadata_availability.has_inner_instructions());
    tx.is_dev_buy = views.iter().any(|v| v.is_dev_buy);
    tx.toolchain_fingerprint = ToolchainFingerprintInput::default();
    macro_rules! fingerprint {
        ($field:ident, $inner:expr, $topology:expr) => {{
            let (value, conflict) = consistent_option(
                views
                    .iter()
                    .filter(|v| {
                        if $inner {
                            v.metadata_availability.has_inner_instructions()
                        } else {
                            // Some dla pola outer pochodzi z message/instrukcji,
                            // nie z wyniku wykonania w metadata.
                            true
                        }
                    })
                    .map(|v| v.toolchain_fingerprint.$field),
            );
            tx.toolchain_fingerprint.$field = value;
            conflicts[1] |= conflict;
            if $topology {
                conflicts[0] |= conflict;
            }
        }};
    }
    fingerprint!(account_keys_len, false, false);
    fingerprint!(outer_instruction_count, false, false);
    fingerprint!(has_set_compute_unit_limit, false, false);
    fingerprint!(has_set_compute_unit_price, false, false);
    fingerprint!(inner_instruction_group_count, true, false);
    fingerprint!(external_fee_transfer_count, true, true);
    fingerprint!(internal_fee_transfer_count, true, true);
    let (pre, pre_conflict) =
        consistent_option(views.iter().map(|v| v.signer_pre_balance_lamports));
    let (post, post_conflict) =
        consistent_option(views.iter().map(|v| v.signer_post_balance_lamports));
    conflicts[2] = pre_conflict || post_conflict;
    // Para musi występować w jednym raw widoku. Nie składamy pre z jednego
    // rekordu i post z drugiego, nawet gdy to ta sama tożsamość zdarzenia.
    let pair_observed = views.iter().any(|v| {
        v.signer_pre_balance_lamports.is_some() && v.signer_post_balance_lamports.is_some()
    });
    let pair = if conflicts[2] {
        (None, None)
    } else if pair_observed {
        (pre, post)
    } else if pre.is_some() {
        (pre, None)
    } else {
        (None, post)
    };
    tx.signer_pre_balance_lamports = pair.0;
    tx.signer_post_balance_lamports = pair.1;
    macro_rules! price {
        ($field:ident) => {{
            let (value, conflict) =
                consistent_option(views.iter().map(|v| v.$field.map(f64::to_bits)));
            tx.$field = value.map(f64::from_bits);
            let _ = conflict; // Pole historyczne, nie authority ceny DES V2.
        }};
    }
    price!(price_quote);
    price!(v_sol_in_bonding_curve);
    price!(v_tokens_in_bonding_curve);
    tx.curve_data_known = views.iter().any(|v| v.curve_data_known);
    let (sol, sol_conflict) = consistent_option(views.iter().map(|v| v.virtual_sol_reserves));
    let (tokens, token_conflict) =
        consistent_option(views.iter().map(|v| v.virtual_token_reserves));
    let (mint, mint_conflict) = consistent_option(views.iter().map(|v| v.token_mint.clone()));
    conflicts[3] |= sol_conflict || token_conflict || mint_conflict;
    // Obie rezerwy muszą współwystępować w jednym widoku tego samego eventu.
    // Nie tworzymy fikcyjnego snapshotu z dwóch osobno niekompletnych rekordów.
    let pair_observed = views
        .iter()
        .any(|v| v.virtual_sol_reserves.is_some() && v.virtual_token_reserves.is_some());
    (tx.virtual_sol_reserves, tx.virtual_token_reserves) = if pair_observed && !conflicts[3] {
        (sol, tokens)
    } else {
        (None, None)
    };
    tx.token_mint = mint;
    (Some(Cow::Owned(tx)), conflicts)
}

struct BuyWindow<'a> {
    buys: Vec<Cow<'a, PoolTransaction>>,
    unknown_status: bool,
    unknown_receipt: bool,
    unknown_identity: bool,
    conflicts: [bool; 4],
}

impl<'a> BuyWindow<'a> {
    fn new(
        transactions: impl IntoIterator<Item = &'a PoolTransaction>,
        cutoff_ingress_wall_ms: Option<u64>,
    ) -> Self {
        let mut window = Self {
            buys: Vec::new(),
            unknown_status: false,
            unknown_receipt: false,
            unknown_identity: false,
            conflicts: [false; 4],
        };
        let mut available = BTreeMap::<SybilEventKey, Vec<&PoolTransaction>>::new();
        let mut unknown_receipt_keys = HashSet::new();
        for tx in transactions {
            if let Some(cutoff) = cutoff_ingress_wall_ms {
                match tx.event_time.ingress_wall_ts_ms {
                    Some(received) if received > cutoff => continue,
                    Some(_) => {}
                    None => {
                        if tx.is_buy && !(tx.metadata_availability.status_known && !tx.success) {
                            if let Some(key) = sybil_event_key(tx) {
                                unknown_receipt_keys.insert(key);
                            } else {
                                window.unknown_receipt = true;
                            }
                        }
                        continue;
                    }
                }
            }
            let Some(key) = sybil_event_key(tx) else {
                if tx.is_buy && !(tx.metadata_availability.status_known && !tx.success) {
                    window.unknown_identity = true;
                    window.unknown_status |= !tx.metadata_availability.status_known;
                }
                continue;
            };
            available.entry(key).or_default().push(tx);
        }
        // Widok bez czasu nie dostarcza danych. Nie jest jednak dodatkowym
        // nieznanym eventem, jeżeli ta sama tożsamość ma dostępny widok przed cutoff.
        window.unknown_receipt |= unknown_receipt_keys
            .iter()
            .any(|key| !available.contains_key(key));
        for views in available.values() {
            if !views.iter().any(|tx| tx.is_buy) {
                continue;
            }
            if !views.iter().any(|tx| tx.metadata_availability.status_known) {
                window.unknown_status = true;
            }
            let (resolved, conflicts) = resolve_event_views(views);
            for (flag, conflict) in window.conflicts.iter_mut().zip(conflicts) {
                *flag |= conflict;
            }
            if let Some(tx) = resolved {
                window.buys.push(tx);
            }
        }
        window
    }

    fn buy_refs(&self) -> Vec<&PoolTransaction> {
        self.buys.iter().map(AsRef::as_ref).collect()
    }

    fn append_reasons(&self, metric: &str, reasons: &mut Vec<String>) {
        let index = match metric {
            "FTDI" => 0,
            "DBIA" => 1,
            "SFD" => 2,
            _ => 3,
        };
        for (present, suffix) in [
            (self.unknown_status, "STATUS_UNAVAILABLE"),
            (self.unknown_receipt, "RECEIPT_TIME_UNAVAILABLE"),
            (self.unknown_identity, "EVENT_IDENTITY_UNAVAILABLE"),
            (self.conflicts[index], "CONFLICTING_VIEWS"),
        ] {
            if present {
                reasons.push(format!("{metric}_INPUT_{suffix}"));
            }
        }
    }
}

fn successful_buy_samples<'a>(transactions: &[&'a PoolTransaction]) -> Vec<SequencedBuyTx<'a>> {
    transactions
        .iter()
        .copied()
        .filter(|tx| tx.is_buy && tx.is_confirmed_success())
        .map(|tx| SequencedBuyTx { tx })
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

/// Dowód porządku z provenance, niezależny od ordinala tożsamości.
/// Prefiks ścieżki nie dowodzi momentu mutacji rodzica wobec potomka.
fn canonical_buy_order(left: &PoolTransaction, right: &PoolTransaction) -> Option<Ordering> {
    let slot_order = left.slot?.cmp(&right.slot?);
    if slot_order != Ordering::Equal {
        return Some(slot_order);
    }
    if !left.signature.is_empty() && left.signature == right.signature {
        if matches!((left.tx_index, right.tx_index), (Some(a), Some(b)) if a != b) {
            return None;
        }
        let outer = left
            .outer_instruction_index?
            .cmp(&right.outer_instruction_index?);
        if outer != Ordering::Equal {
            return Some(outer);
        }
        let a = left.inner_instruction_path.as_deref()?;
        let b = right.inner_instruction_path.as_deref()?;
        for (x, y) in a.iter().zip(b) {
            let order = x.cmp(y);
            if order != Ordering::Equal {
                return Some(order);
            }
        }
        return None;
    }
    let tx_order = left.tx_index?.cmp(&right.tx_index?);
    (tx_order != Ordering::Equal).then_some(tx_order)
}

fn first_canonical_sample<'a>(
    samples: &[&'a PoolTransaction],
) -> Result<Option<&'a PoolTransaction>, ()> {
    let Some(&initial) = samples.first() else {
        return Ok(None);
    };
    let mut first = initial;
    for &sample in samples.iter().skip(1) {
        if canonical_buy_order(sample, first) == Some(Ordering::Less) {
            first = sample;
        }
    }
    // Niepewność wśród późniejszych zdarzeń nie odbiera dowodu pierwszeństwa.
    if samples.iter().all(|sample| {
        std::ptr::eq(*sample, first) || canonical_buy_order(first, sample) == Some(Ordering::Less)
    }) {
        Ok(Some(first))
    } else {
        Err(())
    }
}

struct BuyerSelection<'a> {
    samples: Vec<&'a PoolTransaction>,
    order_unavailable: bool,
}

fn unique_buyer_samples<'a>(
    buy_txs: &[&'a PoolTransaction],
    qualifies: impl Fn(&PoolTransaction) -> bool,
) -> BuyerSelection<'a> {
    let mut by_signer = BTreeMap::<&str, Vec<&PoolTransaction>>::new();
    for &tx in buy_txs {
        if qualifies(tx) {
            by_signer.entry(tx.signer.as_str()).or_default().push(tx);
        }
    }
    let mut selection = BuyerSelection {
        samples: Vec::with_capacity(by_signer.len()),
        order_unavailable: false,
    };
    for candidates in by_signer.values() {
        match first_canonical_sample(candidates) {
            Ok(Some(first)) => selection.samples.push(first),
            Ok(None) => {}
            Err(()) => selection.order_unavailable = true,
        }
    }
    selection
}

/// Oba salda pochodzą z tego samego rekordu transakcji i tego samego signera.
/// Brakujące lub rosnące saldo nie jest zastępowane kwotą BUY, zerem ani clampem.
fn sfd_spend_fraction(tx: &PoolTransaction) -> Option<f64> {
    let pre = tx.signer_pre_balance_lamports?;
    let post = tx.signer_post_balance_lamports?;
    if pre == 0 {
        return None;
    }
    let spent = pre.checked_sub(post)?;
    Some(spent as f64 / pre as f64)
}

fn resolve_dev_wallet<'a>(
    buy_txs: &[&'a PoolTransaction],
    explicit_dev_wallet: Option<&'a str>,
) -> Option<&'a str> {
    explicit_dev_wallet.or_else(|| {
        let mut marked = buy_txs
            .iter()
            .filter(|tx| tx.is_dev_buy)
            .map(|tx| tx.signer.as_str());
        let dev = marked.next()?;
        marked.all(|wallet| wallet == dev).then_some(dev)
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
    let mut txs: Vec<_> = buy_samples.iter().map(|sample| sample.tx).collect();
    // tx_index jest współrzędną transakcji. Częściowe widoki różnych eventów
    // tej samej transakcji nie mogą rozdzielić jej instrukcji podczas sortowania.
    let mut indices = BTreeMap::new();
    for tx in &txs {
        let key = (tx.slot?, tx.signature.as_str());
        if let Some(index) = tx.tx_index {
            if indices.insert(key, index).is_some_and(|old| old != index) {
                return None;
            }
        }
    }
    // Sortowanie techniczne wymaga poniżej dowodu porządku kanonicznego.
    txs.sort_by(|a, b| {
        (
            a.slot,
            a.slot
                .and_then(|slot| indices.get(&(slot, a.signature.as_str())).copied()),
            a.signature.as_str(),
            a.outer_instruction_index,
            a.inner_instruction_path.as_deref(),
        )
            .cmp(&(
                b.slot,
                b.slot
                    .and_then(|slot| indices.get(&(slot, b.signature.as_str())).copied()),
                b.signature.as_str(),
                b.outer_instruction_index,
                b.inner_instruction_path.as_deref(),
            ))
    });
    for pair in txs.windows(2) {
        if canonical_buy_order(pair[0], pair[1]) != Some(Ordering::Less) {
            return None;
        }
    }
    txs.into_iter()
        .map(|tx| Some(OrderedBuyTx { tx, slot: tx.slot? }))
        .collect()
}

/// Jeden potwierdzony rodzaj ceny: surowe rezerwy Pump post-trade.
/// Jednostka to lamporty / bazowe jednostki tokena; nie zależy od skalowania
/// znormalizowanych pól i nie korzysta z niezweryfikowanego price_quote.
fn curve_price(tx: &PoolTransaction) -> Option<f64> {
    let sol = tx.virtual_sol_reserves?;
    let tokens = tx.virtual_token_reserves?;
    if sol == 0 || tokens == 0 {
        return None;
    }
    let price = sol as f64 / tokens as f64;
    (price.is_finite() && price > 0.0).then_some(price)
}

/// Kendall tau-b. Oś czasu pozostaje całkowita nawet powyżej dokładności f64.
/// Remis na obu osiach nie jest dopisywany do żadnego licznika pojedynczych remisów.
fn kendall_tau(x_values: &[f64], y_values: &[u64]) -> Option<f64> {
    if x_values.len() != y_values.len()
        || x_values.len() < MIN_DIAGNOSTIC_SAMPLE_COUNT
        || x_values.iter().any(|x| !x.is_finite())
    {
        return None;
    }
    let (mut concordant, mut discordant, mut ties_x, mut ties_y) = (0u64, 0u64, 0u64, 0u64);
    for i in 0..x_values.len() {
        for k in (i + 1)..x_values.len() {
            let x_order = x_values[i].partial_cmp(&x_values[k])?;
            let y_order = y_values[i].cmp(&y_values[k]);
            let counter = match (x_order, y_order) {
                (Ordering::Equal, Ordering::Equal) => continue,
                (Ordering::Equal, _) => &mut ties_x,
                (_, Ordering::Equal) => &mut ties_y,
                (x, y) if x == y => &mut concordant,
                _ => &mut discordant,
            };
            *counter = counter.checked_add(1)?;
        }
    }
    let comparable = concordant.checked_add(discordant)?;
    if comparable == 0 {
        return None;
    }
    let denominator = ((comparable.checked_add(ties_x)? as f64)
        * (comparable.checked_add(ties_y)? as f64))
        .sqrt();
    if denominator == 0.0 || !denominator.is_finite() {
        return None;
    }
    Some((concordant as f64 - discordant as f64) / denominator)
}

pub fn compute_ftdi<'a>(
    transactions: impl IntoIterator<Item = &'a PoolTransaction>,
) -> FtdiComputation {
    let window = BuyWindow::new(transactions, None);
    let mut result = compute_ftdi_from_buys(&window.buy_refs());
    window.append_reasons("FTDI", &mut result.degraded_reasons);
    result.gate_input_quality();
    result
}

/// Jeden licznik HHI: najpierw dokładne sumy całkowite, dopiero na końcu f64.
fn coordination_hhi_from_counts(counts: impl IntoIterator<Item = u64>) -> Option<f64> {
    let (n, squares) = counts
        .into_iter()
        .try_fold((0u64, 0u128), |(n, squares), count| {
            if count == 0 {
                return None;
            }
            let count128 = u128::from(count);
            Some((
                n.checked_add(count)?,
                squares.checked_add(count128.checked_mul(count128)?)?,
            ))
        })?;
    if n == 0 {
        return None;
    }
    let n128 = u128::from(n);
    let denominator = n128.checked_mul(n128)?;
    Some(squares as f64 / denominator as f64)
}

fn compute_ftdi_from_buys(buy_txs: &[&PoolTransaction]) -> FtdiComputation {
    let stats = buy_sample_stats(buy_txs);
    let selection = unique_buyer_samples(buy_txs, |tx| {
        tx.metadata_availability.has_inner_instructions()
            && tx.toolchain_fingerprint.fee_topology().is_some()
    });
    let represented_signer_count = selection.samples.len() as u64;
    let mut topology_counts = BTreeMap::<FeeTopology, u64>::new();
    for tx in &selection.samples {
        let (external_fee_count, internal_fee_count) = tx
            .toolchain_fingerprint
            .fee_topology()
            .expect("kwalifikacja wymaga kompletnej topologii");
        *topology_counts
            .entry(FeeTopology {
                external_fee_count,
                internal_fee_count,
            })
            .or_insert(0) += 1;
    }
    let mut result = FtdiComputation {
        fee_topology_diversity_index: None,
        unique_topology_count: topology_counts.len() as u64,
        coordination_hhi: None,
        legacy_buy_tx_actionable: false,
        unique_buyer_actionable_v2: false,
        degraded_reasons: Vec::new(),
        buy_sample_count: stats.buy_sample_count,
        signer_sample_count: stats.signer_sample_count,
        represented_signer_count,
    };
    if selection.order_unavailable {
        result
            .degraded_reasons
            .push("FTDI_INPUT_ORDER_UNAVAILABLE".to_string());
    } else if represented_signer_count != stats.signer_sample_count {
        result
            .degraded_reasons
            .push(FTDI_RAW_FEE_TOPOLOGY_UNAVAILABLE_REASON.to_string());
    } else if represented_signer_count == 0 {
        result
            .degraded_reasons
            .push(FTDI_INSUFFICIENT_BUYS_REASON.to_string());
    } else if let Some(hhi) = coordination_hhi_from_counts(topology_counts.values().copied()) {
        result.coordination_hhi = Some(hhi);
        result.fee_topology_diversity_index = Some(1.0 - hhi);
        if represented_signer_count < MIN_CLEAN_UNIQUE_BUYER_SAMPLE_COUNT_V2 {
            result
                .degraded_reasons
                .push(FTDI_INSUFFICIENT_BUYS_REASON.to_string());
        }
        // Te flagi opisują wyłącznie historyczne kontrakty K/N, nie jakość M2.
        result.legacy_buy_tx_actionable = represented_signer_count
            >= MIN_DIAGNOSTIC_SAMPLE_COUNT as u64
            && stats.buy_sample_count >= MIN_CLEAN_BUY_SAMPLE_COUNT;
        result.unique_buyer_actionable_v2 =
            represented_signer_count >= MIN_CLEAN_UNIQUE_BUYER_SAMPLE_COUNT_V2;
    } else {
        result
            .degraded_reasons
            .push("FTDI_INPUT_ARITHMETIC_UNAVAILABLE".to_string());
    }
    result
}

pub fn compute_dbia<'a>(
    transactions: impl IntoIterator<Item = &'a PoolTransaction>,
    dev_wallet: Option<&'a str>,
) -> DbiaComputation {
    let window = BuyWindow::new(transactions, None);
    let mut result = compute_dbia_from_buys(&window.buy_refs(), dev_wallet);
    window.append_reasons("DBIA", &mut result.degraded_reasons);
    result
}

fn compute_dbia_from_buys<'a>(
    buy_txs: &[&'a PoolTransaction],
    dev_wallet: Option<&'a str>,
) -> DbiaComputation {
    let stats = buy_sample_stats(buy_txs);
    let selection = unique_buyer_samples(buy_txs, |tx| {
        tx.metadata_availability.has_inner_instructions()
            && InfrastructureFingerprint::from_input(&tx.toolchain_fingerprint).is_some()
    });
    let order_unavailable = selection.order_unavailable;
    let unique_samples = selection.samples;
    let represented_signer_count = unique_samples.len() as u64;
    let Some(dev_wallet) = resolve_dev_wallet(buy_txs, dev_wallet)
        .filter(|wallet| buy_txs.iter().any(|tx| tx.signer == *wallet))
    else {
        return DbiaComputation {
            dev_buyer_infrastructure_affinity: None,
            degraded_reasons: vec![DBIA_NO_DEV_BUY_REASON.to_string()],
            buy_sample_count: stats.buy_sample_count,
            signer_sample_count: stats.signer_sample_count,
            represented_signer_count,
        };
    };
    if order_unavailable || unique_samples.len() as u64 != stats.signer_sample_count {
        return DbiaComputation {
            dev_buyer_infrastructure_affinity: None,
            degraded_reasons: vec![if order_unavailable {
                "DBIA_INPUT_ORDER_UNAVAILABLE".to_string()
            } else {
                DBIA_RAW_FINGERPRINT_UNAVAILABLE_REASON.to_string()
            }],
            buy_sample_count: stats.buy_sample_count,
            signer_sample_count: stats.signer_sample_count,
            represented_signer_count,
        };
    }
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
            represented_signer_count,
        };
    };
    let Some(dev_fp) = InfrastructureFingerprint::from_input(&dev_tx.toolchain_fingerprint) else {
        return DbiaComputation {
            dev_buyer_infrastructure_affinity: None,
            degraded_reasons: vec![DBIA_RAW_FINGERPRINT_UNAVAILABLE_REASON.to_string()],
            buy_sample_count: stats.buy_sample_count,
            signer_sample_count: stats.signer_sample_count,
            represented_signer_count,
        };
    };

    let buyer_txs: Vec<&PoolTransaction> = unique_samples
        .into_iter()
        .filter(|tx| tx.signer != dev_wallet)
        .collect();
    if buyer_txs.is_empty() {
        return DbiaComputation {
            dev_buyer_infrastructure_affinity: None,
            degraded_reasons: vec![DBIA_INSUFFICIENT_BUYERS_REASON.to_string()],
            buy_sample_count: stats.buy_sample_count,
            signer_sample_count: stats.signer_sample_count,
            represented_signer_count,
        };
    }
    let mut degraded_reasons = Vec::new();
    if buyer_txs.len() < MIN_CLEAN_DBIA_BUYER_COUNT {
        degraded_reasons.push(DBIA_INSUFFICIENT_BUYERS_REASON.to_string());
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
                represented_signer_count,
            };
        };
        similarity_sum += dev_fp.similarity(&fingerprint);
    }

    DbiaComputation {
        dev_buyer_infrastructure_affinity: Some(similarity_sum / buyer_txs.len() as f64),
        degraded_reasons,
        buy_sample_count: stats.buy_sample_count,
        signer_sample_count: stats.signer_sample_count,
        represented_signer_count,
    }
}

pub fn compute_sfd<'a>(
    transactions: impl IntoIterator<Item = &'a PoolTransaction>,
) -> SfdComputation {
    let window = BuyWindow::new(transactions, None);
    let mut result = compute_sfd_from_buys(&window.buy_refs());
    window.append_reasons("SFD", &mut result.degraded_reasons);
    result
}

fn compute_sfd_from_buys(buy_txs: &[&PoolTransaction]) -> SfdComputation {
    let stats = buy_sample_stats(buy_txs);
    let selection = unique_buyer_samples(buy_txs, |tx| sfd_spend_fraction(tx).is_some());
    let represented_signer_count = selection.samples.len() as u64;
    let represented_signers: HashSet<_> = selection
        .samples
        .iter()
        .map(|tx| tx.signer.as_str())
        .collect();
    let mut zero_prebalance_skipped = false;
    let mut missing_balance = false;
    let mut invalid_balance_pair = false;
    // Przyczyny dotyczą pominiętych signerów, nie wcześniejszych błędnych BUY
    // signera, który ma późniejszego poprawnego reprezentanta przed cutoff.
    for tx in buy_txs {
        if represented_signers.contains(tx.signer.as_str()) {
            continue;
        }
        zero_prebalance_skipped |= tx.signer_pre_balance_lamports == Some(0);
        missing_balance |=
            tx.signer_pre_balance_lamports.is_none() || tx.signer_post_balance_lamports.is_none();
        invalid_balance_pair |= matches!(
            (tx.signer_pre_balance_lamports, tx.signer_post_balance_lamports),
            (Some(pre), Some(post)) if post > pre
        );
    }
    let spend_fractions: Vec<_> = selection
        .samples
        .iter()
        .map(|tx| sfd_spend_fraction(tx).expect("kwalifikacja wymaga poprawnej pary sald"))
        .collect();
    let spend_fraction_divergence = if spend_fractions.len() < MIN_DIAGNOSTIC_SAMPLE_COUNT {
        None
    } else {
        let median_fraction = median(&spend_fractions).expect("co najmniej dwie frakcje");
        let deviations: Vec<_> = spend_fractions
            .iter()
            .map(|value| (value - median_fraction).abs())
            .collect();
        median(&deviations)
    };
    let mut degraded_reasons = Vec::new();
    if zero_prebalance_skipped {
        degraded_reasons.push(SFD_ZERO_PREBALANCE_SKIPPED_REASON.to_string());
    }
    if missing_balance {
        degraded_reasons.push(SFD_POSTBALANCE_UNAVAILABLE_REASON.to_string());
    }
    if invalid_balance_pair {
        degraded_reasons.push(SFD_INVALID_BALANCE_PAIR_REASON.to_string());
    }
    if selection.order_unavailable {
        degraded_reasons.push("SFD_INPUT_ORDER_UNAVAILABLE".to_string());
    }
    if represented_signer_count != stats.signer_sample_count {
        degraded_reasons.push(SFD_PARTIAL_BALANCE_COVERAGE_REASON.to_string());
    }
    if represented_signer_count < MIN_CLEAN_BUY_SAMPLE_COUNT {
        degraded_reasons.push(SFD_INSUFFICIENT_BUYS_REASON.to_string());
    }
    SfdComputation {
        spend_fraction_divergence,
        degraded_reasons,
        buy_sample_count: stats.buy_sample_count,
        signer_sample_count: stats.signer_sample_count,
        represented_signer_count,
    }
}

pub fn compute_des<'a>(
    transactions: impl IntoIterator<Item = &'a PoolTransaction>,
) -> DesComputation {
    compute_des_from_window(&BuyWindow::new(transactions, None))
}

/// Cutoff jest osią dostępności u odbiorcy, nie slotem ani czasem chain.
pub fn compute_des_at_cutoff<'a>(
    transactions: impl IntoIterator<Item = &'a PoolTransaction>,
    cutoff_ingress_wall_ms: u64,
) -> DesComputation {
    compute_des_from_window(&BuyWindow::new(transactions, Some(cutoff_ingress_wall_ms)))
}

fn empty_des(buy_txs: &[&PoolTransaction]) -> DesComputation {
    let stats = buy_sample_stats(buy_txs);
    DesComputation {
        definition: DesDefinitionV2::NextBuySlotTauB,
        interval_unit: DesIntervalUnitV2::Slots,
        price_source: DesPriceSourceV2::PumpVirtualPostTradeReserves,
        demand_elasticity_score: None,
        degraded_reasons: Vec::new(),
        buy_sample_count: stats.buy_sample_count,
        signer_sample_count: stats.signer_sample_count,
        priced_buy_count: buy_txs
            .iter()
            .filter(|tx| curve_price(tx).is_some())
            .count() as u64,
        candidate_triple_count: 0,
        closed_triple_count: 0,
    }
}

fn compute_des_from_window(window: &BuyWindow<'_>) -> DesComputation {
    let buys = window.buy_refs();
    // Usunięte zdarzenie o niepewnym statusie/tożsamości/kolejności może być
    // następnym BUY. Nie wolno zbudować trójki ponad taką luką w sekwencji.
    let sequence_incomplete = window.unknown_status
        || window.unknown_receipt
        || window.unknown_identity
        || window.conflicts[3];
    let mut result = if sequence_incomplete {
        empty_des(&buys)
    } else {
        compute_des_from_transactions(&buys)
    };
    window.append_reasons("DES", &mut result.degraded_reasons);
    result
}

fn compute_des_from_transactions(transactions: &[&PoolTransaction]) -> DesComputation {
    let buy_samples = successful_buy_samples(transactions);
    let buy_txs: Vec<&PoolTransaction> = buy_samples.iter().map(|sample| sample.tx).collect();
    let mut result = empty_des(&buy_txs);
    if buy_samples.len() < 3 {
        result
            .degraded_reasons
            .push(DES_INSUFFICIENT_TRIPLES_REASON.to_string());
        return result;
    }
    let Some(ordered) = ordered_buy_samples(&buy_samples) else {
        result
            .degraded_reasons
            .push(DES_SLOT_ORDER_UNAVAILABLE_REASON.to_string());
        return result;
    };
    result.candidate_triple_count = ordered.len().saturating_sub(2) as u64;
    let pool = &ordered[0].tx.pool_amm_id;
    let (_, mint_conflict) = consistent_option(ordered.iter().map(|v| v.tx.token_mint.as_deref()));
    if pool.is_empty() || ordered.iter().any(|v| &v.tx.pool_amm_id != pool) || mint_conflict {
        result
            .degraded_reasons
            .push(DES_PRICE_DOMAIN_MISMATCH_REASON.to_string());
        return result;
    }
    let prices: Vec<_> = ordered
        .iter()
        .map(|sample| curve_price(sample.tx))
        .collect();
    let mut changes = Vec::with_capacity(ordered.len().saturating_sub(2));
    let mut next_intervals = Vec::with_capacity(ordered.len().saturating_sub(2));
    // Nie filtrujemy braków przed windows(3): każda luka rozcina trójki.
    for (triple, prices) in ordered.windows(3).zip(prices.windows(3)) {
        let [Some(previous), Some(current), Some(_next)] = prices else {
            continue;
        };
        let change = (current - previous) / previous;
        let Some(next_interval) = triple[2].slot.checked_sub(triple[1].slot) else {
            continue;
        };
        if !change.is_finite() {
            continue;
        }
        changes.push(change);
        next_intervals.push(next_interval);
    }
    result.closed_triple_count = changes.len() as u64;
    if result.closed_triple_count != result.candidate_triple_count {
        result
            .degraded_reasons
            .push(DES_CURVE_DATA_UNAVAILABLE_REASON.to_string());
    }
    if result.closed_triple_count < MIN_CLEAN_DES_TRIPLE_COUNT {
        result
            .degraded_reasons
            .push(DES_INSUFFICIENT_TRIPLES_REASON.to_string());
    }
    result.demand_elasticity_score = kendall_tau(&changes, &next_intervals);
    if changes.len() >= MIN_DIAGNOSTIC_SAMPLE_COUNT && result.demand_elasticity_score.is_none() {
        result
            .degraded_reasons
            .push(DES_NO_COMPARABLE_PAIRS_REASON.to_string());
    }
    result
}

pub fn compute_sybil_resistance<'a>(
    transactions: impl IntoIterator<Item = &'a PoolTransaction>,
    dev_wallet: Option<&'a str>,
) -> SybilResistanceFeatures {
    compute_sybil_resistance_with_ftdi(transactions, dev_wallet).features
}

pub fn compute_sybil_resistance_with_ftdi<'a>(
    transactions: impl IntoIterator<Item = &'a PoolTransaction>,
    dev_wallet: Option<&'a str>,
) -> SybilResistanceComputationV1 {
    compute_sybil_resistance_from_window(BuyWindow::new(transactions, None), dev_wallet)
}

/// Historyczny odczyt według czasu dostępności epoch; filtr poprzedza deduplikację.
pub fn compute_sybil_resistance_with_ftdi_at_cutoff<'a>(
    transactions: impl IntoIterator<Item = &'a PoolTransaction>,
    dev_wallet: Option<&'a str>,
    cutoff_ingress_wall_ms: u64,
) -> SybilResistanceComputationV1 {
    compute_sybil_resistance_from_window(
        BuyWindow::new(transactions, Some(cutoff_ingress_wall_ms)),
        dev_wallet,
    )
}

fn compute_sybil_resistance_from_window<'a>(
    window: BuyWindow<'a>,
    dev_wallet: Option<&'a str>,
) -> SybilResistanceComputationV1 {
    let mut ftdi = compute_ftdi_from_buys(&window.buy_refs());
    let mut dbia = compute_dbia_from_buys(&window.buy_refs(), dev_wallet);
    let mut sfd = compute_sfd_from_buys(&window.buy_refs());
    let des = compute_des_from_window(&window);
    window.append_reasons("FTDI", &mut ftdi.degraded_reasons);
    window.append_reasons("DBIA", &mut dbia.degraded_reasons);
    window.append_reasons("SFD", &mut sfd.degraded_reasons);
    ftdi.gate_input_quality();
    // Kontrakt MFS V1 pozostaje K/N do jawnej migracji definicji w M6.
    let legacy_ftdi = ftdi.legacy_contract_v1();

    let mut degraded_reasons = Vec::<String>::new();
    for reason in legacy_ftdi
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

    // Nowy wynik nigdy nie trafia pod stary klucz/progi DES V1.
    // To powód niedostępności porównania, nie degradacja pomiaru V2.
    degraded_reasons.push(DES_COMPARISON_DEFINITION_MISMATCH_REASON.to_string());
    let features = SybilResistanceFeatures {
        fee_topology_diversity_index: legacy_ftdi.fee_topology_diversity_index,
        dev_buyer_infrastructure_affinity: dbia.dev_buyer_infrastructure_affinity,
        spend_fraction_divergence: sfd.spend_fraction_divergence,
        demand_elasticity_score: None,
        demand_elasticity_v2: Some(des.clone()),
        fee_topology_diversity_v2: Some(ftdi.evidence_v2()),
        dbia_evidence_v1: Some(dbia.clone()),
        sfd_evidence_v1: Some(sfd.clone()),
        degraded_reasons,
        buy_sample_count: ftdi.buy_sample_count,
        signer_sample_count: ftdi.signer_sample_count,
        ..SybilResistanceFeatures::default()
    };
    SybilResistanceComputationV1 {
        features,
        ftdi,
        dbia,
        sfd,
        des,
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
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT_EXECUTION_INDEX: AtomicU32 = AtomicU32::new(0);
        let digest = solana_sdk::hash::hash(signature.as_bytes()).to_bytes();
        let mut signature_bytes = [0u8; 64];
        signature_bytes[..32].copy_from_slice(&digest);
        signature_bytes[32..].copy_from_slice(&digest);
        PoolTransaction {
            semantic: EventSemanticEnvelope::default(),
            pool_amm_id: "pool-1".to_string(),
            slot: Some(1),
            event_ordinal: Some(0),
            tx_index: Some(NEXT_EXECUTION_INDEX.fetch_add(1, Ordering::Relaxed)),
            outer_instruction_index: None,
            inner_instruction_path: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 1_000,
            event_time: EventTimeMetadata::new(Some(1_000), Some(1_000), Some(1_000)),
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
            signature: solana_sdk::signature::Signature::from(signature_bytes).to_string(),
            metadata_availability: seer::types::TransactionMetadataAvailability {
                status_known: true,
                inner_instructions_known: true,
            },
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
            token_mint: None,
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
        tx.sol_amount_lamports = None;
        tx.volume_sol = 0.0;
        tx
    }

    fn sfd_buy_tx_with_amount(
        signer: &str,
        signature: &str,
        pre_balance: Option<u64>,
        post_balance: Option<u64>,
        buy_amount_lamports: Option<u64>,
    ) -> PoolTransaction {
        let mut tx = sfd_buy_tx(signer, signature, pre_balance, post_balance);
        tx.sol_amount_lamports = buy_amount_lamports;
        tx.volume_sol =
            buy_amount_lamports.map_or(0.0, |lamports| lamports as f64 / 1_000_000_000.0);
        tx
    }

    fn des_buy_tx(
        signer: &str,
        signature: &str,
        slot: Option<u64>,
        tx_index: Option<u32>,
        v_sol: Option<f64>,
        v_tokens: Option<f64>,
    ) -> PoolTransaction {
        let mut tx = buy_tx(
            signer,
            signature,
            dbia_fingerprint(12, 3, true, true, 2, (0, 0)),
        );
        tx.slot = slot;
        tx.tx_index = tx_index;
        tx.event_ordinal = Some(0);
        tx.v_sol_in_bonding_curve = v_sol;
        tx.v_tokens_in_bonding_curve = v_tokens;
        tx.virtual_sol_reserves = v_sol
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map(|value| (value * 1_000_000_000.0).round() as u64);
        tx.virtual_token_reserves = v_tokens
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map(|value| (value * 1_000_000.0).round() as u64);
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

        assert_eq!(homogeneous_ftdi.fee_topology_diversity_index, Some(0.0));
        assert_eq!(
            mixed_ftdi.fee_topology_diversity_index,
            Some(1.0 - 1.0 / 3.0)
        );
        assert!(
            mixed_ftdi.fee_topology_diversity_index.unwrap()
                > homogeneous_ftdi.fee_topology_diversity_index.unwrap()
        );
    }

    #[test]
    fn ftdi_two_buy_sample_exports_degraded_diagnostic_value() {
        let txs = vec![
            buy_tx("a", "sig-a", ftdi_fingerprint(Some((0, 0)))),
            buy_tx("b", "sig-b", ftdi_fingerprint(Some((1, 0)))),
        ];

        let result = compute_ftdi(txs.iter());

        assert_eq!(result.fee_topology_diversity_index, Some(0.5));
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
    fn dbia_single_non_dev_buyer_exports_degraded_diagnostic_value() {
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

        assert_eq!(result.dev_buyer_infrastructure_affinity, Some(1.0));
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
        assert!(!result.has_full_quality());
        assert_eq!(
            result.degraded_reasons,
            vec![
                SFD_ZERO_PREBALANCE_SKIPPED_REASON.to_string(),
                SFD_PARTIAL_BALANCE_COVERAGE_REASON.to_string()
            ]
        );
    }

    #[test]
    fn sfd_two_usable_samples_materialize_degraded_diagnostic_value() {
        let txs = vec![
            sfd_buy_tx("a", "sig-a", Some(100), Some(10)),
            sfd_buy_tx_with_amount("b", "sig-b", Some(100), None, None),
            sfd_buy_tx("c", "sig-c", Some(100), Some(20)),
        ];

        let result = compute_sfd(txs.iter());

        assert_approx_eq(result.spend_fraction_divergence.unwrap(), 0.05);
        assert_eq!(
            result.degraded_reasons,
            vec![
                SFD_POSTBALANCE_UNAVAILABLE_REASON.to_string(),
                SFD_PARTIAL_BALANCE_COVERAGE_REASON.to_string(),
                SFD_INSUFFICIENT_BUYS_REASON.to_string()
            ]
        );
    }

    #[test]
    fn sfd_missing_postbalance_is_skipped_without_buy_amount_fallback() {
        let txs = vec![
            sfd_buy_tx("a", "sig-a", Some(100), Some(10)),
            sfd_buy_tx_with_amount("b", "sig-b", Some(100), None, Some(20)),
            sfd_buy_tx("c", "sig-c", Some(100), Some(20)),
        ];

        let result = compute_sfd(txs.iter());

        assert_approx_eq(result.spend_fraction_divergence.unwrap(), 0.05);
        assert_eq!(
            result.degraded_reasons,
            vec![
                SFD_POSTBALANCE_UNAVAILABLE_REASON.to_string(),
                SFD_PARTIAL_BALANCE_COVERAGE_REASON.to_string(),
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
            sfd_buy_tx_with_amount("d", "sig-d", Some(100), None, None),
        ];

        let result = compute_sfd(txs.iter());

        assert_eq!(result.spend_fraction_divergence, Some(0.0));
        assert!(!result.has_full_quality());
        assert_eq!(
            result.degraded_reasons,
            vec![
                SFD_POSTBALANCE_UNAVAILABLE_REASON.to_string(),
                SFD_PARTIAL_BALANCE_COVERAGE_REASON.to_string()
            ]
        );
    }

    mod m5_des {
        use super::*;

        fn tape(slots: &[u64], prices: &[f64]) -> Vec<PoolTransaction> {
            assert_eq!(slots.len(), prices.len());
            slots
                .iter()
                .zip(prices)
                .enumerate()
                .map(|(i, (&slot, &price))| {
                    des_buy_tx(
                        &format!("s{i}"),
                        &format!("sig{i}"),
                        Some(slot),
                        Some(i as u32),
                        Some(price),
                        Some(1.0),
                    )
                })
                .collect()
        }

        fn complete() -> Vec<PoolTransaction> {
            tape(&[1, 2, 3, 5, 8], &[10.0, 11.0, 13.2, 17.16, 24.024])
        }

        #[test]
        fn des_increasing_observed_changes_with_longer_next_pauses_yield_positive_tau() {
            let result = compute_des(&complete());
            assert_eq!(result.buy_sample_count, 5);
            assert_eq!(result.signer_sample_count, 5);
            assert_eq!(result.closed_triple_count, 3);
            assert_eq!(result.candidate_triple_count, 3);
            assert_eq!(result.interval_unit, DesIntervalUnitV2::Slots);
            assert_eq!(
                result.price_source,
                DesPriceSourceV2::PumpVirtualPostTradeReserves
            );
            assert!(result.has_full_quality());
            assert_approx_eq(result.demand_elasticity_score.unwrap(), 1.0);
        }

        #[test]
        fn des_independent_observed_changes_and_next_timing_yield_supported_zero() {
            let result = compute_des(&tape(&[1, 2, 3, 3, 4], &[10.0, 11.0, 13.2, 17.16, 24.024]));
            assert!(result.has_full_quality());
            assert_approx_eq(result.demand_elasticity_score.unwrap(), 0.0);
        }

        #[test]
        fn des_same_slot_ordering_is_deterministic_when_tx_index_exists() {
            let ordered = tape(&[1, 1, 2, 2, 4], &[10.0, 11.0, 13.2, 17.16, 24.024]);
            let permuted = [3, 0, 4, 1, 2].map(|i| ordered[i].clone());
            let result = compute_des(&ordered);
            assert!(result.has_full_quality());
            assert_eq!(result, compute_des(&permuted));
            assert_approx_eq(result.demand_elasticity_score.unwrap(), 1.0 / 3.0);
        }

        #[test]
        fn des_same_slot_missing_tx_index_is_not_replaced_by_buffer_order() {
            let mut txs = tape(&[1, 1, 1, 2, 4], &[10.0, 11.0, 13.2, 17.16, 24.024]);
            for tx in &mut txs {
                tx.tx_index = None;
            }
            let result = compute_des(&txs);
            assert_eq!(result.demand_elasticity_score, None);
            assert_eq!(result.degraded_reasons, [DES_SLOT_ORDER_UNAVAILABLE_REASON]);
        }

        #[test]
        fn des_missing_curve_data_returns_none_and_reason_without_bridging() {
            let mut txs = complete();
            txs[2].virtual_sol_reserves = None;
            let result = compute_des(&txs);
            assert_eq!(result.demand_elasticity_score, None);
            assert_eq!(result.closed_triple_count, 0);
            assert_eq!(result.priced_buy_count, 4);
            assert_eq!(
                result.degraded_reasons,
                [
                    DES_CURVE_DATA_UNAVAILABLE_REASON,
                    DES_INSUFFICIENT_TRIPLES_REASON
                ]
            );
        }

        #[test]
        fn des_does_not_use_unverified_price_quote_or_normalized_reserve_fallback() {
            let mut txs = complete();
            for tx in &mut txs {
                tx.price_quote = curve_price(tx);
                tx.virtual_sol_reserves = None;
                tx.virtual_token_reserves = None;
            }
            let result = compute_des(&txs);
            assert_eq!(result.demand_elasticity_score, None);
            assert_eq!(result.closed_triple_count, 0);
            assert!(result
                .degraded_reasons
                .iter()
                .any(|r| r == DES_CURVE_DATA_UNAVAILABLE_REASON));
        }

        #[test]
        fn des_missing_slot_returns_none_and_reason() {
            let mut txs = complete();
            txs[1].slot = None;
            let result = compute_des(&txs);
            assert_eq!(result.demand_elasticity_score, None);
            assert_eq!(result.degraded_reasons, [DES_SLOT_ORDER_UNAVAILABLE_REASON]);
        }

        #[test]
        fn audit_reversed_local_ordinals_do_not_reverse_transaction_index() {
            let mut txs = tape(&[1, 1, 1, 1, 1], &[10.0, 11.0, 13.2, 17.16, 24.024]);
            for (i, tx) in txs.iter_mut().enumerate() {
                tx.event_ordinal = Some(4 - i as u32);
            }
            txs.reverse();
            let refs: Vec<_> = txs.iter().collect();
            let ordered = ordered_buy_samples(&successful_buy_samples(&refs)).unwrap();
            assert_eq!(
                ordered
                    .iter()
                    .map(|v| v.tx.tx_index.unwrap())
                    .collect::<Vec<_>>(),
                [0, 1, 2, 3, 4]
            );
            let result = compute_des(&txs);
            assert_eq!(result.demand_elasticity_score, None);
            assert_eq!(result.closed_triple_count, 3);
            assert_eq!(result.degraded_reasons, [DES_NO_COMPARABLE_PAIRS_REASON]);
            assert!(!result.has_full_quality());
        }

        #[test]
        fn audit_next_lag_reverses_the_old_previous_interval_interpretation() {
            let txs = tape(&[1, 2, 5, 6, 9], &[10.0, 11.0, 14.3, 15.73, 20.449]);
            let result = compute_des(&txs);
            assert!(result.has_full_quality());
            assert_approx_eq(result.demand_elasticity_score.unwrap(), -1.0);
        }

        #[test]
        fn audit_tau_b_counts_ties_only_on_one_axis() {
            let expected = 2.0 / 6.0_f64.sqrt();
            assert_approx_eq(kendall_tau(&[0.1, 0.2, 0.3], &[1, 1, 2]).unwrap(), expected);
            assert_approx_eq(kendall_tau(&[1.0, 1.0, 2.0], &[1, 2, 3]).unwrap(), expected);
            assert_eq!(kendall_tau(&[1.0, 1.0, 2.0], &[1, 1, 2]), Some(1.0));
        }

        #[test]
        fn tau_b_has_no_product_epsilon_and_keeps_integer_slots() {
            assert_eq!(
                kendall_tau(&[1e-100, 2e-100, 3e-100], &[1, 2, 3]),
                Some(1.0)
            );
            assert_eq!(kendall_tau(&[-1e308, 0.0, 1e308], &[1, 2, 3]), Some(1.0));
            let big = 1u64 << 53;
            assert_eq!(
                kendall_tau(&[1.0, 2.0, 3.0], &[big, big + 1, big + 2]),
                Some(1.0)
            );
            assert_approx_eq(
                kendall_tau(&[-0.0, 0.0, 1.0], &[0, 1, 2]).unwrap(),
                2.0 / 6.0_f64.sqrt(),
            );
        }

        #[test]
        fn tau_b_rejects_zero_support_and_invalid_axes() {
            assert_eq!(kendall_tau(&[], &[]), None);
            assert_eq!(kendall_tau(&[1.0], &[1]), None);
            assert_eq!(kendall_tau(&[1.0, 2.0], &[1]), None);
            assert_eq!(kendall_tau(&[1.0, 1.0, 1.0], &[1, 2, 3]), None);
            assert_eq!(kendall_tau(&[1.0, 2.0, 3.0], &[0, 0, 0]), None);
            for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                assert_eq!(kendall_tau(&[1.0, value], &[1, 2]), None);
            }
        }

        #[test]
        fn minimum_is_three_complete_closed_triples_not_buy_count() {
            let txs = complete();
            for n in 0..=5 {
                let result = compute_des(&txs[..n]);
                assert_eq!(result.closed_triple_count, n.saturating_sub(2) as u64);
                assert_eq!(result.demand_elasticity_score.is_some(), n >= 4);
                assert_eq!(result.has_full_quality(), n >= 5);
            }
        }

        #[test]
        fn three_surviving_triples_with_a_gap_are_diagnostic_not_complete() {
            let mut txs = tape(
                &[1, 2, 3, 5, 8, 12, 17, 23],
                &[10.0, 11.0, 14.0, 20.0, 30.0, 50.0, 90.0, 170.0],
            );
            txs[3].virtual_token_reserves = None;
            let result = compute_des(&txs);
            assert_eq!(result.buy_sample_count, 8);
            assert_eq!(result.candidate_triple_count, 6);
            assert_eq!(result.closed_triple_count, 3);
            assert!(result.demand_elasticity_score.is_some());
            assert!(!result.has_full_quality());
            assert_eq!(result.degraded_reasons, [DES_CURVE_DATA_UNAVAILABLE_REASON]);
        }

        #[test]
        fn next_buy_must_have_confirmed_price_domain_and_price_is_not_an_input_change() {
            let mut txs = complete();
            let expected = compute_des(&txs);
            txs[4].virtual_sol_reserves = Some(u64::MAX);
            assert_eq!(compute_des(&txs), expected);
            txs[4].virtual_sol_reserves = None;
            let missing = compute_des(&txs);
            assert_eq!(missing.closed_triple_count, 2);
            assert!(!missing.has_full_quality());
        }

        #[test]
        fn same_pool_and_token_domain_are_required() {
            let mut txs = complete();
            txs[2].pool_amm_id = "other-pool".to_string();
            let mixed = compute_des(&txs);
            assert_eq!(mixed.demand_elasticity_score, None);
            assert_eq!(mixed.degraded_reasons, [DES_PRICE_DOMAIN_MISMATCH_REASON]);
            let mut txs = complete();
            for tx in &mut txs {
                tx.token_mint = Some("mint-a".to_string());
            }
            txs[2].token_mint = Some("mint-b".to_string());
            assert_eq!(
                compute_des(&txs).degraded_reasons,
                [DES_PRICE_DOMAIN_MISMATCH_REASON]
            );
        }

        #[test]
        fn all_delivery_permutations_and_redelivery_preserve_measurement() {
            fn visit(txs: &mut [PoolTransaction], start: usize, expected: &DesComputation) {
                if start == txs.len() {
                    assert_eq!(&compute_des(txs.iter()), expected);
                    return;
                }
                for i in start..txs.len() {
                    txs.swap(start, i);
                    visit(txs, start + 1, expected);
                    txs.swap(start, i);
                }
            }
            let mut txs = complete();
            let expected = compute_des(&txs);
            visit(&mut txs, 0, &expected);
            txs.push(txs[0].clone());
            txs.push(txs[2].clone());
            assert_eq!(compute_des(&txs), expected);
        }

        #[test]
        fn receipt_cutoff_precedes_dedup_and_closing_the_next_buy_triple() {
            let mut txs = complete();
            txs[4].event_time.ingress_wall_ts_ms = Some(1_500);
            let early = compute_des_at_cutoff(&txs, 1_000);
            assert_eq!(early, compute_des_at_cutoff(&txs[..4], 1_000));
            assert_eq!(early.closed_triple_count, 2);
            assert!(!early.has_full_quality());
            assert!(compute_des_at_cutoff(&txs, 1_500).has_full_quality());
            txs.reverse();
            assert_eq!(compute_des_at_cutoff(&txs, 1_000), early);
        }

        #[test]
        fn late_raw_price_enrichment_does_not_rewrite_earlier_measurement() {
            let mut txs = complete();
            let mut enriched = txs[2].clone();
            txs[2].virtual_sol_reserves = None;
            enriched.event_time.ingress_wall_ts_ms = Some(1_500);
            let before = compute_des_at_cutoff(&txs, 1_000);
            assert_eq!(before.closed_triple_count, 0);
            assert!(!same_sybil_view(&txs[2], &enriched));
            txs.push(enriched);
            assert_eq!(compute_des_at_cutoff(&txs, 1_000), before);
            assert!(compute_des_at_cutoff(&txs, 1_500).has_full_quality());
            txs.reverse();
            assert_eq!(compute_des_at_cutoff(&txs, 1_000), before);
            assert!(compute_des_at_cutoff(&txs, 1_500).has_full_quality());
        }

        #[test]
        fn two_partial_raw_views_do_not_invent_a_complete_reserve_pair() {
            let mut txs = complete();
            let mut half = txs[2].clone();
            txs[2].virtual_sol_reserves = None;
            half.virtual_token_reserves = None;
            txs.push(half);
            let result = compute_des(&txs);
            assert_eq!(result.closed_triple_count, 0);
            assert_eq!(result.demand_elasticity_score, None);
        }

        #[test]
        fn conflicting_raw_price_views_only_degrade_des() {
            let mut txs = complete();
            let mut conflict = txs[2].clone();
            conflict.virtual_sol_reserves = conflict.virtual_sol_reserves.map(|x| x + 1);
            txs.push(conflict);
            let result = compute_sybil_resistance_with_ftdi(&txs, Some("s0"));
            assert_eq!(result.des.demand_elasticity_score, None);
            assert_eq!(result.des.degraded_reasons, ["DES_INPUT_CONFLICTING_VIEWS"]);
            assert!(result.ftdi.has_full_quality());
            assert!(result.dbia.has_full_quality());
            assert!(result.sfd.has_full_quality());
        }

        #[test]
        fn unused_normalized_price_fields_cannot_change_or_degrade_raw_des() {
            let mut txs = complete();
            let expected = compute_des(&txs);
            let mut irrelevant_view = txs[2].clone();
            irrelevant_view.price_quote = Some(f64::NAN);
            txs.push(irrelevant_view);
            assert_eq!(compute_des(&txs), expected);
            for tx in &mut txs {
                tx.price_quote = Some(-1.0);
                tx.v_sol_in_bonding_curve = Some(f64::INFINITY);
                tx.v_tokens_in_bonding_curve = Some(0.0);
                tx.curve_data_known = false;
            }
            assert_eq!(compute_des(&txs), expected);
        }

        #[test]
        fn unknown_buy_cannot_be_skipped_to_connect_triples_but_failure_can() {
            let mut txs = complete();
            let expected = compute_des(&txs);
            let mut uncertain = des_buy_tx(
                "unknown",
                "unknown",
                Some(4),
                Some(0),
                Some(10.0),
                Some(1.0),
            );
            uncertain.metadata_availability.status_known = false;
            txs.push(uncertain);
            let missing = compute_des(&txs);
            assert_eq!(missing.demand_elasticity_score, None);
            assert_eq!(missing.closed_triple_count, 0);
            assert_eq!(missing.degraded_reasons, ["DES_INPUT_STATUS_UNAVAILABLE"]);
            let tx = txs.last_mut().unwrap();
            tx.metadata_availability.status_known = true;
            tx.success = false;
            assert_eq!(compute_des(&txs), expected);
        }

        #[test]
        fn unknown_identity_or_receipt_cannot_create_a_closed_sequence() {
            let mut txs = complete();
            txs[2].signature.clear();
            let result = compute_des(&txs);
            assert_eq!(result.closed_triple_count, 0);
            assert_eq!(
                result.degraded_reasons,
                ["DES_INPUT_EVENT_IDENTITY_UNAVAILABLE"]
            );
            let mut txs = complete();
            txs[2].event_time.ingress_wall_ts_ms = None;
            let result = compute_des_at_cutoff(&txs, 1_000);
            assert_eq!(result.closed_triple_count, 0);
            assert_eq!(
                result.degraded_reasons,
                ["DES_INPUT_RECEIPT_TIME_UNAVAILABLE"]
            );
        }

        #[test]
        fn zero_reserve_is_not_clamped_and_extreme_raw_prices_remain_finite() {
            let mut txs = complete();
            for field in [true, false] {
                if field {
                    txs[2].virtual_sol_reserves = Some(0);
                } else {
                    txs[2].virtual_token_reserves = Some(0);
                }
                assert_eq!(compute_des(&txs).closed_triple_count, 0);
            }
            txs[2].virtual_sol_reserves = Some(u64::MAX);
            txs[2].virtual_token_reserves = Some(1);
            assert!(curve_price(&txs[2]).unwrap().is_finite());
            txs[2].virtual_sol_reserves = Some(1);
            txs[2].virtual_token_reserves = Some(u64::MAX);
            assert!(curve_price(&txs[2]).unwrap() > 0.0);
        }

        #[test]
        fn sells_are_not_buy_samples_and_do_not_claim_buy_only_price_attribution() {
            let mut txs = tape(&[1, 2, 3, 5, 8], &[10.0, 9.0, 7.2, 5.04, 4.0]);
            let expected = compute_des(&txs);
            assert!(expected.has_full_quality());
            let mut sell = des_buy_tx("seller", "sell", Some(2), Some(99), Some(8.0), Some(1.0));
            sell.is_buy = false;
            txs.push(sell);
            assert_eq!(compute_des(&txs), expected);
        }

        #[test]
        fn v2_roundtrip_does_not_reinterpret_legacy_scalar_and_records_definition() {
            let result = compute_sybil_resistance_with_ftdi(&complete(), Some("s0"));
            assert!(result.des.has_full_quality());
            assert_eq!(result.features.demand_elasticity_score, None);
            assert_eq!(
                result.features.demand_elasticity_v2.as_ref(),
                Some(&result.des)
            );
            let json = serde_json::to_value(&result.features).unwrap();
            assert!(json.get("demand_elasticity_score").is_none());
            assert_eq!(json["demand_elasticity_v2"]["interval_unit"], "slots");
            assert_eq!(
                json["demand_elasticity_v2"]["definition"],
                "next_buy_slot_tau_b"
            );
            let restored: SybilResistanceFeatures = serde_json::from_value(json).unwrap();
            assert_eq!(restored, result.features);
            let legacy: SybilResistanceFeatures =
                serde_json::from_value(serde_json::json!({"demand_elasticity_score": 0.25}))
                    .unwrap();
            assert_eq!(legacy.demand_elasticity_score, Some(0.25));
            assert!(legacy.demand_elasticity_v2.is_none());
        }

        #[test]
        fn view_history_loss_degrades_the_same_evidence_that_mfs_receives() {
            let mut result = compute_sybil_resistance_with_ftdi(&complete(), None);
            result.mark_view_history_unavailable();
            assert!(!result.des.has_full_quality());
            assert!(!result
                .features
                .demand_elasticity_v2
                .as_ref()
                .unwrap()
                .has_full_quality());
            assert_eq!(
                result.features.demand_elasticity_v2.as_ref(),
                Some(&result.des)
            );
        }
    }

    mod m1_audit_regressions {
        use super::*;
        #[test]
        fn ftdi_representative_should_follow_chain_order_not_delivery_order() {
            let mut a0 = buy_tx("a", "a0", ftdi_fingerprint(Some((0, 0))));
            a0.tx_index = Some(0);
            let mut a1 = buy_tx("a", "a1", ftdi_fingerprint(Some((1, 0))));
            a1.tx_index = Some(1);
            let b = buy_tx("b", "b", ftdi_fingerprint(Some((0, 0))));
            let c = buy_tx("c", "c", ftdi_fingerprint(Some((0, 0))));
            let x = compute_ftdi([&a0, &a1, &b, &c]);
            let y = compute_ftdi([&a1, &a0, &b, &c]);
            println!("FTDI ORDER: {x:?} / {y:?}");
            assert_eq!(
                x.fee_topology_diversity_index,
                y.fee_topology_diversity_index
            );
        }

        #[test]
        fn dbia_representative_should_follow_chain_order_not_delivery_order() {
            let shared = dbia_fingerprint(12, 3, true, true, 2, (0, 0));
            let mut d0 = dbia_buy_tx("dev", "d0", true, shared.clone());
            d0.tx_index = Some(0);
            let mut d1 = dbia_buy_tx(
                "dev",
                "d1",
                true,
                dbia_fingerprint(20, 5, false, false, 7, (3, 1)),
            );
            d1.tx_index = Some(1);
            let b = dbia_buy_tx("b", "b", false, shared.clone());
            let c = dbia_buy_tx("c", "c", false, shared);
            let x = compute_dbia([&d0, &d1, &b, &c], Some("dev"));
            let y = compute_dbia([&d1, &d0, &b, &c], Some("dev"));
            println!("DBIA ORDER: {x:?} / {y:?}");
            assert_eq!(
                x.dev_buyer_infrastructure_affinity,
                y.dev_buyer_infrastructure_affinity
            );
        }

        #[test]
        fn sfd_real_exact_sample_must_outrank_fallback_disguised_as_complete() {
            let bad = sfd_buy_tx_with_amount("a", "a0", Some(100), Some(150), Some(10));
            let good = sfd_buy_tx_with_amount("a", "a1", Some(100), Some(20), Some(80));
            let b = sfd_buy_tx("b", "b", Some(100), Some(60));
            let c = sfd_buy_tx("c", "c", Some(100), Some(10));
            let expected = compute_sfd([&good, &b, &c]);
            let actual = compute_sfd([&bad, &good, &b, &c]);
            println!("SFD BEST SAMPLE: expected={expected:?}; actual={actual:?}");
            assert!(
                actual.degraded_reasons.is_empty(),
                "pełna próbka jest dostępna"
            );
            assert_approx_eq(
                actual.spend_fraction_divergence.unwrap(),
                expected.spend_fraction_divergence.unwrap(),
            );
        }

        #[test]
        fn des_transaction_index_must_precede_transaction_local_ordinal() {
            let mut txs = Vec::new();
            for i in 0..4 {
                let mut t = des_buy_tx(
                    &format!("s{i}"),
                    &format!("sig{i}"),
                    Some(1),
                    Some(3 - i),
                    Some(10.0 + i as f64),
                    Some(1.0),
                );
                t.tx_index = Some(i);
                t.event_ordinal = Some(3 - i);
                txs.push(t);
            }
            let refs: Vec<_> = txs.iter().collect();
            let samples = successful_buy_samples(&refs);
            let ordered = ordered_buy_samples(&samples).unwrap();
            let indexes: Vec<_> = ordered
                .iter()
                .map(|sample| sample.tx.tx_index.unwrap())
                .collect();
            println!("DES BLOCK ORDER: {indexes:?}");
            assert_eq!(indexes, vec![0, 1, 2, 3]);
        }
    }

    #[test]
    fn m1_first_complete_feature_wins_for_ftdi_and_dev_reference() {
        let fp = dbia_fingerprint(12, 3, true, true, 2, (0, 0));
        let missing = dbia_buy_tx("dev", "missing", true, ToolchainFingerprintInput::default());
        let dev = dbia_buy_tx("dev", "complete", true, fp.clone());
        let b = dbia_buy_tx("b", "b", false, fp.clone());
        let c = dbia_buy_tx("c", "c", false, fp);
        for txs in [[&missing, &dev, &b, &c], [&c, &dev, &b, &missing]] {
            let ftdi = compute_ftdi(txs);
            assert_eq!(ftdi.fee_topology_diversity_index, Some(0.0));
            assert!(ftdi.degraded_reasons.is_empty());
            let dbia = compute_dbia(txs, Some("dev"));
            assert_eq!(dbia.dev_buyer_infrastructure_affinity, Some(1.0));
            assert!(dbia.degraded_reasons.is_empty());
        }
    }

    #[test]
    fn m1_redelivery_and_all_permutations_preserve_values_and_counts() {
        let a = buy_tx("a", "a", ftdi_fingerprint(Some((0, 0))));
        let later = buy_tx("a", "later", ftdi_fingerprint(Some((2, 0))));
        let b = buy_tx("b", "b", ftdi_fingerprint(Some((0, 0))));
        let c = buy_tx("c", "c", ftdi_fingerprint(Some((1, 0))));
        let txs = [&a, &later, &b, &c];
        let expected = compute_ftdi(txs);
        for i in 0..4 {
            for j in 0..4 {
                for k in 0..4 {
                    for l in 0..4 {
                        let indices = [i, j, k, l];
                        if indices.iter().copied().collect::<HashSet<_>>().len() != 4 {
                            continue;
                        }
                        let permuted = indices.map(|index| txs[index]);
                        assert_eq!(compute_ftdi(permuted), expected);
                        assert_eq!(compute_ftdi(permuted.into_iter().chain([&a, &b])), expected);
                    }
                }
            }
        }
    }

    #[test]
    fn m1_unknown_status_keeps_window_incomplete_but_known_failure_does_not() {
        let a = buy_tx("a", "a", ftdi_fingerprint(Some((0, 0))));
        let b = buy_tx("b", "b", ftdi_fingerprint(Some((0, 0))));
        let c = buy_tx("c", "c", ftdi_fingerprint(Some((1, 0))));
        let mut unknown = buy_tx("d", "d", ftdi_fingerprint(Some((4, 0))));
        unknown.metadata_availability.status_known = false;
        let measured = compute_ftdi([&a, &b, &c]);
        let result = compute_ftdi([&a, &b, &c, &unknown]);
        assert_eq!(
            result.fee_topology_diversity_index,
            measured.fee_topology_diversity_index
        );
        assert_eq!(result.buy_sample_count, 3);
        assert!(!result.legacy_buy_tx_actionable);
        assert!(result
            .degraded_reasons
            .contains(&"FTDI_INPUT_STATUS_UNAVAILABLE".to_string()));
        unknown.metadata_availability.status_known = true;
        unknown.success = false;
        assert_eq!(compute_ftdi([&a, &b, &c, &unknown]), measured);
    }

    #[test]
    fn m1_cutoff_precedes_selection_and_deduplication() {
        let a = buy_tx("a", "a", ftdi_fingerprint(Some((0, 0))));
        let b = buy_tx("b", "b", ftdi_fingerprint(Some((0, 0))));
        let c = buy_tx("c", "c", ftdi_fingerprint(Some((0, 0))));
        let mut late = buy_tx("a", "late", ftdi_fingerprint(Some((3, 0))));
        late.slot = Some(0);
        late.event_time.ingress_wall_ts_ms = Some(1_001);
        late.event_time.chain_event_ts_ms = Some(999);
        late.arrival_ts_ms = 1;
        let compute = |txs: Vec<&PoolTransaction>, cutoff| {
            compute_sybil_resistance_with_ftdi_at_cutoff(txs, None, cutoff).ftdi
        };
        let before = compute(vec![&a, &b, &c], 1_000);
        assert_eq!(compute(vec![&late, &c, &a, &b, &a], 1_000), before);
        assert_ne!(
            compute(vec![&late, &a, &b, &c], 1_001).fee_topology_diversity_index,
            before.fee_topology_diversity_index
        );
        let mut missing_receipt = late.clone();
        missing_receipt.event_time.ingress_wall_ts_ms = None;
        let result = compute(vec![&a, &b, &c, &missing_receipt], 1_001);
        assert_eq!(result.buy_sample_count, 3);
        assert!(result
            .degraded_reasons
            .contains(&"FTDI_INPUT_RECEIPT_TIME_UNAVAILABLE".to_string()));
    }

    #[test]
    fn m1_local_ordinals_and_cpi_do_not_order_distinct_transactions() {
        let mut a = buy_tx("a", "a", ftdi_fingerprint(Some((0, 0))));
        let mut later = buy_tx("a", "later", ftdi_fingerprint(Some((2, 0))));
        a.tx_index = None;
        later.tx_index = None;
        a.event_ordinal = Some(0);
        later.event_ordinal = Some(10);
        a.cpi_stack_height = Some(1);
        later.cpi_stack_height = Some(5);
        let b = buy_tx("b", "b", ftdi_fingerprint(Some((0, 0))));
        let c = buy_tx("c", "c", ftdi_fingerprint(Some((0, 0))));
        let x = compute_ftdi([&a, &later, &b, &c]);
        let y = compute_ftdi([&c, &later, &a, &b]);
        assert_eq!(x, y);
        assert_eq!(x.fee_topology_diversity_index, None);
        assert!(x
            .degraded_reasons
            .contains(&"FTDI_INPUT_ORDER_UNAVAILABLE".to_string()));
        later.signature = a.signature.clone();
        assert_eq!(canonical_buy_order(&a, &later), None);
        a.outer_instruction_index = Some(0);
        later.outer_instruction_index = Some(0);
        a.inner_instruction_path = Some(vec![0]);
        later.inner_instruction_path = Some(vec![1]);
        assert_eq!(canonical_buy_order(&a, &later), Some(Ordering::Less));
        let same_transaction = compute_ftdi([&later, &c, &a, &b]);
        assert_eq!(same_transaction.buy_sample_count, 4);
        assert_eq!(same_transaction.fee_topology_diversity_index, Some(0.0));
    }

    #[test]
    fn m1_missing_dev_is_not_replaced_and_legacy_pool_is_unknown() {
        let fp = dbia_fingerprint(12, 3, true, true, 2, (0, 0));
        let a = buy_tx("a", "a", fp.clone());
        let b = buy_tx("b", "b", fp.clone());
        let c = buy_tx("c", "c", fp);
        assert_eq!(
            compute_dbia([&a, &b, &c], None).dev_buyer_infrastructure_affinity,
            None
        );
        assert_eq!(
            compute_dbia([&a, &b, &c], Some("dev")).dev_buyer_infrastructure_affinity,
            None
        );
        let mut json = serde_json::to_value(a).unwrap();
        json.as_object_mut()
            .unwrap()
            .remove("metadata_availability");
        let old: PoolTransaction = serde_json::from_value(json.clone()).unwrap();
        assert!(!old.is_confirmed_success());
        json.as_object_mut().unwrap().remove("success");
        let old: PoolTransaction = serde_json::from_value(json).unwrap();
        assert!(!old.success);
    }
    #[test]
    fn m1_redelivery_with_new_ingress_time_keeps_event_identity() {
        let mut a = buy_tx("a", "a", ftdi_fingerprint(Some((0, 0))));
        a.event_time.chain_event_ts_ms = None;
        let b = buy_tx("b", "b", ftdi_fingerprint(Some((0, 0))));
        let c = buy_tx("c", "c", ftdi_fingerprint(Some((1, 0))));
        let before = compute_ftdi([&a, &b, &c]);
        let mut redelivery = a.clone();
        redelivery.event_time.ingress_wall_ts_ms = Some(2_000);
        redelivery.timestamp_ms = 2_000;
        redelivery.arrival_ts_ms = 2_000;
        assert_eq!(compute_ftdi([&redelivery, &b, &a, &c]), before);
        let mut missing_identity = a.clone();
        missing_identity.event_ordinal = None;
        let result = compute_ftdi([&a, &b, &c, &missing_identity]);
        assert_eq!(result.buy_sample_count, 3);
        assert!(!result.legacy_buy_tx_actionable);
        assert!(result
            .degraded_reasons
            .contains(&"FTDI_INPUT_EVENT_IDENTITY_UNAVAILABLE".to_string()));
    }
    #[test]
    fn m1_missing_inner_metadata_does_not_discard_complete_balance_metric() {
        let mut txs: Vec<_> = (0..3)
            .map(|i| {
                sfd_buy_tx(
                    &format!("buyer-{i}"),
                    &format!("signature-{i}"),
                    Some(100),
                    Some(70 + i),
                )
            })
            .collect();
        for tx in &mut txs {
            tx.metadata_availability.inner_instructions_known = false;
        }
        let result = compute_sybil_resistance(txs.iter(), None);
        assert_eq!(result.fee_topology_diversity_index, None);
        assert!(result.spend_fraction_divergence.is_some());
        assert!(!result
            .degraded_reasons
            .iter()
            .any(|reason| reason.starts_with("SFD_")));
    }
    fn m2_topology_batch(counts: &[usize]) -> Vec<PoolTransaction> {
        counts
            .iter()
            .enumerate()
            .flat_map(|(class, count)| {
                (0..*count).map(move |index| {
                    let id = format!("m2-{class}-{index}");
                    buy_tx(&id, &id, ftdi_fingerprint(Some((class as u32, 0))))
                })
            })
            .collect()
    }

    #[test]
    fn m2_ftdi_one_topology_is_zero_not_inverse_population() {
        for n in [1, 2, 3, 100] {
            let txs = m2_topology_batch(&[n]);
            let result = compute_ftdi(&txs);
            assert_eq!(result.fee_topology_diversity_index, Some(0.0));
            assert_eq!(result.coordination_hhi, Some(1.0));
            assert_eq!(result.represented_signer_count, n as u64);
            assert_eq!(result.signer_sample_count, n as u64);
            assert_eq!(result.has_full_quality(), n >= 3);
        }
    }

    #[test]
    fn m2_ftdi_five_equal_classes_are_point_eight_for_five_and_hundred() {
        for counts in [[1; 5], [20; 5]] {
            let txs = m2_topology_batch(&counts);
            let result = compute_ftdi(&txs);
            assert_eq!(result.coordination_hhi, Some(0.2));
            assert_eq!(result.fee_topology_diversity_index, Some(0.8));
            assert!(result.has_full_quality());
            assert_eq!(result.unique_topology_count, 5);
            assert_eq!(compute_ftdi(txs.iter().rev()), result);
            assert_eq!(compute_ftdi(txs.iter().chain(txs.iter())), result);
        }
    }

    #[test]
    fn m2_ftdi_measures_class_distribution_not_only_class_count() {
        let even = compute_ftdi(&m2_topology_batch(&[5, 5]));
        let skewed = compute_ftdi(&m2_topology_batch(&[9, 1]));
        let scaled = compute_ftdi(&m2_topology_batch(&[90, 10]));
        assert_eq!(even.fee_topology_diversity_index, Some(0.5));
        assert_approx_eq(skewed.fee_topology_diversity_index.unwrap(), 0.18);
        assert_eq!(
            scaled.fee_topology_diversity_index,
            skewed.fee_topology_diversity_index
        );
    }

    #[test]
    fn m2_integer_hhi_is_order_independent_and_checks_overflow() {
        assert_eq!(coordination_hhi_from_counts([]), None);
        assert_eq!(coordination_hhi_from_counts([1, 0]), None);
        assert_eq!(coordination_hhi_from_counts([u64::MAX]), Some(1.0));
        assert_eq!(coordination_hhi_from_counts([u64::MAX, 1]), None);
        let counts = [3u64, 7, 11, 13];
        let expected = Some(348.0 / 1156.0);
        for i in 0..4 {
            for j in 0..4 {
                for k in 0..4 {
                    for l in 0..4 {
                        let indices = [i, j, k, l];
                        if indices.iter().copied().collect::<HashSet<_>>().len() == 4 {
                            assert_eq!(
                                coordination_hhi_from_counts(indices.map(|index| counts[index])),
                                expected
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn m2_repeated_buyers_do_not_raise_ftdi_quality() {
        let mut txs = m2_topology_batch(&[1, 1]);
        for index in 0..20 {
            txs.push(buy_tx(
                "m2-0-0",
                &format!("repeat-{index}"),
                ftdi_fingerprint(Some((0, 0))),
            ));
        }
        let result = compute_ftdi(&txs);
        assert_eq!(result.fee_topology_diversity_index, Some(0.5));
        assert_eq!(result.represented_signer_count, 2);
        assert_eq!(result.signer_sample_count, 2);
        assert_eq!(result.buy_sample_count, 22);
        assert!(!result.has_full_quality());
        assert!(result
            .degraded_reasons
            .contains(&FTDI_INSUFFICIENT_BUYS_REASON.to_string()));
    }

    #[test]
    fn m2_coverage_counts_missing_and_ambiguous_signers_without_losing_valid_ones() {
        let fp = dbia_fingerprint(12, 3, true, true, 2, (0, 0));
        let dev = dbia_buy_tx("dev", "dev", true, fp.clone());
        let a = buy_tx("a", "a", fp.clone());
        let mut b = buy_tx("b", "b", fp.clone());
        b.toolchain_fingerprint = ToolchainFingerprintInput::default();
        let ftdi = compute_ftdi([&dev, &a, &b]);
        let dbia = compute_dbia([&dev, &a, &b], Some("dev"));
        assert_eq!(
            (ftdi.represented_signer_count, ftdi.signer_sample_count),
            (2, 3)
        );
        assert_eq!(
            (dbia.represented_signer_count, dbia.signer_sample_count),
            (2, 3)
        );
        assert!(!ftdi.has_full_quality());
        assert!(!dbia.has_full_quality());
        assert_eq!(ftdi.fee_topology_diversity_index, None);
        assert_eq!(dbia.dev_buyer_infrastructure_affinity, None);
        b.toolchain_fingerprint = fp.clone();
        b.tx_index = None;
        let mut other_b = buy_tx("b", "other-b", fp);
        other_b.tx_index = None;
        for txs in [[&dev, &a, &b, &other_b], [&other_b, &b, &a, &dev]] {
            let result = compute_sybil_resistance_with_ftdi(txs, Some("dev"));
            assert_eq!(
                (
                    result.ftdi.represented_signer_count,
                    result.ftdi.signer_sample_count
                ),
                (2, 3)
            );
            assert_eq!(
                (
                    result.dbia.represented_signer_count,
                    result.dbia.signer_sample_count
                ),
                (2, 3)
            );
            assert!(!result.ftdi.has_full_quality());
            assert!(!result.dbia.has_full_quality());
        }
    }

    #[test]
    fn m2_dbia_keeps_each_existing_feature_weight() {
        let fp = dbia_fingerprint(12, 3, true, true, 2, (0, 0));
        let dev = dbia_buy_tx("dev", "dev", true, fp.clone());
        let fields: [fn(&mut ToolchainFingerprintInput); 6] = [
            |fp| fp.account_keys_len = Some(13),
            |fp| fp.outer_instruction_count = Some(4),
            |fp| fp.has_set_compute_unit_limit = Some(false),
            |fp| fp.has_set_compute_unit_price = Some(false),
            |fp| fp.inner_instruction_group_count = Some(3),
            |fp| fp.external_fee_transfer_count = Some(1),
        ];
        for (modify, weight) in fields.into_iter().zip([0.20, 0.25, 0.05, 0.05, 0.25, 0.20]) {
            let mut changed = fp.clone();
            modify(&mut changed);
            let a = buy_tx("a", "a", changed.clone());
            let b = buy_tx("b", "b", changed);
            let result = compute_dbia([&dev, &a, &b], Some("dev"));
            assert_approx_eq(
                result.dev_buyer_infrastructure_affinity.unwrap(),
                1.0 - weight,
            );
            assert!(result.has_full_quality());
        }
    }

    #[test]
    fn m2_dbia_buyers_are_equal_weight_and_dev_is_not_in_the_mean() {
        let fp = dbia_fingerprint(12, 3, true, true, 2, (0, 0));
        let dev = dbia_buy_tx("dev", "dev", true, fp.clone());
        let a = buy_tx("a", "a", fp.clone());
        let b = buy_tx("b", "b", dbia_fingerprint(20, 8, false, false, 5, (4, 4)));
        let repeats: Vec<_> = (0..20)
            .map(|index| buy_tx("a", &format!("repeat-{index}"), fp.clone()))
            .collect();
        let result = compute_dbia(
            [&dev, &a, &b].into_iter().chain(repeats.iter()),
            Some("dev"),
        );
        assert_approx_eq(result.dev_buyer_infrastructure_affinity.unwrap(), 0.5);
        assert_eq!(
            (result.represented_signer_count, result.signer_sample_count),
            (3, 3)
        );
        assert!(result.has_full_quality());
        let small = compute_dbia([&dev, &a].into_iter().chain(repeats.iter()), Some("dev"));
        assert_eq!(small.dev_buyer_infrastructure_affinity, Some(1.0));
        assert!(!small.has_full_quality());
        assert_eq!(
            (small.represented_signer_count, small.signer_sample_count),
            (2, 2)
        );
        assert!(!compute_dbia([&a, &b], Some("dev")).has_full_quality());
    }

    #[test]
    fn m2_missing_input_never_becomes_full_quality_despite_enough_representatives() {
        let fp = dbia_fingerprint(12, 3, true, true, 2, (0, 0));
        let dev = dbia_buy_tx("dev", "dev", true, fp.clone());
        let a = buy_tx("a", "a", fp.clone());
        let b = buy_tx("b", "b", fp.clone());
        let mut unknown = buy_tx("unknown", "unknown", fp);
        unknown.metadata_availability.status_known = false;
        let result = compute_sybil_resistance_with_ftdi([&dev, &a, &b, &unknown], Some("dev"));
        assert_eq!(result.ftdi.fee_topology_diversity_index, Some(0.0));
        assert_eq!(result.dbia.dev_buyer_infrastructure_affinity, Some(1.0));
        assert_eq!(result.ftdi.represented_signer_count, 3);
        assert_eq!(result.dbia.represented_signer_count, 3);
        assert!(!result.ftdi.has_full_quality());
        assert!(!result.dbia.has_full_quality());
    }
    #[test]
    fn m3_audit_missing_postbalance_cannot_produce_mad_one_point_nine() {
        let mut txs = vec![
            sfd_buy_tx_with_amount("a", "a", Some(100), None, Some(10)),
            sfd_buy_tx_with_amount("b", "b", Some(100), None, Some(200)),
            sfd_buy_tx_with_amount("c", "c", Some(100), None, Some(400)),
        ];
        let result = compute_sfd(&txs);
        assert_eq!(result.spend_fraction_divergence, None);
        assert_eq!(
            (result.represented_signer_count, result.signer_sample_count),
            (0, 3)
        );
        assert!(!result.has_full_quality());
        assert!(result
            .degraded_reasons
            .contains(&SFD_POSTBALANCE_UNAVAILABLE_REASON.to_string()));
        for volume in [0.0, 10.0, -1.0, f64::NAN, f64::INFINITY, f64::MAX] {
            for tx in &mut txs {
                tx.sol_amount_lamports = Some(u64::MAX);
                tx.volume_sol = volume;
            }
            assert_eq!(compute_sfd(&txs), result);
        }
    }

    #[test]
    fn m3_fraction_validity_and_mad_ranges_include_extreme_balances() {
        let mut tx = sfd_buy_tx("a", "a", Some(1), Some(1));
        for pre in 0..=100u64 {
            for post in 0..=101u64 {
                tx.signer_pre_balance_lamports = Some(pre);
                tx.signer_post_balance_lamports = Some(post);
                let fraction = sfd_spend_fraction(&tx);
                assert_eq!(fraction.is_some(), pre > 0 && post <= pre);
                if let Some(fraction) = fraction {
                    assert!(fraction.is_finite() && (0.0..=1.0).contains(&fraction));
                }
            }
        }
        tx.signer_pre_balance_lamports = Some(u64::MAX);
        for post in [0, 1, u64::MAX / 2, u64::MAX - 1, u64::MAX] {
            tx.signer_post_balance_lamports = Some(post);
            assert!((0.0..=1.0).contains(&sfd_spend_fraction(&tx).unwrap()));
        }
        let mut txs: Vec<_> = (0..4)
            .map(|i| sfd_buy_tx(&format!("s{i}"), &format!("t{i}"), Some(4), Some(0)))
            .collect();
        for a in 0..=4 {
            for b in 0..=4 {
                for c in 0..=4 {
                    for d in 0..=4 {
                        for (tx, post) in txs.iter_mut().zip([a, b, c, d]) {
                            tx.signer_post_balance_lamports = Some(post);
                        }
                        for n in 2..=4 {
                            let result = compute_sfd(&txs[..n]);
                            let mad = result.spend_fraction_divergence.unwrap();
                            assert!(mad.is_finite() && (0.0..=0.5).contains(&mad));
                            assert_eq!(result.has_full_quality(), n >= 3);
                        }
                    }
                }
            }
        }
        txs[0].signer_post_balance_lamports = Some(0);
        txs[1].signer_post_balance_lamports = Some(4);
        assert_eq!(compute_sfd(&txs[..2]).spend_fraction_divergence, Some(0.5));
    }

    #[test]
    fn m3_later_valid_buy_wins_without_inheriting_earlier_bad_balance_reasons() {
        let mut bad = sfd_buy_tx_with_amount("a", "a0", Some(100), None, Some(400));
        let good = sfd_buy_tx("a", "a1", Some(100), Some(20));
        let b = sfd_buy_tx("b", "b", Some(100), Some(60));
        let c = sfd_buy_tx("c", "c", Some(100), Some(10));
        let expected = compute_sfd([&good, &b, &c]);
        for (pre, post) in [
            (Some(100), None),
            (None, Some(20)),
            (Some(100), Some(150)),
            (Some(0), Some(0)),
        ] {
            bad.signer_pre_balance_lamports = pre;
            bad.signer_post_balance_lamports = post;
            let result = compute_sfd([&bad, &good, &b, &c]);
            assert_eq!(
                result.spend_fraction_divergence,
                expected.spend_fraction_divergence
            );
            assert_eq!(
                (result.represented_signer_count, result.signer_sample_count),
                (3, 3)
            );
            assert!(result.has_full_quality());
            assert_eq!(compute_sfd([&c, &good, &bad, &b, &good]), result);
        }
    }

    #[test]
    fn m3_never_stitches_balances_across_transactions_or_signers() {
        let txs = [
            sfd_buy_tx("a", "a-pre", Some(100), None),
            sfd_buy_tx("a", "a-post", None, Some(20)),
            sfd_buy_tx("b", "b", Some(100), None),
            sfd_buy_tx("c", "c", None, Some(20)),
        ];
        let result = compute_sfd(&txs);
        assert_eq!(result.spend_fraction_divergence, None);
        assert_eq!(
            (result.represented_signer_count, result.signer_sample_count),
            (0, 3)
        );
        assert_eq!(result.buy_sample_count, 4);
        assert!(!result.has_full_quality());
    }

    #[test]
    fn m3_minimum_sample_depends_on_valid_distinct_signers_not_buy_count() {
        let a = sfd_buy_tx("a", "a", Some(100), Some(100));
        let b = sfd_buy_tx("b", "b", Some(100), Some(0));
        let c = sfd_buy_tx("c", "c", Some(100), Some(50));
        assert_eq!(compute_sfd([]).spend_fraction_divergence, None);
        let repeats: Vec<_> = (0..10)
            .map(|i| sfd_buy_tx("a", &format!("r{i}"), Some(100), Some(0)))
            .collect();
        let single = compute_sfd([&a].into_iter().chain(repeats.iter()));
        assert_eq!(single.spend_fraction_divergence, None);
        assert_eq!(
            (single.represented_signer_count, single.signer_sample_count),
            (1, 1)
        );
        let two = compute_sfd([&a, &b].into_iter().chain(repeats.iter()));
        assert_eq!(two.spend_fraction_divergence, Some(0.5));
        assert!(!two.has_full_quality());
        assert!(two
            .degraded_reasons
            .contains(&SFD_INSUFFICIENT_BUYS_REASON.to_string()));
        let three = compute_sfd([&a, &b, &c].into_iter().chain(repeats.iter()));
        assert_eq!(three.spend_fraction_divergence, Some(0.5));
        assert!(three.has_full_quality());
        assert_eq!(
            (three.represented_signer_count, three.signer_sample_count),
            (3, 3)
        );
    }

    #[test]
    fn m3_partial_and_ambiguous_balance_coverage_remain_diagnostic() {
        let a = sfd_buy_tx("a", "a", Some(100), Some(90));
        let b = sfd_buy_tx("b", "b", Some(100), Some(80));
        let c = sfd_buy_tx("c", "c", Some(100), Some(10));
        let mut omitted = sfd_buy_tx("d", "d", Some(100), Some(200));
        let full = compute_sfd([&a, &b, &c]);
        let partial = compute_sfd([&a, &b, &c, &omitted]);
        assert_eq!(
            partial.spend_fraction_divergence,
            full.spend_fraction_divergence
        );
        assert_eq!(
            (
                partial.represented_signer_count,
                partial.signer_sample_count
            ),
            (3, 4)
        );
        assert!(!partial.has_full_quality());
        assert!(partial
            .degraded_reasons
            .contains(&SFD_INVALID_BALANCE_PAIR_REASON.to_string()));
        omitted.signer_post_balance_lamports = Some(20);
        omitted.tx_index = None;
        let mut ambiguous = sfd_buy_tx("d", "d1", Some(100), Some(60));
        ambiguous.tx_index = None;
        let result = compute_sfd([&a, &b, &c, &omitted, &ambiguous]);
        assert_eq!(
            result.spend_fraction_divergence,
            full.spend_fraction_divergence
        );
        assert_eq!(
            (result.represented_signer_count, result.signer_sample_count),
            (3, 4)
        );
        assert!(result
            .degraded_reasons
            .contains(&"SFD_INPUT_ORDER_UNAVAILABLE".to_string()));
        assert!(result
            .degraded_reasons
            .contains(&SFD_PARTIAL_BALANCE_COVERAGE_REASON.to_string()));
        assert!(!result.has_full_quality());
        assert_eq!(compute_sfd([&ambiguous, &b, &omitted, &c, &a]), result);
    }

    #[test]
    fn m3_cutoff_prevents_late_valid_balance_from_repairing_previous_sfd() {
        let bad = sfd_buy_tx("a", "a0", Some(100), None);
        let b = sfd_buy_tx("b", "b", Some(100), Some(60));
        let c = sfd_buy_tx("c", "c", Some(100), Some(10));
        let mut late = sfd_buy_tx("a", "a1", Some(100), Some(20));
        late.event_time.chain_event_ts_ms = Some(900);
        late.event_time.ingress_wall_ts_ms = Some(1_001);
        late.arrival_ts_ms = 1;
        let before = compute_sybil_resistance_with_ftdi_at_cutoff([&bad, &b, &c], None, 1_000).sfd;
        assert_eq!(
            (before.represented_signer_count, before.signer_sample_count),
            (2, 3)
        );
        assert_eq!(
            compute_sybil_resistance_with_ftdi_at_cutoff([&late, &c, &bad, &b], None, 1_000).sfd,
            before
        );
        let after =
            compute_sybil_resistance_with_ftdi_at_cutoff([&late, &c, &bad, &b], None, 1_001).sfd;
        assert_eq!(
            (after.represented_signer_count, after.signer_sample_count),
            (3, 3)
        );
        assert!(after.has_full_quality());
        let mut no_receipt = late.clone();
        no_receipt.event_time.ingress_wall_ts_ms = None;
        let missing =
            compute_sybil_resistance_with_ftdi_at_cutoff([&no_receipt, &bad, &b, &c], None, 1_001)
                .sfd;
        assert_eq!(
            missing.spend_fraction_divergence,
            before.spend_fraction_divergence
        );
        assert!(missing
            .degraded_reasons
            .contains(&"SFD_INPUT_RECEIPT_TIME_UNAVAILABLE".to_string()));
    }

    #[test]
    fn m3_sfd_preserves_m1_execution_order_dedup_and_status_contracts() {
        let mut a0 = sfd_buy_tx("a", "a0", Some(100), Some(90));
        let mut a1 = sfd_buy_tx("a", "a1", Some(100), Some(10));
        a0.tx_index = Some(0);
        a0.event_ordinal = Some(9);
        a1.tx_index = Some(1);
        a1.event_ordinal = Some(0);
        let b = sfd_buy_tx("b", "b", Some(100), Some(80));
        let c = sfd_buy_tx("c", "c", Some(100), Some(70));
        let expected = compute_sfd([&a0, &a1, &b, &c]);
        assert_approx_eq(expected.spend_fraction_divergence.unwrap(), 0.1);
        let txs = [&a0, &a1, &b, &c];
        for i in 0..4 {
            for j in 0..4 {
                for k in 0..4 {
                    for l in 0..4 {
                        let indices = [i, j, k, l];
                        if indices.iter().copied().collect::<HashSet<_>>().len() == 4 {
                            assert_eq!(
                                compute_sfd(indices.map(|i| txs[i]).into_iter().chain([&a0, &b])),
                                expected
                            );
                        }
                    }
                }
            }
        }
        let mut ignored = sfd_buy_tx("ignored", "ignored", None, None);
        ignored.is_buy = false;
        assert_eq!(compute_sfd(txs.into_iter().chain([&ignored])), expected);
        ignored.is_buy = true;
        ignored.success = false;
        assert_eq!(compute_sfd(txs.into_iter().chain([&ignored])), expected);
        ignored.metadata_availability.status_known = false;
        let unknown = compute_sfd(txs.into_iter().chain([&ignored]));
        assert_eq!(
            unknown.spend_fraction_divergence,
            expected.spend_fraction_divergence
        );
        assert!(!unknown.has_full_quality());
        assert!(unknown
            .degraded_reasons
            .contains(&"SFD_INPUT_STATUS_UNAVAILABLE".to_string()));
    }
    #[test]
    fn m3_complete_net_sol_estimator_is_independent_of_buy_amount_and_volume() {
        let mut txs = vec![
            sfd_buy_tx("a", "a", Some(100), Some(99)),
            sfd_buy_tx("b", "b", Some(100), Some(50)),
            sfd_buy_tx("c", "c", Some(100), Some(1)),
        ];
        let expected = compute_sfd(&txs);
        assert_approx_eq(expected.spend_fraction_divergence.unwrap(), 0.49);
        assert!(expected.has_full_quality());
        for volume in [0.0, 1e20, f64::NAN, f64::INFINITY] {
            for tx in &mut txs {
                tx.volume_sol = volume;
                tx.sol_amount_lamports = Some(u64::MAX);
            }
            assert_eq!(compute_sfd(&txs), expected);
        }
    }
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) mod review_r1_r3_regressions {
    use crate::events::{PoolTransaction, RawBytesMissingReason};
    use crate::tx_intelligence::*;
    use ghost_core::{CurveFinality, EventSemanticEnvelope, EventTimeMetadata};
    use seer::binary_parser::{BinaryParser, DISC_BUY};
    use seer::grpc_connection::PUMP_FUN_PROGRAM_ID;
    use seer::types::{GeyserEvent, ToolchainFingerprintInput};
    use solana_sdk::{
        pubkey::Pubkey,
        signature::{Keypair, Signature, Signer},
    };
    use std::{collections::HashMap, str::FromStr};
    pub mod types {
        pub use seer::types::*;
    }
    const PUMP_IDX_MINT: usize = 2;
    const PUMP_IDX_BONDING_CURVE: usize = 3;
    const PUMP_IDX_USER: usize = 6;
    const PUMP_IDX_GLOBAL_CONFIG: usize = 0;
    const PUMP_IDX_FEE_RECIPIENT: usize = 1;
    const PUMP_IDX_ASSOCIATED_BONDING_CURVE: usize = 4;
    const PUMP_IDX_TOKEN_PROGRAM: usize = 8;
    const PUMP_IDX_CREATOR_VAULT: usize = 9;
    const PUMP_IDX_BONDING_CURVE_V2: usize = 16;
    const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
    const ASSOCIATED_TOKEN_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
    const SYSTEM_PROGRAM_ID: &str = "11111111111111111111111111111111";
    struct ProgramIds;
    impl ProgramIds {
        const TOKEN_PROGRAM: &'static str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
    }
    fn buy_tx(
        signer: &str,
        signature: &str,
        toolchain_fingerprint: ToolchainFingerprintInput,
    ) -> PoolTransaction {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT_EXECUTION_INDEX: AtomicU32 = AtomicU32::new(0);
        let digest = solana_sdk::hash::hash(signature.as_bytes()).to_bytes();
        let mut signature_bytes = [0u8; 64];
        signature_bytes[..32].copy_from_slice(&digest);
        signature_bytes[32..].copy_from_slice(&digest);
        PoolTransaction {
            semantic: EventSemanticEnvelope::default(),
            pool_amm_id: "pool-1".to_string(),
            slot: Some(1),
            event_ordinal: Some(0),
            tx_index: Some(NEXT_EXECUTION_INDEX.fetch_add(1, Ordering::Relaxed)),
            outer_instruction_index: None,
            inner_instruction_path: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 1_000,
            event_time: EventTimeMetadata::new(Some(1_000), Some(1_000), Some(1_000)),
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
            signature: solana_sdk::signature::Signature::from(signature_bytes).to_string(),
            metadata_availability: seer::types::TransactionMetadataAvailability {
                status_known: true,
                inner_instructions_known: true,
            },
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
            token_mint: None,
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
            toolchain_fingerprint,
            curve_data_known: false,
            curve_finality: CurveFinality::Speculative,
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
    fn trade_data(disc: [u8; 8], amount: u64, sol: u64) -> Vec<u8> {
        let mut d = disc.to_vec();
        d.extend_from_slice(&amount.to_le_bytes());
        d.extend_from_slice(&sol.to_le_bytes());
        d
    }
    fn system_transfer_data(lamports: u64) -> Vec<u8> {
        let mut data = vec![2, 0, 0, 0];
        data.extend_from_slice(&lamports.to_le_bytes());
        data
    }
    fn derived_wsol_ata(owner: &Pubkey) -> Pubkey {
        let associated_token_program = Pubkey::from_str(ASSOCIATED_TOKEN_PROGRAM_ID).unwrap();
        let token_program = Pubkey::from_str(ProgramIds::TOKEN_PROGRAM).unwrap();
        let wsol_mint = Pubkey::from_str(WSOL_MINT).unwrap();
        Pubkey::find_program_address(
            &[owner.as_ref(), token_program.as_ref(), wsol_mint.as_ref()],
            &associated_token_program,
        )
        .0
    }
    fn make_ftdi_buy_event(external_fee_count: usize, include_wsol_self_wrap: bool) -> GeyserEvent {
        let signer = Keypair::new().pubkey();
        let mint = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let fee_recipient = Pubkey::new_unique();
        let global_config = Pubkey::new_unique();
        let associated_bonding_curve = Pubkey::new_unique();
        let token_program = Pubkey::from_str(ProgramIds::TOKEN_PROGRAM).unwrap();
        let signer_wsol_ata = derived_wsol_ata(&signer);
        let system_program = Pubkey::from_str(SYSTEM_PROGRAM_ID).unwrap();

        let external_start = 12usize;
        let signer_wsol_index = external_start + external_fee_count;
        let system_index = signer_wsol_index + 1;
        let mut accounts = vec![Pubkey::new_unique(); system_index + 1];
        accounts[PUMP_IDX_GLOBAL_CONFIG] = global_config;
        accounts[PUMP_IDX_FEE_RECIPIENT] = fee_recipient;
        accounts[PUMP_IDX_MINT] = mint;
        accounts[PUMP_IDX_BONDING_CURVE] = curve;
        accounts[PUMP_IDX_ASSOCIATED_BONDING_CURVE] = associated_bonding_curve;
        accounts[PUMP_IDX_USER] = signer;
        accounts[PUMP_IDX_TOKEN_PROGRAM] = token_program;
        accounts[signer_wsol_index] = signer_wsol_ata;
        accounts[system_index] = system_program;
        for index in 0..external_fee_count {
            accounts[external_start + index] = Pubkey::new_unique();
        }

        let mut inner_ixs = Vec::new();
        if include_wsol_self_wrap {
            inner_ixs.push(seer::types::InnerIx {
                program_id_index: system_index as u8,
                accounts: vec![PUMP_IDX_USER as u8, signer_wsol_index as u8],
                data: system_transfer_data(1_000_000),
                stack_height: Some(2),
            });
        }
        for index in 0..external_fee_count {
            inner_ixs.push(seer::types::InnerIx {
                program_id_index: system_index as u8,
                accounts: vec![PUMP_IDX_USER as u8, (external_start + index) as u8],
                data: system_transfer_data(500_000 + index as u64),
                stack_height: Some(2),
            });
        }

        let mut pre_balances = vec![0; system_index + 1];
        let mut post_balances = vec![0; system_index + 1];
        pre_balances[PUMP_IDX_USER] = 1_500_000_000;
        post_balances[PUMP_IDX_USER] = 1_450_000_000;

        GeyserEvent::Transaction {
            metadata_availability: seer::types::TransactionMetadataAvailability {
                status_known: true,
                inner_instructions_known: true,
            },
            provider_id: None,
            provider_role: None,
            observation_provenance: None,
            slot: Some(42),
            tx_index: None,
            event_ts_ms: None,
            arrival_ts_ms: Some(seer::types::arrival_time_ms()),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: solana_sdk::signature::Signature::new_unique(),
            accounts,
            instructions: vec![seer::types::RawInstruction {
                program_id: Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap(),
                account_indices: (0u8..12u8).collect(),
                data: trade_data(DISC_BUY, 1_000_000, 50_000_000),
            }],
            logs: vec![],
            block_time: None,
            account_data: HashMap::new(),
            pre_balances,
            post_balances,
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: false,
            source: "grpc_global_stream".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            inner_instructions: if inner_ixs.is_empty() {
                vec![]
            } else {
                vec![seer::types::InnerInstructionGroup {
                    index: 0,
                    instructions: inner_ixs,
                }]
            },
            pre_token_balances: vec![],
            post_token_balances: vec![],
        }
    }

    pub(crate) fn full_batch() -> Vec<PoolTransaction> {
        (0..3)
            .map(|i| {
                let mut t = buy_tx(
                    &format!("s{i}"),
                    &format!("sig{i}"),
                    dbia_fingerprint(12, 3, true, true, 2, (i, 0)),
                );
                t.is_dev_buy = i == 0;
                t.signer_pre_balance_lamports = Some(100);
                t.signer_post_balance_lamports = Some(90 - i as u64 * 20);
                t
            })
            .collect()
    }
    #[test]
    fn review_partial_duplicate_permutation_is_invariant() {
        let full = full_batch();
        let mut partial = full[0].clone();
        partial.metadata_availability.inner_instructions_known = false;
        partial.toolchain_fingerprint = Default::default();
        partial.signer_post_balance_lamports = None;
        let a = compute_sybil_resistance_with_ftdi_at_cutoff(
            [&partial, &full[0], &full[1], &full[2]],
            Some("s0"),
            1000,
        );
        let b = compute_sybil_resistance_with_ftdi_at_cutoff(
            [&full[0], &partial, &full[1], &full[2]],
            Some("s0"),
            1000,
        );
        println!(
            "PARTIAL FIRST: FTDI={:?} DBIA={:?} SFD={:?}; FULL FIRST: FTDI={:?} DBIA={:?} SFD={:?}",
            a.ftdi.fee_topology_diversity_index,
            a.dbia.dev_buyer_infrastructure_affinity,
            a.sfd.spend_fraction_divergence,
            b.ftdi.fee_topology_diversity_index,
            b.dbia.dev_buyer_infrastructure_affinity,
            b.sfd.spend_fraction_divergence
        );
        assert_eq!((a.ftdi, a.dbia, a.sfd), (b.ftdi, b.dbia, b.sfd));
    }

    #[test]
    fn review_complete_redelivery_before_cutoff_can_supply_missing_feature() {
        let full = full_batch();
        let mut partial = full[0].clone();
        partial.signer_post_balance_lamports = None;
        let mut later = full[0].clone();
        later.event_time.ingress_wall_ts_ms = Some(1001);
        let before = compute_sybil_resistance_with_ftdi_at_cutoff(
            [&partial, &later, &full[1], &full[2]],
            Some("s0"),
            1000,
        );
        assert!(
            !before.sfd.has_full_quality(),
            "late data must not repair old cutoff"
        );
        let after = compute_sybil_resistance_with_ftdi_at_cutoff(
            [&partial, &later, &full[1], &full[2]],
            Some("s0"),
            1001,
        );
        println!("REDELIVERY before={:?}, after={:?}", before.sfd, after.sfd);
        assert!(
            after.sfd.has_full_quality(),
            "complete record is now before cutoff"
        );
    }
    fn wrapped_event(pda_owner: bool) -> GeyserEvent {
        let mut event = make_ftdi_buy_event(0, true);
        let wrapper = Pubkey::new_unique();
        if let GeyserEvent::Transaction {
            accounts,
            instructions,
            inner_instructions,
            pre_balances,
            post_balances,
            tx_index,
            event_time,
            ..
        } = &mut event
        {
            *tx_index = Some(1);
            *event_time = EventTimeMetadata::new(Some(1000), Some(1000), Some(1000));
            if pda_owner {
                accounts[PUMP_IDX_USER] =
                    Pubkey::find_program_address(&[b"review-user"], &wrapper).0;
                accounts[12] = derived_wsol_ata(&accounts[PUMP_IDX_USER]);
            }
            let pump_index = accounts.len() as u8;
            accounts.push(Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap());
            accounts.push(wrapper);
            pre_balances.resize(accounts.len(), 0);
            post_balances.resize(accounts.len(), 0);
            let buy = instructions[0].clone();
            instructions[0] = types::RawInstruction {
                program_id: wrapper,
                account_indices: (0..accounts.len() as u8).collect(),
                data: vec![0],
            };
            inner_instructions[0].instructions[0].stack_height = Some(3);
            inner_instructions[0].instructions.insert(
                0,
                types::InnerIx {
                    program_id_index: pump_index,
                    accounts: buy.account_indices,
                    data: buy.data,
                    stack_height: Some(2),
                },
            );
        }
        event
    }
    #[test]
    fn review_known_pda_user_preserves_actual_balance_pair() {
        let event = wrapped_event(true);
        let trade = BinaryParser::new(false)
            .parse_trades(&event)
            .unwrap()
            .remove(0);
        assert!(!trade.signer.is_on_curve());
        assert_eq!(trade.toolchain_fingerprint.fee_topology(), Some((0, 0)));
        println!(
            "KNOWN PDA BALANCES: pre={:?} post={:?}",
            trade.signer_pre_balance_lamports, trade.signer_post_balance_lamports
        );
        assert_eq!(
            (
                trade.signer_pre_balance_lamports,
                trade.signer_post_balance_lamports
            ),
            (Some(1_500_000_000), Some(1_450_000_000))
        );
    }
    #[test]
    fn review_oncurve_user_control_retains_balances() {
        let trade = BinaryParser::new(false)
            .parse_trades(&wrapped_event(false))
            .unwrap()
            .remove(0);
        assert!(trade.signer.is_on_curve());
        assert_eq!(
            (
                trade.signer_pre_balance_lamports,
                trade.signer_post_balance_lamports
            ),
            (Some(1_500_000_000), Some(1_450_000_000))
        );
    }
    #[test]
    fn review_same_transaction_cpi_before_next_outer_buy() {
        let mut event = wrapped_event(false);
        if let GeyserEvent::Transaction { instructions, .. } = &mut event {
            instructions.push(types::RawInstruction {
                program_id: Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap(),
                account_indices: (0u8..12).collect(),
                data: trade_data(DISC_BUY, 2_000_000, 100_000_000),
            });
        }
        let trades = BinaryParser::new(false).parse_trades(&event).unwrap();
        assert_eq!(
            trades.len(),
            2,
            "must preserve both actual BUY instructions"
        );
        let early = trades
            .iter()
            .find(|t| t.provenance.as_ref().unwrap().outer_instruction_index == Some(0))
            .unwrap();
        let late = trades
            .iter()
            .find(|t| t.provenance.as_ref().unwrap().outer_instruction_index == Some(1))
            .unwrap();
        let bridge = crate::components::seer::trade_event_to_pool_transaction;
        println!(
            "CPI early ordinal={:?} provenance={:?}; DIRECT late ordinal={:?} provenance={:?}",
            early.event_ordinal, early.provenance, late.event_ordinal, late.provenance
        );
        assert_eq!(
            super::canonical_buy_order(&bridge(early), &bridge(late)),
            Some(std::cmp::Ordering::Less)
        );
    }

    #[test]
    fn review_pda_balance_loss_reaches_sfd_through_real_bridge() {
        let mut txs = Vec::new();
        for i in 0..3u64 {
            let mut event = wrapped_event(i == 0);
            if let GeyserEvent::Transaction {
                pre_balances,
                post_balances,
                ..
            } = &mut event
            {
                pre_balances[PUMP_IDX_USER] = 100_000_000;
                post_balances[PUMP_IDX_USER] = 90_000_000 - i * 20_000_000;
            }
            let trade = BinaryParser::new(false)
                .parse_trades(&event)
                .unwrap()
                .remove(0);
            txs.push(crate::components::seer::trade_event_to_pool_transaction(
                &trade,
            ));
        }
        let result = compute_sfd(txs.iter());
        println!("RAW -> PARSER -> BRIDGE -> SFD: {result:?}");
        assert!(
            result.has_full_quality(),
            "all three recognized users have full raw balance pairs"
        );
        assert!((result.spend_fraction_divergence.unwrap() - 0.2).abs() < 1e-12);
    }
    #[test]
    fn review_complete_unique_inputs_and_identical_redelivery_control() {
        let txs = full_batch();
        let a = compute_sybil_resistance_with_ftdi_at_cutoff(txs.iter(), Some("s0"), 1000);
        let b = compute_sybil_resistance_with_ftdi_at_cutoff(
            [&txs[2], &txs[0], &txs[1], &txs[0]],
            Some("s0"),
            1000,
        );
        assert_eq!(a, b);
        assert!(a.ftdi.has_full_quality() && a.dbia.has_full_quality() && a.sfd.has_full_quality());
        assert!((a.ftdi.fee_topology_diversity_index.unwrap() - 2.0 / 3.0).abs() < 1e-12);
        assert!((a.sfd.spend_fraction_divergence.unwrap() - 0.2).abs() < 1e-12);
    }
    #[test]
    fn r1_views_supply_independent_metrics_without_stitching_balance_pairs() {
        let full = full_batch();
        let mut topology = full[0].clone();
        topology.signer_post_balance_lamports = None;
        let mut balances = full[0].clone();
        balances.metadata_availability.inner_instructions_known = false;
        balances.toolchain_fingerprint = Default::default();
        let expected = compute_sybil_resistance_with_ftdi_at_cutoff(full.iter(), Some("s0"), 1000);
        for views in [
            [&topology, &balances, &full[1], &full[2]],
            [&balances, &topology, &full[2], &full[1]],
        ] {
            let got = compute_sybil_resistance_with_ftdi_at_cutoff(views, Some("s0"), 1000);
            assert_eq!(
                (got.ftdi, got.dbia, got.sfd),
                (
                    expected.ftdi.clone(),
                    expected.dbia.clone(),
                    expected.sfd.clone()
                )
            );
        }
        balances.signer_pre_balance_lamports = None;
        let incomplete = compute_sfd([&topology, &balances, &full[1], &full[2]]);
        assert_eq!(incomplete.represented_signer_count, 2);
        assert!(!incomplete.has_full_quality());
    }

    #[test]
    fn r1_conflicting_balances_degrade_only_sfd_and_never_choose_by_delivery_order() {
        let full = full_batch();
        let mut conflicting = full[0].clone();
        conflicting.signer_post_balance_lamports = Some(10);
        conflicting.event_time.ingress_wall_ts_ms = Some(1001);
        let before = compute_sybil_resistance_with_ftdi_at_cutoff(
            [&full[0], &conflicting, &full[1], &full[2]],
            Some("s0"),
            1000,
        );
        assert!(before.sfd.has_full_quality());
        let mut previous = None;
        for views in [
            [&full[0], &conflicting, &full[1], &full[2]],
            [&conflicting, &full[0], &full[2], &full[1]],
        ] {
            let got = compute_sybil_resistance_with_ftdi_at_cutoff(views, Some("s0"), 1001);
            assert!(got.ftdi.has_full_quality() && got.dbia.has_full_quality());
            assert!(!got.sfd.has_full_quality());
            assert_eq!(got.sfd.represented_signer_count, 2);
            assert!(got
                .sfd
                .degraded_reasons
                .contains(&"SFD_INPUT_CONFLICTING_VIEWS".to_string()));
            if let Some(old) = previous {
                assert_eq!(got, old);
            }
            previous = Some(got);
        }
    }

    #[test]
    fn r1_conflicting_fingerprint_does_not_degrade_valid_balances() {
        let full = full_batch();
        let mut changed = full[0].clone();
        changed.toolchain_fingerprint.account_keys_len = Some(99);
        let got = compute_sybil_resistance_with_ftdi_at_cutoff(
            [&full[0], &changed, &full[1], &full[2]],
            Some("s0"),
            1000,
        );
        assert!(got.ftdi.has_full_quality() && got.sfd.has_full_quality());
        assert!(!got.dbia.has_full_quality());
        changed.toolchain_fingerprint.external_fee_transfer_count = Some(9);
        let got = compute_sybil_resistance_with_ftdi_at_cutoff(
            [&changed, &full[0], &full[1], &full[2]],
            Some("s0"),
            1000,
        );
        assert!(!got.ftdi.has_full_quality() && !got.dbia.has_full_quality());
        assert!(got.sfd.has_full_quality());
    }

    #[test]
    fn r1_unknown_status_can_be_completed_but_conflicting_known_status_cannot() {
        let full = full_batch();
        let mut partial = full[0].clone();
        partial.metadata_availability.status_known = false;
        partial.success = false;
        let got = compute_sybil_resistance_with_ftdi_at_cutoff(
            [&partial, &full[0], &full[1], &full[2]],
            Some("s0"),
            1000,
        );
        assert!(
            got.ftdi.has_full_quality()
                && got.dbia.has_full_quality()
                && got.sfd.has_full_quality()
        );
        assert_eq!(got.sfd.buy_sample_count, 3);
        partial.metadata_availability.status_known = true;
        let got = compute_sybil_resistance_with_ftdi_at_cutoff(
            [&partial, &full[0], &full[1], &full[2]],
            Some("s0"),
            1000,
        );
        assert!(
            !got.ftdi.has_full_quality()
                && !got.dbia.has_full_quality()
                && !got.sfd.has_full_quality()
        );
        assert!(got
            .sfd
            .degraded_reasons
            .contains(&"SFD_INPUT_CONFLICTING_VIEWS".to_string()));
    }

    #[test]
    fn r2_order_needs_provenance_and_never_compares_depth_or_local_ordinal() {
        use std::cmp::Ordering::Less;
        let mut a = full_batch().remove(0);
        let mut b = a.clone();
        a.event_ordinal = Some(99);
        b.event_ordinal = Some(1);
        a.outer_instruction_index = Some(0);
        b.outer_instruction_index = Some(1);
        assert_eq!(super::canonical_buy_order(&a, &b), Some(Less));
        b.outer_instruction_index = Some(0);
        for (left, right) in [
            (vec![0], vec![1]),
            (vec![0, 9], vec![1]),
            (vec![0, 0], vec![0, 1]),
        ] {
            a.inner_instruction_path = Some(left);
            b.inner_instruction_path = Some(right);
            assert_eq!(super::canonical_buy_order(&a, &b), Some(Less));
        }
        for (left, right) in [(vec![], vec![0]), (vec![0], vec![0, 0]), (vec![0], vec![0])] {
            a.inner_instruction_path = Some(left);
            b.inner_instruction_path = Some(right);
            assert_eq!(super::canonical_buy_order(&a, &b), None);
        }
        a.inner_instruction_path = None;
        b.cpi_stack_height = Some(99);
        assert_eq!(super::canonical_buy_order(&a, &b), None);
    }

    #[test]
    fn r3_known_pda_balance_lookup_does_not_borrow_another_accounts_balance() {
        let mut event = wrapped_event(true);
        if let GeyserEvent::Transaction {
            pre_balances,
            post_balances,
            ..
        } = &mut event
        {
            pre_balances[PUMP_IDX_USER] = 150;
            post_balances.truncate(PUMP_IDX_USER);
        }
        let trade = BinaryParser::new(false)
            .parse_trades(&event)
            .unwrap()
            .remove(0);
        assert_eq!(trade.signer_pre_balance_lamports, Some(150));
        assert_eq!(trade.signer_post_balance_lamports, None);
        let mut event = wrapped_event(false);
        if let GeyserEvent::Transaction {
            accounts,
            pre_balances,
            post_balances,
            ..
        } = &mut event
        {
            accounts.push(accounts[PUMP_IDX_USER]);
            pre_balances.push(1);
            post_balances.push(0);
        }
        let trade = BinaryParser::new(false)
            .parse_trades(&event)
            .unwrap()
            .remove(0);
        assert_eq!(
            (
                trade.signer_pre_balance_lamports,
                trade.signer_post_balance_lamports
            ),
            (None, None)
        );
    }
    #[test]
    fn r1_known_receipt_can_replace_an_undated_view_of_the_same_event() {
        let full = full_batch();
        let mut undated = full[0].clone();
        undated.event_time.ingress_wall_ts_ms = None;
        undated.signer_post_balance_lamports = None;
        let mut dated = full[0].clone();
        dated.event_time.ingress_wall_ts_ms = Some(1001);
        let before = compute_sybil_resistance_with_ftdi_at_cutoff(
            [&undated, &dated, &full[1], &full[2]],
            Some("s0"),
            1000,
        );
        assert!(!before.sfd.has_full_quality());
        let after = compute_sybil_resistance_with_ftdi_at_cutoff(
            [&undated, &dated, &full[1], &full[2]],
            Some("s0"),
            1001,
        );
        assert!(
            after.ftdi.has_full_quality()
                && after.dbia.has_full_quality()
                && after.sfd.has_full_quality()
        );
        assert_eq!(after.sfd.buy_sample_count, 3);
    }

    #[test]
    fn r2_chronological_input_keeps_one_transaction_together_with_partial_index() {
        let mut a = full_batch().remove(0);
        let mut b = a.clone();
        a.tx_index = Some(7);
        b.tx_index = None;
        a.event_ordinal = Some(99);
        b.event_ordinal = Some(1);
        a.outer_instruction_index = Some(0);
        b.outer_instruction_index = Some(1);
        for refs in [[&a, &b], [&b, &a]] {
            let samples = super::successful_buy_samples(&refs);
            let ordered = super::ordered_buy_samples(&samples).unwrap();
            assert_eq!(ordered[0].tx.outer_instruction_index, Some(0));
            assert_eq!(ordered[1].tx.outer_instruction_index, Some(1));
        }
        b.tx_index = Some(8);
        assert!(super::ordered_buy_samples(&super::successful_buy_samples(&[&a, &b])).is_none());
    }
}
