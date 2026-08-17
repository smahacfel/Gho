# Skorygowany plan wykonawczy: Pump Research Evidence Tape V1.1

Status: **COMPLETE — GO-D VERIFIED** / CS0 + PR-A PASSED / sealed provenance v5 PASSED / replacement canary PASSED / GO-D `pump-research-1786909252793-3799414` IS THE FINAL VERIFIED FROZEN HISTORICAL SOURCE AUTHORITY / PR-B offline exact materializer and exporter implemented / GO-E EXTERNAL AUDIT RETIRED AND NOT A GATE / no new capture, RPC backfill, runtime, Gatekeeper or execution change authorized
Data zamrożenia planu: 2026-08-13
Repozytorium robocze: /root/Gho_ingest
Lokalny baseline: 832728c9af9aec92bfa3edea8fa9518ee90f7d5b
origin/main odniesienia: 43057b296663129ca9b4f572e793474830a5452c

> **NORMATIVE FINAL AUTHORITY — SUPERSEDES EVERY EARLIER GO-E GATE:**
>
> `GO_D_SOURCE_AUTHORITY = VERIFIED`
>
> `GO_E_EXTERNAL_AUDIT = RETIRED / NOT A GATE`
>
> Niepowodzenie, niedostępność, HTTP 503, pruning ani rate limit zewnętrznego
> RPC nie blokują, nie unieważniają i nie opóźniają pracy na GO-D. Historyczne
> Amendmenty G.3–G.5.2.3 pozostają audytowalnym zapisem wykonanej pracy, lecz
> nie są już sekwencją promotion authority.

## 1. Podsumowanie i macierz korekt

Architektura, zakres oraz kolejność realizacji pozostają bez zmian:

~~~text
PRIMARY YELLOWSTONE
→ source tap przed lossy projection/filtering
→ immutable bounded raw capture
→ versioned offline materializer
→ exact tape
→ generic window exporter
→ strategy experiments
~~~

Kolejność wdrożenia pozostaje:

~~~text
CS0
→ PR-A
→ capture-enabled local A/B gate
→ operator-approved observe-only prospective raw capture
→ inspection immutable raw output
→ PR-B
→ qualification
~~~

### Amendment A — pre-prospective gate i ownership control plane (2026-08-14)

To doprecyzowanie jest normatywne i zastępuje wcześniejsze, sprzeczne skróty
`PR-A → prospective capture`.

1. Po PR-A najpierw musi przejść lokalna bramka capture-enabled A/B. Nie jest
   nią direct-ingress micro-regression ani istniejący PR1B release harness.
   Bramka mierzy rzeczywistą standalone ścieżkę z
   `PumpResearchCaptureIngressV1` i writerem. Capture-disabled no-sink arm
   jest wyłącznie telemetryczny; nie jest odpowiednikiem bounded hand-offu.
   Jej właściwy kontrakt metryczny dla standalone PR-A definiuje Amendment B.
   Raportuje również opóźnienie
   `fatal_reason_recorded → source_cancel_dispatched` przy wymuszonym wolnym
   flush/sync/rotacji. `WRITER_IDLE_POLL_V1` ogranicza jedynie bezczynny poll;
   nie stanowi twardej gwarancji tego opóźnienia podczas filesystem I/O.
2. Dopiero po przejściu tej bramki oraz osobnej decyzji operatora wolno
   uruchomić observe-only prospective raw capture na jawnie dostarczonych
   endpointach i credentialach. Następnie należy obejrzeć immutable manifest,
   segmenty i completion receipt przed rozpoczęciem PR-B.
3. Zarezerwowana control lane przenosi wyłącznie ordered
   `DroppedSource` markers. Source lifecycle jest jawnym stanem atomowym
   ingressu, a segment lifecycle należy wyłącznie do writera i jego
   footerów/receiptów. Nie ma lifecycle markerów na control lane.

### Amendment B — semantycznie porównywalna bramka standalone (2026-08-14)

W sekcji 4.5 pierwotne ratio `throughput >= 0.98` i `p99 <= 1.05` odnosiło
się do wariantu, w którym capture jest dodatkowym hookiem istniejącego parser
workera. Lokalna implementacja PR-A ma inną, świadomie zamrożoną granicę:
research sink przejmuje decoded `SubscribeUpdate` i nie wywołuje
`route_update()` ani parsera. Capture-disabled arm wykonuje więc **brak
odpowiednika** bounded hand-offu, a nie tę samą pracę bez zapisu.

Nie wolno udawać porównywalności przez:

- ratio względem no-op source arm;
- równoległy parser+writer, którego standalone capture nie uruchamia;
- obniżenie starego progu tylko po to, by przejść timing test.

Zastępująca bramka local PR-A pozostaje strict i mierzy dokładnie aktywną
granicę source:

1. Uruchamia prawdziwy `PumpResearchCaptureIngressV1`, bounded writer,
   deterministic V1 encoding i publikację segmentu.
2. Przekazuje 8 192 decoded Pump `SubscribeUpdate` przy capacity 16 384.
3. Wymaga `received == admitted == accepted == 8_192`, zero dropped updates,
   zero typed ingress gaps, zero writer errors, jeden zamknięty segment oraz
   brak nieoczekiwanego source abort.
4. Wymaga enabled source-side `try_capture` p99 `<= 100 µs`. Jest to
   bezpośrednia, absolutna bramka przeciw blockingowi receive path, nie ratio
   z no-opem.
5. Wymaga osobno frozen capture-disabled parser parity; nie zapisuje
   nieprawdziwego `parser_worker_blocking_waits = 0`, bo standalone capture
   nie posiada parser workera.
6. Nadal mierzy `fatal_reason_recorded → source_cancel_dispatched` podczas
   test-only 50-ms slow flush/sync/rotation. `WRITER_IDLE_POLL_V1 = 5 ms`
   nie jest deadline'em podczas filesystem I/O.

Jest to konieczna korekta definicji dowodu, a nie poluzowanie performance
gate: nowy p99 dotyczy dokładnie rzeczywistego receive hand-offu i jest
nieporównanie ciaśniejszy niż test sprawdzający jedynie brak blokowania w
skali milisekund.

### Execution receipt A — local capture-enabled A/B (2026-08-14)

Normatywna bramka z Amendment A przeszła lokalnie, zanim dopuszczono kolejny
krok operacyjny. Wykonano dokładnie:

~~~bash
cargo test --release -p seer --lib \
  research_tape::tests::pr_a_capture_enabled_local_ab_harness \
  -- --ignored --nocapture --test-threads=1
~~~

Receipt czterech niezależnych release executions:

~~~text
events / received / admitted / accepted          = 8,192 / 8,192 / 8,192 / 8,192
dropped updates / ingress gaps / writer errors   = 0 / 0 / 0
segments / clean shutdown                         = 1 / true
enabled `try_capture` p99                         = 260 / 211 / 230 / 330 ns
source-ingress p99 limit                           = 100,000 ns
fatal_reason → source_cancel during 50-ms slow I/O = 53,361,194 / 62,069,322 / 53,754,519 / 53,138,762 ns
~~~

Disabled no-sink throughput jest tylko telemetrycznym punktem odniesienia: nie
ma identycznego queue hand-offu, dlatego nie jest SLA. Harness mierzy actual
standalone ingress/writer, nie twierdzi, że standalone capture posiada parser
worker ani że 5-ms idle poll jest twardym deadline'em w czasie filesystem I/O.
Szczegóły i granice dowodu zapisano w
`docs/ADR/ADR_8D_PUMP_RESEARCH_TAPE_PR_A_CAPTURE_ENABLED_AB_GATE_20260814.md`.
Nie wykonano Yellowstone/RPC capture, nie użyto credentialu i nie utworzono
datasetu. Kolejny krok nadal wymaga osobnej decyzji operatora.

### Amendment C — release artifact i dirty-worktree provenance przed provider capture (2026-08-14)

> **SUPERSEDED FOR BUILD, INVENTORY AND CONFIG PROVENANCE BY AMENDMENT D.**
> This historical section is retained only to explain the original gap; its
> commands and its claim about an all-untracked inventory are not normative.

`repository_commit` w frozen `PumpResearchRunStartManifestV1` pozostaje
wyłącznie Git parent commit. W dirty/untracked worktree nie wolno interpretować
go jako kompletnej identity kodu PR-A. Nie zmieniamy przez to frozen V1
manifestu ani binary layoutu; dodajemy zewnętrzny, immutable operator receipt
oraz jego run-local binding sidecar.

Przed **każdym** realnym capture operator musi wykonać, z jawnie zbudowanej
binarnej wersji release:

~~~bash
cargo build --locked --release -p seer --bin pump-research-tape

target/release/pump-research-tape preflight \
  --config configs/rollout/pump-research-tape-v1.toml \
  --output datasets/pump-research/preflight/<operator-preflight-id>

target/release/pump-research-tape capture \
  --config configs/rollout/pump-research-tape-v1.toml \
  --provenance-receipt datasets/pump-research/preflight/<operator-preflight-id>/operator_preflight_receipt_v1.json
~~~

`preflight` jest local-only i fail-closed. Wymaga release binary, zatwierdzonych
nie-placeholder endpoint references oraz obecnego, niepustego tokenu, jeśli
config wskazuje `grpc_auth_token_env`; nie wykonuje Yellowstone ani RPC i nie
persistuje wartości credentialu. Bundle jest `create_new` i musi być poza
worktree albo w Git-ignored ścieżce, aby sam nie zmieniał source snapshotu.

Bundle zachowuje:

- exact copied release executable oraz jego SHA-256 i BLAKE3;
- `HEAD`, branch, pełny `git status --porcelain -z` i pełny tracked
  `git diff --binary HEAD`;
- hash `Cargo.lock`, wersje `rustc` i `cargo`;
- pełny inventory/hash wszystkich untracked plików oraz snapshot
  source/config/fixture subsetu PR-A;
- hash finalnego configu oraz jego redacted snapshot bez endpoint literals i
  bez token value;
- canonical artifact provenance fingerprint.

`capture` nie rozpoczyna nawet ProgramData RPC, jeżeli receipt nie istnieje,
nie jest release, ma naruszony sidecar albo bieżący executable, config,
`Cargo.lock`, tracked patch, status, untracked inventory lub toolchain różni
się od receipt. Po udanej rewalidacji zapisuje w raw run directory
`operator_preflight_binding_v1.json`, wiążąc konkretny `run_id` z digestem
preflight receipt. Żaden z tych artefaktów nie rozszerza frozen V1 raw record
ani run manifestu.

> **HISTORYCZNE — ZASTĄPIONE PRZEZ AMENDMENT E.** W chwili redakcji
> Amendment D nie istniał jeszcze realny provider profile ani capture. Dwa
> późniejsze runy są opisane i objęte kwarantanną w Amendment E; nie wolno
> interpretować tego historycznego stwierdzenia jako aktualnego statusu.

### Amendment D — mechaniczny build/source receipt, ignored fixtures i external config (2026-08-15)

Amendment C nie wystarczał do deklaracji exact provenance: wspólny hash
bieżącego source i bieżącej binary nie dowodził, że binary została z tego
source zbudowana. Ponadto zwykłe `git ls-files --others --exclude-standard`
pomijało ignored `corpus_manifest_v1.json`, a surowy config mógł trafić do
snapshotu/patcha mimo redacted sidecaru. Niniejszy amendment **zastępuje**
wcześniejszy opis operator receipt w tych trzech punktach.

#### D.1. Sealed build chain

`target/release/pump-research-tape` jest wyłącznie non-debug bootstrapem.
`preflight`:

1. wymaga, aby faktyczny config operatora był regularnym plikiem poza Git
   worktree;
2. tworzy pełny immutable source snapshot bieżącego worktree: wszystkie
   tracked regular files, wszystkie non-ignored untracked regular files oraz
   jawnie allowlistowany required ignored fixture
   `ghost-core/tests/fixtures/pump_research_tape_v1/corpus_manifest_v1.json`;
3. zapisuje `source_snapshot_manifest_v1.json` z SHA-256 i BLAKE3 każdego
   pliku;
4. buduje **ze snapshotu**, w create-new temporary `CARGO_TARGET_DIR`, przez:

   ~~~bash
   cargo build --locked --offline --release -p seer --bin pump-research-tape
   ~~~

5. sprawdza snapshot **oraz build environment** ponownie po buildzie,
   utrwala command, digests `RUSTFLAGS`, compiler-wrapper/build env oraz
   możliwych Cargo config files — Cargo home, snapshot i ancestors build cwd —
   (bez kopiowania ich contents), build log i build receipt;
6. kopiuje dopiero tę binary do `release/pump-research-tape` bundle'a oraz
   wiąże jej digest z snapshotem i build receipt.

Operator uruchamia capture **wyłącznie** przez sealed binary z bundle'a, z
repo root jako cwd:

~~~bash
cargo build --locked --release -p seer --bin pump-research-tape

target/release/pump-research-tape preflight \
  --config /protected/operator/pump-research-tape-v1.toml \
  --output datasets/pump-research/preflight/<operator-preflight-id>

datasets/pump-research/preflight/<operator-preflight-id>/release/pump-research-tape capture \
  --config /protected/operator/pump-research-tape-v1.toml \
  --provenance-receipt datasets/pump-research/preflight/<operator-preflight-id>/operator_preflight_receipt_v1.json
~~~

`capture` rewaliduje bundle, pełny bieżący source manifest, Cargo.lock,
config, toolchain i **digest własnego executable** przed pierwszym RPC. Nie
wolno uruchomić do capture bootstrap binary z `target/release`, starej binary
ani binary z innego target profile.

Regresja wywołuje publiczne `capture` z invalid receipt i testowym probe'em
tuż przed pierwszym ProgramData RPC; wymaga zera wejść do fazy provider I/O.

#### D.2. Inventory i ignored fixtures

Zwykłe ignored files pozostają poza snapshotem (nie obejmujemy `target/`,
datasets ani dowolnych cache'ów). Required ignored files są jednak jawnie
wersjonowaną allowlistą. Brak fixture, brak jego Git-ignore statusu, jego
hash drift albo brak go w source snapshotie jest fatalnym błędem preflightu lub
capture revalidation — nie cichym pominięciem.

#### D.3. Config i endpoint policy

Finalny operator TOML nie może być plikiem tracked ani untracked wewnątrz
worktree. Bundle zapisuje wyłącznie jego digest i redacted projection. Endpoint
jest wymagany jako root-only public HTTPS origin: bez userinfo, path, query i
fragmentu. Credential gRPC i optional read-only RPC credential pochodzą
wyłącznie z nazwanych zmiennych środowiskowych oraz headerów; nie wolno wkładać
credentialu do URL.

Bundle jest mimo to **wrażliwym artefaktem forensic**: pełny source snapshot,
tracked patch i build log mogą zawierać niepowiązane lokalne dane użytkownika.
Preflight tworzy directory `0700` na Unix, lecz operator nie może go publikować
ani traktować jako automatycznie zredagowanego exportu.

#### D.4. Binding time semantics

Receipt jest walidowany w czystej local phase przed zbudowaniem clienta RPC
lub source connection. Raw run directory nadal powstaje dopiero po udanym
start ProgramData receipt. Dlatego run-local binding jawnie zachowuje dwa
czasy: `receipt_validated_wall_ms` (przed pierwszym provider I/O) oraz
`binding_written_wall_ms` (po udanym start receipt). Nie twierdzi, że binding
file powstaje przed RPC.

Amendment D nie jest provider qualification i nie autoryzuje realnego
capture'u sam przez fakt kompilacji. Po lokalnej weryfikacji sealed preflightu
nadal konieczne są operator approval endpointów/credentialów, observe-only
run, inspection immutable output i dopiero później decyzja o PR-B.

### Amendment E — kwarantanna historycznego runu i build/auth isolation (2026-08-16)

> **NORMATYWNY STATUS OPERACYJNY.** Ten amendment zastępuje wyłącznie
> historyczne stwierdzenia `PROSPECTIVE CAPTURE NOT STARTED` i `PR-B PENDING`.
> Nie promuje żadnego istniejącego artefaktu do qualification ani Ready.
>
> Dla sealed builda, inventory, operator-config secrecy oraz PR-A ProgramData
> RPC auth niniejszy amendment zastępuje techniczne postanowienia Amendment D.
> Amendment D pozostaje historycznym opisem poprzedniego kontraktu, a nie
> alternatywną ścieżką wykonania.

Od poprzedniego receipt powstały dwa rzeczywiste raw runy oraz developmentowy
output materializera. Są zachowane bez mutacji jako forensic evidence:

```text
pump-research-1786810400363-3428808
  = INCOMPLETE / interrupted capture / not certifiable

pump-research-1786810567606-3429034
  = CaptureLifecycleComplete + LocalAccountingComplete
  + ProvenancePreflightVulnerable + IndependentCompletenessUnproven
  + QualificationNoGo

exact-prb-20260816-2
  = UNQUALIFIED / development-only / no export / no strategy
```

Drugi raw run ma poprawny lokalny source-to-disk accounting i clean shutdown;
nie jest jednak dowodem qualification. Nie wolno go retroaktywnie "naprawić"
nowym preflight receipt, wykonywać na nim independent qualification auditu,
uruchamiać `export-window` ani używać go jako universe dla strategii. Pierwszy
`.partial` i nieopublikowany `.exact-*.partial` pozostają evidence failure
lifecycle i nie podlegają czyszczeniu.

#### E.1. Hermetyczny fresh build

Wcześniejszy fresh build odziedziczał środowisko parent procesu. To pozwalało
credentialom operatora trafić do Cargo, rustc, build-scriptów oraz trwałego
`release/build.log`. Nowy preflight:

1. odrzuca przed utworzeniem final receipt każde niepuste, nieuszczelnione
   override kompilacji (`RUSTC`, compiler wrappers, `RUSTFLAGS`,
   `CARGO_ENCODED_RUSTFLAGS`, `CARGO_BUILD_*` i `CARGO_PROFILE_RELEASE_*`);
2. wykonuje Cargo przez `env_clear()` z minimalnym jawnie utworzonym child
   environment (`PATH`, fresh `HOME`, fresh `CARGO_HOME`, fresh
   `CARGO_TARGET_DIR`, offline mode); uruchamia bezpośrednio zahashowane
   binary wybranego toolchainu zamiast rustup proxy;
3. nie przekazuje nazwanych gRPC/RPC credential environment variables do
   child procesu;
4. udostępnia fresh Cargo home wyłącznie z cache/index/git DB potrzebnymi do
   `--locked --offline`, bez parent `credentials`, configu i checkoutów;
5. zapisuje canonical executable paths oraz SHA-256/BLAKE3 użytych Cargo i
   rustc, obok ich wersji;
6. przed publikacją finalnego receipt skanuje wszystkie regular files
   sealed bundle'a dokładnymi bytes obu skonfigurowanych credentiali.

Trafienie credentialu przerywa preflight bez publikacji finalnego
`operator_preflight_receipt_v1.json`; skan jest defence in depth i nie
zastępuje scrubowania environment. Historyczny receipt z poprzednią semantyką
nie przechodzi walidacji nowej sealed binary.

#### E.2. Izolacja standalone ProgramData RPC

ProgramData start/completion receipt w PR-A ma odtąd tylko dwa jawne tryby:

```text
explicit configured RPC credential -> standalone explicit-auth client
no configured RPC credential       -> standalone no-auth client
```

Żaden z nich nie używa generic legacy-aware clienta ani nie dziedziczy
`GHOST_SEER_GRPC_X_TOKEN` bądź innego globalnego auth fallbacku. Ta korekta
dotyczy wyłącznie standalone research capture; nie zmienia legacy runtime
Seera ani jego klienta RPC.

#### E.3. Bramy przed replacement capture

Do czasu kompletnego local-only review Amendment E zakazane są kolejne realne
preflighty, capture, provider audit, qualification, `export-window` i
strategy runs. Następna dozwolona kolejność jest następująca:

```text
targeted regressions + full PR-A review
-> local synthetic sealed preflight with external non-secret config
-> operator credential rotation/check and operator GO
-> short observe-only replacement canary
-> immutable-output inspection
-> replacement prospective capture
-> independent qualification
```

PR-B może być rozwijany wyłącznie na skwantannowanym raw materiale jako
development/forensics. `qualification_status = Unqualified` jest celową
blokadą i musi pozostać skuteczna do czasu replacement runu oraz independent
source-completeness proof.

### Amendment F — sealed Cargo-config closure i mechaniczna kwarantanna provenance (2026-08-16)

> **NORMATYWNY STATUS.** Ten amendment zastępuje w Amendment E wyłącznie
> zbyt szerokie twierdzenie o „hermetycznym” fresh buildzie oraz dokumentacyjną
> kwarantannę historycznego raw runu. Nie osłabia credential scrub, credential
> scan ani standalone no-auth RPC z Amendment E; te kontrakty pozostają
> obowiązujące.

#### F.1. Cargo config jest częścią sealed source albo build failuje

`env_clear()` sam nie domyka Cargo, ponieważ Cargo wyszukuje
`.cargo/config{,.toml}` po ancestorach `current_dir`. Od teraz preflight:

1. materializuje wcześniej zweryfikowany `source_snapshot/` do osobnego,
   create-new staging root w katalogu tymczasowym;
2. uruchamia Cargo wyłącznie z `current_dir = <staging>/source`, z fresh
   `CARGO_HOME`, `HOME` i `CARGO_TARGET_DIR` wewnątrz tego samego staging
   root;
3. sprawdza przed buildem, że żaden ancestor stagingowego source root nie ma
   `.cargo/config.toml` ani `.cargo/config`; obecność takiego pliku kończy
   preflight przed uruchomieniem Cargo;
4. dopuszcza wyłącznie config wewnątrz zweryfikowanego snapshotu, zapisuje
   jego digest pod canonical label `sealed_snapshot/...` i weryfikuje
   snapshot przed oraz po buildzie;
5. odrzuca w snapshotowym configu nieuszczelnione wskazania narzędzi lub
   źródeł: `build.rustc`, compiler wrappers, `rustdoc`, `target-dir`, `[env]`,
   `target.*.(linker|runner|ar)`, `[source]`, `[patch]`, `[replace]`,
   credential providers oraz registry credential-provider.

Pierwotna wersja F.1 dopuszczała dowolne snapshotowane `build.rustflags`.
To dopuszczenie zostało zastąpione przez ścisłą allowlistę F.1a: sam digest
tekstu flagi nie zamyka wskazanego przez nią linkera, obiektu, sysrootu,
biblioteki ani response file poza snapshotem.

Kontrakt nazywa się **sanitized sealed Rust build environment**, a nie
absolutnie hermetyczny host build. Wybrane systemowe narzędzia z controlled
`PATH`, systemowy linker/C compiler wymagany przez zależności oraz read-only
offline Cargo cache/index/git DB pozostają jawnie platformowymi inputami. Nie
deklarujemy image/container digestu ani byte-level closure całego hosta.

#### F.1a. Strict Cargo-config allowlist v5

Snapshotowy `.cargo/config{,.toml}` nie jest już sprawdzany denylistą.
Preflight akceptuje wyłącznie zamknięty kontrakt potrzebny bieżącemu repo:

```toml
[build]
rustflags = ["-C", "target-cpu=native"]
jobs = 4

[profile.release]
opt-level = 3
lto = true
codegen-units = 4
```

Dozwolone są wyłącznie top-level tables `build` i `profile`; `profile` może
zawierać wyłącznie `release`. Wartości zatwierdzonych pól są dokładne, nie
tylko typowane. Każdy nieznany top-level table, nieznany klucz, inna wartość
albo inna reprezentacja `rustflags` kończy preflight przed Cargo.

W szczególności zabronione są:

- `build.target`, nawet jeżeli wskazuje target JSON;
- cała tabela `target`, w tym target-specific `rustflags`, linker i runner;
- `linker=`, `link-arg=`, `link-args=`, `-L`, `--sysroot`, `--extern` oraz
  response files przekazane przez `rustflags`;
- aliasy, source/patch/registry/env/net oraz przyszłe, nieznane powierzchnie
  Cargo configu.

Semantyka receiptu zostaje podniesiona do:

```text
...cargo_config_strict_allowlist_v5
```

Semantyka `...cargo_config_closure_v4` jest od tej chwili legacy i nie może
otrzymać `Ready`, nawet gdy credential scan i independent source audit są
idealne. Zachowany synthetic bundle v4 pozostaje wyłącznie historycznym
local evidence; replacement canary wymaga nowego sealed preflightu v5.

#### F.2. Binding provenance jest bramką Ready, nie tylko dokumentem

Nowy run-local `operator_preflight_binding_v1.json` zapisuje:

```text
build_semantics                         = ...cargo_config_strict_allowlist_v5
credential_scan_semantics               = configured_operator_credential_bytes_absent_from_sealed_bundle_v1
qualification_provenance_eligible       = true
sealed_release_binary_digest             = digest release_binary_digest z receiptu
```

Certifier zawsze odczytuje binding podczas indeksowania raw runu. Brak,
malformed, legacy, nieobsługiwana semantyka, `eligible = false` albo
niespójny sealed binary digest pozwalają wyłącznie na development
materialization. Niezależny source-completeness audit nie może nadpisać tej
bramki: exact manifest otrzymuje
`Blocked(CaptureProvenanceUnqualified)`, nigdy `Ready`.

W szczególności historyczny
`pump-research-1786810567606-3429034` ma poprzednią semantykę preflightu i
nie może zostać retroaktywnie promowany, nawet przy idealnym audycie. Jego
raw bytes oraz `exact-prb-20260816-2` pozostają bez mutacji,
development/forensic-only; istniejący exporter nadal odrzuca ich
`Unqualified` output.

#### F.3. Bramy lokalne i kolejność

Obowiązkowe są regresje dla:

- ancestor `.cargo/config.toml` przed stagingiem — fail przed Cargo;
- snapshotowego `rustc-wrapper`/linker/runner — fail;
- `rustflags` z external linker/object/library/sysroot/response file — fail;
- `build.target`, cała tabela `target` i nieznane top-level tables — fail;
- niezatwierdzone wartości `jobs`/release profile — fail;
- dokładnego bieżącego `rustflags`/`jobs`/release profile — pass;
- bindingu `...cargo_config_closure_v4` z idealnym auditem — nie może być
  `Ready`;
- legacy binding z idealnym source auditem — nie może być `Ready`;
- corrected binding z idealnym auditem — może przejść tę konkretną bramkę;
- niespójnego sealed binary digest — nie może być `Ready`.

Pełne local-only review i zachowany synthetic preflight przeszły. Receipt
synthetic bundle'a potwierdza `...cargo_config_strict_allowlist_v5`, wyłącznie
labels `sealed_snapshot/.cargo/config{,.toml}`, create-new staging semantics
oraz zgodność SHA-256 copied sealed executable. Skan wszystkich regular files
bundle'a nie znalazł synthetic credentiali, originów `.invalid` ani ścieżki
external configu. Harness release zachował `8_192 / 8_192 / 8_192` source
records, zero drop/gap i jeden domknięty segment.

To jest wyłącznie dowód lokalny. Realny operator preflight, capture, provider
qualification, export oraz strategia pozostają NO-GO do osobnego review tego
receiptu i osobnego GO operatora.

### Amendment G — GO-D acceptance i qualification preparation boundary (2026-08-16)

> **NORMATYWNY STATUS OPERACYJNY.** Ten amendment nie zmienia frozen raw V1,
> exactness rules ani independent-audit denominatora. Aktualizuje wyłącznie
> faktyczny stan po replacement capture oraz granicę następnego dozwolonego
> działania.

GO-D raw run:

```text
pump-research-1786909252793-3799414
```

został zaakceptowany jako immutable qualification-eligible input. Ma 25
segmentów, `1_661_983` ciągłych source records, zero dropped records, zero
typed gaps, clean terminal footer, zgodne whole-file SHA-256/BLAKE3, poprawny
frozen-V1 decode oraz zgodny ProgramData start/completion receipt. Binding
spełnia semantykę strict Cargo-config allowlist v5 i ma
`qualification_provenance_eligible = true`.

Zewnętrzny OS wait status ad-hoc wrappera GO-D pozostaje `UNKNOWN`. Jest to
trwała uwaga `OperatorSupervisionEvidenceIncomplete`, ale nie blocker raw ani
qualification eligibility: normatywny materializer opiera tę bramkę na
wewnętrznym capture lifecycle, immutable segmentach, ProgramData oraz bindingu
v5. Nie wolno imputować exit statusu ani ponawiać GO-D wyłącznie w celu jego
odtworzenia.

Każdy przyszły capture musi być uruchamiany przez supervisor będący
bezpośrednim rodzicem dokładnego childa. Supervisor nie używa `pgrep`, nie
deleguje ownership do GNU `timeout`, obserwuje exit przez pidfd, wysyła
`SIGINT` dokładnemu child PID, wykonuje jeden finalny `waitpid()` i zapisuje
surowy wait status oraz rzeczywisty exit code albo signal. Startowy próg
wolnego miejsca pozostaje oddzielony od niższego runtime disk floor. Ta
korekta nie wpływa na zaakceptowany GO-D i nie autoryzuje kolejnego capture'u.

Przygotowanie independent qualification jest dozwolone lokalnie. Wykonanie
pozostaje HOLD do osobnego operator GO. Obowiązuje:

1. protected audit config jest regularnym plikiem `0600` poza worktree;
2. `audit_provider_id` oraz faktyczne źródło muszą być niezależne od
   `nln-primary-yellowstone`;
3. endpoint jest root-only HTTPS, bez credentialu w URL/TOML i bez legacy auth;
4. zwykłe `certify` bez `--qualification-audit-config` jest zabronione;
5. combined certify+audit używa nowego, nieistniejącego output directory;
6. każdy status inny niż `Ready` zatrzymuje pipeline;
7. `export-window`, strategia, Gatekeeper i execution pozostają NO-GO.

Structured completion log `certify` musi raportować faktyczny
`qualification_status` zwrócony przez materializer (`Unqualified`, `Ready`
albo typed `Blocked`) i nie może zawsze nazywać outputu unqualified. Manifest
pozostaje SSOT; zmiana dotyczy wyłącznie zgodności operator logu z manifestem.

Wolne miejsce zostało zwiększone powyżej wcześniejszego progu przygotowania.
Nie daje to samoistnego GO do provider I/O; przed startem combined operation
operator ponownie sprawdza aktualne bytes, nowy output path i zapas filesystemu.

Stan przygotowania jest zapisany create-new jako
`datasets/pump-research/operator-logs/go-e-qualification-prep-v1-20260816T222710Z/qualification_preparation_receipt_v1.json`.
Receipt nie autoryzuje wykonania: zachowuje `HOLD_PROVIDER_IO_AND_CERTIFY`,
control hashe raw/configu, brak outputu oraz zerowe rozpoczęcie provider I/O,
certify, qualification, exportu i strategii.

#### Amendment G.1 — future-capture supervisor i qualification snapshot semantics

Korekta nie zmienia zaakceptowanego GO-D, raw bytes, frozen V1 ani eligibility
bieżącego runu. Zamyka wyłącznie dwa kontrakty przyszłego supervisora:

1. Exact child environment jest konstruowane **przed `Popen`**. Oba legacy
   aliasy `GHOST_SEER_GRPC_X_TOKEN` i `GHOST_RPC_AUTH_TOKEN` są usuwane z kopii
   przekazywanej do childa. Config nie może zadeklarować legacy aliasu jako
   dedykowanej zmiennej. Dedykowane zmienne wskazane przez config pozostają
   dostępne wyłącznie dla exact capture childa, a po udanym spawn są usuwane
   ze środowiska procesu supervisora.
2. Exit code `0` childa nie jest wystarczającym dowodem operatorskiego
   sukcesu. Supervisor wymaga jednocześnie dokładnie jednego nowego runu,
   regularnego `raw/run_completion_receipt.json`, zgodnego `run_id`,
   `status = Complete`, `clean_shutdown = true` oraz zera ścieżek
   `*.partial`. Każde naruszenie zapisuje typed operator failure i zwraca
   non-zero, zachowując niezmieniony raw wait status childa.

Qualification preparation receipt jest **create-new snapshotem**, a nie
mechanicznie immutable ani sealed artefaktem. Pozostaje owner-writable i jego
integralność jest wykrywalna przez oczekiwany SHA-256:

```text
qualification_preparation_receipt_v1.json
eab36576a3ad3284fe73da186186f04301a6b5a0809b2e592cf72ca3c7dd0787

/protected/operator/pump-research-audit-v1.toml
c5e1ebb6585639ebe33c70308a838e102d00aa5f45a46012b581e0cb56d9ca16
```

Przed jakimkolwiek przyszłym provider I/O oba hashe muszą zostać policzone
ponownie i związane w nowym create-new operator execution receipt. Sam
preparation snapshot nie jest authority i nie autoryzuje `certify`.

Fizyczna dostępność, retention, pełne `getBlock`, inner instructions, loaded
addresses, failed transactions, tx order oraz bounded capacity niezależnego
providera pozostają `HOLD`. Następnym dopuszczalnym etapem jest osobno
zatwierdzony, read-only i bounded provider-suitability probe bez tworzenia
exact outputu i bez mutacji raw. Combined `certify --qualification-audit-config`
pozostaje zabronione do czasu pozytywnego wyniku tej bramki i osobnego GO.

#### Amendment G.2 — cross-operator-dir capture mutual exclusion

Każdy planowo autoryzowany future capture korzysta z jednego canonical
`output_dir`. Wzajemne wykluczenie jest przypisane do tego fizycznego dataset
root, a nie do dowolnego katalogu logów operatora:

```text
<canonical output_dir>/.pump-research-capture.lock
```

Supervisor rozwiązuje `output_dir` do fizycznej ścieżki, otwiera regularny,
niesymlinkowy lock file z trybem `0600`, przejmuje nonblocking exclusive
`flock` i trzyma go aż do zapisania execution receipt. Lock musi zostać
przejęty przed:

1. skanem aktywnych capture processes;
2. sprawdzeniem miejsca i snapshotem istniejących runów;
3. utworzeniem `operator_dir`;
4. `Popen` exact capture childa.

Dwa supervisory z różnymi `operator_dir` i tym samym canonical `output_dir`
nie mogą więc jednocześnie uruchomić provider streams. Drugi kończy się
fail-closed na locku przed `Popen`. `/proc` scan pozostaje defense-in-depth dla
capture'u uruchomionego poza aktualnym supervisorem, ale nie jest authority
wzajemnego wykluczenia.

Scope locka jest jawnie `canonical_output_directory_v1`, nie host-global.
Równoległe capture'y do różnych output roots nie są autoryzowane przez ten
plan; ewentualny host-global zakaz wymagałby osobnego kontraktu. Amendment nie
zmienia GO-D raw, nie uruchamia future capture i nie daje GO do provider I/O.

#### Amendment G.3 — bounded GO-E0 provider suitability i wynik operacyjny

Stan wykonania opisany w G.3 jest historyczny i dla kolejnych probe'ów został
zastąpiony przez G.4/G.5. Kontrakty bounded read-only, fail-closed i zakazu
tworzenia exact outputu pozostają normatywne.

GO-E0 jest pomocniczą, read-only operacją przygotowania qualification. Nie
zmienia trzech podstawowych operacji tape i nie tworzy nowego źródła raw ani
exact authority. Jawny interfejs ma postać:

```text
pump-research-tape provider-suitability \
  --run-dir <closed-raw-dir> \
  --qualification-audit-config <protected-config> \
  --preparation-receipt <qualification-preparation-receipt> \
  --expected-preparation-sha256 <sha256> \
  --output <new-operator-log-dir>
```

Operacja:

- najpierw wykonuje pełny frozen-V1 index i weryfikację zamkniętego raw;
- parsuje i hashuje dokładnie te same bytes audit configu, eliminując
  config/hash TOCTOU;
- wymaga zgodnych control hashy preparation snapshotu i raw runu;
- używa tego samego explicit-auth albo standalone no-legacy klienta oraz tego
  samego finalized `getBlock` configu co pełny independent audit;
- wymusza concurrency `1`, maksymalnie 16 burst slots, 3 kolejne unavailable
  jako circuit breaker i provider wall budget `420 000 ms`;
- obejmuje first/mid/last qualification range oraz raw representatives dla
  direct top-level, inner CPI, router-to-Pump CPI, v0 loaded address i failed
  Pump transaction;
- porównuje multiset `(slot, tx_index, signature)`, invocation-class counts
  oraz failed-status multiset;
- nie może zapisać outputu wewnątrz raw ani w/poniżej planowanego exact path;
- publikuje atomowo wyłącznie redacted `provider_suitability_receipt_v1.json`;
- zachowuje `raw_write_attempt_count = 0`, `exact_output_created = false` oraz
  zerowe rozpoczęcie certify/export/strategy;
- dla każdego statusu innego niż `ReadyForFullAudit` zwraca non-zero bez
  automatycznego retry pełnej operacji.

Pierwszy autoryzowany GO-E0 został wykonany 2026-08-17 dla GO-D raw
`pump-research-1786909252793-3799414`. Receipt:

```text
datasets/pump-research/operator-logs/
  go-e0-provider-suitability-v1-20260817T062928Z/
  provider_suitability_receipt_v1.json

SHA-256 = 783723443ad47c7e2ae1e9f4d04ac08e37848ea750fd803814de48b6e29fb910
status  = blocked_audit_unavailable
```

Wynik jest fail-closed i nie może zostać reinterpretowany jako częściowy GO:

```text
qualification range       = 439703807..=439708174
sample slots              = 19
attempted                 = 19
fully matched             = 16
unavailable               = 1
request attempts          = 22
provider elapsed          = 136 939 ms
raw representatives read  = 5
missing representative    = 0
```

Provider zwrócił 18 bloków. Szesnaście próbek miało dokładnie zgodne identity,
class counts i failed status. Jeden slot wyczerpał trzy 30-sekundowe próby, a
inny wymagał retry i około 32 sekund. To nie dowodzi capacity dla pełnych około
4,3 tysiąca slotów i blokuje combined audit.

Dwie pozostałe odpowiedzi ujawniły niezależny blocker qualification range:
slot `439703807` miał `raw=0`, `audit=118`, a slot `439703837` miał `raw=3`,
`audit=90`. Następny reprezentatywny slot `439703838`, obejmujący router CPI i
v0 loaded address, zgadzał się dokładnie `85/85`. Jest to dowód, że obecny
lower bound oparty wyłącznie na `first rooted + 1` dopuszcza pre-stream i
częściowy pierwszy slot. Przed combined audit materializer musi wyznaczać
start range dopiero po zachowanej granicy pierwszego kompletnego bloku/streamu
i posiadać regresję dla mid-slot capture start. Nie wolno usuwać tych dwóch
findings ani uznawać ich za provider error.

Receipt jawnie zachowuje
`provider_identity_independence_verified = false`: różny provider ID/hostname
i techniczny `getBlock` nie dowodzą fizycznej niezależności operatora.
Combined `certify --qualification-audit-config`, exact output, export, strategia,
Gatekeeper i execution pozostają HOLD/NO-GO. GO-E0 nie jest ponawiany bez
osobnej decyzji i bez usunięcia obu blockerów.

#### Amendment G.4 — epoch-aware complete-slot range i twardy provider deadline

Review wykonanego GO-E0 potwierdził historyczny receipt i jego fail-closed
status, lecz ujawnił trzy lokalne wady kontraktu PR-B:

1. `first rooted + 1` nie dowodziło granicy pierwszego kompletnego slotu;
2. rooted range selector tracił wcześniejszego kandydata przy luce numerycznej
   pomiędzy dwoma wpisami mapy canonicality;
3. `max_provider_wall_ms` zatrzymywał tylko rozpoczęcie kolejnego slotu, ale
   nie ograniczał czasu bieżącej próby i jej retry.

Normatywny selector qualification jest od tej korekty wspólny dla pełnego
independent audit i `provider-suitability`. Dla każdego `stream_epoch`
zachowanego w nagłówkach segmentów:

- pierwszy `BlockMeta` według `capture_sequence` zamyka wyłącznie obserwowany
  ogon slotu wejściowego;
- pierwszy kwalifikowalny slot to `first_block_meta.slot + 1`;
- ostatni kwalifikowalny slot to slot ostatniego zachowanego `BlockMeta`;
- brak pierwszego albo ostatniego `BlockMeta`, overflow granicy lub pusty
  interwał kończy się typed blockerem
  `CaptureStreamBoundaryUnproven`;
- zakres jest przecinany z `RootedCanonical`, brakiem epoch-local coverage
  gapu, numeryczną ciągłością slotów i już zachowaną parent-lineage
  canonicality;
- zakresy różnych epok nie są łączone. Reconnect zamyka authority starej
  epoki i otwiera nową granicę kompletności;
- wybierany jest najdłuższy poprawny interwał. Tie-break jest deterministyczny:
  wcześniejszy `start_slot`, następnie niższy `stream_epoch`, następnie niższy
  `end_slot`;
- transakcje raw porównywane z audytem muszą pochodzić dokładnie z wybranej
  epoki.

Indexer dodatkowo wymaga, aby epoch każdego source/gap recordu odpowiadał
epoch nagłówka segmentu, a epoki kolejnych segmentów nie cofały się. Nie
zmienia to frozen raw V1: wykorzystywane są istniejące pola nagłówka,
source envelope, `BlockMeta` i coverage gap.

Dla GO-D syntetyczna regresja zamraża znaną granicę:

```text
first preserved BlockMeta slot = 439703837
first qualification slot       = 439703838
```

GO-E0 hard provider deadline jest teraz przekazywany do każdej próby
`getBlock`. `attempt_timeout = min(configured_timeout, remaining_budget)`, a
po wyczerpaniu budżetu nie wolno rozpocząć następnego retry. Lokalny test z
wiszącym mock RPC dowodzi, że krótki provider deadline ucina aktywny request i
nie dziedziczy pełnego timeoutu ani kolejnych ośmiu retry.

Historyczny GO-E0 receipt pozostaje niezmieniony i nadal ma status
`blocked_audit_unavailable`. Jego 136 939 ms nie przekroczyło historycznego
budżetu, więc korekta nie reinterpretuje wyniku ani nie autoryzuje retry.
Fizyczna niezależność/capacity providera pozostaje nieudowodniona. Nowy
provider probe, combined certify, exact Ready, export i strategia nadal
wymagają osobnych decyzji; w ramach Amendment G.4 nie wykonuje się provider
I/O ani nie tworzy exact outputu.

Powyższe dwa zdania zachowują stan w chwili zamrożenia G.4. Późniejszy
Spectrum GO-E0.2 i operatorowy atest są opisane wyłącznie w G.5; nie zmieniają
historycznego G.4 receiptu ani algorytmu epoch-aware selector/deadline.

#### Amendment G.5 — failed-status authority i hash-pinned provider independence

Review technicznie poprawnego GO-E0.1 ujawnił rozjazd pomiędzy deklarowanym
kontraktem pełnego audytu a jego decision logic: provider-suitability porównywał
failed-status multiset, lecz full audit wymagał jedynie zgodnych transaction
identities i invocation-class counts. Od tej korekty oba przepływy korzystają
z jednego komparatora, który zwraca jednocześnie:

```text
identity_multiset_matches
invocation_class_counts_match
failed_status_multiset_matches
raw_failed_transaction_count
audit_failed_transaction_count
```

Slot może mieć status `Matched` wyłącznie wtedy, gdy wszystkie trzy booleany
są `true`. Każdy failed-status mismatch staje się
`SourceCoverageUnproven`; nie może zostać przepisany na `Ready`. Addytywne
liczniki i booleany są zachowywane w każdym slot finding oraz w globalnym
qualification report. Frozen raw V1 nie został zmieniony.

Fizyczna niezależność nie jest wnioskowana z różnego `provider_id`, hostname'u
ani transportu. Combined `certify` wymaga czterech authority inputs naraz:

```text
--qualification-audit-config <protected-config>
--provider-suitability-receipt <ready-receipt>
--provider-independence-attestation <protected-attestation>
--expected-provider-independence-sha256 <sha256>
```

Brak dowolnego argumentu, zły hash, symlink, status inny niż
`verified_independent`, fałszywe assertion, rozjazd runu/range/configu/
endpointu/executable/raw controls albo istniejący planned exact path kończy
się fail-closed przed provider I/O. Te same bytes są rewalidowane bezpośrednio
przed utworzeniem exact outputu. Finalny qualification report może ustawić
`provider_identity_independence_verified = true` wyłącznie z tego
zweryfikowanego atestu i zachowuje jego pełny digest.

GO-E0.1 używał publicznego Solana RPC i nie został retroaktywnie przypisany do
Spectrum. Po osobnym operator GO wykonano nowy, bounded, read-only Spectrum
GO-E0.2 dla właściwego G.4 range:

```text
receipt:
datasets/pump-research/operator-logs/
  go-e0-2-spectrum-provider-suitability-g5-20260817T102801Z/
  provider_suitability_receipt_v1.json

SHA-256                 = 859ba278557e840a0f36440995561ea0c84ce438995c30b00b85a3c9e3154e5d
status                  = ready_for_full_audit
stream_epoch            = 1
qualification range     = 439703838..=439708174
sample/attempted/matched= 17 / 17 / 17
unavailable             = 0
request attempts        = 17
provider elapsed        = 31 394 ms
raw/exact writes        = 0 / 0
```

Spectrum credential jest nadal wyłącznie chronioną wartością endpoint-path
podawaną przez `GHOST_PUMP_RESEARCH_AUDIT_RPC_PATH`. Nie trafia do TOML,
receiptu, logu ani atestu. Audit config zachowuje tylko root-only HTTPS origin;
pełny endpoint istnieje wyłącznie w pamięci standalone no-legacy klienta.

Operator utworzył create-new atest:

```text
/protected/operator/provider_independence_attestation_v1.json
SHA-256 = 286a32fe87cd549ddc9f8e78ceccd99602ff54d271b67ab1979effb32ba6f9db
BLAKE3  = 3833e8b7ac00fadc84c2d2f19656344624ddc42acdcfa373aca3d9b8ae22ceeb
```

Atest wiąże dokładnie Spectrum GO-E0.2, aktualny audit config, executable,
GO-D raw controls, G.4 range i nieistniejący planned exact output. Oficjalne
referencje potwierdzają odrębne produkty i operatorów NoLimitNodes oraz
Spectrum/Simply Staking; dokładne etykiety ASN i regionów są jawnie oznaczone
jako deklaracje operatora, a nie jako automatycznie odkryte fakty sieciowe.

Amendment G.5 nie autoryzuje uruchomienia combined `certify`. Spectrum GO-E0.2
i atest są gotowymi authority inputs do osobnego review. Do jego zakończenia:

```text
GO-D raw                     PASS / IMMUTABLE
Spectrum GO-E0.2             READY_FOR_FULL_AUDIT
failed-status enforcement    PASS LOCALLY
provider attestation         HASH-PINNED / OPERATOR-ASSERTED
combined certify             HOLD / NOT RUN
exact Ready                  NOT CREATED
export / strategy / execution NO-GO
```

#### Amendment G.5.1 — single resolved endpoint i exact suitability plan binding

Review G.5 wykazał, że walidator atestu rozwiązywał protected endpoint A, lecz
pełny audit ponownie wykonywał `config.resolve_connection()`. Mutowalne
process-global environment mogło więc teoretycznie podmienić faktycznie użyty
endpoint na B po sprawdzeniu receiptu i atestu.

Od G.5.1 walidacja tworzy jeden nieserializowany obiekt:

```text
PumpResearchValidatedCombinedAuditAuthorityV1
  audit_config
  resolved_connection
  audit_rpc_endpoint_blake3
  provider_independence
```

Ten sam `resolved_connection` jest przekazywany bezpośrednio do klienta full
auditu. Pętla audytowa nie odczytuje ponownie credential environment. Report
zapisuje digest dokładnie tego endpointu, a rewalidacja przed exact writerem
wymaga jego zgodności z digestem zwalidowanego atestu. Sekret pozostaje
wyłącznie w pamięci authority object; typ połączenia nie implementuje `Debug`
i nie jest serializowany ani logowany.

Walidator Spectrum suitability receiptu odtwarza teraz z immutable GO-D raw
ten sam deterministyczny plan, który utworzył GO-E0:

- exact G.4 epoch/range;
- first/mid/last i bounded-burst slots;
- representative roles direct/CPI/router/v0/failed;
- dokładny, unikalny zbiór slotów i uporządkowane role;
- raw identity/failed/class counts dla każdego findingu;
- bounded config/constants i sumę request attempts.

Duplikat slotu, role drift, brak/nadmiar findingu, niespójny raw-side count,
config drift albo attempt-total mismatch blokuje combined audit przed provider
I/O.

Dwie publiczne regresje zamrażają granicę authority:

1. po walidacji z endpointem A zmiana env na B nie zmienia URL klienta ani
   endpoint digestu qualification reportu;
2. publiczne
   `certify_pump_research_raw_run_with_qualification_audit_v1()` z błędnym
   atestem kończy się przed I/O, lokalny loopback widzi zero połączeń, a exact
   output nie powstaje.

Nowy release certifier:

```text
target/release/pump-research-tape
SHA-256 = 97251d5427e89a762a22ca5c06c29c7e7e9ab43c235bd6f5e82fb31b4c3617cf
BLAKE3  = 42fb85a5a2f4a2301be2ecbdc3c7494114ed357920b16f3fe49ea60a851c3739
bytes   = 12 546 816
```

Atest G.5 nie został nadpisany. Nowy create-new artefakt:

```text
/protected/operator/provider_independence_attestation_g5_1_v1.json
SHA-256 = 25b55aba36a90ce1fca2ec5528cd2438234e2ce584161f6021b61e2372ecb204
BLAKE3  = 8f702ebd2556d987b507cca1db4d048256e5bd640404d76fdedcbbeadd0dd92d
bytes   = 4 266
mode    = 0600
```

wiąże niezmieniony Spectrum GO-E0.2 executable/receipt z nowym combined
certifier executable. Provider assertions pozostają operator-asserted;
G.5.1 zamyka integralność authority flow, lecz nie zamienia oświadczenia
operatora w automatyczny network-discovery proof.

G.5.1 nie wykonuje provider I/O i nie autoryzuje combined runu. Obowiązuje:

```text
Spectrum GO-E0.2               PRESERVED / READY_FOR_FULL_AUDIT
old G.5 attestation            PRESERVED / OLD CERTIFIER DIGEST
new G.5.1 attestation          HASH-PINNED / CURRENT CERTIFIER DIGEST
single resolved endpoint       PASS LOCALLY
deterministic receipt plan     PASS LOCALLY
combined certify               HOLD / NOT RUN
exact Ready                    NOT CREATED
export / strategy / execution  NO-GO
```

#### Amendment G.5.2 — raw audit-to-exact same-snapshot authority

Review G.5.1 zaakceptował single-resolved endpoint, lecz wykazał osobny
TOCTOU: początkowy frozen scan wiązał ścieżki i offsety segmentów, podczas gdy
full audit, account anchors i exact materializer ponownie otwierały raw paths.
Pre-exact revalidation nie obejmowała segmentów. Formalnie audit mógł więc
porównać raw A, a exact materializer przeczytać spójnie podmieniony raw B.

Od G.5.2 combined flow tworzy przed walidacją provider authority jeden
`PumpResearchRawSegmentSetAuthorityV1`:

```text
ordered completion segment receipts
-> symlink_metadata + O_NOFOLLOW/O_CLOEXEC regular-file open
-> private create-new copy with streaming SHA-256/BLAKE3
-> receipt digest equality
-> mode 0400
-> read-only open
-> unlink snapshot filename
-> retain only pinned descriptor
-> deterministic ordered aggregate BLAKE3
```

Każdy entry authority zawiera:

- `segment_index`;
- frozen filename;
- canonical source path;
- byte length;
- whole-file SHA-256;
- whole-file BLAKE3.

Audit fingerprinting i exact materialization czytają wyłącznie te same,
prywatne, odlinkowane descriptors. Zastąpienie oryginalnego path przez rename
nie zmienia bytes czytanych przez proces. Jednocześnie cały source path set i
pinned snapshot set są ponownie hashowane:

1. po utworzeniu snapshotu;
2. po zbudowaniu raw audit fingerprints, bezpośrednio przed pierwszym provider
   I/O;
3. po zakończeniu auditu, przed account anchors i exact writerem;
4. po pełnej materializacji `.partial`, bezpośrednio przed finalnym rename.

Każdy path drift, symlink, size drift, SHA/BLAKE mismatch, descriptor drift
albo aggregate drift kończy operację błędem. Finalny exact directory nigdy nie
powstaje. Po późnym błędzie może pozostać wyłącznie jawny `.partial` do
forensics.

Finalny check przed rename ponownie wiąże również stabilne authority inputs
G.5.1: provider-independence attestation, audit config, suitability receipt,
combined executable oraz raw binding/start/completion JSON. Qualification
report i exact manifest muszą wskazywać ten sam aggregate raw segment-set
digest; rozjazd blokuje publikację.

Qualification report zapisuje addytywne `raw_segment_set_blake3`. Nowe exact
manifesty zapisują addytywne `source_raw_segment_set_blake3`; historyczne JSON
bez pola nadal deserializują się z pustą wartością. Frozen raw binary V1,
header, footer, framing i golden fixtures nie zmieniają się.

Regresje wymagają:

- hash-equivalent control i manifest digest PASS;
- final-segment symlink odrzucony przed frozen scan;
- spójny segment B po snapshot/index, przed provider I/O: fail + zero requestów;
- drift po audicie: fail przed exact writerem, brak `.partial` i final outputu;
- drift po pełnej materializacji: final rename zablokowany, final output absent,
  dozwolony wyłącznie `.partial`;
- historyczny exact JSON bez nowego pola nadal się ładuje.

Nowy release certifier:

```text
target/release/pump-research-tape
SHA-256 = 780a415eadb484dddb51d23e6356e28c273d2f6ccbf2109e5dd3c0becf770203
BLAKE3  = d65d21e14b46075b0e12cd771e360651e97c4996ed5d4c1816bea2835401b40b
bytes   = 12 600 848
mode    = 0700
```

Atesty G.5 i G.5.1 pozostają historyczne i nie są nadpisywane. Nowy
create-new artefakt:

```text
/protected/operator/provider_independence_attestation_g5_2_v1.json
SHA-256 = b06a4a7d91b9b716c46b12c92752c3e4902383a1bbed7821d422b324633c8074
BLAKE3  = 7b3551708b92211861df1371a011bf909887cfdace1f961df261d4304aaa1d44
bytes   = 4 330
mode    = 0600
```

wiąże ten sam, niezmieniony Spectrum GO-E0.2 receipt z nowym combined
certifierem. G.5.2 nie ponawia provider probe i nie wykonuje provider I/O.

Po G.5.2 obowiązuje:

```text
GO-D raw                         PASS / UNCHANGED
Spectrum GO-E0.2                 READY_FOR_FULL_AUDIT / UNCHANGED
single resolved endpoint         PASS / G.5.1 ACCEPTED
raw audit-to-exact snapshot      PASS LOCALLY
raw segment aggregate evidence  PASS LOCALLY
new G.5.2 attestation            CREATED / HASH-PINNED
combined certify                 HOLD / NOT RUN
exact Ready                      NOT CREATED
export / strategy / execution    NO-GO
```

#### Amendment G.5.2.1 — bounded raw opens, pre-side-effect output boundary i staging ownership

Niezależny review G.5.2 potwierdził pinned audit/exact bytes, lecz wykazał trzy
przedoperacyjne luki. `symlink_metadata()` poprzedzało blokujące `open()`, więc
regular-file → FIFO race mógł zatrzymać certifier przed post-open `fstat`.
Hash/copy czytały do EOF, a publiczny combined flow tworzył snapshot przed
sprawdzeniem rozłączności `output` i `raw`. Staging guard pozostawał uzbrojony
po jawnym `remove_dir()`.

G.5.2.1 ustanawia następujący normatywny kontrakt:

```text
canonical raw/output disjoint check
-> frozen index using O_NOFOLLOW/O_CLOEXEC/O_NONBLOCK
-> post-open fstat regular-file authority
-> opened file length becomes exact bounded segment length
-> hash-pinned attested output + current executable check
-> private snapshot copy reads exactly that many bytes
-> post-read fstat length equality
-> receipt SHA-256/BLAKE3 equality
-> pinned unlinked snapshot
-> provider authority and audit
```

Publiczny combined entry point musi przed indeksem i przed jakąkolwiek operacją
zapisu:

1. kanonizować `run_dir` i create-new `output_dir`;
2. wymagać dwukierunkowej rozłączności ścieżek;
3. odrzucić output znajdujący się w immutable raw;
4. wykonać bounded read-only frozen index;
5. lokalnie zweryfikować expected SHA-256 atestu, run ID, approved decision,
   planned exact output i bieżący combined executable digest;
6. dopiero potem tworzyć private snapshot.

Punkty 1-3 poprzedzają indeks. Punkty 1-5 poprzedzają pierwszy zapis i
utworzenie stagingu. Read-only indeks pomiędzy tymi granicami nie jest side
effectem i nie rozwiązuje endpoint credentialu.

Każde raw/authority file open używane przez G.5.2.1 wykonuje
`O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK`, a typ jest ustalany przez `fstat`
otwartego descriptor. Wstępne `symlink_metadata()` pozostaje wyłącznie
diagnostyką. FIFO, device, socket i directory muszą zakończyć się lokalnym
błędem, nigdy oczekiwaniem na peer.

Frozen scan, hash i snapshot copy są ograniczone do jawnego `expected_bytes`.
Short read, post-read length drift, SHA mismatch albo BLAKE3 mismatch blokują
operację. Nie wolno używać nieograniczonego read-until-EOF wobec mutowalnego
source path.

Staging directory jest tworzony przez `DirBuilderExt::mode(0700)`. Guard jest
konstruowany natychmiast po udanym `create`; każdy późniejszy błąd uruchamia
cleanup. Guard przechowuje `Option<PathBuf>`. `close()` wyjmuje path przed
`remove_dir()`, ponownie uzbraja go tylko przy błędzie usunięcia i pozostaje
rozbrojony po sukcesie. Drop nie może usunąć katalogu odtworzonego później pod
tą samą nazwą.

Obowiązkowe regresje:

- regular A → FIFO pomiędzy precheck/open: bounded local error;
- growing regular file podczas exact-size hash: bounded local error;
- growing regular file podczas snapshot copy: bounded local error;
- public output wewnątrz raw: listing i wszystkie bytes bez zmian, zero
  snapshot paths, zero provider connections, brak exact/partial;
- błąd natychmiast po staging create: pełny cleanup;
- close/disarm z odtworzeniem tej samej nazwy: foreign marker przetrwa Drop;
- cały dotychczasowy G.5/G.5.1/G.5.2 corpus pozostaje zielony.

Release G.5.2 oraz
`provider_independence_attestation_g5_2_v1.json` pozostają historyczne. Nowy
certifier wymaga osobnego create-new G.5.2.1 attestation. Spectrum GO-E0.2 nie
jest ponawiany i pozostaje niezmienionym `READY_FOR_FULL_AUDIT` inputem.

Finalny lokalny release G.5.2.1 jest przypięty jako:

```text
target/release/pump-research-tape
SHA-256 = dc4263207adc2ea5ec897f1c564965e7c3b02551307e1b1ed42949c1ef1c8ebb
BLAKE3  = df2f59a2073c3e44f1817de55bb844cd07d813181d2faaa1d6f3011830ddd1ec
bytes   = 12624232
mode    = 0700
```

Create-new attestation dla tego certifiera:

```text
/protected/operator/provider_independence_attestation_g5_2_1_v1.json
SHA-256 = 4617ec14cc20f504d8156e152ea7038054f0039b6a13db3ca7b6e84f661dcb02
BLAKE3  = d6be6b9f1033af76bb5727da01b23481e8aeaa4265ba4787a7ca91c0237d7704
bytes   = 4403
mode    = 0600
```

Atest wiąże ten certifier z niezmienionym Spectrum GO-E0.2, GO-D controls,
G.4 range, protected audit configiem i planowanym create-new exact outputem.
Jest hash-pinned operator assertion; nie stanowi automatycznego dowodu
network-discovery fizycznej niezależności providerów.

Po G.5.2.1 obowiązuje:

```text
GO-D raw                         PASS / UNCHANGED
Spectrum GO-E0.2                 READY_FOR_FULL_AUDIT / UNCHANGED
G.5.2 pinned snapshot            PASS
regular-to-FIFO bounded open     PASS LOCALLY
exact-size raw hash/copy         PASS LOCALLY
output/raw pre-side-effect gate  PASS LOCALLY
staging RAII/disarm              PASS LOCALLY
combined certify                 HOLD / NOT RUN
exact Ready                      NOT CREATED
export / strategy / execution    NO-GO
```

#### Amendment G.5.2.2 — bounded control authority i anonymous descriptor-only snapshots

Review G.5.2.1 potwierdził bounded segment I/O, ale wykazał dwie pozostałe
granice. Raw control JSON i combined authority files nadal używały
`fs::read()` albo nieograniczonego `operator_digest_file()`, więc FIFO mogło
blokować przed dojściem do segmentów, a growing regular file nie miał jawnego
limitu. Nazwany staging był sprzątany przez pathname-only `remove_dir_all()`,
co na ścieżce błędu mogło usunąć obcy replacement directory.

G.5.2.2 ustanawia jeden bounded authority reader dla:

- `run_start_manifest.json` — maksymalnie 1 MiB;
- `run_completion_receipt.json` — maksymalnie 64 MiB;
- `operator_preflight_binding_v1.json` — maksymalnie 1 MiB;
- qualification audit config — maksymalnie 1 MiB;
- qualification preparation receipt — maksymalnie 4 MiB;
- provider-suitability receipt — maksymalnie 64 MiB;
- provider-independence attestation — maksymalnie 4 MiB;
- combined/GO-E0 executable — maksymalnie 256 MiB;
- exact manifest odczytywany przez exporter — maksymalnie 16 MiB.

Każdy taki odczyt wykonuje:

```text
symlink_metadata diagnostic
-> open O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK
-> post-open fstat regular-file authority
-> reject size > per-kind max_bytes
-> exact positional read of the frozen opened length
-> post-read fstat type/length equality
-> SHA-256/BLAKE3 over exactly those bytes
-> parse exactly the same bytes
```

Raw index zachowuje digests dokładnie tych start/completion/binding bytes,
które sparsował. Combined i GO-E0 porównują późniejsze path reads z tym
indexed control authority. Parse wersji B i późniejszy hash przywróconej wersji
A jest błędem, nawet gdy oba JSON-y są semantycznie poprawne.

G.5.2.2 usuwa nazwany staging w całości. Każdy prywatny segment snapshot jest
tworzony jako Linux:

```text
O_TMPFILE | O_EXCL | O_CLOEXEC | O_NONBLOCK | O_RDWR
-> exact-size copy + receipt hashes
-> sync_all
-> fchmod 0400 by descriptor
-> reopen the same dev/ino through /proc/self/fd as O_RDONLY
-> require st_nlink == 0 and identical dev/ino/len
-> drop the writable descriptor
-> retain only Arc<File> read-only descriptor
```

Snapshot nigdy nie ma pathname. Nie istnieje staging directory, guard,
rekurencyjny cleanup ani replacement-on-error surface. Brak `O_TMPFILE` albo
`/proc/self/fd` kończy combined lokalnym błędem przed provider I/O.

Obowiązkowe regresje G.5.2.2:

- common authority regular → FIFO race wraca przed delayed writerem;
- growing authority file oraz przekroczenie per-kind limitu failują bounded;
- public combined odrzuca FIFO kolejno dla start, completion, binding, audit
  config, suitability receipt i attestation; zero provider I/O, exact i partial;
- late post-audit revalidation odrzuca FIFO wszystkich sześciu authority inputs;
- raw control parse-to-digest drift start/completion/binding failuje;
- anonymous snapshot ma `st_nlink == 0`, read-only FD odrzuca write i błąd nie
  tworzy ani nie usuwa żadnego pathname/foreign marker;
- cały wcześniejszy G.5/G.5.1/G.5.2/G.5.2.1 corpus pozostaje zielony.

G.5.2.1 release i attestation pozostają historyczne. G.5.2.2 wymaga nowego
create-new attestation z digestem finalnego certifiera. Spectrum GO-E0.2 nie
jest ponawiany i pozostaje niezmienionym `READY_FOR_FULL_AUDIT` inputem.

Finalne lokalne authority pins G.5.2.2:

```text
target/release/pump-research-tape
SHA-256 = b0a096d6ae4773d0a08d279defbd94c4e0c394729a9f1522e918892b9d102f6f
BLAKE3  = e2d235f67e199e9cb43d7ff3bc19e7a957db3805a7480c24a23096d28362c9bd
bytes   = 12637504
mode    = 0700

/protected/operator/provider_independence_attestation_g5_2_2_v1.json
SHA-256 = 6b6e08ff3b23dfa6a4735cca179e2a80192be145b1be7475556557a0cf175f00
BLAKE3  = bbf781dd4eb0011f592287673b0e4391b706e453c955edd1a1e67b46def82a2c
bytes   = 4579
mode    = 0600
```

Po G.5.2.2 obowiązuje:

```text
GO-D raw                              PASS / UNCHANGED
Spectrum GO-E0.2                      READY_FOR_FULL_AUDIT / UNCHANGED
segment bounded authority             PASS
control/authority bounded reader      PASS LOCALLY
indexed parse/digest authority        PASS LOCALLY
anonymous O_TMPFILE snapshot          PASS LOCALLY
pathname cleanup surface              ABSENT
combined certify                      HOLD / NOT RUN
exact Ready                           NOT CREATED
export / strategy / execution         NO-GO
```

#### Amendment G.5.2.3 — kernel-bound running executable inode authority

Review G.5.2.2 potwierdził bounded control I/O oraz anonymous raw snapshots,
ale wykazał, że `env::current_exe()` nie jest authority wykonywanego obrazu.
Po atomowej zamianie pathname proces nadal wykonuje inode A, podczas gdy
ponowne otwarcie pathname może hashować B. Wielokrotne hashowanie tego samego
pathname nie zamyka tej różnicy.

G.5.2.3 ustanawia `PumpResearchRunningExecutableAuthorityV1`. Pierwszą
operacją publicznego combined certify, przed raw indexem, jest:

```text
open /proc/self/exe O_RDONLY | O_CLOEXEC | O_NONBLOCK
-> post-open fstat regular file
-> reject size > 256 MiB
-> exact positional SHA-256/BLAKE3 over the opened length
-> post-read fstat type/size/dev/ino equality
-> retain Arc<File> for the entire combined lifecycle
```

`/proc/self/exe` jest jedynym celowo śledzonym symlinkiem: jest kernelowym
odwołaniem do mapped executable image bieżącego procesu. Zwykłe
operator-controlled paths nadal podlegają `O_NOFOLLOW`.

Ten sam przypięty deskryptor jest authority dla:

```text
public combined entry
-> pre-snapshot attestation binding
-> post-snapshot full authority validation
-> provider audit
-> pre-exact-writer revalidation
-> final pre-rename revalidation
```

Atest musi wiązać digest tego running inode. `env::current_exe()` nie jest
używany produkcyjnie do qualification provenance. Końcowe rewalidacje haszują
ten sam deskryptor, a nie pathname.

Przyszłe GO-E0 również otwiera running executable authority przed configiem i
raw indexem, wpisuje jej digest do suitability receiptu i ponownie hashuje ten
sam FD po provider phase, przed publikacją receiptu.

Historyczny Spectrum GO-E0.2 pozostaje byte-for-byte niezmieniony. Jego
executable binding ma wcześniejszą pathname semantics, dlatego w G.5.2.3 jest
klasyfikowany wyłącznie jako bounded availability/retention/sample preflight.
Nie jest samodzielnym qualification proof. Full combined audit odtwarza
deterministyczny plan z raw, wykonuje pełny zakres 4337 slotów oraz ponownie
porównuje identities, invocation classes i failed-status multiset. GO-E0.2 nie
jest automatycznie ponawiany; akceptacja tej klasyfikacji pozostaje osobną
decyzją reviewera.

G.5.2.3 nie przepisuje historycznego GO-D bindingu ani Spectrum receiptu i nie
twierdzi, że retroaktywnie zmienia ich executable semantics. Aktualne GO-D raw
bytes oraz ich accepted eligibility pozostają osobnym, zachowanym dowodem.

Obowiązkowa regresja G.5.2.3 jest pełnym subprocess testem:

```text
start copied test executable A
-> child reaches controlled boundary
-> atomically replace its pathname with executable B
-> attestation binds B
-> public combined captures /proc/self/exe inode A
-> running-executable mismatch
-> provider requests = 0
-> exact partial/final = absent
```

Finalne lokalne authority pins G.5.2.3:

```text
target/release/pump-research-tape
SHA-256 = 8fc9c9e9e068d4b375e261f2c3d6e9aa4675007a96bc8cd4d962d102cc334932
BLAKE3  = c6fe14c43eec4804457fbdf741052ab9d750fa0d054b56a4742d8d8f9bf1ea4c
bytes   = 12641744
mode    = 0700

/protected/operator/provider_independence_attestation_g5_2_3_v1.json
SHA-256 = 501cb07f7c13d9be7a3d341ffa1afa735d1c5530a37d330f445895987c5b94e0
BLAKE3  = b66ef73e6435caf872e1e7b291cec016eb58e75ee1cccfda044abc43cf0e68da
bytes   = 4928
mode    = 0600
```

Po G.5.2.3 obowiązuje:

```text
GO-D raw                                  PASS / UNCHANGED
Spectrum GO-E0.2                          AVAILABILITY PREFLIGHT / UNCHANGED
bounded control and segment authority     PASS
anonymous raw snapshots                   PASS
running combined executable inode         PASS LOCALLY / RELEASE PINNED
future GO-E0 running inode                 PASS LOCALLY
pathname replacement A -> B               BLOCKED LOCALLY / SUBPROCESS PASS
combined certify                          HOLD / NOT RUN
exact Ready                               NOT CREATED
export / strategy / execution             NO-GO
```

#### Amendment H — final GO-D frozen-tape authority and GO-E retirement

Operator zakończył zewnętrzny combined audit jako historyczny eksperyment
`Blocked(SourceCoverageUnproven)` po zawodnym providerze RPC. Wynik nie jest
dowodem naruszenia GO-D: child zakończył się czysto, raw nie został zmieniony,
a zewnętrzne `AuditUnavailable`/HTTP 503 nie opisuje integralności frozen tape.

Od Amendment H jedynym source authority dla przypisanych eksperymentów jest:

```text
GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = true
```

GO-E, provider suitability, provider-independence attestation i combined audit
pozostają historycznymi artefaktami audytowymi. Nie są gate’em dla offline
materialization, exportu, prerejestracji ani analizy strategii. CLI odrzuca
nowe GO-E/provider-audit operacje przed provider I/O.

Promocja nie jest blanket bypass dla dowolnego raw runu. Offline
`VerifiedFrozenTape` wymaga hash-pinned:

```text
configs/rollout/pump-research-go-d-source-authority-v1.json
SHA-256 = b583dd1a6a24a87c2035837e3ef0dc9266a35041505fd07f8095987ab1088ab7
```

Receipt wiąże dokładnie:

- run ID `pump-research-1786909252793-3799414`;
- frozen storage V1;
- preflight binding SHA-256 `e59eb392...`;
- start manifest SHA-256 `8a38a98e...`;
- completion receipt SHA-256 `cad3feb7...`;
- aggregate pinned segment-set BLAKE3 `658214f0...`;
- operator decision `VERIFIED` oraz `RETIRED_NOT_A_GATE`.

Materializer nadal failuje przed publikacją, jeżeli:

- capture provenance nie jest eligible v5;
- lifecycle nie jest `Complete`, clean i zero-loss;
- występuje ProgramData boundary, gap, dropped update lub accounting mismatch;
- control file albo segment-set digest nie odpowiada authority;
- raw/output nie są rozłączne;
- source authority zmieni się po walidacji;
- pinned raw snapshot zmieni się przed finalnym rename.

Docelowe polecenie jest całkowicie offline:

```bash
target/release/pump-research-tape certify \
  --run-dir datasets/pump-research/pump-research-1786909252793-3799414/raw \
  --output datasets/pump-research/pump-research-1786909252793-3799414/exact-go-d-verified-v1 \
  --go-d-source-authority configs/rollout/pump-research-go-d-source-authority-v1.json \
  --expected-go-d-source-authority-sha256 \
  b583dd1a6a24a87c2035837e3ef0dc9266a35041505fd07f8095987ab1088ab7
```

Exact manifest i każdy export manifest muszą jawnie zawierać:

```text
qualification_status = VerifiedFrozenTape
GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = true
```

Amendment H nie autoryzuje nowego capture, RPC backfillu, mieszania późniejszych
danych, zmiany active Seer/Gatekeeper/runtime ani otwarcia outcome’ów przed
przejściem prerejestrowanej coverage/liczność/power gate.

Stan końcowy planu:

```text
CS0 / frozen raw V1                    PASS
PR-A capture                           PASS
sealed provenance v5                   PASS
GO-D immutable raw integrity           VERIFIED
PR-B offline materializer/exporter     COMPLETE
GO-E external audit                    RETIRED / NOT A GATE
plan                                   COMPLETE (GO-D VERIFIED)
strategy experiment                    requires separate preregistration
Gatekeeper / execution                 unchanged / not authorized
```

#### Amendment I — reusable OBS Lite / ACE evidence foundation curation

Po finalizacji GO-D wykonano osobną kurację zmian, które historycznie powstały
podczas walidacji OBS Lite/A0/ACE, lecz opisują ogólne fakty źródłowe potrzebne
w kolejnych walidacjach. Do PR należą wyłącznie:

- osobne legacy `Create` i current `CreateV2` wraz z poprawnym account layout;
- current i pre-cashback `CreateV2` decode bez legacy fallbacku;
- Mayhem/Cashback jako tri-state source facts;
- pełne source-backed CreateEvent/TradeEvent reserve fields;
- `PumpCreationRegimeV1`, exact initial virtual quote reserves i canonical
  birth order;
- transaction-local mutation inventory i minimalne neutralne constructor
  defaults.

`PumpCreationRegimeV1` nie zawiera strategio-specyficznego predicate'u
eligibility. Selekcja kohorty należy do osobnej prerejestracji, nie do parsera.

Jawnie wyłączone są OBS lease/anchor/tail/terminalizer, ACE/A0 separate capture
lanes i disposition receipts, active launcher/EventWriter/OracleRuntime wiring,
progi, outcomes oraz jakakolwiek polityka Gatekeeper/Trigger/execution.
Reużywalną authority przyszłych przypisanych walidacji jest GO-D tape, więc
stary strategio-specyficzny capture wiring nie jest dependency tego PR.

Szczegóły kuracji zapisuje:

```text
docs/ADR/ADR_8D_PUMP_RESEARCH_REUSABLE_OBS_ACE_EVIDENCE_FOUNDATION_20260817.md
```

Korekty zamykają wyłącznie kontrakty dowodowe: znaczenie source-losslessness,
niezmienność binary V1, canonicality forków, niezależny audyt kompletności
źródła, on-chain program version, minimalne mutable dependencies oraz
participant trade-token-account balance.

| Korekta | Zmieniane części planu | Wpływ na kod |
|---|---|---|
| SCHEMA-LOSSLESS SOURCE CAPTURE zamiast wire-lossless | CS0, manifest, raw codec, testy round-trip | PR-A, QUALIFICATION |
| Wiecznie niezmienny binary layout V1 | CS0, segment format, decoder, golden fixtures | PR-A, QUALIFICATION |
| RootedCanonical / Dead / Unresolved | raw SlotUpdate, materializer, coverage, exporter | PR-A, PR-B, QUALIFICATION |
| Niezależny source-completeness audit | procedura qualification i raport | QUALIFICATION |
| Dowód direct/CPI/router/v0 coverage filtra | profil subskrypcji, fixtures, qualification | PR-A, QUALIFICATION |
| ProgramData start/end i transition dependencies | manifest/receipt, raw Global evidence, certifier | PR-A, PR-B, QUALIFICATION |
| Participant trade-token-account balances | exact mutation schema i exporter evidence gate | PR-B |
| Osobny PumpResearchCaptureConfigV1 | konfiguracja standalone capture | PR-A |

Nie zmieniają się wcześniejsze decyzje dotyczące:

- transaction-local curve trajectories jako jednostki autorytatywnej;
- pełnego mutation inventory;
- offline materializera;
- PumpObservationLedgerV1;
- AccountObservationArbiter;
- RawPumpMutationLocatorV1;
- CanonicalPumpOrderKeyV1;
- EventTimeMetadata;
- bounded nonblocking capture;
- braku RPC backfillu do tape;
- braku bazy danych, Kafka, Parquet/Arrow i nowego runtime authority;
- rozdzielenia capture, certify i export-window;
- wyłączenia PumpSwap po migracji;
- wyłączenia strategii, execution replay i PnL.

## 2. Zamrożone kontrakty V1.1

### 2.1. Publiczne operacje

Pozostają trzy podstawowe operacje:

~~~text
pump-research-tape capture
pump-research-tape certify
pump-research-tape export-window
~~~

Interfejs docelowy:

~~~bash
pump-research-tape capture \
  --config configs/rollout/pump-research-tape-v1.toml
~~~

~~~bash
pump-research-tape certify \
  --run-dir datasets/pump-research/<run_id>/raw \
  --output datasets/pump-research/<run_id>/exact
~~~

Qualification nie staje się osobnym subsystemem ani źródłem raw. Uruchamia
się przez opcjonalny, read-only tryb istniejącego certify:

~~~bash
pump-research-tape certify \
  --run-dir datasets/pump-research/<run_id>/raw \
  --output datasets/pump-research/<run_id>/exact \
  --qualification-audit-config configs/rollout/pump-research-audit-v1.toml
~~~

Certify bez niezależnego audytu może utworzyć exact tape, ale jego manifest ma
status UNQUALIFIED. Nie może wyemitować PUMP_RESEARCH_TAPE_V1_READY.

Eksporter otrzymuje istniejący jawny wybór osi czasu oraz nowy, opcjonalny
wymóg:

~~~bash
pump-research-tape export-window \
  --tape datasets/pump-research/<run_id>/exact \
  --time-axis observed \
  --observation-ms 150000 \
  --forward-ms 180000 \
  --require-evidence participant_balance \
  --output datasets/experiments/rift-150/<run_id>
~~~

Dozwolone wartości --require-evidence pozostają jawne i typowane. Dodanie
participant_balance nie implementuje żadnej strategii.

### 2.2. Schema-lossless source capture

V1 gwarantuje wyłącznie:

~~~text
SCHEMA-LOSSLESS SOURCE CAPTURE
~~~

względem:

~~~text
zamrożonego wygenerowanego protobuf schema
+ zdekodowanego payloadu update_oneof
+ deterministycznego prost re-encoding
~~~

Dla poszczególnych rekordów zachowywany jest odpowiedni payload znany
zamrożonemu schema:

- SubscribeUpdateTransaction;
- SubscribeUpdateAccount;
- SubscribeUpdateSlot;
- SubscribeUpdateBlockMeta.

Nie gwarantujemy i nigdzie nie deklarujemy:

~~~text
WIRE-BYTE LOSSLESS
UNKNOWN-FIELD LOSSLESS
ORIGINAL GRPC FRAME IDENTITY
~~~

Payload_hash jest BLAKE3-256 dokładnie tych bytes, które powstały przez
deterministyczne prost re-encoding z już zdekodowanej wiadomości.

CS0 zamraża aktualne lokalne zależności źródłowe:

~~~text
yellowstone-grpc-proto = 1.14.2
yellowstone-grpc-client = 1.15.4
prost = 0.12.6
~~~

Do corpus zostaje dodany wygenerowany i zapisany jako fixture FileDescriptorSet
obejmujący geyser.proto, solana-storage.proto i ich importy. Jego canonical
bytes oraz SHA-256 zostają zamrożone. Runtime nie generuje deskryptora ponownie.

Run_start_manifest zawiera:

~~~text
source_proto_schema_version
source_proto_descriptor_hash
source_proto_crate
source_proto_crate_version
source_client_crate
source_client_version
source_capture_semantics = "decoded_protobuf_schema_lossless_v1"
~~~

Test round-trip dowodzi:

~~~text
decode_v1(deterministic_prost_encode(source_message))
==
source_message
~~~

dla wszystkich pól znanych zamrożonemu schema. Test nie porównuje bytes z
niedostępną ramką HTTP/2/gRPC.

### 2.3. Wiecznie niezmienny binary storage V1

Po CS0 następujące kontrakty są niezmienne:

~~~text
PumpResearchRawRecordV1
PumpRawSegmentHeaderV1
PumpRawSegmentClosedV1
wszystkie nested V1 storage structs i enumy
~~~

Po freeze zabronione są:

- dodanie pola;
- usunięcie pola;
- zmiana kolejności pól;
- zmiana wariantu lub kolejności wariantów enum;
- zmiana typu pola;
- zmiana reprezentacji pubkey/signature/hash;
- reinterpretacja semantyki;
- zmiana konfiguracji bincode.

Każda taka potrzeba tworzy:

~~~text
PumpResearchRawRecordV2
PumpResearchRawDecoderV2
~~~

V1 nie otrzymuje addytywnego wariantu ani pola.

Fizyczny format V1 zostaje zamrożony jako:

~~~text
u32 little-endian payload_length
bincode-1.3.3(payload)
32-byte BLAKE3(payload)
~~~

Bincode używa jawnie zamrożonych opcji:

~~~text
fixed integer encoding
little endian
reject trailing bytes podczas decode
~~~

Pozostaje limit pojedynczego rekordu 16 MiB. Przekroczenie limitu generuje
typed local coverage gap; nie zwiększa automatycznie limitu i nie uruchamia
alternatywnego storage.

Persistowane V1 structs używają storage-owned typów opartych o stałe
reprezentacje, między innymi [u8; 32] dla pubkey, [u8; 64] dla signature oraz
[u8; 32] dla hash. Typy domenowe Solany są konwertowane na granicy, aby
przyszła zmiana ich implementacji serde nie zmieniła binary V1.

Każdy raw manifest, segment header i segment footer zawiera:

~~~text
storage_format_version = 1
~~~

Qualification zawiera co najmniej dwa golden binary fixtures:

- pełny zestaw reprezentatywnych wariantów PumpResearchRawRecordV1;
- kompletny mały segment V1 z headerem, rekordami i zamknięciem.

Dla każdego golden fixture zamrażamy:

~~~text
SHA-256 całego pliku
BLAKE3 całego pliku
~~~

Testy wymagają:

~~~text
current V1 decoder odczytuje frozen old fixture
decode → canonical encode daje identyczne bytes
hash SHA-256 pozostaje identyczny
hash BLAKE3 pozostaje identyczny
~~~

Nie dodajemy kompresji, Parquet, Arrow ani innego serialization frameworka.

### 2.4. Zamrożony raw record enum

V1 wykorzystuje jeden enum:

~~~rust
enum PumpResearchRawRecordV1 {
    PrimaryTransaction(PumpPrimaryTransactionEvidenceV1),
    PrimaryAccountUpdate(PumpPrimaryAccountUpdateEvidenceV1),
    PrimarySlotUpdate(PumpPrimarySlotEvidenceV1),
    PrimaryBlockMeta(PumpPrimaryBlockMetaEvidenceV1),
    CoverageGap(PumpRawCoverageGapV1),
    SegmentClosed(PumpRawSegmentClosedV1),
}
~~~

PumpRawCoverageGapV1 jest storage-owned, zamrożoną reprezentacją semantyki
istniejącego LocalCoverageGapV1. Jest to adapter persistence, nie drugi system
gapów.

Entry pozostaje wyłączone zgodnie z V1.1. Nie dodajemy EntryAnchor do frozen
enum. Slot continuity i chain time pochodzą z SlotUpdate oraz BlockMeta.

### 2.5. Wszystkie nowe raw fields i records wymagane przez korekty

Poniższa lista jest kompletna względem korekt.

| Lokalizacja | Nowe pole lub rekord | Semantyka |
|---|---|---|
| run_start_manifest | storage_format_version = 1 | Wybór frozen binary decoder |
| run_start_manifest | source_proto_schema_version | Identyfikacja schema użytego do decoded capture |
| run_start_manifest | source_proto_descriptor_hash | sha256:<hex> frozen FileDescriptorSet |
| run_start_manifest | source_proto_crate, source_proto_crate_version | Dokładna wersja generated message types |
| run_start_manifest | source_client_crate, source_client_version | Dokładna wersja Yellowstone clienta |
| run_start_manifest | source_capture_semantics | Stała decoded_protobuf_schema_lossless_v1 |
| run_start_manifest | pump_program_id | Program objęty capture i certifierem |
| run_start_manifest | pump_program_account_owner | Właściciel konta programu |
| run_start_manifest | pump_programdata_pubkey | ProgramData powiązane z upgradeable Program |
| run_start_manifest | program_data_hash_algorithm | blake3-256 |
| run_start_manifest | program_data_hash_at_start | Hash całych raw bytes ProgramData |
| run_start_manifest | program_deployment_slot_at_start | Slot z UpgradeableLoaderState::ProgramData, jeśli dostępny |
| run_start_manifest | program_observed_context_slot_at_start | Finalized RPC context użyty do receipt |
| run_completion_receipt | storage_format_version = 1 | Zgodność z segmentami |
| run_completion_receipt | program_data_hash_at_completion | Końcowy ProgramData hash |
| run_completion_receipt | program_deployment_slot_at_completion | Końcowe deployment evidence |
| run_completion_receipt | program_observed_context_slot_at_completion | Kontekst końcowego odczytu |
| segment header/footer | storage_format_version = 1 | Samodzielna identyfikacja formatu segmentu |
| PumpPrimaryAccountUpdateEvidenceV1 | account_role | BondingCurve albo TransitionDependencyGlobal |
| PumpPrimaryAccountUpdateEvidenceV1 | is_startup | Zachowanie źródłowej semantyki Yellowstone startup snapshot |
| PumpPrimarySlotEvidenceV1 | slot, parent: Option<u64>, source_status: i32 | Surowe Processed/Confirmed/Finalized evidence; nie derived canonicality |
| raw source envelope | zamrożony hash re-enkodowanego payloadu | Payload_hash otrzymuje skorygowaną schema-lossless semantykę |

Korekta nie dodaje osobnego wariantu raw dla konta Global. Global wykorzystuje
PrimaryAccountUpdate z account_role = TransitionDependencyGlobal.

Korekta nie dodaje raw recordów dla:

- PumpSlotCanonicalityV1 — jest wynikiem materializacji;
- participant balance — pochodzi z już zachowanych pre/post token balances;
- independent audit — zapisuje wyłącznie qualification findings;
- ProgramData — start/end evidence znajduje się w manifest/receipt, a nie w
  event tape.

Independent audit nie ma API pozwalającego zapisać
PumpResearchRawRecordV1.

### 2.6. Canonical slot i fork certification

Dodajemy:

~~~rust
enum PumpSlotCanonicalityV1 {
    RootedCanonical,
    Dead,
    Unresolved,
}
~~~

Lokalny frozen Yellowstone schema przekazuje dla slotu:

~~~text
slot
optional parent
Processed / Confirmed / Finalized status
~~~

Nie posiada osobnego wire pola Dead. Dlatego materializer stosuje wyłącznie
poniższy, konserwatywny algorytm:

- RootedCanonical — dla slotu istnieje zachowany Finalized SlotUpdate; local
  coverage nie przecina wymaganej evidential continuity.
- Dead — slot był obserwowany jako processed/confirmed, nie otrzymał
  finalization, późniejszy zachowany finalized root oraz kompletny parent graph
  jednoznacznie dowodzą, że slot nie leży na rooted lineage.
- Unresolved — każdy pozostały przypadek, w szczególności brak finalization,
  brak parent edge, przecięcie przez local gap albo niezamknięty tail runu.

Brak dowodu Dead nie może zostać zastąpiony domysłem opartym na numerze slotu.
Taki slot pozostaje Unresolved.

Independent RPC audit może sprawdzić wynik, ale nie może nadpisać canonicality.
Autorytetem canonicality pozostają wyłącznie raw
SlotUpdate/parent/finalized-root evidence.

Reguły materializacji:

- raw rekordy ze wszystkich trzech klas pozostają zachowane;
- tylko RootedCanonical może wejść do canonical exact trajectory;
- Dead emituje coverage/trajectory status NON_CANONICAL_FORK;
- Unresolved emituje UNRESOLVED_CANONICALITY;
- żaden window przecinający unresolved canonicality nie otrzymuje COMPLETE;
- dead/unresolved rows nie są cicho usuwane z raportu.

Coverage liczymy jako:

\[
\frac{
\text{successful mutations należące do Exact trajectories na RootedCanonical chain}
}{
\text{wszystkie successful committed Pump mutation inventory entries na RootedCanonical chain}
}
\]

Multi-mutation transaction wnosi do licznika i mianownika każdą mutację, a nie
jeden rekord transakcji.

Raport osobno pokazuje:

~~~text
processed observations
rooted-canonical observations
dead-fork observations
unresolved-tail observations
failed transactions
~~~

UNRESOLVED_CANONICALITY blokuje konkretną trajectory/window oraz qualification
range, w którym występuje. Run może zakwalifikować maksymalny automatycznie
wyznaczony, gap-free rooted prefix, ale jego unresolved tail pozostaje jawnie
raportowany i nie wchodzi do complete windows.

Nie powstaje live fork manager.

### 2.7. On-chain program version receipt

Standalone capture wykonuje dwa read-only odczyty Pump Program/ProgramData:

1. finalized start receipt bezpośrednio przed otwarciem source streamu;
2. finalized completion receipt po zakończeniu admitowania, drainie writerów i
   ustaleniu końcowej granicy runu.

Receipt:

- odczytuje Pump Program account;
- potwierdza upgradeable loader ownership;
- dekoduje powiązane ProgramData;
- zachowuje Program i ProgramData identity;
- hashuje całe raw ProgramData account bytes przez BLAKE3-256;
- zachowuje deployment slot, jeżeli występuje w UpgradeableLoaderState;
- zachowuje RPC context slot i commitment.

Warunek single-version runu:

~~~text
program ID start == completion
ProgramData pubkey start == completion
ProgramData owner start == completion
ProgramData hash start == completion
deployment slot start == completion, jeżeli oba są dostępne
~~~

Każda niezgodność daje:

~~~text
PROGRAM_VERSION_BOUNDARY
~~~

Raw segmenty pozostają zachowane, lecz cały run nie może uzyskać single-version
qualification. Nie dzielimy automatycznie runu i nie implementujemy
multi-version transition engine. Następny capture rozpoczyna nowy run po
upgrade.

Brak możliwego do utworzenia start receipt zatrzymuje capture przed przyjęciem
pierwszego rekordu. Brak completion receipt oznacza niekompletne zamknięcie i
blokuje qualification.

### 2.8. Zamknięcie mutable transition dependencies

CS0 zamraża następujący wynik audytu lokalnego kodu:

| Wariant | Bit-exact reserve transition | Mutable dependency V1 |
|---|---|---|
| LegacyBuy, BuyV2 | pre-curve state + exact token amount/direct event + program rounding + final anchor | Brak fee-config dependency dla reserve state |
| BuyExactQuoteInV2 | pre-curve state + exact spendable curve quote input + program rounding + final anchor | Brak fee-config dependency dla reserve state |
| LegacySell, SellV2 | pre-curve state + exact token input/direct event + program rounding + final anchor | Brak fee-config dependency dla reserve state |
| Create, CreateV2 z kompletnym direct CreateEvent | exact event initial tuple + final anchor | Brak dodatkowego fallbacku |
| Create, CreateV2 bez kompletnego initial tuple | program-versioned Create semantics + effective Pump Global state + final anchor | Pump Global account |

Fee schedule, fee recipient, creator-fee config i buyback fee config wpływają na
wallet settlement, lecz nie na ruch bonding-curve reserves. Nie są więc
dodawane do raw capture V1. Pola protocol_fee_lamports i
creator_fee_lamports pozostają nieznane, jeśli bezpośrednie source evidence ich
nie dostarcza. Syntetyczny lub aktualny fee schedule nie może zostać
zastosowany historycznie.

PR-A dodaje wyłącznie exact subscription dla canonical Pump Global:

~~~text
4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf
~~~

obok globalnego owner+BondingCurve-discriminator filter.

Global evidence zachowuje:

~~~text
raw bytes
raw bytes BLAKE3
slot
write_version
txn_signature, jeśli provider dostarczył
provider provenance
stream epoch
capture sequence
is_startup
~~~

PR-B zawiera research-only, program-versioned minimal Global decoder potrzebny
do initial-state semantics. Nie używa aktualnego stanu RPC do historycznej
transakcji.

Effective Global state dla konkretnego Create wybiera istniejący
AccountObservationArbiter i canonical account order. Wymagane są:

- rooted canonicality;
- identyczna account identity;
- brak provider conflict;
- brak coverage gap;
- jednoznaczny predecessor;
- brak niejednoznacznej same-slot kolejności.

Jeżeli nie istnieje dokładny startup/predecessor Global snapshot albo ta sama
slotowa kolejność jest niejednoznaczna:

~~~text
TRANSITION_DEPENDENCY_UNCAPTURED
~~~

Nie stosujemy hardkodowanych „typowych” initial reserves.

Research state transition math pozostaje oddzielone od wallet settlement. Jego
virtual-reserve movement przechodzi parity test względem istniejącego
ProgramStateTransition; real-reserve movement przechodzi bit-exact final-anchor
corpus. Nie modyfikujemy live quote authority.

### 2.9. Participant trade-token-account balance

PumpCertifiedMutationV1 otrzymuje:

~~~rust
participant_token_account: EvidenceValueV1<Pubkey>,

participant_token_balance_before_units: EvidenceValueV1<u64>,

participant_token_balance_after_units: EvidenceValueV1<u64>,

participant_balance_scope: ParticipantBalanceScopeV1,

participant_balance_provenance: ParticipantBalanceProvenanceV1,
~~~

Minimalny scope:

~~~rust
enum ParticipantBalanceScopeV1 {
    CanonicalTradeTokenAccount,
    Unknown,
}
~~~

Minimalna provenance:

~~~rust
enum ParticipantBalanceProvenanceV1 {
    TransactionMetaAndCanonicalAtaProof {
        message_account_index: u32,
        instruction_account_position: u16,
        token_program: Pubkey,
    },
    Unknown,
}
~~~

Semantyka pola to wyłącznie:

~~~text
exact pre/post balance of the concrete token account
through which this Pump mutation executed
~~~

Nie jest to:

~~~text
wallet total
owner global inventory
sum of all owner token accounts
~~~

Certifier oznacza evidence jako canonical tylko wtedy, gdy jednocześnie:

- wariant instrukcji jednoznacznie identyfikuje participant token account;
- account index poprawnie rozwiązuje się przez static i loaded addresses;
- mint zgadza się z mutation mint;
- participant zgadza się z instrukcją;
- token program jest jednoznaczny;
- ATA wyliczone dla participant + mint + token_program jest identyczne z
  kontem instrukcji;
- pre i post token balances zawierają ten sam account index;
- raw amounts parsują się dokładnie do u64;
- nie ma innej mutation ani zewnętrznego token-account touch, który czyniłby
  transaction-boundary balance niejednoznacznym jako mutation-boundary
  balance.

Dla multi-mutation transaction z tym samym participant token account pre/post
transaction metadata nie dowodzi stanów pośrednich. W takim przypadku
per-mutation participant balances pozostają Unknown, chyba że pełny
transaction-local tree jednoznacznie izoluje pojedynczą zmianę konta. Nie
implementujemy transfer replay ani SPL indexera.

Brak pre lub post entry nie jest automatycznie interpretowany jako zero. ATA
creation/close bez pełnego dowodu również daje Unknown.

--require-evidence participant_balance przepuszcza tylko mutation rows, dla
których:

~~~text
account = Known
before = Known
after = Known
scope = CanonicalTradeTokenAccount
provenance = TransactionMetaAndCanonicalAtaProof
~~~

Brak participant balance:

- nie zmienia Exact reserve trajectory;
- nie zmienia state/order certification;
- powoduje wyłącznie exporter status
  MISSING_REQUIRED_EVIDENCE(participant_balance) dla strategii, która tego
  wymaga.

## 3. Realizacja

### Change Set 0 — freeze kontraktów

CS0 nie zmienia live runtime. Ma wykonać:

1. Zamrożenie bazowego checkpointu na lokalnym stanie projektu z zachowaniem
   wcześniej zaakceptowanych lokalnych zmian CreateV2/Mayhem, bez wciągania
   niepowiązanych zmian z dirty worktree.
2. Zamrożenie PumpResearchRawRecordV1, wszystkich nested storage structs,
   kolejności pól i wariantów.
3. Zamrożenie bincode 1.3.3 options, framingu, limitu rekordu i
   storage_format_version = 1.
4. Zamrożenie SCHEMA-LOSSLESS SOURCE CAPTURE oraz zakazu wire-lossless claims.
5. Zamrożenie protobuf descriptor fixture i wersji Yellowstone/prost.
6. Zamrożenie PumpSlotCanonicalityV1 i reguł Rooted/Dead/Unresolved.
7. Zamrożenie independent source-audit contract.
8. Zamrożenie Program/ProgramData receipt schema.
9. Zamrożenie dependency closure: BUY/SELL bez fee dependency dla reserves;
   Create fallback wymaga wyłącznie Pump Global.
10. Zamrożenie participant balance schema oraz conservative evidence rules.
11. Zamrożenie reason/status taxonomy:
    - NON_CANONICAL_FORK;
    - UNRESOLVED_CANONICALITY;
    - SOURCE_COVERAGE_UNPROVEN;
    - SOURCE_FILTER_CPI_COVERAGE_UNPROVEN;
    - PROGRAM_VERSION_BOUNDARY;
    - TRANSITION_DEPENDENCY_UNCAPTURED;
    - MISSING_REQUIRED_EVIDENCE.
12. Dodanie golden binary fixtures, descriptor fixture oraz deterministic
    corpus.
13. Zachowanie parser parity hash dla capture disabled/research disabled.
14. Przygotowanie wymaganego przez repo ADR-8D według istniejącego szablonu
    podczas implementacji; ADR nie jest runtime ani dataset artifact.

CS0 corpus zostaje rozszerzony o:

- processed slot później rooted;
- processed fork później udowodniony jako dead;
- unresolved tail;
- direct top-level Pump;
- inner Pump CPI;
- router→Pump CPI;
- v0 transaction z Pump programem w loaded address;
- ProgramData hash match/mismatch;
- Global startup snapshot i brak Global predecessor;
- canonical ATA participant balance;
- non-ATA/touch-only participant account;
- multi-mutation transaction z niejednoznacznym participant balance;
- frozen binary segment V1.

Jeżeli zaakceptowanych lokalnych zmian nie da się odizolować bez naruszenia
innych zmian użytkownika, CS0 zatrzymuje implementację przed edycją repo. Nie
wykonuje resetu ani automatycznego czyszczenia dirty worktree.

### PR-A — standalone immutable raw capture

PR-A implementuje wyłącznie capture i frozen codec.

#### Konfiguracja

Powstaje osobny:

~~~rust
PumpResearchCaptureConfigV1
~~~

Ładuje go wyłącznie pump-research-tape capture.

Konfiguracja obejmuje:

~~~text
primary provider identity i endpoint/auth reference
program ID
RPC endpoint wyłącznie dla ProgramData receipts
output_dir
required_for_run
queue_capacity
flush_interval_ms
segment_max_bytes
segment_max_duration_ms
record_max_bytes = 16 MiB
~~~

Nie dodajemy research_tape do aktywnego SeerConfig. Lokalne GrpcConfig jest już
niezależnym kontraktem, więc standalone config może zostać na nie
przekonwertowany bez rozszerzania production config.

#### Profil Yellowstone

Dodajemy research-only:

~~~text
GrpcSubscriptionProfile::PumpResearchGlobalV1
~~~

Zamrożone parametry:

~~~text
one primary provider
one stream
commitment = processed
vote = false
failed = None
manual RPC backfill = false
registry/candidate scoping = false
Pump.fun program only
PumpSwap excluded
BlockMeta enabled
SlotUpdate enabled z filter_by_commitment = false
Entry disabled
~~~

Transaction filter wykorzystuje najmniejszą planowaną provider-side granicę:

~~~text
account_include = [Pump.fun program ID]
~~~

Account filters:

~~~text
Pump-owned BondingCurve discriminator
+
exact canonical Pump Global pubkey
~~~

Profil i canonical serialized subscription-request fingerprint trafiają do
manifestu.

#### Source tap i writer

GrpcConnection otrzymuje research-only capture output mode, który przejmuje
decoded source payload przed GeyserEvent projection i parser/candidate
filtering.

Active connect_geyser() i jego domyślny output pozostają bez zmian.

W capture mode:

- source receive task nie wykonuje disk I/O;
- nie wykonuje bincode ani prost encoding;
- przekazuje owned decoded payload do bounded queue;
- writer thread wykonuje deterministic prost encoding, bincode framing, hash i
  flush;
- nie powstaje per-event task;
- queue jest bounded;
- osobna zarezerwowana control lane przenosi wyłącznie ordered
  `DroppedSource` markers potrzebne do typed gap evidence;
- source lifecycle jest przechowywany atomowo przez ingress, a segment
  lifecycle jest własnością writera, footerów i receiptów;
- pełna data queue tworzy jeden typed gap episode;
- accepted records są drainowane przed shutdown;
- .partial pozostaje incomplete po crashu;
- poprawnie zamknięty segment jest atomowo publikowany dopiero po footerze;
- reconnect zwiększa stream_epoch;
- capture_sequence jest monotoniczna wewnątrz runu.

PR-A zapisuje:

~~~text
run_start_manifest.json
segment_*.bin
run_completion_receipt.json
~~~

Program start receipt musi powstać przed otwarciem streamu. Completion receipt
powstaje po writer drain. Niezgodny ProgramData hash nie usuwa raw evidence,
ale oznacza run jako PROGRAM_VERSION_BOUNDARY.

PR-A nie implementuje:

- materializera;
- exact state;
- participant balance;
- independent block audit;
- strategii;
- execution replay;
- Gatekeepera;
- selector;
- OFA/RIFT;
- live authority.

Po PR-A należy najpierw przejść capture-enabled local A/B gate z Amendment A.
Dopiero potem i po osobnej decyzji operatora można rozpocząć observe-only
prospective capture; inspekcja immutable output poprzedza PR-B.

### PR-B — exact materializer i exporter

PR-B:

1. Dodaje complete transaction-local mutation inventory do istniejącego
   pojedynczego scanowania.
2. Zachowuje runtime-compatible initialize_pool i trades; research inventory
   jest osobnym addytywnym wynikiem.
3. Obsługuje Create, CreateV2, wszystkie zamrożone Buy/Sell variants,
   complete/migrate/withdraw i unknown curve mutation.
4. Zachowuje więcej niż jeden Create, wiele curves, outer, inner oraz router
   CPI.
5. Rozdziela protocol_creator od create_user.
6. Zachowuje kompletne initial reserve fields.
7. Zachowuje Mayhem i Cashback jako tri-state z provenance; nie konwertuje
   unknown na false.
8. Replayuje PumpObservationLedgerV1.
9. Replayuje AccountObservationArbiter dla BondingCurve i minimalnego Global
   dependency.
10. Buduje slot graph i materializuje PumpSlotCanonicalityV1.
11. Buduje transaction-local curve trajectories.
12. Egzekwuje ProgramData start/completion compatibility.
13. Wykorzystuje effective Global state wyłącznie dla Create fallback.
14. Implementuje pure state-only certifier; nie modyfikuje live quote
    authority.
15. Materializuje optional participant trade-token-account balances.
16. Zapisuje births, trajectories i coverage/status JSONL.
17. Implementuje export-window z --require-evidence participant_balance.
18. Dodaje read-only qualification audit mode do certify.

Canonical trajectory nadal wymaga:

- kompletnego inventory;
- jednoznacznego order;
- wspieranego wariantu;
- RootedCanonical slotu;
- exact pre/genesis anchor;
- exact txn-signature final anchor;
- gap-free evidence;
- exact transition;
- zgodności direct CPI state tuple;
- bit-exact final account proof;
- braku account/provider/identity conflict;
- braku program version boundary;
- dostępności wszystkich wymaganych mutable dependencies.

Unknown participant/Mayhem/Cashback nie odbiera Exact state trajectory. Każdy
wymóg eksportera jest sprawdzany oddzielnie.

PR-B nie zmienia live canonical permit, AccountStateCore authority, Gatekeepera,
MFS, execution ani quote authority.

## 4. Qualification, testy i stop conditions

### 4.1. Independent source-completeness audit

Qualification używa osobnego read-only
PumpResearchQualificationAuditConfigV1:

~~~text
audit_provider_id
audit_rpc_endpoint/auth reference
bounded concurrency
bounded retry policy
request timeout
~~~

Audit_provider_id musi różnić się od primary_provider_id. Endpoint lub jego
zredagowany hash trafia do qualification receipt; credentials nie są
zapisywane.

Procedura:

1. Odczytać raw tape bez uchwytu writera.
2. Wyznaczyć automatycznie maksymalny contiguous, local-gap-free qualification
   range, którego sloty mają raw rooted evidence.
3. Początek zakresu następuje po pierwszym potencjalnie częściowym slocie runu.
4. Unresolved tail pozostaje poza zakresem i jest raportowany.
5. Poczekać, aż independent source potwierdza finalized availability ostatniego
   analizowanego slotu.
6. Dla każdego numeru slotu od pierwszego do ostatniego qualification slotu
   pobrać canonical finalized block albo jawny dowód skipped slot.
7. Unavailable, pruned, niepełny v0 decode albo brak inner instructions blokuje
   dowód.
8. Dla każdej transakcji rozwiązać account keys jako:

   ~~~text
   static keys
   + loaded writable addresses
   + loaded readonly addresses
   ~~~

9. Structural-scan outer i inner instructions.
10. Znaleźć każdą Pump.fun invocation, niezależnie od parser discriminator.
11. Zbudować multiset identity:

    ~~~text
    slot + tx_index + signature
    ~~~

12. Porównać exact multiset canonical block source z rooted raw-tape
    transactions.
13. Porównać również failed Pump transactions; nie należą do exact
    denominatora, ale należą do source completeness.
14. Zapisać wyłącznie qualification findings.
15. Nie otwierać raw segmentów w trybie write i nie wywoływać raw writer API.

Brak jednej Pump transaction lub różnica identity daje:

~~~text
SOURCE_COVERAGE_UNPROVEN
~~~

Dane z audytu:

- nie są dopisywane do raw tape;
- nie uzupełniają mutation inventory;
- nie tworzą anchorów;
- nie naprawiają coverage;
- nie zmieniają canonicality;
- nie zmieniają exact trajectories.

### 4.2. Source-filter CPI proof

Qualification klasyfikuje każdą independent Pump invocation przez niezależne,
nakładające się flagi:

~~~text
DIRECT_TOP_LEVEL
INNER_CPI
ROUTER_TO_PUMP_CPI
V0_LOADED_ADDRESS
~~~

Dowód składa się z trzech warstw:

1. Source-profile unit test dokładnego SubscribeRequest.
2. Frozen source fixtures dowodzą poprawnego rozpoznania każdej klasy po
   dostarczeniu wiadomości.
3. Independent finalized-block audit dowodzi, że primary provider faktycznie
   dostarczył każdą invocation z qualification range.

Qualification run musi zaobserwować co najmniej jeden przypadek każdej
wspieranej klasy. Jeżeli minimalne 30 minut lub 10 000 mutations nie dostarczy
klasy, run zostaje przedłużony; brak klasy nie jest zamieniany na domniemany
sukces.

Dla każdej zaobserwowanej klasy:

~~~text
independent count == raw-tape count
missing identities == 0
unexpected duplicate identities == 0
~~~

Brak dowodu dla inner/router/v0 daje:

~~~text
SOURCE_FILTER_CPI_COVERAGE_UNPROVEN
~~~

Nie przełączamy automatycznie capture na full-chain.

### 4.3. Artefakty qualification

Do dotychczasowych artefaktów dochodzą wyłącznie wymagane correction evidence:

~~~text
exact/qualification/source_completeness_v1.jsonl
exact/qualification/qualification_report_v1.json
~~~

Qualification report zawiera:

- qualified slot range;
- first/last finalized audit slot;
- audit provider identity;
- per-class invocation counts;
- exact raw-versus-audit set differences;
- rooted/dead/unresolved/failed counts;
- program start/completion receipt comparison;
- Global dependency coverage;
- exact trajectory numerator i denominator;
- binary golden fixture hashes;
- hot-path/writer results;
- final status.

Nie powstaje osobna baza ani trwały block index.

### 4.4. Minimalne testy

#### Schema i binary storage

- known-field protobuf round-trip zachowuje source message;
- test jawnie nie twierdzi wire-frame identity;
- manifest zawiera frozen descriptor hash i client versions;
- V1 golden record bytes pozostają identyczne;
- V1 golden segment bytes pozostają identyczne;
- old V1 fixture dekoduje się current decoderem;
- canonical encode old fixture daje dokładnie frozen bytes;
- SHA-256 i BLAKE3 są stabilne;
- trailing bytes, zły hash i rekord powyżej 16 MiB fail closed.

#### Canonicality

- processed slot później Finalized → RootedCanonical;
- processed fork wykluczony przez complete finalized parent graph → Dead;
- dead raw records pozostają w tape;
- dead trajectory → NON_CANONICAL_FORK, bez Exact;
- missing parent/finalization → Unresolved;
- unresolved window → bez COMPLETE;
- independent RPC nie może awansować raw Unresolved do canonical.

#### Source completeness i filter

- brak Pump transaction w jednym finalized slocie →
  SOURCE_COVERAGE_UNPROVEN;
- audit skanuje także slot bez lokalnie znalezionej Pump transaction;
- audit data nie może zostać zapisane do raw tape;
- top-level Pump invocation captured;
- inner CPI Pump invocation captured;
- router→Pump CPI captured;
- v0/loaded-address Pump invocation captured;
- brak jednej z klas → SOURCE_FILTER_CPI_COVERAGE_UNPROVEN;
- failed Pump transaction również uczestniczy w source comparison.

#### Program version i dependency closure

- identyczne start/completion ProgramData receipts → zgodność;
- różny hash → PROGRAM_VERSION_BOUNDARY;
- różny ProgramData identity lub deployment slot →
  PROGRAM_VERSION_BOUNDARY;
- brak start receipt → capture nie startuje;
- brak completion receipt → run incomplete;
- exact Global predecessor → Create fallback evaluable;
- brak Global predecessor → TRANSITION_DEPENDENCY_UNCAPTURED;
- current Global snapshot nie może zostać zastosowany wstecz;
- fee config nie jest wymagany dla reserve-only transition;
- bit-exact final anchor różniący się o 1 lamport → conflict.

#### Participant balance

- canonical ATA + exact pre/post → Known;
- v0 loaded canonical ATA → Known z poprawnym message index;
- non-ATA account nie otrzymuje CanonicalTradeTokenAccount;
- unrelated/touch-only account nie staje się participant inventory;
- brak pre albo post nie jest imputowany jako zero;
- kilka mutations na tym samym token account → per-mutation balance Unknown;
- participant balance Unknown nie odbiera Exact reserve trajectory;
- --require-evidence participant_balance odrzuca row bez pełnego canonical
  evidence.

#### Istniejące testy V1.1

Pozostają:

- multi-Buy/Sell same curve;
- multiple curves same signature;
- Create + initial Buy;
- unknown mutation;
- failed transaction;
- missing pre/final anchor;
- missing final txn_signature;
- same-version/different-hash account conflict;
- direct event state mismatch;
- final state mismatch;
- clean drain;
- queue saturation;
- one typed gap episode;
- crash-incomplete segment;
- process restart boundary;
- window coverage gap;
- chain/observed time separation;
- parser parity przy capture disabled.

### 4.5. Bramy jakości

#### Correctness

~~~text
false EXACT = 0
silent dropped mutations = 0
unknown mutation classified = 100%
conservation mismatch w EXACT = 0
final-anchor mismatch w EXACT = 0
non-rooted mutation w EXACT = 0
program-version mismatch w EXACT = 0
missing required dependency w EXACT = 0
~~~

#### Source completeness

~~~text
missing canonical Pump transaction = 0
duplicate canonical identity mismatch = 0
all finalized slots in qualification range audited = 100%
direct/CPI/router/v0 supported classes proven = 100%
audit-to-tape writes = 0
~~~

#### Exact coverage

Dla rooted canonical successful mutations:

\[
\text{exact mutation coverage} \ge 99.9\%
\]

Każdy brak ma typed reason. Dead, unresolved i failed są raportowane oddzielnie,
nie usuwane przed klasyfikacją.

#### Per-launch

COMPLETE wymaga:

- birth w qualified rooted range;
- wszystkie curve-mutating transactions w window mają Exact trajectory;
- brak local coverage gap;
- brak process boundary;
- brak unresolved canonicality;
- brak program version boundary;
- wymagane przez eksportera optional evidence jest Known;
- cały observation/forward horizon mieści się w qualified range.

#### Capture hot path — SUPERSEDED FOR STANDALONE PR-A BY AMENDMENT B

> Poniższe ratio było kontraktem dla innej topologii („capture jako dodatkowy
> hook parser workera”). Nie jest bramką launchową standalone PR-A. Obowiązuje
> normatywna bramka z Amendment B: real ingress/writer/segment accounting,
> zero loss/gaps/errors oraz enabled `try_capture` p99 `<= 100 µs`; frozen
> capture-disabled parser parity pozostaje oddzielnym dowodem.

~~~text
capture disabled:
parser output/parity unchanged

capture enabled:
throughput ratio >= 0.98
p99 latency ratio <= 1.05
parser-worker blocking waits = 0
disk I/O na receive/parser task = 0
silent loss = 0
~~~

#### Qualification run

Minimum:

~~~text
30 minut
lub
10 000 successful rooted-canonical Pump mutations
~~~

oraz:

~~~text
live execution disabled
required research artifact
clean writer shutdown
single Pump ProgramData version
full qualified slot-range independent audit
wszystkie cztery source-filter classes udowodnione
~~~

Jeżeli kryterium liczby/czasu przejdzie wcześniej niż source-filter class proof,
run trwa dalej do uzyskania dowodu albo zostaje niezakwalifikowany.

### 4.6. Stop conditions

Implementacja lub qualification zatrzymuje się przy:

~~~text
SOURCE_EVIDENCE_INSUFFICIENT
MUTATION_INVENTORY_INCOMPLETE
TRANSITION_SEMANTICS_UNRESOLVED
SOURCE_FILTER_CPI_COVERAGE_UNPROVEN
SOURCE_COVERAGE_UNPROVEN
PROGRAM_VERSION_BOUNDARY
TRANSITION_DEPENDENCY_UNCAPTURED
UNRESOLVED_CANONICALITY
CREATE_V2_UNSUPPORTED
~~~

UNRESOLVED_CANONICALITY zatrzymuje trajectory/window albo qualification range
zawierający dany slot; unresolved tail pozostaje jawnie raportowany.

Writer saturation nadal wymaga najpierw pomiaru bytes/event, serialization cost
i queue dwell. Nie autoryzuje unbounded queue, Kafka, ClickHouse ani nowego
spill systemu.

Żadnego stop condition nie wolno „naprawić” przez:

- RPC insertion/backfill do raw tape;
- nearest snapshot;
- join po curve+slot;
- current Global/fee config użyty historycznie;
- rounding tolerance;
- podmianę computed final state final anchorem;
- ciche usunięcie dead/unresolved rows;
- usunięcie multi-mutation transactions z denominatora;
- założenie kompletności provider filter;
- potraktowanie jednego token account jako wallet-total inventory.

## 5. Blast radius, założenia i potwierdzenie granic

### 5.1. Blast radius

Architektoniczny i runtime blast radius nie zmienia się.

Korekty powodują jedynie ograniczone rozszerzenia wewnątrz wcześniej
zaakceptowanego research scope:

- PR-A: source schema metadata, frozen codec, SlotUpdate evidence, ProgramData
  receipts i jeden exact Pump Global account filter;
- PR-B: canonicality classifier, minimalny Global decoder i participant balance
  materialization;
- qualification: read-only finalized-block comparison;
- artefakty: protobuf descriptor fixture, golden binary fixtures i
  source-completeness report.

Blast radius konfiguracji zmniejsza się względem wcześniejszego wariantu planu:
research_tape nie jest dodawane do aktywnego SeerConfig. Używany jest standalone
PumpResearchCaptureConfigV1.

Planowane obszary kodu pozostają ograniczone do:

- ghost-core/src/pump_research_tape.rs;
- research-only modułów i binary w off-chain/components/seer;
- minimalnych addytywnych zmian w Seer grpc_connection, binary_parser i
  eksportach modułów.

Nie jest wymagana zmiana ghost-launcher, ghost-brain, runtime Gatekeepera,
AccountStateCore ani execution backendu.

### 5.2. Jawne potwierdzenie

~~~text
Gatekeeper unchanged: TAK
MFS unchanged: TAK
execution unchanged: TAK
active Seer runtime unchanged: TAK
no strategy implementation: TAK
~~~

Active Seer runtime unchanged oznacza:

- istniejący connect_geyser() zachowuje dotychczasowe zachowanie;
- research profile nie jest wybierany przez production config;
- standalone capture nie emituje runtime events;
- parser research inventory jest addytywny;
- capture/research disabled zachowuje frozen parser parity;
- nie zmienia się Event Bus, OracleRuntime ani live/shadow authority.

### 5.3. Założenia zamrożone dla implementacji

- Lokalny baseline to obecny checkout
  832728c9af9aec92bfa3edea8fa9518ee90f7d5b; origin/main pozostaje
  43057b296663129ca9b4f572e793474830a5452c.
- Dirty worktree należy zachować; implementer nie może wykonywać resetu ani
  obejmować niepowiązanych 136 zmian.
- CS0 ma odizolować wyłącznie zaakceptowane dependency/parsing hunks wymagane
  przez V1.
- Frozen protobuf/client/prost versions pochodzą z aktualnego Cargo.lock.
- Pump Program ID pozostaje
  6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P.
- Minimalnym mutable account dependency jest wyłącznie canonical Pump Global.
- Fee schedule nie jest authority dla reserve trajectory V1.
- Qualification audit provider jest niezależny od primary Yellowstone
  provider.
- Independent audit może wyłącznie weryfikować.
- Brak dowodu jest wynikiem typed non-evaluable/unqualified, nigdy imputacją.

## 6. Klasyfikacja i ślad routingu

~~~yaml
task_classification: "cross-cutting research ingest and replay implementation"
primary_specialist: "Seer Ingest Event Integrity Specialist"
supporting_specialists:
  - "Config Rollout Safety Reviewer"
  - "Decision Logging Replay Analyst"
  - "Rust low-latency and bounded-concurrency engineering"
  - "Solana Pump.fun architecture"
runtime_area_touched:
  - "new standalone research-only Yellowstone capture"
  - "offline materialization"
  - "offline qualification"
contracts_at_risk:
  - "source identity and completeness"
  - "slot/fork canonicality"
  - "binary replay compatibility"
  - "bounded queue and shutdown semantics"
  - "program-version provenance"
  - "historical mutable dependency attribution"
active_or_legacy_path: "new research-only path; active runtime explicitly unchanged"
recommended_action: "implement strictly in CS0 → PR-A → capture-enabled local A/B gate → operator-approved prospective capture → immutable-output inspection → PR-B → qualification order"
verification_steps:
  - "freeze descriptor and binary golden fixtures"
  - "prove capture-disabled parser parity"
  - "qualify bounded writer behavior"
  - "prove rooted/dead/unresolved classification"
  - "run independent finalized-block source audit"
  - "prove direct/CPI/router/v0 filter coverage"
  - "verify ProgramData start/completion identity"
  - "audit exact trajectory denominator and per-launch completeness"
risk_level: "high"
~~~

~~~yaml
delegation_trace:
  task_classification: "non-trivial cross-cutting ingest, storage, replay and qualification implementation"
  routing_performed: true
  primary_specialist: "Seer Ingest Event Integrity Specialist"
  supporting_specialists_considered:
    - "Config Rollout Safety Reviewer"
    - "Decision Logging Replay Analyst"
    - "Rust Master"
    - "Solana Pump.fun Architect"
    - "Statistical Research Engine"
  specialist_docs_to_use:
    - "docs/agents/seer-ingest-event-integrity-specialist.md"
    - "docs/agents/config-rollout-safety-reviewer.md"
    - "docs/agents/decision-logging-replay-analyst.md"
  specialist_docs_not_required:
    - name: "Gatekeeper Policy Auditor"
      reason: "Gatekeeper policy, verdict order and thresholds są poza scope."
    - name: "SSOT Feature Materialization Guardian"
      reason: "MaterializedFeatureSet i materialization authority pozostają bez zmian."
    - name: "Solana Execution Path Engineer"
      reason: "Brak zmian transaction construction, submission, confirmation i reconciliation."
    - name: "Oracle Session Runtime Engineer"
      reason: "Brak zmian OracleRuntime session lifecycle i Event Bus routing."
  skills_to_use:
    - "ghost-execution"
    - "solana-pumpfun-architect"
    - "rust-master"
    - "statistical-research-engine"
  fast_path_used: false
  contracts_to_check:
    - "ingest identity and ordering"
    - "processed versus rooted canonicality"
    - "source-filter completeness"
    - "bounded nonblocking backpressure"
    - "immutable binary replay"
    - "time-axis separation"
    - "program and mutable-state provenance"
    - "Gatekeeper and MFS isolation"
    - "shadow/live and execution isolation"
  unresolved_routing_uncertainty: []
~~~

## 7. Finalny werdykt planu

READY_FOR_IMPLEMENTATION
