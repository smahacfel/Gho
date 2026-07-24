# ADR-8D: RUG SCALP V2 canonical Pump program-ID universe guard

Status: `IMPLEMENTED / FIVE_MINUTE_SHADOW_PREFLIGHT_PENDING`

Typ: ADR-8D / prospective shadow validation correction

Data: `2026-07-23`

Repo: `smahacfel/Gho`

Branch: `agent/rug-scalp-v2-prospective-shadow-20260721`

Plan SSOT: `PLANS/DO_REALIZACJI/PLAN_RUG_SCALP_V2_PROSPECTIVE_SHADOW_20260721.md`

## D0. Decyzja

`RugScalpSignalReducerV2` uznaje pool za należący do universe wyłącznie,
gdy `DetectedPool.amm_program` po poprawnym parsowaniu jako `Pubkey` jest
równy `RUG_SCALP_PUMP_PROGRAM`.

Etykieta `"pumpfun"` nie jest canonical runtime identity i nie jest
akceptowana jako alternatywna ścieżka.

## D1. Problem

R6 zapisał 2 663 canonical Pump birthy, lecz wszystkie miały
`amm_program=6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P` zamiast testowej
etykiety `"pumpfun"`. Poprzedni literalny guard odrzucił przez to 331 419
kolejnych ocen jako `universe_ineligible` przed uruchomieniem pozostałych,
zamrożonych predykatów sygnału.

## D2. Zakres

Zmiana obejmuje tylko universe predicate i jego fixture'y w
`ghost-launcher/src/rug_scalp_v2.rs`.

Nie zmieniono progów sygnału, sizingu 0.10/0.20 SOL, quote math, validation
tape, Position Managera, fee authority, backfillu, PM ani Gatekeepera.

## D3. Kontrakt fail-closed

Canonical Pump ID wraz z WSOL, niepustym bonding curve i obecnym slotem jest
eligible. Label `"pumpfun"`, inny/malformed program ID, inny quote mint,
pusty curve lub brak slotu są ineligible.

Brak `tx_index` z RPC `grpc_backfill` nadal kończy RUG jako non-evaluable;
nie wprowadzono timestampowego ani syntetycznego porządku.

## D4. Weryfikacja

Przechodzą cztery targeted tests:

1. canonical Pump `Pubkey` jest eligible;
2. label `"pumpfun"` jest odrzucony;
3. inny program ID jest odrzucony;
4. WSOL/curve/slot nadal fail-closed.

Przechodzi także istniejący test pełnego, dwuslotowego burstu oraz `cargo
check --bin ghost-launcher`, `cargo fmt --check` i `git diff --check`.

## D5. Następny krok

Po release build uruchomić jeden czysty, maksymalnie pięciominutowy,
shadow-only preflight. PASS nie wymaga accepted signal; musi jedynie dowieść,
że canonical Pump birthy przechodzą universe guard i assessmenty osiągają
rzeczywiste późniejsze reason codes.

Run A pozostaje niedozwolony do czasu zakończenia tego preflightu i freeze'u
commit/binary/config/authority evidence.
