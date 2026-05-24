use ghost_core::features::coordination::{
    build_observed_buy_txs_from_fixture, sequence_buys, summarize_observed_buy_txs,
    unique_first_buys_by_signer, CoordinationSampleFixture, ObservedBuyTx, SequenceBuildError,
    T0Source, TxTimeSource,
};
use smallvec::{smallvec, SmallVec};
use solana_sdk::{pubkey::Pubkey, signature::Signature};

fn pk(seed: u8) -> Pubkey {
    Pubkey::new_from_array([seed; 32])
}

fn observed_buy_tx(signer_seed: u8, slot: u64, slot_index: Option<u64>) -> ObservedBuyTx {
    ObservedBuyTx {
        signature: Signature::new_unique(),
        pool_id: pk(1),
        mint: pk(2),
        signer: pk(signer_seed),
        slot,
        slot_index,
        tx_elapsed_ms_from_pool_create: Some(100),
        t0_source: Some(T0Source::ReplayFixture),
        tx_time_source: Some(TxTimeSource::ReplayFixture),
        is_success: true,
        is_buy: true,
        is_sell: false,
        is_dev: false,
        is_create_or_init_tx: false,
        is_unknown_direction: false,
        account_keys_resolved: smallvec![pk(signer_seed), pk(3)],
        outer_ix_count: Some(2),
        inner_ix_group_count: Some(1),
        fee_lamports: Some(5_000),
        pre_balance_signer: Some(1_000_000),
        post_balance_signer: Some(900_000),
        decoded_buy_sol_lamports: Some(100_000),
        curve_sol_delta_lamports: Some(100_000),
        economic_spent_lamports: None,
        tokens_received: Some(42),
        price_before: Some(0.001),
        price_after: Some(0.0011),
        compute_units_consumed: Some(50_000),
        cost_units: Some(50_000),
        fee_topology_fp: None,
        execution_template_fp: None,
        capital_template_fp: None,
    }
}

#[test]
fn fixture_builder_preserves_raw_observed_samples_without_filtering() {
    let mut failed = observed_buy_tx(10, 10, Some(0));
    failed.is_success = false;
    let valid = observed_buy_tx(11, 11, Some(0));
    let fixture = CoordinationSampleFixture::new(smallvec![failed.clone(), valid.clone()]);

    let built = build_observed_buy_txs_from_fixture(&fixture);

    assert_eq!(built.len(), 2);
    assert_eq!(built[0], failed);
    assert_eq!(built[1], valid);
}

#[test]
fn buyer_sample_selection_excludes_non_buy_candidates() {
    let valid_a = observed_buy_tx(20, 20, Some(0));
    let mut failed = observed_buy_tx(21, 21, Some(0));
    failed.is_success = false;
    let mut sell = observed_buy_tx(22, 22, Some(0));
    sell.is_sell = true;
    let mut unknown = observed_buy_tx(23, 23, Some(0));
    unknown.is_buy = false;
    unknown.is_unknown_direction = true;
    let mut dev_create = observed_buy_tx(24, 24, Some(0));
    dev_create.is_dev = true;
    dev_create.is_create_or_init_tx = true;
    let mut valid_missing_slot_index = observed_buy_tx(25, 25, None);
    valid_missing_slot_index.compute_units_consumed = None;
    valid_missing_slot_index.pre_balance_signer = None;

    let txs = vec![
        failed,
        sell,
        unknown,
        dev_create,
        valid_missing_slot_index.clone(),
        valid_a.clone(),
    ];

    let buyers = unique_first_buys_by_signer(&txs);
    let buyer_signers: Vec<Pubkey> = buyers.iter().map(|tx| tx.signer).collect();

    assert_eq!(
        buyer_signers,
        vec![valid_a.signer, valid_missing_slot_index.signer]
    );

    let summary = summarize_observed_buy_txs(&txs);
    assert_eq!(summary.total_txs_seen, 6);
    assert_eq!(summary.successful_buy_txs, 2);
    assert_eq!(summary.unique_buyers, 2);
    assert_eq!(summary.excluded_failed, 1);
    assert_eq!(summary.excluded_sell, 1);
    assert_eq!(summary.excluded_unknown_direction, 1);
    assert_eq!(summary.excluded_dev_create_or_init, 1);
    assert_eq!(summary.missing_slot_index_count, 1);
    assert_eq!(summary.missing_compute_units_count, 1);
    assert_eq!(summary.missing_balance_count, 1);
}

#[test]
fn first_buy_per_signer_is_deterministic_and_does_not_require_slot_index() {
    let later_same_signer = observed_buy_tx(30, 40, Some(2));
    let first_same_signer = observed_buy_tx(30, 39, None);
    let other_signer = observed_buy_tx(31, 38, Some(5));

    let txs_a = vec![
        later_same_signer.clone(),
        first_same_signer.clone(),
        other_signer.clone(),
    ];
    let txs_b = vec![
        other_signer.clone(),
        later_same_signer,
        first_same_signer.clone(),
    ];

    let selected_a: Vec<Signature> = unique_first_buys_by_signer(&txs_a)
        .iter()
        .map(|tx| tx.signature)
        .collect();
    let selected_b: Vec<Signature> = unique_first_buys_by_signer(&txs_b)
        .iter()
        .map(|tx| tx.signature)
        .collect();

    assert_eq!(selected_a, selected_b);
    assert_eq!(
        selected_a,
        vec![other_signer.signature, first_same_signer.signature]
    );
}

#[test]
fn sequence_buys_sorts_by_slot_and_slot_index_only_after_selection() {
    let tx_a = observed_buy_tx(40, 11, Some(2));
    let tx_b = observed_buy_tx(41, 10, Some(9));
    let tx_c = observed_buy_tx(42, 11, Some(1));
    let mut failed_missing_slot_index = observed_buy_tx(43, 9, None);
    failed_missing_slot_index.is_success = false;

    let txs = vec![
        tx_a.clone(),
        failed_missing_slot_index,
        tx_b.clone(),
        tx_c.clone(),
    ];

    let sequence = sequence_buys(&txs).expect("sequence should ignore excluded failed tx");
    let ordered_positions: Vec<(u64, Option<u64>)> =
        sequence.iter().map(|tx| (tx.slot, tx.slot_index)).collect();

    assert_eq!(
        ordered_positions,
        vec![(10, Some(9)), (11, Some(1)), (11, Some(2))]
    );
    assert_eq!(
        sequence.iter().map(|tx| tx.signature).collect::<Vec<_>>(),
        vec![tx_b.signature, tx_c.signature, tx_a.signature]
    );
}

#[test]
fn missing_slot_index_blocks_sequence_metrics_but_not_cross_sectional_selection() {
    let missing_slot_index = observed_buy_tx(50, 50, None);
    let txs: SmallVec<[ObservedBuyTx; 32]> = smallvec![missing_slot_index.clone()];

    let first_buys = unique_first_buys_by_signer(&txs);
    assert_eq!(first_buys.len(), 1);
    assert_eq!(first_buys[0].signature, missing_slot_index.signature);

    let err = sequence_buys(&txs).expect_err("sequence metrics require slot_index");
    assert_eq!(
        err,
        SequenceBuildError::MissingSlotIndex {
            signature: missing_slot_index.signature
        }
    );
}

#[test]
fn duplicate_slot_index_is_not_resolved_with_non_causal_tiebreaker() {
    let first = observed_buy_tx(60, 60, Some(1));
    let duplicate = observed_buy_tx(61, 60, Some(1));
    let txs = vec![duplicate.clone(), first.clone()];

    let err = sequence_buys(&txs).expect_err("duplicate causal positions are ambiguous");

    match err {
        SequenceBuildError::DuplicateSlotIndex {
            slot,
            slot_index,
            first_signature,
            duplicate_signature,
        } => {
            assert_eq!(slot, 60);
            assert_eq!(slot_index, 1);
            assert!(
                (first_signature == first.signature && duplicate_signature == duplicate.signature)
                    || (first_signature == duplicate.signature
                        && duplicate_signature == first.signature)
            );
        }
        other => panic!("unexpected sequence error: {other:?}"),
    }
}
