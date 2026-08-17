# ADR-8D: Pump Research Capture — izolacja środowiska childa i postcondition runu

**Data:** 2026-08-16

**Status:** IMPLEMENTED / LOCAL-ONLY / PROVIDER I/O HOLD

**Task:** `PUMP_RESEARCH_CAPTURE_SUPERVISOR_ENV_AND_POSTCONDITION`

## D0. Problem

Pierwsza wersja exact-child supervisora poprawnie zachowywała PID, pidfd,
sygnały i pojedynczy `waitpid()`, ale miała dwie luki operatorskie:

- kopia środowiska była przekazywana do `Popen` przed usunięciem legacy
  credential aliases, więc capture child nadal je dziedziczył;
- exit code `0` był zwracany bez wymagania dokładnie jednego nowego,
  poprawnie domkniętego raw runu.

Qualification preparation receipt został ponadto opisany zbyt mocno jako
sealed/immutable, mimo że jest owner-writable create-new snapshotem. Żaden z
tych problemów nie naruszył zaakceptowanego GO-D raw i nie wymaga jego
ponowienia.

## D1. Decyzja: legacy credentials nie wchodzą do capture childa

Supervisor tworzy exact child environment przed `Popen` i usuwa z niego:

```text
GHOST_SEER_GRPC_X_TOKEN
GHOST_RPC_AUTH_TOKEN
```

Config nie może użyć żadnej z tych nazw jako dedykowanego credential env.
Dedykowane nazwy `grpc_auth_token_env` i `rpc_auth_token_env` pozostają w
środowisku childa, ponieważ są wymagane przez standalone capture. Po udanym
spawn zostają usunięte ze środowiska procesu supervisora. Wartości żadnego
credentialu nie są zapisywane do launch ani execution receipt.

## D2. Decyzja: operatorski sukces wymaga dokładnie jednego Complete runu

Po jednym finalnym `waitpid()` supervisor wylicza addytywny postcondition.
Sukces wymaga łącznie:

1. child exit code `0`;
2. dokładnie jednego nowego `pump-research-*` run directory;
3. regularnego, niesymlinkowego `raw/`;
4. regularnego, niesymlinkowego `raw/run_completion_receipt.json`;
5. zgodnego `run_id`;
6. `status = Complete`;
7. `clean_shutdown = true`;
8. zera ścieżek `*.partial` w run directory.

Execution receipt zachowuje raw wait status oraz typed operator failure, między
innymi `NEW_RUN_COUNT_NOT_ONE`, `COMPLETION_STATUS_NOT_COMPLETE` i
`PARTIAL_PATH_PRESENT`. Exit `0` childa z niespełnionym postcondition staje się
niezerowym wynikiem supervisora, lecz nie modyfikuje raw runu ani wait statusu.

Wzajemne wykluczenie nie zależy od `operator_dir`. Supervisor przejmuje
`<canonical output_dir>/.pump-research-capture.lock` przed skanem procesów,
snapshotem runów i `Popen`, a zwalnia dopiero po zapisaniu execution receipt.
Scope jest jawnie output-directory-scoped.

## D3. Decyzja: preparation receipt jest snapshotem, provider suitability HOLD

Istniejący qualification preparation artifact jest create-new snapshotem,
nie mechanicznie immutable ani sealed receiptem. Oczekiwane SHA-256 wynoszą:

```text
qualification_preparation_receipt_v1.json
eab36576a3ad3284fe73da186186f04301a6b5a0809b2e592cf72ca3c7dd0787

/protected/operator/pump-research-audit-v1.toml
c5e1ebb6585639ebe33c70308a838e102d00aa5f45a46012b581e0cb56d9ca16
```

Każda przyszła operacja providerowa musi najpierw ponownie policzyć oba hashe
i związać je w nowym create-new execution receipt. Fizyczna dostępność,
retention i capacity niezależnego providera nie zostały sprawdzone. Provider
suitability probe oraz combined certify pozostają na HOLD i wymagają osobnego
GO. Ta zmiana nie wykonuje RPC, Yellowstone, certify, exportu ani strategii.

## D4. Wpływ, testy i rollback

Zmiana dotyczy wyłącznie research-only skryptu operatorskiego, jego regresji,
planu i ADR. Nie zmienia frozen raw V1, parsera, Yellowstone requestu, aktywnego
Seera, Event Busa, Gatekeepera, MFS ani execution.

Regresje obejmują:

- dedykowany credential obecny w childzie;
- oba legacy aliasy nieobecne w childzie;
- zakaz zadeklarowania legacy aliasu jako dedykowanego credential env;
- publiczny exit `0` z dokładnie jednym Complete runem;
- publiczny exit `0` bez nowego runu kończący się typed failure;
- dwa różne drzewa `operator-dir` ze wspólnym `output_dir`, z których tylko
  jedno może dojść do `Popen`;
- wiele nowych runów, nie-Complete receipt oraz obecny `*.partial` kończące się
  fail-closed;
- zachowanie dokładnie jednego `waitpid()` i surowego wait statusu.

Rollback oznacza niewykonywanie przyszłego supervisora. Nie cofamy ani nie
modyfikujemy GO-D raw. Do czasu osobnego provider-suitability GO zabronione są
provider I/O, combined certify, export-window oraz strategia.
