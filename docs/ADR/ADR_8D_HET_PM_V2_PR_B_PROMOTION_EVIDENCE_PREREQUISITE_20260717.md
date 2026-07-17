# ADR-8D: HET-PM V2 PR B — deterministyczny promotion-evidence prerequisite

Status: ACCEPTED FOR VALIDATION BURN-IN / NO AUTHORITY CHANGE

Typ: ADR-8D / rollout evidence / promotion gate prerequisite

Data: 2026-07-17

Repozytorium: `/root/Gho_dynamic_exit_v1_pr2b`

Gałąź: `agent/het-pm-v2-promotion-evidence`

Podstawa: merge PR #71, commit `4ff01d876fbf41206f6669c836c5f3e38338800b`

Uwaga o szablonie: wskazany globalnie plik
`/root/Gho/docs/ADR/ADR_8D_SZABLON.md` nie istnieje. Dokument zachowuje lokalny układ
D1-D8 używany przez ADR-8D w repozytorium.

## 1. Problem

PR #71 dostarczył neutralnego, observe-only producenta HET-PM V2, ale nie stanowił
dowodu pozwalającego wykonać PR B. Plan wymaga przed cutoverem:

- zamrożonych, machine-readable kryteriów;
- co najmniej dwóch niezależnych validation runów;
- ścisłego joinu comparison, writer-health, lifecycle, replay i kohort;
- oceny CVaR, tail loss, MFE capture, kosztów, ekstremów i stabilności kohortowej;
- deterministycznego artefaktu, w którym `promotion_gate_passed` jest koniunkcją
  Gate 1-5.

Pierwszy pełny run kalibracyjny `r1b` wykazał dodatkowo dwa błędy producenta:

1. próbka z minimalnie przyszłym timestampem prowadziła do unsigned underflow w
   materiale CrashGuarda;
2. eksportowy denominator FTDI sumował liczbę dostępnych sygnatariuszy jako `u8`,
   co powodowało overflow powyżej 255 obserwowanych sygnatariuszy.

Run zakończony z panicami i bez clean shutdown nie może być użyty jako evidence
promocyjne, nawet jeśli część obserwacji jest poprawna.

## 2. Decyzja

Przed właściwym PR B utrzymywana jest osobna faza promotion evidence:

1. zamrożenie criteria v1 przed validation runami;
2. deterministyczne narzędzie manifest/evaluate/validate;
3. zachowanie kalibracyjnego `r1b` jako jawnego artefaktu `FAIL`;
4. usunięcie runtime paniców bez zmiany BUY/REJECT ani ekonomii V1/V2;
5. bounded, jawny shutdown z administracyjnym usunięciem pozycji censored;
6. dwa niezależne runy `r2a` i `r2b` z różnymi launch cohort IDs;
7. brak authority cutoveru, dopóki kanoniczny artefakt PASS nie zostanie
   wygenerowany, zwalidowany i committed.

Żądany profil wejściowy zachowuje dokładnie:

- `min_tx_count = 5`;
- `min_buy_count = 3`;
- `min_unique_signers = 3`.

Pozostałe progi doboru kohorty są poluzowane do tolerancyjnych wartości. Nie
zmienia to HET-PM V2, V1 ani TimeStop V2 config identity. Run pozostaje shadow-only,
a V1 pozostaje jedynym proposal/apply/terminal/capacity ownerem.

## 3. D1 — Dane i źródła prawdy

Jednostką analizy jest unikalne:

```text
(run_id, position_id, position_epoch)
```

Denominator pozycji pochodzi wyłącznie z primary shadow `PositionOpened`, gdzie
order ID ma prefiks `shadow-entry-`. Źródła evidence:

- `het_pm_v2_observations_v1.jsonl` — same-tick V1/V2 comparison;
- `het_pm_v2_writer_health_v1.*.json` — pełny producer/writer denominator;
- `shadow_lifecycle.jsonl` — kanoniczna prawda V1 o terminalu;
- `shadow_exit_replay_v1.jsonl` — bounded mark path po wejściu;
- primary `PositionOpened` — denominator pozycji;
- Gatekeeper BUY rows — wyłącznie creator/funder cohort attribution;
- runtime log — panic, forced shutdown i clean-shutdown evidence;
- dokładny brain config i run config — content identity runu.

Join BUY do pozycji używa `(pool_id, base_mint)`, ponieważ BUY rows nie posiadają
`candidate_id`. Creator/funder identity służy tylko do segmentacji offline i nie
wchodzi do HET policy.

Executable candidate/terminal return i mark-only replay są odrębnymi klasami
pomiaru. Raport nie nazywa mark proxy ani gross executable value authoritative
net PnL.

## 4. D2 — Determinizm

Każdy input w run manifeście posiada SHA-256 i rozmiar. Manifest utrwala osobno:

- comparison i writer-health schema version;
- HET, V1 oraz TimeStop V2 config hash;
- pełny brain config content hash;
- run config content hash;
- run ID, launch cohort ID i rolę calibration/validation.

`validate` ponownie oblicza wszystkie gate checks i root conjunction. Odrzucane są:

- ręcznie zmieniony `promotion_gate_passed`;
- ręcznie zmieniony wynik pojedynczego gate'u;
- brakujące lub non-finite wartości;
- nieznane metryki i niepełny exact metric contract;
- mixed config/schema identities;
- zmieniony analysis tool lub criteria file;
- zmieniony, brakujący albo niezgodny input artifact.

Identyczne inputy, criteria i tool version tworzą bitowo identyczny JSON.

## 5. D3 — Coverage i reconciliation

Criteria v1 definiują osobno 62 metryki wraz z jednostką, denominatorem,
kierunkiem, progiem oraz missing-data semantics. Gate 1-5 obejmują:

- lifecycle integrity i zero runtime paniców;
- exact position/comparison/replay coverage;
- producer i writer end-to-end capture;
- route, quote, trajectory i anchor coverage;
- per-tick quote deduplication i bounded quote budget;
- candidate-versus-terminal gross executable outcome;
- CVaR lower-tail 20%, tail p10 i terminal-loss delta;
- MFE capture, peak-to-terminal giveback, vitality occupancy;
- jawne false-early-exit oraz missed-protection proxy;
- cost scenarios 0/50/100/200 bps;
- top-3 positive-improvement concentration i trimmed mean;
- run, terminal reason, trajectory, anchor, route, age i entry-time segmenty;
- creator/funder identity coverage oraz dominację liczebną i ekonomiczną kohort.

Gate 4 nie przechodzi bez minimalnej liczby matched candidates, MFE, vitality i
missed-protection eligible positions. Gate 5 nie przechodzi bez dwóch niezależnych
validation runów i dwóch launch cohortów. Brak danych nie jest zerem ani HOLD-em.

## 6. D4 — Runtime, shutdown i zasoby

Lifecycle launcher:

- używa tego samego release binary dla preflightu i runtime;
- nie wymaga istnienia opcjonalnego `.env`;
- uruchamia runtime z `RUST_BACKTRACE=1`;
- stosuje `timeout --signal=INT --kill-after=120s`;
- zachowuje twardy horyzont runu i ograniczony shutdown.

Post-buy shutdown ma dziesięciosekundowy drain terminali. Po budżecie pozostałe
pozycje są klasyfikowane jako censored i usuwane administracyjnie dopiero po:

1. zatrzymaniu authority monitora;
2. flushu exit replay;
3. finalizacji HET writer-health.

Administracyjne usunięcie nie tworzy proposal, fill, PnL ani fałszywego terminal
disposition. Zapobiega natomiast zakleszczeniu shutdown watcherów i umożliwia
trwały clean-shutdown marker.

## 7. D5 — Degradacja i fail-closed

Próbka z timestampem większym niż `now_ms` staje się invalid/stale evidence;
nie może wywołać panicu ani zostać potraktowana jako potwierdzony Crash.

FTDI zachowuje trwały saturating `u8` schema, ale lokalny denominator coverage jest
liczony jako `usize`. Jest to zmiana diagnostyczno-eksportowa: nie modyfikuje
Gatekeeper BUY/REJECT, `MaterializedFeatureSet` authority ani progów policy.

Każdy z poniższych stanów blokuje promocję:

- panic, forced shutdown lub brak clean-shutdown marker;
- niepełny producer/writer shutdown;
- brak exact joins lub correlation;
- brak wymaganej próbki;
- missing/non-finite economic evidence;
- pogorszenie tail/CVaR/MFE/cost thresholds;
- wynik napędzany top ekstremami albo jedną kohortą;
- causal/data contract violation.

FAIL promotion gate nie zmienia działającego V1 authority.

## 8. D6 — Deployment i rollback

Profile `r2a` i `r2b` zachowują:

- `[execution].execution_mode = "shadow"`;
- `[trigger].entry_mode = "shadow_only"`;
- HET-PM V2 `mode = "observe_only"`;
- TimeStop V2 `mode = "observe_only"`;
- CrashGuard `mode = "observe_only"`;
- brak live submitu i brak V2 lifecycle authority.

Rollback fazy evidence oznacza zatrzymanie ograniczonego runu i powrót do
zaakceptowanego PR A. Nie wymaga migracji pozycji i nie tworzy dual authority.

## 9. D7 — Diagnostyka

Kalibracyjny `r1b` został przeliczony z pełnego manifestu jako `FAIL`:

- primary positions: `85`;
- comparison rows: `1597`;
- position/comparison coverage: `0.2823529411764706`;
- position/replay coverage: `0.2235294117647059`;
- runtime panic count: `7`;
- clean-shutdown marker count: `0`;
- matched V2 candidate positions: `2`;
- Gate 3 quote budget: PASS;
- Gate 1, 2, 4 i 5: FAIL;
- root `promotion_gate_passed`: `false`.

Jest to prawidłowy artefakt kalibracyjny. Nie jest i nie może zostać użyty jako
waiver do PR B.

## 10. D8 — Acceptance

Faza prerequisite jest gotowa do validation burn-in, gdy:

- criteria JSON i tool są committed;
- golden PASS i negative gate tests przechodzą;
- `r1b` FAIL odtwarza się deterministycznie;
- runtime regressions oraz launcher guard tests przechodzą;
- `r2a` i `r2b` posiadają oddzielne run/launch identities;
- żaden cutover authority nie został wprowadzony.

Właściwy PR B może rozpocząć się dopiero po:

```text
r2a complete
AND r2b complete
AND exact multi-run manifest reconciliation
AND reports/het_pm_v2/het_pm_v2_promotion_gate_v1.json committed
AND promotion_gate_passed = true
```

Do tego czasu status pozostaje `NO AUTHORITY CHANGE`.
