# RAPORT OPERACYJNY P1 V3 SHADOW RERUN - 2026-05-15

## Zakres

Run dotyczy P1 `V3 calibrated shadow sidecar` w namespace artefaktow `shadow-burnin-v3-p1`, na configu:

- rollout: `/root/Gho/configs/rollout/shadow-burnin.toml`
- Ghost Brain: `/root/Gho/ghost-brain/ghost_brain_config.toml`

Non-goals zachowane:

- bez zmiany active V2/V2.5 policy behavior
- bez promocji V3 do active path
- bez zmian w IWIM
- bez zmian w execution/live sender

## Procedura wykonania

Przed runem wykonano clean rerun sequence:

1. `cargo test --workspace --no-run`
2. odswiezenie `.ghost/baseline_accepted_revision` do `b27db61627600fc27633ccd137fe21137a8740ed`
3. backup poprzednich artefaktow `shadow-burnin-v3-p1` do suffixu `.20260515T111441Z.pre-rerun`
4. `bash ./scripts/ghost_production_preflight.sh --config /root/Gho/configs/rollout/shadow-burnin.toml`
5. start runtime:

```bash
timeout 30m env RUST_LOG=info \
cargo run --release -p ghost-launcher --bin ghost-launcher -- \
  --config /root/Gho/configs/rollout/shadow-burnin.toml
```

Run wystartowal okolo `2026-05-15 11:16 UTC` i zakonczyl sie naturalnie po `timeout 30m`.

## Artefakty

Swieze artefakty runu:

- `logs/rollout/shadow-burnin-v3-p1/system.log.2026-05-15`
- `logs/rollout/shadow-burnin-v3-p1/oracle.log.2026-05-15`
- `logs/rollout/shadow-burnin-v3-p1/decisions/seer_runtime_coverage_audit.jsonl`
- `logs/rollout/shadow-burnin-v3-p1/decisions/shadow-burnin-v3-p1/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl`
- `logs/rollout/shadow-burnin-v3-p1/decisions/shadow-burnin-v3-p1/v2.2/legacy_live/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl`
- `datasets/events/shadow-burnin-v3-p1/exec_launcher-1778844115667_20260515_112155_0000.jsonl`
- `datasets/events/shadow-burnin-v3-p1/exec_launcher-1778844115724_20260515_112155_0000.jsonl`
- `datasets/events/shadow-burnin-v3-p1/exec_launcher-1778844115724_20260515_112700_0001.jsonl`
- `datasets/events/shadow-burnin-v3-p1/exec_launcher-1778844115724_20260515_113202_0002.jsonl`
- `datasets/events/shadow-burnin-v3-p1/exec_launcher-1778844115724_20260515_113714_0003.jsonl`
- `datasets/events/shadow-burnin-v3-p1/exec_launcher-1778844115724_20260515_114216_0004.jsonl`
- `data/rollout/shadow-burnin-v3-p1/snapshots/shadow_ledger_snapshot_1778845435668.bin`
- `data/rollout/shadow-burnin-v3-p1/snapshots/shadow_ledger_snapshot_1778845495667.bin`
- `data/rollout/shadow-burnin-v3-p1/snapshots/shadow_ledger_snapshot_1778845555667.bin`

## Wynik V3

Polecenie:

```bash
python3 scripts/v3_shadow_report.py --config configs/rollout/shadow-burnin.toml --json
```

Wynik:

- `status=ok`
- `replay_status=hash_only`
- `artifact_freshness.stale_against_config=false`
- `counts.raw_rows=86`
- `counts.deduped_rows=86`
- `counts.v3_rows=86`
- `hash_coverage.v3_policy_config_hash.coverage=1.0`
- `hash_coverage.v3_feature_snapshot_hash.coverage=1.0`
- `hash_consistency.policy_hash_unique_count=1`
- `hash_consistency.snapshot_hash_unique_count=86`
- `pre_dedupe_conflicts.conflict_groups=0`
- `execution.success_count=0`
- `execution.outcomes.missing=86`

Macierz active vs V3 dla wybranego `v25_shadow`:

- aktywny `REJECT` -> V3 `REJECT`: `70`
- aktywny `REJECT` -> V3 `PENDING`: `16`

Dominujace reason codes dla `v25_shadow`:

- active:
  - `REJECT_PDD_ENTRY_DRIFT`: `48`
  - `REJECT_PDD_WHALE`: `33`
  - `REJECT_PDD_FLASH_CRASH`: `2`
  - `HARD_FAIL_MARKET_CAP`: `2`
  - `REJECT_LOW_TRAJECTORY`: `1`
- V3:
  - `REJECT_V3_MANIPULATION_CONTRADICTION`: `70`
  - `PENDING_V3_WAIT_EVIDENCE`: `13`
  - `PENDING_V3_WAIT_SAMPLE`: `3`

Interpretacja:

- swiezy rerun po remediacji zostal wykonany poprawnie i daje nowe V3 rows
- V3 pozostaje strict sidecar i nie zmienia aktywnego verdict path
- V3 jest interpretacyjnie ostrzejszy na `manipulation_contradiction`
- pozostale `16` rekordow V3 zostawia jako `PENDING`, glownie przez wait-evidence / wait-sample

## Coverage Audit

Plik:

- `logs/rollout/shadow-burnin-v3-p1/decisions/seer_runtime_coverage_audit.jsonl`

Agregaty:

- liczba okien auditowych: `263`
- `audit_status=ok`: `263`
- verdicty:
  - `REJECT`: `86`
  - `TIMEOUT`: `177`
- `window_close_reason`:
  - `END_REACHED`: `200`
  - `END_REACHED_BY_SWEEP`: `63`
- `timeout_primary_cause`:
  - `unclassified`: `86`
  - `genuine_no_interest`: `82`
  - `ingest_miss`: `8`
  - `stale_or_late_arrival`: `1`
- truth windows:
  - `no_truth_no_missing`: `88`
  - `full_truth`: `92`
  - `partial_or_missing`: `83`
- diagnostyka latency:
  - `canonical_first_update_latency_ms mean=7.0`
  - `canonical_first_update_latency_ms max=42`
  - `canonical_update_count mean=13.39`
  - `account_update_runtime_accepted_total mean=13.37`

Interpretacja:

- runtime zakonczyl coverage audit bez corruption sygnalow
- `emitted_without_rx=0`, `runtime_accepted_without_emitted=0`, `missing_reason_fallbacks=0`
- widoczne sa pojedyncze okna z `ingest_miss` i `stale_or_late_arrival`, ale nie dominuja

## Runtime Residuals

W runie nadal wystepowal infrastrukturalny residual:

- `funding_lane_full_chain:primary:0` wielokrotnie dostawal `ResourceExhausted: Concurrent Yellowstone Geyser stream limit reached`

Ocena:

- to nie zablokowalo glownego shadow rerunu
- primary path nadal wygenerowal swieze `v25_shadow` decision rows i coverage audit
- residual nalezy traktowac jako issue lane pomocniczego / dodatkowego feedu, nie jako blocker formalnej walidacji P1

## Weryfikacja koncowa

Uruchomione sprawdzenia:

- `cargo test --workspace --no-run`
- `bash ./scripts/ghost_production_preflight.sh --config /root/Gho/configs/rollout/shadow-burnin.toml`
- `python3 -m unittest scripts/test_v3_shadow_report.py -v`
- `python3 scripts/v3_shadow_report.py --config configs/rollout/shadow-burnin.toml --json`
- `git diff --check`

Wyniki:

- wszystkie powyzsze sprawdzenia przeszly
- rerun zakonczyl sie naturalnie
- swieze P1 V3 evidence po remediacji istnieje

## Ostateczny werdykt

`APPROVED` dla formalnego domkniecia runtime evidence pakietu P1.

Uzasadnienie:

- clean rerun po remediacji zostal wykonany
- artefakty sa swieze wzgledem configu
- raport V3 ma `status=ok`
- `v3_rows > 0` i konkretnie `86`
- hash coverage jest pelne
- replay status pozostaje uczciwie `hash_only`
- V3 pozostaje sidecar-only

Residual do dalszego sledzenia poza P1 closure:

- `funding_lane_full_chain` nadal moze wpasc w `ResourceExhausted`
