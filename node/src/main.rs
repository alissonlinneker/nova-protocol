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

use nova_protocol::storage::db::NovaDB;
use nova_protocol::storage::state::StateTree;

use cli::{Commands, NovaNodeCli};
use logging::LogFormat;
use metrics::NodeMetrics;

/// Broadcast channel capacity for live event streaming.
/// 256 is large enough to absorb short bursts without dropping events
/// for connected WebSocket clients.
const EVENT_CHANNEL_CAPACITY: usize = 256;

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

/// Starts the full validator node: API server, metrics endpoint, and
/// consensus participation.
async fn run_node(args: cli::RunArgs) -> Result<()> {
    logging::init_logging(
        "nova_node=info,nova_protocol=info,tower_http=debug",
        LogFormat::Pretty,
    );

    tracing::info!(
        rpc_port = args.rpc_port,
        p2p_port = args.p2p_port,
        metrics_port = args.metrics_port,
        data_dir = %args.data_dir.display(),
        "starting nova-node"
    );

    // --- Persistent storage ---
    let db_path = args.data_dir.join("db");
    std::fs::create_dir_all(&db_path)
        .with_context(|| format!("failed to create database directory: {}", db_path.display()))?;

    let db = Arc::new(
        NovaDB::open(&db_path)
            .with_context(|| format!("failed to open database at {}", db_path.display()))?,
    );
    tracing::info!(path = %db_path.display(), "database opened");

    // --- State tree ---
    let state_tree = Arc::new(RwLock::new(StateTree::new((*db).clone())));

    // --- Metrics ---
    let node_metrics = Arc::new(NodeMetrics::new());

    // --- Event broadcast ---
    let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);

    // --- Block height ---
    let block_height = Arc::new(std::sync::atomic::AtomicU64::new(0));

    // --- Genesis initialization ---
    api::initialize_genesis(&db, &block_height);

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

    // --- API server ---
    let api_router = api::create_router(app_state.clone());
    let api_addr = format!("0.0.0.0:{}", args.rpc_port);
    let api_listener = tokio::net::TcpListener::bind(&api_addr)
        .await
        .with_context(|| format!("failed to bind RPC listener on {}", api_addr))?;
    tracing::info!("RPC/API server listening on {}", api_addr);

    // --- Metrics server ---
    let metrics_router = axum::Router::new()
        .route("/metrics", axum::routing::get(metrics::metrics_handler))
        .with_state(Arc::clone(&node_metrics));
    let metrics_addr = format!("0.0.0.0:{}", args.metrics_port);
    let metrics_listener = tokio::net::TcpListener::bind(&metrics_addr)
        .await
        .with_context(|| format!("failed to bind metrics listener on {}", metrics_addr))?;
    tracing::info!("Metrics server listening on {}", metrics_addr);

    // --- Block production stub ---
    // In production, this would be replaced by the consensus engine. For now,
    // we run a simple loop that increments the block height every BLOCK_TIME_MS
    // to exercise the metrics pipeline and WebSocket broadcasting.
    let height_ref = Arc::clone(&app_state.block_height);
    let metrics_ref = Arc::clone(&node_metrics);
    let event_tx_ref = event_tx.clone();
    let block_loop = tokio::spawn(async move {
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

            tracing::debug!(height = h, "block produced");
        }
    });

    // --- Serve ---
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

    block_loop.abort();
    tracing::info!("nova-node stopped");
    Ok(())
}

/// Initializes a new node data directory and generates a validator keypair.
fn init_node(args: cli::InitArgs) -> Result<()> {
    logging::init_logging("nova_node=info", LogFormat::Pretty);

    let data_dir = &args.data_dir;
    tracing::info!(data_dir = %data_dir.display(), network = %args.network, "initializing node");

    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("failed to create data directory: {}", data_dir.display()))?;

    // Generate validator keypair.
    let keypair = nova_protocol::identity::NovaKeypair::generate();
    let pubkey_hex = keypair.public_key().to_hex();

    // Write the secret key to a file inside the data directory.
    let key_path = data_dir.join("validator.key");
    let secret_bytes = keypair.secret_key_bytes();
    std::fs::write(&key_path, hex::encode(secret_bytes))
        .with_context(|| format!("failed to write validator key to {}", key_path.display()))?;

    // Restrict permissions on Unix.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
    }

    tracing::info!(
        public_key = %pubkey_hex,
        key_path = %key_path.display(),
        "validator keypair generated"
    );

    println!("Node initialized successfully.");
    println!("  Data directory : {}", data_dir.display());
    println!("  Network        : {}", args.network);
    println!("  Validator key  : {}", key_path.display());
    println!("  Public key     : {}", pubkey_hex);

    Ok(())
}

/// Queries a running node's status endpoint and prints the result.
async fn query_status(args: cli::StatusArgs) -> Result<()> {
    let url = format!("{}/status", args.rpc_url.trim_end_matches('/'));
    let body: String = reqwest_get_stub(&url).await?;
    println!("{}", body);
    Ok(())
}

/// Minimal HTTP GET without pulling in `reqwest` as a dependency.
/// In a real deployment, swap this for a proper HTTP client.
async fn reqwest_get_stub(url: &str) -> Result<String> {
    // Use tokio's TCP stream + raw HTTP/1.1 to avoid adding reqwest.
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

/// Prints version information to stdout.
fn print_version() {
    println!("nova-node {}", env!("CARGO_PKG_VERSION"));
    println!("protocol  {}", nova_protocol::config::PROTOCOL_VERSION);
    println!("rustc     {}", rustc_version());
}

/// Returns the Rust compiler version used to build this binary.
fn rustc_version() -> &'static str {
    option_env!("RUSTC_VERSION").unwrap_or("unknown")
}

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
