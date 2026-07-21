# ADR-8D: HET-PM V2 executable peak anchor, quote source, peak-arm trailing i forward state revision

Status: `IMPLEMENTED / SHADOW MANAGER`

Typ: ADR-8D / post-buy manager / executable trailing / anchor evidence / peak-arm semantics

Data: `2026-07-20`

Repozytorium: `/root/Gho_dynamic_exit_v1_pr2b`

Uwaga o szablonie: wskazany w globalnych instrukcjach plik
`/root/Gho/docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym środowisku.
Dokument używa lokalnego układu D1--D8 stosowanego w repozytorium.

## D1. Problem

Run `shadow-het-pm-v2-authoritative-20260720-retry8` pokazał, że Trailing
jest znacznie mniej aktywny niż VitalityDecay i AbsoluteMaxHold:

- terminalne shadow exity: `177`;
- `vitality_decay`: `89`;
- `absolute_max_hold`: `73`;
- `executable_trailing`: `6`;
- `stop_loss`: `9`.

Analiza kandydatów wykazała, że znaczna część niskiej aktywności Trailingu
wynikała z charakteru kupowanych tokenów:

- `182` pozycji łącznie;
- tylko `20` pozycji osiągnęło `current_executable_gross_return_bps >= 500`;
- tylko `63` pozycje miały drawdown od executable peak >= `300` bps;
- tylko `9` pozycji miało w tym samym ticku zarówno zysk executable >= `500`
  bps, jak i drawdown od peak >= `300` bps.

Jednocześnie znaleziono wewnętrzny błąd mechaniki anchora:

- `quote_required_on_new_canonical_peak`: `6839`;
- quote resolved: `6755`;
- `anchor_applied`: `204`;
- `anchor_not_applied`: `6635`.

W części przypadków resolved quote wskazywał realnie wyższy executable peak,
ale anchor nie był zapisywany, ponieważ aktualna pozycja miała już wyższy
`state_revision` niż snapshot, dla którego zaplanowano quote.

Efekt praktyczny:

```text
tick materializuje peak i rozwiązuje quote
→ pozycja przesuwa state_revision do przodu w tej samej pętli lifecycle
→ anchor write wymaga dokładnego state_revision
→ świeży executable peak jest odrzucony
→ późniejszy Trailing porównuje się do zaniżonego/starego anchora
→ część pozycji spada do VitalityDecay lub MaxHold zamiast Trailing
```

Kolejny run `shadow-het-pm-v2-authoritative-20260720-retry9` poprawił zapisywanie
anchora, ale ujawnił drugi błąd runtime:

```text
V2/anchor quote plan używał raw_canonical_snapshot.or(latest_snapshot)
→ raw_canonical_snapshot był poprawny dla CrashGuarda
→ ten sam stary sample był używany także dla Trailing/Vitality/anchora
→ quote cell miał klucz starego sample
→ Trailing/anchor szukały klucza świeżego runtime sample
→ quote było formalnie resolved, ale nie dla właściwego gate/source
```

CrashGuard potrzebuje raw canonical provenance. Trailing, Vitality, HardLoss,
AbsoluteMaxHold i executable peak anchor muszą natomiast oceniać bieżącą
wykonywalną ścieżkę. Wspólny fallback raw-first powodował ciche tłumienie
Trailingu przez stare evidence CrashGuarda.

Run `shadow-het-pm-v2-authoritative-20260720-retry10` potwierdził poprawę
quote-source i anchora, ale ujawnił kolejną semantyczną niespójność:

- `37` pozycji osiągnęło peak mark return >= `500` bps i drawdown od peak >=
  `300` bps;
- tylko `19` pozycji miało jednocześnie bieżący mark return po drawdown nadal >=
  `500` bps;
- Trailing był uzbrajany z bieżącego mark return, a nie z peak mark return.

To oznaczało, że pozycja mogła spełnić właściwy model trailing stopu:

```text
osiągnęła profit peak
→ oddała skonfigurowany giveback od tego peak
```

ale zostać zablokowana, jeżeli bieżąca cena po oddaniu zysku spadła poniżej
samego progu uzbrojenia. To było błędne, bo próg uzbrojenia dotyczy historycznie
osiągniętego peak, a nie ceny po drawdown.

## D2. Decyzja

Guard `apply_het_pm_v2_anchor_after_v1()` nie wymaga już dokładnej równości:

```rust
pos.state_revision == key.state_revision
```

Zamiast tego odrzuca wyłącznie quote z przyszłości:

```rust
pos.state_revision < key.state_revision
```

Anchor może zostać zastosowany, gdy pozycja przesunęła się do nowszej rewizji,
ale nadal zgadza się cała tożsamość lifecycle:

- `position_id`;
- `position_epoch`;
- `remaining_token_amount_raw`;
- `route_id`;
- brak terminalnego `last_shadow_outcome`.

W dalszym ciągu nie wolno zastosować anchora, jeżeli quote dotyczy innej ilości,
innej pozycji, innego route albo przyszłej rewizji pozycji.

Dodatkowo quote source selection jest teraz jawne per gate:

- `Crash` używa `raw_canonical_snapshot.or(latest_snapshot)`;
- `HardLoss`, `ExecutableTrailing`, `VitalityDecay` i `AbsoluteMaxHold` używają
  `latest_snapshot.or(raw_canonical_snapshot)`;
- executable peak anchor dodaje quote wyłącznie wtedy, gdy istnieje source o
  dokładnie tym samym `sample_slot` i `sample_timestamp_ms`, które znajdują się
  w żądaniu anchora.

To zamyka klasę błędów, w której quote dla starego raw Crash sample był
przypadkowo używany jako jedyny resolved quote dla niższych gate'ów.

Semantyka uzbrojenia Trailingu została również zmieniona:

- warunek `trailing_arm_mark_return_bps` korzysta z `trajectory.peak_mark_price_sol`
  względem entry value i remaining quantity;
- bieżący mark po drawdown nie może rozbroić Trailingu, jeżeli peak wcześniej
  przekroczył próg aktywacji;
- executable quote nadal jest wymagany do finalnego `ExitAll(ExecutableTrailing)`.

## D3. Granice bezpieczeństwa

Zmiana nie dodaje nowej ścieżki sell authority.

Zmiana może zaplanować osobne quote cells dla różnych source timestampów w tym
samym ticku, nadal w istniejącym bounded `HET_PM_V2_MAX_QUOTE_CELLS`.

Zmiana nie zmienia progów Trailing/Vitality/MaxHold.

Zmiana nie obniża guardów quantity/route/terminal state.

Zmiana dotyczy wyłącznie observer-only executable peak anchor, który jest używany
do decyzji Trailingu. Anchor nadal rośnie monotonicznie po `peak_mark_price_sol`;
ten sam lub niższy peak nie może zastąpić istniejącego anchora.

CrashGuard nie został rozluźniony: jego quote-side confirmation nadal korzysta
z raw canonical source i istniejącej semantyki V1.

Peak-arm fix nie pozwala sprzedać bez executable quote. Zmienia wyłącznie warunek
wejścia Trailingu do ścieżki quote-required: armowanie jest liczone z peak mark,
a końcowa decyzja nadal wymaga resolved executable quote i przechodzi przez
istniejące route/quantity/freshness guardy.

## D4. Konfiguracja

Brak zmian konfiguracji.

Rollback:

```rust
pos.state_revision < key.state_revision
```

przywrócić do:

```rust
pos.state_revision != key.state_revision
```

Rollback jest prosty, ale odtworzy ryzyko utraty świeżych executable peak anchorów
przy normalnym forward progress lifecycle.

Rollback quote-source fixu polega na przywróceniu jednego raw-first source dla
V2 quote planu. Nie jest zalecany, bo odtworzy tłumienie Trailingu/Vitality przez
stare Crash evidence.

## D5. Implementacja

Zmieniony obszar:

- `ghost-brain/src/guardian/post_buy/engine.rs`
- `ghost-brain/src/guardian/post_buy/exit_policy_v2.rs`

Zmiana:

- `apply_het_pm_v2_anchor_after_v1()` akceptuje historical quote dla tej samej
  pozycji, jeżeli bieżąca rewizja pozycji jest nowsza niż rewizja quote;
- `prepare_het_pm_v2_tick()` buduje pełny gate lattice przed quote planem i
  planuje quote dla każdego gate'u z właściwego source;
- `het_pm_v2_quote_source_for_reason()` rozdziela raw canonical source dla
  CrashGuarda od fresh runtime source dla Trailing/Vitality/MaxHold;
- `het_pm_v2_resolved_quote_for_candidate()` finalizuje każdy gate z quote cella
  pasującego do jego własnego source key;
- executable peak anchor wymaga dokładnego source key, zamiast dodawać raw-first
  quote pod kluczem anchora;
- `ExitPolicyV2` posiada helper `trailing_arm_mark_return_bps()`, który liczy
  armowanie Trailingu z `trajectory.peak_mark_price_sol`, entry value i remaining
  quantity;
- prequote i gate lattice używają tej samej peak-arm semantyki;
- test anchora został rozszerzony o trzy przypadki:
  - ten sam/lower peak nie zastępuje istniejącego anchora;
  - nowsza pozycja z tą samą identity/quantity/route akceptuje wyższy historical
    anchor;
  - quote z przyszłej rewizji pozycji jest odrzucany.

## D6. Testy

Wykonane lokalnie:

```text
cargo test -q -p ghost-brain het_anchor_apply_allows_forward_revisions_and_never_moves_down
RUSTFLAGS="-Awarnings" cargo test -q -p ghost-brain het_anchor
RUSTFLAGS="-Awarnings" cargo test -q -p ghost-brain het_trailing_quote_uses_latest_runtime_source_not_stale_crash_source
RUSTFLAGS="-Awarnings" cargo test -q -p ghost-brain trailing
cargo build --release -p ghost-launcher
```

Wynik:

```text
het_anchor: 2 passed; 0 failed
het_trailing_quote_uses_latest_runtime_source_not_stale_crash_source: 1 passed; 0 failed
trailing: 6 passed; 0 failed
release build: passed
```

Test `het_trailing_quote_uses_latest_runtime_source_not_stale_crash_source`
reprodukuje regresję: stary raw sample jest obecny w timeline, świeży runtime
sample jest bieżącą ceną, a Trailing musi dostać resolved quote ze świeżego
sample i zakończyć jako `ExitAll(ExecutableTrailing)`.

Test `executable_trailing_arms_from_peak_mark_return_not_current_return_after_giveback`
reprodukuje drugi błąd: peak przekracza próg armowania, późniejszy current mark
spada poniżej tego progu po giveback, a Trailing nadal musi wejść w ścieżkę
`QuoteRequired(ExecutableTrailing)`.

## D7. Oczekiwana weryfikacja runtime

Kolejny 30-min shadow run powinien pokazać, czy poprawka zwiększa udział
Trailingu w tych pozycjach, które rzeczywiście spełniają warunki:

- executable profit peak >= configured activation;
- executable drawdown od peak >= configured giveback;
- resolved executable quote w ticku decyzji;
- supported route.

Nie oczekuje się, że Trailing stanie się dominującym exit reasonem, jeżeli
kupowane tokeny nie osiągają najpierw wystarczającego executable zysku.

Najważniejsze metryki porównawcze względem `retry8` i `retry10`:

- liczba `quote_required_on_new_canonical_peak`;
- liczba resolved anchor quotes;
- liczba `anchor_applied`;
- udział `current_executable_gross_return_bps` na wszystkich tickach;
- udział resolved quote dla `QuoteRequired`;
- liczba przypadków `anchor_not_applied` przy wyższym executable peak;
- terminalne `selected_execution_reason = executable_trailing`;
- terminalne `vitality_decay` i `absolute_max_hold`.

## D8. Następny krok

Zbudować release binary i uruchomić kolejny 30-min run.

Jeżeli po tej zmianie resolved new peak nadal nie zapisuje anchora, należy
przejść do analizy pozostałych guardów:

- quantity mismatch;
- route mismatch;
- terminal outcome;
- monotonic peak check.

Jeżeli anchor zapisuje się poprawnie, ale Trailing nadal pozostaje niski, przyczyną
jest prawdopodobnie charakter próby tokenów, a nie mechanika HET-PM V2.

Weryfikacja runtime po peak-arm fixie została wykonana w runie
`shadow-het-pm-v2-authoritative-20260720-retry11` na binary:

```text
f1c441920d5ccc52407b0f72be8cd77bc1acc690d754437de5abeaa2d3dfa9f8
```

Wynik:

- launcher report: `PASS`;
- HET observations: `47448`;
- HET positions: `314`;
- writer health: `47448 / 47448`, zero dropów i timeoutów;
- admission health: `1256 / 1256`, zero dropów i błędów;
- quote blockers: brak stale quote blockerów;
- anchor apply: `466 / 469` requestów zliczanych przez analyzer;
- terminalne `executable_trailing`: `26` pozycji;
- gate-level `executable_trailing`: `28` pozycji;
- `peak >= 500 bps` i `drawdown >= 300 bps`: `30` pozycji;
- przypadki spełniające peak/drawdown bez gate quote-required: `2`, z czego
  jeden to realny `anchor_unavailable`, a drugi późniejszy `trajectory_invalid`.

Interpretacja: wcześniejsze masowe tłumienie Trailingu przez stale quote/source
i current-arm zostało usunięte. Pozostał mały edge jednopojedynczej pozycji,
w której peak istniał przed utworzeniem executable anchora i Trailing został
zablokowany przez `anchor_unavailable`.

## D9. Korekta residualu `AnchorUnavailable` przy historycznym peak

Run `retry11` pokazał jeden realny residual po stronie managera:

```text
trajectory zna historyczny peak
trajectory zna późniejszy drawdown
executable_peak_anchor == None
→ ExecutableTrailing = Blocked(AnchorUnavailable)
```

Przyczyną był kontrakt `evaluate_anchor_request()`: anchor quote było tworzone
tylko wtedy, gdy peak był jednocześnie najnowszym samplem. Jeżeli pozycja została
zarejestrowana lub pierwszy tick HET nastąpił już po peak, bounded
`SnapshotTimeline` nadal zawierał historyczny peak, ale anchor request nie
powstawał, bo `peak_sample_timestamp_ms != newest_sample_timestamp_ms`.

Poprawka:

- `materialize_post_buy_snapshot_bundle()` zwraca teraz również exact
  `trajectory_peak_snapshot` znaleziony w tym samym bounded timeline;
- `prepare_het_pm_v2_tick()` wykrywa przypadek:

```text
ExecutableTrailing = Blocked(AnchorUnavailable)
AND anchor_request = NoChange
AND exact trajectory peak snapshot exists
```

- w takim przypadku tworzy `quote_required_on_historical_peak_backfill`;
- anchor quote jest rozwiązywane osobną ścieżką `resolve_shadow_exit_truth_for_anchor()`;
- historyczny anchor quote nie jest przekazywany do policy/finalization quote cells,
  więc nie może zostać użyty jako świeży current exit quote;
- aktualny Trailing exit quote nadal przechodzi zwykły freshness/current-source
  contract;
- pierwszy tick utrwala/backfilluje anchor, a następny tick może podjąć normalną
  decyzję `ExitAll(ExecutableTrailing)`.

To jest celowo bezpieczniejszy wariant niż wykonywanie exitu w tym samym ticku
na anchorze, który jeszcze nie przeszedł przez observer-state apply. Nie dodaje
nowego sell authority i nie rozluźnia current executable quote.

Dodany test:

```text
het_missing_anchor_is_backfilled_from_historical_peak_snapshot_before_next_tick
```

Test reprodukuje dokładny residual:

1. pozycja ma historyczny peak w `SnapshotTimeline`;
2. bieżący sample jest już po drawdown;
3. `executable_peak_anchor` jest pusty;
4. pierwszy tick tworzy i aplikuje historical peak anchor backfill;
5. drugi tick widzi anchor i finalizuje `ExecutableTrailing` jako resolved
   `ExitAll`.

Wykonane testy po korekcie:

```text
RUSTFLAGS="-Awarnings" cargo test -q -p ghost-brain het_missing_anchor_is_backfilled_from_historical_peak_snapshot_before_next_tick
RUSTFLAGS="-Awarnings" cargo test -q -p ghost-brain het_anchor
RUSTFLAGS="-Awarnings" cargo test -q -p ghost-brain trailing
cargo fmt --check
```

Wynik:

```text
het_missing_anchor_is_backfilled_from_historical_peak_snapshot_before_next_tick: 1 passed
het_anchor: 2 passed
trailing: 6 passed
cargo fmt --check: passed
```

## D10. Alignment analyzera z `quote_required_on_historical_peak_backfill`

Pierwszy run kontrolny po D9 (`retry12`) poprawnie wyemitował nowy label
sidecara:

```text
anchor_request = quote_required_on_historical_peak_backfill
```

Runtime był poprawny, ale offline analyzer PR-A odrzucał rekord jako
`invalid anchor_request enum label`, ponieważ jego strict schema znało tylko:

```text
quote_required_on_new_canonical_peak
blocked:<reason>
None
```

Poprawka:

- `het_pm_v2_analysis.py` akceptuje teraz również
  `quote_required_on_historical_peak_backfill`;
- analyzer raportuje oddzielnie:
  - `new_canonical_peak_anchor_request_count`;
  - `historical_peak_anchor_request_count`;
  - łączny `anchor_quote_request_count`;
- dodano test:

```text
test_historical_peak_anchor_backfill_label_is_accepted
```

Wynik retry12 po korekcie analyzera:

```text
retry11:
  trailing_anchor_unavailable_rows = 11
  affected_positions = 1
  later_trailing_exit_after_anchor_block = 0

retry12:
  trailing_anchor_unavailable_rows = 1
  affected_positions = 1
  historical_peak_backfill_rows = 1
  later_trailing_exit_after_anchor_block = 1
```

Interpretacja: residual z retry11 został usunięty w runtime. Jedyny
`AnchorUnavailable` w retry12 był pierwszym tickiem backfillu, po którym
kolejny tick wykonał `ExecutableTrailing`.

Wykonane testy analyzera:

```text
python3 -m py_compile scripts/het_pm_v2_analysis.py scripts/test_het_pm_v2_analysis.py
python3 -m unittest scripts/test_het_pm_v2_analysis.py
```

Wynik:

```text
py_compile: passed
test_het_pm_v2_analysis.py: 30 tests passed
```
