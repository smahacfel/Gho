# Raport Shadow V2 PR18F Canonical Event Ordering 2026-07-02

## 1. Executive verdict

Finalny verdict PR32:

```text
PR32_IMPLEMENTATION_READY_FOR_VALIDATION
```

PR32 naprawia kontrakt ordering dla canonical Shadow V2 event stream bez uruchamiania kolejnego burnina i bez zmian w BUY/REJECT, Gatekeeper policy, selector runtime, TX/Jito/live path, `shadow_close_only` ani active close.

Zakres jest implementacyjny i dotyczy tylko:

- canonical `shadow_position_event_v2` wrapper;
- `shadow_terminal_truth_v2` event ordering;
- explicit ordering exemption dla `shadow_position_v2` / `POSITION_CREATED`;
- per-position canonical `event_seq_in_process`;
- temporal/no-lookahead audit script.

PR32 nie nadaje:

```text
research_grade = true
live_equivalence = true
runtime_approval = true
shadow_close_only_approval = true
active_close_approval = true
strategy_research_unblocked = true
```

## 2. Root cause z PR31 / PR18E

Analiza lokalnego scope:

```text
reports/selector/shadow-v2-fidelity-validation-pr18e-r1/shadow_position_event_v2.jsonl
```

wykazala:

```text
event_order_key_missing_rows = 379
non_monotonic_event_seq_in_process = 1
```

Rozklad missing `event_order_key`:

| event_family | missing_event_order_key_count | reason |
|---|---:|---|
| `shadow_position_v2` / `POSITION_CREATED` | 190 | position-created i smoke marker nie mialy `event_order_key` ani explicit ordering exemption |
| `shadow_terminal_truth_v2` / `TERMINAL_TRUTH` | 189 | terminal truth byl uzywany do replay/lifecycle terminal reconciliation, ale nie mial `event_order_key` |

Przyklad missing position-created:

```text
event_id = validation_smoke_marker_v2:validation-smoke-marker:shadow-burnin-v2-fidelity-validation-pr18e-r1:1782999242290
position_id = validation-smoke-marker:shadow-burnin-v2-fidelity-validation-pr18e-r1
event_kind = POSITION_CREATED
```

Przyklad missing terminal truth:

```text
event_id = shadow_v2_terminal_truth:9ghvnaNNEDPetncnNXuoqAcn1Jir9A6toqouLqeAB1vD:ARDBJbmhJgy4CiVetpuGXELsbKuF1BWGrsuehgRwpump:1782999265172:1782999302896:TIMEOUT
event_kind = TERMINAL_TRUTH
```

Root cause non-monotonic sequence:

- runtime emitowal zdarzenia dla tej samej pozycji w kolejnosci durable append innej niz timestamp-derived `event_seq_in_process`;
- stary `event_seq_in_process = timestamp * 10 + offset` mogl regresowac, gdy entry/lifecycle family byla dopisana po pozniejszych path/exit rows;
- audit liczyl monotonicity per `position_id`, a writer nie gwarantowal canonical per-position high-watermark.

Przyklad pozycji z regresja:

```text
position_id = CgTPH2J4WXAbfvtUDtorTBV5gkcMT5dBmwVUgb7XHPvm:7tyQys64BDcHX7TDyW9gAFxLQqwD4b95m9U6aXsmpump:1783000835797
PATH_SAMPLE seq = 17830008358111
EXIT_ATTEMPT seq = 17830008358112
POOL_STATE_SAMPLE seq = 17830008358113
EXIT_FILL seq = 17830008358114
ENTRY_ATTEMPT seq = 17830008291001
ENTRY_FILL seq = 17830008291002
PATH_SAMPLE seq = 17830008358871
```

## 3. Implementacja PR32

### 3.1 Terminal truth ordering

`shadow_terminal_truth_v2` ma teraz `event_order_key`.

Konstruktor lifecycle-to-terminal ustawia:

- `slot` z `exit_landed_slot` albo `exit_sample_slot`, jesli znany;
- `signature` z `exit_market_anchor_tx_signature`, jesli znany;
- `observed_at_wall_ms` / process sequence z terminal timestamp;
- explicit `UNKNOWN` dla nieznanych chain-order components.

Nie tworzono fake signature, fake tx index, fake instruction index, fake inner index, fake log index ani fake block time.

### 3.2 Position-created ordering exemption

`shadow_position_v2` / `POSITION_CREATED` nie dostaje syntetycznego chain ordering. Zamiast tego canonical wrapper ma explicit:

```text
ORDERING_EXEMPT_POSITION_CREATED
```

Dla validation smoke marker:

```text
ORDERING_EXEMPT_VALIDATION_SMOKE_MARKER
```

Ta exemption jest dopuszczona tylko dla `POSITION_CREATED`; nie pozwala `TERMINAL_TRUTH`, fill, path ani pool-state rows przejsc bez ordering.

### 3.3 Required ordering fail-closed

Canonical writer failuje, gdy ordering-sensitive event family nie ma `event_order_key`.

Ordering wymagany jest dla:

```text
pool_state_sample_v2
shadow_entry_attempt_v2
shadow_entry_fill_v2
shadow_path_sample_v2
shadow_exit_attempt_v2
shadow_exit_fill_v2
shadow_terminal_truth_v2
```

Typed error:

```text
MissingRequiredEventOrderKey
```

### 3.4 Monotonic `event_seq_in_process`

Canonical stream utrzymuje high-watermark per:

```text
(run_id, position_id)
```

Helper:

```text
shadow_v2_event_seq_for_position(previous_seq, attempted_seq)
```

gwarantuje:

```text
canonical_seq = max(attempted_seq, previous_seq + 1)
```

Jesli writer musi podniesc sequence, aktualizuje rowniez nested payload:

```text
payload.record.event_order_key.event_seq_in_process
```

Dzieki temu top-level wrapper i canonical payload nie rozjezdzaja sie.

### 3.5 Temporal audit script

`scripts/shadow_v2_temporal_no_lookahead_audit.py` rozroznia teraz:

- rows with `event_order_key`;
- rows with explicit ordering exemption;
- rows missing required ordering;
- rows missing unclassified ordering.

FAIL jest generowany tylko dla:

- canonical row wymagajacego ordering bez `event_order_key`;
- row bez ordering i bez jawnej exemption;
- non-monotonic sequence;
- post-entry field jako pre-decision evidence;
- terminal truth jako pre-entry evidence;
- derived replay/lifecycle jako canonical input.

BLOCKED pozostaje dla explicit `UNKNOWN` chain-order components przy zachowanym ordering contract.

## 4. Granice runtime

PR32 nie uruchamia burnina i nie zmienia strategii.

Nie zmieniono:

- BUY/REJECT;
- Gatekeeper policy;
- selector runtime;
- TX/Jito/live path;
- `shadow_close_only`;
- active close;
- runtime approval flags;
- R51.

PR32 nie stage'uje raw JSONL, logow, runtime scope ani lokalnych `*.local.toml`.

## 5. Testy i walidacja

Targeted tests dodane lub zmodyfikowane:

```text
cargo test -p ghost-brain shadow_v2_terminal_truth_has_event_order_key -- --nocapture
cargo test -p ghost-brain shadow_v2_position_created_ordering_exemption_is_explicit -- --nocapture
cargo test -p ghost-brain shadow_v2_event_seq_is_monotonic_per_position -- --nocapture
cargo test -p ghost-brain shadow_v2_temporal_audit_fails_missing_required_event_order_key -- --nocapture
cargo test -p ghost-brain shadow_v2_temporal_audit_allows_explicit_position_created_exemption -- --nocapture
python3 tests/test_shadow_v2_temporal_no_lookahead_audit.py
```

Wymagane checki PR:

```text
cargo check -p ghost-brain
cargo check -p ghost-launcher
cargo fmt --check
git diff --check
git diff --cached --check
python3 -m py_compile scripts/shadow_v2_temporal_no_lookahead_audit.py
forbidden staged-file guard
```

## 6. Odpowiedzi kontraktowe

### Czy `shadow_terminal_truth_v2` ma event_order_key?

Tak. `ShadowTerminalTruthV2` ma pole `event_order_key`, a `ShadowV2Record::event_order_key()` zwraca `Some(&EventOrderKey)` dla terminal truth.

### Czy `shadow_position_v2` ma event_order_key albo explicit ordering exemption?

`shadow_position_v2` ma explicit ordering exemption:

```text
ORDERING_EXEMPT_POSITION_CREATED
ORDERING_EXEMPT_VALIDATION_SMOKE_MARKER
```

Nie dostaje fake chain-order.

### Czy non-monotonic event_seq_in_process zostal usuniety w testach?

Tak. Test `shadow_v2_event_seq_is_monotonic_per_position` dowodzi, ze writer podnosi regresyjny per-position sequence do `previous + 1`, a inna pozycja ma niezalezny licznik.

## 7. Follow-up

Po merge PR32 wymagany jest osobny operator decision przed kolejnym burninem.

Nastepny validation/fidelity burnin powinien sprawdzic:

```text
event_order_key_missing_required_rows = 0
event_order_key_missing_unclassified_rows = 0
non_monotonic_event_seq_in_process = 0
terminal truth ordering present
position-created exemption counted explicitly
no lookahead/pre-decision violation
```

PR32 sam nie nadaje research-grade, runtime approval, active close ani live-equivalence.
