use anyhow::{Context, Result};
use solana_account_decoder::{parse_account_data::ParsedAccount, UiAccountData, UiAccountEncoding};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_request::TokenAccountsFilter;
use solana_sdk::program_pack::Pack;
use solana_sdk::pubkey::Pubkey;
use spl_token_2022::state::Account as SplTokenAccount;
use std::str::FromStr;

const TOKEN_LEGACY_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalletPosition {
    pub mint: Pubkey,
    pub amount: u64,
    pub ata: Pubkey,
}

pub async fn scan_wallet_positions(rpc: &RpcClient, owner: &Pubkey) -> Result<Vec<WalletPosition>> {
    let mut positions = Vec::new();
    for program_id in token_program_ids()? {
        let accounts = rpc
            .get_token_accounts_by_owner(owner, TokenAccountsFilter::ProgramId(program_id))
            .await
            .with_context(|| {
                format!(
                    "failed to scan token accounts for owner {} via program {}",
                    owner, program_id
                )
            })?;

        for keyed_account in accounts {
            let ata = Pubkey::from_str(&keyed_account.pubkey).with_context(|| {
                format!(
                    "wallet scan returned invalid token account pubkey '{}'",
                    keyed_account.pubkey
                )
            })?;

            positions.push(decode_wallet_position(&keyed_account.account.data, ata)?);
        }
    }

    Ok(positions)
}

fn decode_wallet_position(data: &UiAccountData, ata: Pubkey) -> Result<WalletPosition> {
    match data {
        UiAccountData::Json(parsed) => decode_json_wallet_position(parsed, ata),
        _ => {
            let data = decode_account_data(data, ata)?;
            let token_account = SplTokenAccount::unpack(&data)
                .with_context(|| format!("failed to decode SPL token account data for {}", ata))?;

            Ok(WalletPosition {
                mint: token_account.mint,
                amount: token_account.amount,
                ata,
            })
        }
    }
}

fn decode_account_data(data: &UiAccountData, ata: Pubkey) -> Result<Vec<u8>> {
    match data {
        UiAccountData::Binary(encoded, UiAccountEncoding::Base64) => base64::decode(encoded)
            .with_context(|| format!("invalid base64 account data for {}", ata)),
        UiAccountData::Binary(_, encoding) => Err(anyhow::anyhow!(
            "unsupported token account encoding {:?} for {}",
            encoding,
            ata
        )),
        UiAccountData::LegacyBinary(encoded) => bs58::decode(encoded)
            .into_vec()
            .with_context(|| format!("invalid legacy base58 account data for {}", ata)),
        UiAccountData::Json(_) => unreachable!("json payloads are handled before binary decoding"),
    }
}

fn decode_json_wallet_position(parsed: &ParsedAccount, ata: Pubkey) -> Result<WalletPosition> {
    let account_type = parsed
        .parsed
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if account_type != "account" {
        return Err(anyhow::anyhow!(
            "unexpected parsed token account type '{}' for {}",
            account_type,
            ata
        ));
    }

    let info = parsed
        .parsed
        .get("info")
        .and_then(|value| value.as_object())
        .with_context(|| {
            format!(
                "parsed token account payload missing info object for {}",
                ata
            )
        })?;

    let mint = info
        .get("mint")
        .and_then(|value| value.as_str())
        .with_context(|| format!("parsed token account payload missing mint for {}", ata))
        .and_then(|raw| {
            Pubkey::from_str(raw).with_context(|| {
                format!("parsed token account payload has invalid mint for {}", ata)
            })
        })?;

    let amount = info
        .get("tokenAmount")
        .and_then(|value| value.get("amount"))
        .and_then(|value| value.as_str())
        .with_context(|| {
            format!(
                "parsed token account payload missing tokenAmount.amount for {}",
                ata
            )
        })
        .and_then(|raw| {
            raw.parse::<u64>().with_context(|| {
                format!(
                    "parsed token account payload has invalid tokenAmount.amount for {}",
                    ata
                )
            })
        })?;

    Ok(WalletPosition { mint, amount, ata })
}

fn token_program_ids() -> Result<[Pubkey; 2]> {
    Ok([
        Pubkey::from_str(TOKEN_2022_PROGRAM_ID)
            .context("invalid token-2022 program id constant")?,
        Pubkey::from_str(TOKEN_LEGACY_PROGRAM_ID)
            .context("invalid legacy token program id constant")?,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decode_wallet_position_accepts_json_parsed_token_account() {
        let ata = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let parsed = ParsedAccount {
            program: "spl-token".to_string(),
            parsed: json!({
                "type": "account",
                "info": {
                    "mint": mint.to_string(),
                    "tokenAmount": {
                        "amount": "12345"
                    }
                }
            }),
            space: 165,
        };

        let position =
            decode_wallet_position(&UiAccountData::Json(parsed), ata).expect("json parsed account");

        assert_eq!(position.ata, ata);
        assert_eq!(position.mint, mint);
        assert_eq!(position.amount, 12_345);
    }
}
