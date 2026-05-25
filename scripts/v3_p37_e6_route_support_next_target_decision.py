#!/usr/bin/env python3
"""Build the P3.7-E6 route-support next-target decision report.

E6 is an offline decision step. It does not call RPC, run shadow simulation, or
infer route support from the filesystem. The input is the already audited E1
route matrix plus E5A/E5B route closure evidence.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


SCHEMA_VERSION = 1
DEFAULT_E1_JSON = Path(
    "PLANS/AUDYT/RAPORT_P3_7_E1_PUMPFUN_EXECUTABLE_ROUTE_SUPPORT_MATRIX_20260524.json"
)
DEFAULT_E5A_MD = Path(
    "PLANS/AUDYT/RAPORT_P3_7_E5A_DIRECT_BUY_BUILDER_ROUTE_ABI_MANIFEST_AUDIT_20260525.md"
)
DEFAULT_E5B_MD = Path(
    "PLANS/AUDYT/RAPORT_P3_7_E5B_LEGACY_BUY_ROUTE_CLOSURE_20260525.md"
)
DEFAULT_MD_OUTPUT = Path(
    "PLANS/AUDYT/RAPORT_P3_7_E6_ROUTE_SUPPORT_NEXT_TARGET_DECISION_20260525.md"
)
DEFAULT_JSON_OUTPUT = Path(
    "PLANS/AUDYT/RAPORT_P3_7_E6_ROUTE_SUPPORT_NEXT_TARGET_DECISION_20260525.json"
)


def read_json(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError(f"{path} did not contain a JSON object")
    return payload


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8", errors="ignore")


def top_counter_key(counter: dict[str, Any]) -> str:
    if not counter:
        return "unknown"
    return max(counter.items(), key=lambda item: (int(item[1]), str(item[0])))[0]


def observed_account_count(route: dict[str, Any]) -> int | None:
    values = route.get("account_count_values")
    if not isinstance(values, dict) or not values:
        return None
    numeric: list[tuple[int, int]] = []
    for key, value in values.items():
        try:
            numeric.append((int(str(key)), int(value)))
        except (TypeError, ValueError):
            continue
    if not numeric:
        return None
    return max(numeric, key=lambda item: (item[1], item[0]))[0]


def contains_bcv2(route: dict[str, Any]) -> bool:
    roles = route.get("account_role_counts")
    if isinstance(roles, dict) and int(roles.get("bonding_curve_v2", 0) or 0) > 0:
        return True
    for field in ("required_simulation_load_accounts", "required_precheck_accounts"):
        values = route.get(field)
        if isinstance(values, list) and any(str(item).startswith("bonding_curve_v2:") for item in values):
            return True
    account_map = route.get("account_index_to_role_map")
    if isinstance(account_map, dict):
        for counter in account_map.values():
            if isinstance(counter, dict) and any(str(key).startswith("bonding_curve_v2:") for key in counter):
                return True
    return False


def route_has_load_readiness(route: dict[str, Any]) -> bool:
    try:
        return int(route.get("rpc_load_ready_true", 0) or 0) > 0
    except (TypeError, ValueError):
        return False


def route_account_map_complete(route: dict[str, Any], account_count: int | None) -> bool:
    if account_count is None or account_count <= 0:
        return False
    try:
        coverage = int(route.get("account_index_to_role_map_coverage", 0) or 0)
    except (TypeError, ValueError):
        return False
    return coverage >= account_count


def legacy_buy_closed_by_e5b(e5b_text: str) -> bool:
    return "LEGACY_BUY_UNSUPPORTED_REMOVED_FROM_FALLBACK" in e5b_text


def legacy_layout_uses_bcv2(e5a_text: str) -> bool:
    return "BUILDER_LEGACY_LAYOUT_USES_BCV2" in e5a_text


def analyze_route(
    route_variant: str,
    route: dict[str, Any],
    *,
    e5a_text: str,
    e5b_text: str,
) -> dict[str, Any]:
    account_count = observed_account_count(route)
    closed_legacy = route_variant == "legacy_buy" and legacy_buy_closed_by_e5b(e5b_text)
    bcv2_required = contains_bcv2(route) or (
        route_variant == "legacy_buy" and legacy_layout_uses_bcv2(e5a_text)
    )
    map_complete = route_account_map_complete(route, account_count)
    rpc_ready = route_has_load_readiness(route)
    shadow_status = str(route.get("shadow_simulation_support_status") or "unknown")

    builder_status = str(route.get("builder_support_status") or "unknown")
    route_closure_status = "open"
    implementation_risk = "unknown"
    simulation_support_possible = False
    recommendation = "not_selected"

    if closed_legacy:
        builder_status = "unsupported_closed_builder_layout_requires_bcv2"
        route_closure_status = "closed_unsupported_builder_layout_requires_bcv2"
        implementation_risk = "closed_without_clean_true_legacy_abi"
        recommendation = "do_not_use_as_fallback_without_clean_true_legacy_abi"
    elif shadow_status == "executable":
        route_closure_status = "verified_executable"
        implementation_risk = "low_already_executable"
        simulation_support_possible = True
        recommendation = "scope_candidate"
    elif bcv2_required and not rpc_ready:
        route_closure_status = "blocked_bcv2_not_load_ready"
        implementation_risk = "not_a_new_builder_target_bcv2_readiness_blocker"
        recommendation = "exclude_until_bcv2_load_ready_or_alternate_route"
    elif not map_complete:
        route_closure_status = "abi_map_incomplete"
        implementation_risk = "requires_observed_account_position_map"
        recommendation = "abi_discovery_required"
    elif builder_status in {"unknown", "observed_tx_identity_only"}:
        route_closure_status = "builder_support_missing"
        implementation_risk = "requires_builder_implementation"
        recommendation = "possible_e7_after_builder_contract_design"
    else:
        route_closure_status = "candidate_for_next_route_implementation"
        implementation_risk = "medium_requires_targeted_builder_implementation"
        simulation_support_possible = True
        recommendation = "candidate_for_e7"

    return {
        "route_variant": route_variant,
        "observed_count": int(route.get("observed_count", 0) or 0),
        "observed_account_count": account_count,
        "program_id": top_counter_key(route.get("program_id_counts") or {}),
        "instruction_discriminator": top_counter_key(route.get("instruction_discriminator_counts") or {}),
        "account_position_map_complete": map_complete,
        "account_position_map_coverage": int(route.get("account_index_to_role_map_coverage", 0) or 0),
        "loaded_addresses_required": bool(route.get("loaded_address_usage") or {}),
        "builder_support_status": builder_status,
        "prepared_request_support_status": str(route.get("prepared_request_support_status") or "unknown"),
        "final_manifest_requires_bcv2": bcv2_required,
        "rpc_load_readiness_observed": {
            "ready_true": int(route.get("rpc_load_ready_true", 0) or 0),
            "ready_false": int(route.get("rpc_load_ready_false", 0) or 0),
            "rate": route.get("rpc_load_readiness_rate"),
        },
        "shadow_simulation_support_status": shadow_status,
        "simulation_support_possible": simulation_support_possible,
        "route_closure_status": route_closure_status,
        "primary_failure_class": str(route.get("primary_failure_class") or "unknown"),
        "implementation_risk": implementation_risk,
        "expected_unlock_value": (
            int(route.get("observed_count", 0) or 0)
            if recommendation in {"candidate_for_e7", "scope_candidate"}
            else 0
        ),
        "recommendation": recommendation,
    }


def choose_final_decision(routes: dict[str, dict[str, Any]]) -> dict[str, Any]:
    if not routes:
        return {
            "final_decision": "BLOCK_ROUTE_SUPPORT_ABI_DISCOVERY_REQUIRED",
            "recommended_next_path": "route_artifact_gap",
            "recommended_next_route_to_implement": "unknown",
            "decision_reason": "E1 route matrix contains no observed route classes.",
        }

    candidates = [
        route
        for route in routes.values()
        if route["recommendation"] == "candidate_for_e7"
    ]
    if candidates:
        selected = max(candidates, key=lambda route: (route["expected_unlock_value"], route["route_variant"]))
        return {
            "final_decision": "GO_E7_IMPLEMENT_NEXT_ROUTE_CLASS",
            "recommended_next_path": "implement_next_route_class",
            "recommended_next_route_to_implement": selected["route_variant"],
            "decision_reason": (
                f"{selected['route_variant']} has a complete observed account map and is not "
                "closed by E5B or blocked by current BCV2 readiness evidence."
            ),
        }

    executable_scope = [
        route["route_variant"]
        for route in routes.values()
        if route["route_closure_status"] == "verified_executable"
    ]
    if executable_scope:
        return {
            "final_decision": "GO_SCOPE_RESTRICT_TO_SUPPORTED_ROUTE_CLASSES",
            "recommended_next_path": "restrict_execution_universe_to_verified_routes",
            "recommended_next_route_to_implement": "none",
            "decision_reason": (
                "No new route implementation target is justified, but at least one "
                "route class already has verified executable support."
            ),
        }

    return {
        "final_decision": "BLOCK_ROUTE_SUPPORT_ABI_DISCOVERY_REQUIRED",
        "recommended_next_path": "collect_or_extract_observed_route_abi_map",
        "recommended_next_route_to_implement": "no_implementable_route_found",
        "decision_reason": (
            "E1 observed only legacy_buy and routed_exact_sol_in. E5B closes legacy_buy, "
            "and routed_exact_sol_in remains blocked by route-required BCV2 load readiness, "
            "so there is no safe next route implementation target in the current artifacts."
        ),
    }


def build_report(e1_report: dict[str, Any], e5a_text: str, e5b_text: str) -> dict[str, Any]:
    e1_routes = e1_report.get("routes")
    if not isinstance(e1_routes, dict):
        e1_routes = {}

    routes = {
        route_variant: analyze_route(
            route_variant,
            route if isinstance(route, dict) else {},
            e5a_text=e5a_text,
            e5b_text=e5b_text,
        )
        for route_variant, route in sorted(e1_routes.items())
    }
    decision = choose_final_decision(routes)

    excluded_from_l2 = sorted(
        route["route_variant"]
        for route in routes.values()
        if route["route_closure_status"] != "verified_executable"
    )
    supported_executable = sorted(
        route["route_variant"]
        for route in routes.values()
        if route["route_closure_status"] == "verified_executable"
    )

    summary = {
        "observed_route_variant_counts": {
            route: item["observed_count"]
            for route, item in sorted(routes.items())
        },
        "evaluated_route_classes": sorted(routes),
        "closed_route_classes": sorted(
            route for route, item in routes.items() if item["route_closure_status"].startswith("closed_")
        ),
        "candidate_route_classes": sorted(
            route for route, item in routes.items() if item["recommendation"] == "candidate_for_e7"
        ),
        "supported_executable_route_classes": supported_executable,
        "route_classes_excluded_from_l2": excluded_from_l2,
        "legacy_buy_closed_by_e5b": legacy_buy_closed_by_e5b(e5b_text),
        "builder_legacy_layout_uses_bcv2": legacy_layout_uses_bcv2(e5a_text),
        "next_route_to_implement": decision["recommended_next_route_to_implement"],
        "scope_restriction_supported_route_count": len(supported_executable),
    }

    return {
        "schema_version": SCHEMA_VERSION,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "inputs": {
            "e1_final_decision": e1_report.get("final_decision"),
            "e1_recommended_next_path": e1_report.get("recommended_next_path"),
            "e5a_verdict": (
                "BUILDER_LEGACY_LAYOUT_USES_BCV2"
                if legacy_layout_uses_bcv2(e5a_text)
                else "unknown"
            ),
            "e5b_verdict": (
                "LEGACY_BUY_UNSUPPORTED_REMOVED_FROM_FALLBACK"
                if legacy_buy_closed_by_e5b(e5b_text)
                else "unknown"
            ),
        },
        "summary": summary,
        "routes": routes,
        **decision,
    }


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def markdown_table(rows: list[list[Any]]) -> str:
    if not rows:
        return ""
    lines = [
        "| " + " | ".join(str(item) for item in rows[0]) + " |",
        "| " + " | ".join("---" for _ in rows[0]) + " |",
    ]
    for row in rows[1:]:
        lines.append("| " + " | ".join(str(item) for item in row) + " |")
    return "\n".join(lines)


def write_markdown(path: Path, report: dict[str, Any]) -> None:
    summary = report["summary"]
    route_rows = [[
        "route_variant",
        "observed_count",
        "account_count",
        "map_complete",
        "builder_support",
        "requires_bcv2",
        "rpc_ready",
        "closure_status",
        "recommendation",
    ]]
    for route, item in sorted(report["routes"].items()):
        readiness = item["rpc_load_readiness_observed"]
        route_rows.append([
            route,
            item["observed_count"],
            item["observed_account_count"] if item["observed_account_count"] is not None else "unknown",
            item["account_position_map_complete"],
            item["builder_support_status"],
            item["final_manifest_requires_bcv2"],
            f"{readiness['ready_true']}/{readiness['ready_true'] + readiness['ready_false']}",
            item["route_closure_status"],
            item["recommendation"],
        ])

    content = f"""# P3.7-E6 Route Support Next Target Decision

Generated at: `{report['generated_at']}`

## Decision

- final_decision: `{report['final_decision']}`
- recommended_next_path: `{report['recommended_next_path']}`
- recommended_next_route_to_implement: `{report['recommended_next_route_to_implement']}`
- decision_reason: `{report['decision_reason']}`

## Inputs

```json
{json.dumps(report['inputs'], ensure_ascii=False, indent=2, sort_keys=True)}
```

Runtime was not run. E6 is an offline decision over the existing E1/E5A/E5B
artifact chain.

## Summary

```json
{json.dumps(summary, ensure_ascii=False, indent=2, sort_keys=True)}
```

## Route Decision Matrix

{markdown_table(route_rows)}

## Interpretation

`legacy_buy` is closed by E5B as `unsupported_builder_layout_requires_bcv2`.
It must not be used as a fallback unless a clean true-legacy ABI/account layout
is supplied and test-proven first.

`routed_exact_sol_in` remains a known builder path, but E1 classifies it as
non-executable because the route-required `bonding_curve_v2` account is not
RPC-load-ready in the observed artifacts. That is not a new route class to
implement in E7.

The current supported executable route scope is therefore empty:

```json
{json.dumps(summary['supported_executable_route_classes'], ensure_ascii=False, indent=2)}
```

## Consequence

Do not run another `legacy_buy` smoke.
Do not run R18, L2D2, Phase B, P2/live, V3 selector, or threshold tuning from
this artifact set.

The next valid work is route ABI discovery / observed account-map extraction
for additional Pump.fun buy variants, or an explicit product decision that the
current execution universe is empty until route support expands.

## Route Details

```json
{json.dumps(report['routes'], ensure_ascii=False, indent=2, sort_keys=True)}
```
"""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--e1-json", type=Path, default=DEFAULT_E1_JSON)
    parser.add_argument("--e5a-md", type=Path, default=DEFAULT_E5A_MD)
    parser.add_argument("--e5b-md", type=Path, default=DEFAULT_E5B_MD)
    parser.add_argument("--output-md", type=Path, default=DEFAULT_MD_OUTPUT)
    parser.add_argument("--output-json", type=Path, default=DEFAULT_JSON_OUTPUT)
    parser.add_argument("--json", action="store_true", help="Print report JSON to stdout")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    report = build_report(
        read_json(args.e1_json),
        read_text(args.e5a_md),
        read_text(args.e5b_md),
    )
    write_json(args.output_json, report)
    write_markdown(args.output_md, report)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
