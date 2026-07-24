# RUG reality R6 — transaction-local exact Pump state replay

**Data:** 2026-07-23  
**Zakres:** offline-only replay istniejącego R6.  
**Źródło:** `datasets/events/rug_reality_capture/r6/exec_launcher-1784820600263_20260723_153000_0000.jsonl`  
**SHA-256 źródła:** `507ced8ac1291d6985e2aa373a7d17f7f382a70dabbd2f916d652724408e9991`

## Werdykt

```text
R6_EXACT_PER_INSTRUCTION_STATE   = FAIL
OBSERVED-FACT REPLAY UPPER BOUND = 58.759664%
RELEASE_BUILD / LIVE_PREFLIGHT = NOT AUTHORIZED
RUG_REALITY_CAPTURE_1          = NOT AUTHORIZED
```

R6 nie oblał dlatego, że nie ma bezpośredniego snapshotu po każdej
instrukcji. Replay wykorzystał końcowy pełny state **dołączony** do
kanonicznie ostatniego trade factu tej samej grupy
`(slot, signature, bonding_curve)`, wykonał
reverse walk po raw `token_amount_units` i
`effective_curve_quote_lamports`, a potem forward replay.

Nie użyto joinu po samym `slot + bonding_curve`, późniejszego snapshotu,
arrival order, mark price, ShadowLedgera ani interpolacji konta.

## Wynik na 6 079 live successful Pump trade rows

| Klasa | Trade rows |
|---|---:|
| `DIRECT_EXACT` | 3 340 |
| `RECONSTRUCTED_EXACT` | 232 |
| `NON_EVALUABLE_NO_ANCHOR` | 791 |
| `NON_EVALUABLE_UNKNOWN_MUTATION` | 4 |
| `NON_EVALUABLE_CONSERVATION_MISMATCH` | 1 519 |
| `NON_EVALUABLE_OTHER` | 193 |
| **replay upper bound na zachowanych factach** | **3 572 / 6 079 = 58.759664%** |

Licznik `DIRECT_EXACT` oznacza kanonicznie ostatni fact z pełnym tuplem w
**obserwowanym-fact premise**. `RECONSTRUCTED_EXACT` oznacza wcześniejszą
instrukcję w tej samej udanej grupie. Wszystkie klasy non-evaluable są
przypisane całej grupie, zgodnie z fail-closed kontraktem.

R6 nie zachowuje manifestu wszystkich instrukcji, które modyfikują dany
`BondingCurve`, ani provenance final state'u rozróżniającego local direct
state od ewentualnej suplementacji writer'a. Z tego powodu `3 572 / 6 079`
jest **górnym limitem** replayu pod założeniem, że zachowany zestaw trade
facts jest pełny; nie jest dowodem braku nieznanej mutacji. Pod rygorystycznym
kontraktem `UNKNOWN_MUTATION` exact coverage może być tylko niższe — nigdy
wyższe. R6 oblewa bramkę już na tym korzystnym upper bound.

## Integralność replayu

| Sprawdzenie | Wynik |
|---|---:|
| grupy `(slot, signature, bonding_curve)` | 5 830 |
| forward/reverse state mismatch | 0 |
| mismatch dostępnego eventowego virtual tuple | 0 |
| typed transition geometry mismatch | 1 519 grup / rows |
| arithmetic underflow | 0 |
| nierozstrzygnięte przejście `complete` | 2 grupy / 4 rows |
| brak końcowego transaction-local anchoru | 776 grup / 791 rows |
| zero albo brak trade factu (`token` lub `curve quote`) | 193 grup / rows |

`typed_transition_geometry_mismatch` nie jest tolerancją roundingową. Dla
każdego takiego factu policzona reverse/forward delta wraca do tego samego
tuple, lecz surowe `(virtual_sol, virtual_token, token_amount, curve_quote)`
nie spełnia bit-exact żadnej z dwóch znanych typed geometrii buy ani geometrii
exact-base-in sell. To oznacza, że nie można nazwać tych rows exact state
transition evidence.

Przykładowy direct final state o niezgodnej geometrii:

```text
slot       434741107
signature  3jwHUpuecgaWNQM6FhE333bHq9K6ce9xzyLn8Pw8vKhHSNcUdUfX2TcVBnVNmb1xMByjt7d8SN1kgFmLRZtz8ScA
curve      8aR1NoDzcaRdjWmyvzHpWK6C4do21t9jurLNREkbzjRT
order      tx_index=82, event_ordinal=13
side       buy
token      31_891_021_615_008
curve q    1_185_185_184 lamports
```

Przy odwróconym stanie wejściowym dokładna reguła `ceil(k / base_after)`
daje `35_185_185_183`, gdy durability row deklaruje końcowe virtual SOL
`35_185_185_184`; wariant exact-quote-in również nie odtwarza deklarowanego
virtual token tuple. Różnica nawet jednego lamporta jest mismatch — nie
zostaje zaokrąglona ani dopasowana heurystycznie.

## Co ten wynik dowodzi

1. **Sama korekta bramki była słuszna.** 232 wcześniejsze instruction states
   zostały odtworzone exact bez osobnego AccountUpdate po każdej instrukcji.
2. **R6 mimo to nie jest wystarczającym exact tape.** 791 rows nie ma
   transaction-local final anchoru, 193 nie ma używalnego raw trade factu,
   a 1 519 nie przechodzi conservation z aktualnym typed transition
   contractem.
3. **Nie wolno przejść do testów z kroku 2 ani do live preflightu.** PASS
   wymaga minimum 99% coverage, udowodnionego braku nieznanej mutacji oraz
   zerowych conservation mismatchy. R6 ma najwyżej 58.759664% i 1 519
   mismatchów.

Nie jest dozwolone zastąpienie tych classes późniejszym account snapshotem,
faktycznym lub mark price, estymacją state'u z sąsiedniego slotu ani
normalizacją roundingową.

## Reprodukcja

```bash
python3 scripts/rug_reality_exact_state_audit.py \
  --input datasets/events/rug_reality_capture/r6/exec_launcher-1784820600263_20260723_153000_0000.jsonl \
  --output logs/rug_reality_capture/r6/exact_state_audit_v1.json
```

Skrypt kończy się kodem `2`, gdy bramka nie przechodzi; jest to oczekiwany
fail-closed wynik R6, nie błąd uruchomienia. Pełny, maszynowy receipt jest w
`logs/rug_reality_capture/r6/exact_state_audit_v1.json`.

## Granica kolejnego ruchu

Ten raport nie autoryzuje nowego capture'u, nowego detektora, zmian PM,
Gatekeepera, fee authority, quote math ani strategii. Ustala tylko, że
obecne R6 nie może spełnić bramki `>=99% EXACT PER-INSTRUCTION STATE`.
