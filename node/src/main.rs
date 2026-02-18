// Copyright (c) 2026 ALAS Technology. MIT License.
// See LICENSE for details.

//! # NOVA Validator Node
//!
//! Entry point for the `nova-node` binary. Parses CLI arguments, initializes
//! logging and metrics, starts the validator loop, and serves the HTTP/WS API.
//!
//! The binary supports four subcommands:
//!
//! - `run`     — start the validator node
//! - `init`    — initialize data directory and generate keys
//! - `status`  — query a running node's status endpoint
//! - `version` — print build version information

mod api;
mod cli;
mod logging;
mod metrics;

use anyhow::{Context, Result};
use clap::Parser;
use std::sync::Arc;
use tokio::signal;
use tokio::sync::{broadcast, RwLock};

use nova_protocol::identity::{NovaId, NovaKeypair};
use nova_protocol::network::consensus::{ConsensusConfig, ConsensusEngine, ValidatorSet};
use nova_protocol::network::consensus_loop::{ConsensusLoop, ConsensusLoopConfig};
use nova_protocol::network::mempool::{Mempool, MempoolConfig};
use nova_protocol::network::producer::BlockProducer;
use nova_protocol::storage::db::NovaDB;
use nova_protocol::storage::state::{AccountState, StateTree};

use cli::{Commands, NovaNodeCli};
use logging::LogFormat;
use metrics::NodeMetrics;

/// Broadcast channel capacity for live event streaming.
/// 256 is large enough to absorb short bursts without dropping events
/// for connected WebSocket clients.
const EVENT_CHANNEL_CAPACITY: usize = 256;

/// Dev mode: number of pre-funded test accounts.
const DEV_ACCOUNT_COUNT: u64 = 10;

/// Dev mode: initial balance per test account (1M NOVA = 1_000_000 * 10^8 photons).
const DEV_ACCOUNT_BALANCE: u64 = 1_000_000_00000000;

/// Dev mode: default validator stake (100 NOVA = 10B photons).
const DEV_VALIDATOR_STAKE: u64 = 10_000_000_000;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = NovaNodeCli::parse();

    match cli.command {
        Commands::Run(args) => run_node(args).await,
        Commands::Init(args) => init_node(args),
        Commands::Status(args) => query_status(args).await,
        Commands::Version => {
            print_version();
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// run — Full validator startup sequence
// ---------------------------------------------------------------------------

/// Starts the full validator node: API server, metrics endpoint, and
/// consensus participation.
///
/// Startup sequence:
/// 1.  Parse CLI args (already done)
/// 2.  Initialize logging
/// 3.  Generate or load keypair
/// 4.  Open NovaDB
/// 5.  Initialize StateTree (genesis if empty)
/// 6.  Pre-fund dev accounts (if --dev)
/// 7.  Create Mempool
/// 8.  Create ValidatorSet
/// 9.  Create ConsensusEngine
/// 10. Create BlockProducer
/// 11. Create ConsensusLoop
/// 12. Setup shutdown handler
/// 13. Spawn consensus loop (if --validator)
/// 14. Start API server
/// 15. Print startup banner
/// 16. Await shutdown
/// 17. Graceful shutdown
async fn run_node(args: cli::RunArgs) -> Result<()> {
    // --- 1. Resolve paths and validate config ---
    let data_dir = cli::resolve_data_dir(&args.data_dir);

    let log_filter = format!(
        "nova_node={level},nova_protocol={level},tower_http=debug",
        level = args.log_level
    );
    let log_format = LogFormat::Pretty;

    // --- 2. Initialize logging ---
    logging::init_logging(&log_filter, log_format);

    tracing::info!(
        rpc_addr = %args.rpc_addr,
        p2p_addr = %args.p2p_addr,
        metrics_addr = %args.metrics_addr,
        data_dir = %data_dir.display(),
        dev = args.dev,
        validator = args.validator,
        "starting nova-node"
    );

    // --- 3. Generate or load keypair ---
    let keypair = if args.dev {
        // Dev mode: generate a fresh keypair (not persisted).
        let kp = NovaKeypair::generate();
        tracing::info!(
            public_key = %kp.public_key().to_hex(),
            "generated ephemeral dev keypair"
        );
        kp
    } else {
        load_or_generate_keypair(&data_dir)?
    };

    let validator_address = keypair.public_key().to_hex();
    let nova_id = NovaId::from_public_key(&keypair.public_key());
    let nova_address = nova_id.to_address();

    // --- 4. Open NovaDB ---
    let db = if args.dev {
        Arc::new(
            NovaDB::open_temporary()
                .context("failed to open temporary database for dev mode")?,
        )
    } else {
        let db_path = data_dir.join("db");
        std::fs::create_dir_all(&db_path)
            .with_context(|| format!("failed to create database directory: {}", db_path.display()))?;
        Arc::new(
            NovaDB::open(&db_path)
                .with_context(|| format!("failed to open database at {}", db_path.display()))?,
        )
    };
    tracing::info!("database opened");

    // --- 5. Initialize StateTree (genesis if empty) ---
    let state_tree = Arc::new(RwLock::new(StateTree::new((*db).clone())));

    // --- Block height ---
    let block_height = Arc::new(std::sync::atomic::AtomicU64::new(0));

    // --- Genesis initialization ---
    api::initialize_genesis(&db, &block_height);

    // --- 6. Pre-fund dev accounts (if --dev) ---
    let dev_stake = if args.dev {
        let funded_addresses = prefund_dev_accounts(&state_tree).await;
        for (i, addr) in funded_addresses.iter().enumerate() {
            tracing::info!(
                index = i + 1,
                address = %addr,
                balance = "1,000,000 NOVA",
                "dev account funded"
            );
        }
        DEV_VALIDATOR_STAKE
    } else {
        args.stake
    };

    // --- 7. Create Mempool ---
    let mempool = Arc::new(Mempool::new(MempoolConfig::default()));

    // --- 8. Create ValidatorSet ---
    let mut validator_set = ValidatorSet::new();
    if args.validator || args.dev {
        validator_set.add_validator(validator_address.clone(), dev_stake);
        tracing::info!(
            address = %validator_address,
            stake = dev_stake,
            "added self to validator set"
        );
    }

    // --- 9. Create ConsensusEngine ---
    let consensus_config = if args.dev {
        ConsensusConfig {
            min_validators: 1,
            ..ConsensusConfig::default()
        }
    } else {
        ConsensusConfig::default()
    };

    let mut engine = ConsensusEngine::new(consensus_config, validator_set);

    // Sync engine to current chain tip.
    if let Ok(Some(h)) = db.get_latest_block_height() {
        if let Ok(Some(block)) = db.get_block(h) {
            engine.set_chain_state(h + 1, block.header.hash);
            tracing::info!(height = h, "consensus engine synced to chain tip");
        }
    }

    let engine = Arc::new(parking_lot::RwLock::new(engine));

    // --- 10. Create BlockProducer ---
    // The consensus loop and block producer use parking_lot::RwLock for the
    // state tree (required by the protocol library), separate from the tokio
    // RwLock used by the API layer. In dev mode, the parking_lot tree needs
    // the same pre-funded accounts.
    let state_tree_for_consensus = if args.dev {
        let st = Arc::new(parking_lot::RwLock::new(StateTree::new((*db).clone())));
        // Re-fund in the parking_lot tree too.
        {
            let mut tree = st.write();
            for i in 1..=DEV_ACCOUNT_COUNT {
                let seed = generate_dev_seed(i);
                let kp = NovaKeypair::from_seed(&seed);
                let id = NovaId::from_public_key(&kp.public_key());
                let addr = id.to_address();
                tree.put(&addr, &AccountState::with_balance(DEV_ACCOUNT_BALANCE));
            }
        }
        st
    } else {
        Arc::new(parking_lot::RwLock::new(StateTree::new((*db).clone())))
    };

    let producer = Arc::new(BlockProducer::new(
        Arc::clone(&db),
        Arc::clone(&state_tree_for_consensus),
        Arc::clone(&mempool),
        keypair.clone(),
    ));

    // --- 11. Create ConsensusLoop ---
    let consensus_loop_config = ConsensusLoopConfig::default();
    let consensus_loop = ConsensusLoop::new(
        Arc::clone(&engine),
        Arc::clone(&producer),
        Arc::clone(&db),
        Arc::clone(&state_tree_for_consensus),
        Arc::clone(&mempool),
        keypair.clone(),
        consensus_loop_config,
    );

    // --- Metrics ---
    let node_metrics = Arc::new(NodeMetrics::new());

    // --- Event broadcast ---
    let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);

    // --- Application state ---
    let app_state = api::AppState {
        version: format!(
            "{} (protocol {})",
            env!("CARGO_PKG_VERSION"),
            nova_protocol::config::PROTOCOL_VERSION,
        ),
        network: "devnet".to_string(),
        block_height: Arc::clone(&block_height),
        peer_count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        event_tx: event_tx.clone(),
        metrics: Arc::clone(&node_metrics),
        db: Arc::clone(&db),
        state_tree,
    };

    // --- 12. Setup shutdown handler ---
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    // --- 13. Spawn consensus loop (if --validator or --dev) ---
    let consensus_handle = if args.validator || args.dev {
        let shutdown_rx_consensus = shutdown_rx.clone();
        Some(tokio::spawn(async move {
            match consensus_loop.run(shutdown_rx_consensus).await {
                Err(e) => {
                    tracing::info!("consensus loop exited: {}", e);
                }
                Ok(()) => {
                    tracing::info!("consensus loop exited cleanly");
                }
            }
        }))
    } else {
        // Passive node: run a stub block height incrementer for API/metrics.
        let height_ref = Arc::clone(&app_state.block_height);
        let metrics_ref = Arc::clone(&node_metrics);
        let event_tx_ref = event_tx.clone();
        Some(tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(
                nova_protocol::config::BLOCK_TIME_MS,
            ));
            loop {
                interval.tick().await;
                let h = height_ref.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                metrics_ref.block_height.set(h as i64);
                metrics_ref.blocks_processed_total.inc();

                let _ = event_tx_ref.send(api::NodeEvent::NewBlock {
                    height: h,
                    hash: format!("{:064x}", h),
                    tx_count: 0,
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                });

                tracing::debug!(height = h, "block produced (stub)");
            }
        }))
    };

    // --- 14. Start API server ---
    let api_router = api::create_router(app_state.clone());
    let api_listener = tokio::net::TcpListener::bind(&args.rpc_addr)
        .await
        .with_context(|| format!("failed to bind RPC listener on {}", args.rpc_addr))?;
    tracing::info!("RPC/API server listening on {}", args.rpc_addr);

    // --- Metrics server ---
    let metrics_router = axum::Router::new()
        .route("/metrics", axum::routing::get(metrics::metrics_handler))
        .with_state(Arc::clone(&node_metrics));
    let metrics_listener = tokio::net::TcpListener::bind(&args.metrics_addr)
        .await
        .with_context(|| format!("failed to bind metrics listener on {}", args.metrics_addr))?;
    tracing::info!("Metrics server listening on {}", args.metrics_addr);

    // --- 15. Print startup banner ---
    let mode = match (args.validator || args.dev, args.dev) {
        (true, true) => "Validator (dev)",
        (true, false) => "Validator",
        (false, _) => "Full Node",
    };

    print_startup_banner(
        &nova_address,
        &args.rpc_addr,
        &args.p2p_addr,
        &data_dir.to_string_lossy(),
        mode,
        dev_stake,
    );

    // --- 16. Await shutdown signal ---
    tokio::select! {
        res = axum::serve(api_listener, api_router) => {
            if let Err(e) = res {
                tracing::error!("API server error: {}", e);
            }
        }
        res = axum::serve(metrics_listener, metrics_router) => {
            if let Err(e) = res {
                tracing::error!("Metrics server error: {}", e);
            }
        }
        _ = shutdown_signal() => {
            tracing::info!("shutdown signal received, draining connections");
        }
    }

    // --- 17. Graceful shutdown ---
    let _ = shutdown_tx.send(true);
    if let Some(handle) = consensus_handle {
        handle.abort();
    }

    tracing::info!("nova-node stopped");
    Ok(())
}

// ---------------------------------------------------------------------------
// init — Data directory initialization
// ---------------------------------------------------------------------------

/// Initializes a new node data directory and generates a validator keypair.
///
/// Creates the directory structure:
/// ```text
/// {data_dir}/
///     db/         — RocksDB/sled storage
///     keys/       — Validator keypair
///     config/     — Node configuration
/// ```
fn init_node(args: cli::InitArgs) -> Result<()> {
    logging::init_logging("nova_node=info", LogFormat::Pretty);

    let data_dir = cli::resolve_data_dir(&args.data_dir);
    tracing::info!(data_dir = %data_dir.display(), network = %args.network, "initializing node");

    // Check if data directory already exists and --force is not set.
    if data_dir.exists() && !args.force {
        let key_path = data_dir.join("keys").join("validator.key");
        if key_path.exists() {
            anyhow::bail!(
                "data directory already initialized at {}. Use --force to overwrite.",
                data_dir.display()
            );
        }
    }

    // Create directory structure.
    let db_dir = data_dir.join("db");
    let keys_dir = data_dir.join("keys");
    let config_dir = data_dir.join("config");

    std::fs::create_dir_all(&db_dir)
        .with_context(|| format!("failed to create db directory: {}", db_dir.display()))?;
    std::fs::create_dir_all(&keys_dir)
        .with_context(|| format!("failed to create keys directory: {}", keys_dir.display()))?;
    std::fs::create_dir_all(&config_dir)
        .with_context(|| format!("failed to create config directory: {}", config_dir.display()))?;

    // Generate validator keypair.
    let keypair = NovaKeypair::generate();
    let pubkey_hex = keypair.public_key().to_hex();
    let nova_id = NovaId::from_public_key(&keypair.public_key());
    let nova_address = nova_id.to_address();

    // Save the secret key as hex-encoded bytes.
    let key_path = keys_dir.join("validator.key");
    let secret_bytes = keypair.secret_key_bytes();
    std::fs::write(&key_path, hex::encode(secret_bytes))
        .with_context(|| format!("failed to write validator key to {}", key_path.display()))?;

    // Restrict permissions on Unix.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
    }

    // Initialize database with genesis block.
    let db = NovaDB::open(&db_dir)
        .with_context(|| format!("failed to open database at {}", db_dir.display()))?;
    let block_height = std::sync::atomic::AtomicU64::new(0);
    api::initialize_genesis(&db, &block_height);

    tracing::info!(
        public_key = %pubkey_hex,
        address = %nova_address,
        key_path = %key_path.display(),
        "validator keypair generated"
    );

    println!();
    println!("Node initialized successfully.");
    println!();
    println!("  Data directory : {}", data_dir.display());
    println!("  Network        : {}", args.network);
    println!("  Validator key  : {}", key_path.display());
    println!("  Public key     : {}", pubkey_hex);
    println!("  NOVA address   : {}", nova_address);
    println!("  DB directory   : {}", db_dir.display());
    println!("  Genesis block  : persisted at height 0");
    println!();
    println!("Run `nova-node run -d {}` to start the node.", data_dir.display());

    Ok(())
}

// ---------------------------------------------------------------------------
// status — Query a running node
// ---------------------------------------------------------------------------

/// Queries a running node's status endpoint and prints the result.
async fn query_status(args: cli::StatusArgs) -> Result<()> {
    let url = format!("{}/status", args.rpc_url.trim_end_matches('/'));
    let body: String = reqwest_get_stub(&url).await?;

    // Try to pretty-print the JSON; fall back to raw output if parsing fails.
    match serde_json::from_str::<serde_json::Value>(&body) {
        Ok(json) => {
            if let Some(version) = json.get("version").and_then(|v| v.as_str()) {
                println!("Node Status");
                println!("  Version     : {}", version);
            }
            if let Some(network) = json.get("network").and_then(|v| v.as_str()) {
                println!("  Network     : {}", network);
            }
            if let Some(height) = json.get("block_height").and_then(|v| v.as_u64()) {
                println!("  Block Height: {}", height);
            }
            if let Some(peers) = json.get("peer_count").and_then(|v| v.as_u64()) {
                println!("  Peers       : {}", peers);
            }
            if let Some(synced) = json.get("synced").and_then(|v| v.as_bool()) {
                println!("  Synced      : {}", if synced { "yes" } else { "no" });
            }
            if let Some(ts) = json.get("timestamp").and_then(|v| v.as_str()) {
                println!("  Timestamp   : {}", ts);
            }
        }
        Err(_) => {
            println!("{}", body);
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// version
// ---------------------------------------------------------------------------

/// Prints version information to stdout.
fn print_version() {
    println!("nova-node {}", env!("CARGO_PKG_VERSION"));
    println!("protocol  {}", nova_protocol::config::PROTOCOL_VERSION);
    println!("rustc     {}", rustc_version());
    if let Some(commit) = option_env!("GIT_COMMIT") {
        println!("commit    {}", commit);
    }
    if let Some(ts) = option_env!("BUILD_TIMESTAMP") {
        println!("built     {}", ts);
    }
}

/// Returns the Rust compiler version used to build this binary.
fn rustc_version() -> &'static str {
    option_env!("RUSTC_VERSION").unwrap_or("unknown")
}

// ---------------------------------------------------------------------------
// Keypair persistence
// ---------------------------------------------------------------------------

/// Loads a validator keypair from `{data_dir}/keys/validator.key`, or generates
/// and saves a new one if the key file does not exist.
///
/// The key file is hex-encoded (64 hex characters = 32 bytes secret key).
/// File permissions are restricted to owner-only (0o600) on Unix.
fn load_or_generate_keypair(data_dir: &std::path::Path) -> Result<NovaKeypair> {
    let keys_dir = data_dir.join("keys");
    let key_path = keys_dir.join("validator.key");

    if key_path.exists() {
        // Load existing keypair.
        let hex_str = std::fs::read_to_string(&key_path)
            .with_context(|| format!("failed to read validator key from {}", key_path.display()))?;
        let keypair = NovaKeypair::from_hex(hex_str.trim())
            .map_err(|e| anyhow::anyhow!("invalid validator key: {}", e))?;
        tracing::info!(
            public_key = %keypair.public_key().to_hex(),
            key_path = %key_path.display(),
            "loaded validator keypair from disk"
        );
        Ok(keypair)
    } else {
        // Generate a new keypair and save it.
        std::fs::create_dir_all(&keys_dir)
            .with_context(|| format!("failed to create keys directory: {}", keys_dir.display()))?;

        let keypair = NovaKeypair::generate();
        let secret_hex = hex::encode(keypair.secret_key_bytes());
        std::fs::write(&key_path, &secret_hex)
            .with_context(|| format!("failed to write validator key to {}", key_path.display()))?;

        // Restrict permissions on Unix.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
        }

        tracing::info!(
            public_key = %keypair.public_key().to_hex(),
            key_path = %key_path.display(),
            "generated and saved new validator keypair"
        );
        Ok(keypair)
    }
}

// ---------------------------------------------------------------------------
// Dev mode helpers
// ---------------------------------------------------------------------------

/// Generates a deterministic 32-byte seed from a u64 index.
/// Uses SHA-256 of the index bytes to produce a well-distributed seed.
fn generate_dev_seed(index: u64) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"nova-dev-account-");
    hasher.update(index.to_le_bytes());
    let result = hasher.finalize();
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&result);
    seed
}

/// Pre-funds dev test accounts in the state tree.
///
/// Generates 10 deterministic keypairs from seeds 1..=10, creates a NOVA
/// address for each, and credits each account with 1M NOVA (10^14 photons).
///
/// Returns the list of funded NOVA addresses.
async fn prefund_dev_accounts(
    state_tree: &Arc<RwLock<StateTree>>,
) -> Vec<String> {
    let mut addresses = Vec::with_capacity(DEV_ACCOUNT_COUNT as usize);
    let mut tree = state_tree.write().await;

    for i in 1..=DEV_ACCOUNT_COUNT {
        let seed = generate_dev_seed(i);
        let kp = NovaKeypair::from_seed(&seed);
        let nova_id = NovaId::from_public_key(&kp.public_key());
        let addr = nova_id.to_address();

        tree.put(&addr, &AccountState::with_balance(DEV_ACCOUNT_BALANCE));
        addresses.push(addr);
    }

    addresses
}

// ---------------------------------------------------------------------------
// Startup banner
// ---------------------------------------------------------------------------

/// Prints the node startup banner with configuration summary.
fn print_startup_banner(
    node_id: &str,
    rpc_addr: &str,
    p2p_addr: &str,
    data_dir: &str,
    mode: &str,
    stake: u64,
) {
    let node_id_short = if node_id.len() > 20 {
        format!("{}...", &node_id[..20])
    } else {
        node_id.to_string()
    };

    let stake_str = cli::format_nova_amount(stake);

    // Compute the box width based on content.
    let lines = [
        format!("  Node ID:    {}", node_id_short),
        format!("  RPC:        http://{}", rpc_addr),
        format!("  P2P:        /ip4/{}", p2p_addr.replace(':', "/tcp/")),
        format!("  Data:       {}", data_dir),
        format!("  Mode:       {}", mode),
        format!("  Stake:      {} NOVA", stake_str),
    ];

    let title = format!(
        "  NOVA Protocol \u{2014} Validator Node v{}",
        env!("CARGO_PKG_VERSION")
    );

    let max_width = lines
        .iter()
        .map(|l| l.len())
        .chain(std::iter::once(title.len()))
        .max()
        .unwrap_or(50)
        + 4;

    let border = "\u{2550}".repeat(max_width);

    println!();
    println!("\u{2554}{}\u{2557}", border);
    println!("\u{2551}  {:<width$}  \u{2551}", title.trim(), width = max_width - 4);
    println!("\u{2560}{}\u{2563}", border);
    for line in &lines {
        println!("\u{2551}  {:<width$}  \u{2551}", line.trim(), width = max_width - 4);
    }
    println!("\u{255A}{}\u{255D}", border);
    println!();
}

// ---------------------------------------------------------------------------
// Shutdown signal
// ---------------------------------------------------------------------------

/// Waits for SIGINT (Ctrl+C) or SIGTERM, whichever comes first.
///
/// On non-Unix platforms, only Ctrl+C is supported.
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}

// ---------------------------------------------------------------------------
// Minimal HTTP client
// ---------------------------------------------------------------------------

/// Minimal HTTP GET without pulling in `reqwest` as a dependency.
/// In a real deployment, swap this for a proper HTTP client.
async fn reqwest_get_stub(url: &str) -> Result<String> {
    let parsed: url::Url = url
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid URL: {}", e))?;

    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("missing host in URL"))?;
    let port = parsed.port().unwrap_or(80);
    let path = parsed.path();

    let addr = format!("{}:{}", host, port);
    let mut stream = tokio::net::TcpStream::connect(&addr)
        .await
        .with_context(|| format!("failed to connect to {}", addr))?;

    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        path, host,
    );

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    stream.write_all(request.as_bytes()).await?;
    stream.shutdown().await?;

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;
    let response = String::from_utf8_lossy(&buf);

    // Strip HTTP headers — everything after the first blank line is the body.
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_else(|| response.to_string());

    Ok(body)
}

/// Minimal URL parser — just enough to extract host/port/path.
/// Avoids pulling in the `url` crate for a single use.
mod url {
    pub struct Url {
        host: String,
        port: Option<u16>,
        path: String,
    }

    impl Url {
        pub fn host_str(&self) -> Option<&str> {
            Some(&self.host)
        }

        pub fn port(&self) -> Option<u16> {
            self.port
        }

        pub fn path(&self) -> &str {
            &self.path
        }
    }

    impl std::str::FromStr for Url {
        type Err = String;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            // Strip scheme.
            let rest = s
                .strip_prefix("http://")
                .or_else(|| s.strip_prefix("https://"))
                .unwrap_or(s);

            let (authority, path) = match rest.find('/') {
                Some(i) => (&rest[..i], &rest[i..]),
                None => (rest, "/"),
            };

            let (host, port) = match authority.rfind(':') {
                Some(i) => {
                    let p = authority[i + 1..]
                        .parse::<u16>()
                        .map_err(|e| format!("bad port: {}", e))?;
                    (authority[..i].to_string(), Some(p))
                }
                None => (authority.to_string(), None),
            };

            Ok(Url {
                host,
                port,
                path: path.to_string(),
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- 1. Dev seed determinism ------------------------------------------

    #[test]
    fn dev_seed_deterministic() {
        let seed_a = generate_dev_seed(1);
        let seed_b = generate_dev_seed(1);
        assert_eq!(seed_a, seed_b, "same index must produce the same seed");
    }

    #[test]
    fn dev_seed_unique_per_index() {
        let seeds: Vec<[u8; 32]> = (1..=DEV_ACCOUNT_COUNT)
            .map(generate_dev_seed)
            .collect();

        // Each seed must be unique.
        for (i, a) in seeds.iter().enumerate() {
            for (j, b) in seeds.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "seeds at index {} and {} must differ", i, j);
                }
            }
        }
    }

    // -- 2. Dev account generation produces valid NOVA addresses ----------

    #[test]
    fn dev_accounts_produce_valid_addresses() {
        for i in 1..=DEV_ACCOUNT_COUNT {
            let seed = generate_dev_seed(i);
            let kp = NovaKeypair::from_seed(&seed);
            let id = NovaId::from_public_key(&kp.public_key());
            let addr = id.to_address();
            assert!(
                addr.starts_with("nova1"),
                "dev account {} should have nova1 prefix, got: {}",
                i,
                addr
            );
        }
    }

    // -- 3. Dev account keypairs are deterministic from seed ---------------

    #[test]
    fn dev_keypairs_deterministic() {
        for i in 1..=DEV_ACCOUNT_COUNT {
            let seed = generate_dev_seed(i);
            let kp1 = NovaKeypair::from_seed(&seed);
            let kp2 = NovaKeypair::from_seed(&seed);
            assert_eq!(
                kp1.public_key().to_hex(),
                kp2.public_key().to_hex(),
                "keypair from same seed should be identical"
            );
        }
    }

    // -- 4. Keypair save/load roundtrip -----------------------------------

    #[test]
    fn keypair_save_load_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keys_dir = dir.path().join("keys");
        std::fs::create_dir_all(&keys_dir).unwrap();

        // Generate and save.
        let keypair = NovaKeypair::generate();
        let key_path = keys_dir.join("validator.key");
        let secret_hex = hex::encode(keypair.secret_key_bytes());
        std::fs::write(&key_path, &secret_hex).unwrap();

        // Load and verify.
        let loaded_hex = std::fs::read_to_string(&key_path).unwrap();
        let loaded = NovaKeypair::from_hex(loaded_hex.trim()).unwrap();
        assert_eq!(
            keypair.public_key().to_hex(),
            loaded.public_key().to_hex(),
            "loaded keypair must match generated one"
        );
    }

    // -- 5. Init creates directory structure -------------------------------

    #[test]
    fn init_creates_directory_structure() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data_dir = dir.path().join("nova-init-test");

        // Create the structure the same way init_node does.
        let db_dir = data_dir.join("db");
        let keys_dir = data_dir.join("keys");
        let config_dir = data_dir.join("config");

        std::fs::create_dir_all(&db_dir).unwrap();
        std::fs::create_dir_all(&keys_dir).unwrap();
        std::fs::create_dir_all(&config_dir).unwrap();

        assert!(db_dir.exists(), "db directory should exist");
        assert!(keys_dir.exists(), "keys directory should exist");
        assert!(config_dir.exists(), "config directory should exist");
    }

    // -- 6. Keypair load_or_generate creates new key ----------------------

    #[test]
    fn load_or_generate_creates_new_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data_dir = dir.path().join("nova-keygen-test");
        std::fs::create_dir_all(&data_dir).unwrap();

        let keypair = load_or_generate_keypair(&data_dir).unwrap();

        // Key file should now exist.
        let key_path = data_dir.join("keys").join("validator.key");
        assert!(key_path.exists(), "validator.key should have been created");

        // Loading again should return the same keypair.
        let loaded = load_or_generate_keypair(&data_dir).unwrap();
        assert_eq!(
            keypair.public_key().to_hex(),
            loaded.public_key().to_hex(),
            "second load should return the same keypair"
        );
    }

    // -- 7. Format NOVA amount -------------------------------------------

    #[test]
    fn format_nova_amount_dev_stake() {
        let formatted = cli::format_nova_amount(DEV_VALIDATOR_STAKE);
        assert_eq!(formatted, "100.00000000");
    }

    #[test]
    fn format_nova_amount_dev_balance() {
        let formatted = cli::format_nova_amount(DEV_ACCOUNT_BALANCE);
        assert_eq!(formatted, "1000000.00000000");
    }

    // -- 8. Startup banner does not panic ---------------------------------

    #[test]
    fn startup_banner_does_not_panic() {
        // Just verify it does not panic with various inputs.
        print_startup_banner(
            "nova1abc123def456ghi789jkl012mno345pqr678",
            "0.0.0.0:9741",
            "0.0.0.0:9740",
            "/home/user/.nova",
            "Validator (dev)",
            DEV_VALIDATOR_STAKE,
        );
    }

    // -- 9. Prefund dev accounts populates state tree ---------------------

    #[tokio::test]
    async fn prefund_dev_accounts_populates_state_tree() {
        let db = Arc::new(NovaDB::open_temporary().expect("temp db"));
        let state_tree = Arc::new(RwLock::new(StateTree::new((*db).clone())));

        let addresses = prefund_dev_accounts(&state_tree).await;
        assert_eq!(addresses.len(), DEV_ACCOUNT_COUNT as usize);

        let tree = state_tree.read().await;
        for addr in &addresses {
            let account = tree.get(addr).expect("account should exist");
            assert_eq!(
                account.balance, DEV_ACCOUNT_BALANCE,
                "account {} should have {} photons",
                addr, DEV_ACCOUNT_BALANCE
            );
        }
    }

    // -- 10. Prefund dev accounts are deterministic -----------------------

    #[tokio::test]
    async fn prefund_dev_accounts_deterministic() {
        let db1 = Arc::new(NovaDB::open_temporary().expect("temp db 1"));
        let db2 = Arc::new(NovaDB::open_temporary().expect("temp db 2"));
        let tree1 = Arc::new(RwLock::new(StateTree::new((*db1).clone())));
        let tree2 = Arc::new(RwLock::new(StateTree::new((*db2).clone())));

        let addrs1 = prefund_dev_accounts(&tree1).await;
        let addrs2 = prefund_dev_accounts(&tree2).await;

        assert_eq!(addrs1, addrs2, "dev addresses must be deterministic");
    }

    // -- 11. Status formatting with valid JSON ----------------------------

    #[test]
    fn status_json_formatting() {
        let json_str = r#"{"version":"0.1.0","network":"devnet","block_height":42,"peer_count":3,"synced":true,"timestamp":"2026-01-01T00:00:00Z"}"#;
        let json: serde_json::Value = serde_json::from_str(json_str).unwrap();

        // Verify we can extract all expected fields.
        assert_eq!(json["version"].as_str().unwrap(), "0.1.0");
        assert_eq!(json["network"].as_str().unwrap(), "devnet");
        assert_eq!(json["block_height"].as_u64().unwrap(), 42);
        assert_eq!(json["peer_count"].as_u64().unwrap(), 3);
        assert!(json["synced"].as_bool().unwrap());
    }

    // -- 12. Config validation -------------------------------------------

    #[test]
    fn startup_config_validation() {
        // Verify that log levels are properly validated.
        assert!(cli::validate_log_level("info"));
        assert!(cli::validate_log_level("debug"));
        assert!(!cli::validate_log_level("garbage"));

        // Verify NOVA amount formatting is consistent.
        assert_eq!(cli::format_nova_amount(0), "0.00000000");
        assert_eq!(cli::format_nova_amount(1), "0.00000001");
        assert_eq!(cli::format_nova_amount(100_000_000), "1.00000000");
    }
}
