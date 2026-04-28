# binary_parser.rs — analiza optymalizacji

Data: 2026-03-12
Plik: `off-chain/components/seer/src/binary_parser.rs` (6404 linie)

---

## Podsumowanie wykonawcze

Pięć proponowanych optymalizacji zostało ocenionych pod kątem rzeczywistego kodu.
**Dwie wprowadzono bezpośrednio** (zmiany jednolocal, bez ryzyka regresji).
**Trzy wymagają głębszej przebudowy** — opisano je jako plany implementacyjne.

---

## Opt-1: `FastCurveRegistry` — `Pubkey` zamiast `String` jako kluczy

### Stan obecny
```rust
// binary_parser.rs:831-867
pub struct CurveMintRegistry {
    curve_to_mint: Arc<DashMap<String, String>>,
    mint_to_curve: Arc<DashMap<String, String>>,
}
```

Każde zapytanie `mint_for_curve(&str)` / `curve_for_mint(&str)` robi:
- hash stringa (43 bajty Base58) przez domyślny `SipHash`
- `.clone()` na zwracanym `String` z DashMap

### Ocena
**Zasadna, ale wymaga przebudowy API wielu plików.**

`CurveMintRegistry` jest używany przez:
- `binary_parser.rs` — wewnętrznie w ~30 miejscach, klucz zawsze `&str` z Base58
- `lib.rs` — `pending_curve_updates`, `register_curve_mapping`, replay path
- `grpc_connection.rs` — `AccountRegistry` trzyma mapę curve→mint jako `String`

Granica cięcia jest na `GeyserEvent::Transaction.accounts: Vec<Pubkey>` — wewnętrznie
parser już operuje na `Pubkey`. Ale interfejsy zewnętrzne (`CurveMintRegistry::insert`,
`CurveMintRegistry::mint_for_curve`) przyjmują `&str` bo `grpc_connection` i `lib.rs`
dostarczają Base58 z protobuf.

Zmiana wymaga:
1. Nowego API `CurveMintRegistry` z kluczami `Pubkey`
2. Aktualizacji wszystkich call-site w `lib.rs` i `grpc_connection.rs`
3. Zmiany `CompleteTracker` (linia 962) — analogicznie `DashMap<String, bool>`
4. Zmiany `ResolveQueue` (linia 882) — klucz `curve: String` → `Pubkey`

Dodanie `ahash::RandomState` do `DashMap` — `parking_lot` jest już w Cargo.toml,
`ahash` wymaga dodania zależności. DashMap domyślnie używa `AHasher` od wersji 5.x —
**sprawdzić wersję w Cargo.toml przed dodawaniem.**

### Plan implementacji
```
1. Sprawdzić wersję dashmap w Cargo.toml — jeśli >= 5.0, AHash już jest domyślny.
2. Zmienić CurveMintRegistry: klucze Pubkey, wystawić stare &str-API jako shim
   (parsuje Base58 wewnętrznie) żeby nie ruszać grpc_connection.rs w tym samym PR.
3. Zmienić CompleteTracker analogicznie.
4. Zmienić ResolveQueue.inner: VecDeque<(Pubkey, ...)> — tylko wewnętrznie,
   push() przyjmuje nadal &str → konwertuje przy wejściu.
5. Osobny PR: usunięcie shimów gdy grpc_connection i lib.rs zostaną zaktualizowane.
```

**Priorytet:** średni. DashMap 5.x i tak używa AHash, więc główny zysk to
zredukowanie klonowania `String` → `Copy` dla `Pubkey`. Warte zrobienia razem z
większym refaktorem, nie jako osobna zmiana.

---

## Opt-2: Jito tip bez `bincode`

### Stan obecny
```rust
// binary_parser.rs:3377-3378
let Ok(SystemInstruction::Transfer { .. }) =
    bincode::deserialize::<SystemInstruction>(&instruction.data)
```

`bincode::deserialize` dla `SystemInstruction` — enum z 18 wariantami. Deserializuje
cały payload tylko po to żeby sprawdzić czy to Transfer.

### Ocena
**Zasadna i bezpieczna — zmiana lokalna, jeden plik, jeden blok.**

`SystemInstruction::Transfer` ma dyskryminator `2u32` w LE na bajtach 0-3. Wystarczy
sprawdzić `data[0..4] == [2, 0, 0, 0]`. Reszta payloadu (kwota u64) jest irrelewantna
dla detekcji Jito — nas interesuje tylko konto docelowe.

**Zmiana wprowadzona bezpośrednio** (patrz diff poniżej).

---

## Opt-3: Jeden skan zamiast trzech pętli (`extract_runtime_trade_context`)

### Stan obecny
```rust
// binary_parser.rs:3284-3291
let (compute_unit_limit, cu_price_micro_lamports) =
    extract_compute_budget_profile(instructions);   // pętla 1
let (inner_ix_count, cpi_depth, ata_create_count) =
    extract_inner_instruction_stats(inner_instructions); // pętla 2 (inner_ix, inna kolekcja)
let jito_tip_detected = detect_jito_tip(accounts, instructions); // pętla 3
```

### Ocena
**Pętle 1 i 3 iterują po tej samej kolekcji `instructions`** — scalenie w jeden
przelot jest zasadne. Pętla 2 (`inner_instructions`) to inna kolekcja, nie da się
połączyć bez zmiany sygnatury.

**Zmiana lokalna** — `extract_compute_budget_profile` i `detect_jito_tip` mogą być
połączone w jedną funkcję `extract_compute_and_jito_profile`. Nie dotyka innych plików.

**Zmiana wprowadzona bezpośrednio** (patrz diff poniżej).

---

## Opt-4: `parking_lot::Mutex` zamiast `std::sync::Mutex`

### Stan obecny
```rust
// binary_parser.rs:883
inner: std::sync::Mutex<VecDeque<(String, u64, Vec<u8>, Instant)>>,
```

### Ocena
**Zasadna, prosta, bezpieczna — `parking_lot` już jest w Cargo.toml.**

`ResolveQueue` jest używany wyłącznie w `binary_parser.rs` (definicja) i wywoływany
z `lib.rs` z jednego wątku asynchronicznego w krytycznej ścieżce accountów.
`parking_lot::Mutex` eliminuje syscall przy braku rywalizacji.

**Zmiana wprowadzona bezpośrednio.**

---

## Opt-5: `short()` w logach — zero-cost

### Stan obecny
```rust
// binary_parser.rs:2366-2368
fn short(s: &str) -> &str {
    &s[..8.min(s.len())]
}
```

Używane w `debug!()` i `trace!()` makrach — np. linia 157:
```rust
debug!("IX_DROPPED ... program={} ...", short(program), ...);
```

### Ocena
**Prawdziwy problem, ale `tracing` makra już go częściowo rozwiązują.**

`tracing::debug!` ocenia argumenty **zawsze** — w przeciwieństwie do `log::debug!`
który w release z `max_level_info` może być wyoptymalizowany przez `cfg`. W tracing
wartości są przekazywane jako `dyn Value`, a ewaluacja pól następuje przy każdym
wywołaniu niezależnie od aktywnego poziomu — o ile nie użyje się `enabled!()` guard.

`short(s)` to `min + slice` — bardzo tanie, ale zasada jest słuszna: w HFT każde
wywołanie funkcji na hot path z `debug!` to regresja gdy poziom logowania jest wyżej.

### Plan implementacji
```
Opcja A (wystarczająca): otoczenie wywołań debug!/trace! na hot path guardami:
    if tracing::enabled!(tracing::Level::DEBUG) {
        debug!("... {}", short(program));
    }

Opcja B (czysta): usunięcie short() z logów i użycie lazy formatowania:
    debug!(program = %&program[..program.len().min(8)], ...);
    — tracing field syntax ocenia wartość tylko gdy span/event jest aktywny.

Opcja C (radykalna): usunięcie debug!/trace! z hot path całkowicie,
    pozostawienie tylko warn!/error!.
```

Rekomendacja: **Opcja B** — bez zmiany logiki, poprawna semantyka tracing.
Hot path to `parse_account_raw` i `handle_buy`/`handle_sell` — wymaga
inwentaryzacji wszystkich wywołań `short()` (ok. 15-20 miejsc).

---

## Opt-6 (dodatkowa): `resolve_ata_owner` — `find_program_address` na każdym koncie

### Stan obecny
```rust
// binary_parser.rs:3920-3955
fn resolve_ata_owner(accounts: &[Pubkey], token_account: &Pubkey, mint: &Pubkey) -> Option<Pubkey> {
    for candidate_owner in accounts.iter().copied().filter(is_candidate_owner) {
        let derived = Pubkey::find_program_address(..., &associated_token_program).0;
        if derived == *token_account { return Some(candidate_owner); }
        let derived_2022 = Pubkey::find_program_address(...).0; // drugi hash
        if derived_2022 == *token_account { return Some(candidate_owner); }
    }
    None
}
```

### Ocena
**Problem jest realny, ale kod jest już zabezpieczony.**

`resolve_token_balance_owner` (linia 3634) sprawdza najpierw `balance_hint`:
```rust
fn resolve_token_balance_owner(
    balance_hint: Option<&RawTokenBalance>,
    accounts: &[Pubkey],
    token_account: &Pubkey,
    mint: &Pubkey,
) -> Option<Pubkey> {
    if let Some(balance) = balance_hint {
        if let Some(owner) = token_balance_owner_pubkey(balance) {
            return Some(owner);  // ← early return, find_program_address NIE jest wywoływany
        }
    }
    resolve_ata_owner(accounts, token_account, mint)  // fallback
}
```

W praktyce Yellowstone/Geyser **zawsze** wypełnia `owner` w `pre/post_token_balances`
dla kont z saldem — `resolve_ata_owner` odpala się tylko dla kont bez metadata w
protobuf (edge case: account z zerowym saldem w pre, brak w post, lub nowe konto
bez pre_balance).

Niemniej `is_candidate_owner` (linia 3956) wykonuje `pubkey.is_on_curve()` + 5x
`pubkey_str != PROGRAM_CONSTANT` dla każdego kandydata. `is_on_curve()` to operacja
grupowa (mnożenie na Ed25519) — kosztowna przy wielu kontach.

### Plan implementacji
```
1. Lazy-cache stałych programowych jako statyczne Pubkey:
   static ASSOCIATED_TOKEN_PROGRAM_PUBKEY: Lazy<Pubkey> = ...
   static TOKEN_PROGRAM_PUBKEY: Lazy<Pubkey> = ...
   (eliminuje Pubkey::from_str w każdym wywołaniu resolve_ata_owner)

2. is_candidate_owner: porównywać bezpośrednio jako Pubkey (== na [u8;32])
   zamiast .to_string() — o(32 bajtów) vs o(43 bajtów + alokacja).

3. resolve_ata_owner: ograniczyć candidates tylko do signerów (is_signer flag
   z TransactionStatusMeta) — signer to zwykle 1-2 konta, nie ~20.
   Wymaga przekazania informacji o signerach do funkcji.
```

---

## Co zostało zmienione

| # | Optymalizacja | Status |
|---|---------------|--------|
| 1 | `[u8;32]` klucze w `CurveMintRegistry`, `CompleteTracker`, `ResolveQueue` | **Wprowadzona** |
| 2 | Jito tip bez `bincode` | **Wprowadzona** |
| 3 | Jeden skan compute+jito | **Wprowadzona** |
| 4 | `parking_lot::Mutex` w `ResolveQueue` | **Wprowadzona** |
| 5 | `short()` zero-cost w logach — tracing named fields | **Wprowadzona** |
| 6 | `resolve_ata_owner` lazy statics + Pubkey cmp | Wymaga osobnego PR |
