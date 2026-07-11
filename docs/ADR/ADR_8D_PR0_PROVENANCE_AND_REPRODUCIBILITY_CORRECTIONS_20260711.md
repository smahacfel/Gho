# ADR-8D: Korekty provenance i reprodukowalności PR0 kontraktów metryk

Status: `ACCEPTED / PR0_PROVENANCE_AND_REPRODUCIBILITY_PASS / DOCUMENTATION_ONLY`

Typ: ADR-8D / review-amendment decision

Data: 2026-07-11

Repo: `/root/Gho_dynamic_exit_v1`

PR: `https://github.com/smahacfel/Gho/pull/60`

Plan:
`PLANS/DO_REALIZACJI/PLAN_KOREKTY_KONTRAKTOW_INTERPRETACJI_METRYK_V1_20260710.md`

ADR amendowany:
`docs/ADR/ADR_8D_PR0_METRIC_CONTRACT_BASELINE_RECONCILIATION_AND_FEASIBILITY_20260711.md`

Poziom ryzyka: `LOW` dla runtime — wyłącznie dokumentacja i evidence artifacts;
`HIGH` dla przyszłego PR1, jeśli canonical hash/config/identity contracts nie
zostałyby zachowane.

Uwaga o szablonie: `/Gho/docs/ADR/ADR_8D_SZABLON.md` i lokalny
`docs/ADR/ADR_8D_SZABLON.md` nie istnieją w checkoutcie. Dokument używa
sekcyjnego formatu ADR-8D stosowanego w repo.

## 1. Kontekst review

Review PR #60 zaakceptowało merytoryczny rdzeń dziesięciu kontraktów, ale
zablokowało PR1 do usunięcia sześciu luk dowodowych:

1. `f3318f3...` był błędnie nazwany jednocześnie audytowanym commitem i
   merge-base PR #60.
2. Liczby feasibility nie miały checked machine-readable manifest/summary ani
   utrwalonego skanera.
3. Różne `brain_config_hash` były niepoprawnie przedstawione jako automatyczna
   niezgodność z bundle wymagającym Gatekeeper config hash.
4. Duplicate record identity był zlewany z powtórzeniem samego `join_key` między
   runami.
5. SHA-256 nie miał normatywnej canonicalization i self/transport exclusion.
6. Existing PR0 ADR opisywał trzy outputy tak, jakby były pełnym ośmioplikowym
   PR-em.

Nie było potrzeby powtarzania code audit ani 19 testów semantycznych.

## 2. Decyzja provenance

Rozdzielono role:

```text
audited_code_commit      = f3318f3a71a9202ced7af9cf43c064fa9f2f0c4a
pr60_base_and_merge_base = f1e3292aae935d1b43e2c265c078f9ec74a62563
```

Oba commity mają tree OID:

```text
92e97058349157b591a24f11da3bec0642051cd7
```

Exact `git diff --quiet <audited> <base> -- .` zwrócił 0. Wniosek:
`TREE_EQUIVALENCE_PASS`; audytowana zawartość jest zgodna z base PR #60, bez
fałszywego utożsamienia SHA.

## 3. Decyzja reprodukcji feasibility

Dodano:

```text
reports/metric_contracts/pr0_input_manifest_v1.json
reports/metric_contracts/pr0_feasibility_summary_v1.json
reports/metric_contracts/pr0_reproduction_v1.md
```

Manifest zawiera dla każdego stabilnego inputu path/basename, SHA-256, bytes,
rows, min/max timestamp, schema, run ID, decision plane, Gatekeeper i brain
config hashes oraz immutable classification. Mutable r5 jest oddzielnym
`excluded_input` z null SHA/counts.

Reproduction doc zawiera pełne źródło
`pr0_feasibility_scanner_v1`, SHA źródła, wersje narzędzi, exact extraction/run/
compare commands, nearest-rank percentile definition, duration, record identity
i field-coverage rules.

Surowe 3.220 GB JSONL nie są przechowywane w Git. GitHub-only recomputation jest
niemożliwa bez inputs pod manifest SHA; ta granica jest jawna. Z content-addressed
inputs skaner failuje na jakimkolwiek mismatchu i odtwarza checked summary.

## 4. Decyzja effective config

Przyjęto trzy role:

```text
brain_config_hash                         provenance only
gatekeeper_config_hash                    policy parity
metric_contract_effective_config_hash     metric evidence equivalence
```

PR1 definiuje `ResolvedMetricContractEffectiveConfigV1`. Hash obejmuje wszystkie
resolved values, defaults i stałe wpływające na producentów dziesięciu metryk,
population, success/dust, dedupe, windows, identity/order, quality/status,
actionability i comparator. Nie obejmuje unrelated selector/exit/execution
settings bez wpływu na evidence plane.

Historyczne v33 rows nie emitują tego hasha. Mimo wspólnego Gatekeeper hash nie
można dowieść effective-config equivalence; wynik to
`NOT_MEASURABLE_PRE_IMPLEMENTATION`. Różny pełny brain hash nie jest samodzielnym
FAIL.

## 5. Decyzja identity

Record identity:

```text
(run_id, join_key, decision_plane)
```

Tylko powtórzenie pełnej krotki jest duplicate record. Powtórzenie `join_key`
między runami jest diagnostyką, nie automatycznym duplicate.

Underlying-event collision używa odrębnego
`stable_event_identity: Option<StableEventIdentityV1>`. Brak tego pola daje
unavailable/not-evaluable, nie zero collisions. Historyczne v33 rows nie emitują
stable identity, więc cross-run event collision pozostaje
`NOT_MEASURABLE_PRE_IMPLEMENTATION`.

## 6. Decyzja canonical hash

Przyjęto `CanonicalHashV1`:

```text
canonicalization = RFC 8785 JCS
encoding = UTF-8
algorithm = SHA-256
digest = lowercase 64-char hex
hash input = typed semantic payload without self-hash and transport fields
optional unavailable = explicit null
omitted required key = error
newline/BOM = excluded
non-finite float = forbidden
wide integers outside interoperable I-JSON range = typed canonical base-10 strings
```

Profile, effective config i evidence mają osobne hash payload structs. Pola
transportowe nie są filtrowane dynamicznie; są poza semantic type. PR1 musi mieć
JCS test vectors obejmujące Unicode, key order, liczby, `-0`, null/omitted,
NaN/Inf i self-hash exclusion.

## 7. Scope i lista plików

Zmodyfikowano:

- `PLANS/DO_REALIZACJI/PLAN_KOREKTY_KONTRAKTOW_INTERPRETACJI_METRYK_V1_20260710.md`
- `reports/metric_contracts/baseline_reconciliation_v1.md`
- `reports/metric_contracts/historical_feasibility_preflight_v1.md`
- `docs/ADR/ADR_8D_PR0_METRIC_CONTRACT_BASELINE_RECONCILIATION_AND_FEASIBILITY_20260711.md`
- `docs/ADR/ADR_8D_PLAN_KOREKTY_KONTRAKTOW_INTERPRETACJI_METRYK_V1_1_20260711.md`

Dodano:

- `reports/metric_contracts/pr0_input_manifest_v1.json`
- `reports/metric_contracts/pr0_feasibility_summary_v1.json`
- `reports/metric_contracts/pr0_reproduction_v1.md`
- `docs/ADR/ADR_8D_PR0_PROVENANCE_AND_REPRODUCIBILITY_CORRECTIONS_20260711.md`

Po korekcie pełny PR #60 ma 12 dokumentacyjnych plików. Nie zmieniono Rust,
tracked `.py`, TOML, testów, policy, runtime schema ani aktywnego r5. Embedded
scanner w Markdown nie jest runtime/toolchain code.

## 8. Acceptance

Wymagane przed przywróceniem PASS:

- audited/base/merge-base roles i tree equality są machine-readable;
- input manifest JSON parsuje się i wszystkie expected fields przechodzą;
- scanner source extraction ma deklarowany SHA;
- scanner kompiluje się Pythonem 3.12.3;
- generated summary jest byte-identical z checked JSON;
- record/event identities są rozdzielone;
- effective config hash contract jest normatywny;
- RFC 8785 JCS hash contract jest normatywny;
- pełny PR scope jest poprawnie opisany;
- brak nowych runtime/code changes.

Po spełnieniu:

```text
PR0_SEMANTIC_CONTENT_PASS
PR0_PROVENANCE_AND_REPRODUCIBILITY_PASS
BASELINE_RECONCILIATION_PASS
PR1_FOUNDATION_ALLOWED
```

```yaml
delegation_trace:
  task_classification: PR review provenance and reproducibility amendment
  routing_performed: true
  primary_specialist: Ghost Runtime Coordinator
  supporting_specialists_considered:
    - Decision Logging Replay Analyst
    - Config Rollout Safety Reviewer
    - Statistical Research Engine
  specialist_docs_loaded: []
  specialist_docs_not_loaded:
    - name: runtime specialist documents
      reason: semantic audit was accepted and no runtime implementation changed
    - name: Solana Execution Path Engineer
      reason: execution path is outside this documentation-only amendment
  skills_used:
    - github:gh-address-comments
    - ghost-execution
    - large-data-analytics
  fast_path_used: false
  contracts_checked:
    - audited commit versus PR base provenance
    - content-addressed feasibility inputs
    - machine-readable deterministic reproduction
    - effective producer configuration equivalence
    - record identity versus underlying-event identity
    - RFC 8785 canonical hashing
    - PR file-scope accounting
    - no runtime or policy drift
  unresolved_routing_uncertainty: []
```
