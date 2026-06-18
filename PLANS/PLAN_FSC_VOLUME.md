# Plan: FSC Capture na dedykowanym wolumenie z bezpiecznym TTL

## Cel

Celem jest utrzymanie FSC jako potencjalnego modulu diagnostycznego bez ryzyka
zapelniania glownego filesystemu serwera. Aktualny model raw capture FSC jest
nieakceptowalny kosztowo: przy niskim coverage generuje dziesiatki GB danych,
a po naprawie coverage moglby skalowac sie do setek GB lub TB na dobe.

Plan wdraza wariant `FSC-only`:

- logiczna sciezka `logs/nln_capture/<scope>` pozostaje kompatybilna dla runtime
  i skryptow offline,
- fizyczny backend tej sciezki jest przeniesiony na dedykowany wolumen przez
  bind mount,
- capture przechodzi z plaskich, stale rosnacych plikow JSONL na rotowane
  segmenty,
- osobny janitor usuwa wylacznie zamkniete segmenty starsze niz bezpieczny TTL,
- wartosci TTL i rozmiar wolumenu zostaja dobrane dopiero po naprawie FSC
  coverage i pomiarze realnego wymaganego lookbacku.

Ten plan nie naprawia FSC coverage. Ten plan zabezpiecza storage, zeby naprawione
coverage FSC nie zabilo dysku.

## Decyzje architektoniczne

### 1. Zakres storage: FSC-only

Przenosimy tylko FSC/NLN raw capture i ewentualny FSC-derived scratch.

Nie przenosimy i nie wlaczamy do cleanupu:

- `logs/rollout`,
- `logs/shadow_run`,
- `reports`,
- `datasets/events`,
- DecisionLogger paths,
- shadow lifecycle,
- WAL/snapshots,
- innych runow niezwiązanych z FSC capture.

Uzasadnienie:

- cleanup FSC nie moze przypadkowo usunac dowodow decyzyjnych,
- dotychczasowe skrypty czesto zakladaja logiczna sciezke
  `logs/nln_capture/<scope>`,
- bind mount pozwala zachowac kompatybilnosc bez przepisywania wszystkich
  narzedzi od razu.

### 2. Model cleanupu: segmenty + janitor

Nie wolno opierac TTL o usuwanie jednego aktywnego append-only JSONL.

Powod:

- w Linuxie usuniecie otwartego pliku nie musi zwolnic miejsca, dopoki proces
  trzyma deskryptor,
- usuniecie aktywnego pliku moze rozwalic ciaglosc capture,
- aktywny writer i cleanup musza miec jasna granice odpowiedzialnosci.

Docelowo:

- runtime writer tylko zapisuje, flushuje, rotuje i zamyka segmenty,
- janitor tylko czyta manifest i usuwa zamkniete segmenty,
- aktywne segmenty `.open.jsonl` sa nietykalne.

### 3. TTL nie jest stala wartoscia planu

TTL zostaje dobrany po naprawie FSC coverage.

Implementacja musi jednak wymuszac twardy warunek:

```text
cleanup_ttl_ms >= fsc_required_lookback_ms + safety_buffer_ms
```

Do czasu pomiaru:

- cleanup moze dzialac tylko w `dry-run`,
- albo `artifact_cleanup_enabled = false`.

## Zmiany implementacyjne

### 1. Bind mount FSC capture na dedykowany wolumen

Przygotowac docelowy katalog na powiekszalnym wolumenie, np.:

```text
/mnt/<FSC_VOLUME>/ghost-fsc/nln_capture
```

Logiczna sciezka w repo pozostaje:

```text
/root/Gho/logs/nln_capture
```

Docelowy mount:

```text
/root/Gho/logs/nln_capture -> /mnt/<FSC_VOLUME>/ghost-fsc/nln_capture
```

Wariant operacyjny:

```bash
mkdir -p /mnt/<FSC_VOLUME>/ghost-fsc/nln_capture
mkdir -p /root/Gho/logs/nln_capture
mount --bind /mnt/<FSC_VOLUME>/ghost-fsc/nln_capture /root/Gho/logs/nln_capture
```

Docelowo mount powinien byc utrwalony przez systemd mount unit albo `/etc/fstab`,
ale sam run Ghost musi miec preflight, ktory blokuje start FSC capture, jesli
sciezka logiczna nie jest na oczekiwanym mount source.

### 2. Preflight storage przed startem runu FSC

Dodac preflight dla profili FSC capture:

- `logs/nln_capture` istnieje,
- jest writable,
- `findmnt -T /root/Gho/logs/nln_capture` pokazuje oczekiwany wolumen,
- sciezka nie znajduje sie na `/dev/sda1` root filesystem,
- wolne miejsce przekracza minimalny budzet operatora,
- jesli cleanup ma byc wlaczony, TTL przechodzi walidacje lookback + buffer.

Brak poprawnego mounta ma blokowac FSC capture run.

Nie wolno fallbackowac do pisania na root filesystem.

### 3. Segmentowy writer w runtime

Zmienic obecny model plaskich plikow JSONL w writerach NLN/FSC.

Potwierdzone miejsca do zmiany:

- `ghost-launcher/src/components/seer.rs`
  - `NlnArtifactCaptureConfig`,
  - `spawn_nln_artifact_writer`,
  - `open_nln_artifact_file`,
  - `write_nln_artifact_line`,
  - `flush_nln_artifact_writer`.
- `off-chain/components/seer/src/lib.rs`
  - `spawn_raw_pumpfun_instruction_evidence_writer`.

Obecny writer tworzy plaskie pliki, m.in.:

- `pumpfun_create_raw_v1.jsonl`,
- `pumpfun_trade_raw_v1.jsonl`,
- `nln_pumpfun_buy_raw_v1.jsonl`,
- `nln_pumpfun_buy_exact_sol_in_raw_v1.jsonl`,
- `system_transfers_raw_v1.jsonl`,
- `nln_normalization_errors_v1.jsonl`,
- `nln_candidate_birth_v1.jsonl`,
- `route_manifest_evidence_candidates_v1.jsonl`,
- `funding_events_v1.jsonl`,
- `raw_pumpfun_instruction_evidence_v1.jsonl`.

Docelowo kazdy artifact ma katalog segmentow:

```text
logs/nln_capture/<scope>/segments/<artifact>/
```

Format nazw:

```text
<artifact>.<start_ms>.<seq>.open.jsonl
<artifact>.<start_ms>.<end_ms>.<seq>.closed.jsonl
```

Reguly:

- writer otwiera tylko segment `.open.jsonl`,
- rotacja nastepuje po czasie albo po rozmiarze,
- przed zamknieciem segment jest flushowany,
- po flushu writer atomowo zmienia suffix z `.open.jsonl` na `.closed.jsonl`,
- writer natychmiast otwiera kolejny segment `.open.jsonl`,
- writer nie usuwa zadnych plikow,
- przy bledzie zapisu degraduje tylko artifact capture lane.

### 4. Manifest segmentow

Dodac append-only manifest:

```text
logs/nln_capture/<scope>/capture_segments_manifest_v1.jsonl
```

Minimalne rekordy:

- `segment_opened`,
- `segment_closed`,
- `segment_cleanup_deleted`,
- `segment_cleanup_skipped`.

Minimalne pola:

- `schema_version`,
- `scope`,
- `artifact`,
- `event_type`,
- `segment_path`,
- `segment_state`,
- `seq`,
- `start_ts_ms`,
- `end_ts_ms`,
- `row_count`,
- `bytes`,
- `close_reason`,
- `cleanup_reason`,
- `writer_instance_id`.

Manifest jest SSOT dla cleanupu.

Janitor nie powinien zgadywac stanu segmentu tylko po nazwie, poza negatywnym
guardem: `.open.jsonl` zawsze jest nietykalne.

### 5. Config surface

Dodac pola do `seer.program_streams` z `#[serde(default)]`.

Domyslnie wszystko musi byc backward-compatible i inert.

Proponowane pola:

```toml
artifact_segmented_capture_enabled = false
artifact_segment_duration_ms = 300000
artifact_segment_max_bytes = 1073741824
artifact_segment_manifest_enabled = true
artifact_cleanup_enabled = false
artifact_cleanup_ttl_ms = 0
artifact_cleanup_min_retained_closed_segments = 2
artifact_cleanup_max_bytes = 0
artifact_cleanup_safety_buffer_ms = 900000
artifact_cleanup_expected_mount_source = ""
```

Semantyka:

- `artifact_segmented_capture_enabled = false` zachowuje stary flat JSONL path,
- `true` wlacza nowy segment writer,
- cleanup domyslnie jest wylaczony,
- `artifact_cleanup_ttl_ms = 0` oznacza brak realnego usuwania,
- `artifact_cleanup_expected_mount_source` jest uzywane przez preflight.

### 6. Compatibility layer dla skryptow offline

Istniejace narzedzia czesto czytaja:

```text
logs/nln_capture/<scope>/<artifact>.jsonl
```

Nie wolno wymuszac kopiowania segmentow do jednego wielkiego pliku.

Dodac wspolny resolver/iterator w Pythonie, np.:

```text
scripts/lib/fsc_capture_paths.py
```

Zachowanie:

1. Jesli istnieje segmented manifest, zwroc zamkniete segmenty dla artifactu
   w porzadku `start_ts_ms`, `seq`.
2. W trybie live-tail opcjonalnie dolacz aktualny `.open.jsonl`.
3. Jesli segmented manifest nie istnieje, fallback do starego flat JSONL.
4. Jesli znaleziono oba formaty, preferuj segmented i loguj warning o mixed mode.

Zaktualizowac co najmniej:

- `scripts/run_fsc_v2_pr8_artifact_builder_loop.sh`,
- `scripts/build_fsc_v2_provider_qualification.py`,
- skrypty route evidence czytajace `logs/nln_capture/<scope>`.

### 7. Janitor FSC capture

Dodac osobny skrypt:

```text
scripts/fsc_capture_janitor.py
```

Tryby:

- `--dry-run` domyslnie,
- `--apply` wymagane do kasowania,
- `--scope <scope>` dla jednego runu,
- `--all-scopes` tylko z jawna flaga,
- `--ttl-ms`,
- `--min-retained-closed-segments`,
- `--max-bytes`,
- `--expected-mount-source`,
- `--lookback-ms`,
- `--safety-buffer-ms`.

Twarde reguly:

- usuwaj tylko `*.closed.jsonl`,
- usuwaj tylko segmenty obecne w manifest jako `segment_closed`,
- nigdy nie usuwaj `*.open.jsonl`,
- nigdy nie wychodz poza `logs/nln_capture`,
- odrzuc symlinki w drzewie segmentow,
- odrzuc sciezki z `..`,
- wymagaj zgodnego mount source,
- odrzuc TTL mniejszy niz `lookback_ms + safety_buffer_ms`,
- zostaw minimum N zamknietych segmentow per artifact.

Janitor zapisuje audit:

```text
logs/nln_capture/<scope>/capture_cleanup_audit_v1.jsonl
```

Minimalne metryki:

- liczba usunietych segmentow,
- usuniete bajty,
- skipped open,
- skipped too new,
- skipped min retained,
- skipped not in manifest,
- skipped mount mismatch,
- wolne miejsce przed/po.

### 8. Hard cap i degrade behavior

Jesli katalog capture przekracza `artifact_cleanup_max_bytes`:

- janitor probuje usunac najstarsze kwalifikujace sie zamkniete segmenty,
- jesli nie da sie zejsc pod cap bez naruszenia TTL/lookback, runtime ma oznaczyc
  FSC capture jako degraded,
- degradacja dotyczy tylko FSC artifact lane,
- Seer/Gatekeeper/execution nie moga byc blokowane przez cleanup.

Nie wolno cicho kontynuowac zapisu na root filesystem.

## Plan wdrozenia

### Etap 1: Implementacja bez kasowania

- Dodac config fields z defaultami.
- Dodac segment writer.
- Dodac manifest segmentow.
- Dodac Python resolver dla segmented/flat capture.
- Zaktualizowac builder FSC i route evidence skrypty do resolvera.
- Dodac janitor w `dry-run`, bez `--apply` w pierwszym smoke.

Acceptance:

- stare configi laduja sie bez zmian,
- nowe configi segmentuja capture,
- builder czyta segmented capture,
- janitor dry-run pokazuje kandydatow, ale nic nie usuwa.

### Etap 2: Mount preflight

- Dodac preflight dla FSC capture profili.
- Wymagac oczekiwanego mount source.
- Zablokowac run, jesli `logs/nln_capture` jest na root filesystem.

Acceptance:

- preflight PASS na poprawnym bind mount,
- preflight FAIL bez mounta,
- brak fallbacku na `/`.

### Etap 3: Canary z cleanup disabled

- Uruchomic krotki run FSC capture z segmentacja.
- Ustawic `artifact_segment_duration_ms` nisko, np. 60-300s.
- Cleanup tylko dry-run.

Acceptance:

- segmenty `.open` i `.closed` rotuja poprawnie,
- manifest jest spojny,
- builder czyta segmenty,
- root filesystem nie rosnie przez FSC capture.

### Etap 4: Canary z cleanup apply

Uruchomic dopiero po naprawie coverage i pomiarze realnego lookbacku.

- Wyliczyc TTL:

```text
TTL = measured_required_fsc_lookback + safety_buffer
```

- Wlaczyc janitor `--apply` na jednym scope.
- Monitorowac wolumen i coverage.

Acceptance:

- janitor usuwa tylko stare `.closed.jsonl`,
- FSC coverage nie spada przez zbyt agresywny TTL,
- aktywne segmenty nie sa usuwane,
- wolumen stabilizuje uzycie miejsca.

## Test plan

### Unit tests

- Config deserializes old profiles bez nowych pol.
- New config fields deserialize and default correctly.
- Segment writer rotates by duration.
- Segment writer rotates by max bytes.
- Segment close flushes then atomically renames `.open` to `.closed`.
- Manifest emits `segment_opened` and `segment_closed`.
- Janitor ignores `.open`.
- Janitor ignores files not present in manifest.
- Janitor refuses TTL lower than lookback + safety buffer.
- Janitor preserves minimum retained closed segments.
- Resolver reads flat JSONL when no manifest exists.
- Resolver reads segmented capture when manifest exists.

### Integration tests

- Tempdir capture run writes rows, rotates segments, then builder sees identical
  logical row count.
- Janitor dry-run returns expected delete plan.
- Janitor apply deletes only eligible closed segments.
- Bind-mounted logical path works with existing `logs/nln_capture/<scope>` contract.
- Preflight fails when mount source is wrong.

### Runtime smoke

- Start FSC canary with segment duration 60s and cleanup disabled.
- Confirm:
  - `nln_artifact_capture_available=1`,
  - rows grow,
  - manifest grows,
  - no root growth from FSC capture,
  - builder reads segmented inputs.

## Ryzyka i guardrails

### Ryzyko: usuniecie danych potrzebnych FSC

Guardrail:

- TTL musi byc wiekszy niz wymagany lookback + safety buffer.
- Janitor odmawia pracy przy zbyt malym TTL.
- Cleanup najpierw dry-run.

### Ryzyko: usuniecie aktywnego pliku

Guardrail:

- `.open.jsonl` jest nietykalne.
- Usuwane sa tylko segmenty `.closed.jsonl` obecne w manifest.

### Ryzyko: przypadkowe kasowanie innych logow

Guardrail:

- scope ograniczony do `logs/nln_capture`.
- cleanup allowlist-only.
- brak cleanupu `logs/rollout`, `logs/shadow_run`, `reports`, `datasets`, WAL.

### Ryzyko: bind mount nieaktywny i zapis na root

Guardrail:

- preflight wymaga oczekiwanego mount source.
- brak mounta blokuje FSC capture run.
- brak fallbacku do root.

### Ryzyko: builder offline nie widzi segmentow

Guardrail:

- wspolny resolver segmented/flat.
- test zgodnosci flat fixture vs segmented fixture.

## Assumptions

- Wolumen moze zostac powiekszony operacyjnie, wiec plan nie ustala stalego
  rozmiaru docelowego.
- TTL zostanie ustalony po naprawie FSC coverage i pomiarze realnego okna
  potrzebnego do lookupow.
- FSC pozostaje telemetry/evidence-only, dopoki osobny plan nie zmieni jego roli
  w policy.
- Implementacja tego planu wymaga osobnego ADR-8D po zmianach w kodzie.

## Delegation trace

```yaml
delegation_trace:
  task_classification: "storage_architecture_plan_for_fsc_capture"
  routing_performed: true
  primary_specialist: "Config Rollout Safety Reviewer"
  supporting_specialists_considered:
    - "Decision Logging Replay Analyst"
    - "Ghost Runtime Coordinator"
    - "Rust Runtime Engineer"
  specialist_docs_loaded:
    - "/root/Gho/.agents/skills/ghost-execution/SKILL.md"
    - "/root/Gho/.agents/skills/trading-systems/SKILL.md"
    - "/root/Gho/.agents/skills/rust-master/SKILL.md"
  specialist_docs_not_loaded:
    - name: "docs/agents/solana-execution-path-engineer.md"
      reason: "Plan dotyczy FSC capture/storage, nie konstrukcji ani symulacji transakcji."
    - name: "docs/agents/gatekeeper-policy-auditor.md"
      reason: "Plan nie zmienia decyzji Gatekeeper ani progow."
  skills_used:
    - "ghost-execution"
    - "trading-systems"
    - "rust-master"
  fast_path_used: false
  contracts_checked:
    - "shadow/live separation"
    - "config backward compatibility"
    - "DecisionLogger artifact isolation"
    - "runtime storage boundedness"
    - "FSC lookback safety"
    - "replay/audit discoverability"
  unresolved_routing_uncertainty:
    - "Finalny TTL i rozmiar wolumenu musza zostac dobrane po naprawie FSC coverage i realnym pomiarze wymaganej retencji."
```
