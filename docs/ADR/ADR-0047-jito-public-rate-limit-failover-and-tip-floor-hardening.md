# ADR-0047: Jito public rate-limit failover and tip-floor hardening

**Date:** 2026-03-28
**Status:** Accepted
**Author:** Ghost Father

## Context

After `ADR-0046` fixed the deterministic runtime URL mismatch (`.../bundles/bundles`), a fresh controlled runtime pass proved that the live Jito path was no longer failing on an invalid endpoint contract.

At that point the dominant remaining live BUY failure mode changed from deterministic URL breakage to public Jito congestion / rate-limit behavior, primarily surfaced as JSON-RPC error `-32097` and equivalent temporary transport failures.

Repository and runtime facts verified during this round:

- the active production hot path was still JSON-RPC based, not Jito gRPC based,
- public Jito regional hosts remain subject to per-region congestion / rate limits,
- runtime handling for `-32097` after the URL fix was too weak for sustained public-endpoint use,
- launcher-side tip sizing still relied mainly on static local configuration even though public `tip_floor` data was available.

The operator also supplied an external mitigation direction: rotate endpoints, retry with backoff, raise tip dynamically from `tip_floor`, and consider Jito gRPC later.

That required an explicit scope decision, because an immediate transport migration to Jito gRPC would materially expand the change surface:

- new connection model,
- auth / credential handling,
- operational rollout changes,
- new failure modes distinct from current JSON-RPC runtime behavior.

## Decision

Keep the production runtime on the current JSON-RPC Jito submit path for this change, but harden it against public-endpoint congestion.

The accepted mitigation contract is:

1. **Public endpoint failover is first-class runtime behavior**
   - `off-chain/components/trigger/src/jito_client.rs` maintains the official public regional host set.
   - When the configured endpoint is one of those public hosts, runtime submit builds a deterministic failover chain with the requested region first and the remaining public regions following.

2. **`-32097`, `429`, and retryable transport failures are treated as failoverable, not fatal on first contact**
   - submit attempts are classified into:
     - `RateLimited`,
     - `RetryableTransport`,
     - `Fatal`.
   - only `Fatal` errors stop rotation immediately.

3. **Bundle submission uses direct JSON-RPC POST control for the hot send path**
   - status-oriented SDK behavior may remain where useful,
   - but runtime `sendBundle` must keep repo-owned control over:
     - exact endpoint,
     - HTTP timeout,
     - retry classification,
     - multi-region failover order.

4. **Failover uses bounded round backoff rather than immediate hammering**
   - after a failoverable round exhausts available public endpoints, runtime waits approximately one slot (`400 ms`, exponential by round) before retrying.

5. **Launcher BUY preparation raises the requested Jito tip to live market floor before signing**
   - launcher fetches `https://bundles.jito.wtf/api/v1/bundles/tip_floor`,
   - preferred floor source is `p75`,
   - fallback order is EMA / `p50` / `p25`,
   - resulting requested tip is raised to at least that floor before request preparation.

6. **Existing rollout safety controls remain authoritative**
   - dynamic tip-floor raise happens before downstream budget resolution,
   - existing TipGuard / bulkhead / balance clipping remain the final safety boundary,
   - this ADR does not authorize uncapped tip escalation.

7. **Jito gRPC migration is explicitly deferred to a separate architectural task**
   - the current change is transport hardening for the existing production path,
   - not a silent transport migration.

## Architectural Impact

Affected components:

- `off-chain/components/trigger/src/jito_client.rs`
- `ghost-launcher/src/components/trigger/component.rs`
- `ghost-launcher/Cargo.toml`

System-level effect:

- the runtime Jito submit path is no longer single-endpoint brittle against public `-32097` congestion,
- endpoint reachability and runtime resilience now extend beyond preflight correctness into actual submit behavior,
- launcher tip sizing becomes partially market-aware while still obeying local risk budgets.

The architectural boundary is also now explicit:

- **this ADR** = JSON-RPC hardening on the current live path,
- **future ADR** = possible Jito gRPC migration if separately approved.

## Risk Assessment

**Rate:** Medium

Why not low:

- runtime now performs multi-endpoint submit rotation, which changes live request behavior under failure,
- live tip requests now depend on a public `tip_floor` API,
- broad launcher integration tests remain memory-constrained on this workstation, so validation was narrowed to touched logic.

Why not high:

- changes are confined to the Jito submit/tip path,
- existing balance and tip caps remain in force,
- targeted regression tests cover endpoint ordering, retry classification, failover success, and tip-floor selection.

## Consequences

### Positive

- public Jito congestion is now handled as a runtime resilience problem rather than as an immediate terminal failure,
- live BUY path can rotate across official public regions instead of repeatedly failing on one saturated host,
- requested tips are less likely to undershoot current bundle-market conditions.

### Trade-offs

- runtime behavior is more complex than single-endpoint submit,
- one more public dependency (`tip_floor`) participates in pre-signing tip selection,
- operators must not misread this as a guarantee that public Jito will always succeed under extreme congestion.

## Alternatives Considered

### 1. Immediate migration to Jito gRPC

Rejected for this fix because it would materially expand scope beyond the proven current bottleneck and would require a new rollout and auth contract.

### 2. Keep single-endpoint JSON-RPC submit and only retry locally

Rejected because the dominant remaining failures came from public regional congestion; retrying the same saturated host was not sufficient.

### 3. Raise tips statically from config only

Rejected because the bundle market is dynamic and public `tip_floor` data was already available for a narrower, safer improvement.

### 4. Treat `-32097` as terminal operator-visible failure

Rejected because this would preserve a known runtime weakness after the endpoint contract itself was already fixed.

## Validation Steps

Verified in-session with targeted tests:

1. `cargo test -p trigger --lib jito_client::tests -- --nocapture`
   - result: `21 passed; 0 failed`
2. `cargo test -p ghost-launcher --lib select_jito_tip_floor_lamports -- --nocapture`
   - result: `2 passed; 0 failed`

Additional validation facts:

- edited Rust files reported no source diagnostics after the implementation,
- one incorrect test expectation around normalized custom endpoint shape was fixed and rerun green,
- broad `ghost-launcher` test targets were not used as the main acceptance gate because the machine hit linker OOM on a heavy integration target.

Required follow-up validation before declaring the mitigation operationally closed:

1. rerun controlled live observation on the rollout profile,
2. compare `-32097` frequency before vs after failover/tip-floor hardening,
3. confirm at least one clean live Jito submit under the new policy,
4. keep Jito gRPC as a separate, explicitly approved future task rather than folding it into this fix retroactively.