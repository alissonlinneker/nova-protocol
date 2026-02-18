//! # CLI Interface
//!
//! Defines the command-line argument structure for `nova-node` using
//! `clap` derive. Supports four subcommands: `run`, `init`, `status`,
//! and `version`.
//!
//! Address and port arguments default to sane devnet values. Every configurable
//! value has a corresponding environment variable for container-friendly
//! deployment — because nobody wants to pass 12 flags to a Docker entrypoint.

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
#[derive(Parser, Debug, Clone)]
pub struct RunArgs {
    /// Path to the node data directory where blocks, state, and keys are stored.
    ///
    /// Created on first run if it does not exist.
    #[arg(long, short = 'd', env = "NOVA_DATA_DIR", default_value = "~/.nova")]
    pub data_dir: PathBuf,

    /// Full bind address for the JSON-RPC and REST API.
    #[arg(long, env = "NOVA_RPC_ADDR", default_value = "0.0.0.0:9741")]
    pub rpc_addr: String,

    /// Full bind address for P2P communication with other validators.
    #[arg(long, env = "NOVA_P2P_ADDR", default_value = "0.0.0.0:9740")]
    pub p2p_addr: String,

    /// Full bind address for the Prometheus metrics endpoint.
    #[arg(long, env = "NOVA_METRICS_ADDR", default_value = "0.0.0.0:9742")]
    pub metrics_addr: String,

    /// Run in development mode: temporary DB, pre-funded test accounts,
    /// single-validator consensus. Useful for local hacking — never use
    /// this in anything that touches real money.
    #[arg(long)]
    pub dev: bool,

    /// Log verbosity level: trace, debug, info, warn, error.
    #[arg(long, env = "NOVA_LOG_LEVEL", default_value = "info")]
    pub log_level: String,

    /// Participate in consensus as a block-producing validator.
    /// Without this flag the node is a passive full node: it syncs the
    /// chain, serves the API, but never proposes or votes on blocks.
    #[arg(long)]
    pub validator: bool,

    /// Stake amount in photons. Only meaningful when `--validator` is set.
    /// One NOVA = 100_000_000 photons (8 decimal places).
    #[arg(long, default_value_t = 0)]
    pub stake: u64,

    /// Port for the JSON-RPC and REST API (legacy, prefer --rpc-addr).
    #[arg(long, env = "NOVA_RPC_PORT", default_value_t = 9741)]
    pub rpc_port: u16,

    /// Port for P2P communication (legacy, prefer --p2p-addr).
    #[arg(long, env = "NOVA_P2P_PORT", default_value_t = 9740)]
    pub p2p_port: u16,

    /// Port for the Prometheus metrics endpoint (legacy, prefer --metrics-addr).
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
#[derive(Parser, Debug, Clone)]
pub struct InitArgs {
    /// Path to the data directory to initialize.
    #[arg(long, short = 'd', env = "NOVA_DATA_DIR", default_value = "~/.nova")]
    pub data_dir: PathBuf,

    /// Network to configure for: mainnet, testnet, or devnet.
    #[arg(long, default_value = "devnet")]
    pub network: String,

    /// Overwrite an existing data directory. Use with caution — this will
    /// destroy any existing keypair and chain state.
    #[arg(long)]
    pub force: bool,
}

/// Arguments for the `status` subcommand.
#[derive(Parser, Debug, Clone)]
pub struct StatusArgs {
    /// RPC endpoint of the running node.
    #[arg(long, default_value = "http://127.0.0.1:9741")]
    pub rpc_url: String,
}

/// Resolves the data directory path, expanding the `~` prefix to the
/// user's home directory. Returns the path unchanged if it does not
/// start with `~`.
pub fn resolve_data_dir(path: &std::path::Path) -> PathBuf {
    let path_str = path.to_string_lossy();
    if path_str.starts_with("~/") || path_str == "~" {
        if let Some(home) = dirs_home() {
            return home.join(path_str.strip_prefix("~/").unwrap_or(""));
        }
    }
    path.to_path_buf()
}

/// Returns the user's home directory, or `None` if it cannot be determined.
fn dirs_home() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}

/// Validates that the given log level string is recognized.
#[allow(dead_code)]
pub fn validate_log_level(level: &str) -> bool {
    matches!(
        level.to_lowercase().as_str(),
        "trace" | "debug" | "info" | "warn" | "error"
    )
}

/// Formats a photon amount as a human-readable NOVA string with 8 decimals.
/// Example: `10_000_000_000` -> `"100.00000000"`.
pub fn format_nova_amount(photons: u64) -> String {
    let whole = photons / 100_000_000;
    let frac = photons % 100_000_000;
    format!("{}.{:08}", whole, frac)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn verify_cli_structure() {
        // Ensures the derive macros produce a valid CLI definition.
        NovaNodeCli::command().debug_assert();
    }

    #[test]
    fn run_subcommand_defaults() {
        let args = NovaNodeCli::parse_from(["nova-node", "run"]);
        match args.command {
            Commands::Run(run) => {
                assert_eq!(run.rpc_addr, "0.0.0.0:9741");
                assert_eq!(run.p2p_addr, "0.0.0.0:9740");
                assert_eq!(run.metrics_addr, "0.0.0.0:9742");
                assert!(!run.dev);
                assert!(!run.validator);
                assert_eq!(run.stake, 0);
                assert_eq!(run.log_level, "info");
            }
            _ => panic!("expected Run subcommand"),
        }
    }

    #[test]
    fn run_subcommand_dev_mode() {
        let args = NovaNodeCli::parse_from(["nova-node", "run", "--dev", "--validator"]);
        match args.command {
            Commands::Run(run) => {
                assert!(run.dev);
                assert!(run.validator);
            }
            _ => panic!("expected Run subcommand"),
        }
    }

    #[test]
    fn run_subcommand_custom_addresses() {
        let args = NovaNodeCli::parse_from([
            "nova-node",
            "run",
            "--rpc-addr",
            "127.0.0.1:8080",
            "--p2p-addr",
            "127.0.0.1:8081",
            "--metrics-addr",
            "127.0.0.1:8082",
            "--data-dir",
            "/tmp/nova-test",
            "--log-level",
            "debug",
        ]);
        match args.command {
            Commands::Run(run) => {
                assert_eq!(run.rpc_addr, "127.0.0.1:8080");
                assert_eq!(run.p2p_addr, "127.0.0.1:8081");
                assert_eq!(run.metrics_addr, "127.0.0.1:8082");
                assert_eq!(run.data_dir, PathBuf::from("/tmp/nova-test"));
                assert_eq!(run.log_level, "debug");
            }
            _ => panic!("expected Run subcommand"),
        }
    }

    #[test]
    fn init_subcommand_defaults() {
        let args = NovaNodeCli::parse_from(["nova-node", "init"]);
        match args.command {
            Commands::Init(init) => {
                assert_eq!(init.network, "devnet");
                assert!(!init.force);
            }
            _ => panic!("expected Init subcommand"),
        }
    }

    #[test]
    fn init_subcommand_force_flag() {
        let args =
            NovaNodeCli::parse_from(["nova-node", "init", "--force", "--network", "testnet"]);
        match args.command {
            Commands::Init(init) => {
                assert!(init.force);
                assert_eq!(init.network, "testnet");
            }
            _ => panic!("expected Init subcommand"),
        }
    }

    #[test]
    fn status_subcommand_defaults() {
        let args = NovaNodeCli::parse_from(["nova-node", "status"]);
        match args.command {
            Commands::Status(status) => {
                assert_eq!(status.rpc_url, "http://127.0.0.1:9741");
            }
            _ => panic!("expected Status subcommand"),
        }
    }

    #[test]
    fn status_subcommand_custom_url() {
        let args =
            NovaNodeCli::parse_from(["nova-node", "status", "--rpc-url", "http://my-node:9741"]);
        match args.command {
            Commands::Status(status) => {
                assert_eq!(status.rpc_url, "http://my-node:9741");
            }
            _ => panic!("expected Status subcommand"),
        }
    }

    #[test]
    fn version_subcommand_parses() {
        let args = NovaNodeCli::parse_from(["nova-node", "version"]);
        assert!(matches!(args.command, Commands::Version));
    }

    #[test]
    fn resolve_data_dir_expands_tilde() {
        let path = PathBuf::from("~/.nova");
        let resolved = resolve_data_dir(&path);
        // On any reasonable system, the resolved path should not start with "~".
        assert!(
            !resolved.to_string_lossy().starts_with('~'),
            "tilde should have been expanded: {:?}",
            resolved
        );
    }

    #[test]
    fn resolve_data_dir_absolute_unchanged() {
        let path = PathBuf::from("/tmp/nova-data");
        let resolved = resolve_data_dir(&path);
        assert_eq!(resolved, path);
    }

    #[test]
    fn validate_log_level_accepts_valid() {
        assert!(validate_log_level("trace"));
        assert!(validate_log_level("debug"));
        assert!(validate_log_level("info"));
        assert!(validate_log_level("warn"));
        assert!(validate_log_level("error"));
        assert!(validate_log_level("INFO")); // case-insensitive
    }

    #[test]
    fn validate_log_level_rejects_invalid() {
        assert!(!validate_log_level("verbose"));
        assert!(!validate_log_level(""));
        assert!(!validate_log_level("critical"));
    }

    #[test]
    fn format_nova_amount_whole_number() {
        assert_eq!(format_nova_amount(100_000_000), "1.00000000");
        assert_eq!(format_nova_amount(100_000_000_000_000), "1000000.00000000");
    }

    #[test]
    fn format_nova_amount_fractional() {
        assert_eq!(format_nova_amount(50_000), "0.00050000");
        assert_eq!(format_nova_amount(123_456_789), "1.23456789");
    }

    #[test]
    fn format_nova_amount_zero() {
        assert_eq!(format_nova_amount(0), "0.00000000");
    }

    #[test]
    fn run_with_stake() {
        let args =
            NovaNodeCli::parse_from(["nova-node", "run", "--validator", "--stake", "10000000000"]);
        match args.command {
            Commands::Run(run) => {
                assert!(run.validator);
                assert_eq!(run.stake, 10_000_000_000);
            }
            _ => panic!("expected Run subcommand"),
        }
    }
}
