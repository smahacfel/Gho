  FSC Coverage
  Sprawdzałem oba decision logi (v2.2/legacy_live i v2.5/v25_shadow). Są prawie identyczne. Dla v2.5/v25_shadow:

  - rows: 7660 w momencie pełnego FSC parse
  - funding_source_v2.status = degraded: 6460 czyli 84.33%
  - funding_source_v2.status = unavailable: 1077 czyli 14.06%
  - funding_source_v2.status = clean: 123 czyli 1.61%

  shadow_fsc_v2_reason_if_enabled:

  - FSC_V2_SHADOW_NOT_CLEAN:Degraded: 6460
  - FSC_V2_SHADOW_NOT_CLEAN:Unavailable: 1077
  - FSC_V2_SHADOW_BELOW_THRESHOLD: 81
  - FSC_V2_SHADOW_HIGH_FSC: 26
  - FSC_V2_SHADOW_COVERAGE_WINDOW_UNAVAILABLE: 16

  shadow_fsc_v2_policy_signal:

  - False: 7628
  - True: 26

  Czyli FSC jest emitowane i materializowane, ale jako użyteczny clean signal ma bardzo niskie pokrycie.

  FSC Drop / Degradation Reasons
  Główne excluded_reason:

  - insufficient_non_neutral_support: 6115
  - no_buyer_cohort: 1077
  - low_attribution_confidence: 175
  - low_coverage: 149
  - None: 123 czyli clean rows
  - same_slot_ordering_unavailable: 21

  Miss classes:

  - indeterminate: 22512
  - structural: 1045
  - operational: 715

  Miss reasons:

  - FSC_NO_RETAINED_RECIPIENT_HISTORY: 22138
  - FSC_RELATIVE_FUNDING_TOO_SMALL: 776
  - FSC_GLOBAL_RECIPIENT_EVICTED: 715
  - FSC_ABS_ATTRIBUTION_TOO_SMALL: 263
  - FSC_LOW_ATTRIBUTION_CONFIDENCE: 256
  - FSC_SAME_SLOT_ORDERING_UNAVAILABLE: 118
  - FSC_NO_PREBUY_TRANSFER_IN_WINDOW: 6

  Najważniejsza interpretacja: drop FSC nie wygląda jak brak działającego streamu. Wygląda jak problem coverage/attribution: dla
  większości buyerów nie ma retained recipient history albo nie ma wystarczającego non-neutral support, więc status idzie w degraded
  zamiast clean.

  FSC Technical Health
  Z decision rows:

  - source_topics: ghost.funding_transfers
  - funding_lane_lag_slots:
      - <=10: 6170
      - <=50: 413

  To jest dobry znak techniczny: lane jest blisko decyzyjnego slotu. Gdyby funding lane był martwy, spodziewałbym się dominującego
  unavailable, dużego laga albo braku source_topics.

  Known source / unknown buyer:

  - known_source_count=0: 4883
  - known_source_count=1: 2452
  - known_source_count=2: 182
  - unknown_buyer_count=0: 1560
  - unknown_buyer_count=1: 2007
  - unknown_buyer_count=2: 1600
  - unknown_buyer_count=3: 881

  Known coverage buckets:

  - 0: 4888
  - <0.25: 568
  - <0.5: 990
  - <0.75: 707
  - <1.0: 23
  - 1.0: 484

  Tu widać rdzeń problemu: bardzo dużo rowów ma known_coverage=0 i known_non_neutral_buyers=0.

  Parametry FSC, z którymi odpalono R28
  Runtime config:

  - config:
    /root/Gho/configs/rollout/shadow-burnin-v3-r28-all-decision-counterfactual-30-30-maxwait4000.toml

  - brain:
    /root/Gho/configs/rollout/ghost_brain_selector_dataset_sampler_r28_maxwait4000.toml

  Seer / ingest:

  [seer]
  source_mode = "grpc"
  stream_mode = "single_global"
  funding_lane_mode = "full_chain"
  commitment = "processed"
  tx_filter_strategy = "per_pool"

  Program streams:

  [seer.program_streams]
  enabled = true
  max_streams = 2
  quota_policy = "fail_fast"
  enabled_topics = [
    "solana.pump_fun.buy",
    "solana.pump_fun.buy_exact_sol_in",
  ]

  Disabled stream topics in config:

  disabled_streams = [
    "prod.rpc.solana.pumpfun.trade",
    "prod.rpc.solana.system.transfers",
  ]

  FSC v2 section:

  [fsc_v2]
  capture_enabled = true
  feature_emit_enabled = true
  decision_enabled = false
  hard_reject_enabled = false
  provider = "nln_program_streams"
  snapshot_decision_time_enabled = true
  snapshot_eventual_enabled = true
  lookback_window_s = 1800
  warmup_window_s = 300
  min_abs_store_lamports = 1000000
  min_abs_attribution_lamports = 10000000
  min_rel_to_buy = 0.20
  min_attribution_confidence = 0.60
  min_total_buyers = 2
  min_known_non_neutral_buyers = 2
  min_known_coverage = 0.50
  min_non_neutral_known_coverage = 0.30
  same_slot_cross_signature_policy = "require_tx_index"
  include_wsol = false
  include_spl = false
  neutral_funder_set_path = "configs/fsc/neutral_funders_v1.toml"
  neutral_funder_set_version = "neutral_funders_v1"

  Gatekeeper/FSC policy-related params:

  max_funding_source_concentration = 0.99
  soft_penalty_high_fsc = 0
  soft_penalty_high_fsc_high_cpv_combo = 0
  max_sybil_soft_points = 255
  dev_unknown_max_sybil_soft_points = 255
  enable_sybil_interference_layer = false
  enable_sybil_combo_veto = false
  emit_sybil_meta_score = false
  require_ready_fsc_for_combo_veto = true
  funding_lookback_window_s = 180
  funding_dust_threshold_lamports = 1000000
  fsc_per_recipient_cap = 256
  fsc_global_recipient_cap = 50000

  Czyli R28 jest zgodny z założeniem: FSC capture/evidence only, nie aktywny hard reject ani scoring/tuning.

  Werdykt

  - R28 runtime: RUNNING
  - BUY shadow simulation: działa, terminal coverage około 89.2% względem entries
  - non-BUY counterfactual probe: działa, terminal coverage 84.3% względem selected i 100% względem skutecznie zasymulowanych
    transportów

  - FSC lane: technicznie działa, source ghost.funding_transfers, lag zwykle niski
  - FSC usable/clean coverage: słabe, ~1.6% clean
  - główny blocker FSC: brak retained recipient history i insufficient non-neutral support, nie brak samego lane
  - dysk: 33G wolne, trzeba obserwować, bo R28 szybko produkuje logi

dane z R28 run. 
