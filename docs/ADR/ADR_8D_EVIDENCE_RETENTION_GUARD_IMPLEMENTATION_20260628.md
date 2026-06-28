# ADR-8D: Evidence retention cleanup guard implementation

Status: IMPLEMENTED / OFFLINE_GUARD_ONLY / NO_RUNTIME_CHANGE
Typ: ADR-8D / operational safety guard
Data: 2026-06-28
Zakres: rollout evidence retention guard przed kolejnym research
Poziom ryzyka: LOW runtime risk / HIGH evidence-retention importance

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzywany w repo.

## 1. Decyzja

Dodano operacyjny guard:

`scripts/guard_rollout_evidence_cleanup.py`

Guard jest wymagany przed jakimkolwiek przyszlym cleanupem rollout/shadow evidence. Domyslnie dziala w trybie dry-run i nie usuwa danych.

Nie zmieniono runtime, Gatekeepera, BUY/REJECT, selector runtime, TX/Jito/live path, configow rollout ani zadnych sciezek wykonawczych.

## 2. Powod

Po incydencie utraty R48/R49 rollout evidence cleanup logow musi byc fail-closed. Usuniecie evidence nie moze byc mozliwe przez szeroki root, implicit scope, brak manifestu, brak weryfikacji archiwum albo przypadkowe skasowanie krytycznych JSONL.

## 3. Guard semantics

Guard wymusza:

- dry-run default,
- explicit scope allowlist przez `--scope`,
- odmowe broad roots,
- odmowe rootow pod `/mnt/HC_Volume_105935807`,
- odmowe deletion of critical evidence files,
- pre-delete manifest,
- `archive_verified=true`,
- second confirmation token,
- brak runtime side effects.

## 4. Critical evidence files

Cleanup jest blokowany, jezeli target zawiera m.in.:

- `gatekeeper_v2_decisions.jsonl`,
- `gatekeeper_v2_buys.jsonl`,
- `materialized_feature_snapshot.jsonl`,
- `shadow_lifecycle.jsonl`,
- `probe_shadow_lifecycle.jsonl`,
- `shadow_exit_replay_v1.jsonl`,
- `selector_shadow_score_v1.jsonl`,
- lifecycle launcher PASS reports.

## 5. Execution contract

Tryb wykonawczy jest dopuszczalny tylko po spelnieniu wszystkich warunkow:

1. `--execute`,
2. `--manifest <existing-pre-delete-manifest>`,
3. `--archive-verified true`,
4. manifest zawiera `archive_verified: true`,
5. current file set dokladnie pasuje do manifestu,
6. brak critical evidence files,
7. brak symlinkow,
8. `--confirm-token DELETE_EVIDENCE:<digest>`.

Bez tych warunkow guard konczy sie fail-closed.

## 6. Non-goals

Ten PR nie:

- uruchamia cleanupu,
- usuwa danych,
- startuje research run,
- startuje RUG-MARKUP-A0,
- zmienia runtime,
- zmienia log schema,
- zmienia Gatekeeper/selector/TX/Jito/live behavior.

## 7. Konsekwencje

Do czasu zacommitowania tego guarda nie wolno startowac RUG-MARKUP-A0 ani innego nowego research runu.

Po zacommitowaniu guard staje sie minimalnym operacyjnym wymogiem przed jakimkolwiek cleanupem evidence. Cleanup bez manifestu i tokenu ma byc traktowany jako violation.

## 8. Files

- `scripts/guard_rollout_evidence_cleanup.py`
- `docs/ADR/ADR_8D_EVIDENCE_RETENTION_GUARD_IMPLEMENTATION_20260628.md`
- `PLANS/AUDYT/RAPORT_EVIDENCE_RETENTION_GUARD_IMPLEMENTATION_20260628.md`
