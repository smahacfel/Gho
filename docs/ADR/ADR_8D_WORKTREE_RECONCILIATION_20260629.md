# ADR-8D: Worktree Reconciliation Gho to TSV2 Clean

Data: 2026-06-29

Status:

```text
ACCEPTED_AS_LOCAL_WORKTREE_RECONCILIATION
```

## D1. Problem

Istnieja dwa lokalne worktree:

- `/root/Gho`
- `/root/Gho-tsv2-a1-a2-clean`

`/root/Gho-tsv2-a1-a2-clean` jest nowsza linia Git/mainline i zawiera P0 Shadow Fidelity Audit, downgrade pack oraz rozpoczety kontrakt Shadow V2. `/root/Gho` zawiera jednak czesc starszych lokalnych plikow roboczych, raportow audytowych, skryptow i konfiguracji, ktorych nie ma w `tsv2`.

Ryzyko: dalsza praca tylko w `tsv2` moglaby pominac nie-run-artifact pliki istniejace w `/root/Gho`.

## D2. Decision

Wykonano lokalna migracje-kopie z `/root/Gho` do `/root/Gho-tsv2-a1-a2-clean` dla plikow:

- `.rs`
- `.py`
- `.md`
- `.toml`

Kopiowanie bylo ograniczone do plikow niebedacych artefaktami runow. Nie usuwano niczego z `/root/Gho`.

## D3. Scope

Dozwolone klasy plikow:

- kod Rust i Python,
- dokumenty Markdown,
- konfiguracje TOML,
- raporty audytowe niebedace artefaktami runow.

Wykluczone klasy plikow:

- `.git`,
- `target`,
- `__pycache__`,
- `*.pyc`,
- `logs/`,
- `data/`,
- `datasets/`,
- `_archive*`,
- `reports/selector/shadow-burnin-*`,
- `reports/selector/shadow_run`,
- `configs/rollout/logs`,
- `configs/rollout/reports`,
- `runtime.log`,
- katalogi `run_lifecycle_guard_*`.

## D4. Conflict Policy

Dla plikow istniejacych w obu worktree zastosowano regule:

```text
keep destination when destination mtime >= source mtime
overwrite destination only when source mtime > destination mtime
```

W praktyce:

- 22 brakujace pliki zostaly skopiowane z `/root/Gho`,
- 1 konflikt zostal nadpisany wersja z `/root/Gho`,
- 1487 wspolnych kandydatow pozostawiono w wersji z `/root/Gho-tsv2-a1-a2-clean`.

Jedyny nadpisany konflikt:

```text
ghost-brain/ghost_brain_config.toml
```

## D5. Evidence

Skopiowane brakujace pliki:

```text
polecenia.md
configs/rollout/shadow-burnin-v3-p37-x5s-working-builder-account-source-smoke.local.toml
configs/rollout/shadow-burnin-v3-p37-x4s-probe-working-builder-parity-smoke.local.toml
configs/rollout/shadow-burnin-v3-score-tail-v1-r1.local.toml
configs/rollout/shadow-burnin-v3-p37-x8as-bcv2-exact-watch-coverage-restoration-smoke.local.toml
configs/rollout/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r1.toml
configs/rollout/shadow-burnin-v3-p37-x7s-bcv2-local-coverage-smoke.local.toml
configs/rollout/shadow-burnin-buy-heavy.local.toml
configs/rollout/ghost_brain_buy_heavy.local.toml
configs/rollout/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1.toml
configs/rollout/shadow-burnin-v3-p37-x6s-bcv2-readiness-reconciliation-smoke.local.toml
configs/rollout/shadow-burnin-v3-p37-x3s-working-builder-account-source-smoke.local.toml
configs/rollout/shadow-burnin-v3-p37-x2s-working-builder-parity-shadow-smoke.local.toml
configs/rollout/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target24-stop3-fsc-off-r1.toml
RAPORTY/AUDYT_SHADOW_BURNIN_SIMULATION_ONCHAIN_TRUST_20260623.md
scripts/porownaj_zbiory.py
scripts/zbiory.py
docs/ADR/ADR_8D_AUDYT_SHADOW_BURNIN_SIMULATION_ONCHAIN_TRUST_20260623.md
docs/ADR/ADR_8D_POROWNAJ_ZBIORY_ANTI_LEAKAGE_20260623.md
docs/ADR/ADR_8D_ZBIORY_LIFECYCLE_MAIN_RECORD_SELECTION_20260622.md
docs/ADR/ADR_8D_R47_R38_REPEAT_THRESHOLD_PROBE_START_20260624.md
docs/ADR/ADR_8D_R48_R38_TARGET24_STOP3_ROLLOUT_PROFILE_20260625.md
```

## D6. Runtime Boundary

Ta operacja nie uruchamia runtime, nie startuje runow, nie zatrzymuje R51, nie wykonuje RCE proof i nie stage'uje plikow.

Uwaga: `ghost-brain/ghost_brain_config.toml` zostal nadpisany nowsza lokalna wersja z `/root/Gho`, wiec przed jakimkolwiek uruchomieniem runtime wymagana jest osobna decyzja wlasciciela, czy ta konfiguracja ma byc aktywnie uzyta.

## D7. Consequences

`/root/Gho-tsv2-a1-a2-clean` zawiera teraz brakujace nie-run-artifact pliki robocze z `/root/Gho`, przy zachowaniu nowszej linii P0/PR1 tam, gdzie pliki docelowe byly nowsze.

Stare artefakty runow, raw datasets, logi i katalogi run lifecycle nie zostaly przeniesione w ramach tej operacji.

## D8. Verification

Weryfikacja po operacji powinna obejmowac:

- `git status --porcelain=v1 --untracked-files=all`,
- `git diff --check`,
- `git diff --cached --name-only`.

Ta operacja pozostaje lokalna do czasu osobnej decyzji o stagingu lub commicie.
