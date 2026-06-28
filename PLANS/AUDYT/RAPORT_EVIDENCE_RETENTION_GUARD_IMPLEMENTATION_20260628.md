# Evidence retention guard implementation

Data: `2026-06-28`

Status: `IMPLEMENTED / OFFLINE_GUARD_ONLY / NO_RUNTIME_CHANGE`

## Cel

Dodac operacyjny guard blokujacy przypadkowy cleanup rollout/shadow evidence przed jakimkolwiek nowym research.

To jest etap guard-only. Nie startuje RUG-MARKUP-A0, nie startuje zadnego runu i nie wykonuje cleanupu.

## Utworzony guard

`scripts/guard_rollout_evidence_cleanup.py`

## Wymagania i implementacja

| wymaganie | implementacja |
| --- | --- |
| dry-run default | Bez `--execute` skrypt tylko raportuje plan/manifest status i nie usuwa plikow. |
| explicit scope allowlist required | `--scope` jest wymagane; scope nie moze byc sciezka, globem, `all`, `rollout`, `shadow_run`, `.` ani `..`. |
| refuse broad roots | Skrypt odmawia rootow typu `/`, `/root`, `/tmp`, `/mnt`, repo root, `logs`, archive root i rootow pod `/mnt/HC_Volume_105935807`. |
| refuse deletion of critical evidence files | Skrypt blokuje cleanup, jezeli target zawiera krytyczne evidence pliki, np. `gatekeeper_v2_decisions.jsonl`, `shadow_lifecycle.jsonl`, `shadow_exit_replay_v1.jsonl`. |
| require pre-delete manifest | `--execute` wymaga `--manifest` z istniejacym manifestem zgodnym z aktualnym file set. |
| require archive_verified=true | `--execute` wymaga `--archive-verified true` i `archive_verified: true` w manifescie. |
| require second confirmation token | `--execute` wymaga `--confirm-token DELETE_EVIDENCE:<digest>` wyliczonego z manifestu. |
| no runtime changes | Skrypt jest offline CLI i nie jest importowany przez runtime. |
| no research run | Nie uruchomiono zadnego runu. |
| no cleanup execution | Nie uruchomiono `--execute`; cleanup nie zostal wykonany. |

## Tryby uzycia

Dry-run bez manifestu:

```bash
python3 scripts/guard_rollout_evidence_cleanup.py \
  --root logs/rollout \
  --scope <explicit-scope>
```

Dry-run z wygenerowaniem pre-delete manifestu:

```bash
python3 scripts/guard_rollout_evidence_cleanup.py \
  --root logs/rollout \
  --scope <explicit-scope> \
  --write-manifest reports/selector/<scope>/pre_delete_manifest.json
```

Execution guard, tylko po osobnej decyzji operacyjnej:

```bash
python3 scripts/guard_rollout_evidence_cleanup.py \
  --root logs/rollout \
  --scope <explicit-scope> \
  --manifest reports/selector/<scope>/pre_delete_manifest.json \
  --archive-verified true \
  --confirm-token DELETE_EVIDENCE:<digest> \
  --execute
```

## Fail-closed cases

Guard ma odmowic, gdy:

- scope allowlist jest puste,
- scope jest globem albo sciezka,
- root jest broad root,
- root jest pod archive volume,
- target zawiera symlink,
- target zawiera critical evidence file,
- manifest nie istnieje,
- manifest nie pasuje do aktualnego file set,
- `archive_verified` nie jest `true`,
- confirmation token nie pasuje.

## Runtime boundary

`runtime_changed = false`

`research_run_started = false`

`cleanup_executed = false`

Nie bylo zmian:

- Gatekeeper,
- BUY/REJECT,
- selector runtime,
- TX/Jito/live path,
- config rollout,
- log schema.

## Decyzja operacyjna

Do czasu zacommitowania tego guarda nie wolno startowac RUG-MARKUP-A0.

Po zacommitowaniu, kazdy cleanup evidence musi przejsc przez ten guard albo przez rownowazny, jawnie zatwierdzony proces z manifestem, archive verification i confirmation token.
