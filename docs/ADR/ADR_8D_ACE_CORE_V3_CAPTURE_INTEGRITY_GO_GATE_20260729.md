# ADR-8D: ACE Core V3 — bramka integralności capture przed Dniem 1

Status: `IMPLEMENTED / FOCUSED_VALIDATION_PASS / RELEASE_BUILD_PASS / DAY1_NOT_STARTED /
OBSERVE_ONLY / PR2_STILL_BLOCKED`

Typ: ADR-8D / remediation capture-integrity / offline falsification evidence

Data: `2026-07-29`

Repo: `smahacfel/Gho`

Baseline naukowy: `origin/main = 43057b296663129ca9b4f572e793474830a5452c`

Plan SSOT:
`PLANS/DO_REALIZACJI/PLAN_ACE_CORE_ONE_DAY_KILL_TEST_V3_POST_PR86.md`

Poprzedni ADR implementacyjny:
`docs/ADR/ADR_8D_ACE_CORE_ONE_DAY_KILL_TEST_V3_POST_PR86_IMPLEMENTATION_20260728.md`

Uwaga o szablonie: ścieżka wskazana w instrukcji globalnej,
`/Gho/docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Dokument
zachowuje lokalny format ADR-8D stosowany w `docs/ADR/`.

## D0. Decyzja

Przed 24-godzinnym Dniem 1 ACE Core V3 musi istnieć bramka `GO`, która
fail-closed chroni wynik `SELECTED` versus `REST` przed czterema klasami
fałszu:

1. failed transaction jako rzekomy Pump reserve state;
2. ciche znikanie malformed births albo unjoinable/divergent trade evidence;
3. niedowodliwy stan PR1E/EventWriter podczas capture;
4. fałszywy source provenance przypisany do baseline PR #86.

Ta korekta implementuje tę bramkę. Nie uruchamia 24 h capture, nie tworzy
nowej decyzji runtime i nie zmienia Gatekeepera, `MaterializedFeatureSet`,
quote math, routingu, Position Managera ani PR2.

## D1. Canonical economic state

`reserve_observation()` przyjmuje state do entry, triggera, landing i
confirmation tylko wtedy, gdy durable payload równocześnie ma:

```text
success = true
is_synthetic = Some(false)
complete = Some(false)
```

W szczególności reserves z failed transakcji nie są canonical post-state i
nie mogą zostać użyte do ekonomicznego proxy. Brak legalnego stanu daje
istniejący typed `NON_EVALUABLE_RESERVES` albo
`NON_EVALUABLE_SUSTAIN_COVERAGE`, nigdy ukryty fallback.

## D2. Full-universe i strict evidence integrity

Pump/SOL birth z niejednoznacznym mintem, curve albo pool identity nie jest
pomijany: zachowuje terminalny row, a capture staje się `INVALID_CAPTURE`.
Wyłącznie observation rzeczywiście poza Pump/SOL universe nie należy do
denominatora ACE.

Każdy `PoolTransaction` musi mieć oczekiwane schema `v1`, zgodny quote mint,
jednoznaczne mint aliases oraz zgodne `pool_amm_id`, `pool_id` i bonding
curve. Trade, którego nie można jednoznacznie zjoinować z canonical birth,
unieważnia run zamiast selektywnie oczyszczać feature window.

Pełny mutation key pozostaje:

```text
signature + slot + tx_index + outer_instruction_index +
inner_group_index + event_ordinal
```

Identyczny duplicate delivery jest legalny. Dwa różne material payloads pod
tym samym pełnym kluczem unieważniają capture. Nie ma reguły `first-wins`.

## D3. Dowodliwy health capture

Trzy wymagane PR1 counters są teraz eksportowane przez istniejący launcher
loopback Prometheus:

```text
pr1_runtime_bypass_attempt_total
pr1_runtime_candidate_admission_closed_total
pr1_runtime_primary_coverage_gap_total
```

Wszystkie istniejące miejsca zwiększające bypass counter aktualizują także
ten sam Prometheus counter. Zamknięcie candidate admission oraz primary local
coverage gap dostają analogiczny eksport. EventWriter jawnie loguje i liczy
write/lock failures zamiast zamieniać brak durable eventu w cichy fakt.

`scripts/ace_core_one_day_capture_health.py` nie jest nową usługą runtime.
Zapisuje dwa immutable scrape'y loopback, sprawdza ich zero-count, syntax i
final newline wszystkich `exec_*.jsonl`, logi EventWritera, coverage/fee
markers oraz kontrolowany shutdown. Następnie tworzy raz manifest-bound
receipt pod zarezerwowaną ścieżką.

Offline probe wymaga receiptu. Weryfikuje jego schema, run ID i SHA-256
manifestu oraz odrzuca capture przy niezerowym counterze, writer failure,
braku clean flushu albo braku dowodu kontrolowanego shutdownu.

## D4. Prawdziwy provenance

Manifest schema v3 rozróżnia:

```text
baseline_sha        = naukowy parent PR #86
implementation_sha  = pełny SHA commitu implementacji ACE
code_hash           = git:<implementation_sha>
binary_hash         = hash faktycznie zbudowanej binarki
```

`signal_detector` ma neutralną nazwę
`ace_core_one_day_probe_v3_observe_only`, a nie nazwę odrzuconego RUG V2.
Probe propaguje implementation/code/binary provenance do calibration i
summary oraz fail-closed odrzuca mismatch.

Zamrożona implementacja wynosi:

```text
implementation_sha = 3bc28fc320518e13c3a5113ed9d1fab1c4e115be
```

Config rollout celowo zawiera do czasu zamrożenia commitu placeholder
`PENDING_FINAL_ACE_IMPLEMENTATION_SHA`; preflight odrzuca taki capture.
Po utworzeniu immutable commitu implementacji jedyny następny commit
operacyjny wpisuje jego SHA w `implementation_sha` i `code_hash`. Dzięki temu
source implementation i config provenance są rozdzielone od baseline PR #86.

## D5. Weryfikacja wymagana przed `GO`

Focused suite obejmuje dodatkowo failed reserve state we wszystkich rolach,
malformed birth denominator, unjoinable trade, divergent full-key duplicate,
schema mismatch, manifest-bound health receipt i rzeczywisty eksport trzech
series przez `/metrics`.

Na finalnym source/config state wykonano:

```text
cargo fmt --all --check                                                    PASS
cargo test -p ghost-launcher ace_core_one_day_probe --lib -- --nocapture  PASS (29/29)
cargo test -p ghost-launcher oracle_metrics --lib -- --nocapture          PASS (1/1)
cargo test -p ghost-launcher metrics_server_tests --bin ghost-launcher    PASS (6/6)
cargo build --release -p ghost-launcher --bin ghost-launcher
  --bin ace_core_one_day_probe                                             PASS
python3 -m py_compile scripts/ace_core_one_day_capture_health.py          PASS
```

Przed Dniem 1 trzeba wykonać w nowym scope 2–5 minut smoke:

1. uruchomić observe-only launcher z nowym run ID/paths;
2. zapisać startowy scrape po widoczności trzech counters;
3. potwierdzić rosnące `exec_*.jsonl` z birth i `PoolTransaction`, balances,
   `is_synthetic=false`, full order key i complete reserves;
4. bezpośrednio przed SIGINT zapisać końcowy scrape;
5. zakończyć launcher kontrolowanie i uruchomić `finalize`;
6. uznać smoke tylko przy kodzie 0 i immutable health receipt.

Smoke nie jest Dniem 1 i jego artefakty nie mogą wejść do analizy sygnału.

## D6. Granice i rollback

Zmiana zachowuje observe-only/offline-only boundary. Nie tworzy BUY, entry
intent, live/shadow execution, nowej polityki Gatekeepera, modelu ML ani
PR2 ingest–state–quote. Rollback polega na nieuruchamianiu nowego rollout;
nie istnieje aktywna ścieżka, która wymaga cofnięcia zachowania rynkowego.

`GO` dla Dnia 1 nie wynika z tego ADR. Jest możliwe dopiero po zielonym
finalnym buildzie, wpisaniu prawdziwego implementation SHA, pozytywnym
smoke i receipt z kodem 0.
