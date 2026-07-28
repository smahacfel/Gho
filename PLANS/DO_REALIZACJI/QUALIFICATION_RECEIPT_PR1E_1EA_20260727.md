# PR1E 1E-A — executable qualification receipt

Status: `1E-A QUALIFIED / EXPECTED BASELINE FAILURES RECORDED`
Base: `103212b16bfc059db367e1ceb3c7d00fd307d6c5`
Authority cutover: `NOT STARTED`

## Zamrożone artefakty

| Artefakt | BLAKE3 | SHA-256 |
|---|---|---|
| `ghost-launcher/src/pr1e_qualification.rs` | `f9fee32cd036f9126b98b20ee7075c5dba194104f14c589765b4a34dd42edccd` | `e1a2fecd0c85954392b87faecde3408526bb5ce6c19a3986459e4aba6a59d32f` |
| `ghost-launcher/tests/fixtures/pr1e/pr1e_corpus_manifest_v1.json` | `cd28c798082999cf2377842199ffabb6601f7115417da244a3cf864e5ef27208` | `2a7afd172806cac8e4a8542c7e0c82d04f3f3bac3fc1fd37933db841610608fe` |
| `ghost-launcher/tests/fixtures/pr1e/pr1e_cross_layer_scenarios_v1.jsonl` | `30fbf78344afd77958fe573af5c2414139023db4c770b03c0f710026b7cdd38c` | `d7e0b2fc0c26f496b7aeb4fa40c8552956f1217183f8d3cd4158e7d0dbef1c4c` |

## Manifest korpusów

Manifest odwołuje istniejące artefakty bez kopiowania:

- PR1B legacy parser projection:
  `549d66a347a3e56b516bc5b77a5f22929604442d409ece7eb1a55525eaa51202`;
- PR1C AccountObservationArbiter v2:
  `63839d047310638fe0d8643ee6c71148ac292f4390fc9098a2e573ce0ac1e051`;
- PR1D PumpObservationLedger v1:
  `833de2bd384c964712f2e7127f9bc1db57745644633c1c66facef540cdf4c2a4`;
- PR1D PumpObservationLedger v2:
  `c81d7b4f0cc3792c2bb2c4e71bfd0634fcfdd69723758d741ee2405770603415`;
- PR1E cross-layer v1:
  `30fbf78344afd77958fe573af5c2414139023db4c770b03c0f710026b7cdd38c`.

Cross-layer inventory zawiera dokładnie 23 scenariusze wymagane przez plan.

## Wykonywalny runner

Runner:

- używa produkcyjnego `PumpObservationLedgerV1`;
- używa produkcyjnego `CandidateIntegrityRegistry`;
- używa produkcyjnego `ingest_pump_observation()`;
- używa produkcyjnego `SessionPoolTradeBridge`;
- używa produkcyjnego Event Bus adaptera;
- nie zawiera fake Ledgera;
- nie zawiera fake CandidateIntegrity;
- nie zmienia aktywnej authority w 1E-A.

## Targeted gates

| Polecenie | Wynik |
|---|---|
| `cargo test -p ghost-launcher --lib pr1e_ -- --nocapture` | PASS — 2/2 |
| `cargo test -p ghost-core --test account_observation_arbiter_corpus_tests` | PASS |
| `cargo test -p ghost-core --test pump_observation_ledger_corpus_tests` | PASS |
| `cargo test -p seer canonical_parity_snapshot -- --nocapture` | PASS |
| `cargo fmt --all --check` | PASS |
| `git diff --check` | PASS |

## Pełny gate matrix 1E-A

Każda bramka została wykonana jeden raz na zamrożonym diffie 1E-A.
Historyczne czerwone wyniki porównano z
`BASELINE_RECEIPT_PR1E_103212B_20260727.md`; 1E-A nie naprawia ani nie
rozszerza fixture'ów bazowych.

| Polecenie | Czas | Wynik |
|---|---:|---|
| `cargo test -p ghost-core` | 13.34 s | `EXPECTED_BASELINE_FAILURE` — wyłącznie `foundational_types_serialize_and_deserialize_roundtrip`, `InvalidTagEncoding(104)` |
| `cargo test -p seer` | 5.42 s | `EXPECTED_BASELINE_FAILURE` — zamrożone 12 failure testów biblioteki oraz `source_router::test_geyser_mode_parses_raw_events` |
| `cargo test -p trigger` | 4.21 s | `EXPECTED_BASELINE_FAILURE` — dwa brakujące `status_uuid` oraz `presigned.size_bytes < 700` |
| `cargo test -p ghost-launcher` | 34.00 s | `EXPECTED_BASELINE_FAILURE` — testowy `PoolTransaction` E0063 z tym samym brakującym zbiorem pól |
| `cargo test --workspace` | 46.40 s | `EXPECTED_BASELINE_FAILURE` — testowy `PoolTransaction` E0063 w `metric_contracts_pr2b_producers.rs`; fixture istniał na bazie i nie został zmieniony przez 1E-A |
| `cargo build --release --workspace` | 528.75 s | `PASS` |

## Log receipts

| Log | SHA-256 |
|---|---|
| `/tmp/pr1e_1ea_ghost_core.log` | `54d210e3aa577188e3f4eeececc2c41c5c30f265fee3e6fe2902233cda26af94` |
| `/tmp/pr1e_1ea_seer.log` | `5efed2a9a5f720cce5fedc70df558cdb82bc1f1c36cec4960221f1dce775e6c3` |
| `/tmp/pr1e_1ea_trigger.log` | `cc5cb0c288946d4e74b3d3f60906a24bdbe8dc8e240ed45e1eea3d8838813455` |
| `/tmp/pr1e_1ea_launcher.log` | `c4ab2fd2eca8ede7f1d6c6086b17b4b2b86f14460273a2f1170e80c7d3bb5bfe` |
| `/tmp/pr1e_1ea_workspace.log` | `08491d24413844f6bc7b9f90fd9d402dc16293a7259c47aa93282f4da60a2df9` |
| `/tmp/pr1e_1ea_release.log` | `03a28084c9e882b027bd5d2172b481b4f47063ad96ffe63061c841af8d15d9ba` |

## Wniosek 1E-A

- istniejące digests PR1B/PR1C/PR1D pozostały niezmienione;
- executable runner używa produkcyjnych adapterów, Ledgera i registry;
- fake Ledger i fake CandidateIntegrity: `0`;
- authority cutover nadal nie rozpoczął się w tym commicie;
- release workspace build pozostaje zielony;
- jedyne czerwone wyniki są sklasyfikowanymi failure signatures zamrożonej
  bazy `103212b16bfc059db367e1ceb3c7d00fd307d6c5`.
