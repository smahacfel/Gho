# OGÓLNE:

1. ILOŚĆ ZAJĘTEGO MIEJSCA       df -h             
2.  WYŚWIETL ZAWARTOŚĆ FOLDERU: du -sh */ | sort -rh  
3.  ZUŻYCIE PAMIĘCI RAM:        free -m           
4.  USUŃ FOLDER                 rm -rf nazwa      
5.  MENADŻER ZADAŃ              htop              
6.  LISTA PROCESÓW              ps aux            
7.  ZAKOŃCZ PROCES PO NR PID:   kill -9 PID 

# RUNY UŻYTKOWE PROCESÓW : 

> SHADOW BURNIN:       tmux new -s bot -d -c /root/Gho "cargo run --release -p ghost-launcher --bin ghost-launcher -- --config /root/Gho/configs/rollout/shadow-burnin.toml"
> DRY RUN:             tmux new -s bot_B -d -c /root/Gho "cargo run --release --bin ghost-launcher"
> ZAKOŃCZ PROCES:      tmux kill-session -t bot
> OTWÓRZ OKNO PROCESU: tmux attach -t bot


# RUST:
> ZOPTYMALIZOWANY BUILD:          cargo build --release -j 4
> STANDARDOWY RUN:                cargo run --release --bin ghost-launcher
> KOMPLETNE TESTY:                cargo test --release
> PREFLIGHT: mkdir -p /root/Gho/.ghost && cargo test --workspace --no-run && printf '%s\n' "$(git -C /root/Gho rev-parse HEAD)" > /root/Gho/.ghost/baseline_accepted_revision
./scripts/ghost_production_preflight.sh --config /root/Gho/configs/rollout/paper-burnin.toml
> RUN TRYBU SHADOW BURNIN:        cargo run --release -p ghost-launcher --bin ghost-launcher -- --config /root/Gho/configs/rollout/shadow-burnin.toml

# LOGI SHADOW BURNIN:
> tail -f /root/Gho/logs/rollout/shadow-burnin/system.log.$(date +%Y-%m-%d)
> tail -f /root/Gho/logs/rollout/shadow-burnin/oracle.log.$(date +%Y-%m-%d)
> tail -f /root/Gho/logs/shadow_run/shadow-burnin-buys.jsonl
> ls -lah /root/Gho/datasets/events/shadow-burnin
> nano logs/rollout/shadow-burnin/decisions/gatekeeper_v2_buys.jsonl
> nano logs/rollout/shadow-burnin/decisions/gatekeeper_v2_decisions.jsonl

# ANALIZA:

> cd decisions.jsonl
> python3 /root/gh/tools/gatekeeper_analyzer.py /root/gh/logs/decisions.jsonl/gatekeeper_v2_buys.jsonl      Ogólna analiza statystyczna decyzji gatekeepera                                                                
> python3 fetch_pool_tx_counts.py                                                                           Wygeneruj listę decyzji z total_tx dla każdego rekordu
> python3 sort_pool_tx.py                                                                       
> python3 /root/gh/tools/analiza_porownawcza.py                                                             Porównaj zbior A ze zbiorem B
