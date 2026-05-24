use crate::features::coordination::types::{
    CoordinationSampleFixture, CoordinationSampleSummary, ObservedBuyTx,
};
use smallvec::SmallVec;
use solana_sdk::{pubkey::Pubkey, signature::Signature};
use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SequenceBuildError {
    MissingSlotIndex {
        signature: Signature,
    },
    DuplicateSlotIndex {
        slot: u64,
        slot_index: u64,
        first_signature: Signature,
        duplicate_signature: Signature,
    },
}

#[must_use]
pub fn build_observed_buy_txs_from_fixture(
    fixture: &CoordinationSampleFixture,
) -> SmallVec<[ObservedBuyTx; 32]> {
    fixture.txs.clone()
}

#[must_use]
pub fn unique_first_buys_by_signer(txs: &[ObservedBuyTx]) -> SmallVec<[&ObservedBuyTx; 16]> {
    let mut sorted: SmallVec<[&ObservedBuyTx; 32]> = txs
        .iter()
        .filter(|tx| tx.is_buyer_sample_candidate())
        .collect();
    sorted.sort_by(|left, right| deterministic_sample_order(left, right));

    let mut seen_signers: SmallVec<[Pubkey; 16]> = SmallVec::new();
    let mut selected: SmallVec<[&ObservedBuyTx; 16]> = SmallVec::new();

    for tx in sorted {
        if seen_signers.iter().any(|signer| signer == &tx.signer) {
            continue;
        }

        seen_signers.push(tx.signer);
        selected.push(tx);
    }

    selected
}

pub fn sequence_buys(
    txs: &[ObservedBuyTx],
) -> Result<SmallVec<[&ObservedBuyTx; 32]>, SequenceBuildError> {
    let mut sorted: SmallVec<[&ObservedBuyTx; 32]> = SmallVec::new();

    for tx in txs.iter().filter(|tx| tx.is_buyer_sample_candidate()) {
        if tx.slot_index.is_none() {
            return Err(SequenceBuildError::MissingSlotIndex {
                signature: tx.signature,
            });
        }

        sorted.push(tx);
    }

    sorted.sort_by(|left, right| causal_sequence_order(left, right));

    for pair in sorted.windows(2) {
        let previous = pair[0];
        let current = pair[1];

        if previous.slot == current.slot && previous.slot_index == current.slot_index {
            let Some(slot_index) = current.slot_index else {
                return Err(SequenceBuildError::MissingSlotIndex {
                    signature: current.signature,
                });
            };

            return Err(SequenceBuildError::DuplicateSlotIndex {
                slot: current.slot,
                slot_index,
                first_signature: previous.signature,
                duplicate_signature: current.signature,
            });
        }
    }

    Ok(sorted)
}

#[must_use]
pub fn summarize_observed_buy_txs(txs: &[ObservedBuyTx]) -> CoordinationSampleSummary {
    let mut summary = CoordinationSampleSummary {
        total_txs_seen: saturating_u16(txs.len()),
        ..CoordinationSampleSummary::default()
    };

    for tx in txs {
        if !tx.is_success {
            summary.excluded_failed = summary.excluded_failed.saturating_add(1);
            continue;
        }

        if tx.is_sell {
            summary.excluded_sell = summary.excluded_sell.saturating_add(1);
            continue;
        }

        if tx.is_unknown_direction || !tx.is_buy {
            summary.excluded_unknown_direction =
                summary.excluded_unknown_direction.saturating_add(1);
            continue;
        }

        if tx.is_dev && tx.is_create_or_init_tx {
            summary.excluded_dev_create_or_init =
                summary.excluded_dev_create_or_init.saturating_add(1);
            continue;
        }

        summary.successful_buy_txs = summary.successful_buy_txs.saturating_add(1);

        if tx.slot_index.is_none() {
            summary.missing_slot_index_count = summary.missing_slot_index_count.saturating_add(1);
        }

        if tx.compute_units_consumed.is_none() {
            summary.missing_compute_units_count =
                summary.missing_compute_units_count.saturating_add(1);
        }

        if tx.pre_balance_signer.is_none() || tx.post_balance_signer.is_none() {
            summary.missing_balance_count = summary.missing_balance_count.saturating_add(1);
        }
    }

    summary.unique_buyers = saturating_u16(unique_first_buys_by_signer(txs).len());
    summary
}

fn deterministic_sample_order(left: &ObservedBuyTx, right: &ObservedBuyTx) -> Ordering {
    left.slot
        .cmp(&right.slot)
        .then_with(|| optional_slot_index_with_missing_last(left.slot_index, right.slot_index))
        .then_with(|| cmp_signature(&left.signature, &right.signature))
        .then_with(|| cmp_pubkey(&left.signer, &right.signer))
}

fn causal_sequence_order(left: &ObservedBuyTx, right: &ObservedBuyTx) -> Ordering {
    left.slot
        .cmp(&right.slot)
        .then_with(|| left.slot_index.cmp(&right.slot_index))
        .then_with(|| cmp_signature(&left.signature, &right.signature))
}

fn optional_slot_index_with_missing_last(left: Option<u64>, right: Option<u64>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn cmp_signature(left: &Signature, right: &Signature) -> Ordering {
    left.as_ref().cmp(right.as_ref())
}

fn cmp_pubkey(left: &Pubkey, right: &Pubkey) -> Ordering {
    left.as_ref().cmp(right.as_ref())
}

fn saturating_u16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}
