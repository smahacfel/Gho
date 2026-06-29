# ADR-8D: Shadow Burnin Fidelity Audit 2026-06-29

## Status

Accepted as audit decision.

## Decyzja

Finalny verdict enum: **SHADOW_REPLAY_LIFECYCLE_MISMATCH**.

Shadow burnin nie moze byc uzywany jako spojny lifecycle/replay dataset ani jako live-equivalent/runtime approval proof. `shadow_exit_replay_v1` moze byc uzywany tylko komponentowo jako offline path-label evidence z ograniczeniami. Stare raporty strategii, ktore zakladaly live-equivalence albo lifecycle/replay equivalence, wymagaja downgrade label.

## Kontekst

Audyt dotyczy systemu pomiarowego: `shadow_exit_replay_v1`, `shadow_lifecycle`, `probe_shadow_lifecycle`, `gatekeeper_v2_decisions`, `selector_shadow_score_v1`, state/provenance i path density. Audyt nie zmienial runtime semantics, BUY/REJECT, Gatekeeper policy, selector runtime, TX/Jito/live path, `shadow_close_only` ani active close.

Uwaga o szablonie: plik `docs/ADR/ADR_8D_SZABLON.md` nie byl obecny w tej kopii worktree podczas generowania ADR, wiec zastosowano istniejacy styl ADR-8D z repo i wymagane pola z zadania.

## Evidence

- source inventory: `reports/selector/shadow_fidelity_inventory.csv`
- entry price reconstruction: `reports/selector/shadow_fidelity_entry_price_reconstruction.csv`
- exit price reconstruction: `reports/selector/shadow_fidelity_exit_price_reconstruction.csv`
- pool state provenance: `reports/selector/shadow_fidelity_pool_state_provenance.csv`
- temporal integrity: `reports/selector/shadow_fidelity_temporal_integrity.csv`
- replay/lifecycle reconciliation: `reports/selector/shadow_fidelity_replay_lifecycle_reconciliation.csv`
- live-equivalence gap: `reports/selector/shadow_fidelity_live_equivalence_gap.csv`
- path density: `reports/selector/shadow_fidelity_path_sampling_density.csv`
- deterministic fixtures: `reports/selector/shadow_fidelity_fixture_results.csv`
- claim evidence matrix: `reports/selector/shadow_fidelity_claim_evidence_matrix.csv`

Claim status summary: `{"DISPROVEN": 8, "NOT_PROVEN": 1, "PARTIALLY_PROVEN": 10, "PROVEN": 3}`.

## Limitations

- Entry price is not proven as a live landed fill.
- Exact reserve/state reconstruction is partial and blocked where raw state evidence is missing.
- Exit path is sampled/compressed and not a full executable sell stream.
- Path density must be evaluated per horizon and per row.
- Lifecycle/replay exact join issues and duplicate terminal rows must not be collapsed silently.
- Live execution gaps remain unmodeled: latency, landing/failure, slippage, own impact, AMM fees, blockhash, priority fee/Jito, contention.

## Runtime boundary

No runtime path was changed. Shadow evidence remains shadow evidence. Submit, simulation and lifecycle shadow close are not live confirmation.

## Research boundary

Allowed: offline relative research over shadow/path labels when the horizon is covered and OUTCOME fields are not used as selection features.

Forbidden: live-equivalent PnL, runtime approval, RCE approval, claims that rely on unmodeled landing/slippage/failure/own-impact.

## Required instrumentation

- entry quote/min_out/reserve-before/reserve-after/decimals;
- decision-to-submit and submit-to-land timestamps;
- actual landing slot or failed/no-fill status;
- exit quote/min_out/slippage/fees/own sell impact;
- path sample slot/timestamp/commitment;
- exact tie-break metadata for same-slot target/stop;
- lifecycle/replay exact join id and terminal-event cardinality.

## Consequences for ORG/TSV2/EIX/RTP/RUG/RCE

- ORG/TSV2/RTP/RUG research remains valid only where it used pre/at-decision features and post-entry fields strictly as labels.
- EIX/RCE claims must not treat shadow as execution-quality proof.
- Any report claiming live-equivalent outcome must be downgraded.
- Any report treating lifecycle and replay as one consistent position story must be downgraded.
- Any report inferring 300s/500s without replay coverage must be downgraded.
- Any selector conclusion using outcome/path fields as features must be treated as temporal leakage risk until disproven.

## Old report downgrade labels

Required for old reports that used unsupported assumptions:

- `DOWNGRADE_SHADOW_NOT_LIVE_EQUIVALENT`
- `DOWNGRADE_REPLAY_LIFECYCLE_MISMATCH`
- `DOWNGRADE_ENTRY_FILL_NOT_PROVEN`
- `DOWNGRADE_EXIT_FILL_NOT_PROVEN`
- `DOWNGRADE_HORIZON_COVERAGE_NOT_PROVEN`
- `DOWNGRADE_TEMPORAL_LABEL_FEATURE_SEPARATION_UNPROVEN`

## No runtime changes confirmation

This ADR records an offline measurement-system audit only. It does not approve runtime behavior and does not change runtime semantics.
