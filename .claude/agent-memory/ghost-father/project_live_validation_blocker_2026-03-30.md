---
name: live validation blocker 2026-03-30
description: Compiled ghost-launcher ran, but live BUY-path validation was blocked by reject-only flow and trigger floor budget.
type: project
---
Compiled `ghost-launcher` was validated on 2026-03-30 against `/root/Gho/config.toml` in `paper + shadow_only` mode.

fact: The binary started and appended fresh decision rows, proving runtime ingestion was live.
**Why:** This separates runtime liveness from BUY/shadow branch reachability.
**How to apply:** Do not diagnose this run as a launcher-dead or logger-dead failure.

fact: Preflight reported trigger balance `0.007327349 SOL`, below required reserve+trade budget `0.011000000 SOL` (`0.008 floor + 0.002 buffer + 0.001 size`).
**Why:** Even if a BUY-capable candidate appears, the trigger guard is not currently satisfiable.
**How to apply:** For conclusive BUY-path validation, either fund the wallet or use an explicitly authorized temporary config.

fact: During live observation, `gatekeeper_v2_decisions.jsonl` advanced while `gatekeeper_v2_buys.jsonl` stayed at 2 rows and `logs/shadow_run/buys.jsonl` stayed at 10351.
**Why:** The observed window produced fresh rejects/timeouts only and did not reach a fresh BUY/shadow execution branch.
**How to apply:** Treat the result as an operational validation blocker, not proof that the BUY-path fixes regressed.
