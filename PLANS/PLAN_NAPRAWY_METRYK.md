# PLAN NAPRAWY METRYK ANTI-SYBIL / ANTI-CABAL

## 1. Summary

Celem jest naprawa sześciu metryk `FTDI`, `DBIA`, `SFD`, `DES`, `CPV`, `FSC` bez złamania aktywnego kontraktu Ghost:

- kanoniczny snapshot decyzji pozostaje `MaterializedFeatureSet.sybil_resistance`,
- materializacja pozostaje w `/root/Gho/ghost-launcher/src/session/observation.rs::PoolObservationSession::materialize_features()`,
- Gatekeeper konsumuje wyłącznie zmaterializowane pola, bez rekonstrukcji z raw eventów,
- `None` lub nieprodukcyjny/degraded evidence nie może dawać kary,
- neutralne defaulty progów i soft-penalty pozostają poza zakresem naprawy, zgodnie z konspektem,
- nie ruszamy TX buildera, Sendera, live execution, legacy HyperPrediction/Chaos ani starego `score_pool()` path.

Plan składa się z 12 PRów. Każdy PR jest bramką dla kolejnego. Po akceptacji tego planu osobny tryb Agenta ma tylko zapisać dokument do `/root/Gho/PLANS/PLAN_NAPRAWY_METRYK.md`, bez implementowania planu.

## 2. Publiczne Kontrakty I Typy

W toku PRów wolno dodawać wyłącznie backward-compatible pola i reason-code'y:

- W `/root/Gho/ghost-core/src/tx_intelligence/types.rs` dodać nowe reason-code constants:
  `FTDI_PARTIAL_FEE_TOPOLOGY_COVERAGE`, `DBIA_PARTIAL_FINGERPRINT_COVERAGE`, `DES_PARTIAL_SEQUENCE_COVERAGE`, `DES_NO_COMPARABLE_PAIRS`, `SFD_NEGATIVE_BALANCE_DELTA_SKIPPED`, `SFD_BUY_AMOUNT_UNAVAILABLE`, `CPV_COVERAGE_WINDOW_UNAVAILABLE`, `FSC_V2_STATUS_NOT_CLEAN`, `FSC_COVERAGE_WINDOW_UNAVAILABLE`.
- W configu `GatekeeperV2Config` dodać serde-defaulted quality knobs:
  `min_toolchain_metric_coverage = 0.70`, `min_des_valid_sequence_coverage = 0.75`, `cpv_min_observed_window_ratio = 1.0`, `fsc_require_clean_v2_for_actionability = true`, `fsc_require_coverage_window_for_actionability = true`.
- W `SybilResistanceFeatures` dodać wyłącznie optional/additive fields dla diagnostyki CPV i quality coverage, np. `cpv_distinct_other_pools_mean`, `cpv_other_pool_activity_count_p95`, `toolchain_fingerprint_coverage`, `des_valid_sequence_coverage`; wszystkie z `#[serde(default, skip_serializing_if = "Option::is_none")]`.
- W `FscV2Evidence` dodać additive fields: `coverage_window_ready`, `coverage_window_remaining_ms`, `authoritative_buy_ready`; wszystkie serde-defaulted.
- `funding_source_concentration` pozostaje publicznym kanonicznym polem FSC, ale po naprawie jego wartość ma pochodzić z clean `funding_source_v2.scoring_hhi_non_neutral`, nie z legacy `1 - distinct/N`.
- `FscV2Config.decision_enabled` i `hard_reject_enabled` pozostają walidacyjnie zabronione; nie używać ich do aktywacji hard policy. Naprawa polega na poprawie kanonicznej wartości i actionability, nie na włączeniu osobnego FSC-v2 hard-reject branch.

## 3. Chronologiczny Plan PRów

### PR1 - Quality Contract, Reason Codes I Config Plumbing

Cel: przygotować wspólny kontrakt jakości evidence dla wszystkich napraw, bez zmiany decyzji.

Zakres:
`/root/Gho/ghost-core/src/tx_intelligence/types.rs`, `/root/Gho/ghost-brain/src/config/ghost_brain_config.rs`, testy config/serde.

Kroki:
- Dodać reason-code constants wymienione w sekcji 2.
- Dodać serde-defaulted pola quality config do `GatekeeperV2Config` z walidacją zakresu `[0.0, 1.0]`.
- Dodać mały internal struct `SybilMetricQualityConfig` w `/root/Gho/ghost-launcher/src/tx_intelligence/sybil_metrics.rs`, budowany z `GatekeeperV2Config`.
- Nie zmieniać jeszcze algorytmów metryk ani policy.

Kryteria wejścia:
- Start z czystej gałęzi lub jawnie odseparowanymi cudzymi zmianami; żadnego `git add .`.
- Aktualny kod kompiluje albo znane unrelated failures są opisane przed pracą.

DoD:
- Stare TOML-e bez nowych pól nadal parsują się poprawnie.
- `cargo test -p ghost-brain test_gatekeeper_v2_from_toml_file_partial_override -- --nocapture` przechodzi.
- `cargo test -p ghost-brain test_fsc_v2_defaults_are_capture_inert -- --nocapture` przechodzi.
- Brak zmian w BUY/REJECT/TIMEOUT w testach `gatekeeper_policy_tests`.

### PR2 - FSC V2 Coverage Evidence W Materializacji

Cel: dopiąć `coverage_window_status()` do kanonicznego FSC evidence, żeby readiness był widoczny w snapshot/logach, a nie tylko w live-buy gate.

Zakres:
`/root/Gho/ghost-launcher/src/tx_intelligence/funding_source.rs`, `/root/Gho/ghost-launcher/src/session/observation.rs`, `/root/Gho/ghost-core/src/tx_intelligence/types.rs`, buy-log/DecisionLogger mirror.

Kroki:
- Rozszerzyć `FscV2Evidence` o `coverage_window_ready`, `coverage_window_remaining_ms`, `authoritative_buy_ready`.
- Rozszerzyć `FundingSourceIndex::compute_for_transactions` o deterministyczny parametr `decision_wall_ms` albo osobną metodę `compute_for_transactions_at(...)`; nie używać ukrytego zegara w testach.
- W `PoolObservationSession::materialize_features()` przekazać decision-time wall timestamp i zapisać coverage fields w `funding_source_v2`.
- Jeśli `fsc_require_coverage_window_for_actionability = true` i `authoritative_buy_ready = false`, dodać degraded reason `FSC_COVERAGE_WINDOW_UNAVAILABLE`; sama diagnostyka nadal może być emitowana.

Kryteria wejścia:
- PR1 merged.
- Nowe reason-code'y i config pola istnieją, stare configi parsują się bez zmian.

DoD:
- Unit test w `funding_source.rs`: coverage window przed pełnym lookback daje `authoritative_buy_ready=false`.
- Session-level test w `session_lifecycle_tests.rs`: materialized `funding_source_v2.authoritative_buy_ready` zgadza się z `FundingSourceIndex::coverage_window_status`.
- Decision log test potwierdza additive serialization nowych pól bez schema break.
- Brak odczytu live/raw state w `gatekeeper_policy.rs`.

### PR3 - FSC Actionability Gate Honoruje `funding_source_v2.status`

Cel: zamknąć najgroźniejszą lukę: FSC nie może być actionable, gdy `funding_source_v2.status != Clean` albo coverage nie jest gotowy.

Zakres:
`/root/Gho/ghost-launcher/src/components/gatekeeper_policy.rs`, testy policy.

Kroki:
- Dodać helper `fsc_metric_is_actionable(sybil, config)`:
  wymaga `funding_source_concentration.is_some()`, `funding_source_v2.is_some()`, `snapshot_mode == DecisionTime`, `status == Clean`, `scoring_hhi_non_neutral.is_some()`, oraz jeśli config wymaga coverage: `authoritative_buy_ready == true`.
- `sybil_metric_is_actionable(..., Fsc)` ma używać tego helpera.
- `sybil_combo_veto_reason()` ma traktować każdy `FscEvidenceStatus::Degraded/Unavailable` oraz każdy `FscExcludedReason` jako blokadę FSC-combo, chyba że config jawnie wyłączy ready requirement.
- Nie zmieniać jeszcze matematyki `funding_source_concentration`.

Kryteria wejścia:
- PR2 merged.
- `funding_source_v2` niesie status i coverage w materialized snapshot.

DoD:
- Test policy: `funding_source_concentration=Some(high)` + `funding_source_v2.status=Degraded(LowCoverage)` daje `high_fsc=false`, zero punktów i brak combo-veto.
- Test policy: `status=Clean`, `authoritative_buy_ready=true`, HHI above threshold daje `high_fsc=true`.
- Test policy: `snapshot_mode=EventualPostfill` nigdy nie jest actionable.
- Istniejące `degraded_sybil_metrics_do_not_score_even_with_active_penalties` nadal przechodzi.

### PR4 - FSC Primary Score = Normalized HHI Clean-Only

Cel: zastąpić legacy `1 - distinct/N` jako główny scoring FSC przez normalized HHI z `funding_source_v2`.

Zakres:
`/root/Gho/ghost-launcher/src/tx_intelligence/funding_source.rs`, session/event-bus tests, policy tests.

Kroki:
- W `compute_for_transactions_at(...)` po zbudowaniu `funding_source_v2` ustawiać `funding_source_concentration = funding_source_v2.scoring_hhi_non_neutral` tylko gdy `funding_source_v2.status == Clean` i score istnieje.
- Gdy v2 nie jest clean, `funding_source_concentration = None`, a degraded reason mapuje się deterministycznie z `excluded_reason` na istniejące lub nowe reason-code'y.
- Usunąć użycie legacy `1 - distinct/N` z pola decyzyjnego. Jeżeli potrzebny jest debug legacy, wolno zostawić go tylko jako test-local helper lub additive diagnostics, nie jako policy input.
- Fallback `FundingSourceConfig::from_configs(config, None)` ma używać bezpiecznego `min_rel_to_buy = FscV2Config::default().min_rel_to_buy` zamiast `0.0`.

Kryteria wejścia:
- PR3 merged.
- FSC degraded status blokuje actionability.

DoD:
- Unit testy:
  `[A,A] -> 1.0`,
  `[A,A,B,B] -> 1/3`,
  `[A,A,A,B] -> 0.5`,
  `[A,B,C] -> 0.0`.
- Existing tests oczekujące `Some(0.5)` dla dwóch buyerów z tym samym funderem są zaktualizowane do `Some(1.0)`.
- Test neutral funders: `NeutralOnly` daje `funding_source_concentration=None`, mimo że raw including-neutral HHI może być wysokie.
- Test low confidence / low coverage / same-slot unorderable: `funding_source_concentration=None`.
- `cargo test -p ghost-launcher funding_source -- --nocapture` przechodzi.

### PR5 - FTDI/DBIA Policy Corroboration Guard

Cel: zanim metryki toolchainowe staną się bardziej dostępne przez partial coverage, policy musi przestać nadawać pełną moc solo `high_dbia` i solo `low_ftdi`.

Zakres:
`/root/Gho/ghost-launcher/src/components/gatekeeper_policy.rs`, `gatekeeper_policy_tests`.

Kroki:
- Zachować flagi telemetryczne `low_ftdi` i `high_dbia`.
- Zmienić naliczanie punktów:
  `high_dbia` solo przy `low_ftdi=false` daje 0 punktów i nie jest lead signal.
  `low_ftdi` solo przy `high_dbia=false` może dawać tylko skonfigurowaną lekką karę, ale nie może tworzyć structural cabal lead signal.
  `high_dbia && low_ftdi` pozostaje głównym structural pattern.
- `HighDbiaLowFtdiLowSfd` nadal może wejść do combo-veto po spełnieniu pozostałych warunków.
- Nie zmieniać progów configu.

Kryteria wejścia:
- PR3 merged co najmniej dla ogólnego modelu actionability.
- Sybil layer tests są zielone przed zmianą.

DoD:
- Test `high_dbia_with_high_ftdi_does_not_change_policy_verdict` oczekuje `soft_points=0` dla DBIA solo z wysokim FTDI.
- Test `high_dbia_low_ftdi` potwierdza punkty/pattern tylko przy obu sygnałach.
- Test `low_ftdi` bez DBIA nie może wygenerować `RejectSybilInterference`.
- Żaden degraded metric nie nalicza punktów.

### PR6 - FTDI/DBIA Best Complete Sample I Partial Coverage

Cel: usunąć hard-fail na pojedynczym brakującym fingerprintcie i zastąpić first-sample bias selekcją najlepszej próbki per signer.

Zakres:
`/root/Gho/ghost-launcher/src/tx_intelligence/sybil_metrics.rs`, `/root/Gho/ghost-launcher/src/session/observation.rs`, testy session/materialization.

Kroki:
- Dodać deterministic selector `best_toolchain_sample_per_signer(metric_kind)`.
- Dla FTDI wybierać per signer próbkę z dostępną `fee_topology()`, jeśli istnieje; inaczej signer liczy się jako missing.
- Dla DBIA wybierać per signer próbkę z kompletnym `InfrastructureFingerprint`; dev też musi przejść przez ten selector.
- FTDI materializuje się, jeśli usable unique signers >= 3 i coverage >= `min_toolchain_metric_coverage`.
- DBIA materializuje się, jeśli dev fingerprint jest kompletny, usable non-dev buyers >= 2 i coverage >= `min_toolchain_metric_coverage`.
- Przy częściowym, ale wystarczającym coverage dodać `FTDI_PARTIAL_FEE_TOPOLOGY_COVERAGE` lub `DBIA_PARTIAL_FINGERPRINT_COVERAGE`; te reason-code'y nie blokują actionability.
- Przy coverage poniżej progu zachować `*_RAW_*_UNAVAILABLE` i `None`.

Kryteria wejścia:
- PR5 merged, żeby większa dostępność FTDI/DBIA nie zwiększała false-positive risku przez solo DBIA.
- Quality config z PR1 jest dostępny w materializacji.

DoD:
- Test: signer A ma pierwszy buy bez fingerprintu i drugi kompletny; FTDI/DBIA używa drugiego.
- Test: 1 missing signer, 4 complete signers, coverage >= 0.70 -> value `Some`, partial reason present.
- Test: coverage < 0.70 -> value `None`, raw unavailable reason present.
- Session-level test potwierdza, że `MaterializedFeatureSet.sybil_resistance` zawiera value i degraded reasons zgodnie z helperem.
- `cargo test -p ghost-launcher tx_intelligence::sybil_metrics -- --nocapture` przechodzi.

### PR7 - DBIA Normalized Numeric Distance

Cel: zmienić DBIA z binarnego `!=` dla cech liczbowych na stabilniejszą odległość znormalizowaną.

Zakres:
`/root/Gho/ghost-launcher/src/tx_intelligence/sybil_metrics.rs`, unit tests.

Kroki:
- Zostawić booleany jako binary distance.
- Dla `account_keys_len` użyć `min(abs(a-b)/8.0, 1.0) * DBIA_ACCOUNT_KEYS_WEIGHT`.
- Dla `outer_instruction_count` użyć `min(abs(a-b)/4.0, 1.0) * DBIA_OUTER_INSTRUCTION_WEIGHT`.
- Dla `inner_instruction_group_count` użyć `min(abs(a-b)/4.0, 1.0) * DBIA_INNER_GROUP_WEIGHT`.
- Dla `fee_topology` użyć średniej znormalizowanej różnicy `external/internal`, każda capowana przez scale `3.0`, pomnożonej przez `DBIA_FEE_TOPOLOGY_WEIGHT`.
- Wynik similarity nadal clampowany do `[0.0, 1.0]`.

Kryteria wejścia:
- PR6 merged, bo normalized distance działa na kompletnych próbkach.

DoD:
- Test: dev `outer_instruction_count=7`, buyer `8` spada lekko, nie o pełne `0.25`.
- Test: wszystkie składowe skrajnie różne nadal daje similarity blisko `0.0`.
- Test: identyczne fingerprinty nadal `1.0`.
- Testy DBIA policy po PR5 nadal zielone.

### PR8 - DES Partial Sequence I `NO_COMPARABLE_PAIRS`

Cel: DES nie może znikać przez pojedynczy brak curve/slot, ale też nie może udawać neutralnego `0.0`, gdy nie ma informacji porównawczej.

Zakres:
`/root/Gho/ghost-launcher/src/tx_intelligence/sybil_metrics.rs`, session tests.

Kroki:
- Zastąpić globalny hard-fail algorytmem najdłuższego poprawnego segmentu:
  ordered successful buys -> segmenty z pełnym `slot` i `curve_price`.
- Missing `slot` lub missing/invalid curve price rozcina segment i zwiększa partial coverage counters.
- DES liczyć na najdłuższym segmencie długości >= 4; przy remisie wybrać wcześniejszy segment w ordered sequence.
- `kendall_tau` zastąpić Tau-b semantics albo użyć czystego helpera `ghost_core::features::coordination::stats::kendall_tau_b`; brak porównywalnych par musi zwrócić `None`.
- Przy częściowym, ale wystarczającym segmencie dodać `DES_PARTIAL_SEQUENCE_COVERAGE`.
- Przy braku porównywalnych par dodać `DES_NO_COMPARABLE_PAIRS` i `demand_elasticity_score=None`.

Kryteria wejścia:
- PR1 quality config dostępny.
- FTDI/DBIA zmiany nie są w trakcie w tym samym PR.

DoD:
- Test: jeden invalid curve sample w środku, istnieje segment 4 valid buys -> DES `Some`, partial reason.
- Test: invalid sample powoduje brak segmentu >=4 -> DES `None`.
- Test: wszystkie delta-price lub delta-time ties -> DES `None` + `DES_NO_COMPARABLE_PAIRS`, nie `Some(0.0)`.
- Test same-slot z `event_ordinal` i bez niego pozostaje deterministyczny.
- Brak hybrydowego Spearman w tym PR; dokumentacja ma jasno mówić `Kendall/Tau-b only`.

### PR9 - SFD Weighted MAD I Anomalous Balance Semantics

Cel: domknąć rozjazd spec-kod przez weighted MAD `sqrt(buy_amount)` i oddzielić anomalne próbki od zerowego spendu.

Zakres:
`/root/Gho/ghost-launcher/src/tx_intelligence/sybil_metrics.rs`, opcjonalnie reuse pure helpers z `/root/Gho/ghost-core/src/features/coordination/stats.rs`.

Kroki:
- SFD ma liczyć weighted MAD:
  value = spend fraction, weight = `sqrt(buy_amount_sol)`.
- Źródło weight:
  najpierw `sol_amount_lamports` jeśli `Some > 0`,
  fallback `volume_sol` jeśli finite i > 0,
  inaczej sample skip + `SFD_BUY_AMOUNT_UNAVAILABLE`.
- Spend fraction v1:
  jeśli dostępny jest `sol_amount_lamports`, użyć `sol_amount_lamports / pre_balance`.
  jeśli go brak, fallback do `pre_balance.saturating_sub(post_balance) / pre_balance` tylko z degraded reason `SFD_BUY_AMOUNT_UNAVAILABLE`.
- Jeśli `post_balance > pre_balance`, sample nie może wejść jako `0.0`; pominąć go i dodać `SFD_NEGATIVE_BALANCE_DELTA_SKIPPED`.
- Wartość materializuje się tylko jeśli po filtrach zostają >= 3 usable signers; częściowe braki nadal mogą być actionable jak obecne `SFD_PARTIAL_BALANCE_COVERAGE`.

Kryteria wejścia:
- PR1 reason-code'y dostępne.
- DES PR nie jest mieszany w tym samym diffie.

DoD:
- Test cabal i organic z konspektu nadal dają odpowiednio niski/wysoki SFD.
- Test dust spam: weighted MAD różni się od unweighted i nie jest zdominowany przez dust.
- Test `post > pre`: próbka pominięta, reason present, brak sklejenia z zerowym spendem.
- Test missing buy amount: fallback lub skip zgodnie z powyższą regułą, z reason.
- Policy test `partial_sfd_coverage_remains_actionable_when_value_is_present` nadal przechodzi.

### PR10 - CPV Coverage Window I Intensity Diagnostics

Cel: CPV nie może być actionable tuż po starcie procesu na niepełnej historii, a diagnostyka ma rozróżniać płytką rotację od rotacyjnej sieci.

Zakres:
`/root/Gho/ghost-launcher/src/tx_intelligence/cross_pool_velocity.rs`, `/root/Gho/ghost-core/src/tx_intelligence/types.rs`, materializacja i logs.

Kroki:
- Dodać do `CrossPoolVelocityInner` `first_observed_at_ms` i `last_observed_at_ms`.
- Readiness w `compute_for_transactions`:
  `observed_window_ms = last_observed_at_ms - first_observed_at_ms`,
  required window = `lookback_window_ms * cpv_min_observed_window_ratio`.
- Jeśli window nie spełnia wymogu, `signer_cross_pool_velocity=None` + `CPV_COVERAGE_WINDOW_UNAVAILABLE`.
- Dodać diagnostics:
  `distinct_other_pools_mean`,
  `other_pool_activity_count_p95`,
  liczone tylko z historii w lookback window i tylko dla current unique signers.
- Zmaterializować diagnostics additive w `SybilResistanceFeatures`; nie używać ich jeszcze w policy.

Kryteria wejścia:
- PR1 config dostępny.
- `CPV_ROLLING_STATE_UNAVAILABLE` nadal oznacza cold/empty; nowy reason oznacza partial warmup.

DoD:
- Test: fresh process, pojedyncza historia, 3 signers -> CPV `None` + `CPV_COVERAGE_WINDOW_UNAVAILABLE`.
- Test: observed window >= lookback -> CPV jak dotychczas.
- Test TTL nadal usuwa stale history.
- Test repeated buys count unique signers once.
- DecisionLogger serializuje nowe CPV diagnostics addytywnie.

### PR11 - Ingest/Source Coverage Observability

Cel: metryki muszą mieć widoczną jakość danych per source path, zwłaszcza dla FTDI/DBIA, DES i SFD.

Zakres:
`/root/Gho/off-chain/components/seer/src/binary_parser.rs`, `/root/Gho/off-chain/components/seer/src/types.rs`, `/root/Gho/off-chain/components/seer/src/pumpportal_connection.rs`, `/root/Gho/off-chain/components/seer/src/nln_program_streams.rs`, `/root/Gho/ghost-launcher/src/components/seer.rs`, metrics.

Kroki:
- Dodać liczniki/gauge coverage:
  `seer_toolchain_fingerprint_coverage_total{source,complete}`,
  `seer_fee_topology_coverage_total{source,available}`,
  `seer_curve_data_coverage_total{source,known}`,
  `seer_signer_balance_coverage_total{source,pre,post}`,
  `seer_funding_transfer_observations_total{lane,coverage}` jeśli nie jest już kompletne.
- Source labels muszą rozróżniać co najmniej binary parser, PumpPortal, NLN program streams.
- Nie defaultować fingerprintu jako "complete"; `ToolchainFingerprintInput::default()` ma być liczony jako missing coverage.
- Dodać parser tests dla binary parser: populated fingerprint daje coverage complete.
- Dodać PumpPortal/NLN tests lub fixtures: default fingerprint daje coverage missing, nie complete.

Kryteria wejścia:
- PR6/PR8/PR9 merged, żeby wiadomo było, jakie coverage jest decyzyjne.
- Nie zmieniać formatów transportowych poza additive metrics.

DoD:
- `cargo test -p seer binary_parser -- --nocapture` przechodzi.
- `cargo test -p ghost-launcher components::seer -- --nocapture` albo najbliższy istniejący test modułu przechodzi.
- Metrics są opisane w `/root/Gho/docs/RUNBOOK_HOT_PATH_METRICS.md`.
- Brak zmian w `PoolTransaction` required fields; stare payloady deserialize.

### PR12 - Replay, Logging, Docs I Final Acceptance Gate

Cel: zamknąć temat formalnie: decision logs, replay/report i dokumentacja muszą pokazywać nową semantykę metryk bez mylenia shadow/live ani legacy/v2.

Zakres:
`/root/Gho/ghost-brain/src/oracle/decision_logger.rs`, `/root/Gho/ghost-launcher/src/components/gatekeeper.rs`, docs/ADR/runbook/plan.

Kroki:
- Upewnić się, że buy log zawiera:
  wszystkie sześć wartości,
  degraded reasons,
  `funding_source_v2.status/excluded_reason/coverage`,
  CPV diagnostics,
  coverage diagnostics dla FTDI/DBIA/DES/SFD.
- Zaktualizować `NOWE_METRYKI_DO_WDROZENIA.md` i/lub dodać ADR follow-up:
  FSC primary score = normalized HHI clean-only,
  DES = Kendall/Tau-b only, no Spearman in v1,
  SFD = weighted MAD with explicit fallback/degraded semantics,
  FTDI/DBIA partial coverage semantics,
  CPV coverage-window readiness.
- Dodać lub zaktualizować replay/report check, który failuje, gdy:
  FSC policy używa degraded v2,
  DES `NO_COMPARABLE_PAIRS` zapisuje `0.0`,
  DBIA solo z high FTDI nalicza structural penalty,
  CPV działa bez coverage window.
- Dokument końcowy ma zachować informację, że neutralne progi i soft-penalty pozostają świadomie poza zakresem tej naprawy.

Kryteria wejścia:
- PR1-PR11 merged i zielone.
- Nie ma aktywnych unrelated zmian w plikach objętych allowlistą.

DoD:
- `cargo test -p ghost-launcher tx_intelligence::sybil_metrics -- --nocapture` przechodzi.
- `cargo test -p ghost-launcher funding_source -- --nocapture` przechodzi.
- `cargo test -p ghost-launcher cross_pool_velocity -- --nocapture` przechodzi.
- `cargo test -p ghost-launcher --test gatekeeper_policy_tests sybil -- --nocapture` przechodzi.
- `cargo test -p ghost-launcher --test session_lifecycle_tests sybil -- --nocapture` albo najbliższy istniejący filtr metryk przechodzi.
- `cargo test -p ghost-launcher --test oracle_event_bus_integration fsc -- --nocapture` przechodzi.
- `cargo test -p ghost-brain decision_logger -- --nocapture` przechodzi.
- `cargo test -p ghost-core --test coordination_stats_pr3 -- --nocapture` przechodzi, jeśli PRy użyły helperów `ghost-core::features::coordination::stats`.
- `cargo fmt --check` przechodzi.
- `cargo test --workspace --no-fail-fast` jest docelowym finalnym gate'em; jeśli za długo trwa albo ma unrelated failures, muszą być jawnie udokumentowane z wąskim zielonym zestawem powyżej.

## 4. Acceptance Matrix

Każdy problem z konspektu ma mieć zamknięcie:

- `FSC legacy math weak`: zamknięte w PR4 przez normalized HHI clean-only.
- `FSC v2 degraded still actionable`: zamknięte w PR2-PR3.
- `FSC readiness not controlling decision`: zamknięte w PR2-PR3 i potwierdzone coverage fields.
- `FSC min_rel_to_buy fallback 0.0`: zamknięte w PR4 fallbackiem do `FscV2Config::default().min_rel_to_buy`.
- `FTDI hard-fail on one missing topology`: zamknięte w PR6 partial coverage.
- `FTDI first sample bias`: zamknięte w PR6 best complete sample.
- `FTDI false positives with retail bot`: ograniczone w PR5 przez DBIA/FTDI corroboration policy.
- `DBIA hard-fail on missing fingerprint`: zamknięte w PR6.
- `DBIA binary numeric distance`: zamknięte w PR7.
- `DBIA solo too strong`: zamknięte w PR5.
- `SFD weighted spec drift`: zamknięte w PR9.
- `SFD pre-post contamination`: ograniczone w PR9 przez preferencję `sol_amount_lamports/pre_balance`; dokładne fee/tip fields pozostają future additive, jeśli parser je dostarczy.
- `SFD saturating_sub hides anomalies`: zamknięte w PR9.
- `DES hard-fail on one missing curve/slot`: zamknięte w PR8.
- `DES comparable_pairs == 0 returns 0.0`: zamknięte w PR8.
- `DES hybrid spec drift`: zamknięte dokumentacyjnie w PR12 jako `Kendall/Tau-b only v1`.
- `CPV cold-start readiness weak`: zamknięte w PR10.
- `CPV binary intensity loss`: zamknięte diagnostycznie w PR10.
- `Ingest/source completeness invisible`: zamknięte w PR11.
- `Neutral default thresholds`: explicit out-of-scope; tylko serde/backward compatibility i brak regresji są testowane.

## 5. Delegation Trace

```yaml
task_classification: "cross-cutting architecture and execution plan for active Ghost decision metrics"
routing_performed: true
primary_specialist: "Ghost Runtime Coordinator"
supporting_specialists_considered:
  - "SSOT Feature Materialization Guardian"
  - "Gatekeeper Policy Auditor"
  - "Decision Logging Replay Analyst"
  - "Config Rollout Safety Reviewer"
  - "Seer Ingest Event Integrity Specialist"
  - "Oracle Session Runtime Engineer"
specialist_docs_loaded:
  - "/root/Gho/docs/agents/ghost-runtime-coordinator.md"
  - "/root/Gho/docs/agents/ssot-feature-materialization-guardian.md"
  - "/root/Gho/docs/agents/gatekeeper-policy-auditor.md"
  - "/root/Gho/docs/agents/seer-ingest-event-integrity-specialist.md"
  - "/root/Gho/docs/agents/config-rollout-safety-reviewer.md"
  - "/root/Gho/docs/agents/decision-logging-replay-analyst.md"
specialist_docs_not_loaded:
  - name: "Solana Execution Path Engineer"
    reason: "Plan nie dotyka TX buildera, Sendera, blockhash, fee submission ani live inclusion."
  - name: "Oracle Session Runtime Engineer"
    reason: "Kontekst użyty z AGENTS i kodu wystarczył; plan dotyka materializacji, nie schedulingu/tokio lifecycle."
skills_used:
  - "ghost-execution"
  - "trading-systems"
  - "statistical-research-engine"
  - "large-data-analytics"
fast_path_used: false
runtime_area_touched:
  - "Seer parser/source coverage"
  - "TxIntelligence sybil metrics"
  - "FundingSourceIndex"
  - "CrossPoolVelocityIndex"
  - "PoolObservationSession materialization"
  - "Gatekeeper sybil policy"
  - "DecisionLogger/buy-log evidence"
contracts_checked:
  - "MaterializedFeatureSet remains SSOT"
  - "No raw-state recomputation in Gatekeeper policy"
  - "No legacy HyperPrediction/score_pool revival"
  - "Serde backward compatibility for config/log fields"
  - "Degraded/None metrics produce zero penalty"
  - "Shadow/live boundary unchanged"
  - "FSC v2 decision/hard_reject flags stay disabled"
unresolved_routing_uncertainty: []
risk_level: "high"
recommended_action: "Po akceptacji zapisać ten plan jako /root/Gho/PLANS/PLAN_NAPRAWY_METRYK.md; implementację wykonywać później PR-by-PR, allowlist-only."
```
