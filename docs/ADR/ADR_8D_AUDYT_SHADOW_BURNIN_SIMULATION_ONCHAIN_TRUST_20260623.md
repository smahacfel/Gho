# ADR-8D: Audyt Shadow Burn-in Simulation On-chain Trust

Status: IMPLEMENTED / AUDIT_REPORT_CREATED
Typ: ADR-8D / audit report / shadow lifecycle evidence trust
Data: 2026-06-23
Autor/Agent: Codex
Repo/branch: `/root/Gho`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: ocena wiarygodnosci aktualnej shadow burn-in simulation dla metric-segment discovery i selector edge analysis
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `RAPORTY/AUDYT_SHADOW_BURNIN_SIMULATION_ONCHAIN_TRUST_20260623.md`
- `docs/ADR/ADR_8D_AUDYT_SHADOW_BURNIN_SIMULATION_ONCHAIN_TRUST_20260623.md`

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w ostatnich raportach.

## 1. Przygotowanie i dzialania wstepne

Cel:
Udokumentowac wynik audytu, czy aktualna shadow burn-in simulation jest wystarczajaco bliska on-chain reality, aby uzywac jej do metric-segment discovery i selector edge analysis.

Zalozenia:
- Zadanie bylo audytem i utworzeniem raportu, nie implementacja runtime.
- Shadow/live separation pozostaje bez zmian.
- Gatekeeper policy, `MaterializedFeatureSet`, TX builder, sender, retry i live execution nie byly modyfikowane.
- R45/R46 artefakty sa oceniane jako material dowodowy o roznej klasie zaufania, a nie jako automatyczny dowod produkcyjnego edge.

## 2. Opis problemu - 3W2H

What:
Potrzebny byl metodyczny raport, czy shadow simulation mozna z zaufaniem wykorzystac do odkrywania segmentow metryk i analiz selector edge oraz jak blisko ta symulacja jest realnego on-chain zachowania.

Where:
- `ghost-launcher/src/components/trigger/shadow_run.rs`
- `ghost-launcher/src/components/trigger/component.rs`
- `ghost-brain/src/guardian/post_buy/engine.rs`
- `scripts/check_selector_lifecycle_canary.py`
- `scripts/shadow_onchain_lifecycle_report.py`
- `scripts/shadow_run_report.py`
- `reports/selector/` R45/R46
- `RAPORTY/`

Why:
Bez jawnej klasy zaufania latwo pomylic RPC shadow simulation, canonical observed account-state truth, p37 counterfactual probe i realny live inclusion. To mogloby prowadzic do falszywego selector edge albo do promocji hipotez bez dowodu wykonalnosci.

How:
- Przeczytano repo instructions, skille i specjalistyczne docs dla Decision Logging, Solana Execution, Gatekeeper, Seer, Config Rollout i Ghost Runtime.
- Zmapowano sciezke shadow entry simulation, shadow dispatch lifecycle i post-buy canonical truth.
- Zweryfikowano R45/R46 launcher/canary/report semantics.
- Uruchomiono istniejace testy skryptow i wybrane targetowane testy.
- Uruchomiono lokalne reportery on-chain lifecycle dla R45/R46.
- Spisano werdykt, findings i minimalny dataset contract w raporcie.

How many:
Zmiana dodaje tylko dokumenty: jeden raport w `RAPORTY/` i jeden ADR-8D. Nie zmienia kodu runtime ani configow.

## 3. Przyczyna zrodlowa

Root cause:
Shadow burn-in dane maja kilka warstw semantycznych:

- RPC transaction simulation dla entry,
- shadow dispatch/lifecycle telemetry,
- canonical account-state truth dla post-buy labels,
- p37 counterfactual probe plane,
- launcher/canary proof gates.

Bez formalnego rozroznienia tych warstw ten sam rekord moze byc blednie odczytany jako dowod live inclusion, strict lifecycle proof albo tylko exploratory telemetry.

## 4. Strategia naprawy

Przyjeta strategia:
- Nie modyfikowac runtime.
- Zamiast tego opisac evidence classes A/B/C/D/E.
- Oddzielic "discovery allowed" od "promotion readiness".
- Oddzielic active shadow lifecycle od p37 counterfactual probe.
- Traktowac `bad_json`, missing terminal lifecycle, unresolved truth i duze truth-gap/drift jako bramki zaufania.
- Zapisac rekomendacje w raporcie, bez zmiany polityki Gatekeepera ani selector logic.

## 5. Przeprowadzone akcje naprawcze

Zmiana 1: raport audytowy
- Dodano `RAPORTY/AUDYT_SHADOW_BURNIN_SIMULATION_ONCHAIN_TRUST_20260623.md`.
- Raport zawiera werdykt, scope, klasy zaufania, ocene R45/R46, findings F1-F7, dataset contract i workflow dla discovery.

Zmiana 2: ADR-8D
- Dodano `docs/ADR/ADR_8D_AUDYT_SHADOW_BURNIN_SIMULATION_ONCHAIN_TRUST_20260623.md`.
- ADR dokumentuje przyczyne, zakres i konsekwencje samego raportu.

## 6. Walidacja

Walidacja wykonana:

- `python3 -m py_compile scripts/shadow_onchain_lifecycle_report.py scripts/shadow_run_report.py scripts/check_selector_lifecycle_canary.py scripts/start_selector_lifecycle_run.py` - PASS.
- `python3 -m unittest scripts/test_shadow_onchain_lifecycle_report_contract.py scripts/test_shadow_run_report.py -v` - PASS, 13 tests.
- `CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher p5_shadow_dispatch_lifecycle_writes_closed_with_idempotency_join_key_rollout_profile -- --nocapture` - PASS, 1 test.
- `CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-brain shadow_runtime_time_stop_uses_currently_observed_canonical_state_for_quiet_pool -- --nocapture` - nie zakonczyl sie; kompilacja zatrzymala sie na `No space left on device` przed wykonaniem testu.

Raportowe walidacje:

- `shadow_onchain_lifecycle_report.py` dla R45: `rows_written=14`, close truth coverage `14/14`, exit drift p95 abs `0.000082`, entry drift p95 abs `22.245354`.
- `shadow_onchain_lifecycle_report.py` dla R46: `rows_written=2188`, close truth coverage `2188/2196`, exit drift p95 abs `0.000049`, entry drift p95 abs `46.974389`.
- `shadow_run_report.py` dla R45: NO-GO mimo `runtime_lifecycle_complete=true`, przez brak mandatory artifacts/recovery/economics.
- `shadow_run_report.py` dla R46: NO-GO, m.in. `bad_lifecycle_rows=12`, lifecycle gaps i trace-correlation gaps.

Walidacja dokumentow do wykonania po zapisie:

- `git diff --check -- RAPORTY/AUDYT_SHADOW_BURNIN_SIMULATION_ONCHAIN_TRUST_20260623.md docs/ADR/ADR_8D_AUDYT_SHADOW_BURNIN_SIMULATION_ONCHAIN_TRUST_20260623.md`

## 7. Ryzyka i zabezpieczenia

Ryzyko 1: raport zostanie odczytany jako zgoda na promocje selector edge.
- Zabezpieczenie: werdykt rozdziela warunkowy GO dla discovery od NO-GO dla bezposredniej promocji.

Ryzyko 2: shadow simulation zostanie pomylona z live inclusion.
- Zabezpieczenie: raport jawnie wskazuje, ze RPC simulation nie dowodzi submitu, landing, fee competition, account locks, confirmation ani reconciliation.

Ryzyko 3: R46 zostanie potraktowany jako clean strict dataset.
- Zabezpieczenie: raport klasyfikuje R46 jako exploratory/quarantine ze wzgledu na brak launcher proof, bad JSON rows i `shadow_run_report` NO-GO.

Ryzyko 4: p37 counterfactual probe zostanie zmieszany z active lifecycle.
- Zabezpieczenie: raport wymaga `artifact_plane` / `probe_plane` i oddzielnej analizy.

Ryzyko 5: brak lokalnego szablonu ADR.
- Zabezpieczenie: zachowano lokalny format ADR-8D obecny w repo i jawnie odnotowano brak `docs/ADR/ADR_8D_SZABLON.md`.

## 8. Decyzja

Audyt zostaje zamkniety raportem. Aktualna shadow burn-in simulation jest dopuszczalna do kontrolowanego metric-segment discovery i selector edge hypothesis generation, ale nie jest wystarczajaca jako samodzielny dowod live execution reality ani jako podstawa do bezposredniej promocji selector edge. Przed promotion readiness wymagany jest nowy clean formal lifecycle proof run, `bad_json=0`, jawna separacja active/probe planes, truth-gap/drift labeling, temporal split validation i anti-leakage validation.
