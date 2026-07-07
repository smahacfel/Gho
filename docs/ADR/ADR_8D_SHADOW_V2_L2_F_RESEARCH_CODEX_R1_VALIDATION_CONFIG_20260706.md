# ADR-8D: Shadow V2 L2-F Research Codex R1 Validation Config 20260706

## Status

Local research validation run configuration created.

## Decision

Utworzono osobny run-local profil research:

```text
run_id=shadow-v2-l2-f-research-codex-20260706-r1
launcher_config=configs/rollout/shadow-v2-l2-f-research-codex-20260706-r1.local.toml
ghost_brain_config=configs/rollout/ghost_brain_shadow_v2_l2_f_research_codex_20260706_r1.local.toml
base_profile=shadow-v2-l2-f-collection-20260705-r16
```

Profil dziedziczy ustawienia ostatniego runa R16, poza jawnie wskazanymi
wartosciami progow:

```text
min_tx_count=4
min_unique_signers=3
min_buy_count=3
max_wait_time_ms=11111
```

## Context

Operator zlecil nowy research validation run na profilu podobnym do ostatniego
L2-F collection scope, ale z mniej agresywnie poluzowanym Phase 1 quantity gate
oraz dluzszym oknem obserwacji.

## Scope

In scope:

- nowy lokalny namespace runa;
- nowy launcher config;
- nowy Ghost Brain config;
- odrebne sciezki logow, datasets i Shadow V2 raw artefaktow.

Out of scope:

- zmiany kodu runtime;
- zmiany Gatekeeper policy;
- zmiany BUY/REJECT logic;
- zmiany selector runtime;
- zmiany TX/Jito/live path;
- zmiany provider streams;
- threshold tuning poza czterema wskazanymi wartosciami;
- runtime approval, live equivalence, shadow close lub active close.

## Safety

Profil pozostaje shadow-only:

```text
entry_mode=shadow_only
execution_mode=shadow
runtime_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
shadow_close_only=false
active_close=false
```

## Expected Verification

Przed pozostawieniem runa w tle nalezy potwierdzic:

1. launcher preflight PASS;
2. proces `ghost-launcher` zyje;
3. logi runtime sa zapisywane;
4. katalog `reports/selector/<run_id>` istnieje;
5. Shadow V2 artefakty zaczynaja byc emitowane albo runtime jest zdrowy w fazie
   oczekiwania na pierwsze pozycje.

## Final Decision

```text
run_local_validation_config_changes=YES
runtime_decision_behavior_changes=NONE
provider_stream_changes=NONE
runtime_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
shadow_close_only=false
active_close=false
```
