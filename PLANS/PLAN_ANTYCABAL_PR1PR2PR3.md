Poniżej przesyłam poprawioną, kompletną treść pierwszych 3 zadań / PR-ów po uwzględnieniu Twoich uwag.

Granica jest twarda:

GO:
  PR1 — Canonical coordination sample substrate
  PR2 — Metric evidence model
  PR3 — Stats primitives

NO-GO:
  implementacja konkretnych metryk
  scoring
  Gatekeeper penalty
  reject threshold
  size-down
  full JSONL evidence
  replay L2 / Phase B
  SFD / FTDI / DBIA / CPV / CTC / CPCR / ETC / CUCD

Ten tor powinien być równoległy i neutralny względem bieżących prac L1R13/R16-r10 oraz route/BCV2. Coordination-risk stack nie rozwiązuje problemu bonding_curve_v2, więc nie powinien ani blokować tej ścieżki, ani być z nią mieszany.


---

Zakres etapu: Coordination Risk Foundations — PR1–PR3 only

Celem tego etapu nie jest wdrożenie antysybilowego scoringu. Celem jest przygotowanie inertnych fundamentów, które później pozwolą liczyć wszystkie metryki na tym samym sample’u, z tym samym modelem evidence/confidence/degraded status i tym samym modułem statystyk.

Na tym etapie nie zmieniamy aktywnej polityki BUY.

Nie zmieniamy Gatekeepera.

Nie dodajemy nowych penalty.

Nie dodajemy rejectów.

Nie dodajemy size-down.

Nie aktywujemy żadnej metryki.

Nie reaktywujemy HyperPrediction / legacy scoring.

Zakres PR1–PR3:

PR1:
  wspólny kanoniczny model próbek transakcyjnych

PR2:
  wspólny model evidence / confidence / status / degraded reasons

PR3:
  wspólny moduł statystyk: HHI, MAD, robust CV, Tau-b

Dopiero po zakończeniu shadow burnin Gatekeepera V3 można przejść do implementacji właściwych metryk.


---

PR1 — Canonical coordination sample substrate

Cel

Celem PR1 jest zbudowanie kanonicznego substratu próbek dla przyszłych metryk coordination-risk.

Każda przyszła metryka ma liczyć na tym samym obiekcie wejściowym, a nie parsować raw gRPC, raw transaction meta albo session events po swojemu.

Najważniejsza zasada:

ObservedBuyTx jest internal sample substrate.
Nie jest ciężkim payloadem MaterializedFeatureSet.
Nie jest automatycznie serializowany do każdego decision row.
Nie wpływa na Gatekeepera.

Na tym etapie ObservedBuyTx ma służyć jako wewnętrzny, deterministyczny model danych dla przyszłych metryk i testów. Później do MaterializedFeatureSet powinny trafiać wyłącznie zagregowane CoordinationRiskFeatures, nie pełne listy próbek.


---

Zakres PR1

Dodać moduł:

features/coordination/samples.rs
features/coordination/types.rs

Dodać typy:

pub struct ObservedBuyTx {
    pub signature: Signature,

    pub pool_id: Pubkey,
    pub mint: Pubkey,
    pub signer: Pubkey,

    pub slot: u64,
    pub slot_index: Option<u64>,

    pub tx_elapsed_ms_from_pool_create: Option<u64>,
    pub t0_source: Option<T0Source>,
    pub tx_time_source: Option<TxTimeSource>,

    pub is_success: bool,
    pub is_buy: bool,
    pub is_sell: bool,
    pub is_dev: bool,
    pub is_create_or_init_tx: bool,
    pub is_unknown_direction: bool,

    pub account_keys_resolved: SmallVec<[Pubkey; 64]>,

    pub outer_ix_count: Option<u8>,
    pub inner_ix_group_count: Option<u8>,

    pub fee_lamports: Option<u64>,

    pub pre_balance_signer: Option<u64>,
    pub post_balance_signer: Option<u64>,

    pub decoded_buy_sol_lamports: Option<u64>,
    pub curve_sol_delta_lamports: Option<u64>,
    pub economic_spent_lamports: Option<EconomicSpend>,

    pub tokens_received: Option<u64>,

    pub price_before: Option<f64>,
    pub price_after: Option<f64>,

    pub compute_units_consumed: Option<u64>,
    pub cost_units: Option<u64>,

    pub fee_topology_fp: Option<FeeTopologyFingerprint>,
    pub execution_template_fp: Option<ExecutionTemplateFingerprint>,
    pub capital_template_fp: Option<CapitalTemplateFingerprint>,
}


---

Korekta względem pierwotnego planu: slot_index jako Option<u64>

W pierwotnym planie slot_index było traktowane jako wymagane. To jest dobre dla idealnego runtime, ale zbyt ostre dla replayu, historycznych danych albo nietypowych źródeł.

Poprawka:

pub slot_index: Option<u64>

Zasada:

slot_index wymagany dla sequence metrics:
  DES
  BSE
  inne metryki przyczynowo-sekwencyjne

slot_index niewymagany dla non-sequence metrics:
  SFD
  FTDI
  CTC
  ETC
  CUCD
  DBIA

Jeżeli slot_index jest niedostępny:

DES/BSE = None + MissingSlotIndex

ale pozostałe metryki mogą dalej działać.

Zakaz:

Nie wolno fallbackować do receiver arrival time jako kolejności przyczynowej.

Arrival time może być logowany diagnostycznie, ale nie może być używany do wnioskowania przyczynowego.


---

Korekta nazwy czasu

Nie używać nazwy:

observed_ms_from_create

bo może sugerować pre-execution observation albo mempool/intention.

Zamiast tego:

pub tx_elapsed_ms_from_pool_create: Option<u64>
pub t0_source: Option<T0Source>
pub tx_time_source: Option<TxTimeSource>

Przykładowe enumy:

pub enum T0Source {
    PoolCreateTx,
    FirstObservedPoolAccountUpdate,
    FirstObservedBuy,
    ReplayFixture,
    Unknown,
}

pub enum TxTimeSource {
    SlotIndex,
    BlockTime,
    LocalReceiverTimeDiagnosticOnly,
    ReplayFixture,
    Unknown,
}

Ważne założenie:

SubscribeUpdateTransaction jest post-execution evidence.
Nie jest mempool/intention evidence.

To ma znaczenie dla przyszłych DES/BSE i replayu.


---

Typy pomocnicze

Dodać typy, ale bez implementowania metryk:

pub enum EconomicSpendSource {
    DecodedPumpInstruction,
    CurveRealSolDelta,
    SignerDeltaMinusKnownOverheads,
}

pub struct EconomicSpend {
    pub lamports: u64,
    pub source: EconomicSpendSource,
    pub confidence: f64,
}

Fingerprinti można dodać jako shell typów, ale bez aktywnego obliczania metryk:

pub struct FeeTopologyFingerprint {
    pub external_fee_count: u8,
    pub internal_fee_count: u8,
    pub external_amount_pattern_hash: u16,
    pub has_wsol_self_flow: bool,
    pub has_create_ata_flow: bool,
}

pub struct ExecutionTemplateFingerprint {
    pub compute_budget_shape: u8,
    pub outer_program_sequence_hash: u16,
    pub inner_program_sequence_hash: u16,
    pub inner_instruction_count_bucket: u8,
    pub account_role_pattern_hash: u16,
    pub fee_topology_hash: u16,
    pub ata_wsol_shape: u8,
}

pub struct CapitalTemplateFingerprint {
    pub pre_balance_bucket: u16,
    pub residual_bucket: u16,
    pub overhead_bucket: u8,
}

Na PR1 te typy nie muszą jeszcze być wypełniane produkcyjnie. Mogą istnieć jako przyszły kontrakt danych.


---

Funkcje selection helpers

Dodać funkcje, ale bez podpinania ich do aktywnego MaterializedFeatureSet i bez wpływu na Gatekeepera.

pub fn build_observed_buy_txs(
    session: &PoolObservationSession,
) -> SmallVec<[ObservedBuyTx; 32]>;

Jeżeli nie chcemy jeszcze dotykać realnej sesji w runtime, wariant bezpieczniejszy na PR1:

pub fn build_observed_buy_txs_from_fixture(
    fixture: &CoordinationSampleFixture,
) -> SmallVec<[ObservedBuyTx; 32]>;

Docelowe helpery:

pub fn unique_first_buys_by_signer(
    txs: &[ObservedBuyTx],
) -> SmallVec<[&ObservedBuyTx; 16]>;

pub fn sequence_buys(
    txs: &[ObservedBuyTx],
) -> Result<SmallVec<[&ObservedBuyTx; 32]>, SequenceBuildError>;


---

Zasady sample selection

Dla przyszłych metryk przekrojowych używać:

first successful buy per unique signer

Dla przyszłych metryk sekwencyjnych używać:

successful buy sequence sorted by (slot, slot_index)

Do sample’u nie powinny wejść:

failed tx
sell tx
unknown direction tx
dev CreatePool/init tx jako zwykły buyer sample
transakcje bez pewnej klasyfikacji buy

Reguła:

if !tx.is_success {
    exclude;
}

if tx.is_sell || tx.is_unknown_direction {
    exclude;
}

if tx.is_dev && tx.is_create_or_init_tx {
    exclude_from_buyer_sample;
}

Dev create/init może być później użyte do DBIA tylko w specjalnym trybie DevFingerprintMode, ale nie jako zwykły buyer buy.


---

Determinizm sortowania

Dla sequence metrics:

(slot, slot_index)

jest jedyną dopuszczalną kolejnością przyczynową.

Jeżeli slot_index == None, sequence metrics mają zwrócić błąd:

SequenceBuildError::MissingSlotIndex

albo przyszłościowo:

MetricValue.status = Unavailable / InsufficientSample
reason = MissingSlotIndex

Dla non-sequence metrics można działać bez slot_index.


---

Lekki debug/export

Można dodać lekki summary typ:

pub struct CoordinationSampleSummary {
    pub total_txs_seen: u16,
    pub successful_buy_txs: u16,
    pub unique_buyers: u16,
    pub excluded_failed: u16,
    pub excluded_sell: u16,
    pub excluded_unknown_direction: u16,
    pub excluded_dev_create_or_init: u16,
    pub missing_slot_index_count: u16,
    pub missing_compute_units_count: u16,
    pub missing_balance_count: u16,
}

Ten typ może być używany w testach albo diagnostyce, ale nie powinien automatycznie nadmuchiwać aktywnego decision JSONL.


---

Out of scope PR1

Nie robić w PR1:

SFD
FTDI
DBIA
CPV
CTC
CPCR
ETC
CUCD
DES
BSE
CoordinationRiskFeatures w MaterializedFeatureSet
Gatekeeper penalty
reject threshold
size-down
full JSONL evidence


---

Acceptance criteria PR1

PR1 jest zaakceptowany, jeżeli:

1. ObservedBuyTx istnieje jako internal canonical sample substrate.

2. Żadna przyszła metryka nie musi parsować raw gRPC po swojemu.

3. Sample selection jest deterministyczny.

4. first buy per signer jest deterministyczny.

5. failed / sell / unknown / dev-create są poprawnie wykluczane z buyer sample.

6. sequence_buys sortuje po (slot, slot_index).

7. Brak slot_index blokuje tylko sequence metrics, nie cały sample substrate.

8. Receiver arrival time nie jest używany jako kolejność przyczynowa.

9. ObservedBuyTx nie trafia automatycznie do MaterializedFeatureSet jako ciężki payload.

10. PR1 nie zmienia Gatekeepera, scoringu, penalty, rejectów ani size-down.


---

PR2 — Metric evidence model

Cel

Celem PR2 jest stworzenie wspólnego modelu evidence dla przyszłych metryk.

Najważniejsze rozróżnienie:

metric value
severity
confidence
coverage
sample_n
evidence status
degraded/unavailable reasons

Bez tego metryki będą mylić:

niska wartość
brak danych
niedostępna warstwa
zbyt mały sample
clean pass
degraded evidence

To jest szczególnie ważne dla FSC:

FSC unavailable != FSC 0.0

Brak funding lane oznacza brak obserwacji, a nie dowód organiczności.


---

Zakres PR2

Dodać moduł:

features/coordination/evidence.rs
features/coordination/config.rs

Dodać:

pub struct MetricValue {
    pub value: f64,
    pub severity: f64,
    pub confidence: f64,
    pub sample_n: u8,
    pub coverage: f64,
    pub status: MetricEvidenceStatus,
    pub degraded_reasons: SmallVec<[DegradedReason; 4]>,
}


---

Evidence status

Dodać status, nie tylko degraded_reasons.

pub enum MetricEvidenceStatus {
    Clean,
    Degraded,
    Unavailable,
    InsufficientSample,
    NotConfigured,
    ExportOnly,
}

Semantyka:

Clean:
  metryka ma pełne dane i może być interpretowana normalnie.

Degraded:
  metryka ma dane, ale część evidence jest słabsza albo fallbackowa.

Unavailable:
  źródło danych nie istnieje albo warstwa jest niedostępna.

InsufficientSample:
  dane istnieją, ale sample jest zbyt mały.

NotConfigured:
  metryka albo feature jest wyłączony konfiguracyjnie.

ExportOnly:
  metryka może być liczona/logowana, ale nie może wpływać na scoring.

Przykład FSC w obecnym stanie:

funding_source_concentration = None;
funding_visibility = FundingVisibility::Unavailable;
status = MetricEvidenceStatus::Unavailable;
reason = DegradedReason::FundingLaneUnavailable;

Nie wolno robić:

funding_source_concentration = Some(MetricValue {
    value: 0.0,
    status: Clean,
    ...
});


---

DegradedReason

Dodać enum:

pub enum DegradedReason {
    InsufficientBuys,
    InsufficientUniqueSigners,

    MissingMeta,
    MissingSlotIndex,
    MissingComputeUnits,
    MissingCostUnits,
    MissingPrePostBalances,
    MissingCurveState,
    MissingEconomicSpend,

    MissingDevBuy,
    DevTxNotComparable,

    RollingStateUnavailable,
    RollingStateNotWarm,

    FundingLaneUnavailable,

    LowCoverage,
    SameSlotDominated,

    ZeroOrInvalidMean,
    AllXTies,
    AllYTies,
    DenominatorZero,

    NotConfigured,
    ExportOnly,
}


---

FundingVisibility

Dodać:

pub enum FundingVisibility {
    Available,
    Unavailable,
    Warmup,
}

Na obecnym etapie:

FundingVisibility::Unavailable

bo funding lane nie jest aktywny.

Zasada:

FundingVisibility::Unavailable
nie daje positive.
nie daje penalty.
nie jest clean pass.


---

Severity jako metric-local

Dodać jasną semantykę:

severity = jak bardzo zła jest wartość tej konkretnej metryki
           zgodnie z kierunkiem tej konkretnej metryki.

Przykłady:

low SFD  -> high severity
high CTC -> high severity
low FTDI -> high severity
high CPV -> high severity
low CUCD -> high severity
high DBIA -> high severity

Nie wolno globalnie interpretować value bez znajomości kierunku metryki.

Dodać helper enum / descriptor:

pub enum MetricBadDirection {
    LowIsBad,
    HighIsBad,
}

Dodać helpery:

pub fn severity_low(value: f64, threshold: f64) -> f64 {
    if threshold <= 0.0 {
        return 0.0;
    }

    ((threshold - value) / threshold).clamp(0.0, 1.0)
}

pub fn severity_high(value: f64, threshold: f64) -> f64 {
    if threshold >= 1.0 {
        return 0.0;
    }

    ((value - threshold) / (1.0 - threshold)).clamp(0.0, 1.0)
}

Te helpery mogą istnieć w PR2, ale nie mogą jeszcze być użyte do aktywnego scoringu Gatekeepera.


---

CoordinationRiskFeatures shell

Można dodać shell typu, ale nie podpinać go jeszcze do MaterializedFeatureSet.

pub struct CoordinationRiskFeatures {
    pub funding_visibility: FundingVisibility,

    pub fee_topology_diversity_index: Option<MetricValue>,
    pub dev_buyer_infra_affinity: Option<MetricValue>,
    pub spend_fraction_divergence: Option<MetricValue>,
    pub funding_source_concentration: Option<MetricValue>,
    pub signer_cross_pool_velocity: Option<MetricValue>,
    pub demand_elasticity_score: Option<MetricValue>,
    pub buy_sizing_elasticity: Option<MetricValue>,

    pub capital_template_concentration: Option<MetricValue>,
    pub cross_pool_cohort_recurrence: Option<MetricValue>,
    pub execution_template_concentration: Option<MetricValue>,
    pub compute_unit_consumption_dispersion: Option<MetricValue>,

    pub total_coordination_penalty: Option<f64>,
    pub interaction_penalty: Option<f64>,
}

Ale na PR2:

nie wypełniać metryk
nie liczyć penalty
nie integrować z Gatekeeperem
nie integrować z aktywnym MFS

total_coordination_penalty i interaction_penalty powinny na tym etapie być None albo w ogóle pominięte, jeżeli istnieje ryzyko nieporozumienia. Preferowana wersja na PR2:

pub total_coordination_penalty: Option<f64>,
pub interaction_penalty: Option<f64>,

żeby nie sugerować, że scoring już działa.


---

CoordinationRiskConfig shell

Można dodać config shell, ale bez aktywowania metryk.

pub struct CoordinationRiskConfig {
    pub enabled: bool,
    pub export_only: bool,

    pub min_unique_buyers_for_diagnostics: u8,
    pub min_unique_buyers_for_soft_scoring: u8,

    pub funding_visibility: FundingVisibility,
}

Domyślnie:

enabled = false;
export_only = true;
funding_visibility = FundingVisibility::Unavailable;

Jeżeli config jest serializowany:

serde(default)

wszędzie tam, gdzie to potrzebne, żeby legacy rows / stare logi / stare replaye nie psuły parsowania.


---

Brak FSC jako stan niedostępności

W PR2 dodać test lub fixture semantyczny:

let fsc: Option<MetricValue> = None;
let funding_visibility = FundingVisibility::Unavailable;
let reason = DegradedReason::FundingLaneUnavailable;

Acceptance:

brak FSC nie daje positive
brak FSC nie daje penalty
brak FSC nie jest Clean
brak FSC nie jest 0.0


---

N=5–8 jako zasada przyszłego scoringu

PR2 może dodać komentarz / config shell, ale nie scoring.

Zasada projektowa:

N=3–4:
  diagnostic / export-only / interaction candidate only

N>=5:
  soft-risk possible, ale nigdy hard reject przez pojedynczą metrykę

N>=8:
  wyższa confidence

W PR2 można przygotować pola:

pub min_unique_buyers_for_diagnostics: u8,   // default 3
pub min_unique_buyers_for_soft_scoring: u8,  // default 5

Ale nie używać ich jeszcze w Gatekeeperze.


---

Out of scope PR2

Nie robić w PR2:

aktywnych metryk
penalty calculation
interaction penalty
Gatekeeper integration
size-down
reject threshold
JSONL full evidence
SFD/FTDI/DBIA/CPV/CTC/CPCR/ETC/CUCD
DES/BSE calculation


---

Acceptance criteria PR2

PR2 jest zaakceptowany, jeżeli:

1. MetricValue ma status, nie tylko degraded_reasons.

2. MetricEvidenceStatus rozróżnia Clean, Degraded, Unavailable, InsufficientSample, NotConfigured, ExportOnly.

3. FSC disabled jest reprezentowane jako None + FundingVisibility::Unavailable + FundingLaneUnavailable.

4. Brak FSC nie jest interpretowany jako value=0.0.

5. severity jest jasno zdefiniowane jako metric-local.

6. Istnieją helpery severity_low / severity_high, ale nie są używane do scoringu.

7. CoordinationRiskFeatures może istnieć jako shell, ale nie jest podpięty do aktywnego MaterializedFeatureSet.

8. Config shell może istnieć, ale nie aktywuje metryk.

9. Serde/default nie psuje legacy rows ani starych replayów.

10. PR2 nie zmienia Gatekeepera, scoringu, penalty, rejectów ani size-down.


---

PR3 — Stats primitives

Cel

Celem PR3 jest wspólny moduł statystyczny dla przyszłych metryk.

Bez tego każda metryka zacznie implementować HHI, MAD, CV albo Tau-b po swojemu, co utrudni replay, porównanie i debug.

Dodać moduł:

features/coordination/stats.rs

Ten moduł musi być czystą biblioteką:

bez zależności od Solany
bez zależności od Gatekeepera
bez zależności od PoolObservationSession
bez zależności od MaterializedFeatureSet
bez wiedzy o konkretnych metrykach


---

Funkcje do dodania

pub fn normalized_hhi_from_counts(counts: &[u8]) -> Option<f64>;

pub fn median(values: &mut [f64]) -> Option<f64>;

pub fn mad(values: &[f64]) -> Option<f64>;

pub fn weighted_median(values: &[(f64, f64)]) -> Option<f64>;

pub fn weighted_mad(values: &[(f64, f64)]) -> Option<f64>;

pub fn kendall_tau_b(xs: &[f64], ys: &[f64]) -> Option<f64>;

pub fn cv(values: &[f64]) -> Option<f64>;

pub fn robust_cv(values: &[f64]) -> Option<f64>;


---

Krytyczna korekta: HHI denominator

Wzór:

hhi = Σ p_i²
hhi_norm = (hhi - 1 / sample_n) / (1 - 1 / sample_n)

Najważniejsze:

sample_n = sum(counts)
nie counts.len()

To jest krytyczne.

Przykład:

counts = [4, 1]
sample_n = 5

p = [4/5, 1/5]
hhi = 0.8² + 0.2² = 0.68

min_hhi = 1 / sample_n = 1 / 5 = 0.20

hhi_norm = (0.68 - 0.20) / (1.00 - 0.20)
         = 0.60

Błędna implementacja używająca counts.len() = 2 dałaby:

min_hhi = 1 / 2 = 0.50

hhi_norm = (0.68 - 0.50) / (1.00 - 0.50)
         = 0.36

To zaniżyłoby koncentrację i zepsuło FTDI/CTC/ETC.


---

Wymagana implementacja HHI

Semantyka funkcji:

pub fn normalized_hhi_from_counts(counts: &[u8]) -> Option<f64> {
    let sample_n: u64 = counts.iter().map(|&c| c as u64).sum();

    if counts.is_empty() || sample_n < 2 {
        return None;
    }

    let sample_n_f = sample_n as f64;

    let hhi: f64 = counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / sample_n_f;
            p * p
        })
        .sum();

    let min_hhi = 1.0 / sample_n_f;
    let max_hhi = 1.0;

    let denom = max_hhi - min_hhi;
    if denom <= 0.0 {
        return None;
    }

    Some(((hhi - min_hhi) / denom).clamp(0.0, 1.0))
}

Zasady:

counts = []              => None
sample_n < 2             => None
counts = [5]             => Some(1.0)
counts = [1,1,1,1,1]     => Some(0.0)
counts = [4,1]           => Some(0.60)

Dla pojedynczej próbki nie zwracamy Some(1.0), bo jedna próbka nie daje informacji o koncentracji. To ma być obsłużone wyżej jako:

InsufficientUniqueSigners


---

Diversity z HHI

Nie trzeba robić osobnej skomplikowanej funkcji, ale można dodać helper:

pub fn diversity_from_hhi_norm(hhi_norm: f64) -> f64 {
    (1.0 - hhi_norm).clamp(0.0, 1.0)
}

Semantyka:

hhi_norm wysokie  = wysoka koncentracja
diversity wysokie = wysoka różnorodność

Przyszłe użycie:

FTDI = 1 - hhi_norm
CTC  = hhi_norm
ETC  = hhi_norm

Ale PR3 nie implementuje żadnej z tych metryk.


---

Median

Implementacja:

pub fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }

    if values.iter().any(|v| !v.is_finite()) {
        return None;
    }

    values.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let n = values.len();

    if n % 2 == 1 {
        Some(values[n / 2])
    } else {
        Some((values[n / 2 - 1] + values[n / 2]) / 2.0)
    }
}


---

MAD

Median Absolute Deviation:

pub fn mad(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }

    if values.iter().any(|v| !v.is_finite()) {
        return None;
    }

    let mut copy = values.to_vec();
    let med = median(&mut copy)?;

    let mut deviations: Vec<f64> = values
        .iter()
        .map(|v| (v - med).abs())
        .collect();

    median(&mut deviations)
}

MAD powinien być odporny na outliery i będzie później używany dla SFD oraz CUCD.


---

Weighted median / weighted MAD

Weighted median:

pub fn weighted_median(values: &[(f64, f64)]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }

    if values.iter().any(|(v, w)| !v.is_finite() || !w.is_finite() || *w < 0.0) {
        return None;
    }

    let total_weight: f64 = values.iter().map(|(_, w)| *w).sum();

    if total_weight <= 0.0 {
        return None;
    }

    let mut sorted = values.to_vec();

    sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

    let mut cumulative = 0.0;
    let half = total_weight / 2.0;

    for (value, weight) in sorted {
        cumulative += weight;

        if cumulative >= half {
            return Some(value);
        }
    }

    None
}

Weighted MAD:

pub fn weighted_mad(values: &[(f64, f64)]) -> Option<f64> {
    let med = weighted_median(values)?;

    let deviations: Vec<(f64, f64)> = values
        .iter()
        .map(|(v, w)| ((v - med).abs(), *w))
        .collect();

    weighted_median(&deviations)
}

Na tym etapie nie implementujemy jeszcze SFD, ale funkcje będą gotowe.


---

CV

pub fn cv(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }

    if values.iter().any(|v| !v.is_finite()) {
        return None;
    }

    let mean = values.iter().sum::<f64>() / values.len() as f64;

    if mean.abs() < f64::EPSILON {
        return None;
    }

    let variance = values
        .iter()
        .map(|v| {
            let d = v - mean;
            d * d
        })
        .sum::<f64>() / values.len() as f64;

    Some(variance.sqrt() / mean.abs())
}


---

Robust CV

pub fn robust_cv(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }

    if values.iter().any(|v| !v.is_finite()) {
        return None;
    }

    let mut copy = values.to_vec();
    let med = median(&mut copy)?;

    if med.abs() < f64::EPSILON {
        return None;
    }

    let mad_value = mad(values)?;

    Some(1.4826 * mad_value / med.abs())
}

W przyszłości CUCD będzie używać raczej robust_cv jako głównej wartości, a cv jako evidence.


---

Kendall Tau-b

Tau-b musi obsługiwać remisy.

Nie wolno zwracać 0.0, jeżeli statystyka jest nieokreślona.

Warunki None:

len < 3
xs.len() != ys.len()
all x ties
all y ties
denominator zero
non-finite input

Wymagana semantyka:

pub fn kendall_tau_b(xs: &[f64], ys: &[f64]) -> Option<f64> {
    if xs.len() != ys.len() || xs.len() < 3 {
        return None;
    }

    if xs.iter().chain(ys.iter()).any(|v| !v.is_finite()) {
        return None;
    }

    let n = xs.len();

    let mut concordant = 0.0;
    let mut discordant = 0.0;
    let mut ties_x = 0.0;
    let mut ties_y = 0.0;

    for i in 0..n {
        for j in (i + 1)..n {
            let dx = xs[i].partial_cmp(&xs[j])?;
            let dy = ys[i].partial_cmp(&ys[j])?;

            match (dx, dy) {
                (std::cmp::Ordering::Equal, std::cmp::Ordering::Equal) => {
                    // tie on both; excluded from both tie-only counts
                }
                (std::cmp::Ordering::Equal, _) => {
                    ties_x += 1.0;
                }
                (_, std::cmp::Ordering::Equal) => {
                    ties_y += 1.0;
                }
                (std::cmp::Ordering::Less, std::cmp::Ordering::Less)
                | (std::cmp::Ordering::Greater, std::cmp::Ordering::Greater) => {
                    concordant += 1.0;
                }
                _ => {
                    discordant += 1.0;
                }
            }
        }
    }

    let numerator = concordant - discordant;

    let denom_x = concordant + discordant + ties_x;
    let denom_y = concordant + discordant + ties_y;

    if denom_x <= 0.0 || denom_y <= 0.0 {
        return None;
    }

    let denom = (denom_x * denom_y).sqrt();

    if denom <= 0.0 {
        return None;
    }

    Some((numerator / denom).clamp(-1.0, 1.0))
}

Uwaga: w przyszłych DES/BSE przy same-slot gaps będzie dużo remisów. Tau-b ma to obsłużyć, ale jeżeli denominator jest zerowy, wynik ma być None, nie 0.0.


---

Testy PR3

Dodać testy jednostkowe.

HHI tests

#[test]
fn hhi_empty_is_none() {
    assert_eq!(normalized_hhi_from_counts(&[]), None);
}

#[test]
fn hhi_single_sample_is_none() {
    assert_eq!(normalized_hhi_from_counts(&[1]), None);
}

#[test]
fn hhi_all_same_bucket_is_one() {
    assert_approx_eq(normalized_hhi_from_counts(&[5]).unwrap(), 1.0);
}

#[test]
fn hhi_all_unique_is_zero() {
    assert_approx_eq(normalized_hhi_from_counts(&[1, 1, 1, 1, 1]).unwrap(), 0.0);
}

#[test]
fn hhi_four_one_is_point_six() {
    assert_approx_eq(normalized_hhi_from_counts(&[4, 1]).unwrap(), 0.60);
}

MAD / robust CV tests

values = [50_000, 50_000, 50_000, 50_000, 100_000]

cv:
  powinno być wyraźnie podbite przez outlier

robust_cv:
  powinno być blisko 0.0, bo większość klastra jest identyczna

Tau-b tests

Testy obowiązkowe:

len < 3 => None

all x ties => None

all y ties => None

perfect positive monotonic => close to 1.0

perfect negative monotonic => close to -1.0

ties in y but not denominator zero => valid Some(...)

same-slot-like repeated y values => Some albo None zależnie od denominatora, ale nigdy panic


---

Out of scope PR3

Nie robić w PR3:

żadnej metryki
żadnego scoringu
żadnego Gatekeeper wiring
żadnego MFS wiring
żadnego JSONL evidence poza ewentualnymi test fixtures
żadnej interpretacji marketowej


---

Acceptance criteria PR3

PR3 jest zaakceptowany, jeżeli:

1. stats.rs nie ma zależności od Solany, Gatekeepera ani PoolObservationSession.

2. normalized_hhi_from_counts używa sample_n = sum(counts), nie counts.len().

3. HHI testy przechodzą dla [], [1], [5], [1,1,1,1,1], [4,1].

4. MAD i robust_cv obsługują outliery zgodnie z oczekiwaniem.

5. Kendall Tau-b obsługuje ties.

6. Tau-b zwraca None dla len < 3, all x ties, all y ties i denominator zero.

7. Żadna funkcja nie zwraca 0.0 jako zamiennika dla statystycznie nieokreślonego wyniku.

8. PR3 nie implementuje żadnej konkretnej metryki coordination-risk.

9. PR3 nie zmienia Gatekeepera, scoringu, penalty, rejectów ani size-down.


---

Zasady globalne dla PR1–PR3

Twarde GO

GO:
  canonical sample substrate
  evidence/status model
  degraded/unavailable reason model
  funding visibility enum
  pure stats module
  unit tests
  synthetic fixtures
  zero active BUY impact


---

Twarde NO-GO

NO-GO:
  SFD v2
  FTDI v2
  DBIA v2
  CPV v2
  CTC
  CPCR
  ETC
  CUCD
  DES calculation
  BSE calculation
  coordination penalty
  interaction penalty
  Gatekeeper reject threshold
  size-down
  active JSONL full evidence
  MaterializedFeatureSet wiring
  PoolObservationSession::materialize_coordination_risk_features()


---

Najważniejsze decyzje projektowe po korektach

1. ObservedBuyTx jest internal canonical sample substrate, nie ciężki MFS payload.

2. slot_index jest Option<u64>.

3. Brak slot_index blokuje tylko sequence metrics, nie wszystkie metryki.

4. Receiver arrival time nie może być używany jako kolejność przyczynowa.

5. Nazwa czasu to tx_elapsed_ms_from_pool_create, z t0_source i tx_time_source.

6. MetricValue ma MetricEvidenceStatus.

7. FSC unavailable = None + FundingVisibility::Unavailable + FundingLaneUnavailable.

8. FSC unavailable nigdy nie oznacza value=0.0.

9. severity jest metric-local.

10. normalized_hhi_from_counts używa sample_n=sum(counts).

11. Jedna próbka dla HHI daje None, nie koncentrację.

12. Tau-b zwraca None przy nieokreślonej statystyce.

13. PR1–PR3 nie mają żadnego wpływu na Gatekeepera V3 ani bieżący shadow burnin.


---

Finalna decyzja wdrożeniowa

Ten zakres jest bezpieczny:

PR1 — Canonical coordination sample substrate
PR2 — Metric evidence model
PR3 — Stats primitives

Ten zakres odkładamy:

PR4+ — wszystkie realne metryki i integracje

Czyli po tej korekcie plan dla pierwszego etapu brzmi:

Budujemy fundamenty.
Nie liczymy metryk.
Nie score’ujemy.
Nie zmieniamy Gatekeepera.
Nie dotykamy aktywnej polityki BUY.
Nie mieszamy tego z route/BCV2.

To daje solidny, replayowalny i neutralny fundament pod przyszły coordination-risk stack, bez ryzyka zabrudzenia shadow burninu Gatekeepera V3.
