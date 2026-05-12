# Secret Hygiene i Rollout Profiles

Ten dokument definiuje kontrakt PR-6 dla sekretów runtime, walletów rolloutowych i gotowych profili uruchomieniowych.

## Zasady niepodlegające negocjacji

- Repo przechowuje tylko kod i bezpieczne szablony configów.
- Sekrety endpointów i ścieżki do walletów produkcyjnych są dostarczane wyłącznie przez env lub lokalny `.env`.
- Funding wallet rolloutowy jest oddzielony od innych aktywów operatora.
- Każdy profil rolloutowy ma jednoznaczny `execution_mode` i `entry_mode`.
- `future-live` pozostaje artefaktem konfiguracyjnym do czasu zamknięcia PR-7.

## Zmienne środowiskowe runtime

| Zmienna | Znaczenie |
| --- | --- |
| `GHOST_SEER_GRPC_ENDPOINT` | gRPC endpoint dla ingestu |
| `GHOST_SEER_GRPC_X_TOKEN` | token x-token dla Yellowstone/Chainstack |
| `GHOST_SEER_RPC_ENDPOINT` | RPC pomocniczy Seera |
| `GHOST_TRIGGER_RPC_URL` | RPC używany przez Trigger |
| `GHOST_TRIGGER_KEYPAIR_PATH` | ścieżka do rollout walleta |
| `GHOST_TRIGGER_SHADOW_RPC_URL` | RPC dla shadow-run |
| `GHOST_TRIGGER_JITO_ENDPOINT` | endpoint Jito dla profili live/dual |
| `GHOST_ENV_FILE` | opcjonalna ścieżka do pliku `.env` innego niż rootowy |

Loader launchera i GUI najpierw sprawdza procesowe env, potem lokalny `.env`, a dopiero na końcu bezpieczne placeholdery z trackowanych configów.

## Polityka walletów

1. `shadow-burnin` używa osobnego walleta rolloutowego, bez innych aktywów.
2. `paper-burnin` pozostaje osobnym legacy walletem kompatybilnościowym; nie współdziel go z `shadow-burnin`.
3. `dual-micro-live` używa osobnego walleta niż `shadow-burnin` i `paper-burnin`.
4. `future-live` ma mieć własny wallet przygotowany dopiero po formalnym go/no-go.
5. Funding ma być mały i jawnie ograniczony do wartości potrzebnej dla jednego slotu ekspozycji.
6. Po cleanupie lub incydencie wallet rolloutowy podlega rotacji; nie odzyskujemy starego walleta do kolejnych faz.

## Profile rolloutowe

| Profil | Plik | Semantyka | Status użycia |
| --- | --- | --- | --- |
| `shadow-burnin` | `configs/rollout/shadow-burnin.toml` | `execution_mode=shadow`, `entry_mode=shadow_only`, `funding_lane_mode=full_chain`, `trigger.shadow_run.payer_strategy=ephemeral` | canonical clean rerun po repair stream |
| `paper-burnin` | `configs/rollout/paper-burnin.toml` | `execution_mode=paper`, `entry_mode=shadow_only`, `funding_lane_mode=full_chain` | legacy compare-only / kompatybilność |
| `dual-micro-live` | `configs/rollout/dual-micro-live.toml` | `execution_mode=dual`, `entry_mode=live_and_shadow`, `funding_lane_mode=disabled` | przygotowany, nie uruchamiać przed formalnym GO po shadow-burnin |
| `future-live` | `configs/rollout/future-live.toml` | `execution_mode=live`, `entry_mode=live` | przygotowany, nie używać przed PR-7 |

## Procedura operatora

1. Skopiuj `.env.example` do lokalnego `.env` albo ustaw zmienne środowiskowe w systemd/shellu.
2. Podstaw wyłącznie lokalną ścieżkę do walleta rolloutowego poza repo lub w ignorowanym katalogu `wallets/`.
3. Wybierz konkretny profil rolloutowy z `configs/rollout/`.
4. Uruchom preflight dla wybranego profilu.
5. Jeżeli preflight wypisze placeholder lub brak sekretu, traktuj to jako blokadę startu.
6. Dla repaired `shadow-burnin` oczekuj czystych artefaktów pod `logs/rollout/shadow-burnin-v25-repair/`, `logs/shadow_run/shadow-burnin-v25-repair/` i `datasets/events/shadow-burnin-v25-repair/`; stare dumpy nie są źródłem prawdy dla tego rerunu.
7. Po zakończeniu sesji uruchom `python3 scripts/gatekeeper_v25_repair_validation.py --config configs/rollout/shadow-burnin.toml --json`; brak artefaktów, mixed-plane drift, coverage bez `schema_version >= 5` albo odblokowany promotion lock mają kończyć się `NO-GO`.

## Czego nie robić

- Nie commitować `.env` z realnymi sekretami.
- Nie przywracać `solana/*.json` do trackowanych plików.
- Nie współdzielić jednego walleta między shadow, paper, dual i future live.
- Nie traktować `future-live.toml` jako zgody na start live.
