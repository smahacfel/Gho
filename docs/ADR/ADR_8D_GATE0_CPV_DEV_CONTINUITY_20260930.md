# ADR-8D: Gate 0 — ciągłość CPV, tożsamość twórcy i poprawna jakość DBIA

Status: LOCAL VALIDATION COMPLETE / LIVE LIFECYCLE NOT READY — brak Phase V
Data: 2026-09-30
Repo/branch: /root/gho_gate0_20260927 / agent/gate0-600s-20260927
HEAD: 38acb83f492dc55b9244f0461c2d6b9e513771e4
Git delivery: wyłącznie lokalnie; bez commit/push/PR/merge.

Uwaga o formacie: wskazany `/Gho/docs/ADR/ADR_8D_SZABLON.md` nie istnieje w dostępnym systemie; nie znaleziono go również w `/root/Gho` ani bieżącym repo. Użyto ośmiu sekcji istniejącego `ADR_8D_R37_CPV_OTHER_POOL_ACTIVITY_THRESHOLD_20260618.md`. Nie deklarujemy zgodności z niedostępnym szablonem.

## 1. Przygotowanie i działania wstępne

Przeczytano gate0.md, AGENTS.md, .agent/STATE.md i aktualne źródła. Zachowano zastane zmiany. Nowsze od gate0.md poprawki i testy znajdowały się już w drzewie; nie przypisujemy ich temu wznowieniu. Ich kopie wejściowe: /tmp/gate0_cpvdev_baseline_20260930T103329Z oraz /tmp/gate0_cpvdev_resume_20260930T1102. Zastana binarka release miała SHA256 6f108d366f557660096e2f9d0f974539c1f61dbeb9d1844b2340cd9cd8bdd3db.

## 2. Wykorzystane skills i role

Skills: ghost-execution, rust-master, trading-systems. Rola główna: SSOT Feature Materialization Guardian; przeczytano docs/agents/ssot-feature-materialization-guardian.md. Rozważono role Seer Ingest Event Integrity i Decision Logging Replay. Nie uruchamiano osobnych subagentów. Pozostałe dokumenty specjalistów nie były potrzebne do miejscowej poprawki jakości i testów adaptera; nie zmieniano polityki Gatekeepera ani execution.

## 3. Opis problemu — 3W2H

W smoke_metric_surface_legacy_900s_20260930T004445Z dla mintu tZb9aZdDRS95vVqoKgphYZQnGScFV1gdggctvsJPSob CPV było clean w fazie IV, niedostępne w fazie V. To przeczy hipotezie całkowitego braku podłączenia progressu. dev_wallet_known=false występuje już w fazie I, przed migracją. Stary birth nie zapisuje creator; artefakt nie pozwala ustalić konkretnego źródłowego adresu twórcy ani dowieść jego utraty przy migracji.

Mimo nieznanej identity pole DBIA otrzymywało not_applicable/DBIA_NO_DEV_BUY, co sugerowało ustalony brak zakupu przez developera. Brak tożsamości nie uzasadnia takiego rozstrzygnięcia.

## 4. Przyczyna źródłowa

Zastana poprawka CPV usuwa sytuację, w której oddzielne obcięcie epoch i elapsed czyni czas konsumenta wcześniejszym od raw ingress. Producent poprawnie odrzuca takie wejście lub unieważnia ciągłość. Test producenta odtwarza ten mechanizm; sam historyczny JSONL nie dowodzi, że był jedyną przyczyną każdego nulla.

Zastana poprawka parsera tylko w Gate0 zachowuje protokołowy creator PDA potwierdzony w CreateEvent. Zwykły parser nadal ma wallet-only sanitization. Nie jest to dowód, że konkretny historyczny mint miał taki adres.

Potwierdzona przyczyna mylnego statusu DBIA: adapter quality bezwarunkowo tłumaczył DBIA_NO_DEV_BUY na not_applicable, nie sprawdzając dev_wallet_known.

## 5. Strategia naprawy

Zachować jednego producenta CPV, konfigurację lookbacku, fail-closed gap oraz istniejący materializer. Oddzielić nieznaną identity od znanego developera bez BUY. Udokumentować provenance i poddać bieżącą binarkę ograniczonemu smoke przed jakimkolwiek runem 10 h.

## 6. Przeprowadzone akcje naprawcze

Zmiany tego wznowienia:
- ghost-launcher/src/gate0/metrics.rs: nieznany developer => DBIA input_unavailable/verified_developer_identity_unavailable; znany bez BUY => not_applicable/DBIA_NO_DEV_BUY.
- ghost-launcher/src/gate0/tests.rs: regresja known/unknown DBIA i rzeczywisty adapter progress → indeks sesji, epoch reset, gap, recovery przed admission oraz fatal gap po admission.
- configs/gate0.toml: po potwierdzonym wyczerpaniu limitu zmieniono wyłącznie profil obserwatora na max_events_per_token=65536; global131072, default kodu16384 i fail-closed bez zmian.
- .codex/active-task.md: checkpoint pracy.

Zastane, poddane review zmiany: zegar ghost_gate0, creator z CreateEvent w parserze, additive birth/snapshot provenance, source_diagnostics CPV oraz test zachowania dev metrics i logicznego rynku po migracji. Nie zmieniano wzorów metryk, progów C/D/E, trybu zwykłego runtime ani /root/Gho/.env. Endpoint sprawdzony: grpc.nln.clr3.org:443. Nie wykonywano RPC backfillu.

## 7. Walidacja działań naprawczych

W bieżącym wznowieniu Gate0 18/18, CPV 30/30, parser 134/134, CLI 3/3 i osiem dokładnych regresji FSC/DES/dev/top3 PASS. Filtr failed transactions PASS. Cargo check, fmt, git diff --check, Clippy --no-deps i Python6 PASS. Release build exit0, SHA256 17bdbb3dd03f39288a01d204c494b854fa744c94490f226e3ca59cda21d2f297. Źródła zgodne z manifestem przed i po kompilacji.

Preflight 60 s: smoke_cpvdev_final_60s_20260930T133326Z — process exit0, reason=smoke_only, shutdown_error=null, 27 births/27 known creators, 12 snapshotów, zero naruszeń 60 pól/identity, primary30651/funding100566, zero dropów/expiry, lag6ms. CPV cold-start jest oczekiwany przed pełnym lookbackiem300s.

Pierwszy odbiór900s: smoke_cpvdev_final_900s_20260930T133440Z — FAIL po560s: per-token retention capacity exhausted. Jeden token (HxhDdpS5MBVZNiYd87na6EWFPkwcXGnGZM2KtELnpump) osiągnął dokładnie16384 wpisy:1231 successful swaps +15153 failed signatures w wieku190029ms. Zachowano failed signatures dla dokładnej deduplikacji i ratio. 238births,346snapshotów,zero naruszeń schemy/identity,124CPV clean,49insufficient sample,173source unavailable. Fazy229/65/52/0/0,3migrations+initial,no drops/expiry,lag28ms. Dane oznaczono accepted=false w meta. Nie jest to PASS lifecycle.

Najmniejsza poprawka potwierdzonej przyczyny to konfiguracja pojemności65536/token (global131072). Wydłuża dostępny budżet względem obserwowanych16384 wpisów/190s, ale nie gwarantuje dowolnego przyszłego ruchu. Nie zmienia definicji metryk ani nie wyłącza limitu globalnego.

Drugi preflight60s: smoke_cpvdev_final_60s_20260930T134612Z — PASS,38births,23snapshoty,no drops/expiry,lag7ms.

Drugi odbiór 900 s: smoke_cpvdev_final_900s_20260930T134739Z — exit=1, reason=lifecycle smoke has no phase-V snapshot, shutdown_error=null. Ta sama binarka, nowy config hash i limity zweryfikowane z run_start. Run przepracował 900 s, ale nie jest PASS procesu/lifecycle.

Wyniki: 467 births (467 known creators), 728 snapshotów, fazy [450,148,129,1,0], 7 migracji i 7 initial states, 0 completed. Primary 660271/funding 1330477; zero dropów, zero pending expiry, lag maksymalnie 38 ms. 713 snapshotów pump_curve i 15 pumpswap. We wszystkich 728 snapshotach dokładnie 60 statusów, brak brakujących kluczy/niefinitywnych wartości/nulli bez powodu i brak zmiany identity od birth. failed_tx_ratio oraz dev_buy_sol/dev_tx_ratio/dev_volume_ratio liczbowe 728/728.

CPV: 358 clean, 137 insufficient sample, 233 source unavailable. Początek ciągłości źródła identyczny we wszystkich snapshotach. Phase IV na PumpSwap ma CPV clean (1114 buyerów), znanego developera i liczbowy failed_tx_ratio; dev_paperhand_latency_ms=null jest censored/no_dev_sell_by_cutoff. Brak Phase V nadal uniemożliwia zamknięcie pełnego odbioru CPV/dev po 600 s. Przyczyny wszystkich nulli zapisano w raporcie; nie utożsamiamy samej obecności reason code z dowodem każdej wartości.

Pełny raport 60 pól i ograniczeń: /root/gho_gate0_runs_20260927/smoke_cpvdev_final_900s_20260930T134739Z.audit.md; dane strukturalne w .audit.json; metadane w .meta.json. Obydwa 900 s runs mają accepted=false. Drugi run nie przekroczył starej granicy retencji: maksimum na terminal 5855, więc nowy budżet nie został sprawdzony powyżej 16384 na live danych.

Logi: /tmp/gate0_resume_final_tests.log, /tmp/gate0_resume_validation/, /tmp/gate0_resume_final_release.log. Manifest źródeł: /tmp/gate0_resume_source_manifest.json.

## 8. Zabezpieczenia antyregresyjne

Utrzymano null z precyzyjną przyczyną, shared CPV index, reset przy epoch/gap, brak sztucznego ready, niezmienione defaulty zwykłego parsera, canonical curve/PumpSwap jako jeden rynek, tożsamość sesji po migracji i obserwator bez execution. Self-review: zakres zgodny z gate0.md; brak nowych progów i brak rozszerzenia parsera wrapperów.

## Otwarte ryzyka / follow-up

Pozostaje uzyskać Phase V i completed_with_migration_initial_state na aktualnej binarce/profilu oraz odebrać ich metryki. Obecny 900 s smoke nie dostarczył tej próbki. Nie uruchomiono kolejnego capture ani runu 10 h po zakończeniu zaplanowanego ponowienia. Nawet zaliczony smoke nie dowodzi 10 h stabilności. Nie wykonywać commit/push/PR bez zgody użytkownika.

GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
To zadanie dotyczy nowego smoke Gate0; nie prowadzono eksperymentu na GO-D ani zewnętrznego audytu GO-E.

## Końcowy self-review

Zakres końcowy: quality DBIA, dwie regresje Gate0, konfiguracja budżetu retencji i dokumentacja. Zachowano wszystkie zastane zmiany, pusty index Git i HEAD. Nie zmieniono endpointu, wzorów metryk, progów C/D/E, ani authority materializera. Nie wykonywano commit/push/PR/merge, nie uruchomiono 10 h, nie ingerowano w trenchd. Po smoke brak aktywnego ghost_gate0. Finalny source manifest i config hash porównano z metadanymi artefaktu.
