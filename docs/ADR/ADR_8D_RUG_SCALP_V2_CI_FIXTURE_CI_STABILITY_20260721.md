# ADR-8D: RUG SCALP V2 — CI fixture compatibility and canonical tick stability

Status: `IMPLEMENTED / SHADOW-ONLY / NOT A SMOKE OR RUN AUTHORIZATION`

Typ: ADR-8D / CI repair / test-fixture determinism

Data: `2026-07-21`

Repo: `smahacfel/Gho`

PR: `#78`

## D0. Decyzja

Naprawiono wyłącznie testową kompatybilność po dodaniu opcjonalnego pola
`PostBuyRuntimeConfig::rug_scalp_outcome_log_path` oraz wzmocniono dwa testy
terminalnego Position Managera RUG SCALP V2.

Nie zmieniono runtime strategy, progów, Pump quote math, Position Manager
authority, preflightu ani shadow/live boundary.

## D1. Przyczyna

Zdalne CI PR #78 wykryło cztery ręczne initializery `PostBuyRuntimeConfig`
bez nowego opcjonalnego pola outcome logu. Ten błąd dotyczył kompilacji
integration testu, nie serde defaults ani produkcyjnego ładowania configu.

Ten sam pełny job ujawnił, że dwa nowe testy PM przekazywały `None` do ticku,
chociaż fixture uprzednio publikował konkretny canonical snapshot do
`ShadowLedger`. Test opierał się więc na pośrednim odczycie fixture state,
zamiast jawnie dostarczyć identyczny snapshot do obserwowanego ticku.

## D2. Zmiana

- Cztery testowe initializery podają
  `rug_scalp_outcome_log_path: None`.
- Każdy z dwóch PM testów tworzy jeden immutable `canonical_snapshot`, zapisuje
  jego clone do `ShadowLedger`, a następnie przekazuje referencję do tego
  samego snapshotu do `run_shadow_runtime_tick`.

To nie dodaje retry, polling loop ani second exit path. Nadal wymagane są:

```text
canonical material sell / dwa complete empty sloty
-> PM candidate
-> jeden terminal close
```

## D3. Zachowane inwarianty

- Position Manager pozostaje wyłącznym właścicielem exit i terminal close.
- `MATERIAL_SELL_EMERGENCY` nadal wymaga `SLOT_COMPLETE`.
- `FLOW_EXHAUSTED` nadal wymaga dwóch complete empty slots.
- Test nie maskuje braku close drugim tickiem ani retry.
- `DATA_INVALIDATED`, watermark i same-slot ordering pozostają niezmienione.
- RUG SCALP pozostaje default-disabled i shadow-only.

## D4. Weryfikacja

Wykonano po korekcie:

```text
cargo fmt --all
RUSTFLAGS='-C target-cpu=x86-64' cargo test -p ghost-brain --lib guardian::post_buy
RUSTFLAGS='-C target-cpu=x86-64' cargo test -p ghost-launcher --test post_buy_runtime_integration
git diff --check
```

Wyniki lokalne przeszły w tym samym `target-cpu` profilu, którego użył
nieudany job CI.

## D5. Rollback

Rollback tej naprawy oznacza wycofanie wyłącznie commit'u test/ADR. Nie ma
produkcjnych danych, pozycji ani zmigrowanego configu do usunięcia.

## D6. Granica operacyjna

Green CI jest koniecznym, ale niewystarczającym warunkiem smoke. External
Pump parity i operational latency/cost freeze pozostają osobnymi bramkami.

## D7. Otwarte bramki

- wersjonowany aktualny on-chain Pump parity fixture;
- techniczne evidence p90 entry i exit dla aktualnego private RPC/Jito path;
- freeze cost/config/binary/PM hashes;
- green remote CI na końcowym PR SHA.
