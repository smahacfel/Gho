# OGÓLNE:

1. ILOŚĆ ZAJĘTEGO MIEJSCA       df -h             
2.  WYŚWIETL ZAWARTOŚĆ FOLDERU: du -sh */ | sort -rh  
3.  ZUŻYCIE PAMIĘCI RAM:        free -m           
4.  USUŃ FOLDER                 rm -rf nazwa      
5.  MENADŻER ZADAŃ              htop              
6.  LISTA PROCESÓW              ps aux            
7.  ZAKOŃCZ PROCES PO NR PID:   kill -9 PID 

# RUNY UŻYTKOWE PROCESÓW :

> SHADOW BURNIN COLLECTOR: tmux new-session -d -s shadow_burnin_collector -c /root/Gho 'cargo run --release -p ghost-launcher --bin ghost-launcher -- --config /root/Gho/configs/rollout/shadow-burnin.toml'
> SHADOW BURNIN SELECTOR:  tmux new-session -d -s shadow_burnin_selector_phase0_20260529 -c /root/Gho 'KEEP_OPEN=1 scripts/run_selector_phase0_validation.sh --scope selector-phase0-shadow-burnin-v3-p1-20260529-restart --events logs/rollout/shadow-burnin-v3-p1/decisions/seer_runtime_coverage_audit.jsonl --decisions logs/rollout/shadow-burnin-v3-p1/decisions/shadow-burnin-v3-p1/v2.2/legacy_live/3a3c9f35b46593cf15e1553e1fe9a498434eb57adbd452a3da047c003d1cd17a/gatekeeper_v2_decisions.jsonl --lifecycle-report logs/shadow_run/shadow-burnin-v3-p1/shadow_onchain_lifecycle_report_all.jsonl --config-snapshot configs/rollout/shadow-burnin.toml --config-snapshot configs/rollout/ghost_brain_v3_p37_l1_standard_softpdd.toml --config-snapshot config.toml --config-snapshot ghost-brain/ghost_brain_config.toml --pnl-target-net-pct 40 --target-net-pct 40 --stop-net-pct 40 --horizon-ms 60000 --replay-artifact-version shadow-burnin-v3-p1-current-local-restart'
> DRY RUN:                  tmux new-session -d -s bot_B -c /root/Gho 'cargo run --release --bin ghost-launcher'
> LISTA SESJI:              tmux ls
> ZAKOŃCZ COLLECTOR:        tmux kill-session -t shadow_burnin_collector
> ZAKOŃCZ SELECTOR:         tmux kill-session -t shadow_burnin_selector_phase0_20260529
> OTWÓRZ COLLECTOR:         tmux attach -t shadow_burnin_collector
> OTWÓRZ SELECTOR:          tmux attach -t shadow_burnin_selector_phase0_20260529


# RUST:
> ZOPTYMALIZOWANY BUILD:          cargo build --release -j 4
> STANDARDOWY RUN:                cargo run --release --bin ghost-launcher
> KOMPLETNE TESTY:                cargo test --release
> PREFLIGHT: mkdir -p /root/Gho/.ghost && cargo test --workspace --no-run && printf '%s\n' "$(git -C /root/Gho rev-parse HEAD)" > /root/Gho/.ghost/baseline_accepted_revision
./scripts/ghost_production_preflight.sh --config /root/Gho/configs/rollout/paper-burnin.toml
> RUN TRYBU SHADOW BURNIN COLLECTOR: cargo run --release -p ghost-launcher --bin ghost-launcher -- --config /root/Gho/configs/rollout/shadow-burnin.toml
> RUN TRYBU SHADOW BURNIN SELECTOR:  KEEP_OPEN=0 scripts/run_selector_phase0_validation.sh --scope selector-phase0-shadow-burnin-v3-p1-20260529-restart --events logs/rollout/shadow-burnin-v3-p1/decisions/seer_runtime_coverage_audit.jsonl --decisions logs/rollout/shadow-burnin-v3-p1/decisions/shadow-burnin-v3-p1/v2.2/legacy_live/3a3c9f35b46593cf15e1553e1fe9a498434eb57adbd452a3da047c003d1cd17a/gatekeeper_v2_decisions.jsonl --lifecycle-report logs/shadow_run/shadow-burnin-v3-p1/shadow_onchain_lifecycle_report_all.jsonl --config-snapshot configs/rollout/shadow-burnin.toml --config-snapshot configs/rollout/ghost_brain_v3_p37_l1_standard_softpdd.toml --config-snapshot config.toml --config-snapshot ghost-brain/ghost_brain_config.toml --pnl-target-net-pct 40 --target-net-pct 40 --stop-net-pct 40 --horizon-ms 60000 --replay-artifact-version shadow-burnin-v3-p1-current-local-restart

# LOGI SHADOW BURNIN:
> tmux capture-pane -pt shadow_burnin_collector -S -120
> tmux capture-pane -pt shadow_burnin_selector_phase0_20260529 -S -120
> tail -f /root/Gho/logs/rollout/shadow-burnin-v3-p1/system.log.$(date +%Y-%m-%d)
> tail -f /root/Gho/logs/rollout/shadow-burnin-v3-p1/oracle.log.$(date +%Y-%m-%d)
> tail -f /root/Gho/logs/shadow_run/shadow-burnin-v3-p1-buys.jsonl
> tail -f /root/Gho/logs/shadow_run/shadow-burnin-v3-p1/shadow_entries.jsonl
> tail -f /root/Gho/logs/shadow_run/shadow-burnin-v3-p1/shadow_lifecycle.jsonl
> tail -f /root/Gho/reports/selector/selector-phase0-shadow-burnin-v3-p1-20260529-restart/phase0_validation_run.log
> ls -lah /root/Gho/datasets/events/shadow-burnin-v3-p1
> ls -lah /root/Gho/datasets/selector/selector-phase0-shadow-burnin-v3-p1-20260529-restart
> ls -lah /root/Gho/reports/selector/selector-phase0-shadow-burnin-v3-p1-20260529-restart
> find /root/Gho/logs/rollout/shadow-burnin-v3-p1/decisions/shadow-burnin-v3-p1 -path '*gatekeeper_v2_decisions.jsonl' -printf '%T@ %p\n' | sort -nr | head -5
> find /root/Gho/logs/rollout/shadow-burnin-v3-p1/decisions/shadow-burnin-v3-p1 -path '*gatekeeper_v2_buys.jsonl' -printf '%T@ %p\n' | sort -nr | head -5
> nano /root/Gho/logs/rollout/shadow-burnin-v3-p1/decisions/seer_runtime_coverage_audit.jsonl

# ANALIZA:

> cd decisions.jsonl
> python3 /root/gh/tools/gatekeeper_analyzer.py /root/gh/logs/decisions.jsonl/gatekeeper_v2_buys.jsonl      Ogólna analiza statystyczna decyzji gatekeepera                                                                
> python3 fetch_pool_tx_counts.py                                                                           Wygeneruj listę decyzji z total_tx dla każdego rekordu
> python3 sort_pool_tx.py                                                                       
> python3 /root/gh/tools/analiza_porownawcza.py                                                             Porównaj zbior A ze zbiorem B
