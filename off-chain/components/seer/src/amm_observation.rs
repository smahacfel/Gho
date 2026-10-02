//! Read-only PumpSwap price evidence for Gate 0. Existing execution/compatibility
//! reserve fields are untouched. Post reserves come from transaction token
//! balances, not Pool account bytes or a slippage argument.
use crate::{
    binary_parser::{DISC_SWAP_EVENT_BUY, DISC_SWAP_EVENT_SELL, DISC_SWAP_OUTER_WRAPPER},
    types::{GeyserEvent, RawTokenBalance, TradeEvent},
};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

const AMM: Pubkey = solana_sdk::pubkey!("pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA");
const WSOL: &str = "So11111111111111111111111111111111111111112";
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AmmObservation {
    pub pool: Pubkey,
    pub base_reserves: u64,
    pub quote_reserves: u64,
    pub pre_base: Option<u64>,
    pub pre_quote: Option<u64>,
    pub virtual_quote: i128,
    pub base_supply: Option<u64>,
    pub initialization: bool,
}
fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(at..at.checked_add(8)?)?.try_into().ok()?,
    ))
}
fn owned_balance(balances: &[RawTokenBalance], owner: &str, mint: &str) -> Option<u64> {
    let mut found = balances
        .iter()
        .filter(|b| b.owner.as_deref() == Some(owner) && b.mint == mint);
    let amount = found.next()?.amount;
    found.next().is_none().then_some(amount)
}
/// Parse only fields needed for price/supply from the published additive event
/// layout. Short/ambiguous payloads do not acquire a guessed supply or quote.
fn tail(payload: &[u8], buy: bool) -> Option<(i128, u64)> {
    let quote_at = if buy {
        if *payload.get(352)? > 1 {
            return None;
        }
        let len = u32::from_le_bytes(payload.get(393..397)?.try_into().ok()?) as usize;
        if len > 128 {
            return None;
        }
        std::str::from_utf8(payload.get(397..397usize.checked_add(len)?)?).ok()?;
        429usize.checked_add(len)?
    } else {
        384
    };
    let virtual_quote = i128::from_le_bytes(
        payload
            .get(quote_at..quote_at.checked_add(16)?)?
            .try_into()
            .ok()?,
    );
    if *payload.get(quote_at + 16)? > 1 {
        return None;
    }
    let supply = u64_at(payload, quote_at + 17)?;
    (supply > 0).then_some((virtual_quote, supply))
}
#[derive(Debug)]
struct MarketEvent {
    outer: u32,
    path: Option<Vec<u16>>,
    buy: bool,
    user: Pubkey,
    amount: u64,
    state: AmmObservation,
}

pub(crate) fn from_transaction(event: &GeyserEvent, trade: &TradeEvent) -> Option<AmmObservation> {
    if !trade.is_pumpswap
        || !trade.success
        || !trade.metadata_availability.status_known
        || !trade.metadata_availability.inner_instructions_known
    {
        return None;
    }
    let GeyserEvent::Transaction {
        accounts,
        inner_instructions,
        post_token_balances,
        ..
    } = event
    else {
        return None;
    };
    if trade.is_dev_buy {
        return initial_pool_state(event, trade);
    }
    let pool = trade.pool_amm_id.to_string();
    let mint = trade.mint.to_string();
    let base = owned_balance(post_token_balances, &pool, &mint)?;
    let quote = owned_balance(post_token_balances, &pool, WSOL)?;
    let mut market_events = Vec::new();
    for group in inner_instructions {
        let mut path = smallvec::SmallVec::new();
        let mut siblings = smallvec::SmallVec::new();
        for ix in &group.instructions {
            let inner_path = crate::binary_parser::next_inner_instruction_path(
                ix.stack_height,
                &mut path,
                &mut siblings,
            );
            if accounts.get(ix.program_id_index as usize) != Some(&AMM) {
                continue;
            }
            let Some(data) = ix.data.strip_prefix(&DISC_SWAP_OUTER_WRAPPER) else {
                continue;
            };
            let buy = if data.starts_with(&DISC_SWAP_EVENT_BUY) {
                true
            } else if data.starts_with(&DISC_SWAP_EVENT_SELL) {
                false
            } else {
                continue;
            };
            let payload = data.get(8..)?;
            if payload.get(112..144) != Some(trade.pool_amm_id.as_ref()) {
                continue;
            }
            let amount = u64_at(payload, 8)?;
            let pre_base = u64_at(payload, 40)?;
            let pre_quote = u64_at(payload, 48)?;
            // Nazwane kwoty z eventu obejmują opłatę LP zatrzymywaną w puli.
            // Walidujemy cały łańcuch oraz stan końcowy względem raw vault balances.
            let quote_flow = u64_at(payload, 96)?;
            let (post_base, post_quote) = if buy {
                (
                    pre_base.checked_sub(amount)?,
                    pre_quote.checked_add(quote_flow)?,
                )
            } else {
                (
                    pre_base.checked_add(amount)?,
                    pre_quote.checked_sub(quote_flow)?,
                )
            };
            let (virtual_quote, supply) = tail(payload, buy)?;
            market_events.push(MarketEvent {
                outer: group.index,
                path: inner_path,
                buy,
                user: Pubkey::new_from_array(payload.get(144..176)?.try_into().ok()?),
                amount,
                state: AmmObservation {
                    pool: trade.pool_amm_id,
                    base_reserves: post_base,
                    quote_reserves: post_quote,
                    pre_base: Some(pre_base),
                    pre_quote: Some(pre_quote),
                    virtual_quote,
                    base_supply: Some(supply),
                    initialization: false,
                },
            });
        }
    }
    // Sortowanie tylko po indeksie instrukcji z metadanych Solany; kolejność
    // wewnątrz grupy zachowuje oryginalny traversal. Nie używamy czasu receipt.
    market_events.sort_by_key(|event| event.outer);
    for pair in market_events.windows(2) {
        if Some(pair[0].state.base_reserves) != pair[1].state.pre_base
            || Some(pair[0].state.quote_reserves) != pair[1].state.pre_quote
        {
            return None;
        }
    }
    let final_state = &market_events.last()?.state;
    if final_state.base_reserves != base || final_state.quote_reserves != quote {
        return None;
    }
    let mut matches = market_events.iter().filter(|event| {
        event.buy == trade.is_buy
            && event.amount == trade.amount
            && event.user == trade.signer
            && trade.provenance.as_ref().is_none_or(|p| {
                p.outer_instruction_index
                    .is_none_or(|outer| outer == event.outer)
                    && (!p.from_cpi
                        || p.inner_instruction_path
                            .as_ref()
                            .is_none_or(|path| Some(path) == event.path.as_ref()))
            })
    });
    let state = matches.next()?.state.clone();
    matches.next().is_none().then_some(state)
}

fn initial_pool_state(event: &GeyserEvent, trade: &TradeEvent) -> Option<AmmObservation> {
    let GeyserEvent::Transaction {
        accounts,
        inner_instructions,
        ..
    } = event
    else {
        return None;
    };
    let discriminator = solana_sdk::hash::hash(b"event:CreatePoolEvent").to_bytes();
    let mut result = None;
    for ix in inner_instructions
        .iter()
        .flat_map(|group| &group.instructions)
    {
        if accounts.get(ix.program_id_index as usize) != Some(&AMM) {
            continue;
        }
        let Some(data) = ix.data.strip_prefix(&DISC_SWAP_OUTER_WRAPPER) else {
            continue;
        };
        let Some(payload) = data.strip_prefix(&discriminator[..8]) else {
            continue;
        };
        if payload.get(165..197) != Some(trade.pool_amm_id.as_ref())
            || payload.get(42..74) != Some(trade.mint.as_ref())
        {
            continue;
        }
        if payload.get(74..106)
            != Some(solana_sdk::pubkey!("So11111111111111111111111111111111111111112").as_ref())
            || payload.get(106..108) != Some(&[6, 9])
            || result.is_some()
        {
            return None;
        }
        result = Some(AmmObservation {
            pool: trade.pool_amm_id,
            base_reserves: u64_at(payload, 124)?,
            quote_reserves: u64_at(payload, 132)?,
            pre_base: None,
            pre_quote: None,
            virtual_quote: 0,
            base_supply: None,
            initialization: true,
        });
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn buy_tail_uses_checked_string_length_and_exact_supply_offset() {
        let mut p = vec![0; 457];
        p[393..397].copy_from_slice(&3u32.to_le_bytes());
        p[397..400].copy_from_slice(b"buy");
        p[432..448].copy_from_slice(&19i128.to_le_bytes());
        p[449..457].copy_from_slice(&999u64.to_le_bytes());
        assert_eq!(tail(&p, true), Some((19, 999)));
        assert_eq!(tail(&p[..456], true), None);
        p[393..397].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(tail(&p, true), None);
    }

    #[test]
    fn duplicate_or_wrong_owner_vault_balance_is_not_a_state() {
        let balance = RawTokenBalance {
            account_index: 1,
            mint: "mint".into(),
            owner: Some("pool".into()),
            amount: 99,
        };
        assert_eq!(
            owned_balance(std::slice::from_ref(&balance), "pool", "mint"),
            Some(99)
        );
        assert_eq!(
            owned_balance(std::slice::from_ref(&balance), "other", "mint"),
            None
        );
        assert_eq!(
            owned_balance(&[balance.clone(), balance], "pool", "mint"),
            None
        );
    }

    #[test]
    fn short_tail_is_not_zero_supply_and_virtual_quote_is_signed() {
        assert_eq!(tail(&[0; 176], false), None);
        let mut p = vec![0; 409];
        p[384..400].copy_from_slice(&(-17i128).to_le_bytes());
        p[401..409].copy_from_slice(&900u64.to_le_bytes());
        assert_eq!(tail(&p, false), Some((-17, 900)));
        p[400] = 2;
        assert_eq!(tail(&p, false), None);
    }
}
