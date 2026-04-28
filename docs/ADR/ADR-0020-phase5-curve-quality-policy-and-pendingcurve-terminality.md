# ADR-0020: Phase-5 Curve Quality Policy and PendingCurve Terminality

**Date:** 2026-03-22  
**Status:** Accepted  
**Author:** Ghost Father  

## Context

Po domknięciu Fazy 4 runtime miał już deterministic recovery, ale nadal utrzymywał częściowo niejawny kontrakt jakości curve state:

- `OracleRuntime` sprowadzał freshness do degradacji `curve_data_known=false` dla stale snapshotów,
- `GatekeeperBuffer` rozumiał głównie binarny latch `curve_ready` zamiast jawnego modelu quality,
- `PendingCurve` nie miał kompletnej, jawnej i terminalnej telemetryki,
- polityka freshness była rozdzielona między top-level `[shadow_ledger]`, env override runtime oraz gatekeeperowe pola `curve_wait_ms/curve_require_for_buy`.

To naruszało cel Fazy 5 z `PLAN_UPORZADKOWANIA_ARCHITEKTURY_PIPELINE_20260320.md`: jeden kontrakt freshness/finality, jedno SSOT config, przewidywalna ścieżka końcowa `PendingCurve` i brak ukrytego sprowadzania jakości do pojedynczego boola.

## Decision

Wdrożono jawny Phase-5 contract dla curve state:

1. **Model quality został ujednolicony** przez rozszerzenie `CurveFreshnessState` do:
	- `unknown`
	- `stale`
	- `fresh`
	- `committed`

2. **Model finality pozostał osobnym wymiarem** i dalej używa:
	- `speculative`
	- `provisional`
	- `finalized`

3. **Top-level `[shadow_ledger]` został ustanowiony jako SSOT dla Phase-5 policy**:
	- `enrichment_freshness_ms`
	- `stale_fallback`
	- `curve_wait_ms`
	- `curve_require_for_buy`

4. **`main.rs` synchronizuje Phase-5 policy do `GatekeeperV2Config`**, dzięki czemu `[gatekeeper_v2]` dalej steruje progami analitycznymi, ale nie freshness/pending semantics.

5. **`OracleRuntime` przestał degradować stale curve do `curve_data_known=false`**. Zamiast tego:
	- zachowuje raw truth (`curve_data_known`, `curve_finality`),
	- rozwiązuje jawny quality state,
	- przekazuje quality/finality do `GatekeeperBuffer::record_curve_state(...)`.

6. **`GatekeeperBuffer` używa policy matrix zamiast samego `curve_ready` boola**:
	- `unknown` → `PendingCurve` albo immediate reject zależnie od `curve_require_for_buy`,
	- `stale speculative/provisional` → `PendingCurve` albo reject zależnie od `stale_fallback`,
	- `stale finalized` → allow tylko dla `use_stale_with_warning`,
	- `fresh` / `committed` → normal path.

7. **`PendingCurve` ma teraz jawne terminal states**:
	- `recovered`
	- `rejected`
	- `timed_out`

## Architectural Impact

Zmiana spina razem trzy warstwy:

- `ghost-core` — publiczny model quality/finality (`CurveFreshnessState`, `CurveFreshnessInfo`),
- `ghost-launcher/oracle_runtime` — rozwiązywanie explicit quality na hot path,
- `ghost-launcher/components/gatekeeper` — egzekwowanie Phase-5 policy matrix i terminal telemetry,
- `ghost-launcher/config` / `main.rs` — jedno SSOT config i sync do runtime.

Efekt uboczny celowy: `curve_data_known` nie jest już przeciążone znaczeniem świeżości. Oznacza wyłącznie raw truth availability, a nie decyzję policy.

## Risk Assessment

**Rate: Medium**

Główne ryzyka regresji:

1. **Zmiana semantyki stale curve** — część starych ścieżek mogła milcząco zakładać, że stale == `curve_data_known=false`.
2. **Config precedence** — jeśli ktoś oczekiwał, że `[gatekeeper_v2]` jest SSOT również dla freshness policy, po tej zmianie nadrzędny jest `[shadow_ledger]`.
3. **Terminalność `PendingCurve`** — double-emission lub brak emission byłby błędem operacyjnym i telemetrycznym.

Ryzyko zostało ograniczone przez testy kontraktowe dla:

- `curve_latch_*`
- `curve_policy_*`
- `enrich_pool_tx_*`
- config SSOT promotion / precedence

## Consequences

### Pozytywne

- runtime ma jawny model quality zamiast ukrytego „downgrade do boola”,
- `PendingCurve` jest operacyjnie obserwowalny od startu do końca,
- policy freshness/finality jest sterowana z jednego miejsca,
- finality caution (`speculative` / `provisional`) nadal działa bez mieszania z freshness.

### Negatywne / trade-offs

- `GatekeeperBuffer` utrzymuje więcej stanu policy per pool,
- testy i dokumentacja muszą rozróżniać raw truth (`curve_data_known`) od quality (`fresh/stale/...`),
- `use_stale_with_warning` jest teraz semantycznie węższe: stale non-finalized nie omija już policy wait/reject.

## Alternatives Considered

### 1. Pozostawić degradację stale -> `curve_data_known=false`

Odrzucone, bo dalej mieszałoby availability truth z freshness policy i utrzymywało niejawny kontrakt.

### 2. Przenieść całą policy logikę do samego `ShadowLedger`

Odrzucone, bo decyzja `PendingCurve/reject/allow` zależy od runtime contextu Gatekeepera (deadline, terminal telemetry, pool state), a nie tylko od storage lookup.

### 3. Wprowadzić osobny nowy struct event-busowy zamiast jawnego quality injection do Gatekeepera

Odrzucone, bo zwiększałoby blast radius publicznego kontraktu `PoolTransaction` bez potrzeby. Mniejszy i bezpieczniejszy był explicit handoff `OracleRuntime -> GatekeeperBuffer`.

## Validation Steps

Zweryfikowano przez:

1. `cargo test -p ghost-core test_get_curve_freshness_info_ --lib`
2. `cargo test -p ghost-launcher curve_latch --lib`
3. `cargo test -p ghost-launcher curve_policy_ --lib`
4. `cargo test -p ghost-launcher enrich_pool_tx --lib`
5. `cargo test -p ghost-launcher test_config_from_shadow_ledger_config_uses_launcher_ssot --lib`
6. `cargo test -p ghost-launcher test_top_level_shadow_ledger_wins_over_legacy_nested_alias --lib`

W staging/production należy dodatkowo obserwować:

- `shadow_ledger_curve_freshness_total{state=...}`
- `shadow_ledger_curve_finality_total{state=...}`
- `gatekeeper_pending_curve_total{reason=...}`
- `gatekeeper_pending_curve_terminal_total{outcome=...}`
