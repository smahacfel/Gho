#!/usr/bin/env python3
"""Fail-closed offline audit of transaction-local exact Pump reserve state.

The tool intentionally consumes only durable ``PoolTransaction`` rows.  It
does not open RPC, inspect a mutable runtime cache, use a price, or choose an
anchor using arrival time.  A group is strictly scoped to
``(slot, signature, bonding_curve)`` and can be exact only when its canonical
last trade row itself carries the complete, direct post-trade reserve tuple.

For an anchored group the tool reverse-walks every ordered trade using the
recorded base and curve-quote amounts, then replays forward.  The replay must
return exactly to the anchor and every available event virtual tuple must
match.  Any arithmetic, geometry, ordering, or completion ambiguity makes the
whole group non-evaluable.  If an input does not also retain a complete
mutation inventory for the bonding curve, its successful replay coverage is a
trade-fact upper bound rather than a proof that no unknown mutation occurred.

This is an offline audit utility, not a runtime component and not an execution
or quote authority.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable


SCHEMA_VERSION = 1


@dataclass(frozen=True)
class ReserveState:
    virtual_sol: int
    virtual_token: int
    real_sol: int
    real_token: int
    complete: bool


@dataclass(frozen=True)
class TradeFact:
    slot: int
    signature: str
    bonding_curve: str
    tx_index: int
    event_ordinal: int
    side: str
    token_amount: int
    curve_quote_amount: int
    virtual_sol: int | None
    virtual_token: int | None
    real_sol: int | None
    real_token: int | None
    complete: bool | None
    curve_data_known: bool

    @property
    def order_key(self) -> tuple[int, int]:
        return (self.tx_index, self.event_ordinal)

    @property
    def has_full_direct_state(self) -> bool:
        return (
            self.virtual_sol is not None
            and self.virtual_token is not None
            and self.real_sol is not None
            and self.real_token is not None
            and self.complete is not None
            and self.curve_data_known
        )

    def direct_state(self) -> ReserveState:
        if not self.has_full_direct_state:
            raise ValueError("missing full direct state")
        assert self.virtual_sol is not None
        assert self.virtual_token is not None
        assert self.real_sol is not None
        assert self.real_token is not None
        assert self.complete is not None
        return ReserveState(
            virtual_sol=self.virtual_sol,
            virtual_token=self.virtual_token,
            real_sol=self.real_sol,
            real_token=self.real_token,
            complete=self.complete,
        )


class AuditFailure(Exception):
    """Typed, group-scoped non-evaluable condition."""

    def __init__(self, bucket: str, reason: str) -> None:
        super().__init__(reason)
        self.bucket = bucket
        self.reason = reason


def iter_jsonl(path: Path) -> Iterable[dict[str, Any]]:
    with path.open("r", encoding="utf-8") as handle:
        for line_number, line in enumerate(handle, 1):
            raw = line.strip()
            if not raw:
                continue
            try:
                value = json.loads(raw)
            except json.JSONDecodeError as exc:
                raise SystemExit(f"{path}:{line_number}: invalid JSONL: {exc}") from exc
            if isinstance(value, dict):
                yield value


def as_int(value: Any, field: str) -> int:
    # ``bool`` is intentionally rejected even though Python treats it as int.
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise AuditFailure("NON_EVALUABLE_OTHER", f"invalid_{field}")
    return value


def as_optional_int(value: Any, field: str) -> int | None:
    if value is None:
        return None
    return as_int(value, field)


def as_optional_bool(value: Any, field: str) -> bool | None:
    if value is None:
        return None
    if not isinstance(value, bool):
        raise AuditFailure("NON_EVALUABLE_OTHER", f"invalid_{field}")
    return value


def row_to_fact(row: dict[str, Any]) -> TradeFact | None:
    kind = row.get("kind")
    if not isinstance(kind, dict) or kind.get("type") != "PoolTransaction":
        return None
    payload = kind.get("payload")
    if not isinstance(payload, dict) or payload.get("success") is not True:
        return None

    try:
        slot = as_int(payload.get("slot"), "slot")
        signature = payload.get("signature")
        bonding_curve = payload.get("bonding_curve")
        if not isinstance(signature, str) or not signature:
            raise AuditFailure("NON_EVALUABLE_OTHER", "invalid_signature")
        if not isinstance(bonding_curve, str) or not bonding_curve:
            raise AuditFailure("NON_EVALUABLE_OTHER", "invalid_bonding_curve")
        side = payload.get("side")
        if side not in {"buy", "sell"}:
            raise AuditFailure("NON_EVALUABLE_OTHER", "invalid_side")
        return TradeFact(
            slot=slot,
            signature=signature,
            bonding_curve=bonding_curve,
            tx_index=as_int(payload.get("tx_index"), "tx_index"),
            event_ordinal=as_int(payload.get("event_ordinal"), "event_ordinal"),
            side=side,
            token_amount=as_int(payload.get("token_amount_units"), "token_amount_units"),
            curve_quote_amount=as_int(
                payload.get("effective_curve_quote_lamports"),
                "effective_curve_quote_lamports",
            ),
            virtual_sol=as_optional_int(payload.get("virtual_sol_reserves"), "virtual_sol_reserves"),
            virtual_token=as_optional_int(
                payload.get("virtual_token_reserves"), "virtual_token_reserves"
            ),
            real_sol=as_optional_int(payload.get("real_sol_reserves"), "real_sol_reserves"),
            real_token=as_optional_int(
                payload.get("real_token_reserves"), "real_token_reserves"
            ),
            complete=as_optional_bool(payload.get("complete"), "complete"),
            curve_data_known=payload.get("curve_data_known") is True,
        )
    except AuditFailure:
        raise


def reverse_transition(after: ReserveState, fact: TradeFact) -> ReserveState:
    amount = fact.token_amount
    quote = fact.curve_quote_amount
    if amount == 0 or quote == 0:
        raise AuditFailure("NON_EVALUABLE_OTHER", "zero_trade_fact")

    if fact.side == "buy":
        if after.virtual_sol < quote or after.real_sol < quote:
            raise AuditFailure("NON_EVALUABLE_OTHER", "reverse_buy_quote_underflow")
        try:
            return ReserveState(
                virtual_sol=after.virtual_sol - quote,
                virtual_token=after.virtual_token + amount,
                real_sol=after.real_sol - quote,
                real_token=after.real_token + amount,
                complete=after.complete,
            )
        except OverflowError as exc:  # defensive; Python integers are unbounded.
            raise AuditFailure("NON_EVALUABLE_OTHER", "reverse_buy_overflow") from exc

    if fact.side == "sell":
        if after.virtual_token < amount or after.real_token < amount:
            raise AuditFailure("NON_EVALUABLE_OTHER", "reverse_sell_base_underflow")
        try:
            return ReserveState(
                virtual_sol=after.virtual_sol + quote,
                virtual_token=after.virtual_token - amount,
                real_sol=after.real_sol + quote,
                real_token=after.real_token - amount,
                complete=after.complete,
            )
        except OverflowError as exc:  # defensive; Python integers are unbounded.
            raise AuditFailure("NON_EVALUABLE_OTHER", "reverse_sell_overflow") from exc

    raise AuditFailure("NON_EVALUABLE_OTHER", "unknown_side")


def forward_transition(before: ReserveState, fact: TradeFact) -> ReserveState:
    amount = fact.token_amount
    quote = fact.curve_quote_amount
    if amount == 0 or quote == 0:
        raise AuditFailure("NON_EVALUABLE_OTHER", "zero_trade_fact")

    if fact.side == "buy":
        if before.virtual_token < amount or before.real_token < amount:
            raise AuditFailure("NON_EVALUABLE_OTHER", "forward_buy_base_underflow")
        return ReserveState(
            virtual_sol=before.virtual_sol + quote,
            virtual_token=before.virtual_token - amount,
            real_sol=before.real_sol + quote,
            real_token=before.real_token - amount,
            complete=before.complete,
        )
    if fact.side == "sell":
        if before.virtual_sol < quote or before.real_sol < quote:
            raise AuditFailure("NON_EVALUABLE_OTHER", "forward_sell_quote_underflow")
        return ReserveState(
            virtual_sol=before.virtual_sol - quote,
            virtual_token=before.virtual_token + amount,
            real_sol=before.real_sol - quote,
            real_token=before.real_token + amount,
            complete=before.complete,
        )
    raise AuditFailure("NON_EVALUABLE_OTHER", "unknown_side")


def ceil_div(numerator: int, denominator: int) -> int:
    if denominator <= 0:
        raise AuditFailure("NON_EVALUABLE_OTHER", "zero_or_negative_denominator")
    return (numerator + denominator - 1) // denominator


def transition_matches_typed_geometry(
    before: ReserveState, after: ReserveState, fact: TradeFact
) -> bool:
    """Validate the virtual part of one known Pump transition.

    BuyV2/LegacyBuy use exact-base-out (ceil quote reserve); the typed
    exact-quote-in variant uses the paired floor base reserve.  R6 does not
    retain a per-trade route discriminator, so the audit accepts only either
    of those two exact typed geometries; no approximate tolerance exists.
    Sell uses the exact-base-in typed transition.
    """

    if before.virtual_sol <= 0 or before.virtual_token <= 0:
        return False
    invariant = before.virtual_sol * before.virtual_token

    if fact.side == "sell":
        return ceil_div(invariant, after.virtual_token) == after.virtual_sol
    if fact.side == "buy":
        exact_base_out = ceil_div(invariant, after.virtual_token) == after.virtual_sol
        exact_quote_in = invariant // after.virtual_sol == after.virtual_token
        return exact_base_out or exact_quote_in
    return False


def assert_event_state_matches(fact: TradeFact, state: ReserveState) -> None:
    if fact.virtual_sol is not None and fact.virtual_sol != state.virtual_sol:
        raise AuditFailure("NON_EVALUABLE_CONSERVATION_MISMATCH", "virtual_sol_tuple_mismatch")
    if fact.virtual_token is not None and fact.virtual_token != state.virtual_token:
        raise AuditFailure(
            "NON_EVALUABLE_CONSERVATION_MISMATCH", "virtual_token_tuple_mismatch"
        )
    if fact.has_full_direct_state and fact.direct_state() != state:
        raise AuditFailure("NON_EVALUABLE_CONSERVATION_MISMATCH", "full_direct_state_mismatch")


def audit_group(group: list[TradeFact]) -> tuple[str, str, int, int]:
    """Return ``(bucket, reason, direct_rows, reconstructed_rows)``."""

    ordered = sorted(group, key=lambda fact: fact.order_key)
    seen_order_keys: set[tuple[int, int]] = set()
    for fact in ordered:
        if fact.order_key in seen_order_keys:
            return ("NON_EVALUABLE_OTHER", "duplicate_canonical_instruction_order", 0, 0)
        seen_order_keys.add(fact.order_key)
        if fact.token_amount == 0 or fact.curve_quote_amount == 0:
            return ("NON_EVALUABLE_OTHER", "missing_or_zero_pump_trade_fact", 0, 0)

    final_fact = ordered[-1]
    if not final_fact.has_full_direct_state:
        return ("NON_EVALUABLE_NO_ANCHOR", "missing_transaction_local_final_anchor", 0, 0)

    # A completed final state proves an unrecorded completion/migration
    # transition unless the record carries a typed completion fact. R6 does
    # not retain one, so it cannot legally be replayed as a trade-only group.
    if final_fact.complete:
        return ("NON_EVALUABLE_UNKNOWN_MUTATION", "unresolved_complete_transition", 0, 0)

    try:
        final_state = final_fact.direct_state()
        states_after: list[ReserveState] = [final_state] * len(ordered)
        current = final_state
        for index in range(len(ordered) - 1, -1, -1):
            states_after[index] = current
            current = reverse_transition(current, ordered[index])

        replay = current
        for fact, reconstructed_after in zip(ordered, states_after):
            replay_after = forward_transition(replay, fact)
            if replay_after != reconstructed_after:
                raise AuditFailure(
                    "NON_EVALUABLE_CONSERVATION_MISMATCH", "forward_reverse_state_mismatch"
                )
            if not transition_matches_typed_geometry(replay, replay_after, fact):
                raise AuditFailure(
                    "NON_EVALUABLE_CONSERVATION_MISMATCH", "typed_transition_geometry_mismatch"
                )
            assert_event_state_matches(fact, replay_after)
            replay = replay_after
        if replay != final_state:
            raise AuditFailure(
                "NON_EVALUABLE_CONSERVATION_MISMATCH", "forward_final_anchor_mismatch"
            )
    except AuditFailure as exc:
        return (exc.bucket, exc.reason, 0, 0)

    return ("EXACT", "exact_transaction_local_replay", 1, len(ordered) - 1)


def source_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return f"sha256:{digest.hexdigest()}"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, required=True, help="durable RUG reality JSONL")
    parser.add_argument("--output", type=Path, required=True, help="new JSON audit receipt")
    parser.add_argument(
        "--source",
        default="grpc_global_stream",
        help="PoolTransaction source to include (default: grpc_global_stream)",
    )
    args = parser.parse_args()

    groups: dict[tuple[int, str, str], list[TradeFact]] = defaultdict(list)
    counters: Counter[str] = Counter()
    invalid_rows: list[tuple[str, str]] = []

    for row in iter_jsonl(args.input):
        kind = row.get("kind")
        payload = kind.get("payload") if isinstance(kind, dict) else None
        if not isinstance(payload, dict) or kind.get("type") != "PoolTransaction":
            continue
        if payload.get("success") is not True or payload.get("source") != args.source:
            continue
        counters["live_successful_pump_trade_rows"] += 1
        try:
            fact = row_to_fact(row)
        except AuditFailure as exc:
            counters[exc.bucket] += 1
            invalid_rows.append((exc.reason, str(payload.get("signature", ""))))
            continue
        if fact is None:
            continue
        groups[(fact.slot, fact.signature, fact.bonding_curve)].append(fact)

    group_counts: Counter[str] = Counter()
    reason_counts: Counter[str] = Counter()
    sample_groups: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for key in sorted(groups):
        group = groups[key]
        bucket, reason, direct, reconstructed = audit_group(group)
        group_counts[bucket] += 1
        reason_counts[reason] += 1
        if bucket == "EXACT":
            counters["DIRECT_EXACT"] += direct
            counters["RECONSTRUCTED_EXACT"] += reconstructed
        else:
            counters[bucket] += len(group)
        if len(sample_groups[reason]) < 3:
            sample_groups[reason].append(
                {
                    "slot": key[0],
                    "signature": key[1],
                    "bonding_curve": key[2],
                    "trade_rows": len(group),
                    "canonical_order": [
                        {
                            "tx_index": fact.tx_index,
                            "event_ordinal": fact.event_ordinal,
                            "side": fact.side,
                            "token_amount_units": fact.token_amount,
                            "effective_curve_quote_lamports": fact.curve_quote_amount,
                        }
                        for fact in sorted(group, key=lambda value: value.order_key)
                    ],
                }
            )

    total = counters["live_successful_pump_trade_rows"]
    direct = counters["DIRECT_EXACT"]
    reconstructed = counters["RECONSTRUCTED_EXACT"]
    exact = direct + reconstructed
    # R6's durable PoolTransaction schema does not retain a complete
    # transaction-local curve-mutation inventory. An observed-fact replay can
    # diagnose the bound below, but it cannot certify the hard absence of an
    # unknown curve mutation required for an authority-grade PASS.
    strict_unknown_mutation_absence_proven = False
    report = {
        "schema_version": SCHEMA_VERSION,
        "audit": "rug_reality_transaction_local_exact_pump_state_v1",
        "input": {
            "path": str(args.input),
            "sha256": source_sha256(args.input),
            "source": args.source,
        },
        "scope": {
            "group_key": ["slot", "transaction_signature", "bonding_curve"],
            "canonical_instruction_order": ["tx_index", "event_ordinal"],
            "anchor_rule": "only complete direct state on canonical final trade row",
            "forbidden": [
                "slot_curve_only_account_join",
                "later_snapshot_assignment",
                "arrival_order",
                "mark_price",
                "shadow_ledger",
                "account_state_interpolation",
            ],
            "mutation_inventory": "not retained by the R6 PoolTransaction payload",
        },
        "counts": {
            "live_successful_pump_trade_rows": total,
            "transaction_local_groups": len(groups),
            "DIRECT_EXACT": direct,
            "RECONSTRUCTED_EXACT": reconstructed,
            "NON_EVALUABLE_NO_ANCHOR": counters["NON_EVALUABLE_NO_ANCHOR"],
            "NON_EVALUABLE_UNKNOWN_MUTATION": counters[
                "NON_EVALUABLE_UNKNOWN_MUTATION"
            ],
            "NON_EVALUABLE_CONSERVATION_MISMATCH": counters[
                "NON_EVALUABLE_CONSERVATION_MISMATCH"
            ],
            "NON_EVALUABLE_OTHER": counters["NON_EVALUABLE_OTHER"],
        },
        "group_counts": dict(sorted(group_counts.items())),
        "failure_reasons": dict(sorted(reason_counts.items())),
        "integrity": {
            # The captured durable schema has no per-transaction inventory of
            # every curve mutation.  This value is therefore an upper bound
            # under the recorded-trade-facts premise, not an authority claim
            # that an unknown mutation was absent.
            "trade_fact_replay_coverage_upper_bound": exact / total if total else 0.0,
            "trade_fact_replay_coverage_upper_bound_percent": (
                100.0 * exact / total
            )
            if total
            else 0.0,
            "strict_unknown_mutation_absence_proven": strict_unknown_mutation_absence_proven,
            "exact_coverage": exact / total if total else 0.0,
            "exact_coverage_percent": (100.0 * exact / total) if total else 0.0,
            "forward_reverse_mismatch_groups": reason_counts[
                "forward_reverse_state_mismatch"
            ]
            + reason_counts["forward_final_anchor_mismatch"],
            "virtual_tuple_mismatch_groups": reason_counts["virtual_sol_tuple_mismatch"]
            + reason_counts["virtual_token_tuple_mismatch"],
            "conservation_mismatch_groups": reason_counts[
                "typed_transition_geometry_mismatch"
            ],
            "arithmetic_underflow_groups": sum(
                value
                for reason, value in reason_counts.items()
                if reason.endswith("underflow")
            ),
            "pass": (
                total > 0
                and strict_unknown_mutation_absence_proven
                and exact / total >= 0.99
                and reason_counts["forward_reverse_state_mismatch"] == 0
                and reason_counts["forward_final_anchor_mismatch"] == 0
                and reason_counts["virtual_sol_tuple_mismatch"] == 0
                and reason_counts["virtual_token_tuple_mismatch"] == 0
                and reason_counts["typed_transition_geometry_mismatch"] == 0
            ),
        },
        "sample_groups_by_result": dict(sorted(sample_groups.items())),
        "invalid_row_samples": invalid_rows[:3],
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(report["counts"], sort_keys=True))
    print(
        "trade_fact_replay_coverage_upper_bound_percent="
        f"{report['integrity']['trade_fact_replay_coverage_upper_bound_percent']:.6f} "
        f"pass={report['integrity']['pass']}"
    )
    return 0 if report["integrity"]["pass"] else 2


if __name__ == "__main__":
    raise SystemExit(main())
