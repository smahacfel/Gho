// Publiczna transakcja z r4, slot 448405529; zapis kont i instrukcji RPC.
fn identity_route_fixture() -> GeyserEvent {
    identity_transaction_fixture(include_str!(
        "../tests/fixtures/identity_route_448405529.fixture"
    ))
}

fn identity_transaction_fixture(fixture: &str) -> GeyserEvent {
    let value: serde_json::Value = serde_json::from_str(fixture).unwrap();
    let meta = &value["meta"];
    let message = &value["transaction"]["message"];
    let accounts: Vec<Pubkey> = message["accountKeys"]
        .as_array()
        .unwrap()
        .iter()
        .chain(meta["loadedAddresses"]["writable"].as_array().unwrap())
        .chain(meta["loadedAddresses"]["readonly"].as_array().unwrap())
        .map(|v| v.as_str().unwrap().parse().unwrap())
        .collect();
    let indices = |v: &serde_json::Value| -> Vec<u8> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_u64().unwrap() as u8)
            .collect()
    };
    let instructions = message["instructions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|ix| crate::types::RawInstruction {
            program_id: accounts[ix["programIdIndex"].as_u64().unwrap() as usize],
            account_indices: indices(&ix["accounts"]),
            data: bs58::decode(ix["data"].as_str().unwrap())
                .into_vec()
                .unwrap(),
        })
        .collect();
    let inner = meta["innerInstructions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|group| crate::types::InnerInstructionGroup {
            index: group["index"].as_u64().unwrap() as u32,
            instructions: group["instructions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|ix| crate::types::InnerIx {
                    program_id_index: ix["programIdIndex"].as_u64().unwrap() as u8,
                    accounts: indices(&ix["accounts"]),
                    data: bs58::decode(ix["data"].as_str().unwrap())
                        .into_vec()
                        .unwrap(),
                    stack_height: ix["stackHeight"].as_u64().map(|n| n as u32),
                })
                .collect(),
        })
        .collect();
    let mut event = make_decoded_tx_event_with_inner(accounts, instructions, inner);
    if let GeyserEvent::Transaction {
        signature,
        slot,
        pre_balances,
        post_balances,
        pre_token_balances,
        post_token_balances,
        ..
    } = &mut event
    {
        *signature = value["transaction"]["signatures"][0]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        *slot = value["slot"].as_u64();
        *pre_balances = serde_json::from_value(meta["preBalances"].clone()).unwrap();
        *post_balances = serde_json::from_value(meta["postBalances"].clone()).unwrap();
        let balances = |v: &serde_json::Value| -> Vec<crate::types::RawTokenBalance> {
            v.as_array()
                .unwrap()
                .iter()
                .map(|b| crate::types::RawTokenBalance {
                    account_index: b["accountIndex"].as_u64().unwrap() as u32,
                    mint: b["mint"].as_str().unwrap().to_string(),
                    owner: b["owner"].as_str().map(str::to_string),
                    amount: b["uiTokenAmount"]["amount"]
                        .as_str()
                        .unwrap()
                        .parse()
                        .unwrap(),
                })
                .collect()
        };
        *pre_token_balances = balances(&meta["preTokenBalances"]);
        *post_token_balances = balances(&meta["postTokenBalances"]);
    }
    event
}

#[test]
fn identity_route_non_sol_pair_cannot_reenter_through_cpi() {
    let event = identity_transaction_fixture(include_str!(
        "../tests/fixtures/identity_route_448462658.fixture"
    ));
    let pool: Pubkey = "6VcDLAMaKGEQLX7YiR8JnwUnoyU9mMJuMSYZnK9XmVTN"
        .parse()
        .unwrap();
    for cached in [false, true] {
        let parser = BinaryParser::new(false);
        if cached {
            parser
                .curve_mint_registry()
                .insert_pk(&pool, &Pubkey::new_unique());
        }
        let trades = parser.parse_trades(&event).unwrap();
        assert!(
            trades.iter().all(|t| t.pool_amm_id != pool),
            "Para bez SOL odrzucona przez instrukcję nie może wrócić przez CPI: {trades:?}"
        );
    }
}

#[test]
fn identity_route_uses_pool_instruction_mint_with_cold_and_conflicting_cache() {
    let event = identity_route_fixture();
    let pool: Pubkey = "Gf7sXMoP8iRw4iiXmJ1nq4vxcRycbGXy5RL8a8LnTd3v"
        .parse()
        .unwrap();
    let mint: Pubkey = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
        .parse()
        .unwrap();
    for conflicting_cache in [false, true] {
        let parser = BinaryParser::new(false);
        if conflicting_cache {
            parser
                .curve_mint_registry()
                .insert_pk(&pool, &Pubkey::new_unique());
        }
        let trades = parser.parse_trades(&event).unwrap();
        let pool_trades: Vec<_> = trades.iter().filter(|t| t.pool_amm_id == pool).collect();
        eprintln!(
            "identity_route cache={conflicting_cache} trades={:?}",
            trades
                .iter()
                .map(|t| (t.pool_amm_id, t.mint, t.event_ordinal))
                .collect::<Vec<_>>()
        );
        assert_eq!(pool_trades.len(), 1);
        assert_eq!(pool_trades[0].mint, mint);
        assert_eq!(
            parser.curve_mint_registry().mint_for_curve_pk(&pool),
            Some(mint)
        );
    }
}
