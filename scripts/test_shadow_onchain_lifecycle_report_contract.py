#!/usr/bin/env python3
from __future__ import annotations

import argparse
import contextlib
from datetime import datetime, timezone
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import shadow_onchain_lifecycle_report as report


REPO_ROOT = Path(__file__).resolve().parents[1]
FIXTURE_PATH = (
    REPO_ROOT / "tests" / "fixtures" / "shadow_lifecycle" / "raportneu_sample.json"
)
REQUIRED_ROW_FIELDS = {
    "candidate_id",
    "close_reason",
    "entry_execution_ts_ms",
    "close_ts_ms",
    "position_duration_ms",
    "entry_price_logged",
    "effective_exit_price_sol",
    "final_pnl_pct",
    "fills",
}
REQUIRED_FILL_FIELDS = {
    "fill_index",
    "target_sample_slot",
    "shadow_exit_vs_onchain_executable_pct",
    "shadow_exit_vs_onchain_spot_pct",
}


def load_fixture() -> list[dict[str, object]]:
    payload = json.loads(FIXTURE_PATH.read_text(encoding="utf-8"))
    assert isinstance(payload, list)
    return payload


def full_lifecycle_row(*, exit_fills: list[dict[str, object]] | None = None) -> dict[str, object]:
    return {
        "schema_version": 1,
        "analysis_status": "ok",
        "candidate_id": "mint_pool_1700000000000",
        "position_id": "pool:mint:1700000000100",
        "mint_id": "mint",
        "pool_id": "pool",
        "close_reason": "Target",
        "truth_status": "resolved",
        "truth_source": "canonical_account_state_snapshot",
        "sample_price_state": "Valid",
        "timing": {
            "curve_t0_event_ts_ms": 1699999999000,
            "entry_execution_ts_ms": 1700000000100,
            "close_ts_ms": 1700000000900,
            "position_duration_ms": 800,
        },
        "shadow": {
            "entry_price_logged": 0.00000007,
            "effective_exit_price_sol": 0.00000012,
            "final_pnl_pct": 60.0,
        },
        "onchain": {
            "entry": {
                "match_slot": 12345,
            },
        },
        "exit_fills": exit_fills
        if exit_fills is not None
        else [
            {
                "fill_index": 1,
                "target_sample_slot": 12346,
                "shadow_exit_vs_onchain_executable_pct": -0.001,
                "shadow_exit_vs_onchain_spot_pct": -1.0,
            }
        ],
    }


def iso_ms(timestamp_ms: int) -> str:
    return datetime.fromtimestamp(timestamp_ms / 1000, tz=timezone.utc).isoformat(
        timespec="milliseconds"
    ).replace("+00:00", "Z")


def write_jsonl(path: Path, rows: list[dict[str, object]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        "".join(json.dumps(row, sort_keys=False) + "\n" for row in rows),
        encoding="utf-8",
    )


def write_config(path: Path, root: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        f"""
[oracle]
decision_log_path = "{root / "decisions"}"

[trigger.shadow_run]
output_path = "{root / "buys.jsonl"}"

[execution.shadow]
entry_log_path = "{root / "shadow_entries.jsonl"}"
lifecycle_log_path = "{root / "shadow_lifecycle.jsonl"}"

[execution.events]
output_dir = "{root / "events"}"

[logging]
file_path = "{root / "system.log"}"
""".lstrip(),
        encoding="utf-8",
    )


def diag_line(timestamp_ms: int, mint: str, slot: int, finality: str = "finalized") -> str:
    return (
        f"{iso_ms(timestamp_ms)} INFO DIAG_ACCOUNT_UPDATE_RELAY "
        f"base_mint={mint} bonding_curve=bc_{mint} slot={slot} "
        "sol_reserves=1000000000 token_reserves=100000000000000 "
        f"complete=0 curve_finality={finality}\n"
    )


def build_report_fixture(root: Path, *, max_truth_gap_ms: int | None = None) -> dict[str, object]:
    base = 1_700_000_000_000
    config_path = root / "configs" / "shadow.toml"
    output_path = root / "report.jsonl"
    write_config(config_path, root)
    (root / "events").mkdir(parents=True, exist_ok=True)

    good_candidate = f"mint_pool_{base + 1000}"
    future_candidate = f"mintfuture_poolfuture_{base + 1100}"
    error_candidate = f"minterr_poolerr_{base + 900}"

    write_jsonl(
        root / "buys.jsonl",
        [
            {
                "candidate_id": good_candidate,
                "base_mint": "mint",
                "pool_amm_id": "pool",
                "decision_ts_ms": base + 1900,
                "sim_started_ts_ms": base + 1950,
                "sim_finished_ts_ms": base + 2000,
                "amount_lamports": 7_000_000,
                "ab_record_id": "ab-1",
                "v3_feature_snapshot_hash": "mfs-hash",
                "v3_policy_config_hash": "policy-hash",
                "decision_plane": "v25_shadow",
                "rollout_namespace": "unit-ns",
            },
            {
                "candidate_id": future_candidate,
                "base_mint": "mintfuture",
                "pool_amm_id": "poolfuture",
                "decision_ts_ms": base + 1900,
                "sim_started_ts_ms": base + 2050,
                "sim_finished_ts_ms": base + 2100,
                "amount_lamports": 7_000_000,
            },
            {
                "candidate_id": error_candidate,
                "base_mint": "minterr",
                "pool_amm_id": "poolerr",
                "decision_ts_ms": base + 1500,
                "error_class": "simulation_error",
            },
        ],
    )
    write_jsonl(
        root / "shadow_entries.jsonl",
        [
            {
                "candidate_id": good_candidate,
                "pool_id": "pool",
                "mint_id": "mint",
                "entry_price": 0.00000002,
                "slot": 10,
                "timestamp_ms": base + 2000,
                "execution_outcome": "shadow_simulated",
            },
            {
                "candidate_id": future_candidate,
                "pool_id": "poolfuture",
                "mint_id": "mintfuture",
                "entry_price": 0.00000002,
                "slot": 20,
                "timestamp_ms": base + 2100,
                "execution_outcome": "shadow_simulated",
            },
        ],
    )
    write_jsonl(
        root / "shadow_lifecycle.jsonl",
        [
            {
                "record_type": "exit_filled",
                "candidate_id": good_candidate,
                "position_id": "pool:mint:entry",
                "pool_id": "pool",
                "mint_id": "mint",
                "timestamp_ms": base + 4000,
                "sample_timestamp_ms": base + 4000,
                "sample_slot": 14,
                "sample_age_ms": 0,
                "fraction_bps": 5000,
                "remaining_fraction_bps": 5000,
                "entry_price": 0.00000002,
                "exit_price": 0.00000003,
                "entry_value_sol": 0.0035,
                "exit_value_sol": 0.004,
                "truth_status": "resolved",
                "truth_source": "canonical_account_state_snapshot",
                "sample_price_state": "Valid",
            },
            {
                "record_type": "exit_filled",
                "candidate_id": good_candidate,
                "position_id": "pool:mint:entry",
                "pool_id": "pool",
                "mint_id": "mint",
                "timestamp_ms": base + 5000,
                "sample_timestamp_ms": base + 5000,
                "sample_slot": 15,
                "sample_age_ms": 0,
                "fraction_bps": 5000,
                "remaining_fraction_bps": 0,
                "entry_price": 0.00000002,
                "exit_price": 0.00000003,
                "entry_value_sol": 0.0035,
                "exit_value_sol": 0.004,
                "truth_status": "resolved",
                "truth_source": "canonical_account_state_snapshot",
                "sample_price_state": "Valid",
            },
            {
                "record_type": "position_closed",
                "candidate_id": good_candidate,
                "position_id": "pool:mint:entry",
                "pool_id": "pool",
                "mint_id": "mint",
                "timestamp_ms": base + 5000,
                "sample_timestamp_ms": base + 5000,
                "sample_slot": 15,
                "entry_price": 0.00000002,
                "entry_value_sol": 0.007,
                "exit_value_sol": 0.008,
                "gross_pnl_sol": 0.001,
                "net_pnl_sol": 0.001,
                "estimated_costs_sol": 0.0,
                "final_pnl": 0.001,
                "final_pnl_pct": 14.285714,
                "duration_ms": 3000,
                "close_reason": "Target",
                "total_exits": 2,
                "truth_status": "resolved",
                "truth_source": "canonical_account_state_snapshot",
                "sample_price_state": "Valid",
            },
            {
                "record_type": "exit_filled",
                "candidate_id": future_candidate,
                "position_id": "poolfuture:mintfuture:entry",
                "pool_id": "poolfuture",
                "mint_id": "mintfuture",
                "timestamp_ms": base + 5000,
                "sample_timestamp_ms": base + 5000,
                "sample_slot": 25,
                "sample_age_ms": 0,
                "fraction_bps": 10000,
                "remaining_fraction_bps": 0,
                "entry_price": 0.00000002,
                "exit_price": 0.00000003,
                "entry_value_sol": 0.007,
                "exit_value_sol": 0.008,
                "truth_status": "resolved",
                "truth_source": "canonical_account_state_snapshot",
                "sample_price_state": "Valid",
            },
            {
                "record_type": "position_closed",
                "candidate_id": future_candidate,
                "position_id": "poolfuture:mintfuture:entry",
                "pool_id": "poolfuture",
                "mint_id": "mintfuture",
                "timestamp_ms": base + 5000,
                "sample_timestamp_ms": base + 5000,
                "sample_slot": 25,
                "entry_price": 0.00000002,
                "entry_value_sol": 0.007,
                "exit_value_sol": 0.008,
                "gross_pnl_sol": 0.001,
                "net_pnl_sol": 0.001,
                "estimated_costs_sol": 0.0,
                "final_pnl": 0.001,
                "final_pnl_pct": 14.285714,
                "duration_ms": 2900,
                "close_reason": "Target",
                "total_exits": 1,
                "truth_status": "resolved",
                "truth_source": "canonical_account_state_snapshot",
                "sample_price_state": "Valid",
            },
        ],
    )
    write_jsonl(
        root / "decisions" / "gatekeeper_v2_buys.jsonl",
        [
            {
                "base_mint": "mint",
                "pool_id": "pool",
                "first_seen_ts_ms": base + 900,
                "observation_start_ts_ms": base + 900,
                "observation_end_ts_ms": base + 1800,
                "curve_t0_event_ts_ms": base + 800,
                "timestamp": iso_ms(base + 1800),
                "shadow_execution_outcome": "shadow_simulated",
                "decision_verdict_buy": True,
                "verdict_type": "BUY",
                "decision_reason": "unit",
            }
        ],
    )
    (root / "system.log").write_text(
        "".join(
            [
                diag_line(base + 1500, "mint", 11),
                diag_line(base + 3500, "mint", 13),
                diag_line(base + 4500, "mint", 14),
                diag_line(base + 6000, "mint", 16),
                diag_line(base + 2600, "mintfuture", 21),
            ]
        ),
        encoding="utf-8",
    )

    args = argparse.Namespace(
        config=config_path,
        output=output_path,
        outcome_summary_output=root / "compact.json",
        manifest_output=None,
        summary_output=None,
        emit_skipped_rows="",
        label_output=None,
        label_summary_output=None,
        label_summary_md_output=None,
        session_start_ms=None,
        session_end_ms=None,
        all_sessions=True,
        artifact_plane="shadow",
        probe=False,
        max_truth_gap_ms=max_truth_gap_ms,
    )
    with contextlib.redirect_stdout(io.StringIO()):
        return report.run_report(args)


class ShadowOnchainLifecycleReportContractTests(unittest.TestCase):
    def test_raportneu_fixture_contract_fields(self) -> None:
        rows = load_fixture()

        self.assertGreaterEqual(len(rows), 3)
        self.assertEqual(
            {"Target", "TimeStop", "StopLoss"},
            {str(row.get("close_reason")) for row in rows},
        )
        for row in rows:
            self.assertTrue(REQUIRED_ROW_FIELDS.issubset(row), row)
            self.assertIsInstance(row["fills"], list)
            self.assertGreaterEqual(len(row["fills"]), 1)
            for fill in row["fills"]:
                self.assertIsInstance(fill, dict)
                self.assertTrue(REQUIRED_FILL_FIELDS.issubset(fill), fill)

    def test_project_outcome_summary_row_matches_fixture_shape(self) -> None:
        expected = load_fixture()[0]
        full_row = full_lifecycle_row()
        full_row["candidate_id"] = expected["candidate_id"]
        full_row["close_reason"] = expected["close_reason"]
        full_row["timing"] = {
            "curve_t0_event_ts_ms": expected["curve_t0_event_ts_ms"],
            "entry_execution_ts_ms": expected["entry_execution_ts_ms"],
            "close_ts_ms": expected["close_ts_ms"],
            "position_duration_ms": expected["position_duration_ms"],
        }
        full_row["shadow"] = {
            "entry_price_logged": expected["entry_price_logged"],
            "effective_exit_price_sol": expected["effective_exit_price_sol"],
            "final_pnl_pct": expected["final_pnl_pct"],
        }
        full_row["onchain"] = {"entry": {"match_slot": expected["match_slot"]}}
        full_row["exit_fills"] = [
            {
                "fill_index": fill["fill_index"],
                "target_sample_slot": fill["target_sample_slot"],
                "shadow_exit_vs_onchain_executable_pct": fill[
                    "shadow_exit_vs_onchain_executable_pct"
                ],
                "shadow_exit_vs_onchain_spot_pct": fill[
                    "shadow_exit_vs_onchain_spot_pct"
                ],
            }
            for fill in expected["fills"]
        ]

        self.assertEqual(expected, report.project_outcome_summary_row(full_row))

    def test_project_outcome_summary_preserves_multi_fill_contract(self) -> None:
        row = full_lifecycle_row(
            exit_fills=[
                {
                    "fill_index": 1,
                    "target_sample_slot": 222,
                    "shadow_exit_vs_onchain_executable_pct": -0.01,
                    "shadow_exit_vs_onchain_spot_pct": -1.0,
                },
                {
                    "fill_index": 2,
                    "target_sample_slot": 333,
                    "shadow_exit_vs_onchain_executable_pct": -0.02,
                    "shadow_exit_vs_onchain_spot_pct": -1.1,
                },
            ]
        )

        compact = report.project_outcome_summary_row(row)

        self.assertEqual(2, len(compact["fills"]))
        self.assertEqual(222, compact["fills"][0]["target_sample_slot"])
        self.assertEqual(333, compact["fills"][1]["target_sample_slot"])

    def test_optional_summary_write_does_not_change_jsonl_output(self) -> None:
        rows = [full_lifecycle_row()]

        with tempfile.TemporaryDirectory() as tmp_raw:
            tmp = Path(tmp_raw)
            without_flag_jsonl = tmp / "without.jsonl"
            with_flag_jsonl = tmp / "with.jsonl"
            compact_json = tmp / "raportneu.json"

            report.write_jsonl(without_flag_jsonl, rows)
            report.write_jsonl(with_flag_jsonl, rows)
            report.write_json(compact_json, report.project_outcome_summary_rows(rows))

            self.assertFalse((tmp / "without_raportneu.json").exists())
            self.assertEqual(
                without_flag_jsonl.read_text(encoding="utf-8"),
                with_flag_jsonl.read_text(encoding="utf-8"),
            )
            compact = json.loads(compact_json.read_text(encoding="utf-8"))
            self.assertEqual([report.project_outcome_summary_row(rows[0])], compact)

    def test_previous_only_truth_matching_and_neighbor_deltas(self) -> None:
        timeline = report.DiagTimeline(
            timestamps_ms=[2000],
            updates=[
                report.DiagUpdate(
                    timestamp_ms=2000,
                    base_mint="mint",
                    bonding_curve="bc",
                    slot=1,
                    sol_reserves_lamports=1,
                    token_reserves_raw=1,
                    complete=0,
                    curve_finality="finalized",
                )
            ],
        )

        self.assertIsNone(report.find_causal_truth(timeline, 1000))
        matched, prev_delta, next_delta = report.find_causal_truth_with_neighbors(timeline, 1000)
        self.assertIsNone(matched)
        self.assertIsNone(prev_delta)
        self.assertEqual(1000, next_delta)

    def test_synthetic_report_writes_manifest_summary_skips_and_labels(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_raw:
            tmp = Path(tmp_raw)
            result = build_report_fixture(tmp)
            rows = result["rows"]
            summary = result["summary"]
            manifest = result["manifest"]
            outputs = result["outputs"]

            self.assertEqual(1, len(rows))
            row = rows[0]
            self.assertEqual("shadow_burnin_lifecycle_onchain", row["truth_dataset_kind"])
            self.assertEqual("active_shadow", row["collection_plane"])
            self.assertEqual("shadow_onchain_finalized_verified", row["execution_verification_class_hint"])
            self.assertEqual("ab-1", row["ab_record_id"])
            self.assertEqual("mfs-hash", row["v3_feature_snapshot_hash"])
            self.assertEqual("policy-hash", row["v3_policy_config_hash"])
            self.assertEqual("v25_shadow", row["decision_plane"])
            self.assertEqual("unit-ns", row["rollout_namespace"])
            self.assertEqual(report.PUMP_FUN_FEE_BPS, row["consistency"]["fee_bps"])
            self.assertIsInstance(row["shadow"]["entry_price_scale_candidates"], list)
            self.assertEqual(500, row["onchain"]["entry"]["match_prev_delta_ms"])
            self.assertEqual(1500, row["onchain"]["entry"]["match_next_delta_ms"])
            self.assertEqual(2, len(row["exit_fills"]))
            self.assertEqual(500, row["exit_fills"][0]["onchain_match_prev_delta_ms"])
            self.assertEqual(500, row["exit_fills"][0]["onchain_match_next_delta_ms"])
            self.assertEqual(2, row["onchain"]["exit"]["fill_count"])

            self.assertEqual(3, summary["transport_candidates"])
            self.assertEqual(2, summary["transport_simulated"])
            self.assertEqual({"simulation_error": 1}, summary["transport_errors_by_class"])
            self.assertEqual(2, summary["entry_rows"])
            self.assertEqual(2, summary["lifecycle_candidates"])
            self.assertEqual(2, summary["position_closed"])
            self.assertEqual(1, summary["rows_written"])
            self.assertEqual({"entry_truth_future_only": 1}, summary["skipped_by_reason"])
            self.assertEqual(1, summary["denominator_breakdown"]["simulation_error"])
            self.assertEqual(1, summary["denominator_breakdown"]["no_position_closed"])

            self.assertTrue(outputs.manifest_output.exists())
            self.assertTrue(outputs.summary_output.exists())
            self.assertTrue(outputs.skipped_rows_output.exists())
            self.assertTrue(outputs.label_output.exists())
            self.assertTrue(outputs.label_summary_output.exists())
            self.assertTrue(outputs.label_summary_md_output.exists())
            self.assertEqual(str(outputs.raw_output), manifest["outputs"]["raw_jsonl"])
            self.assertEqual("ok", manifest["label_generation_status"]["status"])
            self.assertEqual(1, manifest["rows_written"])
            self.assertEqual({"entry_truth_future_only": 1}, manifest["skipped_by_reason"])

            skipped_rows = [
                json.loads(line)
                for line in outputs.skipped_rows_output.read_text(encoding="utf-8").splitlines()
            ]
            self.assertEqual(1, len(skipped_rows))
            self.assertEqual("entry_truth_future_only", skipped_rows[0]["reason"])
            self.assertEqual(500, skipped_rows[0]["match_next_delta_ms"])

            labels = [
                json.loads(line)
                for line in outputs.label_output.read_text(encoding="utf-8").splitlines()
            ]
            self.assertEqual(1, len(labels))
            self.assertEqual("shadow_burnin_lifecycle_onchain", labels[0]["truth_dataset_kind"])
            self.assertEqual("active_shadow", labels[0]["collection_plane"])
            self.assertEqual("ab-1", labels[0]["ab_record_id"])
            self.assertEqual(
                "shadow_onchain_finalized_verified",
                labels[0]["execution_verification_class_hint"],
            )

    def test_max_truth_gap_skips_rows_before_clean_output(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_raw:
            result = build_report_fixture(Path(tmp_raw), max_truth_gap_ms=100)

            self.assertEqual([], result["rows"])
            self.assertEqual(1, result["skipped"]["entry_truth_too_far"])
            self.assertEqual(1, result["skipped"]["entry_truth_future_only"])


if __name__ == "__main__":
    unittest.main()
