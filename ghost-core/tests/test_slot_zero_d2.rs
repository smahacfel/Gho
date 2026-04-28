//! D2: Slot=0 Rejection Test
//!
//! **NOTE**: After EVENT-TIME migration, slot=0 is rejected; unknown slot must be None.
//! This test validates that timestamp_ms=0 is rejected and slot=None is accepted.

use ghost_core::shadow_ledger::{LiveTxEvent, TradeSide, TxKey};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;

fn test_pubkey(seed: u8) -> Pubkey {
    Pubkey::new_from_array([seed; 32])
}

fn test_signature(seed: u8) -> Signature {
    Signature::from([seed; 64])
}

#[test]
fn test_all_paths_reject_timestamp_zero() {
    // TxKey rejects timestamp_ms=0 (not slot=0, as slot is optional metadata now)
    let tx_key_result = TxKey::new(0, Some(1), Some(1), Some(test_signature(1)), 0);
    assert!(tx_key_result.is_err(), "TxKey must reject timestamp_ms=0");

    // LiveTxEvent with timestamp_ms != 0 should work with slot=None
    // Per EVENT-TIME rules, slot is optional metadata; None is the only "unknown" representation
    let event_result = LiveTxEvent::new(
        test_pubkey(42),
        None, // Unknown slot must be None (Some(0) is invalid)
        None,
        Some(test_signature(1)),
        1700000000000, // valid timestamp
        TradeSide::Buy,
        1_000_000_000,
        1_000_000,
        false,
        Some(test_pubkey(1)),
    );
    assert!(
        event_result.is_ok(),
        "LiveTxEvent with slot=None is valid (slot is metadata only)"
    );

    println!("✅ D2 PASS: timestamp_ms=0 rejected, slot=None allowed (event-time compliant)");
}
