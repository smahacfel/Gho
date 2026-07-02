# ADR-8D: Shadow V2 PostBuyRuntime Manifest Shutdown PR18C

## Status

Proposed for PR27 / PR18C.

## D1. Problem

PR18/PR17B validation burnin wygenerowal canonical Shadow V2 evidence, ale runtime shutdown zostal zablokowany przez:

`POSTBUY_RUNTIME_SHUTDOWN_ABORTED_BEFORE_RUNTIME_POST_RUN_MANIFEST`

Launcher abortowal `PostBuyRuntime` po generic `30s` component join timeout. W efekcie runtime nie zdazyl wykonac Shadow V2 post-run manifest generation + strict audit przed zakonczeniem komponentu.

## D2. Decyzja

Wprowadzamy waski shutdown fix:

- `PostBuyRuntime` wykonuje Shadow V2 post-run manifest generation + strict audit przed zwroceniem z `run()`;
- Shadow V2 post-run manifest ma dedykowany budget `post_run_manifest_drain_timeout_ms`;
- domyslny budget wynosi `180000ms`;
- launcher daje `PostBuyRuntime` osobny join timeout rowny Shadow V2 budget + `30s` margin, tylko gdy `shadow_v2_burnin.enabled=true` i `logging_only=true`;
- przekroczenie budgetu manifestu jest klasyfikowane jako `SHADOW_V2_POST_RUN_MANIFEST_DRAIN_TIMEOUT`;
- forced component abort nadal blokuje clean shutdown claim.

## D3. Runtime Boundary

Nie zmieniamy:

- BUY/REJECT;
- Gatekeeper policy;
- selector runtime;
- TX/Jito/live path;
- `shadow_close_only`;
- active close;
- runtime approval flags.

Zmiana dotyczy tylko shutdownowego materializowania Shadow V2 validation evidence.

## D4. Implementation Scope

Zmiany:

- `ShadowV2BurninConfig.post_run_manifest_drain_timeout_ms`;
- logging-only rollout config deklaruje `180000`;
- `PostBuyRuntime` uzywa async `tokio::process::Command` dla manifest audit;
- `PostBuyRuntime` owija post-run generation + strict audit w typed timeout;
- launcher wybiera dluzszy join timeout tylko dla `PostBuyRuntime` w Shadow V2 logging-only validation;
- launcher ma testowalna klasyfikacje clean vs forced-abort/failure shutdown.

## D5. Evidence and Tests

Wymagane walidacje PR:

- `cargo check -p ghost-brain`;
- `cargo check -p ghost-launcher`;
- targeted test: `post_buy_runtime_shutdown_waits_for_shadow_v2_post_run_manifest`;
- targeted test: existing OracleRuntime shutdown test;
- targeted test: `test_forced_abort_is_not_clean_component_shutdown`;
- `cargo fmt --check`;
- `git diff --check`;
- forbidden staged-file guard.

## D6. Consequences

Po merge PR27 nalezy uruchomic tylko 45-min validation burnin.

Required result:

- runtime `post_run_manifest.status=PASS`;
- post-run strict audit `PASS`;
- no `PostBuyRuntime` abort;
- no forced component abort;
- clean shutdown proven;
- canonical V2 fill/path/terminal rows present;
- `real_shadow_v2_positions > 50`.

## D7. Limitations

Ten PR nie przyznaje:

- research-grade verdict;
- live-equivalence;
- strategy proof;
- RCE proof;
- runtime approval;
- `shadow_close_only` approval;
- active close approval.

## D8. Rollback

Rollback polega na revert PR27. Domyslne stare configi nadal laduja sie przez `#[serde(default)]`, bo nowe pole ma default.
