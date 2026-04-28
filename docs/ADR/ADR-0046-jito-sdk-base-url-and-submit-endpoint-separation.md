# ADR-0046: Jito SDK Base URL and Submit Endpoint Separation

**Date:** 2026-03-28
**Status:** Accepted
**Author:** Ghost Father

## Context

`dual-micro-live` first live run exposed a deterministic runtime failure in the Jito BUY path:

- preflight probe was green,
- shadow lane executed,
- live candidates were produced,
- but live BUY submission repeatedly failed with:
  - `error decoding response body: EOF while parsing a value at line 1 column 0`

RCA in `ADR-0045` proved the immediate cause:

- repo normalization produced a canonical endpoint ending in `.../api/v1/bundles`,
- `jito-sdk-rust` expects a **base URL** and appends `"/bundles"` itself,
- runtime therefore submitted to `.../api/v1/bundles/bundles`.

This created a false-green preflight/runtime split:

- preflight checked the correct bundle JSON-RPC path,
- runtime submit used a duplicated path and received `404` with empty body.

## Decision

`JitoClient` now stores and uses two different URL forms with distinct responsibilities:

1. **Canonical submit endpoint**
   - stored in `endpoint`
   - normalized to `.../api/v1/bundles`
   - used for diagnostics/logging and as the operator-facing canonical bundle URL

2. **SDK base URL**
   - stored in `sdk_base_url`
   - normalized to `.../api/v1`
   - passed into `jito-sdk-rust`, which appends operation suffixes like `"/bundles"`

A dedicated helper `normalize_jito_sdk_base_url(...)` converts operator input into the SDK-safe base URL.

Constructor behavior was updated so that:

- `JitoClient::new(...)` and `JitoClient::new_with_uuid(...)` no longer pass `.../api/v1/bundles` as the SDK base,
- runtime `send_bundle()` now resolves to the same logical path family as preflight.

Regression tests were added to lock this contract:

- canonical endpoint remains `.../api/v1/bundles`,
- SDK base URL becomes `.../api/v1`,
- host-only operator input is normalized into those two distinct forms.

## Architectural Impact

Affected components:

- `off-chain/components/trigger/src/jito_client.rs`
- runtime Jito submission path used by `ghost-launcher/src/components/trigger/component.rs`
- operational contract between launcher preflight and live runtime submit

Architectural outcome:

- preflight and runtime no longer disagree on the logical Jito route family,
- `jito-sdk-rust` is now fed the URL shape it actually expects,
- live BUY path no longer depends on an accidental duplicated `/bundles` suffix.

## Risk Assessment

**Rate:** Medium

Regression risks:

- low risk to preflight, because probe logic was not changed,
- medium risk to runtime Jito flows if there are untested callers that relied on the old, incorrect stored semantics,
- low risk to non-Jito execution paths.

Main safety concern is ensuring future code does not again conflate:

- canonical JSON-RPC operation URL,
- SDK base URL.

## Consequences

### Positive

- fixes the proven runtime/preflight mismatch,
- restores a consistent URL contract for Jito submit,
- adds test coverage specifically against the path duplication regression.

### Trade-offs

- `JitoClient` now carries two URL representations instead of one,
- future maintainers must preserve the distinction between canonical submit endpoint and SDK base URL.

## Alternatives Considered

### 1. Keep one normalized endpoint and modify preflight instead

Rejected because preflight was already probing the correct route; runtime was the broken half.

### 2. Bypass `jito-sdk-rust` entirely and hand-roll bundle submission

Rejected for this fix because it would expand scope beyond the narrow runtime path regression.

### 3. Treat the issue as transient public-endpoint instability

Rejected because the duplicated path was reproduced deterministically and matched the observed `EOF` failure mode exactly.

## Validation Steps

Verified in-session with:

1. Targeted diagnostics on edited files: no editor errors.
2. `cargo test -p trigger jito_client -- --nocapture`
   - unit tests passed
   - new regression test passed
3. Trigger integration tests selected by the same command path also passed, including:
   - `test_jito_client_builder_pattern`

Follow-up runtime validation before another live attempt:

1. rerun dual preflight,
2. rerun controlled `dual-micro-live`,
3. verify absence of `.../bundles/bundles` symptom,
4. confirm at least one live BUY path completes without `Jito bundle submission failed`.
