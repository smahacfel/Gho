# ADR-8D: R33 shadow-only legacy BUY refresh przed symulacja

Status: IMPLEMENTED / SMOKE VALIDATED
Typ: Runtime shadow simulation reliability fix
Data: 2026-06-18
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `codex/gatekeeper-edge-policy-redesign-r1`
Commit/PR: not committed
Zakres: aktywna sciezka `TriggerEntryMode::ShadowOnly` dla legacy pump.fun BUY simulation
Dotkniete moduly/pliki:
- `ghost-launcher/src/components/trigger/component.rs`
- `configs/rollout/shadow-burnin-v3-r33-maxwait15000-fsc-off-r1.toml`
Powiazane runy/logi/raporty:
- R33 smoke start: `start_ms=1781744352305`
- `logs/shadow_run/shadow-burnin-v3-r33-maxwait15000-fsc-off-r1-buys.jsonl`
- `logs/shadow_run/shadow-burnin-v3-r33-maxwait15000-fsc-off-r1/shadow_entries.jsonl`
- `logs/rollout/shadow-burnin-v3-r33-maxwait15000-fsc-off-r1/system.log`
Poziom ryzyka: medium

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Ustalic, dlaczego R33 ma tylko ok. 78.5% pelnych sukcesow shadow simulation i skategoryzowac dominujace bledy bez zmiany Gatekeeper policy, progow, scoringu, execution/send path ani runtime BUY semantics.

Rzeczywisty przebieg:
Zatrzymano aktywny smoke/run R33, przeanalizowano transport rows oraz logi runtime dla `quote_slippage_error`, `account_pda_constraint_error` i `route_materialization_error`. Potwierdzono, ze dominujaca klasa przed poprawka byla `quote_slippage_error` z `TooMuchSolRequired` / `Custom(6002)`. Nastepnie wdrozono waski refresh legacy-buy request tuz przed shadow-only simulation i uruchomiono krotki smoke na nowej release binarce.

Odchylenia od planu:
Poczatkowo rozwazano zwiekszenie concurrency `trigger.shadow_run.max_concurrent`, ale ustalono, ze aktywna sciezka R33 `entry_mode = "shadow_only"` uzywa inline `simulate_buy()` i ta wartosc nie steruje glownym BUY path. Config R33 przywrocono do `trigger.shadow_run.max_concurrent = 1`; `p37_shadow_probe.max_concurrent = 16` pozostalo bez zmian.

## 2. Wykorzystane skills/sub-agenci

Nazwa: ghost-execution
Powod uzycia: zmiana dotyka aktywnej sciezki shadow-only oraz granicy shadow/live.
Zakres uzycia: weryfikacja, ze Gatekeeper policy, SSOT i shadow/live boundary nie sa zmieniane.
Wynik: zmiana ograniczona do post-verdict shadow simulation freshness.
Ograniczenia: skill nie rozstrzyga sam Solana stale-quote mechanics.

Nazwa: solana-pumpfun-architect
Powod uzycia: blad dotyczy swiezosci pump.fun legacy buy quote i symulacji Solana.
Zakres uzycia: klasyfikacja `TooMuchSolRequired` jako freshness/stale execution assumption zamiast provider/RPC-only failure.
Wynik: refresh oparty o aktualny AccountStateCore tuz przed simulation.
Ograniczenia: nie naprawia niezaleznych bledow PDA/route materialization.

Nazwa: rust-master
Powod uzycia: zmiana w async runtime hot path i testach Rust.
Zakres uzycia: minimalna implementacja bez globalnego stanu, bez niekontrolowanego retry i bez rozszerzania lock scope.
Wynik: helper lokalny w `TriggerComponent`, test jednostkowy i release build.
Ograniczenia: smoke sample jest krotki, nie zastapi dlugiego R33/R34 runu.

## 3. Opis problemu - 3W2H

What:
R33 shadow BUY simulation mial niedopuszczalnie niski odsetek pelnych sukcesow `shadow_simulated`, ok. 78.5%.

Where:
Aktywna sciezka `TriggerEntryMode::ShadowOnly` w `ghost-launcher/src/components/trigger/component.rs`, dla legacy pump.fun BUY request budowanego przed opozniona symulacja RPC.

Why it matters:
Shadow lifecycle i pozniejsze labelowanie ekonomiczne traca pokrycie, jesli shadow simulation odpada na bledach technicznych. Przy 78.5% sukcesu dataset jest zanieczyszczony brakami technicznymi, a nie tylko rynkowymi outcome.

How observed:
Analiza R33 `...-buys.jsonl` przed poprawka pokazala dominujacy `quote_slippage_error`, szczegolnie `TooMuchSolRequired` / `Custom(6002)`. Dodatkowe logi pokazaly, ze pomiedzy przygotowaniem legacy-buy quote a startem symulacji AccountStateCore otrzymuje kolejne aktualizacje krzywej.

How many / scale:
Przed poprawka, po restarcie kontrolnym: 122 rows, 94 sukcesy = 77.05%, 23 `quote_slippage_error`, 4 `route_materialization_error`, 1 `account_pda_constraint_error`. Wczesniejszy filtr R33: 785 rows, 618 sukcesow = 78.73%.

Evidence:
- `TooMuchSolRequired` wystepowal jako `quote_slippage_error`.
- Przykladowe logi pokazaly stary `token_param`/min tokens z requestu i kolejne `DIAG_SHADOW_BOOTSTRAP_SYNCED_FROM_CANONICAL` przed simulation start.
- Po patchu smoke: 55 fresh rows, 52 sukcesy = 94.55%, 0 `TooMuchSolRequired` w fresh transport rows.

## 4. Przyczyna zrodlowa

Root cause:
Prepared legacy-buy request byl budowany na krzywej z momentu materializacji/handoffu, a nastepnie uzywany po opoznieniu simulation. W tym czasie canonical curve w `AccountStateCore` byla juz czesto nowsza, wiec token amount / `min_tokens_out` w legacy-buy request stawal sie stale.

Mechanizm bledu:
Fallback route/handoff tworzy `LegacyBuy` request z `account_overrides.legacy_buy_curve`. Potem active shadow precheck, runtime delay i kolejka symulacji przesuwaja realny start RPC simulation. Pump program waliduje stary parametr tokenowy wzgledem aktualnego stanu krzywej i odrzuca transakcje jako `TooMuchSolRequired` / `Custom(6002)`.

Miejsce:
`TriggerComponent::dispatch_prepared_buy_shadow_only()` oraz `TriggerEntryMode::ShadowOnly` branch w `dispatch_prepared_buy_with_shadow()`.

Skutek:
Techniczny drop coverage shadow simulation, nie bedacy rynkowym STOP/TARGET/TIMEOUT.

Dowod:
Po late refresh z `AccountStateCore` tuz przed simulation, swiezy smoke spadl z dominujacego `TooMuchSolRequired` do 0 takich bledow w probce 55 rows, a success proxy wzrosl do 94.55%.

Odrzucone hipotezy:
- Provider/RPC endpoint jako glowna przyczyna: brak dowodu; blad byl deterministycznie spojny ze stale quote.
- `trigger.shadow_run.max_concurrent` jako glowny regulator: nie dotyczy aktywnego R33 BUY path w `entry_mode = "shadow_only"`.
- Gatekeeper threshold/policy jako przyczyna: blad wystepowal po decyzji BUY, w warstwie shadow simulation.

## 5. Strategia naprawy

Przyjeta strategia:
Nie zmieniac decyzji, progow ani route policy. Odnowic tylko legacy-buy request tuz przed shadow-only simulation, uzywajac canonical `AccountStateCore` jako swiezego zrodla krzywej.

Zakres ingerencji:
Dodano lokalny helper `refresh_shadow_only_legacy_buy_request_before_simulation()` i wywolano go przed `try_reserve_position_slot()` oraz przed `simulate_buy()` w dwoch shadow-only dispatch paths.

Czego nie zmieniano:
- Gatekeeper policy
- progi
- scoring
- live execution
- send path
- route resolver semantics
- R2/label semantics
- `p37_shadow_probe` sampling

Ryzyka:
- Refresh wymaga dostepnosci payera i canonical curve; przy braku danych helper failuje otwarcie diagnostycznie i zostawia oryginalny request.
- Nie naprawia PDA constraint ani brakow route manifest / BCV2.
- Smoke sample 55 rows jest pozytywny, ale nie jest dlugim burn-inem.

Odrzucone alternatywy:
- Podbijanie concurrency: nie naprawia stale quote i nie steruje aktywna sciezka R33 BUY simulation.
- Zmiana slippage/progow: ukrywalaby blad stale request zamiast go naprawic.
- Rebuild calego route resolvera: zbyt szeroki zakres wzgledem zdiagnozowanej przyczyny.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/modul: `ghost-launcher/src/components/trigger/component.rs`
- Co zmieniono: dodano `refresh_shadow_only_legacy_buy_request_before_simulation()`.
- Dlaczego: legacy-buy request musi byc odswiezony o najnowsza canonical curve tuz przed shadow simulation.
- Efekt: `legacy_buy_curve_source` dla swiezych rows przyjmuje `account_state_core_pre_shadow_simulation_refresh`, a `entry_token_amount_raw`/`min_tokens_out` sa ponownie materializowane.

Zmiana 2:
- Plik/modul: `ghost-launcher/src/components/trigger/component.rs`
- Co zmieniono: helper podlaczono w `dispatch_prepared_buy_shadow_only()` oraz w `TriggerEntryMode::ShadowOnly` branch.
- Dlaczego: oba shadow-only wejscia musza miec ta sama semantyke freshness.
- Efekt: late refresh obejmuje aktywna sciezke R33 bez dotykania live submit path.

Zmiana 3:
- Plik/modul: `ghost-launcher/src/components/trigger/component.rs`
- Co zmieniono: dodano test `dispatch_prepared_buy_shadow_only_refreshes_legacy_curve_before_simulation`.
- Dlaczego: zabezpiecza przypadek stale curve -> fresh curve przed symulacja.
- Efekt: test wymusza, ze shadow report uzywa `fresh_entry_token_amount`, nie stale prepared request.

Zmiana 4:
- Plik/modul: `configs/rollout/shadow-burnin-v3-r33-maxwait15000-fsc-off-r1.toml`
- Co zmieniono: `trigger.shadow_run.max_concurrent` pozostawiono/przywrocono jako `1`; nie zmieniono `p37_shadow_probe.max_concurrent = 16`.
- Dlaczego: ustalono, ze `trigger.shadow_run.max_concurrent` nie jest kontrolka aktywnej R33 shadow-only BUY simulation.
- Efekt: config nie sugeruje falszywej naprawy przez concurrency.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowod |
|---|---|---|---|---|
| Format | `cargo fmt --check` | exit 0 | PASS | brak outputu po finalnym uruchomieniu |
| Unit | `cargo test -p ghost-launcher dispatch_prepared_buy_shadow_only_refreshes_legacy_curve_before_simulation` | 1 passed | PASS | test potwierdzil stale -> fresh curve refresh |
| Build | `cargo build --release -p ghost-launcher` | finished release profile in 5m 51s | PASS | `target/release/ghost-launcher` zbudowany po patchu |
| Replay/simulation | R33 smoke `r33-shadow-refresh-smoke`, `start_ms=1781744352305` | 55 fresh rows, 52 success, 94.55% success proxy | PASS | `...-buys.jsonl`, fresh filter po `decision_ts_ms >= start_ms` |
| Guard negative case | Unit stale curve setup | shadow report `entry_token_amount_raw == fresh_entry_token_amount` | PASS | test nie przejdzie, jesli request zostanie stale |
| Runtime error regression | fresh R33 smoke | 0 `TooMuchSolRequired` / `Custom(6002)` w fresh transport rows | PASS | fresh rows: 2 PDA constraint, 1 route materialization, 0 quote slippage |

Wniosek walidacyjny:
Gowny mechanizm spadku coverage zostal potwierdzony jako stale legacy-buy quote i naprawiony w waskiej sciezce shadow-only. Smoke poprawil success proxy z ok. 77-79% do 94.55% na 55 swiezych rows.

Ograniczenia walidacji:
Smoke jest krotki i nie dowodzi docelowego coverage w wielogodzinnym runie. Pozostale bledy `account_pda_constraint_error` i `route_materialization_error` sa niezalezne od `TooMuchSolRequired` i wymagaja osobnej analizy, jesli beda istotne liczebnie.

## 8. Wdrozone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: unit test
- Co zabezpiecza: legacy-buy shadow-only simulation nie moze uzyc stale curve, jesli AccountStateCore ma swieza canonical curve.
- Kiedy sie aktywuje: przy regresji w `dispatch_prepared_buy_shadow_only()`.
- Jak przetestowano: `cargo test -p ghost-launcher dispatch_prepared_buy_shadow_only_refreshes_legacy_curve_before_simulation`.
- Co pozostaje poza zakresem: route materialization/BCV2 readiness oraz PDA seed mismatch.

Guardrail 2:
- Typ: runtime diagnostic log
- Co zabezpiecza: audytowalnosc odswiezenia requestu.
- Kiedy sie aktywuje: gdy legacy-buy request zostaje odswiezony przed shadow-only simulation.
- Jak przetestowano: R33 smoke wygenerowal 54 logi `Trigger: refreshed shadow-only legacy BUY request...`.
- Co pozostaje poza zakresem: brak canonical curve lub payer mismatch powoduje skip z warningiem i wymaga osobnej obserwacji.

## Otwarte ryzyka / follow-up

- Przeprowadzic dluzszy R33/R34 smoke/run, aby potwierdzic stabilne coverage >90% na probce setek/tysiecy rows.
- Oddzielnie skategoryzowac pozostale `account_pda_constraint_error` / `Custom(2006)`.
- Oddzielnie skategoryzowac `route_materialization_error` typu `primary_route_bcv2_missing`.
- Brain config R33 nadal ma `max_wait_time_ms = 2999` mimo nazwy runa `maxwait15000`; nie zmieniano tego bez osobnej dyspozycji.
