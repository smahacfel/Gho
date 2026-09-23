# ADR-8D: Pump Research Tape — akceptacja GO-D i przygotowanie qualification

**Data:** 2026-08-16

**Status:** IMPLEMENTED / LOCAL PREPARATION ONLY / PROVIDER I/O HOLD

**Task:** `PUMP_RESEARCH_TAPE_GO_D_ACCEPTANCE_AND_QUALIFICATION_PREPARATION`

## D0. Problem

GO-D `pump-research-1786909252793-3799414` zakończył się poprawnym immutable
raw tape, lecz ad-hoc wrapper utracił zewnętrzny wait status po odłączeniu
supervisora. Immutable lifecycle, footery, pełne accounting, ProgramData i
binding v5 pozostały poprawne. Plan ani materializer nie traktują zewnętrznego
wait statusu jako wejścia qualification eligibility, więc powtarzanie capture
nie byłoby uzasadnione.

Przed independent qualification pozostały trzy lokalne zadania:

- przyszły capture supervisor nie może odkrywać childa przez skrócone Linux
  `comm` ani tracić wait statusu;
- CLI `certify` nie może zawsze logować outputu jako unqualified, gdy manifest
  może być `Ready` albo typed `Blocked`;
- protected independent-audit config musi istnieć poza worktree i zostać
  zweryfikowany bez wykonywania provider I/O.

## D1. Decyzja: GO-D pozostaje immutable eligibility input

GO-D jest zaakceptowany bez mutacji raw bytes. `UNKNOWN` operator wait status
jest zachowany jako `OperatorSupervisionEvidenceIncomplete`, ale nie nadpisuje
wewnętrznego `Complete`, clean shutdownu, frozen segment proof ani provenance
v5. Nie wykonujemy replacement capture tylko dla tego pola.

Independent source-completeness audit pozostaje jedynym źródłem promocji do
`Ready`. Zwykłe `certify` bez audytu jest nadal niedozwolone w operacyjnym
workflow, chociaż kod zachowuje je jako development-only materialization.

## D2. Decyzja: exact-child supervisor dla przyszłych capture'ów

Nowy research-only supervisor:

- uruchamia sealed binary bezpośrednio przez `subprocess.Popen`;
- zachowuje dokładny child PID, bez `pgrep` i bez GNU `timeout`;
- używa pidfd wyłącznie do obserwacji exit readiness;
- wysyła `SIGINT` dokładnemu childowi po duration, disk-floor albo sygnale
  operatora;
- po przekroczeniu bounded drain timeout wysyła `SIGKILL` i zachowuje signal;
- wykonuje jeden finalny `waitpid()`, zapisuje surowy status oraz realny exit
  code albo signal;
- rozdziela startowy próg wolnego miejsca od niższego runtime disk floor;
- konstruuje child environment przed `Popen`, usuwa z niego legacy aliasy
  `GHOST_SEER_GRPC_X_TOKEN` i `GHOST_RPC_AUTH_TOKEN`, a po spawn usuwa
  dedykowane credential variables ze środowiska supervisora;
- przejmuje wspólny, output-directory-scoped lock przed skanem procesów,
  snapshotem runów, utworzeniem operator directory i `Popen`, niezależnie od
  wybranego `--operator-dir`;
- uznaje operatorski sukces wyłącznie przy exit `0`, dokładnie jednym nowym
  runie, poprawnym `Complete` completion receipt, `clean_shutdown = true` i
  zerze ścieżek `*.partial`; każde naruszenie zapisuje typed failure i zwraca
  non-zero bez zmiany surowego wait statusu;
- zapisuje create-new launch/execution receipts i nigdy nie uruchamia dalszej
  fazy pipeline'u.

To narzędzie nie jest używane do ponowienia GO-D i nie zmienia aktywnego Seera.

## D3. Decyzja: status CLI i independent-audit config

`PumpResearchCertificationSummaryV1` zwraca faktyczny
`PumpResearchTapeQualificationStatusV1`. CLI zapisuje go jako structured field
oraz neutralny completion message. `Ready` nie może zostać opisane jako
„explicitly unqualified”. Exact manifest pozostaje SSOT.

Protected config:

```text
/protected/operator/pump-research-audit-v1.toml
```

ma tryb `0600`, znajduje się poza worktree, nie zawiera tokenu i wskazuje
root-only HTTPS no-auth candidate niezależny od `nln-primary-yellowstone`.
Concurrency jest ograniczone do jednego requestu. Samo przygotowanie configu
nie wykonuje RPC i nie stanowi GO na qualification. Dostępność, retention i
capacity źródła zostaną ocenione dopiero w osobnej, jawnie zatwierdzonej fazie
provider I/O; failure pozostaje typed i fail-closed.

Local preparation jest utrwalone w create-new operator snapshot:

```text
datasets/pump-research/operator-logs/
  go-e-qualification-prep-v1-20260816T222710Z/
  qualification_preparation_receipt_v1.json
```

Snapshot wiąże control hashe zaakceptowanego raw, SHA-256 protected configu,
różne identyfikatory providerów, bounded request policy, brak exact outputu i
jawny stan `HOLD_PROVIDER_IO_AND_CERTIFY`. Nie zawiera endpointu ani wartości
credentialu. Plik pozostaje owner-writable i nie jest mechanicznie immutable
ani sealed. Przed przyszłym provider I/O wymagane jest ponowne sprawdzenie
oczekiwanego SHA-256 snapshotu
`eab36576a3ad3284fe73da186186f04301a6b5a0809b2e592cf72ca3c7dd0787`
oraz SHA-256 configu
`c5e1ebb6585639ebe33c70308a838e102d00aa5f45a46012b581e0cb56d9ca16`
i związanie obu wartości w nowym operator execution receipt.

## D4. Wpływ, testy i rollback

Zmiana jest research-only i addytywna. Nie modyfikuje frozen raw V1,
Yellowstone subscription, parsera, active `connect_geyser()`, `SeerConfig`,
Event Busa, AccountStateCore, Gatekeepera, MFS, execution ani historycznych
dataset bytes.

Weryfikacja obejmuje:

- clean exact-child SIGINT i realny exit `0`;
- early non-zero child exit bez imputacji;
- bounded drain timeout i zachowany `SIGKILL` signal;
- surowy status `waitpid` zgodny ze znormalizowanym exit code albo signal;
- disk-floor shutdown bez process-name lookup;
- dokładnie jeden finalny wait;
- usunięcie obu legacy credential aliases z child environment przed `Popen`;
- zachowanie dedykowanego credentialu wyłącznie w capture childzie;
- dwa równoległe supervisory z różnymi `operator-dir` i wspólnym `output_dir`
  dopuszczają dokładnie jeden `Popen`;
- fail-closed exit przy zerze lub wielu nowych runach, niekompletnym receipt,
  braku clean shutdownu albo obecności `*.partial`;
- status labels `unqualified`, `ready` i `blocked`;
- local-only parsing bez-tokenowego root-only HTTPS audit configu;
- odrzucenie URL z path, userinfo albo query credentialem;
- targeted Rust/Python tests, frozen contracts, parser parity i whitespace.

Rollback oznacza niewykonywanie supervisora ani qualification. Nie cofamy
akceptacji immutable GO-D i nie przywracamy mylącego stałego logu. Każdy exact
status inny niż `Ready` zatrzymuje dalszą promocję.
