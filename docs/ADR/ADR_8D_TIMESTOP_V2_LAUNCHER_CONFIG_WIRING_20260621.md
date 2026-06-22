# ADR-8D: TimeStop V2 Launcher Config Wiring

Status: IMPLEMENTED / TARGETED_TESTS_PASSED
Typ: ADR-8D / post-buy shadow lifecycle config bridge / rollout safety
Data: 2026-06-21
Autor/Agent: Codex
Repo/branch: `/root/Gho`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: przepiecie `[post_buy_guardian.time_stop_v2]` z `ghost-brain` configu do aktywnego `ghost-launcher` post-buy runtime
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `ghost-brain/src/guardian/post_buy/mod.rs`
- `ghost-launcher/src/components/post_buy_runtime.rs`
- `ghost-launcher/src/main.rs`
- `ghost-launcher/tests/post_buy_runtime_integration.rs`
- `docs/ADR/ADR_8D_TIMESTOP_V2_LAUNCHER_CONFIG_WIRING_20260621.md`

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w ostatnich raportach.

## 1. Przygotowanie i dzialania wstepne

Cel:
Zapewnic, ze profil R44 z `time_stop_v2.enabled = true` faktycznie wlacza observe-only telemetry w aktywnym runtime uruchamianym przez `ghost-launcher`.

Kontekst:
Pierwszy start R44 przeszedl launcher canary, ale walidator `scripts/check_time_stop_v2_observe.py` pokazal `time_stop_v2_window_rows = 0`. Kod `ghost-brain` mial implementacje V2, a TOML mial wlaczony blok, lecz aktywny `PostBuyRuntimeConfig` przenosil tylko legacy pola:
- `shadow_target_threshold`
- `shadow_stoploss_threshold`
- `shadow_wait_for_timestop`

Nested config `time_stop_v2` nie byl przekazywany do `PostBuyGuardianConfig` budowanego przez launcher.

## 2. Opis problemu - 3W2H

What:
`[post_buy_guardian.time_stop_v2]` byl obecny w brain configu, ale nie dochodzil do aktywnego `MonitoringEngine`.

Where:
- `ghost-launcher/src/main.rs`
- `ghost-launcher/src/components/post_buy_runtime.rs`
- `ghost-brain/src/guardian/post_buy/mod.rs`

Why:
Launcher mial osobny adapter `PostBuyRuntimeConfig`, ktory mapowal tylko czesc `post_buy_guardian`. Nowo dodany nested config V2 zostal w `ghost-brain`, ale nie byl czescia bridge'u runtime.

How:
Dodano jawne pole `shadow_time_stop_v2: Option<TimeStopV2Config>` do `PostBuyRuntimeConfig`, re-export typu z `ghost-brain`, mapowanie z `ghost_brain_config.post_buy_guardian.time_stop_v2` w `main.rs` oraz ustawienie `guardian.time_stop_v2` w `build_shadow_guardian_config`.

How many:
Zmiana dotyka tylko config bridge dla shadow/probe post-buy runtime. Nie zmienia Gatekeeper policy, `MaterializedFeatureSet`, TX buildera, sendera, live execution ani semantyki legacy close reason.

## 3. Przyczyna zrodlowa

Root cause:
Konfiguracyjny SSOT dla TimeStop V2 byl zdefiniowany w `ghost-brain`, ale aktywna sciezka launchera uzywala oddzielnego adaptera runtime i nie miala pola dla nested V2 configu.

Skutek:
R44 byl poprawnie opisany w TOML, ale runtime zachowywal sie jak `time_stop_v2.enabled = false`, bo `PostBuyGuardianConfig::default()` pozostawial V2 disabled.

## 4. Strategia naprawy

Przyjeta strategia:
- Nie duplikowac progow V2 jako osobnych prostych pol w launcherze.
- Przeniesc caly typowany `TimeStopV2Config`, aby zachowac jeden ksztalt configu.
- Zachowac kompatybilnosc testow i lokalnych inicjalizatorow przez `shadow_time_stop_v2: None`.
- Nie zmieniac domyslnego zachowania starych configow.

## 5. Przeprowadzone akcje naprawcze

Zmiana 1: export typu
- `ghost-brain/src/guardian/post_buy/mod.rs` re-exportuje `TimeStopV2Config`.

Zmiana 2: launcher runtime config
- `PostBuyRuntimeConfig` dostal pole `shadow_time_stop_v2: Option<TimeStopV2Config>`.
- `Default` ustawia `shadow_time_stop_v2 = None`.
- `build_shadow_guardian_config` kopiuje V2 config do `PostBuyGuardianConfig`, jezeli jest obecny.

Zmiana 3: main bridge
- `ghost-launcher/src/main.rs` mapuje `ghost_brain_config.post_buy_guardian.time_stop_v2` do `PostBuyRuntimeConfig`.

Zmiana 4: test initializers
- Lokalne pelne inicjalizatory `PostBuyRuntimeConfig` w testach dostaly `shadow_time_stop_v2: None`, aby utrzymac jawny kontrakt kompilacyjny.

## 6. Walidacja

| Walidacja | Wynik | Status |
|---|---|---|
| `rustfmt --edition 2021 ghost-launcher/src/components/post_buy_runtime.rs ghost-launcher/src/main.rs ghost-launcher/tests/post_buy_runtime_integration.rs ghost-brain/src/guardian/post_buy/mod.rs` | wykonane | PASS |
| `cargo test -p ghost-launcher --lib shadow_exit_thresholds_use_post_buy_guardian_percent_fields --quiet` | 1 passed | PASS |
| `cargo test -q -p ghost-launcher components::trigger::shadow_run::tests::p5_precheck_failure_writes_not_dispatched_lifecycle_record` | target passed | PASS |
| `git diff --check` dla dotknietych plikow | no whitespace errors | PASS |

## 7. Ryzyka i zabezpieczenia

Ryzyko 1: przypadkowa zmiana live behavior.
- Zabezpieczenie: pole dotyczy tylko shadow/probe guardian configu. Nie wlacza live execution i nie dotyka live sell thresholds.

Ryzyko 2: config drift miedzy brain i launcher.
- Zabezpieczenie: launcher przenosi caly `TimeStopV2Config`, zamiast kopiowac pojedyncze stale.

Ryzyko 3: stare profile bez V2.
- Zabezpieczenie: `shadow_time_stop_v2` jest opcjonalne, a default zachowuje `None`.

Ryzyko 4: brudny worktree.
- Obserwacja: repo bylo juz brudne przed ta poprawka. Nie wykonywano stagingu, commita, resetu ani revertu cudzych zmian.

## 8. Decyzja

`time_stop_v2` jest teraz czescia aktywnego launcher bridge'u. Kolejny start R44 powinien emitowac rekordy `time_stop_v2_window`, jezeli runtime otworzy shadow/probe pozycje i `MonitoringEngine` przejdzie przez zaplanowane okna V2.
