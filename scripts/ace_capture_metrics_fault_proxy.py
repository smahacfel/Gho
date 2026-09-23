#!/usr/bin/env python3
"""Local, non-authoritative HTTP fault injector for ACE resilience tests.

The proxy can sit between the test supervisor and the launcher's loopback
``/metrics`` endpoint, or in front of the read-only RPC endpoint during
startup retry validation.  It never obtains a Ghost shutdown handle and never
talks to Seer, Oracle, EventWriter, or capture control channels.  Its sole
purpose is to make timeout, HTTP status, reset, and temporary-unavailable
responses reproducible without granting infrastructure monitoring authority.
"""

from __future__ import annotations

import argparse
import json
import os
import signal
import socket
import threading
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Final


FAULT_KINDS: Final[tuple[str, ...]] = (
    "timeout",
    "http_429",
    "http_500",
    "connection_reset",
    "http_503",
)


def parse_fault_sequence(raw: str) -> tuple[str, ...]:
    values = tuple(item.strip() for item in raw.split(",") if item.strip())
    unknown = sorted(set(values).difference(FAULT_KINDS))
    if unknown:
        raise ValueError(f"unsupported metrics fault kind(s): {', '.join(unknown)}")
    if not values:
        raise ValueError("fault sequence must contain at least one fault kind")
    return values


def fault_for_request(
    request_index: int,
    *,
    every_requests: int,
    sequence: tuple[str, ...],
) -> str | None:
    """Return a deterministic fault without making the proxy a scheduler."""
    if every_requests <= 0 or request_index <= 0 or request_index % every_requests:
        return None
    return sequence[(request_index // every_requests - 1) % len(sequence)]


def ready_payload(
    *,
    bind: str,
    every_requests: int,
    sequence: tuple[str, ...],
    max_faults: int | None,
) -> dict[str, object]:
    """Describe the local injector without persisting its credential-bearing URL."""
    return {
        "bind": bind,
        "upstream_configured": True,
        "fault_every_requests": every_requests,
        "fault_sequence": sequence,
        "max_faults": max_faults,
    }


def resolve_upstream_url(
    upstream_url: str | None,
    upstream_url_env: str | None,
) -> str:
    """Load an upstream without placing a credential-bearing value in argv."""
    if upstream_url is not None:
        return upstream_url
    assert upstream_url_env is not None
    value = os.environ.get(upstream_url_env, "").strip()
    if not value:
        raise RuntimeError(f"fault proxy upstream environment variable is empty: {upstream_url_env}")
    return value


def upstream_response_metadata(headers: object) -> tuple[str, str | None]:
    """Retain content encoding so a forwarded JSON-RPC body stays decodable."""
    get = getattr(headers, "get")
    content_type = get("Content-Type", "text/plain; charset=utf-8")
    content_encoding = get("Content-Encoding")
    return content_type, content_encoding


class FaultState:
    def __init__(
        self,
        *,
        upstream_url: str,
        every_requests: int,
        sequence: tuple[str, ...],
        timeout_delay_s: float,
        upstream_timeout_s: float,
        max_faults: int | None,
        log_path: Path,
    ) -> None:
        self.upstream_url = upstream_url
        self.every_requests = every_requests
        self.sequence = sequence
        self.timeout_delay_s = timeout_delay_s
        self.upstream_timeout_s = upstream_timeout_s
        self.max_faults = max_faults
        self.log_path = log_path
        self._lock = threading.Lock()
        self._request_index = 0
        self._injected_faults = 0

    def next_fault(self) -> tuple[int, str | None]:
        with self._lock:
            self._request_index += 1
            request_index = self._request_index
            fault = fault_for_request(
                request_index,
                every_requests=self.every_requests,
                sequence=self.sequence,
            )
            if fault is not None and (
                self.max_faults is None or self._injected_faults < self.max_faults
            ):
                self._injected_faults += 1
                return request_index, fault
        return request_index, None

    def log(self, **payload: object) -> None:
        record = {"timestamp_unix_ms": time.time_ns() // 1_000_000, **payload}
        encoded = (json.dumps(record, sort_keys=True) + "\n").encode("utf-8")
        with self._lock:
            with self.log_path.open("ab") as handle:
                handle.write(encoded)
                handle.flush()
                os.fsync(handle.fileno())


def handler_for(state: FaultState) -> type[BaseHTTPRequestHandler]:
    class MetricsFaultHandler(BaseHTTPRequestHandler):
        server_version = "AceCaptureMetricsFaultProxy/1"

        def log_message(self, _format: str, *_args: object) -> None:
            # JSONL audit is the durable test output; suppress HTTP access noise.
            return

        def do_GET(self) -> None:  # noqa: N802 - BaseHTTPRequestHandler API
            self._handle_proxy_request(None)

        def do_POST(self) -> None:  # noqa: N802 - BaseHTTPRequestHandler API
            content_length = int(self.headers.get("Content-Length", "0"))
            self._handle_proxy_request(self.rfile.read(content_length))

        def _handle_proxy_request(self, request_body: bytes | None) -> None:
            request_index, fault = state.next_fault()
            if fault is not None:
                state.log(
                    request_index=request_index,
                    action="inject",
                    fault=fault,
                    method=self.command,
                )
                if fault == "timeout":
                    time.sleep(state.timeout_delay_s)
                    self._respond(504, b"fault-injected metrics timeout\n")
                    return
                if fault == "http_429":
                    self._respond(429, b"fault-injected metrics rate limit\n")
                    return
                if fault == "http_500":
                    self._respond(500, b"fault-injected metrics server error\n")
                    return
                if fault == "http_503":
                    self._respond(503, b"fault-injected metrics unavailable\n")
                    return
                if fault == "connection_reset":
                    try:
                        self.connection.shutdown(socket.SHUT_RDWR)
                    except OSError:
                        pass
                    self.connection.close()
                    return
                raise AssertionError(f"validated fault kind was not handled: {fault}")

            try:
                forward_headers = {
                    key: value
                    for key, value in self.headers.items()
                    if key.lower() not in {"host", "connection", "content-length"}
                }
                request = urllib.request.Request(
                    state.upstream_url,
                    data=request_body,
                    headers=forward_headers,
                    method=self.command,
                )
                with urllib.request.urlopen(request, timeout=state.upstream_timeout_s) as response:
                    body = response.read()
                    status = response.status
                    content_type, content_encoding = upstream_response_metadata(response.headers)
            except urllib.error.HTTPError as error:
                body = error.read()
                status = error.code
                content_type, content_encoding = upstream_response_metadata(error.headers)
            except Exception as error:
                state.log(
                    request_index=request_index,
                    action="upstream_error",
                    method=self.command,
                    error_class=type(error).__name__,
                )
                self._respond(503, b"upstream metrics endpoint unavailable\n")
                return

            state.log(
                request_index=request_index,
                action="forward",
                method=self.command,
                upstream_status=status,
            )
            self._respond(
                status,
                body,
                content_type=content_type,
                content_encoding=content_encoding,
            )

        def _respond(
            self,
            status: int,
            body: bytes,
            *,
            content_type: str = "text/plain; charset=utf-8",
            content_encoding: str | None = None,
        ) -> None:
            self.send_response(status)
            self.send_header("Content-Type", content_type)
            if content_encoding:
                self.send_header("Content-Encoding", content_encoding)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            try:
                self.wfile.write(body)
            except BrokenPipeError:
                pass

    return MetricsFaultHandler


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bind", default="127.0.0.1:19190")
    upstream = parser.add_mutually_exclusive_group(required=True)
    upstream.add_argument("--upstream-url")
    upstream.add_argument(
        "--upstream-url-env",
        help="environment-variable name holding the upstream URL; keeps its value out of argv",
    )
    parser.add_argument("--log", required=True)
    parser.add_argument("--ready-file", required=True)
    parser.add_argument("--fault-every-requests", type=int, default=30)
    parser.add_argument("--fault-sequence", default=",".join(FAULT_KINDS))
    parser.add_argument(
        "--max-faults",
        type=int,
        default=0,
        help="inject only this many faults; 0 means no limit",
    )
    parser.add_argument("--timeout-delay-s", type=float, default=3.0)
    parser.add_argument("--upstream-timeout-s", type=float, default=2.0)
    args = parser.parse_args()
    host, separator, port_text = args.bind.rpartition(":")
    if not separator or not host:
        parser.error("--bind must be HOST:PORT")
    try:
        args.port = int(port_text)
    except ValueError:
        parser.error("--bind port must be numeric")
    if not 1 <= args.port <= 65535:
        parser.error("--bind port must be between 1 and 65535")
    if args.fault_every_requests <= 0:
        parser.error("--fault-every-requests must be positive")
    if args.max_faults < 0:
        parser.error("--max-faults cannot be negative")
    if args.timeout_delay_s <= 0 or args.upstream_timeout_s <= 0:
        parser.error("proxy timeouts must be positive")
    try:
        args.fault_sequence = parse_fault_sequence(args.fault_sequence)
    except ValueError as error:
        parser.error(str(error))
    args.host = host
    return args


def main() -> int:
    args = parse_args()
    upstream_url = resolve_upstream_url(args.upstream_url, args.upstream_url_env)
    log_path = Path(args.log)
    ready_path = Path(args.ready_file)
    if log_path.exists() or ready_path.exists():
        raise RuntimeError("fault-proxy log and ready-file paths must be fresh")
    log_path.parent.mkdir(parents=True, exist_ok=True)
    ready_path.parent.mkdir(parents=True, exist_ok=True)
    state = FaultState(
        upstream_url=upstream_url,
        every_requests=args.fault_every_requests,
        sequence=args.fault_sequence,
        timeout_delay_s=args.timeout_delay_s,
        upstream_timeout_s=args.upstream_timeout_s,
        max_faults=None if args.max_faults == 0 else args.max_faults,
        log_path=log_path,
    )
    server = ThreadingHTTPServer((args.host, args.port), handler_for(state))
    server.daemon_threads = True
    ready_path.write_text(
        json.dumps(
            ready_payload(
                bind=f"{args.host}:{args.port}",
                every_requests=args.fault_every_requests,
                sequence=args.fault_sequence,
                max_faults=args.max_faults,
            ),
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    state.log(action="started", bind=f"{args.host}:{args.port}")

    stop = threading.Event()

    def request_stop(_signum: int, _frame: object) -> None:
        if not stop.is_set():
            stop.set()
            threading.Thread(target=server.shutdown, daemon=True).start()

    signal.signal(signal.SIGINT, request_stop)
    signal.signal(signal.SIGTERM, request_stop)
    try:
        server.serve_forever(poll_interval=0.2)
    finally:
        server.server_close()
        state.log(action="stopped")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
