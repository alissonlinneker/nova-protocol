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
use tokio::sync::{broadcast, RwLock};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use nova_protocol::storage::db::NovaDB;
use nova_protocol::storage::state::StateTree;

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
    /// Persistent storage engine for blocks, transactions, and accounts.
    pub db: Arc<NovaDB>,
    /// Sparse Merkle Tree for account state lookups and proofs.
    pub state_tree: Arc<RwLock<StateTree>>,
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
        .route("/blocks/:height", get(block_by_height_handler))
        .route("/transactions/:hash", get(transaction_by_hash_handler))
        .route("/accounts/:address", get(account_handler))
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
#[derive(Debug, Serialize, Deserialize)]
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
#[derive(Debug, Serialize, Deserialize)]
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
#[derive(Debug, Serialize, Deserialize)]
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
#[derive(Debug, Serialize, Deserialize)]
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
#[derive(Debug, Serialize, Deserialize)]
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
#[derive(Debug, Serialize, Deserialize)]
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
#[derive(Debug, Serialize, Deserialize)]
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

/// Generic error body returned by REST endpoints on failure.
#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
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
///
/// Reads the latest block height from NovaDB for ground truth, falling
/// back to the in-memory atomic counter if the DB is unreachable.
async fn status_handler(State(state): State<AppState>) -> impl IntoResponse {
    let height = match state.db.get_latest_block_height() {
        Ok(Some(h)) => h,
        _ => state
            .block_height
            .load(std::sync::atomic::Ordering::Relaxed),
    };

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
            let height = match state.db.get_latest_block_height() {
                Ok(Some(h)) => h,
                _ => state
                    .block_height
                    .load(std::sync::atomic::Ordering::Relaxed),
            };
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
                Some(h) => match state.db.get_block(h) {
                    Ok(Some(block)) => {
                        let resp = BlockResponse {
                            height: block.header.height,
                            hash: block.header.hash_hex(),
                            parent_hash: block.header.parent_hash_hex(),
                            proposer: block.header.validator.clone(),
                            tx_count: block.transactions.len() as u64,
                            timestamp: block.header.timestamp,
                        };
                        (Some(serde_json::to_value(resp).unwrap()), None)
                    }
                    Ok(None) => (
                        None,
                        Some(JsonRpcError {
                            code: -32001,
                            message: format!("Block not found at height {}", h),
                            data: None,
                        }),
                    ),
                    Err(e) => (
                        None,
                        Some(JsonRpcError {
                            code: -32603,
                            message: format!("Internal error: {}", e),
                            data: None,
                        }),
                    ),
                },
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
                Some(h) => match state.db.get_transaction(&h) {
                    Ok(Some(tx)) => {
                        let resp = TransactionResponse {
                            hash: tx.id.clone(),
                            sender: tx.sender.clone(),
                            recipient: tx.receiver.clone(),
                            amount: tx.amount.value,
                            fee: tx.fee,
                            block_height: None, // Would require reverse index
                            status: "confirmed".into(),
                            timestamp: tx.timestamp,
                        };
                        (Some(serde_json::to_value(resp).unwrap()), None)
                    }
                    Ok(None) => (
                        None,
                        Some(JsonRpcError {
                            code: -32001,
                            message: format!("Transaction not found: {}", h),
                            data: None,
                        }),
                    ),
                    Err(e) => (
                        None,
                        Some(JsonRpcError {
                            code: -32603,
                            message: format!("Internal error: {}", e),
                            data: None,
                        }),
                    ),
                },
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
/// TODO: Wire to the consensus module's active validator list once the
/// validator registry is implemented. Currently returns a static
/// placeholder set for API contract stability.
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
///
/// Fetches the block from NovaDB. Returns 404 if no block exists at
/// the requested height.
async fn block_by_height_handler(
    Path(height): Path<u64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.db.get_block(height) {
        Ok(Some(block)) => {
            let resp = BlockResponse {
                height: block.header.height,
                hash: block.header.hash_hex(),
                parent_hash: block.header.parent_hash_hex(),
                proposer: block.header.validator.clone(),
                tx_count: block.transactions.len() as u64,
                timestamp: block.header.timestamp,
            };
            (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response()
        }
        Ok(None) => {
            let err = ErrorResponse {
                error: format!("Block not found at height {}", height),
            };
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::to_value(err).unwrap()),
            )
                .into_response()
        }
        Err(e) => {
            let err = ErrorResponse {
                error: format!("Database error: {}", e),
            };
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::to_value(err).unwrap()),
            )
                .into_response()
        }
    }
}

/// `GET /transactions/:hash` — returns a transaction by its hex-encoded hash.
///
/// Fetches the transaction from NovaDB. Returns 404 if no matching
/// transaction exists.
async fn transaction_by_hash_handler(
    Path(hash): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.db.get_transaction(&hash) {
        Ok(Some(tx)) => {
            let resp = TransactionResponse {
                hash: tx.id.clone(),
                sender: tx.sender.clone(),
                recipient: tx.receiver.clone(),
                amount: tx.amount.value,
                fee: tx.fee,
                block_height: None, // Would require a reverse index (tx -> block height)
                status: "confirmed".into(),
                timestamp: tx.timestamp,
            };
            (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response()
        }
        Ok(None) => {
            let err = ErrorResponse {
                error: format!("Transaction not found: {}", hash),
            };
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::to_value(err).unwrap()),
            )
                .into_response()
        }
        Err(e) => {
            let err = ErrorResponse {
                error: format!("Database error: {}", e),
            };
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::to_value(err).unwrap()),
            )
                .into_response()
        }
    }
}

/// `GET /accounts/:address` — returns account state for the given address.
///
/// Queries the StateTree for the account. Returns a default (zeroed)
/// account response for addresses that have never appeared on-chain.
async fn account_handler(
    Path(address): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let tree = state.state_tree.read().await;
    let account_state = tree.get(&address);
    drop(tree);

    let (balance, nonce) = match account_state {
        Some(acct) => (acct.balance, acct.nonce),
        None => (0, 0),
    };

    let account = AccountResponse {
        address,
        balance,
        nonce,
        tx_count: nonce, // Nonce tracks the number of outbound transactions.
    };
    Json(account)
}

// ---------------------------------------------------------------------------
// Genesis Initialization
// ---------------------------------------------------------------------------

/// Ensures the genesis block exists in the database.
///
/// If the DB is empty (no latest block height), creates and persists the
/// genesis block and updates the in-memory block height counter. This is
/// idempotent — calling it on an already-initialized DB is a no-op.
pub fn initialize_genesis(db: &NovaDB, block_height: &std::sync::atomic::AtomicU64) {
    match db.get_latest_block_height() {
        Ok(Some(h)) => {
            // DB already has blocks. Sync the in-memory counter.
            block_height.store(h, std::sync::atomic::Ordering::Relaxed);
            tracing::info!(height = h, "database loaded, latest block height synced");
        }
        Ok(None) => {
            // Empty DB — persist the genesis block.
            let genesis = nova_protocol::storage::block::Block::genesis();
            if let Err(e) = db.put_block(&genesis) {
                tracing::error!("failed to persist genesis block: {}", e);
                return;
            }
            block_height.store(0, std::sync::atomic::Ordering::Relaxed);
            tracing::info!("genesis block persisted at height 0");
        }
        Err(e) => {
            tracing::error!("failed to read latest block height from DB: {}", e);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use nova_protocol::storage::block::Block;
    use nova_protocol::storage::db::NovaDB;
    use nova_protocol::storage::state::{AccountState, StateTree};
    use nova_protocol::transaction::builder::TransactionBuilder;
    use nova_protocol::transaction::types::{Amount, Currency, TransactionType};
    use std::sync::Arc;
    use tower::ServiceExt;

    /// Creates a test AppState backed by a temporary in-memory database.
    fn test_app_state() -> AppState {
        let db = Arc::new(NovaDB::open_temporary().expect("temp db"));
        let state_tree = Arc::new(RwLock::new(StateTree::new((*db).clone())));
        let (event_tx, _) = broadcast::channel(16);
        let metrics = Arc::new(crate::metrics::NodeMetrics::new());

        AppState {
            version: "0.1.0-test".into(),
            network: "devnet".into(),
            block_height: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            peer_count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            event_tx,
            metrics,
            db,
            state_tree,
        }
    }

    /// Creates a test AppState and persists the genesis block.
    fn test_app_state_with_genesis() -> AppState {
        let state = test_app_state();
        let genesis = Block::genesis();
        state.db.put_block(&genesis).expect("persist genesis");
        state
            .block_height
            .store(0, std::sync::atomic::Ordering::Relaxed);
        state
    }

    /// Helper to build a test transaction.
    fn make_test_tx(nonce: u64) -> nova_protocol::transaction::Transaction {
        TransactionBuilder::new(TransactionType::Transfer)
            .sender("nova1alice")
            .receiver("nova1bob")
            .amount(Amount::new(500, Currency::NOVA))
            .fee(10)
            .nonce(nonce)
            .timestamp(1_000_000)
            .build()
    }

    /// Sends a GET request and returns the (status, body_bytes).
    async fn get(router: &Router, path: &str) -> (StatusCode, Vec<u8>) {
        let req = Request::builder().uri(path).body(Body::empty()).unwrap();
        let resp = router.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let body = resp
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec();
        (status, body)
    }

    /// Sends a POST request with JSON body and returns (status, body_bytes).
    async fn post_json(
        router: &Router,
        path: &str,
        body: serde_json::Value,
    ) -> (StatusCode, Vec<u8>) {
        let req = Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = router.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let body = resp
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec();
        (status, body)
    }

    // -- 1. Health endpoint still works --------------------------------------

    #[tokio::test]
    async fn health_endpoint_returns_ok() {
        let state = test_app_state();
        let router = create_router(state);
        let (status, body) = get(&router, "/health").await;

        assert_eq!(status, StatusCode::OK);
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
    }

    // -- 2. Status endpoint returns real block height ------------------------

    #[tokio::test]
    async fn status_endpoint_returns_real_block_height() {
        let state = test_app_state_with_genesis();

        // Persist a second block so height = 1.
        let genesis = Block::genesis();
        let block1 = Block::new(&genesis, vec![], "nova:validator".into(), [1u8; 32]);
        state.db.put_block(&block1).expect("persist block 1");

        let router = create_router(state);
        let (status, body) = get(&router, "/status").await;

        assert_eq!(status, StatusCode::OK);
        let resp: StatusResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(resp.block_height, 1);
        assert_eq!(resp.network, "devnet");
    }

    // -- 3. Block endpoint returns genesis block -----------------------------

    #[tokio::test]
    async fn block_endpoint_returns_genesis() {
        let state = test_app_state_with_genesis();
        let router = create_router(state);
        let (status, body) = get(&router, "/blocks/0").await;

        assert_eq!(status, StatusCode::OK);
        let resp: BlockResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(resp.height, 0);
        assert_eq!(resp.tx_count, 0);
        // Genesis has all-zero parent hash.
        assert_eq!(resp.parent_hash, hex::encode([0u8; 32]));
    }

    // -- 4. Block endpoint returns 404 for missing block ---------------------

    #[tokio::test]
    async fn block_endpoint_returns_404_for_missing() {
        let state = test_app_state_with_genesis();
        let router = create_router(state);
        let (status, body) = get(&router, "/blocks/999").await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        let err: ErrorResponse = serde_json::from_slice(&body).unwrap();
        assert!(err.error.contains("not found"));
    }

    // -- 5. Transaction endpoint returns 404 for missing tx ------------------

    #[tokio::test]
    async fn transaction_endpoint_returns_404_for_missing() {
        let state = test_app_state_with_genesis();
        let router = create_router(state);
        let (status, body) = get(&router, "/transactions/deadbeef").await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        let err: ErrorResponse = serde_json::from_slice(&body).unwrap();
        assert!(err.error.contains("not found"));
    }

    // -- 6. Transaction endpoint returns real data for known tx ---------------

    #[tokio::test]
    async fn transaction_endpoint_returns_real_data() {
        let state = test_app_state_with_genesis();
        let tx = make_test_tx(1);
        let tx_id = tx.id.clone();
        state.db.put_transaction(&tx).expect("persist tx");

        let router = create_router(state);
        let (status, body) = get(&router, &format!("/transactions/{}", tx_id)).await;

        assert_eq!(status, StatusCode::OK);
        let resp: TransactionResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(resp.hash, tx_id);
        assert_eq!(resp.sender, "nova1alice");
        assert_eq!(resp.recipient, "nova1bob");
        assert_eq!(resp.amount, 500);
        assert_eq!(resp.fee, 10);
    }

    // -- 7. Account endpoint returns default for unknown address --------------

    #[tokio::test]
    async fn account_endpoint_returns_default_for_unknown() {
        let state = test_app_state_with_genesis();
        let router = create_router(state);
        let (status, body) = get(&router, "/accounts/nova1nobody").await;

        assert_eq!(status, StatusCode::OK);
        let resp: AccountResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(resp.address, "nova1nobody");
        assert_eq!(resp.balance, 0);
        assert_eq!(resp.nonce, 0);
    }

    // -- 8. Account endpoint returns real data for known address ---------------

    #[tokio::test]
    async fn account_endpoint_returns_real_data() {
        let state = test_app_state_with_genesis();

        // Populate an account in the state tree.
        {
            let mut tree = state.state_tree.write().await;
            tree.put("nova1alice", &AccountState::with_balance(42_000));
        }

        let router = create_router(state);
        let (status, body) = get(&router, "/accounts/nova1alice").await;

        assert_eq!(status, StatusCode::OK);
        let resp: AccountResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(resp.address, "nova1alice");
        assert_eq!(resp.balance, 42_000);
        assert_eq!(resp.nonce, 0);
    }

    // -- 9. JSON-RPC nova_blockHeight returns real value -----------------------

    #[tokio::test]
    async fn rpc_block_height_returns_real_value() {
        let state = test_app_state_with_genesis();

        // Add a block so height = 1.
        let genesis = Block::genesis();
        let block1 = Block::new(&genesis, vec![], "nova:v".into(), [1u8; 32]);
        state.db.put_block(&block1).expect("persist block 1");

        let router = create_router(state);
        let rpc_body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "nova_blockHeight",
            "params": [],
            "id": 1
        });
        let (status, body) = post_json(&router, "/rpc", rpc_body).await;

        assert_eq!(status, StatusCode::OK);
        let resp: JsonRpcResponse = serde_json::from_slice(&body).unwrap();
        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap(), serde_json::json!(1));
    }

    // -- 10. JSON-RPC nova_getBlock returns real block -------------------------

    #[tokio::test]
    async fn rpc_get_block_returns_real_block() {
        let state = test_app_state_with_genesis();
        let router = create_router(state);

        let rpc_body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "nova_getBlock",
            "params": [0],
            "id": 2
        });
        let (status, body) = post_json(&router, "/rpc", rpc_body).await;

        assert_eq!(status, StatusCode::OK);
        let resp: JsonRpcResponse = serde_json::from_slice(&body).unwrap();
        assert!(resp.error.is_none());

        let block: BlockResponse = serde_json::from_value(resp.result.unwrap()).unwrap();
        assert_eq!(block.height, 0);
        assert_eq!(block.tx_count, 0);
    }

    // -- 11. JSON-RPC nova_getBlock returns error for missing block ------------

    #[tokio::test]
    async fn rpc_get_block_returns_error_for_missing() {
        let state = test_app_state_with_genesis();
        let router = create_router(state);

        let rpc_body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "nova_getBlock",
            "params": [999],
            "id": 3
        });
        let (status, body) = post_json(&router, "/rpc", rpc_body).await;

        assert_eq!(status, StatusCode::OK);
        let resp: JsonRpcResponse = serde_json::from_slice(&body).unwrap();
        assert!(resp.result.is_none());
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32001);
    }

    // -- 12. JSON-RPC nova_getTransaction returns error for missing tx --------

    #[tokio::test]
    async fn rpc_get_transaction_returns_error_for_missing() {
        let state = test_app_state_with_genesis();
        let router = create_router(state);

        let rpc_body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "nova_getTransaction",
            "params": ["deadbeefcafebabe"],
            "id": 4
        });
        let (status, body) = post_json(&router, "/rpc", rpc_body).await;

        assert_eq!(status, StatusCode::OK);
        let resp: JsonRpcResponse = serde_json::from_slice(&body).unwrap();
        assert!(resp.result.is_none());
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32001);
    }

    // -- 13. JSON-RPC nova_getTransaction returns real tx data -----------------

    #[tokio::test]
    async fn rpc_get_transaction_returns_real_data() {
        let state = test_app_state_with_genesis();
        let tx = make_test_tx(7);
        let tx_id = tx.id.clone();
        state.db.put_transaction(&tx).expect("persist tx");

        let router = create_router(state);
        let rpc_body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "nova_getTransaction",
            "params": [tx_id],
            "id": 5
        });
        let (status, body) = post_json(&router, "/rpc", rpc_body).await;

        assert_eq!(status, StatusCode::OK);
        let resp: JsonRpcResponse = serde_json::from_slice(&body).unwrap();
        assert!(resp.error.is_none());

        let tx_resp: TransactionResponse = serde_json::from_value(resp.result.unwrap()).unwrap();
        assert_eq!(tx_resp.hash, tx_id);
        assert_eq!(tx_resp.sender, "nova1alice");
    }

    // -- 14. Genesis initialization on empty DB --------------------------------

    #[tokio::test]
    async fn initialize_genesis_persists_block_zero() {
        let db = Arc::new(NovaDB::open_temporary().expect("temp db"));
        let block_height = std::sync::atomic::AtomicU64::new(u64::MAX);

        // DB is empty, genesis should be created.
        initialize_genesis(&db, &block_height);

        assert_eq!(block_height.load(std::sync::atomic::Ordering::Relaxed), 0);
        let genesis = db.get_block(0).unwrap().expect("genesis should exist");
        assert_eq!(genesis.header.height, 0);
    }

    // -- 15. Genesis initialization is idempotent on populated DB --------------

    #[tokio::test]
    async fn initialize_genesis_is_idempotent() {
        let db = Arc::new(NovaDB::open_temporary().expect("temp db"));
        let block_height = std::sync::atomic::AtomicU64::new(0);

        // Manually persist genesis + one more block.
        let genesis = Block::genesis();
        db.put_block(&genesis).unwrap();
        let block1 = Block::new(&genesis, vec![], "nova:v".into(), [1u8; 32]);
        db.put_block(&block1).unwrap();

        // initialize_genesis should detect existing data and sync height.
        initialize_genesis(&db, &block_height);
        assert_eq!(block_height.load(std::sync::atomic::Ordering::Relaxed), 1);

        // DB should still have 2 blocks, not be overwritten.
        assert_eq!(db.block_count(), 2);
    }

    // -- 16. Block with transactions is returned correctly --------------------

    #[tokio::test]
    async fn block_with_transactions_returns_correct_count() {
        let state = test_app_state_with_genesis();
        let genesis = Block::genesis();
        let tx1 = make_test_tx(1);
        let tx2 = make_test_tx(2);
        let block1 = Block::new(&genesis, vec![tx1, tx2], "nova:validator".into(), [1u8; 32]);
        state.db.put_block(&block1).expect("persist block 1");

        let router = create_router(state);
        let (status, body) = get(&router, "/blocks/1").await;

        assert_eq!(status, StatusCode::OK);
        let resp: BlockResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(resp.height, 1);
        assert_eq!(resp.tx_count, 2);
    }

    // -- 17. JSON-RPC version and networkId return config values ---------------

    #[tokio::test]
    async fn rpc_version_and_network_id() {
        let state = test_app_state_with_genesis();
        let router = create_router(state);

        // nova_version
        let rpc_body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "nova_version",
            "params": [],
            "id": 10
        });
        let (_, body) = post_json(&router, "/rpc", rpc_body).await;
        let resp: JsonRpcResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(resp.result.unwrap(), "0.1.0-test");

        // nova_networkId
        let rpc_body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "nova_networkId",
            "params": [],
            "id": 11
        });
        let (_, body) = post_json(&router, "/rpc", rpc_body).await;
        let resp: JsonRpcResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(resp.result.unwrap(), "devnet");
    }

    // -- 18. JSON-RPC invalid version returns error ---------------------------

    #[tokio::test]
    async fn rpc_invalid_version_returns_error() {
        let state = test_app_state_with_genesis();
        let router = create_router(state);

        let rpc_body = serde_json::json!({
            "jsonrpc": "1.0",
            "method": "nova_blockHeight",
            "params": [],
            "id": 20
        });
        let (_, body) = post_json(&router, "/rpc", rpc_body).await;
        let resp: JsonRpcResponse = serde_json::from_slice(&body).unwrap();
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32600);
    }
}
