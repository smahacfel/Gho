# RAPORT OPERACYJNY P3.2 V3 FULL REPLAY R3 - 2026-05-16

## Status

**NO-GO / fail-closed.**

P3.2 r3 zostal uruchomiony w nowym namespace po remediacji hash boundary, bez reuse r2:

- rollout config: `configs/rollout/shadow-burnin-v3-p32-replay-r3.toml`
- artifact namespace: `shadow-burnin-v3-p32-replay-r3`
- decision log: `logs/rollout/shadow-burnin-v3-p32-replay-r3/decisions/shadow-burnin-v3-p32-replay-r3/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl`

Run wygenerowal swieze V3 rows z payloadem, ale strict full replay nadal nie przeszedl.

## Runtime Closure

Po uplywie okna runu nie zostal znaleziony aktywny proces:

```bash
pgrep -af 'ghost-launcher|cargo run|target/release/ghost-launcher'
```

Wynik zawieral tylko samo polecenie `pgrep`, bez aktywnego `ghost-launcher`.

Decision log jest swiezy wzgledem rollout configu:

```text
decision log mtime: 2026-05-16 13:28:51 +0000
rollout config mtime: 2026-05-16 12:58:39 +0000
```

## Strict Full Replay

Polecenie:

```bash
python3 scripts/v3_full_replay_report.py \
  --config configs/rollout/shadow-burnin-v3-p32-replay-r3.toml \
  --strict \
  --json
```

Wynik:

- exit code: `2`
- `status=fail_closed`
- `replay_status=fail_closed`
- `total_rows=111`
- `bad_rows=0`
- `v3_rows=111`
- `status_counts.payload_hash_mismatch=111`

Pierwszy mismatch:

```text
line_number=1
ab_record_id=9yXbeyLh7Gz9yCVBGeBvi8LCPL5mWQB99y8J8ugu5b6Y:1778936883456:1778936885456:REJECT
expected=454de56dd48938fdfc36ac7ccdc6324512b0dbc7025756f7ee3e70eb844c72a6
recomputed=5fcb00f84c83aa13ffc1fe1e10a144ef4ae566997cec89bda0dbd585a97bf207
```

Klasyfikacja bledu:

- `payload_hash_mismatch`: **111/111**
- `policy_hash_mismatch`: `0`
- `stage_mismatch`: `0` observed before hash gate; validator stops at payload hash
- `reason_mismatch`: `0` observed before hash gate; validator stops at payload hash
- `score_mismatch`: `0` observed before hash gate; validator stops at payload hash

Interpretacja: r3 nie dowodzi full replay readiness. Bloker pozostaje w granicy hash payloadu albo w tym, jaki binary/runtime faktycznie emituje `v3_feature_snapshot_hash` podczas runu.

## Shadow Report

Polecenie:

```bash
python3 scripts/v3_shadow_report.py \
  --config configs/rollout/shadow-burnin-v3-p32-replay-r3.toml \
  --json
```

Wynik:

- `status=ok`
- `replay_status=full`
- `artifact_freshness.stale_against_config=false`
- `counts.raw_rows=111`
- `counts.deduped_rows=111`
- `counts.v3_rows=111`
- `replay.full_snapshot_payload_rows=111`
- `replay.hash_only_rows=0`
- `hash_coverage.v3_feature_snapshot_hash.coverage=1.0`
- `hash_coverage.v3_policy_config_hash.coverage=1.0`
- `hash_consistency.policy_hash_unique_count=1`
- `hash_consistency.snapshot_hash_unique_count=111`
- `pre_dedupe_conflicts.conflict_groups=0`

V3 reason distribution:

- `REJECT_V3_MANIPULATION_CONTRADICTION`: `73`
- `PENDING_V3_WAIT_EVIDENCE`: `27`
- `PENDING_V3_WAIT_SAMPLE`: `6`
- `REJECT_V3_LOW_ORGANIC_BROADENING`: `5`

Shadow report potwierdza swieze payload rows, ale nie jest gate'em full replay. Decydujacy pozostaje `v3_full_replay_report.py --strict`, ktory zakonczyl sie fail-closed.

## Verdict

**P3.2 r3: NO-GO.**

Nie przechodzimy do P3.3 counterfactual ablation ani multi-run stability, bo strict replay nie osiagnal `full_replay_ok`.

Nie uruchamiac P2. Nie tunowac progow ani scoringu. Nastepny krok powinien byc techniczna diagnostyka konkretnego `payload_hash_mismatch` na r3: porownac hash emitowany przez runtime z helperem `v3_feature_snapshot_hash_from_payload()` na tym samym pierwszym row i ustalic, czy r3 uruchomil binary z remediacja, czy nadal loguje hash liczony ze starej reprezentacji.
