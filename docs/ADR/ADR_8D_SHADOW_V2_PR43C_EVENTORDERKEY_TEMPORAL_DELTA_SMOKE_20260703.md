# ADR-8D: Shadow V2 PR43-C EventOrderKey Temporal Delta Smoke

## Status

Accepted as report-only evidence.

## Decyzja

Po merge PR44 / PR43-B wykonano 900-sekundowy smoke `shadow-smoke-v2-eventorderkey-pr43b-r1`, aby zmierzyc realny runtime delta klasyfikacji `EventOrderKey`. Wynik:

```text
PR43C_EVENTORDERKEY_TEMPORAL_DELTA_CLASSIFIED_STILL_BLOCKED
```

Nie przyznano L2. Nie przyznano research-grade, live-equivalence ani runtime approval.

## Kontekst

PR43-A0 ustalil, ze sciezka do L2 istnieje, ale blokuja ja temporal/order ambiguity, brak `account_data_hash`, density i sample size. PR43-B mial poprawic klasyfikacje chain-order, nie magicznie dostarczyc brakujace dane z upstream.

## Dowody

- `runtime_post_run_manifest_status=PASS`
- `post_run_strict_audit_status=PASS`
- `clean_shutdown_proven=true`
- `event_order_key_missing_required_rows=0`
- `non_monotonic_event_seq_in_process=0`
- `entry_pool_state_handoff_signature_reused_count=0`
- `terminal_truth_derived_component_count=276`
- `explicit_unknown_chain_order_components=2134`
- `unknown_but_required_for_research_count=2134`

## Konsekwencje

1. PR43-B dziala jako klasyfikacja temporalna w realnym runtime smoke.
2. Brak wymaganych `event_order_key` zostal usuniety dla ordering-sensitive canonical rows.
3. Position-created i smoke marker sa explicit exempt, nie cicho pomijane.
4. Terminal truth uzywa jawnych komponentow `DERIVED`.
5. Entry pool-state nie recyklinguje timestamp/signature z handoffu jako chain signature.
6. L2 nadal jest zablokowane przez jawne `UNKNOWN` komponenty wymagane do research provenance.

## Odrzucone interpretacje

- Nie interpretujemy tego smoke jako L2 PASS.
- Nie interpretujemy diagnostic executable roundtrip jako strategy proof.
- Nie traktujemy `event_seq_in_process` jako substytutu chain order dla L2.
- Nie uznajemy `DERIVED` terminal truth za observed chain order.
- Nie przyznajemy runtime approval, shadow_close_only approval ani active close approval.

## Granice runtime

Nie zmieniono BUY/REJECT, Gatekeeper policy, selector runtime, TX/Jito/live path, R51, shadow_close_only ani active close. Run byl validation/fidelity-only.

## Approval flags

- runtime_approval=false
- shadow_close_only_approval=false
- active_close_approval=false
- research_grade=false
- live_equivalence=false
- strategy_research_unblocked=false

## Nastepny krok

PR43-D powinien skupic sie na realnych zrodlach brakujacych chain-order komponentow albo na formalnym blockerze `BLOCKED_EVENTORDERKEY_SOURCE_MISSING` dla tych pol, ktorych obecny ingest/handoff nie moze dostarczyc bez rozszerzenia runtime evidence surface.
