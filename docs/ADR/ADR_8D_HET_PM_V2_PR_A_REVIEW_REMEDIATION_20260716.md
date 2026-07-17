# ADR-8D: HET Position Manager V2 PR A — naprawa wiarygodności burn-in evidence po review PR #71

Status: `IMPLEMENTED / LOCAL VALIDATION COMPLETE / PR #71 REVIEW READY`

Typ: ADR-8D / review remediation / aktywny shadow post-buy / HET-PM V2 PR A / evidence integrity

Data: 2026-07-16

Repo: `smahacfel/Gho`

Branch: `agent/het-pm-v2-pr-a`

Base SHA: `18d94b0cc5a226496a5ac2bc616e7488a7f78d5d`

Plan: `PLANS/DO_REALIZACJI/POSITION_MANAGER_HET_V2.md`, wyłącznie PR A.

Powiązany dokument: `ADR_8D_HET_PM_V2_PR_A_OBSERVE_ONLY_20260716.md`.
Niniejszy ADR jest addendum po review i zastępuje wcześniejsze opisy wszędzie,
gdzie dotyczą one Crash finalization, same-snapshot comparison, vitality wiring,
startup validation, route bootstrap, strictness analizatora albo formalnego
base/head test evidence. Drugi focused re-review rozszerza ten dokument o
identity konfiguracji źródłowych, lokalną walidację receipt, semantykę
UnknownEvidence i diff-scoped Clippy. Trzeci focused re-review rozszerza ADR o
trwałą granicę terminalnego comparison, korelację retry i rozdzielenie faktu
zastosowania exitu od stanu canonical terminal commit.

Poziom ryzyka: `MEDIUM-HIGH` — poprawka dotyka aktywnego shadow post-buy
runtime i kontraktu danych burn-in. Nie zmienia prebuy Gatekeepera, nie dodaje
V2 authority, nie aktywuje live execution i nie zmienia ownership terminala.

## 1. Problem

Review PR #71 potwierdziło właściwą izolację lifecycle V2, ale wykazało, że
sidecar nie był jeszcze wiarygodnym źródłem evidence do późniejszej decyzji o
PR B. Zidentyfikowano cztery blockery, dwa problemy major i niezamknięty
formalny gate testowy:

1. Crash V2 nie stosował quote-side semantyki istniejącego CrashGuarda V1;
2. sidecar i authority V1 mogły użyć dwóch materializacji snapshotu;
3. aktywny profil HET nie uruchamiał źródła vitality TimeStop V2;
4. produkcyjnie dostępny constructor mógł połknąć błąd HET configu;
5. brak AccountStateCore był traktowany jako PumpCurveSupported;
6. analizator akceptował niepełne lub mieszane kontrakty danych;
7. wymagany launcher test miał failure bez formalnego dowodu base/head parity.

Decyzja: naprawić wszystkie siedem punktów w granicach PR A bez zmiany
V1 policy thresholds, terminal commit, capacity ownera, shadow/live boundary,
Gatekeepera albo niezwiązanego emitera density.

## 2. CrashGuard: jedna istniejąca semantyka quote-side

`ExitPolicyV2::finalize_with_quote()` dla `Crash` deleguje teraz do dokładnie
istniejącego:

```text
ExitPolicyV1::evaluate_crash_guard_quote(
    base_snapshot,
    full_position_quote,
    quote_evidence,
    immutable_crash_requirement,
    effective_v1_config,
)
```

V2 nie posiada uproszczonego, konkurencyjnego progu Crash. Finalizacja używa:

- pełnej `remaining_quantity_raw`;
- `ExecutableExitQuote::is_resolved()`;
- immutable `CrashGuardQuoteRequirementV1` z candidate boundary;
- same-or-newer slot i timestamp;
- istniejącego `crash_max_sample_age_ms`;
- istniejącego `crash_max_executable_return_pct`.

Typed wynik sidecara rozróżnia:

- `ExitAll { reason: Crash, ... }` oraz `CrashGuardQuoteDecision::Confirmed`;
- `CrashRejectedByQuote { reason }`;
- `CrashBlockedByData`.

Odrzucenie przez łagodny executable return nie jest mapowane do anonimowego
`Hold`. W konsekwencji disagreement i późniejsza analiza false-early-exit nie
tracą przyczyny odrzucenia.

## 3. Jeden bundle i receipt rzeczywistego V1 authority ticku

Aktywna ścieżka HET ma teraz konstrukcyjną kolejność:

```text
remember latest canonical shadow snapshot
  -> one read-lock materialization of PostBuySnapshotBundle
  -> V1 prequote + Crash prequote from bundle.base
  -> V2 prequote + local bounded quote plan from the same bundle
  -> resolve local quote cells
  -> prepare + validate immutable comparison core
  -> run V1 authority with references to the exact base/prequotes/cells
  -> apply exit and stage terminal, without canonical append
  -> derive V1AuthorityTickReceiptV1 from guarded runtime outcome
  -> observer-only anchor apply after V1
  -> finalize + bound + pre-serialize comparison
  -> attach comparison to PendingTerminalCommit
  -> persist comparison or typed Skipped
  -> append operational/canonical terminal truth
  -> cleanup mirrors and release capacity
```

`run_shadow_runtime_tick_v1()` przyjmuje gotowe:

- `&PostBuyDecisionSnapshot`;
- `&PreQuoteDecision`;
- `&CrashGuardPreQuoteDecision`;
- `&[HetPmV2QuoteCell]`;
- ten sam effective V1 policy.

Funkcja nie materializuje ponownie pozycji, nie liczy ponownie prequote i nie
wybiera nowego evidence boundary. Guarded proposal/apply nadal może legalnie
odrzucić snapshot, jeżeli po materializacji wystąpi konkurencyjna zmiana;
sidecar zapisze wtedy actual `ApplyRejected`, a nie przewidywany exit.

Receipt posiada:

```text
snapshot_id
state_revision
remaining_quantity_raw
outcome = Hold | ProposalStarted | ExitApplied |
          PendingRecovery | Blocked | ApplyRejected
exit_apply_status = NotApplied | Applied | Rejected
terminal_commit_status = NotRequired | Pending | Committed
action_id
reason
crash_quote_decision
```

`V1V2ComparisonRecord.v1_final` jest wyprowadzane z receipt. Serializer failuje
przed appendem, jeżeli receipt nie ma dokładnie tego samego snapshot ID,
revision albo quantity co rekord. `terminal_tick` jest wyprowadzany z
obu osi: jest prawdziwy, gdy exit został zastosowany albo terminal commit jest
wymagany. Outcome posiada typed `as_label()`; runtime serializer
odrzuca również każdy rekord, w którym `v1_final` nie odpowiada receipt albo
`terminal_tick` nie odpowiada osiom apply/commit.

`PreQuoteDecision::UnknownEvidence` nie jest finalizowane jako świadome
`Hold`. Receipt ma wtedy `Blocked` i reason `prequote_unknown:<typed reason>`.
Pozycja pozostaje otwarta, ale evidence nie zaciera braku rozstrzygalności V1.

Ścieżka bez aktywnego HET nadal materializuje V1 base snapshot samodzielnie.
V1 base snapshot nie zależy już od obecności valid HET extras/configu.

## 4. Vitality i osiągalność AbsoluteMaxHold

Aktywny profil burn-in jawnie ustawia:

```toml
[post_buy_guardian.time_stop_v2]
enabled = true
mode = "observe_only"
```

Effective HET config failuje, jeżeli jednocześnie:

```text
het_pm_v2.enabled == true
time_stop_v2.enabled == false
```

Dzięki temu `TimeStopV2State` produkuje rzeczywiste window status, failed
windows, window timestamps i freshness, zanim HET wykona immutable `project()`.
HET nadal nie wywołuje dodatkowego `evaluate()` i nie mutuje TimeStop state.

Test integracyjny obejmuje świeże `Alive` windows, bounded trajectory i jawne
route evidence, a następnie potwierdza, że `AbsoluteMaxHold` przy 120 s nie jest
maskowany przez domyślne `StaleOrUnknown`.

## 5. Fail-closed construction i niezależność V1

Produkcyjny constructor to wyłącznie:

```text
MonitoringEngine::try_new(...) -> Result<MonitoringEngine, MonitoringEngineConfigError>
```

`MonitoringEngine::new()` istnieje tylko pod `#[cfg(test)]`. Produkcyjne
call-site'y w launcherze i pipeline builderze używają `try_new()` i propagują
błąd z kontekstem. Invalid HET config, w tym `authoritative_shadow`, nie może
zdegradować się do `None` ani do cichego disabled.

Materializacja V1 snapshotu używa `EffectiveExitPolicyV1Config` i nie pobiera
HET configu. Dodatkowy observer nie może więc wyłączyć V1 snapshotu/authority.

## 6. Route truth jest fail-closed

Nowa pozycja zawsze zaczyna z:

```text
RouteStatusV1::Unknown
```

`PumpCurveSupported` może powstać wyłącznie z jawnego canonical
`AccountStateCore` evidence. `StatePhase::Migrated` mapuje się do typed
`CurveCompletePumpSwapUnsupported`; brak truth source nie jest dowodem venue.

Launcher odrzuca startup, gdy HET jest enabled, a `AccountStateCore` nie jest
podłączony. E2E pipeline również nie tworzy aktywnego post-buy shadow monitora
bez wymaganego AccountStateCore feedu.

## 7. Strict offline schema i granica promotion evidence

`scripts/het_pm_v2_analysis.py` wymaga pełnego kontraktu identity/provenance:

- schema;
- osobne identity HET: policy ID, version i config hash;
- osobne identity V1: policy ID, version i config hash;
- osobny pełny `time_stop_v2_config_hash` źródła vitality;
- run ID;
- lane, position/epoch/revision/quantity/snapshot ID;
- sampling mode, measurement grade i monitor tick;
- V1 prequote, Crash prequote, actual final i authority receipt;
- V2 prequote/final/winning gate/Crash decision;
- trajectory, vitality, route, entry value i anchor;
- quote keys/statuses/cardinality;
- authority i observe-only flags.

Hash TimeStop V2 obejmuje kompletną konfigurację źródła, w tym scheduling,
window/candidate thresholds, meaningful-progress thresholds, heartbeat
thresholds, mode, enabled oraz emission setting. Jest liczony raz przy
fail-closed konstrukcji engine'u. Nie jest włączany do HET hash, ponieważ
kontrakty HET, V1 i TimeStop pozostają rozdzielonymi identities.

Loader odrzuca:

- mixed schema/HET config/V1 config/TimeStop V2 config/sampling/tick;
- lane inne niż `shadow`;
- `consumed_by_policy = true`;
- brak V1 authority albo obecność V2/live authority;
- receipt niezgodny ze snapshot ID/revision/quantity/final outcome;
- nieznane etykiety enum decyzji, gate, route, vitality, trajectory,
  anchor request, Crash result i quote failure;
- `NaN`, `Infinity` i inne non-finite values;
- quote cardinality większe niż dwa albo niespójne listy.

Raport rozdziela dwie klasy:

```text
producer_asserted_integrity
  -> self-reported flags + internal sidecar consistency
  -> promotion_evidence = false

independently_measured_integrity
  -> not_evaluated_requires_lifecycle_reconciliation_artifact
  -> promotion_gate_1_satisfied = false
```

Literalne `false` zapisane przez producenta nie są uznawane za niezależny
dowód isolation. Promotion Gate 1 wymaga odrębnego reconciliation artifactu
lub równoważnego niezależnego dowodu poza sidecarem PR A.

## 8. Formalny gate: dokładny base/head parity dla launcher testu

Wymagane polecenie:

```text
cargo test -p ghost-launcher components::post_buy_runtime --lib
```

uruchomiono na dokładnym base SHA i na head w identycznym środowisku:

- base SHA: `18d94b0cc5a226496a5ac2bc616e7488a7f78d5d`;
- ten sam checkout dependency lock, SHA-256
  `3362b5fcd6cb305407906a9dbcd6a9094398ae1c7e6830d34ee4fb5c30c7a7c3`;
- `rustc 1.95.0 (59807616e 2026-04-14)`;
- `cargo 1.95.0 (f2d3ce0bd 2026-03-21)`;
- ten sam target directory i świeże kompilacje właściwych crate'ów.

Wynik base:

```text
66 tests: 65 passed, 1 failed
failed: components::post_buy_runtime::tests::
        shadow_v2_validation_smoke_marker_writes_required_artifacts_without_handoff
cause: density jsonl: Os { code: 2, kind: NotFound, ... }
```

Wynik head:

```text
68 tests: 67 passed, 1 failed
failed: ten sam dokładny test
cause: ten sam brak density JSONL / No such file or directory
```

Dwa dodatkowe testy head dotyczą fail-closed HET startupu i oba przechodzą.
Formalny wariant akceptacyjny „exact test na base i head z identycznym failure”
jest zatem spełniony. PR A nie rozszerza zakresu o naprawę niezwiązanego
emitera density.

## 9. Drugi focused re-review i jawna granica burn-inu

Drugi review wykazał cztery pozostałe problemy:

1. diff-scoped Clippy zgłaszał `needless_borrow`, `clone_on_copy`,
   `too_many_arguments` i `needless_update` w plikach PR;
2. sidecar posiadał wyłącznie HET config hash;
3. runtime serializer nie porównywał final/terminal z receipt;
4. V1 `UnknownEvidence` kończyło jako `Hold`.

Wszystkie cztery klasy zostały naprawione. Exact lokalny gate:

```text
python3 scripts/guard_diff_scoped_clippy.py \
  --base 18d94b0cc5a226496a5ac2bc616e7488a7f78d5d \
  --head HEAD
```

zwraca:

```text
PASS: no new Clippy diagnostics and no diagnostic with a primary span in changed Rust source.
```

PR A nadal jest wyłącznie producentem observe-only evidence. W ramach tej
implementacji nie uruchomiono shadow burn-inu i nie policzono promotion Gate 4
ani Gate 5 z sekcji 19 planu. W szczególności nie wyliczano:

- CVaR ani tail-loss delta;
- MFE capture ratio;
- outlier concentration/share;
- creator/funder cohort dominance;
- stabilności kierunku między runami i launch cohorts.

W repo nie ma jeszcze committed promotion criteria, promotion gate tool ani
`het_pm_v2_promotion_gate_v1.json`. Analyzer zachowuje zatem jawnie:

```text
promotion_gate_evaluated = false
promotion_gate_passed = false
counterfactual_outcome_attribution_status =
  not_evaluated; requires explicit lifecycle_and_replay_join
```

Gotowość PR A oznacza gotowość do późniejszego zbierania porównywalnego
evidence, nie ekonomiczną lub kohortową promocję PR B.

## 10. Trzeci focused re-review: trwała terminalna granica §14/§15

Trzeci review wykazał, że comparison używał poprawnego pre-mutation snapshotu,
ale był finalizowany i zapisywany dopiero po powrocie z pełnego V1 authority
ticku. W udanym terminalu canonical append, cleanup i capacity release mogły
więc poprzedzić sidecar. W wariancie canonical retry pierwszy tick zapisywał
`PendingRecovery`, a tick retry terminalizował pozycję bez rekordu HET.

Poprawiona granica ma dwa jawne typy przygotowanego payloadu:

```text
PreparedV1V2ComparisonCoreV1
  = Ready(immutable pre-authority record)
  | Skipped { correlation, typed reason }

PreparedHetComparisonV1
  = Ready { correlation, action_id, bounded encoded bytes }
  | Skipped { correlation, action_id, typed reason }
```

Core jest walidowany przed wejściem do V1. Dopiero actual receipt uzupełnia
post-authority pola, po czym pełny rekord jest ponownie walidowany, ograniczany
rozmiarem i serializowany. Terminalny payload jest przenoszony przez istniejący
`PendingTerminalCommit`; nie tworzy nowego terminal ownera i nie uczestniczy w
`canonical_committed()`.

`PendingTerminalCommit` zachowuje:

```text
prepared_het_comparison
het_comparison_write_status = NotAttempted | Written | Skipped
comparison_id
source_snapshot_id
original V1 action_id
```

Przed każdą pierwszą próbą canonical append runtime wykonuje synchroniczny,
best-effort append gotowych bytes. Wynik `Written` albo typed `Skipped` jest
nanoszony na operational terminal record i canonical `TERMINAL_TRUTH`.
Obsługiwane powody degradacji obejmują błędy core/final validation,
serialization, payload bound, brak writer configu oraz writer I/O failure.
Żaden z nich nie blokuje canonical terminal commit ani capacity release.
Canonical terminal zachowuje korelację w namespaced `envelope.source_refs`,
bez rozszerzania istniejącego `ShadowTerminalTruthV2` i jego historycznego
kontraktu schema.

Retry używa wyłącznie payloadu z pierwotnego decision ticku. Nie materializuje
nowego snapshotu, nie uruchamia V2 i nie zapisuje drugi raz porównania, jeżeli
status jest już `Written` albo `Skipped`. Korelacja canonical terminal truth
pozwala połączyć wynik z dokładnym `comparison_id`, `source_snapshot_id` oraz
V1 `action_id`.

Semantyka V1 została rozdzielona na niezależne osie. Pełny fill z awarią
canonical writera jest teraz:

```text
outcome = ExitApplied
exit_apply_status = Applied
terminal_commit_status = Pending
```

Nie jest już anonimowym `PendingRecovery`. `PendingRecovery` pozostaje dla
przypadków, w których ekonomiczny exit nie został zastosowany, np. oczekiwania
na executable quote albo terminalizacji unresolved bez fillu.

## 11. Testy regresyjne dodane lub rozszerzone

Crash:

- `crash_mark_candidate_with_mild_executable_loss_is_rejected`;
- `crash_quote_older_than_candidate_is_blocked`;
- `crash_quantity_mismatch_is_rejected`;
- `crash_confirmed_requires_v1_threshold`.

Bundle/receipt:

- `actual_v1_receipt_and_v2_record_share_exact_snapshot_id_revision_quantity`.
- `comparison_rejects_v1_final_receipt_mismatch`;
- `comparison_rejects_terminal_tick_receipt_mismatch`;
- `v1_unknown_prequote_is_receipted_as_blocked_not_hold`.

Vitality/startup/route:

- `active_profile_produces_vitality_windows_and_max_hold_remains_reachable`;
- `invalid_het_config_fails_every_non_test_constructor`;
- `authoritative_shadow_never_degrades_to_disabled`;
- `invalid_het_config_cannot_disable_v1_snapshot_or_authority`;
- `route_defaults_to_unknown_without_canonical_account_state_evidence`;
- `het_enabled_without_account_state_core_fails_startup`.

Analyzer:

- deterministic report;
- unsupported schema;
- mixed HET, V1 i TimeStop V2 config hash;
- wrong lane/authority;
- non-finite JSON;
- missing provenance;
- unknown decision enum;
- V1 final/receipt mismatch;
- poprawny `ExitApplied + Applied + Pending`;
- odrzucenie sprzecznych osi apply/commit;
- odrzucenie zduplikowanego `comparison_id`;
- quote ownership attribution.

Terminal same-tick/retry:

- `het_exit_tick_persists_original_pre_mutation_comparison`;
- `het_terminal_retry_uses_original_comparison_without_v2_reevaluation`;
- `het_terminal_retry_success_emits_exactly_one_terminal_outcome_for_original_action`;
- `exit_applied_with_terminal_commit_pending_is_not_labeled_generic_pending_recovery`;
- `process_boundary_after_canonical_commit_cannot_silently_erase_exit_comparison`;
- `terminal_sidecar_failure_records_typed_skipped_without_blocking_capacity_release`.

## 12. Inwarianty zachowane

- V1 pozostaje jedynym proposal/apply/terminal/capacity ownerem;
- V2 nie wywołuje `begin_exit_proposal` ani żadnego apply;
- V2 nie konsumuje własnego wyniku do policy/execution;
- `MaterializedFeatureSet` i prebuy Gatekeeper pozostają nietknięte;
- HET comparison używa immutable post-buy bundle, nie live-state policy read;
- Crash threshold i freshness pozostają SSOT w ExitPolicyV1 configu;
- TimeStop mutation pozostaje w istniejącej ścieżce, HET używa projection;
- quote plan pozostaje lokalny dla ticku, deduplikowany i bounded do dwóch;
- sidecar jest `Written` albo terminal truth zawiera typed `Skipped` przed
  canonical commit; każda taka degradacja pozostaje fail-open względem V1;
- sidecar nie jest canonical terminal truth ani niezależnym lifecycle proof;
- retry terminala zachowuje oryginalny snapshot/action i nie ewaluuje V2;
- exit apply i terminal persistence są raportowane na osobnych osiach;
- executable anchor nadal ma własne `anchor_seq` i nie zwiększa economic revision;
- unknown/unsupported route blokuje V2 zamiast udawać Hold/exit;
- V1 UnknownEvidence jest receipted jako Blocked, nie Hold;
- shadow simulation nie jest live inclusion;
- live authority pozostaje disabled.

## 13. Zakres plików remediation

- `ghost-brain/ghost_brain_config.toml` — aktywne observe-only TimeStop vitality;
- `ghost-brain/src/guardian/post_buy/engine.rs` — one-bundle orchestration,
  receipt, source identities, UnknownEvidence, V1-independent snapshot i route
  bootstrap;
- `ghost-brain/src/guardian/post_buy/config.rs` — deterministyczny pełny hash
  konfiguracji źródła TimeStop V2;
- `ghost-brain/src/guardian/post_buy/exit_policy_v1.rs` — serializowalny typed
  Crash quote result dla evidence;
- `ghost-brain/src/guardian/post_buy/exit_policy_v2.rs` — V1 Crash finalization,
  typed Crash outcomes, prepared comparison core i dwuosiowy receipt;
- `ghost-brain/src/pipeline/builder.rs` — produkcyjny fail-closed constructor;
- `ghost-launcher/src/components/post_buy_runtime.rs` — route source startup guard
  i produkcyjne `try_new()` call-site'y;
- `scripts/het_pm_v2_analysis.py` — strict schema i evidence-class split;
- `scripts/test_het_pm_v2_analysis.py` — negatywne testy schema;
- niniejszy ADR-8D.

## 14. Walidacja lokalna remediation

| Kontrola | Wynik |
| --- | --- |
| `cargo check -p ghost-brain --lib` | PASS. |
| `cargo test -p ghost-brain guardian::post_buy --lib` | PASS — 231/231. |
| sześć exact terminal same-tick/retry fault-injection tests | PASS — 6/6. |
| `cargo test -p ghost-brain guardian::post_buy::exit_policy_v2::tests::crash_ --lib` | PASS — 4/4. |
| `cargo test -p ghost-brain guardian::post_buy::engine::tests --lib` | PASS — 64/64. |
| `cargo test -q -p ghost-launcher shadow_handoff --lib` | PASS — 3/3. |
| exact launcher base/head comparison | PARITY PROVEN — base 65/66 i head 67/68; ten sam jedyny niezwiązany density failure. |
| `python3 -m unittest scripts/test_het_pm_v2_analysis.py` | PASS — 14/14. |
| `python3 -m py_compile scripts/het_pm_v2_analysis.py scripts/test_het_pm_v2_analysis.py` | PASS. |
| exact diff-scoped Clippy względem base SHA | PASS — brak nowych diagnostics i brak primary spans w zmienionym Rust. |
| `cargo fmt --all -- --check` | PASS; wymagane ponownie bezpośrednio przed commitem. |
| `git diff --check` | PASS; wymagane ponownie bezpośrednio przed commitem. |

## 15. Rollback

Rollback to revert jednego remediation commita do poprzedniego head PR #71.
Nie ma migracji canonical state ani publicznego API. Taki rollback przywraca
jednak wszystkie opisane luki evidence i nie powinien być użyty do burn-in ani
promocji PR B.

Nie należy wykonywać częściowego rollbacku samego receipt, Crash delegation,
TimeStop wiring albo route guardu. Te elementy wspólnie tworzą kontrakt, że
rekord V1/V2 jest porównaniem tego samego boundary i posiada właściwą jakość
źródłową.
