//! Offline golden-fixture parity for the Pump route/quote boundary.
//!
//! These tests deliberately have no RPC dependency.  The fixtures retain the
//! canonical instruction, reserve transition, program settlement and separate
//! transaction costs captured from successful mainnet transactions.

use ghost_core::{
    quote_exact_base_in_sell, quote_exact_base_out, FeeRounding, ProgramFeeRule,
    ProgramFeeSchedule, PumpReserveState, PumpRouteVariant, TransactionCosts,
};
use serde::Deserialize;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use trigger::{
    decode_pump_route_instruction, validate_legacy_sell_instruction, validate_route_account_count,
    DecodedPumpRouteInstruction, PumpLegacySellRouteAccounts, PumpLegacySellRouteBuilder,
    PumpV2RouteAccounts, PumpV2RouteBuilder, SPL_TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID,
    WRAPPED_SOL_MINT,
};

#[derive(Debug, Deserialize)]
struct Fixture {
    source: Source,
    route_variant: String,
    instruction_data_hex: String,
    instruction_args: InstructionArgs,
    accounts: Vec<String>,
    reserve_before: Reserves,
    reserve_after: Reserves,
    program_settlement: Settlement,
    transaction_costs: TransactionCostsFixture,
    #[serde(default)]
    wallet_debit_including_transaction_costs: Option<u64>,
    #[serde(default)]
    wallet_credit_after_transaction_costs: Option<u64>,
    #[serde(default)]
    builder_simulation: Option<BuilderSimulation>,
    chain_execution_status: String,
}

#[derive(Debug, Deserialize)]
struct BuilderSimulation {
    captured_slot: u64,
    status: String,
    units_consumed: u64,
    sig_verify: bool,
    replace_recent_blockhash: bool,
    prepended_idempotent_base_ata_create: bool,
}

#[derive(Debug, Deserialize)]
struct Source {
    signature: String,
    slot: u64,
    idl_commit: String,
    idl_sha256: String,
}

#[derive(Debug, Deserialize)]
struct InstructionArgs {
    #[serde(default)]
    requested_base_out: Option<u64>,
    #[serde(default)]
    max_sol_cost: Option<u64>,
    #[serde(default)]
    base_amount_in: Option<u64>,
    #[serde(default)]
    min_sol_output: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct Reserves {
    virtual_base: u64,
    virtual_quote: u64,
}

#[derive(Debug, Deserialize)]
struct Settlement {
    #[serde(default)]
    curve_quote_input: Option<u64>,
    #[serde(default)]
    curve_quote_output: Option<u64>,
    fees: Fees,
    #[serde(default)]
    wallet_debit: Option<u64>,
    #[serde(default)]
    wallet_credit_before_transaction_costs: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct Fees {
    #[serde(default)]
    lp_fee: u64,
    fee_recipient: u64,
    buyback_fee_recipient: u64,
    #[serde(default)]
    creator_fee: u64,
}

#[derive(Debug, Deserialize)]
struct TransactionCostsFixture {
    base_fee: u64,
    priority_fee: u64,
    jito_tip: u64,
    ata_rent: u64,
    #[serde(default)]
    user_volume_accumulator_rent: u64,
}

fn read_fixture(raw: &str) -> Fixture {
    serde_json::from_str(raw).expect("static Pump fixture must remain valid JSON")
}

fn decode_hex(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0, "fixture hex must have complete bytes");
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).unwrap())
        .collect()
}

fn fixture_v2_accounts() -> PumpV2RouteAccounts {
    PumpV2RouteAccounts {
        base_mint: Pubkey::from_str("6P1EKxhPB1ZBMmduhuHZ9DcMVcgHqzj7frPNSduZpump").unwrap(),
        quote_mint: WRAPPED_SOL_MINT,
        base_token_program: TOKEN_2022_PROGRAM_ID,
        quote_token_program: SPL_TOKEN_PROGRAM_ID,
        user: Pubkey::from_str("56S29mZ3wqvw8hATuUUFqKhGcSGYFASRRFNT38W8q7G3").unwrap(),
        creator: Pubkey::from_str("79iC1835GFEruVVoh1tp6mcmAo3tVdR3V6TCs1BXBXWY").unwrap(),
        fee_recipient: Pubkey::from_str("FWsW1xNtWscwNmKv6wVsU1iTzRN6wmmk3MjxRP5tT7hz").unwrap(),
        buyback_fee_recipient: Pubkey::from_str("9M4giFFMxmFGXtc3feFzRai56WbBqehoSeRE5GK7gf7")
            .unwrap(),
    }
}

fn fixture_legacy_sell_accounts() -> PumpLegacySellRouteAccounts {
    PumpLegacySellRouteAccounts {
        base_mint: Pubkey::from_str("6P1EKxhPB1ZBMmduhuHZ9DcMVcgHqzj7frPNSduZpump").unwrap(),
        base_token_program: TOKEN_2022_PROGRAM_ID,
        user: Pubkey::from_str("6VCqJ1Jcoy2i2jcfZt15PFQfb6jBb8LQsSExv3kiuEUA").unwrap(),
        creator: Pubkey::from_str("79iC1835GFEruVVoh1tp6mcmAo3tVdR3V6TCs1BXBXWY").unwrap(),
        fee_recipient: Pubkey::from_str("7VtfL8fvgNfhz17qKRMjzQEXgbdpnHHHQRh54R9jP2RJ").unwrap(),
        buyback_fee_recipient: Pubkey::from_str("5YxQFdt3Tr9zJLvkFccqXVUwhdTWJQc1fFg2YPbxvxeD")
            .unwrap(),
    }
}

#[test]
fn buy_v2_fixture_has_exact_builder_reserves_settlement_and_cost_conservation() {
    let fixture = read_fixture(include_str!("fixtures/pump_buy_v2_434365563.json"));
    assert_eq!(fixture.source.slot, 434_365_563);
    assert_eq!(
        fixture.source.idl_commit,
        "9c82f61cb711b044a17f770ab8ce9f9bdf78f333"
    );
    assert_eq!(
        fixture.source.idl_sha256,
        "b90bc471327f671449271d5d1d42354d1fae6f5a06502f5834459a3108138e49"
    );
    assert_eq!(fixture.chain_execution_status, "success");
    assert_eq!(fixture.route_variant, "buy_v2");
    let builder_simulation = fixture.builder_simulation.as_ref().unwrap();
    assert_eq!(builder_simulation.captured_slot, 434_374_399);
    assert_eq!(builder_simulation.status, "success");
    assert_eq!(builder_simulation.units_consumed, 101_029);
    assert!(!builder_simulation.sig_verify);
    assert!(builder_simulation.replace_recent_blockhash);
    assert!(builder_simulation.prepended_idempotent_base_ata_create);

    let decoded =
        decode_pump_route_instruction(&decode_hex(&fixture.instruction_data_hex)).unwrap();
    assert_eq!(
        decoded,
        DecodedPumpRouteInstruction::BuyV2 {
            amount: fixture.instruction_args.requested_base_out.unwrap(),
            max_sol_cost: fixture.instruction_args.max_sol_cost.unwrap(),
        }
    );

    let accounts = fixture_v2_accounts();
    let builder = PumpV2RouteBuilder::build_buy_v2(
        &accounts,
        fixture.instruction_args.requested_base_out.unwrap(),
        fixture.instruction_args.max_sol_cost.unwrap(),
    )
    .unwrap();
    assert_eq!(
        builder
            .accounts
            .iter()
            .map(|account| account.pubkey.to_string())
            .collect::<Vec<_>>(),
        fixture.accounts,
    );
    let schedule = ProgramFeeSchedule {
        fee_schedule_id: format!("fixture-buy-v2-{}", fixture.source.slot),
        effective_slot: fixture.source.slot,
        rules: vec![
            ProgramFeeRule {
                component_id: "fee_recipient".into(),
                numerator: 95,
                denominator: 20_000,
                rounding: FeeRounding::Ceil,
            },
            ProgramFeeRule {
                component_id: "buyback_fee_recipient".into(),
                numerator: 95,
                denominator: 20_000,
                rounding: FeeRounding::Floor,
            },
        ],
    };
    let quote = quote_exact_base_out(
        PumpRouteVariant::BuyV2,
        PumpReserveState {
            virtual_base_reserves: fixture.reserve_before.virtual_base,
            virtual_quote_reserves: fixture.reserve_before.virtual_quote,
            real_base_reserves: fixture.reserve_before.virtual_base,
            real_quote_reserves: 0,
        },
        fixture.instruction_args.requested_base_out.unwrap(),
        fixture.instruction_args.max_sol_cost.unwrap(),
        &schedule,
    )
    .unwrap();
    assert_eq!(
        quote.reserve_transition.base_after,
        fixture.reserve_after.virtual_base
    );
    assert_eq!(
        quote.reserve_transition.quote_after,
        fixture.reserve_after.virtual_quote
    );
    assert_eq!(
        quote.curve_quote_amount,
        fixture.program_settlement.curve_quote_input.unwrap()
    );
    assert_eq!(
        quote.program_settlement.program_fee_breakdown[0].amount,
        fixture.program_settlement.fees.fee_recipient
    );
    assert_eq!(
        quote.program_settlement.program_fee_breakdown[1].amount,
        fixture.program_settlement.fees.buyback_fee_recipient
    );
    assert_eq!(
        quote.program_settlement.wallet_debit_or_credit,
        fixture.program_settlement.wallet_debit.unwrap()
    );
    assert!(quote.instruction_limit_check.passed);

    let costs = TransactionCosts {
        base_fee_lamports: fixture.transaction_costs.base_fee,
        priority_fee_lamports: fixture.transaction_costs.priority_fee,
        jito_tip_lamports: fixture.transaction_costs.jito_tip,
        ata_rent_lamports: fixture.transaction_costs.ata_rent
            + fixture.transaction_costs.user_volume_accumulator_rent,
        ..TransactionCosts::default()
    };
    assert_eq!(
        quote.program_settlement.wallet_debit_or_credit + costs.net_wallet_debit().unwrap(),
        fixture.wallet_debit_including_transaction_costs.unwrap(),
    );
}

#[test]
fn legacy_sell_fixture_has_exact_gross_net_and_transaction_cost_conservation() {
    let fixture = read_fixture(include_str!("fixtures/pump_legacy_sell_434365533.json"));
    assert_eq!(
        fixture.source.signature,
        "2Y68uh5FrbALZFBetEdDkwrPYVfrC2BPuu1sHutGLoMSXrVx2vvfphskstv5t1JziHagqdGDLs1Eb4as54nEQUXk"
    );
    assert_eq!(fixture.route_variant, "legacy_sell");
    assert_eq!(fixture.chain_execution_status, "success");
    let decoded =
        decode_pump_route_instruction(&decode_hex(&fixture.instruction_data_hex)).unwrap();
    assert_eq!(
        decoded,
        DecodedPumpRouteInstruction::LegacySell {
            amount: fixture.instruction_args.base_amount_in.unwrap(),
            min_sol_output: fixture.instruction_args.min_sol_output.unwrap(),
        }
    );
    validate_route_account_count(PumpRouteVariant::LegacySell, fixture.accounts.len()).unwrap();
    let builder = PumpLegacySellRouteBuilder::build_sell(
        &fixture_legacy_sell_accounts(),
        fixture.instruction_args.base_amount_in.unwrap(),
        fixture.instruction_args.min_sol_output.unwrap(),
    )
    .unwrap();
    assert_eq!(
        builder
            .accounts
            .iter()
            .map(|account| account.pubkey.to_string())
            .collect::<Vec<_>>(),
        fixture.accounts,
    );
    assert_eq!(
        validate_legacy_sell_instruction(&builder, &fixture_legacy_sell_accounts()).unwrap(),
        decoded
    );

    let schedule = ProgramFeeSchedule {
        fee_schedule_id: format!("fixture-legacy-sell-{}", fixture.source.slot),
        effective_slot: fixture.source.slot,
        rules: vec![
            ProgramFeeRule {
                component_id: "lp_fee".into(),
                numerator: 3,
                denominator: 1_000,
                rounding: FeeRounding::Ceil,
            },
            ProgramFeeRule {
                component_id: "fee_recipient".into(),
                numerator: 95,
                denominator: 20_000,
                rounding: FeeRounding::Ceil,
            },
            ProgramFeeRule {
                component_id: "buyback_fee_recipient".into(),
                numerator: 95,
                denominator: 20_000,
                rounding: FeeRounding::Floor,
            },
            ProgramFeeRule {
                component_id: "creator_fee".into(),
                numerator: 395,
                denominator: 40_000,
                rounding: FeeRounding::Floor,
            },
        ],
    };
    let quote = quote_exact_base_in_sell(
        PumpRouteVariant::LegacySell,
        PumpReserveState {
            virtual_base_reserves: fixture.reserve_before.virtual_base,
            virtual_quote_reserves: fixture.reserve_before.virtual_quote,
            real_base_reserves: 0,
            real_quote_reserves: 0,
        },
        fixture.instruction_args.base_amount_in.unwrap(),
        fixture.instruction_args.min_sol_output.unwrap(),
        &schedule,
    )
    .unwrap();
    assert_eq!(
        quote.reserve_transition.base_after,
        fixture.reserve_after.virtual_base
    );
    assert_eq!(
        quote.reserve_transition.quote_after,
        fixture.reserve_after.virtual_quote
    );
    assert_eq!(
        quote.curve_quote_amount,
        fixture.program_settlement.curve_quote_output.unwrap()
    );
    assert_eq!(
        quote.program_settlement.program_fee_breakdown[0].amount,
        fixture.program_settlement.fees.lp_fee
    );
    assert_eq!(
        quote.program_settlement.program_fee_breakdown[1].amount,
        fixture.program_settlement.fees.fee_recipient
    );
    assert_eq!(
        quote.program_settlement.program_fee_breakdown[2].amount,
        fixture.program_settlement.fees.buyback_fee_recipient
    );
    assert_eq!(
        quote.program_settlement.program_fee_breakdown[3].amount,
        fixture.program_settlement.fees.creator_fee
    );
    assert_eq!(
        quote.program_settlement.wallet_debit_or_credit,
        fixture
            .program_settlement
            .wallet_credit_before_transaction_costs
            .unwrap()
    );

    let costs = TransactionCosts {
        base_fee_lamports: fixture.transaction_costs.base_fee,
        priority_fee_lamports: fixture.transaction_costs.priority_fee,
        jito_tip_lamports: fixture.transaction_costs.jito_tip,
        ata_rent_lamports: fixture.transaction_costs.ata_rent,
        ..TransactionCosts::default()
    };
    assert_eq!(
        quote.program_settlement.wallet_debit_or_credit - costs.net_wallet_debit().unwrap(),
        fixture.wallet_credit_after_transaction_costs.unwrap(),
    );
}
