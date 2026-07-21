# ADR-8D: HET-PM V2 - diff-scoped Clippy gate remediation

Data: 2026-07-21

Typ: ADR-8D / CI remediation / HET-PM V2 active shadow

Status: IMPLEMENTED

Repozytorium: `/root/Gho_dynamic_exit_v1_pr2b`

Gałąź: `agent/het-pm-v2-reproducible-validation`

## D1. Problem

GitHub Actions dla PR #75 przeszedl testy HET-PM, targeted Clippy oraz
resource diagnostic, ale zatrzymal sie na `Diff-scoped Clippy diagnostic gate`.
Gate wykryl nowe diagnostyki w zmienionym zakresie:

- `clippy::type_complexity` na typie zwracanym przez materializacje bundle'a;
- `clippy::unnecessary_map_or` w budowaniu snapshotow trajectory;
- `clippy::needless_borrow` przy przekazaniu `GatekeeperAssessment`;
- `clippy::nonminimal_bool` w IWIM;
- `dead_code` dla nieuzywanego helpera `runtime_lane`.

To byly problemy bramki CI, nie nowy runtime finding managera. Nie wymagaly
zmiany semantyki HET-PM.

## D2. Decyzja

Zmieniono tylko kod w miejscach wskazanych przez Clippy:

- wprowadzono alias typu dla materializacji `PostBuySnapshotBundle`;
- zastapiono `map_or(true, ...)` przez `is_none_or(...)`;
- usunieto redundantne `&` przy juz referencyjnym `assessment`;
- uproszczono rownowazny warunek boolean;
- usunieto martwa metode `runtime_lane`.

Nie zmieniono wyboru authority, route guardow, quote freshness, snapshot
boundary, executor-a, configu ani schemy evidence.

## D3. Weryfikacja

Lokalnie uruchomiono:

```text
cargo fmt --all
python3 scripts/guard_diff_scoped_clippy.py --base aaec7382907efa4581f7a541dad2757dee978ce5 --head HEAD
```

Diff-scoped Clippy zakonczyl sie wynikiem PASS. Po commicie i pushu GitHub
Actions ma ponownie uruchomic matrix dla nowego SHA.

## D4. Inwarianty

- HET-PM V2 pozostaje active shadow only;
- live authority pozostaje wylaczone;
- MaxHold nadal wymaga wykonywalnej route;
- RPC refresh nadal rozdziela observation freshness od data-change activity;
- PR pozostaje Draft do zakonczenia pelnego CI i pozniejszego locka.

## D5. Zakres negatywny

Nie zmieniono:

- konfiguracji HET-PM;
- progow trailing, vitality, crash, max-hold ani quote freshness;
- wyboru candidate lub authority;
- JSONL schema;
- writerow sidecar/admission/lifecycle;
- promotion locka.

Ta zmiana nie jest nowym dowodem skutecznosci managera. Jest technicznym
domknieciem required CI gate po runtime remediation.

## D6. Testy lokalne

Lokalnie uruchomiono:

```text
cargo fmt --all
cargo fmt --all -- --check
python3 scripts/guard_diff_scoped_clippy.py --base aaec7382907efa4581f7a541dad2757dee978ce5 --head HEAD
cargo test -q -p ghost-brain --lib guardian::post_buy
cargo test -q -p ghost-launcher components::post_buy_runtime --lib
python3 -m unittest scripts/test_het_pm_v2_analysis.py scripts/test_het_pm_v2_promotion_gate_v1.py scripts/test_guard_diff_scoped_clippy.py
git diff --check
```

Wszystkie powyzsze kontrole przeszly lokalnie.

## D7. Ryzyko

Glowne ryzyko tej poprawki to roznica miedzy lokalnym branch HEAD a merge SHA,
na ktorym GitHub uruchamia diff-scoped Clippy. Ryzyko jest ograniczone, bo
naprawiono dokladne diagnostyki z joba GitHub Actions i pozostawiono test
diff-scoped jako lokalna bramke przed pushem.

## D8. Nastepny krok

Po pushu nalezy potwierdzic nowy GitHub Actions run dla PR #75. Jezeli gate
ponownie failnie, nastepna analiza ma zaczac sie od logu joba Actions dla
nowego merge SHA, a nie od zgadywania lokalnych lintow.
