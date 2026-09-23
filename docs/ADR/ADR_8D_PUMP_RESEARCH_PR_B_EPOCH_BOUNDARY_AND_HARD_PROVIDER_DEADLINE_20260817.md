# ADR-8D: Pump Research PR-B — epoch-aware complete-slot boundary i twardy provider deadline

**Data:** 2026-08-17

**Status:** IMPLEMENTED LOCALLY / PROVIDER I/O NOT RUN / COMBINED CERTIFY HOLD

**Task:** `PUMP_RESEARCH_PR_B_EPOCH_BOUNDARY_AND_HARD_PROVIDER_DEADLINE`

## D0. Problem

Jedyny wykonany GO-E0 zakończył się poprawnie jako
`blocked_audit_unavailable`, lecz jego findings ujawniły, że wcześniejszy
qualification range zaczynał się od `first rooted + 1`. Taki lower bound
dopuszczał sloty sprzed zestawienia streamu oraz slot, w którego środku capture
zaczął odbierać dane. Dla GO-D było to widoczne jako:

```text
439703807  raw=0  audit=118
439703837  raw=3  audit=90
439703838  raw=85 audit=85
```

Dodatkowy review wykazał dwa niezależne defekty:

- selector najdłuższego rooted range tracił wcześniejszego kandydata przy
  rooted-to-rooted luce numerycznej w mapie slotów;
- provider wall budget był sprawdzany przed slotem, ale aktywny wielokrotny
  request mógł przekroczyć budżet o pełny timeout i retry.

Żaden z tych problemów nie zmienił immutable GO-D raw ani historycznego
GO-E0 receiptu. Wszystkie wymagały lokalnej korekty przed kolejnym probe lub
combined qualification.

## D1. Decyzja: kompletność wyznaczana per `stream_epoch`

PR-B wyznacza teraz granice z istniejącego frozen evidence:

```text
segment header stream_epoch
→ first BlockMeta by capture_sequence
→ eligible start = first BlockMeta slot + 1
→ last BlockMeta by capture_sequence
→ eligible end = last BlockMeta slot
```

Pierwszy `BlockMeta` dowodzi zamknięcia wyłącznie zaobserwowanej części slotu
wejściowego. Dopiero następny slot może być kompletny. Ostatni `BlockMeta`
zamyka authority przed nieudowodnionym ogonem shutdownu.

Brak któregokolwiek boundary, overflow albo pusty interwał zwraca typed
blocker `CaptureStreamBoundaryUnproven`. Nie istnieje fallback do nearest
rooted slot ani do innej epoki.

Każdy kandydat jest dodatkowo dzielony przez:

- status inny niż `RootedCanonical`;
- epoch-local coverage gap;
- brak kolejnego numerycznego slotu w mapie canonicality.

Zakresy po reconnect nie są łączone. Z poprawnych interwałów wybierany jest
najdłuższy, a tie-break wynosi kolejno: wcześniejszy start, niższy epoch,
niższy end. Zarówno pełny independent audit, jak i GO-E0 wywołują ten sam
selector i filtrują raw transactions do wybranego `stream_epoch`.

Indexer sprawdza również zgodność epoch każdego source/gap recordu z
nagłówkiem segmentu oraz niedecreasing epoch między segmentami. Wykorzystuje
wyłącznie istniejące bytes frozen V1; layout, codec, warianty raw enum i
golden fixtures nie zostały zmienione.

## D2. Decyzja: twardy deadline GO-E0

GO-E0 tworzy jeden deadline dla całej provider phase. Przed każdą próbą
wylicza:

```text
remaining_budget = deadline - now
attempt_timeout  = min(configured_timeout, remaining_budget)
```

Po wyczerpaniu `remaining_budget` nie rozpoczyna kolejnego requestu ani retry.
Timeout aktywnego `getBlock` na granicy deadline'u jest zachowywany jako
typed `Unavailable`; kolejne sample slots są oznaczane jako not attempted.
Nie podniesiono timeoutów, retry, concurrency ani wall budgetu.

## D3. Regresje i dowód lokalny

Nowy corpus obejmuje:

1. start streamu w środku slotu;
2. reconnect i zakaz łączenia epok;
3. epokę bez `BlockMeta`;
4. pusty epoch interval;
5. shutdown tail po ostatnim `BlockMeta`;
6. rooted-to-rooted lukę numeryczną;
7. deterministyczny longest-range i tie-break;
8. epoch-local coverage gap;
9. granicę GO-D `837 → 838`;
10. wiszący lokalny RPC, krótki hard deadline i zakaz retry.

Test deadline'u używa wyłącznie loopback TCP i nie wykonuje zewnętrznego
provider I/O. Finalny targeted suite uzyskał `26 passed`, w tym jedną próbę
RPC mimo skonfigurowanych ośmiu retry.

Pełna lokalna bramka po korekcie:

```text
cargo fmt --all -- --check                              PASS
cargo check -p seer --lib                               PASS
cargo check -p seer --bin pump-research-tape            PASS
research_tape_materializer                              26 passed
research_tape filter                                    62 passed, 1 ignored
grpc_connection::tests                                  95 passed
rpc_http_client                                          6 passed
standalone CLI                                           7 passed
ghost-core frozen Pump Research V1                      10 passed
CS0 protobuf/descriptor                                  2 passed
parser parity                                            1 passed
future-capture supervisor                               10 passed
```

Release capture-enabled harness został rzeczywiście uruchomiony z
`--ignored` i uzyskał:

```text
received / admitted / accepted = 8192 / 8192 / 8192
dropped / gaps                 = 0 / 0
published segments             = 1
writer clean                   = true
capture abort                  = false
receive hand-off p99           = 211 ns
SLA                            = 100 000 ns
fatal -> source cancel         = 53 244 307 ns
```

## D4. Wpływ, rollback i następna bramka

Zmiana jest research-only. Nie dotyka active Seer runtime, Yellowstone request,
Gatekeepera, MaterializedFeatureSet, Event Busa, OracleRuntime, execution ani
strategii. Frozen raw V1 pozostaje niezmieniony; addytywny blocker należy do
exact/qualification reportu, nie do binary raw storage.

Rollback oznacza pozostawienie combined qualification w stanie HOLD i brak
wykonywania kolejnego GO-E0. Nie wolno wrócić do `first rooted + 1`, scalać
epok ani zwiększać timeoutów w celu obejścia niedostępności providera.

Po tej korekcie nadal obowiązuje:

```text
GO-D immutable raw                  PASS
historical GO-E0 receipt            ACCEPT / BLOCKED_AUDIT_UNAVAILABLE
epoch-aware local PR-B correction   PASS LOCALLY
provider independence/capacity      UNPROVEN
next provider probe                 SEPARATE GO REQUIRED
combined certify                    NO-GO
exact Ready                         NOT CREATED
export / strategy / execution       NO-GO
```
