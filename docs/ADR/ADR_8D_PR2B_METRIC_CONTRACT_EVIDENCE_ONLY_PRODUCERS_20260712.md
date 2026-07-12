# ADR-8D: PR2B evidence-only producers i atomowa projekcja kontraktów metryk

Status: `IMPLEMENTED / VALIDATED / READY_FOR_REVIEW`

Typ: ADR-8D / cross-cutting SSOT, evidence, materialization i resource contract

Data: 2026-07-12

Repo: `smahacfel/Gho`

Branch: `agent/metric-contract-pr2b-evidence-producers`

Base: `55a9c5ce306e3a0c4fceb85015f5b796567073c3`

Plan normatywny:
`PLANS/DO_REALIZACJI/PLAN_KOREKTY_KONTRAKTOW_INTERPRETACJI_METRYK_V1_20260710.md`

Poprzedni etap:
`docs/ADR/ADR_8D_PR2A_METRIC_CONTRACT_PARITY_SENSITIVE_PRODUCERS_20260712.md`

Raport dowodowy:
`reports/metric_contracts/pr2b_evidence_only_producers_verification_v1.md`

Poziom ryzyka: `HIGH`. Zmiana przecina czterech canonical owners, pełny evidence
root, compact projection oraz terminalną materializację MFS. Ryzyko policy jest
ograniczone przez rollout `Legacy`, niezmienione authority assignments, brak
odczytu nowej projection przez Gatekeeper oraz fail-closed typed materialization.

## 1. Kontekst

PR2A dostarczył sześć rodzin parity-sensitive, lecz celowo nie tworzył
niekompletnego root projection. PR2B domyka pozostałe cztery rodziny:

- Flip V2;
- manipulation numeric presence i derived flags;
- reserve velocity typed evidence;
- recent buy/sell typed evidence.

Od tego etapu każdy poprawny terminalny snapshot bieżącego buildu musi zawierać
kompletną dziesięciorodzinną projection. Historyczne rekordy bez nowego pola
pozostają rozróżnialne jako `None`.

## 2. Decyzja architektoniczna

Jedynym dozwolonym przepływem jest:

```text
canonical per-family producers
→ jeden immutable frozen producer input set
→ MetricContractsEvidenceSetV1
→ pure MetricContractDecisionEvidenceProjectionV1
→ MaterializedFeatureSet.metric_contract_decision_projection_v1
```

Pełny evidence set jest lokalnym wynikiem granicy builda. Nie jest kopiowany do
MFS, nie jest zapisywany do pliku i nie tworzy sidecara. Projection nie czyta
raw transakcji, sesji, indeksów, owner state ani zegara. Konwersja przyjmuje
wyłącznie full evidence oraz frozen context: rollout, Profile A,
effective-config i decision cutoff.

`MaterializedFeatureSet` zawiera dokładnie:

```rust
#[serde(default)]
pub metric_contract_decision_projection_v1:
    Option<MetricContractDecisionEvidenceProjectionV1>
```

Semantyka:

- historyczny brak pola deserializuje się do `None`;
- bieżąca skuteczna materializacja tworzy pełne `Some`;
- błąd producenta, configu, evidence, projection lub resource gate zwraca
  `MetricContractMaterializationErrorV1` na aktywnej terminalnej ścieżce;
- nie istnieje częściowe `Some` ani `None` jako fallback bieżącego buildu;
- błąd nie jest mapowany na Gatekeeper verdict, reason, phase ani soft point.

## 3. Canonical owners

| Rodzina | Canonical owner | Nowa reprezentacja | Legacy behavior |
| --- | --- | --- | --- |
| Flip V2 | istniejący `TxIntelligenceEngine` / fingerprint owner | owner-state full evidence + compact aggregates | `flip_ratio_10s` bez zmian |
| Manipulation | frozen V3 materialization owner | 7 presence-aware numeric fields + 6 derived flags | `ManipulationContradictionFeatures` i V3 v1 bez zmian |
| Reserve velocity | `AccountStateReducer` | typed status, reserves, interval, count, receive-time clock | legacy scalar SOL/s bez zmian |
| Recent buy/sell | istniejący RCE recent-window owner | counts, legacy scalar, unbounded ratio, bounded share | legacy scalar bez zmian |

Każdy producer jest wywoływany jeden raz w terminalnej materializacji. Static
call-count guard obejmuje również sześć rodzin PR2A i pojedyncze wywołanie
complete buildera. Test pełnego boundary dowodzi deterministycznej równości
`full evidence → projection` z projection przechowywaną w wyniku granicy.

## 4. Flip V2

Flip V2 jest bounded automatem per owner osadzonym w istniejącym
`TxIntelligenceEngine`. Eligible event wymaga success, non-dust, canonical
window, resolved owner, present slot oraz stable identity/order. Signature jest
preferowana; fallback wymaga `slot + transaction_index` albo
`slot + event_ordinal`. Receive order nie jest canonical order.

Stany:

```text
no_anchor
tracking
flipper
closed_non_flipper
```

Pierwszy eligible BUY tworzy niezmienny anchor. Pre-anchor SELL nie jest
retroaktywny. Kolejne BUY i SELL używają checked cumulative arithmetic.
Pierwszy SELL spełniający jednocześnie wall-clock, slot-gap i dump-ratio zamraża
`flipper`. Denominator liczy ownera raz, a aggregate ratio jest zawsze w 0..1.

Dedupe FIFO, event state i owner set są ograniczone konfiguracją. Dedupe
eviction usuwa także odpowiadający event i degraduje wynik fail-closed. Wallet
cap jest egzekwowany przed zapisaniem nowego ownera. Reconnect/gap, capacity
loss, sprzeczny canonical order i overflow tworzą unavailable/non-evaluable
snapshot z typed reasons i telemetryką eviction.

## 5. Manipulation presence-aware evidence

Full evidence zawiera dokładnie siedem pól:

```text
same_ms_tx_ratio
bundle_suspicion_ratio
top3_signer_volume_ratio
hhi
max_tx_per_signer
dev_volume_ratio
contradiction_score
```

`Null` nie jest zerem. Legacy/default zero pozostaje w oddzielnym legacy field
set z `LegacyDefault`; explicit measured zero pozostaje `Value(0)`. Maska
`measured_fields_mask` odpowiada wyłącznie polom o jakości `Measured` albo
`Degraded`. Group quality jest górnym ograniczeniem jakości pola.

Sześć derived high flags zachowuje field ID, raw value/status, comparator,
threshold, wynik, exact policy-stage/version i effective-config hash. Comparator
tej wersji to strict `GreaterThan`; equality jest false. Brak raw value daje
`Null`, nigdy false. Projection zachowuje wyłącznie pola numeryczne i maski;
pełne derived provenance pozostaje w full evidence.

## 6. Reserve velocity

`AccountStateReducer` jest jedynym właścicielem typed snapshotu. Session nie
rekonstruuje velocity z MFS scalar. Owner zwraca jawnie:

- `Measured` — co najmniej dwa canonical updates, dodatni interval, exact
  reserves/formula i finite SOL/s;
- `FirstUpdate` — current reserve obecne, brak interval/value;
- `ZeroDeltaTime` — reserves obecne, interval zero, brak measured value;
- `BootstrapFallback` — owner-known bootstrap, brak measured value;
- `Unavailable` — brak owner evidence albo fail-closed arithmetic state.

Źródłem czasu jest wyłącznie `receive_time`; fallback i first update nie mogą
udawać measured zero. Pełny evidence i compact validator niezależnie sprawdzają
count/presence/formula parity.

## 7. Recent buy/sell

Canonical owner korzysta z successful-only RCE window z inclusive start/end.
Snapshot zachowuje checked counts oraz liczbę odrzuconych failed events.

```text
transaction_count = buy_count + sell_count

legacy:
  transaction_count == 0 → null
  sell_count == 0        → buy_count
  otherwise              → buy_count / sell_count

buy_to_sell_ratio:
  sell_count == 0 → null
  otherwise       → buy_count / sell_count

buy_share:
  transaction_count == 0 → null
  otherwise               → buy_count / transaction_count
```

Nowa surface pozostaje `LoggingOnly`, `NonPolicy` i nieactionable. Static guard
potwierdza brak consumera w aktywnych plikach Gatekeeper policy i V3.

## 8. Effective-config coverage

Zamknięta tabela `PR2B_EFFECTIVE_CONFIG_KEY_BOUNDARIES_V1` wyprowadza oczekiwany
zbiór z `METRIC_EFFECTIVE_CONFIG_KEYS_V1` dla czterech kontraktów PR2B. Każdy
klucz jest przypisany dokładnie raz.

`FrozenProducerBoundaryValidated`:

- `FlipCandidateDustThresholdSol`;
- `FlipCandidateDedupeKey`;
- `FlipCandidateDedupeCapacity`;
- `FlipCandidateEvictionPolicy`;
- `FlipCandidateMaxWallets`;
- `FlipCandidateReconnectBehavior`.

Pozostałe PR2B keys są `CompactValidated`, ponieważ ich wartości są
reprezentowane przez aggregate fields albo są stałą semantyką zamkniętej
compact schema. Family builders sprawdzają te same frozen context values przed
utworzeniem evidence; `validate_context()` i validated hash sprawdzają je
ponownie bez dostępu do producer state.

## 9. Compact schema i resource gate

Projection zachowuje zatwierdzone exact field sets. Nie zawiera owner IDs,
anchorów, qualifying sells, event identities, raw collections, full field
lists ani derived provenance arrays. Wszystkie dziesięć family keys jest
wymagane, a unknown/partial root jest odrzucany.

Canonical hash nadal używa canonical JSON i jest dostępny wyłącznie przez
`validated_canonical_hash(context)`. Resource measurement i hard gate używają
deterministycznej binarnej reprezentacji serde/bincode tego samego typed root.
To rozróżnienie jest jawne: pełna, opisowa postać JSON ma większy narzut nazw
pól, natomiast limit 12/16 KiB dotyczy transportowo zwartej reprezentacji
projection. Oversized bounded-reason payload jest odrzucany przed hashowaniem
przez `ProjectionTooLarge`.

Family builders zachowują fail-closed public context validation, natomiast
root builder używa prywatnej, typowo ograniczonej ścieżki dla już
zwalidowanego immutable contextu. Eliminuje to ponowne liczenie hashy profile i
effective-config dla każdego compact envelope bez osłabienia publicznej
granicy. Release resource harness mierzący dokładnie full-evidence → projection
build + semantic validation uzyskał p50 `509 us`, p95 `753 us` i p99 `874 us`.
Standardowy payload ma `2780 B`, a duży dozwolony fixture `8932 B`.

## 10. Zakres wyłączony

Bez zmian pozostają Gatekeeper V2/V2.5 i V3 policy, thresholds, weights,
phases, soft points, verdicts, reasons, aktywny dev source, DecisionLogger v33,
V3 replay v1, selector score, IWIM, post-buy, sender, Jito, execution oraz
live/shadow boundary. Rollout pozostaje `Legacy`.

PR2B nie implementuje v34, sidecara, writera, comparatora, replayu v2, audit
CLI, burn-in, Type-5, DualCompute, V2 rollout ani PR3 cutover.

## 11. Konsekwencje

Pozytywne:

- MFS ma jeden atomowy, kompletny i replay-safe compact contract snapshot;
- pełne evidence i compact projection pochodzą z tych samych frozen inputs;
- brak pola historycznego nie jest mylony z measured zero;
- owner/event detail nie obciąża MFS;
- config drift, arithmetic loss i partial evidence są fail-closed.

Koszt:

- terminal materialization może zwrócić typed error zamiast emitować snapshot;
- runtime wykonuje dodatkową pure validation i canonical-hash proof;
- pełna durability evidence pozostaje świadomie odłożona do PR2C.

## 12. Walidacja i decyzja końcowa

Pełna macierz PR2A/PR2B, Gatekeeper V2/V2.5/V3, session lifecycle, logger,
replay v1, checks, targeted Clippy, rustfmt i diff checks przeszła. Jedyny
uruchomiony failure to niezmieniony baseline
`test_selector_shadow_score_filters_non_finite_feature_values`; jego owner file
`decision_logger.rs` ma identyczny SHA-256 i pusty diff względem base.

Zamrożone pliki policy, V3, loggera, replayu, IWIM, sendera, post-buy, Jito i
execution mają identyczne SHA-256 jak base. Rollout pozostaje `Legacy`.
PR2C, PR3 oraz Type-5 T1 nie zostały rozpoczęte.

Markery akceptacyjne:

```text
PR2B_FLIP_V2_STATE_MACHINE_PASS
PR2B_MANIPULATION_PRESENCE_AWARE_EVIDENCE_PASS
PR2B_RESERVE_VELOCITY_TYPED_EVIDENCE_PASS
PR2B_RECENT_BUY_SELL_TYPED_EVIDENCE_PASS
PR2B_EFFECTIVE_CONFIG_KEY_COVERAGE_CLOSED
PR2B_ONE_PRODUCER_ONE_SNAPSHOT_TWO_REPRESENTATIONS_PASS
PR2B_MFS_ATOMIC_PROJECTION_MATERIALIZATION_PASS
PR2B_PROJECTION_RESOURCE_GATE_PASS
GATEKEEPER_POLICY_UNCHANGED
V3_V1_REPLAY_UNCHANGED
TYPE5_NOT_STARTED
METRIC_CONTRACTS_V1_1_EVIDENCE_PRODUCERS_READY
PR2B_READY_FOR_REVIEW
```
