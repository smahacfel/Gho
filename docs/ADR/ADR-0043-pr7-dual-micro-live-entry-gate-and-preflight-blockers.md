# ADR-0043: PR-7 dual-micro-live entry gate and preflight blockers

**Date:** 2026-03-28
**Status:** Accepted
**Author:** Ghost Father

## Context

The operator requested a concrete procedure for entering PR-7 / first live run after the paper burn-in closure work.

Fresh verification on 2026-03-28 established two separate facts:

1. **PR-6 promotion gate is now formally green.**
	The latest formal paper burn-in report returned `GO` for session `launcher-1774651565666` with:
	- `paper_lifecycle_complete = passed`
	- `trace_correlation = passed`
	- `economics_not_fatal = passed`
	- `no_live_side_effects = passed`
	- `recovery_contract = passed`

2. **Dual micro-live is not yet operacyjnie startable from the current workstation state.**
	Running:

	`./scripts/ghost_production_preflight.sh --config /root/Gho/configs/rollout/dual-micro-live.toml`

	failed on two checks only:
	- `trigger.keypair`: config expects `/root/Gho/wallets/dual-micro-live-wallet.json`, but the file does not exist
	- `trigger.jito_endpoint`: `getTipAccounts` probe failed for the configured Jito endpoint

Additional verification of config loading showed that secret env overrides are applied **only when the tracked config value is a placeholder** (`example.invalid`, `replace-me`, etc.). Therefore:

- `GHOST_TRIGGER_JITO_ENDPOINT` from `.env` correctly overrides the placeholder Jito URL in `dual-micro-live.toml`
- `GHOST_TRIGGER_KEYPAIR_PATH` from `.env` does **not** override `dual-micro-live.toml`, because the config already contains a non-placeholder path (`../../wallets/dual-micro-live-wallet.json`)

Additional manual probing showed that:

- `https://frankfurt.mainnet.block-engine.jito.wtf` and `/api/v1` do not return a usable `getTipAccounts` JSON-RPC result in this environment
- `https://frankfurt.mainnet.block-engine.jito.wtf/api/v1/bundles` responds, but currently returns a rate-limit error rather than a valid `result` array

## Decision

Treat PR-7 entry as a **two-stage gate**:

### Stage A — promotion permission

Promotion from paper burn-in to dual micro-live is allowed only after a formal paper report returns `GO`.

This gate is now satisfied.

### Stage B — operational readiness

Starting the first dual micro-live run is allowed only when all of the following are true:

1. `dual-micro-live.toml` preflight passes with no `[fail]` checks
2. a **dedicated dual rollout wallet** exists at the path expected by the profile (`/root/Gho/wallets/dual-micro-live-wallet.json`) or the profile is intentionally changed to another approved dedicated path
3. the dedicated dual wallet is funded above:

	`emergency_floor_sol + position_size_buffer_sol + max_position_size_sol`

	which for the current profile means strictly above:

	`0.05 + 0.02 + 0.00001 = 0.07001 SOL`

4. the Jito endpoint used by the dual profile returns a valid `getTipAccounts` result during preflight
5. the operator preserves the phase boundary:
	- PR-7 first live run uses `configs/rollout/dual-micro-live.toml`
	- `configs/rollout/future-live.toml` remains forbidden until PR-7 is closed

## Architectural Impact

- Promotion governance remains strict: a green paper report is necessary but not sufficient for the first live-side-effect run.
- Secret loading behavior is now operationally important: `.env` is not a universal override layer; some config paths remain authoritative unless they are placeholders.
- The first live run remains anchored to `dual + live_and_shadow`, preserving simultaneous live and shadow evidence for each BUY.

## Risk Assessment

**Rate:** High

Main risks if the operator skips these blockers:

- using the wrong wallet (or a non-dedicated wallet) breaks rollout blast-radius discipline
- forcing startup without a working Jito probe breaks the declared dual execution contract
- misreading `.env` as an unconditional override could produce false confidence about which signing key is actually used

## Consequences

What becomes easier:

- the operator has a binary checklist before the first real BUY
- the path from paper `GO` to dual startup is explicit and reproducible

What becomes harder:

- the first live run cannot start immediately just because paper is green
- wallet segregation and Jito validation are enforced as real gates, not optional hygiene

## Alternatives Considered

### 1. Start dual micro-live immediately because paper burn-in is `GO`

Rejected because operational preflight still fails and would violate the rollout contract.

### 2. Reuse `GHOST_TRIGGER_KEYPAIR_PATH=/root/.config/solana/id.json` from `.env` without changing anything else

Rejected because the current config loader does not override non-placeholder `trigger.keypair_path`, so this would not actually change the active keypair path for the dual profile.

### 3. Disable Jito for the first dual run

Rejected because the current approved dual rollout profile explicitly requires `use_jito = true`; changing that would be a separate architectural decision, not an operator shortcut.

## Validation Steps

1. Confirm baseline stamp equals current HEAD
2. Run formal paper burn-in report and verify `verdict = GO`
3. Run dual preflight and confirm current failures are limited to:
	- missing dual wallet file
	- failing Jito `getTipAccounts` probe
4. Verify config loader behavior in `ghost-launcher/src/config.rs`:
	- secret env overrides apply only to placeholder values
5. Manually probe candidate Jito URLs and confirm current endpoint still lacks a usable `getTipAccounts` result in this environment
