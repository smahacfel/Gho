# Walidacja P0 V3 Shadow/Evidence - 2026-05-14

## Status

**GO dla P0 shadow/evidence.**

Walidacja potwierdza, ze V3 Stack dzialal jako additive shadow/evidence sidecar bez promocji do active policy. Aktywny kontrakt `reason_code_version` pozostal `2`, a V3 nie pojawil sie jako routed `decision_plane`.

## Zakres

- Repo: `/root/Gho`
- SSOT config: `/root/Gho/configs/rollout/shadow-burnin.toml`
- Tryb oczekiwany:
  - `entry_mode = "shadow_only"`
  - `execution_mode = "shadow"`
- Cel: pelna walidacja P0 shadow/evidence, bez potwierdzania live execution i bez promotion readiness.
- Runtime window: krotki rollout ograniczony komenda `timeout 30m`.

## Caveat - Dirty Config

Walidacja zostala wykonana na dirty worktree. Przed runem i po runie obecny byl lokalny diff w:

```text
ghost-brain/ghost_brain_config.toml
```

Zmiana:

```diff
-min_market_cap_sol = 41.0
+min_market_cap_sol = 30.0
```

Ten caveat nie narusza bezposrednio kontraktu P0 V3 shadow/evidence, ale oznacza, ze wynik operacyjny nalezy traktowac jako wykonany na lokalnie zmienionym configu brain. Nie nalezy przedstawiać tej walidacji jako wyniku z czystego `HEAD`.

Pozostale zmiany worktree po walidacji:

- `_archive_p0_validation_20260514T083840Z/` - archiwum starych artefaktow przed runem.
- `doc_backup.tar.gz` - niezwiazany nieledzony plik obecny w repo.

## Preflight

Wykonano:

```bash
git status --short --branch
git log --oneline --max-count=10
git diff --check origin/main..HEAD
```

Wynik:

- Branch: `main...origin/main`.
- V3 P0 commit chain obecny, m.in.:
  - `d99c66e Add V3 evidence feature types`
  - `109317d Materialize V3 shadow evidence features`
  - `97440cc Add V3 shadow logger schema fields`
  - `fdc96c1 Add V3 shadow evaluator`
  - `f20f104 Log V3 shadow sidecar decisions`
  - `5c83ec0 Add V3 shadow report script`
  - `2021f53 Address V3 P0 shadow evidence review gaps`
  - `9b959a7 PLANS DLA GK V3 + P0 V3 SHADOW ONLY.`
- `git diff --check origin/main..HEAD`: bez outputu.
- Worktree dirty przez caveat opisany wyzej.

## Testy Kontraktowe

Wszystkie wymagane narrow gates przeszly:

```bash
cargo test -p ghost-core feature_builder
cargo test -p ghost-core materialized
cargo test -p ghost-brain reason_code
cargo test -p ghost-brain decision_logger
cargo test -p ghost-launcher gatekeeper_v3
cargo test -p ghost-launcher v3_shadow
cargo test -p ghost-launcher --test session_lifecycle_tests v3
cargo test -p ghost-launcher --test gatekeeper_v25_regression
```

Zakres pokrycia:

- serde/defaulty `MaterializedFeatureSet`
- aktywny `reason_code_version = 2`
- V3 typed reason codes
- additive `GatekeeperBuyLog` schema v20
- pure evaluator V3
- sidecar log enrichment
- brak regresji V2.5

## Raporty Offline

Wykonano i przeszlo:

```bash
python3 -m unittest scripts/test_shadow_run_report.py
python3 -m unittest scripts/test_v3_shadow_report.py
```

Przed swiezym runem `v3_shadow_report.py` zwrocil `status: "no_rows"`, co bylo oczekiwane po odsunieciu starych artefaktow.

Finalny zapisany snapshot raportu:

```text
/root/Gho/logs/rollout/shadow-burnin-v25-repair-r2/reports/v3_p0_shadow_report_20260514T090916Z.json
```

Finalny wynik raportu:

```text
status = ok
raw_rows = 72
deduped_rows = 72
v3_rows = 72
bad_rows = 0
duplicate_rows_removed = 0
execution.success_count = 0
execution.outcomes.missing = 72
```

Interpretacja: raport nie traktuje `submitted/no_dispatch/no_execution/missing` jako sukcesu; dla P0 shadow/evidence `missing=72` jest zgodne z brakiem live execution/promotion proof.

## Runtime

Uruchomiono:

```bash
timeout 30m env RUST_LOG=info \
cargo run --release -p ghost-launcher --bin ghost-launcher -- \
  --config /root/Gho/configs/rollout/shadow-burnin.toml
```

Potwierdzone z logow startu:

- `execution_mode=Shadow`
- `entry_mode=shadow_only`
- `Decision Logger: AKTYWNY`
- event stream dzialal
- Seer emitowal `PoolTransaction`
- snapshoty byly zapisywane
- runtime zakonczyl sie przez `timeout`

Po runie nie zostaly aktywne procesy:

- brak `ghost-launcher`
- brak `cargo run`
- brak `target/release/ghost-launcher`
- brak `timeout 30m`

## Kontrole JSONL

Glowny plik decyzji V2.5/V3 sidecar:

```text
/root/Gho/logs/rollout/shadow-burnin-v25-repair-r2/decisions/shadow-burnin-v25-repair-r2/v2.5/v25_shadow/05d5df619448d740abf4e6cde740d027837b7d75bc091cf93f078354faf29f68/gatekeeper_v2_decisions.jsonl
```

Wynik semantycznego checka:

```text
V3 P0 JSONL semantic checks OK: 72
decision_planes: ['v25_shadow']
reason_code_versions: [2]
```

Sprawdzone warunki:

- istnieja rows z `v3_shadow_*`
- wszystkie V3 rows maja `reason_code_version == 2`
- zadna V3 row nie ma `decision_plane == "v3_shadow"`
- kazda V3 row ma `v3_shadow_reason_code`
- kazda V3 row ma `v3_shadow_notes.p0 == "shadow_only"`

Aktywne reason codes:

```text
REJECT_PDD_ENTRY_DRIFT = 51
REJECT_PDD_FLASH_CRASH = 2
REJECT_PDD_RAMPING = 2
REJECT_PDD_SPIKE = 1
REJECT_PDD_WHALE = 16
```

V3 sidecar reason codes:

```text
PENDING_V3_WAIT_EVIDENCE = 12
PENDING_V3_WAIT_SAMPLE = 2
REJECT_V3_LOW_ORGANIC_BROADENING = 1
REJECT_V3_MANIPULATION_CONTRADICTION = 57
```

Active vs V3 verdict:

```text
Active REJECT -> V3 PENDING = 14
Active REJECT -> V3 REJECT = 58
```

## Artefakty Reliktu

Decision logs:

```text
/root/Gho/logs/rollout/shadow-burnin-v25-repair-r2/decisions
```

V2.5/V3 sidecar JSONL:

```text
/root/Gho/logs/rollout/shadow-burnin-v25-repair-r2/decisions/shadow-burnin-v25-repair-r2/v2.5/v25_shadow/05d5df619448d740abf4e6cde740d027837b7d75bc091cf93f078354faf29f68/gatekeeper_v2_decisions.jsonl
```

Legacy mirror JSONL:

```text
/root/Gho/logs/rollout/shadow-burnin-v25-repair-r2/decisions/shadow-burnin-v25-repair-r2/v2.2/legacy_live/05d5df619448d740abf4e6cde740d027837b7d75bc091cf93f078354faf29f68/gatekeeper_v2_decisions.jsonl
```

Coverage audit:

```text
/root/Gho/logs/rollout/shadow-burnin-v25-repair-r2/decisions/seer_runtime_coverage_audit.jsonl
```

Formal JSON report snapshot:

```text
/root/Gho/logs/rollout/shadow-burnin-v25-repair-r2/reports/v3_p0_shadow_report_20260514T090916Z.json
```

System log:

```text
/root/Gho/logs/rollout/shadow-burnin-v25-repair-r2/system.log.2026-05-14
```

Oracle log:

```text
/root/Gho/logs/rollout/shadow-burnin-v25-repair-r2/oracle.log.2026-05-14
```

Event dataset:

```text
/root/Gho/datasets/events/shadow-burnin-v25-repair-r2
```

Snapshot data:

```text
/root/Gho/data/rollout/shadow-burnin-v25-repair-r2/snapshots
```

Shadow run dir:

```text
/root/Gho/logs/shadow_run/shadow-burnin-v25-repair-r2
```

Pre-run archive:

```text
/root/Gho/_archive_p0_validation_20260514T083840Z
```

## Rozmiary Artefaktow

```text
236M  /root/Gho/logs/rollout/shadow-burnin-v25-repair-r2
4.0K  /root/Gho/logs/shadow_run/shadow-burnin-v25-repair-r2
148K  /root/Gho/datasets/events/shadow-burnin-v25-repair-r2
132K  /root/Gho/data/rollout/shadow-burnin-v25-repair-r2
44K   /root/Gho/_archive_p0_validation_20260514T083840Z
```

Event files:

```text
exec_launcher-1778748296459_20260514_084456_0000.jsonl 0
exec_launcher-1778748296506_20260514_084456_0000.jsonl 21181
exec_launcher-1778748296506_20260514_084959_0001.jsonl 21641
exec_launcher-1778748296506_20260514_085500_0002.jsonl 24220
exec_launcher-1778748296506_20260514_090003_0003.jsonl 29962
exec_launcher-1778748296506_20260514_090517_0004.jsonl 21137
```

Snapshot files: `3`.

## Konkluzja

P0 V3 shadow/evidence jest formalnie zwalidowane dla tego 30-minutowego runu:

- testy kontraktowe przeszly
- runtime generowal dane
- raport V3 ma `status: ok`
- `v3_rows > 0`
- JSONL checks przeszly
- V3 nie jest routed plane
- aktywny `reason_code_version` pozostal `2`
- V3 sidecar nie zmienil aktywnego `decision_plane`, `verdict_type` ani `reason_code`
- brak aktywnych pozostalosci procesowych po runie

Ograniczenie wyniku: dirty config `ghost-brain/ghost_brain_config.toml` z lokalna zmiana `min_market_cap_sol = 30.0`.

## Clean Rerun Follow-Up

Dirty-config caveat zostal domkniety osobnym clean-rerun artefaktem po jawnym zaakceptowaniu `min_market_cap_sol = 30.0` jako rollout baseline:

```text
/root/Gho/PLANS/AUDYT/WALIDACJA_P0_V3_SHADOW_EVIDENCE_CLEAN_RERUN_20260514.md
```

Clean rerun wynik:

```text
status = ok
raw_rows = 141
v3_rows = 141
execution.success_count = 0
decision_plane = v25_shadow only
reason_code_version = 2 only
```

Wniosek: czysty baseline P0 shadow/evidence jest GO; nie oznacza to promotion readiness ani przejscia do P2.
