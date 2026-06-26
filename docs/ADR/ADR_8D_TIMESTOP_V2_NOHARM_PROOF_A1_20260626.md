# ADR-8D: TimeStop V2 No-Harm Proof A1

Status: IMPLEMENTED / OFFLINE_PROOF_GENERATED / RUNTIME_GATE_CLOSED
Typ: ADR-8D / offline research proof / exit-side action precision
Data: 2026-06-26
Autor/Agent: Codex
Repo/branch: `/root/Gho`, local working tree
Commit/PR: not committed at ADR creation time
Zakres: PR-TSV2-A1 TimeStop V2 independent no-harm / action-precision proof
Poziom ryzyka: LOW runtime risk / MEDIUM analytical risk

Dotkniete moduly/pliki:
- `scripts/time_stop_v2_counterfactual_lab.py`
- `scripts/test_time_stop_v2_counterfactual_lab.py`
- `reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/time_stop_v2_noharm_summary_v1.csv`
- `reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/time_stop_v2_noharm_cost_sensitivity_v1.csv`
- `reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/time_stop_v2_noharm_stability_v1.csv`
- `reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/time_stop_v2_noharm_grid_neighborhood_v1.csv`
- `reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/TIME_STOP_V2_NOHARM_PROOF_A1.md`
- `docs/ADR/ADR_8D_TIMESTOP_V2_NOHARM_PROOF_A1_20260626.md`

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzywany w repo.

## 1. Przygotowanie i dzialania wstepne

Cel:
Sprawdzic offline, czy TimeStop V2 jako exit-side dead-flow / loss reducer ma stabilna przewage po kosztach oraz czy `exit_action_precision` realnie zbliza sie do 65-70%.

Twarde ograniczenia:
- offline-only,
- bez zmian Gatekeeper runtime,
- bez zmian BUY/REJECT,
- bez zmian `v25_confidence`,
- bez V3/runtime selector changes,
- bez `alpha_31100`,
- bez TX builder / sender / Jito / live execution changes,
- bez mutowania istniejacych logow,
- bez nowego runtime sidecara.

Definicja precision uzyta w A1:
`exit_action_precision = beneficial_exit / (beneficial_exit + harmful_exit)`.

Nie mieszano tego z:
- `entry_target_precision`,
- `entry_nonloss_precision`.

## 2. Wykorzystane skills/specjalizacja

Routing:
- primary specialist: Decision Logging Replay Analyst,
- supporting: large-data analytics, statistical research, Ghost execution boundary.

Skills:
- `ghost-execution`,
- `large-data-analytics`,
- `statistical-research-engine`.

## 3. Opis problemu - 3W2H

What:
ORG-A0 zostal zamkniety jako negatywna evidence dla runtime. Nastepny etap sprawdza nie wejscia organiczne, tylko exit-side no-harm/action precision TimeStop V2.

Where:
- `scripts/time_stop_v2_counterfactual_lab.py`,
- R49/R48 final scope: `shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1`,
- R48/R2 jako negative no-window coverage control.

Why it matters:
TimeStop V2 moze byc wartosciowy tylko wtedy, gdy aktywne zamkniecia czesciej pomagaja niz szkodza, po kosztach, stabilnie chronologicznie, bez stale/no-action leakage i bez nadmiernego ciecia przyszlych Targetow.

How observed:
Rozszerzono istniejacy offline lab o:
- full grid target/stop/max_hold,
- cost sensitivity 0/50/100/150/200 bps,
- action accounting,
- Wilson lower bound 95%,
- stale/no-action exclusions,
- exact/path replay accounting,
- resurrection checks 4000/8000/12000 ms,
- chronological terciles,
- grid-neighborhood stability.

How many:
- R49 positions: 3594,
- R49 positions with exit replay: 3079,
- R49 positions with TSV2 windows: 3594,
- R49 candidate positions: 3517,
- R48/R2 control positions with TSV2 windows: 0.

## 4. Przyczyna zrodlowa

Root cause analityczny:
Wczesniejszy pojedynczy policy snapshot `6000/-6000/120000` wygladal obiecujaco, ale nie rozdzielal wystarczajaco action precision, target-cut damage i stabilnosci grid-neighborhood. A1 pokazuje, ze najlepszy wariant finalnego R49 ma dodatni delta i action precision >70%, ale nie przechodzi target-cut guard.

## 5. Strategia naprawy

Nie zmieniano runtime.

Przyjeta strategia:
- rozszerzyc istniejacy offline lab,
- zachowac stare outputy counterfactual,
- dodac nowe `time_stop_v2_noharm_*` artefakty,
- traktowac R48/R2 tylko jako no-window control,
- wygenerowac osobny raport A1 i ADR.

## 6. Przeprowadzone akcje

Zmiana 1:
`scripts/time_stop_v2_counterfactual_lab.py` dostal:
- streaming parser lifecycle JSON objects,
- cache `path_bps` i baseline per grid cell,
- action precision accounting,
- cost sensitivity CSV,
- chronological stability CSV,
- grid-neighborhood CSV,
- no-harm markdown report,
- verdict enum mapping.

Zmiana 2:
`scripts/test_time_stop_v2_counterfactual_lab.py` dostal test, ze `exit_action_precision` liczy tylko beneficial/harmful actions i nie miesza no-action rows.

Zmiana 3:
Wygenerowano A1 outputs pod R49 scope.

## 7. Wynik operacyjny

Final verdict:
`INCONCLUSIVE_RESEARCH / REJECTED_FOR_RUNTIME`

No basis for runtime change.
No basis for shadow_close_only plan.
Positive action precision is blocked by target-cut guard.

Najlepszy wariant:
- target_bps: `2000`,
- stop_bps: `-6000`,
- max_hold_ms: `120000`.

Best cost100:
- supported_rows: `3079`,
- action_taken_count: `2914`,
- delta_sum_bps: `448725`,
- delta_avg_bps: `145.737`,
- delta_median_bps: `0`,
- exit_action_precision: `0.702415`,
- Wilson lower 95%: `0.678009`,
- beneficial_exit_count: `989`,
- harmful_exit_count: `419`,
- saved_stop_count: `59`,
- saved_stop_damage_bps: `246158`,
- timeout_improved_count: `930`,
- timeout_improved_bps: `795329`,
- target_cut_count: `226`,
- target_cut_damage_bps: `461016`.

Cost sensitivity:
Delta jest dodatni przy 0/50/100/150/200 bps. Poniewaz koszt roundtrip jest symetrycznie odejmowany od baseline i TSV2, paired delta nie zmienia sie miedzy kosztami.

Chronological stability:
- train delta_sum_bps: `165837`, action_precision: `0.672584`,
- validation delta_sum_bps: `187164`, action_precision: `0.727483`,
- holdout delta_sum_bps: `95724`, action_precision: `0.711538`.

Grid-neighborhood:
Sasiednie warianty wokol best cell sa dodatnie, wiec wynik nie jest pojedynczym ostrzem gridu.

Runtime blockers:
- `target_cut_damage_bps > 25% gross_saved_damage_bps`,
- `target_cut_count > saved_stop_count + 10% timeout_improved_count`.

Shadow-close-only blockers:
- wymagane sa minimum dwa niezalezne scope z TSV2 windows,
- obecnie jest tylko jeden pelny scope R49; R48/R2 jest tylko no-window control.

## 8. Walidacja

Walidacje wykonane:
- `python3 -m py_compile scripts/time_stop_v2_counterfactual_lab.py`
- `python3 scripts/test_time_stop_v2_counterfactual_lab.py`
- `python3 scripts/time_stop_v2_counterfactual_lab.py --help`
- full R49 A1 run z gridem `targets=1000,1500,2000,3000,5000,6000,7500,10000`, `stops=-200,-300,-500,-700,-1000,-1500,-2000,-3000,-5000,-6000`, `max_hold=30000,60000,120000`, costs `0,50,100,150,200`, resurrection `4000,8000,12000`.

## 9. Ryzyka resztkowe

- To nadal offline replay/counterfactual, nie runtime proof.
- Jeden pelny R49 scope nie wystarcza do `ELIGIBLE_FOR_SHADOW_CLOSE_ONLY_PLAN`.
- Positive delta istnieje, ale target-cut damage jest zbyt wysoki wzgledem gross saved damage.
- R48/R2 nie ma TSV2 windows, wiec nie jest niezaleznym pozytywnym potwierdzeniem.

## 10. Scope out

Nie wykonano:
- runtime change,
- Gatekeeper BUY/REJECT change,
- selector runtime change,
- `shadow_close_only`,
- `alpha_31100`,
- XGBoost,
- TX builder / sender / Jito / live execution change.

## 11. Decyzja

PR-TSV2-A1 wynik:
`INCONCLUSIVE_RESEARCH / REJECTED_FOR_RUNTIME`.

No basis for runtime change.
No basis for shadow_close_only plan.
Positive action precision is blocked by target-cut guard.

Nie ma podstaw do runtime ani `shadow_close_only` na bazie A1. Zachowac artefakty jako offline evidence i blocker list.
