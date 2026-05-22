# RAPORT P3.7-L1R4 / R16-r5 — Bonding Curve V2 Precheck Contract Finding

Data: 2026-05-22
Zakres: `P3.7-L1R4 / J3N AccountNotFound Candidate Narrowing`
Namespace: `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r5-candidate-narrowing`

## Werdykt

`R16-r5` domknął diagnostykę `AccountNotFound` do konkretnej klasy błędu kontraktu:

```text
L1R4/J3N candidate narrowing: PASS
AccountNotFound blind error: FIXED
AccountNotFound narrowed attribution: PASS
Probe execution: BLOCKED
Current blocker: bonding_curve_v2 simulation-load account is excluded from precheck while still present in transaction account metas
L2 / ablation / collection / Phase B / P2 / live: HOLD / NO-GO
```

Najważniejszy fakt: to nie jest już problem payera, user ATA, user volume accumulator, hash continuity ani policy thresholds. Brakujące konto jest zawężone do `bonding_curve_v2` z routowanego `buy_exact_sol_in`.

## Stan Aktualny R16-r5

Artefakty runtime:

```text
probe_selection_rows = 48
probe_transport_rows = 15
probe_shadow_entries_rows = 15
probe_lifecycle_rows = 0

active_buy_rows = 9
active_shadow_entry_rows = 9
active_shadow_lifecycle_rows = 9
active_closed_lifecycle_rows = 0
onchain_lifecycle_rows = 0
shadow_lifecycle_labels_rows = 0
```

Replay i join:

```text
strict replay = full_replay_ok
v3_rows = 632
bad_rows = 0
exact decision/V3 join = PASS
probe chain join = PASS
```

Probe execution:

```text
probe_transport_rows = 15
execution_outcome = counterfactual_shadow_probe_simulation_error: 15
simulation_error_kind = AccountNotFound: 15
simulation_error_category = simulation_account_not_found_attributed: 15
simulation_error_account_role = bonding_curve_v2: 15
simulation_error_account_source = route_builder: 15
simulation_error_account_narrowing_status = exact_after_narrowing: 15
successful_probe_entry_rows = 0
simulation_error_entry_rows = 15
lifecycle_eligible_entry_rows = 0
```

Raw candidate set przed narrowing:

```text
payer_pubkey = 15
user_ata = 15
user_volume_accumulator = 15
bonding_curve_v2 = 15
```

Candidate set po narrowing:

```text
bonding_curve_v2 = 15
```

Wykluczenia non-fatal:

```text
ephemeral_payer_not_rpc_required = 15
idempotent_ata_create_attached = 15
route_user_volume_accumulator_not_precheck_required = 15
```

## Dowód z Artefaktów

Wszystkie `15/15` probe transport rows mają ten sam wzorzec:

```text
buy_variant = routed_exact_sol_in
execution_account_readiness_status = ready
account_set_match = true
simulation_error_account_role = bonding_curve_v2
simulation_error_account_source = route_builder
simulation_error_instruction_index = 3
simulation_error_account_index = 16
simulation_error_account_narrowing_status = exact_after_narrowing
```

Przykładowy row:

```text
pool_id = 6NVRT3H1QmSq5Rrzx1GAaNb1SvqrnCMq6HMThRTezqpD
base_mint = UTK3ZusLcJQX7HBNQWn6L55SgsDD3GnmFozWdFwpump
simulation_error_account_pubkey = C2PjgepAfaXuxGXxsztYdhEnCuPv43yCMf9dLoBQYTFx
account_manifest_summary = precheck_count=17;prepared_count=21;simulation_count=21;match=true;missing_candidates=4;lookup_error=none
```

Interpretacja:

- manifest lookup działał (`lookup_error=none`);
- szeroki set brakujących kont został zawężony;
- po wykluczeniu kont non-fatal jedynym fatal candidate jest `bonding_curve_v2`;
- `account_set_match=true` nie oznacza, że wszystkie account metas symulacji istnieją, tylko że required precheck subset zgadza się z aktualnym modelem required-set.

## Dowód z Kodu

`DirectBuyBuilder` zawsze dodaje `bonding_curve_v2` do account metas transakcji na indeksie `16`:

```rust
accounts.push(AccountMeta::new_readonly(bonding_curve_v2, false)); // 16
```

Źródło: `off-chain/components/trigger/src/direct_buy_builder.rs`

Równocześnie `TriggerComponent::counterfactual_probe_required_account_roles()` ma wyjątek, który usuwa ten sam account z required-account precheck:

```rust
if Self::counterfactual_probe_can_use_missing_bonding_curve_v2(request, &pubkey, &role) {
    return;
}
```

Wyjątek działa wtedy, gdy:

```rust
role == "bonding_curve_v2"
&& profile.buy_instruction.accounts.get(16).pubkey == pubkey
```

Źródło: `ghost-launcher/src/components/trigger/component.rs`

To tworzy niespójny kontrakt:

```text
precheck:
  bonding_curve_v2 missing -> allowed

prepared/simulation tx:
  bonding_curve_v2 remains in transaction account metas

RPC/simulation:
  AccountNotFound
```

## Klasyfikacja Problemu

Poprawna klasyfikacja:

```text
simulation_required_account_not_in_precheck
or
simulation_account_meta_missing_on_rpc
```

Niepoprawna klasyfikacja jako naprawiony/zaakceptowany stan:

```text
execution_account_readiness_status = ready
```

To konto może być optional z perspektywy historycznego precheck workaroundu, ale nie jest optional dla obecnej transakcji symulacyjnej, jeśli nadal znajduje się w account metas. Solana/RPC simulation musi załadować konta transakcji; brak account meta nie może być traktowany jako success.

## Decyzja Operacyjna

Nie przechodzić do:

```text
L2 ablation
threshold tuning
bounded collection
Phase B
P2/live
```

Następny etap powinien być wąski:

```text
P3.7-L1R5 — BondingCurveV2 Simulation Account Contract Repair
```

Cel:

1. Jeżeli `bonding_curve_v2` pozostaje w prepared/simulation account metas, traktować go jako `simulation-load required`.
2. Jeżeli `bonding_curve_v2` nie istnieje na RPC, skipować probe przed `simulate_buy`:

```text
probe_skipped
probe_skip_reason = execution_account_not_ready
precheck_failure_reason = execution_account_not_ready:bonding_curve_v2:<pubkey>
```

3. Alternatywnie, jeśli route naprawdę potrafi działać bez tego konta, builder musi nie dodawać go do account metas. Nie wolno zostawiać go w tx i równocześnie omijać precheck.
4. Rozszerzyć audyt tak, żeby odróżniał:

```text
required_precheck_account_set
simulation_load_account_set
creatable_in_tx_account_set
```

5. `account_set_match=true` nie może maskować sytuacji, gdzie konto niewymagane przez precheck jest nadal wymagane przez transaction account loading.

## Acceptance dla Następnego Fixu

R16-r6 / kolejny smoke może iść dalej tylko jeśli:

```text
strict replay = full_replay_ok
exact decision/V3 join = 100%
AccountNotFound unattributed = 0
AccountNotFound narrowed to bonding_curve_v2 = 0
or bonding_curve_v2 rows are precheck-skipped as execution_account_not_ready before simulation
simulation_error_entry_rows are not lifecycle-eligible
active BUY / live / P2 untouched
```

Jeśli po fixie większość probe rows zostanie skipnięta jako `execution_account_not_ready:bonding_curve_v2`, to będzie poprawny diagnostycznie wynik. Wtedy decyzja przechodzi do route/account coverage albo policy universe, ale nie do collection.

## Pliki / Artefakty Użyte jako Dowód

Runtime artifacts:

```text
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r5-candidate-narrowing/probe_transport.jsonl
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r5-candidate-narrowing/probe_shadow_entries.jsonl
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r5-candidate-narrowing/probe_selection.jsonl
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r5-candidate-narrowing/shadow_lifecycle.jsonl
```

Code paths:

```text
ghost-launcher/src/components/trigger/component.rs
off-chain/components/trigger/src/direct_buy_builder.rs
ghost-launcher/src/oracle_runtime.rs
scripts/v3_p37_mfs_lifecycle_join_key_audit.py
```

## Finalny Wniosek

`L1R4/J3N` spełniło swój cel: `AccountNotFound` nie jest już ślepym błędem.

Obecny blocker jest konkretny:

```text
bonding_curve_v2 is excluded from required precheck
but remains in simulation transaction account metas
and therefore can still produce AccountNotFound
```

To jest bug kontraktu probe precheck/simulation request, nie problem policy ani progów.
