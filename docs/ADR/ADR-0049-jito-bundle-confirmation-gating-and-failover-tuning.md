# ADR-0049: Jito bundle confirmation gating and failover tuning

**Date:** 2026-03-28
**Status:** Accepted
**Author:** Ghost Father

## Context

`ADR-0047` established public Jito JSON-RPC failover and launcher-side tip-floor hardening as the accepted production direction.

A follow-up verification showed the implementation was still incomplete in two materially important ways:

1. runtime failover parameters were still too conservative for public-endpoint congestion,
2. `trigger` runtime code had already moved to a receipt / bundle-UUID confirmation contract in some call sites, but `jito_client.rs` had not yet fully implemented that contract.

The approved implementation scope for this round was intentionally narrow:

- **in scope:**
  - increase retry rounds,
  - lengthen per-endpoint timeout,
  - lengthen failover round backoff,
  - enforce preferred regional failover order,
  - add receipt-based bundle submission and explicit confirmation polling helpers,
  - keep tip-floor logic in the launcher layer,
- **out of scope:**
  - dynamic tip escalation inside `jito_client.rs` after `-32097`,
  - rate-limit driven bundle param rewriting in the submit loop.

That scope boundary was explicit and had to be preserved.

## Decision

The runtime keeps launcher-owned tip-floor handling and tightens only the Jito client execution contract required by the existing trigger code.

The accepted decisions are:

1. **Public JSON-RPC submit retries increase from 2 to 3 rounds**
   - `MAX_BUNDLE_RETRIES = 3`.
   - This yields one additional full regional sweep before terminal failure.

2. **Per-endpoint submit timeout increases from 750ms to 1200ms**
   - this reduces false `RetryableTransport` classification during public congestion.

3. **Failover round backoff increases from 400ms to 600ms exponential base**
   - effective backoff sequence becomes `600ms -> 1200ms -> 2400ms`.

4. **Regional failover order becomes explicit and operator-preferred**
   - order is:
     - Frankfurt,
     - Amsterdam,
     - London,
     - Dublin,
     - New York,
     - Singapore,
     - Tokyo,
     - Salt Lake City.
   - the public alias `mainnet.block-engine.jito.wtf` is treated as a valid trigger for the full regional sweep.

5. **Bundle submission becomes receipt-aware where the runtime already expects it**
   - `JitoBundleSubmission` returns:
     - transaction signature,
     - Jito bundle UUID.
   - `submit_bundle_with_redundancy_receipt(...)` becomes the authoritative multi-submit receipt API.
   - single-transaction Jito submission now also returns a receipt instead of only a signature.

6. **Confirmed SELL-path Jito execution is first-class**
   - `JitoConfirmedBundle` represents a bundle that both submitted and reached Jito confirmation.
   - `submit_single_transaction_and_confirm(...)` blocks on bundle status and only returns success when the bundle is accepted.

7. **Bundle status polling gains an explicit timeout helper**
   - `wait_for_bundle_status_with_timeout(...)` returns terminal state when reached,
   - returns `Pending` on timeout so the caller can enforce policy,
   - this is the intended contract for `bundle_builder.rs` confirmation gating.

8. **Tip floor remains in the launcher layer**
   - no dynamic tip-floor fetch or tip escalation logic is added to `jito_client.rs` in this ADR,
   - `ghost-launcher/src/components/trigger/component.rs` remains the authoritative place where the requested tip is raised to the observed Jito floor before signing.

## Architectural Impact

Affected components:

- `off-chain/components/trigger/src/jito_client.rs`
- `off-chain/components/trigger/src/bundle_builder.rs`
- `off-chain/components/trigger/src/revolver_price_feed.rs`
- `off-chain/components/trigger/src/ipc_integration.rs`
- `off-chain/components/trigger/src/main.rs`

System effects:

- BUY and SELL bundle flows now share a consistent receipt / bundle-UUID contract,
- Jito confirmation gating is no longer half-implemented across call sites,
- public failover behavior is better aligned with live slot timing,
- tip-floor responsibility remains outside the runtime submit client.

This ADR also clarifies the layering boundary:

- **launcher** owns market-aware tip-floor uplift before signing,
- **trigger Jito client** owns bundle submission, failover, receipts, and confirmation polling.

## Risk Assessment

**Rate:** Medium

Why medium:

- public Jito live behavior changes due to longer timeouts and one extra retry round,
- single-transaction submit now returns a richer receipt type used across multiple call sites,
- confirmation gating is stricter and can intentionally suppress follow-on actions when Jito never confirms landing.

Why not high:

- scope explicitly excludes dynamic tip mutation in the runtime submit loop,
- tip-floor ownership did not move,
- validation covered both unit-level Jito behavior and dedicated bundle integration tests.

## Consequences

### Positive

- fewer false terminal failures under public Jito congestion,
- consistent bundle UUID availability for downstream confirmation logic,
- SELL-path Jito execution can now enforce actual landing confirmation,
- runtime and launcher layering is clearer.

### Trade-offs

- public failure resolution may take longer before terminal error,
- receipt-based submit APIs are slightly more complex than signature-only helpers,
- callers must interpret `Pending` timeout as policy failure when strict confirmation is required.

## Alternatives Considered

### 1. Keep 2 retry rounds and the shorter timeouts

Rejected because the current public Jito behavior already proved those values too aggressive under congestion.

### 2. Move tip-floor fetching into `jito_client.rs`

Rejected because this would blur the established launcher/runtime boundary and re-open out-of-scope work from points 4 and 5.

### 3. Keep confirmation logic only in `bundle_builder2.rs` via raw HTTP polling

Rejected because the repo had already converged on a client-owned receipt/status contract; duplicating transport logic in the builder would worsen drift.

### 4. Leave single-transaction Jito submit as signature-only

Rejected because existing runtime call sites already required bundle UUID-aware behavior and confirmed SELL execution.

## Validation Steps

Verified in-session:

1. `cargo test -p trigger jito_client -- --nocapture`
   - result: passed
   - notable output: `22 passed; 0 failed`

2. `cargo test -p trigger --test jito_bundle_integration -- --nocapture`
   - result: passed
   - notable output: `9 passed; 0 failed`

3. VS Code diagnostics check on touched trigger files
   - result: no source errors in:
     - `jito_client.rs`
     - `bundle_builder.rs`
     - `main.rs`
     - `ipc_integration.rs`
     - `revolver_price_feed.rs`

Operational follow-up before production closure:

1. observe live public Jito submit latency with the new `1200ms` timeout,
2. compare bundle failures before vs after the third retry round,
3. confirm SELL-path gating blocks when bundle status remains pending or rejected,
4. keep points 4 and 5 as a separate explicitly approved change if runtime tip escalation is still desired later.
