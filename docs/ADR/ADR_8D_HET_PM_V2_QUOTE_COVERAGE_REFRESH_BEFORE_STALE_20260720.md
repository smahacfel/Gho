# ADR-8D: HET-PM V2 quote coverage i refresh przed granicą stale

Status: `IMPLEMENTED / SHADOW MANAGER`

Typ: ADR-8D / post-buy manager / quote evidence / rollout config

Data: `2026-07-20`

Repozytorium: `/root/Gho_dynamic_exit_v1_pr2b`

Uwaga o szablonie: wskazany w globalnych instrukcjach plik
`/root/Gho/docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym środowisku.
Dokument używa lokalnego układu D1--D8 stosowanego w repozytorium.

## D1. Problem

Run `shadow-het-pm-v2-authoritative-20260719-retry6` potwierdził znaczną
poprawę świeżości trajectory:

- `Blocked(TrajectoryStale)` spadło z `18_010` do `0`;
- `trajectory.quality = stale` spadło z `18_373` do `66`;
- `current_executable_gross_return_bps` wzrosło z `884` do `14_215` rekordów.

Pozostał jednak praktyczny problem interpretacji i quote freshness:

1. Dotychczasowe raportowanie mieszało `current_executable_*` względem
   wszystkich ticków z realnym quote-required denominator. All-tick coverage
   `47.05%` wyglądało jak problem managera, mimo że `12_339` rekordów nie
   planowało żadnego quote cell.
2. `3_459` rekordów miało `quote_status = blocked:StaleSnapshot`. Większość
   tych rekordów miała jednocześnie świeżą trajectory (`newest_sample_age_ms=0`),
   więc problem nie był już stale trajectory, tylko opóźniony executable quote
   source względem strict quote stale boundary.
3. Aktywny profil refreshował market state dopiero przy `stale_after_ms=1500`,
   czyli dokładnie na granicy `oracle_stale_hard_ms=1500`. W praktyce refresh
   startował za późno, bo następny monitor tick mógł już odrzucić quote jako
   stale.

## D2. Decyzja

Wprowadzono trzy ograniczone poprawki.

### Jawne quote coverage denominators

Analyzer `scripts/het_pm_v2_analysis.py` raportuje teraz osobno:

- `all_tick_current_executable_record_count`;
- `all_tick_current_executable_presence_rate`;
- `quote_planned_record_count`;
- `quote_not_planned_record_count`;
- `quote_required_current_executable_record_count`;
- `quote_required_current_executable_resolution_rate`;
- `quote_required_stale_snapshot_record_count`;
- `quote_required_stale_snapshot_rate`.

Promotion/report tool `scripts/het_pm_v2_promotion_gate_v1.py` przenosi
najważniejsze pola do sekcji quote observed:

- `all_tick_current_executable_presence_rate`;
- `quote_planned_record_count`;
- `quote_required_current_executable_resolution_rate`;
- `quote_required_stale_snapshot_rate`.

### Analyzer obsługuje aktywny shadow-manager

`het_pm_v2_analysis.py` nie zakłada już wyłącznie observe-only PR-A evidence.
Akceptuje dwa legalne tryby shadow:

1. `consumed_by_policy=false`, `v1_shadow_authority=true`,
   `v2_shadow_authority=false`;
2. `consumed_by_policy=true`, `v1_shadow_authority=false`,
   `v2_shadow_authority=true`.

`live_authority=true` nadal jest fail-closed.

### Refresh przed quote-stale boundary

Aktywny profil HET-PM V2 refreshuje read-only market state wcześniej:

```toml
[post_buy_guardian.shadow_market_refresh]
enabled = true
stale_after_ms = 500
interval_ms = 250
per_position_cooldown_ms = 250
max_requests_per_cycle = 32
rpc_timeout_ms = 750
```

Granica quote stale pozostaje `oracle_stale_hard_ms = 1500`. Refresh ma zatem
rozpocząć się przed odrzuceniem quote jako stale, a nie dopiero na tej samej
granicy.

## D3. Granice bezpieczeństwa

Zmiana nie dodaje live execution.

Zmiana nie robi quote na każdym ticku. `current_executable_*` pozostaje
ustawiane tylko wtedy, gdy istnieje resolved executable quote cell dla bieżącego
V2 source.

Refresh pozostaje bounded:

- działa poza monitor tickiem;
- używa read-only RPC;
- ma cooldown per pozycja;
- ma batch cap;
- ma timeout RPC;
- wynik trafia do `AccountStateCore` jako `RpcRefresh`.

Nie luzowano `oracle_stale_hard_ms`. Stary snapshot nadal jest odrzucany przez
quote resolver.

## D4. Konfiguracja

Zmieniony plik:

- `configs/rollout/ghost_brain_het_pm_v2_promotion_evidence_v1.toml`

Zmiana:

```diff
 [post_buy_guardian.shadow_market_refresh]
 enabled = true
-stale_after_ms = 1500
+stale_after_ms = 500
 interval_ms = 250
-per_position_cooldown_ms = 1000
-max_requests_per_cycle = 8
+per_position_cooldown_ms = 250
+max_requests_per_cycle = 32
 rpc_timeout_ms = 750
```

Rollback:

```toml
[post_buy_guardian.shadow_market_refresh]
enabled = false
```

albo powrót do poprzednich wartości batch/cooldown, jeśli RPC pressure okaże
się zbyt wysokie.

## D5. Implementacja

Zmienione obszary:

- `scripts/het_pm_v2_analysis.py` — nowe quote coverage denominators oraz
  legalny active-shadow authority mode;
- `scripts/het_pm_v2_promotion_gate_v1.py` — propagacja nowych quote metrics
  do artifactu;
- `scripts/test_het_pm_v2_analysis.py` — testy denominatorów i active-shadow
  authority;
- `ghost-brain/tests/ghost_brain_config_load_test.rs` — test aktywnego rollout
  configu, który wymaga refreshu przed quote stale boundary;
- `configs/rollout/ghost_brain_het_pm_v2_promotion_evidence_v1.toml` —
  wcześniejszy i szerszy bounded refresh.

## D6. Testy

Wykonane lokalnie:

```text
python3 scripts/test_het_pm_v2_analysis.py
python3 scripts/test_het_pm_v2_promotion_gate_v1.py
python3 -m py_compile scripts/het_pm_v2_analysis.py scripts/het_pm_v2_promotion_gate_v1.py scripts/test_het_pm_v2_analysis.py scripts/test_het_pm_v2_promotion_gate_v1.py
cargo test -q -p ghost-brain het_pm_v2_rollout_refreshes_before_quote_stale_boundary --test ghost_brain_config_load_test
```

Wynik:

- Python analyzer tests: `29 passed`;
- Python promotion tests: `40 passed`;
- py_compile: PASS;
- Rust config test: PASS.

Rust test emituje istniejące warningi z innych modułów, niezwiązane z tą
zmianą.

## D7. Weryfikacja na ostatnim runie

Po zmianie analyzera uruchomiono go na istniejącym runie `retry6`, używając
właściwego writer instance:

```text
logs/shadow_run/shadow-het-pm-v2-authoritative-20260719-retry6/het_pm_v2_writer_health_v1.f0c3b9013d63e6f203dd35b3511d1a524f293aa3db3e91eb9bbea31e4db92598.json
```

Nowe pola raportu dla starego runu pokazują prawidłowy rozdział:

```text
all_tick_current_executable_record_count              14215
all_tick_current_executable_presence_rate             0.4705239813313032
quote_planned_record_count                            17872
quote_not_planned_record_count                        12339
quote_required_current_executable_record_count        14215
quote_required_current_executable_resolution_rate     0.7953782452999105
quote_required_stale_snapshot_record_count            3459
quote_required_stale_snapshot_rate                    0.1935429722470904
```

Te liczby są baseline dla kolejnego runu po zmianie configu. Oczekiwany efekt
tej decyzji to spadek `quote_required_stale_snapshot_rate`, a nie sztuczne
podniesienie all-tick coverage do 100%.

## D8. Następny krok

Uruchomić kolejny krótki shadow run z nowym configiem i porównać:

- `quote_required_stale_snapshot_rate`;
- `quote_required_current_executable_resolution_rate`;
- `Blocked(QuoteUnavailable)`;
- `current_executable_gross_return_bps` all-tick presence;
- brak dropów writer/admission health;
- brak runtime panic.

Jeżeli `quote_required_stale_snapshot_rate` pozostanie wysoki, kolejną zmianą
powinna być priorytetyzacja refreshu pozycji, które w poprzednim ticku miały
quote-required gate, zamiast dalszego luzowania stale boundary.
