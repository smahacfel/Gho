#!/usr/bin/env python3
from __future__ import annotations

import unittest

import shadow_exit_replay_eval as replay_eval


def row(
    *,
    first_hit_ms: dict[str, int] | None = None,
    path_bps: list[list[int]] | None = None,
    last_pnl_bps: int = 0,
    levels_bps: list[int] | None = None,
    quality: str = "clean",
    truncated: bool = False,
    entry_ts_ms: int = 1_000,
    pool_id: str = "pool-a",
    base_mint: str = "mint-a",
) -> dict:
    return {
        "schema": "shadow_exit_replay_v1",
        "run_id": "run-a",
        "session_id": "session-a",
        "pool_id": pool_id,
        "base_mint": base_mint,
        "entry_ts_ms": entry_ts_ms,
        "horizon_ms": 120_000,
        "levels_bps": levels_bps
        if levels_bps is not None
        else [-6000, -300, -200, -100, 100, 200, 300, 400, 500, 6000],
        "first_hit_ms": first_hit_ms or {},
        "path_bps": path_bps or [[0, 0]],
        "last_pnl_bps": last_pnl_bps,
        "quality": quality,
        "truncated": truncated,
    }


class ShadowExitReplayEvalTests(unittest.TestCase):
    def test_exact_stop_loss_wins_when_stop_hits_first(self) -> None:
        rows = [
            row(
                first_hit_ms={
                    "-300": 500,
                    "100": 1_000,
                    "200": 1_000,
                    "300": 1_000,
                    "400": 1_000,
                    "500": 1_000,
                },
                path_bps=[[0, 0], [500, -300], [1_000, 500]],
                last_pnl_bps=500,
            )
        ]
        result = replay_eval.evaluate(rows, [400], [-300])[0]
        self.assertEqual(result["result_quality"], replay_eval.EXACT_LEVELS)
        self.assertEqual(result["stop_count"], 1)
        self.assertEqual(result["target_count"], 0)
        self.assertEqual(result["sum_pnl_bps"], -300)

    def test_timestop_uses_last_pnl_when_no_level_hits(self) -> None:
        rows = [
            row(
                first_hit_ms={},
                path_bps=[[0, 0], [1_000, 80], [2_000, 90]],
                last_pnl_bps=90,
            )
        ]
        result = replay_eval.evaluate(rows, [400], [-300])[0]
        self.assertEqual(result["timestop_count"], 1)
        self.assertEqual(result["sum_pnl_bps"], 90)

    def test_non_grid_threshold_uses_path_approx(self) -> None:
        rows = [
            row(
                path_bps=[[0, 0], [1_000, 170], [2_000, -40]],
                last_pnl_bps=-40,
            )
        ]
        result = replay_eval.evaluate(rows, [170], [-40])[0]
        self.assertEqual(result["result_quality"], replay_eval.PATH_APPROX)
        self.assertEqual(result["target_count"], 1)
        self.assertEqual(result["sum_pnl_bps"], 170)

    def test_matrix_target_before_stop(self) -> None:
        record = replay_eval.ReplayRecord(
            raw=row(first_hit_ms={"6000": 10_000, "-6000": 20_000}),
            order_index=1,
            run_id="run-a",
            session_id="session-a",
            pool_id="pool-a",
            base_mint="mint-a",
            entry_ts_ms=1_000,
            horizon_ms=120_000,
            quality="clean",
            truncated=False,
            levels_bps=frozenset([-6000, 6000]),
            first_hit_ms={6000: 10_000, -6000: 20_000},
            path_bps=((0, 0), (10_000, 6000), (20_000, -6000)),
        )
        outcome = replay_eval.simulate_record(record, 6000, -6000, 120_000)
        self.assertEqual(outcome.label, replay_eval.MATRIX_TARGET)
        self.assertEqual(outcome.pnl_bps, 6000)

    def test_matrix_stop_before_target(self) -> None:
        record = replay_eval.ReplayRecord(
            raw=row(first_hit_ms={"6000": 20_000, "-6000": 10_000}),
            order_index=1,
            run_id="run-a",
            session_id="session-a",
            pool_id="pool-a",
            base_mint="mint-a",
            entry_ts_ms=1_000,
            horizon_ms=120_000,
            quality="clean",
            truncated=False,
            levels_bps=frozenset([-6000, 6000]),
            first_hit_ms={6000: 20_000, -6000: 10_000},
            path_bps=((0, 0), (10_000, -6000), (20_000, 6000)),
        )
        outcome = replay_eval.simulate_record(record, 6000, -6000, 120_000)
        self.assertEqual(outcome.label, replay_eval.MATRIX_STOP)
        self.assertEqual(outcome.pnl_bps, -6000)

    def test_matrix_same_millisecond_stop_wins(self) -> None:
        record = replay_eval.ReplayRecord(
            raw=row(first_hit_ms={"6000": 10_000, "-6000": 10_000}),
            order_index=1,
            run_id="run-a",
            session_id="session-a",
            pool_id="pool-a",
            base_mint="mint-a",
            entry_ts_ms=1_000,
            horizon_ms=120_000,
            quality="clean",
            truncated=False,
            levels_bps=frozenset([-6000, 6000]),
            first_hit_ms={6000: 10_000, -6000: 10_000},
            path_bps=((0, 0), (10_000, -6000)),
        )
        outcome = replay_eval.simulate_record(record, 6000, -6000, 120_000)
        self.assertEqual(outcome.label, replay_eval.MATRIX_STOP)
        self.assertEqual(outcome.pnl_bps, -6000)

    def test_matrix_timeout_uses_path_prev(self) -> None:
        record = replay_eval.ReplayRecord(
            raw=row(path_bps=[[10_000, 100], [30_000, 800]], last_pnl_bps=800),
            order_index=1,
            run_id="run-a",
            session_id="session-a",
            pool_id="pool-a",
            base_mint="mint-a",
            entry_ts_ms=1_000,
            horizon_ms=120_000,
            quality="clean",
            truncated=False,
            levels_bps=frozenset([-6000, 6000]),
            first_hit_ms={},
            path_bps=((10_000, 100), (30_000, 800)),
        )
        outcome = replay_eval.simulate_record(record, 6000, -6000, 20_000)
        self.assertEqual(outcome.label, replay_eval.MATRIX_TIMEOUT)
        self.assertEqual(outcome.pnl_bps, 100)
        self.assertEqual(outcome.source, replay_eval.PATH_PREV_TIMEOUT)

    def test_matrix_missing_timeout_point_is_unavailable_not_zero(self) -> None:
        record = replay_eval.ReplayRecord(
            raw=row(path_bps=[[30_000, 800]], last_pnl_bps=800),
            order_index=1,
            run_id="run-a",
            session_id="session-a",
            pool_id="pool-a",
            base_mint="mint-a",
            entry_ts_ms=1_000,
            horizon_ms=120_000,
            quality="clean",
            truncated=False,
            levels_bps=frozenset([-6000, 6000]),
            first_hit_ms={},
            path_bps=((30_000, 800),),
        )
        outcome = replay_eval.simulate_record(record, 6000, -6000, 20_000)
        self.assertEqual(outcome.label, replay_eval.MATRIX_UNAVAILABLE)
        self.assertIsNone(outcome.pnl_bps)

    def test_load_records_excludes_degraded_and_unavailable(self) -> None:
        import tempfile
        from pathlib import Path

        rows = [
            row(pool_id="pool-clean"),
            row(pool_id="pool-degraded", quality="degraded"),
            row(pool_id="pool-unavailable", quality="unavailable"),
        ]
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "shadow_exit_replay_v1.jsonl"
            path.write_text("\n".join(__import__("json").dumps(item) for item in rows) + "\n")
            records, controls = replay_eval.load_records(path)
        self.assertEqual(len(records), 1)
        self.assertEqual(controls["quality_counts"]["clean"], 1)
        self.assertEqual(controls["quality_counts"]["degraded"], 1)
        self.assertEqual(controls["quality_counts"]["unavailable"], 1)
        self.assertEqual(controls["damaged_records"], 2)

    def test_matrix_result_is_deterministic_independent_of_record_order(self) -> None:
        records = []
        for idx, first_hit_ms in enumerate(
            [{"6000": 10_000}, {"-6000": 20_000}, {}],
            start=1,
        ):
            records.append(
                replay_eval.ReplayRecord(
                    raw=row(first_hit_ms=first_hit_ms, path_bps=[[0, 0], [120_000, idx * 100]]),
                    order_index=idx,
                    run_id="run-a",
                    session_id="session-a",
                    pool_id=f"pool-{idx}",
                    base_mint=f"mint-{idx}",
                    entry_ts_ms=idx,
                    horizon_ms=120_000,
                    quality="clean",
                    truncated=False,
                    levels_bps=frozenset([-6000, 6000]),
                    first_hit_ms={int(k): v for k, v in first_hit_ms.items()},
                    path_bps=((0, 0), (120_000, idx * 100)),
                )
            )
        matrix_a, _ = replay_eval.evaluate_matrix(records, [6000], [-6000], [120_000])
        matrix_b, _ = replay_eval.evaluate_matrix(list(reversed(records)), [6000], [-6000], [120_000])
        self.assertEqual(matrix_a, matrix_b)

    def test_matrix_6000_minus6000_120000_matches_legacy_denominator(self) -> None:
        rows = [
            row(first_hit_ms={"6000": 10_000}, path_bps=[[0, 0], [120_000, 7000]], last_pnl_bps=7000),
            row(first_hit_ms={"-6000": 10_000}, path_bps=[[0, 0], [120_000, -7000]], last_pnl_bps=-7000),
            row(first_hit_ms={}, path_bps=[[0, 0], [120_000, 250]], last_pnl_bps=250),
        ]
        records = [
            replay_eval.ReplayRecord(
                raw=item,
                order_index=idx,
                run_id=item["run_id"],
                session_id=item["session_id"],
                pool_id=item["pool_id"],
                base_mint=item["base_mint"],
                entry_ts_ms=item["entry_ts_ms"],
                horizon_ms=item["horizon_ms"],
                quality=item["quality"],
                truncated=item["truncated"],
                levels_bps=frozenset(item["levels_bps"]),
                first_hit_ms={int(k): v for k, v in item["first_hit_ms"].items()},
                path_bps=tuple((p[0], p[1]) for p in item["path_bps"]),
            )
            for idx, item in enumerate(rows, start=1)
        ]
        legacy = replay_eval.evaluate(rows, [6000], [-6000])[0]
        matrix = replay_eval.evaluate_matrix(records, [6000], [-6000], [120_000])[0][0]
        self.assertEqual(legacy["total"], matrix["eligible_count"])
        self.assertEqual(legacy["target_count"], matrix["target_count"])
        self.assertEqual(legacy["stop_count"], matrix["stop_count"])
        self.assertEqual(legacy["timestop_count"], matrix["timeout_count"])
        self.assertEqual(legacy["sum_pnl_bps"], matrix["sum_pnl_bps"])


if __name__ == "__main__":
    unittest.main()
