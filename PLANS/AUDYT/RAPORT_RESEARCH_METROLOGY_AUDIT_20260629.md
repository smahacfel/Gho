# P0 Research Metrology Audit - 2026-06-29

## Status

Final verdict: **METROLOGY_PASS_WITH_WARNINGS**

Decision: Poprzednie negatywne wyniki pozostaja wazne w audytowanych horyzontach i przy jawnych ograniczeniach pomiaru.

Runtime approval: **false**
Shadow close approval: **false**
Active close approval: **false**

## Zakres

Audyt jest offline-only. Nie uruchamia nowych runow, nie zmienia runtime, nie
zmienia BUY/REJECT, Gatekeepera, selectora ani TX/Jito/live path. Skrypt czyta
lokalne JSONL evidence i istniejace CSV/MD raporty, ale outputs to tylko
kompaktowe raporty CSV/MD.

Audytowane scopes raw replay/lifecycle:

- `shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2`
- `shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1`
- `shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1`
- `shadow-burnin-v3-r51-rce-logging-only-target12-stop6-maxwait45000-r1`

## Wynik po wymiarach

- Simulator fixtures: sprawdzone w `reports/selector/research_metrology_audit_simulation_fixtures.csv`.
- Tie-break sensitivity: zapis w `research_metrology_audit_metric_consistency.csv`.
- Replay/lifecycle reconciliation: `research_metrology_audit_replay_lifecycle_reconciliation.csv`.
- Metric consistency: `research_metrology_audit_metric_consistency.csv`.
- Config sensitivity: `research_metrology_audit_config_sensitivity.csv`.
- Horizon sensitivity: `research_metrology_audit_horizon_sensitivity.csv`.
- Missing metrics inventory: `research_metrology_audit_summary.csv`.

## Najwazniejsze ostrzezenia

- replay_lifecycle_warn=shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- long_horizon_not_evaluable=shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- tie_break_sign_flip_scope=shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1
- replay_lifecycle_warn=shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1
- long_horizon_not_evaluable=shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1
- replay_lifecycle_warn=shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1
- long_horizon_not_evaluable=shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1
- replay_lifecycle_active_partial=shadow-burnin-v3-r51-rce-logging-only-target12-stop6-maxwait45000-r1
- long_horizon_not_evaluable=shadow-burnin-v3-r51-rce-logging-only-target12-stop6-maxwait45000-r1
- long_horizon_surface_insufficient_300000_500000

## Failure flags

- Brak twardych failure flags.

## Interpretacja

Poprzednie negatywne wyniki pozostaja wazne w audytowanych horyzontach i przy jawnych ograniczeniach pomiaru.

Nie wolno inferowac wnioskow dla 300000/500000 ms, jezeli coverage jest NOT_EVALUABLE.

R51, jezeli jest aktywny, jest traktowany jako `ACTIVE_PARTIAL`; jego brak
post-run manifestu nie jest uzywany jako negatywny wynik strategii.

## Pliki wynikowe

- `reports/selector/research_metrology_audit_summary.csv`
- `reports/selector/research_metrology_audit_simulation_fixtures.csv`
- `reports/selector/research_metrology_audit_config_sensitivity.csv`
- `reports/selector/research_metrology_audit_horizon_sensitivity.csv`
- `reports/selector/research_metrology_audit_metric_consistency.csv`
- `reports/selector/research_metrology_audit_replay_lifecycle_reconciliation.csv`
