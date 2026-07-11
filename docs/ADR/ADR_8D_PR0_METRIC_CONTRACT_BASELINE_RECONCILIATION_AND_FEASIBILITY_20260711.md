# ADR-8D: PR0 baseline reconciliation i feasibility preflight kontraktów metryk

Status: `ACCEPTED / BASELINE_RECONCILIATION_PASS / PROVENANCE_AMENDED / DOCUMENTATION_ONLY`

Typ: ADR-8D / audit baseline decision

Data: 2026-07-11

Repo: `/root/Gho_dynamic_exit_v1`

Audytowany commit:
`f3318f3a71a9202ced7af9cf43c064fa9f2f0c4a`

Base i merge-base PR #60:
`f1e3292aae935d1b43e2c265c078f9ec74a62563`

Tree equivalence: `PASS`, wspólny tree OID
`92e97058349157b591a24f11da3bec0642051cd7`.

Plan:
`PLANS/DO_REALIZACJI/PLAN_KOREKTY_KONTRAKTOW_INTERPRETACJI_METRYK_V1_20260710.md`

Raporty decyzji:

- `reports/metric_contracts/baseline_reconciliation_v1.md`
- `reports/metric_contracts/historical_feasibility_preflight_v1.md`

Poziom ryzyka zmiany: `LOW` — tylko dokumentacja i read-only audit. Ryzyko
przyszłej implementacji pozostaje `HIGH` dla SSOT, policy parity, schema/replay,
event identity/order i evidence logging.

Uwaga o szablonie: wskazany globalnie `/Gho/docs/ADR/ADR_8D_SZABLON.md` oraz
lokalny `docs/ADR/ADR_8D_SZABLON.md` nie istnieją w tym checkoutcie. Dokument
zachowuje sekcyjny format ADR-8D używany przez sąsiednie decyzje.

## 1. Kontekst

Plan V1.1 zablokował runtime implementation do czasu formalnego pogodzenia
dziesięciu kontraktów z aktualnym kodem oraz oceny wykonalności prospective
burn-in. Było to konieczne, ponieważ część pracy już istniała, a część nazw
łączyła różne semantyki lub ukrywała fallback/missing state.

PR0 miał rozstrzygnąć w szczególności:

- czy top3 helper i preferred field są już gotowe;
- który dev-buy surface jest rzeczywistym aktywnym Phase 5 authority;
- czy FTDI value i FTDI actionability używają tej samej populacji;
- czy legacy flip jest literalnym 10 s kontraktem;
- czy FSC legacy i FSC v2 quality są rozdzielone;
- czy manipulation/reserve/RCE odróżniają missing od real zero;
- które schema/replay/logger capabilities istnieją;
- czy historyczny payload pozwala policzyć flip-v2 evaluable i dev divergence;
- czy początkowa skala 8 h/700/100 jest operacyjnie osiągalna bez post-hoc.

## 2. Decyzja

Przyjęto oba raporty PR0 i status:

```text
BASELINE_RECONCILIATION_PASS
PR1_FOUNDATION_ALLOWED
RUNTIME_AND_POLICY_UNCHANGED
PROVENANCE_AND_REPRODUCIBILITY_PASS
```

PASS oznacza, że baseline jest kompletny i gotowy jako wejście PR1. Nie oznacza
gotowości do PR2C burn-in ani PR3 cutover.

## 3. Rozstrzygnięcie provenance i reprodukcji

Pierwsza wersja błędnie nazwała `f3318f3...` jednocześnie audytowanym commitem i
merge-base. Role są rozdzielone:

```text
audited_code_commit      = f3318f3a71a9202ced7af9cf43c064fa9f2f0c4a
pr60_base_and_merge_base = f1e3292aae935d1b43e2c265c078f9ec74a62563
audited_tree_oid         = 92e97058349157b591a24f11da3bec0642051cd7
base_tree_oid            = 92e97058349157b591a24f11da3bec0642051cd7
```

`git diff --quiet <audited> <base> -- .` zwrócił 0. Audit nie jest więc
semantycznie stale, ale dokument nie utożsamia już commitów. Exact komendy i
wyniki utrwala `pr0_reproduction_v1.md`.

Feasibility evidence otrzymuje:

- machine-readable input manifest;
- machine-readable generated summary;
- wersjonowany skaner osadzony w reproduction doc, z SHA źródła;
- exact calculation/tool/version/test transcript;
- jawne ograniczenie: raw 3.220 GB inputs nie są w Git i muszą być dostępne pod
  manifest SHA do ponownego przeliczenia.

## 4. Najważniejsze rozstrzygnięcia baseline

1. `MaterializedFeatureSet` i `evaluate_from_features` pozostają jedyną
   autorytatywną ścieżką snapshot → policy.
2. FTDI value już używa `unique_topologies / unique_buyers`, ale legacy
   clean/degraded gate używa całkowitego BUY count. Corrected actionability musi
   pozostać counterfactual.
3. Aktywny Phase 5 dev read to MFS TxIntel first-observed. GatekeeperBuffer
   create-signature primary buy jest osobnym compatibility surface.
4. Same-ms ma co najmniej exact, `<50 ms` cluster i successful recent exact
   semantykę; source jest częścią kontraktu.
5. Top3 preferred field, fallback helper i aktywne reads już istnieją. Nie wolno
   implementować helpera drugi raz.
6. Legacy flip łączy globalne time window z per-owner slot gap i ingestuje przed
   TxIntel dedupe/dust/success handling. Flip V2 nie istnieje.
7. Legacy FSC jest distinct-known collision ratio; FSC v2 types/evidence już
   istnieją, ale są non-authoritative.
8. `evidence_status.fsc` opisuje legacy presence, nie FSC v2 quality.
9. Manipulation `high_*` pozostają default false, a raw default `0.0` nie
   rozróżnia missing od measured zero.
10. Reserve velocity jest per-update receive-time rate z zero bootstrap/fallback;
    RCE `buy_sell_ratio_recent` zwraca buy count przy zero sells.
11. Canonical availability/quality/authority/actionability envelope i rollout
    profile są brakujące i należą do PR1.

## 5. Decyzja feasibility

Cztery stabilne historyczne runy zawierają:

```text
36.2505 h
31,266 unique decisions
28,489 dev-known
0 malformed rows
0 duplicate record identities
```

To potwierdza wykonalność skali duration/decisions/dev-known. Historyczny payload
ma jednak zero create-signature rows, zero raw tx sequences i zero tx-order
provenance. Dlatego:

```text
clean flip-v2 evaluable = NOT_MEASURABLE_PRE_IMPLEMENTATION
real dev divergence     = NOT_MEASURABLE_PRE_IMPLEMENTATION
```

Aktywny r5 został wyłączony z zamrożonego agregatu jako
`ACTIVE_MUTABLE_NOT_IMMUTABLE`. Historyczne pre-run manifests nie obejmują
decision JSONL, a Gatekeeper V2 strict replay readiness wynosi tylko 76.74%.
Żaden historyczny run nie jest validation evidence ani burn-in PASS.

## 6. Canonical hash, effective config i identity

Przed PR1 obowiązuje `CanonicalHashV1`:

```text
canonicalization = RFC 8785 JCS
encoding = UTF-8
algorithm = SHA-256, lowercase 64-char hex
hash input = schema-defined semantic payload bez self-hash i transport fields
optional unavailable = explicit null
omitted required key = error
trailing newline = excluded
NaN/+Inf/-Inf = forbidden
wide integers = schema-typed canonical base-10 strings
```

Profile, effective config i evidence mają osobne typed hash payload structs.
Nie filtruje się pól dynamicznie po nazwie. Evidence hash nie obejmuje writer
timestamp, rotation/part metadata ani własnego digestu.

Bundle equality używa nowego `metric_contract_effective_config_hash`, który
obejmuje resolved settings wpływające na producer, population, success/dust,
dedupe, identity/order, windows, status/actionability i comparator. Pełny
`brain_config_hash` pozostaje provenance-only; Gatekeeper hash nadal chroni
policy parity. Historyczne runy nie emitują effective hash, więc ich config
equivalence jest `NOT_MEASURABLE_PRE_IMPLEMENTATION`.

Duplicate record identity to powtórzenie pełnej krotki
`(run_id, join_key, decision_plane)`. Cross-run powtórzenie samego `join_key` jest
diagnostyką. Underlying-event collision ma osobne `stable_event_identity`; jego
brak daje unavailable/not-evaluable, nie clean zero.

## 7. Anty-post-hoc i burn-in freeze

`BURN_IN_CONTRACT_V1` nie jest zamrożony przez PR0. Po implementacji V2
producers oraz audit CLI w PR2B/PR2C należy ponowić kontrolowany feasibility
audit, następnie właściciel planu zatwierdzi exact minima przed prospective
collection.

Po `frozen_at` zmiana gate unieważnia bundle i wymaga nowej wersji oraz nowych
runów. Historyczne feasibility rows nigdy nie liczą się do validation counts.

## 8. Konsekwencje dla kolejnych PR-ów

PR1 może rozpocząć się od registry/profile/status foundation. Musi adaptować
istniejące top3/FSC/status surfaces, a nie je dublować. Implementuje również
`CanonicalHashV1`, effective config hash i rozdzielone record/event identities.

PR2A zachowuje aktywne legacy authority i dodaje typed/counterfactual evidence.
PR2B implementuje evidence-only flip/manipulation/reserve/RCE. PR2C dodaje v34,
paired sidecar, full replay/audit CLI i dopiero wtedy zamraża burn-in contract.
PR3 może objąć wyłącznie formalnie równoważne entries.

## 9. Granice decyzji

ADR nie autoryzuje:

- zmiany BUY/REJECT/TIMEOUT, progów, wag, phase order ani reason codes;
- przełączenia dev-primary, corrected FTDI actionability, flip-v2 lub FSC-v2 do
  aktywnej policy;
- bypassu MFS ani live-state reads w policy;
- destructive schema/config migration;
- zatrzymania lub modyfikacji aktywnego r5;
- użycia feasibility data jako validation evidence;
- post-hoc obniżenia minimów.

## 10. Pliki tej zmiany

PR0-specific semantic outputs w pierwotnym zadaniu:

- `reports/metric_contracts/baseline_reconciliation_v1.md`
- `reports/metric_contracts/historical_feasibility_preflight_v1.md`
- `docs/ADR/ADR_8D_PR0_METRIC_CONTRACT_BASELINE_RECONCILIATION_AND_FEASIBILITY_20260711.md`

Pierwotny pełny PR documentation chain na commicie `7d0116e` miał 8 plików:
source audit, plan, trzy wcześniejsze ADR-y oraz powyższe trzy PR0 outputs.

Korekta provenance dodaje:

- `reports/metric_contracts/pr0_input_manifest_v1.json`
- `reports/metric_contracts/pr0_feasibility_summary_v1.json`
- `reports/metric_contracts/pr0_reproduction_v1.md`
- `docs/ADR/ADR_8D_PR0_PROVENANCE_AND_REPRODUCIBILITY_CORRECTIONS_20260711.md`

Pełny PR documentation chain po korekcie ma 12 plików. Nie zmieniono Rust,
tracked `.py`, TOML, testów, aktywnego runa ani istniejących artefaktów
użytkownika. Skaner jest utrwalony wyłącznie jako źródło w Markdown i nie jest
częścią runtime/toolchain.

## 11. Weryfikacja

PR0 zweryfikowano przez:

- claim-by-claim code/path audit na commit SHA;
- SHA/row/schema/join/replay/size scan czterech stabilnych v33 JSONL;
- machine-readable manifest i exact generated summary;
- scanner source SHA, Python compile i exact output comparison;
- audited/base tree OID equality i zero tree diff;
- jawne wyłączenie mutable r5;
- siedem wąskich poleceń testowych obejmujących FTDI, top3, legacy flip,
  dev-primary, MFS FTDI/FSC/temporal materialization i reserve units;
- scope audit potwierdzający documentation-only change.

Wszystkie uruchomione właściwe testy przeszły. Zastane compiler warnings nie
zostały zmienione ani ukryte.

```yaml
delegation_trace:
  task_classification: cross-cutting read-only metric-contract baseline and feasibility audit
  routing_performed: true
  primary_specialist: Ghost Runtime Coordinator
  supporting_specialists_considered:
    - SSOT Feature Materialization Guardian
    - Gatekeeper Policy Auditor
    - Decision Logging Replay Analyst
    - Config Rollout Safety Reviewer
    - Seer Ingest Event Integrity Specialist
    - Statistical Research Engine
  specialist_docs_loaded:
    - docs/agents/ghost-runtime-coordinator.md
    - docs/agents/ssot-feature-materialization-guardian.md
    - docs/agents/gatekeeper-policy-auditor.md
    - docs/agents/decision-logging-replay-analyst.md
    - docs/agents/config-rollout-safety-reviewer.md
    - docs/agents/seer-ingest-event-integrity-specialist.md
  specialist_docs_not_loaded:
    - name: Oracle Session Runtime Engineer
      reason: session scheduling and deadline behavior were inspected only as existing materialization context and were not changed
    - name: Solana Execution Path Engineer
      reason: builder, submit, confirmation and live execution are outside PR0 scope
  skills_used:
    - ghost-execution
    - large-data-analytics
    - statistical-research-engine
  fast_path_used: false
  contracts_checked:
    - MaterializedFeatureSet SSOT
    - producer and consumer authority
    - active versus compatibility versus shadow versus logging/export-only paths
    - deterministic policy parity
    - config and schema compatibility
    - replay completeness and artifact identity
    - event identity, order and duplicate provenance
    - logger backpressure and resource baseline
    - prospective validation and anti-post-hoc freeze
    - shadow/live separation
  unresolved_routing_uncertainty: []
```
