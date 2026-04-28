# ADR-0044: Public Jito endpoint normalization and rate-limit-aware preflight

**Date:** 2026-03-28
**Status:** Accepted
**Author:** Ghost Father

## Context

PR-7 first live entry remained blocked after the dual wallet and balance issues were resolved.

The last failing gate was the Jito endpoint preflight used by `ghost-launcher --preflight` via `scripts/ghost_production_preflight.sh`.

Direct JSON-RPC probing established a precise external contract for public regional Jito block-engine hosts:

1. Bare public hosts such as `https://frankfurt.mainnet.block-engine.jito.wtf` and `https://amsterdam.mainnet.block-engine.jito.wtf` do **not** serve `getTipAccounts` successfully at the root path.
2. `/api/v1` also does **not** provide the usable bundle JSON-RPC endpoint for this probe.
3. `/api/v1/bundles` is the canonical public JSON-RPC path for `getTipAccounts`.
4. A public endpoint can be fully reachable yet temporarily return `429` / JSON-RPC congestion or rate-limit errors instead of a normal `result` array.

That means the previous repository contract was too strict in two ways:

- it depended on operator-supplied endpoint formatting being already correct,
- it treated temporary public rate-limit responses as if the endpoint itself were broken.

This incorrectly pushed an infrastructure/contract bug onto the operator, even though the reachable public endpoints were valid.

## Decision

Normalize all operator-supplied Jito public endpoints to the canonical JSON-RPC bundle path:

`/api/v1/bundles`

and classify preflight outcomes into two success classes:

1. **TipAccounts success** — endpoint returns a valid `getTipAccounts` result array.
2. **Reachable but rate-limited success** — endpoint is reachable at the normalized bundle path but currently returns public congestion / rate-limit semantics.

Implementation details accepted as SSOT:

- `off-chain/components/trigger/src/jito_client.rs` owns endpoint normalization and Jito probe behavior.
- `JitoClient::new(...)` and `JitoClient::new_with_uuid(...)` normalize the runtime submission endpoint, not only preflight.
- launcher preflight delegates to the trigger-side probe helper instead of maintaining a separate Jito contract.
- true failures remain only for:
  - missing Jito endpoint,
  - unreachable endpoint,
  - wrong/unsupported response after normalization,
  - malformed endpoint input.

A temporary public `429` / `-32097` congestion response is explicitly **not** a startup blocker anymore.

## Architectural Impact

This aligns runtime and preflight behind a single Jito endpoint contract:

- **runtime** submits bundles to normalized `/api/v1/bundles`,
- **preflight** probes the same normalized path,
- **operator input** no longer needs secret undocumented path knowledge.

The change reduces divergence between:

- launcher-side preflight validation,
- trigger-side Jito runtime submission,
- real-world public Jito behavior under congestion.

This also keeps `use_jito = true` intact for the approved dual rollout profile without forcing a scope-expanding config redesign.

## Risk Assessment

**Rate:** Medium

Why not low:

- preflight now accepts a reachable but temporarily rate-limited Jito endpoint, so a later runtime submission can still fail under live congestion.
- normalization changes the effective runtime URL for some operator inputs.

Why not high:

- the accepted rate-limited state is backed by direct JSON-RPC evidence from the real public endpoint.
- runtime and preflight now share one contract instead of two divergent behaviors.
- invalid or unreachable endpoints still fail explicitly.

## Consequences

What becomes easier:

- public Jito endpoints work out of the box when the operator provides only the regional host,
- preflight validates reachability instead of demanding zero market congestion,
- PR-7 dual preflight can succeed on realistic public infrastructure.

What becomes harder:

- operators must understand that preflight success for a rate-limited endpoint means “reachable and contract-valid”, not “guaranteed uncongested at runtime”.

## Alternatives Considered

### 1. Require the operator to manually discover the exact hidden Jito path

Rejected because the public host is a valid operator input and the repository should own path normalization.

### 2. Keep treating public `429` / congestion as a hard invalid-endpoint error

Rejected because this confuses temporary network conditions with broken configuration.

### 3. Disable Jito for dual micro-live

Rejected because `dual-micro-live.toml` explicitly requires `use_jito = true`; changing that would be a separate rollout decision outside scope.

## Validation Steps

1. Unit-test trigger-side Jito normalization behavior.
2. Unit-test launcher preflight acceptance of a reachable but rate-limited Jito endpoint.
3. Re-run targeted launcher bin tests for `test_run_preflight*`.
4. Re-run real dual preflight:
   `./scripts/ghost_production_preflight.sh --config /root/Gho/configs/rollout/dual-micro-live.toml`
5. Confirm the real dual preflight reports all checks `[ok]`, including Jito.
