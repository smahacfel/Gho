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
base/head test evidence.

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
  -> run V1 authority with references to the exact base/prequotes/cells
  -> derive V1AuthorityTickReceiptV1 from guarded runtime outcome
  -> observer-only anchor apply after V1
  -> sidecar append
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
outcome = Hold | ProposalStarted | TerminalApplied |
          PendingRecovery | Blocked | ApplyRejected
action_id
reason
crash_quote_decision
```

`V1V2ComparisonRecord.v1_final` jest wyprowadzane z receipt. Serializer failuje
przed appendem, jeżeli receipt nie ma dokładnie tego samego snapshot ID,
revision albo quantity co rekord. `terminal_tick` jest wyprowadzany z
`TerminalApplied`.

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

- schema, policy ID i policy version;
- policy config hash i run ID;
- lane, position/epoch/revision/quantity/snapshot ID;
- sampling mode, measurement grade i monitor tick;
- V1 prequote, Crash prequote, actual final i authority receipt;
- V2 prequote/final/winning gate/Crash decision;
- trajectory, vitality, route, entry value i anchor;
- quote keys/statuses/cardinality;
- authority i observe-only flags.

Loader odrzuca:

- mixed schema/policy version/config hash/sampling/tick;
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

## 9. Testy regresyjne dodane lub rozszerzone

Crash:

- `crash_mark_candidate_with_mild_executable_loss_is_rejected`;
- `crash_quote_older_than_candidate_is_blocked`;
- `crash_quantity_mismatch_is_rejected`;
- `crash_confirmed_requires_v1_threshold`.

Bundle/receipt:

- `actual_v1_receipt_and_v2_record_share_exact_snapshot_id_revision_quantity`.

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
- mixed policy config hash;
- wrong lane/authority;
- non-finite JSON;
- missing provenance;
- unknown decision enum;
- V1 final/receipt mismatch;
- quote ownership attribution.

## 10. Inwarianty zachowane

- V1 pozostaje jedynym proposal/apply/terminal/capacity ownerem;
- V2 nie wywołuje `begin_exit_proposal` ani żadnego apply;
- V2 nie konsumuje własnego wyniku do policy/execution;
- `MaterializedFeatureSet` i prebuy Gatekeeper pozostają nietknięte;
- HET comparison używa immutable post-buy bundle, nie live-state policy read;
- Crash threshold i freshness pozostają SSOT w ExitPolicyV1 configu;
- TimeStop mutation pozostaje w istniejącej ścieżce, HET używa projection;
- quote plan pozostaje lokalny dla ticku, deduplikowany i bounded do dwóch;
- sidecar failure pozostaje fail-open względem V1 terminal commit;
- sidecar nie jest canonical terminal truth ani niezależnym lifecycle proof;
- executable anchor nadal ma własne `anchor_seq` i nie zwiększa economic revision;
- unknown/unsupported route blokuje V2 zamiast udawać Hold/exit;
- shadow simulation nie jest live inclusion;
- live authority pozostaje disabled.

## 11. Zakres plików remediation

- `ghost-brain/ghost_brain_config.toml` — aktywne observe-only TimeStop vitality;
- `ghost-brain/src/guardian/post_buy/engine.rs` — one-bundle orchestration,
  receipt, V1-independent snapshot i route bootstrap;
- `ghost-brain/src/guardian/post_buy/exit_policy_v1.rs` — serializowalny typed
  Crash quote result dla evidence;
- `ghost-brain/src/guardian/post_buy/exit_policy_v2.rs` — V1 Crash finalization,
  typed Crash outcomes i strict receipt record;
- `ghost-brain/src/pipeline/builder.rs` — produkcyjny fail-closed constructor;
- `ghost-launcher/src/components/post_buy_runtime.rs` — route source startup guard
  i produkcyjne `try_new()` call-site'y;
- `scripts/het_pm_v2_analysis.py` — strict schema i evidence-class split;
- `scripts/test_het_pm_v2_analysis.py` — negatywne testy schema;
- niniejszy ADR-8D.

## 12. Walidacja lokalna remediation

| Kontrola | Wynik |
| --- | --- |
| `cargo check -p ghost-brain --lib` | PASS. |
| `cargo test -p ghost-brain guardian::post_buy --lib` | PASS — 219/219. |
| `cargo test -p ghost-brain guardian::post_buy::exit_policy_v2::tests::crash_ --lib` | PASS — 4/4. |
| `cargo test -p ghost-brain guardian::post_buy::engine::tests --lib` | PASS — 64/64. |
| `cargo test -q -p ghost-launcher shadow_handoff --lib` | PASS — 3/3. |
| exact launcher base/head comparison | PARITY PROVEN — base 65/66 i head 67/68; ten sam jedyny niezwiązany density failure. |
| `python3 -m unittest scripts/test_het_pm_v2_analysis.py` | PASS — 9/9. |
| `python3 -m py_compile scripts/het_pm_v2_analysis.py scripts/test_het_pm_v2_analysis.py` | PASS. |
| `cargo fmt --all -- --check` | wymagane ponownie bezpośrednio przed commitem. |
| `git diff --check` | wymagane ponownie bezpośrednio przed commitem. |

## 13. Rollback

Rollback to revert jednego remediation commita do poprzedniego head PR #71.
Nie ma migracji canonical state ani publicznego API. Taki rollback przywraca
jednak wszystkie opisane luki evidence i nie powinien być użyty do burn-in ani
promocji PR B.

Nie należy wykonywać częściowego rollbacku samego receipt, Crash delegation,
TimeStop wiring albo route guardu. Te elementy wspólnie tworzą kontrakt, że
rekord V1/V2 jest porównaniem tego samego boundary i posiada właściwą jakość
źródłową.
