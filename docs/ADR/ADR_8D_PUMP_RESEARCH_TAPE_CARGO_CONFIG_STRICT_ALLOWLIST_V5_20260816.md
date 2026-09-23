# ADR-8D: Pump Research Evidence Tape V1.1 — strict Cargo-config allowlist v5

**Data:** 2026-08-16

**Status:** IMPLEMENTED / LOCAL-ONLY VERIFICATION PASSED / PROVIDER NO-GO

**Task:** `PUMP_RESEARCH_TAPE_CARGO_CONFIG_STRICT_ALLOWLIST_V5`

## D0. Problem

Amendment F odizolował ancestor Cargo config i odrzucił jawne pola wyboru
compilera, wrappera, linkera i runnera. Walidator nadal dopuszczał jednak
dowolne snapshotowane `build.rustflags`. Sam hash tekstu flag nie zamyka bytes
linkera, obiektu, biblioteki, sysrootu, target JSON ani response file, które
flagi mogą wskazać poza sealed source snapshotem.

Przykładowo kontrakt v4 mógł zaakceptować:

```toml
[build]
rustflags = ["-C", "linker=/tmp/unsealed-linker"]
```

Problem dotyczył fail-closed provenance przyszłego preflightu. Nie znaleziono
takiej flagi w bieżącym repozytoryjnym configu ani w zachowanym synthetic
bundle'u v4. Nie wykonano provider I/O, capture ani qualification.

## D1. Decyzja: closed schema zamiast denylisty

Snapshotowy `.cargo/config{,.toml}` jest walidowany ścisłą allowlistą. Jedyny
zatwierdzony kontrakt to:

```toml
[build]
rustflags = ["-C", "target-cpu=native"]
jobs = 4

[profile.release]
opt-level = 3
lto = true
codegen-units = 4
```

Pola mogą być pominięte i wtedy obowiązuje domyślna semantyka Cargo, ale każde
występujące pole musi mieć dokładnie zatwierdzoną wartość. Top-level może
zawierać wyłącznie `build` i `profile`; `profile` może zawierać wyłącznie
`release`. Nieznany table, klucz, typ albo wartość failuje przed Cargo.

Cała tabela `target` i `build.target` są niedozwolone. Tym samym nie istnieje
snapshotowa droga do external target JSON, target-specific `rustflags`,
linkera, runnera ani build-script override. Exact `rustflags` zamyka również
bypass przez `linker=`, `link-arg`, `-L`, `--sysroot`, `--extern` i response
files.

To nadal jest **sanitized sealed Rust build environment**, nie byte-level
hermetyczność hosta. Controlled system `PATH`, system linker/C compiler i
read-only offline Cargo cache pozostają jawnie platformowymi inputami.

## D2. Decyzja: nowa semantyka provenance

Build receipt, operator preflight receipt oraz run-local binding wymagają:

```text
fresh_cargo_target_locked_offline_release_from_isolated_snapshot_staging_clean_toolchain_binary_child_env_and_cargo_config_strict_allowlist_v5
```

V4 `...cargo_config_closure_v4` jest semantyką legacy. Materializer klasyfikuje
każdy taki binding jako ineligible, a idealny independent audit nadal daje:

```text
Blocked(CaptureProvenanceUnqualified)
```

W ten sposób zachowany synthetic bundle v4 oraz wszystkie starsze provider
runy nie mogą zostać użyte do qualification. Replacement canary wymaga nowego
create-new sealed preflightu v5.

## D3. Regresje

Regresje fail-closed obejmują:

- `build.rustflags = ["-C", "linker=/tmp/external-linker"]`;
- `link-arg=/tmp/object.o`, `-L native=/tmp/libs`, external sysroot, `--extern`
  i response file;
- `build.target = "/tmp/external-target.json"`;
- całą tabelę `target`;
- nieznany top-level table oraz nieznany klucz w `build`, `profile` lub
  `profile.release`;
- inną wartość `jobs`, `opt-level`, `lto` lub `codegen-units`;
- dokładny bieżący repozytoryjny config jako przypadek pozytywny;
- binding v4 z idealnym auditem jako przypadek niekwalifikowalny.

Pełny local-only scope review, testy debug/release oraz zachowany synthetic
preflight v5 przeszły bez użycia realnych endpointów i credentiali. Nie
uruchomiono RPC, Yellowstone, provider audit, capture, export ani strategii.

## D4. Wpływ i rollback

Zmiana jest research-only. Nie modyfikuje frozen raw V1, parsera, aktywnego
`connect_geyser()`, `SeerConfig`, Event Busa, AccountStateCore, Gatekeepera,
MFS, execution ani dataset bytes.

Rollback operacyjny oznacza niewykonywanie realnego preflightu/capture. Nie
wolno przywrócić semantyki v4 ani promować v4 bundle'a do qualification.
Przyszła potrzeba innego bezpiecznego pola Cargo config wymaga jawnej nowej
semantyki provenance i osobnych regresji.
