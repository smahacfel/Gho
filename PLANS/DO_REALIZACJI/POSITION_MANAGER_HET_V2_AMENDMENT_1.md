# HET-PM V2 — Amendment 1: same-tick V1/V2 comparison boundary

Status: `NORMATIVE CLARIFICATION`

Ten dokument doprecyzowuje rozdział 13 planu `POSITION_MANAGER_HET_V2.md`.

## Problem

Jeżeli engine najpierw zastosuje V1 authority, a dopiero później oceni V2 observe-only, tick kończący pozycję przez V1 może usunąć lub terminalizować stan przed materializacją V2. Skutkiem byłby systematyczny brak V2 evidence na najważniejszych tickach terminalnych oraz bias w porównaniu V1 versus V2.

Jednocześnie V2 observe-only nie może mutować pozycji przed guarded apply V1 ani zwiększać ekonomicznego `state_revision`.

## Normatywna kolejność PR A

```text
1. refresh existing SnapshotTimeline
2. update canonical peak
3. evaluate/update existing TimeStopV2 state
4. pod jednym read boundary zmaterializuj immutable snapshot bundle:
     - V1 snapshot
     - V2 snapshot
     - latest runtime sample
     - latest raw canonical sample
     - CrashGuard evidence sample
5. bez mutacji oceń pure V1 prequote i pure V2 prequote
6. zbuduj lokalny plan quote requests
7. rozwiąż potrzebne in-memory quote cells bez mutacji pozycji
8. zapamiętaj immutable V2 observation result dla tego ticku
9. zastosuj wyłącznie V1 authority przez istniejący sticky proposal/guarded apply
10. po V1 apply zapisz V2 observation z immutable pre-mutation bundle
11. zaktualizuj observer-only peak anchor tylko jeżeli dokładna pozycja/epoka/quantity nadal istnieje i observer guard jest aktualny
```

## Inwarianty

- V1 decision i V2 candidate widzą ten sam causal snapshot boundary.
- V2 observation może zostać zapisana nawet wtedy, gdy V1 terminalizuje pozycję w tym samym ticku.
- Po terminalizacji V1 nie wolno wykonywać nowego V2 quote ani nowej V2 oceny; zapis dotyczy wyłącznie wyniku policzonego przed mutacją.
- Jeżeli pozycja została usunięta przed observer apply, observation nadal może zostać zapisana z immutable bundle, ale peak anchor nie jest już mutowany.
- V2 nie zwiększa ekonomicznego `state_revision`.
- V2 nie tworzy `PendingExitProposal`.
- V2 nie zmienia V1 evidence source ani quote key.
- Quote jest współdzielony tylko przy identycznym `ExecutableQuoteKey`.
- V1 apply ma zawsze pierwszeństwo przed observer-state apply.

## Dodatkowe testy

- `v1_terminal_tick_still_emits_v2_observation_from_same_snapshot`;
- `v2_observation_uses_pre_v1_mutation_quantity_and_revision`;
- `v1_terminal_removal_skips_anchor_apply_but_keeps_observation`;
- `v2_precomputation_does_not_change_v1_guard`;
- `no_v2_quote_is_started_after_v1_terminalization`;
- `same_key_quote_can_be_shared_without_changing_v1_source`;
- `different_raw_vs_runtime_evidence_keys_never_share_quote`.

## Skutek dla PR B

Przy cutoverze V2 authority mechanizm snapshot bundle pozostaje. Zmienia się wyłącznie owner guarded apply:

```text
V2 authority apply
V1 baseline observation after decision
```

V1 baseline nadal jest liczony z tego samego immutable boundary, ale nie może utworzyć proposal ani terminalizować pozycji.
