# ADR-8D: R30 Gatekeeper Open Thresholds Restart

Status: Done
Typ: Config rollout / Gatekeeper threshold relaxation / runtime restart
Data: 2026-06-15
Repo/branch: /root/Gho / codex/gatekeeper-edge-policy-redesign-r1
Commit/PR: local working tree
Zakres: R30 continuation with maximally open Gatekeeper V2 thresholds except Phase1 count minima
Dotkniete moduly/pliki:
- configs/rollout/ghost_brain_selector_dataset_sampler_r30_fsc_lookback_1800.toml
Powiazane runy/logi/raporty:
- configs/rollout/shadow-burnin-v3-r30-fsc-lookback-window-canary.toml
- reports/selector/shadow-burnin-v3-r30-fsc-lookback-window-canary/r30_open_gatekeeper_thresholds_fingerprintfix_20260615T001926Z/runtime.log
- backups/r30-before-open-thresholds-20260615T001557Z/
- backups/r30-before-open-thresholds-fingerprintfix-20260615T001911Z/
Poziom ryzyka: High

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
- Zmapowac reason codes z biezacego R30 na konkretne Gatekeeper V2 metryki/progi.
- Otworzyc progi odpowiedzialne za dominujace `HARD_FAIL_EXTREME_TOP3`, `REJECT_CORE_FAIL` oraz timeouty, z wyjatkiem Phase1 count minima.
- Zachowac `min_tx_count = 3`, `min_unique_signers = 2`, `min_buy_count = 2`.
- Zrestartowac R30 na tym samym rollout configu.

Rzeczywisty przebieg:
- Potwierdzono, ze `HARD_FAIL_EXTREME_TOP3` jest sterowany przez `hard_fail_top3_volume_pct`.
- Potwierdzono, ze `TIMEOUT_PHASE1_NO_DATA` i `TIMEOUT_PHASE1_INSUFFICIENT` sa Phase1/timeout verdictami; zakazane minima 3/2/2 zostaly zachowane.
- Potwierdzono, ze `REJECT_CORE_FAIL` w biezacym R30 dominowal jako `core2=false`, a po pierwszym patchu pozostaly Core2 fails byly powodowane przez `max_early_top3_buy_volume_pct_3s = 0.99`.
- Otworzono Gatekeeper V2 Phase2/Phase3/Phase4/Phase5/Phase6/hard-fail/fingerprint thresholds do maksymalnie tolerancyjnych wartosci.
- R30 zatrzymano, artefakty przeniesiono do backupow i uruchomiono ponownie w `tmux` jako `gho-r30`.

Odchylenia od planu:
- Wykonano drugi krotki restart, bo pierwszy patch zostawil nieotwarty Phase4 fingerprint cap `max_early_top3_buy_volume_pct_3s`.

## 2. Wykorzystane skills/sub-agenci

Nazwa: ghost-execution
Powod uzycia: Zmiana dotyka Gatekeeper verdict behavior i shadow execution boundary.
Zakres uzycia: Mapowanie reason codes do aktywnego Gatekeeper V2 policy path oraz zachowanie shadow-only runtime.
Wynik: Zmieniono tylko config progow; nie zmieniano kodu polityki, SSOT ani DecisionLogger schema.
Ograniczenia: Smoke-check po restarcie jest krotki i nie stanowi finalnej walidacji statystycznej.

Nazwa: trading-systems
Powod uzycia: Threshold relaxation zmienia selektywnosc runtime i przeplyw kandydatow do shadow execution.
Zakres uzycia: Zachowanie rozdzialu risk/evidence oraz niepromowanie shadow do live.
Wynik: Runtime nadal dziala w `execution_mode=Shadow`, `entry_mode=shadow_only`.
Ograniczenia: Nie oceniano jakosci ekonomicznej po dlugim oknie.

Nazwa: gatekeeper-policy-auditor
Powod uzycia: Zadanie wymaga wskazania, ktore metryki odpowiadaja za konkretne Gatekeeper verdict/reason codes.
Zakres uzycia: Odczyt aktywnych policy files i mapowanie Core2/Core3/hard-fail/timeout progow.
Wynik: `min_tx_count`, `min_unique_signers`, `min_buy_count` zostaly zachowane; pozostale wskazane progi otwarto.
Ograniczenia: Czesciowy breakdown `REJECT_CORE_FAIL` w legacy logu jest skracany do `core1/core2/core3`; szczegoly Phase4 zweryfikowano przez dodatkowe pola w decision JSON.

## 3. Opis problemu - 3W2H

What:
- R30 generowal dominujace verdict/reason buckets: `HARD_FAIL_EXTREME_TOP3`, `TIMEOUT_PHASE1_NO_DATA`, `TIMEOUT_PHASE1_INSUFFICIENT`, `BUY_EXTENDED`, `REJECT_CORE_FAIL`.

Where:
- Aktywny config brain: `configs/rollout/ghost_brain_selector_dataset_sampler_r30_fsc_lookback_1800.toml`.
- Aktywny rollout launcher config: `configs/rollout/shadow-burnin-v3-r30-fsc-lookback-window-canary.toml`.

Why it matters:
- Run R30 ma zbierac szeroki dataset i counterfactual evidence; zbyt selektywne progi Gatekeepera obnizaja population coverage.

How observed:
- Biezace decision logs przed zmiana wskazywaly m.in. `HARD_FAIL_EXTREME_TOP3: 413`, `TIMEOUT_PHASE1_NO_DATA: 410`, `TIMEOUT_PHASE1_INSUFFICIENT: 337`, `BUY_EXTENDED: 227`, `REJECT_CORE_FAIL: 124`.
- Lokalna analiza logow pokazala, ze `REJECT_CORE_FAIL` dominowal jako `CORE_FAIL: core1=true core2=false core3=true`.

How many / scale:
- Zmieniono jeden aktywny config brain R30.
- Restart zachowal run name/scope `shadow-burnin-v3-r30-fsc-lookback-window-canary`.

Evidence:
- Preflight PASS po patchu: `2026-06-15T00:19:16Z`.
- Finalny restart report dir: `reports/selector/shadow-burnin-v3-r30-fsc-lookback-window-canary/r30_open_gatekeeper_thresholds_fingerprintfix_20260615T001926Z/`.

## 4. Przyczyna zrodlowa

Root cause:
- R30 config nadal zawieral nietolerancyjne lub czesciowo nietolerancyjne progi Gatekeeper V2 mimo dataset/counterfactual celu.

Mechanizm bledu:
- `hard_fail_top3_volume_pct = 0.99` powodowal `HARD_FAIL_EXTREME_TOP3`.
- Core2/Phase4 fail wynikal z volume/capital/fingerprint gates.
- Po pierwszym patchu pozostaly Core2 fail wynikal z `early_top3_buy_volume_pct_3s=1.0` przy `max_early_top3_buy_volume_pct_3s=0.99`.
- `TIMEOUT_PHASE1_*` jest powiazany z Phase1 i observation timeout; minima 3/2/2 nie zostaly zmienione zgodnie z instrukcja.

Miejsce:
- `[gatekeeper_v2]` w `ghost_brain_selector_dataset_sampler_r30_fsc_lookback_1800.toml`.

Skutek:
- Czesci kandydatow nie dopuszczano do BUY/shadow path mimo celu szerokiego dataset capture.

Dowod:
- `gatekeeper_policy.rs` mapuje `diversity.top3_volume_pct > config.hard_fail_top3_volume_pct` do `HardFailExtremeTop3`.
- `volume_phase_passes_base` i fingerprint thresholds steruja Core2/Phase4.
- Smoke po pierwszym patchu pokazal `phase4_passed=false` przy `early_top3_buy_volume_pct_3s=1.0` i `max_early_top3_buy_volume_pct_3s=0.99`.

Odrzucone hipotezy:
- Nie luzowano `min_tx_count`, `min_unique_signers`, `min_buy_count`.
- Nie zmieniano kodu Gatekeepera.
- Nie zmieniano V3 profile minima 4/3/2, bo analizowane reasony pochodzily z aktywnego Gatekeeper V2/legacy_live path R30.

## 5. Strategia naprawy

Przyjeta strategia:
- Otworzyc wszystkie istotne V2 hard/core/fingerprint thresholds, ktore moga powodowac wskazane buckets.
- Zachowac Phase1 count minima 3/2/2.
- Zachowac runtime jako shadow-only.
- Zachowac poprzednie artefakty przez backup zamiast kasowania.

Zakres ingerencji:
- Config-only threshold relaxation.
- Runtime restart.

Czego nie zmieniano:
- Rust code.
- `min_tx_count = 3`.
- `min_unique_signers = 2`.
- `min_buy_count = 2`.
- Execution mode / entry mode.
- RPC routing.
- IWIM disabled state.

Ryzyka:
- Maksymalnie otwarte progi obnizaja selektywnosc i moga zwiekszyc liczbe shadow/probe attempts oraz obciazenie RPC.
- Economics po takim runie nie powinno byc interpretowane jako selector quality.
- FSC warmup zaczyna sie od nowa po restarcie i backupie artefaktow.

Odrzucone alternatywy:
- Zmiana kodu polityki.
- Luzowanie Phase1 count minima.
- Restart bez backupu artefaktow.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/modul: `configs/rollout/ghost_brain_selector_dataset_sampler_r30_fsc_lookback_1800.toml`
- Co zmieniono: Otwarto Phase2/Phase3/Phase4/Phase5/Phase6 thresholds: m.in. `max_avg_interval_ms`, `max_hhi`, `max_top3_volume_pct`, `min_buy_ratio`, `min_total_volume_sol`, `max_dev_*`, `min_market_cap_sol`.
- Dlaczego: Ograniczaly Core2/Core3 i inne core/hard buckets.
- Efekt: Config przestal odcinac te metryki na dotychczasowych wartosciach.

Zmiana 2:
- Plik/modul: `configs/rollout/ghost_brain_selector_dataset_sampler_r30_fsc_lookback_1800.toml`
- Co zmieniono: `hard_fail_hhi`, `hard_fail_same_ms_tx_ratio`, `hard_fail_top3_volume_pct` ustawiono na `9999.0`.
- Dlaczego: `HARD_FAIL_EXTREME_TOP3` mapuje sie na `hard_fail_top3_volume_pct`.
- Efekt: Hard-fail top3 dominance zostal praktycznie wylaczony w R30 dataset run.

Zmiana 3:
- Plik/modul: `configs/rollout/ghost_brain_selector_dataset_sampler_r30_fsc_lookback_1800.toml`
- Co zmieniono: `max_early_top3_buy_volume_pct_3s = 9999.0`.
- Dlaczego: Po pierwszym patchu pozostaly Core2 fail byl spowodowany `early_top3_buy_volume_pct_3s=1.0 > 0.99`.
- Efekt: Phase4 fingerprint cap zostal otwarty.

Zmiana 4:
- Plik/modul: runtime/tmux
- Co zmieniono: Zatrzymano poprzedni R30, przeniesiono artefakty do backupow, uruchomiono R30 ponownie jako `gho-r30`.
- Dlaczego: Runtime laduje config przy starcie; restart byl konieczny.
- Efekt: Finalny aktywny report dir: `r30_open_gatekeeper_thresholds_fingerprintfix_20260615T001926Z`.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowod |
|---|---|---|---|---|
| Preflight | `target/release/ghost-launcher --config configs/rollout/shadow-burnin-v3-r30-fsc-lookback-window-canary.toml --preflight` | all runtime checks passed | PASS | `2026-06-15T00:19:16Z` |
| Phase1 minima preserved | preflight + config grep | min_tx=3 min_unique=2 min_buy=2 | PASS | preflight output |
| Runtime restart | `tmux new-session -d -s gho-r30 ...` | PID active | PASS | `gho-r30`, PID 117139 |
| Smoke RPC | runtime grep | 429=0, shadow transport errors=0 | PASS | smoke at `2026-06-15T00:20:30Z` |
| Fresh decisions | decision JSON parse | initial sample only timeout rows | PROVISIONAL | very small post-restart sample |

Wniosek walidacyjny:
- Config laduje sie poprawnie, run dziala w `tmux`, a wskazane progi zostaly otwarte z zachowaniem 3/2/2.

Ograniczenia walidacji:
- Krotki smoke-check po finalnym restarcie nie daje jeszcze stabilnego rozkladu verdictow.
- FSC coverage window startuje od nowa po restarcie.
- Dalsza ocena wymaga dluzszego snapshotu po zebraniu decyzji.

## 8. Wdrozone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: Explicit exclusion
- Co zabezpiecza: Phase1 count minima nie zostaly poluzowane.
- Kiedy sie aktywuje: Przy config review/preflight.
- Jak przetestowano: Preflight potwierdzil `min_tx=3 min_unique=2 min_buy=2`.
- Co pozostaje poza zakresem: `TIMEOUT_PHASE1_*` nadal moze wystepowac, gdy dane nie dojda lub nie spelnia 3/2/2.

Guardrail 2:
- Typ: Artifact preservation
- Co zabezpiecza: Poprzednie statystyki R30 nie mieszaja sie z nowym restartem.
- Kiedy sie aktywuje: Przed restartem.
- Jak przetestowano: Artefakty przeniesiono do backupow.
- Co pozostaje poza zakresem: Interpretacja dlugookresowa wymaga nowego raportu po runie.

## Otwarte ryzyka / follow-up

- Po minimum kilkunastu minutach policzyc ponownie verdict distribution, active shadow coverage, probe coverage i FSC status.
- Zweryfikowac, czy `REJECT_CORE_FAIL` faktycznie spada po otwarciu `max_early_top3_buy_volume_pct_3s`.
- Monitorowac RPC 429, bo bardziej otwarte progi moga zwiekszyc liczbe shadow attempts.
