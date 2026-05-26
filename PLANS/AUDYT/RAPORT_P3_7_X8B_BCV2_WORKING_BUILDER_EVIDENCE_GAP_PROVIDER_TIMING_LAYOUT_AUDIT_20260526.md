# RAPORT P3.7-X8B BCV2 Working-Builder Evidence Gap Provider/Timing/Layout Audit

Data: 2026-05-26

## Cel

X8B jest waskim audit-only etapem po X8AS. Celem jest odpowiedz na jedno pytanie:

Czy globalne `BCV2_ACCOUNT_UPDATE_RECEIVED` dotycza tych samych `bcv2_pubkey`, ktore blokuja working-builder readiness rows?

Zakres pozostaje diagnostyczny:

- join po `bcv2_pubkey`,
- dedupe rows vs unique pubkeys,
- klasyfikacja luki working-builder evidence,
- osobne oznaczenie watchdog/runtime inconclusive.

## Non-Goals

Nie ruszano:

- TX buildera,
- Helius Sendera,
- Gatekeepera,
- scoringu / thresholds / V3 policy,
- `legacy_buy` / fallback,
- R18 / P2 / live path.

## Implementacja Audit

Rozszerzono `scripts/v3_p37_mfs_lifecycle_join_key_audit.py` o sekcje `bcv2_working_builder_pubkey_join`.

Join obejmuje:

- `BCV2_EXACT_WATCH_REGISTERED`,
- `BCV2_EXACT_WATCH_SUBSCRIBE_INCLUDED`,
- `BCV2_EXACT_WATCH_RESUBSCRIBE_SENT`,
- `BCV2_ACCOUNT_UPDATE_RECEIVED`,
- `BCV2_RPC_HYDRATION_READY`,
- `BCV2_RPC_HYDRATION_MISSING`,
- working-builder `working_builder_bcv2_pubkey`,
- working-builder `working_builder_bcv2_precheck_pubkey`.

Wazny detal interpretacyjny:

- `BCV2_EXACT_WATCH_REGISTERED` niesie `pubkey` i `registry_version`.
- `BCV2_EXACT_WATCH_RESUBSCRIBE_SENT` niesie `registry_version`, ale nie niesie `pubkey`.
- `BCV2_EXACT_WATCH_SUBSCRIBE_INCLUDED` niesie counters, ale nie niesie `pubkey`.

Dlatego X8B przypisuje `SUBSCRIBE_INCLUDED` do pubkeya przez lokalna sekwencje:

`REGISTERED(pubkey, registry_version)` -> `SUBSCRIBE_INCLUDED(...)` -> `RESUBSCRIBE_SENT(registry_version)`.

To jest audit attribution, nie nowy runtime marker.

## Artefakty

Audit output:

- JSON: `logs/shadow_run/shadow-burnin-v3-p37-x8as-bcv2-exact-watch-coverage-restoration-smoke/v3_p37_x8b_working_builder_bcv2_pubkey_join_audit.json`
- MD: `logs/shadow_run/shadow-burnin-v3-p37-x8as-bcv2-exact-watch-coverage-restoration-smoke/v3_p37_x8b_working_builder_bcv2_pubkey_join_audit.md`

Runtime source pozostaje X8AS:

- namespace: `shadow-burnin-v3-p37-x8as-bcv2-exact-watch-coverage-restoration-smoke`
- watchdog exit: `exit=2`
- gRPC stall: `359166ms > 300000ms`

## Wyniki X8B

Rows markerow:

- `BCV2_EXACT_WATCH_REGISTERED`: `2304`
- `BCV2_EXACT_WATCH_SUBSCRIBE_INCLUDED`: `1508`
- `BCV2_EXACT_WATCH_RESUBSCRIBE_SENT`: `1504`
- `BCV2_EXACT_WATCH_SUBSCRIBE_DROPPED`: `0`
- `BCV2_RPC_HYDRATION_READY`: `0`
- `BCV2_RPC_HYDRATION_MISSING`: `366`
- `BCV2_ACCOUNT_UPDATE_RECEIVED`: `64`

Unique pubkeys po dedupe:

- `BCV2_EXACT_WATCH_REGISTERED`: `90`
- `BCV2_EXACT_WATCH_SUBSCRIBE_INCLUDED`: `88`
- `BCV2_EXACT_WATCH_RESUBSCRIBE_SENT`: `88`
- `BCV2_RPC_HYDRATION_READY`: `0`
- `BCV2_RPC_HYDRATION_MISSING`: `88`
- `BCV2_ACCOUNT_UPDATE_RECEIVED`: `15`

Working-builder join:

- `working_builder_bcv2_rows`: `20`
- `working_builder_bcv2_unique_pubkeys`: `20`
- `working_builder_bcv2_registered_unique_pubkeys`: `20`
- `working_builder_bcv2_included_unique_pubkeys`: `20`
- `working_builder_bcv2_resubscribe_sent_unique_pubkeys`: `20`
- `working_builder_bcv2_hydration_ready_unique_pubkeys`: `0`
- `working_builder_bcv2_hydration_missing_unique_pubkeys`: `20`
- `working_builder_bcv2_account_update_same_pubkey_unique_pubkeys`: `7`
- `global_bcv2_account_update_unique_pubkeys`: `15`
- `global_bcv2_account_update_other_pubkey_unique_pubkeys`: `8`
- `watchdog_fatal_rows`: `2`

## Odpowiedz Na Glowne Pytanie

Tak, czesc globalnych account updates dotyczy tych samych pubkeyow, ktore blokuja working-builder rows.

Konkretnie:

- `7 / 20` working-builder BCV2 unique pubkeys ma globalny same-pubkey `BCV2_ACCOUNT_UPDATE_RECEIVED`.
- `13 / 20` working-builder BCV2 unique pubkeys nie ma same-pubkey update.
- Globalne account updates obejmuja `15` unique pubkeys, z czego `8` to pubkeye spoza 20 working-builder blockers.

To zmienia blocker:

- nie jest to juz tylko `included_no_update`,
- dla `7 / 20` jest to `update_received_unmapped`,
- dla `13 / 20` pozostaje `included_no_update` / `other_pubkey_only`,
- dla `20 / 20` nadal jest `hydration_missing_after_include`,
- dla `20 / 20` runtime pozostaje `inconclusive_watchdog`.

## Klasyfikacja Luki

`classification_unique_pubkeys`:

- `working_builder_bcv2_hydration_missing_after_include`: `20`
- `working_builder_bcv2_provider_stall_before_evidence`: `20`
- `working_builder_bcv2_runtime_inconclusive_watchdog`: `20`
- `working_builder_bcv2_included_no_update`: `13`
- `working_builder_bcv2_update_received_other_pubkey_only`: `13`
- `working_builder_bcv2_update_received_unmapped`: `7`

`working_builder_bcv2_true_missing_or_not_loadable` nie zostal potwierdzony w X8B, bo run ma watchdog caveat i istnieje `7` same-pubkey update rows.

## Werdykt

X8B verdict: PASS-B / GAP CLASSIFIED.

PASS-A: NIE.

R18: NO-GO.

Execution unlock: NIE.

Uzasadnienie:

- exact-watch dziala dla 20/20 working-builder BCV2 pubkeys,
- wszystkie 20 byly registered, included i mialy resubscribe,
- wszystkie 20 mialy hydration missing po include,
- 7 z 20 dostalo same-pubkey account update globalnie, ale working-builder row nadal raportuje `working_builder_bcv2_account_update_received=false` / `mapped=false`,
- 13 z 20 nie dostalo same-pubkey account update w tym runie,
- watchdog exit=2 blokuje mocny wniosek, ze brak update bylby trwaly w pelnym 30-min burninie,
- observed tx meta nadal nie odblokowalo readiness bez evidence.

## Nastepny Waski Krok

Nie R18.

Nastepny etap powinien byc nadal audit/diagnostic:

1. dla `7 / 20` same-pubkey update rows sprawdzic, dlaczego marker `BCV2_ACCOUNT_UPDATE_RECEIVED` nie podniosl working-builder `account_update_received/mapped`;
2. dla `13 / 20` included-no-update sprawdzic provider timing oraz czy update mogl wypasc po watchdog stall;
3. osobno ustabilizowac watchdog/transport, bo obecny sample nie jest czystym 30-min burninem;
4. dopiero po tym decydowac, czy problem jest mapping/timing/provider, czy true missing/layout.

## Invarianty

Zachowane:

- no TX builder changes,
- no Sender changes,
- no Gatekeeper/scoring/threshold changes,
- no fallback/legacy revival,
- no R18/live evidence,
- no readiness unlock from observed tx meta alone.
