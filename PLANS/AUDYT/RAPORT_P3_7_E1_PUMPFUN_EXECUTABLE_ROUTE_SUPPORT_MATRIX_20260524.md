# P3.7-E1 Pump.fun Executable Route Support Matrix

Generated at: `2026-05-24T18:25:24.073442+00:00`

## Decision

- final_decision: `GO_E2_IMPLEMENT_TOP_ROUTE_SUPPORT`
- recommended_next_path: `implement_legacy_buy_executable_account_set_materialization`
- recommended_next_route_to_implement: `legacy_buy_executable_account_set_materialization`
- recommendation_reason: `legacy_buy is the attempted fallback and the only observed route-compatible tx-meta source, but current fallback evidence lacks a complete executable legacy account set: active rows miss the core bonding_curve source and probe rows still depend on the primary BCV2 route account set`

## Inputs

- shadow_root: `/root/Gho-r17-clean/logs/shadow_run/shadow-burnin-v3-p37-r17-replay-ready-diagnostic`
- decision_root: `/root/Gho-r17-clean/logs/rollout/shadow-burnin-v3-p37-r17-replay-ready-diagnostic/decisions`
- runtime: not run; offline artifact audit only

Artifact rows:

```json
{
  "active_shadow_entry": 1,
  "active_shadow_lifecycle": 1,
  "active_shadow_transport": 1,
  "probe_entry": 0,
  "probe_lifecycle": 0,
  "probe_selection": 4,
  "probe_skip": 44,
  "probe_transport": 0,
  "seer_runtime_coverage": 44
}
```

## Required E1 Counters

```json
{
  "builder_supported_route_counts": {
    "legacy_buy": 5,
    "routed_exact_sol_in": 5
  },
  "executable_route_count": 0,
  "observed_route_variant_counts": {
    "legacy_buy": 5,
    "routed_exact_sol_in": 5
  },
  "recommended_next_route_to_implement": "legacy_buy_executable_account_set_materialization",
  "route_account_role_map_coverage": {
    "legacy_buy": 1,
    "routed_exact_sol_in": 14
  },
  "route_classes_excluded_from_l2": [
    "legacy_buy",
    "routed_exact_sol_in"
  ],
  "route_variant_total": 2,
  "rpc_load_readiness_by_route": {
    "legacy_buy": {
      "rate": 0.0,
      "ready_false": 10,
      "ready_true": 0
    },
    "routed_exact_sol_in": {
      "rate": 0.0,
      "ready_false": 7,
      "ready_true": 0
    }
  },
  "simulation_support_by_route": {
    "legacy_buy": "not_executable_no_executable_route_account_set",
    "routed_exact_sol_in": "not_executable_no_executable_route_account_set"
  },
  "unsupported_route_counts": {
    "legacy_buy": 5,
    "routed_exact_sol_in": 5
  }
}
```

## Route Matrix

| route_variant | observed_count | builder_support | prepared_request_support | rpc_ready_true | rpc_ready_false | simulation_support | primary_failure |
| --- | --- | --- | --- | --- | --- | --- | --- |
| legacy_buy | 5 | supported_by_current_builder | fallback_account_set_available | 0 | 10 | not_executable_no_executable_route_account_set | fallback_route_requires_same_bcv2_simulation_load_account |
| routed_exact_sol_in | 5 | supported_by_current_builder | prepared_request_manifest_available | 0 | 7 | not_executable_no_executable_route_account_set | bonding_curve_v2_identity_authoritative_but_not_load_ready |

## Account Role / Source Coverage

| route_variant | role_map_coverage | top_missing_roles | top_account_sources |
| --- | --- | --- | --- |
| legacy_buy | 1 | bonding_curve:3, bonding_curve_v2:7, payer_pubkey:3, user_ata:3, user_volume_accumulator:3 | account_overrides:18, legacy_buy_curve:3, materialized_feature_set:6, observed_tx_account_meta:9, payer:6, primary_route_account_set:4, route_builder:57, system_program:6 |
| routed_exact_sol_in | 14 | - | account_overrides:6, materialized_feature_set:2, observed_tx_account_meta:2, payer:2, route_builder:18, system_program:2, token_program:2, unknown:6 |

## Route Details

```json
{
  "legacy_buy": {
    "account_count_values": {
      "21": 12
    },
    "account_index_to_role_map": {
      "16": {
        "bonding_curve_v2:observed_tx_account_meta": 3
      }
    },
    "account_index_to_role_map_coverage": 1,
    "account_role_counts": {
      "associated_bonding_curve": 6,
      "bonding_curve": 6,
      "bonding_curve_v2": 6,
      "buyback_fee_recipient": 6,
      "creator_vault": 6,
      "event_authority": 6,
      "fee_config": 6,
      "fee_program": 6,
      "fee_recipient": 6,
      "global_config": 6,
      "global_volume_accumulator": 6,
      "mint": 6,
      "payer_pubkey": 3,
      "pump_program": 6,
      "system_program": 6,
      "token_program": 6,
      "transaction_account": 18,
      "user_ata": 6,
      "user_volume_accumulator": 6
    },
    "account_source_counts": {
      "account_overrides": 18,
      "legacy_buy_curve": 3,
      "materialized_feature_set": 6,
      "observed_tx_account_meta": 9,
      "payer": 6,
      "primary_route_account_set": 4,
      "route_builder": 57,
      "system_program": 6,
      "token_program": 6,
      "unknown": 18,
      "user_ata": 9
    },
    "builder_support_signals": {
      "fallback_route_attempted": 7,
      "fallback_route_candidate": 7,
      "observed_tx_account_meta_identity": 3
    },
    "builder_support_status": "supported_by_current_builder",
    "creatable_accounts": [
      "user_ata:G8zFDUm8RTckUpnVVd4PecEp3HEHQcZTFsYhMThyfTBD:user_ata",
      "user_volume_accumulator:2wpKCaSXBc1oY5uqiccSKfLhWLrqr7cgHDBZ41Zbh1C9:route_builder"
    ],
    "evidence_row_count": 10,
    "failure_class_counts": {
      "fallback_builder_account_source_unverified": 4,
      "fallback_missing_core_curve_account": 3,
      "fallback_route_missing_legacy_buy_curve": 3,
      "fallback_route_requires_same_bcv2_simulation_load_account": 4,
      "missing_on_rpc_precheck": 3
    },
    "instruction_account_positions": [
      "16"
    ],
    "instruction_discriminator_counts": {
      "66063d1201daebea": 3
    },
    "loaded_address_usage": {
      "resolved_transaction_account_keys": 3
    },
    "message_account_indices": [
      "16"
    ],
    "missing_pubkey_counts": {
      "2wpKCaSXBc1oY5uqiccSKfLhWLrqr7cgHDBZ41Zbh1C9": 3,
      "3bwRyhJ3HFUqH6wHpXVZYDPgwe47zct2m58XBgqbiEeB": 1,
      "8fPC9ux53kBYkWieTS9xChBceyYA6fP7V1krFqK5owB4": 1,
      "AKG7xGcHnh3ny45mYq5iSfewrkieFfko9en9ZkbyKpxp": 3,
      "DWE7sN5yEmRc89rmYCKBF8FQ6txjEu1UBQ45BpaZXjpu": 1,
      "FHPManqKgjyE8vchDU316rRgfc4DCUwSW78kebpj1vpm": 3,
      "G8zFDUm8RTckUpnVVd4PecEp3HEHQcZTFsYhMThyfTBD": 3,
      "HfZRdRvfHPZnezqjz9Y7kpxBNEgfN5ctR4qbpNahGwem": 1
    },
    "missing_role_counts": {
      "bonding_curve": 3,
      "bonding_curve_v2": 7,
      "payer_pubkey": 3,
      "user_ata": 3,
      "user_volume_accumulator": 3
    },
    "observation_kind_counts": {
      "fallback_route_candidate": 7,
      "observed_tx_account_meta": 3
    },
    "observed_count": 5,
    "prepared_request_support_signals": {
      "fallback_required_precheck_account_set": 3,
      "fallback_simulation_load_account_set": 3
    },
    "prepared_request_support_status": "fallback_account_set_available",
    "primary_failure_class": "fallback_route_requires_same_bcv2_simulation_load_account",
    "program_id_counts": {
      "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P": 3
    },
    "readiness_reason_counts": {
      "fallback_route_missing_legacy_buy_curve": 3,
      "fallback_route_requires_same_bcv2_simulation_load_account": 4,
      "missing_on_rpc_precheck": 3,
      "observed_bcv2_provenance:route_compatible": 3
    },
    "required_precheck_accounts": [
      "associated_bonding_curve:ErV6MS3ohGFEbHMubr2iNwYTAbh7tRUuUtUCYRchMnMa:account_overrides",
      "bonding_curve:Hi6En3SEEQqKE2PA7zNrm4TdHyc9XutFRERuPFWtP8Qz:route_builder",
      "bonding_curve_v2:FHPManqKgjyE8vchDU316rRgfc4DCUwSW78kebpj1vpm:observed_tx_account_meta",
      "buyback_fee_recipient:9M4giFFMxmFGXtc3feFzRai56WbBqehoSeRE5GK7gf7:route_builder",
      "creator_vault:AhJsMzAmqbtC8MpwhSvEqKpzWwERcM5xeF7kKg2gkAaH:route_builder",
      "event_authority:Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1:route_builder",
      "fee_config:8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt:route_builder",
      "fee_program:pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ:route_builder",
      "fee_recipient:7VtfL8fvgNfhz17qKRMjzQEXgbdpnHHHQRh54R9jP2RJ:account_overrides",
      "global_config:4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf:account_overrides",
      "global_volume_accumulator:Hq2wp8uJ9jCPsYgNHex8RtqdvMPfVGoYwjvF1ATiwn2Y:route_builder",
      "mint:DDKNDABgaGyyg9ChCDzzkhspCYNkRXebkD89xrqV43o:materialized_feature_set",
      "pump_program:6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P:route_builder",
      "system_program:11111111111111111111111111111111:system_program",
      "token_program:TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb:token_program",
      "transaction_account:4vieeGHPYPG2MmyPRcYjdiDmmhN3ww7hsFNap8pVN3Ey:unknown",
      "transaction_account:ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL:unknown",
      "transaction_account:ComputeBudget111111111111111111111111111111:unknown"
    ],
    "required_simulation_load_accounts": [
      "associated_bonding_curve:ErV6MS3ohGFEbHMubr2iNwYTAbh7tRUuUtUCYRchMnMa:account_overrides",
      "bonding_curve:Hi6En3SEEQqKE2PA7zNrm4TdHyc9XutFRERuPFWtP8Qz:route_builder",
      "bonding_curve_v2:FHPManqKgjyE8vchDU316rRgfc4DCUwSW78kebpj1vpm:observed_tx_account_meta",
      "buyback_fee_recipient:9M4giFFMxmFGXtc3feFzRai56WbBqehoSeRE5GK7gf7:route_builder",
      "creator_vault:AhJsMzAmqbtC8MpwhSvEqKpzWwERcM5xeF7kKg2gkAaH:route_builder",
      "event_authority:Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1:route_builder",
      "fee_config:8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt:route_builder",
      "fee_program:pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ:route_builder",
      "fee_recipient:7VtfL8fvgNfhz17qKRMjzQEXgbdpnHHHQRh54R9jP2RJ:account_overrides",
      "global_config:4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf:account_overrides",
      "global_volume_accumulator:Hq2wp8uJ9jCPsYgNHex8RtqdvMPfVGoYwjvF1ATiwn2Y:route_builder",
      "mint:DDKNDABgaGyyg9ChCDzzkhspCYNkRXebkD89xrqV43o:materialized_feature_set",
      "payer_pubkey:AKG7xGcHnh3ny45mYq5iSfewrkieFfko9en9ZkbyKpxp:payer",
      "pump_program:6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P:route_builder",
      "system_program:11111111111111111111111111111111:system_program",
      "token_program:TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb:token_program",
      "transaction_account:4vieeGHPYPG2MmyPRcYjdiDmmhN3ww7hsFNap8pVN3Ey:unknown",
      "transaction_account:ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL:unknown",
      "transaction_account:ComputeBudget111111111111111111111111111111:unknown",
      "user_ata:G8zFDUm8RTckUpnVVd4PecEp3HEHQcZTFsYhMThyfTBD:user_ata",
      "user_volume_accumulator:2wpKCaSXBc1oY5uqiccSKfLhWLrqr7cgHDBZ41Zbh1C9:route_builder"
    ],
    "route_variant": "legacy_buy",
    "rpc_load_readiness_rate": 0.0,
    "rpc_load_ready_false": 10,
    "rpc_load_ready_true": 0,
    "shadow_simulation_support_counts": {
      "route_not_ready": 10
    },
    "shadow_simulation_support_status": "not_executable_no_executable_route_account_set",
    "source_plane_counts": {
      "active_shadow_entry": 2,
      "active_shadow_lifecycle": 2,
      "active_shadow_transport": 2,
      "probe_skip": 4
    }
  },
  "routed_exact_sol_in": {
    "account_count_values": {
      "21": 8
    },
    "account_index_to_role_map": {
      "0": {
        "global_config:account_overrides": 2,
        "payer_pubkey:payer": 2
      },
      "1": {
        "fee_recipient:account_overrides": 2,
        "transaction_account:unknown": 2,
        "user_ata:user_ata": 2
      },
      "10": {
        "event_authority:route_builder": 2
      },
      "11": {
        "pump_program:route_builder": 2
      },
      "12": {
        "global_volume_accumulator:route_builder": 2
      },
      "13": {
        "user_volume_accumulator:route_builder": 2
      },
      "14": {
        "fee_config:route_builder": 2
      },
      "15": {
        "fee_program:route_builder": 2
      },
      "16": {
        "bonding_curve_v2:observed_tx_account_meta": 2
      },
      "17": {
        "buyback_fee_recipient:route_builder": 2
      },
      "3": {
        "bonding_curve:route_builder": 2,
        "mint:materialized_feature_set": 2
      },
      "4": {
        "associated_bonding_curve:account_overrides": 2,
        "system_program:system_program": 2
      },
      "5": {
        "token_program:token_program": 2
      },
      "9": {
        "creator_vault:route_builder": 2
      }
    },
    "account_index_to_role_map_coverage": 14,
    "account_role_counts": {
      "associated_bonding_curve": 2,
      "bonding_curve": 2,
      "bonding_curve_v2": 2,
      "buyback_fee_recipient": 2,
      "creator_vault": 2,
      "event_authority": 2,
      "fee_config": 2,
      "fee_program": 2,
      "fee_recipient": 2,
      "global_config": 2,
      "global_volume_accumulator": 2,
      "mint": 2,
      "payer_pubkey": 2,
      "pump_program": 2,
      "system_program": 2,
      "token_program": 2,
      "transaction_account": 6,
      "user_ata": 2,
      "user_volume_accumulator": 2
    },
    "account_source_counts": {
      "account_overrides": 6,
      "materialized_feature_set": 2,
      "observed_tx_account_meta": 2,
      "payer": 2,
      "route_builder": 18,
      "system_program": 2,
      "token_program": 2,
      "unknown": 6,
      "user_ata": 2
    },
    "builder_support_signals": {
      "primary_route_candidate": 7
    },
    "builder_support_status": "supported_by_current_builder",
    "creatable_accounts": [],
    "evidence_row_count": 7,
    "failure_class_counts": {
      "bonding_curve_v2_identity_authoritative_but_not_load_ready": 8,
      "bonding_curve_v2_observed_meta_missing_on_rpc": 6
    },
    "instruction_account_positions": [
      "0",
      "1",
      "3",
      "4",
      "5",
      "9",
      "10",
      "11",
      "12",
      "13",
      "14",
      "15",
      "16",
      "17"
    ],
    "instruction_discriminator_counts": {},
    "loaded_address_usage": {},
    "message_account_indices": [],
    "missing_pubkey_counts": {},
    "missing_role_counts": {},
    "observation_kind_counts": {
      "primary_route_candidate": 7
    },
    "observed_count": 5,
    "prepared_request_support_signals": {
      "simulation_account_manifest": 2
    },
    "prepared_request_support_status": "prepared_request_manifest_available",
    "primary_failure_class": "bonding_curve_v2_identity_authoritative_but_not_load_ready",
    "program_id_counts": {},
    "readiness_reason_counts": {
      "bonding_curve_v2_identity_authoritative_but_not_load_ready": 4,
      "bonding_curve_v2_observed_meta_missing_on_rpc": 3
    },
    "required_precheck_accounts": [],
    "required_simulation_load_accounts": [
      "associated_bonding_curve:ErV6MS3ohGFEbHMubr2iNwYTAbh7tRUuUtUCYRchMnMa:account_overrides",
      "bonding_curve:Hi6En3SEEQqKE2PA7zNrm4TdHyc9XutFRERuPFWtP8Qz:route_builder",
      "bonding_curve_v2:FHPManqKgjyE8vchDU316rRgfc4DCUwSW78kebpj1vpm:observed_tx_account_meta",
      "buyback_fee_recipient:9M4giFFMxmFGXtc3feFzRai56WbBqehoSeRE5GK7gf7:route_builder",
      "creator_vault:AhJsMzAmqbtC8MpwhSvEqKpzWwERcM5xeF7kKg2gkAaH:route_builder",
      "event_authority:Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1:route_builder",
      "fee_config:8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt:route_builder",
      "fee_program:pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ:route_builder",
      "fee_recipient:7VtfL8fvgNfhz17qKRMjzQEXgbdpnHHHQRh54R9jP2RJ:account_overrides",
      "global_config:4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf:account_overrides",
      "global_volume_accumulator:Hq2wp8uJ9jCPsYgNHex8RtqdvMPfVGoYwjvF1ATiwn2Y:route_builder",
      "mint:DDKNDABgaGyyg9ChCDzzkhspCYNkRXebkD89xrqV43o:materialized_feature_set",
      "payer_pubkey:AKG7xGcHnh3ny45mYq5iSfewrkieFfko9en9ZkbyKpxp:payer",
      "pump_program:6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P:route_builder",
      "system_program:11111111111111111111111111111111:system_program",
      "token_program:TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb:token_program",
      "transaction_account:4vieeGHPYPG2MmyPRcYjdiDmmhN3ww7hsFNap8pVN3Ey:unknown",
      "transaction_account:ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL:unknown",
      "transaction_account:ComputeBudget111111111111111111111111111111:unknown",
      "user_ata:G8zFDUm8RTckUpnVVd4PecEp3HEHQcZTFsYhMThyfTBD:user_ata",
      "user_volume_accumulator:2wpKCaSXBc1oY5uqiccSKfLhWLrqr7cgHDBZ41Zbh1C9:route_builder"
    ],
    "route_variant": "routed_exact_sol_in",
    "rpc_load_readiness_rate": 0.0,
    "rpc_load_ready_false": 7,
    "rpc_load_ready_true": 0,
    "shadow_simulation_support_counts": {
      "primary_route_blocked": 7,
      "route_not_ready": 7
    },
    "shadow_simulation_support_status": "not_executable_no_executable_route_account_set",
    "source_plane_counts": {
      "active_shadow_entry": 1,
      "active_shadow_lifecycle": 1,
      "active_shadow_transport": 1,
      "probe_skip": 4
    }
  }
}
```

## Interpretation

R17A closed the replay-readiness side. E1 shows whether the current route
builder/resolver has an executable Pump.fun buy route universe. Non-executable
route classes remain excluded from L2/lifecycle denominator until a route has a
complete simulation-load account set and successful shadow/probe entry evidence.
