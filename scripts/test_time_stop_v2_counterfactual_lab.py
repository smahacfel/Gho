#!/usr/bin/env python3
from __future__ import annotations

import json
import contextlib
import io
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

import time_stop_v2_counterfactual_lab as lab


RUN_ID = "test-run"
SESSION_ID = "test-session"


def replay_row(
    *,
    pool_id: str = "pool",
    base_mint: str = "mint",
    entry_ts_ms: int = 1_000,
    first_hit_ms: dict[str, int] | None = None,
    path_bps: list[list[int]] | None = None,
    last_pnl_bps: int = 0,
    levels_bps: list[int] | None = None,
) -> dict:
    return {
        "schema": "shadow_exit_replay_v1",
        "run_id": RUN_ID,
        "session_id": SESSION_ID,
        "candidate_id": f"{base_mint}_{pool_id}_{entry_ts_ms}",
        "position_id": f"{pool_id}:{base_mint}:{entry_ts_ms}",
        "pool_id": pool_id,
        "base_mint": base_mint,
        "entry_ts_ms": entry_ts_ms,
        "entry_price": 1.0,
        "entry_source": "shadow_simulated",
        "horizon_ms": 120_000,
        "close_age_ms": 120_000,
        "levels_bps": levels_bps or [-6000, 6000],
        "first_hit_ms": first_hit_ms or {},
        "mfe_bps": None,
        "mae_bps": None,
        "last_pnl_bps": last_pnl_bps,
        "path_bps": path_bps or [[0, 0]],
        "quality": "clean",
        "truncated": False,
    }


def window_row(
    *,
    pool_id: str = "pool",
    base_mint: str = "mint",
    entry_ts_ms: int = 1_000,
    age_ms: int,
    index: int,
    status: str = "heartbeat",
    subreason: str = "micro_tx_heartbeat_no_price_progress",
    candidate: bool = False,
    price_delta_from_entry_pct: float | None = None,
    volume_delta_sol: float = 0.01,
) -> dict:
    return {
        "record_type": "time_stop_v2_window",
        "run_id": RUN_ID,
        "session_id": SESSION_ID,
        "candidate_id": f"{base_mint}_{pool_id}_{entry_ts_ms}",
        "position_id": f"{pool_id}:{base_mint}:{entry_ts_ms}",
        "pool_id": pool_id,
        "mint_id": base_mint,
        "timestamp_ms": entry_ts_ms + age_ms,
        "time_stop_v2_window_index": index,
        "time_stop_v2_position_age_ms": age_ms,
        "time_stop_v2_scheduled_check_ms": entry_ts_ms + age_ms,
        "time_stop_v2_status": status,
        "time_stop_v2_subreason": subreason,
        "time_stop_v2_failed_windows": index + 1,
        "time_stop_v2_candidate": candidate,
        "time_stop_v2_price_delta_pct_from_entry": price_delta_from_entry_pct,
        "time_stop_v2_price_delta_pct_window": 0.1,
        "time_stop_v2_mcap_delta_pct_window": 0.1,
        "time_stop_v2_bonding_delta_pct_window": 0.01,
        "time_stop_v2_volume_delta_sol_window": volume_delta_sol,
    }


def terminal_row(
    *,
    pool_id: str = "pool",
    base_mint: str = "mint",
    entry_ts_ms: int = 1_000,
    age_ms: int = 60_000,
    close_reason: str = "StopLoss",
    final_pnl_pct: float = -60.0,
) -> dict:
    return {
        "record_type": "position_closed",
        "run_id": RUN_ID,
        "session_id": SESSION_ID,
        "candidate_id": f"{base_mint}_{pool_id}_{entry_ts_ms}",
        "position_id": f"{pool_id}:{base_mint}:{entry_ts_ms}",
        "pool_id": pool_id,
        "mint_id": base_mint,
        "timestamp_ms": entry_ts_ms + age_ms,
        "duration_ms": age_ms,
        "close_reason": close_reason,
        "final_pnl_pct": final_pnl_pct,
    }


class TimeStopV2CounterfactualLabTests(unittest.TestCase):
    def run_scope(
        self,
        *,
        replay_rows: list[dict],
        lifecycle_rows: list[dict],
        target_bps: int = 6000,
        stop_bps: int = -6000,
        max_hold_ms: int = 120_000,
    ) -> tuple[dict, list[dict]]:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "scope"
            base = root / "logs" / "shadow_run" / scope
            base.mkdir(parents=True)
            (base / "shadow_exit_replay_v1.jsonl").write_text(
                "".join(json.dumps(row) + "\n" for row in replay_rows),
                encoding="utf-8",
            )
            (base / "shadow_lifecycle.jsonl").write_text(
                "".join(json.dumps(row) + "\n" for row in lifecycle_rows),
                encoding="utf-8",
            )
            (base / "probe_shadow_lifecycle.jsonl").write_text("", encoding="utf-8")
            with contextlib.redirect_stdout(io.StringIO()):
                exit_code = lab.main(
                    [
                        "--root",
                        str(root),
                        "--scope",
                        scope,
                        "--target-bps",
                        str(target_bps),
                        "--stop-bps",
                        str(stop_bps),
                        "--max-hold-ms",
                        str(max_hold_ms),
                        "--resurrection-windows-ms",
                        "4000,8000",
                    ]
                )
            self.assertEqual(exit_code, 0)
            out = root / "reports" / "selector" / scope
            report = json.loads((out / "time_stop_v2_counterfactual_report_v1.json").read_text())
            records = [
                json.loads(line)
                for line in (out / "time_stop_v2_counterfactual_exit_v1.jsonl").read_text().splitlines()
                if line.strip()
            ]
            return report, records

    def test_candidate_before_stop_is_saved_stop(self) -> None:
        report, records = self.run_scope(
            replay_rows=[
                replay_row(
                    first_hit_ms={"-6000": 50_000},
                    path_bps=[[0, 0], [12_000, -200], [50_000, -6000]],
                    last_pnl_bps=-6000,
                )
            ],
            lifecycle_rows=[
                window_row(age_ms=3_000, index=0),
                window_row(age_ms=7_000, index=1),
                window_row(age_ms=12_000, index=2, candidate=True, price_delta_from_entry_pct=-2.0),
                terminal_row(age_ms=50_000, close_reason="StopLoss", final_pnl_pct=-60.0),
            ],
        )
        self.assertEqual(records[0]["actual_classification"], "saved_stop")
        self.assertEqual(records[0]["delta_vs_baseline_bps"], 5800)
        self.assertEqual(report["actual_counterfactual_summary"]["saved_stop_count"], 1)

    def test_candidate_before_target_is_cut_target(self) -> None:
        _, records = self.run_scope(
            replay_rows=[
                replay_row(
                    first_hit_ms={"6000": 50_000},
                    path_bps=[[0, 0], [12_000, 300], [50_000, 6000]],
                    last_pnl_bps=6000,
                )
            ],
            lifecycle_rows=[
                window_row(age_ms=3_000, index=0),
                window_row(age_ms=7_000, index=1),
                window_row(age_ms=12_000, index=2, candidate=True, price_delta_from_entry_pct=3.0),
                terminal_row(age_ms=50_000, close_reason="Target", final_pnl_pct=60.0),
            ],
        )
        self.assertEqual(records[0]["actual_classification"], "cut_target")
        self.assertLess(records[0]["delta_vs_baseline_bps"], 0)

    def test_stale_candidate_is_excluded(self) -> None:
        report, records = self.run_scope(
            replay_rows=[
                replay_row(
                    first_hit_ms={"-6000": 50_000},
                    path_bps=[[0, 0], [12_000, -200], [50_000, -6000]],
                    last_pnl_bps=-6000,
                )
            ],
            lifecycle_rows=[
                window_row(
                    age_ms=12_000,
                    index=0,
                    status="stale_or_insufficient",
                    subreason="invalid_market_sample",
                    candidate=True,
                    price_delta_from_entry_pct=-2.0,
                ),
                terminal_row(age_ms=50_000),
            ],
        )
        self.assertEqual(records[0]["candidate_class"], "stale_data_no_action")
        self.assertFalse(records[0]["active_exit_eligible"])
        self.assertEqual(report["matrix_summary"][0]["tsv2_exit_count"], 0)

    def test_no_candidate_preserves_coverage(self) -> None:
        report, records = self.run_scope(
            replay_rows=[replay_row(path_bps=[[0, 0], [120_000, 10]], last_pnl_bps=10)],
            lifecycle_rows=[
                window_row(age_ms=3_000, index=0, candidate=False),
                terminal_row(age_ms=120_000, close_reason="TimeStop", final_pnl_pct=0.1),
            ],
        )
        self.assertEqual(records[0]["candidate_class"], "no_candidate")
        self.assertEqual(report["coverage"]["positions_with_tsv2_windows"], 1)
        self.assertEqual(report["matrix_summary"][0]["tsv2_exit_count"], 0)

    def test_candidate_after_terminal_is_not_active(self) -> None:
        _, records = self.run_scope(
            replay_rows=[
                replay_row(
                    first_hit_ms={"-6000": 50_000},
                    path_bps=[[0, 0], [50_000, -6000], [70_000, -7000]],
                    last_pnl_bps=-7000,
                )
            ],
            lifecycle_rows=[
                terminal_row(age_ms=50_000),
                window_row(age_ms=70_000, index=0, candidate=True, price_delta_from_entry_pct=-70.0),
            ],
        )
        self.assertFalse(records[0]["active_exit_eligible"])
        self.assertFalse(records[0]["candidate_before_terminal"])

    def test_duplicate_fallback_key_is_not_joined_silently(self) -> None:
        report, records = self.run_scope(
            replay_rows=[replay_row(entry_ts_ms=1_000)],
            lifecycle_rows=[
                {
                    **terminal_row(entry_ts_ms=2_000),
                    "session_id": "",
                    "position_id": "p1",
                    "candidate_id": "mint_pool_2000",
                },
                {
                    **terminal_row(entry_ts_ms=3_000),
                    "session_id": "",
                    "position_id": "p2",
                    "candidate_id": "mint_pool_3000",
                },
            ],
        )
        self.assertEqual(report["join_quality"]["duplicate_fallback_key_count"], 1)
        replay_record = next(record for record in records if record["has_exit_replay"])
        self.assertEqual(replay_record["join_quality"], "fallback_duplicate_ambiguous")

    def test_matrix_tie_stop_wins(self) -> None:
        result = lab.simulate_baseline(
            replay_row(
                first_hit_ms={"6000": 10_000, "-6000": 10_000},
                path_bps=[[0, 0], [10_000, -6000]],
            ),
            6000,
            -6000,
            120_000,
        )
        self.assertIsNotNone(result)
        self.assertEqual(result.result, lab.STOP)

    def test_max_hold_timeout_uses_pnl_at_max_hold(self) -> None:
        result = lab.simulate_baseline(
            replay_row(path_bps=[[0, 0], [10_000, 100], [20_000, 200]], last_pnl_bps=200),
            6000,
            -6000,
            15_000,
        )
        self.assertIsNotNone(result)
        self.assertEqual(result.result, lab.TIMEOUT)
        self.assertEqual(result.pnl_bps, 100)

    def test_no_tsv2_windows_returns_no_windows_recommendation(self) -> None:
        report, _ = self.run_scope(
            replay_rows=[replay_row(path_bps=[[0, 0], [120_000, 0]], last_pnl_bps=0)],
            lifecycle_rows=[terminal_row(age_ms=120_000, close_reason="TimeStop", final_pnl_pct=0.0)],
        )
        self.assertEqual(report["recommendation"], lab.RECOMMEND_NO_WINDOWS)

    def test_noharm_action_precision_excludes_no_actions(self) -> None:
        actions = [
            {
                "supported": True,
                "action_taken": True,
                "classification": "saved_stop",
                "delta_after_cost_bps": 100,
                "baseline_pnl_after_cost_bps": -200,
                "tsv2_pnl_after_cost_bps": -100,
                "baseline_result": lab.STOP,
                "baseline_result_quality": lab.EXACT_LEVELS,
                "exclusion_reason": "",
            },
            {
                "supported": True,
                "action_taken": True,
                "classification": "cut_target",
                "delta_after_cost_bps": -40,
                "baseline_pnl_after_cost_bps": 200,
                "tsv2_pnl_after_cost_bps": 160,
                "baseline_result": lab.TARGET,
                "baseline_result_quality": lab.EXACT_LEVELS,
                "exclusion_reason": "",
            },
            {
                "supported": True,
                "action_taken": False,
                "classification": "no_active_exit",
                "delta_after_cost_bps": 0,
                "baseline_pnl_after_cost_bps": 10,
                "tsv2_pnl_after_cost_bps": 10,
                "baseline_result": lab.TIMEOUT,
                "baseline_result_quality": lab.EXACT_LEVELS,
                "exclusion_reason": "no_candidate",
            },
        ]
        summary = lab.summarize_action_rows(actions)
        self.assertEqual(summary["beneficial_exit_count"], 1)
        self.assertEqual(summary["harmful_exit_count"], 1)
        self.assertEqual(summary["no_action_count"], 1)
        self.assertEqual(summary["exit_action_precision_denominator"], 2)
        self.assertEqual(summary["exit_action_precision"], 0.5)
        self.assertEqual(summary["target_cut_damage_bps"], 40)
        self.assertEqual(summary["saved_stop_damage_bps"], 100)

    def test_a2_masks_are_candidate_time_safe(self) -> None:
        replay = replay_row(
            first_hit_ms={"6000": 50_000},
            path_bps=[[0, 0], [12_000, 500], [16_000, 200], [50_000, 6000]],
            last_pnl_bps=6000,
        )
        record = {
            "_exit_replay_row": replay,
            "run_id": RUN_ID,
            "session_id": SESSION_ID,
            "pool_id": "pool",
            "base_mint": "mint",
            "entry_ts_ms": 1_000,
            "join_quality": "exact",
            "has_exit_replay": True,
            "has_tsv2_windows": True,
            "has_candidate": True,
            "active_exit_eligible": True,
            "candidate_class": "mixed_failed_vitality_candidate",
            "first_candidate_age_ms": 12_000,
            "candidate_pnl_bps": 500,
            "actual_close_age_ms": 60_000,
            "candidate_window_count_before_action": 3,
            "heartbeat_only_flag": False,
            "stale_flag": False,
            "candidate_source_reason": "weak:no_progress",
            "alive_within_4000ms_after_candidate": False,
        }

        m0 = lab.cell_action_rows([record], 6000, -6000, 120_000, mask_name="M0_ALL")[0]
        self.assertTrue(m0["action_taken"])
        self.assertEqual(m0["classification"], "cut_target")

        m1 = lab.cell_action_rows([record], 6000, -6000, 120_000, mask_name="M1_NEGATIVE_OR_FLAT_ONLY")[0]
        self.assertFalse(m1["action_taken"])
        self.assertEqual(m1["exclusion_reason"], "mask_excluded_positive_candidate_pnl")

        m5 = lab.cell_action_rows([record], 6000, -6000, 120_000, mask_name="M5_DELAY_4000MS_CONFIRM")[0]
        self.assertTrue(m5["action_taken"])
        self.assertEqual(m5["candidate_age_ms"], 16_000)
        self.assertEqual(m5["candidate_pnl_bps"], 200)
        self.assertEqual(m5["mask_action_source"], "delay_4000ms_path_prev")

    def test_segment_target_cut_failure_blocks_promising_offline_only(self) -> None:
        summary = {
            "mask_name": "M4_CONFIRM_2_WINDOWS",
            "target_bps": 10_000,
            "stop_bps": -6_000,
            "max_hold_ms": 120_000,
            "cost100_action_taken_count": 1_500,
            "cost100_ambiguous_unjoined_exclusions": 0,
            "cost100_exit_action_precision": 0.72,
            "cost100_exit_action_precision_wilson95_lower": 0.66,
            "cost100_delta_sum_bps": 270_000,
            "cost100_delta_avg_bps": 180.0,
            "cost100_delta_median_bps": 0.0,
            "cost100_target_cut_damage_guard_pass": True,
            "cost100_target_cut_count_guard_pass": True,
        }
        selected = {
            "selection_passed_train_gate": True,
            "summary_row": summary,
            "train_failures": [],
        }
        stability_rows = [
            {
                "mask_name": "M4_CONFIRM_2_WINDOWS",
                "target_bps": 10_000,
                "stop_bps": -6_000,
                "max_hold_ms": 120_000,
                "segment": "train",
                "delta_sum_bps": 100_000,
                "exit_action_precision": 0.72,
                "target_cut_damage_ratio": 0.20,
            },
            {
                "mask_name": "M4_CONFIRM_2_WINDOWS",
                "target_bps": 10_000,
                "stop_bps": -6_000,
                "max_hold_ms": 120_000,
                "segment": "validation",
                "delta_sum_bps": 90_000,
                "exit_action_precision": 0.71,
                "target_cut_damage_ratio": 0.24,
            },
            {
                "mask_name": "M4_CONFIRM_2_WINDOWS",
                "target_bps": 10_000,
                "stop_bps": -6_000,
                "max_hold_ms": 120_000,
                "segment": "holdout",
                "delta_sum_bps": 80_000,
                "exit_action_precision": 0.73,
                "target_cut_damage_ratio": 0.31,
            },
        ]
        verdict, blockers, warnings = lab.evaluate_a2_verdict(
            selected,
            stability_rows,
            [{"positive_delta": True}],
            {"exact_join_rate_over_exit_replay": 1.0},
        )
        self.assertEqual(verdict, lab.VERDICT_TARGET_CUT_RISK_UNRESOLVED)
        self.assertIn("holdout: target_cut_damage_ratio > 0.25", blockers)
        self.assertEqual(warnings, [])


if __name__ == "__main__":
    unittest.main()
