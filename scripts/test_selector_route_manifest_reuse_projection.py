#!/usr/bin/env python3
from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import build_selector_route_manifest_reuse_projection as route_manifest_reuse


PUMPFUN_PROGRAM_ID = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
TOKEN_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row, sort_keys=True) + "\n")


def read_jsonl(path: Path) -> list[dict]:
    with path.open(encoding="utf-8") as handle:
        return [json.loads(line) for line in handle if line.strip()]


class RouteManifestReuseProjectionTests(unittest.TestCase):
    def write_simcov_fixture(
        self,
        root: Path,
        *,
        scope: str,
        buy_rows: list[dict],
        shadow_rows: list[dict],
    ) -> None:
        decision_dir = (
            root
            / "logs"
            / "rollout"
            / scope
            / "decisions"
            / scope
            / "v2.2"
            / "legacy_live"
            / "fixture"
        )
        decision_rows = [
            {
                **row,
                "log_schema_version": 25,
                "decision_plane": "legacy_live",
                "gatekeeper_version": "v2.2",
                "decision_verdict_buy": True,
                "verdict_type": "BUY",
            }
            for row in buy_rows
        ]
        write_jsonl(decision_dir / "gatekeeper_v2_buys.jsonl", decision_rows)
        write_jsonl(decision_dir / "gatekeeper_v2_decisions.jsonl", decision_rows)
        write_jsonl(root / "logs" / "shadow_run" / f"{scope}-buys.jsonl", shadow_rows)
        shadow_dir = root / "logs" / "shadow_run" / scope
        write_jsonl(shadow_dir / "shadow_entries.jsonl", shadow_rows)
        write_jsonl(shadow_dir / "shadow_lifecycle.jsonl", shadow_rows)
        rollout_dir = root / "logs" / "rollout" / scope
        rollout_dir.mkdir(parents=True, exist_ok=True)
        (rollout_dir / "system.log").write_text("", encoding="utf-8")
        (rollout_dir / "oracle.log").write_text("", encoding="utf-8")

    def run_projection(self, root: Path, scope: str) -> dict:
        args = route_manifest_reuse.build_parser().parse_args(
            [
                "--scope",
                scope,
                "--root",
                str(root),
                "--decision-plane",
                "legacy_live",
            ]
        )
        return route_manifest_reuse.build_report(args)

    def legacy_tail_buy(self, *, ab_record_id: str = "pool1:mint1:BUY") -> tuple[dict, dict]:
        buy = {
            "pool_id": "pool1",
            "base_mint": "mint1",
            "ab_record_id": ab_record_id,
            "decision_ts_ms": 2_000,
            "shadow_execution_outcome": "shadow_unknown_error",
        }
        shadow = {
            "record_type": "shadow_dispatch",
            "pool_id": "pool1",
            "mint_id": "mint1",
            "base_mint": "mint1",
            "ab_record_id": ab_record_id,
            "decision_plane": "legacy_live",
            "decision_ts_ms": 2_000,
            "rpc_slot": 20,
            "dispatch_status": "not_dispatched",
            "simulation_outcome": "not_attempted",
            "execution_feasibility_status": "not_executable_route",
            "route_resolution_status": "no_executable_route_account_set",
            "dispatch_attempted": False,
            "simulation_attempted": False,
            "fallback_route_kind": "legacy_buy",
            "fallback_route_attempted": True,
            "legacy_buy_curve_pubkey": "pool1",
            "legacy_buy_associated_bonding_curve_pubkey": "abc1",
            "selected_route_account_set_roles": [
                "bonding_curve:pool1:account_state_core",
                "associated_bonding_curve:abc1:account_overrides",
                f"token_program:{TOKEN_PROGRAM}:token_program",
            ],
            "precheck_failure_reason": (
                "no_executable_route_account_set:"
                "legacy_buy_missing_buyback_remaining_accounts:count=0:expected=2"
            ),
        }
        return buy, shadow

    def raw_route_evidence(
        self,
        *,
        signature: str = "sig1",
        slot: int = 10,
        ix_index: int = 2,
        associated_bonding_curve: str = "abc1",
        remaining_accounts: list[str] | None = None,
        resolver_validation_status: str = "PASS",
        account_manifest_hash: str = "manifest1",
    ) -> dict:
        if remaining_accounts is None:
            remaining_accounts = ["buyback_fee", "buyback_quote"]
        ordered_accounts = [
            "global1",
            "mint1",
            "pool1",
            associated_bonding_curve,
            "user1",
            TOKEN_PROGRAM,
            PUMPFUN_PROGRAM_ID,
            *remaining_accounts,
        ]
        account_keys = ["unused0", *ordered_accounts]
        return {
            "artifact": "raw_pumpfun_instruction_evidence_v1",
            "parser_status": "OK",
            "signature": signature,
            "slot": slot,
            "tx_index": None,
            "ix_index": ix_index,
            "route_kind": "legacy_buy",
            "mint": "mint1",
            "program_id": PUMPFUN_PROGRAM_ID,
            "account_manifest_hash": account_manifest_hash,
            "account_keys": account_keys,
            "compiled_instruction_account_indices": list(range(1, len(account_keys))),
            "remaining_accounts": remaining_accounts,
            "remaining_accounts_count": len(remaining_accounts),
            "has_legacy_tail": len(remaining_accounts) == 2,
            "resolver_validation_status": resolver_validation_status,
            "can_unlock_execution": False,
            "named_accounts": [
                {"role": "global", "pubkey": "global1"},
                {"role": "mint", "pubkey": "mint1"},
                {"role": "bonding_curve", "pubkey": "pool1"},
                {"role": "associated_bonding_curve", "pubkey": associated_bonding_curve},
                {"role": "user", "pubkey": "user1"},
                {"role": "token_program", "pubkey": TOKEN_PROGRAM},
                {"role": "program", "pubkey": PUMPFUN_PROGRAM_ID},
            ],
        }

    def write_raw_route_evidence(self, root: Path, scope: str, rows: list[dict]) -> None:
        write_jsonl(
            root
            / "logs"
            / "nln_capture"
            / scope
            / "raw_pumpfun_instruction_evidence_v1.jsonl",
            rows,
        )

    def test_r17_tail_row_recoverable_by_exact_pool_route_manifest_without_unlock(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "r17-tail-recoverable"
            buy, shadow = self.legacy_tail_buy()
            self.write_simcov_fixture(root, scope=scope, buy_rows=[buy], shadow_rows=[shadow])
            self.write_raw_route_evidence(root, scope, [self.raw_route_evidence(slot=10)])

            report = self.run_projection(root, scope)
            tail_rows = read_jsonl(Path(report["outputs"]["legacy_tail_rows"]))
            store_rows = read_jsonl(Path(report["outputs"]["manifest_store"]))

        self.assertEqual(report["baseline"]["buy_rows"], 1)
        self.assertEqual(report["r17_tail_resolution"]["LEGACY_TAIL_MISSING_rows"], 1)
        self.assertEqual(report["r17_tail_resolution"]["tail_recoverable_rows"], 1)
        self.assertEqual(report["r17_tail_resolution"]["projected_attempt_coverage"]["display"], "1 / 1 = 100.00%")
        self.assertEqual(tail_rows[0]["projected_recoverability"], "TAIL_RECOVERABLE_BY_RAW_TX_MANIFEST")
        self.assertEqual(tail_rows[0]["manifest_lookup_status"], "exact_pool_route_manifest_found")
        self.assertEqual(tail_rows[0]["tail_accounts"], ["buyback_fee", "buyback_quote"])
        self.assertFalse(tail_rows[0]["can_unlock_execution"])
        self.assertFalse(store_rows[0]["can_unlock_execution"])
        self.assertEqual(report["claim_boundaries"]["can_unlock_execution_true_rows"], 0)

    def test_r17_tail_row_blocked_by_no_prior_manifest_has_row_reason(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "r17-tail-no-manifest"
            buy, shadow = self.legacy_tail_buy()
            self.write_simcov_fixture(root, scope=scope, buy_rows=[buy], shadow_rows=[shadow])

            report = self.run_projection(root, scope)
            tail_rows = read_jsonl(Path(report["outputs"]["legacy_tail_rows"]))

        self.assertEqual(report["r17_tail_resolution"]["LEGACY_TAIL_MISSING_rows"], 1)
        self.assertEqual(report["r17_tail_resolution"]["tail_recoverable_rows"], 0)
        self.assertEqual(report["r17_tail_resolution"]["tail_blocked_by_no_manifest"], 1)
        self.assertEqual(tail_rows[0]["projected_recoverability"], "BLOCKED_BY_NO_PRIOR_MANIFEST")
        self.assertEqual(tail_rows[0]["blocking_reason"], "no raw manifest for mint+route")
        self.assertFalse(tail_rows[0]["can_unlock_execution"])

    def test_r17_tail_row_blocked_by_route_cache_conflict(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "r17-tail-conflict"
            buy, shadow = self.legacy_tail_buy()
            self.write_simcov_fixture(root, scope=scope, buy_rows=[buy], shadow_rows=[shadow])
            self.write_raw_route_evidence(
                root,
                scope,
                [
                    self.raw_route_evidence(
                        signature="sig1",
                        remaining_accounts=["buyback_fee", "buyback_quote"],
                        account_manifest_hash="manifest-a",
                    ),
                    self.raw_route_evidence(
                        signature="sig2",
                        remaining_accounts=["other_fee", "other_quote"],
                        account_manifest_hash="manifest-b",
                    ),
                ],
            )

            report = self.run_projection(root, scope)
            tail_rows = read_jsonl(Path(report["outputs"]["legacy_tail_rows"]))

        self.assertEqual(report["r17_tail_resolution"]["LEGACY_TAIL_MISSING_rows"], 1)
        self.assertEqual(report["r17_tail_resolution"]["tail_recoverable_rows"], 0)
        self.assertEqual(report["r17_tail_resolution"]["tail_blocked_by_conflict"], 1)
        self.assertEqual(tail_rows[0]["projected_recoverability"], "BLOCKED_BY_ROUTE_CACHE_CONFLICT")
        self.assertEqual(tail_rows[0]["conflict_status"], "conflicted")
        self.assertFalse(tail_rows[0]["can_unlock_execution"])


if __name__ == "__main__":
    unittest.main()
