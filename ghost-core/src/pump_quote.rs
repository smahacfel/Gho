//! Versioned Pump bonding-curve quote contract.
//!
//! This module deliberately does **not** reuse `BondingCurve::simulate_buy`
//! or `simulate_sell`.  Those compatibility helpers model the historical
//! one-percent path and accept an ambiguous SOL input.  Pump's current V2
//! routes distinguish an exact-base-out buy, an exact-quote-in buy and an
//! exact-base-in sell, and their instruction limits have different meanings.
//!
//! The output is split into three non-overlapping layers:
//!
//! * [`ProgramStateTransition`] — constant-product reserve movement only;
//! * [`ProgramSettlement`] — curve quote plus program fees and user debit or
//!   credit only;
//! * [`TransactionCosts`] — network, tip, ATA and retry costs only.
//!
//! Callers must provide a versioned [`ProgramFeeSchedule`].  There is no
//! fallback to `FEE_BPS = 100`: absent or invalid fee evidence fails closed.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Route semantics, rather than a lossy `is_buy` boolean.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PumpRouteVariant {
    LegacyBuy,
    BuyV2,
    BuyExactQuoteInV2,
    LegacySell,
    SellV2,
}

impl PumpRouteVariant {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LegacyBuy => "legacy_buy",
            Self::BuyV2 => "buy_v2",
            Self::BuyExactQuoteInV2 => "buy_exact_quote_in_v2",
            Self::LegacySell => "legacy_sell",
            Self::SellV2 => "sell_v2",
        }
    }
}

/// Virtual and real reserves observed at one canonical state boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpReserveState {
    pub virtual_base_reserves: u64,
    pub virtual_quote_reserves: u64,
    pub real_base_reserves: u64,
    pub real_quote_reserves: u64,
}

/// The reserve movement performed by the bonding curve.  It intentionally
/// excludes protocol, creator, network and tip costs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramStateTransition {
    pub base_before: u64,
    pub base_after: u64,
    pub quote_before: u64,
    pub quote_after: u64,
    pub base_amount: u64,
    pub curve_quote_amount: u64,
}

/// Deterministic rounding rule declared by a fee schedule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeeRounding {
    Floor,
    Ceil,
}

/// One program-level fee rule.  A rational representation is intentional:
/// current Pump fee splits include half-basis-point legs, so `u64 bps` is not
/// an adequate authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramFeeRule {
    pub component_id: String,
    pub numerator: u64,
    pub denominator: u64,
    pub rounding: FeeRounding,
}

/// Fee authority supplied from an on-chain config snapshot, an effective-slot
/// registry, or a complete canonical fixture.  The caller owns acquisition of
/// that evidence; the quote engine never assumes a default schedule.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramFeeSchedule {
    pub fee_schedule_id: String,
    pub effective_slot: u64,
    pub rules: Vec<ProgramFeeRule>,
}

/// One materialised program fee.  Its amount is always based on the curve
/// quote amount, never on transaction-level costs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramFeeCharge {
    pub component_id: String,
    pub amount: u64,
}

/// Settlement between the user and Pump itself.  `wallet_debit_or_credit` is
/// positive for a buy debit and a sell credit; the direction is explicit in
/// the enclosing route variant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramSettlement {
    pub curve_quote_amount: u64,
    pub program_fee_breakdown: Vec<ProgramFeeCharge>,
    pub program_fee_total: u64,
    pub wallet_debit_or_credit: u64,
}

/// Result of checking the instruction-level limit.  It has no transaction
/// fees, priority fees, tip, ATA or retry costs mixed into it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstructionLimitCheck {
    pub limit: u64,
    pub required_or_produced: u64,
    pub passed: bool,
}

/// Costs charged by the transaction envelope, not by the Pump program.  A
/// close refund is represented separately so cost accounting cannot silently
/// subtract program fees twice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TransactionCosts {
    pub base_fee_lamports: u64,
    pub priority_fee_lamports: u64,
    pub jito_tip_lamports: u64,
    pub ata_rent_lamports: u64,
    pub ata_close_refund_lamports: u64,
    pub retry_or_failure_cost_lamports: u64,
}

impl TransactionCosts {
    /// Net transaction envelope debit after an explicit close refund.
    pub fn net_wallet_debit(&self) -> Result<u64, PumpQuoteError> {
        let debit = self
            .base_fee_lamports
            .checked_add(self.priority_fee_lamports)
            .and_then(|value| value.checked_add(self.jito_tip_lamports))
            .and_then(|value| value.checked_add(self.ata_rent_lamports))
            .and_then(|value| value.checked_add(self.retry_or_failure_cost_lamports))
            .ok_or(PumpQuoteError::ArithmeticOverflow)?;
        debit
            .checked_sub(self.ata_close_refund_lamports)
            .ok_or(PumpQuoteError::TransactionRefundExceedsDebit)
    }
}

/// Complete versioned quote.  The three layers may be accounted separately
/// and must not be summed implicitly by the quote engine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpQuoteV1 {
    pub route_variant: PumpRouteVariant,
    pub fee_schedule_id: String,
    pub reserve_transition: ProgramStateTransition,
    pub token_amount: u64,
    pub curve_quote_amount: u64,
    pub program_settlement: ProgramSettlement,
    /// Present only where the instruction separately constrains its produced
    /// base amount (`buy_exact_quote_in_v2`).  It is deliberately distinct
    /// from [`Self::instruction_limit_check`]: comparing a token amount to a
    /// lamport debit cap would hide a units error.
    pub output_limit_check: Option<InstructionLimitCheck>,
    pub instruction_limit_check: InstructionLimitCheck,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PumpQuoteError {
    #[error("requested amount must be non-zero")]
    ZeroAmount,
    #[error("requested base amount exceeds observed real base reserves")]
    InsufficientRealBaseReserves,
    #[error("requested base amount exhausts virtual base reserves")]
    ExhaustedVirtualBaseReserves,
    #[error("fee schedule contains no rules")]
    MissingFeeSchedule,
    #[error("fee rule {component_id} has denominator zero")]
    InvalidFeeRule { component_id: String },
    #[error("fee total exceeds the gross curve quote output")]
    FeeExceedsSellOutput,
    #[error("arithmetic overflow while quoting Pump route")]
    ArithmeticOverflow,
    #[error("ATA close refund exceeds transaction envelope debit")]
    TransactionRefundExceedsDebit,
}

fn checked_u64(value: u128) -> Result<u64, PumpQuoteError> {
    u64::try_from(value).map_err(|_| PumpQuoteError::ArithmeticOverflow)
}

fn ceil_div(numerator: u128, denominator: u128) -> Result<u128, PumpQuoteError> {
    let adjusted = numerator
        .checked_add(
            denominator
                .checked_sub(1)
                .ok_or(PumpQuoteError::ArithmeticOverflow)?,
        )
        .ok_or(PumpQuoteError::ArithmeticOverflow)?;
    Ok(adjusted / denominator)
}

fn fee_charges(
    curve_quote_amount: u64,
    schedule: &ProgramFeeSchedule,
) -> Result<(Vec<ProgramFeeCharge>, u64), PumpQuoteError> {
    if schedule.rules.is_empty() {
        return Err(PumpQuoteError::MissingFeeSchedule);
    }

    let mut charges = Vec::with_capacity(schedule.rules.len());
    let mut total = 0u64;
    for rule in &schedule.rules {
        if rule.denominator == 0 {
            return Err(PumpQuoteError::InvalidFeeRule {
                component_id: rule.component_id.clone(),
            });
        }
        let numerator = (curve_quote_amount as u128)
            .checked_mul(rule.numerator as u128)
            .ok_or(PumpQuoteError::ArithmeticOverflow)?;
        let denominator = rule.denominator as u128;
        let amount = match rule.rounding {
            FeeRounding::Floor => numerator / denominator,
            FeeRounding::Ceil => ceil_div(numerator, denominator)?,
        };
        let amount = checked_u64(amount)?;
        total = total
            .checked_add(amount)
            .ok_or(PumpQuoteError::ArithmeticOverflow)?;
        charges.push(ProgramFeeCharge {
            component_id: rule.component_id.clone(),
            amount,
        });
    }
    Ok((charges, total))
}

/// Quote the current `buy_v2` / legacy exact-base-out semantics.
///
/// `max_quote_cost` is a cap, never curve input.  The curve input is derived
/// first from the desired base output, then program fees are calculated, then
/// the cap is checked against the resulting program wallet debit.
pub fn quote_exact_base_out(
    route_variant: PumpRouteVariant,
    reserves: PumpReserveState,
    requested_base_out: u64,
    max_quote_cost: u64,
    schedule: &ProgramFeeSchedule,
) -> Result<PumpQuoteV1, PumpQuoteError> {
    if requested_base_out == 0 {
        return Err(PumpQuoteError::ZeroAmount);
    }
    if requested_base_out > reserves.real_base_reserves {
        return Err(PumpQuoteError::InsufficientRealBaseReserves);
    }
    if requested_base_out >= reserves.virtual_base_reserves {
        return Err(PumpQuoteError::ExhaustedVirtualBaseReserves);
    }

    let base_after = reserves
        .virtual_base_reserves
        .checked_sub(requested_base_out)
        .ok_or(PumpQuoteError::ArithmeticOverflow)?;
    let invariant = (reserves.virtual_base_reserves as u128)
        .checked_mul(reserves.virtual_quote_reserves as u128)
        .ok_or(PumpQuoteError::ArithmeticOverflow)?;
    let quote_after = checked_u64(ceil_div(invariant, base_after as u128)?)?;
    let curve_quote_amount = quote_after
        .checked_sub(reserves.virtual_quote_reserves)
        .ok_or(PumpQuoteError::ArithmeticOverflow)?;
    let (program_fee_breakdown, program_fee_total) = fee_charges(curve_quote_amount, schedule)?;
    let wallet_debit_or_credit = curve_quote_amount
        .checked_add(program_fee_total)
        .ok_or(PumpQuoteError::ArithmeticOverflow)?;

    Ok(PumpQuoteV1 {
        route_variant,
        fee_schedule_id: schedule.fee_schedule_id.clone(),
        reserve_transition: ProgramStateTransition {
            base_before: reserves.virtual_base_reserves,
            base_after,
            quote_before: reserves.virtual_quote_reserves,
            quote_after,
            base_amount: requested_base_out,
            curve_quote_amount,
        },
        token_amount: requested_base_out,
        curve_quote_amount,
        program_settlement: ProgramSettlement {
            curve_quote_amount,
            program_fee_breakdown,
            program_fee_total,
            wallet_debit_or_credit,
        },
        output_limit_check: None,
        instruction_limit_check: InstructionLimitCheck {
            limit: max_quote_cost,
            required_or_produced: wallet_debit_or_credit,
            passed: wallet_debit_or_credit <= max_quote_cost,
        },
    })
}

/// Quote exact curve quote input (`buy_exact_quote_in_v2`).
///
/// `spendable_quote_in` is the exact curve amount encoded by the instruction,
/// not the all-in wallet debit.  Its second on-chain limit, `min_base_out`, is
/// emitted as a separate [`PumpQuoteV1::output_limit_check`] because it is
/// denominated in base tokens rather than lamports.  The calculated program
/// wallet debit is settlement evidence; it is not invented as a third,
/// off-instruction cap.
pub fn quote_exact_quote_in(
    reserves: PumpReserveState,
    spendable_quote_in: u64,
    min_base_out: u64,
    schedule: &ProgramFeeSchedule,
) -> Result<PumpQuoteV1, PumpQuoteError> {
    if spendable_quote_in == 0 {
        return Err(PumpQuoteError::ZeroAmount);
    }
    let quote_after = reserves
        .virtual_quote_reserves
        .checked_add(spendable_quote_in)
        .ok_or(PumpQuoteError::ArithmeticOverflow)?;
    let invariant = (reserves.virtual_base_reserves as u128)
        .checked_mul(reserves.virtual_quote_reserves as u128)
        .ok_or(PumpQuoteError::ArithmeticOverflow)?;
    let base_after = checked_u64(invariant / quote_after as u128)?;
    let base_amount = reserves
        .virtual_base_reserves
        .checked_sub(base_after)
        .ok_or(PumpQuoteError::ArithmeticOverflow)?;
    if base_amount == 0 {
        return Err(PumpQuoteError::ZeroAmount);
    }
    let (program_fee_breakdown, program_fee_total) = fee_charges(spendable_quote_in, schedule)?;
    let wallet_debit_or_credit = spendable_quote_in
        .checked_add(program_fee_total)
        .ok_or(PumpQuoteError::ArithmeticOverflow)?;

    Ok(PumpQuoteV1 {
        route_variant: PumpRouteVariant::BuyExactQuoteInV2,
        fee_schedule_id: schedule.fee_schedule_id.clone(),
        reserve_transition: ProgramStateTransition {
            base_before: reserves.virtual_base_reserves,
            base_after,
            quote_before: reserves.virtual_quote_reserves,
            quote_after,
            base_amount,
            curve_quote_amount: spendable_quote_in,
        },
        token_amount: base_amount,
        curve_quote_amount: spendable_quote_in,
        program_settlement: ProgramSettlement {
            curve_quote_amount: spendable_quote_in,
            program_fee_breakdown,
            program_fee_total,
            wallet_debit_or_credit,
        },
        output_limit_check: Some(InstructionLimitCheck {
            limit: min_base_out,
            required_or_produced: base_amount,
            passed: base_amount >= min_base_out,
        }),
        instruction_limit_check: InstructionLimitCheck {
            limit: spendable_quote_in,
            required_or_produced: spendable_quote_in,
            passed: true,
        },
    })
}

/// Quote exact base input sell (`sell_v2` / legacy sell).  Program fees are
/// subtracted from the gross curve output; network costs stay outside this
/// quote in [`TransactionCosts`].
pub fn quote_exact_base_in_sell(
    route_variant: PumpRouteVariant,
    reserves: PumpReserveState,
    base_amount_in: u64,
    min_quote_output: u64,
    schedule: &ProgramFeeSchedule,
) -> Result<PumpQuoteV1, PumpQuoteError> {
    if base_amount_in == 0 {
        return Err(PumpQuoteError::ZeroAmount);
    }
    let base_after = reserves
        .virtual_base_reserves
        .checked_add(base_amount_in)
        .ok_or(PumpQuoteError::ArithmeticOverflow)?;
    let invariant = (reserves.virtual_base_reserves as u128)
        .checked_mul(reserves.virtual_quote_reserves as u128)
        .ok_or(PumpQuoteError::ArithmeticOverflow)?;
    // Pump's sell transition rounds the remaining quote reserve upward.  The
    // user therefore receives `quote_before - ceil(k / base_after)`, matching
    // the canonical reserve decrement rather than the historical simulator's
    // one-percent post-processing.
    let quote_after = checked_u64(ceil_div(invariant, base_after as u128)?)?;
    let curve_quote_amount = reserves
        .virtual_quote_reserves
        .checked_sub(quote_after)
        .ok_or(PumpQuoteError::ArithmeticOverflow)?;
    let (program_fee_breakdown, program_fee_total) = fee_charges(curve_quote_amount, schedule)?;
    let wallet_debit_or_credit = curve_quote_amount
        .checked_sub(program_fee_total)
        .ok_or(PumpQuoteError::FeeExceedsSellOutput)?;

    Ok(PumpQuoteV1 {
        route_variant,
        fee_schedule_id: schedule.fee_schedule_id.clone(),
        reserve_transition: ProgramStateTransition {
            base_before: reserves.virtual_base_reserves,
            base_after,
            quote_before: reserves.virtual_quote_reserves,
            quote_after,
            base_amount: base_amount_in,
            curve_quote_amount,
        },
        token_amount: base_amount_in,
        curve_quote_amount,
        program_settlement: ProgramSettlement {
            curve_quote_amount,
            program_fee_breakdown,
            program_fee_total,
            wallet_debit_or_credit,
        },
        output_limit_check: None,
        instruction_limit_check: InstructionLimitCheck {
            limit: min_quote_output,
            required_or_produced: wallet_debit_or_credit,
            passed: wallet_debit_or_credit >= min_quote_output,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buy_v2_exact_base_out_uses_cap_only_as_limit() {
        let reserves = PumpReserveState {
            virtual_base_reserves: 494_936_173_200_993,
            virtual_quote_reserves: 65_038_689_937,
            real_base_reserves: 421_936_173_200_993,
            real_quote_reserves: 0,
        };
        let schedule = ProgramFeeSchedule {
            fee_schedule_id: "fixture-buy-v2-434365563".into(),
            effective_slot: 434_365_563,
            rules: vec![ProgramFeeRule {
                component_id: "protocol_and_buyback".into(),
                numerator: 95,
                denominator: 10_000,
                rounding: FeeRounding::Ceil,
            }],
        };

        let quote = quote_exact_base_out(
            PumpRouteVariant::BuyV2,
            reserves,
            3_488_479_091_111,
            467_749_638,
            &schedule,
        )
        .unwrap();

        assert_eq!(quote.curve_quote_amount, 461_668_888);
        assert_eq!(quote.reserve_transition.quote_after, 65_500_358_825);
        assert_eq!(quote.program_settlement.program_fee_total, 4_385_855);
        assert_eq!(quote.program_settlement.wallet_debit_or_credit, 466_054_743);
        assert!(quote.instruction_limit_check.passed);
        assert_ne!(quote.curve_quote_amount, 467_749_638);
    }

    #[test]
    fn sell_exact_base_in_keeps_program_and_transaction_costs_separate() {
        let reserves = PumpReserveState {
            virtual_base_reserves: 462_474_426_627_677,
            virtual_quote_reserves: 69_603_849_321,
            real_base_reserves: 0,
            real_quote_reserves: 0,
        };
        let schedule = ProgramFeeSchedule {
            fee_schedule_id: "fixture-legacy-sell-434365533".into(),
            effective_slot: 434_365_533,
            rules: vec![
                ProgramFeeRule {
                    component_id: "lp_fee".into(),
                    numerator: 3,
                    denominator: 1_000,
                    rounding: FeeRounding::Ceil,
                },
                ProgramFeeRule {
                    component_id: "protocol_fee_recipient".into(),
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
            reserves,
            14_753_444_836_747,
            1_487_428_297,
            &schedule,
        )
        .unwrap();
        assert_eq!(quote.curve_quote_amount, 2_151_795_007);
        assert_eq!(quote.program_settlement.program_fee_total, 48_146_414);
        assert_eq!(
            quote.program_settlement.wallet_debit_or_credit,
            2_103_648_593
        );
        assert!(quote.instruction_limit_check.passed);

        let costs = TransactionCosts {
            base_fee_lamports: 3_005_000,
            jito_tip_lamports: 1_185_624,
            ..TransactionCosts::default()
        };
        assert_eq!(costs.net_wallet_debit().unwrap(), 4_190_624);
        assert_eq!(
            quote.program_settlement.wallet_debit_or_credit - costs.net_wallet_debit().unwrap(),
            2_099_457_969
        );
    }

    #[test]
    fn empty_fee_schedule_fails_closed() {
        let result = quote_exact_base_out(
            PumpRouteVariant::BuyV2,
            PumpReserveState {
                virtual_base_reserves: 10,
                virtual_quote_reserves: 10,
                real_base_reserves: 10,
                real_quote_reserves: 0,
            },
            1,
            10,
            &ProgramFeeSchedule {
                fee_schedule_id: "missing".into(),
                effective_slot: 0,
                rules: vec![],
            },
        );
        assert_eq!(result, Err(PumpQuoteError::MissingFeeSchedule));
    }

    #[test]
    fn exact_quote_in_keeps_curve_input_and_token_output_limits_separate() {
        let quote = quote_exact_quote_in(
            PumpReserveState {
                virtual_base_reserves: 1_000_000,
                virtual_quote_reserves: 1_000_000,
                real_base_reserves: 1_000_000,
                real_quote_reserves: 0,
            },
            10_000,
            9_000,
            &ProgramFeeSchedule {
                fee_schedule_id: "unit".into(),
                effective_slot: 1,
                rules: vec![ProgramFeeRule {
                    component_id: "protocol".into(),
                    numerator: 1,
                    denominator: 100,
                    rounding: FeeRounding::Ceil,
                }],
            },
        )
        .unwrap();

        assert_eq!(quote.instruction_limit_check.limit, 10_000);
        assert_eq!(quote.instruction_limit_check.required_or_produced, 10_000);
        assert!(quote.instruction_limit_check.passed);
        let output = quote.output_limit_check.unwrap();
        assert_eq!(output.limit, 9_000);
        assert_eq!(output.required_or_produced, quote.token_amount);
        assert!(output.passed);
    }
}
