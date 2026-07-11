# Raport audytowy: najdokładniejszy kontrakt interpretacji 10 metryk Ghost

Status: AUDIT_COMPLETE / REPORT_CORRECTED / NO_RUNTIME_CHANGE
Typ: audyt semantyki, źródeł, denominatorów, fallbacków i konsumentów metryk
Data: 2026-07-10
Repo: /root/Gho_dynamic_exit_v1
HEAD podczas audytu: f3318f3
Zakres: dokładnie 10 metryk z listy korekty interpretacji przekazanej do audytu
Poziom ryzyka: MEDIUM analytical/replay risk / LOW runtime risk dla tej zmiany dokumentacyjnej

## 0. Proweniencja i granice dowodu

Źródłowy wcześniejszy raport RAPORT_AUDYT_WIARYGODNOSCI_METRYK_GHOST_20260630.md
nie jest obecny w bieżącym checkoutcie. Zakres wejściowy tego audytu stanowi więc
dokładnie tabela 10 metryk przekazana w zleceniu. Wnioski wcześniejszego audytu
nie zostały przyjęte na wiarę: każda z 10 pozycji została ponownie prześledzona
w aktualnym kodzie, konfiguracji bazowej i istniejących testach.

Raport potwierdza implementację i kontrakty statyczne w HEAD f3318f3. Nie jest
dowodem, że konkretny live/shadow proces uruchomiony z nieznanym profilem
konfiguracyjnym wyemitował każdą metrykę. Do takiego twierdzenia potrzebny jest
osobny runtime proof: PID/proces, rzeczywisty config, rosnący artefakt i przykładowe
rekordy.

W ramach tej pracy nie zmieniono:

- MaterializedFeatureSet ani materializacji,
- Gatekeeper BUY/REJECT/TIMEOUT,
- progów lub configów,
- DecisionLogger schema,
- selector score,
- shadow/live behavior,
- kodu Rust lub Python.

## 1. Werdykt wykonawczy

### 1.1 Ocena listy wejściowej

Wszystkie 10 ostrzeżeń z listy wejściowej jest merytorycznie zasadne. Nie wszystkie
oznaczają jednak ten sam rodzaj defektu:

- część to aktywny rozjazd nazwy i formuły,
- część to przeciążenie nazwy między różnymi powierzchniami,
- część to brak jawnego statusu jakości lub denominatora,
- top3_volume_pct jest już naprawione w canonical read path, a ryzyko pozostało
  głównie w legacy nazwie i downstream,
- pola ManipulationContradictionFeatures.high_* są błędne jako samodzielne
  fakty, ale nie powodują obecnie pełnego bypassu V3, ponieważ V3 równolegle
  sprawdza odpowiadające im wartości numeryczne,
- evidence_status.fsc ma niejednoznaczny kontrakt wersji: jest spójny z samą
  obecnością legacy scalar, lecz nie dowodzi jakości FSC v2.

### 1.2 Ocena poprzedniej wersji tego raportu

Poprzednia wersja raportu miała dobrą strukturę i większość prawidłowych callsite'ów,
ale wymagała korekty przed użyciem jako SSOT planu. Najważniejsze błędy i braki:

1. Zawierała jedenasty finding nln_artifact_capture_unavailable_total, którego nie
   było w przekazanej liście 10 metryk. Był to scope drift i został usunięty.
2. Dla FTDI stwierdzała zbyt szeroko, że zbyt mała próbka zawsze daje None.
   Przy dwóch unikalnych buyerach kod emituje wartość Some(...) z degraded reason;
   aktywna polityka uznaje ją jednak za non-actionable.
3. Nazywała dev_volume_ratio total exposure. To nieprawda: pole jest udziałem
   brutto obrotu deva, czyli (buy volume + sell volume) / total volume. Nie jest
   ekspozycją netto, stanem posiadania ani sumą dev buy.
4. Nie rozdzielała wystarczająco canonical MFS path od GatekeeperBuffer compat
   dla dev primary buy i same_ms_tx_ratio.
5. Opisywała FSC v2 zbyt ogólnie jako export-only. Per-pool FSC v2 jest
   addytywnym evidence i counterfactual shadow policy input; osobny globalny
   funding coverage gate może chronić live execution, ale nie używa per-pool
   concentration jako aktywnego sygnału BUY/REJECT.
6. Nadawała polom high_* zbyt szeroki skutek. Ich serializowana wartość jest
   niewiarygodna, lecz obecna V3 shadow policy ma numeryczne kontrole zastępcze.
7. Nie rozdzielała defaultu typu V3 od bieżącego bazowego TOML:
   GatekeeperV3EvidenceRequirements domyślnie ma fsc=true, ale bazowy
   ghost_brain_config.toml jawnie ustawia fsc=false. Wpływ na V3 shadow gating
   jest więc profile-dependent; wpływ na replay/reporting pozostaje bezwarunkowy.

Werdykt jakości poprzedniej wersji: REVISE_REQUIRED.

Werdykt jakości niniejszej wersji: FIT_AS_PLAN_INPUT, z jawnymi decyzjami
architektonicznymi pozostawionymi do planu wykonawczego.

## 2. Syntetyczna macierz 10 metryk

| Metryka | Werdykt po weryfikacji | Canonical/current surface | Najważniejsza korekta |
| --- | --- | --- | --- |
| fee_topology_diversity_index | CONFIRMED_WITH_SAMPLE_NUANCE | active MFS Sybil + osobny export-only coordination sidecar | Runtime używa unique topologies / unique buyers; wartość zależy od n i może istnieć jako degraded diagnostic. |
| dev_buy_total_sol | CONFIRMED_WITH_SOURCE_DIVERGENCE | log alias z MFS tx_intel_features.dev_buy_sol | To first observed dev buy, nie total; dev_volume_ratio to gross turnover share, nie exposure. |
| same_ms_tx_ratio | CONFIRMED_WITH_ACTIVE_PATH_REFINEMENT | active MFS exact collision; compat helper używa <50 ms | Nazwa, populacja i consumer path muszą być jawne. |
| top3_volume_pct | CONFIRMED_ALREADY_REPAIRED_CANONICALLY | preferred top3_signer_volume_ratio + legacy alias | Skala 0..1; canonical helper jest już poprawiony, pozostała migracja downstream. |
| flip_ratio_10s | CONFIRMED_AS_HYBRID | active early fingerprint with default 10 s collection and 20-slot flip condition | Nie jest ani wyłącznie 10 s, ani wyłącznie slot-based; kontrakt łączy oba ograniczenia. |
| funding_source_concentration | CONFIRMED_HIGH_SEMANTIC_RISK | active legacy MFS scalar; FSC v2 osobno | Legacy to 1 - distinct / known sample count, nie HHI i nie volume concentration. |
| evidence_status.fsc | CONFIRMED_VERSION_AMBIGUITY | V3 evidence plane, shadow/replay | Clean oznacza legacy scalar present; nie oznacza FSC v2 clean/readiness. |
| ManipulationContradictionFeatures.high_* | CONFIRMED_EVIDENCE_DEFECT | V3 shadow/replay | Materializer zostawia false; numeric V3 checks ograniczają verdict bypass, ale flagi same w sobie są fałszywe. |
| reserve_velocity_sol_per_sec | CONFIRMED_MISSING_VALIDITY_STATE | MFS account evidence, V3 serialization | To interval-average między update'ami; zero może oznaczać pomiar, pierwszy update, fallback lub zero delta time. |
| buy_sell_ratio_recent | CONFIRMED_LOGGING_ONLY_UNBOUNDED | RCE logging-only MFS evidence | To buy_count / sell_count, a przy zero sells zwraca buy_count; nie jest bounded ratio. |

## 3. Routing audytu

task_classification: cross-cutting metric-semantics-and-evidence-audit
primary_specialist: Ghost Runtime Coordinator
supporting_specialists:
- SSOT Feature Materialization Guardian
- Gatekeeper Policy Auditor
- Decision Logging Replay Analyst
skills_used:
- ghost-execution
- statistical-research-engine
- large-data-analytics
- trading-systems
- abstract-reasoning
references_loaded:
- .agents/skills/ghost-execution/SKILL.md
- .agents/skills/ghost-execution/references.md
- .agents/skills/statistical-research-engine/SKILL.md
- .agents/skills/statistical-research-engine/references.md
- .agents/skills/large-data-analytics/SKILL.md
- .agents/skills/trading-systems/SKILL.md
- .agents/skills/abstract-reasoning/SKILL.md
- docs/agents/ghost-runtime-coordinator.md
- docs/agents/ssot-feature-materialization-guardian.md
- docs/agents/gatekeeper-policy-auditor.md
- docs/agents/decision-logging-replay-analyst.md
runtime_area_touched_by_analysis:
- TxIntelligence
- PoolObservationSession materialization
- SybilResistanceFeatures and FSC evidence
- Gatekeeper V2 feature-driven policy
- Gatekeeper V3 shadow/replay evidence
- Seer EarlyFingerprintAggregator
- AccountStateCore
- RCE logging-only session regime evidence
contracts_at_risk:
- MaterializedFeatureSet SSOT
- one semantic owner per metric
- metric unit, range, population and denominator
- degraded/fallback truthfulness
- legacy versus v2 evidence separation
- active versus compat/export/shadow/logging-only separation
- additive JSONL compatibility
active_or_legacy_path: mixed active MFS, compat buffer, V3 shadow, export-only sidecar and logging-only RCE
risk_level: medium

Nie ładowano specjalistów Solana Execution Path ani Oracle Session Runtime jako
głównych ról, ponieważ raport nie zmienia transakcji, submit/confirmation,
harmonogramu sesji ani routingu eventów. Seer Ingest i Config Rollout zostały
sprawdzone na konkretnych callsite'ach bez pełnego rozszerzania audytu na parsery
i rollout, ponieważ nie zmieniamy ich kontraktów w tej iteracji.

## 4. Findings claim-by-claim

### 4.1 fee_topology_diversity_index

Werdykt: problem interpretacyjny potwierdzony; poprzedni raport wymagał korekty
zachowania na małej próbce.

#### Canonical aktywny kontrakt

Aktywna materializacja Sybil:

1. bierze tylko transakcje is_buy && success,
2. zachowuje pierwszą napotkaną próbkę dla każdego unikalnego signera,
3. wymaga surowego fee topology dla każdej wybranej próbki,
4. liczy:

   FTDI_runtime = liczba_unikalnych_topologii / liczba_unikalnych_buyerów

Dowód:

- ghost-launcher/src/tx_intelligence/sybil_metrics.rs:149-188
- ghost-launcher/src/tx_intelligence/sybil_metrics.rs:342-391
- ghost-launcher/src/tx_intelligence/sybil_metrics.rs:664-697
- ghost-launcher/src/session/observation.rs:2601-2614

Ważne własności, których nie wolno zgubić:

- dla n buyerów zakres wynosi od 1/n do 1, a nie od 0 do 1 w sensie
  osiągalnego minimum przy danym n,
- homogeneous topology dla trzech buyerów daje 1/3, nie 0,
- dwa unikalne buyery mogą dać Some(value), ale z reason
  FTDI_INSUFFICIENT_BUYS,
- aktywna Gatekeeper policy blokuje actionability takiej degraded wartości,
- mniej niż dwa unikalne buyery daje None,
- brak fee topology choćby dla jednej wybranej próbki daje None z
  FTDI_RAW_FEE_TOPOLOGY_UNAVAILABLE.

Dowód small-sample i policy:

- ghost-launcher/src/tx_intelligence/sybil_metrics.rs:21-25
- ghost-launcher/src/tx_intelligence/sybil_metrics.rs:899-913
- ghost-launcher/src/components/gatekeeper_policy.rs:3008-3078

#### Osobna powierzchnia coordination-risk

Coordination-risk FTDI:

- również deduplikuje do first buy per signer,
- wymaga minimalnego sample i fingerprint coverage,
- liczy normalized HHI po topology counts,
- eksportuje diversity = 1 - normalized_hhi,
- jest oznaczony policy_mode=ExportOnly i score_eligible=false.

Dowód:

- ghost-core/src/features/coordination/metrics.rs:81-153
- ghost-core/src/features/coordination/metrics.rs:844-927
- ghost-core/src/features/coordination/metrics.rs:1098-1137
- ghost-core/tests/coordination_metrics_phase06.rs:165-214

Runtime tworzy i loguje ten sidecar osobno od Gatekeeper decision record:

- ghost-launcher/src/oracle_runtime.rs:1178-1192

#### Najrzetelniejsza interpretacja

Nie istnieje jedna wymienna wartość FTDI. Należy przechowywać co najmniej:

- surface = mfs_sybil_unique_topology_ratio albo coordination_export_hhi_diversity,
- sample_n i signer_sample_count,
- degraded status/reasons,
- coverage dla coordination sidecar.

Nie rekomenduje się zmiany aktywnej formuły w ramach korekty nazwy. Ewentualne
przejście na HHI diversity byłoby zmianą sygnału i wymaga osobnej walidacji
statystycznej oraz policy ADR.

### 4.2 dev_buy_total_sol

Werdykt: nazwa total jest myląca, lecz poprzednie zalecenie total exposure przez
dev_volume_ratio było nieprawidłowe.

#### Canonical MFS path

W TxIntelligence:

- per-signer first_buy_volume_sol jest ustawiane przy pierwszym napotkanym BUY,
- dev_buy_total_sol jest odświeżane z first_buy_volume_sol,
- MFS emituje tę wartość jako tx_intel_features.dev_buy_sol,
- feature-driven Gatekeeper mapuje ją z powrotem do pola profilu/logu
  dev_buy_total_sol.

Dowód:

- ghost-launcher/src/tx_intelligence/engine.rs:29-39
- ghost-launcher/src/tx_intelligence/engine.rs:150-239
- ghost-launcher/src/tx_intelligence/engine.rs:653-673
- ghost-launcher/src/tx_intelligence/engine.rs:287-315
- ghost-launcher/src/components/gatekeeper_policy.rs:2911-2921
- ghost-brain/src/oracle/decision_logger.rs:1154-1173

Dokładna populacja jest ważna: TxIntelligence odrzuca duplikaty i transakcje
poniżej dust threshold, ale nie odrzuca transakcji tylko dlatego, że success=false.
Dlatego wartość oznacza pierwszą zaakceptowaną przez TxIntelligence obserwację BUY
rozpoznanego dev signera, nie bezwarunkowo pierwszy successful on-chain dev BUY.

Zero jest przeciążone:

- dev wallet nieznany,
- dev nieobecny w signer stats,
- brak zaobserwowanego buy deva,
- ewentualna prawdziwa wartość zero.

Companion fields dev_wallet_known i dev_tx_count są konieczne do interpretacji.

#### Rozjazd z GatekeeperBuffer compat

GatekeeperBuffer ma osobną, mocniejszą semantykę primary creator buy:

- preferuje creator BUY z create signature,
- w przeciwnym razie wybiera najwcześniejszy creator BUY według tx key,
- osobno agreguje całkowity buy i sell volume deva.

Dowód:

- ghost-launcher/src/components/gatekeeper.rs:4937-4986
- docs/ADR/ADR-0025-gatekeeper-dev-buy-primary-anchoring.md:20-28
- docs/ADR/ADR-0030-gatekeeper-dev-buy-observed-only.md:24-42

Production terminal path jest jednak feature-driven przez MFS:

- ghost-launcher/src/oracle_runtime.rs:16901-16977

Nie wolno zatem opisywać MFS/log field jako canonical create-signature primary buy,
dopóki ta sama reguła nie zostanie przeniesiona do właściciela MFS lub jawnie
zmaterializowana jako provenance.

#### Co naprawdę oznacza dev_volume_ratio

Kod liczy:

dev_volume_ratio =
  (dev_buy_volume_total_sol + dev_sell_total_sol) / total_volume

Dowód:

- ghost-launcher/src/tx_intelligence/analysis.rs:314-355

To udział brutto aktywności/obrotu deva w obserwowanym wolumenie. Nie jest to:

- total dev buy SOL,
- net exposure,
- aktualny stan posiadania,
- udział deva w supply,
- zrealizowany lub niezrealizowany PnL.

#### Najrzetelniejsza interpretacja i kierunek naprawy

- dev_buy_total_sol / MFS dev_buy_sol: first observed dev buy amount na konkretnej
  powierzchni i według jej reguły kolejności,
- dev_volume_ratio: gross dev turnover share,
- jeśli potrzebna jest suma buyów: dodać osobne addytywne
  dev_buy_volume_total_sol,
- jeśli potrzebna jest ekspozycja: zdefiniować nową metrykę holdings/net exposure
  z osobnym źródłem, nie reinterpretować dev_volume_ratio,
- docelowo ujednolicić primary-buy ownership w MFS i dodać provenance
  dev_first_buy_source/order_key.

### 4.3 same_ms_tx_ratio

Werdykt: przeciążenie nazwy potwierdzone. Najważniejsza korekta to rozdzielenie
canonical aktywnego path od helpera compat.

#### TxIntelFeatures.same_ms_tx_ratio

TxIntelligence:

- sortuje timestampy,
- liczy sąsiednie delty równe 0,
- dzieli liczbę takich kolizji przez liczbę zaakceptowanych tx, nie przez liczbę
  par n-1,
- populacja zawiera deduplikowane, non-dust tx i nie filtruje success=false.

Formula:

same_ms_tx_ratio_exact = count(adjacent_delta_ms == 0) / tx_count

Dowód:

- ghost-launcher/src/tx_intelligence/engine.rs:150-192
- ghost-launcher/src/tx_intelligence/engine.rs:287-313
- ghost-launcher/src/tx_intelligence/engine.rs:688-706

Canonical Gatekeeper policy odtwarza SignerDiversityProfile z MFS i przypisuje
właśnie tę exact wartość:

- ghost-launcher/src/components/gatekeeper_policy.rs:2880-2895
- ghost-launcher/src/components/gatekeeper_policy.rs:2236-2243
- ghost-launcher/src/oracle_runtime.rs:16930-16977

#### bundle_suspicion_ratio i helper SignerDiversityProfile

W tym samym TxIntelligence istnieje osobny licznik:

bundle_suspicion_ratio_lt_50ms =
  count(adjacent_delta_ms < 50) / tx_count

W helperze compute_signer_diversity pole nazwane same_ms_tx_ratio również liczy
delta < 50 ms. Ten helper jest używany do obliczeń profilu i przez
GatekeeperBuffer assessment/compat paths. TxIntelligence nie kopiuje jednak tej
wartości helpera do canonical TxIntelFeatures.same_ms_tx_ratio; wstawia własny
exact counter.

Dowód:

- ghost-launcher/src/tx_intelligence/analysis.rs:1-3
- ghost-launcher/src/tx_intelligence/analysis.rs:174-239
- ghost-launcher/src/components/gatekeeper.rs:6010-6052
- ghost-launcher/src/components/gatekeeper.rs:7138-7173

#### RCE same_ms_tx_ratio_recent

RCE:

- bierze tylko successful tx,
- używa recent event-time window do 10 s,
- grupuje identyczne timestampy,
- liczy sumę count-1 dla każdej grupy,
- dzieli przez liczbę tx w tym oknie,
- jest logging-only evidence.

Dowód:

- ghost-launcher/src/session/observation.rs:1440-1503
- ghost-launcher/src/session/observation.rs:1520-1556
- ghost-core/src/checkpoint/types.rs:923-931

#### Najrzetelniejsza interpretacja

Każdy odczyt musi wskazywać:

- source/surface,
- exact 0 ms versus clustered <50 ms,
- populację success/dust/dedup,
- window,
- denominator tx_count, nie pair_count.

Zalecane jawne nazwy:

- tx_intel_same_ms_collision_ratio_exact,
- tx_intel_bundle_cluster_ratio_lt_50ms,
- compat_phase3_cluster_ratio_lt_50ms,
- rce_same_ms_collision_ratio_recent_exact.

Przed zmianą aktywnych progów trzeba ustalić, na której semantyce kalibrowano
historyczne max_same_ms_tx_ratio i hard_fail_same_ms_tx_ratio.

### 4.4 top3_volume_pct

Werdykt: historyczna nazwa jest błędna, ale canonical read path ma już właściwy
kontrakt compatibility.

#### Formula i skala

TxIntelligence agreguje wolumen per signer, sortuje malejąco i liczy:

top3_signer_volume_ratio =
  suma wolumenu trzech największych signerów / total_volume

Jest to ratio scale 0.0..1.0. Suffix pct nie oznacza 0..100.

Populacja obejmuje wolumen tx przyjętych przez TxIntelligence; nie jest to
wyłącznie buy volume.

Dowód:

- ghost-launcher/src/tx_intelligence/analysis.rs:174-239
- ghost-launcher/src/tx_intelligence/engine.rs:287-313

#### Aktualny compatibility contract

- preferred: top3_signer_volume_ratio: Option<f64>,
- legacy alias: top3_volume_pct: f64,
- effective_top3_signer_volume_ratio() czyta preferred i fallbackuje do aliasu,
- Gatekeeper policy używa effective helpera,
- DecisionLogger schema v33 zachowuje oba pola addytywnie.

Dowód:

- ghost-core/src/tx_intelligence/types.rs:65-70
- ghost-core/src/tx_intelligence/types.rs:104-114
- ghost-launcher/src/components/gatekeeper_policy.rs:2880-2895
- ghost-brain/src/oracle/decision_logger.rs:97-99
- ghost-brain/src/oracle/decision_logger.rs:1113-1123
- docs/ADR/ADR_8D_PR4_GATEKEEPER_TOP3_SIGNER_VOLUME_CONTRACT_20260624.md

#### Najrzetelniejsza interpretacja i pozostała praca

- nowe analizy i schematy mają używać top3_signer_volume_ratio,
- legacy top3_volume_pct pozostaje ratio-scale aliasem,
- nie zmieniać progów tylko z powodu nazwy pct,
- sprawdzić stare CSV/dashboardy pod kątem mnożenia przez 100,
- status tego findingu to migration/documentation debt, nie brak canonical fixu.

### 4.5 flip_ratio_10s

Werdykt: problem potwierdzony jako niepełna nazwa hybrydowego kontraktu.

#### Rzeczywista formula

Aktywny OracleRuntime tworzy fingerprint config przez
EarlyFingerprintConfig::default():

- window_secs = 10,
- flip_dump_pct = 0.50,
- max_flip_slots = 20.

Dowód:

- ghost-launcher/src/oracle_runtime.rs:24877-24907
- off-chain/components/seer/src/early_fingerprint.rs:34-97

TxIntelligence wywołuje in_window() przed ingestem fingerprintu. Zatem aktywna
populacja jest ograniczona do event.timestamp_ms <= t0 + 10 s.

Dowód:

- ghost-launcher/src/tx_intelligence/engine.rs:641-650
- off-chain/components/seer/src/early_fingerprint.rs:374-378

Istotny population-contract risk: ingest_fingerprint jest wykonywane przed
TxIntelligence tx-key dedup i dust filter. Adapter odrzuca synthetic events i
brak slotu, ale nie filtruje success=false. FingerprintAggregator nie ma własnego
signature dedupe. Powtórne dostarczenie eventu lub failed tx z token delta może
więc wejść do flip population, nawet jeśli późniejszy TxIntel path odrzuci
duplikat albo dust.

Dowód:

- ghost-launcher/src/tx_intelligence/engine.rs:150-167
- ghost-launcher/src/tx_intelligence/engine.rs:819-870
- off-chain/components/seer/src/early_fingerprint.rs:322-371

W tej populacji wallet jest flipperem, jeżeli:

- ma bought_tokens > 0,
- cumulative sold_tokens >= floor(bought_tokens * 0.50),
- saturating(last_sell_slot - first_buy_slot) <= 20.

Formula końcowa:

flip_ratio_10s = liczba_flipper_wallets / liczba_wallets_z_buy

Dowód:

- off-chain/components/seer/src/early_fingerprint.rs:407-424
- off-chain/components/seer/src/early_fingerprint.rs:566-578
- off-chain/components/seer/src/early_fingerprint.rs:773-793

#### Co było trafne, a co wymaga korekty

Nie jest prawdą, że obecny aktywny path nie ma dowodu 10-sekundowego okna:
callsite używa default config i in_window. Nie jest również prawdą, że metryka
jest literalnie tylko 10-sekundowa: zakwalifikowanie flipa ma dodatkowy limit
slot-gap.

Najdokładniejsza nazwa kontraktowa byłaby zbliżona do:

buyer_flip_ratio_t10s_dump50_max20slots

Nie musi to być nazwa publicznego pola; może być machine-readable metadata.

#### Ryzyka i test gap

- czas slotu nie jest stałą sekundą,
- obecny runtime hardwire'uje default zamiast ładować te parametry z configu,
- fingerprint population nie ma lokalnego dedupe i nie wymaga success=true,
- test slot-gap wywołuje ingest bez przejścia przez in_window, więc nie dowodzi
  połączonego kontraktu time-window + slot-window.

Dowód testów:

- off-chain/components/seer/src/early_fingerprint.rs:1130-1228

Kierunek naprawy: zachować legacy field, zdefiniować jawnie dedupe/success/dust
population contract, dodać do evidence użyte window_secs, flip_dump_pct i
max_flip_slots oraz test pełnego aktywnego call path.

### 4.6 funding_source_concentration

Werdykt: problem potwierdzony; to jedna z najbardziej ryzykownych nazw w aktywnym
MFS.

#### Populacja legacy FSC

FundingSourceIndex:

- bierze successful BUY,
- deduplikuje znanych buyerów po canonical buyer identity i wybiera pierwszy
  według BuyOrderKey,
- unresolved buyer identities pozostawia jako osobne unresolved lookup units,
- do legacy scalar włącza tylko resolved known sources,
- unknown buyers nie wchodzą do denominatora legacy concentration,
- wymaga co najmniej dwóch known source samples.

Dowód:

- ghost-launcher/src/tx_intelligence/funding_source.rs:1003-1075
- ghost-launcher/src/tx_intelligence/funding_source.rs:1890-1917
- ghost-core/src/tx_intelligence/types.rs:117-153

#### Formula legacy

funding_source_concentration_legacy =
  1 - distinct_known_sources / known_source_samples

Dowód:

- ghost-launcher/src/tx_intelligence/funding_source.rs:1153-1169

Konsekwencje:

- to nie HHI,
- to nie volume-weighted concentration,
- to nie udział top1,
- unknown coverage nie jest częścią samej wartości,
- maksimum przy n znanych próbek wynosi 1 - 1/n, a nie dokładnie 1,
- wartość zależy od sample size.

Przykład dla dwóch known buyerów z jednym wspólnym funderem:

- legacy FSC = 0.5,
- FSC v2 normalized count HHI = 1.0.

Dowód:

- ghost-launcher/src/tx_intelligence/funding_source.rs:2802-2830

#### FSC v2 jest osobnym kontraktem

FSC v2 niesie między innymi:

- total/known/non-neutral buyers,
- known and non-neutral coverage,
- top1 count/SOL shares,
- kilka jawnie nazwanych HHI,
- attribution confidence,
- index_warm, capture_ready, status, excluded_reason,
- lane health i provenance.

Dowód:

- ghost-core/src/tx_intelligence/types.rs:203-325

Coordination-risk eksportuje FSC tylko z clean decision-time FSC v2 i używa
hhi_norm_count:

- ghost-core/src/features/coordination/metrics.rs:964-990

#### Aktywny versus shadow/export boundary

- active Gatekeeper V2 Sybil soft signal nadal czyta legacy scalar,
- per-pool FSC v2 policy signal jest logowany counterfactual/shadow-only,
- FscV2Config.decision_enabled i hard_reject_enabled są walidacyjnie zabronione,
- osobny authoritative funding coverage gate może blokować/degradować live
  execution, ale używa gotowości lane/coverage, nie per-pool HHI concentration.

Dowód:

- ghost-launcher/src/components/gatekeeper_policy.rs:3008-3067
- ghost-launcher/src/components/gatekeeper.rs:895-962
- ghost-brain/src/config/ghost_brain_config.rs:1434-1533
- ghost-launcher/src/oracle_runtime.rs:17260-17307
- ghost-launcher/src/oracle_runtime.rs:19422-19555

#### Najrzetelniejsza interpretacja

- funding_source_concentration bez wersji = legacy collision/compression ratio,
- jakość i concentration v2 czytać wyłącznie z funding_source_v2.*,
- nie porównywać legacy scalar z thresholdem kalibrowanym na HHI,
- nie reinterpretować istniejącego pola w miejscu,
- ewentualna promocja FSC v2 do aktywnej polityki wymaga osobnego planu,
  schema/version strategy, shadow parity i kalibracji.

### 4.7 evidence_status.fsc

Werdykt: ostrzeżenie trafne, ale dokładny defekt to brak wersjonowanego kontraktu,
nie sam fakt istnienia legacy Clean.

#### Obecne zachowanie

Materializer ustawia evidence_status.fsc = Clean, jeśli
sybil_resistance.funding_source_concentration.is_some().

Nie sprawdza wtedy:

- funding_source_v2.status,
- snapshot_mode,
- capture_ready,
- index_warm,
- gap_suspected,
- excluded_reason,
- known/non-neutral coverage.

Dowód:

- ghost-launcher/src/session/observation.rs:854-873

Neutral-only test dowodzi możliwej sprzeczności:

- legacy concentration = Some(0.0),
- legacy degraded_reasons = empty,
- FSC v2 status = Degraded,
- FSC v2 excluded_reason = NeutralOnly.

Dowód:

- ghost-launcher/src/tx_intelligence/funding_source.rs:2234-2278

#### Wpływ na V3 zależy od profilu

MaterializedEvidenceStatus jest V3 evidence plane. Rustowy default
GatekeeperV3EvidenceRequirements wymaga grupy fsc i V3 może używać jej statusu
w non-clean evidence gating.

Dowód:

- ghost-core/src/checkpoint/types.rs:906-916
- ghost-brain/src/config/gatekeeper_v3_config.rs:325-380
- ghost-launcher/src/components/gatekeeper_v3.rs:704-760

Aktualny bazowy config ma V3 enabled/shadow_emit/replay_payload włączone i
promotion wyłączone, lecz jawnie ustawia gatekeeper_v3.evidence_requirements.fsc
na false:

- ghost-brain/ghost_brain_config.toml:381-404
- ghost-brain/ghost_brain_config.toml:449-465

W bazowym profilu błędny status nie blokuje ani nie przepuszcza V3 przez required
FSC evidence gate. Nadal jest serializowany i może mylić replay/offline consumer.
W profilu, który pozostawi default fsc=true albo jawnie włączy wymaganie, status
może zmienić V3 shadow evidence gating. Nie wpływa na aktywny Gatekeeper V2
BUY/REJECT.

#### Dwie możliwe semantyki i rekomendacja

Semantyka A:

- fsc oznacza availability legacy scalar używanego przez obecny V3 risk numeric
  field.

Semantyka B:

- fsc oznacza jakość nowoczesnego FSC evidence, czyli v2 readiness/coverage.

Obecna nazwa nie mówi, którą semantykę reprezentuje. Ciche przedefiniowanie
istniejącego fsc z A na B zmieniłoby V3 shadow decisions i złamałoby replay
porównywalność.

Rekomendowane rozwiązanie planistyczne:

- dodać jawne fsc_legacy i fsc_v2 statusy,
- zachować istniejące fsc jako wersjonowany compatibility alias albo wykonać
  jawną migrację schema,
- wskazać w GatekeeperV3EvidenceRequirements, której wersji wymaga policy,
- dla fsc_v2 mapować Clean dopiero z decision-time clean evidence oraz wymaganych
  readiness/gap/excluded conditions,
- dodać testy neutral-only, low coverage, index cold, stream unavailable i clean.

### 4.8 ManipulationContradictionFeatures.high_*

Werdykt: flagi są niewiarygodne jako pola faktograficzne, ale poprzedni raport
przeszacowywał ich wpływ na aktualny verdict.

#### Obecne zachowanie

Struct definiuje:

- high_same_ms_tx_ratio,
- high_bundle_suspicion_ratio,
- high_top3_volume_pct,
- high_hhi,
- high_signer_concentration,
- high_dev_concentration.

Materializer ustawia wartości numeryczne i inne composite flags, po czym używa
Default dla reszty. Wszystkie high_* pozostają false.

Dowód:

- ghost-core/src/checkpoint/types.rs:753-809
- ghost-launcher/src/session/observation.rs:590-686

V3 replay payload serializuje te flagi:

- ghost-launcher/src/components/gatekeeper_v3.rs:491-518

#### Czy powoduje to bypass?

V3 has_hard_risk_contradiction sprawdza flagi, ale równolegle sprawdza:

- same_ms_tx_ratio,
- bundle_suspicion_ratio,
- top3_volume_pct,
- hhi,
- max_tx_per_signer,
- dev_volume_ratio,
- CPV,
- legacy FSC.

Dowód:

- ghost-launcher/src/components/gatekeeper_v3.rs:766-792

Dlatego obecne default false nie tworzy pełnego bypassu hard risk dla wartości
numerycznych. Tworzy natomiast:

- fałszywe dane dla konsumenta czytającego tylko bool,
- niespójność replay payload,
- niejasny kontrakt progów,
- ryzyko przyszłej regresji, jeśli numeric fallback zostanie zmieniony.

#### Gdzie powinny być liczone flagi

High jest pojęciem zależnym od GatekeeperV3StageProfile i od stage
early/normal/extended. MaterializedFeatureSet powinien zachować surowe liczby,
a policy powinno porównywać je z wersjonowanym configiem.

Preferowany kierunek:

- traktować numeric values w MFS jako SSOT,
- zdeprecjonować high_* jako surowe materialized facts,
- jeśli bool jest potrzebny w logu, emitować derived policy flags razem ze
  stage, profile/config hash i threshold,
- nie wstrzykiwać stage-specific config do ogólnego materializera tylko po to,
  by ustawić convenience bool.

Do czasu naprawy nie wolno używać high_* jako samodzielnych prawdziwych flag.

### 4.9 reserve_velocity_sol_per_sec

Werdykt: formula ma poprawną jednostkę, ale wartość nie ma wystarczającego statusu
ważności.

#### Rzeczywista formula

AccountStateCore liczy:

reserve_velocity_sol_per_sec =
  (current_real_sol_reserves - previous_real_sol_reserves) / elapsed_receive_time_seconds

Rezerwy są konwertowane z lamportów do SOL. Timestamp to receive_ts_ms kolejnych
applied account updates, nie ciągły zegar samplera ani deklarowany block time.

Dowód:

- ghost-core/src/account_state_core/types.rs:101-145
- ghost-core/src/account_state_core/reducer.rs:98-120
- ghost-core/src/account_state_core/reducer.rs:462-477

To interval-average między dwoma update'ami. Może być dodatnie, ujemne albo zero.

#### Cztery różne znaczenia 0.0

1. prawdziwy brak zmiany reserve między dwoma update'ami,
2. pierwszy canonical update, update_count=1,
3. current_ts_ms <= previous_ts_ms po saturating_sub, czyli delta_ms=0,
4. fallback bez canonical account state, is_bootstrap=true i update_count=0.

Dowód:

- ghost-core/src/account_state_core/reducer.rs:103-120
- ghost-launcher/src/session/observation.rs:2820-2893
- ghost-core/tests/account_state_core_tests.rs:190-224

Companion is_bootstrap, state_phase i update_count odróżniają fallback oraz
pierwszy update, ale nie odróżniają prawdziwego zera od zero-delta-time po dwóch
update'ach.

#### Consumer classification

W bieżącym checkoutcie pole jest częścią MFS account evidence i jest serializowane
do V3 feature snapshot. Nie znaleziono aktywnego Gatekeeper V2 threshold consumer.

Dowód:

- ghost-launcher/src/components/gatekeeper_v3.rs:307-313

#### Kierunek naprawy

Bezpieczna zmiana jest addytywna:

- reserve_velocity_status = measured | first_update | bootstrap_fallback |
  zero_delta_time,
- reserve_velocity_interval_ms,
- opcjonalnie source_clock = receive_time,
- consumer może uznać measured dopiero przy update_count >= 2 i dodatnim delta time.

Nie należy zmieniać istniejącego f64 na Option bez jawnej migracji schema.

### 4.10 buy_sell_ratio_recent

Werdykt: problem potwierdzony i ograniczony do logging-only RCE evidence.

#### Rzeczywista formula

RCE recent window:

- jest zakotwiczone do ostatniego event timestampu,
- obejmuje do 10 sekund,
- liczy tylko successful tx,
- liczy buy i sell counts.

Następnie:

- jeśli sell_count > 0: buy_count / sell_count,
- jeśli sell_count == 0: buy_count jako f64.

Dowód:

- ghost-launcher/src/session/observation.rs:1440-1503
- ghost-launcher/src/session/observation.rs:1520-1556
- ghost-core/src/checkpoint/types.rs:855-885

Istniejący test potwierdza wartość 6.0 dla okna z sześcioma buyami i bez selli,
oraz oznaczenie logging-only:

- ghost-launcher/tests/session_lifecycle_tests.rs:1003-1025

#### Konsekwencje

- pole jest unbounded,
- nie jest buy share,
- nie jest porównywalne z buy_ratio = buys / total tx,
- 1 buy / 0 sells oraz 1 buy / 1 sell oba dają 1.0,
- bez raw counts nie można odtworzyć denominatora.

#### Kierunek naprawy

Zachować legacy field dla kompatybilności i dodać:

- buy_count_recent,
- sell_count_recent,
- buy_to_sell_ratio_recent: Option<f64>, gdzie None przy zero sells,
- opcjonalnie bounded buy_ratio_recent = buys / (buys + sells),
- denominator_status = measured | no_sells.

Nie promować tej metryki do policy bez osobnej walidacji; obecny
template_reason_code jawnie mówi rce_a0_not_evaluated_logging_only.

## 5. Wspólne przyczyny źródłowe

### 5.1 Nazwa pola nie jest kontraktem

Poprawny kontrakt metryki musi zawierać co najmniej:

- semantic version,
- owner component,
- surface/path,
- active/shadow/export/logging-only classification,
- population i filtry,
- ordering rule,
- observation window,
- numerator,
- denominator,
- unit i scale,
- null/fallback behavior,
- evidence status/reasons,
- policy/log/replay consumers.

### 5.2 Canonical i compat nadal bywają mylone

Najważniejsze przykłady:

- MFS dev first buy versus GatekeeperBuffer create-signature primary buy,
- MFS exact same-ms versus SignerDiversityProfile <50 ms,
- runtime FTDI unique-count ratio versus coordination HHI diversity,
- legacy FSC scalar versus FSC v2 HHI/readiness.

Compat path nie może stać się drugim źródłem prawdy. Jeśli ma lepszą semantykę,
należy przenieść ją do właściwego właściciela MFS, nie czytać compat buffer
bezpośrednio w policy.

### 5.3 Zero i Some(value) nie dowodzą jakości

Przykłady:

- reserve velocity 0.0,
- dev buy 0.0,
- legacy FSC Some(0.0),
- FTDI Some(value) z degraded reason.

Wartość musi być czytana razem z readiness/status/sample/provenance.

### 5.4 Derived flag nie powinien udawać raw evidence

Pola high_* zależą od policy thresholds i stage. Raw MFS powinien przechowywać
liczby, a policy-derived log powinien przechowywać wynik porównania wraz z
threshold/config hash.

## 6. Priorytety wejściowe dla następnego planu

Poniższa lista nie jest zgodą na implementację ani zmianę policy. Jest bounded
inputem do kolejnej iteracji planistycznej.

### P0: kontrakty i brak fałszywego clean evidence

1. Zdefiniować wersjonowane fsc_legacy versus fsc_v2 evidence status.
2. Zabezpieczyć replay oraz profile V3 wymagające FSC przed traktowaniem legacy
   presence jako automatycznego FSC v2 clean; bazowy profil ma obecnie fsc=false.
3. Ustalić docelowy kontrakt high_* i nie opierać żadnego konsumenta na ich
   obecnych default false.

### P1: source i denominator truth

4. Ujednolicić dev primary-buy semantics w canonical MFS oraz dodać provenance.
5. Dodać source-qualified same-ms/bundle contracts.
6. Naprawić lub jawnie zakontraktować dedupe/success population oraz dodać
   parametry/metadata hybrydowego flip ratio.
7. Dodać validity status i interval dla reserve velocity.
8. Dodać raw buy/sell counts oraz denominator status dla RCE.

### P2: migracja nazw bez policy drift

9. Utrwalić FTDI surface/sample contract w metric registry.
10. Migrować downstream z top3_volume_pct do top3_signer_volume_ratio.
11. Utrwalić legacy FSC jako versioned legacy formula; v2 czytać osobno.

## 7. In-scope i out-of-scope następnego planu

### In scope

- machine-readable lub audytowalny metric contract registry dla dokładnie 10
  rodzin z tego raportu,
- addytywne pola provenance/status/counts,
- schema-version i backward-compat strategy,
- targeted tests producer -> MFS -> policy/log/replay,
- downstream migration bez zmiany thresholdów,
- explicit active/compat/shadow/export/logging-only labels.

### Out of scope bez osobnej zgody

- zmiana BUY/REJECT/TIMEOUT,
- zmiana progów,
- zmiana selector score,
- promocja coordination-risk do active policy,
- promocja per-pool FSC v2 concentration do active Gatekeeper,
- włączenie live behavior,
- usuwanie legacy JSON fields,
- szeroki refactor TxIntelligence, Gatekeeper, Seer lub AccountStateCore,
- traktowanie dev_volume_ratio jako holdings/exposure,
- materializowanie stage-specific high_* bez decyzji o ownership.

## 8. Acceptance gates proponowane dla planu wykonawczego

### FTDI

- test runtime formula i n-dependent minimum,
- test two-buyer Some(value) + degraded + policy non-actionable,
- test missing topology -> None,
- test coordination HHI diversity pozostaje export-only.

Istniejące testy bazowe:

- cargo test -p ghost-launcher tx_intelligence::sybil_metrics::tests::ftdi_two_buy_sample_exports_degraded_diagnostic_value
- cargo test -p ghost-core --test coordination_metrics_phase06 ftdi_v2_uses_hhi_diversity_and_keeps_export_only_policy

### Dev

- test first observed versus create-signature primary divergence,
- test ordering/provenance po docelowej decyzji,
- test failed/non-dust semantics,
- test gross dev turnover formula,
- serde compatibility dla legacy dev_buy_total_sol.

### Same-ms

- test exact 0 ms versus 1..49 ms,
- test denominator tx_count,
- test canonical MFS policy używa exact,
- test RCE success-only recent grouping.

### Top3

- cargo test -p ghost-core --test tx_intelligence_contract_tests
- cargo test -p ghost-launcher --test gatekeeper_policy_tests top3_legacy_payload_missing_new_field_falls_back_to_compatibility_alias

### Flip

- cargo test -p seer test_flip_ratio_basic
- cargo test -p seer test_flip_ratio_slot_gap_too_large
- nowy test aktywnego in_window + slot-gap + emitted config metadata,
- nowy test duplicate signature i success=false zgodny z wybraną population policy.

### FSC i V3 evidence

- cargo test -p ghost-launcher --test session_lifecycle_tests materialize_features_populates_fsc_from_shared_funding_source_index
- nowy test neutral-only: legacy Some(0.0), v2 Degraded, jawne dwa statusy,
- test low coverage, index cold, stream unavailable, gap suspected i clean,
- replay test przed/po schema version.

### Manipulation high_*

- test materializer nie przedstawia default false jako measured flag,
- test V3 numeric hard-risk parity,
- test derived policy flags zawierają stage/config hash, jeśli ten wariant zostanie wybrany.

### Reserve velocity i RCE ratio

- cargo test -p ghost-core --test account_state_core_tests reducer_preserves_raw_reserves_but_exposes_normalized_feature_units
- nowy test true zero versus first update versus zero-delta-time versus fallback,
- cargo test -p ghost-launcher --test session_lifecycle_tests materialize_features_populates_decision_series_and_temporal_deltas_from_session_buffer
- nowy serde test raw recent buy/sell counts i zero-sell denominator status.

### Global

- cargo fmt --check
- cargo test dla dotkniętych crates/test targets
- serde old-record compatibility fixtures
- replay parity dla niezmienionych pól legacy
- git diff --check
- jawny audit diffu potwierdzający brak zmian policy/progów/shadow-live.

## 9. Weryfikacja wykonana w tym audycie

Prześledzono aktualne definicje, producentów, materializację i konsumentów w:

- ghost-launcher/src/tx_intelligence/sybil_metrics.rs
- ghost-launcher/src/tx_intelligence/engine.rs
- ghost-launcher/src/tx_intelligence/analysis.rs
- ghost-launcher/src/tx_intelligence/funding_source.rs
- ghost-launcher/src/session/observation.rs
- ghost-launcher/src/components/gatekeeper.rs
- ghost-launcher/src/components/gatekeeper_policy.rs
- ghost-launcher/src/components/gatekeeper_v3.rs
- ghost-launcher/src/oracle_runtime.rs
- off-chain/components/seer/src/early_fingerprint.rs
- ghost-core/src/tx_intelligence/types.rs
- ghost-core/src/checkpoint/types.rs
- ghost-core/src/account_state_core/types.rs
- ghost-core/src/account_state_core/reducer.rs
- ghost-core/src/features/coordination/config.rs
- ghost-core/src/features/coordination/evidence.rs
- ghost-core/src/features/coordination/metrics.rs
- ghost-brain/src/config/ghost_brain_config.rs
- ghost-brain/src/config/gatekeeper_v3_config.rs
- ghost-brain/src/oracle/decision_logger.rs
- ghost-brain/ghost_brain_config.toml
- odpowiadające testy ghost-core, ghost-launcher i seer.

Uruchomiono następujące testy kontraktowe na HEAD `f3318f3`; wszystkie zakończyły
się wynikiem PASS:

- `cargo test -p ghost-launcher --lib tx_intelligence::sybil_metrics::tests::ftdi_two_buy_sample_exports_degraded_diagnostic_value -- --exact`
  - 1 passed; potwierdza `Some(value)` plus degraded evidence dla próbki dwóch kupujących,
- `cargo test -p ghost-core --test coordination_metrics_phase06 ftdi_v2_uses_hhi_diversity_and_keeps_export_only_policy -- --exact`
  - 1 passed; potwierdza odrębną HHI-diversity semantykę i `ExportOnly`,
- `cargo test -p ghost-core --test tx_intelligence_contract_tests`
  - 2 passed; potwierdza addytywny kontrakt `top3_signer_volume_ratio` i legacy fallback,
- `cargo test -p seer test_flip_ratio_basic`
  - 1 pasujący test passed; potwierdza podstawową formułę flip ratio,
- `cargo test -p seer test_flip_ratio_slot_gap_too_large`
  - 1 pasujący test passed; potwierdza kwalifikację przez limit slotów,
- `cargo test -p ghost-launcher --test session_lifecycle_tests materialize_features_populates_decision_series_and_temporal_deltas_from_session_buffer -- --exact`
  - 1 passed; potwierdza materializację RCE, w tym zero-sell wynik `6.0`,
- `cargo test -p ghost-launcher --test session_lifecycle_tests materialize_features_populates_fsc_from_shared_funding_source_index -- --exact`
  - 1 passed; potwierdza materializację legacy FSC i FSC v2 z jednego snapshotu,
- `cargo test -p ghost-launcher --lib tx_intelligence::funding_source::tests::neutral_funders_do_not_artificially_cluster_buyers -- --exact`
  - 1 passed; potwierdza przypadek legacy `Some(0.0)` przy FSC v2 `NeutralOnly`,
- `cargo test -p ghost-core --test account_state_core_tests reducer_preserves_raw_reserves_but_exposes_normalized_feature_units -- --exact`
  - 1 passed; potwierdza jednostki i aktualizacyjny charakter cech reserve.

Testy są dowodem lokalnych kontraktów, a nie dowodem rozwiązania wskazanych
problemów. W szczególności obecne testy flip osobno dowodzą formuły i limitu
slotów, ale nie dowodzą całego aktywnego połączenia: okno 10 s, deduplikacja,
filtr powodzenia i limit slotów. Nie uruchamiano pełnego workspace test suite;
nie wolno interpretować tego audytu jako pełnego runtime proof.

## 10. Final verdict

Final verdict: METRIC_INTERPRETATION_CORRECTION_REQUIRED

Plan-readiness verdict: READY_FOR_TARGETED_IMPLEMENTATION_PLAN

Najważniejsza zasada następnej iteracji:

MaterializedFeatureSet pozostaje canonical decision snapshot. Naprawa nie może
tworzyć drugiej prawdy w GatekeeperBuffer, raportach lub sidecarze. Legacy pola
pozostają kompatybilne, a nowe znaczenie musi być addytywne, wersjonowane i
opatrzone source, sample, denominator oraz evidence status. Żadna korekta nazwy
nie jest automatyczną zgodą na zmianę policy lub thresholdów.
