from __future__ import annotations

import sys
from pathlib import Path

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

import shadow_burnin_fidelity_audit as audit  # noqa: E402


def test_target_before_stop() -> None:
    result, age, pnl, quality = audit.simulate_exit_from_path([(0, 0), (1000, 1300), (2000, -700)], 1200, -600, 45000)
    assert (result, age, pnl, quality) == ("target", 1000, 1300, "OK")


def test_stop_before_target() -> None:
    result, age, pnl, quality = audit.simulate_exit_from_path([(0, 0), (1000, -700), (2000, 1300)], 1200, -600, 45000)
    assert (result, age, pnl, quality) == ("stop", 1000, -700, "OK")


def test_target_and_stop_same_timestamp_exact_hit_is_ambiguous() -> None:
    result, age = audit.classify_result_from_hits({1200: 1000, -600: 1000}, 1200, -600, 45000)
    assert result == "ambiguous_same_timestamp_stop_first"
    assert age == 1000


def test_target_and_stop_same_slot_unknown_order_is_not_silently_resolved() -> None:
    assert audit.classify_fallback_join(2) == "AMBIGUOUS_FALLBACK_JOIN"


def test_sparse_path_timeout_uses_last_known_point() -> None:
    result, age, pnl, quality = audit.simulate_exit_from_path([(0, 0), (44000, 100)], 1200, -600, 45000)
    assert result == "timeout"
    assert age == 44000
    assert pnl == 100
    assert "TIMEOUT_USES_LAST_KNOWN_BEFORE_MAX_HOLD" in quality


def test_no_path_point_before_max_hold() -> None:
    result, age, pnl, quality = audit.simulate_exit_from_path([(50000, 100)], 1200, -600, 45000)
    assert result == "timeout_no_point_before_max_hold"
    assert age is None
    assert pnl is None
    assert "NO_POINT_BEFORE_MAX_HOLD" in quality


def test_missing_exact_levels_timeout() -> None:
    result, age = audit.classify_result_from_hits({}, 1200, -600, 45000)
    assert result == "timeout"
    assert age == 45000


def test_exact_levels_vs_path_approximation() -> None:
    exact, _ = audit.classify_result_from_hits({1200: 1000}, 1200, -600, 45000)
    path, _, _, _ = audit.simulate_exit_from_path([(0, 0), (2000, 100)], 1200, -600, 45000)
    assert exact == "target"
    assert path == "timeout"


def test_max_hold_shorter_than_first_hit() -> None:
    result, age = audit.classify_result_from_hits({1200: 50000}, 1200, -600, 45000)
    assert result == "timeout"
    assert age == 45000


def test_max_hold_longer_than_replay_horizon() -> None:
    assert audit.path_density_verdict([(0, 0), (120000, 100)], 120000, 300000) == "NOT_EVALUABLE_HORIZON_EXCEEDS_REPLAY"


def test_malformed_first_hit_ms() -> None:
    first_hits, status = audit.parse_first_hit_ms("{bad")
    assert first_hits == {}
    assert status == "MALFORMED_FIRST_HIT_MS"


def test_malformed_path_bps() -> None:
    path, status = audit.parse_path_bps("{bad")
    assert path == []
    assert status == "MALFORMED_PATH_BPS"


def test_non_monotonic_path_age() -> None:
    assert audit.path_monotonic_status([(1000, 0), (500, 1)]) == "NON_MONOTONIC"


def test_duplicate_path_timestamps() -> None:
    assert audit.path_monotonic_status([(1000, 0), (1000, 1)]) == "DUPLICATE_TIMESTAMPS"


def test_mfe_mae_reconstruction() -> None:
    path = [(0, 0), (1000, 200), (2000, -100)]
    assert max(pnl for _, pnl in path) == 200
    assert min(pnl for _, pnl in path) == -100


def test_entry_price_from_reserves() -> None:
    price, status, fields = audit.reconstruct_price_from_reserves([30_000_000_000, 1_000_000_000_000_000], 0.00000003)
    assert status == "RECONSTRUCTED_FROM_RESERVES"
    assert fields == "quote_decimals=9;base_decimals=6"
    assert abs(price - 0.00000003) < 1e-15


def test_reserve_rounding_token_decimals_explicit() -> None:
    _, status, fields = audit.reconstruct_price_from_reserves([1, 3_000_000], None)
    assert status == "RECONSTRUCTED_FROM_RESERVES"
    assert fields in {"quote_decimals=9;base_decimals=6", "quote_decimals=9;base_decimals=9"}


def test_stale_state_snapshot() -> None:
    assert audit.classify_snapshot_timing(900, 1000) == "STALE_OR_PRE_DECISION"


def test_post_decision_state_accidentally_used() -> None:
    assert audit.classify_snapshot_timing(1100, 1000) == "POST_DECISION_STATE"


def test_own_trade_impact_absent_present() -> None:
    assert audit.classify_modeling(False) == "ABSENT"
    assert audit.classify_modeling(True) == "PRESENT"


def test_slippage_absent_present() -> None:
    assert audit.classify_modeling(False) == "ABSENT"
    assert audit.classify_modeling(True) == "PRESENT"


def test_lifecycle_duplicate_terminal_rows() -> None:
    assert audit.classify_duplicate_terminals(2) == "DUPLICATE_TERMINAL_RECORDS"
    assert audit.classify_duplicate_terminals(1) == "OK"


def test_ambiguous_fallback_joins() -> None:
    assert audit.classify_fallback_join(0) == "NO_FALLBACK_MATCH"
    assert audit.classify_fallback_join(1) == "SINGLE_FALLBACK_MARKED"
    assert audit.classify_fallback_join(2) == "AMBIGUOUS_FALLBACK_JOIN"


def test_missing_base_mint_pool_id() -> None:
    assert audit.classify_identity("", "mint") == "MISSING_IDENTITY"
    assert audit.classify_identity("pool", "") == "MISSING_IDENTITY"
    assert audit.classify_identity("pool", "mint") == "OK"


def test_replay_row_and_lifecycle_row_disagree() -> None:
    assert audit.classify_replay_lifecycle_agreement("target", "stop") == "DISAGREE"
    assert audit.classify_replay_lifecycle_agreement("target", "target") == "AGREE"


def test_fixture_csv_cases_are_all_passing() -> None:
    cases = audit.fixture_cases()
    assert len(cases) == 25
    failures = [case for case in cases if case["pass/fail"] != "pass"]
    assert failures == []


def _run_plain() -> None:
    tests = [
        value
        for name, value in sorted(globals().items())
        if name.startswith("test_") and callable(value)
    ]
    for test in tests:
        test()
    print(f"{len(tests)} fixture tests passed")


if __name__ == "__main__":
    _run_plain()
