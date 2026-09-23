# ADR-8D: ACE Day 1 — lokalizacja alias conflict po downstream apply

**Data:** 2026-07-31
**Status:** FOCUSED VALIDATED / RELEASE BUILD PASS / PROVENANCE FREEZE PENDING / SMOKE HOLD / DAY 1 NO-GO
**Zakres:** Jedna korekta klasyfikacji błędu w PR1E acknowledgement Oracle.
Nie zmienia CandidateIntegrity storage, Ledger, Gatekeepera, Brain, ACE probe'a,
capacity, backpressure, execution ani PR2.

## D0. Dowód przyczyny

Świeży 10-minutowy smoke został poprawnie zatrzymany przez health receipt,
ponieważ `pr1_runtime_candidate_admission_closed_total = 1`.

Pierwszy istotny trace jest następujący:

```text
2026-07-31T00:00:52.064881Z
OracleRuntime::complete_canonical_apply
→ CandidateIntegrityRegistry::mark_canonical_apply_succeeded
→ CandidateAliasConflict
→ Oracle bezwarunkowo close_candidate_admission_with_integrity_invalidation(
      "ready_publication_failed"
  )
```

Dotyczył applied mutation:

```text
pool      = 7FHtFp56ryvHXuTw1Vbb1WFhjE22PPouzimxg7ktYk3W
mint      = sddut2J8dbFtEeDScDkcSvWvXebJnqV6hqB3qpJpump
signature = 32uXAjkw6VKhih2Fw7uBrFA3YSxnB9wcJmPELhEeG7aNJChjXHzYL1zqtMMRbubhuVPzjKLLdq3JAQkkxj92tkc4
locator   = outer=2, inner=[8], ordinal=29
```

Wcześniejsza obserwacja `ContinuityOnly` PumpSwap dla tego mintu została już
poprawnie odcięta przed Ledgerem i CandidateIntegrity. Nie była źródłem tego
close. Brakująca obsługa znajdowała się wyłącznie w Oracle, po tym jak
canonical mutation była już prawidłowo applied downstream.

## D1. Decyzja

`CandidateAliasConflict` zwracany przez
`mark_canonical_apply_succeeded(receipt)` jest resultatem lokalnym:

```text
downstream applied
→ CandidateIntegrity alias conflict
→ receipt/proof terminalnie reclaimed przez registry
→ zero Ready release dla konfliktującej mutacji
→ global candidate admission pozostaje otwarte
```

Wszystkie pozostałe błędy acknowledgementu nadal wykonują istniejące globalne
fail-close `ready_publication_failed`.

## D2. Inwarianty

- applied mutation z `CandidateAliasConflict` nie otrzymuje Ready release ani
  evaluation authority;
- failed receipt oraz inventory proof nie pozostają nierozwiązane;
- pojedynczy candidate-local conflict nie zamyka admission dla niezwiązanych
  przyszłych kandydatów;
- `RegistryUnavailable`, capacity, fence contradiction oraz każdy inny błąd
  acknowledgementu pozostają globalnym fail-close;
- runtime nadal jest shadow/observe-only; nie powstaje Trigger, Position
  Manager, live execution ani nowy decision plane.

## D3. Weryfikacja

Focused test odtwarza właściwe okno czasu:

```text
target receipt staged
→ target transaction inventory sealed
→ niezależny candidate zajmuje shared mint alias
→ Oracle acknowledgement targetu
→ CandidateAliasConflict
→ receipt/proof = 0/0
→ target has no evaluation guard
→ global admission remains open
```

Wykonane local checks:

```text
cargo test -p ghost-launcher \
  downstream_candidate_alias_conflict_reclaims_receipt_without_closing_global_admission \
  --lib -- --nocapture                         PASS
cargo test -p ghost-launcher alias_conflict --lib -- --nocapture
                                                    PASS (5)
cargo test -p ghost-launcher \
  pr1e_frozen_cross_layer_corpus_executes_each_scenario_through_production_adapters \
  --lib -- --nocapture                         PASS
cargo fmt --all --check                        PASS
cargo build --release -p ghost-launcher \
  --bin ghost-launcher --bin ace_core_one_day_probe
                                                    PASS
```

Kolejny qualifying smoke musi mieć wszystkie wcześniej zamrożone health,
drain, finalization i offline-probe bramki.

## D4. Poza zakresem

Nie zmieniono zasad identyfikacji aliasów ani terminal tombstones,
nie wyłączono reclaimu, nie otwarto ponownie admission po prawdziwym błędzie,
nie zmieniono queue/capacity/backpressure i nie dokonano żadnej zmiany
strategicznej ACE.
