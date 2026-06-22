# ADR-8D: TimeStop V2 Stale Market Sample Reduction

Status: IMPLEMENTED / TARGETED_TESTS_PASSED
Typ: ADR-8D / post-buy shadow lifecycle telemetry / TimeStop V2 stale-classification repair
Data: 2026-06-22
Autor/Agent: Codex
Repo/branch: `/root/Gho`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: ograniczenie zawyzonego `stale_or_missing_market_sample` w TimeStop V2 przez rozdzielenie braku probki, invalid probki i valid probki bez nowego market update
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `ghost-brain/src/guardian/post_buy/engine.rs`
- `docs/ADR/ADR_8D_TIMESTOP_V2_STALE_MARKET_SAMPLE_REDUCTION_20260622.md`

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w ostatnich raportach.

## 1. Przygotowanie i dzialania wstepne

Cel:
Wyjasnic i zmniejszyc zawyzony udzial `stale_or_missing_market_sample` w rekordach `time_stop_v2_window`, bez zmiany Gatekeeper policy, SSOT, live execution, legacy `TimeStop` ani aktywnego close behavior.

Kontekst runtime:
- TimeStop V2 pozostaje mechanizmem telemetrycznym / observe-only.
- Rekordy `time_stop_v2_window` sa emitowane w shadow lifecycle jako dowod analityczny.
- R46 uruchomiony przed ta zmiana nie uzyje nowej logiki do czasu rebuild/restart.

Non-goals:
- brak zmiany progow `Target` / `StopLoss`
- brak zmiany `wait_for_timestop`
- brak aktywnego zamykania pozycji przez TimeStop V2
- brak zmiany `MaterializedFeatureSet`
- brak zmiany Gatekeeper policy
- brak zmiany shadow/live boundary

## 2. Opis problemu - 3W2H

What:
Analiza R46 pokazala bardzo wysoki udzial `stale_or_missing_market_sample` w TimeStop V2 windows. Przy 9577 window rows stale stanowil 5897 rekordow, czyli okolo 61.6%.

Where:
- `ghost-brain/src/guardian/post_buy/engine.rs`
- `TimeStopV2State::evaluate()`
- lifecycle JSONL: `shadow_lifecycle.jsonl`, `probe_shadow_lifecycle.jsonl`
- pola `time_stop_v2_status`, `time_stop_v2_subreason`, `time_stop_v2_*_delta_*`

Why:
Poprzednia klasyfikacja laczyla w jednym koszyku kilka roznych stanow:
- brak probki rynkowej,
- invalid/unknown price sample,
- valid probka, ale bez nowego slotu / timestampu / tx count wzgledem poprzedniego checkpointu V2.

To zawyzalo missing/stale i ukrywalo okna, ktore powinny byc mierzalne jako valid, ale z zerowa delta.

How:
Zmieniono logike `TimeStopV2State::evaluate()` tak, aby gdy istnieje poprzedni checkpoint i istnieje aktualna probka:
- delty byly emitowane zawsze,
- invalid price/market sample byl klasyfikowany jako `invalid_market_sample`,
- valid probka bez nowego market update byla klasyfikowana jako `weak/no_new_market_sample`,
- tylko realny brak probki pozostawal `missing_market_sample`.

How many:
Zmiana dotyczy jednego modulu runtime telemetry i addytywnej semantyki string enum w JSONL. Nie zmienia aktywnego zamykania pozycji.

## 3. Przyczyna zrodlowa

Root cause:
`TimeStopV2State::evaluate()` traktowal `fresh_latest == false` jak brak swiezej probki, a nie jak valid probke bez nowej informacji rynkowej. W praktyce canonical snapshot timeline moze zwrocic ten sam latest snapshot w kolejnym ticku, jezeli stan poola nie zmienil sie od poprzedniego checkpointu.

Skutek:
- valid martwe okna byly raportowane jako stale/missing,
- `missing_all` w analizie zero-fraction bylo zawyzone,
- operator nie mogl odroznic realnego braku danych od prawdziwej stagnacji poola,
- delty dla takich okien nie byly emitowane, wiec analityka widziala missing zamiast zero.

## 4. Strategia naprawy

Przyjeta strategia:
- Zachowac konserwatywna klasyfikacje realnych brakow danych.
- Rozdzielic subreasony:
  - `missing_market_sample` dla braku aktualnej probki,
  - `invalid_market_sample` dla invalid/unknown lub niefinitywnej ceny/mcap,
  - `no_new_market_sample` dla valid probki bez postepu wzgledem checkpointu.
- Dla valid unchanged sample emitowac zero-delta metryki, aby analiza window zero-fraction widziala prawdziwe zera zamiast missing.
- Nie przesuwac checkpointu V2 przy `no_new_market_sample`, aby kolejne okno dalej porownywalo sie z ostatnim rzeczywistym market update.

## 5. Przeprowadzone akcje naprawcze

Zmiana 1: jawniejsza semantyka subreasonow
- Dodano `MissingMarketSample`.
- Dodano `InvalidMarketSample`.
- Dodano `NoNewMarketSample`.
- Zachowano stary `StaleOrMissingMarketSample` jako kompatybilny fallback dla nietypowego stanu bez poprzedniego checkpointu.

Zmiana 2: walidacja price sample
- `TimeStopV2Checkpoint` przechowuje teraz `price_state`.
- `invalid_market_sample` obejmuje:
  - niefinitywna lub niepozytywna cena,
  - niefinitywny lub niepozytywny market cap,
  - `price_state` inny niz valid.

Zmiana 3: zero-delta dla valid unchanged sample
- Jezeli aktualna probka jest valid, ale nie jest nowsza od checkpointu, status to `weak`, subreason to `no_new_market_sample`.
- W takim rekordzie emitowane sa delty:
  - `time_stop_v2_tx_delta_window = 0`
  - `time_stop_v2_volume_delta_sol_window = 0`
  - `time_stop_v2_price_delta_pct_window = 0`
  - `time_stop_v2_mcap_delta_pct_window = 0`
  - `time_stop_v2_bonding_delta_pct_window = 0`

Zmiana 4: testy regresyjne
- Dodano test valid unchanged sample -> `weak/no_new_market_sample` z zerowymi deltami.
- Dodano test invalid sample -> `stale_or_insufficient/invalid_market_sample`.

## 6. Walidacja

| Walidacja | Wynik | Status |
|---|---|---|
| `cargo fmt --package ghost-brain` | wykonane po zmianie importu i logiki | PASS |
| `cargo test -q -p ghost-brain --lib time_stop_v2` | 7 passed | PASS |
| `git diff --check -- ghost-brain/src/guardian/post_buy/engine.rs docs/ADR/ADR_8D_TIMESTOP_V2_STALE_MARKET_SAMPLE_REDUCTION_20260622.md` | no whitespace errors | PASS |

## 7. Ryzyka i zabezpieczenia

Ryzyko 1: pomylenie stagnacji z realnym brakiem danych.
- Zabezpieczenie: valid unchanged sample dostaje osobny subreason `no_new_market_sample`; realny brak probki dostaje `missing_market_sample`.

Ryzyko 2: invalid probka traktowana jako zero-delta.
- Zabezpieczenie: invalid cena/mcap albo invalid `price_state` pozostaje `stale_or_insufficient/invalid_market_sample`.

Ryzyko 3: aktywna zmiana close behavior.
- Zabezpieczenie: zmiana dotyczy TimeStop V2 observe-only telemetry. Nie wywoluje close, nie ustawia legacy `CloseReason`, nie usuwa pozycji.

Ryzyko 4: R46 nie pokazuje efektu po patchu.
- Zabezpieczenie: jawnie oznaczono, ze dzialajacy proces uruchomiony przed rebuild/restart nadal uzywa starego binarium.

Ryzyko 5: canonical timeline nadal moze miec stale timestamp semantics.
- Obserwacja: ta zmiana nie normalizuje timestampow canonical snapshot timeline. Naprawia zawyzone missing przez valid unchanged sample, ale osobny runtime proof po rebuildzie jest wymagany, aby ocenic pozostala czesc `missing_market_sample`.

## 8. Decyzja

TimeStop V2 powinien rozdzielac brak danych od stagnacji rynku. Valid snapshot bez nowego market update nie jest juz raportowany jako `stale_or_missing_market_sample`; jest raportowany jako `weak/no_new_market_sample` z zerowymi deltami. Dzieki temu kolejne analizy window-size i vitality beda operowaly na rozroznialnym sygnale: missing/invalid vs prawdziwe zero-delta.
