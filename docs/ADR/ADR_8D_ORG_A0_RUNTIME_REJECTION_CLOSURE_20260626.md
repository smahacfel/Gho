# ADR-8D: PR-ORG-A0 Runtime Rejection Closure

Status: IMPLEMENTED / RUNTIME_REJECTED / RESEARCH_INCONCLUSIVE
Typ: ADR-8D / offline research closure
Data: 2026-06-26
Autor/Agent: Codex
Repo/branch: `/root/Gho`, local working tree
Commit/PR: not committed at ADR creation time
Zakres: closure verdict dla PR-ORG-A0 organic pool candidate policy
Poziom ryzyka: LOW runtime risk / LOW repo risk / MEDIUM analytical risk

Dotkniete moduly/pliki:
- `PLANS/AUDYT/RAPORT_ORGANIC_POOL_CANDIDATE_POLICY_A0_20260626.md`
- `docs/ADR/ADR_8D_ORG_A0_RUNTIME_REJECTION_CLOSURE_20260626.md`

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzywany w repo.

## 1. Przygotowanie i dzialania wstepne

Cel:
Zamknac PR-ORG-A0 mocniejszym werdyktem po tail-source audit dla dodatniego avg w S1/F5 i C1.

Twarde ograniczenia:
- bez runtime change,
- bez `shadow_close_only`,
- bez Gatekeeper policy change,
- bez selector change,
- bez `alpha_31100`,
- bez XGBoost,
- bez kolejnego strojenia progow na R48/R2,
- bez stagingu i commita.

Audit input:
- juz wygenerowane `shadow_exit_replay_v1` i decision-time features,
- kandydackie kohorty S1/F5 i C1 z PR-ORG-A0,
- exit cell `target_bps=7500`, `stop_bps=-100`, `max_hold_ms=30000`,
- `roundtrip_cost_bps=100`.

## 2. Wykorzystane skills/specjalizacja

Routing:
- primary specialist: Decision Logging Replay Analyst,
- supporting: Gatekeeper Policy Auditor, large-data analytics, statistical research,
- specialist doc previously loaded for PR-ORG-A0: `docs/agents/decision-logging-replay-analyst.md`.

Skills previously used for PR-ORG-A0:
- `ghost-execution`,
- `large-data-analytics`,
- `statistical-research-engine`.

## 3. Opis problemu - 3W2H

What:
Pierwotny raport PR-ORG-A0 mial werdykt `INCONCLUSIVE`, ale metryki holdout i tail-source audit pokazuja, ze nie ma podstaw do runtime.

Where:
- `PLANS/AUDYT/RAPORT_ORGANIC_POOL_CANDIDATE_POLICY_A0_20260626.md`,
- R48/R2 `shadow_exit_replay_v1`,
- summary/stability CSV wygenerowane przez `scripts/organic_candidate_policy_proof.py`.

Why it matters:
Sredni dodatni PnL bez dodatniej mediany i bez stabilnej precision moze byc skutkiem rzadkiego prawego ogona. Taki wynik nie moze otwierac runtime gate.

How observed:
Tail-source audit rozbil cost100 sum PnL na Target / Stop / TimeOut oraz policzyl top 1%, 5%, 10% rekordow wedlug replay PnL.

How many:
- S1/F5: 1154 rows total, 458 holdout,
- C1: 768 rows total, 306 holdout.

## 4. Przyczyna zrodlowa

Root cause wyniku dodatniego:
Dodatni avg w F5/C1 nie wynika ze stabilnej precision. Wynika z rzadkiego prawego ogona: Targetow i dodatnich TimeOutow. Po odjeciu top 5% rekordow reszta S1/F5 i C1 jest ujemna zarowno overall, jak i na holdoucie.

## 5. Strategia zamkniecia

Zamiast proponowac nowe progi albo runtime action:
- wzmocniono verdict w raporcie,
- jawnie oznaczono runtime jako rejected,
- zachowano research jako inconclusive tylko w sensie naukowym,
- wskazano, ze ewentualny kolejny research to tail-source audit, nie tuning progow.

## 6. Przeprowadzone akcje

Zmiana 1:
Zaktualizowano final verdict w raporcie na:
`REJECTED_FOR_RUNTIME / INCONCLUSIVE_RESEARCH`.

Zmiana 2:
Dopisano hard closure findings:
- C1 nie bije F5 na holdout avg/sum,
- C2-C5 maja 0% Target na holdoucie,
- wszystkie mediany sa ujemne po kosztach,
- dodatni avg pochodzi z prawego ogona / dodatnich TimeOutow / duzych hitow,
- organic edge gate nie jest spelniony.

Zmiana 3:
Dopisano tail-source audit dla S1/F5 i C1.

## 7. Wynik operacyjny

Werdykt:
`REJECTED_FOR_RUNTIME / INCONCLUSIVE_RESEARCH`

Najwazniejsze fakty:
- S1/F5 holdout: avg `188.659`, sum `86406`, median `-200`.
- C1 holdout: avg `186.637`, sum `57111`, median `-200`.
- S1/F5 all: TimeOut sum `303610`, Target sum `111000`, Stop sum `-190800`.
- C1 all: TimeOut sum `208397`, Target sum `74000`, Stop sum `-124600`.
- S1/F5 top 5% all: top sum `290890`, rest sum `-67080`.
- C1 top 5% all: top sum `198546`, rest sum `-40749`.
- S1/F5 top 5% holdout: top sum `111718`, rest sum `-25312`.
- C1 top 5% holdout: top sum `78508`, rest sum `-21397`.

Conclusion:
Dodatni avg w F5/C1 jest zalezne od prawego ogona i dodatnich TimeOutow. Nie jest powtarzalnym wzorcem precision wystarczajacym do runtime.

## 8. Walidacja

Walidacje wykonane:
- import offline skryptu z `sys.modules` guard,
- ponowne zbudowanie kohort z `shadow_exit_replay_v1` i decision rows,
- tail-source audit dla S1/F5 i C1 na cost100,
- patch raportu tylko w dokumentacji offline.

## 9. Ryzyka resztkowe

- To nadal pojedynczy run R48/R2.
- Tail-source audit odpowiada na source of avg, ale nie dowodzi zadnej alternatywnej polityki.
- Winner profile pokazuje pewne roznice decision-time, lecz nie daje runtime-safe edge.

## 10. Scope out

Nie wykonano:
- runtime change,
- Gatekeeper policy change,
- selector change,
- `shadow_close_only`,
- `alpha_31100`,
- XGBoost,
- kolejnego strojenia progow.

## 11. Decyzja

PR-ORG-A0 jest zamkniety jako:
`REJECTED_FOR_RUNTIME / INCONCLUSIVE_RESEARCH`.

Nastepny dopuszczalny research, jesli temat wraca, to analiza zrodla ogona na nowych/niezaleznych danych, nie kolejne strojenie progow na R48/R2.
