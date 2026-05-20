# RAPORT P3.7-J3Q4 Simulation Instruction Error Analysis

## Verdict

```text
J3Q4 diagnostic propagation: IMPLEMENTED
error classification: PASS when program/log/account-role fields are present
rows predating Q4 fields: diagnostic-limited
small bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```

Rows without `simulation_error_program_id`, instruction account roles or log tail
are parsed but treated as diagnostic-limited, not fully understood.

## Summary

```text
transport_rows = 5
simulation_error_rows = 1
category_counts = {'simulation_slippage_or_price_mismatch': 1}
custom_code_counts = {'6002': 1}
program_counts = {'6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P': 1}
```

## Error Rows

### `2952383241d0c26e3eb4c66c650538ea5c3859d37f04f017a04c7cb3ecc33365`

```text
ab_record_id = D6r9AfXN3AmWUw99BxyWczuM5zgew9QtyStB5owL5BVG:1779315513954:1779315515954:TIMEOUT
pool_id = D6r9AfXN3AmWUw99BxyWczuM5zgew9QtyStB5owL5BVG
base_mint = 5BLZLfY6kzoKeSkFrQCgxN5rdW26jzA5tE7btczwpump
probe_bucket = v3_reject_manipulation_contradiction
err = InstructionError(3, Custom(6002))
instruction_index = 3
custom_code = 6002
program_id = 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P
program_name = pumpfun
program_error_name = too_much_sol_required
category = simulation_slippage_or_price_mismatch
route_kind = legacy_buy
buy_variant = None
token_param_role = None
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
entry_token_amount_raw = None
min_tokens_out = None
diagnostic_limit = None
```

Instruction account roles:

- `0:global_config:4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf`
- `1:fee_recipient:GesfTA3X2arioaHp8bbKdjG9vJtskViWACZoYvxp4twS`
- `2:mint:5BLZLfY6kzoKeSkFrQCgxN5rdW26jzA5tE7btczwpump`
- `3:bonding_curve:D6r9AfXN3AmWUw99BxyWczuM5zgew9QtyStB5owL5BVG`
- `4:associated_bonding_curve:4mTGuTVb8D7qzCjxLsZ5h8XCtjqrQ93SXu1Z67XazDJ3`
- `5:user_ata:78UsrFc4Yd7GQWkjJbTv4N44edrZUM9tVkGBAyXEtw5H`
- `6:payer_pubkey:9MCkR8iiQLRxS242CbQijfaKT5AGNr2bWoSsXbQqvbaw`
- `7:system_program:11111111111111111111111111111111`
- `8:token_program:TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb`
- `9:creator_vault:6CU1kyQiUU2DwdeTLqBBcL7zKtkFteJUyfVST7fktaQL`
- `10:event_authority:Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1`
- `11:pump_program:6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P`
- `12:global_volume_accumulator:Hq2wp8uJ9jCPsYgNHex8RtqdvMPfVGoYwjvF1ATiwn2Y`
- `13:user_volume_accumulator:8UumV46GD8N54XbSFxYuTiPLEzMfbD8zxybph1aFHQSF`
- `14:fee_config:8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt`
- `15:fee_program:pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ`
- `16:bonding_curve_v2:HYKNoHgygcRwDbg2vWaR7LYXLEHQ5zbcuqWyTcpGkSef`
- `17:buyback_fee_recipient:GXPFM2caqTtQYC2cJ5yJRi9VDkpsYZXzYdwYpGnLmtDL`

Simulation log tail:

```text
Program log: Instruction: Buy
Program pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ invoke [2]
Program log: Instruction: GetFees
Program pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ consumed 3302 of 336281 compute units
Program return: pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ AAAAAAAAAABfAAAAAAAAAB4AAAAAAAAA
Program pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ success
Program log: AnchorError thrown in programs/pump/src/lib.rs:444. Error Code: TooMuchSolRequired. Error Number: 6002. Error Message: slippage: Too much SOL required to buy the given amount of tokens..
Program log: Left: 7000000
Program log: Right: 11425995
Program 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P consumed 53861 of 381351 compute units
Program return: 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P AAAAAAAAAABfAAAAAAAAAB4AAAAAAAAA
Program 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P failed: custom program error: 0x1772
```

## Decision

Rows without `simulation_error_program_id`, instruction account roles or log tail
are treated as pre-Q4 diagnostic-limited rows. Future probe transport rows now
carry the fields needed to classify whether the error is isolated, route-specific,
amount/slippage-related, or an account-layout mismatch.
