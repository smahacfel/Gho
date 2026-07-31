#!/usr/bin/env python3
"""Focused deterministic tests for the non-authoritative metrics fault proxy."""

from __future__ import annotations

import importlib.util
import os
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT_PATH = Path(__file__).with_name("ace_capture_metrics_fault_proxy.py")
SPEC = importlib.util.spec_from_file_location("ace_capture_metrics_fault_proxy", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
proxy = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = proxy
SPEC.loader.exec_module(proxy)


class AceCaptureMetricsFaultProxyTests(unittest.TestCase):
    def test_fault_sequence_is_explicit_and_rejects_unknown_values(self) -> None:
        self.assertEqual(
            proxy.parse_fault_sequence("timeout,http_429,connection_reset"),
            ("timeout", "http_429", "connection_reset"),
        )
        with self.assertRaisesRegex(ValueError, "unsupported"):
            proxy.parse_fault_sequence("timeout,process_shutdown")

    def test_faults_are_deterministic_and_never_apply_to_other_requests(self) -> None:
        sequence = ("timeout", "http_429", "http_500")
        self.assertIsNone(proxy.fault_for_request(29, every_requests=30, sequence=sequence))
        self.assertEqual(
            proxy.fault_for_request(30, every_requests=30, sequence=sequence), "timeout"
        )
        self.assertEqual(
            proxy.fault_for_request(60, every_requests=30, sequence=sequence), "http_429"
        )
        self.assertEqual(
            proxy.fault_for_request(90, every_requests=30, sequence=sequence), "http_500"
        )

    def test_max_faults_bounds_injection_without_affecting_forwarding_path(self) -> None:
        state = proxy.FaultState(
            upstream_url="http://127.0.0.1:9/metrics",
            every_requests=1,
            sequence=("timeout",),
            timeout_delay_s=1.0,
            upstream_timeout_s=1.0,
            max_faults=2,
            log_path=Path(tempfile.mkdtemp(prefix="ace-fault-proxy-test-")) / "proxy.jsonl",
        )
        self.assertEqual(state.next_fault(), (1, "timeout"))
        self.assertEqual(state.next_fault(), (2, "timeout"))
        self.assertEqual(state.next_fault(), (3, None))

    def test_ready_payload_never_persists_the_upstream_url(self) -> None:
        payload = proxy.ready_payload(
            bind="127.0.0.1:19191",
            every_requests=1,
            sequence=("timeout",),
            max_faults=4,
        )
        self.assertEqual(payload["upstream_configured"], True)
        self.assertNotIn("upstream_url", payload)

    def test_upstream_url_can_be_loaded_without_putting_its_value_in_argv(self) -> None:
        previous = os.environ.get("ACE_PROXY_TEST_UPSTREAM")
        os.environ["ACE_PROXY_TEST_UPSTREAM"] = "https://rpc.example.test/private-token"
        try:
            self.assertEqual(
                proxy.resolve_upstream_url(None, "ACE_PROXY_TEST_UPSTREAM"),
                "https://rpc.example.test/private-token",
            )
        finally:
            if previous is None:
                os.environ.pop("ACE_PROXY_TEST_UPSTREAM", None)
            else:
                os.environ["ACE_PROXY_TEST_UPSTREAM"] = previous

    def test_forwarding_retains_upstream_content_encoding(self) -> None:
        content_type, content_encoding = proxy.upstream_response_metadata(
            {"Content-Type": "application/json", "Content-Encoding": "gzip"}
        )
        self.assertEqual(content_type, "application/json")
        self.assertEqual(content_encoding, "gzip")


if __name__ == "__main__":
    unittest.main()
