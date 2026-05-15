# LEGACY_SCORING

Audyt starego, wycofanego silnika scoringowego HyperOracle / HyperPrediction.

## Stan repozytorium w chwili audytu
- repo root: /root/Gho
- HEAD: 9b959a7
- plik jest artefaktem dokumentacyjnym, nie elementem aktywnego runtime
- nie zmieniono istniejacych modulow projektu

## Najwazniejsza konkluzja
stary silnik HyperPrediction / HyperOracle nadal istnieje w kodzie jako
warstwa kompatybilnosci, diagnostyki, testow i bibliotek sygnalow, ale nie jest
kanoniczna sciezka decyzyjna Gatekeeper V2/V2.5. Aktywna sciezka decyzji
przechodzi przez PoolObservationSession::materialize_features(),
MaterializedFeatureSet i Gatekeeper V2/V2.5. Legacy scoring moze nadal zostac
uruchomiony przez test-only helper OracleRuntime::score_pool() albo przez
kompatybilnosciowy OraclePipeline w Triggerze, ale PoolScored ma plane
legacy_observation, a Trigger blokuje autorytatywne BUY side effects po tym
zdarzeniu.

## Wazne ograniczenie zakresu
QASS nie jest tu traktowany jako glowny modulator oceny. W aktualnym kodzie
QASS jest zdeprecjonowany, stubowany albo pozostawiony jako drugorzedny,
kompatybilnosciowy modifier. Glownym opisanym rdzeniem legacy score jest
HyperPrediction + SurvivorScore + HyperOracle + zestaw modulow sygnalowych.


## 1. Zakres audytu


### In-scope
- ghost-brain/src/oracle/hyper_prediction/ (wszystkie pliki)
- ghost-brain/src/oracle/hyper_oracle.rs
- ghost-brain/src/oracle/survivor_score.rs
- ghost-brain/src/oracle/scoring.rs
- ghost-brain/src/oracle/snapshot_engine.rs
- ghost-brain/src/oracle/score_history.rs
- ghost-brain/src/oracle/followup_scoring.rs
- ghost-brain/src/oracle/predator_strategy.rs
- ghost-brain/src/oracle/engine.rs
- ghost-brain/src/oracle/bva.rs
- ghost-brain/src/oracle/tcf/ (wszystkie pliki)
- ghost-brain/src/oracle/ultrafast/ (wszystkie pliki)
- ghost-brain/src/chaos/ (wszystkie pliki)
- ghost-launcher/src/oracle_runtime.rs
- ghost-launcher/src/events.rs
- ghost-launcher/src/components/oracle_pipeline.rs
- ghost-launcher/src/components/trigger/component.rs
- off-chain/components/seer/src/paradox_sensor/ (wszystkie pliki)
- konfiguracja ghost-brain/ghost_brain_config.toml dotyczaca
  hyper_prediction, scoring, tcf, paradox

### Out-of-scope
- projektowanie nowego aktywnego Gatekeepera
- zmiana runtime, configow, JSONL schema albo logow
- przywracanie HyperPrediction / Chaos / PoolScored do aktywnej sciezki
- stary model scoringu z QASS jako glownym operatorem oceny


## 2. Status aktywny vs legacy


### 2.1. Aktywna sciezka decyzyjna

Aktualny runtime Ghosta powinien byc rozumiany jako:

Seer / Yellowstone / EventBus
-> OracleRuntime
-> PoolObservationSession
-> PoolObservationSession::materialize_features()
-> MaterializedFeatureSet
-> Gatekeeper V2/V2.5 policy evaluation
-> IWIM veto jezeli BUY path tego wymaga
-> shadow/live execution boundary
-> DecisionLogger / replay / post-buy lifecycle

W oracle_runtime.rs aktywny loop obsluguje m.in.:
- GhostEvent::NewPoolDetected
- GhostEvent::PoolTransaction
- GhostEvent::FundingTransferObserved
- GhostEvent::GatekeeperCommitted
- GhostEvent::AccountUpdate

Aktywna terminalna ewaluacja materializuje MaterializedFeatureSet i idzie przez
evaluate_from_features. Nie uzywa HyperPredictionOracle jako autorytatywnego
scorera dla BUY.

### 2.2. Legacy runtime i test-only helper

ghost-launcher/src/oracle_runtime.rs zawiera OracleRuntime::score_pool().
Funkcja jest oznaczona jako deprecated:

- since = "3.0.0"
- note = "Test-only helper. Production path uses GatekeeperV2::evaluate."

To jest najwazniejszy runtime anchor starego silnika. Funkcja nadal pokazuje,
jak dawny pipeline skladal dane:

1. probuje znalezc sesje poola przez lookup_pool_session()
2. jezeli nie ma sesji, probuje lookup_detected_pool()
3. jezeli tego tez nie ma, probuje build_runtime_state_pool_snapshot()
4. buduje EnhancedCandidate przez build_enhanced_candidate_from_pool_data()
5. w ostatecznosci buduje minimalny EnhancedCandidate z pool identity
6. koryguje timestamp z history_buffer
7. czyta ParadoxState z paradox_rx
8. czyta ShadowLedger curve state dla bonding_curve
9. wzbogaca candidate o virtual_sol_reserves, token_total_supply,
   bonding_curve_progress
10. IWIM w tym helperze jest ustawiony na None, bo Gatekeeper V2 Phase 5
    przejal dev behavior inline
11. timestampy bierze z ShadowLedger snapshots dla base_mint
12. jezeli sa co najmniej 4 timestampy, uruchamia ResonanceDetector
13. buduje TransactionMetrics przez build_shadow_tx_metrics()
14. buduje AmmPool z live curve albo fallback genesis pool
15. jezeli ma realne metrics, uruchamia ChaosEngine::run_simulation()
16. wywoluje self.hyper_oracle.score_candidate(...)
17. opcjonalnie mark_pool_scored()

To jest kompletna sciezka starego score_pool:

Pool/session/runtime snapshot
-> EnhancedCandidate
-> ShadowLedger enrichment
-> ParadoxState
-> timestamps
-> ResonanceDetector
-> TransactionMetrics
-> AmmPool
-> ChaosEngine
-> HyperPredictionOracle::score_candidate()
-> HyperPredictionResult

### 2.3. PoolScored jako legacy observation

ghost-launcher/src/events.rs nadal definiuje PoolScoredEvent:
- pool_amm_id
- base_mint
- score
- passed
- risk_level
- interpretation
- processing_time_us
- component_scores

GhostEvent::PoolScored istnieje, ale runtime_plane() mapuje go do:
RuntimePlane::LegacyObservation.

To znaczy, ze samo wystapienie PoolScored nie jest BUY i nie jest
kanonicznym zatwierdzeniem wejscia. Jest to event obserwacyjny/debugowy
starego pipeline.

### 2.4. Trigger i blokada side effects

ghost-launcher/src/components/trigger/component.rs ma
handle_legacy_pool_scored_event().

Jezeli PoolScored(passed=true) przyjdzie na Trigger:
- Trigger usuwa pending pool z mapy
- rejestruje legacy_path_side_effect_block_total
- loguje, ze legacy PoolScored path jest zablokowany przed emisja
  authoritative BUY side effects
- zwraca LegacyPoolScoredHandling { blocked_side_effect: true, ... }

Embedded OraclePipeline w Triggerze jest rowniez klasyfikowany jako
compatibility-only / legacy_observation. Trigger loguje, ze ten pipeline
nie moze emitowac autorytatywnych BUY side effects.


## 3. Makro-flow starego silnika


Pelny legacy flow, w uproszczeniu logicznym:

1. Seer wykrywa token/pool i buduje CandidatePool / DetectedPool.
2. Runtime lub pipeline konwertuje to do EnhancedCandidate.
3. SnapshotEngine i ShadowLedger zbieraja transakcje, snapshoty, rezerwy,
   timestampy, sygnatury, signerow, wolumeny i kierunek buy/sell.
4. ParadoxSensor moze publikowac ParadoxState z telemetrii sieciowej.
5. HyperPredictionOracle przyjmuje:
   - EnhancedCandidate
   - PumpCurveStateCache
   - opcjonalny AmmPool
   - opcjonalne tx_timestamps
   - opcjonalne raw tx bytes
   - opcjonalny IWIM result
   - opcjonalny ChaosResult
   - opcjonalny ResonanceResult
   - opcjonalny gene safety result
   - opcjonalny hunter score
   - opcjonalny TransactionMetrics
   - opcjonalny ClusterAnalysis
   - opcjonalny ParadoxState
   - opcjonalne tuned scoring weights
   - opcjonalne LIGMA result
   - opcjonalne behavioral signals
6. Orchestrator wykrywa faze EarlyStage albo FullAnalysis.
7. Orchestrator odpala twarde veto:
   - ClusterHunter cabal risk
   - LIGMA liquidity/tradability veto
   - FRE Skip
   - QEDD/MCI abort tylko w full analysis
   - SurvivorScore critical cutoff
8. Orchestrator zbiera sygnaly:
   - base score z legacy score_enhanced()
   - ShadowLedger diagnostics
   - QASS neutral/stub compatibility
   - SSMI
   - MPCF
   - IWIM
   - PRAECOG
   - FRE
   - SCR
   - ULVF
   - MESA
   - POVC
   - QMAN
   - QEDD
   - MCI
   - Paradox delay signal
   - SecondWaveDetector
   - SurvivorScore
   - TCF
9. scoring::calculate_final_score() sklada:
   - SurvivorScore jako baze
   - QASS jako drugorzedny modifier tylko gdy valid/confident
   - fallback confidence multiplier
   - penalties
   - boosters
   - risk level z raw score
   - display clamp 0..100
   - passed = survivor_passed && display_score >= threshold
10. Wynikiem jest HyperPredictionResult.
11. to_scored_candidate_impl konwertuje HyperPredictionResult do
    ScoredCandidate dla starego pipeline.
12. Dawny runtime mogl emitowac PoolScored, ale obecnie PoolScored jest
    legacy_observation i Trigger blokuje side effects.


## 4. Kontrakty danych


### 4.1. CandidatePool / DetectedPool / EnhancedCandidate

Zrodlo danych startowych to Seer i launcher eventy:
- slot
- event time
- signature
- amm_program_id
- pool_amm_id
- base_mint
- quote_mint
- bonding_curve
- creator
- bonding_curve_progress
- initial_liquidity_sol
- token_total_supply

Legacy scoring nie powinien byc mylony z aktualnym MaterializedFeatureSet.
EnhancedCandidate jest rozszerzonym obiektem wejscia dla starych scoringow,
a MaterializedFeatureSet jest kanonicznym snapshotem decyzji obecnego
Gatekeepera.

### 4.2. TransactionMetrics

TransactionMetrics w legacy flow sluzy jako syntetyczne, skumulowane okno
zachowania poola:
- tx_count
- buy_count / sell_count
- total_volume_sol
- buy_volume_sol / sell_volume_sol
- unique_addresses
- buy_pressure
- temporal/spread metrics, jezeli dostepne

W score_pool() TransactionMetrics powstaje przez build_shadow_tx_metrics()
na podstawie ShadowLedger.

### 4.3. SnapshotEngine::MarketSnapshot

MarketSnapshot przechowuje:
- timestamp_ms
- event_ts_source
- slot jako Option<u64>
- cum_volume_sol
- tx_count
- unique_addrs
- cum_buy_volume_sol
- cum_sell_volume_sol
- window_tx_count
- window_volume_sol
- window_buy_volume_sol
- window_sell_volume_sol
- reserve_base
- reserve_quote
- price_quote
- price_state
- price_reason
- dev_buy_lamports
- data_source jako SoftTruth albo HardTruth

SnapshotEngine ma kontrakt osi czasu:
- slot jest metadana
- slot == 0 jest invalid i normalizowany do None
- brak slotu zostaje None
- decision code nie powinien traktowac slotu 0 jako realnej wartosci

### 4.4. SnapshotEngine::TransactionRecord

TransactionRecord zasila SOBP/MPCF i czesc behavioral scoringu. Bufor transakcji
w RingSnapshots trzyma ostatnie rekordy z okna event-time. Decision-eligible
rekordy nie powinny byc wypychane przez storage-only timestamps.

### 4.5. HyperPredictionResult

HyperPredictionResult jest glownym wynikiem starego silnika. Zawiera:
- score
- passed
- risk_level
- processing_time_us
- interpretation
- analysis_phase
- fallback tracker
- ssmi_result
- mpcf_result
- iwim_result
- praecog_result
- mesa_result
- scr_score
- ulvf_divergence
- ulvf_curl
- povc_cluster
- qedd_result
- mci_result
- qman_score/confidence/metadata
- ligma_result
- cluster_result
- paradox_result
- second_wave_result
- survivor_result
- tcf_result
- shadow_progress
- shadow_price_ratio
- chaos_result
- resonance_result
- gene_safety_result
- hunter_score
- should_delay_entry
- recommended_delay_ms

### 4.6. SurvivorScoreInput / SurvivorScoreResult

SurvivorScoreInput sklada sygnaly:
- session_stage
- qedd_survival_60s
- iwim_threat_score
- cluster_risk_score
- sobp_momentum
- qman_score
- chaos_pump_prob
- mpcf_organic_ratio
- mesa_organic_likeness
- scr_bot_score
- unique_wallet_ratio
- mesa_wash_likeness
- qman_exit_signal
- price_crash_detected
- paradox_anomaly
- ligma_tradability_score
- ligma_psi
- ligma_liquidity_trap_risk
- ecto_score
- bva_score
- panic_score
- tcr_score
- cir_score
- ecto_verdict
- tx_count
- age_secs

SurvivorScoreResult zawiera score, raw_score, passed, breakdown,
interpretation, confidence, signals_used, analysis_time_us i optional veto.


## 5. SnapshotEngine jako stara baza danych rynkowych


SnapshotEngine jest opisany jako high-performance market snapshot system dla
SCR/ULVF/POVC oraz przyszlych modulow HyperPrediction/SSMI/ULVFExtended.

### 5.1. Inicjalizacja poola

handle_initialize_pool_event():
- jezeli pool nie jest aktywny, buforuje InitPoolEvent
- normalizuje slot
- tworzy albo pobiera PoolState
- ustawia last_event_ts_ms
- wylicza price przez derive_price()
- tworzy bootstrap snapshots g0/g1/g2
- zapisuje stan rezerw, price_state i event_ts_source

Bootstrap snapshoty sa istotne dla starych algorytmow, bo ULVF i podobne
moduly wymagaja par snapshotow. Jednoczesnie sa one syntetyczne i nie powinny
byc traktowane jako dowod realnego flow rynku.

### 5.2. Obsługa transakcji

handle_tx_event():
- mierzy ingest hot path
- filtruje non-active pool lifecycle
- buforuje transakcje dla nieaktywnego poola
- rozstrzyga timestamp przez resolve_tx_event_timestamp()
- normalizuje slot
- buduje TxKey z event time, slot, event ordinal, signature i fallback counter
- deduplikuje transakcje
- pozwala na enrichment duplikatu jezeli pozniejszy event niesie lepsze dane
- aktualizuje akumulatory wolumenu i kierunku
- aktualizuje unique signers
- wypycha TransactionRecord do ring buffer
- emituje snapshot, gdy okno czasowe przekroczy snapshot_interval_ms

### 5.3. Dane udostepniane modulom scoringowym

SnapshotEngine dostarcza:
- get_latest_snapshot()
- latest_pair() dla ULVF
- last_n()
- get_live_counters()
- get_transactions()
- get_transactions_since()
- mark_pool_committed()

W legacy engine te dane karmia:
- HyperOracle SCR/ULVF/POVC
- SOBP
- MPCF/CIR/BVA/PANIC
- Chaos simulation input
- Followup scoring
- ScoreHistory / cyclic loops


## 6. HyperOracle


Plik: ghost-brain/src/oracle/hyper_oracle.rs

HyperOracle jest matematycznym rdzeniem starych sygnalow:
- SCR: Spectral Coherence Ratio
- ULVF: Ultra Liquidity Vector Field
- POVC: Principal Oracle Vector Clustering

### 6.1. SCR

calculate_scr(timestamps):
- wymaga co najmniej 4 timestampow
- liczy delty miedzy timestampami
- odpala FFT na sekwencji inter-arrival times
- liczy high frequency energy / total energy
- wynik jest interpretowany jako bot/timing synchrony score
- wysokie SCR oznacza zbyt regularne, maszynowe lub zsynchronizowane zachowanie

W final scoring SCR:
- w full analysis, scr_score > 0.7 daje kare -10 * scr_penalty_mult
- w SurvivorScore quality jest zwykle odwracany jako 1 - SCR

### 6.2. ULVF

calculate_ulvf(snapshot_a, snapshot_b):
- bierze dwie migawki rynku
- liczy dt
- liczy gradienty wolumenu, tx_count i unique_addrs
- divergence = suma przeplywow / norma
- curl = norma rotacji wektora / norma, ograniczona

Interpretacja:
- niska divergence moze oznaczac slaby przeplyw organiczny
- wysoki curl moze oznaczac chaotyczny, manipulowany lub wirujacy flow

W final scoring:
- divergence < 0.3 daje kare -5 * ulvf_div_penalty_mult
- curl > 15 daje kare -10 * ulvf_curl_penalty_mult

### 6.3. POVC

calculate_povc(snapshot):
- buduje wektor z volume, tx_count, unique_addrs
- skaluje volume * 0.001, tx_count * 0.01, unique * 0.02
- projektuje wektor przez PCA basis
- wybiera najblizszy centroid

Komentarze w kodzie opisują:
- cluster 0 = ULTRA_ORGANIC
- cluster 1 = ORGANIC
- cluster 2 = BOT_NOISE
- cluster 3 = SYBIL_ATTACK

Istotna niezgodnosc:
implementacja HyperOracle ma centroidy zwracajace zakres 0..2, natomiast
penalties/boosters obsluguja rowniez cluster 3. To jest wazna luka projektowa
do omowienia przed jakimkolwiek odrodzeniem POVC.


## 7. HyperPredictionOracle


Plik: ghost-brain/src/oracle/hyper_prediction/mod.rs

HyperPredictionOracle zawiera:
- ssmi: SubSlotMicroentropy
- hyper: HyperOracle
- mesa_analyzer
- scr_extended
- ulvf_extended
- qedd
- mci
- wallet_energy_tracker
- transition_matrix
- unitary_evolution
- qman_signal_detector
- second_wave_detector
- survivor_calculator
- fractal_engine
- threshold
- risk_thresholds
- normalization_config
- ligma_config
- fallback_config
- hyper_prediction_config
- scoring_weights
- tcf_config

Publiczne wejscia:
- new(threshold)
- new_with_config(threshold, GhostBrainConfig)
- score_candidate(...)
- score_candidate_with_behavioral(...)
- to_scored_candidate(...)

### 7.1. Konfiguracja

HyperPredictionConfig laczy:
- gatekeeper_min_tx_count
- early_stage_multiplier
- survivor_critical_threshold
- qass_secondary_max_adjustment
- qass_min_confidence_for_modifier
- cold_start_max_adjustment
- cold_start_qedd_mci_weight
- MESA thresholds
- min_volume_scale
- relative_factor_cap
- burst_normalization
- risk_thresholds
- followup_scoring
- survivor_thresholds
- risk_multipliers
- orchestrator_thresholds

Domyslny early-stage threshold:
gatekeeper_min_tx_count * early_stage_multiplier.
Przy defaultowym min_tx_count 15 i multiplier 1.5 daje to ok. 22 tx.

### 7.2. Fazy analizy

AnalysisPhase:
- EarlyStage
- FullAnalysis

Komentarz w state.rs mowi o EarlyStage dla tx_count < 2, ale orchestrator
uzywa praktycznie gatekeeper_min_tx_count * early_stage_multiplier. To jest
drift dokumentacji wewnatrz starego silnika.

EarlyStage:
- pomija czesc trend/microstructure modulow wymagajacych historii
- ma unikac falszywych negatywow na zbyt mlodych poolach
- nadal moze uzywac MPCF, LIGMA, Cluster, PRAECOG, QEDD/MCI bez abortu,
  SurvivorScore i czesc fallbacks

FullAnalysis:
- aktywuje SSMI/SCR/ULVF/POVC/MESA oraz czesc veto QEDD/MCI
- uruchamia pelniejszy zestaw kar i boostow

### 7.3. Veto / fail-fast w orchestratorze

ClusterHunter veto:
- jesli cluster risk_score przekracza cabal_risk_threshold, wynik to
  score 0, VeryHigh, passed=false

LIGMA veto:
- liczone zawsze, jezeli enabled
- korzysta z candidate i explicit/cached pool state
- veto dla liquidity_trap_risk > veto_trap_threshold
- veto dla psi_ligma < veto_psi_ligma_threshold

FRE veto:
- Fractal Resonance Engine analizuje swapy, gdy jest co najmniej 10 early swaps
- FractalAction::Skip zwraca score 0 i VeryHigh

QEDD/MCI veto:
- tylko FullAnalysis
- QEDD lambda > abort threshold moze zabic kandydata
- MCI coherence < abort threshold moze zabic kandydata

Survivor critical cutoff:
- jezeli SurvivorScore spadnie ponizej survivor_critical_threshold,
  orchestrator robi SKIP zanim wyda dalszy pozytywny verdict


## 8. Sygnaly zbierane przez HyperPrediction


### 8.1. Base score

orchestrator.rs wywoluje legacy score_enhanced(candidate, threshold).
To jest prosty adapter z ghost-brain/src/oracle/scoring.rs:
- startuje z bazowego score 50
- dodaje za wysoka liquidity
- dodaje za bardzo wczesny bonding curve
- odejmuje za pozny bonding curve
- dodaje za rozsadna podaz
- odejmuje za brak/niska liquidity
- ryzyko wylicza przez deprecated RiskLevel::from_candidate
- final score dostaje kare RiskLevel::penalty()

Ten score jest pomocniczy/fallbackowy i nie powinien byc traktowany jako
obecna semantyka Gatekeepera.

### 8.2. ShadowLedger diagnostics

score_pool() i orchestrator moga czytac:
- virtual_sol_reserves
- token_total_supply
- real_token_reserves
- bonding_curve_progress
- expected price / shadow price ratio

Te dane byly uzywane do diagnostyki i starych fal scoringowych, ale w obecnym
systemie ShadowLedger nie powinien stac sie kanoniczna prawda Gatekeeper policy.

### 8.3. QASS compatibility

qass_stub.rs mowi wprost:
- QASS zostal zastapiony przez SurvivorScore
- QuantumAmplitudeScorer::score() zawsze zwraca QASSResult::default()
- default QASSResult: score 0.5, score_100 50, confidence 0.0,
  is_valid false, data_source Synthetic

wave_builder_stub.rs zwraca neutral/default waves albo puste wektory.

W calculate_final_score QASS moze dac drugorzedny modifier tylko gdy:
- qass_result.is_valid == true
- confidence > qass_min_confidence_for_modifier

Formula:
(score_100 - 50) / 50 * qass_secondary_max_adjustment

Przy aktualnym stubie ten modifier jest praktycznie 0.

### 8.4. SSMI

Plik: ghost-brain/src/oracle/ultrafast/ssmi.rs

SSMI rozszerza SCR o Shannon entropy inter-arrival jitter:
- liczy entropy timestampow
- liczy AR correlation
- laczy entropy, SCR i AR
- klasyfikuje SourceType: Bot, Human, ViralLaunch albo Unknown

Progi:
- bot_scr_threshold ok. 0.7
- bot_ar_threshold ok. 0.8
- bot_entropy_threshold ok. 1.5
- human_entropy_threshold ok. 3.0
- viral wymaga co najmniej 6 tx i entropy w przedziale ok. 2.5..4.0

W final scoring:
- Bot w full analysis daje kare -15 * ssmi_bot_penalty_mult
- ViralLaunch w full analysis daje +10 * ssmi_viral_boost_mult
- Human w full analysis daje +5 * ssmi_human_boost_mult

### 8.5. MPCF

Plik: ghost-brain/src/oracle/ultrafast/mpcf.rs

MPCF analizuje raw transaction bytes:
- byte morphology
- entropy signature
- instruction spacing signature

ActorType obejmuje m.in.:
- HumanMobile
- HumanDesktop
- SniperScript
- MEVArb
- LiquidityBot
- RpcFiller
- SybilBot
- Unknown

W final scoring:
- SniperScript lub MEVArb daje kare -10 * mpcf_sniper_penalty_mult
- SybilBot daje kare -20 * mpcf_sybil_penalty_mult

W praktyce MPCF wymaga raw tx bytes. W wielu sciezkach legacy tx_bytes jest
None, wiec modul bywa niewykorzystany mimo obecnosci w API.

### 8.6. IWIM

Plik: ghost-brain/src/oracle/ultrafast/iwim.rs

IWIM analizuje creator/dev wallet:
- Lightning CTP: burst/quiet detection, authority chain
- CMM: creator micro-movement model
- CDIS: aggregate intent signature

IwimResult:
- organic_score
- sybil_score
- rug_threat_score
- confidence
- execution_time_us

W HyperPrediction:
- IWIM moze dawac kare za rug_threat_score > 0.8 lub > 0.6
- sybil_score > 0.6 daje kare
- organic_score > 0.7 daje boost

W score_pool() IWIM jest ustawione na None, bo komentarz mowi, ze Gatekeeper
V2 Phase 5 przejal dev behavior inline. W nowszym aktywnym pipeline IWIM jest
post-Gatekeeper veto, a nie legacy scoring base.

### 8.7. PRAECOG

Plik: ghost-brain/src/oracle/ultrafast/praecog.rs

PRAECOG jest adversarial simulation / counterfactual guard:
- symuluje attack paths
- mierzy minimalny kapital do crashu
- mierzy sandwich feasibility
- liczy adversarial vulnerability score

Domyslnie:
- fast params w early stage
- thorough/default params w full analysis
- target <250 us przy 256 attack paths

Orchestrator preferuje explicit_pool_state, potem PumpCurveStateCache.

### 8.8. MESA

MESA microstructure analyzer mierzy:
- wash_likeness
- bot_likeness
- organic_likeness
- entropy_score
- impact_efficiency
- tx_count

W final scoring full analysis:
- wash > 0.85: -25 * wash_penalty_mult
- wash > 0.70: -12 * wash_penalty_mult
- bot > 0.90: -15 * bot_penalty_mult
- bot > 0.75: -8 * bot_penalty_mult
- organic > 0.75 i wash < 0.40: +8 * mesa_organic_boost_mult
- entropy > 0.80 i wash < 0.50: +5 * mesa_entropy_boost_mult

### 8.9. QEDD i MCI

QEDD/MCI dostaja MarketSignals z orchestratora.
MarketSignals sa budowane m.in. z:
- buy pressure / SOBP
- volume
- wallet count
- resonance risk
- ULVF divergence
- entropy z SSMI/MPCF
- volume signals
- price momentum

QEDD:
- daje survival estimate / lambda
- w full analysis moze veto przez lambda abort threshold
- w SurvivorScore zasila survival component

MCI:
- daje coherence
- w full analysis moze veto przez coherence abort threshold
- moze uczestniczyc w followup penalties

### 8.10. QMAN

QMAN korzysta z wallet_energy_tracker, transition_matrix i unitary_evolution.
Score zaczyna od neutralnego ok. 0.5:
- AllInMainTrend: +0.3
- PrepareSecondWave: +0.2
- ExitNow: -0.25
- net energy flow moze dodac/odjac ok. 0.15
- confidence dampens score do neutralnego, gdy niska

QMAN trafia do SurvivorScore jako momentum i exit-risk source.

### 8.11. ClusterHunter

ClusterAnalysis wnosi:
- risk_score
- metrics.cluster_count
- top/wallet concentration metrics zalezne od implementacji ClusterHunter

W orchestratorze moze zrobic veto przy wysokim cabal risk.
W penalties:
- risk_score > 0.5 daje proporcjonalna kare do ok. 15 * cluster_penalty_mult
W boosters:
- cluster_count == 0 daje +5 * cluster_clean_boost_mult

### 8.12. ResonanceDetector

ResonanceDetector analizuje wzorce czasowe z timestampow.
W scoring:
- bot-like pattern: -15
- suspicious pattern: -8
- human-like pattern: +5 * resonance_human_boost_mult

### 8.13. Gene Mapper

Gene safety result wnosi risk_level:
- Critical: -50
- High: -30
- Medium: -15
- Low: -5
- Safe: brak kary

### 8.14. ChaosEngine

Plik: ghost-brain/src/chaos/engine.rs

ChaosEngine to Monte Carlo AMM simulation:
- domyslnie 10 000 symulacji
- 5 random whale actions per simulation
- base trade amount 1% reserves
- rayon parallel
- scenariusze: Bullish, Bearish, RugPull, Mixed, Chaotic
- wynik: crash_probability, pump_probability, median_roi, p5_roi,
  p95_roi, mean_price_change, price_volatility, num_simulations,
  execution_time_ms, avg_time_per_sim_us

W score_pool() ChaosEngine odpala sie tylko gdy tx_metrics sa realne.
W scoring:
- crash_probability > 50: -20 * chaos
- crash_probability > 30: -10 * chaos
- median_roi < -10: -15 * chaos
- median_roi < 0: -5 * chaos
- pump_probability > 60: +15 * chaos_pump_boost_mult
- pump_probability > 40: +8 * chaos_pump_boost_mult

### 8.15. Paradox

ParadoxState jest zewnetrznym sygnalem z off-chain Seer ParadoxSensor.
Nie jest twardym veto w signals/paradox.rs; jest opisany jako informational.
W orchestratorze moze ustawic:
- should_delay_entry
- recommended_delay_ms

Reguly delay:
- wysokie phase_sync i tension moga zasugerowac opoznienie
- echo spike moze zasugerowac umiarkowane opoznienie
- domyslny delay bywa ok. 3000 ms

### 8.16. SecondWaveDetector

SecondWaveDetector jest uruchamiany, gdy Paradox sugeruje delay albo w full
analysis. Jego rola to odroznic natychmiastowy entry od wejscia po drugiej
fali albo po ustabilizowaniu ruchu.

### 8.17. FRE

Pliki:
- ghost-brain/src/oracle/ultrafast/fre/engine.rs
- ghost-brain/src/oracle/ultrafast/fre/math.rs

FRE liczy:
- Hurst exponent przez R/S analysis
- fractal roughness
- scale coherence
- FSW: fractal stability window
- STT: scale-transition test
- ARB: asymmetric risk bias

FractalAction:
- Buy
- Watch
- Skip

W orchestratorze:
- Skip moze byc veto
- Buy moze boostowac wynik
- Watch moze obnizyc score

### 8.18. ECTO / BVA / PANIC / TCR-Phi / CIR

Te moduly sa behavioral scoring layer, czesciowo w starym engine.rs:

ECTO:
- Early Chrono-Trade Observer
- 0-7s genesis window
- sygnal, nie final decision
- flags: DEV_SOLD, SNIPER_WALL, RETAIL_SWARM, BUY_STREAK

BVA:
- Behavioral Vacuum Analysis
- bootstrapped value assessment dla pierwszych 0-7s
- korzysta z on-chain behavioral metadata, bez price/reserves/microstructure
- metryki: TDS, DC, SE, CER, ERP
- klasyfikacja: Organic, Steered, Chaotic, Dormant

PANIC:
- aktywne okno ok. 7s
- liczy panic/bot pressure na podstawie zdarzen
- moze ograniczac score w engine.rs

CIR:
- Causal Impact Ratio
- event-time based causal impact scoring
- ocenia, czy transakcja powoduje reakcje niezaleznych aktorow w scislych
  oknach czasowych

TCR-Phi:
- Temporal Causality Resonance
- mierzy, jak reakcje alignuja sie czasowo i kierunkowo z impactem
- moze wykrywac intentional steering i causal breaks

Te sygnaly trafiaja do SurvivorScoreInput jako behavioral fields albo do
engine.rs jako modulatory cyklicznego score.


## 9. SurvivorScore


Plik: ghost-brain/src/oracle/survivor_score.rs

SurvivorScore jest nowszym rdzeniem starego HyperPrediction, ktory zastapil
QASS jako glowna metoda scoringu. Naglowek mowi, ze formuła koncepcyjna to:

(Survival x Momentum x Quality) x (1 - RiskDiscount)

Aktualna implementacja domyslnie uzywa jednak scoringu addytywnego, a stara
wersja multiplikatywna jest pod flaga srodowiskowa:
GHOST_LEGACY_SCORING=true.

### 9.1. Survival

Survival laczy:
- QEDD survival
- IWIM threat
- Cluster risk

W cycle version IWIM bywa celowo niewidoczny w survival, zeby uniknac skokow
score w trakcie cyklu. Finalna wersja calculate_with_iwim() uwzglednia IWIM.

Przy final include:
- QEDD ok. 50%
- IWIM ok. 30%
- Cluster ok. 20%

Przy cycle/no-IWIM:
- QEDD ok. 62.5%
- Cluster ok. 37.5%
- IWIM display neutral

### 9.2. Momentum

Momentum laczy:
- SOBP
- QMAN
- Chaos pump probability

Nowszy zakres momentum jest szeroki:
- SOBP mapowany mniej wiecej z [-0.8, 3.0] do [0.2, 4.0]
- QMAN ok. [0.5, 2.5]
- Chaos ok. [0.3, 2.5]
- geometric mean clamp ok. [0.2, 4.0]

### 9.3. Quality

Quality laczy:
- MPCF organic ratio
- MESA organic likeness
- 1 - SCR
- unique_wallet_ratio
- LIGMA blended score

Typowe wagi bazowe:
- MPCF 0.35
- MESA 0.25
- SCR 0.20
- wallets 0.20

LIGMA moze byc blendowane osobna waga z configu.

### 9.4. Risk discount

Risk discount bierze:
- mesa_wash_likeness
- qman_exit_signal
- price_crash_detected
- paradox_anomaly
- ECTO rug

Hard veto:
- ECTO rug
- Paradox anomaly
- price crash

### 9.5. Additive score

calculate_score_additive:
- suma survival_points + momentum_points + quality_points
- optional excellence boost
- penalty amplification
- odejmuje risk penalty
- clamp 0..100

### 9.6. Multiplicative rollback

calculate_score_multiplicative:
- uzywane tylko przy GHOST_LEGACY_SCORING=true
- mnozy survival, momentum i quality z wagami
- aplikuje (1 - risk_discount)
- skaluje ok. *80

### 9.7. Behavioral modulator

Jezeli wlaczony, scoring moze uzyc:
- ECTO
- BVA
- 1 - PANIC
- TCR
- CIR

Modulator moze byc addytywny albo ograniczony multiplikatywny, zaleznie od
configu.


## 10. Final scoring: penalties, boosters, risk, passed


Plik: ghost-brain/src/oracle/hyper_prediction/scoring/mod.rs

calculate_final_score() robi:

1. Pobierz base z SurvivorScore albo fallback base_score.
2. Zastosuj QASS secondary modifier tylko gdy QASS valid i confidence
   przekracza prog.
3. Zastosuj fallback confidence multiplier.
4. Zastosuj penalties bez clampowania, score moze zejsc ponizej zera.
5. Zastosuj boosters bez clampowania, score moze wyjsc powyzej 100.
6. RiskLevel okresl z raw boosted score:
   - <20: VeryHigh
   - <40: High
   - <60: Medium
   - >=60: Low
7. Display score clamp do 0..100.
8. passed = survivor_passed && display_score >= threshold.

To rozroznienie raw score vs display score jest wazne: stary silnik zachowuje
informacje o ekstremalnie zlych albo ekstremalnie dobrych sygnalach wewnatrz,
ale UI/logiczny display jest 0..100.


## 11. Cyclic HyperPrediction i Patient Observer


### 11.1. cyclic_hyper_predictor.rs

CyclicHyperPredictor implementuje 12 cykli S1-S12 po ok. 400 ms:
- S1-S6: static / early
- S7-S12: full analysis

Kazdy cykl:
- uruchamia evaluate_cycle()
- stosuje TCF modulation
- zapisuje CycleScoreRecord
- moze uruchomic Gunshot early buy, gdy score przekracza prog cyklu

Final:
- weighted geometric mean
- porownanie z progiem ok. 82
- final HyperPredictionResult nadpisywany wynikiem cyklicznym

Wazna luka:
evaluate_cycle() przekazuje do score_candidate_impl wiele None. To znaczy,
ze cykliczny predictor ma forme architektonicznej symulacji/kompatybilnosci,
ale nie zawsze ma pelne dane, ktore idealny HyperPrediction zaklada.

### 11.2. predator_strategy.rs

PredatorStrategy opisuje strategię S1-S12:
- gatekeeper duration ok. 1780 ms
- min tx ok. 15
- early cycle threshold ok. 6
- full analysis tx threshold ok. 23
- cycle weights:
  [1.3, 1.7, 2.2, 2.8, 3.6, 4.6, 6.0, 7.8, 10.0, 13.0, 17.0, 22.0]
- gunshot thresholds:
  [100, 99, 98, 97, 96, 95, 88, 87, 86, 85, 83.5, 82]

Early quality formula:
0.44*MPCF + 0.31*MESA + 0.25*wallet

Full quality formula:
0.35*MPCF + 0.25*MESA + 0.20*(1-SCR) + 0.20*wallet

### 11.3. score_history.rs

ScoreHistory / Patient Observer to osobny mechanizm:
- 38 cykli przez ok. 15s
- ok. 400 ms interwaly
- przechowuje CycleScore
- liczy trend
- liczy weighted average z recency/confidence
- wykrywa second wave
- final decision moze byc Enter, Skip albo Continue zalezne od trendu,
  progu, confidence i momentum

Przykladowe reguly:
- Enter przy second wave near threshold
- Enter przy score >= threshold i trend rising/stable z confidence
- Skip przy falling below threshold
- Skip przy critical low <30
- Skip przy braku momentum po wielu cyklach
- Skip przy niskiej confidence

### 11.4. engine.rs

ghost-brain/src/oracle/engine.rs to wiekszy cyclic prediction engine S1-S12.
Opisuje:
- 420 ms heartbeat
- ok. 5.04s total
- Sniping Mode S1-S2
- Stabilization S3-S7
- Final Verdict S8-S12
- early exit Gunshot

Engine uzywa:
- SnapshotEngine
- ShadowLedger
- SurvivorScore
- QEDD
- HyperOracle
- MESA
- Chaos
- SOBP
- MPCF
- CIR
- BVA
- PANIC
- TCR-Phi
- TCF

Istotne detale:
- GHOST_IWIM_ENABLED kontroluje IWIM integration
- warmup live min wymaga kolejnych live snapshots
- BVA wczesne okno moze blokowac MESA/CHAOS albo blendowac SCR prior
- PANIC moze capowac base_score
- CIR moze decydowac, czy chaos simulation ma sens
- final verdict po S12 uzywa wazonego podejscia i SurvivorScore

To jest mocno rozbudowana alternatywna/cykliczna warstwa starego silnika.
Nie nalezy jej mylic z aktywnym Gatekeeper V2/V2.5 materialization path.


## 12. Followup scoring


Plik: ghost-brain/src/oracle/followup_scoring.rs

Followup scoring mial monitorowac token po initial BUY:
- intervale: 1s, 5s, 30s, 60s
- mci_drop_threshold default ok. 0.35
- qedd_lambda_spike_threshold ok. 2.0
- qedd_survival_drop_pct ok. 0.50
- chaos_loss_prob_threshold ok. 0.60
- gene_match_threshold ok. 0.70
- exit_threshold ok. 40
- score_drop_pct ok. 0.30

Decyzje:
- HOLD
- SELL
- SCALE_OUT

Wazne:
kod ma charakter placeholder/demo. Komentarze wskazuja, ze production
integration powinna pobierac swieze SnapshotEngine/QEDD/MCI/Chaos/Gene.
Nie traktowac tego jako kompletnego live post-buy risk managera.


## 13. ParadoxSensor poza ghost-brain


Pliki:
- off-chain/components/seer/src/paradox_sensor/mod.rs
- off-chain/components/seer/src/paradox_sensor/types.rs

ParadoxSensor:
- trzyma ring buffer NetworkPulse
- record_pulse(size) zapisuje timestamp Instant i size_bytes
- run_analysis_loop() co ok. 50 ms liczy stan
- publikuje ParadoxState przez watch channel

NetworkPulse:
- timestamp
- size_bytes

ParadoxState:
- tension
- jitter_ms
- density_bps
- anomaly_detected
- derivative
- phase_sync
- pds_score
- is_echo_spike

Formula:
- bierze inter-arrival times
- liczy mean IAT
- liczy jitter jako standard deviation
- density = pulses * (1000 / window_ms)
- tension_raw = density^1.1 / (jitter + 1)
- tension_normalized = tension_raw / 50, clamp do 100
- anomaly_detected gdy tension > anomaly threshold
- derivative = zmiana tension w czasie, clamp -1..1
- phase_sync z FFT-like phase detector
- echo spike gdy derivative > 0.5, phase_sync > 0.7 i tension > 70
- PDS = 0.45*tension + 0.25*positive_derivative*100
        + 0.20*phase_sync*100 + 0.10*echo_flag*100

Wiring:
- WebSocket path wywoluje record_pulse(text.len())
- gRPC path przechowuje sensor jako _paradox compat, ale statyczna inspekcja
  nie pokazala aktywnego record_pulse w gRPC
- ghost-launcher/src/components/seer.rs wystawia watch::Receiver<ParadoxState>
- launcher podlacza receiver do OracleRuntime
- score_pool() moze pobrac ParadoxState z paradox_rx

Znaczenie dla scoringu:
- Paradox nie powinien byc hard veto sam w sobie
- w legacy orchestratorze moze sugerowac delay
- w SurvivorScore moze wejsc jako paradox_anomaly / risk-veto


## 14. OraclePipeline w Triggerze


Plik: ghost-launcher/src/components/oracle_pipeline.rs

OraclePipeline zawiera:
- HyperPredictionOracle
- QuantumAmplitudeScorer
- HyperOracle
- PumpCurveStateCache
- ShadowLedger
- metrics
- optional TelemetryRecorder

score_candidate(pool):
- konwertuje DetectedPool do EnhancedCandidate
- odpala HyperPredictionOracle task z timeoutem
- rownolegle odpala placeholder workers:
  VisionCritic, ClusterHunter, DevProfiler
- agreguje wyniki do EnhancedScoringResult

Komentarz nadal wymienia QASS jako czesc historycznego pipeline, ale konstruktor
ma jasny komentarz: QASS jest deprecated, SurvivorScore jest primary scoring
system, QASS zostaje tylko dla compatibility i neutralnych wartosci.

Trigger klasyfikuje embedded OraclePipeline jako compatibility-only.


## 15. Legacy event schema i logging


### 15.1. PoolScoredEvent

PoolScoredEvent jest w ghost-launcher/src/events.rs, nie w nowym
ghost-brain/src/events schema. To wazne, bo nowy ghost-brain event schema nie
modeluje aktywnego "PoolScored" jako glowny event decyzyjny.

### 15.2. DecisionLogger legacy references

ghost-brain/src/oracle/decision_logger.rs zawiera struktury dla starych
oracle logs:
- InitialComponents
- FollowupScore
- OracleDecisionLog
- CorrectionReason::QassScoreDrop
- VetoType::Gene/Qedd/Mci/Shadow/Guardian

Te elementy sa czescia historii starego pipeline. Nie oznaczaja, ze QASS jest
obecnie glownym scoringiem.

### 15.3. Telemetry

ghost-brain/src/telemetry/recorder.rs nadal ma kompatybilnosciowe pola dla
HyperPrediction. QASS bywa ustawiany na None albo neutralny.


## 16. Konfiguracja


### 16.1. [hyper_prediction]

ghost-brain/ghost_brain_config.toml:
- survivor_critical_threshold = 35
- qass_secondary_max_adjustment = 10
- qass_min_confidence_for_modifier = 0.6
- cold_start_max_adjustment = 0.3
- cold_start_qedd_mci_weight = 10.0
- mesa_wash_severe_threshold = 0.85
- mesa_wash_elevated_threshold = 0.70
- mesa_bot_high_threshold = 0.90
- mesa_bot_moderate_threshold = 0.75
- mesa_organic_bonus_threshold = 0.75
- mesa_organic_max_wash = 0.40
- mesa_entropy_bonus_threshold = 0.80
- mesa_entropy_max_wash = 0.50
- min_volume_scale = 0.0001
- relative_factor_cap = 2.0
- burst_normalization = 2.0

### 16.2. [hyper_prediction.risk_thresholds]

- very_high_confidence = 0.5
- high_confidence = 0.7
- medium_score = 60

### 16.3. [hyper_prediction.followup_scoring]

- mci_drop_threshold = 0.35
- qedd_survival_drop_pct = 0.50
- enable_followup_penalties = true

### 16.4. [scoring]

Signal weights:
- ligma = 1.0
- qedd = 1.0
- survivor = 1.0
- qass_secondary = 1.0
- mci = 1.0
- cluster = 1.2
- chaos = 1.0

Penalty multipliers:
- wash_penalty_mult = 1.3
- bot_penalty_mult = 1.2
- rug_penalty_mult = 1.0
- cluster_penalty_mult = 1.4
- ssmi_bot_penalty_mult = 1.0
- scr_penalty_mult = 1.0
- ulvf_div_penalty_mult = 1.0
- ulvf_curl_penalty_mult = 1.0
- povc_penalty_mult = 1.2
- mpcf_sniper_penalty_mult = 1.2
- mpcf_sybil_penalty_mult = 1.3

Boost multipliers:
- organic_boost_mult = 1.2
- smart_money_boost_mult = 1.0
- ssmi_viral_boost_mult = 1.1
- ssmi_human_boost_mult = 1.1
- mesa_organic_boost_mult = 1.1
- mesa_entropy_boost_mult = 1.0
- chaos_pump_boost_mult = 1.0
- resonance_human_boost_mult = 1.1
- cluster_clean_boost_mult = 1.2
- povc_organic_boost_mult = 1.0

### 16.5. [tcf]

TCF:
- enabled = false
- weight_in_final_verdict = 0.15
- tcf_min_modulation = 0.6
- tcf_modulation_range = 0.4
- decay_factor = 0.85
- min_updates_for_primed = 3

### 16.6. [paradox]

- enabled = true
- window_size_ms = 500
- analysis_interval_ms = 50
- anomaly_tension_threshold = 80.0
- max_samples = 2000
- min_samples_for_analysis = 10

Wazna luka:
ghost-brain config ma ParadoxConfig, ale off-chain ParadoxSensor uzywa stalych
w module. Statyczna inspekcja nie potwierdzila pelnego wiring config -> sensor.


## 17. Modul inventory


### 17.1. ghost-brain/src/oracle/hyper_prediction

- mod.rs:
  publiczne API HyperPredictionOracle i re-exporty.

- orchestrator.rs:
  glowny score_candidate_impl, fazy, veto, sygnaly, skladanie wyniku.

- state.rs:
  HyperPredictionResult, AnalysisPhase, QmanResult, TcfResult.

- config.rs:
  HyperPredictionConfig, FollowupScoringConfig, Survivor thresholds,
  RiskMultipliers, OrchestratorThresholds.

- verdict.rs:
  deprecated OracleDecision, RiskLevel, RiskThresholds, FinalVerdict.

- cyclic_hyper_predictor.rs:
  S1-S12 cyclic predictor, Gunshot, TCF modulation.

- utils.rs:
  helpery scoring/orchestrator.

- scoring/mod.rs:
  calculate_final_score.

- scoring/penalties.rs:
  kary dla MESA, SSMI, MPCF, SCR, ULVF, POVC, IWIM, Cluster, Chaos,
  Resonance, Gene.

- scoring/boosters.rs:
  boosty dla SSMI, POVC, MESA, IWIM, Cluster, Chaos, Resonance.

- scoring/weights.rs:
  ScoringWeights i mapowanie configu.

- signals/builders.rs:
  budowanie MarketSignals i danych pomocniczych.

- signals/qedd.rs:
  QEDD signal adapter.

- signals/mci.rs:
  MCI signal adapter.

- signals/ligma.rs:
  LIGMA signal adapter.

- signals/cluster.rs:
  Cluster signal adapter.

- signals/paradox.rs:
  Paradox informational adapter.

- ARCHITECTURE.md:
  historyczny opis architektury HyperPrediction.

### 17.2. ghost-brain/src/oracle/ultrafast

- ssmi.rs:
  sub-slot microentropy, bot/human/viral timing classifier.

- mpcf.rs:
  raw-byte actor fingerprinting.

- iwim.rs:
  dev-wallet intent mapping.

- sobp.rs:
  bucket-over-bucket buying pressure.

- praecog.rs:
  adversarial pool exploitability simulation.

- qass_stub.rs:
  deprecated neutral QASS compatibility.

- wave_builder_stub.rs:
  deprecated neutral wave builders.

- panic.rs:
  short-window panic pressure state.

- market_anomaly.rs:
  failed ratio, fee spike, frantic signers.

- cir.rs:
  causal impact ratio.

- tcr_phi.rs:
  temporal causality resonance.

- signer_entropy.rs:
  entropy tracker dla signer distribution.

- fre/mod.rs, fre/engine.rs, fre/math.rs:
  fractal resonance engine.

### 17.3. ghost-brain/src/chaos

- engine.rs:
  Monte Carlo simulation engine.

- amm_math.rs:
  AMM pool and swap simulation math.

- distributions.rs:
  buyer profiles and market actions.

- flowfield.rs / field_analysis.rs / whf_signals.rs:
  dodatkowe chaos/flow/risk sygnaly.

### 17.4. ghost-launcher

- oracle_runtime.rs:
  aktywny Gatekeeper runtime plus deprecated score_pool helper.

- events.rs:
  PoolScoredEvent i RuntimePlane::LegacyObservation.

- components/oracle_pipeline.rs:
  compatibility HyperPrediction pipeline.

- components/trigger/component.rs:
  legacy PoolScored side-effect blocking.

### 17.5. off-chain Seer

- paradox_sensor/mod.rs:
  network pulse analyzer.

- paradox_sensor/types.rs:
  NetworkPulse i ParadoxState.

- websocket_connection.rs:
  record_pulse(text.len()) dla websocket frames.

- grpc_connection.rs:
  _paradox compat storage, bez znalezionego aktywnego record_pulse.


## 18. Co faktycznie budowalo decyzje w starym engine


Legacy decyzja nie powstawala z jednego modelu ML. To byl zestaw heurystyk,
symulatorow i modulow ryzyka skladany w kilku etapach:

1. Dane bazowe tokena:
   liquidity, bonding curve, supply, pool identity, timestamps.

2. Dane przeplywu:
   tx_count, buy/sell volume, unique signers, buy pressure, windows.

3. Timing:
   SCR, SSMI, Resonance, inter-arrival deltas.

4. Struktura transakcji:
   MPCF raw bytes, actor inference, instruction spacing.

5. Mikrostruktura:
   MESA wash/bot/organic/entropy/impact.

6. Topologia rynku:
   ULVF divergence/curl, POVC cluster.

7. Symulacje:
   Chaos Monte Carlo i PRAECOG adversarial paths.

8. Dev/creator intent:
   IWIM, w nowszym aktywnym systemie przesuniety do Gatekeeper/IWIM veto.

9. Wallet/cabal:
   ClusterHunter risk i clean cluster boost.

10. Behavioral early window:
    ECTO, BVA, PANIC, CIR, TCR-Phi.

11. Trend cohesion:
    TCF i FRE.

12. Network telemetry:
    Paradox tension/phase/echo, glownie jako delay/risk signal.

13. Final score:
    SurvivorScore -> QASS secondary no-op/compat -> fallback multiplier
    -> penalties -> boosters -> raw risk -> display clamp -> threshold.


## 19. Najwieksze luki i ryzyka przy ewentualnym odrodzeniu


### 19.1. Legacy code exists, but active path is elsewhere

Najwieksze ryzyko organizacyjne to pomylenie:
"kod istnieje i kompiluje sie" z "kod jest aktywnym decision source".

HyperPredictionOracle jest inicjalizowany w launcherze i trzymany w
OracleRuntime, ale statyczne dowody pokazuja, ze produkcyjna sciezka decyzji
idzie przez Gatekeeper materialization, a score_pool jest test-only.

### 19.2. QASS residue

Repo nadal ma nazwy QASS w configu, metrykach, telemetry i helperach.
Jednak qass_stub zwraca neutral, wave_builder_stub zwraca neutral/puste fale,
a final scoring traktuje QASS tylko jako secondary modifier. Przy projektowaniu
nowej oceny nie nalezy odbudowywac QASS jako glownego modulatora bez osobnej,
jawnej decyzji architektonicznej i walidacji.

### 19.3. POVC cluster mismatch

Komentarze scoringu mowia o cluster 0..3, ale HyperOracle centroid selection
wydaje sie zwracac 0..2. To moze martwic, bo penalty dla cluster 3 moze byc
martwy, a semantyka cluster labels moze byc niespojna.

### 19.4. Cykliczne sciezki maja duzo None inputs

CyclicHyperPredictor i OraclePipeline czesto wywoluja score_candidate z
brakujacymi optional inputs. Wyniki moga byc bardziej fallbackowe niz sugeruje
architektura.

### 19.5. Followup scoring jest placeholderem

Followup scoring ma ambicje post-buy monitoringu, ale komentarze mowia o TODO
production integration. Nie powinien byc traktowany jako gotowy manager ryzyka.

### 19.6. Paradox config drift

Config ma [paradox], ale off-chain ParadoxSensor ma stale. Brak potwierdzonego
pelnego wiring config -> sensor.

### 19.7. Dokumenty historyczne moga byc stale

Niektore ghost-brain docs opisuja HyperPrediction jako production. Kod i
aktualny pipeline Gatekeeper wskazuja, ze to jest przynajmniej czesciowo stale.
Przy sporze code > stare docs.

### 19.8. MaterializedFeatureSet nie moze zostac ominiety

Ewentualne wykorzystanie starych sygnalow musi je przeniesc przez obecny SSOT:
PoolObservationSession::materialize_features() -> MaterializedFeatureSet.
Nie wolno pozwolic, by stary HyperPrediction czy score_pool czytal mutable
live state rownolegle do Gatekeeper policy jako drugi autorytet.


## 20. Rekomendacja dla Mr. Guru


Nie przywracac starego HyperPrediction jako monolitycznego decision engine.
Rozsadniejsza strategia:

1. Potraktowac legacy silnik jako biblioteke kandydackich sygnalow.
2. Wybrac sygnaly, ktore maja jasny, obserwowalny zwiazek z zyskiem:
   - MESA wash/bot/organic
   - SOBP momentum
   - Cluster concentration
   - LIGMA tradability/trap
   - IWIM dev behavior
   - SCR/SSMI timing only after calibration
   - Chaos/PRAECOG tylko jesli maja koszt/runtime budget i walidacje
3. Kazdy sygnal materializowac w jednym miejscu, przez SSOT, bez live-state
   reads w policy.
4. Nie laczyc wszystkiego naraz w jeden score 0..100 bez kalibracji.
5. Najpierw zbudowac replay dataset:
   - decyzja time T
   - sygnaly dostepne na T
   - wynik ekonomiczny po horyzontach 5s/15s/60s
   - koszt false BUY i false REJECT
6. Oddzielic:
   - hard veto
   - soft evidence
   - diagnostics only
   - delay/observe signals
7. Zachowac Gatekeeper jako active decision shell, dopoki nie zapadnie jawna
   decyzja o architekturze nowego silnika.
8. Jezeli legacy scoring ma byc reaktywowany, najpierw naprawic:
   - POVC label mismatch
   - Paradox config wiring
   - followup production TODO
   - optional inputs w cyclic predictor
   - QASS residue
   - runtime proof, ze PoolScored nie odblokowuje live BUY
9. Każdy nowy scoring musi byc explainable:
   - reason code
   - komponenty score
   - snapshot wejscia
   - replay determinism
   - no hidden mutable state


## 21. Checklist pokrycia prosby


- Uwzgledniono ghost-brain jako glowny obszar starego engine.
- Uwzgledniono oracle_runtime.rs i disabled/test-only score_pool.
- Uwzgledniono HyperPredictionOracle.
- Uwzgledniono HyperOracle SCR/ULVF/POVC.
- Uwzgledniono SurvivorScore.
- Uwzgledniono scoring penalties/boosters/weights.
- Uwzgledniono SnapshotEngine.
- Uwzgledniono ChaosEngine.
- Uwzgledniono ParadoxSensor poza ghost-brain.
- Uwzgledniono PoolScored, RuntimePlane i Trigger side-effect block.
- Uwzgledniono OraclePipeline compatibility path.
- Uwzgledniono cyclic predictor, engine.rs, ScoreHistory i followup scoring.
- Uwzgledniono QASS tylko jako deprecated/secondary compatibility, nie jako
  glowny modulator oceny.
- Oddzielono aktywna sciezke Gatekeeper V2/V2.5 od legacy scoringu.

Koniec raportu.
