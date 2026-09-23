# ADR-8D: Pump Research Evidence Tape V1.1 — sealed Cargo-config closure i mechaniczna kwarantanna provenance

**Data:** 2026-08-16

**Status:** IMPLEMENTED / LOCAL-ONLY VERIFICATION PASSED / PROVIDER NO-GO

**Task:** `PUMP_RESEARCH_TAPE_CARGO_CONFIG_CLOSURE_AND_PROVENANCE_QUARANTINE`

## D0. Problem

Amendment E usunął inherited credential environment Cargo oraz legacy auth
fallback PR-A ProgramData RPC. Nie zamknął jednak całej hierarchii konfiguracji
Cargo: `env_clear()` nie powstrzymuje Cargo przed odczytem
`.cargo/config{,.toml}` z ancestorów `current_dir`. Taki config mógł wskazać
zewnętrzny compiler wrapper, linker lub runner, którego bytes nie były częścią
sealed source snapshotu.

Ponadto kwarantanna realnego raw runu
`pump-research-1786810567606-3429034` była opisana w planie i ADR, ale
certifier nie czytał run-local bindingu. Idealny independent audit mógłby więc
technicznie promować historyczny raw run do `Ready`, mimo starej podatnej
semantyki preflightu.

Ta korekta dotyczy wyłącznie standalone preflight/capture provenance oraz
offline promotion exact tape. Nie zmienia frozen raw V1 codec, parsera,
aktywnych ścieżek Seera, Gatekeepera, MFS, execution ani dataset bytes.

## D1. Decyzja: izolowany staging root i closed Cargo-config scope

Fresh build nie działa już z bundle/source snapshotu ani z bieżącego worktree.
Preflight kopiuje zweryfikowany snapshot do create-new staging root w systemowym
katalogu tymczasowym, a następnie buduje z `<staging>/source`. Fresh
`CARGO_HOME`, `HOME` i `CARGO_TARGET_DIR` są dziećmi tego samego staging root.

Przed Cargo i podczas zbierania build environment kod przechodzi wszystkie
ancestory stagingowego source root. Każdy `.cargo/config.toml` albo
`.cargo/config` poza snapshotem jest fatalnym błędem. Jedynymi dopuszczalnymi
config files są:

```text
<staging>/source/.cargo/config.toml
<staging>/source/.cargo/config
```

Nie wolno posiadać obu naraz. Ich digest jest częścią sealed build environment.
Repozytoryjne rustflags, jobs oraz release profile mogą pozostać, ponieważ są
zamrożonym source inputem. Odrzucane są natomiast compiler/wrapper/rustdoc/
target-dir, `[env]`, target linker/runner/ar, source/patch/replace i credential
provider surfaces. Nie próbujemy zgadywać bytes zewnętrznego executable po
samym configu: wybór takiego executable failuje before-Cargo.

Nie deklarujemy absolutnie hermetycznego host builda. Nazwa kontraktu brzmi
**sanitized sealed Rust build environment**. Controlled `PATH`, bezpośrednio
zahashowane Cargo/rustc, systemowy linker/C compiler oraz read-only offline
cache/index/git DB są platformowymi inputami. Pełne host/container closure
wymagałoby osobnego image/tool manifestu i nie jest częścią V1.1.

## D2. Decyzja: capture provenance jest obowiązkowym inputem qualification

Nowy binding capture zapisuje aktualne `build_semantics`,
`credential_scan_semantics`, flagę `qualification_provenance_eligible = true`
oraz digest sealed release binary. Podczas indeksowania raw runu certifier
ocenia binding bez mutowania raw evidence.

Brak bindingu, błąd odczytu/parse, legacy semantyka, brak jawnej eligibility
albo rozjazd sealed binary digest daje internal provenance status ineligible.
Materialization offline nadal jest możliwa dla developmentu/forensics, ale
status qualification jest fail-closed:

```text
Blocked(CaptureProvenanceUnqualified)
```

Ta reguła ma wyższy priorytet niż status independent source audit. Audit może
opisać source coverage, lecz nie naprawia provenance starego capture.

## D3. Skutek dla historycznych artefaktów

Nie zmieniono żadnego segmentu, footeru, manifestu, receiptu ani exact outputu.

- `pump-research-1786810400363-3428808` pozostaje incomplete/forensic-only.
- `pump-research-1786810567606-3429034` ma legacy/podatny preflight binding;
  może zostać zmaterializowany wyłącznie rozwojowo, nigdy `Ready`.
- `exact-prb-20260816-2` pozostaje `Unqualified`; exporter nadal go odrzuca.

Nowy sealed receipt nie może retrospektywnie uzdrowić historycznych raw bytes.
Jedyny kandydat do future qualification musi pochodzić z replacement capture
wykonanego po tej korekcie oraz po osobnym operator GO.

## D4. Regresje i dowód

Dodano testy dla:

- ancestor Cargo config z external wrapperem — fail przed wywołaniem Cargo;
- snapshot Cargo config z `build.rustc-wrapper` i target tool references —
  fail;
- aktualnego snapshot configu z rustflags/jobs/release profile — pass;
- legacy bindingu z idealnym audit status — nadal
  `CaptureProvenanceUnqualified`;
- bindingu aktualnej semantyki z idealnym audit status — przechodzi wyłącznie
  bramkę provenance;
- mismatchu sealed binary digest — ineligible.

Wykonano pełny local-only test/compile receipt. Targetowane `research_tape`
testy obejmują wszystkie sześć przypadków z D4, a frozen raw V1, CS0,
parser-parity, CLI, standalone no-auth RPC i `grpc_connection` regression
suite również przeszły. Jawnie uruchomiony release harness zachował:

```text
received / admitted / accepted = 8,192 / 8,192 / 8,192
dropped / gaps / writer errors = 0 / 0 / 0
closed segments                = 1
receive hand-off p99           = 331 ns <= 100,000 ns
```

Zachowany local-only synthetic bundle z external configiem i originami
`.invalid` ma aktualną semantykę `...cargo_config_closure_v4`, tylko
`sealed_snapshot/.cargo/config{,.toml}` jako config inputs oraz zgodny digest
sealed executable. Skan wszystkich regular files bundle'a nie znalazł
synthetic credentiali, endpointów ani ścieżki external configu.

Nie wykonano tu RPC, Yellowstone, provider audit, capture ani exportu.

## D5. Rollback i granice

Rollback to niewykonywanie realnego preflightu/capture i pozostawienie
historycznych artefaktów w kwarantannie. Nie wolno przywracać ancestor Cargo
configów, external tool references ani traktować independent auditu jako
substytutu capture provenance.

Korekta pozostaje research-only. Nie dotyka `connect_geyser()`, SeerConfig,
Event Busa, AccountStateCore, canonical permitu, Gatekeepera, MFS, execution
ani strategii.
