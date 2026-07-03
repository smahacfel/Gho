# RAPORT SHADOW V2 PR43-C EVENTORDERKEY TEMPORAL DELTA SMOKE 20260703

## 1. Werdykt

Finalny verdict:

```text
PR43C_EVENTORDERKEY_TEMPORAL_DELTA_CLASSIFIED_STILL_BLOCKED
```

Interpretacja: PR43-B poprawil klasyfikacje i audytowalnosc `EventOrderKey` w realnym runtime smoke. Braki chain-order nie sa ukrywane: pozostaja jawne `UNKNOWN` tam, gdzie runtime nie ma danych, a terminal truth uzywa jawnej klasyfikacji `DERIVED`. To nie jest L2 PASS. `RESEARCH_CANDIDATE` nadal nie jest przyznany, poniewaz `unknown_but_required_for_research_count=2134` i temporal audit zwraca `BLOCKED_TEMPORAL_AMBIGUITY_REMAINS`.

## 2. Zakres runu

- run_id: `shadow-smoke-v2-eventorderkey-pr43b-r1`
- scope: `reports/selector/shadow-v2-eventorderkey-pr43b-r1`
- main_head: `3a97e086b9a961b0a93c3f6c6e04f8272ec439f3`
- PR44 merge commit: `3a97e086b9a961b0a93c3f6c6e04f8272ec439f3`
- configured_run_seconds: `900`
- duration_seconds: `931`
- shutdown_method: `SIGINT`
- forced_sigterm: `false`
- controller_exit_status: `WRAPPER_JSON_WRITE_FAILED_AFTER_LAUNCHER_EXIT_RECONSTRUCTED_AS_RUNTIME_PASS`

Uwaga operacyjna: pierwsze odpalenie launchera uzylo omylkowo lokalnego `ghost_brain_*` configu i zakonczylo sie natychmiastowym parse errorem `missing field seer`, bez runtime smoke. Wlasciwy smoke zostal uruchomiony z `configs/rollout/shadow-v2-eventorderkey-pr43b-r1.local.toml`; ten run wygenerowal canonical stream, manifesty i zakonczyl sie clean shutdown. Blad wrappera po `wait` dotyczy tylko zapisu `controller_summary.json` i zostal odtworzony z logu runtime.

## 3. Manifest i shutdown

- runtime_post_run_manifest_status: `PASS`
- post_run_strict_audit_status: `PASS`
- clean_shutdown_proven: `true`
- manifest_retention_audit_verdict: `PASS_MANIFEST_RETENTION_AUDIT`
- replay_lifecycle_reconciliation_audit_verdict: `PASS_REPLAY_LIFECYCLE_RECONCILED`

Log runtime zawiera `All components shut down successfully` oraz `Ghost Launcher shutdown complete`. Nie bylo wymuszonego SIGTERM.

## 4. Shadow flow

- accepted_shadow_handoff_count: `46`
- validation_smoke_marker_count: `1`
- entry_fill_FILLED_count: `45`
- entry_fill_BLOCKED_BY_DATA_count: `0`
- exit_fill_FILLED_count: `45`
- exit_fill_BLOCKED_BY_DATA_count: `2`
- terminal_truth_with_final_pnl_executable_bps_count: `44`
- complete_executable_roundtrip_positions: `44`

Ten smoke potwierdza, ze flow L1 nadal produkuje diagnostic executable evidence. Nie zmienia to statusu L2.

## 5. EventOrderKey delta

- event_order_key_present_rows: `417`
- event_order_key_missing_required_rows: `0`
- event_order_key_missing_rows: `0`
- event_order_key_exempt_rows: `47`
- non_monotonic_event_seq_in_process: `0`
- explicit_unknown_chain_order_components: `2134`
- unknown_but_required_for_research_count: `2134`
- derived_chain_order_components: `276`
- not_applicable_or_derived_chain_components_count: `276`
- same_slot_ambiguity_count: `276`
- terminal_truth_derived_component_count: `276`
- terminal_truth_derived_rows: `46`
- pool_state_unknown_component_count: `558`
- entry_pool_state_signature_unknown_count: `46`
- entry_pool_state_handoff_signature_reused_count: `0`

Wymagane oczekiwania PR43-C sa spelnione:

- `entry_pool_state_handoff_signature_reused_count = 0`;
- terminal truth ma widoczna klasyfikacje `DERIVED` (`terminal_truth_derived_component_count=276`);
- brak wymaganych event-order keys: `event_order_key_missing_required_rows=0`;
- brak non-monotonic process order: `non_monotonic_event_seq_in_process=0`.

Jednoczesnie L2 pozostaje zablokowane, bo chain-order nadal jest niepelny i jawnie oznaczony jako `UNKNOWN`.

## 6. Audyty post-run

- temporal_audit_verdict: `BLOCKED_TEMPORAL_AMBIGUITY_REMAINS`
- entry_reconstruction_audit_verdict: `FAIL_ENTRY_SCHEMA_OR_JOIN_BROKEN`
- exit_reconstruction_audit_verdict: `BLOCKED_EXIT_FILLS_BLOCKED_BY_DATA`
- replay_lifecycle_reconciliation_audit_verdict: `PASS_REPLAY_LIFECYCLE_RECONCILED`
- manifest_retention_audit_verdict: `PASS_MANIFEST_RETENTION_AUDIT`

`FAIL_ENTRY_SCHEMA_OR_JOIN_BROKEN` i `BLOCKED_EXIT_FILLS_BLOCKED_BY_DATA` nie sa interpretowane jako regresja PR43-B. Ten PR43-C mierzyl temporal/EventOrderKey delta, nie domykal entry/exit reconstruction readiness do L2.

## 7. Approval flags

- runtime_approval=false
- shadow_close_only_approval=false
- active_close_approval=false
- research_grade=false
- live_equivalence=false
- strategy_research_unblocked=false

Nie przyznano L2, research-grade, live-equivalence, runtime approval, shadow_close_only approval ani active close approval.

## 8. Artefakty

Commitowane artefakty report-only:

- `PLANS/AUDYT/RAPORT_SHADOW_V2_PR43C_EVENTORDERKEY_TEMPORAL_DELTA_SMOKE_20260703.md`
- `docs/ADR/ADR_8D_SHADOW_V2_PR43C_EVENTORDERKEY_TEMPORAL_DELTA_SMOKE_20260703.md`
- `reports/selector/shadow_v2_pr43c_eventorderkey_temporal_delta_smoke_summary.csv`
- `reports/selector/shadow_v2_pr43c_eventorderkey_temporal_delta_smoke_summary.json`

Nie commitowano raw JSONL, runtime scope, logow, lokalnych TOML, binariow ani prywatnych configow.

## 9. Nastepny wymagany krok

PR43-D powinien adresowac realne zrodla brakujacych chain-order komponentow albo formalnie rozstrzygnac, ktore komponenty sa niedostepne w runtime bez rozszerzenia ingest/handoff. W szczegolnosci L2 nadal blokuje brak chain `block_time`, `transaction_index`, `instruction_index`, `inner_instruction_index`, `log_index` oraz czesci `signature` dla pool-state/exit-side evidence.
