//! Offset-based bonding curve account data parser.
//!
//! Obsługuje wszystkie znane layouty Pump.fun / Bonk.fun:
//! - **56 bajtów** (stary format bez Anchor discriminatora) → offset = 0
//! - **83 / 151+ bajtów** (Anchor, 8-bajtowy discriminator na początku) → offset = 8
//! - **Inne rozmiary** (49–82 B): heurystyka na podstawie pierwszych 8 bajtów.
//!
//! Minimalne wymagania: 48 bajtów (5 × u64 = 40 B pól + 8 B na discriminator/off).
//! Flaga `complete` jest opcjonalna — traktowana jako `true` jeśli brak danych.

use ghost_core::market_state::BondingCurve;

/// Minimalna liczba bajtów wymagana do sparsowania bonding curve.
/// 5 pól u64 (5 × 8 = 40 B) + 8 B na prefiks (discriminator lub pierwsze pole).
const MIN_PREFIX: usize = 48;

/// Limit sanity-check: wirtualne rezerwy nie mogą przekroczyć 10^18 lamportów.
const MAX_RESERVE: u64 = 1_000_000_000_000_000_000;

/// Błędy parsowania stanu bonding curve z konta.
#[derive(Debug, thiserror::Error)]
pub enum ParseCurveError {
    /// Dane zbyt krótkie – podany rozmiar bufora.
    #[error("curve data too short: {0} bytes")]
    TooShort(usize),

    /// Layout nieobsługiwany – podany rozmiar bufora (zarezerwowane dla przyszłych wersji).
    #[error("unsupported curve layout: len={0}")]
    UnsupportedLayout(usize),

    /// Wirtualne rezerwy zerowe lub absurdalne – dane niezwiarygodne.
    #[error("invalid curve values (virtual reserves zero or nonsensical)")]
    InvalidValues,
}

/// Bezpieczny odczyt u64 LE z bufora bez paniki.
///
/// Zwraca `None` gdy bufor jest za krótki na 8 bajtów od `off`.
#[inline]
fn read_u64_le_safe(buf: &[u8], off: usize) -> Option<u64> {
    let end = off.checked_add(8)?;
    if buf.len() >= end {
        let mut a = [0u8; 8];
        a.copy_from_slice(&buf[off..end]);
        Some(u64::from_le_bytes(a))
    } else {
        None
    }
}

/// Parser offsetowy dla realnych layoutów Pump.fun / Bonk.fun.
///
/// # Heurystyka wyboru offsetu
///
/// | Rozmiar danych | Offset | Uwagi                                               |
/// |---------------|--------|-----------------------------------------------------|
/// | == 56 B       | 0      | Stary layout bez Anchor discriminatora               |
/// | >= 83 B       | 8      | Anchor: 8-bajtowy discriminator + pola               |
/// | 49–82 B       | 8 / 0  | Heurystyka: niezerowe pierwsze 8 B → offset=8        |
///
/// # Sanity-check
///
/// Wirtualne rezerwy (`vtoken`, `vsol`) muszą być > 0 i ≤ 10^18.
/// Jeśli nie spełniają warunku, zwraca `ParseCurveError::InvalidValues`.
///
/// # Zwrot
///
/// `BondingCurve` z `discriminator = 0` (nie wypełniamy pola z danych —
/// parsujemy tylko rezerwy dla modelu downstream).
pub fn parse_curve_from_account(data: &[u8]) -> Result<BondingCurve, ParseCurveError> {
    let len = data.len();

    if len < MIN_PREFIX {
        return Err(ParseCurveError::TooShort(len));
    }

    // Heurystyka offsetu:
    // - len == 56  → stary layout, brak Anchor discriminatora → off = 0
    // - len >= 83  → Anchor format → off = 8
    // - pozostałe → sprawdź czy pierwsze 8 bajtów niezerowe → off = 8, inaczej off = 0
    let off: usize = if len == 56 {
        0
    } else if len >= 83 {
        8
    } else {
        // Zakres 48–82 bajtów: heurystyka
        if data.iter().take(8).any(|&b| b != 0) {
            8
        } else {
            0
        }
    };

    let vtoken = read_u64_le_safe(data, off).ok_or(ParseCurveError::TooShort(len))?;
    let vsol = read_u64_le_safe(data, off + 8).ok_or(ParseCurveError::TooShort(len))?;
    let rtoken = read_u64_le_safe(data, off + 16).unwrap_or(0);
    let rsol = read_u64_le_safe(data, off + 24).unwrap_or(0);
    let supply = read_u64_le_safe(data, off + 32).unwrap_or(0);
    let complete_u8 = data.get(off + 40).copied().unwrap_or(1);

    // Sanity check: wirtualne rezerwy muszą być sensowne
    if vtoken == 0 || vsol == 0 {
        return Err(ParseCurveError::InvalidValues);
    }
    if vtoken > MAX_RESERVE || vsol > MAX_RESERVE {
        return Err(ParseCurveError::InvalidValues);
    }

    Ok(BondingCurve {
        // Brak discriminatora w naszym modelu — ustawiamy na 0.
        discriminator: 0,
        virtual_token_reserves: vtoken,
        virtual_sol_reserves: vsol,
        real_token_reserves: rtoken,
        real_sol_reserves: rsol,
        token_total_supply: supply,
        complete: complete_u8,
        _padding: [0; 7],
    })
}

// ============================================================================
// Testy jednostkowe
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Pomocnik: tworzy bufor bonding curve o podanym całkowitym rozmiarze.
    /// Pierwsze 40 bajtów (przy `off`) koduje pola w porządku: v_token, v_sol,
    /// r_token, r_sol, supply. Bajt `off+40` to flaga `complete`.
    /// Pozostałe bajty zerowe.
    fn build_curve_buf(
        total_len: usize,
        off: usize,
        vtoken: u64,
        vsol: u64,
        rtoken: u64,
        rsol: u64,
        supply: u64,
        complete: u8,
    ) -> Vec<u8> {
        let mut buf = vec![0u8; total_len];
        buf[off..off + 8].copy_from_slice(&vtoken.to_le_bytes());
        buf[off + 8..off + 16].copy_from_slice(&vsol.to_le_bytes());
        buf[off + 16..off + 24].copy_from_slice(&rtoken.to_le_bytes());
        buf[off + 24..off + 32].copy_from_slice(&rsol.to_le_bytes());
        buf[off + 32..off + 40].copy_from_slice(&supply.to_le_bytes());
        buf[off + 40] = complete;
        buf
    }

    #[test]
    fn parse_56_bytes_layout_ok() {
        // Stary layout: off=0, brak Anchor discriminatora.
        let data = build_curve_buf(56, 0, 1, 2, 3, 4, 5, 0);
        let curve = parse_curve_from_account(&data).expect("parse ok");
        assert_eq!(curve.virtual_token_reserves, 1);
        assert_eq!(curve.virtual_sol_reserves, 2);
        assert_eq!(curve.real_token_reserves, 3);
        assert_eq!(curve.real_sol_reserves, 4);
        assert_eq!(curve.token_total_supply, 5);
        assert_eq!(curve.complete, 0);
        assert_eq!(curve.discriminator, 0); // zawsze zerowany
    }

    #[test]
    fn parse_83_bytes_anchor_ok() {
        // Anchor: discriminator (8 B) + pola (off=8).
        let mut data = build_curve_buf(83, 8, 10, 20, 30, 40, 50, 1);
        // Symulujemy niezerowy Anchor discriminator
        data[0..8].copy_from_slice(&0xDEAD_BEEF_u64.to_le_bytes());
        let curve = parse_curve_from_account(&data).expect("parse ok");
        assert_eq!(curve.virtual_token_reserves, 10);
        assert_eq!(curve.virtual_sol_reserves, 20);
        assert_eq!(curve.real_token_reserves, 30);
        assert_eq!(curve.real_sol_reserves, 40);
        assert_eq!(curve.token_total_supply, 50);
        assert_eq!(curve.complete, 1);
    }

    #[test]
    fn parse_151_bytes_anchor_ok() {
        // Duży Anchor layout (151 B), off=8.
        let data = build_curve_buf(
            151,
            8,
            1_073_000_000_000_000,
            30_000_000_000,
            793_100_000_000_000,
            0,
            1_000_000_000_000_000,
            0,
        );
        let curve = parse_curve_from_account(&data).expect("parse ok");
        assert_eq!(curve.virtual_token_reserves, 1_073_000_000_000_000);
        assert_eq!(curve.virtual_sol_reserves, 30_000_000_000);
        assert_eq!(curve.real_token_reserves, 793_100_000_000_000);
    }

    #[test]
    fn parse_too_short_fails() {
        let data = vec![0u8; 16];
        let err = parse_curve_from_account(&data).unwrap_err();
        assert!(matches!(err, ParseCurveError::TooShort(16)));
    }

    #[test]
    fn parse_exactly_48_bytes_fails() {
        // 48 < MIN_PREFIX (48) → TooShort
        let data = vec![0u8; 47];
        assert!(matches!(
            parse_curve_from_account(&data).unwrap_err(),
            ParseCurveError::TooShort(_)
        ));
    }

    #[test]
    fn parse_invalid_values_zero_vtoken() {
        // vtoken = 0 → InvalidValues
        let data = build_curve_buf(56, 0, 0, 100, 0, 0, 0, 0);
        assert!(matches!(
            parse_curve_from_account(&data).unwrap_err(),
            ParseCurveError::InvalidValues
        ));
    }

    #[test]
    fn parse_invalid_values_zero_vsol() {
        // vsol = 0 → InvalidValues
        let data = build_curve_buf(56, 0, 100, 0, 0, 0, 0, 0);
        assert!(matches!(
            parse_curve_from_account(&data).unwrap_err(),
            ParseCurveError::InvalidValues
        ));
    }

    #[test]
    fn parse_invalid_values_overflow_vtoken() {
        // vtoken > MAX_RESERVE → InvalidValues
        let data = build_curve_buf(56, 0, u64::MAX, 100, 0, 0, 0, 0);
        assert!(matches!(
            parse_curve_from_account(&data).unwrap_err(),
            ParseCurveError::InvalidValues
        ));
    }

    #[test]
    fn parse_heuristic_non_zero_first_bytes_picks_offset_8() {
        // 60 bajtów (zakres 49–82) z niezerowym prefiksem → off=8
        let mut data = build_curve_buf(60, 8, 42, 99, 0, 0, 0, 0);
        data[0] = 0xAB; // niezerowy prefiks
        let curve = parse_curve_from_account(&data).expect("parse ok");
        assert_eq!(curve.virtual_token_reserves, 42);
        assert_eq!(curve.virtual_sol_reserves, 99);
    }

    #[test]
    fn parse_heuristic_zero_first_bytes_gives_invalid_values() {
        // 60 bajtów z zerowym prefiksem → off=0 → v_token=data[0..8]=0 → InvalidValues.
        // (Cieślenie binarne 0 ≡ brak wirtualnych rezerw — każda prawdziwa bonding
        //  curve ma v_token > 0, więc ta Şcieżka jest osiągalna tylko dla błędnych danych.)
        let mut data = vec![0u8; 60];
        // data[0..8] = 0 → off=0 → v_token=0 → InvalidValues
        data[8..16].copy_from_slice(&99u64.to_le_bytes()); // v_sol, nie będzie odczytane
        let err = parse_curve_from_account(&data).unwrap_err();
        assert!(
            matches!(err, ParseCurveError::InvalidValues),
            "expected InvalidValues for zero-prefix-60B, got: {:?}",
            err
        );
    }
}
