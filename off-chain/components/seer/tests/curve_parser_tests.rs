//! Testy integracyjne dla `curve_parser::parse_curve_from_account`.
//!
//! Uruchamiane jako osobna skrzynka testowa (`cargo test --test curve_parser_tests`).

use seer::curve_parser::{parse_curve_from_account, ParseCurveError};

// ============================================================================
// Testy layoutu 56-bajtowego (stary format bez Anchor discriminatora, off=0)
// ============================================================================

#[test]
fn parse_56_bytes_layout_ok() {
    let mut data = vec![0u8; 56];
    // Stary layout: virtual_token od offsetu 0 (bez discriminatora Anchor).
    data[0..8].copy_from_slice(&1u64.to_le_bytes()); // virtual_token
    data[8..16].copy_from_slice(&2u64.to_le_bytes()); // virtual_sol
    data[16..24].copy_from_slice(&3u64.to_le_bytes()); // real_token
    data[24..32].copy_from_slice(&4u64.to_le_bytes()); // real_sol
    data[32..40].copy_from_slice(&5u64.to_le_bytes()); // supply
    data[40] = 0; // complete = false

    let curve = parse_curve_from_account(&data).expect("parse ok");
    assert_eq!(curve.virtual_token_reserves, 1);
    assert_eq!(curve.virtual_sol_reserves, 2);
    assert_eq!(curve.real_token_reserves, 3);
    assert_eq!(curve.real_sol_reserves, 4);
    assert_eq!(curve.token_total_supply, 5);
    assert_eq!(curve.complete, 0);
    // Discriminator zawsze zerowany przez nowy parser
    assert_eq!(curve.discriminator, 0);
}

// ============================================================================
// Testy layoutu Anchor 151-bajtowego (off=8)
// ============================================================================

#[test]
fn parse_151_bytes_anchor_ok() {
    let mut data = vec![0u8; 151];
    // Symuluj niezerowy Anchor discriminator
    data[0] = 1;
    // Pola od offsetu 8
    data[8..16].copy_from_slice(&10u64.to_le_bytes()); // virtual_token
    data[16..24].copy_from_slice(&20u64.to_le_bytes()); // virtual_sol
    data[24..32].copy_from_slice(&30u64.to_le_bytes()); // real_token
    data[32..40].copy_from_slice(&40u64.to_le_bytes()); // real_sol
    data[40..48].copy_from_slice(&50u64.to_le_bytes()); // supply
    data[48] = 1; // complete = true

    let curve = parse_curve_from_account(&data).expect("parse ok");
    assert_eq!(curve.virtual_token_reserves, 10);
    assert_eq!(curve.virtual_sol_reserves, 20);
    assert_eq!(curve.real_token_reserves, 30);
    assert_eq!(curve.real_sol_reserves, 40);
    assert_eq!(curve.token_total_supply, 50);
    assert_eq!(curve.complete, 1);
}

// ============================================================================
// Testy błędów
// ============================================================================

#[test]
fn parse_too_short_fails() {
    let data = vec![0u8; 16];
    let err = parse_curve_from_account(&data).unwrap_err();
    assert!(
        matches!(err, ParseCurveError::TooShort(16)),
        "expected TooShort(16), got: {:?}",
        err
    );
}

#[test]
fn parse_zero_virtual_token_fails() {
    // 56-bajtowy layout z virtual_token=0 → InvalidValues
    let mut data = vec![0u8; 56];
    data[0..8].copy_from_slice(&0u64.to_le_bytes()); // virtual_token = 0
    data[8..16].copy_from_slice(&100u64.to_le_bytes()); // virtual_sol != 0
    let err = parse_curve_from_account(&data).unwrap_err();
    assert!(
        matches!(err, ParseCurveError::InvalidValues),
        "expected InvalidValues, got: {:?}",
        err
    );
}

#[test]
fn parse_zero_virtual_sol_fails() {
    // 56-bajtowy layout z virtual_sol=0 → InvalidValues
    let mut data = vec![0u8; 56];
    data[0..8].copy_from_slice(&100u64.to_le_bytes()); // virtual_token != 0
    data[8..16].copy_from_slice(&0u64.to_le_bytes()); // virtual_sol = 0
    let err = parse_curve_from_account(&data).unwrap_err();
    assert!(
        matches!(err, ParseCurveError::InvalidValues),
        "expected InvalidValues, got: {:?}",
        err
    );
}

#[test]
fn parse_overflow_vtoken_fails() {
    // virtual_token > 10^18 → InvalidValues
    let mut data = vec![0u8; 56];
    data[0..8].copy_from_slice(&u64::MAX.to_le_bytes()); // vtoken = u64::MAX
    data[8..16].copy_from_slice(&100u64.to_le_bytes()); // vsol != 0
    let err = parse_curve_from_account(&data).unwrap_err();
    assert!(
        matches!(err, ParseCurveError::InvalidValues),
        "expected InvalidValues for overflow vtoken, got: {:?}",
        err
    );
}

// ============================================================================
// Testy heurystyki offsetu (zakres 48-82 B)
// ============================================================================

#[test]
fn parse_60_bytes_nonzero_prefix_picks_offset_8() {
    // 60 bajtów z niezerowym prefiksem → off=8
    let mut data = vec![0u8; 60];
    data[0] = 0xFF; // niezerowy bajt w prefiksie → off=8
    data[8..16].copy_from_slice(&42u64.to_le_bytes()); // virtual_token @ off=8
    data[16..24].copy_from_slice(&99u64.to_le_bytes()); // virtual_sol @ off=8
    let curve = parse_curve_from_account(&data).expect("parse ok");
    assert_eq!(curve.virtual_token_reserves, 42);
    assert_eq!(curve.virtual_sol_reserves, 99);
}

#[test]
fn parse_60_bytes_zero_prefix_gives_invalid_values() {
    // Zerowy prefiks (data[0..8]=0) → heurystyka wybiera off=0 → v_token=0 → InvalidValues.
    // Każda prawdziwa bonding curve ma v_token > 0, więc LE-encoding dałoby niezerowy
    // pierwsz bajt. Ten test wskazuje, że Şcieżka zerowego prefiksu jest blokiem danych
    // (nie można uzyskać prawidłowych rezerw przy off=0 gdy pierwsze 8 bajtów = 0).
    let data = vec![0u8; 60]; // całkowicie zerowe dane
    let err = parse_curve_from_account(&data).unwrap_err();
    assert!(
        matches!(err, ParseCurveError::InvalidValues),
        "expected InvalidValues for all-zero 60B, got {:?}",
        err
    );
}

// ============================================================================
// Test realnych wartości Pump.fun (sanity)
// ============================================================================

#[test]
fn parse_151_bytes_pumpfun_realistic_values() {
    // Realistyczne wartości z typowej bonding curve Pump.fun
    let mut data = vec![0u8; 151];
    // Anchor discriminator (8 B)
    data[0..8].copy_from_slice(&0xDEAD_BEEF_CAFE_BABEu64.to_le_bytes());
    // Pola od off=8
    data[8..16].copy_from_slice(&1_073_000_000_000_000u64.to_le_bytes()); // v_token
    data[16..24].copy_from_slice(&30_000_000_000u64.to_le_bytes()); // v_sol ~30 SOL
    data[24..32].copy_from_slice(&793_100_000_000_000u64.to_le_bytes()); // r_token
    data[32..40].copy_from_slice(&0u64.to_le_bytes()); // r_sol = 0 (dozwolone)
    data[40..48].copy_from_slice(&1_000_000_000_000_000u64.to_le_bytes()); // supply
    data[48] = 0; // complete = false

    let curve = parse_curve_from_account(&data).expect("parse ok");
    assert_eq!(curve.virtual_token_reserves, 1_073_000_000_000_000);
    assert_eq!(curve.virtual_sol_reserves, 30_000_000_000);
    assert_eq!(curve.real_token_reserves, 793_100_000_000_000);
    assert_eq!(curve.real_sol_reserves, 0);
    assert_eq!(curve.token_total_supply, 1_000_000_000_000_000);
    assert_eq!(curve.complete, 0);
}
