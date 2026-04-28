# ADR-0027: Session Bridge Buffer Revert and Silent-Drop Restore

**Date:** 2026-03-22  
**Status:** Accepted  
**Author:** Ghost Father  

## Context

After enabling short-lived buffering for unknown-pool trades in `ghost-launcher/src/components/seer.rs`, production runtime showed two unacceptable outcomes:

1. The original Phase-5 symptom remained present in fresh `gatekeeper_v2_buys.jsonl` output:
   - `dev_buy_total_sol = 0.0`
   - often together with `dev_wallet_known=true` and `dev_has_sold=true`
2. The launcher started emitting new operational noise:
   - `Seer: trade buffer EXPIRED - pool detected too late or missing, trade DROPPED ...`
   - this created log spam on the hot path and introduced a new failure surface that did not exist before the change.

This means ADR-0026's assumption was wrong in practice for the active runtime: the launcher-side unknown-pool trade buffer did not eliminate the `dev_buy_total_sol=0.0` issue, but it did add a regression in the form of expiry-driven warning spam and extra buffering behavior on global-stream traffic.

## Decision

Revert the launcher-side unknown-pool trade buffering and restore the prior silent-drop contract.

Specifically:

- `SessionTradeDecision::BufferedPending` is removed from the active contract.
- `SessionPoolTradeBridge::ingest_trade(...)` again returns `SilentDrop` for unknown pools.
- Unknown-pool trades are no longer inserted into the short pending buffer on the launcher bridge path.
- The expiry warning path introduced by the buffering change is removed from the active behavior.
- Tests are restored to assert silent-drop semantics for `Trade -> PoolDetected` ordering at the launcher session gate.

ADR-0026 is therefore superseded by this decision.

## Architectural Impact

The launcher-side bridge contract returns to its earlier shape:

- `PoolDetected -> Trade` remains the only forwarding path for a newly tracked pool within a launcher session.
- `Trade -> PoolDetected` is not recovered by the launcher wrapper.
- Unknown-pool trades are treated as non-authoritative session noise and are discarded immediately.

This rollback is intentionally narrow:

- it does not modify Gatekeeper Phase-5 formulas,
- it does not alter Seer parser contracts,
- it does not change ShadowLedger SSOT or replay behavior,
- it only removes the launcher regression introduced by the short-lived unknown-pool buffer.

## Risk Assessment

**Rate:** Medium

Risks accepted by this rollback:

1. The original `dev_buy_total_sol=0.0` issue can still exist, because this rollback is explicitly about removing an ineffective regression, not claiming the metric bug is solved.
2. `Trade -> PoolDetected` races remain unrecovered at the launcher session gate.
3. A future fix for the metric issue must target a different root cause or a different layer.

Risks reduced by this rollback:

1. Removes expiry-driven warn spam from the launcher hot path.
2. Removes short-lived buffering of pre-session global-stream noise.
3. Restores previously understood session-gate semantics.

## Consequences

### Positive

- Hot-path warn spam is removed as a product of behavior, not log-level suppression.
- Launcher session-gate behavior is back to the pre-regression contract.
- The failed hypothesis behind ADR-0026 is explicitly documented instead of silently lingering.

### Negative

- The unresolved `dev_buy_total_sol=0.0` problem remains open.
- The codebase returns to a stricter session gate that does not try to rescue unknown-pool race-window trades.

## Alternatives Considered

### 1. Keep buffering and only silence or downgrade the warning

Rejected because that would hide the operational regression without removing the ineffective behavior that introduced it.

### 2. Keep buffering and retune TTL/caps

Rejected because the fresh logs demonstrated lack of efficacy against the target metric problem while the added behavior already polluted the hot path.

### 3. Revert buffering and separately continue root-cause analysis for `dev_buy_total_sol`

Accepted. This rollback removes the regression first and restores the last known good launcher contract before any further fix attempt.

## Validation Steps

1. Confirm `ghost-launcher/src/components/seer.rs` no longer buffers unknown-pool trades on the launcher bridge path.
2. Confirm targeted session-bridge tests again assert silent-drop semantics.
3. Confirm no launcher hot-path warning remains for `trade buffer EXPIRED` after the rollback.
4. Treat `dev_buy_total_sol=0.0` as still unresolved and requiring a separate investigation/fix path.
