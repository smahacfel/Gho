# PR0: kontrakt reprodukcji baseline i feasibility

Status:

```text
PR0_REPRODUCTION_CONTRACT_V1
INPUT_MANIFEST_VERIFIED
FEASIBILITY_SUMMARY_REPRODUCIBLE_WITH_FROZEN_INPUTS
RAW_INPUTS_NOT_STORED_IN_GIT
```

Data utrwalenia: 2026-07-11

Manifest wejść:
`reports/metric_contracts/pr0_input_manifest_v1.json`

Machine-readable wynik:
`reports/metric_contracts/pr0_feasibility_summary_v1.json`

## 1. Granica reprodukcji

Surowe decision JSONL mają łącznie 3,220,314,932 bytes i celowo nie są częścią
PR #60. Reprodukcja liczb wymaga dostępu do czterech plików wymienionych w
manifeście, dokładnie pod kontrolowanymi SHA-256. GitHub-only review może
zweryfikować kontrakt, kod skanera, manifest i wynik, ale bez tych content-addressed
inputs nie może ponownie przeliczyć 31,266 rows. Dokument nie ukrywa tej granicy.

Każdy mismatch path/SHA/byte count/row count/timestamp/schema/run/config kończy
skaner kodem 2 i `input_validation.status=FAIL`. Mutable r5 jest zapisany tylko
w `excluded_inputs`; nie ma zamrożonych count/SHA i nigdy nie jest skanowany.

## 2. Provenance kodu

Audyt semantyczny wykonano na head PR #59:

```text
audited_code_commit = f3318f3a71a9202ced7af9cf43c064fa9f2f0c4a
```

Faktyczny base i merge-base PR #60:

```text
pr60_base_and_merge_base = f1e3292aae935d1b43e2c265c078f9ec74a62563
```

Komendy i wyniki:

```text
$ git rev-parse 'f3318f3a71a9202ced7af9cf43c064fa9f2f0c4a^{tree}'
92e97058349157b591a24f11da3bec0642051cd7

$ git rev-parse 'f1e3292aae935d1b43e2c265c078f9ec74a62563^{tree}'
92e97058349157b591a24f11da3bec0642051cd7

$ git merge-base origin/main HEAD
f1e3292aae935d1b43e2c265c078f9ec74a62563

$ git diff --quiet f3318f3a71a9202ced7af9cf43c064fa9f2f0c4a \
    f1e3292aae935d1b43e2c265c078f9ec74a62563 -- .
$ echo $?
0
```

Werdykt: `TREE_EQUIVALENCE_PASS`. Merge commit PR #59 i audytowany head mają
identyczne drzewo; nie utożsamia się już ich SHA ani roli provenance.

## 3. Środowisko referencyjne

```text
Python 3.12.3
git version 2.43.0
sha256sum (GNU coreutils) 9.4
scanner_version = pr0_feasibility_scanner_v1
scanner_source_sha256 = c7d38911e20b214f39eac8bceab6a138034058d9d72185a34ee576422a5c52ab
```

Skaner korzysta wyłącznie z Python standard library.

## 4. Normatywne definicje obliczeń

- `row_count`: liczba fizycznych linii odczytanych w trybie binary; malformed
  linia nadal zwiększa row count i osobno zwiększa `malformed_rows`.
- `byte_count`: `stat().st_size`, porównany z manifestem.
- SHA: SHA-256 wszystkich surowych bytes pliku, łącznie z line terminators.
- bytes/record: długość surowej linii po usunięciu wyłącznie końcowego CR/LF.
- p50/p95/p99: nearest-rank, czyli
  `sorted_values[ceil(q * n) - 1]`, z indeksem ograniczonym do `[0,n-1]`.
- min/max timestamp: minimum i maksimum poprawnie sparsowanego pola RFC3339
  `timestamp`.
- duration per run: `max_timestamp - min_timestamp`.
- aggregate duration: suma duration czterech wejść, nie długość union wall-clock.
- bytes/hour: suma byte count / suma observed duration hours.
- record identity: dokładnie `(run_id, join_key, decision_plane)`.
- duplicate record: powtórzenie pełnego record identity.
- powtórzenie samego `join_key` między różnymi runami jest tylko diagnostycznym
  `cross_run_join_key_collisions_observed`, a nie duplicate record.
- underlying-event collision: wymaga osobnego `stable_event_identity`. Pole nie
  istnieje w v33, więc wynik jest `NOT_MEASURABLE_PRE_IMPLEMENTATION`, nie zero.
- V3 full replay input: schema v1 + MFS snapshot + policy config + string hash.
- Gatekeeper V2 strict replay input: schema v1 + oba readiness bools true +
  config payload + decision payload.
- dev-known: `dev_wallet_known is true`.
- legacy flip presence: `flip_ratio_10s is not null`; nie jest to flip-v2 clean.
- create/raw/order coverage: dokładne pola zakodowane w skanerze poniżej.

## 5. Ekstrakcja i uruchomienie

Poniższa komenda wycina dokładne źródło spomiędzy markerów. Markdown fences i
markery nie wchodzą do SHA; źródło kończy dokładnie jeden LF.

```bash
python3 - <<'PY'
from pathlib import Path

doc = Path("reports/metric_contracts/pr0_reproduction_v1.md").read_text(encoding="utf-8")
begin = "<!-- PR0_SCANNER_SOURCE_BEGIN -->\n```python\n"
end = "\n```\n<!-- PR0_SCANNER_SOURCE_END -->"
source = doc.split(begin, 1)[1].split(end, 1)[0] + "\n"
Path("/tmp/pr0_feasibility_scanner_v1.py").write_text(source, encoding="utf-8")
PY

sha256sum /tmp/pr0_feasibility_scanner_v1.py
python3 -m py_compile /tmp/pr0_feasibility_scanner_v1.py
python3 /tmp/pr0_feasibility_scanner_v1.py \
  --repo-root "$PWD" \
  --manifest reports/metric_contracts/pr0_input_manifest_v1.json \
  > /tmp/pr0_feasibility_summary_v1.json
cmp -s \
  reports/metric_contracts/pr0_feasibility_summary_v1.json \
  /tmp/pr0_feasibility_summary_v1.json
echo $?
```

Oczekiwane:

```text
c7d38911e20b214f39eac8bceab6a138034058d9d72185a34ee576422a5c52ab  /tmp/pr0_feasibility_scanner_v1.py
cmp exit = 0
```

## 6. Utrwalony transkrypt testów semantycznych

Testów nie powtarzano podczas poprawki provenance. PR0 wykonał wcześniej:

| Polecenie | Wynik |
| --- | --- |
| `cargo test -p ghost-launcher ftdi_two_buy_sample_exports_degraded_diagnostic_value` | exit 0; 1 passed |
| `cargo test -p ghost-core --test tx_intelligence_contract_tests` | exit 0; 2 passed |
| `cargo test -p seer test_flip_ratio` | exit 0; 2 passed |
| `cargo test -p ghost-launcher canonical_creator_dev_buy` | exit 0; 3 passed |
| `cargo test -p ghost-launcher --test session_lifecycle_tests materialize_features_populates_` | exit 0; 7 passed |
| `cargo test -p ghost-core --test account_state_core_tests reducer_preserves_raw_reserves_but_exposes_normalized_feature_units` | exit 0; 1 passed |
| `cargo test -p ghost-launcher --test gatekeeper_policy_tests top3_` | exit 0; 3 passed |

Razem: 19 passed, 0 failed. Pierwsza próba FTDI z niepełnym filtrem `--exact`
uruchomiła zero testów i nie jest liczona jako evidence.

## 7. Źródło skanera

<!-- PR0_SCANNER_SOURCE_BEGIN -->
```python
#!/usr/bin/env python3
"""Deterministic PR0 feasibility scanner embedded in pr0_reproduction_v1.md."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
from collections import Counter
from datetime import datetime
from pathlib import Path
from typing import Any


SCANNER_VERSION = "pr0_feasibility_scanner_v1"


def nearest_rank(values: list[int], quantile: float) -> int | None:
    """Nearest-rank percentile: sorted[ceil(q*n)-1], clamped to [0, n-1]."""
    if not values:
        return None
    ordered = sorted(values)
    index = max(0, min(len(ordered) - 1, math.ceil(quantile * len(ordered)) - 1))
    return ordered[index]


def parse_timestamp(value: Any) -> datetime | None:
    if not isinstance(value, str):
        return None
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None


def one(counter: Counter[str]) -> str | None:
    return next(iter(counter)) if len(counter) == 1 else None


def audit_input(repo_root: Path, spec: dict[str, Any]) -> dict[str, Any]:
    path = repo_root / spec["path"]
    digest = hashlib.sha256()
    record_bytes: list[int] = []
    identities: set[tuple[str, str, str]] = set()
    join_keys: set[str] = set()
    stable_event_ids: set[str] = set()
    duplicate_record_identities = 0
    duplicate_join_keys_within_input = 0
    malformed_rows = 0
    missing_record_identity_rows = 0
    dev_known_rows = 0
    legacy_flip_present_rows = 0
    funding_source_v2_present_rows = 0
    materialized_snapshot_rows = 0
    v3_full_replay_input_rows = 0
    gatekeeper_v2_full_replay_input_rows = 0
    create_signature_rows = 0
    raw_tx_sequence_rows = 0
    tx_order_provenance_rows = 0
    stable_event_identity_rows = 0
    schema_counts: Counter[str] = Counter()
    run_id_counts: Counter[str] = Counter()
    decision_plane_counts: Counter[str] = Counter()
    config_hash_counts: Counter[str] = Counter()
    brain_config_hash_counts: Counter[str] = Counter()
    replay_readiness_counts: Counter[str] = Counter()
    first_timestamp: datetime | None = None
    last_timestamp: datetime | None = None
    row_count = 0

    with path.open("rb") as handle:
        for raw in handle:
            digest.update(raw)
            row_count += 1
            record_bytes.append(len(raw.rstrip(b"\r\n")))
            try:
                row = json.loads(raw)
            except (json.JSONDecodeError, UnicodeDecodeError):
                malformed_rows += 1
                continue

            schema_counts[str(row.get("log_schema_version"))] += 1
            run_id = row.get("run_id")
            join_key = row.get("join_key")
            decision_plane = row.get("decision_plane")
            run_id_counts[str(run_id)] += 1
            decision_plane_counts[str(decision_plane)] += 1
            config_hash_counts[str(row.get("config_hash"))] += 1
            brain_config_hash_counts[str(row.get("brain_config_hash"))] += 1

            if not all(isinstance(value, str) and value for value in (run_id, join_key, decision_plane)):
                missing_record_identity_rows += 1
            else:
                identity = (run_id, join_key, decision_plane)
                if identity in identities:
                    duplicate_record_identities += 1
                identities.add(identity)
                if join_key in join_keys:
                    duplicate_join_keys_within_input += 1
                join_keys.add(join_key)

            stable_event_identity = row.get("stable_event_identity")
            if isinstance(stable_event_identity, str) and stable_event_identity:
                stable_event_identity_rows += 1
                stable_event_ids.add(stable_event_identity)

            dev_known_rows += row.get("dev_wallet_known") is True
            legacy_flip_present_rows += row.get("flip_ratio_10s") is not None
            funding_source_v2_present_rows += row.get("funding_source_v2") is not None
            materialized_snapshot = row.get("materialized_feature_snapshot")
            materialized_snapshot_rows += materialized_snapshot is not None

            v3_full_replay_input_rows += (
                row.get("v3_replay_payload_schema_version") == 1
                and row.get("v3_materialized_feature_snapshot") is not None
                and row.get("v3_policy_config_payload") is not None
                and isinstance(row.get("v3_feature_snapshot_hash"), str)
            )
            gatekeeper_ready = (
                row.get("gatekeeper_v2_replay_input_schema_version") == 1
                and row.get("gatekeeper_v2_replay_ready_non_temporal") is True
                and row.get("gatekeeper_v2_replay_ready_temporal") is True
                and row.get("gatekeeper_v2_config_payload") is not None
                and row.get("gatekeeper_decision_payload") is not None
            )
            gatekeeper_v2_full_replay_input_rows += gatekeeper_ready
            readiness_key = "|".join(
                (
                    f"schema={row.get('gatekeeper_v2_replay_input_schema_version')}",
                    f"non_temporal={row.get('gatekeeper_v2_replay_ready_non_temporal')}",
                    f"temporal={row.get('gatekeeper_v2_replay_ready_temporal')}",
                    f"config={row.get('gatekeeper_v2_config_payload') is not None}",
                    f"decision={row.get('gatekeeper_decision_payload') is not None}",
                    f"reason={row.get('gatekeeper_v2_replay_incomplete_reason')}",
                )
            )
            replay_readiness_counts[readiness_key] += 1

            create_signature_rows += bool(row.get("pool_create_signature") or row.get("create_signature"))
            snapshot = materialized_snapshot if isinstance(materialized_snapshot, dict) else {}
            raw_tx_sequence_rows += any(
                snapshot.get(name) is not None
                for name in ("transactions", "tx_sequence", "raw_transactions")
            )
            tx_order_provenance_rows += any(
                row.get(name) is not None
                for name in ("tx_key", "transaction_index", "event_ordinal", "source_order_key")
            )

            timestamp = parse_timestamp(row.get("timestamp"))
            if timestamp is not None:
                first_timestamp = timestamp if first_timestamp is None or timestamp < first_timestamp else first_timestamp
                last_timestamp = timestamp if last_timestamp is None or timestamp > last_timestamp else last_timestamp

    duration_seconds = (
        (last_timestamp - first_timestamp).total_seconds()
        if first_timestamp is not None and last_timestamp is not None
        else None
    )
    result = {
        "input_id": spec["input_id"],
        "path": spec["path"],
        "basename": path.name,
        "classification": spec["classification"],
        "sha256": digest.hexdigest(),
        "byte_count": path.stat().st_size,
        "row_count": row_count,
        "malformed_rows": malformed_rows,
        "min_timestamp": first_timestamp.isoformat() if first_timestamp else None,
        "max_timestamp": last_timestamp.isoformat() if last_timestamp else None,
        "duration_seconds": duration_seconds,
        "duration_hours": duration_seconds / 3600 if duration_seconds is not None else None,
        "schema_counts": dict(sorted(schema_counts.items())),
        "run_id_counts": dict(sorted(run_id_counts.items())),
        "decision_plane_counts": dict(sorted(decision_plane_counts.items())),
        "config_hash_counts": dict(sorted(config_hash_counts.items())),
        "brain_config_hash_counts": dict(sorted(brain_config_hash_counts.items())),
        "unique_record_identities": len(identities),
        "missing_record_identity_rows": missing_record_identity_rows,
        "duplicate_record_identities": duplicate_record_identities,
        "unique_join_keys": len(join_keys),
        "duplicate_join_keys_within_input": duplicate_join_keys_within_input,
        "stable_event_identity_rows": stable_event_identity_rows,
        "dev_known_rows": dev_known_rows,
        "legacy_flip_present_rows": legacy_flip_present_rows,
        "funding_source_v2_present_rows": funding_source_v2_present_rows,
        "materialized_snapshot_rows": materialized_snapshot_rows,
        "v3_full_replay_input_rows": v3_full_replay_input_rows,
        "gatekeeper_v2_full_replay_input_rows": gatekeeper_v2_full_replay_input_rows,
        "gatekeeper_v2_readiness_counts": dict(sorted(replay_readiness_counts.items())),
        "create_signature_rows": create_signature_rows,
        "raw_tx_sequence_rows": raw_tx_sequence_rows,
        "tx_order_provenance_rows": tx_order_provenance_rows,
        "record_bytes": {
            "min": min(record_bytes) if record_bytes else None,
            "p50_nearest_rank": nearest_rank(record_bytes, 0.50),
            "p95_nearest_rank": nearest_rank(record_bytes, 0.95),
            "p99_nearest_rank": nearest_rank(record_bytes, 0.99),
            "max": max(record_bytes) if record_bytes else None,
            "mean": sum(record_bytes) / len(record_bytes) if record_bytes else None,
        },
        "bytes_per_observed_hour": (
            path.stat().st_size / (duration_seconds / 3600)
            if duration_seconds is not None and duration_seconds > 0
            else None
        ),
    }
    result["_identities"] = identities
    result["_join_keys"] = join_keys
    result["_stable_event_ids"] = stable_event_ids
    result["_record_bytes"] = record_bytes
    return result


def expected_mismatches(spec: dict[str, Any], result: dict[str, Any]) -> list[dict[str, Any]]:
    mismatches = []
    for field, expected in spec["expected"].items():
        actual = result.get(field)
        if actual != expected:
            mismatches.append(
                {"input_id": spec["input_id"], "field": field, "expected": expected, "actual": actual}
            )
    return mismatches


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo-root", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    args = parser.parse_args()

    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    specs = manifest["inputs"]
    results = [audit_input(args.repo_root, spec) for spec in specs]
    mismatches = [item for spec, result in zip(specs, results) for item in expected_mismatches(spec, result)]

    global_identities: set[tuple[str, str, str]] = set()
    seen_join_keys: set[str] = set()
    seen_stable_event_ids: set[str] = set()
    cross_run_duplicate_record_identities = 0
    cross_run_join_key_collisions_observed = 0
    cross_run_underlying_event_collisions = 0
    all_record_bytes: list[int] = []
    for result in results:
        for identity in result.pop("_identities"):
            cross_run_duplicate_record_identities += identity in global_identities
            global_identities.add(identity)
        for join_key in result.pop("_join_keys"):
            cross_run_join_key_collisions_observed += join_key in seen_join_keys
            seen_join_keys.add(join_key)
        for event_id in result.pop("_stable_event_ids"):
            cross_run_underlying_event_collisions += event_id in seen_stable_event_ids
            seen_stable_event_ids.add(event_id)
        all_record_bytes.extend(result.pop("_record_bytes"))

    rows = sum(result["row_count"] for result in results)
    duration_hours = sum(result["duration_hours"] or 0 for result in results)
    byte_count = sum(result["byte_count"] for result in results)
    stable_event_identity_rows = sum(result["stable_event_identity_rows"] for result in results)
    public_runs = [
        {
            key: result[key]
            for key in (
                "input_id",
                "path",
                "classification",
                "sha256",
                "byte_count",
                "row_count",
                "malformed_rows",
                "min_timestamp",
                "max_timestamp",
                "duration_seconds",
                "duration_hours",
                "schema_counts",
                "run_id_counts",
                "decision_plane_counts",
                "config_hash_counts",
                "brain_config_hash_counts",
                "unique_record_identities",
                "missing_record_identity_rows",
                "duplicate_record_identities",
                "unique_join_keys",
                "duplicate_join_keys_within_input",
                "stable_event_identity_rows",
                "dev_known_rows",
                "legacy_flip_present_rows",
                "funding_source_v2_present_rows",
                "materialized_snapshot_rows",
                "v3_full_replay_input_rows",
                "gatekeeper_v2_full_replay_input_rows",
                "gatekeeper_v2_readiness_counts",
                "create_signature_rows",
                "raw_tx_sequence_rows",
                "tx_order_provenance_rows",
                "record_bytes",
                "bytes_per_observed_hour",
            )
        }
        for result in results
    ]
    summary = {
        "$schema": "ghost/pr0_feasibility_summary_v1",
        "schema_version": 1,
        "summary_id": "metric_contract_pr0_feasibility_v1",
        "classification": "FEASIBILITY_ONLY",
        "scanner": manifest["scanner"],
        "input_manifest": {
            "path": manifest["artifact_path"],
            "manifest_id": manifest["manifest_id"],
        },
        "provenance": manifest["provenance"],
        "input_validation": {"status": "PASS" if not mismatches else "FAIL", "mismatches": mismatches},
        "identity_contract": {
            "record_identity": ["run_id", "join_key", "decision_plane"],
            "duplicate_record_identity_is_failure": True,
            "cross_run_join_key_collision_is_record_duplicate": False,
            "underlying_event_identity_field": "stable_event_identity",
            "underlying_event_collision_status": (
                "EVALUATED" if stable_event_identity_rows == rows else "NOT_MEASURABLE_PRE_IMPLEMENTATION"
            ),
        },
        "aggregate": {
            "run_count": len(results),
            "observed_duration_hours_sum": duration_hours,
            "row_count": rows,
            "unique_record_identities": len(global_identities),
            "missing_record_identity_rows": sum(result["missing_record_identity_rows"] for result in results),
            "duplicate_record_identities_within_inputs": sum(result["duplicate_record_identities"] for result in results),
            "duplicate_record_identities_across_inputs": cross_run_duplicate_record_identities,
            "unique_join_keys_sum": sum(result["unique_join_keys"] for result in results),
            "cross_run_join_key_collisions_observed": cross_run_join_key_collisions_observed,
            "stable_event_identity_rows": stable_event_identity_rows,
            "cross_run_underlying_event_collisions": (
                cross_run_underlying_event_collisions if stable_event_identity_rows == rows else None
            ),
            "malformed_rows": sum(result["malformed_rows"] for result in results),
            "dev_known_rows": sum(result["dev_known_rows"] for result in results),
            "legacy_flip_present_rows": sum(result["legacy_flip_present_rows"] for result in results),
            "funding_source_v2_present_rows": sum(result["funding_source_v2_present_rows"] for result in results),
            "materialized_snapshot_rows": sum(result["materialized_snapshot_rows"] for result in results),
            "v3_full_replay_input_rows": sum(result["v3_full_replay_input_rows"] for result in results),
            "gatekeeper_v2_full_replay_input_rows": sum(result["gatekeeper_v2_full_replay_input_rows"] for result in results),
            "create_signature_rows": sum(result["create_signature_rows"] for result in results),
            "raw_tx_sequence_rows": sum(result["raw_tx_sequence_rows"] for result in results),
            "tx_order_provenance_rows": sum(result["tx_order_provenance_rows"] for result in results),
            "byte_count": byte_count,
            "bytes_per_observed_hour": byte_count / duration_hours if duration_hours > 0 else None,
            "records_per_observed_hour": rows / duration_hours if duration_hours > 0 else None,
            "record_bytes": {
                "min": min(all_record_bytes) if all_record_bytes else None,
                "p50_nearest_rank": nearest_rank(all_record_bytes, 0.50),
                "p95_nearest_rank": nearest_rank(all_record_bytes, 0.95),
                "p99_nearest_rank": nearest_rank(all_record_bytes, 0.99),
                "max": max(all_record_bytes) if all_record_bytes else None,
                "mean": sum(all_record_bytes) / len(all_record_bytes) if all_record_bytes else None,
            },
        },
        "effective_config_equivalence": {
            "gatekeeper_config_hashes": sorted({one(Counter(result["config_hash_counts"])) for result in results}),
            "brain_config_hashes": sorted({one(Counter(result["brain_config_hash_counts"])) for result in results}),
            "metric_contract_effective_config_hash": None,
            "status": "NOT_EMITTED_PRE_IMPLEMENTATION",
            "burn_in_equivalence_conclusion": "NOT_MEASURABLE_PRE_IMPLEMENTATION",
        },
        "feasibility": {
            "duration_decisions_dev_known_scale": "FEASIBLE",
            "clean_flip_v2_evaluable": "NOT_MEASURABLE_PRE_IMPLEMENTATION",
            "real_dev_legacy_v2_divergence": "NOT_MEASURABLE_PRE_IMPLEMENTATION",
            "burn_in_contract_v1": "NOT_YET_FROZEN",
            "validation_evidence": False,
        },
        "runs": public_runs,
    }
    print(json.dumps(summary, indent=2, sort_keys=True, ensure_ascii=False))
    return 0 if not mismatches else 2


if __name__ == "__main__":
    raise SystemExit(main())
```
<!-- PR0_SCANNER_SOURCE_END -->

## 8. Werdykt reprodukcji

```text
INPUT_HASH_AND_SHAPE_VALIDATION = PASS
MACHINE_READABLE_SUMMARY_MATCH = PASS
GITHUB_ONLY_RAW_RECOMPUTATION = NOT_POSSIBLE_WITHOUT_CONTENT_ADDRESSED_INPUTS
PR0_REPRODUCIBILITY = PASS_WITH_DECLARED_INPUT_AVAILABILITY_BOUNDARY
```
