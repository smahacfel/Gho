use seer::{binary_parser::BinaryParser, types::GeyserEvent};

#[test]
fn r21_actual_create_buy_v2_has_instruction_route_without_duplicate_trade() {
    let event: GeyserEvent =
        serde_json::from_str(include_str!("fixtures/r21/create_buy_v2.json")).unwrap();
    let bundle = BinaryParser::new(false)
        .parse_transaction_bundle(&event)
        .unwrap();
    assert!(bundle.initialize_pool.is_some());
    assert_eq!(bundle.trades.len(), 1);
    let trade = &bundle.trades[0];
    assert!(trade.is_buy);
    assert_eq!(trade.buy_variant.as_deref(), Some("buy_v2"));
    assert_eq!(
        trade.pool_amm_id.to_string(),
        "AwP7iVUXdUZ2HEbPEj6mFL4VsxUvNU4fx8TWm6uHFDyz"
    );
    assert_eq!(
        trade.signer.to_string(),
        "3xtyS7oYkE8wQ3uZN4uMf3jdhBj5SKnM4HM6kZr7M6uc"
    );
    assert_eq!(
        bundle.trade_observations[0]
            .as_ref()
            .unwrap()
            .claims
            .route_variant
            .unwrap()
            .as_str(),
        "buy_v2"
    );
}

#[test]
fn r21_actual_sell_v2_resolves_identity_without_prior_birth() {
    let event: GeyserEvent =
        serde_json::from_str(include_str!("fixtures/r21/sell_v2.json")).unwrap();
    let bundle = BinaryParser::new(false)
        .parse_transaction_bundle(&event)
        .unwrap();
    assert_eq!(bundle.trades.len(), 1);
    let trade = &bundle.trades[0];
    assert!(!trade.is_buy);
    assert_eq!(trade.buy_variant.as_deref(), Some("sell_v2"));
    assert_eq!(
        trade.pool_amm_id.to_string(),
        "AwP7iVUXdUZ2HEbPEj6mFL4VsxUvNU4fx8TWm6uHFDyz"
    );
    assert_eq!(
        bundle.trade_observations[0]
            .as_ref()
            .unwrap()
            .claims
            .route_variant
            .unwrap()
            .as_str(),
        "sell_v2"
    );
}

fn create_buy_fixture() -> GeyserEvent {
    serde_json::from_str(include_str!("fixtures/r21/create_buy_v2.json")).unwrap()
}

#[test]
fn r21_legacy_buy_and_sell_keep_their_own_account_layouts() {
    use seer::binary_parser::{DISC_BUY, DISC_SELL};
    for (disc, variant, buy) in [
        (DISC_BUY, "legacy_buy", true),
        (DISC_SELL, "legacy_sell", false),
    ] {
        let mut event = create_buy_fixture();
        if let GeyserEvent::Transaction {
            instructions,
            inner_instructions,
            logs,
            ..
        } = &mut event
        {
            let mut ix = instructions[4].clone();
            let original = ix.account_indices.clone();
            let mut layout = vec![0, 6, 1, 10, 11, 14, 13, 24, 3, 16, 25, 26, 19, 20, 22, 23];
            if !buy {
                layout.swap(8, 9);
            }
            ix.account_indices = layout.into_iter().map(|p| original[p]).collect();
            ix.data[..8].copy_from_slice(&disc);
            // Zero jest poprawnym minimum output SELL, nie brakiem instrukcji.
            if !buy {
                ix.data[16..24].copy_from_slice(&0u64.to_le_bytes());
            }
            *instructions = vec![ix];
            inner_instructions.clear();
            logs.clear();
        }
        let bundle = BinaryParser::new(false)
            .parse_transaction_bundle(&event)
            .unwrap();
        assert_eq!(bundle.trades.len(), 1);
        let trade = &bundle.trades[0];
        assert_eq!(trade.buy_variant.as_deref(), Some(variant));
        assert_eq!(trade.is_buy, buy);
        assert_eq!(
            trade.token_program.unwrap().to_string(),
            "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
        );
    }
}

#[test]
fn r21_v2_truncated_payload_or_non_sol_quote_never_gains_route_from_cpi() {
    use solana_sdk::pubkey::Pubkey;
    for invalid_quote in [false, true] {
        let mut event = create_buy_fixture();
        if let GeyserEvent::Transaction {
            instructions,
            accounts,
            ..
        } = &mut event
        {
            if invalid_quote {
                let index = instructions[4].account_indices[2] as usize;
                accounts[index] = Pubkey::new_unique();
            } else {
                instructions[4].data.truncate(23);
            }
        }
        let bundle = BinaryParser::new(false)
            .parse_transaction_bundle(&event)
            .unwrap();
        assert!(bundle.trades.iter().all(|t| t.buy_variant.is_none()));
        assert!(bundle
            .trade_observations
            .iter()
            .flatten()
            .all(|o| o.claims.route_variant.is_none()));
    }
}

#[test]
fn r21_two_v2_calls_remain_two_trades() {
    let mut event = create_buy_fixture();
    if let GeyserEvent::Transaction {
        instructions,
        inner_instructions,
        ..
    } = &mut event
    {
        let next_index = instructions.len() as u32;
        instructions.push(instructions[4].clone());
        let mut group = inner_instructions
            .iter()
            .find(|g| g.index == 4)
            .unwrap()
            .clone();
        group.index = next_index;
        inner_instructions.push(group);
    }
    let bundle = BinaryParser::new(false)
        .parse_transaction_bundle(&event)
        .unwrap();
    assert_eq!(bundle.trades.len(), 2);
    assert!(bundle
        .trades
        .iter()
        .all(|t| t.buy_variant.as_deref() == Some("buy_v2")));
    assert_ne!(
        bundle.trades[0].event_ordinal,
        bundle.trades[1].event_ordinal
    );
    assert_eq!(
        bundle.trade_observations[0]
            .as_ref()
            .unwrap()
            .raw_transaction_mutation_count,
        Some(3)
    );
}

#[test]
fn r21_mixed_variants_bind_metadata_to_the_owning_instruction() {
    let mut event = create_buy_fixture();
    if let GeyserEvent::Transaction {
        instructions,
        inner_instructions,
        ..
    } = &mut event
    {
        let next_index = instructions.len() as u32;
        let mut legacy = instructions[4].clone();
        let keys = legacy.account_indices.clone();
        legacy.account_indices = [0, 6, 1, 10, 11, 14, 13, 24, 3, 16, 25, 26, 19, 20, 22, 23]
            .into_iter()
            .map(|i| keys[i])
            .collect();
        legacy.data[..8].copy_from_slice(&seer::binary_parser::DISC_BUY);
        instructions.push(legacy);
        let mut group = inner_instructions
            .iter()
            .find(|g| g.index == 4)
            .unwrap()
            .clone();
        group.index = next_index;
        inner_instructions.push(group);
    }
    let bundle = BinaryParser::new(false)
        .parse_transaction_bundle(&event)
        .unwrap();
    assert_eq!(bundle.trades.len(), 2);
    let mut variants: Vec<_> = bundle
        .trades
        .iter()
        .map(|t| {
            (
                t.provenance
                    .as_ref()
                    .unwrap()
                    .outer_instruction_index
                    .unwrap(),
                t.buy_variant.as_deref(),
            )
        })
        .collect();
    variants.sort();
    assert_eq!(variants[0].1, Some("buy_v2"));
    assert_eq!(variants[1].1, Some("legacy_buy"));
}

#[test]
fn r21_sibling_cpi_calls_do_not_borrow_each_others_route() {
    use seer::types::{InnerInstructionGroup, InnerIx};
    let mut event = create_buy_fixture();
    if let GeyserEvent::Transaction {
        instructions,
        inner_instructions,
        accounts,
        logs,
        ..
    } = &mut event
    {
        let buy = instructions[4].clone();
        let program_id_index = accounts.iter().position(|p| *p == buy.program_id).unwrap() as u8;
        let old_group = inner_instructions
            .iter()
            .find(|g| g.index == 4)
            .unwrap()
            .clone();
        let mut calls = Vec::new();
        for legacy in [false, true] {
            let mut ix = buy.clone();
            if legacy {
                ix.account_indices = [0, 6, 1, 10, 11, 14, 13, 24, 3, 16, 25, 26, 19, 20, 22, 23]
                    .into_iter()
                    .map(|i| buy.account_indices[i])
                    .collect();
                ix.data[..8].copy_from_slice(&seer::binary_parser::DISC_BUY);
            }
            calls.push(InnerIx {
                program_id_index,
                accounts: ix.account_indices,
                data: ix.data,
                stack_height: Some(2),
            });
            for mut child in old_group.instructions.clone() {
                child.stack_height = child.stack_height.map(|h| h + 1);
                calls.push(child);
            }
        }
        let mut router = buy;
        router.program_id = solana_sdk::pubkey::Pubkey::new_unique();
        router.data.clear();
        *instructions = vec![router];
        *inner_instructions = vec![InnerInstructionGroup {
            index: 0,
            instructions: calls,
        }];
        logs.clear();
    }
    let bundle = BinaryParser::new(false)
        .parse_transaction_bundle(&event)
        .unwrap();
    assert_eq!(bundle.trades.len(), 2);
    let mut routes: Vec<_> = bundle
        .trades
        .iter()
        .map(|t| {
            (
                t.provenance
                    .as_ref()
                    .unwrap()
                    .inner_instruction_path
                    .clone()
                    .unwrap(),
                t.buy_variant.as_deref(),
            )
        })
        .collect();
    routes.sort();
    assert_eq!(routes[0].1, Some("buy_v2"));
    assert_eq!(routes[1].1, Some("legacy_buy"));
}

#[test]
fn r21_literal_instruction_limit_is_separate_from_flow_and_serde_compatible() {
    use ghost_core::PumpInstructionLimitV1;
    for (fixture, disc, buy) in [
        (
            include_str!("fixtures/r21/create_buy_v2.json"),
            seer::binary_parser::DISC_BUY_V2,
            true,
        ),
        (
            include_str!("fixtures/r21/sell_v2.json"),
            seer::binary_parser::DISC_SELL_V2,
            false,
        ),
    ] {
        let mut event: GeyserEvent = serde_json::from_str(fixture).unwrap();
        let original = BinaryParser::new(false)
            .parse_transaction_bundle(&event)
            .unwrap()
            .trades[0]
            .clone();
        let limit = if buy { 123_456_789 } else { 0 };
        if let GeyserEvent::Transaction {
            instructions,
            inner_instructions,
            ..
        } = &mut event
        {
            let data = if let Some(ix) = instructions
                .iter_mut()
                .find(|ix| ix.data.starts_with(&disc))
            {
                &mut ix.data
            } else {
                &mut inner_instructions
                    .iter_mut()
                    .flat_map(|g| &mut g.instructions)
                    .find(|ix| ix.data.starts_with(&disc))
                    .unwrap()
                    .data
            };
            data[16..24].copy_from_slice(&u64::to_le_bytes(limit));
        }
        let bundle = BinaryParser::new(false)
            .parse_transaction_bundle(&event)
            .unwrap();
        let trade = &bundle.trades[0];
        let expected = if buy {
            PumpInstructionLimitV1::MaxWalletDebitLamports(limit)
        } else {
            PumpInstructionLimitV1::MinWalletCreditLamports(limit)
        };
        assert_eq!(trade.instruction_limit, Some(expected.clone()));
        assert_eq!(
            bundle.trade_observations[0]
                .as_ref()
                .unwrap()
                .claims
                .instruction_limit,
            Some(expected)
        );
        assert_eq!(
            (trade.max_sol_cost, trade.min_sol_output),
            (original.max_sol_cost, original.min_sol_output)
        );
        let mut json = serde_json::to_value(trade).unwrap();
        json.as_object_mut().unwrap().remove("instruction_limit");
        let old: seer::types::TradeEvent = serde_json::from_value(json).unwrap();
        assert_eq!(old.instruction_limit, None);
    }
}
