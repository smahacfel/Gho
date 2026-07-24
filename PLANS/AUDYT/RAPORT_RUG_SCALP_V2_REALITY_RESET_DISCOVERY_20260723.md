# RUG SCALP V2 — Reality Reset: discovery z Run A R8

**Data:** 2026-07-23
**Zakres:** wyłącznie odczyt artefaktów Run A R8 i czysty replay offline.
**Kod strategii / PM / quote math / fee authority / tape / transport:** niezmienione.

## Status źródła

| Pole | Wartość |
|---|---|
| Run A R8 | `INVALID_TRANSPORT_GAP` |
| Alpha | `ALPHA_NOT_EVALUATED` |
| Start runu | `2026-07-23T01:25:49.158Z` |
| Pierwszy silent stall | `2026-07-23T06:31:13.288Z` |
| Jedyny użyty prefix | od startu do kompletnego slotu `434663908` włącznie |
| Wyłączona granica | slot `434663909` i wszystkie dane po reconnectcie |
| Cel analizy | discovery; nie końcowe EV ani wynik Run A |

Stream wznowił emisję po reconnectcie, lecz nie naprawia to przerwy w ciągłości. Późniejsze dane nie zostały połączone z prefixem.

## A. Sekwencyjna attrition V2

Jednostką jest pool (`candidate_id` / mint), nie pojedynczy assessment. Pool przechodzi etap wyłącznie wtedy, gdy **jedna** kanoniczna ocena przed entry spełnia jednocześnie wszystkie poprzednie warunki.

| Sekwencyjny etap | Poole przechodzące |
|---|---:|
| assessed births | 2 777 |
| dwa sloty dostępne | 2 483 |
| wymagane buy counts | 412 |
| wymagany flow | 230 |
| unique users | 225 |
| koncentracja | 199 |
| brak selli | 25 |
| self-impact | 2 |
| `Q_TP / V_2 <= 0,5` | 0 |
| accepted | 0 |

Pierwszy nieosiągnięty etap dla każdego z 2 777 pooli:

| Pierwszy failing guard | Poole |
|---|---:|
| dwa sloty | 294 |
| buy counts | 2 071 |
| flow | 182 |
| unique users | 5 |
| concentration | 26 |
| sell veto | 174 |
| self-impact | 23 |
| `Q_TP / V_2` | 2 |

Wybrane obserwowane maksimum/minimum per pool:

| Miara | Kwantyle / wartość |
|---|---:|
| maks. `n_prev` (P50 / P90) | 1 / 7 |
| maks. `n_curr` (P50 / P90) | 1 / 7 |
| maks. `n_2` (P50 / P90) | 2 / 9 |
| maks. `u_2` (P50 / P90) | 2 / 9 |
| maks. `V_2` (SOL, P50 / P90) | 0,8000 / 10,9869 |
| min. `top1_share` (P10 / P50) | 0,2469 / 0,9990 |
| min. self-impact (bps, P10 / P50), gdy quote powstał | 55 / 55 |
| min. `Q_TP / V_2` (P10 / P50), gdy quote powstał | 0,5696 / 0,5696 |

Quote primary zmaterializował się tylko dla 2 z 2 777 pooli. Oznacza to, że `Q_TP/V_2` jest końcową twardą blokadą w nielicznych już przypadkach, ale największa wcześniejsza attrition wynika z buy counts, a następnie z sell veto.

## B. Ablacja jednego guardu

Wymagany wynik „wyłącz tylko jeden guard” jest identyfikowalny tylko, gdy reducer zmaterializował dane potrzebne po tym guardzie. V2 kończy ocenę fail-closed, dlatego dla większości guardów nie wolno raportować zera jako wyniku ablacjii.

| Wyłączony pojedynczy guard | Obserwowalny dolny limit pooli, które przeszłyby pozostałe warunki | Status |
|---|---:|---|
| `Q_TP / V_2` | 2 | identyfikowalne |
| dwa sloty | — | nieidentyfikowalne przez short-circuit |
| buy counts | — | nieidentyfikowalne przez short-circuit |
| flow | — | nieidentyfikowalne przez short-circuit |
| unique users | — | nieidentyfikowalne przez short-circuit |
| concentration | — | nieidentyfikowalne przez short-circuit |
| sell veto | — | nieidentyfikowalne przez short-circuit |
| self-impact | — | nieidentyfikowalne przez short-circuit |

Wniosek z A–B: V2 jest empirycznie nadmiernie restrykcyjny dla tego ruchu. Nie wynika z tego jeszcze, że każda usunięta bramka poprawi EV ani jaka powinna być nowa wartość progu.

## C. Rzeczywiste zachowania dumpu: dostępny proxy label

W retained raw streamie nie ma `real_sol_reserves`, `real_token_reserves` ani kompletnego historycznego `BondingCurve` potrzebnego do literalnego `RUG_LIKE_DUMP_V1`. Nie wolno zatem nazwać poniższych zdarzeń potwierdzonymi rugami według tej definicji.

Zastosowany **wyłącznie discovery proxy**:

```text
RUG_LIKE_VIRTUAL_DRAIN_PROXY =
  sekwencja successful sell w jednym lub dwóch slotach,
  przed migracją i <= 60 s od birth,
  przy obserwowanym spadku virtual quote reserve >= 30%.
```

| Miara | Wynik |
|---|---:|
| raw births | 3 225 |
| poole z uporządkowanym successful trade <= 60 s | 2 769 |
| birthy bez takiego trade | 456 |
| błędy canonical trade ordering w prefixie | 0 |
| poole z dowolnym sel­lem | 2 120 |
| `RUG_LIKE_VIRTUAL_DRAIN_PROXY` | 588 |
| proxy dump w tym samym slocie | 400 |
| proxy dump cross-slot | 188 |
| proxy dump z sel­lem przed terminalną sekwencją | 412 |

| Rozkład proxy dumpu od birth | P10 | P50 | P90 |
|---|---:|---:|---:|
| czas (ms) | 3 169 | 11 338 | 35 954 |
| buy count przed dumpem | 1 | 4 | 17 |
| buy flow przed dumpem (SOL) | 0,0862 | 0,6949 | 10,2544 |

Wszystkie 588 proxy pooli miały co najmniej jeden successful buy w pierwszych pięciu sekundach przed dumpem. 412/588 miało wcześniejszy sell, co jest zgodne z hipotezą, że veto każdego sellu odcina dużą część interesujących sekwencji; nie dowodzi jednak jeszcze predykcyjnej wartości żadnej alternatywnej reguły.

## D. Opportunity envelope: program-settlement discovery only

Replay użył:

- każdego successful buy w pierwszych 5 s przed terminalną sekwencją jako kandydackiego momentu;
- scenariuszy landing `+1` i `+2` sloty;
- typed wzorów `BuyV2` i `LegacySell` z zamrożonego evidence R8 (`sha256:afde22d496bc7ae8...`), nie `FEE_BPS=100` i nie `BondingCurve::simulate_*`;
- tie target/dump w tym samym slocie jako `DUMP_WINS`;
- zerowych transaction-envelope costs, ponieważ R8 był technicznym capture z tymi kosztami ustawionymi na zero.

Czysty mirror dał 5/5 dokładnych zgodności z dostępnymi zapisanymi `PumpQuoteV1` z R8 (dla dwóch pooli), po znalezieniu kanonicznego poprzednika stanu w tym samym slocie. To jest kontrola implementacji formuły na skąpym zbiorze, nie dowód kompletnej historycznej authority dla wszystkich pooli.

| Scenariusz | Ewaluowalne proxy poole | signal→landing candidates | opportunity poole z +10% program-settlement net | same-slot ties |
|---|---:|---:|---:|---:|
| 0,10 SOL, +1 slot | 519 | 881 | 323 | 1 |
| 0,10 SOL, +2 sloty | 517 | 890 | 319 | 1 |
| 0,20 SOL, +1 slot | 519 | 881 | 322 | 1 |
| 0,20 SOL, +2 sloty | 517 | 890 | 317 | 1 |

Strukturalnie 588/588 proxy pooli miało co najmniej jeden pełny slot od wczesnego buy do dumpu, a 587/588 co najmniej dwa sloty. Nie uzasadnia to odrzucenia koncepcji z powodu całkowitego braku okna czasowego.

## Granica ważności i decyzja

Nie można jednak użyć tabeli D jako:

1. pełnego `RUG_LIKE_DUMP_V1` — brak zachowanego real-reserve evidence;
2. exact executable all-cost PnL — R8 nie rejestrował base fee, priority fee, tip, rent ani retry costów;
3. authority-grade held-out replay — nie ma dla wszystkich przypadków historycznego canonical `PumpReserveState` z real reserves;
4. podstawy do strojenia lub wdrożenia `RugScalpSignalReducerV3`.

Zastąpienie brakujących danych wirtualną rezerwą albo założeniem standardowego reserve offsetu byłoby zmianą definicji eksperymentu, nie walidacją hipotezy.

```text
RUN_A_R8                                = INVALID_TRANSPORT_GAP
ALPHA                                   = NOT_EVALUATED
V2_ATTRITION                            = PROVEN_OVERCONSTRAINED_FOR_PREFIX
NO_PHYSICAL_TIME_WINDOW_KILL            = NOT_SUPPORTED_BY_PROXY_DISCOVERY
AUTHORITY_GRADE_RUG_LABEL_AND_EV_REPLAY = BLOCKED_BY_RETAINED_EVIDENCE
V3_SIGNAL_CHANGE                        = NOT AUTHORIZED
```

Do kontynuacji zgodnej z żądaną definicją potrzeba nowej, jawnie autoryzowanej decyzji o minimalnym capture evidence dla 60-sekundowego prefixu: canonical `BondingCurve` z virtual i real reserves oraz niezerowe, zamrożone koszty execution envelope. Bez tej decyzji jedynym poprawnym wynikiem jest zatrzymanie po discovery, bez nowej reguły i bez kolejnego Run A.
