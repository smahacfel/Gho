# ADR-8D: Pump Research Evidence Tape V1.1 — provenance release artifactu przed provider capture

**Data:** 2026-08-14

**Status:** SUPERSEDED IN PART BY `ADR_8D_PUMP_RESEARCH_TAPE_PR_A_SEALED_BUILD_PROVENANCE_20260815.md` / NO PROVIDER RUN / PR-B BLOCKED

**Task:** `PUMP_RESEARCH_TAPE_PR_A_OPERATOR_PREFLIGHT_PROVENANCE`

> **Supersession note (2026-08-15):** This ADR records the first preflight
> boundary only. Its claims about a copied release binary, all untracked files
> and config non-persistence are replaced by the sealed source-snapshot build,
> required ignored-fixture allowlist and external-config policy in
> `ADR_8D_PUMP_RESEARCH_TAPE_PR_A_SEALED_BUILD_PROVENANCE_20260815.md`.

## D0. Problem i decyzja

Standalone PR-A przechowuje w frozen `run_start_manifest.json` pole
`repository_commit`, ale obecny checkout zawiera mieszany dirty worktree oraz
untracked pliki PR-A. Sam `git rev-parse HEAD` nie dowodzi więc, że commit
zawierał dokładny kod executable, który stworzył raw tape.

Nie rozszerzamy frozen `PumpResearchRunStartManifestV1` ani żadnego raw V1
recordu. Zamiast tego wprowadzono osobny, immutable operator preflight bundle
i run-local binding sidecar:

```text
release binary
→ local-only preflight bundle (create_new)
→ capture revalidates exact provenance before provider I/O
→ raw/<run_id>/operator_preflight_binding_v1.json
→ existing frozen manifest / segments / completion receipt
```

Preflight jest warunkiem koniecznym `capture`, nie dodatkowym raportem, który
operator może pominąć. `capture` przyjmuje obowiązkowo:

```text
--provenance-receipt <.../operator_preflight_receipt_v1.json>
```

## D1. Release-only operator contract

`preflight` i `capture` odrzucają debug binary przez `cfg!(debug_assertions)`.
Prawidłowa procedura jest następująca:

```bash
cargo build --release -p seer --bin pump-research-tape

target/release/pump-research-tape preflight \
  --config configs/rollout/pump-research-tape-v1.toml \
  --output datasets/pump-research/preflight/<operator-preflight-id>

target/release/pump-research-tape capture \
  --config configs/rollout/pump-research-tape-v1.toml \
  --provenance-receipt datasets/pump-research/preflight/<operator-preflight-id>/operator_preflight_receipt_v1.json
```

`preflight` jest local-only. Nie otwiera Yellowstone streamu, nie wykonuje RPC
i nie tworzy raw runu. Weryfikuje natomiast, że:

- endpointy nie są literalnymi template placeholderami;
- endpoint nie zawiera `userinfo`, query ani fragmentu, więc credentials nie
  trafiają do configu ani receiptu;
- jeśli config wskazuje `grpc_auth_token_env`, zmienna istnieje i nie jest
  pusta, lecz jej wartość nigdy nie jest logowana ani persystowana;
- output bundle jest poza worktree albo w Git-ignored location — bundle nie
  może sam zmienić statusu/patche’a, który poświadcza.

## D2. Immutable preflight bundle

`operator_preflight_receipt_v1.json` wiąże SHA-256 i BLAKE3 następujących
artefaktów:

- copy dokładnego executable `release/pump-research-tape`;
- `git_status_porcelain_v1.bin` z pełnym `git status --porcelain=v1 -z`;
- `tracked_worktree.patch` z pełnym `git diff --binary --no-ext-diff HEAD`;
- `untracked_inventory_v1.json` z hashami wszystkich Git-untracked regular
  files oraz snapshotami untracked kodu, fixtures i config/ADR subsetu PR-A;
- `Cargo.lock`;
- zredagowany snapshot finalnego configu — ma hash endpointów, nazwę env-var
  tokenu i parametry capture, lecz nie zawiera endpoint literals ani tokenu;
- wersje `rustc -Vv`, `cargo -V` i jeden canonical provenance fingerprint.

Zachowany `repository_commit` jest opisany jako Git parent commit, a nie jako
pełna identity dirty source tree. Pełna identity wynika z jego połączenia z
patch, status, inventory, Cargo lock i exact binary copy.

## D3. Revalidation przed capture

Przed pierwszym ProgramData RPC albo source connection `capture`:

1. sprawdza integrity każdego sidecaru preflight bundle;
2. ponownie wylicza status, tracked patch, untracked inventory, Cargo.lock,
   binary hash, config, redacted config i toolchain;
3. failuje przy najmniejszym driftcie;
4. dopiero potem rozwiązuje credential do pamięci i kontynuuje normalny PR-A
   start receipt / writer / Yellowstone lifecycle;
5. zapisuje `operator_preflight_binding_v1.json` w raw directory przez
   `create_new`, wiążąc `run_id` z digestem receipt i provenance fingerprintem.

Nie ma fallbacku do `repository_commit` samego w sobie, do debug binary, do
najbliższego builda z `target/`, do zmodyfikowanego configu ani do starego
receipt.

## D4. Granice i niezmienione kontrakty

Nie zmieniono:

```text
frozen PumpResearchRawRecordV1 / segment header/footer / raw binary V1
Geyser source profile i source-filter semantics
active connect_geyser / SeerConfig / Event Bus
Gatekeeper / MaterializedFeatureSet / AccountStateCore / execution
PR-B materializer, certifier, exporter i qualification
```

Preflight nie jest provider qualification. Nie dowodzi gRPC connectivity,
source completeness, direct/CPI/router/v0 coverage, canonicality, ProgramData
start/completion equality ani exact trajectories. Te dowody pozostają po
operator-approved observe-only prospective capture, a PR-B nadal nie może
rozpocząć się przed inspekcją jego immutable output.

## D5. Weryfikacja

Dodano regresje dla:

- odrzucenia placeholdera oraz endpointu z inline/query credential;
- odmowy debug binary dla operator preflight/capture;
- integrity receipt bundle: poprawny bundle przechodzi, zmiana copied release
  binary jest wykrywana fail-closed;
- CLI: `capture` wymaga zarówno `--config`, jak i `--provenance-receipt`, a
  `preflight` wymaga `--config` i nowego `--output`.

Nadal trzeba osobno wykonać release build, operator preflight z zatwierdzonym
configiem oraz provider-backed observe-only capture. W aktualnym checkoutcie
template endpointy i brak tokenu celowo uniemożliwiają zapieczętowanie realnego
operator receipt.

## D6. Rollback

Rollback nie usuwa żadnego raw artifactu. Przed pierwszym capture można po
prostu nie uruchamiać `preflight`; wtedy `capture` failuje bez provider I/O.
Nie wolno przywracać debug `cargo run`, czynić receipt opcjonalnym, dopisywać
provenance do frozen V1 binary structs ani zastępować exact receipt zwykłym
`git rev-parse HEAD`.
