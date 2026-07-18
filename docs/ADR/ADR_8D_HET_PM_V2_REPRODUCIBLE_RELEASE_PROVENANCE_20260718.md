# ADR-8D: HET-PM V2 — odtwarzalny release build przed prospective validation

Data: 2026-07-18

Typ: ADR-8D / PR #73 promotion-evidence prerequisite / release provenance

Status: Accepted source amendment; criteria wróciły do `calibration_pending`
po fail-closed preflightcie payer provenance i wymagają finalnego relocku.

## D1. Problem

Operacyjny preflight przed `validation-v1a` wykazał, że release binary wpisana
do criteria jako:

```text
8f38ee7879f4c8ce58b43c3757b4fe1cd09d4b398a07e56d99165c690e6a3804
```

nie była zachowana jako artefakt i nie dawała się odtworzyć z deklarowanego
commita. Fresh resolver build oraz build z lokalnie zachowanym lockfile'em
wytworzyły dwa inne SHA-256. Runtime nie został uruchomiony.

Repo ignorowało `Cargo.lock`, criteria nie wiązały toolchainu ani native target
features, a `ghost-brain/build.rs` osadzał w binarce stan czystości worktree.
Samo `expected_release_binary_sha256` identyfikowało więc konkretny plik, ale nie
stanowiło odtwarzalnego source-to-binary contractu.

## D2. Zakres decyzji

Decyzja dotyczy wyłącznie build/provenance prerequisite'u PR #73 i launchera
prospective validation.

Poza zakresem pozostają:

- HET-PM V2 authority cutover;
- proposal/apply/terminal/capacity ownership;
- ekonomiczne progi Gate 1-5;
- zmiana konfiguracji wejścia albo strategii;
- uznanie wcześniejszych diagnostic runów za validation evidence.

## D3. Root cause

1. `Cargo.lock` był globalnie ignorowany mimo budowania produkcyjnej binarki z
   workspace'u aplikacyjnego.
2. Launcher wykonywał `cargo build --release`, więc resolver mógł zmienić
   wersje zależności przed startem runu.
3. Criteria nie utrwalały identity `rustc`, `cargo`, `.cargo/config.toml` ani
   efektywnego `target-cpu=native`.
4. Lock tool hashował istniejącą binarkę, lecz sam nie wymagał clean detached
   runtime worktree i nie wykonywał canonical locked builda.
5. Stan dirty/clean wpływa na bytes przez `GIT_WORKTREE_CLEAN`, ale nie był
   elementem launch proofu.
6. Dwa pierwsze clean buildy z identycznego commita i toolchainu nadal dawały
   różne SHA, ponieważ kod wygenerowany przez protobuf zawierał absolutny
   `OUT_DIR` zależny od ścieżki detached worktree. Sam lockfile i toolchain nie
   usuwają checkout-path entropy z wynikowego ELF.
7. Po remapowaniu rustc pozostała jeszcze absolutna wartość
   `env!("CARGO_MANIFEST_DIR")` w produkcyjnym `gui-backend`. Literały z makr
   środowiskowych nie podlegają `--remap-path-prefix`, więc wymagały runtime
   workspace discovery zamiast compile-time source path.
8. Canonical package clean początkowo nie otrzymywał tego samego
   `CARGO_ENCODED_RUSTFLAGS` co build ani jawnego profilu `--release`. Cargo
   wybiera artefakty/fingerprint także przez effective flags i profil, więc
   clean raportował `Removed 0 files` i nie gwarantował fresh provenance
   rebuilda.
9. Pierwszy launcher dry-run przeszedł static lifecycle guard, lecz preflight
   odrzucił `payer_strategy="configured"` bez jawnego keypair contractu.
   Podstawienie walleta przez niehashowany environment override zmieniałoby
   runtime identity poza zamrożonym exact/normalized configiem.
10. Po zmianie configu na `ephemeral` preflight nadal bezwarunkowo wykonywał
    configured-keypair check. Była to niespójność z istniejącym
    `validate_trigger_payer_contract()`, który prawidłowo dopuszcza ten dokładny
    shadow-only profil bez keypaira.

## D4. Decyzja

1. Root `Cargo.lock` staje się tracked source contractem.
2. `rust-toolchain.toml` przypina Rust `1.95.0` z profilem `minimal`.
3. Canonical build procedure najpierw usuwa wyłącznie cache pakietów, które
   zawierają/osadzają provenance, a następnie buduje locked release:

```text
cargo clean --release -p ghost-brain -p ghost-launcher
CARGO_ENCODED_RUSTFLAGS='-C<US>target-cpu=native<US>--remap-path-prefix=<runtime-source-root>=/workspace/ghost' \
  cargo build --release --locked -p ghost-launcher
```

`<US>` oznacza separator `0x1f` wymagany przez `CARGO_ENCODED_RUSTFLAGS`.
Rzeczywista ścieżka source root jest dynamiczna, ale trwały kontrakt przechowuje
stabilną postać
`--remap-path-prefix=<runtime-source-root>=/workspace/ghost`.
Ten sam encoded rustflags env obowiązuje clean i build, a clean jawnie wybiera
profil `--release`; rozdzielenie środowisk albo profili jest błędem kontraktu.
Po udanym cleanie zarówno criteria locker, jak i launcher wymagają, aby
`target/release/ghost-launcher` nie istniał przed rozpoczęciem builda.

Package clean jest wymagany, ponieważ no-op Cargo build mógłby zachować
wcześniejszy artefakt z inną wartością embedded `GIT_WORKTREE_CLEAN`.

4. `lock-criteria` wymaga clean detached worktree na dokładnym reviewed runtime
   commicie, sam wykonuje canonical build i akceptuje wyłącznie binarkę z
   `<runtime-worktree>/target/release/ghost-launcher`.
5. Criteria i launcher proof utrwalają osobno:
   - `Cargo.lock` SHA-256;
   - `rust-toolchain.toml` SHA-256;
   - `.cargo/config.toml` SHA-256;
   - hash pełnego `rustc -vV`;
   - hash `cargo -V`;
   - hash `rustc --print cfg -C target-cpu=native`;
   - canonical build command;
   - canonical native-codegen/source-path-remap rustflags contract;
   - potwierdzenie clean worktree przed i po buildzie.
6. Jakakolwiek niezgodność degraduje start/evidence fail-closed. Nie wpływa na
   runtime authority, bo proces validation nie zostaje uruchomiony.
7. Criteria/tool/promotion/run-manifest schema przechodzą na wersję 4; HET
   policy version 2 i comparison schema version 3 pozostają bez zmian.
8. `gui-backend` nie osadza source checkout path. Workspace jest wykrywany w
   runtime z jawnego `GHOST_WORKSPACE_ROOT`, następnie z położenia release
   binary, a ostatecznie z bieżącego katalogu. Zachowuje to dostęp do static i
   config files bez wprowadzania lokalnej ścieżki builda do ELF.
9. Oba prospective shadow-only run configs używają
   `payer_strategy="ephemeral"`. Nie wymagają zewnętrznego wallet secretu, a
   wybór strategii pozostaje częścią exact i normalized config hash.
10. Preflight pomija odczyt keypaira i balance probe wyłącznie dla
    `trigger.enabled + shadow_only + shadow_run.enabled + ephemeral`. Live,
    live-and-shadow oraz configured shadow payer zachowują dotychczasowy
    fail-closed check.

## D5. Konsekwencje

- Fresh dependency resolution nie może cicho zmienić validation binary.
- Dirty source checkout nie może wyprodukować kwalifikowanego launcher proofu.
- Dwa runy muszą używać tego samego source, lockfile'a, toolchainu, native
  target contractu, remap contractu, build command i wynikowej binarki.
- Zmiana toolchainu, CPU targetu, Cargo.lock albo build flags wymaga nowej wersji
  criteria i dwóch nowych prospective runów.
- Poprzedni locked SHA zostaje wycofany jako nieodtwarzalny; nie daje evidence
  dla authority promotion.

## D6. Implementacja

Zmiany obejmują:

- `.gitignore` i tracked `Cargo.lock`;
- `rust-toolchain.toml`;
- `scripts/start_selector_lifecycle_run.py`;
- `scripts/het_pm_v2_promotion_gate_v1.py`;
- `gui-backend/src/workspace.rs` i trzy istniejące GUI call sites;
- testy obu narzędzi;
- `PLANS/DO_REALIZACJI/HET_PM_V2_PROMOTION_CRITERIA_V1.json`.

Nie zmieniono kodu gate'ów, HET evaluation, V1 authority ani ścieżki terminalnej.

## D7. Weryfikacja

Wymagane przed startem `validation-v1a`:

```text
python3 scripts/test_selector_lifecycle_run_guard.py
python3 scripts/test_het_pm_v2_promotion_gate_v1.py
python3 scripts/test_het_pm_v2_analysis.py
python3 -m py_compile scripts/start_selector_lifecycle_run.py scripts/het_pm_v2_promotion_gate_v1.py
cargo clean --release -p ghost-brain -p ghost-launcher
CARGO_ENCODED_RUSTFLAGS='-C<US>target-cpu=native<US>--remap-path-prefix=<runtime-source-root>=/workspace/ghost' \
  cargo build --release --locked -p ghost-launcher
```

Dodatkowy dowód materializacyjny:

```text
clean build A SHA-256 == clean build B SHA-256
criteria expected_release_binary_sha256 == clean build SHA-256
launcher proof release/build identities == criteria
```

Dowód reprodukowalności uzyskany przed finalnym clean-env amendmentem:

```text
runtime source commit = 4617024ff49ac59c80b7b5e6a47fb5a8577c5556
clean build A SHA-256 = 530e7b157a725d4b869fa0e88fac639a397febb53b582a044faae773617a7656
clean build B SHA-256 = 530e7b157a725d4b869fa0e88fac639a397febb53b582a044faae773617a7656
canonical criteria SHA-256 = 77e3ab706952d2c1490017d2e5a5137c561814da800a9a651dbb726ef4c1f851
brain config SHA-256 = 7fc14662d1433872978dcb8d5be0413c7f7769905b6b84f36f6bc46f7e9a2028
normalized behavioral config SHA-256 = bfa1a00497ab9b4babe4cc8198ade4d424e52ff70f38c882cfcee0939ef6ceac
```

`cmp` potwierdził identyczne bytes obu binarek i obu niezależnie wygenerowanych
criteria. `strings` nie znalazł lokalnej ścieżki żadnego detached worktree w
odpowiadającej mu binarce.

Ten wynik potwierdził usunięcie path entropy, ale nie był finalnym prospective
lockiem: późniejszy audyt wykazał `Removed 0 files` przy clean uruchomionym bez
canonical rustflags env i profilu `--release`. Criteria zostały świadomie
cofnięte do pending przed finalnym source amendmentem.

Finalny materialized proof:

```text
runtime source commit = a620765d7fdf6bda8f015398e57fb359b25a692e
clean build A SHA-256 = 5d5a79a7c4c63be3006edf6570414d3b968cfa9a0915da92563e95a083979220
clean build B SHA-256 = 5d5a79a7c4c63be3006edf6570414d3b968cfa9a0915da92563e95a083979220
canonical criteria SHA-256 = 6d6d5a153ca61a87eebef6af16fc621fc08b408769f607dcd38843afabe6ab7c
promotion tool SHA-256 = 18b25934a7849b81982ff79f67c3f0afe8b22cfeb6d0a3176f400d7489a59491
PR-A analyzer SHA-256 = 5e26d19b10ea2613f0da9c4e532d60cc9a646ee000289befdd98f4f6f9e1faa0
brain config SHA-256 = 7fc14662d1433872978dcb8d5be0413c7f7769905b6b84f36f6bc46f7e9a2028
normalized behavioral config SHA-256 = bfa1a00497ab9b4babe4cc8198ade4d424e52ff70f38c882cfcee0939ef6ceac
validation-v1a exact config SHA-256 = 3f82e27ae67b7afa443d58926de8c6901147bc9c1a788e778d1c91a90555ecbd
validation-v1b exact config SHA-256 = 8121c4b4bd6a538639e6b42df14ffc51cff93a2537f88c85b4a421c1ca359e72
```

Build A startował po niezależnie potwierdzonym braku release binary. Build B
wykonał canonical clean `Removed 55 files, 431.8 MiB`, po którym guard również
potwierdził brak binarki. Porównanie bajtowe obu ELF i obu criteria zwróciło
zgodność; w żadnym ELF nie znaleziono lokalnej ścieżki source worktree. Exact
config map używa pełnych runtime run IDs, a nie skrótów administracyjnych.
Ten proof został następnie wycofany jako finalny lock, ponieważ launcher
preflight wykrył niezahashowaną zależność od configured payera. Nie rozpoczęto
runtime ani tmuxa.

## D8. Następne kroki

1. Zacommitować symetryczny ephemeral-payer config i pending criteria.
2. Ponownie wykonać dwa canonical buildy i criteria lock.
3. Wykonać fail-closed launcher dry-run z dokładnym `validation-v1a` configiem.
4. Po pozytywnym dry-runie uruchomić `validation-v1a`, następnie niezależne
   `validation-v1b`, bez strojenia pomiędzy runami.
5. Authority cutover pozostaje zabroniony do czasu source-recomputed
   `promotion_gate_passed=true`.
