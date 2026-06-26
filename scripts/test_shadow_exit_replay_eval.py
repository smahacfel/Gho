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
) -> dict:
    return {
        "schema": "shadow_exit_replay_v1",
        "levels_bps": levels_bps
        if levels_bps is not None
        else [-300, -200, -100, 100, 200, 300, 400, 500],
        "first_hit_ms": first_hit_ms or {},
        "path_bps": path_bps or [[0, 0]],
        "last_pnl_bps": last_pnl_bps,
        "quality": "clean",
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


if __name__ == "__main__":
    unittest.main()
