use parking_lot::Mutex as ParkingMutex;
/// pump_parser.rs
///
/// Ghost domain parser — Pump.fun + PumpSwap instruction and account decoder.
/// Responsibilities:
///   • Decode Create/Buy/Sell/Migrate/Withdraw/Initialize/SetParams (top-level + CPI)
///   • Walk inner_instructions for CPI calls — migrate is here 70-90% of the time
///   • Decode Anchor CPI event logs (EventTrade/Create/Complete) — highest fidelity
///   • Maintain curve_pubkey → mint_pubkey registry (CurveMintRegistry)
///     required because BondingCurveState account data does NOT contain the mint
///   • Emit MigrateReady when complete=true detected in account update
///   • Enrich trades with post-tx curve state (SOL delta, reserves, mcap, progress)
///   • Route account updates → ShadowLedger (truth-first: curve state, not trades)
///   • Classify backfill replays identically to live events (parser-first policy)
///   • Resolve queue for account updates that arrive before curve→mint mapping exists
use std::{collections::VecDeque, sync::Arc, time::Instant};

use borsh::BorshDeserialize;
use dashmap::DashMap;
use prost::Message as _;
use smallvec::SmallVec;
use tracing::{debug, info, trace, warn};

use crate::grpc_connection::{
    AccountRegistry, PumpEvent, PUMP_FUN_PROGRAM_ID, PUMP_SWAP_PROGRAM_ID,
};
use crate::types::{record_trade_outcome_metric, InstructionProvenance, TradeOutcome};

// ─── Discriminators ───────────────────────────────────────────────────────────

pub const DISC_INITIALIZE: [u8; 8] = [0xaf, 0xaf, 0x6d, 0x1f, 0x0d, 0x98, 0x9b, 0xed];
// sha256("global:set_params")[..8]
pub const DISC_SET_PARAMS: [u8; 8] = [0x1b, 0xea, 0xb2, 0x34, 0x93, 0x02, 0xbb, 0x8d];
/// SHA256("global:create")[..8] — this is the *theoretical* Anchor discriminator but
/// Pump.fun does NOT use it on-chain. Kept for completeness; real on-chain disc is below.
pub const DISC_CREATE_ANCHOR: [u8; 8] = [0x18, 0x1e, 0xc8, 0x28, 0x05, 0x1c, 0x07, 0x77];
/// Actual Pump.fun on-chain CREATE discriminator (custom, not SHA256("global:create")).
/// Observed value: d6 90 4c ec 5f 8b 31 b4 — confirmed in init_pool_parser.rs + pumpfun_collector.rs.
pub const DISC_CREATE: [u8; 8] = [0xd6, 0x90, 0x4c, 0xec, 0x5f, 0x8b, 0x31, 0xb4];
pub const DISC_BUY: [u8; 8] = [0x66, 0x06, 0x3d, 0x12, 0x01, 0xda, 0xeb, 0xea];
pub const DISC_SELL: [u8; 8] = [0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad];
pub const DISC_WITHDRAW: [u8; 8] = [0xb7, 0x12, 0x46, 0x9c, 0x94, 0x6d, 0xa1, 0x22];
// sha256("global:migrate")[..8]
pub const DISC_MIGRATE: [u8; 8] = [0x9b, 0xea, 0xe7, 0x92, 0xec, 0x9e, 0xa2, 0x1e];
// PumpSwap buy/sell share the same disc as pump.fun (both sha256("global:buy/sell")[..8]).
// Routing is done by program_id inside the DISC_BUY/DISC_SELL match arms.
pub const DISC_SWAP_BUY: [u8; 8] = DISC_BUY;
pub const DISC_SWAP_SELL: [u8; 8] = DISC_SELL;
// sha256("global:create_pool")[..8]
pub const DISC_SWAP_CREATE_POOL: [u8; 8] = [0xe9, 0x92, 0xd1, 0x8e, 0xcf, 0x68, 0x40, 0xbc];
pub const DISC_BONDING_CURVE: [u8; 8] = [0x17, 0xb7, 0xf8, 0x37, 0x60, 0xd8, 0xac, 0x60];
pub const DISC_GLOBAL_STATE: [u8; 8] = [0x8a, 0xc4, 0x40, 0x08, 0xf2, 0x33, 0x0e, 0xfb];
pub const DISC_AMM_POOL: [u8; 8] = [0xf1, 0x9a, 0x6d, 0x28, 0x1c, 0x37, 0xe4, 0x55];
// sha256("event:TradeEvent")[..8]
pub const DISC_EVENT_TRADE: [u8; 8] = [0xbd, 0xdb, 0x7f, 0xd3, 0x4e, 0xe6, 0x61, 0xee];
// sha256("event:CreateEvent")[..8]
pub const DISC_EVENT_CREATE: [u8; 8] = [0x1b, 0x72, 0xa9, 0x4d, 0xde, 0xeb, 0x63, 0x76];
// sha256("event:CompleteEvent")[..8]
pub const DISC_EVENT_COMPLETE: [u8; 8] = [0x5f, 0x72, 0x61, 0x9c, 0xd4, 0x2e, 0x98, 0x08];

// ─── PumpSwap Anchor event log discriminators ──────────────────────────────
//
// PumpSwap uses a DOUBLE-DISCRIMINATOR pattern for Anchor CPI event logs.
// inner_ix.data layout:
//   [0..8]  = DISC_SWAP_OUTER_WRAPPER  (Anchor write_event CPI wrapper)
//   [8..16] = inner event disc (BuyEvent or SellEvent)
//   [16..]  = actual event payload (Borsh)
//
// try_decode_cpi_event strips data[0..8] as disc, leaving
//   payload = [inner_disc(8) + event_fields]
// For PumpSwap we re-read inner disc from payload[0..8].
// Confirmed from on-chain log hex dump: payload_len=424, first 8 bytes = BuyEvent disc.
pub const DISC_SWAP_OUTER_WRAPPER: [u8; 8] = [0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];
// sha256("event:BuyEvent")[..8]
pub const DISC_SWAP_EVENT_BUY: [u8; 8] = [0x67, 0xf4, 0x52, 0x1f, 0x2c, 0xf5, 0x77, 0x77];
// sha256("event:SellEvent")[..8]
pub const DISC_SWAP_EVENT_SELL: [u8; 8] = [0x3e, 0x2f, 0x37, 0x0a, 0xa5, 0x03, 0xdc, 0x2a];
// sha256("global:buy_exact_quote_in")[..8]
pub const DISC_SWAP_BUY_EXACT_QUOTE_IN: [u8; 8] = [0xc6, 0x2e, 0x15, 0x52, 0xb4, 0xd9, 0xe8, 0x70];
// Observed on-chain on routed pump.fun buys carried by aggregator / terminal flows.
// Payload matches TradeParams and account layout matches the canonical pump.fun buy.
pub const DISC_PUMP_BUY_ROUTED: [u8; 8] = [0x38, 0xfc, 0x74, 0x08, 0x9e, 0xdf, 0xcd, 0x5f];

// Supported external router programs carrying pump.fun / PumpSwap swaps.
const JUPITER_V6_PROGRAM_ID: &str = "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4";
const DFLOW_V4_PROGRAM_ID: &str = "DF1ow4tspfHX9JwWJsAb9epbkA8hmpSEAtxXy1V27QBH";

// sha256("global:routeV2")[..8]
pub const DISC_JUPITER_ROUTE_V2: [u8; 8] = [0x1e, 0x92, 0x8f, 0x3c, 0x22, 0x7a, 0x2c, 0x3c];
// sha256("global:route")[..8]
pub const DISC_JUPITER_ROUTE: [u8; 8] = [0xe5, 0x17, 0xcb, 0x97, 0x7a, 0xe3, 0xad, 0x2a];
// sha256("global:swap2")[..8]
pub const DISC_DFLOW_SWAP2: [u8; 8] = [0x41, 0x4b, 0x3f, 0x4c, 0xeb, 0x5b, 0x5b, 0x88];
// sha256("global:swap")[..8]
pub const DISC_DFLOW_SWAP: [u8; 8] = [0xf8, 0xc6, 0x9e, 0x91, 0xe1, 0x75, 0x87, 0xc8];
// sha256("global:swap2WithDestination")[..8]
pub const DISC_DFLOW_SWAP2_WITH_DESTINATION: [u8; 8] =
    [0x01, 0x08, 0x0f, 0x67, 0x2f, 0xd4, 0x47, 0xc6];
// sha256("global:swap2WithDestinationNative")[..8]
pub const DISC_DFLOW_SWAP2_WITH_DESTINATION_NATIVE: [u8; 8] =
    [0x1d, 0xce, 0x28, 0xef, 0xb1, 0xf7, 0xa7, 0xaa];

// ─── IDL account index constants ──────────────────────────────────────────────

// Pump.fun BUY / SELL instruction layout:
//   0=Global, 1=Fee, 2=Mint, 3=BondingCurve, 4=AssocBondingCurve, 5=AssocUser, 6=User/Signer
const PUMP_IDX_MINT: usize = 2;
const PUMP_IDX_BONDING_CURVE: usize = 3;
const PUMP_IDX_USER: usize = 6;
const PUMP_IDX_GLOBAL_CONFIG: usize = 0;
const PUMP_IDX_FEE_RECIPIENT: usize = 1;
const PUMP_IDX_ASSOCIATED_BONDING_CURVE: usize = 4;
const PUMP_IDX_TOKEN_PROGRAM: usize = 8;
const PUMP_IDX_BONDING_CURVE_V2: usize = 16;

// Pump.fun CREATE instruction layout (different from Buy/Sell!):
//   0=Mint, 1=MintAuthority, 2=BondingCurve, 3=AssocBondingCurve, 4=Global,
//   5=MPLTokenMetadata, 6=MetadataAccount, 7=Creator/Signer, ...
const CREATE_IDX_MINT: usize = 0;
const CREATE_IDX_BONDING_CURVE: usize = 2;
const CREATE_IDX_USER: usize = 7;
const MIG_IDX_MINT: usize = 1;
const MIG_IDX_BONDING_CURVE: usize = 2;
const MIG_IDX_POOL: usize = 7;
const MIG_IDX_USER: usize = 12;
const SWAP_IDX_POOL: usize = 0;
const SWAP_IDX_USER: usize = 1;
// PumpSwap CreatePool does not share the exact trader/signer layout of buy/sell.
// On observed CreatePool transactions the funding signer sits at account index 2,
// while account index 1 may be a non-wallet metadata/config account.
const SWAP_CREATE_IDX_SIGNER: usize = 2;
const SWAP_IDX_BASE_MINT: usize = 3;
const SWAP_IDX_QUOTE_MINT: usize = 4;
const SWAP_IDX_POOL_BASE: usize = 7;
const SWAP_IDX_POOL_QUOTE: usize = 8;

const JUPITER_ROUTE_V2_IDX_SIGNER: usize = 0;
const JUPITER_ROUTE_V2_IDX_SOURCE_MINT: usize = 3;
const JUPITER_ROUTE_V2_IDX_DESTINATION_MINT: usize = 4;

const JUPITER_ROUTE_IDX_SIGNER: usize = 1;
const JUPITER_ROUTE_IDX_DESTINATION_MINT: usize = 5;

const DFLOW_SWAP_IDX_SIGNER: usize = 3;
const DFLOW_SWAP_WITH_DESTINATION_IDX_SIGNER: usize = 3;
const DFLOW_SWAP_WITH_DESTINATION_IDX_DESTINATION_MINT: usize = 6;
const DFLOW_SWAP_WITH_DESTINATION_NATIVE_IDX_SIGNER: usize = 3;

/// Wrapped-SOL mint address — canonical quote side of all PumpSwap pools.
const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";

// ─── Instruction params ───────────────────────────────────────────────────────

#[derive(Debug, Clone, BorshDeserialize)]
pub struct CreateParams {
    pub name: String,
    pub symbol: String,
    pub uri: String,
}

#[derive(Debug, Clone, BorshDeserialize)]
pub struct TradeParams {
    pub amount: u64,
    pub sol_bound: u64,
}

/// Lenient borsh deserializer.
///
/// `borsh::try_from_slice` returns "Not all bytes read" if payload is longer
/// than the struct — this happens for pump.fun buy/sell because:
///   • Some aggregators append routing metadata after the canonical args
///   • PumpSwap buy/sell share the same discriminator but have a different
///     (larger) payload — we consume only what our struct needs
///   • Newer pump.fun IDL revisions may add trailing optional fields
///
/// `deserialize_reader` stops when the struct is fully read and ignores any tail.
#[inline(always)]
fn borsh_read<T: borsh::BorshDeserialize>(payload: &[u8]) -> Option<T> {
    T::deserialize_reader(&mut std::io::Cursor::new(payload)).ok()
}

#[inline(always)]
fn borsh_read_with_len<T: borsh::BorshDeserialize>(payload: &[u8]) -> Option<(T, usize)> {
    let mut cursor = std::io::Cursor::new(payload);
    let value = T::deserialize_reader(&mut cursor).ok()?;
    let consumed = usize::try_from(cursor.position()).ok()?;
    Some((value, consumed))
}

#[inline(always)]
fn log_drop_oob_accounts(
    disc: &str,
    program: &str,
    slot: u64,
    from_cpi: bool,
    signature: Option<&str>,
    required: usize,
    got: usize,
) {
    debug!(
        reason = "OOB_ACCOUNTS",
        disc,
        program = &program[..program.len().min(8)],
        slot,
        from_cpi,
        sig = signature.unwrap_or("-"),
        required,
        got,
        "IX_DROPPED"
    );
    ::metrics::increment_counter!("seer_parser_ix_dropped_oob_accounts_total");
}

#[inline(always)]
fn log_drop_role_mismatch(
    disc: &str,
    program: &str,
    slot: u64,
    from_cpi: bool,
    signature: Option<&str>,
    detail: &str,
) {
    debug!(
        reason = "ROLE_MISMATCH",
        disc,
        program = &program[..program.len().min(8)],
        slot,
        from_cpi,
        sig = signature.unwrap_or("-"),
        detail,
        "IX_DROPPED"
    );
    ::metrics::increment_counter!("seer_parser_ix_dropped_role_mismatch_total");
}

#[inline(always)]
fn has_min_accounts(
    accounts: &SmallVec<[String; 14]>,
    min_required: usize,
    disc: &str,
    program: &str,
    slot: u64,
    from_cpi: bool,
    signature: Option<&str>,
) -> bool {
    if accounts.len() >= min_required {
        return true;
    }
    log_drop_oob_accounts(
        disc,
        program,
        slot,
        from_cpi,
        signature,
        min_required,
        accounts.len(),
    );
    false
}

#[inline(always)]
fn is_valid_curve_role(mint: &str, bonding_curve: &str) -> bool {
    !mint.is_empty()
        && !bonding_curve.is_empty()
        && mint != bonding_curve
        && mint != WSOL_MINT
        && bonding_curve != WSOL_MINT
}

#[inline(always)]
fn decode_anchor_event_kind_with_len(
    disc: [u8; 8],
    payload: &[u8],
) -> Option<(ParsedEventKind, usize)> {
    match disc {
        DISC_EVENT_TRADE => borsh_read_with_len::<EventTrade>(payload)
            .map(|(event, consumed_len)| (ParsedEventKind::CpiTrade(event), consumed_len)),
        DISC_EVENT_CREATE => borsh_read_with_len::<EventCreate>(payload)
            .map(|(event, consumed_len)| (ParsedEventKind::CpiCreate(event), consumed_len)),
        DISC_EVENT_COMPLETE => borsh_read_with_len::<EventComplete>(payload)
            .map(|(event, consumed_len)| (ParsedEventKind::CpiComplete(event), consumed_len)),
        DISC_SWAP_EVENT_BUY => decode_swap_buy_event_with_len(payload)
            .map(|(event, consumed_len)| (ParsedEventKind::CpiSwapBuy(event), consumed_len)),
        DISC_SWAP_EVENT_SELL => decode_swap_sell_event_with_len(payload)
            .map(|(event, consumed_len)| (ParsedEventKind::CpiSwapSell(event), consumed_len)),
        _ => None,
    }
}

#[inline(always)]
fn read_u64_le(payload: &[u8], offset: &mut usize) -> Option<u64> {
    let end = offset.checked_add(8)?;
    let bytes: [u8; 8] = payload.get(*offset..end)?.try_into().ok()?;
    *offset = end;
    Some(u64::from_le_bytes(bytes))
}

#[inline(always)]
fn read_i64_le(payload: &[u8], offset: &mut usize) -> Option<i64> {
    let end = offset.checked_add(8)?;
    let bytes: [u8; 8] = payload.get(*offset..end)?.try_into().ok()?;
    *offset = end;
    Some(i64::from_le_bytes(bytes))
}

#[inline(always)]
fn read_pubkey_bytes(payload: &[u8], offset: &mut usize) -> Option<[u8; 32]> {
    let end = offset.checked_add(32)?;
    let bytes: [u8; 32] = payload.get(*offset..end)?.try_into().ok()?;
    *offset = end;
    Some(bytes)
}

fn decode_swap_buy_event_with_len(payload: &[u8]) -> Option<(SwapBuyEvent, usize)> {
    let mut offset = 0usize;
    let event = SwapBuyEvent {
        timestamp: read_i64_le(payload, &mut offset)?,
        base_amount_out: read_u64_le(payload, &mut offset)?,
        max_quote_amount_in: read_u64_le(payload, &mut offset)?,
        user_base_token_reserves: read_u64_le(payload, &mut offset)?,
        user_quote_token_reserves: read_u64_le(payload, &mut offset)?,
        pool_base_token_reserves: read_u64_le(payload, &mut offset)?,
        pool_quote_token_reserves: read_u64_le(payload, &mut offset)?,
        quote_amount_in: read_u64_le(payload, &mut offset)?,
        lp_fee_basis_points: read_u64_le(payload, &mut offset)?,
        lp_fee_amount: read_u64_le(payload, &mut offset)?,
        protocol_fee_basis_points: read_u64_le(payload, &mut offset)?,
        protocol_fee_amount: read_u64_le(payload, &mut offset)?,
        quote_amount_in_with_lp_fee: read_u64_le(payload, &mut offset)?,
        user_quote_amount_in: read_u64_le(payload, &mut offset)?,
        pool: read_pubkey_bytes(payload, &mut offset)?,
        user: read_pubkey_bytes(payload, &mut offset)?,
    };
    Some((event, offset))
}

fn decode_swap_sell_event_with_len(payload: &[u8]) -> Option<(SwapSellEvent, usize)> {
    let mut offset = 0usize;
    let event = SwapSellEvent {
        timestamp: read_i64_le(payload, &mut offset)?,
        base_amount_in: read_u64_le(payload, &mut offset)?,
        min_quote_amount_out: read_u64_le(payload, &mut offset)?,
        user_base_token_reserves: read_u64_le(payload, &mut offset)?,
        user_quote_token_reserves: read_u64_le(payload, &mut offset)?,
        pool_base_token_reserves: read_u64_le(payload, &mut offset)?,
        pool_quote_token_reserves: read_u64_le(payload, &mut offset)?,
        quote_amount_out: read_u64_le(payload, &mut offset)?,
        lp_fee_basis_points: read_u64_le(payload, &mut offset)?,
        lp_fee_amount: read_u64_le(payload, &mut offset)?,
        protocol_fee_basis_points: read_u64_le(payload, &mut offset)?,
        protocol_fee_amount: read_u64_le(payload, &mut offset)?,
        quote_amount_out_without_lp_fee: read_u64_le(payload, &mut offset)?,
        user_quote_amount_out: read_u64_le(payload, &mut offset)?,
        pool: read_pubkey_bytes(payload, &mut offset)?,
        user: read_pubkey_bytes(payload, &mut offset)?,
    };
    Some((event, offset))
}

/// Normalize a PumpSwap pair so that base = token mint, quote = WSOL.
///
/// Returns `(base_mint, quote_mint, swapped)` where `swapped` is `true` when
/// the on-chain base was actually WSOL and we flipped the pair.  Callers MUST
/// also swap the corresponding amounts (base_amount ↔ quote_amount) when
/// `swapped == true`, because account-index-derived deltas still follow the
/// original on-chain layout.
fn normalize_swap_pair(raw_base: String, raw_quote: String) -> Option<(String, String, bool)> {
    if raw_base.is_empty() || raw_quote.is_empty() || raw_base == raw_quote {
        return None;
    }

    // Exactly one side must be WSOL (the quote/SOL side).
    // Pairs where neither side is WSOL are not Pump.fun AMM trades.
    if raw_base == WSOL_MINT {
        // base was WSOL — swap: base=non-WSOL token, quote=WSOL
        Some((raw_quote, raw_base, true))
    } else if raw_quote == WSOL_MINT {
        Some((raw_base, raw_quote, false))
    } else {
        None
    }
}

/// Returns `true` when the PumpSwap pool identified by `pool` has WSOL as its
/// on-chain *base* mint.  In that case the CPI event fields `base_*` carry
/// SOL/lamport values and `quote_*` carry token raw units — the opposite of
/// the normal (token-base, WSOL-quote) layout.
///
/// Checks both top-level `instructions` and every `inner_instructions` group
/// so that routed / aggregator flows (where the PumpSwap ix is a CPI) are
/// handled correctly.
fn pumpswap_pool_wsol_is_base(event: &GeyserEvent, pool: &Pubkey) -> bool {
    let GeyserEvent::Transaction {
        ref accounts,
        ref instructions,
        ref inner_instructions,
        ..
    } = event
    else {
        return false;
    };
    let all_keys: Vec<String> = accounts.iter().map(|p| p.to_string()).collect();
    let pool_str = pool.to_string();

    // 1. Top-level instructions (RawInstruction has a direct program_id Pubkey).
    for ix in instructions {
        if ix.program_id.to_string() != PUMP_SWAP_PROGRAM_ID {
            continue;
        }
        let ix_accounts = resolve_accounts(&ix.account_indices, &all_keys);
        if acs(&ix_accounts, SWAP_IDX_POOL) != pool_str {
            continue;
        }
        return acs(&ix_accounts, SWAP_IDX_BASE_MINT) == WSOL_MINT;
    }

    // 2. Inner instructions / CPI (InnerIx uses program_id_index into accounts).
    for group in inner_instructions {
        for ix in &group.instructions {
            let prog = key_at(&all_keys, ix.program_id_index as usize);
            if prog != PUMP_SWAP_PROGRAM_ID {
                continue;
            }
            let ix_accounts = resolve_accounts(&ix.accounts, &all_keys);
            if acs(&ix_accounts, SWAP_IDX_POOL) != pool_str {
                continue;
            }
            return acs(&ix_accounts, SWAP_IDX_BASE_MINT) == WSOL_MINT;
        }
    }

    false
}

fn sanitize_creator_pubkey(creator: Pubkey) -> Pubkey {
    if is_candidate_owner(&creator) {
        creator
    } else {
        warn!(
            "INIT_POOL_CREATOR_INVALID creator={} reason=non_wallet_or_program",
            creator
        );
        Pubkey::default()
    }
}

fn has_matching_pumpfun_cpi(
    events: &[ParsedPumpEvent],
    side: TradeSide,
    mint: &Pubkey,
    bonding_curve: &Pubkey,
    cm_reg: &CurveMintRegistry,
) -> bool {
    events.iter().any(|event| {
        let ParsedEventKind::CpiTrade(cpi_trade) = &event.kind else {
            return false;
        };
        if cpi_trade.is_buy != matches!(side, TradeSide::Buy) {
            return false;
        }

        let cpi_mint = Pubkey::new_from_array(cpi_trade.mint);
        if cpi_mint != *mint {
            return false;
        }

        match cm_reg.curve_for_mint_pk(mint) {
            Some(curve) => curve == *bonding_curve,
            None => true,
        }
    })
}

fn has_matching_pumpswap_cpi(
    events: &[ParsedPumpEvent],
    side: TradeSide,
    pool: &Pubkey,
    base_mint: &Pubkey,
    cm_reg: &CurveMintRegistry,
) -> PumpSwapStructuralMatchKind {
    let target_side_is_buy = matches!(side, TradeSide::Buy);
    let mut seen_pool_side = false;

    for event in events {
        let (cpi_side_is_buy, cpi_pool) = match &event.kind {
            ParsedEventKind::CpiSwapBuy(cpi) => (true, Pubkey::new_from_array(cpi.pool)),
            ParsedEventKind::CpiSwapSell(cpi) => (false, Pubkey::new_from_array(cpi.pool)),
            _ => continue,
        };
        if cpi_side_is_buy != target_side_is_buy || cpi_pool != *pool {
            continue;
        }

        seen_pool_side = true;
        // PumpSwap registers pool->mint in CurveMintRegistry by using pool_amm_id as
        // the "curve" key, so `mint_for_curve_pk(pool)` is the correct lookup here.
        match cm_reg.mint_for_curve_pk(pool) {
            Some(mapped_mint) => {
                if mapped_mint == *base_mint {
                    return PumpSwapStructuralMatchKind::MatchingCpiSwap;
                }
            }
            None => return PumpSwapStructuralMatchKind::CpiHasUnresolvedMint,
        }
    }

    if seen_pool_side && cm_reg.mint_for_curve_pk(pool).is_none() {
        PumpSwapStructuralMatchKind::CpiHasUnresolvedMint
    } else {
        PumpSwapStructuralMatchKind::NoMatchingCpiSwap
    }
}

const TRADE_EVENT_DEDUP_STAGE_TOTAL: &str = "ghost.parser.trade_event_dedup_stage_total";
const TRADE_EVENT_DEDUP_TOTAL: &str = "ghost.parser.trade_event_dedup_total";
const TRADE_CANDIDATE_DEDUP_STAGE_TOTAL: &str = "ghost.parser.trade_candidate_dedup_stage_total";
const TRADE_CANDIDATE_DEDUP_TOTAL: &str = "ghost.parser.trade_candidate_dedup_total";
const TRADE_CANDIDATE_ORDINAL_TOTAL: &str = "ghost.parser.trade_candidate_ordinal_total";
const ORPHAN_INNER_GROUP_TOTAL: &str = "ghost.parser.orphan_inner_group_total";
const MISSING_PROVENANCE_TOTAL: &str = "ghost.parser.missing_provenance_total";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PumpSwapStructuralMatchKind {
    MatchingCpiSwap,
    CpiHasUnresolvedMint,
    NoMatchingCpiSwap,
}

impl PumpSwapStructuralMatchKind {
    const fn reason_code(self) -> &'static str {
        match self {
            Self::MatchingCpiSwap => "drop_ix_swap_because_matching_cpi_swap",
            Self::CpiHasUnresolvedMint => "keep_ix_swap_because_cpi_has_unresolved_mint",
            Self::NoMatchingCpiSwap => "keep_ix_swap_because_no_matching_cpi_swap",
        }
    }

    const fn should_drop(self) -> bool {
        matches!(self, Self::MatchingCpiSwap)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TradeCandidateMatchKind {
    None,
    ExactMatch,
    WeakDuplicate,
    OrdinalMismatchExactMatch,
    OrdinalMismatchWeakDuplicate,
}

impl TradeCandidateMatchKind {
    const fn label(self) -> &'static str {
        match self {
            Self::ExactMatch | Self::OrdinalMismatchExactMatch => "exact_match",
            Self::WeakDuplicate | Self::OrdinalMismatchWeakDuplicate => "weak_duplicate_match",
            Self::None => "none",
        }
    }
}

#[inline(always)]
fn parsed_event_kind_label(kind: &ParsedEventKind) -> &'static str {
    match kind {
        ParsedEventKind::Trade { .. } => "trade",
        ParsedEventKind::SwapTrade { .. } => "swap_trade",
        ParsedEventKind::CpiTrade(_) => "cpi_trade",
        ParsedEventKind::CpiSwapBuy(_) => "cpi_swap_buy",
        ParsedEventKind::CpiSwapSell(_) => "cpi_swap_sell",
        _ => "other",
    }
}

#[inline(always)]
fn trade_candidate_score_label(score: Option<u32>) -> String {
    match score {
        Some(score) => score.to_string(),
        None => "none".to_string(),
    }
}

#[inline(always)]
fn parsed_event_provenance_counts(events: &[ParsedPumpEvent]) -> (usize, usize) {
    let present = events
        .iter()
        .filter(|event| event.provenance.is_some())
        .count();
    (present, events.len().saturating_sub(present))
}

#[inline(always)]
fn trade_provenance_counts(trades: &[TradeEvent]) -> (usize, usize) {
    let present = trades
        .iter()
        .filter(|trade| trade.provenance.is_some())
        .count();
    (present, trades.len().saturating_sub(present))
}

#[inline(always)]
fn sample_parsed_event_signature(events: &[ParsedPumpEvent]) -> String {
    events
        .iter()
        .find_map(|event| event.signature.as_ref().map(|sig| sig.to_string()))
        .unwrap_or_else(|| "-".to_string())
}

#[inline(always)]
fn sample_trade_signature(trades: &[TradeEvent]) -> String {
    trades
        .first()
        .map(|trade| trade.signature.to_string())
        .unwrap_or_else(|| "-".to_string())
}

#[inline(always)]
fn summarize_parsed_event_kinds(events: &[ParsedPumpEvent]) -> String {
    let mut trade = 0usize;
    let mut swap_trade = 0usize;
    let mut cpi_trade = 0usize;
    let mut cpi_swap_buy = 0usize;
    let mut cpi_swap_sell = 0usize;
    let mut other = 0usize;

    for event in events {
        match &event.kind {
            ParsedEventKind::Trade { .. } => trade += 1,
            ParsedEventKind::SwapTrade { .. } => swap_trade += 1,
            ParsedEventKind::CpiTrade(_) => cpi_trade += 1,
            ParsedEventKind::CpiSwapBuy(_) => cpi_swap_buy += 1,
            ParsedEventKind::CpiSwapSell(_) => cpi_swap_sell += 1,
            _ => other += 1,
        }
    }

    format!(
        "trade={trade},swap_trade={swap_trade},cpi_trade={cpi_trade},cpi_swap_buy={cpi_swap_buy},cpi_swap_sell={cpi_swap_sell},other={other}"
    )
}

#[inline(always)]
fn summarize_trade_winner_kinds(trades: &[TradeEvent]) -> String {
    let mut ix_trade = 0usize;
    let mut ix_swap = 0usize;
    let mut cpi_trade = 0usize;
    let mut cpi_swap = 0usize;
    let mut unknown = 0usize;

    for trade in trades {
        match (
            trade.is_pumpswap,
            trade.provenance.as_ref().map(|value| value.from_cpi),
        ) {
            (false, Some(true)) => cpi_trade += 1,
            (false, Some(false)) => ix_trade += 1,
            (true, Some(true)) => cpi_swap += 1,
            (true, Some(false)) => ix_swap += 1,
            (_, None) => unknown += 1,
        }
    }

    format!(
        "ix_trade={ix_trade},ix_swap={ix_swap},cpi_trade={cpi_trade},cpi_swap={cpi_swap},unknown={unknown}"
    )
}

#[inline(always)]
fn record_trade_event_dedup_stage(stage: &'static str, count: usize) {
    ::metrics::counter!(TRADE_EVENT_DEDUP_STAGE_TOTAL, count as u64, "stage" => stage);
}

#[inline(always)]
fn record_trade_event_dedup_decision(
    kind: &ParsedEventKind,
    decision: &'static str,
    reason: &'static str,
) {
    ::metrics::increment_counter!(
        TRADE_EVENT_DEDUP_TOTAL,
        "event_kind" => parsed_event_kind_label(kind),
        "decision" => decision,
        "reason" => reason
    );
}

#[inline(always)]
fn record_trade_candidate_dedup_stage(stage: &'static str, count: usize) {
    ::metrics::counter!(TRADE_CANDIDATE_DEDUP_STAGE_TOTAL, count as u64, "stage" => stage);
}

#[inline(always)]
fn record_trade_candidate_dedup_decision(
    decision: &'static str,
    reason_code: &'static str,
    match_kind: &'static str,
    ordinal_relation: &'static str,
    provenance_relation: &'static str,
    merge_action: &'static str,
    winner: &'static str,
    incoming_score: Option<u32>,
    existing_score: Option<u32>,
) {
    ::metrics::increment_counter!(
        TRADE_CANDIDATE_DEDUP_TOTAL,
        "decision" => decision,
        "reason_code" => reason_code,
        "match_kind" => match_kind,
        "ordinal_relation" => ordinal_relation,
        "provenance_relation" => provenance_relation,
        "merge_action" => merge_action,
        "winner" => winner,
        "incoming_score" => trade_candidate_score_label(incoming_score),
        "existing_score" => trade_candidate_score_label(existing_score)
    );
}

#[inline(always)]
fn record_trade_candidate_ordinal(case: &'static str, reason: &'static str) {
    ::metrics::increment_counter!(TRADE_CANDIDATE_ORDINAL_TOTAL, "case" => case, "reason" => reason);
}

#[inline(always)]
fn record_orphan_inner_group(
    walker: &'static str,
    signature: Option<&str>,
    outer_index: u32,
    outer_instruction_count: usize,
) {
    warn!(
        walker,
        sig = signature.unwrap_or("-"),
        outer_index,
        outer_instruction_count,
        "ORPHAN_INNER_GROUP"
    );
    ::metrics::increment_counter!(ORPHAN_INNER_GROUP_TOTAL, "walker" => walker);
}

#[inline(always)]
fn record_missing_inner_provenance(
    walker: &'static str,
    signature: Option<&str>,
    outer_index: u32,
    outer_program: Option<&str>,
    invoked_program: &str,
) {
    let reason = match outer_program {
        None => "outer_program_missing",
        Some(outer_program) if !is_pump_program(outer_program) => "non_pump_outer",
        Some(outer_program) if outer_program != invoked_program => "outer_program_mismatch",
        Some(_) => return,
    };

    debug!(
        walker,
        sig = signature.unwrap_or("-"),
        outer_index,
        outer_program = outer_program.unwrap_or("-"),
        invoked_program,
        reason,
        "INNER_PROVENANCE_GAP"
    );
    ::metrics::increment_counter!(
        MISSING_PROVENANCE_TOTAL,
        "walker" => walker,
        "reason" => reason
    );
}

#[inline(always)]
fn trade_candidate_ordinal_relation(existing: &TradeEvent, candidate: &TradeEvent) -> &'static str {
    match (existing.event_ordinal, candidate.event_ordinal) {
        (Some(existing), Some(candidate)) if existing == candidate => "same_event_ordinal",
        (Some(_), Some(_)) => "different_event_ordinal",
        (Some(_), None) => "incoming_event_ordinal_missing",
        (None, Some(_)) => "existing_event_ordinal_missing",
        (None, None) => "both_event_ordinals_missing",
    }
}

#[inline(always)]
fn trade_candidate_provenance_relation(
    existing: &TradeEvent,
    candidate: &TradeEvent,
) -> &'static str {
    match (&existing.provenance, &candidate.provenance) {
        (Some(existing), Some(candidate)) if existing == candidate => "same_provenance",
        (Some(_), Some(_)) => "different_provenance",
        (Some(_), None) | (None, Some(_)) => "one_missing_provenance",
        (None, None) => "both_provenance_missing",
    }
}

#[inline(always)]
fn record_parsed_event_arbitration_sample(before: &[ParsedPumpEvent], after: &[ParsedPumpEvent]) {
    let (before_provenance_present, before_provenance_missing) =
        parsed_event_provenance_counts(before);
    let (after_provenance_present, after_provenance_missing) =
        parsed_event_provenance_counts(after);
    let sample_sig = sample_parsed_event_signature(before);
    let before_kinds = summarize_parsed_event_kinds(before);
    let after_kinds = summarize_parsed_event_kinds(after);

    debug!(
        sig = sample_sig.as_str(),
        before_count = before.len(),
        after_count = after.len(),
        before_kinds = before_kinds.as_str(),
        winner_kinds = after_kinds.as_str(),
        before_provenance_present,
        before_provenance_missing,
        after_provenance_present,
        after_provenance_missing,
        "PARSED_EVENT_ARBITRATION_SAMPLE"
    );
}

#[inline(always)]
fn record_trade_candidate_arbitration_sample(
    sample_sig: &str,
    before_count: usize,
    before_kinds: &str,
    before_provenance_present: usize,
    before_provenance_missing: usize,
    after: &[TradeEvent],
) {
    let (after_provenance_present, after_provenance_missing) = trade_provenance_counts(after);
    let after_kinds = summarize_trade_winner_kinds(after);

    debug!(
        sig = sample_sig,
        before_count,
        after_count = after.len(),
        before_candidate_kinds = before_kinds,
        winner_kinds = after_kinds.as_str(),
        before_provenance_present,
        before_provenance_missing,
        after_provenance_present,
        after_provenance_missing,
        "TRADE_CANDIDATE_ARBITRATION_SAMPLE"
    );
}

#[inline(always)]
fn trade_candidate_match_kind(
    existing: &TradeEvent,
    candidate: &TradeEvent,
) -> TradeCandidateMatchKind {
    let same_signature = existing.signature == candidate.signature;
    let same_signer = existing.signer == candidate.signer;
    let same_side = existing.is_buy == candidate.is_buy;
    let same_amount = existing.amount == candidate.amount;
    let same_quote_amount =
        trade_dedup_quote_amount(existing) == trade_dedup_quote_amount(candidate);
    let exact_match =
        same_signature && same_signer && same_side && same_amount && same_quote_amount;
    let weak_duplicate = same_signature
        && same_side
        && same_amount
        && same_quote_amount
        && (trade_is_weak_candidate(existing) || trade_is_weak_candidate(candidate));

    if trade_ordinals_differ(existing, candidate) {
        if exact_match {
            return TradeCandidateMatchKind::OrdinalMismatchExactMatch;
        }
        if weak_duplicate {
            return TradeCandidateMatchKind::OrdinalMismatchWeakDuplicate;
        }
        return TradeCandidateMatchKind::None;
    }

    if exact_match {
        TradeCandidateMatchKind::ExactMatch
    } else if weak_duplicate {
        TradeCandidateMatchKind::WeakDuplicate
    } else {
        TradeCandidateMatchKind::None
    }
}

fn dedup_trade_events(out: &mut Vec<ParsedPumpEvent>, cm_reg: &CurveMintRegistry) {
    let snapshot = out.clone();
    let mut deduped = Vec::with_capacity(out.len());
    record_trade_event_dedup_stage("input", snapshot.len());

    for event in out.drain(..) {
        let (should_drop, reason) = match &event.kind {
            ParsedEventKind::Trade {
                side,
                mint,
                bonding_curve,
                ..
            } => {
                let mint_pk = Pubkey::from_str(mint).unwrap_or_default();
                let bc_pk = Pubkey::from_str(bonding_curve).unwrap_or_default();
                let should_drop =
                    has_matching_pumpfun_cpi(&snapshot, *side, &mint_pk, &bc_pk, cm_reg);
                (
                    should_drop,
                    if should_drop {
                        "drop_ix_trade_because_matching_cpi_trade"
                    } else {
                        "keep_ix_trade_because_no_matching_cpi_trade"
                    },
                )
            }
            ParsedEventKind::SwapTrade {
                side,
                pool,
                base_mint,
                base_amount,
                quote_amount,
                ..
            } => {
                let pool_pk = Pubkey::from_str(pool).unwrap_or_default();
                let base_mint_pk = Pubkey::from_str(base_mint).unwrap_or_default();
                let match_kind =
                    has_matching_pumpswap_cpi(&snapshot, *side, &pool_pk, &base_mint_pk, cm_reg);
                let zero_delta_placeholder = matches!(
                    match_kind,
                    PumpSwapStructuralMatchKind::CpiHasUnresolvedMint
                ) && *base_amount == 0
                    && *quote_amount == 0;
                (
                    match_kind.should_drop() || zero_delta_placeholder,
                    if zero_delta_placeholder {
                        PumpSwapStructuralMatchKind::MatchingCpiSwap.reason_code()
                    } else {
                        match_kind.reason_code()
                    },
                )
            }
            ParsedEventKind::CpiTrade(_) => (false, "keep_cpi_trade_as_structural_candidate"),
            ParsedEventKind::CpiSwapBuy(_) | ParsedEventKind::CpiSwapSell(_) => {
                (false, "keep_cpi_swap_as_structural_candidate")
            }
            _ => (false, "not_subject_to_structural_arbitration"),
        };

        record_trade_event_dedup_decision(
            &event.kind,
            if should_drop { "dropped" } else { "kept" },
            reason,
        );
        if should_drop {
            debug!(
                "TRADE_DEDUP outcome={} reason={} kind={:?}",
                TradeOutcome::DedupDropped.as_str(),
                reason,
                event.kind
            );
            record_trade_outcome_metric(TradeOutcome::DedupDropped);
        } else {
            deduped.push(event);
        }
    }

    record_trade_event_dedup_stage("output", deduped.len());
    record_parsed_event_arbitration_sample(&snapshot, &deduped);
    *out = deduped;
}

#[derive(Debug, Clone, BorshDeserialize)]
pub struct SetParamsData {
    pub fee_recipient: [u8; 32],
    pub initial_virtual_token_reserves: u64,
    pub initial_virtual_sol_reserves: u64,
    pub initial_real_token_reserves: u64,
    pub token_total_supply: u64,
    pub fee_basis_points: u64,
}

#[derive(Debug, Clone, BorshDeserialize)]
pub struct SwapParams {
    pub base_in: u64,
    pub quote_in: u64,
    pub min_base_out: u64,
    pub min_quote_out: u64,
}

/// [FIX-2] Migrate instruction params — decoded from the Pump.fun migrate Ix.
/// Previously the case matched the discriminator but fell through to Unknown
/// because there were no params defined.  With these params the parser emits
/// a fully-enriched Migrate event including pool_pubkey and lp_mint.
///
/// IDL layout (after discriminator):
///   pool_pubkey: [u8; 32]
///   lp_mint:     [u8; 32]
#[derive(Debug, Clone, BorshDeserialize)]
pub struct MigrateParams {
    pub pool_pubkey: [u8; 32],
    pub lp_mint: [u8; 32],
}

/// [FIX-3] SwapCreatePool params — decoded from the PumpSwap create_pool Ix.
/// Previously decoded only the account indices; now also pulls on-chain params
/// (initial amounts, lp_mint, fee config) from the instruction data itself.
/// Use lenient Borsh decoding because live CreatePool payloads may carry a
/// trailing reserved byte after the canonical struct fields.
///
/// IDL layout (after discriminator):
///   index:            u16
///   base_amount_in:   u64
///   quote_amount_in:  u64
///   lp_mint:          [u8; 32]
#[derive(Debug, Clone, BorshDeserialize)]
pub struct CreatePoolParams {
    pub index: u16,
    pub base_amount_in: u64,
    pub quote_amount_in: u64,
    pub lp_mint: [u8; 32],
}

// ─── CPI event log types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, BorshDeserialize)]
pub struct EventTrade {
    pub mint: [u8; 32],
    pub sol_amount: u64,
    pub token_amount: u64,
    pub is_buy: bool,
    pub user: [u8; 32],
    pub timestamp: i64,
    pub virtual_sol_reserves: u64,
    pub virtual_token_reserves: u64,
}

#[derive(Debug, Clone, BorshDeserialize)]
pub struct EventCreate {
    pub name: String,
    pub symbol: String,
    pub uri: String,
    pub mint: [u8; 32],
    pub bonding_curve: [u8; 32],
    pub user: [u8; 32],
}

#[derive(Debug, Clone, BorshDeserialize)]
pub struct EventComplete {
    pub user: [u8; 32],
    pub mint: [u8; 32],
    pub bonding_curve: [u8; 32],
    pub timestamp: i64,
}

// ─── PumpSwap AMM event structs ──────────────────────────────────────────────────

/// PumpSwap BuyEvent — emitted by pAMMBay6 self-CPI on AMM buys.
///
/// The on-chain event layout has evolved over time. For coverage we only need the
/// stable prefix that is present in current mainnet traffic:
///   timestamp + 13 u64 metrics + pool pubkey + user pubkey + trailing bytes.
/// The trailing bytes are intentionally ignored here and resolved from tx meta.
#[derive(Debug, Clone)]
pub struct SwapBuyEvent {
    pub timestamp: i64,
    pub base_amount_out: u64,
    pub max_quote_amount_in: u64,
    pub user_base_token_reserves: u64,
    pub user_quote_token_reserves: u64,
    pub pool_base_token_reserves: u64,
    pub pool_quote_token_reserves: u64,
    pub quote_amount_in: u64,
    pub lp_fee_basis_points: u64,
    pub lp_fee_amount: u64,
    pub protocol_fee_basis_points: u64,
    pub protocol_fee_amount: u64,
    pub quote_amount_in_with_lp_fee: u64,
    pub user_quote_amount_in: u64,
    pub pool: [u8; 32],
    pub user: [u8; 32],
}

/// PumpSwap SellEvent — sell-side mirror of the stable on-chain prefix.
#[derive(Debug, Clone)]
pub struct SwapSellEvent {
    pub timestamp: i64,
    pub base_amount_in: u64,
    pub min_quote_amount_out: u64,
    pub user_base_token_reserves: u64,
    pub user_quote_token_reserves: u64,
    pub pool_base_token_reserves: u64,
    pub pool_quote_token_reserves: u64,
    pub quote_amount_out: u64,
    pub lp_fee_basis_points: u64,
    pub lp_fee_amount: u64,
    pub protocol_fee_basis_points: u64,
    pub protocol_fee_amount: u64,
    pub quote_amount_out_without_lp_fee: u64,
    pub user_quote_amount_out: u64,
    pub pool: [u8; 32],
    pub user: [u8; 32],
}

// ─── Account state types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, BorshDeserialize)]
pub struct BondingCurveState {
    pub virtual_token_reserves: u64,
    pub virtual_sol_reserves: u64,
    pub real_token_reserves: u64,
    pub real_sol_reserves: u64,
    pub token_total_supply: u64,
    pub complete: bool,
}

impl BondingCurveState {
    #[inline(always)]
    pub fn price_sol_per_token(&self) -> f64 {
        if self.virtual_token_reserves == 0 {
            return 0.0;
        }
        (self.virtual_sol_reserves as f64 / 1e9) / (self.virtual_token_reserves as f64 / 1e6)
    }
    #[inline(always)]
    pub fn market_cap_sol(&self) -> f64 {
        self.price_sol_per_token() * (self.token_total_supply as f64 / 1e6)
    }
    #[inline(always)]
    pub fn progress(&self) -> f64 {
        if self.token_total_supply == 0 {
            return 0.0;
        }
        1.0 - (self.real_token_reserves as f64 / self.token_total_supply as f64)
    }
}

/// Snapshot sent to Ghost ShadowLedger on every bonding-curve account update.
/// This is the truth lane — ShadowLedger state is driven by curve account data,
/// NOT by trade instruction params.
#[derive(Debug, Clone)]
pub struct CurveSnapshot {
    pub curve_pubkey: String,
    pub mint_pubkey: Option<String>,
    pub slot: u64,
    pub virtual_token_reserves: u64,
    pub virtual_sol_reserves: u64,
    pub real_token_reserves: u64,
    pub real_sol_reserves: u64,
    pub token_total_supply: u64,
    pub price_sol_per_token: f64,
    pub market_cap_sol: f64,
    pub progress: f64,
    pub complete: bool,
    pub received_at: Instant,
}

impl CurveSnapshot {
    pub fn from_state(
        curve_pubkey: String,
        mint_pubkey: Option<String>,
        slot: u64,
        s: &BondingCurveState,
        received_at: Instant,
    ) -> Self {
        Self {
            curve_pubkey,
            mint_pubkey,
            slot,
            virtual_token_reserves: s.virtual_token_reserves,
            virtual_sol_reserves: s.virtual_sol_reserves,
            real_token_reserves: s.real_token_reserves,
            real_sol_reserves: s.real_sol_reserves,
            token_total_supply: s.token_total_supply,
            price_sol_per_token: s.price_sol_per_token(),
            market_cap_sol: s.market_cap_sol(),
            progress: s.progress(),
            complete: s.complete,
            received_at,
        }
    }
}

#[derive(Debug, Clone, BorshDeserialize)]
pub struct GlobalState {
    pub authority: [u8; 32],
    pub initialized: bool,
    pub fee_recipient: [u8; 32],
    pub initial_virtual_token_reserves: u64,
    pub initial_virtual_sol_reserves: u64,
    pub initial_real_token_reserves: u64,
    pub token_total_supply: u64,
    pub fee_basis_points: u64,
}

#[derive(Debug, Clone, BorshDeserialize)]
pub struct AmmPoolState {
    pub pool_bump: u8,
    pub index: u16,
    pub creator: [u8; 32],
    pub base_mint: [u8; 32],
    pub quote_mint: [u8; 32],
    pub lp_mint: [u8; 32],
    pub pool_base_token_account: [u8; 32],
    pub pool_quote_token_account: [u8; 32],
    pub base_amount: u64,
    pub quote_amount: u64,
}

impl AmmPoolState {
    #[inline(always)]
    pub fn price_quote_per_base(&self) -> f64 {
        if self.base_amount == 0 {
            return 0.0;
        }
        self.quote_amount as f64 / self.base_amount as f64
    }
}

#[derive(Debug, Clone)]
pub enum PumpAccountState {
    BondingCurve(BondingCurveState),
    Global(GlobalState),
    AmmPool(AmmPoolState),
    Unknown { discriminator: [u8; 8] },
}

// ─── Parsed event kinds ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeSource {
    BondingCurve, // direct Pump.fun instruction
    PumpSwapAmm,  // PumpSwap AMM instruction (post-graduation)
    CpiDirect,    // inner instruction / CPI
    CpiEventLog,  // Anchor self-CPI event (highest fidelity)
}

#[derive(Debug, Clone)]
pub enum ParsedEventKind {
    Initialize,
    SetParams(SetParamsData),
    Create {
        params: CreateParams,
        mint: String,
        bonding_curve: String,
        user: String,
    },
    Trade {
        side: TradeSide,
        source: TradeSource,
        mint: String,
        bonding_curve: String,
        user: String,
        global_config: Option<Pubkey>,
        fee_recipient: Option<Pubkey>,
        token_program: Option<Pubkey>,
        token_amount: u64,
        sol_amount: u64,
        virtual_token_reserves: u64,
        virtual_sol_reserves: u64,
        real_token_reserves: u64,
        real_sol_reserves: u64,
        market_cap_sol: f64,
        progress: f64,
        is_complete: bool,
    },
    MigrateReady {
        mint: String,
        bonding_curve: String,
        timestamp: Option<i64>,
    },
    /// [FIX-2] Migrate now carries pool_pubkey and lp_mint decoded from instruction params.
    Migrate {
        mint: String,
        bonding_curve: String,
        pool: String,
        user: String,
        /// Decoded from MigrateParams Borsh payload (previously Unknown because params missing).
        pool_pubkey_param: Option<String>,
        lp_mint_param: Option<String>,
    },
    /// [FIX-3] SwapPoolCreated now carries initial amounts and lp_mint from CreatePoolParams.
    SwapTrade {
        side: TradeSide,
        pool: String,
        base_mint: String,
        quote_mint: String,
        user: String,
        base_amount: u64,
        quote_amount: u64,
    },
    SwapPoolCreated {
        pool: String,
        base_mint: String,
        quote_mint: String,
        creator: String,
        /// Decoded from CreatePoolParams Borsh payload.
        base_amount_in: Option<u64>,
        quote_amount_in: Option<u64>,
        lp_mint_param: Option<String>,
    },
    Withdraw {
        mint: Option<String>,
        bonding_curve: Option<String>,
    },
    CpiTrade(EventTrade),
    CpiCreate(EventCreate),
    CpiComplete(EventComplete),
    /// PumpSwap BuyEvent — double-discriminator Anchor CPI event log.
    /// Pool base_amount_out (tokens bought), quote_amount_in (SOL spent),
    /// post-tx pool reserves. Highest-fidelity source for AMM buys.
    CpiSwapBuy(SwapBuyEvent),
    /// PumpSwap SellEvent — same double-discriminator pattern, sell side.
    CpiSwapSell(SwapSellEvent),
    AccountChange {
        pubkey: String,
        state: PumpAccountState,
        snapshot: Option<CurveSnapshot>,
    },
    /// [FIX-1] Coverage denominator — emitted once per EntryUpdate.
    ///
    /// Ghost coverage monitor sums `executed_transaction_count` per slot and
    /// divides by received Transaction event count to get real coverage %.
    ///
    /// `executed_transaction_count` = how many txs were packed into this entry.
    /// One slot may have multiple entries; sum all EntryAnchor values for the slot.
    EntryAnchor {
        executed_transaction_count: u64,
    },
    Unknown {
        discriminator: [u8; 8],
        program: String,
    },
}

#[derive(Debug, Clone)]
pub struct ParsedPumpEvent {
    pub received_at: Instant,
    pub slot: u64,
    pub signature: Option<String>,
    pub event_ordinal: Option<u32>,
    pub provenance: Option<InstructionProvenance>,
    pub kind: ParsedEventKind,
    pub from_cpi: bool,
    pub is_backfill: bool,
}

impl ParsedPumpEvent {
    pub fn e2e_ms(&self) -> u64 {
        self.received_at.elapsed().as_millis() as u64
    }
}

#[inline(always)]
fn top_level_provenance(
    outer_instruction_index: u32,
    invoked_program_id: &str,
) -> InstructionProvenance {
    InstructionProvenance {
        outer_instruction_index: Some(outer_instruction_index),
        inner_group_index: None,
        outer_program_id: None,
        invoked_program_id: invoked_program_id.to_string(),
        stack_height: None,
        from_cpi: false,
    }
}

#[inline(always)]
fn inner_instruction_provenance(
    outer_instruction_index: u32,
    outer_program_id: Option<&str>,
    invoked_program_id: &str,
    stack_height: Option<u32>,
) -> InstructionProvenance {
    InstructionProvenance {
        outer_instruction_index: Some(outer_instruction_index),
        inner_group_index: Some(outer_instruction_index),
        outer_program_id: outer_program_id.map(ToOwned::to_owned),
        invoked_program_id: invoked_program_id.to_string(),
        stack_height,
        from_cpi: true,
    }
}

#[inline(always)]
fn stamp_new_events(
    out: &mut Vec<ParsedPumpEvent>,
    start_len: usize,
    ordinal: u32,
    provenance: &InstructionProvenance,
) {
    for event in &mut out[start_len..] {
        event.event_ordinal = Some(ordinal);
        event.provenance = Some(provenance.clone());
    }
}

// ─── Curve ↔ Mint registry ────────────────────────────────────────────────────

/// Bidirectional curve_pubkey ↔ mint_pubkey map.
///
/// REQUIRED because BondingCurveState account data does NOT store the mint pubkey.
/// Without this registry: account-update truth lane cannot be attributed to a token,
/// and ShadowLedger/SnapshotEngine cannot correlate curve updates with positions.
///
/// Populated by:
///   - Create instruction decode (primary source)
///   - Buy/Sell instruction decode (fallback enrichment)
///   - CpiCreate / CpiTrade event log decode (highest confidence)
/// Curve↔mint bidirectional registry.
///
/// Keys stored as raw `[u8; 32]` (Pubkey bytes) — stack-allocated, `Copy`, no heap
/// allocation per entry. DashMap 5.x uses AHash by default, which is faster than
/// SipHash on fixed-size keys.
///
/// Hot path: use `insert_pk` / `mint_for_curve_pk` / `curve_for_mint_pk` when the
/// caller already has a `Pubkey` — zero Base58 codec overhead.
/// Legacy `&str` methods are kept for call-sites that originate from protobuf strings.
#[derive(Clone, Default)]
pub struct CurveMintRegistry {
    curve_to_mint: Arc<DashMap<[u8; 32], [u8; 32]>>,
    mint_to_curve: Arc<DashMap<[u8; 32], [u8; 32]>>,
}

impl CurveMintRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    // ── Hot-path API: Pubkey / [u8;32] — zero codec ───────────────────────────

    #[inline(always)]
    pub fn insert_pk(&self, curve: &Pubkey, mint: &Pubkey) {
        self.curve_to_mint.insert(curve.to_bytes(), mint.to_bytes());
        self.mint_to_curve
            .entry(mint.to_bytes())
            .or_insert(curve.to_bytes());
    }

    /// Returns the mint bytes for `curve`, or `None`.  No allocation.
    #[inline(always)]
    pub fn mint_for_curve_pk(&self, curve: &Pubkey) -> Option<Pubkey> {
        self.curve_to_mint
            .get(&curve.to_bytes())
            .map(|e| Pubkey::new_from_array(*e.value()))
    }

    /// Returns the curve bytes for `mint`, or `None`.  No allocation.
    #[inline(always)]
    pub fn curve_for_mint_pk(&self, mint: &Pubkey) -> Option<Pubkey> {
        self.mint_to_curve
            .get(&mint.to_bytes())
            .map(|e| Pubkey::new_from_array(*e.value()))
    }

    // ── Legacy &str API: for callers that receive Base58 from protobuf ─────────

    pub fn insert(&self, curve: impl AsRef<str>, mint: impl AsRef<str>) {
        let Ok(c) = Pubkey::from_str(curve.as_ref()) else {
            return;
        };
        let Ok(m) = Pubkey::from_str(mint.as_ref()) else {
            return;
        };
        self.insert_pk(&c, &m);
    }

    pub fn mint_for_curve(&self, curve: &str) -> Option<String> {
        let key = Pubkey::from_str(curve).ok()?;
        self.mint_for_curve_pk(&key).map(|p| p.to_string())
    }

    pub fn curve_for_mint(&self, mint: &str) -> Option<String> {
        let key = Pubkey::from_str(mint).ok()?;
        self.curve_for_mint_pk(&key).map(|p| p.to_string())
    }

    pub fn len(&self) -> usize {
        self.curve_to_mint.len()
    }
    pub fn is_empty(&self) -> bool {
        self.curve_to_mint.is_empty()
    }
}

// ─── Resolve queue ────────────────────────────────────────────────────────────

/// [FIX-5] Raised from 100 to 2048.
///
/// Root cause: at launch burst >100 new tokens/s the queue overflowed and
/// dropped the oldest entries.  At ~100 unresolved curves × 20s TTL window
/// before Create arrives, 2048 gives safe headroom even at 10× normal launch rate.
/// Each entry is ~(32+32+8+8) ≈ 80 bytes of metadata + raw account bytes (~200B) ≈ 280B.
/// 2048 × 280B ≈ 570 KB — negligible.
pub const DEFAULT_RESOLVE_QUEUE_CAP: usize = 2_048;

/// Holds account updates that arrived before the curve→mint mapping was established.
/// Ghost integration: drain this after every Create/CpiCreate decode.
pub struct ResolveQueue {
    inner: ParkingMutex<VecDeque<([u8; 32] /*curve*/, u64 /*slot*/, Vec<u8>, Instant)>>,
    max: usize,
}

impl Clone for ResolveQueue {
    fn clone(&self) -> Self {
        let snapshot = self.inner.lock().clone();
        Self {
            inner: ParkingMutex::new(snapshot),
            max: self.max,
        }
    }
}

impl ResolveQueue {
    /// Construct with explicit capacity.
    pub fn new(max: usize) -> Self {
        Self {
            inner: ParkingMutex::new(VecDeque::with_capacity(max.min(4096))),
            max,
        }
    }

    /// Construct with default capacity ([`DEFAULT_RESOLVE_QUEUE_CAP`]).
    pub fn with_default_cap() -> Self {
        Self::new(DEFAULT_RESOLVE_QUEUE_CAP)
    }

    pub fn push(&self, curve: String, slot: u64, raw: Vec<u8>, received_at: Instant) {
        let Ok(key) = Pubkey::from_str(&curve) else {
            return;
        };
        let mut q = self.inner.lock();
        if q.len() >= self.max {
            q.pop_front();
        }
        q.push_back((key.to_bytes(), slot, raw, received_at));
    }

    /// Drain all entries whose curve now has a known mint.
    pub fn drain_resolved(
        &self,
        reg: &CurveMintRegistry,
    ) -> Vec<(
        String, /*curve*/
        String, /*mint*/
        u64,
        Vec<u8>,
        Instant,
    )> {
        let mut q = self.inner.lock();
        let mut out = Vec::new();
        q.retain(|(curve_bytes, slot, raw, received_at)| {
            let curve_str = Pubkey::new_from_array(*curve_bytes).to_string();
            if let Some(mint) = reg.mint_for_curve(&curve_str) {
                out.push((curve_str, mint, *slot, raw.clone(), *received_at));
                false
            } else {
                true
            }
        });
        out
    }

    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }
}

// ─── [FIX-4] Complete state tracker ──────────────────────────────────────────
//
// Root cause: parse_account_raw previously emitted MigrateReady on EVERY
// account update where complete=true — including updates during and after
// migration.  This floods Ghost with duplicate MigrateReady events, causing
// ledger state machine to re-trigger migrate logic on already-migrated tokens.
//
// Fix: track the last-seen complete flag per curve.  Emit MigrateReady ONLY
// on the false→true transition.  Subsequent updates with complete=true
// (e.g., AMM pool state updates) are silently ignored.
//
// Memory: one bool per token.  ~1M tokens × ~50 bytes = ~50 MiB worst case.
// In practice Pump.fun sees ~5-10k tokens/day so this is negligible.
#[derive(Clone, Default)]
pub struct CompleteTracker(Arc<DashMap<[u8; 32], bool>>);

impl CompleteTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Hot-path: caller already has a `Pubkey` — zero Base58 codec.
    #[inline(always)]
    pub fn check_and_set_pk(&self, curve: &Pubkey, complete: bool) -> bool {
        if !complete {
            return false;
        }
        let key = curve.to_bytes();
        match self.0.entry(key) {
            dashmap::mapref::entry::Entry::Vacant(e) => {
                e.insert(true);
                true
            }
            dashmap::mapref::entry::Entry::Occupied(mut e) => {
                if *e.get() {
                    false
                } else {
                    e.insert(true);
                    true
                }
            }
        }
    }

    /// Returns `true` when `complete` just flipped from `false` to `true` for `curve`.
    /// Always returns `false` for `complete=false` (no-op / reset path).
    #[inline(always)]
    pub fn check_and_set(&self, curve: &str, complete: bool) -> bool {
        if !complete {
            return false;
        }
        let Ok(key) = Pubkey::from_str(curve) else {
            return false;
        };
        self.check_and_set_pk(&key, complete)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
// ─── Parser ───────────────────────────────────────────────────────────────────

pub struct PumpParser;

impl PumpParser {
    // ═════════════════════════════════════════════════════════════════════════
    // PUBLIC ENTRY POINTS
    // ═════════════════════════════════════════════════════════════════════════

    /// Unified dispatcher.  All event variants route through here.
    ///
    /// Backfill policy enforced here:
    ///   BackfillTransaction → parse_transaction_raw (identical path as live)
    ///   Never short-circuit on is_backfill.  Classify AFTER parsing.
    pub fn parse(
        ev: &PumpEvent,
        cm_reg: &CurveMintRegistry,
        ar_reg: &AccountRegistry,
        rq: &ResolveQueue,
        complete_tracker: &CompleteTracker, // [FIX-4]
    ) -> Vec<ParsedPumpEvent> {
        match ev {
            PumpEvent::Transaction {
                signature,
                slot,
                received_at,
                raw,
            } => Self::parse_transaction_raw(
                raw,
                Some(signature),
                *slot,
                *received_at,
                false,
                cm_reg,
                ar_reg,
                rq,
            ),

            // Backfill: RPC-sourced GeyserEvent (no proto bytes available).
            // Uses parse_geyser_transaction which works directly from decoded fields.
            PumpEvent::BackfillTransaction {
                signature,
                slot,
                received_at,
                decoded,
            } => match decoded {
                Some(GeyserEvent::Transaction {
                    accounts,
                    instructions,
                    inner_instructions,
                    logs: _,
                    pre_balances,
                    post_balances,
                    pre_token_balances,
                    post_token_balances,
                    ..
                }) => {
                    let mut evs = Self::parse_geyser_transaction(
                        accounts,
                        instructions,
                        inner_instructions,
                        pre_balances,
                        post_balances,
                        pre_token_balances,
                        post_token_balances,
                        Some(signature),
                        *slot,
                        *received_at,
                        true,
                        cm_reg,
                        ar_reg,
                        rq,
                    );
                    for e in &mut evs {
                        e.is_backfill = true;
                    }
                    evs
                }
                _ => vec![],
            },

            PumpEvent::AccountUpdate {
                pubkey: _,
                slot: _,
                received_at,
                decoded,
            } => match decoded {
                Some(GeyserEvent::AccountUpdate {
                    slot, pubkey, data, ..
                }) => Self::parse_account_raw(
                    data,
                    pubkey,
                    *slot,
                    *received_at,
                    cm_reg,
                    rq,
                    complete_tracker,
                ),
                _ => vec![],
            },

            PumpEvent::EntryUpdate {
                slot,
                received_at,
                executed_transaction_count,
                raw,
            } => Self::parse_entry_raw(raw, *slot, *received_at, *executed_transaction_count),
        }
    }

    // ─── Transaction parsing ─────────────────────────────────────────────────

    fn parse_transaction_raw(
        raw: &[u8],
        sig_str: Option<&String>,
        slot: u64,
        received_at: Instant,
        is_backfill: bool,
        cm_reg: &CurveMintRegistry,
        ar_reg: &AccountRegistry,
        rq: &ResolveQueue,
    ) -> Vec<ParsedPumpEvent> {
        use yellowstone_grpc_proto::prelude::SubscribeUpdateTransaction;

        let update = match SubscribeUpdateTransaction::decode(raw) {
            Ok(u) => u,
            Err(e) => {
                warn!("proto decode Tx: {e}");
                return vec![];
            }
        };

        let tx_info = match update.transaction.as_ref() {
            Some(t) => t,
            None => return vec![],
        };
        let tx = match tx_info.transaction.as_ref() {
            Some(t) => t,
            None => return vec![],
        };
        let msg = match tx.message.as_ref() {
            Some(m) => m,
            None => return vec![],
        };
        let meta = tx_info.meta.as_ref();

        // Full account key table: static keys + ATL-resolved writable + readonly.
        // Without ATL loading, many modern Pump.fun txs (which use address lookup
        // tables) will have wrong account indices and produce garbage pubkeys.
        let mut all_keys: Vec<String> = msg
            .account_keys
            .iter()
            .map(|b| bs58::encode(b).into_string())
            .collect();
        if let Some(m) = meta {
            for k in &m.loaded_writable_addresses {
                all_keys.push(bs58::encode(k).into_string());
            }
            for k in &m.loaded_readonly_addresses {
                all_keys.push(bs58::encode(k).into_string());
            }
        }

        let signature = sig_str.cloned();
        let mut out: Vec<ParsedPumpEvent> = Vec::with_capacity(6);
        let mut instruction_ordinal: u32 = 0;

        let pre_balances = meta.map(|m| m.pre_balances.as_slice()).unwrap_or(&[]);
        let post_balances = meta.map(|m| m.post_balances.as_slice()).unwrap_or(&[]);

        let pre_token_balances = meta
            .map(|m| {
                m.pre_token_balances
                    .iter()
                    .map(|b| crate::types::RawTokenBalance {
                        account_index: b.account_index,
                        mint: b.mint.clone(),
                        owner: if b.owner.is_empty() {
                            None
                        } else {
                            Some(b.owner.clone())
                        },
                        amount: b
                            .ui_token_amount
                            .as_ref()
                            .and_then(|a| a.amount.parse::<u64>().ok())
                            .unwrap_or(0),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let post_token_balances = meta
            .map(|m| {
                m.post_token_balances
                    .iter()
                    .map(|b| crate::types::RawTokenBalance {
                        account_index: b.account_index,
                        mint: b.mint.clone(),
                        owner: if b.owner.is_empty() {
                            None
                        } else {
                            Some(b.owner.clone())
                        },
                        amount: b
                            .ui_token_amount
                            .as_ref()
                            .and_then(|a| a.amount.parse::<u64>().ok())
                            .unwrap_or(0),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        // ── 1. Top-level instructions ──────────────────────────────────────
        for (outer_instruction_index, ix) in msg.instructions.iter().enumerate() {
            let before = out.len();
            let prog = key_at(&all_keys, ix.program_id_index as usize);
            if !is_pump_program(&prog) {
                instruction_ordinal = instruction_ordinal.saturating_add(1);
                continue;
            }

            let ix_accounts = resolve_accounts(&ix.accounts, &all_keys);
            Self::decode_ix(
                &ix.data,
                &ix_accounts,
                &prog,
                slot,
                signature.clone(),
                received_at,
                is_backfill,
                false,
                pre_balances,
                post_balances,
                &pre_token_balances,
                &post_token_balances,
                &all_keys,
                cm_reg,
                ar_reg,
                &mut out,
            );
            let provenance = top_level_provenance(outer_instruction_index as u32, &prog);
            stamp_new_events(&mut out, before, instruction_ordinal, &provenance);
            instruction_ordinal = instruction_ordinal.saturating_add(1);
        }

        // ── 2. Inner instructions (CPI) ────────────────────────────────────
        //
        // This is where migrate and PumpSwap CPI trades live.
        // Migrate instruction appears as a CPI from the bonding-curve program
        // in 70-90% of cases — never decoded if you skip inner_instructions.
        //
        // Two kinds of inner ix to handle:
        //   2a. Pump.fun / PumpSwap program CPI → decode_ix
        //   2b. Anchor event-emitter self-CPI → try_decode_cpi_event
        if let Some(meta) = meta {
            let outer_instruction_count = msg.instructions.len();
            for inner_set in &meta.inner_instructions {
                let outer_program = msg
                    .instructions
                    .get(inner_set.index as usize)
                    .map(|ix| key_at(&all_keys, ix.program_id_index as usize))
                    .filter(|program| !program.is_empty());
                if outer_program.is_none() {
                    record_orphan_inner_group(
                        "raw",
                        signature.as_deref(),
                        inner_set.index,
                        outer_instruction_count,
                    );
                }
                let insts = &inner_set.instructions;
                for (i, inner_ix) in insts.iter().enumerate() {
                    let curr_sh = inner_ix.stack_height;
                    let next_sh = insts.get(i + 1).and_then(|ix| ix.stack_height);
                    let has_deeper_next = matches!((curr_sh, next_sh), (Some(c), Some(n)) if n > c);
                    let before = out.len();
                    let prog = key_at(&all_keys, inner_ix.program_id_index as usize);

                    // 2a. Program instruction CPI
                    if !is_pump_program(&prog) {
                        if !has_deeper_next {
                            instruction_ordinal = instruction_ordinal.saturating_add(1);
                        }
                        continue;
                    }

                    record_missing_inner_provenance(
                        "raw",
                        signature.as_deref(),
                        inner_set.index,
                        outer_program.as_deref(),
                        &prog,
                    );
                    let ix_accounts = resolve_accounts(&inner_ix.accounts, &all_keys);
                    Self::decode_ix(
                        &inner_ix.data,
                        &ix_accounts,
                        &prog,
                        slot,
                        signature.clone(),
                        received_at,
                        is_backfill,
                        true,
                        pre_balances,
                        post_balances,
                        &pre_token_balances,
                        &post_token_balances,
                        &all_keys,
                        cm_reg,
                        ar_reg,
                        &mut out,
                    );

                    // 2b. Anchor event-emitter self-CPI.
                    // Pump.fun emits CpiTrade/CpiCreate/CpiComplete (DISC_EVENT_*).
                    // PumpSwap emits CpiSwapBuy/CpiSwapSell (DISC_SWAP_OUTER_WRAPPER + inner disc).
                    // Both use identical Anchor self-CPI mechanism — route both here.
                    if let Some(ev) = Self::try_decode_cpi_event(
                        &inner_ix.data,
                        slot,
                        signature.clone(),
                        received_at,
                        is_backfill,
                    ) {
                        // If this is an EventCreate, also register the curve↔mint mapping
                        if let ParsedEventKind::CpiCreate(ref ec) = ev.kind {
                            let curve = bs58::encode(&ec.bonding_curve).into_string();
                            let mint = bs58::encode(&ec.mint).into_string();
                            cm_reg.insert(curve.clone(), mint.clone());
                            ar_reg.insert_curve(curve);
                            ar_reg.insert_mint(mint);
                            // Drain resolve queue for any account updates that
                            // arrived before this Create was decoded
                            let _ = rq.drain_resolved(cm_reg);
                        }
                        // If EventComplete: synthesise MigrateReady alongside
                        if let ParsedEventKind::CpiComplete(ref ec) = ev.kind {
                            out.push(ParsedPumpEvent {
                                received_at,
                                slot,
                                signature: signature.clone(),
                                event_ordinal: None,
                                provenance: None,
                                from_cpi: true,
                                is_backfill,
                                kind: ParsedEventKind::MigrateReady {
                                    mint: bs58::encode(&ec.mint).into_string(),
                                    bonding_curve: bs58::encode(&ec.bonding_curve).into_string(),
                                    timestamp: Some(ec.timestamp),
                                },
                            });
                        }
                        out.push(ev);
                    }

                    let provenance = inner_instruction_provenance(
                        inner_set.index,
                        outer_program.as_deref(),
                        &prog,
                        inner_ix.stack_height,
                    );
                    stamp_new_events(&mut out, before, instruction_ordinal, &provenance);
                    if !has_deeper_next {
                        instruction_ordinal = instruction_ordinal.saturating_add(1);
                    }
                }
            }
        }

        // Prefer CPI event logs only when they are at least as informative as ix-level
        // events. For PumpSwap this means keeping `SwapTrade` if the CPI event cannot yet
        // resolve pool->mint; otherwise we end up buffering `mint=111...` and losing the
        // directly available base mint from the ix accounts.
        dedup_trade_events(&mut out, cm_reg);

        out
    }

    /// Parse a transaction from already-decoded GeyserEvent fields.
    ///
    /// Used for RPC backfill (BackfillTransaction), where we have a GeyserEvent
    /// with Vec<Pubkey> accounts and Vec<RawInstruction> instructions rather than
    /// raw proto bytes.  Identical business logic to parse_transaction_raw.
    #[allow(clippy::too_many_arguments)]
    fn parse_geyser_transaction(
        accounts: &[Pubkey],
        instructions: &[crate::types::RawInstruction],
        inner_instructions: &[crate::types::InnerInstructionGroup],
        pre_balances: &[u64],
        post_balances: &[u64],
        pre_token_balances: &[crate::types::RawTokenBalance],
        post_token_balances: &[crate::types::RawTokenBalance],
        sig_str: Option<&String>,
        slot: u64,
        received_at: Instant,
        is_backfill: bool,
        cm_reg: &CurveMintRegistry,
        ar_reg: &AccountRegistry,
        rq: &ResolveQueue,
    ) -> Vec<ParsedPumpEvent> {
        // Convert Pubkey → String once upfront (matches parse_transaction_raw format).
        let all_keys: Vec<String> = accounts.iter().map(|k| k.to_string()).collect();

        let signature = sig_str.cloned();
        let mut out: Vec<ParsedPumpEvent> = Vec::with_capacity(6);
        let mut instruction_ordinal: u32 = 0;

        // ── 1. Top-level instructions ──────────────────────────────────────
        for (outer_instruction_index, ix) in instructions.iter().enumerate() {
            let before = out.len();
            let prog = ix.program_id.to_string();
            if !is_pump_program(&prog) {
                instruction_ordinal = instruction_ordinal.saturating_add(1);
                continue;
            }
            let ix_accounts = resolve_accounts(&ix.account_indices, &all_keys);
            Self::decode_ix(
                &ix.data,
                &ix_accounts,
                &prog,
                slot,
                signature.clone(),
                received_at,
                is_backfill,
                false,
                pre_balances,
                post_balances,
                pre_token_balances,
                post_token_balances,
                &all_keys,
                cm_reg,
                ar_reg,
                &mut out,
            );
            let provenance = top_level_provenance(outer_instruction_index as u32, &prog);
            stamp_new_events(&mut out, before, instruction_ordinal, &provenance);
            instruction_ordinal = instruction_ordinal.saturating_add(1);
        }

        // ── 2. Inner instructions (CPI) ────────────────────────────────────
        let outer_instruction_count = instructions.len();
        for inner_set in inner_instructions {
            let outer_program = instructions
                .get(inner_set.index as usize)
                .map(|ix| ix.program_id.to_string());
            if outer_program.is_none() {
                record_orphan_inner_group(
                    "geyser",
                    signature.as_deref(),
                    inner_set.index,
                    outer_instruction_count,
                );
            }
            let insts = &inner_set.instructions;
            for (i, inner_ix) in insts.iter().enumerate() {
                let curr_sh = inner_ix.stack_height;
                let next_sh = insts.get(i + 1).and_then(|ix| ix.stack_height);
                let has_deeper_next = matches!((curr_sh, next_sh), (Some(c), Some(n)) if n > c);
                let before = out.len();
                let prog = key_at(&all_keys, inner_ix.program_id_index as usize);

                if !is_pump_program(&prog) {
                    if !has_deeper_next {
                        instruction_ordinal = instruction_ordinal.saturating_add(1);
                    }
                    continue;
                }

                record_missing_inner_provenance(
                    "geyser",
                    signature.as_deref(),
                    inner_set.index,
                    outer_program.as_deref(),
                    &prog,
                );
                let ix_accounts = resolve_accounts(&inner_ix.accounts, &all_keys);
                Self::decode_ix(
                    &inner_ix.data,
                    &ix_accounts,
                    &prog,
                    slot,
                    signature.clone(),
                    received_at,
                    is_backfill,
                    true,
                    pre_balances,
                    post_balances,
                    pre_token_balances,
                    post_token_balances,
                    &all_keys,
                    cm_reg,
                    ar_reg,
                    &mut out,
                );

                if let Some(ev) = Self::try_decode_cpi_event(
                    &inner_ix.data,
                    slot,
                    signature.clone(),
                    received_at,
                    is_backfill,
                ) {
                    if let ParsedEventKind::CpiCreate(ref ec) = ev.kind {
                        let curve = bs58::encode(&ec.bonding_curve).into_string();
                        let mint = bs58::encode(&ec.mint).into_string();
                        cm_reg.insert(curve.clone(), mint.clone());
                        ar_reg.insert_curve(curve);
                        ar_reg.insert_mint(mint);
                        let _ = rq.drain_resolved(cm_reg);
                    }
                    if let ParsedEventKind::CpiComplete(ref ec) = ev.kind {
                        out.push(ParsedPumpEvent {
                            received_at,
                            slot,
                            signature: signature.clone(),
                            event_ordinal: None,
                            provenance: None,
                            from_cpi: true,
                            is_backfill,
                            kind: ParsedEventKind::MigrateReady {
                                bonding_curve: bs58::encode(&ec.bonding_curve).into_string(),
                                mint: bs58::encode(&ec.mint).into_string(),
                                timestamp: Some(ec.timestamp),
                            },
                        });
                    }
                    out.push(ev);
                }

                let provenance = inner_instruction_provenance(
                    inner_set.index,
                    outer_program.as_deref(),
                    &prog,
                    inner_ix.stack_height,
                );
                stamp_new_events(&mut out, before, instruction_ordinal, &provenance);
                if !has_deeper_next {
                    instruction_ordinal = instruction_ordinal.saturating_add(1);
                }
            }
        }

        out
    }

    // ─── Account parsing ─────────────────────────────────────────────────────

    fn parse_account_raw(
        raw: &[u8],
        pubkey: &Pubkey,
        slot: u64,
        received_at: Instant,
        cm_reg: &CurveMintRegistry,
        rq: &ResolveQueue,
        complete_tracker: &CompleteTracker, // [FIX-4]
    ) -> Vec<ParsedPumpEvent> {
        use yellowstone_grpc_proto::prelude::SubscribeUpdateAccount;

        let update = match SubscribeUpdateAccount::decode(raw) {
            Ok(u) => u,
            Err(e) => {
                warn!("proto decode Account: {e}");
                return vec![];
            }
        };

        let acc = match update.account.as_ref() {
            Some(a) => a,
            None => return vec![],
        };
        let state = decode_account_data(&acc.data);

        let mut out = Vec::with_capacity(2);

        let snapshot = if let PumpAccountState::BondingCurve(ref bc) = state {
            let mint_opt = cm_reg.mint_for_curve_pk(pubkey).map(|p| p.to_string());

            if mint_opt.is_none() {
                let pubkey_str = pubkey.to_string();
                debug!(
                    curve = &pubkey_str[..pubkey_str.len().min(8)],
                    "AccountUpdate curve queued (no mint mapping yet)"
                );
                rq.push(pubkey_str, slot, raw.to_vec(), received_at);
            }

            let snap =
                CurveSnapshot::from_state(pubkey.to_string(), mint_opt, slot, bc, received_at);

            // [FIX-4] Emit MigrateReady ONLY on the false→true transition.
            // Previous code emitted on every update where complete=true, causing
            // duplicate events for already-migrated tokens (post-migration account
            // updates still show complete=true).
            if complete_tracker.check_and_set_pk(pubkey, bc.complete) {
                debug!(
                    curve = &pubkey.to_string()[..8],
                    slot, "BondingCurve complete flip true"
                );
                out.push(ParsedPumpEvent {
                    received_at,
                    slot,
                    signature: None,
                    event_ordinal: None,
                    provenance: None,
                    from_cpi: false,
                    is_backfill: false,
                    kind: ParsedEventKind::MigrateReady {
                        mint: snap.mint_pubkey.clone().unwrap_or_default(),
                        bonding_curve: pubkey.to_string(),
                        timestamp: None,
                    },
                });
            }
            Some(snap)
        } else {
            None
        };

        out.push(ParsedPumpEvent {
            received_at,
            slot,
            signature: None,
            event_ordinal: None,
            provenance: None,
            from_cpi: false,
            is_backfill: false,
            kind: ParsedEventKind::AccountChange {
                pubkey: pubkey.to_string(),
                state,
                snapshot,
            },
        });

        out
    }

    // ─── Entry parsing ────────────────────────────────────────────────────────
    //
    // [FIX-1] EntryUpdate is now actively parsed for two purposes:
    //
    // PURPOSE 1 — Raw slot-throughput telemetry (always emitted):
    //   Emit ParsedEventKind::EntryAnchor carrying executed_transaction_count.
    //   Ghost uses this as slot-throughput context and a continuity signal, not
    //   as a like-for-like trade coverage denominator.
    //
    // PURPOSE 2 — Embedded CPI event log scan (best-effort):
    //   Some Yellowstone deployments (especially custom validators / Dragon's Mouth
    //   variants) embed Anchor CPI event log data directly in the Entry bytes
    //   as a trailing payload after the standard SubscribeUpdateEntry proto.
    //   We try to decode this in a fault-tolerant way: proto decode the known fields
    //   first, then scan the remainder for known Anchor discriminators.
    //   Standard Yellowstone nodes produce no trailing bytes → fast-path no-op.
    //
    //   This catches the ~1-2% of migrate events that appear only in entry streams
    //   in non-standard validator configurations.
    fn parse_entry_raw(
        raw: &[u8],
        slot: u64,
        received_at: Instant,
        executed_transaction_count: u64,
    ) -> Vec<ParsedPumpEvent> {
        trace!("Entry slot={slot} exec_tx_count={executed_transaction_count}");
        metrics::counter!("ghost.pump.entry_exec_tx", executed_transaction_count);
        let mut out = vec![make_ev(
            received_at,
            slot,
            None,
            false,
            false,
            ParsedEventKind::EntryAnchor {
                executed_transaction_count,
            },
        )];

        // Best-effort scan for embedded CPI event discriminators in raw entry bytes.
        // Standard Yellowstone entries contain no CPI data — this loop is a fast no-op
        // because none of the discriminator patterns match proto-encoded numeric fields.
        // Non-standard deployments (Dragon's Mouth variants) may embed Anchor event logs
        // as trailing payload; we catch those here.
        const KNOWN_CPI_DISCS: [[u8; 8]; 4] = [
            DISC_EVENT_CREATE,
            DISC_EVENT_TRADE,
            DISC_EVENT_COMPLETE,
            DISC_SWAP_OUTER_WRAPPER,
        ];

        if raw.len() >= 8 {
            let mut offset = 0;
            while offset + 8 <= raw.len() {
                let window: [u8; 8] = match raw[offset..offset + 8].try_into() {
                    Ok(w) => w,
                    Err(_) => break,
                };
                if KNOWN_CPI_DISCS.contains(&window) {
                    if let Some((ev, consumed_len)) = Self::try_decode_cpi_event_with_len(
                        &raw[offset..],
                        slot,
                        None,
                        received_at,
                        false,
                    ) {
                        debug!(
                            "Entry CPI event at offset={offset} slot={slot} kind={:?}",
                            std::mem::discriminant(&ev.kind)
                        );
                        metrics::counter!("ghost.pump.entry_cpi_hit", 1);
                        out.push(ev);
                        // Skip the bytes we actually consumed while decoding.
                        // For evolvable event layouts (for example PumpSwap wrappers with
                        // trailing fields) this is a stable prefix, not an assertion about
                        // the full on-chain event length.
                        offset += consumed_len.max(1);
                        continue;
                    }
                }
                offset += 1;
            }
        }

        out
    }

    // ═════════════════════════════════════════════════════════════════════════
    // INSTRUCTION DECODER
    // ═════════════════════════════════════════════════════════════════════════

    #[allow(clippy::too_many_arguments)]
    fn decode_ix(
        data: &[u8],
        accounts: &SmallVec<[String; 14]>,
        program: &str,
        slot: u64,
        signature: Option<String>,
        received_at: Instant,
        is_backfill: bool,
        from_cpi: bool,
        pre_balances: &[u64],
        post_balances: &[u64],
        pre_token_balances: &[crate::types::RawTokenBalance],
        post_token_balances: &[crate::types::RawTokenBalance],
        all_keys: &[String],
        cm_reg: &CurveMintRegistry,
        ar_reg: &AccountRegistry,
        out: &mut Vec<ParsedPumpEvent>,
    ) {
        if data.len() < 8 {
            return;
        }
        // decode_ix is invoked from PumpParser with filtered programs,
        // but keep this guard to protect direct/test calls.
        if !is_pump_program(program) {
            return;
        }

        let disc: [u8; 8] = data[..8].try_into().unwrap();
        let payload: &[u8] = &data[8..];
        let sig = signature.as_deref();

        let kind: ParsedEventKind = match disc {
            DISC_INITIALIZE => ParsedEventKind::Initialize,

            DISC_SET_PARAMS => match SetParamsData::try_from_slice(payload) {
                Ok(p) => ParsedEventKind::SetParams(p),
                Err(e) => {
                    warn!("decode SetParams: {e}");
                    return;
                }
            },

            // Match both the actual on-chain discriminator (DISC_CREATE, real Pump.fun)
            // and the theoretical Anchor SHA256 discriminator (DISC_CREATE_ANCHOR) as a
            // belt-and-suspenders fallback. In practice only DISC_CREATE fires on mainnet.
            DISC_CREATE | DISC_CREATE_ANCHOR => {
                if !has_min_accounts(
                    accounts,
                    CREATE_IDX_USER + 1,
                    "create",
                    program,
                    slot,
                    from_cpi,
                    sig,
                ) {
                    return;
                }
                let p = match borsh_read::<CreateParams>(payload) {
                    Some(p) => p,
                    None => {
                        warn!("decode Create: unreadable payload len={}", payload.len());
                        return;
                    }
                };
                // Use CREATE-specific account indices (different from Buy/Sell layout):
                //   CREATE: Mint=0, BondingCurve=2, Creator=7
                //   BUY/SELL: Mint=2, BondingCurve=3, User=6
                let mint = acs(accounts, CREATE_IDX_MINT);
                let bonding_curve = acs(accounts, CREATE_IDX_BONDING_CURVE);
                let user = acs(accounts, CREATE_IDX_USER);
                if !is_valid_curve_role(&mint, &bonding_curve) {
                    log_drop_role_mismatch(
                        "create",
                        program,
                        slot,
                        from_cpi,
                        sig,
                        "mint_or_curve_invalid",
                    );
                    return;
                }

                // Primary registration: curve ↔ mint mapping.
                // This is the most important side-effect of Create decoding.
                // Without it, every subsequent account update for this curve
                // lands in the resolve queue and truth-lane is delayed.
                if !bonding_curve.is_empty() && !mint.is_empty() {
                    cm_reg.insert(bonding_curve.clone(), mint.clone());
                    ar_reg.insert_curve(bonding_curve.clone());
                    ar_reg.insert_mint(mint.clone());
                    debug!(
                        curve = &bonding_curve[..bonding_curve.len().min(8)],
                        mint = &mint[..mint.len().min(8)],
                        "Create: registered curve+mint"
                    );
                }

                ParsedEventKind::Create {
                    params: p,
                    mint,
                    bonding_curve,
                    user,
                }
            }

            // buy_exact_quote_in is Axiom Trade's variant of PumpSwap buy.
            // Same account layout as buy; CpiSwapBuy event is emitted on-chain.
            // Without this arm, the outer ix is Unknown and swap_deltas never run,
            // leaving only the CPI event which may be mis-routed as a sell.
            DISC_BUY | DISC_SWAP_BUY_EXACT_QUOTE_IN | DISC_PUMP_BUY_ROUTED => {
                // DISC_BUY == DISC_SWAP_BUY (same sha256("global:buy")).
                // DISC_SWAP_BUY_EXACT_QUOTE_IN == sha256("global:buy_exact_quote_in").
                // DISC_PUMP_BUY_ROUTED is a routed pump.fun buy observed on-chain with
                // the canonical pump.fun account layout and TradeParams payload.
                // Distinguish by program_id: PumpSwap has different account layout.
                if program == PUMP_SWAP_PROGRAM_ID {
                    if !has_min_accounts(
                        accounts,
                        SWAP_IDX_POOL_QUOTE + 1,
                        "swap_buy",
                        program,
                        slot,
                        from_cpi,
                        sig,
                    ) {
                        return;
                    }
                    // ── PumpSwap AMM buy ──────────────────────────────────
                    // Layout: pool=0, user=1, global_config=2, base_mint=3,
                    //         quote_mint=4, user_base_ta=5, user_quote_ta=6,
                    //         pool_base_ta=7, pool_quote_ta=8, ...
                    let pool = acs(accounts, SWAP_IDX_POOL);
                    let user = acs(accounts, SWAP_IDX_USER);
                    let raw_base = acs(accounts, SWAP_IDX_BASE_MINT);
                    let raw_quote = acs(accounts, SWAP_IDX_QUOTE_MINT);
                    let Some((base_mint, quote_mint, swapped)) =
                        normalize_swap_pair(raw_base, raw_quote)
                    else {
                        log_drop_role_mismatch(
                            "swap_buy",
                            program,
                            slot,
                            from_cpi,
                            sig,
                            "pool_or_mints_invalid",
                        );
                        return;
                    };
                    if pool.is_empty() || pool == WSOL_MINT {
                        log_drop_role_mismatch(
                            "swap_buy",
                            program,
                            slot,
                            from_cpi,
                            sig,
                            "pool_or_mints_invalid",
                        );
                        return;
                    }
                    let (raw_base_amt, raw_quote_amt) = swap_deltas(
                        TradeSide::Buy,
                        accounts,
                        pre_balances,
                        post_balances,
                        pre_token_balances,
                        post_token_balances,
                        all_keys,
                    );
                    // When on-chain base was WSOL, swap_deltas returns
                    // (SOL delta, token delta) but normalized base=token, quote=SOL.
                    let (base_amount, quote_amount) = if swapped {
                        (raw_quote_amt, raw_base_amt)
                    } else {
                        (raw_base_amt, raw_quote_amt)
                    };
                    ParsedEventKind::SwapTrade {
                        side: TradeSide::Buy,
                        pool,
                        base_mint,
                        quote_mint,
                        user,
                        base_amount,
                        quote_amount,
                    }
                } else {
                    if !has_min_accounts(
                        accounts,
                        PUMP_IDX_USER + 1,
                        "buy",
                        program,
                        slot,
                        from_cpi,
                        sig,
                    ) {
                        return;
                    }
                    // ── Pump.fun bonding-curve buy ────────────────────────
                    let p = match borsh_read::<TradeParams>(payload) {
                        Some(p) => p,
                        None => {
                            warn!("decode Buy: unreadable payload len={}", payload.len());
                            return;
                        }
                    };
                    let mint = acs(accounts, PUMP_IDX_MINT);
                    let bonding_curve = acs(accounts, PUMP_IDX_BONDING_CURVE);
                    let user = acs(accounts, PUMP_IDX_USER);
                    let global_config = Pubkey::from_str(&acs(accounts, PUMP_IDX_GLOBAL_CONFIG))
                        .unwrap_or_default();
                    let fee_recipient = Pubkey::from_str(&acs(accounts, PUMP_IDX_FEE_RECIPIENT))
                        .unwrap_or_default();
                    let token_program = Pubkey::from_str(&acs(accounts, PUMP_IDX_TOKEN_PROGRAM))
                        .unwrap_or_default();
                    if !is_valid_curve_role(&mint, &bonding_curve) {
                        log_drop_role_mismatch(
                            "buy",
                            program,
                            slot,
                            from_cpi,
                            sig,
                            "mint_or_curve_invalid",
                        );
                        return;
                    }

                    if !bonding_curve.is_empty() && !mint.is_empty() {
                        if cm_reg.mint_for_curve(&bonding_curve).is_none() {
                            cm_reg.insert(bonding_curve.clone(), mint.clone());
                            ar_reg.insert_curve(bonding_curve.clone());
                            ar_reg.insert_mint(mint.clone());
                        }
                    }

                    let enriched = enrich_trade(
                        TradeSide::Buy,
                        &bonding_curve,
                        pre_balances,
                        post_balances,
                        all_keys,
                    );

                    if enriched.is_complete {
                        out.push(make_ev(
                            received_at,
                            slot,
                            signature.clone(),
                            from_cpi,
                            is_backfill,
                            ParsedEventKind::MigrateReady {
                                mint: mint.clone(),
                                bonding_curve: bonding_curve.clone(),
                                timestamp: None,
                            },
                        ));
                    }

                    ParsedEventKind::Trade {
                        side: TradeSide::Buy,
                        source: if from_cpi {
                            TradeSource::CpiDirect
                        } else {
                            TradeSource::BondingCurve
                        },
                        mint,
                        bonding_curve,
                        user,
                        token_amount: p.amount,
                        sol_amount: enriched.sol_amount,
                        virtual_token_reserves: enriched.vtr,
                        virtual_sol_reserves: enriched.vsr,
                        real_token_reserves: enriched.rtr,
                        real_sol_reserves: enriched.rsr,
                        market_cap_sol: enriched.market_cap_sol,
                        global_config: (global_config != Pubkey::default())
                            .then_some(global_config),
                        fee_recipient: (fee_recipient != Pubkey::default())
                            .then_some(fee_recipient),
                        token_program: (token_program != Pubkey::default())
                            .then_some(token_program),
                        progress: enriched.progress,
                        is_complete: enriched.is_complete,
                    }
                }
            }

            DISC_SELL => {
                // DISC_SELL == DISC_SWAP_SELL (same sha256("global:sell")).
                // Distinguish by program_id: PumpSwap has different account layout.
                if program == PUMP_SWAP_PROGRAM_ID {
                    if !has_min_accounts(
                        accounts,
                        SWAP_IDX_POOL_QUOTE + 1,
                        "swap_sell",
                        program,
                        slot,
                        from_cpi,
                        sig,
                    ) {
                        return;
                    }
                    // ── PumpSwap AMM sell ─────────────────────────────────
                    let pool = acs(accounts, SWAP_IDX_POOL);
                    let user = acs(accounts, SWAP_IDX_USER);
                    let raw_base = acs(accounts, SWAP_IDX_BASE_MINT);
                    let raw_quote = acs(accounts, SWAP_IDX_QUOTE_MINT);
                    let Some((base_mint, quote_mint, swapped)) =
                        normalize_swap_pair(raw_base, raw_quote)
                    else {
                        log_drop_role_mismatch(
                            "swap_sell",
                            program,
                            slot,
                            from_cpi,
                            sig,
                            "pool_or_mints_invalid",
                        );
                        return;
                    };
                    if pool.is_empty() || pool == WSOL_MINT {
                        log_drop_role_mismatch(
                            "swap_sell",
                            program,
                            slot,
                            from_cpi,
                            sig,
                            "pool_or_mints_invalid",
                        );
                        return;
                    }
                    let (raw_base_amt, raw_quote_amt) = swap_deltas(
                        TradeSide::Sell,
                        accounts,
                        pre_balances,
                        post_balances,
                        pre_token_balances,
                        post_token_balances,
                        all_keys,
                    );
                    // When on-chain base was WSOL, swap_deltas returns
                    // (SOL delta, token delta) but normalized base=token, quote=SOL.
                    let (base_amount, quote_amount) = if swapped {
                        (raw_quote_amt, raw_base_amt)
                    } else {
                        (raw_base_amt, raw_quote_amt)
                    };
                    ParsedEventKind::SwapTrade {
                        side: TradeSide::Sell,
                        pool,
                        base_mint,
                        quote_mint,
                        user,
                        base_amount,
                        quote_amount,
                    }
                } else {
                    if !has_min_accounts(
                        accounts,
                        PUMP_IDX_USER + 1,
                        "sell",
                        program,
                        slot,
                        from_cpi,
                        sig,
                    ) {
                        return;
                    }
                    // ── Pump.fun bonding-curve sell ───────────────────────
                    let p = match borsh_read::<TradeParams>(payload) {
                        Some(p) => p,
                        None => {
                            warn!("decode Sell: unreadable payload len={}", payload.len());
                            return;
                        }
                    };
                    let mint = acs(accounts, PUMP_IDX_MINT);
                    let bonding_curve = acs(accounts, PUMP_IDX_BONDING_CURVE);
                    let user = acs(accounts, PUMP_IDX_USER);
                    let global_config = Pubkey::from_str(&acs(accounts, PUMP_IDX_GLOBAL_CONFIG))
                        .unwrap_or_default();
                    let fee_recipient = Pubkey::from_str(&acs(accounts, PUMP_IDX_FEE_RECIPIENT))
                        .unwrap_or_default();
                    let token_program = Pubkey::from_str(&acs(accounts, PUMP_IDX_TOKEN_PROGRAM))
                        .unwrap_or_default();
                    if !is_valid_curve_role(&mint, &bonding_curve) {
                        log_drop_role_mismatch(
                            "sell",
                            program,
                            slot,
                            from_cpi,
                            sig,
                            "mint_or_curve_invalid",
                        );
                        return;
                    }

                    if !bonding_curve.is_empty() && !mint.is_empty() {
                        if cm_reg.mint_for_curve(&bonding_curve).is_none() {
                            cm_reg.insert(bonding_curve.clone(), mint.clone());
                            ar_reg.insert_curve(bonding_curve.clone());
                            ar_reg.insert_mint(mint.clone());
                        }
                    }

                    let enriched = enrich_trade(
                        TradeSide::Sell,
                        &bonding_curve,
                        pre_balances,
                        post_balances,
                        all_keys,
                    );

                    ParsedEventKind::Trade {
                        side: TradeSide::Sell,
                        source: if from_cpi {
                            TradeSource::CpiDirect
                        } else {
                            TradeSource::BondingCurve
                        },
                        mint,
                        bonding_curve,
                        user,
                        token_amount: p.amount,
                        sol_amount: enriched.sol_amount,
                        virtual_token_reserves: enriched.vtr,
                        virtual_sol_reserves: enriched.vsr,
                        real_token_reserves: enriched.rtr,
                        real_sol_reserves: enriched.rsr,
                        market_cap_sol: enriched.market_cap_sol,
                        global_config: (global_config != Pubkey::default())
                            .then_some(global_config),
                        fee_recipient: (fee_recipient != Pubkey::default())
                            .then_some(fee_recipient),
                        token_program: (token_program != Pubkey::default())
                            .then_some(token_program),
                        progress: enriched.progress,
                        is_complete: enriched.is_complete,
                    }
                }
            }

            DISC_WITHDRAW => ParsedEventKind::Withdraw {
                // keep optional semantics, but avoid OOB for pathological wrappers
                mint: accounts
                    .get(PUMP_IDX_MINT)
                    .cloned()
                    .filter(|s| !s.is_empty()),
                bonding_curve: accounts
                    .get(PUMP_IDX_BONDING_CURVE)
                    .cloned()
                    .filter(|s| !s.is_empty()),
            },

            // Migrate instruction — appears BOTH at top-level and as a CPI.
            // This case handles both; from_cpi flag distinguishes them.
            // [FIX-2] Now decodes MigrateParams (pool_pubkey, lp_mint) from payload.
            DISC_MIGRATE => {
                if !has_min_accounts(
                    accounts,
                    MIG_IDX_USER + 1,
                    "migrate",
                    program,
                    slot,
                    from_cpi,
                    sig,
                ) {
                    return;
                }
                let mint = acs(accounts, MIG_IDX_MINT);
                let bonding_curve = acs(accounts, MIG_IDX_BONDING_CURVE);
                let pool = acs(accounts, MIG_IDX_POOL);
                let user = acs(accounts, MIG_IDX_USER);
                if !is_valid_curve_role(&mint, &bonding_curve)
                    || pool.is_empty()
                    || pool == WSOL_MINT
                {
                    log_drop_role_mismatch(
                        "migrate",
                        program,
                        slot,
                        from_cpi,
                        sig,
                        "mint_curve_or_pool_invalid",
                    );
                    return;
                }

                // Register pool for PumpSwap account subscription
                if !pool.is_empty() {
                    ar_reg.insert_pool(pool.clone());
                }
                // Ensure curve↔mint mapping is established (may have been missed)
                if !bonding_curve.is_empty() && !mint.is_empty() {
                    cm_reg.insert(bonding_curve.clone(), mint.clone());
                }

                // Decode instruction params (previously not decoded → Unknown)
                let params = MigrateParams::try_from_slice(payload).ok();
                let pool_pubkey_param = params
                    .as_ref()
                    .map(|p| bs58::encode(&p.pool_pubkey).into_string());
                let lp_mint_param = params
                    .as_ref()
                    .map(|p| bs58::encode(&p.lp_mint).into_string());

                ParsedEventKind::Migrate {
                    mint,
                    bonding_curve,
                    pool,
                    user,
                    pool_pubkey_param,
                    lp_mint_param,
                }
            }

            // [FIX-3] SwapCreatePool now decodes CreatePoolParams from payload.
            // Previously only account indices were extracted; now also initial amounts and lp_mint.
            // Live CreatePool payloads can carry a trailing reserved byte, so
            // decode leniently instead of requiring an exact payload length.
            DISC_SWAP_CREATE_POOL => {
                if !has_min_accounts(
                    accounts,
                    SWAP_IDX_QUOTE_MINT + 1,
                    "swap_create_pool",
                    program,
                    slot,
                    from_cpi,
                    sig,
                ) {
                    return;
                }
                let pool = acs(accounts, SWAP_IDX_POOL);
                let raw_base = acs(accounts, SWAP_IDX_BASE_MINT);
                let raw_quote = acs(accounts, SWAP_IDX_QUOTE_MINT);
                let Some((base_mint, quote_mint, swapped)) =
                    normalize_swap_pair(raw_base, raw_quote)
                else {
                    log_drop_role_mismatch(
                        "swap_create_pool",
                        program,
                        slot,
                        from_cpi,
                        sig,
                        "pool_or_mints_invalid",
                    );
                    return;
                };
                let creator = acs(accounts, SWAP_CREATE_IDX_SIGNER);
                if pool.is_empty() || pool == WSOL_MINT {
                    log_drop_role_mismatch(
                        "swap_create_pool",
                        program,
                        slot,
                        from_cpi,
                        sig,
                        "pool_or_mints_invalid",
                    );
                    return;
                }
                if !pool.is_empty() {
                    ar_reg.insert_pool(pool.clone());
                }

                let params = borsh_read::<CreatePoolParams>(payload);
                let raw_base_in = params.as_ref().map(|p| p.base_amount_in);
                let raw_quote_in = params.as_ref().map(|p| p.quote_amount_in);
                // When on-chain base was WSOL, payload amounts follow on-chain
                // layout and must be swapped to match the normalized pair.
                let (base_amount_in, quote_amount_in) = if swapped {
                    (raw_quote_in, raw_base_in)
                } else {
                    (raw_base_in, raw_quote_in)
                };
                let lp_mint_param = params
                    .as_ref()
                    .map(|p| bs58::encode(&p.lp_mint).into_string());

                ParsedEventKind::SwapPoolCreated {
                    pool,
                    base_mint,
                    quote_mint,
                    creator,
                    base_amount_in,
                    quote_amount_in,
                    lp_mint_param,
                }
            }

            _ => ParsedEventKind::Unknown {
                discriminator: disc,
                program: program.to_string(),
            },
        };

        out.push(make_ev(
            received_at,
            slot,
            signature,
            from_cpi,
            is_backfill,
            kind,
        ));
    }

    // ─── CPI event log decoder ────────────────────────────────────────────────

    fn try_decode_cpi_event_with_len(
        data: &[u8],
        slot: u64,
        signature: Option<String>,
        received_at: Instant,
        is_backfill: bool,
    ) -> Option<(ParsedPumpEvent, usize)> {
        if data.len() < 8 {
            return None;
        }
        let disc: [u8; 8] = data[..8].try_into().unwrap();
        let payload = &data[8..];

        let (kind, consumed_len) = match disc {
            DISC_EVENT_TRADE | DISC_EVENT_CREATE | DISC_EVENT_COMPLETE | DISC_SWAP_EVENT_BUY
            | DISC_SWAP_EVENT_SELL => match decode_anchor_event_kind_with_len(disc, payload) {
                Some((kind, payload_len)) => (kind, 8 + payload_len),
                None => {
                    warn!(
                        "decode anchor event failed disc={:02x?} len={}",
                        disc,
                        payload.len()
                    );
                    return None;
                }
            },
            // Anchor write_event wrapper. Pump.fun and PumpSwap can both arrive as:
            //   [outer_wrapper][inner_event_disc][borsh_payload]
            // so unwrap once and decode the inner event discriminator generically.
            DISC_SWAP_OUTER_WRAPPER => {
                if payload.len() < 8 {
                    return None;
                }
                let inner_disc: [u8; 8] = payload[..8].try_into().unwrap();
                let inner_payload = &payload[8..];
                match decode_anchor_event_kind_with_len(inner_disc, inner_payload) {
                    Some((kind, inner_payload_len)) => (kind, 16 + inner_payload_len),
                    None => {
                        trace!(
                            "Anchor outer wrapper: unknown inner disc {:02x?} inner_len={}",
                            inner_disc,
                            inner_payload.len()
                        );
                        return None;
                    }
                }
            }
            _ => return None,
        };

        Some((
            ParsedPumpEvent {
                received_at,
                slot,
                signature,
                event_ordinal: None,
                provenance: None,
                from_cpi: true,
                is_backfill,
                kind,
            },
            consumed_len,
        ))
    }

    fn try_decode_cpi_event(
        data: &[u8],
        slot: u64,
        signature: Option<String>,
        received_at: Instant,
        is_backfill: bool,
    ) -> Option<ParsedPumpEvent> {
        Self::try_decode_cpi_event_with_len(data, slot, signature, received_at, is_backfill)
            .map(|(event, _)| event)
    }
}
// ─── Trade enrichment ─────────────────────────────────────────────────────────

struct Enriched {
    sol_amount: u64,
    vtr: u64,
    vsr: u64,
    rtr: u64,
    rsr: u64,
    market_cap_sol: f64,
    progress: f64,
    is_complete: bool,
}

/// Extract post-execution bonding-curve state from transaction meta.
///
/// Two sources, in priority order:
///   1. pre/post_balances[idx] — native SOL balance delta for the SOL amount.
///   2. Account-update truth lane (ShadowLedger) — the authoritative source.
///
/// NOTE: post_accounts (Yellowstone v2 post-tx account snapshots) are NOT available
/// in yellowstone-grpc-proto 1.14. We rely on SOL balance deltas here and let
/// ShadowLedger handle the authoritative curve state via account-update events.
fn enrich_trade(
    side: TradeSide,
    bc_key: &str,
    pre_balances: &[u64],
    post_balances: &[u64],
    all_keys: &[String],
) -> Enriched {
    let zero = Enriched {
        sol_amount: 0,
        vtr: 0,
        vsr: 0,
        rtr: 0,
        rsr: 0,
        market_cap_sol: 0.0,
        progress: 0.0,
        is_complete: false,
    };
    let idx = match all_keys.iter().position(|k| k == bc_key) {
        Some(i) => i,
        None => return zero,
    };

    // SOL delta from native balance changes
    let pre = pre_balances.get(idx).copied().unwrap_or(0);
    let post = post_balances.get(idx).copied().unwrap_or(0);
    let sol_amount = match side {
        TradeSide::Buy => post.saturating_sub(pre),
        TradeSide::Sell => pre.saturating_sub(post),
    };

    // Without post_accounts (Yellowstone v2-only), we return SOL delta only.
    // The authoritative bonding-curve state comes from the account-update truth lane:
    // AccountUpdate → decode_account_data → CurveSnapshot → ShadowLedger.
    Enriched { sol_amount, ..zero }
}

/// Extract base/quote token deltas for a PumpSwap trade.
///
/// Strategy:
///   1. Look up pool_base_ta and pool_quote_ta by pubkey in all_keys.
///   2. Find their entries in pre/post_token_balances by account_index.
///   3. For quote accounts that are wSOL (So111...) token accounts,
///      token_delta works correctly because wSOL TAs appear in token_balances.
///   4. Fallback: if token_delta returns 0 and the account has a native SOL
///      balance change (i.e. it IS the unwrapped SOL account of the pool),
///      use native balance delta instead.
fn swap_deltas(
    side: TradeSide,
    accounts: &SmallVec<[String; 14]>,
    pre_balances: &[u64],
    post_balances: &[u64],
    pre_token_balances: &[crate::types::RawTokenBalance],
    post_token_balances: &[crate::types::RawTokenBalance],
    all_keys: &[String],
) -> (u64, u64) {
    let pool_base_ta = acs(accounts, SWAP_IDX_POOL_BASE);
    let pool_quote_ta = acs(accounts, SWAP_IDX_POOL_QUOTE);

    let base_delta = token_delta(
        &pool_base_ta,
        pre_token_balances,
        post_token_balances,
        all_keys,
    );
    // For quote: try token balance first; fall back to native SOL delta
    let quote_delta = {
        let td = token_delta(
            &pool_quote_ta,
            pre_token_balances,
            post_token_balances,
            all_keys,
        );
        if td > 0 {
            td
        } else {
            // pool_quote_ta might be a native SOL account (wSOL unwrapped).
            // Try native balance delta as fallback.
            native_delta(&pool_quote_ta, side, pre_balances, post_balances, all_keys)
        }
    };
    (base_delta, quote_delta)
}

fn token_delta(
    acct: &str,
    pre_token_balances: &[crate::types::RawTokenBalance],
    post_token_balances: &[crate::types::RawTokenBalance],
    all_keys: &[String],
) -> u64 {
    if acct.is_empty() {
        return 0;
    }
    let idx = match all_keys.iter().position(|k| k == acct) {
        Some(i) => i as u32,
        None => return 0,
    };
    let pre = pre_token_balances
        .iter()
        .find(|b| b.account_index == idx)
        .map(|b| b.amount)
        .unwrap_or(0);
    let post = post_token_balances
        .iter()
        .find(|b| b.account_index == idx)
        .map(|b| b.amount)
        .unwrap_or(0);
    post.abs_diff(pre)
}

/// Native SOL balance delta for an account — used when pool_quote_ta is a
/// native SOL account rather than a wSOL token account.
fn native_delta(
    acct: &str,
    side: TradeSide,
    pre_balances: &[u64],
    post_balances: &[u64],
    all_keys: &[String],
) -> u64 {
    if acct.is_empty() {
        return 0;
    }
    let idx = match all_keys.iter().position(|k| k == acct) {
        Some(i) => i,
        None => return 0,
    };
    let pre = pre_balances.get(idx).copied().unwrap_or(0);
    let post = post_balances.get(idx).copied().unwrap_or(0);
    match side {
        TradeSide::Buy => post.saturating_sub(pre), // SOL into pool on buy
        TradeSide::Sell => pre.saturating_sub(post), // SOL out of pool on sell
    }
}

// ─── Account data decoder ─────────────────────────────────────────────────────

pub fn decode_account_data(data: &[u8]) -> PumpAccountState {
    if data.len() < 8 {
        return PumpAccountState::Unknown {
            discriminator: [0u8; 8],
        };
    }
    let disc: [u8; 8] = data[..8].try_into().unwrap();
    let payload = &data[8..];
    match disc {
        DISC_BONDING_CURVE => match borsh_read::<BondingCurveState>(payload) {
            Some(s) => PumpAccountState::BondingCurve(s),
            None => {
                warn!(
                    "decode BondingCurve: unreadable payload len={}",
                    payload.len()
                );
                PumpAccountState::Unknown {
                    discriminator: disc,
                }
            }
        },
        DISC_GLOBAL_STATE => match GlobalState::try_from_slice(payload) {
            Ok(s) => PumpAccountState::Global(s),
            Err(e) => {
                warn!("decode GlobalState: {e}");
                PumpAccountState::Unknown {
                    discriminator: disc,
                }
            }
        },
        DISC_AMM_POOL => match AmmPoolState::try_from_slice(payload) {
            Ok(s) => PumpAccountState::AmmPool(s),
            Err(e) => {
                warn!("decode AmmPool: {e}");
                PumpAccountState::Unknown {
                    discriminator: disc,
                }
            }
        },
        _ => PumpAccountState::Unknown {
            discriminator: disc,
        },
    }
}

// ─── Micro helpers ────────────────────────────────────────────────────────────

#[inline(always)]
fn is_pump_program(p: &str) -> bool {
    is_pump_fun_program(p) || p == PUMP_SWAP_PROGRAM_ID
}

#[inline(always)]
fn is_pump_fun_program(p: &str) -> bool {
    p == PUMP_FUN_PROGRAM_ID
}

#[inline(always)]
fn resolve_accounts(idx: &[u8], keys: &[String]) -> SmallVec<[String; 14]> {
    idx.iter()
        .filter_map(|&i| keys.get(i as usize).cloned())
        .collect()
}

#[inline(always)]
fn key_at(keys: &[String], i: usize) -> String {
    keys.get(i).cloned().unwrap_or_default()
}

#[inline(always)]
fn acs(a: &SmallVec<[String; 14]>, i: usize) -> String {
    a.get(i).cloned().unwrap_or_default()
}

#[inline(always)]
fn make_ev(
    received_at: Instant,
    slot: u64,
    signature: Option<String>,
    from_cpi: bool,
    is_backfill: bool,
    kind: ParsedEventKind,
) -> ParsedPumpEvent {
    ParsedPumpEvent {
        received_at,
        slot,
        signature,
        event_ordinal: None,
        provenance: None,
        from_cpi,
        is_backfill,
        kind,
    }
}

// ─── BinaryParser adapter ────────────────────────────────────────────────────
//
// Bridges the new PumpParser + ParsedPumpEvent architecture to the lib.rs
// expected API surface:
//   BinaryParser::new(verbose) → parser
//   parser.parse_initialize_pool(&GeyserEvent) → SeerResult<Option<InitializePoolEvent>>
//   parser.parse_trades(&GeyserEvent) → SeerResult<Vec<TradeEvent>>
//
// Internally:
//   1. Extracts raw proto bytes from GeyserEvent::Transaction.mpcf_payload_bytes
//   2. Wraps them in PumpEvent::Transaction
//   3. Calls PumpParser::parse()
//   4. Maps ParsedEventKind → InitializePoolEvent / TradeEvent

use crate::errors::SeerResult;
use crate::types::{
    GeyserEvent, InitializePoolEvent, ObservedAccountMetaProvenance, RawBytesMissingReason,
    TokenDelta, ToolchainFingerprintInput, TradeEvent,
};
use ghost_core::transaction_parser::ProgramIds;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::str::FromStr;

const COMPUTE_BUDGET_PROGRAM_ID: &str = "ComputeBudget111111111111111111111111111111";
const ASSOCIATED_TOKEN_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
const SYSTEM_PROGRAM_ID: &str = "11111111111111111111111111111111";
const JITO_TIP_ACCOUNTS: &[&str] = &[
    "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5",
    "HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe",
    "Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY",
    "ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49",
    "DfXygSm4jCyNCybVYYK6DwvWqjKee8pbDmJGcLWNDXjh",
    "ADuUkR4vqLUMWXxW9gh6D6L8pMSawimctcNZ5pGwDcEt",
    "DttWaMuVvTiduZRnguLF7jNxTgiMBZ1hyAumKUiL2KRL",
    "3AVi9Tg9Uo68tJfuvoKvqKNWKkC5wPdSSdeBnizKZ6jT",
];

#[derive(Debug, Clone, Default)]
struct RuntimeTradeContext {
    timestamp_ms: Option<u64>,
    event_time: ghost_core::EventTimeMetadata,
    success: bool,
    error_code: Option<String>,
    compute_units_consumed: Option<u64>,
    cu_price_micro_lamports: Option<u64>,
    compute_unit_limit: Option<u32>,
    inner_ix_count: Option<u32>,
    cpi_depth: Option<u32>,
    ata_create_count: Option<u32>,
    jito_tip_detected: Option<bool>,
    signer_pre_balance_lamports: HashMap<String, u64>,
    signer_post_balance_lamports: HashMap<String, u64>,
}

#[derive(Clone)]
pub struct BinaryParser {
    #[allow(dead_code)]
    verbose: bool,
    curve_mint_reg: CurveMintRegistry,
    account_reg: AccountRegistry,
    resolve_queue: ResolveQueue,
    complete_tracker: CompleteTracker,
}

impl BinaryParser {
    pub fn new(verbose: bool) -> Self {
        Self {
            verbose,
            curve_mint_reg: CurveMintRegistry::new(),
            account_reg: AccountRegistry::new(),
            resolve_queue: ResolveQueue::with_default_cap(),
            complete_tracker: CompleteTracker::new(),
        }
    }

    /// Parse a GeyserEvent for an InitializePool (Create) event.
    ///
    /// Returns Ok(None) if the event is not a pool initialization.
    pub fn parse_initialize_pool(
        &self,
        event: &GeyserEvent,
    ) -> SeerResult<Option<InitializePoolEvent>> {
        let event_time = crate::types::transaction_event_time(event);
        let event_ts_ms = event.compat_event_ts_ms();
        let parsed = self.parse_pump_events(event);
        // Priority: CpiCreate (Borsh event log, ec.user is always correct) >
        //           Create (direct instruction, account index may shift across Pump.fun versions) >
        //           SwapPoolCreated.
        // Collect all events first, then pick the highest-priority one.
        let mut direct_create: Option<ParsedPumpEvent> = None;
        let mut cpi_create: Option<ParsedPumpEvent> = None;
        let mut swap_pool_created: Option<ParsedPumpEvent> = None;
        for p in parsed {
            match p.kind {
                ParsedEventKind::CpiCreate(_) => {
                    cpi_create = Some(p);
                }
                ParsedEventKind::Create { .. } => {
                    if direct_create.is_none() {
                        direct_create = Some(p);
                    }
                }
                ParsedEventKind::SwapPoolCreated { .. } => {
                    if swap_pool_created.is_none() {
                        swap_pool_created = Some(p);
                    }
                }
                _ => {}
            }
        }
        let chosen = cpi_create.or(direct_create).or(swap_pool_created);
        if let Some(p) = chosen {
            match p.kind {
                ParsedEventKind::Create {
                    params: _params,
                    mint,
                    bonding_curve,
                    user,
                } => {
                    let sig = if let GeyserEvent::Transaction { ref signature, .. } = event {
                        *signature
                    } else {
                        solana_sdk::signature::Signature::default()
                    };

                    return Ok(Some(InitializePoolEvent {
                        slot: Some(p.slot),
                        event_ts_ms,
                        event_time,
                        signature: sig,
                        amm_program_id: Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap_or_default(),
                        pool_amm_id: Pubkey::from_str(&bonding_curve).unwrap_or_default(),
                        base_mint: Pubkey::from_str(&mint).unwrap_or_default(),
                        quote_mint: solana_sdk::pubkey!(
                            "So11111111111111111111111111111111111111112"
                        ),
                        bonding_curve: Pubkey::from_str(&bonding_curve).unwrap_or_default(),
                        creator: sanitize_creator_pubkey(
                            Pubkey::from_str(&user).unwrap_or_default(),
                        ),
                        initial_virtual_token_reserves: None,
                        initial_virtual_sol_reserves: None,
                        initial_real_token_reserves: None,
                        initial_real_sol_reserves: None,
                        token_total_supply: None,
                        block_time: None,
                        raw_data: vec![],
                    }));
                }
                ParsedEventKind::CpiCreate(ref ec) => {
                    let sig = if let GeyserEvent::Transaction { ref signature, .. } = event {
                        *signature
                    } else {
                        solana_sdk::signature::Signature::default()
                    };

                    return Ok(Some(InitializePoolEvent {
                        slot: Some(p.slot),
                        event_ts_ms,
                        event_time,
                        signature: sig,
                        amm_program_id: Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap_or_default(),
                        pool_amm_id: Pubkey::try_from(ec.bonding_curve.as_slice())
                            .unwrap_or_default(),
                        base_mint: Pubkey::try_from(ec.mint.as_slice()).unwrap_or_default(),
                        quote_mint: solana_sdk::pubkey!(
                            "So11111111111111111111111111111111111111112"
                        ),
                        bonding_curve: Pubkey::try_from(ec.bonding_curve.as_slice())
                            .unwrap_or_default(),
                        creator: sanitize_creator_pubkey(
                            Pubkey::try_from(ec.user.as_slice()).unwrap_or_default(),
                        ),
                        initial_virtual_token_reserves: None,
                        initial_virtual_sol_reserves: None,
                        initial_real_token_reserves: None,
                        initial_real_sol_reserves: None,
                        token_total_supply: None,
                        block_time: None,
                        raw_data: vec![],
                    }));
                }
                ParsedEventKind::SwapPoolCreated {
                    ref pool,
                    ref base_mint,
                    ref quote_mint,
                    ref creator,
                    ..
                } => {
                    let sig = if let GeyserEvent::Transaction { ref signature, .. } = event {
                        *signature
                    } else {
                        solana_sdk::signature::Signature::default()
                    };

                    // For PumpSwap pools there is no bonding_curve account.
                    // lib.rs uses bonding_curve as the key for curve→mint registry
                    // (CURVE_MAP_SET / CURVE_SUBSCRIBED).  Setting it to Pubkey::default()
                    // causes curve=11111...1111 (SystemProgram) to be registered, which then
                    // triggers CURVE_SEED_RPC_FAIL.
                    // Fix: use pool_amm_id as the bonding_curve key so the registry maps
                    // pool→mint correctly for PumpSwap.
                    let pool_pk = Pubkey::from_str(pool).unwrap_or_default();
                    return Ok(Some(InitializePoolEvent {
                        slot: Some(p.slot),
                        event_ts_ms,
                        event_time,
                        signature: sig,
                        amm_program_id: Pubkey::from_str(PUMP_SWAP_PROGRAM_ID).unwrap_or_default(),
                        pool_amm_id: pool_pk,
                        base_mint: Pubkey::from_str(base_mint).unwrap_or_default(),
                        quote_mint: Pubkey::from_str(quote_mint).unwrap_or_default(),
                        bonding_curve: pool_pk,
                        creator: sanitize_creator_pubkey(
                            Pubkey::from_str(creator).unwrap_or_default(),
                        ),
                        initial_virtual_token_reserves: None,
                        initial_virtual_sol_reserves: None,
                        initial_real_token_reserves: None,
                        initial_real_sol_reserves: None,
                        token_total_supply: None,
                        block_time: None,
                        raw_data: vec![],
                    }));
                }
                _ => {}
            }
        }
        Ok(None)
    }

    /// Parse a GeyserEvent for trade events.
    ///
    /// Returns all Buy/Sell trades found in the transaction.
    pub fn parse_trades(&self, event: &GeyserEvent) -> SeerResult<Vec<TradeEvent>> {
        let parsed = self.parse_pump_events(event);
        let mut trades = Vec::new();
        let runtime_ctx = extract_runtime_trade_context(event);
        let has_explicit_trade = parsed.iter().any(|p| {
            matches!(
                p.kind,
                ParsedEventKind::Trade { .. }
                    | ParsedEventKind::CpiTrade(_)
                    | ParsedEventKind::SwapTrade { .. }
                    | ParsedEventKind::CpiSwapBuy(_)
                    | ParsedEventKind::CpiSwapSell(_)
            )
        });

        let (sig, slot_val, arrival_ts) = if let GeyserEvent::Transaction {
            ref signature,
            slot,
            arrival_ts_ms,
            ..
        } = event
        {
            (
                *signature,
                *slot,
                arrival_ts_ms.unwrap_or_else(crate::types::arrival_time_ms),
            )
        } else {
            (solana_sdk::signature::Signature::default(), Some(0), 0)
        };

        for p in parsed {
            let event_ordinal = p.event_ordinal;
            let provenance = p.provenance.clone();
            let effective_runtime_ts_ms = runtime_ctx.timestamp_ms.unwrap_or(arrival_ts);
            match p.kind {
                ParsedEventKind::Trade {
                    side,
                    mint,
                    bonding_curve,
                    user,
                    global_config,
                    fee_recipient,
                    token_program,
                    token_amount,
                    sol_amount,
                    virtual_token_reserves,
                    virtual_sol_reserves,
                    real_token_reserves: _,
                    real_sol_reserves: _,
                    market_cap_sol,
                    progress: _,
                    is_complete: _,
                    ..
                } => {
                    trades.push(TradeEvent {
                        semantic: ghost_core::EventSemanticEnvelope::default(),
                        slot: slot_val,
                        signature: sig,
                        event_ordinal,
                        provenance: provenance.clone(),
                        timestamp_ms: effective_runtime_ts_ms,
                        arrival_ts_ms: arrival_ts,
                        event_time: runtime_ctx.event_time,
                        pool_amm_id: Pubkey::from_str(&bonding_curve).unwrap_or_default(),
                        mint: Pubkey::from_str(&mint).unwrap_or_default(),
                        signer: Pubkey::from_str(&user).unwrap_or_default(),
                        is_buy: side == TradeSide::Buy,
                        is_dev_buy: false,
                        amount: token_amount,
                        max_sol_cost: if side == TradeSide::Buy {
                            sol_amount
                        } else {
                            0
                        },
                        min_sol_output: if side == TradeSide::Sell {
                            sol_amount
                        } else {
                            0
                        },
                        success: runtime_ctx.success,
                        error_code: runtime_ctx.error_code.clone(),
                        compute_units_consumed: runtime_ctx.compute_units_consumed,
                        owner_token_deltas: resolve_owner_token_deltas(
                            event,
                            &Pubkey::from_str(&mint).unwrap_or_default(),
                        ),
                        mpcf_payload: vec![],
                        mpcf_payload_missing_reason: RawBytesMissingReason::FilteredByConfig,
                        v_tokens_in_bonding_curve: if virtual_token_reserves > 0 {
                            Some(virtual_token_reserves as f64 / 1e6)
                        } else {
                            None
                        },
                        v_sol_in_bonding_curve: if virtual_sol_reserves > 0 {
                            Some(virtual_sol_reserves as f64 / 1e9)
                        } else {
                            None
                        },
                        market_cap_sol: if market_cap_sol > 0.0 {
                            Some(market_cap_sol)
                        } else {
                            None
                        },
                        global_config,
                        fee_recipient,
                        token_program,
                        buy_variant: None,
                        associated_bonding_curve: None,
                        bonding_curve_v2: None,
                        bonding_curve_v2_provenance: None,
                        is_mayhem_mode: None,
                        cu_price_micro_lamports: runtime_ctx.cu_price_micro_lamports,
                        compute_unit_limit: runtime_ctx.compute_unit_limit,
                        inner_ix_count: runtime_ctx.inner_ix_count,
                        cpi_depth: runtime_ctx.cpi_depth,
                        ata_create_count: runtime_ctx.ata_create_count,
                        signer_pre_balance_lamports: runtime_ctx
                            .signer_pre_balance_lamports
                            .get(&user)
                            .copied(),
                        signer_post_balance_lamports: runtime_ctx
                            .signer_post_balance_lamports
                            .get(&user)
                            .copied(),
                        jito_tip_detected: runtime_ctx.jito_tip_detected,
                        toolchain_fingerprint: ToolchainFingerprintInput::default(),
                        curve_data_known: virtual_token_reserves > 0,
                        curve_finality: ghost_core::CurveFinality::from_curve_data_known(
                            virtual_token_reserves > 0,
                        ),
                        is_pumpswap: false,
                    });
                }
                ParsedEventKind::CpiTrade(ref et) => {
                    let mint_pk = Pubkey::try_from(et.mint.as_slice()).unwrap_or_default();
                    let user_pk = Pubkey::try_from(et.user.as_slice()).unwrap_or_default();
                    let bc = self
                        .curve_mint_reg
                        .curve_for_mint_pk(&mint_pk)
                        .unwrap_or_default();
                    trades.push(TradeEvent {
                        semantic: ghost_core::EventSemanticEnvelope::default(),
                        slot: slot_val,
                        signature: sig,
                        event_ordinal,
                        provenance: provenance.clone(),
                        timestamp_ms: effective_runtime_ts_ms,
                        arrival_ts_ms: arrival_ts,
                        event_time: runtime_ctx.event_time,
                        pool_amm_id: bc,
                        mint: mint_pk,
                        signer: user_pk,
                        is_buy: et.is_buy,
                        is_dev_buy: false,
                        amount: et.token_amount,
                        max_sol_cost: if et.is_buy { et.sol_amount } else { 0 },
                        min_sol_output: if !et.is_buy { et.sol_amount } else { 0 },
                        success: runtime_ctx.success,
                        error_code: runtime_ctx.error_code.clone(),
                        compute_units_consumed: runtime_ctx.compute_units_consumed,
                        owner_token_deltas: resolve_owner_token_deltas(event, &mint_pk),
                        mpcf_payload: vec![],
                        mpcf_payload_missing_reason: RawBytesMissingReason::FilteredByConfig,
                        v_tokens_in_bonding_curve: Some(et.virtual_token_reserves as f64 / 1e6),
                        v_sol_in_bonding_curve: Some(et.virtual_sol_reserves as f64 / 1e9),
                        market_cap_sol: None,
                        global_config: None,
                        fee_recipient: None,
                        token_program: None,
                        buy_variant: None,
                        associated_bonding_curve: None,
                        bonding_curve_v2: None,
                        bonding_curve_v2_provenance: None,
                        is_mayhem_mode: None,
                        cu_price_micro_lamports: runtime_ctx.cu_price_micro_lamports,
                        compute_unit_limit: runtime_ctx.compute_unit_limit,
                        inner_ix_count: runtime_ctx.inner_ix_count,
                        cpi_depth: runtime_ctx.cpi_depth,
                        ata_create_count: runtime_ctx.ata_create_count,
                        signer_pre_balance_lamports: runtime_ctx
                            .signer_pre_balance_lamports
                            .get(&user_pk.to_string())
                            .copied(),
                        signer_post_balance_lamports: runtime_ctx
                            .signer_post_balance_lamports
                            .get(&user_pk.to_string())
                            .copied(),
                        jito_tip_detected: runtime_ctx.jito_tip_detected,
                        toolchain_fingerprint: ToolchainFingerprintInput::default(),
                        curve_data_known: true,
                        curve_finality: ghost_core::CurveFinality::Provisional,
                        is_pumpswap: false,
                    });
                }
                ParsedEventKind::SwapTrade {
                    side,
                    ref pool,
                    ref base_mint,
                    quote_mint: _,
                    ref user,
                    base_amount,
                    quote_amount,
                } => {
                    let pool_pk = Pubkey::from_str(pool).unwrap_or_default();
                    let user_pk = Pubkey::from_str(user).unwrap_or_default();
                    // normalize_swap_pair already swapped base/quote amounts when WSOL is base,
                    // but `side` still reflects the raw on-chain discriminator.  Invert for
                    // WSOL-base pools: DISC_BUY = user buys WSOL (base) = TOKEN SELL.
                    let wsol_is_base = pumpswap_pool_wsol_is_base(event, &pool_pk);
                    let effective_is_buy = if wsol_is_base {
                        side != TradeSide::Buy
                    } else {
                        side == TradeSide::Buy
                    };
                    let base_mint_pk = resolve_trade_mint(
                        &self.curve_mint_reg,
                        event,
                        &pool_pk,
                        &user_pk,
                        if effective_is_buy {
                            TradeSide::Buy
                        } else {
                            TradeSide::Sell
                        },
                        Pubkey::from_str(base_mint).ok(),
                    );
                    trades.push(TradeEvent {
                        semantic: ghost_core::EventSemanticEnvelope::default(),
                        slot: slot_val,
                        signature: sig,
                        event_ordinal,
                        provenance: provenance.clone(),
                        timestamp_ms: effective_runtime_ts_ms,
                        arrival_ts_ms: arrival_ts,
                        event_time: runtime_ctx.event_time,
                        pool_amm_id: pool_pk,
                        mint: base_mint_pk,
                        signer: user_pk,
                        is_buy: effective_is_buy,
                        is_dev_buy: false,
                        amount: base_amount,
                        max_sol_cost: if effective_is_buy { quote_amount } else { 0 },
                        min_sol_output: if effective_is_buy { 0 } else { quote_amount },
                        success: runtime_ctx.success,
                        error_code: runtime_ctx.error_code.clone(),
                        compute_units_consumed: runtime_ctx.compute_units_consumed,
                        owner_token_deltas: resolve_owner_token_deltas(event, &base_mint_pk),
                        mpcf_payload: vec![],
                        mpcf_payload_missing_reason: RawBytesMissingReason::FilteredByConfig,
                        v_tokens_in_bonding_curve: None,
                        v_sol_in_bonding_curve: None,
                        market_cap_sol: None,
                        global_config: None,
                        fee_recipient: None,
                        token_program: None,
                        buy_variant: None,
                        associated_bonding_curve: None,
                        bonding_curve_v2: None,
                        bonding_curve_v2_provenance: None,
                        is_mayhem_mode: None,
                        cu_price_micro_lamports: runtime_ctx.cu_price_micro_lamports,
                        compute_unit_limit: runtime_ctx.compute_unit_limit,
                        inner_ix_count: runtime_ctx.inner_ix_count,
                        cpi_depth: runtime_ctx.cpi_depth,
                        ata_create_count: runtime_ctx.ata_create_count,
                        signer_pre_balance_lamports: runtime_ctx
                            .signer_pre_balance_lamports
                            .get(user)
                            .copied(),
                        signer_post_balance_lamports: runtime_ctx
                            .signer_post_balance_lamports
                            .get(user)
                            .copied(),
                        jito_tip_detected: runtime_ctx.jito_tip_detected,
                        toolchain_fingerprint: ToolchainFingerprintInput::default(),
                        curve_data_known: false,
                        curve_finality: ghost_core::CurveFinality::Speculative,
                        is_pumpswap: true,
                    });
                }
                ParsedEventKind::SwapPoolCreated {
                    ref pool,
                    ref base_mint,
                    ref creator,
                    base_amount_in,
                    quote_amount_in,
                    ..
                } => {
                    if has_explicit_trade {
                        continue;
                    }

                    let (Some(token_amount), Some(sol_amount)) = (
                        base_amount_in.filter(|value| *value > 0),
                        quote_amount_in.filter(|value| *value > 0),
                    ) else {
                        continue;
                    };

                    let pool_pk = Pubkey::from_str(pool).unwrap_or_default();
                    let mint_pk = Pubkey::from_str(base_mint).unwrap_or_default();
                    let creator_pk = Pubkey::from_str(creator).unwrap_or_default();

                    trades.push(TradeEvent {
                        semantic: ghost_core::EventSemanticEnvelope::default(),
                        slot: slot_val,
                        signature: sig,
                        event_ordinal,
                        provenance: provenance.clone(),
                        timestamp_ms: effective_runtime_ts_ms,
                        arrival_ts_ms: arrival_ts,
                        event_time: runtime_ctx.event_time,
                        pool_amm_id: pool_pk,
                        mint: mint_pk,
                        signer: creator_pk,
                        is_buy: true,
                        is_dev_buy: true,
                        amount: token_amount,
                        max_sol_cost: sol_amount,
                        min_sol_output: 0,
                        success: runtime_ctx.success,
                        error_code: runtime_ctx.error_code.clone(),
                        compute_units_consumed: runtime_ctx.compute_units_consumed,
                        owner_token_deltas: resolve_owner_token_deltas(event, &mint_pk),
                        mpcf_payload: vec![],
                        mpcf_payload_missing_reason: RawBytesMissingReason::FilteredByConfig,
                        v_tokens_in_bonding_curve: None,
                        v_sol_in_bonding_curve: None,
                        market_cap_sol: None,
                        global_config: None,
                        fee_recipient: None,
                        token_program: None,
                        buy_variant: None,
                        associated_bonding_curve: None,
                        bonding_curve_v2: None,
                        bonding_curve_v2_provenance: None,
                        is_mayhem_mode: None,
                        cu_price_micro_lamports: runtime_ctx.cu_price_micro_lamports,
                        compute_unit_limit: runtime_ctx.compute_unit_limit,
                        inner_ix_count: runtime_ctx.inner_ix_count,
                        cpi_depth: runtime_ctx.cpi_depth,
                        ata_create_count: runtime_ctx.ata_create_count,
                        signer_pre_balance_lamports: runtime_ctx
                            .signer_pre_balance_lamports
                            .get(creator)
                            .copied(),
                        signer_post_balance_lamports: runtime_ctx
                            .signer_post_balance_lamports
                            .get(creator)
                            .copied(),
                        jito_tip_detected: runtime_ctx.jito_tip_detected,
                        toolchain_fingerprint: ToolchainFingerprintInput::default(),
                        curve_data_known: false,
                        curve_finality: ghost_core::CurveFinality::Speculative,
                        is_pumpswap: true,
                    });
                }
                // PumpSwap BuyEvent — highest-fidelity source for AMM buys.
                // mint is resolved downstream via pool→mint registry (lib.rs set_curve_mapping).
                ParsedEventKind::CpiSwapBuy(ref e) => {
                    let pool = Pubkey::try_from(e.pool.as_slice()).unwrap_or_default();
                    let user = Pubkey::try_from(e.user.as_slice()).unwrap_or_default();
                    // For WSOL-base pools the on-chain "base" is WSOL and "quote" is the token,
                    // so field semantics are inverted: base_* = SOL, quote_* = token.
                    // DISC_BUY with WSOL-base = user pays quote(tokens) to get base(WSOL) = TOKEN SELL.
                    let wsol_is_base = pumpswap_pool_wsol_is_base(event, &pool);
                    let inferred = infer_signer_swap_from_balances(event, &user);
                    let (is_buy, token_amount, sol_amount) = if wsol_is_base {
                        (false, e.quote_amount_in, e.base_amount_out)
                    } else {
                        (true, e.base_amount_out, e.quote_amount_in)
                    };
                    let (v_tok, v_sol) = if wsol_is_base {
                        (
                            (e.pool_quote_token_reserves > 0)
                                .then_some(e.pool_quote_token_reserves as f64 / 1e6),
                            (e.pool_base_token_reserves > 0)
                                .then_some(e.pool_base_token_reserves as f64 / 1e9),
                        )
                    } else {
                        (
                            (e.pool_base_token_reserves > 0)
                                .then_some(e.pool_base_token_reserves as f64 / 1e6),
                            (e.pool_quote_token_reserves > 0)
                                .then_some(e.pool_quote_token_reserves as f64 / 1e9),
                        )
                    };
                    let trade_side = if is_buy {
                        TradeSide::Buy
                    } else {
                        TradeSide::Sell
                    };
                    let resolved_mint = resolve_trade_mint(
                        &self.curve_mint_reg,
                        event,
                        &pool,
                        &user,
                        trade_side,
                        None,
                    );
                    let mint = if resolved_mint != Pubkey::default() {
                        resolved_mint
                    } else {
                        inferred
                            .map(|trade| trade.mint)
                            .or_else(|| resolve_trade_mint_from_event_accounts(event))
                            .unwrap_or_default()
                    };
                    if mint != Pubkey::default()
                        && self.curve_mint_reg.mint_for_curve_pk(&pool).is_none()
                    {
                        self.curve_mint_reg.insert_pk(&pool, &mint);
                    }
                    trades.push(TradeEvent {
                        semantic: ghost_core::EventSemanticEnvelope::default(),
                        slot: slot_val,
                        signature: sig,
                        event_ordinal,
                        provenance: provenance.clone(),
                        timestamp_ms: effective_runtime_ts_ms,
                        arrival_ts_ms: arrival_ts,
                        event_time: runtime_ctx.event_time,
                        pool_amm_id: pool,
                        mint,
                        signer: user,
                        is_buy,
                        is_dev_buy: false,
                        amount: token_amount,
                        max_sol_cost: if is_buy { sol_amount } else { 0 },
                        min_sol_output: if is_buy { 0 } else { sol_amount },
                        success: runtime_ctx.success,
                        error_code: runtime_ctx.error_code.clone(),
                        compute_units_consumed: runtime_ctx.compute_units_consumed,
                        owner_token_deltas: resolve_owner_token_deltas(event, &mint),
                        mpcf_payload: vec![],
                        mpcf_payload_missing_reason: RawBytesMissingReason::FilteredByConfig,
                        v_tokens_in_bonding_curve: v_tok,
                        v_sol_in_bonding_curve: v_sol,
                        market_cap_sol: None,
                        global_config: None,
                        fee_recipient: None,
                        token_program: None,
                        buy_variant: None,
                        associated_bonding_curve: None,
                        bonding_curve_v2: None,
                        bonding_curve_v2_provenance: None,
                        is_mayhem_mode: None,
                        cu_price_micro_lamports: runtime_ctx.cu_price_micro_lamports,
                        compute_unit_limit: runtime_ctx.compute_unit_limit,
                        inner_ix_count: runtime_ctx.inner_ix_count,
                        cpi_depth: runtime_ctx.cpi_depth,
                        ata_create_count: runtime_ctx.ata_create_count,
                        signer_pre_balance_lamports: runtime_ctx
                            .signer_pre_balance_lamports
                            .get(&user.to_string())
                            .copied(),
                        signer_post_balance_lamports: runtime_ctx
                            .signer_post_balance_lamports
                            .get(&user.to_string())
                            .copied(),
                        jito_tip_detected: runtime_ctx.jito_tip_detected,
                        toolchain_fingerprint: ToolchainFingerprintInput::default(),
                        curve_data_known: e.pool_base_token_reserves > 0,
                        curve_finality: ghost_core::CurveFinality::from_curve_data_known(
                            e.pool_base_token_reserves > 0,
                        ),
                        is_pumpswap: true,
                    });
                }
                // PumpSwap SellEvent — sell side mirror of BuyEvent.
                ParsedEventKind::CpiSwapSell(ref e) => {
                    let pool = Pubkey::try_from(e.pool.as_slice()).unwrap_or_default();
                    let user = Pubkey::try_from(e.user.as_slice()).unwrap_or_default();
                    // For WSOL-base pools: base_* = SOL, quote_* = token (inverted layout).
                    // DISC_SELL with WSOL-base = user sells base(WSOL) to get quote(tokens) = TOKEN BUY.
                    let wsol_is_base = pumpswap_pool_wsol_is_base(event, &pool);
                    let inferred = infer_signer_swap_from_balances(event, &user);
                    let (is_buy, token_amount, sol_amount) = if wsol_is_base {
                        (true, e.quote_amount_out, e.base_amount_in)
                    } else {
                        (false, e.base_amount_in, e.quote_amount_out)
                    };
                    let (v_tok, v_sol) = if wsol_is_base {
                        (
                            (e.pool_quote_token_reserves > 0)
                                .then_some(e.pool_quote_token_reserves as f64 / 1e6),
                            (e.pool_base_token_reserves > 0)
                                .then_some(e.pool_base_token_reserves as f64 / 1e9),
                        )
                    } else {
                        (
                            (e.pool_base_token_reserves > 0)
                                .then_some(e.pool_base_token_reserves as f64 / 1e6),
                            (e.pool_quote_token_reserves > 0)
                                .then_some(e.pool_quote_token_reserves as f64 / 1e9),
                        )
                    };
                    let trade_side = if is_buy {
                        TradeSide::Buy
                    } else {
                        TradeSide::Sell
                    };
                    let resolved_mint = resolve_trade_mint(
                        &self.curve_mint_reg,
                        event,
                        &pool,
                        &user,
                        trade_side,
                        None,
                    );
                    let mint = if resolved_mint != Pubkey::default() {
                        resolved_mint
                    } else {
                        inferred
                            .map(|trade| trade.mint)
                            .or_else(|| resolve_trade_mint_from_event_accounts(event))
                            .unwrap_or_default()
                    };
                    if mint != Pubkey::default()
                        && self.curve_mint_reg.mint_for_curve_pk(&pool).is_none()
                    {
                        self.curve_mint_reg.insert_pk(&pool, &mint);
                    }
                    trades.push(TradeEvent {
                        semantic: ghost_core::EventSemanticEnvelope::default(),
                        slot: slot_val,
                        signature: sig,
                        event_ordinal,
                        provenance: provenance.clone(),
                        timestamp_ms: effective_runtime_ts_ms,
                        arrival_ts_ms: arrival_ts,
                        event_time: runtime_ctx.event_time,
                        pool_amm_id: pool,
                        mint,
                        signer: user,
                        is_buy,
                        is_dev_buy: false,
                        amount: token_amount,
                        max_sol_cost: if is_buy { sol_amount } else { 0 },
                        min_sol_output: if is_buy { 0 } else { sol_amount },
                        success: runtime_ctx.success,
                        error_code: runtime_ctx.error_code.clone(),
                        compute_units_consumed: runtime_ctx.compute_units_consumed,
                        owner_token_deltas: resolve_owner_token_deltas(event, &mint),
                        mpcf_payload: vec![],
                        mpcf_payload_missing_reason: RawBytesMissingReason::FilteredByConfig,
                        v_tokens_in_bonding_curve: v_tok,
                        v_sol_in_bonding_curve: v_sol,
                        market_cap_sol: None,
                        global_config: None,
                        fee_recipient: None,
                        token_program: None,
                        buy_variant: None,
                        associated_bonding_curve: None,
                        bonding_curve_v2: None,
                        bonding_curve_v2_provenance: None,
                        is_mayhem_mode: None,
                        cu_price_micro_lamports: runtime_ctx.cu_price_micro_lamports,
                        compute_unit_limit: runtime_ctx.compute_unit_limit,
                        inner_ix_count: runtime_ctx.inner_ix_count,
                        cpi_depth: runtime_ctx.cpi_depth,
                        ata_create_count: runtime_ctx.ata_create_count,
                        signer_pre_balance_lamports: runtime_ctx
                            .signer_pre_balance_lamports
                            .get(&user.to_string())
                            .copied(),
                        signer_post_balance_lamports: runtime_ctx
                            .signer_post_balance_lamports
                            .get(&user.to_string())
                            .copied(),
                        jito_tip_detected: runtime_ctx.jito_tip_detected,
                        toolchain_fingerprint: ToolchainFingerprintInput::default(),
                        curve_data_known: e.pool_base_token_reserves > 0,
                        curve_finality: ghost_core::CurveFinality::from_curve_data_known(
                            e.pool_base_token_reserves > 0,
                        ),
                        is_pumpswap: true,
                    });
                }
                _ => {}
            }
        }
        if trades.is_empty() {
            trades.extend(self.parse_supported_router_outer_trades(
                event,
                &runtime_ctx,
                sig,
                slot_val,
                arrival_ts,
            ));
        }
        let mut deduped = dedup_trade_candidates(&self.curve_mint_reg, trades);
        for trade in &mut deduped {
            enrich_trade_optional_accounts_from_source_ix(event, trade);
            populate_trade_toolchain_fingerprint_from_source_tx(event, trade);
        }
        Ok(deduped)
    }

    fn parse_supported_router_outer_trades(
        &self,
        event: &GeyserEvent,
        runtime_ctx: &RuntimeTradeContext,
        signature: solana_sdk::signature::Signature,
        slot_val: Option<u64>,
        arrival_ts: u64,
    ) -> Vec<TradeEvent> {
        let GeyserEvent::Transaction {
            accounts,
            instructions,
            ..
        } = event
        else {
            return Vec::new();
        };

        let effective_runtime_ts_ms = runtime_ctx.timestamp_ms.unwrap_or(arrival_ts);
        let account_lanes = self.account_reg.snapshot_by_lane();
        let mut trades = Vec::new();

        for (outer_instruction_index, ix) in instructions.iter().enumerate() {
            let Some(hint) =
                decode_supported_router_trade_hint(accounts, ix, outer_instruction_index as u32)
            else {
                continue;
            };
            let Some(inferred) = infer_signer_swap_from_balances(event, &hint.signer) else {
                continue;
            };
            let Some(mint) = choose_router_trade_mint(hint.explicit_mint, inferred.mint) else {
                continue;
            };

            // Safety rail: Jupiter / DFlow are generic routers. Only emit a trade
            // candidate when the inferred mint is already known to belong to our
            // pump.fun / PumpSwap registry. This avoids turning arbitrary router
            // traffic into false pump trades.
            let Some(pool_amm_id) = self
                .curve_mint_reg
                .curve_for_mint_pk(&mint)
                .filter(|pool| *pool != Pubkey::default())
            else {
                continue;
            };

            let pool_key = pool_amm_id.to_string();
            let is_pumpswap = account_lanes.pool_accounts.binary_search(&pool_key).is_ok();

            trades.push(TradeEvent {
                semantic: ghost_core::EventSemanticEnvelope::default(),
                slot: slot_val,
                signature,
                event_ordinal: Some(hint.outer_instruction_index),
                provenance: Some(InstructionProvenance {
                    outer_instruction_index: Some(hint.outer_instruction_index),
                    inner_group_index: None,
                    outer_program_id: None,
                    invoked_program_id: hint.router_program.to_string(),
                    stack_height: None,
                    from_cpi: false,
                }),
                timestamp_ms: effective_runtime_ts_ms,
                arrival_ts_ms: arrival_ts,
                event_time: runtime_ctx.event_time,
                pool_amm_id,
                mint,
                signer: hint.signer,
                is_buy: inferred.is_buy,
                is_dev_buy: false,
                amount: inferred.token_amount,
                max_sol_cost: if inferred.is_buy {
                    inferred.sol_amount
                } else {
                    0
                },
                min_sol_output: if inferred.is_buy {
                    0
                } else {
                    inferred.sol_amount
                },
                success: runtime_ctx.success,
                error_code: runtime_ctx.error_code.clone(),
                compute_units_consumed: runtime_ctx.compute_units_consumed,
                owner_token_deltas: resolve_owner_token_deltas(event, &mint),
                mpcf_payload: vec![],
                mpcf_payload_missing_reason: RawBytesMissingReason::FilteredByConfig,
                v_tokens_in_bonding_curve: None,
                v_sol_in_bonding_curve: None,
                market_cap_sol: None,
                global_config: None,
                fee_recipient: None,
                token_program: None,
                buy_variant: None,
                associated_bonding_curve: None,
                bonding_curve_v2: None,
                bonding_curve_v2_provenance: None,
                is_mayhem_mode: None,
                cu_price_micro_lamports: runtime_ctx.cu_price_micro_lamports,
                compute_unit_limit: runtime_ctx.compute_unit_limit,
                inner_ix_count: runtime_ctx.inner_ix_count,
                cpi_depth: runtime_ctx.cpi_depth,
                ata_create_count: runtime_ctx.ata_create_count,
                signer_pre_balance_lamports: runtime_ctx
                    .signer_pre_balance_lamports
                    .get(&hint.signer.to_string())
                    .copied(),
                signer_post_balance_lamports: runtime_ctx
                    .signer_post_balance_lamports
                    .get(&hint.signer.to_string())
                    .copied(),
                jito_tip_detected: runtime_ctx.jito_tip_detected,
                toolchain_fingerprint: ToolchainFingerprintInput::default(),
                curve_data_known: false,
                curve_finality: ghost_core::CurveFinality::Speculative,
                is_pumpswap,
            });
        }

        trades
    }

    fn parse_transaction_from_decoded(
        &self,
        signature: &solana_sdk::signature::Signature,
        slot: Option<u64>,
        accounts: &[Pubkey],
        instructions: &[crate::types::RawInstruction],
        inner_instructions: &[crate::types::InnerInstructionGroup],
        pre_balances: &[u64],
        post_balances: &[u64],
        pre_token_balances: &[crate::types::RawTokenBalance],
        post_token_balances: &[crate::types::RawTokenBalance],
    ) -> Vec<ParsedPumpEvent> {
        let slot = slot.unwrap_or_default();
        let received_at = Instant::now();
        let signature = Some(signature.to_string());
        let all_keys: Vec<String> = accounts.iter().map(ToString::to_string).collect();
        let mut out = Vec::with_capacity(6);
        let mut instruction_ordinal: u32 = 0;

        for (outer_instruction_index, ix) in instructions.iter().enumerate() {
            let before = out.len();
            let prog = ix.program_id.to_string();
            if !is_pump_program(&prog) {
                instruction_ordinal = instruction_ordinal.saturating_add(1);
                continue;
            }

            let ix_accounts = resolve_accounts(&ix.account_indices, &all_keys);
            PumpParser::decode_ix(
                &ix.data,
                &ix_accounts,
                &prog,
                slot,
                signature.clone(),
                received_at,
                false,
                false,
                pre_balances,
                post_balances,
                pre_token_balances,
                post_token_balances,
                &all_keys,
                &self.curve_mint_reg,
                &self.account_reg,
                &mut out,
            );
            let provenance = top_level_provenance(outer_instruction_index as u32, &prog);
            stamp_new_events(&mut out, before, instruction_ordinal, &provenance);
            instruction_ordinal = instruction_ordinal.saturating_add(1);
        }

        let outer_instruction_count = instructions.len();
        for group in inner_instructions {
            // Track stack heights so that a CPI event log emitted from within an
            // instruction (higher stack_height) shares the *same* ordinal as its
            // parent instruction.
            //
            // Important detail: we must avoid incrementing the ordinal *after* a
            // parent when the next inner instruction is a deeper child. Using a
            // simple prev→curr comparison increments too early. We instead look
            // ahead at the next stack_height and only advance the ordinal when the
            // next instruction is not deeper than the current one.
            let outer_program = instructions
                .get(group.index as usize)
                .map(|ix| ix.program_id.to_string());
            if outer_program.is_none() {
                record_orphan_inner_group(
                    "decoded",
                    signature.as_deref(),
                    group.index,
                    outer_instruction_count,
                );
            }
            let insts = &group.instructions;
            for (i, inner_ix) in insts.iter().enumerate() {
                let curr_sh = inner_ix.stack_height;
                let next_sh = insts.get(i + 1).and_then(|ix| ix.stack_height);
                let has_deeper_next = matches!((curr_sh, next_sh), (Some(c), Some(n)) if n > c);

                let before = out.len();
                let prog = key_at(&all_keys, inner_ix.program_id_index as usize);
                if !is_pump_program(&prog) {
                    if !has_deeper_next {
                        instruction_ordinal = instruction_ordinal.saturating_add(1);
                    }
                    continue;
                }

                record_missing_inner_provenance(
                    "decoded",
                    signature.as_deref(),
                    group.index,
                    outer_program.as_deref(),
                    &prog,
                );
                let ix_accounts = resolve_accounts(&inner_ix.accounts, &all_keys);
                PumpParser::decode_ix(
                    &inner_ix.data,
                    &ix_accounts,
                    &prog,
                    slot,
                    signature.clone(),
                    received_at,
                    false,
                    true,
                    pre_balances,
                    post_balances,
                    pre_token_balances,
                    post_token_balances,
                    &all_keys,
                    &self.curve_mint_reg,
                    &self.account_reg,
                    &mut out,
                );

                if let Some(ev) = PumpParser::try_decode_cpi_event(
                    &inner_ix.data,
                    slot,
                    signature.clone(),
                    received_at,
                    false,
                ) {
                    if let ParsedEventKind::CpiCreate(ref ec) = ev.kind {
                        let curve = bs58::encode(&ec.bonding_curve).into_string();
                        let mint = bs58::encode(&ec.mint).into_string();
                        self.curve_mint_reg.insert(curve.clone(), mint.clone());
                        self.account_reg.insert_curve(curve);
                        self.account_reg.insert_mint(mint);
                        let _ = self.resolve_queue.drain_resolved(&self.curve_mint_reg);
                    }
                    if let ParsedEventKind::CpiComplete(ref ec) = ev.kind {
                        out.push(ParsedPumpEvent {
                            received_at,
                            slot,
                            signature: signature.clone(),
                            event_ordinal: Some(instruction_ordinal),
                            provenance: None,
                            from_cpi: true,
                            is_backfill: false,
                            kind: ParsedEventKind::MigrateReady {
                                mint: bs58::encode(&ec.mint).into_string(),
                                bonding_curve: bs58::encode(&ec.bonding_curve).into_string(),
                                timestamp: Some(ec.timestamp),
                            },
                        });
                    }
                    let mut ev = ev;
                    ev.event_ordinal = Some(instruction_ordinal);
                    out.push(ev);
                }
                let provenance = inner_instruction_provenance(
                    group.index,
                    outer_program.as_deref(),
                    &prog,
                    inner_ix.stack_height,
                );
                stamp_new_events(&mut out, before, instruction_ordinal, &provenance);
                if !has_deeper_next {
                    instruction_ordinal = instruction_ordinal.saturating_add(1);
                }
            }
        }

        dedup_trade_events(&mut out, &self.curve_mint_reg);

        out
    }

    /// Internal: extract raw proto bytes from GeyserEvent and run through PumpParser.
    ///
    /// RC-1.5: avoids a second Vec<u8> allocation by passing the raw bytes slice
    /// directly to `parse_transaction_raw` instead of re-wrapping in a PumpEvent
    /// (which would clone the bytes). The proto is still decoded once inside
    /// `parse_transaction_raw`; a full elimination of the second decode requires
    /// restructuring PumpParser to accept pre-decoded fields, left as a future P1.
    fn parse_pump_events(&self, event: &GeyserEvent) -> Vec<ParsedPumpEvent> {
        match event {
            GeyserEvent::Transaction {
                mpcf_payload_bytes: Some(raw),
                signature,
                slot,
                ..
            } => {
                let sig_str = signature.to_string();
                // Pass raw bytes slice directly — no clone.
                PumpParser::parse_transaction_raw(
                    raw,
                    Some(&sig_str),
                    slot.unwrap_or(0),
                    Instant::now(),
                    false,
                    &self.curve_mint_reg,
                    &self.account_reg,
                    &self.resolve_queue,
                )
            }
            GeyserEvent::Transaction {
                mpcf_payload_bytes: None,
                signature,
                slot,
                accounts,
                instructions,
                inner_instructions,
                pre_balances,
                post_balances,
                pre_token_balances,
                post_token_balances,
                ..
            } => self.parse_transaction_from_decoded(
                signature,
                *slot,
                accounts,
                instructions,
                inner_instructions,
                pre_balances,
                post_balances,
                pre_token_balances,
                post_token_balances,
            ),
            _ => vec![],
        }
    }

    /// Get the CurveMintRegistry (for integration with lib.rs curve tracking).
    pub fn curve_mint_registry(&self) -> &CurveMintRegistry {
        &self.curve_mint_reg
    }

    /// Forward a curve→mint mapping from lib.rs into the parser's internal registry.
    ///
    /// Called by lib.rs after `set_curve_mapping()` so that future `parse_trades()`
    /// calls can resolve `CpiTrade.curve_for_mint()` without RPC.
    pub fn set_curve_mapping(&self, curve: &str, mint: &str) {
        self.curve_mint_reg.insert(curve, mint);
    }

    /// Get the AccountRegistry (for integration with GrpcConnection).
    pub fn account_registry(&self) -> &AccountRegistry {
        &self.account_reg
    }

    /// Scan raw entry bytes for embedded CPI Create events.
    ///
    /// Returns `(curve, mint, creator)` tuples as base58 strings for any
    /// CpiCreate events found in the entry data.  Standard Yellowstone entries
    /// never contain CPI data, so this returns an empty Vec on mainstream nodes.
    pub fn scan_entry_cpi_creates(&self, raw: &[u8], slot: u64) -> Vec<(String, String, String)> {
        let events = PumpParser::parse_entry_raw(raw, slot, Instant::now(), 0);
        events
            .into_iter()
            .filter_map(|ev| {
                if let ParsedEventKind::CpiCreate(ec) = ev.kind {
                    Some((
                        bs58::encode(&ec.bonding_curve).into_string(),
                        bs58::encode(&ec.mint).into_string(),
                        bs58::encode(&ec.user).into_string(),
                    ))
                } else {
                    None
                }
            })
            .collect()
    }
}

fn extract_runtime_trade_context(event: &GeyserEvent) -> RuntimeTradeContext {
    let timestamp_ms = event.compat_event_ts_ms();
    let GeyserEvent::Transaction {
        accounts,
        instructions,
        pre_balances,
        post_balances,
        compute_units_consumed,
        success,
        error_code,
        inner_instructions,
        ..
    } = event
    else {
        return RuntimeTradeContext::default();
    };

    let (compute_unit_limit, cu_price_micro_lamports, jito_tip_detected) =
        extract_compute_and_jito_profile(accounts, instructions);
    let (inner_ix_count, cpi_depth, ata_create_count) =
        extract_inner_instruction_stats(inner_instructions);

    let mut signer_pre_balance_lamports = HashMap::new();
    for (account, lamports) in accounts.iter().zip(pre_balances.iter()) {
        if is_candidate_owner(account) {
            signer_pre_balance_lamports.insert(account.to_string(), *lamports);
        }
    }

    let mut signer_post_balance_lamports = HashMap::new();
    for (account, lamports) in accounts.iter().zip(post_balances.iter()) {
        if is_candidate_owner(account) {
            signer_post_balance_lamports.insert(account.to_string(), *lamports);
        }
    }

    RuntimeTradeContext {
        timestamp_ms,
        event_time: crate::types::transaction_event_time(event),
        success: *success,
        error_code: error_code.clone(),
        compute_units_consumed: *compute_units_consumed,
        cu_price_micro_lamports,
        compute_unit_limit,
        inner_ix_count,
        cpi_depth,
        ata_create_count,
        jito_tip_detected,
        signer_pre_balance_lamports,
        signer_post_balance_lamports,
    }
}

/// Single-pass extraction of compute budget profile AND jito tip flag.
///
/// Previously two separate loops over `instructions` (`extract_compute_budget_profile`
/// + `detect_jito_tip`). Merged into one pass to avoid iterating the same slice twice.
///
/// Jito detection: System Program Transfer discriminator is `[2, 0, 0, 0]` (u32 LE).
/// Checking raw bytes is sufficient — no need for bincode::deserialize<SystemInstruction>.
fn extract_compute_and_jito_profile(
    accounts: &[Pubkey],
    instructions: &[crate::types::RawInstruction],
) -> (Option<u32>, Option<u64>, Option<bool>) {
    if instructions.is_empty() {
        return (None, None, None);
    }

    let mut cu_limit = None;
    let mut cu_price = None;
    let mut jito_detected = false;

    for instruction in instructions {
        let prog = &instruction.program_id;
        let prog_str = prog.to_string();

        if prog_str == COMPUTE_BUDGET_PROGRAM_ID {
            match instruction.data.first().copied() {
                Some(2) if instruction.data.len() >= 5 => {
                    cu_limit = Some(u32::from_le_bytes(
                        instruction.data[1..5].try_into().unwrap(),
                    ));
                }
                Some(3) if instruction.data.len() >= 9 => {
                    cu_price = Some(u64::from_le_bytes(
                        instruction.data[1..9].try_into().unwrap(),
                    ));
                }
                _ => {}
            }
        } else if !jito_detected && prog_str == SYSTEM_PROGRAM_ID {
            // System Program Transfer discriminator = 2u32 LE = [2, 0, 0, 0]
            if instruction.data.get(..4) == Some(&[2, 0, 0, 0]) {
                if let Some(&dest_idx) = instruction.account_indices.get(1) {
                    if let Some(dest) = accounts.get(dest_idx as usize) {
                        if JITO_TIP_ACCOUNTS.contains(&dest.to_string().as_str()) {
                            jito_detected = true;
                        }
                    }
                }
            }
        }
    }

    (cu_limit, cu_price, Some(jito_detected))
}

fn extract_inner_instruction_stats(
    inner_instructions: &[crate::types::InnerInstructionGroup],
) -> (Option<u32>, Option<u32>, Option<u32>) {
    if inner_instructions.is_empty() {
        return (None, None, None);
    }

    let mut ix_count = 0u32;
    let mut max_depth = 0u32;
    let mut ata_create_count = 0u32;
    for group in inner_instructions {
        ix_count = ix_count.saturating_add(group.instructions.len() as u32);
        for instruction in &group.instructions {
            max_depth = max_depth.max(instruction.stack_height.unwrap_or(0));
            if instruction.program_id_index as usize == usize::MAX {
                continue;
            }
        }
        if group.instructions.len() == 4 {
            ata_create_count = ata_create_count.saturating_add(1);
        }
    }

    (Some(ix_count), Some(max_depth), Some(ata_create_count))
}

fn resolve_owner_token_deltas(event: &GeyserEvent, mint: &Pubkey) -> Vec<TokenDelta> {
    let GeyserEvent::Transaction {
        accounts,
        pre_token_balances,
        post_token_balances,
        ..
    } = event
    else {
        return Vec::new();
    };

    let mint_str = mint.to_string();
    let mut by_account: HashMap<u32, (u64, u64)> = HashMap::new();
    for balance in pre_token_balances
        .iter()
        .filter(|balance| balance.mint == mint_str)
    {
        by_account.entry(balance.account_index).or_insert((0, 0)).0 = balance.amount;
    }
    for balance in post_token_balances
        .iter()
        .filter(|balance| balance.mint == mint_str)
    {
        by_account.entry(balance.account_index).or_insert((0, 0)).1 = balance.amount;
    }

    let mut owner_deltas: HashMap<String, i128> = HashMap::new();
    for (account_index, (pre, post)) in by_account {
        if pre == post {
            continue;
        }
        let Some(token_account) = accounts.get(account_index as usize) else {
            continue;
        };
        let owner_hint = pre_token_balances
            .iter()
            .chain(post_token_balances.iter())
            .find(|balance| balance.account_index == account_index);
        let Some(owner) = resolve_token_balance_owner(owner_hint, accounts, token_account, mint)
        else {
            continue;
        };
        *owner_deltas.entry(owner.to_string()).or_insert(0) += post as i128 - pre as i128;
    }

    owner_deltas
        .into_iter()
        .filter(|(_, delta_raw)| *delta_raw != 0)
        .map(|(owner, delta_raw)| TokenDelta {
            owner,
            delta_raw,
            decimals: 6,
        })
        .collect()
}

fn infer_signer_trade_mint_from_balances(
    event: &GeyserEvent,
    signer: &Pubkey,
    side: TradeSide,
) -> Option<Pubkey> {
    let GeyserEvent::Transaction {
        accounts,
        pre_token_balances,
        post_token_balances,
        ..
    } = event
    else {
        return None;
    };

    if *signer == Pubkey::default() {
        return None;
    }

    let mut by_mint_and_account: HashMap<(String, u32), (u64, u64)> = HashMap::new();
    for balance in pre_token_balances {
        by_mint_and_account
            .entry((balance.mint.clone(), balance.account_index))
            .or_insert((0, 0))
            .0 = balance.amount;
    }
    for balance in post_token_balances {
        by_mint_and_account
            .entry((balance.mint.clone(), balance.account_index))
            .or_insert((0, 0))
            .1 = balance.amount;
    }

    let mut signer_deltas_by_mint: HashMap<String, i128> = HashMap::new();
    for ((mint_str, account_index), (pre, post)) in by_mint_and_account {
        if pre == post {
            continue;
        }
        let Ok(mint) = Pubkey::from_str(&mint_str) else {
            continue;
        };
        let Some(token_account) = accounts.get(account_index as usize) else {
            continue;
        };
        let owner_hint = pre_token_balances
            .iter()
            .chain(post_token_balances.iter())
            .find(|balance| balance.account_index == account_index && balance.mint == mint_str);
        let Some(owner) = resolve_token_balance_owner(owner_hint, accounts, token_account, &mint)
        else {
            continue;
        };
        if owner != *signer {
            continue;
        }
        *signer_deltas_by_mint.entry(mint_str).or_insert(0) += post as i128 - pre as i128;
    }

    let mut candidates: Vec<Pubkey> = signer_deltas_by_mint
        .into_iter()
        .filter_map(|(mint_str, delta)| {
            let direction_matches = match side {
                TradeSide::Buy => delta > 0,
                TradeSide::Sell => delta < 0,
            };
            if !direction_matches {
                return None;
            }
            Pubkey::from_str(&mint_str).ok()
        })
        .collect();

    candidates.sort_unstable_by_key(|mint| mint.to_string());
    candidates.dedup();
    if candidates.len() == 1 {
        candidates.into_iter().next()
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy)]
struct SignerSwapInference {
    mint: Pubkey,
    token_amount: u64,
    sol_amount: u64,
    is_buy: bool,
}

#[derive(Debug, Clone, Copy)]
struct SupportedRouterTradeHint {
    outer_instruction_index: u32,
    router_program: Pubkey,
    signer: Pubkey,
    explicit_mint: Option<Pubkey>,
}

fn resolve_instruction_account_pubkey(
    all_accounts: &[Pubkey],
    ix: &crate::types::RawInstruction,
    index: usize,
) -> Option<Pubkey> {
    ix.account_indices
        .get(index)
        .and_then(|flat_index| all_accounts.get(*flat_index as usize))
        .copied()
        .filter(|pubkey| *pubkey != Pubkey::default())
}

fn is_non_wsol_trade_mint(pubkey: &Pubkey) -> bool {
    *pubkey != Pubkey::default() && pubkey.to_string() != WSOL_MINT
}

fn resolve_non_wsol_router_mint(
    all_accounts: &[Pubkey],
    ix: &crate::types::RawInstruction,
    mint_index: usize,
) -> Option<Pubkey> {
    resolve_instruction_account_pubkey(all_accounts, ix, mint_index).filter(is_non_wsol_trade_mint)
}

fn resolve_non_wsol_router_pair_mint(
    all_accounts: &[Pubkey],
    ix: &crate::types::RawInstruction,
    source_mint_index: usize,
    destination_mint_index: usize,
) -> Option<Pubkey> {
    let source_mint = resolve_instruction_account_pubkey(all_accounts, ix, source_mint_index)?;
    let destination_mint =
        resolve_instruction_account_pubkey(all_accounts, ix, destination_mint_index)?;
    let source_is_wsol = source_mint.to_string() == WSOL_MINT;
    let destination_is_wsol = destination_mint.to_string() == WSOL_MINT;
    match (source_is_wsol, destination_is_wsol) {
        (true, false) => Some(destination_mint),
        (false, true) => Some(source_mint),
        _ => None,
    }
}

fn decode_supported_router_trade_hint(
    all_accounts: &[Pubkey],
    ix: &crate::types::RawInstruction,
    outer_instruction_index: u32,
) -> Option<SupportedRouterTradeHint> {
    if ix.data.len() < 8 {
        return None;
    }
    let disc: [u8; 8] = ix.data[..8].try_into().ok()?;
    let router_program = ix.program_id;

    match router_program.to_string().as_str() {
        JUPITER_V6_PROGRAM_ID => match disc {
            DISC_JUPITER_ROUTE_V2 => Some(SupportedRouterTradeHint {
                outer_instruction_index,
                router_program,
                signer: resolve_instruction_account_pubkey(
                    all_accounts,
                    ix,
                    JUPITER_ROUTE_V2_IDX_SIGNER,
                )?,
                explicit_mint: resolve_non_wsol_router_pair_mint(
                    all_accounts,
                    ix,
                    JUPITER_ROUTE_V2_IDX_SOURCE_MINT,
                    JUPITER_ROUTE_V2_IDX_DESTINATION_MINT,
                ),
            }),
            DISC_JUPITER_ROUTE => Some(SupportedRouterTradeHint {
                outer_instruction_index,
                router_program,
                signer: resolve_instruction_account_pubkey(
                    all_accounts,
                    ix,
                    JUPITER_ROUTE_IDX_SIGNER,
                )?,
                explicit_mint: resolve_non_wsol_router_mint(
                    all_accounts,
                    ix,
                    JUPITER_ROUTE_IDX_DESTINATION_MINT,
                ),
            }),
            _ => None,
        },
        DFLOW_V4_PROGRAM_ID => match disc {
            DISC_DFLOW_SWAP2 | DISC_DFLOW_SWAP => Some(SupportedRouterTradeHint {
                outer_instruction_index,
                router_program,
                signer: resolve_instruction_account_pubkey(
                    all_accounts,
                    ix,
                    DFLOW_SWAP_IDX_SIGNER,
                )?,
                explicit_mint: None,
            }),
            DISC_DFLOW_SWAP2_WITH_DESTINATION => Some(SupportedRouterTradeHint {
                outer_instruction_index,
                router_program,
                signer: resolve_instruction_account_pubkey(
                    all_accounts,
                    ix,
                    DFLOW_SWAP_WITH_DESTINATION_IDX_SIGNER,
                )?,
                explicit_mint: resolve_non_wsol_router_mint(
                    all_accounts,
                    ix,
                    DFLOW_SWAP_WITH_DESTINATION_IDX_DESTINATION_MINT,
                ),
            }),
            DISC_DFLOW_SWAP2_WITH_DESTINATION_NATIVE => Some(SupportedRouterTradeHint {
                outer_instruction_index,
                router_program,
                signer: resolve_instruction_account_pubkey(
                    all_accounts,
                    ix,
                    DFLOW_SWAP_WITH_DESTINATION_NATIVE_IDX_SIGNER,
                )?,
                explicit_mint: None,
            }),
            _ => None,
        },
        _ => None,
    }
}

fn choose_router_trade_mint(
    explicit_mint: Option<Pubkey>,
    inferred_mint: Pubkey,
) -> Option<Pubkey> {
    let inferred_mint = is_non_wsol_trade_mint(&inferred_mint).then_some(inferred_mint);
    match (explicit_mint.filter(is_non_wsol_trade_mint), inferred_mint) {
        (Some(explicit), Some(inferred)) if explicit != inferred => None,
        (Some(explicit), _) => Some(explicit),
        (None, Some(inferred)) => Some(inferred),
        (None, None) => None,
    }
}

fn signer_native_sol_delta(event: &GeyserEvent, signer: &Pubkey) -> u64 {
    let GeyserEvent::Transaction {
        accounts,
        pre_balances,
        post_balances,
        ..
    } = event
    else {
        return 0;
    };

    let Some(signer_index) = accounts.iter().position(|candidate| candidate == signer) else {
        return 0;
    };

    let pre = pre_balances.get(signer_index).copied().unwrap_or(0);
    let post = post_balances.get(signer_index).copied().unwrap_or(0);
    post.abs_diff(pre)
}

fn infer_signer_swap_from_balances(
    event: &GeyserEvent,
    signer: &Pubkey,
) -> Option<SignerSwapInference> {
    let GeyserEvent::Transaction {
        accounts,
        pre_token_balances,
        post_token_balances,
        ..
    } = event
    else {
        return None;
    };

    if *signer == Pubkey::default() {
        return None;
    }

    let mut by_mint_and_account: HashMap<(String, u32), (u64, u64)> = HashMap::new();
    for balance in pre_token_balances {
        by_mint_and_account
            .entry((balance.mint.clone(), balance.account_index))
            .or_insert((0, 0))
            .0 = balance.amount;
    }
    for balance in post_token_balances {
        by_mint_and_account
            .entry((balance.mint.clone(), balance.account_index))
            .or_insert((0, 0))
            .1 = balance.amount;
    }

    let mut signer_deltas_by_mint: HashMap<Pubkey, i128> = HashMap::new();
    for ((mint_str, account_index), (pre, post)) in by_mint_and_account {
        if pre == post {
            continue;
        }
        let Ok(mint) = Pubkey::from_str(&mint_str) else {
            continue;
        };
        let Some(token_account) = accounts.get(account_index as usize) else {
            continue;
        };
        let owner_hint = pre_token_balances
            .iter()
            .chain(post_token_balances.iter())
            .find(|balance| balance.account_index == account_index && balance.mint == mint_str);
        let Some(owner) = resolve_token_balance_owner(owner_hint, accounts, token_account, &mint)
        else {
            continue;
        };
        if owner != *signer {
            continue;
        }
        *signer_deltas_by_mint.entry(mint).or_insert(0) += post as i128 - pre as i128;
    }

    let mut traded_candidates: Vec<(Pubkey, i128)> = signer_deltas_by_mint
        .iter()
        .filter_map(|(mint, delta)| {
            (*mint != Pubkey::default() && mint.to_string() != WSOL_MINT && *delta != 0)
                .then_some((*mint, *delta))
        })
        .collect();
    traded_candidates.sort_unstable_by_key(|(mint, _)| mint.to_string());
    traded_candidates.dedup_by_key(|(mint, _)| *mint);
    if traded_candidates.len() != 1 {
        return None;
    }

    let (mint, delta_raw) = traded_candidates[0];
    let sol_amount = signer_deltas_by_mint
        .iter()
        .find_map(|(candidate, delta)| {
            (candidate.to_string() == WSOL_MINT && *delta != 0)
                .then_some(delta.unsigned_abs() as u64)
        })
        .unwrap_or_else(|| signer_native_sol_delta(event, signer));

    Some(SignerSwapInference {
        mint,
        token_amount: delta_raw.unsigned_abs() as u64,
        sol_amount,
        is_buy: delta_raw > 0,
    })
}

fn token_balance_owner_pubkey(balance: &crate::types::RawTokenBalance) -> Option<Pubkey> {
    let owner = balance.owner.as_deref()?;
    let pubkey = Pubkey::from_str(owner).ok()?;
    is_candidate_owner(&pubkey).then_some(pubkey)
}

fn resolve_token_balance_owner(
    balance_hint: Option<&crate::types::RawTokenBalance>,
    accounts: &[Pubkey],
    token_account: &Pubkey,
    mint: &Pubkey,
) -> Option<Pubkey> {
    if let Some(balance) = balance_hint {
        if let Some(owner) = token_balance_owner_pubkey(balance) {
            return Some(owner);
        }
    }

    resolve_ata_owner(accounts, token_account, mint)
}

fn resolve_trade_mint(
    cm_reg: &CurveMintRegistry,
    event: &GeyserEvent,
    pool: &Pubkey,
    signer: &Pubkey,
    side: TradeSide,
    explicit_mint: Option<Pubkey>,
) -> Pubkey {
    if let Some(mint) = explicit_mint.filter(|mint| *mint != Pubkey::default()) {
        return mint;
    }

    if let Some(mapped) = cm_reg
        .mint_for_curve_pk(pool)
        .filter(|mint| *mint != Pubkey::default())
    {
        return mapped;
    }

    infer_signer_trade_mint_from_balances(event, signer, side).unwrap_or_default()
}

fn resolve_trade_mint_from_event_accounts(event: &GeyserEvent) -> Option<Pubkey> {
    let GeyserEvent::Transaction { accounts, .. } = event else {
        return None;
    };
    resolve_trade_mint_from_token_accounts(event, accounts)
        .filter(|mint| *mint != Pubkey::default() && mint.to_string() != WSOL_MINT)
}

fn resolve_trade_mint_from_token_accounts(
    event: &GeyserEvent,
    token_accounts: &[Pubkey],
) -> Option<Pubkey> {
    let GeyserEvent::Transaction {
        accounts,
        pre_token_balances,
        post_token_balances,
        ..
    } = event
    else {
        return None;
    };

    for token_account in token_accounts {
        if *token_account == Pubkey::default() {
            continue;
        }

        let mut resolved = None;
        let mut ambiguous = false;
        for balance in pre_token_balances.iter().chain(post_token_balances.iter()) {
            let Some(balance_pubkey) = accounts.get(balance.account_index as usize) else {
                continue;
            };
            if balance_pubkey != token_account {
                continue;
            }
            let Ok(candidate) = Pubkey::from_str(&balance.mint) else {
                continue;
            };
            if candidate == Pubkey::default() || candidate.to_string() == WSOL_MINT {
                continue;
            }
            match resolved {
                Some(existing) if existing != candidate => {
                    ambiguous = true;
                    break;
                }
                Some(_) => {}
                None => resolved = Some(candidate),
            }
        }

        if !ambiguous {
            if let Some(mint) = resolved {
                return Some(mint);
            }
        }
    }

    None
}

fn trade_dedup_quote_amount(trade: &TradeEvent) -> u64 {
    if trade.is_buy {
        trade.max_sol_cost
    } else {
        trade.min_sol_output
    }
}

fn trade_is_weak_candidate(trade: &TradeEvent) -> bool {
    trade.mint == Pubkey::default()
        || trade.pool_amm_id == Pubkey::default()
        || (trade.owner_token_deltas.is_empty() && !trade.curve_data_known)
}

fn trade_candidate_score(cm_reg: &CurveMintRegistry, trade: &TradeEvent) -> u32 {
    let mut score = 0u32;
    if trade.mint != Pubkey::default() {
        score += 100;
    }
    if trade.pool_amm_id != Pubkey::default() {
        score += 5;
    }
    if trade.curve_data_known {
        score += 10;
    }
    if !trade.owner_token_deltas.is_empty() {
        score += 20;
    }
    if trade.global_config.is_some() {
        score += 5;
    }
    if trade.fee_recipient.is_some() {
        score += 5;
    }
    if trade.token_program.is_some() {
        score += 10;
    }
    if trade.buy_variant.is_some() {
        score += 10;
    }
    if trade.associated_bonding_curve.is_some() {
        score += 10;
    }
    if trade.bonding_curve_v2.is_some() {
        score += 10;
    }
    if trade.mint != Pubkey::default()
        && cm_reg.mint_for_curve_pk(&trade.pool_amm_id) == Some(trade.mint)
    {
        score += 40;
    }
    score
}

fn trade_ordinals_differ(existing: &TradeEvent, candidate: &TradeEvent) -> bool {
    matches!(
        (existing.event_ordinal, candidate.event_ordinal),
        (Some(existing), Some(candidate)) if existing != candidate
    )
}

fn merge_trade_provenance(target: &mut TradeEvent, source: &TradeEvent) {
    if target.provenance.is_none() {
        target.provenance = source.provenance.clone();
    }
}

fn merge_trade_optional_accounts(target: &mut TradeEvent, source: &TradeEvent) {
    merge_trade_provenance(target, source);
    if target.global_config.is_none() {
        target.global_config = source.global_config;
    }
    if target.fee_recipient.is_none() {
        target.fee_recipient = source.fee_recipient;
    }
    if target.token_program.is_none() {
        target.token_program = source.token_program;
    }
    if target.buy_variant.is_none() {
        target.buy_variant = source.buy_variant.clone();
    }
    if target.associated_bonding_curve.is_none() {
        target.associated_bonding_curve = source.associated_bonding_curve;
    }
    if target.bonding_curve_v2.is_none() {
        target.bonding_curve_v2 = source.bonding_curve_v2;
    }
    if target.is_mayhem_mode.is_none() {
        target.is_mayhem_mode = source.is_mayhem_mode;
    }
}

fn dedup_trade_candidates(cm_reg: &CurveMintRegistry, trades: Vec<TradeEvent>) -> Vec<TradeEvent> {
    let before_count = trades.len();
    let before_sample_sig = sample_trade_signature(&trades);
    let before_kinds = summarize_trade_winner_kinds(&trades);
    let (before_provenance_present, before_provenance_missing) = trade_provenance_counts(&trades);
    let mut deduped: Vec<TradeEvent> = Vec::with_capacity(trades.len());
    record_trade_candidate_dedup_stage("input", trades.len());
    for trade in trades {
        let incoming_score = trade_candidate_score(cm_reg, &trade);

        if trade.event_ordinal.is_none() {
            debug!(
                "TRADE_CANDIDATE_MISSING_ORDINAL sig={} is_buy={} amount={}",
                trade.signature, trade.is_buy, trade.amount
            );
            record_trade_candidate_ordinal("missing_event_ordinal", "none");
        }

        let mut matched_index = None;
        let mut matched_kind = TradeCandidateMatchKind::None;
        let mut saw_ordinal_mismatch = false;
        let mut ordinal_mismatch_match_kind = "none";
        let mut ordinal_mismatch_provenance_relation = "not_applicable";
        let mut ordinal_mismatch_existing_score = None;

        for (index, existing) in deduped.iter().enumerate() {
            let match_kind = trade_candidate_match_kind(existing, &trade);
            match match_kind {
                TradeCandidateMatchKind::ExactMatch | TradeCandidateMatchKind::WeakDuplicate => {
                    matched_index = Some(index);
                    matched_kind = match_kind;
                    break;
                }
                TradeCandidateMatchKind::OrdinalMismatchExactMatch
                | TradeCandidateMatchKind::OrdinalMismatchWeakDuplicate => {
                    saw_ordinal_mismatch = true;
                    ordinal_mismatch_match_kind = match_kind.label();
                    ordinal_mismatch_provenance_relation =
                        trade_candidate_provenance_relation(existing, &trade);
                    ordinal_mismatch_existing_score = Some(trade_candidate_score(cm_reg, existing));
                    debug!(
                        "TRADE_CANDIDATE_ORDINAL_MISMATCH sig={} existing_ordinal={:?} incoming_ordinal={:?} reason={}",
                        trade.signature,
                        existing.event_ordinal,
                        trade.event_ordinal,
                        ordinal_mismatch_match_kind
                    );
                    record_trade_candidate_ordinal("ordinals_differ", ordinal_mismatch_match_kind);
                }
                TradeCandidateMatchKind::None => {}
            }
        }

        if let Some(index) = matched_index {
            let existing = &mut deduped[index];
            let existing_score = trade_candidate_score(cm_reg, existing);
            let match_kind = matched_kind.label();
            let ordinal_relation = trade_candidate_ordinal_relation(existing, &trade);
            let provenance_relation = trade_candidate_provenance_relation(existing, &trade);
            if incoming_score > existing_score {
                let mut replacement = trade;
                merge_trade_optional_accounts(&mut replacement, existing);
                *existing = replacement;
                record_trade_candidate_dedup_decision(
                    "replaced",
                    "score_incoming_higher",
                    match_kind,
                    ordinal_relation,
                    provenance_relation,
                    "merge_trade_optional_accounts",
                    "incoming",
                    Some(incoming_score),
                    Some(existing_score),
                );
            } else {
                merge_trade_optional_accounts(existing, &trade);
                record_trade_candidate_dedup_decision(
                    "merged_into_existing",
                    if incoming_score == existing_score {
                        "score_tie_prefers_existing"
                    } else {
                        "score_existing_higher"
                    },
                    match_kind,
                    ordinal_relation,
                    provenance_relation,
                    "merge_trade_optional_accounts",
                    "existing",
                    Some(incoming_score),
                    Some(existing_score),
                );
            }
        } else {
            if saw_ordinal_mismatch {
                record_trade_candidate_ordinal(
                    "match_rejected_due_to_different_event_ordinal",
                    ordinal_mismatch_match_kind,
                );
                record_trade_candidate_dedup_decision(
                    "kept",
                    "different_event_ordinal",
                    ordinal_mismatch_match_kind,
                    "different_event_ordinal",
                    ordinal_mismatch_provenance_relation,
                    "none",
                    "incoming_distinct",
                    Some(incoming_score),
                    ordinal_mismatch_existing_score,
                );
            } else {
                record_trade_candidate_dedup_decision(
                    "kept",
                    "no_duplicate_match",
                    "none",
                    "not_applicable",
                    "not_applicable",
                    "none",
                    "incoming_distinct",
                    Some(incoming_score),
                    None,
                );
            }
            deduped.push(trade);
        }
    }
    record_trade_candidate_dedup_stage("output", deduped.len());
    record_trade_candidate_arbitration_sample(
        before_sample_sig.as_str(),
        before_count,
        before_kinds.as_str(),
        before_provenance_present,
        before_provenance_missing,
        &deduped,
    );
    deduped
}

fn trade_matches_pump_instruction(
    trade: &TradeEvent,
    ix_program_id: &Pubkey,
    ix_data: &[u8],
    ix_accounts: &SmallVec<[String; 14]>,
) -> bool {
    if ix_program_id.to_string() != PUMP_FUN_PROGRAM_ID || ix_data.len() < 8 {
        return false;
    }

    let is_matching_discriminator = if trade.is_buy {
        ix_data.starts_with(&DISC_BUY)
            || ix_data.starts_with(&DISC_SWAP_BUY_EXACT_QUOTE_IN)
            || ix_data.starts_with(&DISC_PUMP_BUY_ROUTED)
    } else {
        ix_data.starts_with(&DISC_SELL)
    };

    if !is_matching_discriminator || ix_accounts.len() <= PUMP_IDX_BONDING_CURVE {
        return false;
    }

    // When curve→mint mapping was not yet resolved at parse time, pool_amm_id and/or
    // mint are Pubkey::default().  In that case we cannot match by identity — but the
    // discriminator + program already uniquely identify the instruction as a Pump.fun
    // buy/sell for this trade.  Accept the match so enrich_trade_optional_accounts_from_source_ix
    // can salvage buy_variant, fee_recipient, token_program and associated_bonding_curve
    // from the instruction accounts.  Without this, buy_variant stays None for every
    // unresolved/buffered-tx-fallback pool, causing the builder to silently fall back to
    // RoutedExactSolIn for LegacyBuy pools → Custom(1) on-chain.
    if trade.pool_amm_id == Pubkey::default() || trade.mint == Pubkey::default() {
        return true;
    }

    let mint = Pubkey::from_str(&acs(ix_accounts, PUMP_IDX_MINT)).unwrap_or_default();
    let bonding_curve =
        Pubkey::from_str(&acs(ix_accounts, PUMP_IDX_BONDING_CURVE)).unwrap_or_default();

    mint == trade.mint && bonding_curve == trade.pool_amm_id
}

/// Check whether all enrichment fields on the trade are populated.
#[inline(always)]
fn trade_enrich_complete(trade: &TradeEvent) -> bool {
    trade.global_config.is_some()
        && trade.fee_recipient.is_some()
        && trade.token_program.is_some()
        && (!trade.is_buy
            || (trade.buy_variant.is_some() && trade.associated_bonding_curve.is_some()))
}

fn pump_buy_enrichment_priority(ix_data: &[u8]) -> u8 {
    if ix_data.starts_with(&DISC_PUMP_BUY_ROUTED)
        || ix_data.starts_with(&DISC_SWAP_BUY_EXACT_QUOTE_IN)
    {
        2
    } else if ix_data.starts_with(&DISC_BUY) {
        1
    } else {
        0
    }
}

#[derive(Debug, Clone)]
struct ObservedIxAccountContext {
    source_tx_signature: Option<String>,
    source_slot: Option<u64>,
    source_slot_index: Option<u32>,
    source_instruction_index: Option<u32>,
    source_program_id: String,
    source_discriminator: Option<String>,
    source_buy_variant: Option<String>,
    account_indices: Vec<u8>,
    tx_success: bool,
    meta_err: Option<String>,
}

fn pump_buy_variant_from_ix_data(ix_data: &[u8]) -> Option<&'static str> {
    if ix_data.starts_with(&DISC_PUMP_BUY_ROUTED)
        || ix_data.starts_with(&DISC_SWAP_BUY_EXACT_QUOTE_IN)
    {
        Some("routed_exact_sol_in")
    } else if ix_data.starts_with(&DISC_BUY) {
        Some("legacy_buy")
    } else {
        None
    }
}

fn discriminator_hex(ix_data: &[u8]) -> Option<String> {
    let disc = ix_data.get(..8)?;
    let mut out = String::with_capacity(16);
    for byte in disc {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    Some(out)
}

fn observed_bcv2_provenance_status(
    tx_success: bool,
    source_program_id: &str,
    source_buy_variant: Option<&str>,
    instruction_account_position: Option<u32>,
    message_account_index: Option<u32>,
    resolved_pubkey: Option<&str>,
    expected_pubkey: &Pubkey,
) -> &'static str {
    if !tx_success {
        return "tx_failed";
    }
    if !is_pump_fun_program(source_program_id) {
        return "program_id_mismatch";
    }
    if source_buy_variant.is_none() {
        return "discriminator_mismatch";
    }
    if instruction_account_position != Some(PUMP_IDX_BONDING_CURVE_V2 as u32) {
        return "account_position_out_of_range";
    }
    let Some(resolved_pubkey) = resolved_pubkey else {
        return "message_index_resolution_failed";
    };
    if message_account_index.is_none() || resolved_pubkey.is_empty() {
        return "message_index_resolution_failed";
    }
    if resolved_pubkey != expected_pubkey.to_string() {
        return "message_index_resolution_failed";
    }
    "route_compatible"
}

fn bonding_curve_v2_provenance_from_ix(
    trade: &TradeEvent,
    ix_accounts: &SmallVec<[String; 14]>,
    context: &ObservedIxAccountContext,
    bonding_curve_v2: Pubkey,
) -> ObservedAccountMetaProvenance {
    let message_account_index = context
        .account_indices
        .get(PUMP_IDX_BONDING_CURVE_V2)
        .map(|value| u32::from(*value));
    let resolved_pubkey = ix_accounts.get(PUMP_IDX_BONDING_CURVE_V2).cloned();
    let instruction_account_position =
        (ix_accounts.len() > PUMP_IDX_BONDING_CURVE_V2).then_some(PUMP_IDX_BONDING_CURVE_V2 as u32);
    let status = observed_bcv2_provenance_status(
        trade.success,
        &context.source_program_id,
        context.source_buy_variant.as_deref(),
        instruction_account_position,
        message_account_index,
        resolved_pubkey.as_deref(),
        &bonding_curve_v2,
    );

    ObservedAccountMetaProvenance {
        source_tx_signature: context.source_tx_signature.clone(),
        source_slot: context.source_slot,
        source_slot_index: context.source_slot_index,
        source_instruction_index: context.source_instruction_index,
        source_program_id: Some(context.source_program_id.clone()),
        source_discriminator: context.source_discriminator.clone(),
        source_buy_variant: context.source_buy_variant.clone(),
        instruction_account_position,
        message_account_index,
        resolved_pubkey,
        loaded_address_source: Some("resolved_transaction_account_keys".to_string()),
        tx_success: Some(context.tx_success),
        meta_err: context.meta_err.clone(),
        provenance_status: Some(status.to_string()),
    }
}

/// Fill optional enrichment fields on `trade` from the resolved instruction
/// accounts and data.  Returns `true` when all fields are now populated.
///
/// Shared by both top-level and inner-instruction enrichment passes.
fn fill_trade_from_ix_accounts(
    trade: &mut TradeEvent,
    ix_accounts: &SmallVec<[String; 14]>,
    ix_data: &[u8],
    context: &ObservedIxAccountContext,
) -> bool {
    if trade.global_config.is_none() {
        trade.global_config = Pubkey::from_str(&acs(ix_accounts, PUMP_IDX_GLOBAL_CONFIG)).ok();
    }
    if trade.fee_recipient.is_none() {
        trade.fee_recipient = Pubkey::from_str(&acs(ix_accounts, PUMP_IDX_FEE_RECIPIENT)).ok();
    }
    if trade.token_program.is_none() {
        trade.token_program = Pubkey::from_str(&acs(ix_accounts, PUMP_IDX_TOKEN_PROGRAM)).ok();
    }
    if trade.is_buy && trade.buy_variant.is_none() {
        trade.buy_variant = pump_buy_variant_from_ix_data(ix_data).map(str::to_string);
    }
    if trade.is_buy && trade.associated_bonding_curve.is_none() {
        trade.associated_bonding_curve =
            Pubkey::from_str(&acs(ix_accounts, PUMP_IDX_ASSOCIATED_BONDING_CURVE))
                .ok()
                .filter(|value| *value != Pubkey::default());
    }
    if trade.is_buy && trade.bonding_curve_v2.is_none() {
        trade.bonding_curve_v2 = Pubkey::from_str(&acs(ix_accounts, PUMP_IDX_BONDING_CURVE_V2))
            .ok()
            .filter(|value| *value != Pubkey::default());
    }
    if trade.is_buy && trade.bonding_curve_v2_provenance.is_none() {
        if let Some(bonding_curve_v2) = trade.bonding_curve_v2 {
            trade.bonding_curve_v2_provenance = Some(bonding_curve_v2_provenance_from_ix(
                trade,
                ix_accounts,
                context,
                bonding_curve_v2,
            ));
        }
    }
    trade_enrich_complete(trade)
}

fn enrich_trade_optional_accounts_from_source_ix(event: &GeyserEvent, trade: &mut TradeEvent) {
    // PumpSwap AMM trades never carry pump.fun bonding-curve-specific fields.
    // buy_variant / associated_bonding_curve / fee_recipient (bonding-curve sense) are
    // not part of the pAMM account layout — scanning instructions for them is both
    // pointless and pollutes the ENRICH_RESULT coverage metric.
    if trade.is_pumpswap {
        return;
    }

    if trade_enrich_complete(trade) {
        info!("ENRICH_SKIP already_full sig={}", trade.signature);
        return;
    }

    let GeyserEvent::Transaction {
        slot,
        signature,
        accounts,
        instructions,
        inner_instructions,
        success,
        error_code,
        ..
    } = event
    else {
        info!("ENRICH_SKIP not_transaction sig={}", trade.signature);
        return;
    };

    let all_keys: Vec<String> = accounts.iter().map(ToString::to_string).collect();

    // ── Phase 1: top-level instructions ──────────────────────────────────────
    let mut best_top_level_ix: Option<(
        SmallVec<[String; 14]>,
        Vec<u8>,
        u8,
        ObservedIxAccountContext,
    )> = None;
    for (outer_instruction_index, ix) in instructions.iter().enumerate() {
        let ix_accounts = resolve_accounts(&ix.account_indices, &all_keys);
        if !trade_matches_pump_instruction(trade, &ix.program_id, &ix.data, &ix_accounts) {
            continue;
        }
        let priority = if trade.is_buy {
            pump_buy_enrichment_priority(&ix.data)
        } else {
            1
        };
        let context = ObservedIxAccountContext {
            source_tx_signature: Some(signature.to_string()),
            source_slot: *slot,
            source_slot_index: trade.event_ordinal,
            source_instruction_index: Some(outer_instruction_index as u32),
            source_program_id: ix.program_id.to_string(),
            source_discriminator: discriminator_hex(&ix.data),
            source_buy_variant: pump_buy_variant_from_ix_data(&ix.data).map(str::to_string),
            account_indices: ix.account_indices.clone(),
            tx_success: *success,
            meta_err: error_code.clone(),
        };
        let replace = best_top_level_ix
            .as_ref()
            .map(|(_, _, best_priority, _)| priority > *best_priority)
            .unwrap_or(true);
        if replace {
            best_top_level_ix = Some((ix_accounts, ix.data.clone(), priority, context));
        }
    }
    if let Some((ix_accounts, ix_data, _, context)) = best_top_level_ix {
        fill_trade_from_ix_accounts(trade, &ix_accounts, &ix_data, &context);
    }

    // ── Phase 2: inner instructions (CPI) ────────────────────────────────────
    // Aggregators (Jupiter, Axiom, terminal flows, etc.) place the pump.fun
    // buy/sell as an inner CPI call, not a top-level instruction.  Without
    // this pass, buy_variant / fee / token_program / assoc_bc stay None
    // for ~96% of observed BUY transactions.
    if !trade_enrich_complete(trade) {
        let mut best_inner_ix: Option<(
            SmallVec<[String; 14]>,
            Vec<u8>,
            u8,
            ObservedIxAccountContext,
        )> = None;
        for group in inner_instructions {
            for ix in &group.instructions {
                let prog_str = key_at(&all_keys, ix.program_id_index as usize);
                let Ok(prog_pk) = Pubkey::from_str(&prog_str) else {
                    continue;
                };
                let ix_accounts = resolve_accounts(&ix.accounts, &all_keys);
                if !trade_matches_pump_instruction(trade, &prog_pk, &ix.data, &ix_accounts) {
                    continue;
                }
                let priority = if trade.is_buy {
                    pump_buy_enrichment_priority(&ix.data)
                } else {
                    1
                };
                let context = ObservedIxAccountContext {
                    source_tx_signature: Some(signature.to_string()),
                    source_slot: *slot,
                    source_slot_index: trade.event_ordinal,
                    source_instruction_index: Some(group.index),
                    source_program_id: prog_str,
                    source_discriminator: discriminator_hex(&ix.data),
                    source_buy_variant: pump_buy_variant_from_ix_data(&ix.data).map(str::to_string),
                    account_indices: ix.accounts.clone(),
                    tx_success: *success,
                    meta_err: error_code.clone(),
                };
                let replace = best_inner_ix
                    .as_ref()
                    .map(|(_, _, best_priority, _)| priority > *best_priority)
                    .unwrap_or(true);
                if replace {
                    best_inner_ix = Some((ix_accounts, ix.data.clone(), priority, context));
                }
            }
        }
        if let Some((ix_accounts, ix_data, _, context)) = best_inner_ix {
            fill_trade_from_ix_accounts(trade, &ix_accounts, &ix_data, &context);
        }
    }

    info!(
        "ENRICH_RESULT sig={} is_buy={} buy_variant={:?} assoc_bc={:?} bcv2={:?} fee={:?} token_prog={:?}",
        trade.signature,
        trade.is_buy,
        trade.buy_variant,
        trade.associated_bonding_curve,
        trade.bonding_curve_v2,
        trade.fee_recipient,
        trade.token_program
    );
}

fn populate_trade_toolchain_fingerprint_from_source_tx(
    event: &GeyserEvent,
    trade: &mut TradeEvent,
) {
    let GeyserEvent::Transaction {
        accounts,
        instructions,
        inner_instructions,
        ..
    } = event
    else {
        return;
    };

    let (
        internal_fee_transfer_count,
        external_fee_transfer_count,
        filtered_wsol_self_transfer_count,
    ) = count_trade_fee_transfers(accounts, inner_instructions, trade);

    trade.toolchain_fingerprint = ToolchainFingerprintInput {
        account_keys_len: Some(u32::try_from(accounts.len()).unwrap_or(u32::MAX)),
        outer_instruction_count: Some(u32::try_from(instructions.len()).unwrap_or(u32::MAX)),
        inner_instruction_group_count: Some(
            u32::try_from(inner_instructions.len()).unwrap_or(u32::MAX),
        ),
        has_set_compute_unit_limit: Some(trade.compute_unit_limit.is_some()),
        has_set_compute_unit_price: Some(trade.cu_price_micro_lamports.is_some()),
        internal_fee_transfer_count: Some(internal_fee_transfer_count),
        external_fee_transfer_count: Some(external_fee_transfer_count),
        filtered_wsol_self_transfer_count: Some(filtered_wsol_self_transfer_count),
    };
}

fn count_trade_fee_transfers(
    accounts: &[Pubkey],
    inner_instructions: &[crate::types::InnerInstructionGroup],
    trade: &TradeEvent,
) -> (u32, u32, u32) {
    let mut internal_fee_transfer_count = 0u32;
    let mut external_fee_transfer_count = 0u32;
    let mut filtered_wsol_self_transfer_count = 0u32;

    for group in inner_instructions {
        for instruction in &group.instructions {
            let Some(program_id) = accounts.get(instruction.program_id_index as usize) else {
                continue;
            };
            if program_id.to_string() != SYSTEM_PROGRAM_ID {
                continue;
            }
            if instruction.data.get(..4) != Some(&[2, 0, 0, 0]) {
                continue;
            }

            let Some(&source_index) = instruction.accounts.first() else {
                continue;
            };
            let Some(&destination_index) = instruction.accounts.get(1) else {
                continue;
            };
            let Some(source) = accounts.get(source_index as usize) else {
                continue;
            };
            let Some(destination) = accounts.get(destination_index as usize) else {
                continue;
            };

            if is_signer_wsol_self_transfer(accounts, trade.signer, source, destination) {
                filtered_wsol_self_transfer_count =
                    filtered_wsol_self_transfer_count.saturating_add(1);
                continue;
            }

            if is_trade_internal_fee_destination(trade, destination) {
                internal_fee_transfer_count = internal_fee_transfer_count.saturating_add(1);
                continue;
            }

            if *destination != trade.signer {
                external_fee_transfer_count = external_fee_transfer_count.saturating_add(1);
            }
        }
    }

    (
        internal_fee_transfer_count,
        external_fee_transfer_count,
        filtered_wsol_self_transfer_count,
    )
}

fn is_trade_internal_fee_destination(trade: &TradeEvent, destination: &Pubkey) -> bool {
    *destination == trade.pool_amm_id
        || trade
            .fee_recipient
            .is_some_and(|fee_recipient| fee_recipient == *destination)
        || trade
            .associated_bonding_curve
            .is_some_and(|associated_bonding_curve| associated_bonding_curve == *destination)
}

fn is_signer_wsol_self_transfer(
    accounts: &[Pubkey],
    signer: Pubkey,
    source: &Pubkey,
    destination: &Pubkey,
) -> bool {
    (*source == signer && is_signer_owned_wsol_ata(accounts, &signer, destination))
        || (*destination == signer && is_signer_owned_wsol_ata(accounts, &signer, source))
}

fn is_signer_owned_wsol_ata(accounts: &[Pubkey], signer: &Pubkey, candidate: &Pubkey) -> bool {
    let Ok(wsol_mint) = Pubkey::from_str(WSOL_MINT) else {
        return false;
    };
    resolve_ata_owner(accounts, candidate, &wsol_mint) == Some(*signer)
}

fn resolve_ata_owner(accounts: &[Pubkey], token_account: &Pubkey, mint: &Pubkey) -> Option<Pubkey> {
    let associated_token_program = Pubkey::from_str(ASSOCIATED_TOKEN_PROGRAM_ID).ok()?;
    let token_program = Pubkey::from_str(ProgramIds::TOKEN_PROGRAM).ok()?;
    let token_2022_program = Pubkey::from_str(ProgramIds::TOKEN_2022_PROGRAM).ok()?;

    for candidate_owner in accounts.iter().copied().filter(is_candidate_owner) {
        let derived = Pubkey::find_program_address(
            &[
                candidate_owner.as_ref(),
                token_program.as_ref(),
                mint.as_ref(),
            ],
            &associated_token_program,
        )
        .0;
        if derived == *token_account {
            return Some(candidate_owner);
        }

        let derived_2022 = Pubkey::find_program_address(
            &[
                candidate_owner.as_ref(),
                token_2022_program.as_ref(),
                mint.as_ref(),
            ],
            &associated_token_program,
        )
        .0;
        if derived_2022 == *token_account {
            return Some(candidate_owner);
        }
    }

    None
}

fn is_candidate_owner(pubkey: &Pubkey) -> bool {
    let pubkey_str = pubkey.to_string();
    if !pubkey.is_on_curve() {
        return false;
    }
    pubkey_str != SYSTEM_PROGRAM_ID
        && pubkey_str != ProgramIds::TOKEN_PROGRAM
        && pubkey_str != ProgramIds::TOKEN_2022_PROGRAM
        && pubkey_str != COMPUTE_BUDGET_PROGRAM_ID
        && pubkey_str != ASSOCIATED_TOKEN_PROGRAM_ID
        && pubkey_str != crate::grpc_connection::PUMP_FUN_FEE_ACCOUNT
}

/// Cheap pre-filter: peek the pool/bonding-curve pubkey from the first recognizable
/// BUY or SELL instruction without performing full Borsh deserialization.
///
/// Returns the `pool_amm_id` that a fully-parsed `TradeEvent` would carry:
///   - Pump.fun bonding curve: `accounts[account_indices[PUMP_IDX_BONDING_CURVE]]`
///   - PumpSwap AMM:           `accounts[account_indices[SWAP_IDX_POOL]]`
///
/// Returns `None` if no supported trade discriminator is found in top-level instructions.
pub fn peek_trade_pool_id(event: &crate::types::GeyserEvent) -> Option<solana_sdk::pubkey::Pubkey> {
    let (accounts, instructions) = match event {
        crate::types::GeyserEvent::Transaction {
            accounts,
            instructions,
            ..
        } => (accounts, instructions),
        _ => return None,
    };
    for ix in instructions {
        if ix.data.len() < 8 {
            continue;
        }
        let disc = &ix.data[..8];
        if disc != DISC_BUY
            && disc != DISC_SELL
            && disc != DISC_SWAP_BUY_EXACT_QUOTE_IN
            && disc != DISC_PUMP_BUY_ROUTED
        {
            continue;
        }
        if ix.program_id.to_string() == PUMP_SWAP_PROGRAM_ID {
            let flat_idx = *ix.account_indices.get(SWAP_IDX_POOL)? as usize;
            return accounts.get(flat_idx).copied();
        } else {
            let flat_idx = *ix.account_indices.get(PUMP_IDX_BONDING_CURVE)? as usize;
            return accounts.get(flat_idx).copied();
        }
    }
    None
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use metrics::{
        Counter, CounterFn, Gauge, Histogram, Key, KeyName, Recorder, SharedString, Unit,
    };
    use solana_sdk::signature::Keypair;
    use solana_sdk::signer::Signer as _;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::Instant;

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct RecordedCounter {
        name: String,
        labels: Vec<(String, String)>,
    }

    #[derive(Clone)]
    struct TestMetricsHandle {
        counters: Arc<Mutex<Vec<RecordedCounter>>>,
    }

    struct TestMetricsRecorder {
        handle: TestMetricsHandle,
    }

    struct TestCounter {
        handle: TestMetricsHandle,
        metric: RecordedCounter,
    }

    impl CounterFn for TestCounter {
        fn increment(&self, _value: u64) {
            self.handle
                .counters
                .lock()
                .expect("counter lock")
                .push(self.metric.clone());
        }

        fn absolute(&self, _value: u64) {
            self.handle
                .counters
                .lock()
                .expect("counter lock")
                .push(self.metric.clone());
        }
    }

    impl Recorder for TestMetricsRecorder {
        fn describe_counter(&self, _key: KeyName, _unit: Option<Unit>, _description: SharedString) {
        }

        fn describe_gauge(&self, _key: KeyName, _unit: Option<Unit>, _description: SharedString) {}

        fn describe_histogram(
            &self,
            _key: KeyName,
            _unit: Option<Unit>,
            _description: SharedString,
        ) {
        }

        fn register_counter(&self, key: &Key) -> Counter {
            Counter::from_arc(Arc::new(TestCounter {
                handle: self.handle.clone(),
                metric: RecordedCounter {
                    name: key.name().to_string(),
                    labels: key
                        .labels()
                        .map(|label| (label.key().to_string(), label.value().to_string()))
                        .collect(),
                },
            }))
        }

        fn register_gauge(&self, _key: &Key) -> Gauge {
            Gauge::noop()
        }

        fn register_histogram(&self, _key: &Key) -> Histogram {
            Histogram::noop()
        }
    }

    static TEST_METRICS_HANDLE: OnceLock<TestMetricsHandle> = OnceLock::new();
    static TEST_METRICS_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn metrics_handle() -> TestMetricsHandle {
        TEST_METRICS_HANDLE
            .get_or_init(|| {
                let handle = TestMetricsHandle {
                    counters: Arc::new(Mutex::new(Vec::new())),
                };
                metrics::set_boxed_recorder(Box::new(TestMetricsRecorder {
                    handle: handle.clone(),
                }))
                .expect("install test metrics recorder");
                handle
            })
            .clone()
    }

    fn metrics_test_lock() -> &'static Mutex<()> {
        TEST_METRICS_LOCK.get_or_init(|| Mutex::new(()))
    }

    fn clear_recorded_counters() {
        metrics_handle()
            .counters
            .lock()
            .expect("counter lock")
            .clear();
    }

    fn saw_counter(name: &str, expected_labels: &[(&str, &str)]) -> bool {
        metrics_handle()
            .counters
            .lock()
            .expect("counter lock")
            .iter()
            .any(|counter| {
                counter.name == name
                    && expected_labels.iter().all(|(key, value)| {
                        counter.labels.iter().any(|(observed_key, observed_value)| {
                            observed_key == key && observed_value == value
                        })
                    })
            })
    }

    fn no_meta() -> Option<&'static yellowstone_grpc_proto::prelude::TransactionStatusMeta> {
        None
    }
    fn no_keys() -> Vec<String> {
        vec![]
    }

    /// Deterministic test pubkey: 31 zero bytes + seed byte, encoded as Base58.
    fn tp(seed: u8) -> String {
        let mut bytes = [0u8; 32];
        bytes[31] = seed;
        Pubkey::new_from_array(bytes).to_string()
    }

    fn trade_data(disc: [u8; 8], amount: u64, sol: u64) -> Vec<u8> {
        let mut d = disc.to_vec();
        d.extend_from_slice(&amount.to_le_bytes());
        d.extend_from_slice(&sol.to_le_bytes());
        d
    }

    fn create_data(name: &str, sym: &str, uri: &str) -> Vec<u8> {
        let mut d = DISC_CREATE.to_vec();
        for s in [name, sym, uri] {
            let b = s.as_bytes();
            d.extend_from_slice(&(b.len() as u32).to_le_bytes());
            d.extend_from_slice(b);
        }
        d
    }

    fn cpi_trade_payload(mint: Pubkey, user: Pubkey, is_buy: bool) -> Vec<u8> {
        let mut data = DISC_EVENT_TRADE.to_vec();
        data.extend_from_slice(mint.as_ref());
        data.extend_from_slice(&50_000_000u64.to_le_bytes());
        data.extend_from_slice(&1_000_000u64.to_le_bytes());
        data.push(u8::from(is_buy));
        data.extend_from_slice(user.as_ref());
        data.extend_from_slice(&42i64.to_le_bytes());
        data.extend_from_slice(&456_000_000u64.to_le_bytes());
        data.extend_from_slice(&123_000_000u64.to_le_bytes());
        data
    }

    fn pump_buy_accounts_with_program(program: Pubkey) -> Vec<Pubkey> {
        vec![
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::from_str(ProgramIds::TOKEN_PROGRAM).unwrap(),
            program,
        ]
    }

    fn dummy_accs(n: usize) -> SmallVec<[String; 14]> {
        (0..n)
            .map(|i| format!("Acct{i:04}Pad11111111111111111111111111111"))
            .collect()
    }

    fn decode(data: &[u8], accs: &SmallVec<[String; 14]>, prog: &str) -> Vec<ParsedPumpEvent> {
        let cm = CurveMintRegistry::new();
        let ar = AccountRegistry::new();
        let mut out = Vec::new();
        PumpParser::decode_ix(
            data,
            accs,
            prog,
            1,
            None,
            Instant::now(),
            false,
            false,
            &[],
            &[],
            &[],
            &[],
            &no_keys(),
            &cm,
            &ar,
            &mut out,
        );
        out
    }

    fn decode_cpi(data: &[u8], accs: &SmallVec<[String; 14]>, prog: &str) -> Vec<ParsedPumpEvent> {
        let cm = CurveMintRegistry::new();
        let ar = AccountRegistry::new();
        let mut out = Vec::new();
        PumpParser::decode_ix(
            data,
            accs,
            prog,
            1,
            None,
            Instant::now(),
            false,
            true,
            &[],
            &[],
            &[],
            &[],
            &no_keys(),
            &cm,
            &ar,
            &mut out,
        );
        out
    }

    fn make_decoded_tx_event(
        accounts: Vec<Pubkey>,
        instructions: Vec<crate::types::RawInstruction>,
    ) -> GeyserEvent {
        make_decoded_tx_event_with_inner(accounts, instructions, vec![])
    }

    fn make_decoded_tx_event_with_inner(
        accounts: Vec<Pubkey>,
        instructions: Vec<crate::types::RawInstruction>,
        inner_instructions: Vec<crate::types::InnerInstructionGroup>,
    ) -> GeyserEvent {
        GeyserEvent::Transaction {
            slot: Some(42),
            event_ts_ms: None,
            arrival_ts_ms: Some(crate::types::arrival_time_ms()),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: solana_sdk::signature::Signature::new_unique(),
            accounts,
            instructions,
            logs: vec![],
            block_time: None,
            account_data: HashMap::new(),
            pre_balances: vec![0; 16],
            post_balances: vec![0; 16],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: false,
            source: "curve_resolve".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            inner_instructions,
            pre_token_balances: vec![],
            post_token_balances: vec![],
        }
    }

    fn sample_trade_event(
        signature: solana_sdk::signature::Signature,
        pool_amm_id: Pubkey,
        mint: Pubkey,
        signer: Pubkey,
        event_ordinal: Option<u32>,
    ) -> TradeEvent {
        TradeEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(1),
            signature,
            event_ordinal,
            provenance: None,
            timestamp_ms: 1,
            arrival_ts_ms: 1,
            event_time: ghost_core::EventTimeMetadata::default(),
            pool_amm_id,
            mint,
            signer,
            is_buy: true,
            is_dev_buy: false,
            amount: 123,
            max_sol_cost: 456,
            min_sol_output: 0,
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            market_cap_sol: None,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
            bonding_curve_v2: None,
            bonding_curve_v2_provenance: None,
            is_mayhem_mode: None,
            cu_price_micro_lamports: None,
            compute_unit_limit: None,
            inner_ix_count: None,
            cpi_depth: None,
            ata_create_count: None,
            signer_pre_balance_lamports: None,
            signer_post_balance_lamports: None,
            jito_tip_detected: None,
            toolchain_fingerprint: ToolchainFingerprintInput::default(),
            curve_data_known: true,
            curve_finality: ghost_core::CurveFinality::Provisional,
            is_pumpswap: false,
        }
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
        let signer = Pubkey::new_unique();
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
            inner_ixs.push(crate::types::InnerIx {
                program_id_index: system_index as u8,
                accounts: vec![PUMP_IDX_USER as u8, signer_wsol_index as u8],
                data: system_transfer_data(1_000_000),
                stack_height: Some(2),
            });
        }
        for index in 0..external_fee_count {
            inner_ixs.push(crate::types::InnerIx {
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
            slot: Some(42),
            event_ts_ms: None,
            arrival_ts_ms: Some(crate::types::arrival_time_ms()),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: solana_sdk::signature::Signature::new_unique(),
            accounts,
            instructions: vec![crate::types::RawInstruction {
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
                vec![crate::types::InnerInstructionGroup {
                    index: 0,
                    instructions: inner_ixs,
                }]
            },
            pre_token_balances: vec![],
            post_token_balances: vec![],
        }
    }

    fn encode_swap_sell_event(event: &SwapSellEvent) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 * 14 + 32 * 2);
        out.extend_from_slice(&event.timestamp.to_le_bytes());
        out.extend_from_slice(&event.base_amount_in.to_le_bytes());
        out.extend_from_slice(&event.min_quote_amount_out.to_le_bytes());
        out.extend_from_slice(&event.user_base_token_reserves.to_le_bytes());
        out.extend_from_slice(&event.user_quote_token_reserves.to_le_bytes());
        out.extend_from_slice(&event.pool_base_token_reserves.to_le_bytes());
        out.extend_from_slice(&event.pool_quote_token_reserves.to_le_bytes());
        out.extend_from_slice(&event.quote_amount_out.to_le_bytes());
        out.extend_from_slice(&event.lp_fee_basis_points.to_le_bytes());
        out.extend_from_slice(&event.lp_fee_amount.to_le_bytes());
        out.extend_from_slice(&event.protocol_fee_basis_points.to_le_bytes());
        out.extend_from_slice(&event.protocol_fee_amount.to_le_bytes());
        out.extend_from_slice(&event.quote_amount_out_without_lp_fee.to_le_bytes());
        out.extend_from_slice(&event.user_quote_amount_out.to_le_bytes());
        out.extend_from_slice(&event.pool);
        out.extend_from_slice(&event.user);
        out
    }

    fn encode_swap_buy_event(event: &SwapBuyEvent) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 * 14 + 32 * 2);
        out.extend_from_slice(&event.timestamp.to_le_bytes());
        out.extend_from_slice(&event.base_amount_out.to_le_bytes());
        out.extend_from_slice(&event.max_quote_amount_in.to_le_bytes());
        out.extend_from_slice(&event.user_base_token_reserves.to_le_bytes());
        out.extend_from_slice(&event.user_quote_token_reserves.to_le_bytes());
        out.extend_from_slice(&event.pool_base_token_reserves.to_le_bytes());
        out.extend_from_slice(&event.pool_quote_token_reserves.to_le_bytes());
        out.extend_from_slice(&event.quote_amount_in.to_le_bytes());
        out.extend_from_slice(&event.lp_fee_basis_points.to_le_bytes());
        out.extend_from_slice(&event.lp_fee_amount.to_le_bytes());
        out.extend_from_slice(&event.protocol_fee_basis_points.to_le_bytes());
        out.extend_from_slice(&event.protocol_fee_amount.to_le_bytes());
        out.extend_from_slice(&event.quote_amount_in_with_lp_fee.to_le_bytes());
        out.extend_from_slice(&event.user_quote_amount_in.to_le_bytes());
        out.extend_from_slice(&event.pool);
        out.extend_from_slice(&event.user);
        out
    }

    // ── Create ───────────────────────────────────────────────────────────────

    #[test]
    fn decode_create_ok() {
        let data = create_data("MyToken", "MTK", "https://x.com/m.json");
        let evs = decode(&data, &dummy_accs(12), PUMP_FUN_PROGRAM_ID);
        assert_eq!(evs.len(), 1);
        match &evs[0].kind {
            ParsedEventKind::Create { params, .. } => assert_eq!(params.symbol, "MTK"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn create_registers_curve_and_mint_in_registry() {
        let data = create_data("T", "T", "https://x.com");
        let cm = CurveMintRegistry::new();
        let ar = AccountRegistry::new();
        let mut accs = dummy_accs(12);
        // CREATE instruction layout: Mint=0, BondingCurve=2, Creator=7
        let mint_key = tp(10);
        let curve_key = tp(11);
        accs[CREATE_IDX_MINT] = mint_key.clone();
        accs[CREATE_IDX_BONDING_CURVE] = curve_key.clone();
        let mut out = Vec::new();
        PumpParser::decode_ix(
            &data,
            &accs,
            PUMP_FUN_PROGRAM_ID,
            1,
            None,
            Instant::now(),
            false,
            false,
            &[],
            &[],
            &[],
            &[],
            &no_keys(),
            &cm,
            &ar,
            &mut out,
        );
        assert!(cm.mint_for_curve(&curve_key).is_some());
        assert!(cm.curve_for_mint(&mint_key).is_some());
        let snap = ar.snapshot();
        assert!(snap.contains(&curve_key));
    }

    // ── Buy / Sell ────────────────────────────────────────────────────────────

    #[test]
    fn buy_top_level_source_is_bonding_curve() {
        let evs = decode(
            &trade_data(DISC_BUY, 1_000_000, 50_000_000),
            &dummy_accs(12),
            PUMP_FUN_PROGRAM_ID,
        );
        assert_eq!(evs.len(), 1);
        assert!(!evs[0].from_cpi);
        match &evs[0].kind {
            ParsedEventKind::Trade {
                side: TradeSide::Buy,
                source: TradeSource::BondingCurve,
                token_amount,
                ..
            } => {
                assert_eq!(*token_amount, 1_000_000);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn routed_buy_discriminator_decoded_as_buy() {
        let evs = decode(
            &trade_data(DISC_PUMP_BUY_ROUTED, 1_000_000, 50_000_000),
            &dummy_accs(12),
            PUMP_FUN_PROGRAM_ID,
        );
        assert_eq!(evs.len(), 1);
        match &evs[0].kind {
            ParsedEventKind::Trade {
                side: TradeSide::Buy,
                source: TradeSource::BondingCurve,
                token_amount,
                ..
            } => assert_eq!(*token_amount, 1_000_000),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn buy_cpi_source_is_cpi_direct() {
        let evs = decode_cpi(
            &trade_data(DISC_BUY, 2_000_000, 0),
            &dummy_accs(12),
            PUMP_FUN_PROGRAM_ID,
        );
        assert_eq!(evs.len(), 1);
        assert!(evs[0].from_cpi);
        matches!(
            evs[0].kind,
            ParsedEventKind::Trade {
                source: TradeSource::CpiDirect,
                ..
            }
        );
    }

    #[test]
    fn sell_decoded() {
        let evs = decode(
            &trade_data(DISC_SELL, 500_000, 10_000_000),
            &dummy_accs(12),
            PUMP_FUN_PROGRAM_ID,
        );
        assert_eq!(evs.len(), 1);
        matches!(
            evs[0].kind,
            ParsedEventKind::Trade {
                side: TradeSide::Sell,
                ..
            }
        );
    }

    #[test]
    fn buy_fallback_registers_mapping() {
        let data = trade_data(DISC_BUY, 1, 1);
        let cm = CurveMintRegistry::new();
        let ar = AccountRegistry::new();
        let mut accs = dummy_accs(12);
        let mint_key = tp(20);
        let curve_key = tp(21);
        accs[PUMP_IDX_MINT] = mint_key.clone();
        accs[PUMP_IDX_BONDING_CURVE] = curve_key.clone();
        let mut out = Vec::new();
        PumpParser::decode_ix(
            &data,
            &accs,
            PUMP_FUN_PROGRAM_ID,
            1,
            None,
            Instant::now(),
            false,
            false,
            &[],
            &[],
            &[],
            &[],
            &no_keys(),
            &cm,
            &ar,
            &mut out,
        );
        assert!(
            cm.mint_for_curve(&curve_key).is_some(),
            "Buy must register curve↔mint as fallback when Create was missed"
        );
    }

    // ── Migrate ───────────────────────────────────────────────────────────────

    #[test]
    fn migrate_top_level_decoded() {
        let data = {
            let mut d = DISC_MIGRATE.to_vec();
            d.extend([0u8; 32]);
            d
        };
        let evs = decode(&data, &dummy_accs(14), PUMP_FUN_PROGRAM_ID);
        assert_eq!(evs.len(), 1);
        assert!(!evs[0].from_cpi);
        assert!(matches!(evs[0].kind, ParsedEventKind::Migrate { .. }));
    }

    #[test]
    fn migrate_cpi_decoded_and_flagged() {
        let data = {
            let mut d = DISC_MIGRATE.to_vec();
            d.extend([0u8; 32]);
            d
        };
        let evs = decode_cpi(&data, &dummy_accs(14), PUMP_FUN_PROGRAM_ID);
        assert_eq!(evs.len(), 1);
        assert!(evs[0].from_cpi, "migrate CPI must be flagged from_cpi=true");
        assert!(
            matches!(evs[0].kind, ParsedEventKind::Migrate { .. }),
            "migrate CPI must decode — this is 70-90% of migrate events"
        );
    }

    #[test]
    fn migrate_registers_pool_in_account_registry() {
        let data = {
            let mut d = DISC_MIGRATE.to_vec();
            d.extend([0u8; 32]);
            d
        };
        let ar = AccountRegistry::new();
        let cm = CurveMintRegistry::new();
        let mut accs = dummy_accs(14);
        accs[MIG_IDX_POOL] = "PoolXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX".into();
        let mut out = Vec::new();
        PumpParser::decode_ix(
            &data,
            &accs,
            PUMP_FUN_PROGRAM_ID,
            1,
            None,
            Instant::now(),
            false,
            true,
            &[],
            &[],
            &[],
            &[],
            &no_keys(),
            &cm,
            &ar,
            &mut out,
        );
        assert!(
            ar.snapshot()
                .contains(&"PoolXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX".into()),
            "pool must be registered for PumpSwap account subscriptions"
        );
    }

    // ── Unknown / edge cases ──────────────────────────────────────────────────

    #[test]
    fn unknown_disc_emitted_not_dropped() {
        let mut data = vec![0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x00, 0x00, 0x01];
        data.extend([0u8; 16]);
        let evs = decode(&data, &dummy_accs(4), PUMP_FUN_PROGRAM_ID);
        assert_eq!(evs.len(), 1);
        matches!(evs[0].kind, ParsedEventKind::Unknown { .. });
    }

    #[test]
    fn short_data_produces_no_event() {
        let evs = decode(&[0x01, 0x02], &dummy_accs(4), PUMP_FUN_PROGRAM_ID);
        assert!(evs.is_empty());
    }

    #[test]
    fn non_pump_program_is_ignored() {
        // Hard guard: decode_ix should ignore non-Pump programs even when called directly.
        let data = trade_data(DISC_BUY, 1, 1);
        let evs = decode(
            &data,
            &dummy_accs(12),
            "SomeOtherProgram111111111111111111111111111",
        );
        assert!(
            evs.is_empty(),
            "non-Pump program must be ignored at decode_ix boundary"
        );
    }

    #[test]
    fn buy_dropped_on_oob_accounts() {
        let data = trade_data(DISC_BUY, 1, 1);
        let evs = decode(&data, &dummy_accs(3), PUMP_FUN_PROGRAM_ID);
        assert!(
            evs.is_empty(),
            "buy with OOB account layout must be dropped"
        );
    }

    #[test]
    fn buy_dropped_on_role_mismatch_when_curve_is_wsol() {
        let data = trade_data(DISC_BUY, 1, 1);
        let mut accs = dummy_accs(12);
        accs[PUMP_IDX_MINT] = "MintYY1111111111111111111111111111111111111".into();
        accs[PUMP_IDX_BONDING_CURVE] = WSOL_MINT.into();
        let evs = decode(&data, &accs, PUMP_FUN_PROGRAM_ID);
        assert!(
            evs.is_empty(),
            "curve role mismatch (curve=WSOL) must be dropped"
        );
    }

    #[test]
    fn swap_buy_dropped_when_pool_is_wsol() {
        let data = trade_data(DISC_BUY, 1, 1);
        let mut accs = dummy_accs(12);
        accs[SWAP_IDX_POOL] = WSOL_MINT.into();
        accs[SWAP_IDX_USER] = "User111111111111111111111111111111111111111".into();
        accs[SWAP_IDX_BASE_MINT] = "MintZZ1111111111111111111111111111111111111".into();
        accs[SWAP_IDX_QUOTE_MINT] = WSOL_MINT.into();
        let evs = decode(&data, &accs, PUMP_SWAP_PROGRAM_ID);
        assert!(
            evs.is_empty(),
            "swap trade with pool=WSOL must be dropped as ROLE_MISMATCH"
        );
    }

    #[test]
    fn swap_buy_dropped_when_pair_has_no_wsol() {
        let data = trade_data(DISC_BUY, 1, 1);
        let mut accs = dummy_accs(12);
        accs[SWAP_IDX_POOL] = "Pool111111111111111111111111111111111111111".into();
        accs[SWAP_IDX_USER] = "User111111111111111111111111111111111111111".into();
        accs[SWAP_IDX_BASE_MINT] = "MintAA1111111111111111111111111111111111111".into();
        accs[SWAP_IDX_QUOTE_MINT] = "MintBB1111111111111111111111111111111111111".into();
        let evs = decode(&data, &accs, PUMP_SWAP_PROGRAM_ID);
        assert!(
            evs.is_empty(),
            "swap pair without exactly one WSOL side must be dropped"
        );
    }

    // ── BondingCurveState math ────────────────────────────────────────────────

    #[test]
    fn price_correct() {
        let s = BondingCurveState {
            virtual_token_reserves: 1_000_000_000_000,
            virtual_sol_reserves: 30_000_000_000,
            real_token_reserves: 800_000_000_000,
            real_sol_reserves: 10_000_000_000,
            token_total_supply: 1_000_000_000_000,
            complete: false,
        };
        assert!((s.price_sol_per_token() - 3e-5).abs() < 1e-11);
        assert!((s.progress() - 0.2).abs() < 1e-9);
    }

    #[test]
    fn price_zero_no_panic() {
        let s = BondingCurveState {
            virtual_token_reserves: 0,
            virtual_sol_reserves: 0,
            real_token_reserves: 0,
            real_sol_reserves: 0,
            token_total_supply: 0,
            complete: false,
        };
        assert_eq!(s.price_sol_per_token(), 0.0);
        assert_eq!(s.market_cap_sol(), 0.0);
        assert_eq!(s.progress(), 0.0);
    }

    #[test]
    fn progress_at_graduation() {
        let s = BondingCurveState {
            virtual_token_reserves: 0,
            virtual_sol_reserves: 85_000_000_000,
            real_token_reserves: 0,
            real_sol_reserves: 85_000_000_000,
            token_total_supply: 1_000_000_000_000,
            complete: true,
        };
        assert!((s.progress() - 1.0).abs() < 1e-9);
    }

    // ── CurveMintRegistry ─────────────────────────────────────────────────────

    #[test]
    fn registry_bidirectional() {
        let r = CurveMintRegistry::new();
        let (curve, mint) = (tp(1), tp(2));
        r.insert(&curve, &mint);
        assert_eq!(r.mint_for_curve(&curve).unwrap(), mint);
        assert_eq!(r.curve_for_mint(&mint).unwrap(), curve);
    }

    #[test]
    fn registry_idempotent() {
        let r = CurveMintRegistry::new();
        let (c, m) = (tp(1), tp(2));
        r.insert(&c, &m);
        r.insert(&c, &m);
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn registry_keeps_first_mint_to_curve_mapping() {
        let r = CurveMintRegistry::new();
        let (curve_a, curve_b, mint_x) = (tp(1), tp(2), tp(3));
        r.insert(&curve_a, &mint_x);
        r.insert(&curve_b, &mint_x);
        assert_eq!(
            r.curve_for_mint(&mint_x).as_deref(),
            Some(curve_a.as_str()),
            "mint->curve mapping should remain stable after additional curve inserts for same mint"
        );
    }

    #[test]
    fn registry_does_not_evict_on_growth() {
        let r = CurveMintRegistry::new();
        let (curve_root, mint_root) = (tp(1), tp(2));
        r.insert(&curve_root, &mint_root);
        // fill with other valid pubkeys (seed 10..=255 gives 246 unique pairs)
        for i in 10u8..=255 {
            let c = tp(i);
            let mut mb = [0u8; 32];
            mb[0] = i;
            let m = Pubkey::new_from_array(mb).to_string();
            r.insert(&c, &m);
        }
        assert_eq!(
            r.mint_for_curve(&curve_root).as_deref(),
            Some(mint_root.as_str())
        );
        assert_eq!(
            r.curve_for_mint(&mint_root).as_deref(),
            Some(curve_root.as_str())
        );
    }

    // ── ResolveQueue ─────────────────────────────────────────────────────────

    #[test]
    fn resolve_queue_drains_on_match() {
        let q = ResolveQueue::new(100);
        let cm = CurveMintRegistry::new();
        let (curve1, mint1) = (tp(1), tp(2));
        q.push(curve1.clone(), 1, vec![], Instant::now());
        assert_eq!(q.len(), 1);
        let drained = q.drain_resolved(&cm); // no mapping yet
        assert_eq!(drained.len(), 0);
        assert_eq!(q.len(), 1);
        cm.insert(&curve1, &mint1);
        let drained = q.drain_resolved(&cm);
        assert_eq!(drained.len(), 1);
        assert_eq!(q.len(), 0);
    }

    // resolve_queue_caps_at_max and resolve_queue_default_cap_is_2048 are below in FIX-5 section.

    #[test]
    fn backfill_events_tagged_is_backfill() {
        let ev = PumpEvent::BackfillTransaction {
            signature: "SIG".into(),
            slot: 42,
            received_at: Instant::now(),
            decoded: None,
        };
        let cm = CurveMintRegistry::new();
        let ar = AccountRegistry::new();
        let rq = ResolveQueue::new(100);
        let ct = CompleteTracker::new(); // [FIX-4]
        let results = PumpParser::parse(&ev, &cm, &ar, &rq, &ct);
        assert!(results.is_empty() || results.iter().all(|e| e.is_backfill));
    }

    // ── [FIX-4] CompleteTracker ───────────────────────────────────────────────

    #[test]
    fn complete_tracker_first_true_triggers() {
        let ct = CompleteTracker::new();
        assert!(
            ct.check_and_set(&tp(1), true),
            "first complete=true should trigger MigrateReady"
        );
    }

    #[test]
    fn complete_tracker_duplicate_true_suppressed() {
        let ct = CompleteTracker::new();
        let curve = tp(1);
        ct.check_and_set(&curve, true);
        assert!(
            !ct.check_and_set(&curve, true),
            "second complete=true for same curve should be suppressed (dedup)"
        );
    }

    #[test]
    fn complete_tracker_false_never_triggers() {
        let ct = CompleteTracker::new();
        assert!(!ct.check_and_set(&tp(1), false));
    }

    #[test]
    fn complete_tracker_independent_curves() {
        let ct = CompleteTracker::new();
        let (a, b) = (tp(1), tp(2));
        assert!(ct.check_and_set(&a, true));
        assert!(
            ct.check_and_set(&b, true),
            "independent curves must not interfere"
        );
        assert!(!ct.check_and_set(&a, true), "A already triggered");
    }

    // ── [FIX-1] EntryAnchor event emitted ────────────────────────────────────

    #[test]
    fn entry_raw_emits_entry_anchor() {
        let evs = PumpParser::parse_entry_raw(&[], 100, Instant::now(), 42);
        assert!(
            !evs.is_empty(),
            "parse_entry_raw must emit at least one event"
        );
        let has_anchor = evs.iter().any(|e| {
            matches!(
                e.kind,
                ParsedEventKind::EntryAnchor {
                    executed_transaction_count: 42
                }
            )
        });
        assert!(has_anchor, "must emit EntryAnchor with correct tx count");
    }

    #[test]
    fn entry_anchor_slot_correct() {
        let evs = PumpParser::parse_entry_raw(&[], 999, Instant::now(), 7);
        assert_eq!(evs[0].slot, 999);
    }

    #[test]
    fn parse_initialize_pool_works_without_raw_payload_bytes() {
        let parser = BinaryParser::new(false);
        let mint = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let creator = Keypair::new().pubkey();
        let mut accounts = vec![Pubkey::new_unique(); 12];
        accounts[CREATE_IDX_MINT] = mint;
        accounts[CREATE_IDX_BONDING_CURVE] = curve;
        accounts[CREATE_IDX_USER] = creator;
        let event = make_decoded_tx_event(
            accounts,
            vec![crate::types::RawInstruction {
                program_id: Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap(),
                account_indices: (0u8..12u8).collect(),
                data: create_data("Decoded", "DEC", "https://example.invalid"),
            }],
        );

        let pool = parser
            .parse_initialize_pool(&event)
            .expect("decoded create should parse")
            .expect("decoded create should emit pool");

        assert_eq!(pool.base_mint, mint);
        assert_eq!(pool.pool_amm_id, curve);
        assert_eq!(pool.creator, creator);
    }

    #[test]
    fn parse_initialize_pool_sanitizes_program_creator() {
        let parser = BinaryParser::new(false);
        let mint = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let token_2022 = Pubkey::from_str(ProgramIds::TOKEN_2022_PROGRAM).unwrap();
        let mut accounts = vec![Pubkey::new_unique(); 12];
        accounts[CREATE_IDX_MINT] = mint;
        accounts[CREATE_IDX_BONDING_CURVE] = curve;
        accounts[CREATE_IDX_USER] = token_2022;
        let event = make_decoded_tx_event(
            accounts,
            vec![crate::types::RawInstruction {
                program_id: Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap(),
                account_indices: (0u8..12u8).collect(),
                data: create_data("Decoded", "DEC", "https://example.invalid"),
            }],
        );

        let pool = parser
            .parse_initialize_pool(&event)
            .expect("decoded create should parse")
            .expect("decoded create should emit pool");

        assert_eq!(pool.base_mint, mint);
        assert_eq!(pool.pool_amm_id, curve);
        assert_eq!(pool.creator, Pubkey::default());
    }

    #[test]
    fn parse_trades_works_without_raw_payload_bytes() {
        let parser = BinaryParser::new(false);
        let mint = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let mut accounts = vec![Pubkey::new_unique(); 12];
        accounts[PUMP_IDX_MINT] = mint;
        accounts[PUMP_IDX_BONDING_CURVE] = curve;
        accounts[PUMP_IDX_USER] = user;
        let event = make_decoded_tx_event(
            accounts,
            vec![crate::types::RawInstruction {
                program_id: Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap(),
                account_indices: (0u8..12u8).collect(),
                data: trade_data(DISC_BUY, 1_000_000, 50_000_000),
            }],
        );

        let trades = parser
            .parse_trades(&event)
            .expect("decoded trade should parse");
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].mint, mint);
        assert_eq!(trades[0].pool_amm_id, curve);
        assert_eq!(trades[0].signer, user);
        assert!(trades[0].is_buy);
    }

    #[test]
    fn parse_trades_ftdi_does_not_count_wsol_self_wrap_as_external_fee() {
        let parser = BinaryParser::new(false);
        let event = make_ftdi_buy_event(0, true);

        let trades = parser
            .parse_trades(&event)
            .expect("ftdi fixture should parse");

        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].toolchain_fingerprint.fee_topology(), Some((0, 0)));
        assert_eq!(
            trades[0]
                .toolchain_fingerprint
                .filtered_wsol_self_transfer_count,
            Some(1)
        );
    }

    #[test]
    fn parse_trades_ftdi_distinguishes_external_fee_topologies() {
        let parser = BinaryParser::new(false);

        let topology_00 = parser
            .parse_trades(&make_ftdi_buy_event(0, false))
            .expect("topology (0,0) should parse");
        let topology_10 = parser
            .parse_trades(&make_ftdi_buy_event(1, false))
            .expect("topology (1,0) should parse");
        let topology_20 = parser
            .parse_trades(&make_ftdi_buy_event(2, false))
            .expect("topology (2,0) should parse");

        assert_eq!(topology_00.len(), 1);
        assert_eq!(topology_10.len(), 1);
        assert_eq!(topology_20.len(), 1);
        assert_eq!(
            topology_00[0].toolchain_fingerprint.fee_topology(),
            Some((0, 0))
        );
        assert_eq!(
            topology_10[0].toolchain_fingerprint.fee_topology(),
            Some((1, 0))
        );
        assert_eq!(
            topology_20[0].toolchain_fingerprint.fee_topology(),
            Some((2, 0))
        );
    }

    #[test]
    fn parse_trades_populates_exact_signer_post_balance() {
        let parser = BinaryParser::new(false);
        let event = make_ftdi_buy_event(0, false);

        let trades = parser
            .parse_trades(&event)
            .expect("balance extraction fixture should parse");

        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].signer_pre_balance_lamports, Some(1_500_000_000));
        assert_eq!(trades[0].signer_post_balance_lamports, Some(1_450_000_000));
    }

    #[test]
    fn parse_trades_preserves_success_and_error_code_from_geyser_transaction() {
        let parser = BinaryParser::new(false);
        let mint = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let mut accounts = vec![Pubkey::new_unique(); 12];
        accounts[PUMP_IDX_MINT] = mint;
        accounts[PUMP_IDX_BONDING_CURVE] = curve;
        accounts[PUMP_IDX_USER] = user;
        let mut event = make_decoded_tx_event(
            accounts,
            vec![crate::types::RawInstruction {
                program_id: Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap(),
                account_indices: (0u8..12u8).collect(),
                data: trade_data(DISC_BUY, 1_000_000, 50_000_000),
            }],
        );

        if let GeyserEvent::Transaction {
            success,
            error_code,
            ..
        } = &mut event
        {
            *success = false;
            *error_code = Some("InstructionError(Custom(1))".to_string());
        }

        let trades = parser
            .parse_trades(&event)
            .expect("decoded failed trade should parse");
        assert_eq!(trades.len(), 1);
        assert!(!trades[0].success);
        assert_eq!(
            trades[0].error_code.as_deref(),
            Some("InstructionError(Custom(1))")
        );
    }

    #[test]
    fn parse_trades_prefers_event_ts_ms_over_arrival_time() {
        let parser = BinaryParser::new(false);
        let mint = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let mut accounts = vec![Pubkey::new_unique(); 12];
        accounts[PUMP_IDX_MINT] = mint;
        accounts[PUMP_IDX_BONDING_CURVE] = curve;
        accounts[PUMP_IDX_USER] = user;
        let mut event = make_decoded_tx_event(
            accounts,
            vec![crate::types::RawInstruction {
                program_id: Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap(),
                account_indices: (0u8..12u8).collect(),
                data: trade_data(DISC_BUY, 1_000_000, 50_000_000),
            }],
        );

        if let GeyserEvent::Transaction { event_ts_ms, .. } = &mut event {
            *event_ts_ms = Some(1_777_777_777_000);
        }

        std::thread::sleep(std::time::Duration::from_millis(10));

        let trades = parser.parse_trades(&event).expect("trade should parse");
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].timestamp_ms, 1_777_777_777_000);
    }

    #[test]
    fn event_order_does_not_depend_on_parser_worker_arrival() {
        let parser = BinaryParser::new(false);
        let mint = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let user = Pubkey::new_unique();

        let make_event = |event_ts_ms: u64| {
            let mut accounts = vec![Pubkey::new_unique(); 12];
            accounts[PUMP_IDX_MINT] = mint;
            accounts[PUMP_IDX_BONDING_CURVE] = curve;
            accounts[PUMP_IDX_USER] = user;
            let mut event = make_decoded_tx_event(
                accounts,
                vec![crate::types::RawInstruction {
                    program_id: Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap(),
                    account_indices: (0u8..12u8).collect(),
                    data: trade_data(DISC_BUY, 1_000_000, 50_000_000),
                }],
            );
            if let GeyserEvent::Transaction {
                event_ts_ms: ts, ..
            } = &mut event
            {
                *ts = Some(event_ts_ms);
            }
            event
        };

        let later_parsed_first = make_event(2_000);
        std::thread::sleep(std::time::Duration::from_millis(5));
        let earlier_parsed_second = make_event(1_000);

        let first = parser
            .parse_trades(&later_parsed_first)
            .expect("first trade parse")[0]
            .timestamp_ms;
        std::thread::sleep(std::time::Duration::from_millis(5));
        let second = parser
            .parse_trades(&earlier_parsed_second)
            .expect("second trade parse")[0]
            .timestamp_ms;

        assert_eq!(first, 2_000);
        assert_eq!(second, 1_000);
        assert!(second < first);
    }

    #[test]
    fn parse_trades_cpi_trade_prefers_event_ts_ms_over_cpi_event_timestamp() {
        let parser = BinaryParser::new(false);
        let mint = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let pump_program = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap();
        let mut data = DISC_EVENT_TRADE.to_vec();
        data.extend_from_slice(mint.as_ref());
        data.extend_from_slice(&50_000_000u64.to_le_bytes());
        data.extend_from_slice(&1_000_000u64.to_le_bytes());
        data.push(1u8);
        data.extend_from_slice(user.as_ref());
        data.extend_from_slice(&42i64.to_le_bytes());
        data.extend_from_slice(&456_000_000u64.to_le_bytes());
        data.extend_from_slice(&123_000_000u64.to_le_bytes());

        let event = GeyserEvent::Transaction {
            slot: Some(42),
            event_ts_ms: Some(1_777_777_777_000),
            arrival_ts_ms: Some(9_999),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: solana_sdk::signature::Signature::new_unique(),
            accounts: vec![pump_program],
            instructions: vec![],
            logs: vec![],
            block_time: None,
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: false,
            source: "grpc_global_stream".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: RawBytesMissingReason::FilteredByConfig,
            inner_instructions: vec![crate::types::InnerInstructionGroup {
                index: 0,
                instructions: vec![crate::types::InnerIx {
                    program_id_index: 0,
                    accounts: vec![],
                    data,
                    stack_height: Some(2),
                }],
            }],
            pre_token_balances: vec![],
            post_token_balances: vec![],
        };

        let trades = parser.parse_trades(&event).expect("cpi trade should parse");
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].timestamp_ms, 1_777_777_777_000);
    }

    #[test]
    fn parse_trades_preserves_arrival_ts_ms_separately_from_event_ts_ms() {
        let parser = BinaryParser::new(false);
        let mint = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let mut accounts = vec![Pubkey::new_unique(); 12];
        accounts[PUMP_IDX_MINT] = mint;
        accounts[PUMP_IDX_BONDING_CURVE] = curve;
        accounts[PUMP_IDX_USER] = user;
        let event = GeyserEvent::Transaction {
            slot: Some(42),
            event_ts_ms: Some(1_777_777_777_000),
            arrival_ts_ms: Some(12_345),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: solana_sdk::signature::Signature::new_unique(),
            accounts,
            instructions: vec![crate::types::RawInstruction {
                program_id: Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap(),
                account_indices: (0u8..12u8).collect(),
                data: trade_data(DISC_BUY, 1_000_000, 50_000_000),
            }],
            logs: vec![],
            block_time: None,
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: false,
            source: "grpc_global_stream".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: RawBytesMissingReason::FilteredByConfig,
            inner_instructions: vec![],
            pre_token_balances: vec![],
            post_token_balances: vec![],
        };

        let trades = parser.parse_trades(&event).expect("trade should parse");
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].timestamp_ms, 1_777_777_777_000);
        assert_eq!(trades[0].arrival_ts_ms, 12_345);
    }

    #[test]
    fn parse_trades_supports_jupiter_route_v2_outer_buy_via_balance_fallback() {
        let parser = BinaryParser::new(false);
        let signer = Keypair::new().pubkey();
        let mint = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let user_source_wsol = Pubkey::new_unique();
        let user_destination_token = Pubkey::new_unique();
        let event_authority = Pubkey::new_unique();
        let token_program = Pubkey::from_str(ProgramIds::TOKEN_PROGRAM).unwrap();
        let wsol_mint = Pubkey::from_str(WSOL_MINT).unwrap();

        parser.curve_mint_reg.insert_pk(&curve, &mint);
        parser.account_reg.insert_curve(curve.to_string());

        let event = GeyserEvent::Transaction {
            slot: Some(42),
            event_ts_ms: Some(1_777_777_777_100),
            arrival_ts_ms: Some(12_345),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: solana_sdk::signature::Signature::new_unique(),
            accounts: vec![
                signer,
                user_source_wsol,
                user_destination_token,
                wsol_mint,
                mint,
                token_program,
                token_program,
                user_destination_token,
                event_authority,
            ],
            instructions: vec![crate::types::RawInstruction {
                program_id: Pubkey::from_str(JUPITER_V6_PROGRAM_ID).unwrap(),
                account_indices: (0u8..9u8).collect(),
                data: DISC_JUPITER_ROUTE_V2.to_vec(),
            }],
            logs: vec![],
            block_time: None,
            account_data: HashMap::new(),
            pre_balances: vec![0; 9],
            post_balances: vec![0; 9],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: false,
            source: "grpc_global_stream".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: RawBytesMissingReason::FilteredByConfig,
            inner_instructions: vec![],
            pre_token_balances: vec![
                crate::types::RawTokenBalance {
                    account_index: 1,
                    mint: wsol_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 1_000_000_000,
                },
                crate::types::RawTokenBalance {
                    account_index: 2,
                    mint: mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 0,
                },
            ],
            post_token_balances: vec![
                crate::types::RawTokenBalance {
                    account_index: 1,
                    mint: wsol_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 600_000_000,
                },
                crate::types::RawTokenBalance {
                    account_index: 2,
                    mint: mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 2_500_000,
                },
            ],
        };

        let trades = parser
            .parse_trades(&event)
            .expect("jupiter routeV2 fallback should parse");
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].pool_amm_id, curve);
        assert_eq!(trades[0].mint, mint);
        assert_eq!(trades[0].signer, signer);
        assert!(trades[0].is_buy);
        assert_eq!(trades[0].amount, 2_500_000);
        assert_eq!(trades[0].max_sol_cost, 400_000_000);
        assert_eq!(
            trades[0]
                .provenance
                .as_ref()
                .expect("router fallback provenance")
                .invoked_program_id,
            JUPITER_V6_PROGRAM_ID
        );
    }

    #[test]
    fn parse_trades_supports_jupiter_route_outer_sell_via_balance_fallback() {
        let parser = BinaryParser::new(false);
        let signer = Keypair::new().pubkey();
        let mint = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let user_source_token = Pubkey::new_unique();
        let user_destination_wsol = Pubkey::new_unique();
        let platform_fee = Pubkey::new_unique();
        let event_authority = Pubkey::new_unique();
        let token_program = Pubkey::from_str(ProgramIds::TOKEN_PROGRAM).unwrap();
        let wsol_mint = Pubkey::from_str(WSOL_MINT).unwrap();

        parser.curve_mint_reg.insert_pk(&curve, &mint);
        parser.account_reg.insert_curve(curve.to_string());

        let event = GeyserEvent::Transaction {
            slot: Some(42),
            event_ts_ms: Some(1_777_777_777_200),
            arrival_ts_ms: Some(12_345),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: solana_sdk::signature::Signature::new_unique(),
            accounts: vec![
                token_program,
                signer,
                user_source_token,
                user_destination_wsol,
                user_destination_wsol,
                wsol_mint,
                platform_fee,
                token_program,
                event_authority,
            ],
            instructions: vec![crate::types::RawInstruction {
                program_id: Pubkey::from_str(JUPITER_V6_PROGRAM_ID).unwrap(),
                account_indices: (0u8..9u8).collect(),
                data: DISC_JUPITER_ROUTE.to_vec(),
            }],
            logs: vec![],
            block_time: None,
            account_data: HashMap::new(),
            pre_balances: vec![0; 9],
            post_balances: vec![0; 9],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: false,
            source: "grpc_global_stream".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: RawBytesMissingReason::FilteredByConfig,
            inner_instructions: vec![],
            pre_token_balances: vec![
                crate::types::RawTokenBalance {
                    account_index: 2,
                    mint: mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 3_000_000,
                },
                crate::types::RawTokenBalance {
                    account_index: 3,
                    mint: wsol_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 100_000_000,
                },
            ],
            post_token_balances: vec![
                crate::types::RawTokenBalance {
                    account_index: 2,
                    mint: mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 1_000_000,
                },
                crate::types::RawTokenBalance {
                    account_index: 3,
                    mint: wsol_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 550_000_000,
                },
            ],
        };

        let trades = parser
            .parse_trades(&event)
            .expect("jupiter route fallback should parse");
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].pool_amm_id, curve);
        assert_eq!(trades[0].mint, mint);
        assert_eq!(trades[0].signer, signer);
        assert!(!trades[0].is_buy);
        assert_eq!(trades[0].amount, 2_000_000);
        assert_eq!(trades[0].min_sol_output, 450_000_000);
    }

    #[test]
    fn parse_trades_supports_dflow_swap2_outer_buy_via_balance_fallback() {
        let parser = BinaryParser::new(false);
        let signer = Keypair::new().pubkey();
        let mint = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let event_authority = Pubkey::new_unique();
        let signer_source_wsol = Pubkey::new_unique();
        let signer_destination_token = Pubkey::new_unique();
        let token_program = Pubkey::from_str(ProgramIds::TOKEN_PROGRAM).unwrap();
        let associated_token_program = Pubkey::from_str(ASSOCIATED_TOKEN_PROGRAM_ID).unwrap();
        let system_program = Pubkey::from_str(SYSTEM_PROGRAM_ID).unwrap();
        let wsol_mint = Pubkey::from_str(WSOL_MINT).unwrap();

        parser.curve_mint_reg.insert_pk(&curve, &mint);
        parser.account_reg.insert_curve(curve.to_string());

        let event = GeyserEvent::Transaction {
            slot: Some(42),
            event_ts_ms: Some(1_777_777_777_300),
            arrival_ts_ms: Some(12_345),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: solana_sdk::signature::Signature::new_unique(),
            accounts: vec![
                token_program,
                associated_token_program,
                system_program,
                signer,
                event_authority,
                signer_source_wsol,
                signer_destination_token,
            ],
            instructions: vec![crate::types::RawInstruction {
                program_id: Pubkey::from_str(DFLOW_V4_PROGRAM_ID).unwrap(),
                account_indices: vec![0, 1, 2, 3, 4],
                data: DISC_DFLOW_SWAP2.to_vec(),
            }],
            logs: vec![],
            block_time: None,
            account_data: HashMap::new(),
            pre_balances: vec![0; 7],
            post_balances: vec![0; 7],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: false,
            source: "grpc_global_stream".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: RawBytesMissingReason::FilteredByConfig,
            inner_instructions: vec![],
            pre_token_balances: vec![
                crate::types::RawTokenBalance {
                    account_index: 5,
                    mint: wsol_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 900_000_000,
                },
                crate::types::RawTokenBalance {
                    account_index: 6,
                    mint: mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 0,
                },
            ],
            post_token_balances: vec![
                crate::types::RawTokenBalance {
                    account_index: 5,
                    mint: wsol_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 500_000_000,
                },
                crate::types::RawTokenBalance {
                    account_index: 6,
                    mint: mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 1_750_000,
                },
            ],
        };

        let trades = parser
            .parse_trades(&event)
            .expect("dflow swap2 should parse");
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].pool_amm_id, curve);
        assert_eq!(trades[0].mint, mint);
        assert!(trades[0].is_buy);
        assert_eq!(trades[0].amount, 1_750_000);
        assert_eq!(trades[0].max_sol_cost, 400_000_000);
        assert_eq!(
            trades[0]
                .provenance
                .as_ref()
                .expect("router fallback provenance")
                .invoked_program_id,
            DFLOW_V4_PROGRAM_ID
        );
    }

    #[test]
    fn parse_trades_supports_dflow_swap2_with_destination_native_via_native_sol_fallback() {
        let parser = BinaryParser::new(false);
        let signer = Keypair::new().pubkey();
        let mint = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let event_authority = Pubkey::new_unique();
        let signer_token_account = Pubkey::new_unique();
        let token_program = Pubkey::from_str(ProgramIds::TOKEN_PROGRAM).unwrap();
        let associated_token_program = Pubkey::from_str(ASSOCIATED_TOKEN_PROGRAM_ID).unwrap();
        let system_program = Pubkey::from_str(SYSTEM_PROGRAM_ID).unwrap();

        parser.curve_mint_reg.insert_pk(&curve, &mint);
        parser.account_reg.insert_curve(curve.to_string());

        let mut pre_balances = vec![0; 6];
        let mut post_balances = vec![0; 6];
        pre_balances[3] = 1_000_000_000;
        post_balances[3] = 1_350_000_000;

        let event = GeyserEvent::Transaction {
            slot: Some(42),
            event_ts_ms: Some(1_777_777_777_400),
            arrival_ts_ms: Some(12_345),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: solana_sdk::signature::Signature::new_unique(),
            accounts: vec![
                token_program,
                associated_token_program,
                system_program,
                signer,
                event_authority,
                signer_token_account,
            ],
            instructions: vec![crate::types::RawInstruction {
                program_id: Pubkey::from_str(DFLOW_V4_PROGRAM_ID).unwrap(),
                account_indices: vec![0, 1, 2, 3, 4],
                data: DISC_DFLOW_SWAP2_WITH_DESTINATION_NATIVE.to_vec(),
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
            mpcf_payload_missing_reason: RawBytesMissingReason::FilteredByConfig,
            inner_instructions: vec![],
            pre_token_balances: vec![crate::types::RawTokenBalance {
                account_index: 5,
                mint: mint.to_string(),
                owner: Some(signer.to_string()),
                amount: 3_000_000,
            }],
            post_token_balances: vec![crate::types::RawTokenBalance {
                account_index: 5,
                mint: mint.to_string(),
                owner: Some(signer.to_string()),
                amount: 2_000_000,
            }],
        };

        let trades = parser
            .parse_trades(&event)
            .expect("dflow native destination should parse");
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].pool_amm_id, curve);
        assert_eq!(trades[0].mint, mint);
        assert!(!trades[0].is_buy);
        assert_eq!(trades[0].amount, 1_000_000);
        assert_eq!(trades[0].min_sol_output, 350_000_000);
    }

    #[test]
    fn parse_trades_router_fallback_requires_known_pump_registry_mapping() {
        let parser = BinaryParser::new(false);
        let signer = Keypair::new().pubkey();
        let mint = Pubkey::new_unique();
        let user_source_wsol = Pubkey::new_unique();
        let user_destination_token = Pubkey::new_unique();
        let event_authority = Pubkey::new_unique();
        let token_program = Pubkey::from_str(ProgramIds::TOKEN_PROGRAM).unwrap();
        let wsol_mint = Pubkey::from_str(WSOL_MINT).unwrap();

        let event = GeyserEvent::Transaction {
            slot: Some(42),
            event_ts_ms: Some(1_777_777_777_500),
            arrival_ts_ms: Some(12_345),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: solana_sdk::signature::Signature::new_unique(),
            accounts: vec![
                signer,
                user_source_wsol,
                user_destination_token,
                wsol_mint,
                mint,
                token_program,
                token_program,
                user_destination_token,
                event_authority,
            ],
            instructions: vec![crate::types::RawInstruction {
                program_id: Pubkey::from_str(JUPITER_V6_PROGRAM_ID).unwrap(),
                account_indices: (0u8..9u8).collect(),
                data: DISC_JUPITER_ROUTE_V2.to_vec(),
            }],
            logs: vec![],
            block_time: None,
            account_data: HashMap::new(),
            pre_balances: vec![0; 9],
            post_balances: vec![0; 9],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: false,
            source: "grpc_global_stream".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: RawBytesMissingReason::FilteredByConfig,
            inner_instructions: vec![],
            pre_token_balances: vec![
                crate::types::RawTokenBalance {
                    account_index: 1,
                    mint: wsol_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 1_000_000_000,
                },
                crate::types::RawTokenBalance {
                    account_index: 2,
                    mint: mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 0,
                },
            ],
            post_token_balances: vec![
                crate::types::RawTokenBalance {
                    account_index: 1,
                    mint: wsol_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 750_000_000,
                },
                crate::types::RawTokenBalance {
                    account_index: 2,
                    mint: mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 1_250_000,
                },
            ],
        };

        let trades = parser
            .parse_trades(&event)
            .expect("router fallback without mapping should stay empty");
        assert!(
            trades.is_empty(),
            "generic Jupiter flow without known pump mint must not produce a false trade"
        );
    }

    #[test]
    fn parse_trades_infers_pumpswap_sell_mint_from_signer_token_deltas() {
        let parser = BinaryParser::new(false);
        let signer = Pubkey::new_unique();
        let pool = Pubkey::new_unique();
        let traded_mint = Pubkey::new_unique();
        let quote_mint = Pubkey::new_unique();
        let pool_owner = Pubkey::new_unique();
        let token_program = Pubkey::from_str(ProgramIds::TOKEN_PROGRAM).unwrap();
        let token_2022_program = Pubkey::from_str(ProgramIds::TOKEN_2022_PROGRAM).unwrap();
        let associated_token_program = Pubkey::from_str(ASSOCIATED_TOKEN_PROGRAM_ID).unwrap();

        let signer_traded_ata = Pubkey::find_program_address(
            &[
                signer.as_ref(),
                token_2022_program.as_ref(),
                traded_mint.as_ref(),
            ],
            &associated_token_program,
        )
        .0;
        let signer_quote_ata = Pubkey::find_program_address(
            &[signer.as_ref(), token_program.as_ref(), quote_mint.as_ref()],
            &associated_token_program,
        )
        .0;
        let pool_traded_ata = Pubkey::find_program_address(
            &[
                pool_owner.as_ref(),
                token_2022_program.as_ref(),
                traded_mint.as_ref(),
            ],
            &associated_token_program,
        )
        .0;
        let pool_quote_ata = Pubkey::find_program_address(
            &[
                pool_owner.as_ref(),
                token_program.as_ref(),
                quote_mint.as_ref(),
            ],
            &associated_token_program,
        )
        .0;

        let pumpswap_program = Pubkey::from_str(PUMP_SWAP_PROGRAM_ID).unwrap();
        let sell_event = SwapSellEvent {
            timestamp: 42,
            base_amount_in: 50_000_000,
            min_quote_amount_out: 95_000_000,
            user_base_token_reserves: 50_000_000,
            user_quote_token_reserves: 0,
            pool_base_token_reserves: 1_000_000_000,
            pool_quote_token_reserves: 2_000_000_000,
            quote_amount_out: 95_000_000,
            lp_fee_basis_points: 0,
            lp_fee_amount: 0,
            protocol_fee_basis_points: 0,
            protocol_fee_amount: 0,
            quote_amount_out_without_lp_fee: 95_000_000,
            user_quote_amount_out: 95_000_000,
            pool: pool.to_bytes(),
            user: signer.to_bytes(),
        };

        let mut data = DISC_SWAP_OUTER_WRAPPER.to_vec();
        data.extend_from_slice(&DISC_SWAP_EVENT_SELL);
        data.extend_from_slice(&encode_swap_sell_event(&sell_event));

        let event = GeyserEvent::Transaction {
            slot: Some(42),
            event_ts_ms: None,
            arrival_ts_ms: Some(crate::types::arrival_time_ms()),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: solana_sdk::signature::Signature::new_unique(),
            accounts: vec![
                signer,
                signer_traded_ata,
                signer_quote_ata,
                pool_owner,
                pool_traded_ata,
                pool_quote_ata,
                pumpswap_program,
            ],
            instructions: vec![],
            logs: vec![],
            block_time: None,
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: false,
            source: "grpc_global_stream".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            inner_instructions: vec![crate::types::InnerInstructionGroup {
                index: 0,
                instructions: vec![crate::types::InnerIx {
                    program_id_index: 6,
                    accounts: vec![],
                    data,
                    stack_height: Some(2),
                }],
            }],
            pre_token_balances: vec![
                crate::types::RawTokenBalance {
                    account_index: 1,
                    mint: traded_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 50_000_000,
                },
                crate::types::RawTokenBalance {
                    account_index: 2,
                    mint: quote_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 0,
                },
                crate::types::RawTokenBalance {
                    account_index: 4,
                    mint: traded_mint.to_string(),
                    owner: Some(pool_owner.to_string()),
                    amount: 1_000_000_000,
                },
                crate::types::RawTokenBalance {
                    account_index: 5,
                    mint: quote_mint.to_string(),
                    owner: Some(pool_owner.to_string()),
                    amount: 2_000_000_000,
                },
            ],
            post_token_balances: vec![
                crate::types::RawTokenBalance {
                    account_index: 1,
                    mint: traded_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 0,
                },
                crate::types::RawTokenBalance {
                    account_index: 2,
                    mint: quote_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 95_000_000,
                },
                crate::types::RawTokenBalance {
                    account_index: 4,
                    mint: traded_mint.to_string(),
                    owner: Some(pool_owner.to_string()),
                    amount: 1_050_000_000,
                },
                crate::types::RawTokenBalance {
                    account_index: 5,
                    mint: quote_mint.to_string(),
                    owner: Some(pool_owner.to_string()),
                    amount: 1_905_000_000,
                },
            ],
        };

        let trades = parser
            .parse_trades(&event)
            .expect("pumpswap sell should parse");
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].pool_amm_id, pool);
        assert_eq!(trades[0].mint, traded_mint);
        assert!(!trades[0].is_buy);
        assert_eq!(
            parser
                .curve_mint_registry()
                .mint_for_curve(&pool.to_string()),
            Some(traded_mint.to_string())
        );
    }

    #[test]
    fn parse_trades_infers_pumpswap_sell_mint_from_token_balance_owner() {
        let parser = BinaryParser::new(false);
        let signer = Pubkey::new_unique();
        let pool = Pubkey::new_unique();
        let traded_mint = Pubkey::new_unique();
        let quote_mint = Pubkey::new_unique();
        let pool_owner = Pubkey::new_unique();
        let signer_traded_token = Pubkey::new_unique();
        let signer_quote_token = Pubkey::new_unique();
        let pool_traded_token = Pubkey::new_unique();
        let pool_quote_token = Pubkey::new_unique();
        let pumpswap_program = Pubkey::from_str(PUMP_SWAP_PROGRAM_ID).unwrap();

        let sell_event = SwapSellEvent {
            timestamp: 42,
            base_amount_in: 50_000_000,
            min_quote_amount_out: 95_000_000,
            user_base_token_reserves: 50_000_000,
            user_quote_token_reserves: 0,
            pool_base_token_reserves: 1_000_000_000,
            pool_quote_token_reserves: 2_000_000_000,
            quote_amount_out: 95_000_000,
            lp_fee_basis_points: 0,
            lp_fee_amount: 0,
            protocol_fee_basis_points: 0,
            protocol_fee_amount: 0,
            quote_amount_out_without_lp_fee: 95_000_000,
            user_quote_amount_out: 95_000_000,
            pool: pool.to_bytes(),
            user: signer.to_bytes(),
        };

        let mut data = DISC_SWAP_OUTER_WRAPPER.to_vec();
        data.extend_from_slice(&DISC_SWAP_EVENT_SELL);
        data.extend_from_slice(&encode_swap_sell_event(&sell_event));

        let event = GeyserEvent::Transaction {
            slot: Some(42),
            event_ts_ms: None,
            arrival_ts_ms: Some(crate::types::arrival_time_ms()),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: solana_sdk::signature::Signature::new_unique(),
            accounts: vec![
                signer,
                signer_traded_token,
                signer_quote_token,
                pool_owner,
                pool_traded_token,
                pool_quote_token,
                pumpswap_program,
            ],
            instructions: vec![],
            logs: vec![],
            block_time: None,
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: false,
            source: "grpc_global_stream".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            inner_instructions: vec![crate::types::InnerInstructionGroup {
                index: 0,
                instructions: vec![crate::types::InnerIx {
                    program_id_index: 6,
                    accounts: vec![],
                    data,
                    stack_height: Some(2),
                }],
            }],
            pre_token_balances: vec![
                crate::types::RawTokenBalance {
                    account_index: 1,
                    mint: traded_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 50_000_000,
                },
                crate::types::RawTokenBalance {
                    account_index: 2,
                    mint: quote_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 0,
                },
                crate::types::RawTokenBalance {
                    account_index: 4,
                    mint: traded_mint.to_string(),
                    owner: Some(pool_owner.to_string()),
                    amount: 1_000_000_000,
                },
                crate::types::RawTokenBalance {
                    account_index: 5,
                    mint: quote_mint.to_string(),
                    owner: Some(pool_owner.to_string()),
                    amount: 2_000_000_000,
                },
            ],
            post_token_balances: vec![
                crate::types::RawTokenBalance {
                    account_index: 1,
                    mint: traded_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 0,
                },
                crate::types::RawTokenBalance {
                    account_index: 2,
                    mint: quote_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 95_000_000,
                },
                crate::types::RawTokenBalance {
                    account_index: 4,
                    mint: traded_mint.to_string(),
                    owner: Some(pool_owner.to_string()),
                    amount: 1_050_000_000,
                },
                crate::types::RawTokenBalance {
                    account_index: 5,
                    mint: quote_mint.to_string(),
                    owner: Some(pool_owner.to_string()),
                    amount: 1_905_000_000,
                },
            ],
        };

        let trades = parser
            .parse_trades(&event)
            .expect("pumpswap sell should parse");
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].pool_amm_id, pool);
        assert_eq!(trades[0].mint, traded_mint);
        assert!(!trades[0].is_buy);
    }

    #[test]
    fn decode_swap_sell_event_prefix_matches_live_sample() {
        let payload = bs58::decode(
            "5mPVTm9CVtwmkGePADgpYuqLXCVruQH4napraTLb787V6mfQ4PkXCvW3tCqK9UvicVxHsVuGva8JiUWtV3zosTfCSqqYDfGLGsYPTwc4PPuCwWbv3YrTUYzwxf7P7fv1eu8stASdRtBMLvNLzfeRTzLisiNR3HE1HVbaGMX94NNqR4JcrwwAyDALWksUBh3kVzps5WW9e9J6CZGkUSsTqUXwJmoiVZsoTDkMCekmY8FvATttzQPdUbttYoUDiEmZ2Lkb5aKdZikWFZ7SipDUaUAcEvzri1WsPxxzpMZqwGkoyCE3zEhL8pERQfgbzHGNwFZWBN59bkmDwQWBiwcnwW9BzoCDrotA3u8FebH2ngaWHQZEwRhDGrB7bKtvkFPkELxdNHx7FxxxkCL9RTcfD2FCeynpNNkZVCtHe17K5J53W4f1z9seLrc1nuVBDdVFgPbw9NjDHqvxqVqiZGMsuf7y9XDtzA5pHJspH93GS7dCLVdwGQq6VtYiHxcRgjeE23YkAqNuGTZyH",
        )
        .into_vec()
        .expect("base58 program data");
        let event = match PumpParser::try_decode_cpi_event(&payload, 1, None, Instant::now(), false)
            .expect("wrapped sample should decode")
            .kind
        {
            ParsedEventKind::CpiSwapSell(event) => event,
            other => panic!("unexpected parsed event kind: {other:?}"),
        };

        assert_eq!(event.timestamp, 1_772_841_802);
        assert_eq!(event.base_amount_in, 7_075_401_613);
        assert_eq!(
            Pubkey::new_from_array(event.pool),
            Pubkey::from_str("baUjez5QiztVosh5aT3cLkvzDqd7awjygqa5Nmne1Mp").unwrap()
        );
        assert_eq!(
            Pubkey::new_from_array(event.user),
            Pubkey::from_str("gV5nMy82ynr9vjqpEMiC7sbG7vvWHfh2dTRMvircyYJ").unwrap()
        );
    }

    #[test]
    fn parse_trades_preserves_pumpswap_sell_semantics_from_cpi_sell() {
        let parser = BinaryParser::new(false);
        let signer = Pubkey::new_unique();
        let pool = Pubkey::new_unique();
        let traded_mint = Pubkey::new_unique();
        let wsol_mint = Pubkey::from_str(WSOL_MINT).unwrap();
        let signer_wsol_ata = Pubkey::new_unique();
        let signer_token_ata = Pubkey::new_unique();
        let pool_wsol_ata = Pubkey::new_unique();
        let pool_token_ata = Pubkey::new_unique();
        let pumpswap_program = Pubkey::from_str(PUMP_SWAP_PROGRAM_ID).unwrap();

        let sell_event = SwapSellEvent {
            timestamp: 42,
            base_amount_in: 7_075_401_613,
            min_quote_amount_out: 19_891_310_267_508,
            user_base_token_reserves: 7_075_401_613,
            user_quote_token_reserves: 288_918_625_550,
            pool_base_token_reserves: 265_196_494_818,
            pool_quote_token_reserves: 769_293_443_269_437,
            quote_amount_out: 19_991_266_600_511,
            lp_fee_basis_points: 25,
            lp_fee_amount: 11_996_652_883,
            protocol_fee_basis_points: 5,
            protocol_fee_amount: 9_999_563_301,
            quote_amount_out_without_lp_fee: 20_220_211_426_258,
            user_quote_amount_out: 19_931_292_800_708,
            pool: pool.to_bytes(),
            user: signer.to_bytes(),
        };

        let mut data = DISC_SWAP_OUTER_WRAPPER.to_vec();
        data.extend_from_slice(&DISC_SWAP_EVENT_SELL);
        data.extend_from_slice(&encode_swap_sell_event(&sell_event));

        let event = GeyserEvent::Transaction {
            slot: Some(42),
            event_ts_ms: None,
            arrival_ts_ms: Some(crate::types::arrival_time_ms()),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: solana_sdk::signature::Signature::new_unique(),
            accounts: vec![
                signer,
                signer_wsol_ata,
                signer_token_ata,
                pool_wsol_ata,
                pool_token_ata,
                pumpswap_program,
            ],
            instructions: vec![],
            logs: vec![],
            block_time: None,
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: false,
            source: "grpc_global_stream".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            inner_instructions: vec![crate::types::InnerInstructionGroup {
                index: 0,
                instructions: vec![crate::types::InnerIx {
                    program_id_index: 5,
                    accounts: vec![],
                    data,
                    stack_height: Some(2),
                }],
            }],
            pre_token_balances: vec![
                crate::types::RawTokenBalance {
                    account_index: 1,
                    mint: wsol_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 7_075_401_613,
                },
                crate::types::RawTokenBalance {
                    account_index: 2,
                    mint: traded_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 288_918_625_550,
                },
                crate::types::RawTokenBalance {
                    account_index: 3,
                    mint: wsol_mint.to_string(),
                    owner: Some(pool.to_string()),
                    amount: 265_196_494_818,
                },
                crate::types::RawTokenBalance {
                    account_index: 4,
                    mint: traded_mint.to_string(),
                    owner: Some(pool.to_string()),
                    amount: 769_293_443_269_437,
                },
            ],
            post_token_balances: vec![
                crate::types::RawTokenBalance {
                    account_index: 1,
                    mint: wsol_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 0,
                },
                crate::types::RawTokenBalance {
                    account_index: 2,
                    mint: traded_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 20_220_211_426_258,
                },
                crate::types::RawTokenBalance {
                    account_index: 3,
                    mint: wsol_mint.to_string(),
                    owner: Some(pool.to_string()),
                    amount: 272_271_896_431,
                },
                crate::types::RawTokenBalance {
                    account_index: 4,
                    mint: traded_mint.to_string(),
                    owner: Some(pool.to_string()),
                    amount: 749_362_150_468_729,
                },
            ],
        };

        let trades = parser
            .parse_trades(&event)
            .expect("pumpswap cpi sell should parse");
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].pool_amm_id, pool);
        assert!(
            !trades[0].is_buy,
            "named CpiSwapSell semantics must remain authoritative"
        );
        assert_eq!(trades[0].amount, 7_075_401_613);
        assert_eq!(trades[0].max_sol_cost, 0);
        assert_eq!(trades[0].min_sol_output, 19_991_266_600_511);
        assert!(trades[0].event_ordinal.is_some());
    }

    #[test]
    fn parse_trades_marks_failed_cpi_trade_as_failed() {
        let parser = BinaryParser::new(false);
        let signer = Pubkey::new_unique();
        let pool = Pubkey::new_unique();
        let traded_mint = Pubkey::new_unique();
        let wsol_mint = Pubkey::from_str(WSOL_MINT).unwrap();
        let signer_wsol_ata = Pubkey::new_unique();
        let signer_token_ata = Pubkey::new_unique();
        let pool_wsol_ata = Pubkey::new_unique();
        let pool_token_ata = Pubkey::new_unique();
        let pumpswap_program = Pubkey::from_str(PUMP_SWAP_PROGRAM_ID).unwrap();

        let sell_event = SwapSellEvent {
            timestamp: 42,
            base_amount_in: 7_075_401_613,
            min_quote_amount_out: 19_891_310_267_508,
            user_base_token_reserves: 7_075_401_613,
            user_quote_token_reserves: 288_918_625_550,
            pool_base_token_reserves: 265_196_494_818,
            pool_quote_token_reserves: 769_293_443_269_437,
            quote_amount_out: 19_991_266_600_511,
            lp_fee_basis_points: 25,
            lp_fee_amount: 11_996_652_883,
            protocol_fee_basis_points: 5,
            protocol_fee_amount: 9_999_563_301,
            quote_amount_out_without_lp_fee: 20_220_211_426_258,
            user_quote_amount_out: 19_931_292_800_708,
            pool: pool.to_bytes(),
            user: signer.to_bytes(),
        };

        let mut data = DISC_SWAP_OUTER_WRAPPER.to_vec();
        data.extend_from_slice(&DISC_SWAP_EVENT_SELL);
        data.extend_from_slice(&encode_swap_sell_event(&sell_event));

        let event = GeyserEvent::Transaction {
            slot: Some(42),
            event_ts_ms: None,
            arrival_ts_ms: Some(crate::types::arrival_time_ms()),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: solana_sdk::signature::Signature::new_unique(),
            accounts: vec![
                signer,
                signer_wsol_ata,
                signer_token_ata,
                pool_wsol_ata,
                pool_token_ata,
                pumpswap_program,
            ],
            instructions: vec![],
            logs: vec![],
            block_time: None,
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: false,
            error_code: Some("InstructionError(Custom(1))".to_string()),
            compute_units_consumed: None,
            synthetic: false,
            source: "grpc_global_stream".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            inner_instructions: vec![crate::types::InnerInstructionGroup {
                index: 0,
                instructions: vec![crate::types::InnerIx {
                    program_id_index: 5,
                    accounts: vec![],
                    data,
                    stack_height: Some(2),
                }],
            }],
            pre_token_balances: vec![
                crate::types::RawTokenBalance {
                    account_index: 1,
                    mint: wsol_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 7_075_401_613,
                },
                crate::types::RawTokenBalance {
                    account_index: 2,
                    mint: traded_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 288_918_625_550,
                },
                crate::types::RawTokenBalance {
                    account_index: 3,
                    mint: wsol_mint.to_string(),
                    owner: Some(pool.to_string()),
                    amount: 265_196_494_818,
                },
                crate::types::RawTokenBalance {
                    account_index: 4,
                    mint: traded_mint.to_string(),
                    owner: Some(pool.to_string()),
                    amount: 769_293_443_269_437,
                },
            ],
            post_token_balances: vec![
                crate::types::RawTokenBalance {
                    account_index: 1,
                    mint: wsol_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 0,
                },
                crate::types::RawTokenBalance {
                    account_index: 2,
                    mint: traded_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 20_220_211_426_258,
                },
                crate::types::RawTokenBalance {
                    account_index: 3,
                    mint: wsol_mint.to_string(),
                    owner: Some(pool.to_string()),
                    amount: 272_271_896_431,
                },
                crate::types::RawTokenBalance {
                    account_index: 4,
                    mint: traded_mint.to_string(),
                    owner: Some(pool.to_string()),
                    amount: 749_362_150_468_729,
                },
            ],
        };

        let trades = parser
            .parse_trades(&event)
            .expect("failed pumpswap cpi sell should parse");
        assert_eq!(trades.len(), 1);
        assert!(!trades[0].success);
        assert_eq!(
            trades[0].error_code.as_deref(),
            Some("InstructionError(Custom(1))")
        );
    }

    #[test]
    fn dedup_prefers_pumpswap_cpi_variant_and_uses_registry_mapping() {
        let _guard = metrics_test_lock().lock().expect("metrics test lock");
        clear_recorded_counters();
        let cm = CurveMintRegistry::new();
        let pool = Pubkey::new_unique();
        let kept_mint = Pubkey::new_unique();
        let other_mint = Pubkey::new_unique();
        cm.insert(pool.to_string(), kept_mint.to_string());
        let mut out = vec![
            ParsedPumpEvent {
                received_at: Instant::now(),
                slot: 1,
                signature: None,
                event_ordinal: None,
                provenance: None,
                from_cpi: false,
                is_backfill: false,
                kind: ParsedEventKind::SwapTrade {
                    side: TradeSide::Buy,
                    pool: pool.to_string(),
                    base_mint: kept_mint.to_string(),
                    quote_mint: WSOL_MINT.to_string(),
                    user: Pubkey::new_unique().to_string(),
                    base_amount: 123,
                    quote_amount: 456,
                },
            },
            ParsedPumpEvent {
                received_at: Instant::now(),
                slot: 1,
                signature: None,
                event_ordinal: None,
                provenance: None,
                from_cpi: true,
                is_backfill: false,
                kind: ParsedEventKind::CpiSwapBuy(SwapBuyEvent {
                    timestamp: 1,
                    base_amount_out: 123,
                    max_quote_amount_in: 456,
                    user_base_token_reserves: 0,
                    user_quote_token_reserves: 0,
                    pool_base_token_reserves: 1,
                    pool_quote_token_reserves: 1,
                    quote_amount_in: 456,
                    lp_fee_basis_points: 0,
                    lp_fee_amount: 0,
                    protocol_fee_basis_points: 0,
                    protocol_fee_amount: 0,
                    quote_amount_in_with_lp_fee: 456,
                    user_quote_amount_in: 456,
                    pool: pool.to_bytes(),
                    user: Pubkey::new_unique().to_bytes(),
                }),
            },
            ParsedPumpEvent {
                received_at: Instant::now(),
                slot: 1,
                signature: None,
                event_ordinal: None,
                provenance: None,
                from_cpi: false,
                is_backfill: false,
                kind: ParsedEventKind::SwapTrade {
                    side: TradeSide::Buy,
                    pool: pool.to_string(),
                    base_mint: other_mint.to_string(),
                    quote_mint: WSOL_MINT.to_string(),
                    user: Pubkey::new_unique().to_string(),
                    base_amount: 999,
                    quote_amount: 111,
                },
            },
        ];

        dedup_trade_events(&mut out, &cm);

        assert_eq!(out.len(), 2);
        assert!(out
            .iter()
            .any(|ev| matches!(&ev.kind, ParsedEventKind::SwapTrade { base_mint, .. } if base_mint == &other_mint.to_string())));
        assert!(out
            .iter()
            .any(|ev| matches!(ev.kind, ParsedEventKind::CpiSwapBuy(_))));
        assert!(
            !out.iter()
                .any(|ev| matches!(&ev.kind, ParsedEventKind::SwapTrade { base_mint, .. } if base_mint == &kept_mint.to_string())),
            "registry-resolved SwapTrade duplicate should be dropped when a matching CPI event exists"
        );
        assert!(saw_counter(
            TRADE_EVENT_DEDUP_TOTAL,
            &[
                ("event_kind", "swap_trade"),
                ("decision", "dropped"),
                ("reason", "drop_ix_swap_because_matching_cpi_swap"),
            ],
        ));
        assert!(saw_counter(
            TRADE_EVENT_DEDUP_TOTAL,
            &[
                ("event_kind", "swap_trade"),
                ("decision", "kept"),
                ("reason", "keep_ix_swap_because_no_matching_cpi_swap"),
            ],
        ));
    }

    #[test]
    fn dedup_keeps_ix_swap_when_cpi_has_unresolved_mint() {
        let _guard = metrics_test_lock().lock().expect("metrics test lock");
        clear_recorded_counters();

        let cm = CurveMintRegistry::new();
        let pool = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let mut out = vec![
            ParsedPumpEvent {
                received_at: Instant::now(),
                slot: 1,
                signature: None,
                event_ordinal: None,
                provenance: None,
                from_cpi: false,
                is_backfill: false,
                kind: ParsedEventKind::SwapTrade {
                    side: TradeSide::Buy,
                    pool: pool.to_string(),
                    base_mint: base_mint.to_string(),
                    quote_mint: WSOL_MINT.to_string(),
                    user: Pubkey::new_unique().to_string(),
                    base_amount: 123,
                    quote_amount: 456,
                },
            },
            ParsedPumpEvent {
                received_at: Instant::now(),
                slot: 1,
                signature: None,
                event_ordinal: None,
                provenance: None,
                from_cpi: true,
                is_backfill: false,
                kind: ParsedEventKind::CpiSwapBuy(SwapBuyEvent {
                    timestamp: 1,
                    base_amount_out: 123,
                    max_quote_amount_in: 456,
                    user_base_token_reserves: 0,
                    user_quote_token_reserves: 0,
                    pool_base_token_reserves: 1,
                    pool_quote_token_reserves: 1,
                    quote_amount_in: 456,
                    lp_fee_basis_points: 0,
                    lp_fee_amount: 0,
                    protocol_fee_basis_points: 0,
                    protocol_fee_amount: 0,
                    quote_amount_in_with_lp_fee: 456,
                    user_quote_amount_in: 456,
                    pool: pool.to_bytes(),
                    user: Pubkey::new_unique().to_bytes(),
                }),
            },
        ];

        dedup_trade_events(&mut out, &cm);

        assert_eq!(out.len(), 2);
        assert!(saw_counter(
            TRADE_EVENT_DEDUP_TOTAL,
            &[
                ("event_kind", "swap_trade"),
                ("decision", "kept"),
                ("reason", "keep_ix_swap_because_cpi_has_unresolved_mint"),
            ],
        ));
    }

    #[test]
    fn dedup_drops_zero_amount_ix_swap_when_cpi_has_unresolved_mint() {
        let _guard = metrics_test_lock().lock().expect("metrics test lock");
        clear_recorded_counters();

        let cm = CurveMintRegistry::new();
        let pool = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let mut out = vec![
            ParsedPumpEvent {
                received_at: Instant::now(),
                slot: 1,
                signature: None,
                event_ordinal: None,
                provenance: None,
                from_cpi: false,
                is_backfill: false,
                kind: ParsedEventKind::SwapTrade {
                    side: TradeSide::Buy,
                    pool: pool.to_string(),
                    base_mint: base_mint.to_string(),
                    quote_mint: WSOL_MINT.to_string(),
                    user: Keypair::new().pubkey().to_string(),
                    base_amount: 0,
                    quote_amount: 0,
                },
            },
            ParsedPumpEvent {
                received_at: Instant::now(),
                slot: 1,
                signature: None,
                event_ordinal: None,
                provenance: None,
                from_cpi: true,
                is_backfill: false,
                kind: ParsedEventKind::CpiSwapBuy(SwapBuyEvent {
                    timestamp: 1,
                    base_amount_out: 123,
                    max_quote_amount_in: 456,
                    user_base_token_reserves: 0,
                    user_quote_token_reserves: 0,
                    pool_base_token_reserves: 1,
                    pool_quote_token_reserves: 1,
                    quote_amount_in: 456,
                    lp_fee_basis_points: 0,
                    lp_fee_amount: 0,
                    protocol_fee_basis_points: 0,
                    protocol_fee_amount: 0,
                    quote_amount_in_with_lp_fee: 456,
                    user_quote_amount_in: 456,
                    pool: pool.to_bytes(),
                    user: Keypair::new().pubkey().to_bytes(),
                }),
            },
        ];

        dedup_trade_events(&mut out, &cm);

        assert_eq!(out.len(), 1);
        assert!(matches!(out[0].kind, ParsedEventKind::CpiSwapBuy(_)));
        assert!(saw_counter(
            TRADE_EVENT_DEDUP_TOTAL,
            &[
                ("event_kind", "swap_trade"),
                ("decision", "dropped"),
                ("reason", "drop_ix_swap_because_matching_cpi_swap"),
            ],
        ));
    }

    #[test]
    fn parsed_event_dedup_keeps_higher_confidence_cpi() {
        let cm = CurveMintRegistry::new();
        let curve = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        cm.insert(curve.to_string(), mint.to_string());

        let mut out = vec![
            ParsedPumpEvent {
                received_at: Instant::now(),
                slot: 1,
                signature: None,
                event_ordinal: None,
                provenance: None,
                from_cpi: false,
                is_backfill: false,
                kind: ParsedEventKind::Trade {
                    side: TradeSide::Buy,
                    source: TradeSource::BondingCurve,
                    mint: mint.to_string(),
                    bonding_curve: curve.to_string(),
                    user: Pubkey::new_unique().to_string(),
                    token_amount: 123,
                    sol_amount: 456,
                    virtual_token_reserves: 1,
                    virtual_sol_reserves: 1,
                    real_token_reserves: 1,
                    real_sol_reserves: 1,
                    market_cap_sol: 1.0,
                    global_config: None,
                    fee_recipient: None,
                    token_program: None,
                    progress: 0.5,
                    is_complete: false,
                },
            },
            ParsedPumpEvent {
                received_at: Instant::now(),
                slot: 1,
                signature: None,
                event_ordinal: None,
                provenance: None,
                from_cpi: true,
                is_backfill: false,
                kind: ParsedEventKind::CpiTrade(EventTrade {
                    mint: mint.to_bytes(),
                    sol_amount: 456,
                    token_amount: 123,
                    is_buy: true,
                    user: Pubkey::new_unique().to_bytes(),
                    timestamp: 1,
                    virtual_sol_reserves: 1,
                    virtual_token_reserves: 1,
                }),
            },
        ];

        dedup_trade_events(&mut out, &cm);

        assert_eq!(out.len(), 1);
        assert!(matches!(out[0].kind, ParsedEventKind::CpiTrade(_)));
    }

    #[test]
    fn dedup_trade_events_emits_decision_metrics() {
        let _guard = metrics_test_lock().lock().expect("metrics test lock");
        clear_recorded_counters();

        let cm = CurveMintRegistry::new();
        let curve = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        cm.insert(curve.to_string(), mint.to_string());

        let mut out = vec![
            ParsedPumpEvent {
                received_at: Instant::now(),
                slot: 1,
                signature: None,
                event_ordinal: None,
                provenance: None,
                from_cpi: false,
                is_backfill: false,
                kind: ParsedEventKind::Trade {
                    side: TradeSide::Buy,
                    source: TradeSource::BondingCurve,
                    mint: mint.to_string(),
                    bonding_curve: curve.to_string(),
                    user: Pubkey::new_unique().to_string(),
                    token_amount: 123,
                    sol_amount: 456,
                    virtual_token_reserves: 1,
                    virtual_sol_reserves: 1,
                    real_token_reserves: 1,
                    real_sol_reserves: 1,
                    market_cap_sol: 1.0,
                    global_config: None,
                    fee_recipient: None,
                    token_program: None,
                    progress: 0.5,
                    is_complete: false,
                },
            },
            ParsedPumpEvent {
                received_at: Instant::now(),
                slot: 1,
                signature: None,
                event_ordinal: None,
                provenance: None,
                from_cpi: true,
                is_backfill: false,
                kind: ParsedEventKind::CpiTrade(EventTrade {
                    mint: mint.to_bytes(),
                    sol_amount: 456,
                    token_amount: 123,
                    is_buy: true,
                    user: Pubkey::new_unique().to_bytes(),
                    timestamp: 1,
                    virtual_sol_reserves: 1,
                    virtual_token_reserves: 1,
                }),
            },
        ];

        dedup_trade_events(&mut out, &cm);

        assert!(saw_counter(
            TRADE_EVENT_DEDUP_TOTAL,
            &[
                ("event_kind", "trade"),
                ("decision", "dropped"),
                ("reason", "drop_ix_trade_because_matching_cpi_trade"),
            ],
        ));
        assert!(saw_counter(
            TRADE_EVENT_DEDUP_TOTAL,
            &[
                ("event_kind", "cpi_trade"),
                ("decision", "kept"),
                ("reason", "keep_cpi_trade_as_structural_candidate"),
            ],
        ));
        assert!(saw_counter(
            TRADE_EVENT_DEDUP_STAGE_TOTAL,
            &[("stage", "input")],
        ));
        assert!(saw_counter(
            TRADE_EVENT_DEDUP_STAGE_TOTAL,
            &[("stage", "output")],
        ));
    }

    #[test]
    fn orphan_inner_group_is_counted_not_crashed() {
        let _guard = metrics_test_lock().lock().expect("metrics test lock");
        clear_recorded_counters();

        let cm = CurveMintRegistry::new();
        let ar = AccountRegistry::new();
        let rq = ResolveQueue::with_default_cap();
        let accounts = vec![Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap()];
        let inner_instructions = vec![crate::types::InnerInstructionGroup {
            index: 9,
            instructions: vec![crate::types::InnerIx {
                program_id_index: 0,
                accounts: vec![],
                data: vec![],
                stack_height: Some(2),
            }],
        }];

        let out = PumpParser::parse_geyser_transaction(
            &accounts,
            &[],
            &inner_instructions,
            &[],
            &[],
            &[],
            &[],
            None,
            42,
            Instant::now(),
            false,
            &cm,
            &ar,
            &rq,
        );

        assert!(out.is_empty());
        assert!(saw_counter(
            ORPHAN_INNER_GROUP_TOTAL,
            &[("walker", "geyser")]
        ));
        assert!(saw_counter(
            MISSING_PROVENANCE_TOTAL,
            &[("walker", "geyser"), ("reason", "outer_program_missing")],
        ));
    }

    #[test]
    fn inner_group_index_is_bound_to_outer_program() {
        let parser = BinaryParser::new(false);
        let pump_program = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap();
        let accounts = pump_buy_accounts_with_program(pump_program);
        let event = make_decoded_tx_event_with_inner(
            accounts,
            vec![crate::types::RawInstruction {
                program_id: pump_program,
                account_indices: vec![],
                data: vec![],
            }],
            vec![crate::types::InnerInstructionGroup {
                index: 0,
                instructions: vec![crate::types::InnerIx {
                    program_id_index: 9,
                    accounts: (0u8..9u8).collect(),
                    data: trade_data(DISC_BUY, 1_000_000, 50_000_000),
                    stack_height: Some(2),
                }],
            }],
        );

        let parsed = parser.parse_pump_events(&event);
        let trade = parsed
            .iter()
            .find(|event| event.from_cpi && matches!(event.kind, ParsedEventKind::Trade { .. }))
            .expect("inner pump trade should exist");
        let provenance = trade.provenance.as_ref().expect("inner trade provenance");

        assert_eq!(provenance.outer_instruction_index, Some(0));
        assert_eq!(provenance.inner_group_index, Some(0));
        assert_eq!(
            provenance.outer_program_id.as_deref(),
            Some(PUMP_FUN_PROGRAM_ID)
        );
        assert_eq!(provenance.invoked_program_id, PUMP_FUN_PROGRAM_ID);
        assert_eq!(provenance.stack_height, Some(2));
        assert!(provenance.from_cpi);
    }

    #[test]
    fn non_pump_outer_with_inner_pump_preserves_provenance() {
        let parser = BinaryParser::new(false);
        let pump_program = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap();
        let outer_program = Pubkey::from_str(SYSTEM_PROGRAM_ID).unwrap();
        let accounts = pump_buy_accounts_with_program(pump_program);
        let event = make_decoded_tx_event_with_inner(
            accounts,
            vec![crate::types::RawInstruction {
                program_id: outer_program,
                account_indices: vec![],
                data: vec![],
            }],
            vec![crate::types::InnerInstructionGroup {
                index: 0,
                instructions: vec![crate::types::InnerIx {
                    program_id_index: 9,
                    accounts: (0u8..9u8).collect(),
                    data: trade_data(DISC_BUY, 1_000_000, 50_000_000),
                    stack_height: Some(2),
                }],
            }],
        );

        let parsed = parser.parse_pump_events(&event);
        let trade = parsed
            .iter()
            .find(|event| event.from_cpi && matches!(event.kind, ParsedEventKind::Trade { .. }))
            .expect("inner pump trade should exist");
        let provenance = trade.provenance.as_ref().expect("inner trade provenance");

        assert_eq!(provenance.outer_instruction_index, Some(0));
        assert_eq!(provenance.inner_group_index, Some(0));
        assert_eq!(
            provenance.outer_program_id.as_deref(),
            Some(SYSTEM_PROGRAM_ID)
        );
        assert_eq!(provenance.invoked_program_id, PUMP_FUN_PROGRAM_ID);
        assert_eq!(provenance.stack_height, Some(2));
        assert!(provenance.from_cpi);
    }

    #[test]
    fn cpi_event_provenance_survives_event_build() {
        let parser = BinaryParser::new(false);
        let pump_program = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap();
        let accounts = pump_buy_accounts_with_program(pump_program);
        let mint = accounts[2];
        let user = accounts[6];
        let event = make_decoded_tx_event_with_inner(
            accounts,
            vec![crate::types::RawInstruction {
                program_id: pump_program,
                account_indices: vec![],
                data: vec![],
            }],
            vec![crate::types::InnerInstructionGroup {
                index: 0,
                instructions: vec![crate::types::InnerIx {
                    program_id_index: 9,
                    accounts: vec![],
                    data: cpi_trade_payload(mint, user, true),
                    stack_height: Some(3),
                }],
            }],
        );

        let parsed = parser.parse_pump_events(&event);
        let cpi_trade = parsed
            .iter()
            .find(|event| matches!(event.kind, ParsedEventKind::CpiTrade(_)))
            .expect("cpi trade should exist");
        let provenance = cpi_trade.provenance.as_ref().expect("cpi provenance");

        assert_eq!(provenance.outer_instruction_index, Some(0));
        assert_eq!(provenance.inner_group_index, Some(0));
        assert_eq!(
            provenance.outer_program_id.as_deref(),
            Some(PUMP_FUN_PROGRAM_ID)
        );
        assert_eq!(provenance.invoked_program_id, PUMP_FUN_PROGRAM_ID);
        assert_eq!(provenance.stack_height, Some(3));
        assert!(provenance.from_cpi);
    }

    #[test]
    fn trade_event_preserves_provenance_from_cpi_source() {
        let parser = BinaryParser::new(false);
        let pump_program = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap();
        let accounts = pump_buy_accounts_with_program(pump_program);
        let mint = accounts[2];
        let user = accounts[6];
        let event = make_decoded_tx_event_with_inner(
            accounts,
            vec![crate::types::RawInstruction {
                program_id: pump_program,
                account_indices: vec![],
                data: vec![],
            }],
            vec![crate::types::InnerInstructionGroup {
                index: 0,
                instructions: vec![crate::types::InnerIx {
                    program_id_index: 9,
                    accounts: vec![],
                    data: cpi_trade_payload(mint, user, true),
                    stack_height: Some(3),
                }],
            }],
        );

        let trades = parser.parse_trades(&event).expect("cpi trade should parse");
        let trade = trades.first().expect("cpi trade should be forwarded");
        let provenance = trade.provenance.as_ref().expect("trade provenance");

        assert_eq!(provenance.outer_instruction_index, Some(0));
        assert_eq!(provenance.inner_group_index, Some(0));
        assert_eq!(
            provenance.outer_program_id.as_deref(),
            Some(PUMP_FUN_PROGRAM_ID)
        );
        assert_eq!(provenance.invoked_program_id, PUMP_FUN_PROGRAM_ID);
        assert_eq!(provenance.stack_height, Some(3));
        assert!(provenance.from_cpi);
    }

    #[test]
    fn live_and_decoded_paths_assign_compatible_event_ordinals() {
        let parser = BinaryParser::new(false);
        let pump_program = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap();
        let outer_program = Pubkey::from_str(COMPUTE_BUDGET_PROGRAM_ID).unwrap();
        let accounts = pump_buy_accounts_with_program(pump_program);
        let instructions = vec![crate::types::RawInstruction {
            program_id: outer_program,
            account_indices: vec![],
            data: vec![],
        }];
        let inner_instructions = vec![crate::types::InnerInstructionGroup {
            index: 0,
            instructions: vec![
                crate::types::InnerIx {
                    program_id_index: 9,
                    accounts: (0u8..9u8).collect(),
                    data: trade_data(DISC_BUY, 1_000_000, 50_000_000),
                    stack_height: Some(2),
                },
                crate::types::InnerIx {
                    program_id_index: 9,
                    accounts: (0u8..9u8).collect(),
                    data: trade_data(DISC_SELL, 2_000_000, 40_000_000),
                    stack_height: Some(2),
                },
            ],
        }];
        let event = make_decoded_tx_event_with_inner(
            accounts.clone(),
            instructions.clone(),
            inner_instructions.clone(),
        );

        let live = parser.parse_pump_events(&event);
        let decoded = PumpParser::parse_geyser_transaction(
            &accounts,
            &instructions,
            &inner_instructions,
            &[],
            &[],
            &[],
            &[],
            None,
            42,
            Instant::now(),
            false,
            parser.curve_mint_registry(),
            parser.account_registry(),
            &parser.resolve_queue,
        );

        let live_ordinals: Vec<_> = live.iter().map(|event| event.event_ordinal).collect();
        let decoded_ordinals: Vec<_> = decoded.iter().map(|event| event.event_ordinal).collect();
        let live_provenance: Vec<_> = live.iter().map(|event| event.provenance.clone()).collect();
        let decoded_provenance: Vec<_> = decoded
            .iter()
            .map(|event| event.provenance.clone())
            .collect();

        assert_eq!(live_ordinals, decoded_ordinals);
        assert_eq!(live_provenance, decoded_provenance);
        assert_eq!(live_ordinals, vec![Some(1), Some(2)]);
    }

    #[test]
    fn parse_trades_prefers_cpi_swap_with_token_account_mint_resolution() {
        let parser = BinaryParser::new(false);
        let signer = Pubkey::new_unique();
        let actual_pool = Pubkey::new_unique();
        let routed_pool_account = Pubkey::new_unique();
        let traded_mint = Pubkey::new_unique();
        // PumpSwap pools are WSOL-quoted; use WSOL here so SwapTrade is produced,
        // enabling the dedup path to actually exercise ordinal sharing.
        let quote_mint = Pubkey::from_str(WSOL_MINT).unwrap();
        let global_config = Pubkey::new_unique();
        let user_base_token = Pubkey::new_unique();
        let user_quote_token = Pubkey::new_unique();
        let pool_base_token = Pubkey::new_unique();
        let pool_quote_token = Pubkey::new_unique();
        let token_program = Pubkey::from_str(ProgramIds::TOKEN_PROGRAM).unwrap();
        let pumpswap_program = Pubkey::from_str(PUMP_SWAP_PROGRAM_ID).unwrap();

        let mut cpi_data = DISC_SWAP_OUTER_WRAPPER.to_vec();
        cpi_data.extend_from_slice(&DISC_SWAP_EVENT_BUY);
        cpi_data.extend_from_slice(&encode_swap_buy_event(&SwapBuyEvent {
            timestamp: 42,
            base_amount_out: 250_000,
            max_quote_amount_in: 900_000,
            user_base_token_reserves: 0,
            user_quote_token_reserves: 1_000_000,
            pool_base_token_reserves: 5_000_000,
            pool_quote_token_reserves: 8_000_000,
            quote_amount_in: 900_000,
            lp_fee_basis_points: 0,
            lp_fee_amount: 0,
            protocol_fee_basis_points: 0,
            protocol_fee_amount: 0,
            quote_amount_in_with_lp_fee: 900_000,
            user_quote_amount_in: 900_000,
            pool: actual_pool.to_bytes(),
            user: signer.to_bytes(),
        }));

        let event = GeyserEvent::Transaction {
            slot: Some(42),
            event_ts_ms: None,
            arrival_ts_ms: Some(crate::types::arrival_time_ms()),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: solana_sdk::signature::Signature::new_unique(),
            accounts: vec![
                // The outer routed account is what ix-level decoding sees as the pool.
                // CPI event log carries the *actual* pool pubkey.
                routed_pool_account,
                signer,
                global_config,
                traded_mint,
                quote_mint,
                user_base_token,
                user_quote_token,
                pool_base_token,
                pool_quote_token,
                Pubkey::new_unique(),
                Pubkey::new_unique(),
                token_program,
                token_program,
                pumpswap_program,
            ],
            instructions: vec![],
            logs: vec![],
            block_time: None,
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: false,
            source: "grpc_global_stream".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            inner_instructions: vec![crate::types::InnerInstructionGroup {
                index: 0,
                instructions: vec![
                    crate::types::InnerIx {
                        program_id_index: 13,
                        accounts: vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 11, 12],
                        data: DISC_SWAP_BUY_EXACT_QUOTE_IN.to_vec(),
                        stack_height: Some(2),
                    },
                    crate::types::InnerIx {
                        program_id_index: 13,
                        accounts: vec![],
                        data: cpi_data,
                        stack_height: Some(3),
                    },
                ],
            }],
            pre_token_balances: vec![
                crate::types::RawTokenBalance {
                    account_index: 5,
                    mint: traded_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 0,
                },
                crate::types::RawTokenBalance {
                    account_index: 6,
                    mint: quote_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 1_000_000,
                },
                crate::types::RawTokenBalance {
                    account_index: 7,
                    mint: traded_mint.to_string(),
                    owner: Some(Pubkey::new_unique().to_string()),
                    amount: 5_000_000,
                },
                crate::types::RawTokenBalance {
                    account_index: 8,
                    mint: quote_mint.to_string(),
                    owner: Some(Pubkey::new_unique().to_string()),
                    amount: 8_000_000,
                },
            ],
            post_token_balances: vec![
                crate::types::RawTokenBalance {
                    account_index: 5,
                    mint: traded_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 250_000,
                },
                crate::types::RawTokenBalance {
                    account_index: 6,
                    mint: quote_mint.to_string(),
                    owner: Some(signer.to_string()),
                    amount: 100_000,
                },
                crate::types::RawTokenBalance {
                    account_index: 7,
                    mint: traded_mint.to_string(),
                    owner: Some(Pubkey::new_unique().to_string()),
                    amount: 4_750_000,
                },
                crate::types::RawTokenBalance {
                    account_index: 8,
                    mint: quote_mint.to_string(),
                    owner: Some(Pubkey::new_unique().to_string()),
                    amount: 8_900_000,
                },
            ],
        };

        let trades = parser
            .parse_trades(&event)
            .expect("pumpswap routed buy should parse");
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].pool_amm_id, actual_pool);
        assert_eq!(trades[0].mint, traded_mint);
        assert!(trades[0].is_buy);
        assert_eq!(
            parser
                .curve_mint_registry()
                .mint_for_curve(&actual_pool.to_string()),
            Some(traded_mint.to_string())
        );
    }

    #[test]
    fn dedup_trade_candidates_drops_unresolved_duplicate_even_with_bad_signer() {
        let cm = CurveMintRegistry::new();
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let signature = solana_sdk::signature::Signature::new_unique();

        let resolved = TradeEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(1),
            signature: signature.clone(),
            event_ordinal: Some(0),
            provenance: None,
            timestamp_ms: 1,
            arrival_ts_ms: 1,
            event_time: ghost_core::EventTimeMetadata::default(),
            pool_amm_id: pool,
            mint,
            signer: Pubkey::new_unique(),
            is_buy: true,
            is_dev_buy: false,
            amount: 123_456,
            max_sol_cost: 789_000,
            min_sol_output: 0,
            success: true,
            error_code: None,
            compute_units_consumed: Some(42),
            owner_token_deltas: vec![TokenDelta {
                owner: Pubkey::new_unique().to_string(),
                delta_raw: 123_456,
                decimals: 6,
            }],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            v_tokens_in_bonding_curve: Some(1.0),
            v_sol_in_bonding_curve: Some(2.0),
            market_cap_sol: None,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
            bonding_curve_v2: None,
            bonding_curve_v2_provenance: None,
            is_mayhem_mode: None,
            cu_price_micro_lamports: None,
            compute_unit_limit: None,
            inner_ix_count: None,
            cpi_depth: None,
            ata_create_count: None,
            signer_pre_balance_lamports: None,
            signer_post_balance_lamports: None,
            jito_tip_detected: None,
            toolchain_fingerprint: ToolchainFingerprintInput::default(),
            curve_data_known: true,
            curve_finality: ghost_core::CurveFinality::Provisional,
            is_pumpswap: false,
        };

        let unresolved = TradeEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(1),
            signature,
            event_ordinal: Some(0),
            provenance: None,
            timestamp_ms: 1,
            arrival_ts_ms: 1,
            event_time: ghost_core::EventTimeMetadata::default(),
            pool_amm_id: Pubkey::new_unique(),
            mint: Pubkey::default(),
            signer: Pubkey::new_unique(),
            is_buy: true,
            is_dev_buy: false,
            amount: 123_456,
            max_sol_cost: 789_000,
            min_sol_output: 0,
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            market_cap_sol: None,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
            bonding_curve_v2: None,
            bonding_curve_v2_provenance: None,
            is_mayhem_mode: None,
            cu_price_micro_lamports: None,
            compute_unit_limit: None,
            inner_ix_count: None,
            cpi_depth: None,
            ata_create_count: None,
            signer_pre_balance_lamports: None,
            signer_post_balance_lamports: None,
            jito_tip_detected: None,
            toolchain_fingerprint: ToolchainFingerprintInput::default(),
            curve_data_known: false,
            curve_finality: ghost_core::CurveFinality::Speculative,
            is_pumpswap: false,
        };

        let deduped = dedup_trade_candidates(&cm, vec![unresolved, resolved.clone()]);
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].pool_amm_id, resolved.pool_amm_id);
        assert_eq!(deduped[0].mint, resolved.mint);
        assert_eq!(deduped[0].signer, resolved.signer);
    }

    #[test]
    fn trade_candidate_dedup_does_not_merge_different_ordinals() {
        let _guard = metrics_test_lock().lock().expect("metrics test lock");
        clear_recorded_counters();

        let cm = CurveMintRegistry::new();
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let signature = solana_sdk::signature::Signature::new_unique();

        let trade_a = TradeEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(1),
            signature,
            event_ordinal: Some(0),
            provenance: None,
            timestamp_ms: 1000,
            arrival_ts_ms: 1001,
            event_time: ghost_core::EventTimeMetadata::default(),
            pool_amm_id: pool,
            mint,
            signer: Pubkey::new_unique(),
            is_buy: true,
            is_dev_buy: false,
            amount: 123,
            max_sol_cost: 456,
            min_sol_output: 0,
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            market_cap_sol: None,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
            bonding_curve_v2: None,
            bonding_curve_v2_provenance: None,
            is_mayhem_mode: None,
            cu_price_micro_lamports: None,
            compute_unit_limit: None,
            inner_ix_count: None,
            cpi_depth: None,
            ata_create_count: None,
            signer_pre_balance_lamports: None,
            signer_post_balance_lamports: None,
            jito_tip_detected: None,
            toolchain_fingerprint: ToolchainFingerprintInput::default(),
            curve_data_known: true,
            curve_finality: ghost_core::CurveFinality::Provisional,
            is_pumpswap: false,
        };

        let mut trade_b = trade_a.clone();
        trade_b.event_ordinal = Some(1);

        let deduped = dedup_trade_candidates(&cm, vec![trade_a, trade_b]);
        assert_eq!(
            deduped.len(),
            2,
            "same signature trades with distinct event ordinals must survive candidate dedup"
        );
        assert!(saw_counter(
            TRADE_CANDIDATE_ORDINAL_TOTAL,
            &[("case", "ordinals_differ"), ("reason", "exact_match")],
        ));
        assert!(saw_counter(
            TRADE_CANDIDATE_ORDINAL_TOTAL,
            &[
                ("case", "match_rejected_due_to_different_event_ordinal"),
                ("reason", "exact_match"),
            ],
        ));
    }

    #[test]
    fn trade_candidate_dedup_logs_reason_codes() {
        let _guard = metrics_test_lock().lock().expect("metrics test lock");
        clear_recorded_counters();

        let cm = CurveMintRegistry::new();
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let signer = Pubkey::new_unique();
        let signature = solana_sdk::signature::Signature::new_unique();

        let mut weak = sample_trade_event(signature, pool, mint, signer, Some(0));
        weak.mint = Pubkey::default();
        weak.curve_data_known = false;
        weak.curve_finality = ghost_core::CurveFinality::Speculative;

        let strong = sample_trade_event(signature, pool, mint, signer, Some(0));
        let incoming_score = trade_candidate_score(&cm, &strong).to_string();
        let existing_score = trade_candidate_score(&cm, &weak).to_string();

        let deduped = dedup_trade_candidates(&cm, vec![weak, strong]);
        assert_eq!(deduped.len(), 1);
        assert!(saw_counter(
            TRADE_CANDIDATE_DEDUP_TOTAL,
            &[
                ("decision", "replaced"),
                ("reason_code", "score_incoming_higher"),
                ("match_kind", "exact_match"),
                ("ordinal_relation", "same_event_ordinal"),
                ("provenance_relation", "both_provenance_missing"),
                ("merge_action", "merge_trade_optional_accounts"),
                ("winner", "incoming"),
                ("incoming_score", incoming_score.as_str()),
                ("existing_score", existing_score.as_str()),
            ],
        ));
        assert!(saw_counter(
            TRADE_CANDIDATE_DEDUP_STAGE_TOTAL,
            &[("stage", "input")],
        ));
        assert!(saw_counter(
            TRADE_CANDIDATE_DEDUP_STAGE_TOTAL,
            &[("stage", "output")],
        ));
    }

    #[test]
    fn live_path_without_event_ordinal_is_visible_in_metrics() {
        let _guard = metrics_test_lock().lock().expect("metrics test lock");
        clear_recorded_counters();

        let cm = CurveMintRegistry::new();
        let trade = sample_trade_event(
            solana_sdk::signature::Signature::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            None,
        );

        let deduped = dedup_trade_candidates(&cm, vec![trade]);
        assert_eq!(deduped.len(), 1);
        assert!(saw_counter(
            TRADE_CANDIDATE_ORDINAL_TOTAL,
            &[("case", "missing_event_ordinal"), ("reason", "none")],
        ));
    }

    #[test]
    fn dedup_trade_candidates_merges_buy_account_overrides_from_duplicate() {
        let cm = CurveMintRegistry::new();
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let signer = Pubkey::new_unique();
        let signature = solana_sdk::signature::Signature::new_unique();
        let global_config = Pubkey::new_unique();
        let fee_recipient = Pubkey::new_unique();
        let token_program = Pubkey::new_unique();

        let weak = TradeEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(1),
            signature,
            event_ordinal: Some(0),
            provenance: None,
            timestamp_ms: 1,
            arrival_ts_ms: 1,
            event_time: ghost_core::EventTimeMetadata::default(),
            pool_amm_id: pool,
            mint,
            signer,
            is_buy: true,
            is_dev_buy: false,
            amount: 123_456,
            max_sol_cost: 789_000,
            min_sol_output: 0,
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            v_tokens_in_bonding_curve: Some(1.0),
            v_sol_in_bonding_curve: Some(2.0),
            market_cap_sol: None,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
            bonding_curve_v2: None,
            bonding_curve_v2_provenance: None,
            is_mayhem_mode: None,
            cu_price_micro_lamports: None,
            compute_unit_limit: None,
            inner_ix_count: None,
            cpi_depth: None,
            ata_create_count: None,
            signer_pre_balance_lamports: None,
            signer_post_balance_lamports: None,
            jito_tip_detected: None,
            toolchain_fingerprint: ToolchainFingerprintInput::default(),
            curve_data_known: true,
            curve_finality: ghost_core::CurveFinality::Provisional,
            is_pumpswap: false,
        };

        let mut rich = weak.clone();
        rich.global_config = Some(global_config);
        rich.fee_recipient = Some(fee_recipient);
        rich.token_program = Some(token_program);
        rich.buy_variant = Some("legacy_buy".to_string());
        rich.associated_bonding_curve = Some(Pubkey::new_unique());

        let deduped = dedup_trade_candidates(&cm, vec![weak, rich]);
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].global_config, Some(global_config));
        assert_eq!(deduped[0].fee_recipient, Some(fee_recipient));
        assert_eq!(deduped[0].token_program, Some(token_program));
        assert_eq!(deduped[0].buy_variant.as_deref(), Some("legacy_buy"));
        assert!(deduped[0].associated_bonding_curve.is_some());
    }

    #[test]
    fn dedup_trade_candidates_preserves_provenance_from_lower_scored_duplicate() {
        let cm = CurveMintRegistry::new();
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let signer = Pubkey::new_unique();
        let signature = solana_sdk::signature::Signature::new_unique();

        let mut existing = sample_trade_event(signature, pool, mint, signer, Some(0));
        existing.global_config = Some(Pubkey::new_unique());

        let mut incoming = sample_trade_event(signature, pool, mint, signer, Some(0));
        incoming.provenance = Some(crate::types::InstructionProvenance {
            outer_instruction_index: Some(3),
            inner_group_index: Some(1),
            outer_program_id: Some(SYSTEM_PROGRAM_ID.to_string()),
            invoked_program_id: PUMP_FUN_PROGRAM_ID.to_string(),
            stack_height: Some(2),
            from_cpi: true,
        });

        let deduped = dedup_trade_candidates(&cm, vec![existing, incoming]);
        assert_eq!(deduped.len(), 1);

        let provenance = deduped[0]
            .provenance
            .as_ref()
            .expect("winning trade should retain provenance from duplicate");
        assert_eq!(provenance.outer_instruction_index, Some(3));
        assert_eq!(provenance.inner_group_index, Some(1));
        assert_eq!(
            provenance.outer_program_id.as_deref(),
            Some(SYSTEM_PROGRAM_ID)
        );
        assert_eq!(provenance.invoked_program_id, PUMP_FUN_PROGRAM_ID);
        assert_eq!(provenance.stack_height, Some(2));
        assert!(provenance.from_cpi);
    }

    #[test]
    fn trade_candidate_dedup_preserves_provenance_on_replacement() {
        let cm = CurveMintRegistry::new();
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let signer = Pubkey::new_unique();
        let signature = solana_sdk::signature::Signature::new_unique();

        let mut existing = sample_trade_event(signature, pool, mint, signer, Some(0));
        existing.mint = Pubkey::default();
        existing.curve_data_known = false;
        existing.curve_finality = ghost_core::CurveFinality::Speculative;

        let mut incoming = sample_trade_event(signature, pool, mint, signer, Some(0));
        incoming.provenance = Some(crate::types::InstructionProvenance {
            outer_instruction_index: Some(7),
            inner_group_index: Some(0),
            outer_program_id: Some(PUMP_FUN_PROGRAM_ID.to_string()),
            invoked_program_id: PUMP_FUN_PROGRAM_ID.to_string(),
            stack_height: Some(3),
            from_cpi: true,
        });

        let deduped = dedup_trade_candidates(&cm, vec![existing, incoming]);
        assert_eq!(deduped.len(), 1);

        let provenance = deduped[0]
            .provenance
            .as_ref()
            .expect("replacement winner should retain incoming provenance");
        assert_eq!(provenance.outer_instruction_index, Some(7));
        assert_eq!(provenance.inner_group_index, Some(0));
        assert_eq!(
            provenance.outer_program_id.as_deref(),
            Some(PUMP_FUN_PROGRAM_ID)
        );
        assert_eq!(provenance.stack_height, Some(3));
        assert!(provenance.from_cpi);
    }

    #[test]
    fn enrich_trade_optional_accounts_from_source_ix_salvages_buy_overrides() {
        let mint = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let global_config = Pubkey::new_unique();
        let fee_recipient = Pubkey::new_unique();
        let token_program = Pubkey::new_unique();
        let associated_bonding_curve = Pubkey::new_unique();
        let bonding_curve_v2 = Pubkey::new_unique();

        let mut accounts = vec![Pubkey::new_unique(); 18];
        accounts[PUMP_IDX_GLOBAL_CONFIG] = global_config;
        accounts[PUMP_IDX_FEE_RECIPIENT] = fee_recipient;
        accounts[PUMP_IDX_MINT] = mint;
        accounts[PUMP_IDX_BONDING_CURVE] = curve;
        accounts[PUMP_IDX_ASSOCIATED_BONDING_CURVE] = associated_bonding_curve;
        accounts[PUMP_IDX_USER] = user;
        accounts[PUMP_IDX_TOKEN_PROGRAM] = token_program;
        accounts[PUMP_IDX_BONDING_CURVE_V2] = bonding_curve_v2;

        let event = make_decoded_tx_event(
            accounts,
            vec![crate::types::RawInstruction {
                program_id: Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap(),
                account_indices: (0u8..18u8).collect(),
                data: trade_data(DISC_BUY, 1_000_000, 50_000_000),
            }],
        );

        let mut trade = TradeEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(1),
            signature: solana_sdk::signature::Signature::new_unique(),
            event_ordinal: Some(0),
            provenance: None,
            timestamp_ms: 1,
            arrival_ts_ms: 1,
            event_time: ghost_core::EventTimeMetadata::default(),
            pool_amm_id: curve,
            mint,
            signer: user,
            is_buy: true,
            is_dev_buy: false,
            amount: 1_000_000,
            max_sol_cost: 50_000_000,
            min_sol_output: 0,
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            market_cap_sol: None,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
            bonding_curve_v2: None,
            bonding_curve_v2_provenance: None,
            is_mayhem_mode: None,
            cu_price_micro_lamports: None,
            compute_unit_limit: None,
            inner_ix_count: None,
            cpi_depth: None,
            ata_create_count: None,
            signer_pre_balance_lamports: None,
            signer_post_balance_lamports: None,
            jito_tip_detected: None,
            toolchain_fingerprint: ToolchainFingerprintInput::default(),
            curve_data_known: true,
            curve_finality: ghost_core::CurveFinality::Provisional,
            is_pumpswap: false,
        };

        enrich_trade_optional_accounts_from_source_ix(&event, &mut trade);

        assert_eq!(trade.global_config, Some(global_config));
        assert_eq!(trade.fee_recipient, Some(fee_recipient));
        assert_eq!(trade.token_program, Some(token_program));
        assert_eq!(trade.buy_variant.as_deref(), Some("legacy_buy"));
        assert_eq!(
            trade.associated_bonding_curve,
            Some(associated_bonding_curve)
        );
        assert_eq!(trade.bonding_curve_v2, Some(bonding_curve_v2));
        let provenance = trade
            .bonding_curve_v2_provenance
            .as_ref()
            .expect("observed bcv2 provenance");
        assert_eq!(provenance.source_slot, Some(42));
        assert_eq!(provenance.source_slot_index, Some(0));
        assert_eq!(provenance.source_instruction_index, Some(0));
        assert_eq!(
            provenance.source_program_id.as_deref(),
            Some(PUMP_FUN_PROGRAM_ID)
        );
        assert_eq!(provenance.source_buy_variant.as_deref(), Some("legacy_buy"));
        assert_eq!(
            provenance.instruction_account_position,
            Some(PUMP_IDX_BONDING_CURVE_V2 as u32)
        );
        assert_eq!(
            provenance.message_account_index,
            Some(PUMP_IDX_BONDING_CURVE_V2 as u32)
        );
        assert_eq!(
            provenance.resolved_pubkey.as_deref(),
            Some(bonding_curve_v2.to_string().as_str())
        );
        assert_eq!(
            provenance.loaded_address_source.as_deref(),
            Some("resolved_transaction_account_keys")
        );
        assert_eq!(provenance.tx_success, Some(true));
        assert_eq!(
            provenance.provenance_status.as_deref(),
            Some("route_compatible")
        );
    }

    #[test]
    fn enrich_trade_optional_accounts_resolves_bcv2_instruction_index_not_global_index() {
        let mint = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let global_config = Pubkey::new_unique();
        let fee_recipient = Pubkey::new_unique();
        let token_program = Pubkey::new_unique();
        let associated_bonding_curve = Pubkey::new_unique();
        let bonding_curve_v2 = Pubkey::new_unique();

        let offset = 8usize;
        let mut accounts = vec![Pubkey::new_unique(); 32];
        accounts[offset + PUMP_IDX_GLOBAL_CONFIG] = global_config;
        accounts[offset + PUMP_IDX_FEE_RECIPIENT] = fee_recipient;
        accounts[offset + PUMP_IDX_MINT] = mint;
        accounts[offset + PUMP_IDX_BONDING_CURVE] = curve;
        accounts[offset + PUMP_IDX_ASSOCIATED_BONDING_CURVE] = associated_bonding_curve;
        accounts[offset + PUMP_IDX_USER] = user;
        accounts[offset + PUMP_IDX_TOKEN_PROGRAM] = token_program;
        accounts[offset + PUMP_IDX_BONDING_CURVE_V2] = bonding_curve_v2;

        let event = make_decoded_tx_event(
            accounts.clone(),
            vec![crate::types::RawInstruction {
                program_id: Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap(),
                account_indices: (offset as u8..(offset + 18) as u8).collect(),
                data: trade_data(DISC_BUY, 1_000_000, 50_000_000),
            }],
        );

        let mut trade = sample_trade_event(
            solana_sdk::signature::Signature::new_unique(),
            curve,
            mint,
            user,
            Some(0),
        );

        enrich_trade_optional_accounts_from_source_ix(&event, &mut trade);

        assert_ne!(
            accounts[PUMP_IDX_BONDING_CURVE_V2], bonding_curve_v2,
            "fixture must distinguish global account_keys[16] from instruction position 16"
        );
        assert_eq!(trade.bonding_curve_v2, Some(bonding_curve_v2));
        let provenance = trade
            .bonding_curve_v2_provenance
            .as_ref()
            .expect("observed bcv2 provenance");
        assert_eq!(
            provenance.instruction_account_position,
            Some(PUMP_IDX_BONDING_CURVE_V2 as u32)
        );
        assert_eq!(
            provenance.message_account_index,
            Some((offset + PUMP_IDX_BONDING_CURVE_V2) as u32)
        );
        assert_eq!(
            provenance.resolved_pubkey.as_deref(),
            Some(bonding_curve_v2.to_string().as_str())
        );
        assert_eq!(
            provenance.provenance_status.as_deref(),
            Some("route_compatible")
        );
    }

    #[test]
    fn enrich_trade_optional_accounts_maps_exact_quote_in_to_routed_variant() {
        let mint = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let mut accounts = vec![Pubkey::new_unique(); 12];
        accounts[PUMP_IDX_MINT] = mint;
        accounts[PUMP_IDX_BONDING_CURVE] = curve;
        accounts[PUMP_IDX_ASSOCIATED_BONDING_CURVE] = Pubkey::new_unique();
        accounts[PUMP_IDX_USER] = user;

        let event = make_decoded_tx_event(
            accounts,
            vec![crate::types::RawInstruction {
                program_id: Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap(),
                account_indices: (0u8..12u8).collect(),
                data: trade_data(DISC_SWAP_BUY_EXACT_QUOTE_IN, 1_000_000, 50_000_000),
            }],
        );

        let mut trade = TradeEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(1),
            signature: solana_sdk::signature::Signature::new_unique(),
            event_ordinal: Some(0),
            provenance: None,
            timestamp_ms: 1,
            arrival_ts_ms: 1,
            event_time: ghost_core::EventTimeMetadata::default(),
            pool_amm_id: curve,
            mint,
            signer: user,
            is_buy: true,
            is_dev_buy: false,
            amount: 1_000_000,
            max_sol_cost: 50_000_000,
            min_sol_output: 0,
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            market_cap_sol: None,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
            bonding_curve_v2: None,
            bonding_curve_v2_provenance: None,
            is_mayhem_mode: None,
            cu_price_micro_lamports: None,
            compute_unit_limit: None,
            inner_ix_count: None,
            cpi_depth: None,
            ata_create_count: None,
            signer_pre_balance_lamports: None,
            signer_post_balance_lamports: None,
            jito_tip_detected: None,
            toolchain_fingerprint: ToolchainFingerprintInput::default(),
            curve_data_known: true,
            curve_finality: ghost_core::CurveFinality::Provisional,
            is_pumpswap: false,
        };

        enrich_trade_optional_accounts_from_source_ix(&event, &mut trade);

        assert_eq!(trade.buy_variant.as_deref(), Some("routed_exact_sol_in"));
        assert!(trade.associated_bonding_curve.is_some());
    }

    #[test]
    fn enrich_trade_prefers_top_level_routed_over_top_level_legacy_when_both_match() {
        let mint = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let global_config = Pubkey::new_unique();
        let legacy_fee = Pubkey::new_unique();
        let routed_fee = Pubkey::new_unique();
        let token_program = Pubkey::new_unique();
        let legacy_assoc_bc = Pubkey::new_unique();
        let routed_assoc_bc = Pubkey::new_unique();

        let mut accounts = vec![Pubkey::new_unique(); 24];
        accounts[PUMP_IDX_GLOBAL_CONFIG] = global_config;
        accounts[PUMP_IDX_FEE_RECIPIENT] = legacy_fee;
        accounts[PUMP_IDX_MINT] = mint;
        accounts[PUMP_IDX_BONDING_CURVE] = curve;
        accounts[PUMP_IDX_ASSOCIATED_BONDING_CURVE] = legacy_assoc_bc;
        accounts[PUMP_IDX_USER] = user;
        accounts[PUMP_IDX_TOKEN_PROGRAM] = token_program;

        let routed_base = 12usize;
        accounts[routed_base + PUMP_IDX_GLOBAL_CONFIG] = global_config;
        accounts[routed_base + PUMP_IDX_FEE_RECIPIENT] = routed_fee;
        accounts[routed_base + PUMP_IDX_MINT] = mint;
        accounts[routed_base + PUMP_IDX_BONDING_CURVE] = curve;
        accounts[routed_base + PUMP_IDX_ASSOCIATED_BONDING_CURVE] = routed_assoc_bc;
        accounts[routed_base + PUMP_IDX_USER] = user;
        accounts[routed_base + PUMP_IDX_TOKEN_PROGRAM] = token_program;

        let event = make_decoded_tx_event(
            accounts,
            vec![
                crate::types::RawInstruction {
                    program_id: Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap(),
                    account_indices: (0u8..12u8).collect(),
                    data: trade_data(DISC_BUY, 1_000_000, 50_000_000),
                },
                crate::types::RawInstruction {
                    program_id: Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap(),
                    account_indices: (12u8..24u8).collect(),
                    data: trade_data(DISC_PUMP_BUY_ROUTED, 1_000_000, 50_000_000),
                },
            ],
        );

        let mut trade = TradeEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(1),
            signature: solana_sdk::signature::Signature::new_unique(),
            event_ordinal: Some(0),
            provenance: None,
            timestamp_ms: 1,
            arrival_ts_ms: 1,
            event_time: ghost_core::EventTimeMetadata::default(),
            pool_amm_id: curve,
            mint,
            signer: user,
            is_buy: true,
            is_dev_buy: false,
            amount: 1_000_000,
            max_sol_cost: 50_000_000,
            min_sol_output: 0,
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            market_cap_sol: None,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
            bonding_curve_v2: None,
            bonding_curve_v2_provenance: None,
            is_mayhem_mode: None,
            cu_price_micro_lamports: None,
            compute_unit_limit: None,
            inner_ix_count: None,
            cpi_depth: None,
            ata_create_count: None,
            signer_pre_balance_lamports: None,
            signer_post_balance_lamports: None,
            jito_tip_detected: None,
            toolchain_fingerprint: ToolchainFingerprintInput::default(),
            curve_data_known: true,
            curve_finality: ghost_core::CurveFinality::Provisional,
            is_pumpswap: false,
        };

        enrich_trade_optional_accounts_from_source_ix(&event, &mut trade);

        assert_eq!(trade.buy_variant.as_deref(), Some("routed_exact_sol_in"));
        assert_eq!(trade.fee_recipient, Some(routed_fee));
        assert_eq!(trade.associated_bonding_curve, Some(routed_assoc_bc));
    }

    // ── CPI inner-instruction enrichment tests ───────────────────────────────

    /// Pump.fun legacy BUY living inside inner_instructions (aggregator CPI)
    /// must be enriched with buy_variant, fee, token_program, assoc_bc.
    #[test]
    fn enrich_trade_from_inner_instructions_cpi_legacy_buy() {
        let mint = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let global_config = Pubkey::new_unique();
        let fee_recipient = Pubkey::new_unique();
        let token_program = Pubkey::new_unique();
        let associated_bonding_curve = Pubkey::new_unique();

        // Build accounts list — pump.fun accounts at their IDL indices
        let mut accounts = vec![Pubkey::new_unique(); 12];
        accounts[PUMP_IDX_GLOBAL_CONFIG] = global_config;
        accounts[PUMP_IDX_FEE_RECIPIENT] = fee_recipient;
        accounts[PUMP_IDX_MINT] = mint;
        accounts[PUMP_IDX_BONDING_CURVE] = curve;
        accounts[PUMP_IDX_ASSOCIATED_BONDING_CURVE] = associated_bonding_curve;
        accounts[PUMP_IDX_USER] = user;
        accounts[PUMP_IDX_TOKEN_PROGRAM] = token_program;

        // Add the pump.fun program itself as account[12] so inner ix can
        // reference it via program_id_index.
        let pump_program = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap();
        accounts.push(pump_program);
        let pump_prog_idx = (accounts.len() - 1) as u8; // index 12

        // No top-level pump.fun instruction — only an aggregator placeholder
        let aggregator_program = Pubkey::new_unique();
        let top_level_ix = crate::types::RawInstruction {
            program_id: aggregator_program,
            account_indices: (0u8..12u8).collect(),
            data: vec![0xDE, 0xAD], // non-pump discriminator
        };

        // Pump.fun BUY as inner CPI
        let inner_ix = crate::types::InnerIx {
            program_id_index: pump_prog_idx,
            accounts: (0u8..12u8).collect(),
            data: trade_data(DISC_BUY, 1_000_000, 50_000_000),
            stack_height: Some(2),
        };

        let event = make_decoded_tx_event_with_inner(
            accounts,
            vec![top_level_ix],
            vec![crate::types::InnerInstructionGroup {
                index: 0,
                instructions: vec![inner_ix],
            }],
        );

        let mut trade = TradeEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(1),
            signature: solana_sdk::signature::Signature::new_unique(),
            event_ordinal: Some(0),
            provenance: None,
            timestamp_ms: 1,
            arrival_ts_ms: 1,
            event_time: ghost_core::EventTimeMetadata::default(),
            pool_amm_id: curve,
            mint,
            signer: user,
            is_buy: true,
            is_dev_buy: false,
            amount: 1_000_000,
            max_sol_cost: 50_000_000,
            min_sol_output: 0,
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            market_cap_sol: None,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
            bonding_curve_v2: None,
            bonding_curve_v2_provenance: None,
            is_mayhem_mode: None,
            cu_price_micro_lamports: None,
            compute_unit_limit: None,
            inner_ix_count: None,
            cpi_depth: None,
            ata_create_count: None,
            signer_pre_balance_lamports: None,
            signer_post_balance_lamports: None,
            jito_tip_detected: None,
            toolchain_fingerprint: ToolchainFingerprintInput::default(),
            curve_data_known: true,
            curve_finality: ghost_core::CurveFinality::Provisional,
            is_pumpswap: false,
        };

        enrich_trade_optional_accounts_from_source_ix(&event, &mut trade);

        assert_eq!(trade.global_config, Some(global_config));
        assert_eq!(trade.fee_recipient, Some(fee_recipient));
        assert_eq!(trade.token_program, Some(token_program));
        assert_eq!(trade.buy_variant.as_deref(), Some("legacy_buy"));
        assert_eq!(
            trade.associated_bonding_curve,
            Some(associated_bonding_curve)
        );
    }

    /// Routed pump.fun BUY in inner_instructions → routed_exact_sol_in variant.
    #[test]
    fn enrich_trade_from_inner_instructions_routed_buy() {
        let mint = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let user = Pubkey::new_unique();

        let mut accounts = vec![Pubkey::new_unique(); 12];
        accounts[PUMP_IDX_MINT] = mint;
        accounts[PUMP_IDX_BONDING_CURVE] = curve;
        accounts[PUMP_IDX_ASSOCIATED_BONDING_CURVE] = Pubkey::new_unique();
        accounts[PUMP_IDX_USER] = user;

        let pump_program = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap();
        accounts.push(pump_program);
        let pump_prog_idx = (accounts.len() - 1) as u8;

        let inner_ix = crate::types::InnerIx {
            program_id_index: pump_prog_idx,
            accounts: (0u8..12u8).collect(),
            data: trade_data(DISC_PUMP_BUY_ROUTED, 1_000_000, 50_000_000),
            stack_height: Some(2),
        };

        let event = make_decoded_tx_event_with_inner(
            accounts,
            vec![], // no top-level pump.fun ix
            vec![crate::types::InnerInstructionGroup {
                index: 0,
                instructions: vec![inner_ix],
            }],
        );

        let mut trade = TradeEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(1),
            signature: solana_sdk::signature::Signature::new_unique(),
            event_ordinal: Some(0),
            provenance: None,
            timestamp_ms: 1,
            arrival_ts_ms: 1,
            event_time: ghost_core::EventTimeMetadata::default(),
            pool_amm_id: curve,
            mint,
            signer: user,
            is_buy: true,
            is_dev_buy: false,
            amount: 1_000_000,
            max_sol_cost: 50_000_000,
            min_sol_output: 0,
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            market_cap_sol: None,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
            bonding_curve_v2: None,
            bonding_curve_v2_provenance: None,
            is_mayhem_mode: None,
            cu_price_micro_lamports: None,
            compute_unit_limit: None,
            inner_ix_count: None,
            cpi_depth: None,
            ata_create_count: None,
            signer_pre_balance_lamports: None,
            signer_post_balance_lamports: None,
            jito_tip_detected: None,
            toolchain_fingerprint: ToolchainFingerprintInput::default(),
            curve_data_known: true,
            curve_finality: ghost_core::CurveFinality::Provisional,
            is_pumpswap: false,
        };

        enrich_trade_optional_accounts_from_source_ix(&event, &mut trade);

        assert_eq!(trade.buy_variant.as_deref(), Some("routed_exact_sol_in"));
        assert!(trade.associated_bonding_curve.is_some());
    }

    /// When pump.fun BUY exists in both top-level and inner, top-level takes
    /// priority — inner must NOT overwrite already-filled fields.
    #[test]
    fn enrich_trade_prefers_top_level_over_inner() {
        let mint = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let top_fee = Pubkey::new_unique();
        let inner_fee = Pubkey::new_unique();
        let top_assoc_bc = Pubkey::new_unique();

        let mut accounts = vec![Pubkey::new_unique(); 12];
        accounts[PUMP_IDX_MINT] = mint;
        accounts[PUMP_IDX_BONDING_CURVE] = curve;
        accounts[PUMP_IDX_FEE_RECIPIENT] = top_fee;
        accounts[PUMP_IDX_ASSOCIATED_BONDING_CURVE] = top_assoc_bc;
        accounts[PUMP_IDX_USER] = user;

        let pump_program = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap();
        accounts.push(pump_program);
        let pump_prog_idx = (accounts.len() - 1) as u8;

        // Top-level has pump.fun BUY (legacy)
        let top_ix = crate::types::RawInstruction {
            program_id: pump_program,
            account_indices: (0u8..12u8).collect(),
            data: trade_data(DISC_BUY, 1_000_000, 50_000_000),
        };

        // Inner also has pump.fun BUY (routed) with a DIFFERENT fee
        let mut inner_accounts = vec![Pubkey::new_unique(); 12];
        inner_accounts[PUMP_IDX_MINT] = mint;
        inner_accounts[PUMP_IDX_BONDING_CURVE] = curve;
        inner_accounts[PUMP_IDX_FEE_RECIPIENT] = inner_fee;
        inner_accounts[PUMP_IDX_ASSOCIATED_BONDING_CURVE] = Pubkey::new_unique();

        // Inner ix uses same account indices as top-level for simplicity
        let inner_ix = crate::types::InnerIx {
            program_id_index: pump_prog_idx,
            accounts: (0u8..12u8).collect(),
            data: trade_data(DISC_PUMP_BUY_ROUTED, 1_000_000, 50_000_000),
            stack_height: Some(2),
        };

        let event = make_decoded_tx_event_with_inner(
            accounts,
            vec![top_ix],
            vec![crate::types::InnerInstructionGroup {
                index: 0,
                instructions: vec![inner_ix],
            }],
        );

        let mut trade = TradeEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(1),
            signature: solana_sdk::signature::Signature::new_unique(),
            event_ordinal: Some(0),
            provenance: None,
            timestamp_ms: 1,
            arrival_ts_ms: 1,
            event_time: ghost_core::EventTimeMetadata::default(),
            pool_amm_id: curve,
            mint,
            signer: user,
            is_buy: true,
            is_dev_buy: false,
            amount: 1_000_000,
            max_sol_cost: 50_000_000,
            min_sol_output: 0,
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            market_cap_sol: None,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
            bonding_curve_v2: None,
            bonding_curve_v2_provenance: None,
            is_mayhem_mode: None,
            cu_price_micro_lamports: None,
            compute_unit_limit: None,
            inner_ix_count: None,
            cpi_depth: None,
            ata_create_count: None,
            signer_pre_balance_lamports: None,
            signer_post_balance_lamports: None,
            jito_tip_detected: None,
            toolchain_fingerprint: ToolchainFingerprintInput::default(),
            curve_data_known: true,
            curve_finality: ghost_core::CurveFinality::Provisional,
            is_pumpswap: false,
        };

        enrich_trade_optional_accounts_from_source_ix(&event, &mut trade);

        // Top-level legacy_buy must win over inner routed
        assert_eq!(trade.buy_variant.as_deref(), Some("legacy_buy"));
        // Top-level fee must be preserved
        assert_eq!(trade.fee_recipient, Some(top_fee));
        assert_eq!(trade.associated_bonding_curve, Some(top_assoc_bc));
    }

    // ── [FIX-2] MigrateParams decoded ────────────────────────────────────────

    #[test]
    fn migrate_params_decoded_from_payload() {
        // Build: discriminator + pool_pubkey (32B) + lp_mint (32B)
        let pool_bytes = [0xAA_u8; 32];
        let lp_bytes = [0xBB_u8; 32];
        let mut data = DISC_MIGRATE.to_vec();
        data.extend_from_slice(&pool_bytes);
        data.extend_from_slice(&lp_bytes);

        let cm = CurveMintRegistry::new();
        let ar = AccountRegistry::new();
        let mut out = Vec::new();
        PumpParser::decode_ix(
            &data,
            &dummy_accs(14),
            PUMP_FUN_PROGRAM_ID,
            1,
            None,
            Instant::now(),
            false,
            false,
            &[],
            &[],
            &[],
            &[],
            &no_keys(),
            &cm,
            &ar,
            &mut out,
        );

        assert_eq!(out.len(), 1);
        if let ParsedEventKind::Migrate {
            pool_pubkey_param,
            lp_mint_param,
            ..
        } = &out[0].kind
        {
            let expected_pool = bs58::encode(&pool_bytes).into_string();
            let expected_lp = bs58::encode(&lp_bytes).into_string();
            assert_eq!(
                pool_pubkey_param.as_deref(),
                Some(expected_pool.as_str()),
                "pool_pubkey_param must be decoded from MigrateParams"
            );
            assert_eq!(
                lp_mint_param.as_deref(),
                Some(expected_lp.as_str()),
                "lp_mint_param must be decoded from MigrateParams"
            );
        } else {
            panic!("expected Migrate kind, got {:?}", out[0].kind);
        }
    }

    #[test]
    fn migrate_missing_params_still_emits_migrate() {
        // Empty payload after discriminator — params decode fails gracefully
        let data = DISC_MIGRATE.to_vec(); // no payload bytes
        let mut out = Vec::new();
        let cm = CurveMintRegistry::new();
        let ar = AccountRegistry::new();
        PumpParser::decode_ix(
            &data,
            &dummy_accs(14),
            PUMP_FUN_PROGRAM_ID,
            1,
            None,
            Instant::now(),
            false,
            false,
            &[],
            &[],
            &[],
            &[],
            &no_keys(),
            &cm,
            &ar,
            &mut out,
        );
        assert_eq!(
            out.len(),
            1,
            "must still emit Migrate even if params missing"
        );
        if let ParsedEventKind::Migrate {
            pool_pubkey_param,
            lp_mint_param,
            ..
        } = &out[0].kind
        {
            assert!(
                pool_pubkey_param.is_none(),
                "should be None when params absent"
            );
            assert!(lp_mint_param.is_none());
        } else {
            panic!("expected Migrate");
        }
    }

    // ── [FIX-3] CreatePoolParams decoded ─────────────────────────────────────

    #[test]
    fn swap_create_pool_params_decoded() {
        // Build: discriminator + index (u16 LE) + base_amount (u64) + quote_amount (u64) + lp_mint (32B)
        let lp_bytes = [0xCC_u8; 32];
        let mut data = DISC_SWAP_CREATE_POOL.to_vec();
        data.extend_from_slice(&5u16.to_le_bytes()); // index
        data.extend_from_slice(&1_000_000u64.to_le_bytes()); // base_amount_in
        data.extend_from_slice(&5_000_000u64.to_le_bytes()); // quote_amount_in
        data.extend_from_slice(&lp_bytes); // lp_mint

        let cm = CurveMintRegistry::new();
        let ar = AccountRegistry::new();
        let mut out = Vec::new();
        let mut accs = dummy_accs(12);
        accs[SWAP_IDX_POOL] = "Pool111111111111111111111111111111111111111".into();
        accs[SWAP_IDX_USER] = "User111111111111111111111111111111111111111".into();
        accs[SWAP_IDX_BASE_MINT] = "MintAA1111111111111111111111111111111111111".into();
        accs[SWAP_IDX_QUOTE_MINT] = WSOL_MINT.into();
        PumpParser::decode_ix(
            &data,
            &accs,
            PUMP_SWAP_PROGRAM_ID,
            1,
            None,
            Instant::now(),
            false,
            false,
            &[],
            &[],
            &[],
            &[],
            &no_keys(),
            &cm,
            &ar,
            &mut out,
        );
        assert_eq!(out.len(), 1);
        if let ParsedEventKind::SwapPoolCreated {
            base_amount_in,
            quote_amount_in,
            lp_mint_param,
            ..
        } = &out[0].kind
        {
            assert_eq!(*base_amount_in, Some(1_000_000));
            assert_eq!(*quote_amount_in, Some(5_000_000));
            assert_eq!(
                lp_mint_param.as_deref(),
                Some(bs58::encode(&lp_bytes).into_string().as_str())
            );
        } else {
            panic!("expected SwapPoolCreated");
        }
    }

    #[test]
    fn swap_create_pool_params_decoded_with_trailing_byte() {
        let parser = BinaryParser::new(false);
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let signer = Keypair::new().pubkey();
        let bogus_create_meta = Pubkey::from_str(ProgramIds::TOKEN_2022_PROGRAM).unwrap();
        let quote_mint = Pubkey::from_str(WSOL_MINT).unwrap();
        let mut accounts = vec![Pubkey::new_unique(); 18];
        accounts[SWAP_IDX_POOL] = pool;
        accounts[1] = bogus_create_meta;
        accounts[SWAP_CREATE_IDX_SIGNER] = signer;
        accounts[SWAP_IDX_BASE_MINT] = quote_mint;
        accounts[SWAP_IDX_QUOTE_MINT] = mint;

        let mut data = DISC_SWAP_CREATE_POOL.to_vec();
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&200_000_000_000u64.to_le_bytes());
        data.extend_from_slice(&1_000_000_000_000_000u64.to_le_bytes());
        data.extend_from_slice(&[0u8; 32]);
        data.push(0);

        let event = make_decoded_tx_event(
            accounts,
            vec![crate::types::RawInstruction {
                program_id: Pubkey::from_str(PUMP_SWAP_PROGRAM_ID).unwrap(),
                account_indices: (0u8..18u8).collect(),
                data,
            }],
        );

        let init_pool = parser
            .parse_initialize_pool(&event)
            .expect("swap create pool should parse")
            .expect("swap create pool should emit pool");
        assert_eq!(
            init_pool.creator, signer,
            "CreatePool must attribute creator to the funding signer, not the metadata/config slot"
        );

        let trades = parser
            .parse_trades(&event)
            .expect("swap create pool should emit synthetic trade");
        assert_eq!(trades.len(), 1);
        let trade = &trades[0];
        assert_eq!(trade.pool_amm_id, pool);
        assert_eq!(trade.mint, mint);
        assert_eq!(trade.signer, signer);
        assert_eq!(trade.amount, 1_000_000_000_000_000);
        assert_eq!(trade.max_sol_cost, 200_000_000_000);
        assert!(trade.is_dev_buy);
    }

    #[test]
    fn parse_trades_emits_synthetic_pumpswap_create_trade_when_no_swap_trade_present() {
        let parser = BinaryParser::new(false);
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let quote_mint = Pubkey::from_str(WSOL_MINT).unwrap();
        let mut accounts = vec![Pubkey::new_unique(); SWAP_CREATE_IDX_SIGNER + 3];
        accounts[SWAP_IDX_POOL] = pool;
        accounts[SWAP_IDX_BASE_MINT] = mint;
        accounts[SWAP_IDX_QUOTE_MINT] = quote_mint;
        accounts[SWAP_CREATE_IDX_SIGNER] = creator;

        let mut data = DISC_SWAP_CREATE_POOL.to_vec();
        data.extend_from_slice(&7u16.to_le_bytes());
        data.extend_from_slice(&1_250_000_000u64.to_le_bytes());
        data.extend_from_slice(&85_000_000_000u64.to_le_bytes());
        data.extend_from_slice(&[0xAB_u8; 32]);

        let mut event = make_decoded_tx_event(
            accounts.clone(),
            vec![crate::types::RawInstruction {
                program_id: Pubkey::from_str(PUMP_SWAP_PROGRAM_ID).unwrap(),
                account_indices: (0u8..accounts.len() as u8).collect(),
                data,
            }],
        );
        if let GeyserEvent::Transaction { event_ts_ms, .. } = &mut event {
            *event_ts_ms = Some(1_777_777_777_000);
        }

        let trades = parser
            .parse_trades(&event)
            .expect("swap create pool should parse");
        assert_eq!(
            trades.len(),
            1,
            "create-only PumpSwap tx must emit one synthetic trade"
        );

        let trade = &trades[0];
        assert_eq!(trade.pool_amm_id, pool);
        assert_eq!(trade.mint, mint);
        assert_eq!(trade.signer, creator);
        assert!(
            trade.is_buy,
            "create-only PumpSwap tx should count as creator entry flow"
        );
        assert!(
            trade.is_dev_buy,
            "synthetic creator entry should be surfaced as dev exposure"
        );
        assert!(
            trade.is_pumpswap,
            "synthetic create trade must preserve AMM provenance"
        );
        assert_eq!(trade.amount, 1_250_000_000);
        assert_eq!(trade.max_sol_cost, 85_000_000_000);
        assert_eq!(trade.timestamp_ms, 1_777_777_777_000);
    }

    // ── [FIX-5] ResolveQueue cap raised ──────────────────────────────────────

    #[test]
    fn resolve_queue_caps_at_max() {
        let q = ResolveQueue::new(3);
        for i in 1u8..=5 {
            q.push(tp(i), i as u64, vec![], Instant::now());
        }
        assert_eq!(q.len(), 3); // oldest two dropped
    }

    #[test]
    fn resolve_queue_default_cap_is_2048() {
        let q = ResolveQueue::with_default_cap();
        // Fill to slightly above old cap (100) — should NOT drop
        for i in 1u8..=150 {
            q.push(tp(i), i as u64, vec![], Instant::now());
        }
        assert_eq!(
            q.len(),
            150,
            "default cap 2048 must accommodate 150 entries without dropping"
        );
    }

    #[test]
    fn unknown_account_short() {
        assert!(matches!(
            decode_account_data(&[0u8; 4]),
            PumpAccountState::Unknown { .. }
        ));
    }

    #[test]
    fn unknown_account_wrong_disc() {
        assert!(matches!(
            decode_account_data(&[0u8; 64]),
            PumpAccountState::Unknown { .. }
        ));
    }

    // ── CpiEventLog decode ────────────────────────────────────────────────────

    #[test]
    fn cpi_trade_decoded() {
        let mut p = DISC_EVENT_TRADE.to_vec();
        p.extend([1u8; 32]); // mint
        p.extend_from_slice(&100_000_000u64.to_le_bytes()); // sol_amount
        p.extend_from_slice(&1_000_000u64.to_le_bytes()); // token_amount
        p.push(1u8); // is_buy
        p.extend([2u8; 32]); // user
        p.extend_from_slice(&1_700_000_000i64.to_le_bytes()); // timestamp
        p.extend_from_slice(&30_000_000_000u64.to_le_bytes()); // virtual_sol
        p.extend_from_slice(&1_000_000_000_000u64.to_le_bytes()); // virtual_token
        let ev = PumpParser::try_decode_cpi_event(&p, 1, None, Instant::now(), false);
        assert!(ev.is_some());
        assert!(ev.unwrap().from_cpi);
    }

    #[test]
    fn cpi_trade_wrapped_in_anchor_outer_wrapper_decoded() {
        let mut payload = DISC_SWAP_OUTER_WRAPPER.to_vec();
        payload.extend(DISC_EVENT_TRADE);
        payload.extend([1u8; 32]); // mint
        payload.extend_from_slice(&100_000_000u64.to_le_bytes()); // sol_amount
        payload.extend_from_slice(&1_000_000u64.to_le_bytes()); // token_amount
        payload.push(1u8); // is_buy
        payload.extend([2u8; 32]); // user
        payload.extend_from_slice(&1_700_000_000i64.to_le_bytes()); // timestamp
        payload.extend_from_slice(&30_000_000_000u64.to_le_bytes()); // virtual_sol
        payload.extend_from_slice(&1_000_000_000_000u64.to_le_bytes()); // virtual_token
        let ev = PumpParser::try_decode_cpi_event(&payload, 1, None, Instant::now(), false);
        assert!(ev.is_some(), "wrapped pump.fun trade event should decode");
        assert!(matches!(
            ev.unwrap().kind,
            ParsedEventKind::CpiTrade(EventTrade { is_buy: true, .. })
        ));
    }

    #[test]
    fn cpi_unknown_disc_returns_none() {
        let data: Vec<u8> = [0xFF; 8].iter().chain([0u8; 32].iter()).copied().collect();
        assert!(PumpParser::try_decode_cpi_event(&data, 1, None, Instant::now(), false).is_none());
    }

    #[test]
    fn entry_scan_decodes_wrapped_trade_disc() {
        let mut raw = DISC_SWAP_OUTER_WRAPPER.to_vec();
        raw.extend(DISC_EVENT_TRADE);
        raw.extend([1u8; 32]); // mint
        raw.extend_from_slice(&100_000_000u64.to_le_bytes()); // sol_amount
        raw.extend_from_slice(&1_000_000u64.to_le_bytes()); // token_amount
        raw.push(1u8); // is_buy
        raw.extend([2u8; 32]); // user
        raw.extend_from_slice(&1_700_000_000i64.to_le_bytes()); // timestamp
        raw.extend_from_slice(&30_000_000_000u64.to_le_bytes()); // virtual_sol
        raw.extend_from_slice(&1_000_000_000_000u64.to_le_bytes()); // virtual_token

        let events = PumpParser::parse_entry_raw(&raw, 7, Instant::now(), 0);
        let cpi_trade_count = events
            .iter()
            .filter(|event| matches!(event.kind, ParsedEventKind::CpiTrade(_)))
            .count();

        assert_eq!(
            cpi_trade_count, 1,
            "wrapped trade should be found in entry scan"
        );
    }

    #[test]
    fn entry_scan_skips_full_consumed_event_prefix() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&DISC_EVENT_COMPLETE);
        payload.extend([0xAB; 24]); // remainder of user
        payload.extend([0xCD; 32]); // mint
        payload.extend([0xEF; 32]); // bonding_curve
        payload.extend_from_slice(&1_700_000_000i64.to_le_bytes()); // timestamp

        let mut raw = DISC_EVENT_COMPLETE.to_vec();
        raw.extend_from_slice(&payload);
        raw.extend_from_slice(&[0u8; 8]); // makes the old offset+=8 bug decode a phantom event

        let events = PumpParser::parse_entry_raw(&raw, 11, Instant::now(), 0);
        let cpi_complete_count = events
            .iter()
            .filter(|event| matches!(event.kind, ParsedEventKind::CpiComplete(_)))
            .count();

        assert_eq!(
            cpi_complete_count, 1,
            "entry scan must not re-match payload bytes as a second CPI event"
        );
    }

    // ── AmmPoolState ──────────────────────────────────────────────────────────

    #[test]
    fn amm_pool_price() {
        let s = AmmPoolState {
            pool_bump: 255,
            index: 0,
            creator: [0u8; 32],
            base_mint: [1u8; 32],
            quote_mint: [2u8; 32],
            lp_mint: [3u8; 32],
            pool_base_token_account: [4u8; 32],
            pool_quote_token_account: [5u8; 32],
            base_amount: 1_000_000_000,
            quote_amount: 30_000_000_000,
        };
        assert!((s.price_quote_per_base() - 30.0).abs() < 1e-6);
    }

    #[test]
    fn amm_pool_zero_base_no_panic() {
        let s = AmmPoolState {
            pool_bump: 0,
            index: 0,
            creator: [0u8; 32],
            base_mint: [0u8; 32],
            quote_mint: [0u8; 32],
            lp_mint: [0u8; 32],
            pool_base_token_account: [0u8; 32],
            pool_quote_token_account: [0u8; 32],
            base_amount: 0,
            quote_amount: 1_000,
        };
        assert_eq!(s.price_quote_per_base(), 0.0);
    }

    // ── WSOL-base pool tests ────────────────────────────────────────────────
    //
    // PumpSwap pools where the on-chain *base* mint is WSOL (rather than the
    // traded token) require inverted semantics:
    //   • DISC_BUY  → CpiSwapBuy  → TOKEN SELL (user pays quote=token, gets base=SOL)
    //   • DISC_SELL → CpiSwapSell → TOKEN BUY  (user pays base=SOL, gets quote=token)
    //   • CPI event fields base_* = SOL/lamports, quote_* = token raw units
    //
    // Helper account layout for these tests (mirrors SWAP_IDX_* constants):
    //   0 = pool, 1 = user, 2 = global_config
    //   3 = base_mint  (WSOL for wsol-base pools)
    //   4 = quote_mint (traded token for wsol-base pools)
    //   5 = user_base_ata, 6 = user_quote_ata
    //   7 = pool_base_ata, 8 = pool_quote_ata
    //   9 = token_program, 10 = token_program, 11 = pumpswap_program

    fn wsol_base_accounts() -> (
        Pubkey, // pool
        Pubkey, // user
        Pubkey, // traded_mint (quote)
        Pubkey, // pumpswap_program
        Vec<Pubkey>,
    ) {
        let pool = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let traded_mint = Pubkey::new_unique();
        let wsol_mint = Pubkey::from_str(WSOL_MINT).unwrap();
        let pumpswap = Pubkey::from_str(PUMP_SWAP_PROGRAM_ID).unwrap();
        let token_program = Pubkey::from_str(ProgramIds::TOKEN_PROGRAM).unwrap();
        let accounts = vec![
            pool,                 // 0: SWAP_IDX_POOL
            user,                 // 1: SWAP_IDX_USER
            Pubkey::new_unique(), // 2: global_config
            wsol_mint,            // 3: SWAP_IDX_BASE_MINT = WSOL
            traded_mint,          // 4: SWAP_IDX_QUOTE_MINT = token
            Pubkey::new_unique(), // 5: user_base_ata (WSOL)
            Pubkey::new_unique(), // 6: user_quote_ata (token)
            Pubkey::new_unique(), // 7: pool_base_ata (WSOL)
            Pubkey::new_unique(), // 8: pool_quote_ata (token)
            token_program,        // 9
            token_program,        // 10
            pumpswap,             // 11: program_id_index for self-CPI event
        ];
        (pool, user, traded_mint, pumpswap, accounts)
    }

    #[test]
    fn cpi_swap_buy_wsol_base_pool_is_token_sell() {
        // DISC_BUY on a WSOL-base pool: user pays quote(tokens) to get base(SOL).
        // Parser must emit is_buy=false, amount=quote_amount_in, min_sol_output=base_amount_out.
        let parser = BinaryParser::new(false);
        let (pool, user, _traded_mint, _pumpswap, accounts) = wsol_base_accounts();

        let base_amount_out = 4_862_258_819u64; // SOL in lamports (base=WSOL)
        let quote_amount_in = 2_731_681_363u64; // token raw units (quote=token)
        let pool_base_reserves = 200_000_000_000u64;
        let pool_quote_reserves = 800_000_000_000u64;

        let mut cpi_data = DISC_SWAP_OUTER_WRAPPER.to_vec();
        cpi_data.extend_from_slice(&DISC_SWAP_EVENT_BUY);
        cpi_data.extend_from_slice(&encode_swap_buy_event(&SwapBuyEvent {
            timestamp: 1,
            base_amount_out,
            max_quote_amount_in: quote_amount_in,
            user_base_token_reserves: 0,
            user_quote_token_reserves: 500_000_000,
            pool_base_token_reserves: pool_base_reserves,
            pool_quote_token_reserves: pool_quote_reserves,
            quote_amount_in,
            lp_fee_basis_points: 0,
            lp_fee_amount: 0,
            protocol_fee_basis_points: 0,
            protocol_fee_amount: 0,
            quote_amount_in_with_lp_fee: quote_amount_in,
            user_quote_amount_in: quote_amount_in,
            pool: pool.to_bytes(),
            user: user.to_bytes(),
        }));

        let event = GeyserEvent::Transaction {
            slot: Some(1),
            event_ts_ms: None,
            arrival_ts_ms: None,
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: solana_sdk::signature::Signature::new_unique(),
            accounts: accounts.clone(),
            // Top-level PumpSwap ix → pumpswap_pool_wsol_is_base reads accounts[3]=WSOL
            instructions: vec![crate::types::RawInstruction {
                program_id: Pubkey::from_str(PUMP_SWAP_PROGRAM_ID).unwrap(),
                account_indices: (0u8..9u8).collect(),
                data: DISC_SWAP_BUY.to_vec(),
            }],
            logs: vec![],
            block_time: None,
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: false,
            source: "test".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: RawBytesMissingReason::FilteredByConfig,
            // CPI event log in inner_instructions (Anchor self-CPI)
            inner_instructions: vec![crate::types::InnerInstructionGroup {
                index: 0,
                instructions: vec![crate::types::InnerIx {
                    program_id_index: 11, // pumpswap_program
                    accounts: vec![],
                    data: cpi_data,
                    stack_height: Some(2),
                }],
            }],
            pre_token_balances: vec![],
            post_token_balances: vec![],
        };

        let trades = parser.parse_trades(&event).expect("should parse");
        let trade = trades
            .iter()
            .find(|t| t.pool_amm_id == pool)
            .expect("trade for pool should exist");

        assert!(
            !trade.is_buy,
            "WSOL-base DISC_BUY = TOKEN SELL, got is_buy={}",
            trade.is_buy
        );
        assert_eq!(
            trade.amount, quote_amount_in,
            "token amount must be quote_amount_in (tokens paid)"
        );
        assert_eq!(
            trade.min_sol_output, base_amount_out,
            "SOL output must be base_amount_out (SOL received)"
        );
        assert_eq!(trade.max_sol_cost, 0);
    }

    #[test]
    fn cpi_swap_sell_wsol_base_pool_is_token_buy() {
        // DISC_SELL on a WSOL-base pool: user pays base(SOL) to get quote(tokens).
        // Parser must emit is_buy=true, amount=quote_amount_out, max_sol_cost=base_amount_in.
        let parser = BinaryParser::new(false);
        let (pool, user, _traded_mint, _pumpswap, accounts) = wsol_base_accounts();

        let base_amount_in = 3_000_000_000u64; // 3 SOL in lamports (base=WSOL)
        let quote_amount_out = 1_500_000u64; // 1.5 tokens @ 6 decimals (quote=token)
        let pool_base_reserves = 150_000_000_000u64;
        let pool_quote_reserves = 600_000_000_000u64;

        let mut cpi_data = DISC_SWAP_OUTER_WRAPPER.to_vec();
        cpi_data.extend_from_slice(&DISC_SWAP_EVENT_SELL);
        cpi_data.extend_from_slice(&encode_swap_sell_event(&SwapSellEvent {
            timestamp: 1,
            base_amount_in,
            min_quote_amount_out: quote_amount_out,
            user_base_token_reserves: 0,
            user_quote_token_reserves: 0,
            pool_base_token_reserves: pool_base_reserves,
            pool_quote_token_reserves: pool_quote_reserves,
            quote_amount_out,
            lp_fee_basis_points: 0,
            lp_fee_amount: 0,
            protocol_fee_basis_points: 0,
            protocol_fee_amount: 0,
            quote_amount_out_without_lp_fee: quote_amount_out,
            user_quote_amount_out: quote_amount_out,
            pool: pool.to_bytes(),
            user: user.to_bytes(),
        }));

        let event = GeyserEvent::Transaction {
            slot: Some(1),
            event_ts_ms: None,
            arrival_ts_ms: None,
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: solana_sdk::signature::Signature::new_unique(),
            accounts: accounts.clone(),
            instructions: vec![crate::types::RawInstruction {
                program_id: Pubkey::from_str(PUMP_SWAP_PROGRAM_ID).unwrap(),
                account_indices: (0u8..9u8).collect(),
                data: DISC_SWAP_SELL.to_vec(),
            }],
            logs: vec![],
            block_time: None,
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: false,
            source: "test".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: RawBytesMissingReason::FilteredByConfig,
            inner_instructions: vec![crate::types::InnerInstructionGroup {
                index: 0,
                instructions: vec![crate::types::InnerIx {
                    program_id_index: 11, // pumpswap_program
                    accounts: vec![],
                    data: cpi_data,
                    stack_height: Some(2),
                }],
            }],
            pre_token_balances: vec![],
            post_token_balances: vec![],
        };

        let trades = parser.parse_trades(&event).expect("should parse");
        let trade = trades
            .iter()
            .find(|t| t.pool_amm_id == pool)
            .expect("trade for pool should exist");

        assert!(
            trade.is_buy,
            "WSOL-base DISC_SELL = TOKEN BUY, got is_buy={}",
            trade.is_buy
        );
        assert_eq!(
            trade.amount, quote_amount_out,
            "token amount must be quote_amount_out (tokens received)"
        );
        assert_eq!(
            trade.max_sol_cost, base_amount_in,
            "SOL cost must be base_amount_in (SOL paid)"
        );
        assert_eq!(trade.min_sol_output, 0);
    }

    #[test]
    fn pumpswap_wsol_base_detected_via_inner_ix_only() {
        // Routed / aggregator flow: top-level instructions is empty.
        // PumpSwap swap ix + CPI event both live in inner_instructions.
        // pumpswap_pool_wsol_is_base() must resolve via inner ix, not top-level.
        let parser = BinaryParser::new(false);
        let (pool, user, _traded_mint, pumpswap, accounts) = wsol_base_accounts();

        let base_amount_in = 1_000_000_000u64; // 1 SOL (base=WSOL)
        let quote_amount_out = 500_000u64; // 0.5 tokens (quote=token)

        let mut cpi_data = DISC_SWAP_OUTER_WRAPPER.to_vec();
        cpi_data.extend_from_slice(&DISC_SWAP_EVENT_SELL);
        cpi_data.extend_from_slice(&encode_swap_sell_event(&SwapSellEvent {
            timestamp: 1,
            base_amount_in,
            min_quote_amount_out: quote_amount_out,
            user_base_token_reserves: 0,
            user_quote_token_reserves: 0,
            pool_base_token_reserves: 50_000_000_000,
            pool_quote_token_reserves: 20_000_000_000,
            quote_amount_out,
            lp_fee_basis_points: 0,
            lp_fee_amount: 0,
            protocol_fee_basis_points: 0,
            protocol_fee_amount: 0,
            quote_amount_out_without_lp_fee: quote_amount_out,
            user_quote_amount_out: quote_amount_out,
            pool: pool.to_bytes(),
            user: user.to_bytes(),
        }));

        // pumpswap is at accounts[11], so program_id_index = 11
        let pump_idx = accounts.iter().position(|p| *p == pumpswap).unwrap() as u8;

        let event = GeyserEvent::Transaction {
            slot: Some(2),
            event_ts_ms: None,
            arrival_ts_ms: None,
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: solana_sdk::signature::Signature::new_unique(),
            accounts,
            instructions: vec![], // empty — routed flow
            logs: vec![],
            block_time: None,
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: false,
            source: "test".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: RawBytesMissingReason::FilteredByConfig,
            inner_instructions: vec![crate::types::InnerInstructionGroup {
                index: 0,
                instructions: vec![
                    // Inner swap ix — pumpswap_pool_wsol_is_base reads accounts[3]=WSOL here
                    crate::types::InnerIx {
                        program_id_index: pump_idx,
                        accounts: (0u8..9u8).collect(),
                        data: DISC_SWAP_SELL.to_vec(),
                        stack_height: Some(2),
                    },
                    // Anchor self-CPI event log
                    crate::types::InnerIx {
                        program_id_index: pump_idx,
                        accounts: vec![],
                        data: cpi_data,
                        stack_height: Some(3),
                    },
                ],
            }],
            pre_token_balances: vec![],
            post_token_balances: vec![],
        };

        let trades = parser.parse_trades(&event).expect("should parse");
        let trade = trades
            .iter()
            .find(|t| t.pool_amm_id == pool)
            .expect("trade for pool should exist");

        assert!(
            trade.is_buy,
            "WSOL-base DISC_SELL via inner ix = TOKEN BUY, got is_buy={}",
            trade.is_buy
        );
        assert_eq!(
            trade.amount, quote_amount_out,
            "token amount = quote_amount_out (tokens received)"
        );
        assert_eq!(trade.max_sol_cost, base_amount_in);
        assert_eq!(trade.min_sol_output, 0);
    }
}
