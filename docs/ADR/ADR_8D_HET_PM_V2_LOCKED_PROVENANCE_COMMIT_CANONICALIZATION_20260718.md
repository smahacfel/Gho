# ADR-8D: HET-PM V2 — canonical runtime commit w locked promotion criteria

Data: 2026-07-18

Typ: ADR-8D / PR #73 promotion-evidence prerequisite / locked provenance

Status: Superseded in binary-build provenance scope by
`ADR_8D_HET_PM_V2_REPRODUCIBLE_RELEASE_PROVENANCE_20260718.md`. Canonicalizacja
Git SHA pozostaje obowiązująca, lecz opisany poniżej binary SHA został wycofany
przed uruchomieniem prospective validation.

## D1. Problem

Final focused re-review PR #73 wykazał, że `expected_runtime_commit_sha` w
locked criteria wskazywał 40-znakowy hex, który nie był istniejącym commitem
Git. Git rozwiązywał widoczny skrót `7e162a9` do:

```text
7e162a90846a0425cc3ed01f90bcf2fb52d39c71
```

Natomiast criteria zawierały:

```text
7e162a9d1ebd8e62f0079a21897cbd3bfec84057
```

Promotion evaluator porównuje `launcher_proof.git_commit_sha` z criteria przez
exact equality, więc każdy prawidłowy prospective validation run z rzeczywistego
commita zostałby odrzucony jako `validation runtime contract mismatch`.

## D2. Zakres decyzji

Decyzja dotyczy wyłącznie sposobu materializacji locked criteria i ich
provenance validation.

Poza zakresem pozostają:

- runtime authority cutover;
- HET-PM V2 gate lattice;
- admission writer;
- progi ekonomiczne i per-run stability;
- traktowanie obecnych diagnostic runów jako final validation evidence.

## D3. Root cause

`lock-criteria` weryfikował dotąd tylko kształt wejściowego
`--runtime-commit-sha`: 40 znaków hex i brak placeholdera z samych zer. Nie
pytał Git, czy taki obiekt istnieje, czy jest commitem, ani czy należy do
aktualnej historii PR.

To pozwoliło zablokować syntaktycznie poprawny, ale niewykonalny contract.

## D4. Decyzja

`lock-criteria` kanonizuje teraz runtime commit przez Git:

```text
git rev-parse --verify <input>^{commit}
```

Narzędzie akceptuje pełny SHA albo jednoznaczny skrót hex, zapisuje wyłącznie
pełny 40-znakowy SHA i odrzuca input, który nie rozwiązuje się do commita.

Dodatkowo `lock-criteria` wymaga:

```text
git merge-base --is-ancestor <canonical_commit> HEAD
```

Dzięki temu locked runtime commit może być wcześniejszym reviewed runtime
commitem, ale musi należeć do historii aktualnego PR head.

`validate_criteria()` rozróżnia teraz `expected_runtime_commit_sha` od pozostałych
SHA-256 digestów i dla locked criteria wymaga dokładnie 40 znaków.

## D5. Konsekwencje

- Locked criteria są wykonalne dla prospective launcher proof.
- Nie można już zablokować nieistniejącego runtime commita.
- Skrót commita nie trafia do criteria; zapisywany jest pełny canonical SHA.
- Commit spoza historii aktualnego PR head jest odrzucany podczas lockowania.
- Kryteria zostały ponownie zmaterializowane z:

```text
expected_runtime_commit_sha = 7e162a90846a0425cc3ed01f90bcf2fb52d39c71
expected_release_binary_sha256 = 8f38ee7879f4c8ce58b43c3757b4fe1cd09d4b398a07e56d99165c690e6a3804
expected_promotion_tool_hash = 65ed1ba00bb3b84c6968a4e309e46f5a5d1caf1f5ddeac48f7cf941159c34773
expected_pr_a_analyzer_hash = 5e26d19b10ea2613f0da9c4e532d60cc9a646ee000289befdd98f4f6f9e1faa0
```

## D6. Implementacja

Zmiany obejmują:

- `scripts/het_pm_v2_promotion_gate_v1.py`;
- `scripts/test_het_pm_v2_promotion_gate_v1.py`;
- `PLANS/DO_REALIZACJI/HET_PM_V2_PROMOTION_CRITERIA_V1.json`;
- ADR materialization note z poprzedniego locka.

## D7. Weryfikacja

Wymagane lokalne dowody:

```text
python3 scripts/test_het_pm_v2_promotion_gate_v1.py
python3 scripts/test_het_pm_v2_analysis.py
python3 -m py_compile scripts/het_pm_v2_promotion_gate_v1.py scripts/het_pm_v2_analysis.py
python3 scripts/guard_diff_scoped_clippy.py --base 29e3cfd72b86de0d77bb0a58547cb9b3824fe05e --head HEAD
git diff --check
```

Nowe testy kontraktowe:

- `test_criteria_lock_rejects_nonexistent_runtime_commit`;
- `test_criteria_lock_canonicalizes_short_commit_sha`;
- `test_locked_runtime_commit_must_be_ancestor_of_pr_head`.

## D8. Ryzyka i następne kroki

Ta decyzja nie promuje HET-PM V2 do authority. Następnym krokiem pozostają dwa
prospective validation runy uruchomione dopiero po zaakceptowaniu i
zamrożeniu tego contractu.
