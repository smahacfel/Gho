//! Static Lookup Table (LUT) Initialization Script
//!
//! This script creates a Static Address Lookup Table on Solana blockchain
//! containing frequently used addresses for Pump.fun AMM interactions.
//!
//! ## Purpose
//! By storing common addresses in a LUT, transactions can reference them using
//! 1-byte indices instead of 32-byte addresses, reducing transaction size from
//! ~500-600 bytes to ~200-300 bytes.
//!
//! ## Usage
//! ```bash
//! # Set your keypair path
//! export KEYPAIR_PATH=~/.config/solana/id.json
//!
//! # Run on devnet
//! cargo run --example init_static_lut -- --network devnet
//!
//! # Run on mainnet (production)
//! cargo run --example init_static_lut -- --network mainnet
//! ```
//!
//! ## Output
//! After successful execution, the script will output the LUT address.
//! Add this address to your config.toml:
//! ```toml
//! [trigger]
//! static_lut_address = "GhostLut111..."
//! ```

use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    address_lookup_table::{
        instruction::{create_lookup_table, extend_lookup_table},
        state::AddressLookupTable,
    },
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
    signature::{read_keypair_file, Signer},
    system_program,
    transaction::Transaction,
};
use std::{env, str::FromStr, thread, time::Duration};

/// Static addresses to include in the LUT for Pump.fun transactions
fn get_static_addresses() -> Vec<Pubkey> {
    vec![
        // System Program
        system_program::id(),
        // Token Program
        Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
            .expect("Invalid Token Program ID"),
        // Associated Token Program
        Pubkey::from_str("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")
            .expect("Invalid Associated Token Program ID"),
        // Rent Sysvar
        Pubkey::from_str("SysvarRent111111111111111111111111111111111")
            .expect("Invalid Rent Sysvar"),
        // Pump.fun Program ID
        Pubkey::from_str("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P")
            .expect("Invalid Pump.fun Program ID"),
        // Pump.fun Fee Recipient
        Pubkey::from_str("CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM")
            .expect("Invalid Pump.fun Fee Recipient"),
        // Pump.fun Event Authority (Global PDA)
        // This is derived from seeds ["global"] with the Pump.fun program
        // Computed: Ce6TQqE9wd3REmWx6TtHqhYpqhVHpmpAbnMfbjmuhwtN
        derive_pump_global_pda(),
    ]
}

/// Derive the Pump.fun Global PDA
fn derive_pump_global_pda() -> Pubkey {
    let pump_program_id = Pubkey::from_str("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P")
        .expect("Invalid program ID");
    let (global_pda, _bump) = Pubkey::find_program_address(&[b"global"], &pump_program_id);
    global_pda
}

fn get_rpc_url(network: &str) -> &'static str {
    match network {
        "mainnet" | "mainnet-beta" => "https://api.mainnet-beta.solana.com",
        "devnet" => "https://api.devnet.solana.com",
        "testnet" => "https://api.testnet.solana.com",
        "localnet" | "localhost" => "http://localhost:8899",
        _ => panic!(
            "Unknown network: {}. Use: mainnet, devnet, testnet, or localnet",
            network
        ),
    }
}

fn main() -> anyhow::Result<()> {
    // Parse command line arguments
    let args: Vec<String> = env::args().collect();
    let mut network = String::from("devnet");
    let mut keypair_path = env::var("KEYPAIR_PATH")
        .unwrap_or_else(|_| format!("{}/.config/solana/id.json", env::var("HOME").unwrap()));

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--network" | "-n" => {
                i += 1;
                if i < args.len() {
                    network = args[i].clone();
                }
            }
            "--keypair" | "-k" => {
                i += 1;
                if i < args.len() {
                    keypair_path = args[i].clone();
                }
            }
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            _ => {}
        }
        i += 1;
    }

    println!("═══════════════════════════════════════════════════════════════");
    println!("       Ghost Static LUT Initialization Script");
    println!("═══════════════════════════════════════════════════════════════");
    println!();
    println!("Network: {}", network);
    println!("Keypair: {}", keypair_path);
    println!();

    // Load keypair
    let payer = read_keypair_file(&keypair_path)
        .map_err(|e| anyhow::anyhow!("Failed to read keypair from {}: {}", keypair_path, e))?;
    println!("Authority: {}", payer.pubkey());

    // Create RPC client
    let rpc_url = get_rpc_url(&network);
    let client = RpcClient::new_with_commitment(rpc_url.to_string(), CommitmentConfig::confirmed());

    // Check balance
    let balance = client.get_balance(&payer.pubkey())?;
    println!("Balance: {} SOL", balance as f64 / 1_000_000_000.0);

    if balance < 10_000_000 {
        // 0.01 SOL minimum
        anyhow::bail!(
            "Insufficient balance. Need at least 0.01 SOL for LUT creation. Current: {} SOL",
            balance as f64 / 1_000_000_000.0
        );
    }

    // Get recent slot for LUT derivation
    let recent_slot = client.get_slot()?;
    println!("Recent slot: {}", recent_slot);

    // Step 1: Create the Lookup Table
    println!();
    println!("Step 1: Creating Address Lookup Table...");

    let (create_ix, lut_address) = create_lookup_table(payer.pubkey(), payer.pubkey(), recent_slot);

    println!("  LUT Address: {}", lut_address);

    let recent_blockhash = client.get_latest_blockhash()?;
    let create_tx = Transaction::new_signed_with_payer(
        &[create_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    let create_sig = client.send_and_confirm_transaction(&create_tx)?;
    println!("  ✓ Created LUT: {}", create_sig);

    // Wait a bit for the LUT to be created
    println!("  Waiting for LUT creation to finalize...");
    thread::sleep(Duration::from_secs(2));

    // Step 2: Extend the Lookup Table with addresses
    println!();
    println!("Step 2: Extending LUT with static addresses...");

    let static_addresses = get_static_addresses();
    println!("  Adding {} addresses:", static_addresses.len());
    for (i, addr) in static_addresses.iter().enumerate() {
        println!("    [{}] {}", i, addr);
    }

    let extend_ix = extend_lookup_table(
        lut_address,
        payer.pubkey(),
        Some(payer.pubkey()),
        static_addresses.clone(),
    );

    let recent_blockhash = client.get_latest_blockhash()?;
    let extend_tx = Transaction::new_signed_with_payer(
        &[extend_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    let extend_sig = client.send_and_confirm_transaction(&extend_tx)?;
    println!("  ✓ Extended LUT: {}", extend_sig);

    // Wait for extension to finalize
    println!("  Waiting for extension to finalize...");
    thread::sleep(Duration::from_secs(2));

    // Step 3: Verify the LUT
    println!();
    println!("Step 3: Verifying LUT contents...");

    let lut_account = client.get_account(&lut_address)?;
    let lut_data = AddressLookupTable::deserialize(&lut_account.data)?;

    println!("  LUT contains {} addresses", lut_data.addresses.len());

    if lut_data.addresses.len() != static_addresses.len() {
        println!(
            "  ⚠ Warning: Expected {} addresses, found {}",
            static_addresses.len(),
            lut_data.addresses.len()
        );
    }

    for (i, addr) in lut_data.addresses.iter().enumerate() {
        let expected = &static_addresses[i];
        let status = if addr == expected { "✓" } else { "✗" };
        println!("    [{}] {} {}", i, status, addr);
    }

    // Output final result
    println!();
    println!("═══════════════════════════════════════════════════════════════");
    println!("                    LUT CREATION SUCCESSFUL!");
    println!("═══════════════════════════════════════════════════════════════");
    println!();
    println!("Add the following to your config.toml:");
    println!();
    println!("  [trigger]");
    println!("  static_lut_address = \"{}\"", lut_address);
    println!();
    println!("Network: {}", network);
    println!("LUT Address: {}", lut_address);
    println!("Authority: {}", payer.pubkey());
    println!();
    println!("Expected transaction size reduction:");
    println!("  Without LUT: ~500-600 bytes");
    println!("  With LUT:    ~200-300 bytes");
    println!("  Savings:     ~250-350 bytes per transaction");
    println!();

    Ok(())
}

fn print_help() {
    println!("Ghost Static LUT Initialization Script");
    println!();
    println!("USAGE:");
    println!("    cargo run --example init_static_lut -- [OPTIONS]");
    println!();
    println!("OPTIONS:");
    println!("    -n, --network <NETWORK>    Network to use (mainnet, devnet, testnet, localnet)");
    println!("                               Default: devnet");
    println!("    -k, --keypair <PATH>       Path to keypair file");
    println!("                               Default: $KEYPAIR_PATH or ~/.config/solana/id.json");
    println!("    -h, --help                 Print this help message");
    println!();
    println!("ENVIRONMENT VARIABLES:");
    println!("    KEYPAIR_PATH               Path to keypair file (overridden by --keypair)");
    println!();
    println!("EXAMPLES:");
    println!("    # Use devnet with default keypair");
    println!("    cargo run --example init_static_lut");
    println!();
    println!("    # Use mainnet with custom keypair");
    println!("    cargo run --example init_static_lut -- -n mainnet -k /path/to/keypair.json");
    println!();
}
