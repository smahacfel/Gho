# ADR-8D: R37 shadow-probe route identity coverage repair

Status: IMPLEMENTED / TARGETED_TESTS_PASS / RUNTIME_REPROOF_PENDING
Typ: ADR-8D / shadow-probe route-account materialization / regression repair
Data: 2026-06-18
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `main`
Commit/PR: not committed at report time
Zakres: P37 counterfactual shadow-probe route identity, creator-vault precheck, audit reporting
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `ghost-launcher/src/oracle_runtime.rs`
- `scripts/v3_p37_mfs_lifecycle_join_key_audit.py`
- `scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py`
- `docs/ADR/ADR_8D_R37_SHADOW_PROBE_CREATOR_VAULT_CACHE_REUSE_20260618.md`

Powiazane runy/logi/raporty:
- R37 config:
  `configs/rollout/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1.toml`
- R37 runtime log:
  `reports/selector/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260618T160144Z/runtime.log`
- R37 probe selection/skips/transport/lifecycle:
  `logs/shadow_run/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/`
- Local audit outputs:
  `/tmp/r37_p37_audit_now.json`
  `/tmp/r37_p37_audit_now.md`

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Dokument zachowuje lokalny format ADR-8D uzywany w repo i sekcje wymagane przez uzytkownika.

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Ustalic, dlaczego R37 shadow-probe coverage jest ponizej wymaganego poziomu 92% oraz dlaczego `probe_skipped` stanowi dominujaca czesc decyzji.

Rzeczywisty przebieg:
- Sprawdzono aktywny R37 runtime i artefakty `probe_*`.
- Uruchomiono `scripts/v3_p37_mfs_lifecycle_join_key_audit.py` na aktualnych artefaktach R37.
- Rozbito probe path na selected/skipped/transport/entry/lifecycle.
- Potwierdzono, ze formalne coverage po odjeciu inflight moze byc powyzej 90%, ale guard nadal slusznie failuje przez `Custom(2006)` na `creator_vault`.
- Sprawdzono runtime log dla konkretnych pooli i potwierdzono, ze complete legacy manifest bywa cache'owany przed probe decision, ale P37 probe path go nie odzyskiwal.
- Wprowadzono minimalny patch w P37 shadow-probe route materialization.
- Na biezacym R37 po restarcie `20260618T160144Z` policzono: 3812 decyzji, 629 probe transports/entries, 3178 probe skips, 555 pelnych symulacji, 74 `Custom(2006)` i 5 `Custom(6002)`.
- Potwierdzono, ze `probe_skipped` wynika glownie z braku autorytatywnej route identity: 2406 `creator_vault_source_not_authoritative`, 752 `missing_execution_route_identity`, 20 `no_executable_route_account_set`.
- Dodano jawny top-level skip bucket `route_identity_unavailable` dla nowych artefaktow; szczegolowe powody pozostaja w `precheck_failure_reason` i authority counters.

Odchylenia od planu:
Diagnoza wykazala realny blad po naszej stronie, wiec poza raportem wykonano waska naprawe. Nie restartowano R37; aktywny proces nadal pracuje na starej binarce do czasu osobnego rebuild/restartu.

## 2. Wykorzystane skills/sub-agenci

Nazwa:
`ghost-execution`

Powod uzycia:
Zadanie dotyka shadow-only counterfactual probe, DecisionLogger/probe artifacts, fail-closed precheckow i rozdzielenia shadow/live.

Zakres uzycia:
Ochrona Gatekeeper policy, BUY path, execution i send path przed niezamierzona zmiana.

Wynik:
Zmiana zostala ograniczona do P37 counterfactual shadow-probe account override materialization i prechecku creator-vault.

Ograniczenia:
Skill nie zastapil runtime reproofu po rebuildzie. Nowe coverage musi byc potwierdzone na swiezych artefaktach.

Nazwa:
`solana-pumpfun-architect`

Powod uzycia:
Problem dotyczy pump.fun legacy route accounts, creator-vault seed constraint i bezpiecznej symulacji.

Zakres uzycia:
Rozroznienie kompletnego observed legacy manifestu od nieautorytatywnego fallbacku `detected_pool.creator`.

Wynik:
Authoritative `creator_vault` z complete observed legacy manifestu jest teraz wystarczajacy do P37 prechecku. Nieautorytatywny creator bez takiego manifestu nadal fail-closed.

Ograniczenia:
Nie zmieniano DirectBuyBuilder ani samego RPC simulation path.

Nazwa:
`rust-master`

Powod uzycia:
Patch dotyka aktywnego Rust runtime i musi nie wprowadzac hot-path blokad ani nowych async/race problemow.

Zakres uzycia:
Waski helper, brak nowych kolejek, brak blokujacego I/O, brak zmian ownership boundary.

Wynik:
Reuse cache korzysta z istniejacego bounded manifest cache i cutoff decision timestamp.

Ograniczenia:
Nie uruchomiono pelnego test suite repo; wykonano targeted tests.

## 3. Opis problemu - 3W2H

What:
P37 shadow-probe masowo oznaczal rows jako `probe_skipped`, glownie z powodu braku autorytatywnej route identity. Starszy top-level bucket `creator_vault_source_not_authoritative` mylil role-specific detail z ogolna klasa problemu. Czesc rows przechodzila do transportu i konczyla jako `Custom(2006)` creator-vault account layout mismatch.

Where:
`ghost-launcher/src/oracle_runtime.rs`, funkcje:
- `p37_shadow_probe_derive_account_override_context_for_pool_with_mode`
- `p37_shadow_probe_creator_vault_precheck_failure`
- `maybe_handle_p37_shadow_probe_decision`
- `run_p37_shadow_probe_dispatch`

Why it matters:
Counterfactual probe ma symulowac REJECT/TIMEOUT tylko wtedy, gdy ma decision-time-safe executable route. Jezeli route evidence jest w runtime, ale P37 go nie odzyskuje, tracimy coverage bez powodu. Jezeli route evidence nie jest pewne, probe ma fail-closed jako skip, a nie zanieczyszczac transport/simulation failures.

How observed:
Audit `/tmp/r37_p37_audit_now.json` pokazal:
- `selected_simulation_coverage_excluding_inflight = 0.938144`
- `simulation_coverage_guard_status = fail`
- `simulation_coverage_guard_reasons = ["custom_2006_creator_vault_source_not_authoritative_gt_0"]`
- `custom_2006_creator_vault_source_not_authoritative_rows = 116`
- `skip_reason_counts = {"creator_vault_source_not_authoritative": 2404, "no_executable_route_account_set": 33, "probe_execution_precheck_failed": 830}`

Biezace liczenie dla aktywnej sesji R37 `>=1781798508000` pokazalo:
- decyzje: 3812
- probe entries/transports: 629
- probe skips: 3178
- skip rate vs decisions: 83.37%
- transport simulated OK: 555/629 = 88.24%
- transport errors: 74 `Custom(2006)` + 5 `Custom(6002)`

How many / scale:
W biezacym R37 audit ponad dwa tysiace probe rows bylo blokowanych jako brak autorytatywnej creator-vault identity, a kilkadziesiat prob trafilo do simulation error `Custom(2006)`. Po restarcie R37 stale widoczne byly 2406 skips `creator_vault_source_not_authoritative` i 74 transport errors `Custom(2006)`.

Evidence:
Runtime log dla przykladowych skipped pooli zawieral `ACTIVE_BUY_ROUTE_MANIFEST_CACHE_STORE` przed probe decision, a mimo to P37 probe zapisywal skip z fallbackiem `detected_pool.creator`.

## 4. Przyczyna zrodlowa

Root cause:
P37 counterfactual probe nie reuzywal istniejacego `active_buy_route_manifest_cache`, ktory aktywna BUY sciezka juz wykorzystywala do complete observed legacy manifests.

Mechanizm bledu:
P37 budowal account overrides tylko z lokalnych `buffered_txs` i fallbackowal do `detected_pool.creator`. Dla legacy buy taki creator nie jest autorytatywnym creator-vault evidence, wiec precheck slusznie fail-closed. Problem polegal na tym, ze kompletne observed legacy manifest evidence moglo juz istniec w cache, ale P37 path go nie sprawdzal. Drugi problem byl diagnostyczny: audit i top-level skip reason raportowaly role-specific `creator_vault_source_not_authoritative` zamiast jawnego `route_identity_unavailable`.

Miejsce:
`p37_shadow_probe_derive_account_override_context_for_pool_with_mode`.

Skutek:
- Legitne probe candidates byly odrzucane jako `creator_vault_source_not_authoritative`.
- Czesc niepoprawnych account sets nadal docierala do RPC simulation i konczyla `Custom(2006)`.
- Coverage operatora wygladalo jak provider/RPC problem, chociaz glowny blad byl w runtime materialization.
- Skip breakdown mieszal root cause z rola konta, przez co `probe_skipped` nie pokazywal wprost, ze problemem jest brak autorytatywnej route identity.

Dowod:
Kod aktywnej BUY sciezki zawiera `lookup_active_buy_route_manifest` i `try_reuse_active_buy_route_manifest_cache`; P37 derivation przed poprawka nie wywolywala tego lookupu. Runtime log potwierdzal `ACTIVE_BUY_ROUTE_MANIFEST_CACHE_STORE` dla pooli, ktore pozniej zostaly skipped jako non-authoritative creator.

Odrzucone hipotezy:
- Provider/RPC jako glowna przyczyna: odrzucone dla dominujacego bucketu, bo skip wystepowal przed transportem.
- Naturalny brak market data: czesciowo prawdziwe dla `no_executable_route_account_set`, ale nie tlumaczy cache-miss przy istniejacym manifest store.
- Gatekeeper thresholds: odrzucone; problem dotyczy probe account materialization po verdict, nie samego verdictu.

## 5. Strategia naprawy

Przyjeta strategia:
W P37 probe path dodac decision-time bounded lookup do `active_buy_route_manifest_cache` przed fallbackiem do `detected_pool.creator`, a precheck creator-vault uznaje authoritative `creator_vault` za wystarczajace evidence dla legacy route.

Zakres ingerencji:
- Tylko P37 shadow-probe materialization/precheck.
- Tylko reuse juz istniejacego complete observed legacy manifestu dla tego samego `pool_id` i `base_mint`.
- Lookup ograniczony `decision_ts_ms` i istniejacym TTL cache.

Czego nie zmieniano:
- Gatekeeper policy.
- BUY/REJECT/TIMEOUT verdicts.
- Strict threshold logic.
- DirectBuyBuilder.
- RPC simulation implementation.
- Execution/send path.
- Runtime active BUY decision policy.

Ryzyka:
- Runtime reproof wymaga rebuild/restartu; biezacy R37 nadal dziala na starej binarce.
- Dla pooli bez prior complete observed legacy manifestu skip pozostanie wysoki; to jest oczekiwane fail-closed zachowanie, nie coverage success.

Odrzucone alternatywy:
- Symulowac mimo non-authoritative creator: odrzucone, generuje `Custom(2006)` i falszuje coverage.
- Obnizyc coverage threshold: odrzucone, maskuje blad.
- Globalnie zmienic route manifest cache semantics aktywnej BUY sciezki: odrzucone, za duzy blast radius.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/modul: `ghost-launcher/src/oracle_runtime.rs`
- Co zmieniono: dodano `p37_shadow_probe_reuse_active_buy_route_manifest_cache`.
- Dlaczego: P37 probe powinien korzystac z tego samego decision-time-safe complete legacy manifest cache, ktory istnieje dla aktywnej BUY sciezki.
- Efekt: gdy runtime widzial complete observed legacy manifest przed cutoffem, P37 moze odzyskac authoritative `creator_vault`.

Zmiana 2:
- Plik/modul: `ghost-launcher/src/oracle_runtime.rs`
- Co zmieniono: dodano `p37_shadow_probe_manifest_lookup_current`, ktory usuwa telemetry-only pola przed lookupiem cache.
- Dlaczego: NLN telemetry nie jest authoritative route evidence, ale nie powinno blokowac lookupu complete observed manifestu dla tego samego pool/base.
- Efekt: telemetry-only input nie jest traktowany jako konflikt z realnym observed legacy manifestem.

Zmiana 3:
- Plik/modul: `ghost-launcher/src/oracle_runtime.rs`
- Co zmieniono: `p37_shadow_probe_creator_vault_precheck_failure` akceptuje `creator_vault_authoritative == true` i obecny `creator_vault`.
- Dlaczego: legacy route wymaga authoritative creator-vault; nie musi posiadac autorytatywnego `creator_pubkey`, jezeli creator-vault jest juz autorytatywny z manifestu.
- Efekt: complete observed legacy manifest nie jest mylnie odrzucany przez precheck creator identity.

Zmiana 4:
- Plik/modul: `ghost-launcher/src/oracle_runtime.rs`
- Co zmieniono: dodano test `p37_probe_reuses_cached_legacy_manifest_before_detected_creator_fallback`.
- Dlaczego: zabezpiecza regresje, w ktorej P37 ignoruje cache i wraca do non-authoritative `detected_pool.creator`.
- Efekt: test potwierdza authoritative `creator_vault` z cache, brak creator-vault precheck failure i brak route contract failure.

Zmiana 5:
- Plik/modul: `ghost-launcher/src/oracle_runtime.rs`
- Co zmieniono: dodano `p37_shadow_probe_reason_is_route_identity_unavailable` i mapowanie top-level `probe_skip_reason` na `route_identity_unavailable` dla source-not-authoritative/missing route identity.
- Dlaczego: `creator_vault_source_not_authoritative` jest szczegolem dowodowym, a nie najlepsza nazwa glownej klasy skipu.
- Efekt: nowe probe skips beda raportowac jawny brak route identity, zachowujac szczegoly w `precheck_failure_reason`, `creator_vault_authority_status` i readiness fields.

Zmiana 6:
- Plik/modul: `scripts/v3_p37_mfs_lifecycle_join_key_audit.py`, `scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py`
- Co zmieniono: audit rozpoznaje `route_identity_unavailable`, klasyfikuje stare i nowe artefakty jako `not_executable_route_identity`, a test wymusza liczniki reason/status.
- Dlaczego: regresja coverage musi byc widoczna jako route identity/materialization problem, nie jako ogolny precheck failure.
- Efekt: raporty rozdzielaja realne transport/simulation failures od fail-closed route identity skips.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowod |
|---|---|---|---|---|
| Rust targeted | `cargo test -p ghost-launcher p37_probe_reuses_cached_legacy_manifest_before_detected_creator_fallback -- --nocapture` | 1 test OK | PASS | command exit 0 |
| Rust regression | `cargo test -p ghost-launcher p37_shadow_probe_creator_vault_precheck -- --nocapture` | 2 tests OK | PASS | command exit 0 |
| Rust route identity | `cargo test -p ghost-launcher p37_shadow_probe_precheck_skip_marks_route_identity_unavailable -- --nocapture` | 1 test OK | PASS | command exit 0 |
| Rust creator-vault skip | `cargo test -p ghost-launcher p37_shadow_probe_creator_vault_skip_records_specific_reason -- --nocapture` | 1 test OK | PASS | command exit 0 |
| Python compile | `python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py` | no compile errors | PASS | command exit 0 |
| Python unittest | `python3 -m unittest scripts.test_v3_p37_mfs_lifecycle_join_key_audit -v` | 41 tests OK | PASS | command exit 0 |
| Diff check | `git diff --check -- ghost-launcher/src/oracle_runtime.rs scripts/v3_p37_mfs_lifecycle_join_key_audit.py scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py docs/ADR/ADR_8D_R37_SHADOW_PROBE_CREATOR_VAULT_CACHE_REUSE_20260618.md` | no whitespace errors | PASS | command exit 0 |
| Runtime reproof | fresh R37/R38 after rebuild/restart | not run | PENDING | active R37 still old binary |

Wniosek walidacyjny:
Kodowo potwierdzono, ze P37 probe potrafi teraz odzyskac cached authoritative legacy creator-vault, nadal odrzuca non-authoritative legacy creator source i raportuje brak route identity jawnie jako `route_identity_unavailable`.

Ograniczenia walidacji:
Biezace R37 artefakty nadal pochodza ze starej binarki. Spadek `probe_skipped` i zanik `Custom(2006)` trzeba potwierdzic dopiero po rebuildzie i restarcie runu.

## 8. Wdrozone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: Rust unit test
- Co zabezpiecza: P37 probe reuse complete observed legacy manifest cache before detected creator fallback.
- Kiedy sie aktywuje: przy zmianach w P37 account override derivation lub active buy route manifest cache.
- Jak przetestowano: `cargo test -p ghost-launcher p37_probe_reuses_cached_legacy_manifest_before_detected_creator_fallback -- --nocapture`.
- Co pozostaje poza zakresem: real provider/RPC coverage after restart.

Guardrail 2:
- Typ: existing Rust unit tests
- Co zabezpiecza: non-authoritative legacy creator remains fail-closed, authoritative routed creator remains accepted.
- Kiedy sie aktywuje: przy zmianach w creator-vault precheck semantics.
- Jak przetestowano: `cargo test -p ghost-launcher p37_shadow_probe_creator_vault_precheck -- --nocapture`.
- Co pozostaje poza zakresem: runtime distribution of skip reasons.

Guardrail 3:
- Typ: Rust unit test + Python audit unittest
- Co zabezpiecza: route identity skips sa jawnie raportowane jako `route_identity_unavailable`, a `creator_vault_source_not_authoritative` pozostaje szczegolem dowodowym.
- Kiedy sie aktywuje: przy zmianach w P37 skip mapping lub audit script.
- Jak przetestowano: `cargo test -p ghost-launcher p37_shadow_probe_precheck_skip_marks_route_identity_unavailable -- --nocapture`; `python3 -m unittest scripts.test_v3_p37_mfs_lifecycle_join_key_audit -v`.
- Co pozostaje poza zakresem: runtime reproof po rebuildzie.

## Otwarte ryzyka / follow-up

- Rebuild i restart R37/R38 sa wymagane, zeby potwierdzic nowe coverage na swiezych artefaktach.
- Po reproofie oczekiwane metryki: top-level skip bucket `route_identity_unavailable` zamiast `creator_vault_source_not_authoritative`, spadek route identity skips tam, gdzie istnieje prior complete legacy manifest, oraz `custom_2006_creator_vault_source_not_authoritative_rows = 0`.
- Czesciowy skip bucket `no_executable_route_account_set` pozostanie poprawny dla pooli bez decision-time-safe executable route evidence.
- Jezeli po restarcie nadal pojawi sie `Custom(2006)`, trzeba sprawdzic czy observed manifest cache zawiera bledny creator-vault czy czy tail/creator-vault identity jest nadpisywana po cache-hit.
