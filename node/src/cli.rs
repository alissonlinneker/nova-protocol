//! # CLI Interface
//!
//! Defines the command-line argument structure for `nova-node` using
//! `clap` derive. Supports four subcommands: `run`, `init`, `status`,
//! and `version`.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// NOVA Protocol validator node.
///
/// A full validator node for the NOVA payment network. Participates in
/// consensus, validates transactions, serves the JSON-RPC API, and
/// exposes Prometheus metrics.
#[derive(Parser, Debug)]
#[command(
    name = "nova-node",
    about = "NOVA Protocol validator node",
    version,
    propagate_version = true
)]
pub struct NovaNodeCli {
    /// Subcommand to execute.
    #[command(subcommand)]
    pub command: Commands,
}

/// Top-level subcommands for the NOVA node binary.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Start the validator node.
    Run(RunArgs),
    /// Initialize a new node — creates the data directory and generates
    /// a fresh validator keypair.
    Init(InitArgs),
    /// Query the status of a running node via its RPC endpoint.
    Status(StatusArgs),
    /// Print version information and exit.
    Version,
}

/// Arguments for the `run` subcommand.
#[derive(Parser, Debug)]
pub struct RunArgs {
    /// Path to the node configuration file (TOML).
    ///
    /// When omitted, the node looks for `config.toml` in the data directory.
    #[arg(long, short = 'c', env = "NOVA_CONFIG")]
    pub config: Option<PathBuf>,

    /// Path to the node data directory where blocks, state, and keys are stored.
    ///
    /// Created on first run if it does not exist.
    #[arg(long, short = 'd', env = "NOVA_DATA_DIR", default_value = "~/.nova")]
    pub data_dir: PathBuf,

    /// Port for the JSON-RPC and REST API.
    #[arg(long, env = "NOVA_RPC_PORT", default_value_t = 9741)]
    pub rpc_port: u16,

    /// Port for P2P communication with other validators.
    #[arg(long, env = "NOVA_P2P_PORT", default_value_t = 9740)]
    pub p2p_port: u16,

    /// Port for the Prometheus metrics endpoint.
    #[arg(long, env = "NOVA_METRICS_PORT", default_value_t = 9742)]
    pub metrics_port: u16,

    /// Hex-encoded Ed25519 validator private key.
    ///
    /// If not provided, the node reads the key from the data directory.
    /// **Never pass this flag in production** — use a key file or vault instead.
    #[arg(long, env = "NOVA_VALIDATOR_KEY")]
    pub validator_key: Option<String>,
}

/// Arguments for the `init` subcommand.
#[derive(Parser, Debug)]
pub struct InitArgs {
    /// Path to the data directory to initialize.
    #[arg(long, short = 'd', env = "NOVA_DATA_DIR", default_value = "~/.nova")]
    pub data_dir: PathBuf,

    /// Network to configure for: mainnet, testnet, or devnet.
    #[arg(long, default_value = "devnet")]
    pub network: String,
}

/// Arguments for the `status` subcommand.
#[derive(Parser, Debug)]
pub struct StatusArgs {
    /// RPC endpoint of the running node.
    #[arg(long, default_value = "http://127.0.0.1:9741")]
    pub rpc_url: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn verify_cli_structure() {
        // Ensures the derive macros produce a valid CLI definition.
        NovaNodeCli::command().debug_assert();
    }
}
