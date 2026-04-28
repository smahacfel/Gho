#!/usr/bin/env python3
import argparse
import json
import time
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any

import shadow_run_report as report


DEFAULT_TICK_INTERVAL_MS = 500
DEFAULT_MAX_TICKS_BEFORE_EXIT = 240
DEFAULT_AEM_T_S = 120
DEFAULT_SHUTDOWN_DRAIN_MS = 10_000
DEFAULT_PAPER_CONFIG = report.REPO_ROOT / "configs" / "rollout" / "paper-burnin.toml"


@dataclass
class CandidateCloseoutState:
    candidate_id: str
    candidate_ts_ms: int | None = None
    candidate_event_ms: int | None = None
    entry_submitted_ms: int | None = None
    entry_filled_ms: int | None = None
    position_opened_ms: int | None = None
    position_closed_ms: int | None = None
    last_event_ms: int | None = None

    def mark(self, event_type: str, row_ts: int | None) -> None:
        if row_ts is not None:
            self.last_event_ms = max(self.last_event_ms or row_ts, row_ts)
        if event_type == "Candidate" and self.candidate_event_ms is None:
            self.candidate_event_ms = row_ts
        elif event_type == "EntrySubmitted" and self.entry_submitted_ms is None:
            self.entry_submitted_ms = row_ts
        elif event_type == "EntryFilled" and self.entry_filled_ms is None:
            self.entry_filled_ms = row_ts
        elif event_type == "PositionOpened" and self.position_opened_ms is None:
            self.position_opened_ms = row_ts
        elif event_type == "PositionClosed" and self.position_closed_ms is None:
            self.position_closed_ms = row_ts

    @property
    def admitted(self) -> bool:
        return any(
            value is not None
            for value in (
                self.entry_submitted_ms,
                self.entry_filled_ms,
                self.position_opened_ms,
                self.position_closed_ms,
            )
        )

    @property
    def closed(self) -> bool:
        return self.position_closed_ms is not None

    @property
    def inflight(self) -> bool:
        return self.admitted and not self.closed

    @property
    def lifecycle_anchor_ms(self) -> int | None:
        if self.position_opened_ms is not None:
            return self.position_opened_ms
        for value in (
            self.entry_filled_ms,
            self.entry_submitted_ms,
            self.candidate_event_ms,
            self.candidate_ts_ms,
        ):
            if value is not None:
                return value
        return None


@dataclass
class CloseoutAssessment:
    status: str
    safe_to_stop_now: bool
    reason: str
    session_run_id: str | None
    session_start_ms: int | None
    now_ms: int
    lifecycle_horizon_ms: int
    shutdown_drain_ms: int
    shadow_success_count: int
    paper_seen_count: int
    pending_shadow_without_paper: list[str]
    inflight_candidates: list[dict[str, Any]]
    earliest_safe_stop_ms: int | None
    remaining_wait_ms: int | None


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Guard operacyjny dla paper burn-in closeoutu. "
            "Blokuje stop, gdy istnieje pending shadow->paper handoff albo paper inflight."
        )
    )
    parser.add_argument(
        "--config",
        type=Path,
        default=DEFAULT_PAPER_CONFIG,
        help=f"Launcher config used for the session (default: {DEFAULT_PAPER_CONFIG})",
    )
    parser.add_argument(
        "--tick-interval-ms",
        type=int,
        default=DEFAULT_TICK_INTERVAL_MS,
        help="Paper lifecycle tick interval in ms used by the runtime contract.",
    )
    parser.add_argument(
        "--max-ticks-before-exit",
        type=int,
        default=DEFAULT_MAX_TICKS_BEFORE_EXIT,
        help="Automatic paper lifecycle exit threshold in ticks used by the runtime contract.",
    )
    parser.add_argument(
        "--aem-t-s",
        type=int,
        default=DEFAULT_AEM_T_S,
        help="AEM horizon in seconds used by the runtime contract.",
    )
    parser.add_argument(
        "--shutdown-drain-ms",
        type=int,
        default=DEFAULT_SHUTDOWN_DRAIN_MS,
        help="Bounded post-buy shutdown drain window in ms.",
    )
    parser.add_argument(
        "--now-ms",
        type=int,
        help="Override current unix time in ms (useful for deterministic tests).",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Print machine-readable JSON output instead of human summary.",
    )
    return parser.parse_args()


def current_time_ms() -> int:
    return int(time.time() * 1000)


def resolve_row_ts(envelope: dict[str, Any]) -> int | None:
    event_time_ms = envelope.get("event_time_ms")
    candidate_id = envelope.get("candidate_id")
    run_id = envelope.get("run_id")
    for ts in (
        int(event_time_ms) if isinstance(event_time_ms, (int, float)) else None,
        report.extract_candidate_ts_ms(candidate_id),
        report.extract_numeric_suffix(run_id),
    ):
        if ts is not None:
            return ts
    return None


def scan_candidate_closeout_state(
    events_dir: Path,
    session_start_ms: int | None,
) -> dict[str, CandidateCloseoutState]:
    candidates: dict[str, CandidateCloseoutState] = {}
    if not events_dir.exists():
        return candidates

    for path in sorted(events_dir.rglob("*.jsonl")):
        with path.open("r", encoding="utf-8") as fh:
            for line in fh:
                line = line.strip()
                if not line:
                    continue
                try:
                    event = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if not isinstance(event, dict):
                    continue
                envelope = event.get("envelope", {})
                if not isinstance(envelope, dict):
                    continue
                candidate_id = envelope.get("candidate_id")
                if not isinstance(candidate_id, str) or not candidate_id:
                    continue
                row_ts = resolve_row_ts(envelope)
                if session_start_ms is not None and (row_ts is None or row_ts < session_start_ms):
                    continue
                kind = event.get("kind", {})
                if not isinstance(kind, dict):
                    continue
                event_type = kind.get("type")
                if event_type not in {
                    "Candidate",
                    "EntrySubmitted",
                    "EntryFilled",
                    "PositionOpened",
                    "PositionClosed",
                }:
                    continue
                state = candidates.setdefault(
                    candidate_id,
                    CandidateCloseoutState(
                        candidate_id=candidate_id,
                        candidate_ts_ms=report.extract_candidate_ts_ms(candidate_id),
                    ),
                )
                state.mark(event_type, row_ts)

    return candidates


def build_closeout_assessment(args: argparse.Namespace) -> CloseoutAssessment:
    args.metrics_text = None
    args.min_net_pnl_sol = None
    args.session_end_ms = None
    inputs = report.resolve_inputs(args)

    lifecycle_horizon_ms = max(
        int(args.tick_interval_ms) * int(args.max_ticks_before_exit),
        int(args.aem_t_s) * 1000,
    )
    shutdown_drain_ms = int(args.shutdown_drain_ms)
    now_ms = int(args.now_ms) if args.now_ms is not None else current_time_ms()

    _, shadow_success_ids, _, _, _ = report.scan_shadow_log(
        inputs.shadow_log,
        inputs.session_start_ms,
        None,
    )
    paper_candidates, _, _ = report.scan_event_dir(inputs.events_dir, inputs.session_start_ms, None)
    candidate_states = scan_candidate_closeout_state(inputs.events_dir, inputs.session_start_ms)

    paper_seen_ids = set(paper_candidates)
    pending_shadow_without_paper = sorted(shadow_success_ids - paper_seen_ids)

    inflight_states = [state for state in candidate_states.values() if state.inflight]
    inflight_states.sort(key=lambda state: (state.lifecycle_anchor_ms or -1, state.candidate_id))

    inflight_candidates: list[dict[str, Any]] = []
    earliest_safe_stop_ms: int | None = None
    for state in inflight_states:
        anchor_ms = state.lifecycle_anchor_ms
        candidate_safe_stop_ms = (
            anchor_ms + lifecycle_horizon_ms + shutdown_drain_ms
            if anchor_ms is not None
            else None
        )
        if candidate_safe_stop_ms is not None:
            earliest_safe_stop_ms = max(
                earliest_safe_stop_ms or candidate_safe_stop_ms,
                candidate_safe_stop_ms,
            )
        inflight_candidates.append(
            {
                "candidate_id": state.candidate_id,
                "candidate_ts_ms": state.candidate_ts_ms,
                "entry_submitted_ms": state.entry_submitted_ms,
                "entry_filled_ms": state.entry_filled_ms,
                "position_opened_ms": state.position_opened_ms,
                "last_event_ms": state.last_event_ms,
                "lifecycle_anchor_ms": anchor_ms,
                "safe_stop_after_ms": candidate_safe_stop_ms,
            }
        )

    if pending_shadow_without_paper:
        return CloseoutAssessment(
            status="WAIT_PENDING_PAPER_HANDOFF",
            safe_to_stop_now=False,
            reason=(
                "Istnieją shadow successy bez żadnego paper eventu w bieżącym oknie sesji. "
                "Nie wolno jeszcze wysyłać SIGINT, bo handoff shadow->paper nie jest domknięty."
            ),
            session_run_id=inputs.session_run_id,
            session_start_ms=inputs.session_start_ms,
            now_ms=now_ms,
            lifecycle_horizon_ms=lifecycle_horizon_ms,
            shutdown_drain_ms=shutdown_drain_ms,
            shadow_success_count=len(shadow_success_ids),
            paper_seen_count=len(paper_seen_ids),
            pending_shadow_without_paper=pending_shadow_without_paper,
            inflight_candidates=inflight_candidates,
            earliest_safe_stop_ms=None,
            remaining_wait_ms=None,
        )

    if inflight_candidates:
        remaining_wait_ms = None
        if earliest_safe_stop_ms is not None:
            remaining_wait_ms = max(0, earliest_safe_stop_ms - now_ms)
        reason = (
            "Istnieją papierowe pozycje admitted/opened bez PositionClosed. "
            "Czekaj na pełne domknięcie lifecycle i uruchamiaj guard ponownie, aż zwróci SAFE_TO_STOP."
        )
        if remaining_wait_ms == 0:
            reason = (
                "Minimalny budżet czasu dla paper lifecycle już upłynął, ale PositionClosed nadal nie istnieje. "
                "Nie wolno uznawać closeoutu za bezpieczny; trzeba poczekać na faktyczne domknięcie lifecycle "
                "albo wyjaśnić runtime/session shutdown, jeśli proces już nie żyje."
            )
        return CloseoutAssessment(
            status="WAIT_PAPER_CLOSEOUT",
            safe_to_stop_now=False,
            reason=reason,
            session_run_id=inputs.session_run_id,
            session_start_ms=inputs.session_start_ms,
            now_ms=now_ms,
            lifecycle_horizon_ms=lifecycle_horizon_ms,
            shutdown_drain_ms=shutdown_drain_ms,
            shadow_success_count=len(shadow_success_ids),
            paper_seen_count=len(paper_seen_ids),
            pending_shadow_without_paper=pending_shadow_without_paper,
            inflight_candidates=inflight_candidates,
            earliest_safe_stop_ms=earliest_safe_stop_ms,
            remaining_wait_ms=remaining_wait_ms,
        )

    return CloseoutAssessment(
        status="SAFE_TO_STOP",
        safe_to_stop_now=True,
        reason=(
            "Brak pending shadow->paper handoff i brak paper inflight w najnowszym oknie sesji. "
            "Można wykonać graceful shutdown."
        ),
        session_run_id=inputs.session_run_id,
        session_start_ms=inputs.session_start_ms,
        now_ms=now_ms,
        lifecycle_horizon_ms=lifecycle_horizon_ms,
        shutdown_drain_ms=shutdown_drain_ms,
        shadow_success_count=len(shadow_success_ids),
        paper_seen_count=len(paper_seen_ids),
        pending_shadow_without_paper=pending_shadow_without_paper,
        inflight_candidates=inflight_candidates,
        earliest_safe_stop_ms=earliest_safe_stop_ms,
        remaining_wait_ms=0,
    )


def format_text_output(assessment: CloseoutAssessment) -> str:
    lines = [
        "Paper Burn-in Closeout Guard",
        f"Status: {assessment.status}",
        f"Safe to stop now: {'yes' if assessment.safe_to_stop_now else 'no'}",
        f"Session run_id: {assessment.session_run_id or '<none>'}",
        f"Session start ms: {assessment.session_start_ms}",
        f"Now ms: {assessment.now_ms}",
        f"Lifecycle horizon ms: {assessment.lifecycle_horizon_ms}",
        f"Shutdown drain ms: {assessment.shutdown_drain_ms}",
        f"Shadow success count: {assessment.shadow_success_count}",
        f"Paper seen count: {assessment.paper_seen_count}",
        f"Pending shadow without paper: {len(assessment.pending_shadow_without_paper)}",
        f"Inflight candidates: {len(assessment.inflight_candidates)}",
        f"Reason: {assessment.reason}",
    ]
    if assessment.pending_shadow_without_paper:
        lines.append("Pending handoff candidate_ids:")
        lines.extend(f"- {candidate_id}" for candidate_id in assessment.pending_shadow_without_paper)
    if assessment.inflight_candidates:
        lines.append("Inflight candidates:")
        for candidate in assessment.inflight_candidates:
            lines.append(
                "- "
                f"{candidate['candidate_id']} "
                f"anchor_ms={candidate['lifecycle_anchor_ms']} "
                f"opened_ms={candidate['position_opened_ms']} "
                f"safe_stop_after_ms={candidate['safe_stop_after_ms']}"
            )
    if assessment.earliest_safe_stop_ms is not None:
        lines.append(f"Earliest safe stop ms: {assessment.earliest_safe_stop_ms}")
    if assessment.remaining_wait_ms is not None:
        lines.append(f"Remaining wait ms: {assessment.remaining_wait_ms}")
    return "\n".join(lines)


def main() -> int:
    args = parse_args()
    assessment = build_closeout_assessment(args)
    if args.json:
        print(json.dumps(asdict(assessment), indent=2, sort_keys=True))
    else:
        print(format_text_output(assessment))
    return 0 if assessment.safe_to_stop_now else 2


if __name__ == "__main__":
    raise SystemExit(main())
