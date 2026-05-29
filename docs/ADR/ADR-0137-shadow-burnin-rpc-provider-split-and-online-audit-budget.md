# ADR-0137: Shadow burn-in RPC provider split and online audit budget

Date: 2026-05-29

Status: Accepted

## Context

The `shadow-burnin-v3-p1` collector was migrated to the NLN gRPC/Program Stream
provider. The gRPC ingest path became healthy, but the HTTP RPC path used by
shadow simulation and helper hydration failed at the provider-auth layer.

The hard artifact boundary was visible in:

```text
logs/shadow_run/shadow-burnin-v3-p1/shadow_lifecycle.jsonl
```

Line `1761` was still a normal `position_closed` lifecycle row. Line `1762`
was the first failing `shadow_dispatch` row:

```text
dispatch_status = failed
classification = network_provider_problem
simulation_outcome = failed
err = Failed to fetch payer balance: RPC request error:
      cluster version query failed:
      HTTP status client error (401 Unauthorized)
      for url (https://rpc.nln.clr3.org/)
```

That failure was not a Yellowstone/gRPC ingest failure. It was an HTTP JSON-RPC
authorization failure in the shadow simulation/RPC helper path. The NLN HTTP RPC
endpoint requires an auth mechanism that the plain Solana RPC client path did
not provide when it was given only the endpoint URL.

After moving history/audit RPC to Alchemy, a second problem appeared:

```text
seer_runtime_coverage_audit.jsonl
audit_status = rpc_error
rpc_error = get_signatures_failed: HTTP status client error (429 Too Many Requests)
```

The provider supported historical methods, but the online coverage-audit path
was too expensive for the hot collector. It queried `getSignaturesForAddress`
with a large page size and then attempted transaction reconstruction for each
candidate signature. This made the hot collector vulnerable to provider CUPS
limits and could obscure the actual shadow simulation state.

## Decision

Separate provider roles instead of treating one RPC endpoint as fit for every
runtime responsibility:

1. Keep NLN Program Streams / gRPC as the live ingest source.
2. Use an operational Solana JSON-RPC endpoint for trigger/shadow simulation.
3. Use an archive/history-capable endpoint for replay and coverage-audit
   signature truth.
4. Keep full transaction reconstruction out of the hot coverage-audit loop.

The active environment now follows this role split:

```text
GHOST_SEER_RPC_ENDPOINT          -> history/audit RPC provider
GHOST_TRIGGER_RPC_URL            -> operational trigger RPC provider
GHOST_TRIGGER_SHADOW_RPC_URL     -> operational shadow simulation RPC provider
GHOST_TRIGGER_KEYPAIR_PATH       -> absolute funded shadow wallet path
```

The exact tokenized endpoints are intentionally not copied into this ADR.

## Implementation

### Runtime config

`configs/rollout/shadow-burnin.toml` was updated:

```toml
[seer]
grpc_stall_timeout_secs = 20

[trigger.shadow_run]
timeout_ms = 5000
max_concurrent = 1
```

This keeps the collector sensitive to real gRPC stalls, while making shadow
simulation less fragile under provider latency and preventing simulation fan-out
from multiplying RPC pressure.

`.env` was updated so that:

- seer history/audit RPC points to the Alchemy Solana mainnet provider,
- trigger and shadow simulation RPC point to the same Alchemy Solana mainnet
  provider after the Chainstack operational RPC path produced provider-side
  `403`/`429` and missing-account precheck failures,
- the trigger keypair path is absolute.

### Online coverage audit

`ghost-launcher/src/oracle_runtime.rs` was updated so the hot
`coverage_audit_rpc` path is bounded:

```text
COVERAGE_AUDIT_RPC_MIN_INTERVAL_MS        = 10_000
COVERAGE_AUDIT_RPC_RATE_LIMIT_BACKOFF_MS  = 30_000
COVERAGE_AUDIT_SIGNATURE_PAGE_LIMIT       = 100
COVERAGE_AUDIT_SIGNATURE_MAX_PAGES        = 2
```

The hot audit path now uses `getSignaturesForAddress` as signature-level chain
truth and records `rpc_signature_block_time` as the truth time source. It no
longer performs `getTransaction` for every candidate signature inside the hot
collector loop.

This is an intentional online/offline split:

- online collector: bounded signature-level coverage evidence,
- offline/archive tooling: full transaction reconstruction, WSOL/inner
  instruction classification, getBlock/getTransaction replay.

### New pool evidence

`ghost-launcher/src/oracle_runtime.rs` also emits durable
`NewPoolDetected` evidence rows for offline selector denominator construction
when `GhostEvent::NewPoolDetected` enters `OracleRuntime`.

That event emission is evidence-only. It must not affect Gatekeeper policy,
feature materialization, trigger execution, shadow/live mode, or route
readiness.

## Verification

Release build passed:

```text
cargo build --release -p ghost-launcher
```

Whitespace/diff sanity passed:

```text
git diff --check
```

The collector was restarted from the rebuilt release binary:

```text
target/release/ghost-launcher --config /root/Gho/configs/rollout/shadow-burnin.toml
```

Fresh runtime evidence after the final restart showed the ingest and coverage
audit paths healthy:

```text
gRPC ingest:
  fresh PoolTransaction source=grpc_global_stream
  fresh DIAG_ACCOUNT_UPDATE_RELAY
  fresh DIAG_ACCOUNT_UPDATE_APPLIED
  fresh SEER_COVERAGE

seer_runtime_coverage_audit since final restart:
  fresh rows present
  audit_status=ok for sampled post-restart rows
  rpc_error rows: 0
```

Later shadow lifecycle sampling showed that trigger/shadow simulation could hit
provider-side `429 Too Many Requests` and missing-account precheck failures while
the operational RPC endpoint still pointed at Chainstack. Direct `getAccountInfo`
checks against that endpoint returned `403 Forbidden`, while the same checks
against Alchemy returned account data. The active trigger/shadow RPC env values
were therefore moved to Alchemy as well.

The dominant remaining shadow lifecycle failures after this provider split are
account/route readiness failures, primarily:

```text
no_executable_route_account_set:legacy_buy_curve_readiness_not_checked
no_executable_route_account_set:primary_route_bcv2_missing
```

Those are a separate execution-readiness problem and must be investigated
without conflating them with the earlier NLN HTTP RPC `401` or Alchemy `429`
provider issues.

## Consequences

Positive:

- gRPC ingest remains on the provider that currently works for live feed.
- Shadow simulation no longer depends on the unauthenticated NLN HTTP RPC path.
- History/audit is separated from operational simulation RPC.
- Alchemy rate-limit pressure from hot coverage audit is reduced.
- Coverage audit remains durable and append-only, but uses bounded
  signature-level evidence online.
- Full transaction replay is not accidentally performed in the latency-sensitive
  collector loop.

Negative:

- Online coverage audit no longer performs full transaction reconstruction.
- WSOL context classification, inner-instruction classification, and
  getTransaction/getBlock replay must be handled by offline/archive tooling.
- Signature-level chain truth can prove coverage timing and presence, but not
  every semantic reconstruction detail.

## Non-goals

This ADR does not authorize:

- live execution,
- Gatekeeper policy changes,
- TX builder rewrites,
- MaterializedFeatureSet changes,
- treating shadow simulation as live inclusion,
- using NLN HTTP RPC for unauthenticated JSON-RPC calls,
- using the hot collector as the primary archive replay/indexing backend.

## Follow-up

The first `no_executable_route_account_set` pass found two separate classes:

```text
no_executable_route_account_set:legacy_buy_curve_readiness_not_checked
no_executable_route_account_set:primary_route_bcv2_missing
```

Some rows are real fail-closed route/account readiness failures. However, a
subset was internally contradictory: the `err` field started with
`no_executable_route_account_set`, while later diagnostics for the same lifecycle
row reported:

```text
route_resolution_status = primary_route_ready | fallback_route_ready
selected_route_kind     = legacy_buy
execution_feasibility   = executable
legacy_buy_route_ready  = true
```

The active shadow precheck was the divergence point. In the immediate
pre-dispatch missing-account branch, it called the route resolver without the
fresh `account_set_diagnostics` that the later lifecycle diagnostics used. That
allowed an early missing-account observation to be classified as
`no_executable_route_account_set:*:not_checked`, even when the full manifest RPC
check resolved the currently prepared legacy route as executable.

`ghost-launcher/src/oracle_runtime.rs` was updated so active shadow precheck:

- recomputes fresh `account_set_diagnostics` before classifying
  missing-account and BCV2-source precheck failures,
- does not emit `no_executable_route_account_set` when the full resolver says
  the selected route is executable for the already prepared request,
- does not treat an unapplied fallback route as ready for the current prepared
  request.

This preserves fail-closed behavior for real route failures while preventing the
specific false `no_executable_route_account_set` classification seen in the
contradictory lifecycle rows.

Verification:

```text
cargo test -p ghost-launcher --lib active_shadow_precheck_ -- --nocapture
  3 passed

cargo build --release -p ghost-launcher
  passed

git diff --check
  passed
```

Post-fix activation used the rebuilt release binary:

```text
tmux session: collector_grpc_noexecfix_20260529
process: target/release/ghost-launcher --config /root/Gho/configs/rollout/shadow-burnin.toml
```

The post-restart artifact baselines were:

```text
shadow_lifecycle.jsonl              baseline line: 3309
seer_runtime_coverage_audit.jsonl   baseline line: 24459
```

Fresh post-restart evidence:

```text
seer_runtime_coverage_audit.jsonl lines 24460-24461:
  audit_status=ok
  rpc_error=null

shadow_lifecycle.jsonl lines 3310-3324:
  simulation_completed rows: 4
  program simulation mismatch rows: 2
  network_provider_problem rows: 1
  no_executable_route_account_set rows: 1
  contradictory no_executable_route_account_set + executable rows: 0
  HTTP 401 rows: 0
  HTTP 429 rows: 1
```

The remaining `no_executable_route_account_set` row in the post-fix sample was
not the false contradictory class. It was fail-closed with:

```text
route_resolution_status = no_executable_route_account_set
execution_feasibility_status = not_executable_route
err = no_executable_route_account_set:legacy_buy_simulation_load_not_ready:bonding_curve:...
```

The post-fix sample also contained one operational RPC rate-limit row from
shadow simulation while trigger/shadow RPC still pointed at Chainstack:

```text
HTTP status client error (429 Too Many Requests)
```

That row was transport/capacity pressure on the old operational RPC provider and
is separate from the fixed `no_executable_route_account_set` classification
divergence.

After updating `GHOST_TRIGGER_RPC_URL` and `GHOST_TRIGGER_SHADOW_RPC_URL` to the
Alchemy endpoint, the collector was restarted again:

```text
tmux session: collector_rpc_alchemy_noexecfix_20260529
process: target/release/ghost-launcher --config /root/Gho/configs/rollout/shadow-burnin.toml
```

The Alchemy activation baseline was:

```text
shadow_lifecycle.jsonl              baseline line: 3887
seer_runtime_coverage_audit.jsonl   baseline line: 24547
```

Fresh Alchemy-trigger/shadow evidence:

```text
shadow_lifecycle.jsonl lines 3888-3911:
  shadow_dispatch rows: 12
  simulation_completed rows: 6
  program simulation mismatch rows: 6
  no_executable_route_account_set rows: 0
  HTTP 401 rows: 0
  HTTP 403 rows: 0
  HTTP 429 rows: 0

seer_runtime_coverage_audit.jsonl lines 24548-24551:
  audit_status=ok
  rpc_error=null
```

The remaining failed shadow dispatch rows in that post-Alchemy sample are
program-level simulation failures (`Custom(6062)`, `Custom(2006)`,
`Custom(6024)`) with `route_resolution_status=primary_route_ready` and
`execution_feasibility_status=executable`. They are not RPC provider
authorization, rate-limit, or route-account-set failures.

One unrelated integration-test fixture still fails under full
`cargo test -p ghost-launcher` because
`ghost-launcher/tests/gatekeeper_pdd_tests.rs` constructs `PoolTransaction`
without the newer `bonding_curve_v2`, `bonding_curve_v2_provenance`, and
`buy_remaining_accounts` fields. That failure is pre-existing relative to this
runtime fix and is not caused by the active shadow precheck change.

The remaining investigation should preserve the distinction between:

- provider transport failures,
- missing/late account evidence,
- route manifest construction failures,
- route readiness classification,
- real program-level simulation failures.
