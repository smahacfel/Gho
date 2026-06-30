//! Shadow Burnin V2 deterministic price reconstruction formulas.
//!
//! This module is intentionally side-by-side and inert. It is a pure formula
//! library for Shadow V2 research artifacts; it does not read runtime state,
//! submit transactions, or mutate any active BUY/SELL path.

use serde::{Deserialize, Serialize};
use std::fmt;

pub const SHADOW_V2_PRICE_FORMULA_VERSION: &str = "shadow_v2_constant_product_price_v1";
pub const SHADOW_V2_BPS_DENOMINATOR: u64 = 10_000;
pub const SHADOW_V2_SOL_LAMPORTS: u64 = 1_000_000_000;
pub const SHADOW_V2_MAX_SUPPORTED_TOKEN_DECIMALS: u8 = 18;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ShadowV2PoolPhase {
    BondingCurve,
    Amm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ShadowV2QuoteSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowV2Reserves {
    pub sol_reserves_lamports: u64,
    pub token_reserves_raw: u64,
    pub token_decimals: u8,
    pub sol_lamports: u64,
}

impl ShadowV2Reserves {
    pub const fn new(
        sol_reserves_lamports: u64,
        token_reserves_raw: u64,
        token_decimals: u8,
        sol_lamports: u64,
    ) -> Self {
        Self {
            sol_reserves_lamports,
            token_reserves_raw,
            token_decimals,
            sol_lamports,
        }
    }

    pub fn validate(self) -> Result<(), ShadowV2PriceError> {
        if self.sol_reserves_lamports == 0 {
            return Err(ShadowV2PriceError::MissingOrZeroSolReserves);
        }
        if self.token_reserves_raw == 0 {
            return Err(ShadowV2PriceError::MissingOrZeroTokenReserves);
        }
        if self.sol_lamports == 0 {
            return Err(ShadowV2PriceError::MissingSolLamportsNormalization);
        }
        if self.token_decimals > SHADOW_V2_MAX_SUPPORTED_TOKEN_DECIMALS {
            return Err(ShadowV2PriceError::UnsupportedTokenDecimals {
                token_decimals: self.token_decimals,
            });
        }
        Ok(())
    }

    pub fn mark_price_sol_per_token(self) -> Result<f64, ShadowV2PriceError> {
        self.validate()?;
        Ok(
            normalized_sol(self.sol_reserves_lamports, self.sol_lamports)
                / normalized_tokens(self.token_reserves_raw, self.token_decimals),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowV2Quote {
    pub formula_version: String,
    pub pool_phase: ShadowV2PoolPhase,
    pub side: ShadowV2QuoteSide,
    pub input_amount: u64,
    pub expected_output_amount: u64,
    pub min_output_amount: u64,
    pub fee_bps: u16,
    pub fee_amount_lamports: u64,
    pub slippage_tolerance_bps: u16,
    pub mark_price_sol_per_token: f64,
    pub impact_price_sol_per_token: f64,
    pub fill_price_sol_per_token: f64,
    pub pool_price_after_sol_per_token: f64,
    pub own_impact_bps: i32,
    pub post_sol_reserves_lamports: u64,
    pub post_token_reserves_raw: u64,
}

impl ShadowV2Quote {
    pub fn price_source_label(&self) -> &'static str {
        match self.pool_phase {
            ShadowV2PoolPhase::BondingCurve => "shadow_v2_price:constant_product_virtual_reserves",
            ShadowV2PoolPhase::Amm => "shadow_v2_price:constant_product_real_reserves",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShadowV2PriceError {
    MissingOrZeroSolReserves,
    MissingOrZeroTokenReserves,
    MissingSolLamportsNormalization,
    UnsupportedTokenDecimals { token_decimals: u8 },
    ZeroInputAmount,
    InvalidFeeBps { fee_bps: u16 },
    InvalidSlippageBps { slippage_bps: u16 },
    OutputWouldBeZero,
    ArithmeticOverflow,
}

impl fmt::Display for ShadowV2PriceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingOrZeroSolReserves => write!(f, "missing or zero SOL reserves"),
            Self::MissingOrZeroTokenReserves => write!(f, "missing or zero token reserves"),
            Self::MissingSolLamportsNormalization => {
                write!(f, "missing SOL lamports normalization")
            }
            Self::UnsupportedTokenDecimals { token_decimals } => {
                write!(f, "unsupported token decimals {token_decimals}")
            }
            Self::ZeroInputAmount => write!(f, "input amount must be greater than zero"),
            Self::InvalidFeeBps { fee_bps } => write!(f, "invalid fee_bps {fee_bps}"),
            Self::InvalidSlippageBps { slippage_bps } => {
                write!(f, "invalid slippage_bps {slippage_bps}")
            }
            Self::OutputWouldBeZero => write!(f, "quote output would be zero"),
            Self::ArithmeticOverflow => write!(f, "quote arithmetic overflow"),
        }
    }
}

impl std::error::Error for ShadowV2PriceError {}

pub fn mark_price_sol_per_token(reserves: ShadowV2Reserves) -> Result<f64, ShadowV2PriceError> {
    reserves.mark_price_sol_per_token()
}

pub fn quote_constant_product(
    pool_phase: ShadowV2PoolPhase,
    side: ShadowV2QuoteSide,
    reserves: ShadowV2Reserves,
    input_amount: u64,
    fee_bps: u16,
    slippage_bps: u16,
) -> Result<ShadowV2Quote, ShadowV2PriceError> {
    reserves.validate()?;
    validate_bps(fee_bps, slippage_bps)?;
    if input_amount == 0 {
        return Err(ShadowV2PriceError::ZeroInputAmount);
    }

    match side {
        ShadowV2QuoteSide::Buy => {
            quote_buy_constant_product(pool_phase, reserves, input_amount, fee_bps, slippage_bps)
        }
        ShadowV2QuoteSide::Sell => {
            quote_sell_constant_product(pool_phase, reserves, input_amount, fee_bps, slippage_bps)
        }
    }
}

pub fn apply_slippage_bps_floor(amount: u64, slippage_bps: u16) -> Result<u64, ShadowV2PriceError> {
    if slippage_bps as u64 > SHADOW_V2_BPS_DENOMINATOR {
        return Err(ShadowV2PriceError::InvalidSlippageBps { slippage_bps });
    }
    Ok(
        ((amount as u128).saturating_mul((SHADOW_V2_BPS_DENOMINATOR - slippage_bps as u64) as u128)
            / SHADOW_V2_BPS_DENOMINATOR as u128) as u64,
    )
}

fn quote_buy_constant_product(
    pool_phase: ShadowV2PoolPhase,
    reserves: ShadowV2Reserves,
    sol_in_lamports: u64,
    fee_bps: u16,
    slippage_bps: u16,
) -> Result<ShadowV2Quote, ShadowV2PriceError> {
    let fee_lamports = fee_amount_floor(sol_in_lamports, fee_bps)?;
    let effective_sol_in = sol_in_lamports.saturating_sub(fee_lamports);
    if effective_sol_in == 0 {
        return Err(ShadowV2PriceError::OutputWouldBeZero);
    }

    let k = invariant(reserves)?;
    let post_sol = reserves
        .sol_reserves_lamports
        .checked_add(effective_sol_in)
        .ok_or(ShadowV2PriceError::ArithmeticOverflow)?;
    let post_token = div_u128_to_u64(k, post_sol as u128)?;
    let tokens_out = reserves.token_reserves_raw.saturating_sub(post_token);
    if tokens_out == 0 {
        return Err(ShadowV2PriceError::OutputWouldBeZero);
    }

    let mark = reserves.mark_price_sol_per_token()?;
    let impact_price = normalized_sol(effective_sol_in, reserves.sol_lamports)
        / normalized_tokens(tokens_out, reserves.token_decimals);
    let fill_price = normalized_sol(sol_in_lamports, reserves.sol_lamports)
        / normalized_tokens(tokens_out, reserves.token_decimals);
    let post_price = ShadowV2Reserves::new(
        post_sol,
        post_token,
        reserves.token_decimals,
        reserves.sol_lamports,
    )
    .mark_price_sol_per_token()?;

    Ok(ShadowV2Quote {
        formula_version: SHADOW_V2_PRICE_FORMULA_VERSION.to_string(),
        pool_phase,
        side: ShadowV2QuoteSide::Buy,
        input_amount: sol_in_lamports,
        expected_output_amount: tokens_out,
        min_output_amount: apply_slippage_bps_floor(tokens_out, slippage_bps)?,
        fee_bps,
        fee_amount_lamports: fee_lamports,
        slippage_tolerance_bps: slippage_bps,
        mark_price_sol_per_token: mark,
        impact_price_sol_per_token: impact_price,
        fill_price_sol_per_token: fill_price,
        pool_price_after_sol_per_token: post_price,
        own_impact_bps: adverse_buy_impact_bps(mark, impact_price),
        post_sol_reserves_lamports: post_sol,
        post_token_reserves_raw: post_token,
    })
}

fn quote_sell_constant_product(
    pool_phase: ShadowV2PoolPhase,
    reserves: ShadowV2Reserves,
    token_in_raw: u64,
    fee_bps: u16,
    slippage_bps: u16,
) -> Result<ShadowV2Quote, ShadowV2PriceError> {
    let k = invariant(reserves)?;
    let post_token = reserves
        .token_reserves_raw
        .checked_add(token_in_raw)
        .ok_or(ShadowV2PriceError::ArithmeticOverflow)?;
    let post_sol = div_u128_to_u64(k, post_token as u128)?;
    let gross_sol_out = reserves.sol_reserves_lamports.saturating_sub(post_sol);
    if gross_sol_out == 0 {
        return Err(ShadowV2PriceError::OutputWouldBeZero);
    }
    let fee_lamports = fee_amount_floor(gross_sol_out, fee_bps)?;
    let net_sol_out = gross_sol_out.saturating_sub(fee_lamports);
    if net_sol_out == 0 {
        return Err(ShadowV2PriceError::OutputWouldBeZero);
    }

    let mark = reserves.mark_price_sol_per_token()?;
    let impact_price = normalized_sol(gross_sol_out, reserves.sol_lamports)
        / normalized_tokens(token_in_raw, reserves.token_decimals);
    let fill_price = normalized_sol(net_sol_out, reserves.sol_lamports)
        / normalized_tokens(token_in_raw, reserves.token_decimals);
    let post_price = ShadowV2Reserves::new(
        post_sol,
        post_token,
        reserves.token_decimals,
        reserves.sol_lamports,
    )
    .mark_price_sol_per_token()?;

    Ok(ShadowV2Quote {
        formula_version: SHADOW_V2_PRICE_FORMULA_VERSION.to_string(),
        pool_phase,
        side: ShadowV2QuoteSide::Sell,
        input_amount: token_in_raw,
        expected_output_amount: net_sol_out,
        min_output_amount: apply_slippage_bps_floor(net_sol_out, slippage_bps)?,
        fee_bps,
        fee_amount_lamports: fee_lamports,
        slippage_tolerance_bps: slippage_bps,
        mark_price_sol_per_token: mark,
        impact_price_sol_per_token: impact_price,
        fill_price_sol_per_token: fill_price,
        pool_price_after_sol_per_token: post_price,
        own_impact_bps: adverse_sell_impact_bps(mark, impact_price),
        post_sol_reserves_lamports: post_sol,
        post_token_reserves_raw: post_token,
    })
}

fn validate_bps(fee_bps: u16, slippage_bps: u16) -> Result<(), ShadowV2PriceError> {
    if fee_bps as u64 > SHADOW_V2_BPS_DENOMINATOR {
        return Err(ShadowV2PriceError::InvalidFeeBps { fee_bps });
    }
    if slippage_bps as u64 > SHADOW_V2_BPS_DENOMINATOR {
        return Err(ShadowV2PriceError::InvalidSlippageBps { slippage_bps });
    }
    Ok(())
}

fn invariant(reserves: ShadowV2Reserves) -> Result<u128, ShadowV2PriceError> {
    (reserves.sol_reserves_lamports as u128)
        .checked_mul(reserves.token_reserves_raw as u128)
        .ok_or(ShadowV2PriceError::ArithmeticOverflow)
}

fn div_u128_to_u64(numerator: u128, denominator: u128) -> Result<u64, ShadowV2PriceError> {
    if denominator == 0 {
        return Err(ShadowV2PriceError::ArithmeticOverflow);
    }
    let value = numerator / denominator;
    if value > u64::MAX as u128 {
        return Err(ShadowV2PriceError::ArithmeticOverflow);
    }
    Ok(value as u64)
}

fn fee_amount_floor(amount: u64, fee_bps: u16) -> Result<u64, ShadowV2PriceError> {
    if fee_bps as u64 > SHADOW_V2_BPS_DENOMINATOR {
        return Err(ShadowV2PriceError::InvalidFeeBps { fee_bps });
    }
    Ok(
        ((amount as u128).saturating_mul(fee_bps as u128) / SHADOW_V2_BPS_DENOMINATOR as u128)
            as u64,
    )
}

fn normalized_sol(lamports: u64, sol_lamports: u64) -> f64 {
    lamports as f64 / sol_lamports as f64
}

fn normalized_tokens(raw_tokens: u64, token_decimals: u8) -> f64 {
    raw_tokens as f64 / 10_f64.powi(token_decimals as i32)
}

fn adverse_buy_impact_bps(mark: f64, impact_price: f64) -> i32 {
    if mark <= 0.0 {
        return 0;
    }
    (((impact_price - mark) / mark) * SHADOW_V2_BPS_DENOMINATOR as f64)
        .max(0.0)
        .round() as i32
}

fn adverse_sell_impact_bps(mark: f64, impact_price: f64) -> i32 {
    if mark <= 0.0 {
        return 0;
    }
    (((mark - impact_price) / mark) * SHADOW_V2_BPS_DENOMINATOR as f64)
        .max(0.0)
        .round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reserves() -> ShadowV2Reserves {
        ShadowV2Reserves::new(30_000_000_000, 1_000_000_000_000, 6, SHADOW_V2_SOL_LAMPORTS)
    }

    #[test]
    fn shadow_v2_price_mark_price_normalizes_reserves_and_decimals() {
        let price = mark_price_sol_per_token(reserves()).unwrap();

        assert!((price - 0.00003).abs() < 0.000000000001);
    }

    #[test]
    fn shadow_v2_price_buy_quote_applies_fee_impact_and_min_out() {
        let quote = quote_constant_product(
            ShadowV2PoolPhase::BondingCurve,
            ShadowV2QuoteSide::Buy,
            reserves(),
            1_000_000_000,
            100,
            250,
        )
        .unwrap();

        let sol_after_fee = 990_000_000_u64;
        let k = 30_000_000_000_u128 * 1_000_000_000_000_u128;
        let post_sol = 30_000_000_000_u64 + sol_after_fee;
        let post_token = (k / post_sol as u128) as u64;
        let tokens_out = 1_000_000_000_000_u64 - post_token;

        assert_eq!(quote.formula_version, SHADOW_V2_PRICE_FORMULA_VERSION);
        assert_eq!(quote.pool_phase, ShadowV2PoolPhase::BondingCurve);
        assert_eq!(quote.side, ShadowV2QuoteSide::Buy);
        assert_eq!(quote.fee_amount_lamports, 10_000_000);
        assert_eq!(quote.expected_output_amount, tokens_out);
        assert_eq!(quote.min_output_amount, tokens_out * 9_750 / 10_000);
        assert_eq!(quote.post_sol_reserves_lamports, post_sol);
        assert_eq!(quote.post_token_reserves_raw, post_token);
        assert!(quote.fill_price_sol_per_token > quote.impact_price_sol_per_token);
        assert!(quote.impact_price_sol_per_token > quote.mark_price_sol_per_token);
        assert!(quote.own_impact_bps > 0);
    }

    #[test]
    fn shadow_v2_price_sell_quote_applies_output_fee_and_min_out() {
        let quote = quote_constant_product(
            ShadowV2PoolPhase::BondingCurve,
            ShadowV2QuoteSide::Sell,
            reserves(),
            10_000_000_000,
            100,
            100,
        )
        .unwrap();

        let k = 30_000_000_000_u128 * 1_000_000_000_000_u128;
        let post_token = 1_010_000_000_000_u64;
        let post_sol = (k / post_token as u128) as u64;
        let gross_sol_out = 30_000_000_000_u64 - post_sol;
        let net_sol_out = gross_sol_out - gross_sol_out / 100;

        assert_eq!(quote.expected_output_amount, net_sol_out);
        assert_eq!(quote.fee_amount_lamports, gross_sol_out / 100);
        assert_eq!(quote.min_output_amount, net_sol_out * 9_900 / 10_000);
        assert_eq!(quote.post_sol_reserves_lamports, post_sol);
        assert_eq!(quote.post_token_reserves_raw, post_token);
        assert!(quote.fill_price_sol_per_token < quote.impact_price_sol_per_token);
        assert!(quote.impact_price_sol_per_token < quote.mark_price_sol_per_token);
        assert!(quote.own_impact_bps > 0);
    }

    #[test]
    fn shadow_v2_price_amm_quote_uses_real_reserve_formula_label() {
        let quote = quote_constant_product(
            ShadowV2PoolPhase::Amm,
            ShadowV2QuoteSide::Buy,
            ShadowV2Reserves::new(10_000_000_000, 500_000_000_000, 6, SHADOW_V2_SOL_LAMPORTS),
            500_000_000,
            25,
            100,
        )
        .unwrap();

        assert_eq!(quote.pool_phase, ShadowV2PoolPhase::Amm);
        assert_eq!(
            quote.price_source_label(),
            "shadow_v2_price:constant_product_real_reserves"
        );
        assert!(quote.expected_output_amount > 0);
        assert!(quote.min_output_amount < quote.expected_output_amount);
    }

    #[test]
    fn shadow_v2_price_rejects_invalid_inputs() {
        assert_eq!(
            mark_price_sol_per_token(ShadowV2Reserves::new(0, 1, 6, SHADOW_V2_SOL_LAMPORTS))
                .unwrap_err(),
            ShadowV2PriceError::MissingOrZeroSolReserves
        );
        assert_eq!(
            mark_price_sol_per_token(ShadowV2Reserves::new(1, 0, 6, SHADOW_V2_SOL_LAMPORTS))
                .unwrap_err(),
            ShadowV2PriceError::MissingOrZeroTokenReserves
        );
        assert_eq!(
            mark_price_sol_per_token(ShadowV2Reserves::new(1, 1, 19, SHADOW_V2_SOL_LAMPORTS))
                .unwrap_err(),
            ShadowV2PriceError::UnsupportedTokenDecimals { token_decimals: 19 }
        );
        assert_eq!(
            quote_constant_product(
                ShadowV2PoolPhase::BondingCurve,
                ShadowV2QuoteSide::Buy,
                reserves(),
                0,
                100,
                100,
            )
            .unwrap_err(),
            ShadowV2PriceError::ZeroInputAmount
        );
        assert_eq!(
            quote_constant_product(
                ShadowV2PoolPhase::BondingCurve,
                ShadowV2QuoteSide::Buy,
                reserves(),
                1_000,
                10_001,
                100,
            )
            .unwrap_err(),
            ShadowV2PriceError::InvalidFeeBps { fee_bps: 10_001 }
        );
        assert_eq!(
            apply_slippage_bps_floor(1_000, 10_001).unwrap_err(),
            ShadowV2PriceError::InvalidSlippageBps {
                slippage_bps: 10_001
            }
        );
    }
}
