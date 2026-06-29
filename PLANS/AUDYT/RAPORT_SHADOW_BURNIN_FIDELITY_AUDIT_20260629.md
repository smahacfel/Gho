# Raport Shadow Burnin Fidelity Audit 2026-06-29

## 1. Executive verdict

Finalny verdict enum: **SHADOW_REPLAY_LIFECYCLE_MISMATCH**.

Shadow burnin / `shadow_exit_replay_v1` / `shadow_lifecycle` **nie jest obecnie wiarygodny jako jeden spojny, lifecycle-equivalent system badawczy**. Audyt potwierdzil istnienie i kodowy kontrakt zrodel entry/exit/path oraz zbudowal niezalezna rekonstrukcje replay/path, ale replay i lifecycle materialnie sie rozjezdzaja na close reason / close age / final PnL, mimo wysokiego exact join rate. To wymusza downgrade wszystkich wnioskow, ktore laczyly te artefakty jako jedna historie pozycji.

`shadow_exit_replay_v1` moze byc uzywany tylko komponentowo: jako offline path/label research pod jawnie ograniczonymi zalozeniami. Nie jest live-equivalent. Krytyczne elementy live-equivalence sa nieobecne albo nieudowodnione: landing latency, landing slot, failed tx/no-fill, entry/exit slippage, own trade impact, AMM fees i realne quote/fill divergence.

Nie wolno z tego materialu wyciagac wniosku: "to bylby live PnL". Dopuszczalny wniosek jest waszy: "to jest ograniczony shadow/path label pod zalozeniem, ze mark/path price jest wystarczajacym proxy dla izolowanego eksperymentu i ze lifecycle nie jest uzywany jako potwierdzenie tej samej historii".

## 2. Co shadow faktycznie mierzy

- syntetyczna entry price zapisana w shadow evidence, zwykle zrodzona z decision/shadow request path;
- post-entry sampled mark/path PnL w `path_bps`;
- first-hit exact-level state w `first_hit_ms`;
- MFE/MAE/last PnL z obserwowanych probek;
- lifecycle close/reason/PnL w cieniu, bez dowodu live landing.

Zakresy:
- R48/R2: `shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2`; status=COMPLETED_OR_HISTORICAL; dirs=2; artifacts=3
- R49: `shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1`; status=COMPLETED_OR_HISTORICAL; dirs=2; artifacts=3
- R50: `shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1`; status=COMPLETED_OR_HISTORICAL; dirs=3; artifacts=9
- R51: `shadow-burnin-v3-r51-rce-logging-only-target12-stop6-maxwait45000-r1`; status=ACTIVE_PARTIAL; dirs=3; artifacts=12

## 3. Czego shadow nie mierzy

- live submit-to-land latency;
- rzeczywisty landing slot i intra-slot ordering;
- nieudane transakcje i no-fill;
- slippage/fill divergence na wejsciu i wyjsciu;
- own buy/sell impact jako oddzielny, sprawdzalny komponent;
- priorytet fee, Jito tip/bundle result, blockhash validity, compute/program failure;
- reorg/fork/commitment divergence.

Szczegoly sa w `reports/selector/shadow_fidelity_live_equivalence_gap.csv`.

## 4. Entry price contract

Zrodlo entry price jest znane czesciowo: runtime tworzy shadow entry jako syntetyczna cene shadow/simulation, a replay przenosi `entry_price`. Niezalezna rekonstrukcja z reserve/state evidence jest tylko czesciowa.

Statusy rekonstrukcji entry:

```json
{
  "ENTRY_RECONSTRUCTION_BLOCKED": 9114,
  "RECONSTRUCTED_DECISION_MFS_MARK_ONLY": 6213
}
```

Wniosek: entry price nie jest potwierdzonym live fill. Dla czesci scope'ow dokladny reserve/state snapshot potrzebny do rekonstrukcji jest `BLOCKED_BY_MISSING_EVIDENCE` albo jest tylko decision-MFS mark, nie entry-fill proof.

## 5. Exit price contract

`shadow_exit_replay_v1` zapisuje `levels_bps`, `first_hit_ms`, `path_bps`, `mfe_bps`, `mae_bps`, `last_pnl_bps`, `horizon_ms`, `close_age_ms`, `quality` i `truncated`. Audyt rekonstruuje target/stop/timeout z `first_hit_ms` oraz z `path_bps`.

Jakosc exit reconstruction:

```json
{
  "OK": 15327
}
```

Wniosek: exit result jest w znacznym stopniu rekonstruowalny dla offline path research, ale exact-level i compressed/sampled path moga sie rozejsc. `first_hit_ms` jest silniejszym exact-level dowodem niz `path_bps`; `path_bps` jest ograniczonym zapisem sciezki, nie pelnym tick streamem.

## 6. Pool state acquisition contract

Kodowa sciezka pool state obejmuje SnapshotEngine, AccountStateCore/feature materialization i `MaterializedFeatureSet`. Audyt nie znalazl runtime changes. Artefaktowo state timing pozostaje czesciowo zablokowany tam, gdzie brakuje kompletnego state timestamp/slot/raw account state.

Wniosek: pool-state timing jest **czesciowo potwierdzony kodowo**, ale nie globalnie udowodniony artefaktowo dla kazdego historycznego rekordu.

## 7. Temporal/no-lookahead integrity

Pola decyzyjne z MFS sa klasyfikowane jako PRE/AT_DECISION. Pola `path_bps`, `first_hit_ms`, `mfe_bps`, `mae_bps`, lifecycle final PnL i close reason sa OUTCOME. Sa bezpieczne jako label, ale nie jako selection feature.

Hard rule: gdyby ktorykolwiek OUTCOME/UNKNOWN field byl uzyty jako feature selekcyjny, verdict nalezy zdegradowac do `SHADOW_TEMPORAL_LEAKAGE_RISK`. Ten audyt nie zmienial selector runtime i nie potwierdza, ze wszystkie stare notatniki/raporty poprawnie separowaly feature vs label.

## 8. Replay/lifecycle reconciliation

Agregat reconciliation:

```json
{"ambiguous_join_count": 0, "close_age_match_rate": 0.11485251892456277, "close_reason_match_rate": 0.12868702688593056, "duplicate_terminal_count": 15253, "exact_join_rate": 0.9998042669798395, "fallback_join_rate": 0.0, "final_pnl_match_rate": 0.00026102845210127906, "max_pnl_diff_bps": 17177.48285714286, "median_pnl_diff_bps": 101.4614285714286, "missing_lifecycle_count": 3, "missing_replay_count": 3821, "p95_pnl_diff_bps": 735.0514285714286}
```

Join fallback nie jest akceptowany po cichu: audit przyjmuje exact key `(run_id, session_id, pool_id, base_mint, entry_ts_ms)` i raportuje brak/duplikaty jako ryzyko. Duplikaty terminalne typu `exit_filled` + `position_closed` sa raportowane, bo moga byc benign jako dwa typy zdarzen albo damaging, jesli downstream liczy je jako dwa zamkniecia.

## 9. Path sampling density

Verdicty density:

- 2s: `{'SPARSE_APPROX_ONLY': 15265, 'NOT_EVALUABLE_NO_COVERAGE': 57, 'EVALUABLE_APPROX': 5}`
- 3s: `{'SPARSE_APPROX_ONLY': 15265, 'NOT_EVALUABLE_NO_COVERAGE': 57, 'EVALUABLE_APPROX': 5}`
- 120s: `{'NOT_EVALUABLE_NO_COVERAGE': 15296, 'EVALUABLE_APPROX': 31}`
- 300s: `{'NOT_EVALUABLE_HORIZON_EXCEEDS_REPLAY': 15327}`
- 500s: `{'NOT_EVALUABLE_HORIZON_EXCEEDS_REPLAY': 15327}`

Nie wolno inferowac 300s/500s, jezeli replay horizon i realna sciezka tego nie pokrywaja. `path_bps` moze wspierac krotkie horyzonty tylko per-row, tam gdzie coverage nie jest `NOT_EVALUABLE_*`.

## 10. Live-equivalence gap

Shadow nie modeluje krytycznych komponentow live-equivalence: latency, landing/failure, slippage, own impact, fees, blockhash/Jito/fee policy i contention. Dlatego shadow moze byc uzyty do porownan offline pod jawnie wymienionymi zalozeniami, ale nie do claimu live-equivalent.

## 11. Fixture proof summary

Fixture CSV: `reports/selector/shadow_fidelity_fixture_results.csv`.

Fixture tests obejmuja 25 przypadkow: target-before-stop, stop-before-target, tie same timestamp/slot, sparse timeout, missing path before max_hold, malformed first_hit/path, non-monotonic/duplicate timestamps, MFE/MAE, reserve price, stale/future state, absent own-impact/slippage, duplicate terminal rows, ambiguous fallback joins i replay/lifecycle disagreement.

## 12. Claim evidence matrix summary

Claim statusy:

```json
{
  "DISPROVEN": 8,
  "NOT_PROVEN": 1,
  "PARTIALLY_PROVEN": 10,
  "PROVEN": 3
}
```

Pelna macierz: `reports/selector/shadow_fidelity_claim_evidence_matrix.csv`.

## 13. Ktore poprzednie wnioski research zostaja

Pozostaja tylko wnioski, ktore byly sformulowane jako izolowany offline `shadow_exit_replay_v1` / path-label research i nie wymagaly lifecycle equivalence, live fill, live latency, failed tx/no-fill, entry/exit slippage ani 300s/500s coverage bez realnego horizon coverage.

## 14. Ktore poprzednie wnioski trzeba zdegradowac

Do downgrade label ida wszystkie stare wnioski, ktore:

- nazywaly shadow PnL live-equivalent;
- traktowaly lifecycle i replay jako jedna zgodna historie pozycji;
- traktowaly entry price jako rzeczywisty fill;
- traktowaly exit mark/path jako wykonalny sell fill;
- inferowaly 300s/500s bez coverage;
- uzywaly OUTCOME fields jako selection features;
- ignorowaly missing failed tx/no-fill/latency/slippage/own-impact.

## 15. Co trzeba zinstrumentowac przed dalszym research

Minimum:

- entry quote, min_out, reserve-before/reserve-after, explicit decimals;
- submit timestamp, landed slot/time albo failed/no-fill status;
- exit quote/min_out/sell impact/fees;
- sample slot/timestamp/commitment for every path point;
- exact tie-break metadata for same-slot target/stop;
- lifecycle/replay exact join id and terminal-event cardinality;
- raw pool state provenance for entry and exit.

## 16. Final decision

- usable for offline research: **tylko komponentowo dla `shadow_exit_replay_v1`/path labels; nie jako spojny lifecycle/replay dataset**;
- usable for live-equivalent claims: **nie**;
- usable for RCE: **nie jako runtime approval/live-equivalent proof; tylko jako logging-surface evidence**;
- usable for runtime approval: **nie**.

## Artefakty

- inventory: `reports/selector/shadow_fidelity_inventory.csv`
- entry reconstruction: `reports/selector/shadow_fidelity_entry_price_reconstruction.csv`
- exit reconstruction: `reports/selector/shadow_fidelity_exit_price_reconstruction.csv`
- pool state provenance: `reports/selector/shadow_fidelity_pool_state_provenance.csv`
- temporal integrity: `reports/selector/shadow_fidelity_temporal_integrity.csv`
- replay/lifecycle reconciliation: `reports/selector/shadow_fidelity_replay_lifecycle_reconciliation.csv`
- live gap: `reports/selector/shadow_fidelity_live_equivalence_gap.csv`
- path density: `reports/selector/shadow_fidelity_path_sampling_density.csv`
- fixtures: `reports/selector/shadow_fidelity_fixture_results.csv`
- claims: `reports/selector/shadow_fidelity_claim_evidence_matrix.csv`
- golden traces: `reports/selector/shadow_fidelity_golden_traces` (20 files)
