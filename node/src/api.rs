//! # REST + WebSocket API
//!
//! Builds the axum router that exposes the validator node's HTTP interface.
//! All endpoints share application state through axum's `State` extractor.
//!
//! ## Endpoints
//!
//! | Method | Path                   | Description                         |
//! |--------|------------------------|-------------------------------------|
//! | GET    | `/health`              | Liveness probe                      |
//! | GET    | `/status`              | Node status summary                 |
//! | POST   | `/rpc`                 | JSON-RPC 2.0 gateway                |
//! | GET    | `/ws`                  | WebSocket for live block/tx updates |
//! | GET    | `/validators`          | Current validator set                |
//! | GET    | `/blocks/:height`      | Block by height                     |
//! | GET    | `/transactions/:hash`  | Transaction by hash                 |
//! | GET    | `/accounts/:address`   | Account state                       |

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::{Method, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::metrics::SharedMetrics;

// ---------------------------------------------------------------------------
// Application State
// ---------------------------------------------------------------------------

/// Shared application state available to all request handlers.
///
/// Cheap to clone — everything behind `Arc`.
#[derive(Clone)]
pub struct AppState {
    /// The node's reported version string.
    pub version: String,
    /// Network identifier (e.g., "devnet", "testnet", "mainnet").
    pub network: String,
    /// Current block height (updated by the consensus loop).
    pub block_height: Arc<std::sync::atomic::AtomicU64>,
    /// Number of connected peers (updated by the P2P layer).
    pub peer_count: Arc<std::sync::atomic::AtomicU64>,
    /// Broadcast channel for live event notifications (blocks, txs).
    pub event_tx: broadcast::Sender<NodeEvent>,
    /// Reference to Prometheus metrics for in-handler recording.
    pub metrics: SharedMetrics,
}

/// Events pushed to WebSocket subscribers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NodeEvent {
    /// A new block was finalized.
    #[serde(rename = "new_block")]
    NewBlock {
        height: u64,
        hash: String,
        tx_count: u64,
        timestamp: u64,
    },
    /// A new transaction entered the mempool.
    #[serde(rename = "new_transaction")]
    NewTransaction {
        hash: String,
        sender: String,
        recipient: String,
        amount: u64,
    },
}

// ---------------------------------------------------------------------------
// Router Construction
// ---------------------------------------------------------------------------

/// Builds the full axum [`Router`] with all API routes, CORS, and tracing.
///
/// The returned router is ready to be served on the configured RPC port.
pub fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(Any);

    Router::new()
        .route("/health", get(health_handler))
        .route("/status", get(status_handler))
        .route("/rpc", post(rpc_handler))
        .route("/ws", get(ws_handler))
        .route("/validators", get(validators_handler))
        .route("/blocks/{height}", get(block_by_height_handler))
        .route("/transactions/{hash}", get(transaction_by_hash_handler))
        .route("/accounts/{address}", get(account_handler))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

// ---------------------------------------------------------------------------
// JSON-RPC Types
// ---------------------------------------------------------------------------

/// A JSON-RPC 2.0 request envelope.
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    /// Protocol version. Must be "2.0".
    pub jsonrpc: String,
    /// The method to invoke.
    pub method: String,
    /// Method parameters (positional or named).
    pub params: Option<serde_json::Value>,
    /// Request identifier. Echoed back in the response.
    pub id: serde_json::Value,
}

/// A JSON-RPC 2.0 response envelope.
#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    /// Protocol version. Always "2.0".
    pub jsonrpc: String,
    /// The result on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// The error on failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    /// Request identifier, echoed from the request.
    pub id: serde_json::Value,
}

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    /// Numeric error code.
    pub code: i32,
    /// Short human-readable error description.
    pub message: String,
    /// Optional structured error data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Response Types
// ---------------------------------------------------------------------------

/// Response payload for `GET /status`.
#[derive(Debug, Serialize)]
pub struct StatusResponse {
    /// Node software version.
    pub version: String,
    /// Network identifier.
    pub network: String,
    /// Latest finalized block height.
    pub block_height: u64,
    /// Number of connected P2P peers.
    pub peer_count: u64,
    /// Whether the node considers itself synced.
    pub synced: bool,
    /// ISO-8601 timestamp of the response.
    pub timestamp: String,
}

/// Response payload for `GET /validators`.
#[derive(Debug, Serialize)]
pub struct ValidatorInfo {
    /// Hex-encoded public key.
    pub public_key: String,
    /// Validator stake in photons.
    pub stake: u64,
    /// Whether this validator is in the active set.
    pub active: bool,
    /// Last block height this validator proposed.
    pub last_proposed_block: u64,
}

/// Response payload for `GET /blocks/:height`.
#[derive(Debug, Serialize)]
pub struct BlockResponse {
    /// Block height.
    pub height: u64,
    /// Hex-encoded block hash.
    pub hash: String,
    /// Hex-encoded parent block hash.
    pub parent_hash: String,
    /// Hex-encoded proposer public key.
    pub proposer: String,
    /// Number of transactions in the block.
    pub tx_count: u64,
    /// Unix timestamp (milliseconds).
    pub timestamp: u64,
}

/// Response payload for `GET /transactions/:hash`.
#[derive(Debug, Serialize)]
pub struct TransactionResponse {
    /// Hex-encoded transaction ID.
    pub hash: String,
    /// Sender address.
    pub sender: String,
    /// Recipient address.
    pub recipient: String,
    /// Transfer amount in photons.
    pub amount: u64,
    /// Fee paid in photons.
    pub fee: u64,
    /// Block height (if confirmed).
    pub block_height: Option<u64>,
    /// Transaction status.
    pub status: String,
    /// Unix timestamp (milliseconds).
    pub timestamp: u64,
}

/// Response payload for `GET /accounts/:address`.
#[derive(Debug, Serialize)]
pub struct AccountResponse {
    /// Hex-encoded account address.
    pub address: String,
    /// Available balance in photons.
    pub balance: u64,
    /// Current nonce.
    pub nonce: u64,
    /// Number of transactions sent from this account.
    pub tx_count: u64,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /health` — returns 200 if the node is alive.
///
/// This is the liveness probe for orchestrators (k8s, systemd, etc.).
/// It intentionally does not check internal subsystem health — that
/// belongs in `/status`.
async fn health_handler() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" })))
}

/// `GET /status` — returns node status summary.
async fn status_handler(State(state): State<AppState>) -> impl IntoResponse {
    let height = state
        .block_height
        .load(std::sync::atomic::Ordering::Relaxed);
    let peers = state.peer_count.load(std::sync::atomic::Ordering::Relaxed);

    let resp = StatusResponse {
        version: state.version.clone(),
        network: state.network.clone(),
        block_height: height,
        peer_count: peers,
        synced: peers >= nova_protocol::config::MIN_PEERS_FOR_CONSENSUS as u64,
        timestamp: chrono::Utc::now().to_rfc3339(),
    };
    Json(resp)
}

/// `POST /rpc` — JSON-RPC 2.0 gateway.
///
/// Routes method calls to internal handlers. Unknown methods return
/// error code -32601 (Method not found).
async fn rpc_handler(
    State(state): State<AppState>,
    Json(req): Json<JsonRpcRequest>,
) -> impl IntoResponse {
    if req.jsonrpc != "2.0" {
        return Json(JsonRpcResponse {
            jsonrpc: "2.0".into(),
            result: None,
            error: Some(JsonRpcError {
                code: -32600,
                message: "Invalid Request: jsonrpc must be \"2.0\"".into(),
                data: None,
            }),
            id: req.id,
        });
    }

    let (result, error) = match req.method.as_str() {
        "nova_blockHeight" => {
            let height = state
                .block_height
                .load(std::sync::atomic::Ordering::Relaxed);
            (Some(serde_json::json!(height)), None)
        }
        "nova_peerCount" => {
            let peers = state.peer_count.load(std::sync::atomic::Ordering::Relaxed);
            (Some(serde_json::json!(peers)), None)
        }
        "nova_networkId" => (Some(serde_json::json!(state.network)), None),
        "nova_version" => (Some(serde_json::json!(state.version)), None),
        "nova_getBlock" => {
            // Expects params: [height: u64]
            let height = req
                .params
                .as_ref()
                .and_then(|p| p.as_array())
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_u64());

            match height {
                Some(h) => {
                    let block = BlockResponse {
                        height: h,
                        hash: format!("{:064x}", h),
                        parent_hash: format!("{:064x}", h.saturating_sub(1)),
                        proposer: "0".repeat(64),
                        tx_count: 0,
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    };
                    (Some(serde_json::to_value(block).unwrap()), None)
                }
                None => (
                    None,
                    Some(JsonRpcError {
                        code: -32602,
                        message: "Invalid params: expected [height]".into(),
                        data: None,
                    }),
                ),
            }
        }
        "nova_getTransaction" => {
            let hash = req
                .params
                .as_ref()
                .and_then(|p| p.as_array())
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            match hash {
                Some(h) => {
                    // In production this would look up the tx in storage.
                    let tx = TransactionResponse {
                        hash: h,
                        sender: "0".repeat(64),
                        recipient: "0".repeat(64),
                        amount: 0,
                        fee: 0,
                        block_height: None,
                        status: "unknown".into(),
                        timestamp: 0,
                    };
                    (Some(serde_json::to_value(tx).unwrap()), None)
                }
                None => (
                    None,
                    Some(JsonRpcError {
                        code: -32602,
                        message: "Invalid params: expected [hash]".into(),
                        data: None,
                    }),
                ),
            }
        }
        _ => (
            None,
            Some(JsonRpcError {
                code: -32601,
                message: format!("Method not found: {}", req.method),
                data: None,
            }),
        ),
    };

    Json(JsonRpcResponse {
        jsonrpc: "2.0".into(),
        result,
        error,
        id: req.id,
    })
}

/// `GET /ws` — WebSocket upgrade for live event streaming.
///
/// Clients receive JSON-encoded [`NodeEvent`] messages for each new block
/// and transaction. The connection is read-only from the server's
/// perspective; client messages are ignored.
async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws_connection(socket, state))
}

/// Drives a single WebSocket connection, forwarding broadcast events
/// until the client disconnects or the channel is closed.
async fn handle_ws_connection(mut socket: WebSocket, state: AppState) {
    let mut rx = state.event_tx.subscribe();

    loop {
        tokio::select! {
            event = rx.recv() => {
                match event {
                    Ok(ev) => {
                        let payload = match serde_json::to_string(&ev) {
                            Ok(s) => s,
                            Err(e) => {
                                tracing::warn!("failed to serialize ws event: {}", e);
                                continue;
                            }
                        };
                        if socket.send(Message::Text(payload.into())).await.is_err() {
                            // Client disconnected.
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("ws subscriber lagged by {} events", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(_)) => {
                        // Client messages are ignored — this is a push-only channel.
                    }
                    _ => break, // Disconnected or error.
                }
            }
        }
    }
}

/// `GET /validators` — returns the current validator set.
///
/// In production, this reads from the consensus module's active validator
/// list. The stub returns a static placeholder set.
async fn validators_handler(State(state): State<AppState>) -> impl IntoResponse {
    let _ = &state;
    let validators = vec![ValidatorInfo {
        public_key: "0".repeat(64),
        stake: 100_000_000_000,
        active: true,
        last_proposed_block: 0,
    }];
    Json(validators)
}

/// `GET /blocks/:height` — returns a block by its height.
async fn block_by_height_handler(
    Path(height): Path<u64>,
    State(_state): State<AppState>,
) -> impl IntoResponse {
    // In production, look up the block in storage.
    let block = BlockResponse {
        height,
        hash: format!("{:064x}", height),
        parent_hash: format!("{:064x}", height.saturating_sub(1)),
        proposer: "0".repeat(64),
        tx_count: 0,
        timestamp: chrono::Utc::now().timestamp_millis() as u64,
    };
    Json(block)
}

/// `GET /transactions/:hash` — returns a transaction by its hex-encoded hash.
async fn transaction_by_hash_handler(
    Path(hash): Path<String>,
    State(_state): State<AppState>,
) -> impl IntoResponse {
    // In production, look up the transaction in storage.
    let tx = TransactionResponse {
        hash,
        sender: "0".repeat(64),
        recipient: "0".repeat(64),
        amount: 0,
        fee: 0,
        block_height: None,
        status: "unknown".into(),
        timestamp: 0,
    };
    Json(tx)
}

/// `GET /accounts/:address` — returns account state for the given address.
async fn account_handler(
    Path(address): Path<String>,
    State(_state): State<AppState>,
) -> impl IntoResponse {
    // In production, look up the account in the state trie.
    let account = AccountResponse {
        address,
        balance: 0,
        nonce: 0,
        tx_count: 0,
    };
    Json(account)
}
