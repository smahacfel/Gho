# ADR-8D: Repo Main Worktree Reconciliation

Data: 2026-06-29

Status:

```text
ACCEPTED_AS_LOCAL_REPO_RECONCILIATION
```

## D1. Problem

Po audycie Shadow Burnin i pracach nad planem Shadow V2 istnialy dwa lokalne stany robocze:

- `/root/Gho`
- `/root/Gho-tsv2-a1-a2-clean`

Nowszy i czystszy stan merytoryczny byl w `/root/Gho-tsv2-a1-a2-clean`, ale docelowa nazwa repozytorium roboczego ma pozostac `/root/Gho`.

Cel operacji: przywrocic prace w `/root/Gho` na galezi `main`, z odswiezona zawartoscia z nowszego worktree, bez przenoszenia surowych logow, raw JSONL i starych artefaktow runtime.

## D2. Decision

Wykonano lokalna migracje w kierunku:

```text
/root/Gho-tsv2-a1-a2-clean -> /root/Gho
```

Zastosowano zasade:

- repozytorium robocze docelowo pozostaje pod `/root/Gho`,
- lokalna galaz robocza to `main`,
- pliki runtime/log/raw-artifact nie sa przenoszone,
- linked worktree `/root/Gho-tsv2-a1-a2-clean` zostaje usuniety po wykonaniu backupow i oczyszczonego eksportu.

## D3. Scope

Zakres zachowany w `/root/Gho`:

- kod z katalogow Ghost (`ghost-launcher`, `ghost-core`, `ghost-brain`, `src`, `off-chain`),
- `.agents`,
- `configs`,
- `docs`,
- `PLANS`,
- `scripts`,
- testy i specyfikacje,
- raporty audytowe i CSV niebedace surowymi logami runtime.

Zakres celowo wykluczony:

- `.git` z eksportu plikow,
- `target`,
- `logs`,
- `data`,
- `datasets`,
- `__pycache__`,
- `.pytest_cache`,
- `.mypy_cache`,
- `.ruff_cache`,
- `*.pyc`,
- `*.jsonl`,
- `runtime.log`,
- `*.wal`,
- `*.bin`,
- `*.tmp`,
- `reports/selector/shadow-burnin-*`,
- `reports/selector/shadow_run`,
- `run_lifecycle_guard_*`.

## D4. Git Boundary

Stan Git zostal odtworzony lokalnie po incydencie operacyjnym: pierwszy oczyszczony eksport zawieral plik `.git` z linked worktree, bo filtr wykluczal `.git/`, ale nie wykluczal pliku `.git`. Ten plik wskazywal na:

```text
/root/Gho/.git/worktrees/Gho-tsv2-a1-a2-clean
```

Incydent zostal naprawiony przez:

- zapisanie uszkadzajacego pliku `.git` w katalogu backupu,
- usuniecie pliku `.git` z eksportu,
- odtworzenie realnego katalogu `.git` z bundle backupu,
- ponowna synchronizacje z jawnym wykluczeniem `.git` oraz `.git/`.

Po naprawie `/root/Gho/.git` jest katalogiem Git, a nie plikiem linked-worktree.

## D5. Evidence

Backupi utworzone przed finalna migracja:

```text
/root/Gho_SOURCE_BACKUP_20260629T225647Z
/root/Gho_MIGRATION_BACKUPS_20260629T230625Z
/root/Gho_OLD_NONRUNTIME_SNAPSHOT_20260629T230625Z
/root/Gho_TSV2_SANITIZED_EXPORT_20260629T230625Z
/root/Gho_GIT_RESTORE_20260629T230625Z
```

Kluczowe pliki dowodowe:

```text
/root/Gho_MIGRATION_BACKUPS_20260629T230625Z/MIGRATION_MANIFEST.txt
/root/Gho_MIGRATION_BACKUPS_20260629T230625Z/gho_all_refs_before.bundle
/root/Gho_MIGRATION_BACKUPS_20260629T230625Z/gho_status_short_before.txt
/root/Gho_MIGRATION_BACKUPS_20260629T230625Z/tsv2_status_short_before.txt
/root/Gho_MIGRATION_BACKUPS_20260629T230625Z/bad_gitfile_overwrote_root_Gho_dotgit.txt
/root/Gho_MIGRATION_BACKUPS_20260629T230625Z/bad_gitfile_removed_from_sanitized_export.txt
```

Finalny lokalny punkt Git:

```text
branch: main
HEAD: 87fc232cc4a79dd21452286446611cbab6192344
```

Linked worktree:

```text
/root/Gho-tsv2-a1-a2-clean: removed
```

## D6. Runtime Boundary

Ta operacja:

- nie uruchamia runtime,
- nie startuje nowego runu,
- nie zatrzymuje R51,
- nie wykonuje RCE proof,
- nie zmienia BUY/REJECT,
- nie zmienia Gatekeeper policy,
- nie zmienia selector runtime,
- nie zmienia TX/Jito/live path,
- nie stage'uje plikow.

Zmiany sa stanem plikow roboczych repozytorium i wymagaja osobnej decyzji przed stagingiem, commitem lub push.

## D7. Consequences

`/root/Gho` jest ponownie glownym katalogiem roboczym.

`/root/Gho-tsv2-a1-a2-clean` zostal usuniety jako linked worktree.

Remote `origin` po odtworzeniu `.git` wskazuje lokalnie na bundle backupu:

```text
/root/Gho_MIGRATION_BACKUPS_20260629T230625Z/gho_all_refs_before.bundle
```

Pierwotny URL remote nie zostal wiarygodnie odzyskany z zachowanego `.git/config`. Lokalny upstream dla `main` zostal usuniety, zeby `git status` nie raportowal relacji do nieaktualnego `origin/main` z bundle backupu. Przed push/fetch do zewnetrznego remote trzeba jawnie ustawic poprawny `origin` i upstream.

Stare artefakty runtime i raw JSONL sa nieobecne w filesystemowym eksporcie, ale czesc z nich pozostaje jako tracked deletions wzgledem historii Git. To jest oczekiwany skutek przejscia na oczyszczona zawartosc i wymaga osobnej decyzji, czy deletions maja byc stage'owane/commitowane.

## D8. Verification

Wymagana weryfikacja po operacji:

```text
git status -sb --untracked-files=all
git worktree list --porcelain
git diff --check
git diff --cached --name-only
find /root/Gho -type f -name '*.jsonl'
find /root/Gho -type f -name 'runtime.log'
```

Do czasu osobnej decyzji ownera nie nalezy pushowac ani stage'owac zmian po tej migracji.
