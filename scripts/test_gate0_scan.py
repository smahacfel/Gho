import json
import tempfile
import unittest
from pathlib import Path
import gate0_scan as scan


def fixture():
    rows = [{"kind": "run_start", "run_id": "r", "schema_version": 1, "phase_ages_ms": list(scan.PHASES)}]
    for i in range(6):
        gem = i < 3
        mint = str(i)
        rows.append({"kind": "birth", "run_id": "r", "mint": mint, "born_ms": 100})
        for phase, age in enumerate(scan.PHASES, 1):
            rows.append({"kind": "phase", "run_id": "r", "mint": mint, "phase": phase, "age_ms": age,
                         "cutoff_ms": 100 + age, "snapshot": {"metrics": {"volume_cv": 10 if gem else 1,
                         "whale_reversal_ratio_top3": 0 if gem else 1, "ftdi": None, "tx_count": 100,
                         "custom_existing_metric": 12 if gem else 2}}})
        rows.append({"kind": "terminal", "run_id": "r", "mint": mint, "last_phase": 5,
                     "reason": "completed", "gem": gem})
    rows.append({"kind": "run_end", "run_id": "r", "reason": None})
    return rows


class Gate0ScanTests(unittest.TestCase):
    def analyze(self, rows, **kwargs):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "log.jsonl"
            path.write_text("".join(json.dumps(r) + "\n" for r in rows))
            return scan.analyze([path], minimum=2, **kwargs)

    def test_full_surface_not_five_metrics_and_no_phase5_predictive_claim(self):
        report = self.analyze(fixture())
        self.assertEqual(report["gate"], "GO")
        self.assertEqual(report["gems"], 3)
        self.assertIn("whale_reversal_ratio_top3", {r["metric"] for r in report["candidates"]})
        self.assertIn("custom_existing_metric", {r["metric"] for r in report["candidates"]})
        self.assertTrue(all(r["phase"] < 5 for r in report["candidates"]))
        self.assertTrue(all(r["metric"] != "tx_count" for r in report["candidates"]))

    def test_identical_samples_and_constants_have_zero_effect_full_overlap(self):
        self.assertEqual(scan.effect_size([1, 1], [1, 1]), 0)
        self.assertEqual(scan.overlap([1, 1], [1, 1]), 1)
        self.assertEqual(scan.effect_size([3, 4], [1, 2]), 1)
        self.assertEqual(scan.overlap([3, 4], [1, 2]), 0)

    def test_unknown_is_not_non_gem_and_missing_is_not_zero(self):
        rows = fixture()
        for row in rows:
            if row["kind"] == "terminal" and row["mint"] == "0":
                row["gem"] = None
        report = self.analyze(rows)
        self.assertEqual(report["label_unavailable"], 1)
        missing = next(r for r in report["metrics"] if r["phase"] == 1 and r["metric"] == "ftdi")
        self.assertEqual(missing["gem"]["n"], 0)
        self.assertIsNone(missing["gem"]["median"])
        self.assertIsNone(missing["cliffs_delta"])

    def test_unfinished_and_diagnostic_runs_are_not_usable(self):
        with self.assertRaises(ValueError):
            self.analyze(fixture()[:-1])
        rows = fixture()
        rows[-1]["reason"] = "source_gap"
        with self.assertRaises(ValueError):
            self.analyze(rows)

    def test_duplicate_snapshot_and_missing_prefix_fail(self):
        rows = fixture()
        rows.insert(-1, dict(rows[2]))
        with self.assertRaises(ValueError):
            self.analyze(rows)
        rows = fixture()
        del rows[2]
        with self.assertRaises(ValueError):
            self.analyze(rows)

    def test_outcome_conflict_cannot_be_hidden_by_clean_run_end(self):
        rows = fixture()
        rows.insert(-1, {"kind": "transaction_outcome_conflict", "run_id": "r"})
        with self.assertRaisesRegex(ValueError, "transaction outcome conflict"):
            self.analyze(rows)

    def test_quarantined_conflict_does_not_poison_other_tokens(self):
        rows = fixture()
        rows[0]["error_policy"] = "quarantine_and_continue"
        rows.insert(-1, {"kind": "transaction_outcome_conflict", "run_id": "r", "mint": "0", "handling": "quarantine_token"})
        rows.insert(-1, {"kind": "runtime_issue", "run_id": "r", "mint": "0", "code": "transaction_outcome_conflict", "handling": "quarantine_and_continue"})
        for row in rows:
            if row["kind"] == "terminal" and row["mint"] == "0":
                row["gem"] = None
                row["reason"] = "transaction_outcome_conflict"
        result = self.analyze(rows)
        self.assertEqual(result["gems"], 2)
        self.assertEqual(result["label_unavailable"], 1)
        self.assertEqual(result["data_quality"], "degraded")
        self.assertTrue(all(m["gem"]["n"] <= 2 for m in result["metrics"]))
        for row in rows:
            if row["kind"] == "terminal" and row["mint"] == "0": row["gem"] = True
        with self.assertRaisesRegex(ValueError, "not quarantined"):
            self.analyze(rows)

    def test_wrong_cutoff_fails(self):
        rows = fixture()
        rows[2]["cutoff_ms"] += 1
        with self.assertRaises(ValueError):
            self.analyze(rows)


if __name__ == "__main__":
    unittest.main()
