# Historical feasibility po PR2C

Status:

```text
FEASIBILITY_ONLY
HISTORICAL_V33_REPLAY_V2_NOT_EVALUABLE
NOT_PROSPECTIVE_VALIDATION_EVIDENCE
NO_POST_HOC_GATE_CHANGE
```

Data: 2026-07-13

Base danych: zamrożony PR0 inventory z
`reports/metric_contracts/historical_feasibility_preflight_v1.md` oraz jego
content-addressed input manifest.

## 1. Cel

Po wdrożeniu pełnych producentów PR2B, durable transportu i replayu PR2C
ponowiono pytanie, czy historyczne v33 mogą zostać przepuszczone przez nowy
audit jako full replay evidence. Nie przeliczano nowych rodzin z legacy
scalarów i nie włączono historycznych rows do prospective minima.

## 2. Kontrolowany audit

Nowy `metric-contract-audit single-run` wymaga dla bieżącego rekordu:

- rotation manifestu z part SHA;
- compact v34 summary;
- `metric_contract_evidence_v1.jsonl`;
- pełnej record identity i evidence SHA;
- decision-time MFS Wire V1 projection;
- exact effective-config payload/hash.

Stabilne historyczne runy PR0 mają wyłącznie v33. Nie mają żadnego z nowych
paired artifacts ani `metric_contract_effective_config_hash`. Próba wejścia do
audytu kończy się przed czytaniem wielogigabajtowego v33 komunikatem o braku
`metric_contract_rotation_manifest_v1.json`; jest to oczekiwany fail-closed
wynik `FAIL_SCHEMA_OR_REPLAY/NOT_EVALUABLE`, a nie błąd feasibility skanera.

Wykonana komenda release CLI:

```text
target/release/metric_contract_audit single-run \
  --run-dir /root/Gho_dynamic_exit_v1/logs/rollout/shadow-v2-l2-human-ab-thresholds-15s-20260708-r1-4h \
  --decision-v33 /root/Gho_dynamic_exit_v1/logs/rollout/shadow-v2-l2-human-ab-thresholds-15s-20260708-r1-4h/decisions/shadow-v2-l2-human-ab-thresholds-15s-20260708-r1-4h/v2.2/legacy_live/d0480c9b7b3c26e42918c60833d34c016d2b0188182754ba9a64f52086d80c22/gatekeeper_v2_decisions.jsonl

exit: 3
stderr:
read .../metric_contract_rotation_manifest_v1.json:
No such file or directory (os error 2)
```

Surowe historyczne runy wskazane przez frozen PR0 input manifest nie są już
dostępne w lokalnym filesystemie. Nie przedstawiono więc nowego content scan
ani ponownego SHA tych wielogigabajtowych plików. Liczby poniżej pochodzą
wyłącznie z wcześniej zamrożonych, content-addressed
`pr0_input_manifest_v1.json` i `pr0_feasibility_summary_v1.json`.

## 3. Co historyczne dane nadal dowodzą

Z frozen PR0 summary:

| Wymiar | Wynik feasibility |
| --- | ---: |
| Stabilne niepokrywające się runy | 4 |
| Aggregate observed duration | 36.2505 h |
| Unique v33 decisions | 31,266 |
| Dev-known | 28,489 |
| Legacy flip scalar present | 28,443 |
| v33 size p95 / p99 | 119,890 / 137,102 B |
| Historical storage rate | 0.0888 GB/h |

To potwierdza skalę minimów duration/decisions/dev-known i stanowi frozen
denominator dla limitów v34/combined storage. Nie dowodzi Flip V2 evaluability,
dev-primary divergence, stable-event collision cleanliness, full evidence hash,
projection/full equality ani policy equivalence replay.

## 4. Dlaczego nie wykonano pseudo-replayu

Historyczne v33 nie zawiera raw ordered eligible event feedu wymaganego przez
Flip V2, producer-config fingerprints, pełnych typed family contracts ani
decision-time effective-config. Odtworzenie tych danych z legacy projection,
policy lub domyślnych scalarów naruszyłoby jednokierunkowy kontrakt i mogłoby
zamienić brak dowodu na measured zero.

Dlatego:

```text
historical duration/decision/dev-known scale = FEASIBLE
historical PR2C full replay                 = NOT_EVALUABLE
historical validation count contribution   = 0
```

## 5. Freeze i zasada anty-post-hoc

`BURN_IN_CONTRACT_V3.json` zachowuje początkowe minima planu: 3 runy, 1 h/run,
2 UTC buckets, 8 h aggregate, 700 decisions, 100 dev-known, 100 clean Flip V2
evaluable i 30 real dev divergences. Historyczny wynik nie obniżył żadnego
progu. V2 wersjonował autoryzowaną przed-runową zmianę resource limitu z 1 ms
do 5 ms. V3, również przed pierwszym prospective row, zamraża finite histogram
codebook kończący się bucketem `5_000 us`, aby p99 nie było utożsamiane z
overflow `max_us`. V1 i V2 są superseded pre-run artifacts i nie identyfikują
runu V3. Prospective row musi powstać po nowym V3 `frozen_at` i przejść pełny
per-run audit przed bundle aggregation.

## 6. Werdykt

```text
HISTORICAL_FEASIBILITY_SCALE_CONFIRMED
HISTORICAL_V33_REPLAY_V2_NOT_EVALUABLE
HISTORICAL_ROWS_EXCLUDED_FROM_PROSPECTIVE_COUNTS
BURN_IN_THRESHOLDS_NOT_LOWERED_POST_HOC
```
