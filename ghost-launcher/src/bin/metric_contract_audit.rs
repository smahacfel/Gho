use clap::{Parser, Subcommand};
use ghost_core::metric_contracts::MetricContractAuditTerminalClassV1;
use ghost_launcher::metric_contracts::{
    audit_pr2c_bundle_against_burn_in_contract_v2, audit_pr2c_single_run_v1,
};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "metric-contract-audit",
    version,
    about = "PR2C durable evidence audit"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    SingleRun {
        #[arg(long)]
        run_dir: PathBuf,
        #[arg(long, required = true)]
        decision_v33: Vec<PathBuf>,
    },
    Bundle {
        #[arg(long, required = true)]
        run_dir: Vec<PathBuf>,
        /// One v33 decision JSONL per run-dir, in the same order.
        #[arg(long, required = true)]
        decision_v33: Vec<PathBuf>,
        #[arg(long)]
        burn_in_contract: PathBuf,
    },
}

fn exit_code(class: MetricContractAuditTerminalClassV1) -> i32 {
    match class {
        MetricContractAuditTerminalClassV1::PassCutoverReady => 0,
        MetricContractAuditTerminalClassV1::NotEvaluable => 2,
        MetricContractAuditTerminalClassV1::FailSchemaOrReplay => 3,
        MetricContractAuditTerminalClassV1::FailPolicyDrift => 4,
        MetricContractAuditTerminalClassV1::FailResourceBudget => 5,
    }
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::SingleRun {
            run_dir,
            decision_v33,
        } => audit_pr2c_single_run_v1(&run_dir, &decision_v33).and_then(|report| {
            let class = report.terminal_class;
            serde_json::to_string_pretty(&report)
                .map(|json| (class, json))
                .map_err(
                    |source| ghost_launcher::metric_contracts::Pr2cAuditErrorV1::Json {
                        path: "stdout".to_string(),
                        source,
                    },
                )
        }),
        Command::Bundle {
            run_dir,
            decision_v33,
            burn_in_contract,
        } => {
            if run_dir.len() != decision_v33.len() {
                eprintln!("bundle requires exactly one --decision-v33 per --run-dir");
                std::process::exit(64);
            }
            let runs = run_dir
                .into_iter()
                .zip(decision_v33)
                .map(|(run_dir, decision)| (run_dir, vec![decision]))
                .collect::<Vec<_>>();
            let contract_bytes = std::fs::read(&burn_in_contract).unwrap_or_else(|error| {
                eprintln!("cannot read {}: {error}", burn_in_contract.display());
                std::process::exit(3);
            });
            let contract: ghost_core::metric_contracts::BurnInContractV1 =
                serde_json::from_slice(&contract_bytes).unwrap_or_else(|error| {
                    eprintln!("invalid BURN_IN_CONTRACT_V3: {error}");
                    std::process::exit(3);
                });
            audit_pr2c_bundle_against_burn_in_contract_v2(&runs, &contract).and_then(|report| {
                let class = report.terminal_class;
                serde_json::to_string_pretty(&report)
                    .map(|json| (class, json))
                    .map_err(
                        |source| ghost_launcher::metric_contracts::Pr2cAuditErrorV1::Json {
                            path: "stdout".to_string(),
                            source,
                        },
                    )
            })
        }
    };
    match result {
        Ok((class, json)) => {
            println!("{json}");
            std::process::exit(exit_code(class));
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(3);
        }
    }
}
