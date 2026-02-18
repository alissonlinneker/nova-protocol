//! # JSON-RPC API Definitions
//!
//! Type-safe definitions for the NOVA JSON-RPC API. This module defines the
//! request/response types and method enumeration — the actual HTTP server
//! implementation lives in the node binary (using axum).
//!
//! The API follows the JSON-RPC 2.0 specification with NOVA-specific method
//! names prefixed with `nova_`. This convention avoids collisions with other
//! JSON-RPC services that might run on the same node.
//!
//! ## Method Index
//!
//! | Method                     | Description                           |
//! |---------------------------|---------------------------------------|
//! | `nova_getBalance`          | Query token balance for an address    |
//! | `nova_sendTransaction`     | Submit a signed transaction           |
//! | `nova_getTransaction`      | Retrieve a transaction by hash/ID     |
//! | `nova_getBlock`            | Retrieve a block by height or hash    |
//! | `nova_getBlockHeight`      | Current chain height                  |
//! | `nova_getAccountState`     | Full account state (balance, nonce, etc.) |
//! | `nova_getValidators`       | Active validator set                  |
//! | `nova_estimateFee`         | Estimate fee for a transaction        |
//! | `nova_getCreditOffers`     | Query available credit offers         |

use serde::{Deserialize, Serialize};

use crate::network::consensus::ValidatorInfo;

// ---------------------------------------------------------------------------
// RPC Method Enumeration
// ---------------------------------------------------------------------------

/// Supported JSON-RPC methods.
///
/// Each variant corresponds to a specific API endpoint. The method name
/// on the wire uses the string representation (e.g., `"nova_getBalance"`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RpcMethod {
    /// Query the balance of an address for a specific token.
    /// Parameters: `(address: String, token_id: String)`
    #[serde(rename = "nova_getBalance")]
    GetBalance,
    /// Submit a signed transaction to the mempool.
    /// Parameters: `(signed_tx: Transaction)`
    #[serde(rename = "nova_sendTransaction")]
    SendTransaction,
    /// Retrieve a transaction by its ID (hex-encoded hash).
    /// Parameters: `(tx_hash: String)`
    #[serde(rename = "nova_getTransaction")]
    GetTransaction,
    /// Retrieve a block by height (u64) or hash (hex string).
    /// Parameters: `(height_or_hash: String)`
    #[serde(rename = "nova_getBlock")]
    GetBlock,
    /// Get the current chain height.
    /// Parameters: none.
    #[serde(rename = "nova_getBlockHeight")]
    GetBlockHeight,
    /// Get the full account state for an address.
    /// Parameters: `(address: String)`
    #[serde(rename = "nova_getAccountState")]
    GetAccountState,
    /// Get the current active validator set.
    /// Parameters: none.
    #[serde(rename = "nova_getValidators")]
    GetValidators,
    /// Estimate the fee for a transaction.
    /// Parameters: `(tx: Transaction)`
    #[serde(rename = "nova_estimateFee")]
    EstimateFee,
    /// Get available credit offers for an address and amount.
    /// Parameters: `(address: String, amount: u64)`
    #[serde(rename = "nova_getCreditOffers")]
    GetCreditOffers,
}

// ---------------------------------------------------------------------------
// RPC Request / Response
// ---------------------------------------------------------------------------

/// A JSON-RPC 2.0 request.
///
/// The `id` field is used to match requests with responses. The `params`
/// field carries method-specific arguments as an opaque JSON value —
/// the method handler is responsible for parsing and validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    /// JSON-RPC version. Always "2.0".
    pub jsonrpc: String,
    /// Request identifier. Echoed back in the response.
    pub id: serde_json::Value,
    /// The method to invoke.
    pub method: RpcMethod,
    /// Method-specific parameters.
    #[serde(default)]
    pub params: serde_json::Value,
}

impl RpcRequest {
    /// Creates a new RPC request with the given method and parameters.
    pub fn new(id: serde_json::Value, method: RpcMethod, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            method,
            params,
        }
    }
}

/// A JSON-RPC 2.0 response.
///
/// Exactly one of `result` or `error` will be set. Both being `None`
/// is a protocol violation that should never happen from a conforming node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    /// JSON-RPC version. Always "2.0".
    pub jsonrpc: String,
    /// The request ID this response corresponds to.
    pub id: serde_json::Value,
    /// The successful result, if the method completed without error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// The error, if the method failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl RpcResponse {
    /// Creates a successful response.
    pub fn success(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Creates an error response.
    pub fn error(id: serde_json::Value, error: RpcError) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

// ---------------------------------------------------------------------------
// RPC Errors
// ---------------------------------------------------------------------------

/// JSON-RPC 2.0 error object with standard error codes.
///
/// Error codes follow the JSON-RPC 2.0 specification:
/// - `-32700`: Parse error
/// - `-32600`: Invalid request
/// - `-32601`: Method not found
/// - `-32602`: Invalid params
/// - `-32603`: Internal error
/// - `-32000` to `-32099`: Server error (application-specific)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    /// Numeric error code.
    pub code: i32,
    /// Human-readable error message.
    pub message: String,
    /// Optional additional error data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl RpcError {
    /// JSON parse error.
    pub fn parse_error(msg: impl Into<String>) -> Self {
        Self {
            code: -32700,
            message: msg.into(),
            data: None,
        }
    }

    /// Invalid JSON-RPC request structure.
    pub fn invalid_request(msg: impl Into<String>) -> Self {
        Self {
            code: -32600,
            message: msg.into(),
            data: None,
        }
    }

    /// The requested method does not exist.
    pub fn method_not_found(method: impl Into<String>) -> Self {
        Self {
            code: -32601,
            message: format!("method not found: {}", method.into()),
            data: None,
        }
    }

    /// Invalid method parameters.
    pub fn invalid_params(msg: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: msg.into(),
            data: None,
        }
    }

    /// Internal server error.
    pub fn internal_error(msg: impl Into<String>) -> Self {
        Self {
            code: -32603,
            message: msg.into(),
            data: None,
        }
    }

    /// Transaction not found in the chain or mempool.
    pub fn transaction_not_found(tx_id: &str) -> Self {
        Self {
            code: -32000,
            message: format!("transaction not found: {}", tx_id),
            data: None,
        }
    }

    /// Block not found at the given height or hash.
    pub fn block_not_found(identifier: &str) -> Self {
        Self {
            code: -32001,
            message: format!("block not found: {}", identifier),
            data: None,
        }
    }

    /// Account not found or does not exist.
    pub fn account_not_found(address: &str) -> Self {
        Self {
            code: -32002,
            message: format!("account not found: {}", address),
            data: None,
        }
    }

    /// Transaction was rejected by the mempool.
    pub fn transaction_rejected(reason: impl Into<String>) -> Self {
        Self {
            code: -32003,
            message: reason.into(),
            data: None,
        }
    }

    /// Node is still syncing and cannot serve requests.
    pub fn node_syncing() -> Self {
        Self {
            code: -32004,
            message: "node is syncing".to_string(),
            data: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Typed Response Payloads
// ---------------------------------------------------------------------------

/// Response payload for `nova_getBalance`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceResponse {
    /// The address queried.
    pub address: String,
    /// Token identifier.
    pub token_id: String,
    /// Current balance in the smallest denomination.
    pub balance: u64,
}

/// Response payload for `nova_getBlockHeight`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeightResponse {
    /// Current chain height.
    pub height: u64,
}

/// Response payload for `nova_getValidators`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorsResponse {
    /// Active validators with their stake info.
    pub validators: Vec<ValidatorInfo>,
    /// Total stake across all active validators.
    pub total_stake: u64,
}

/// Response payload for `nova_estimateFee`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeEstimateResponse {
    /// Estimated fee in photons.
    pub estimated_fee: u64,
    /// Fee per byte at current network conditions.
    pub fee_per_byte: u64,
}

/// Response payload for `nova_getCreditOffers`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditOffersResponse {
    /// Available credit offers, sorted by interest rate (ascending).
    pub offers: Vec<serde_json::Value>,
    /// Total number of offers found.
    pub total: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpc_request_serialization() {
        let req = RpcRequest::new(
            serde_json::json!(1),
            RpcMethod::GetBlockHeight,
            serde_json::json!({}),
        );

        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("nova_getBlockHeight"));

        let recovered: RpcRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.method, RpcMethod::GetBlockHeight);
    }

    #[test]
    fn rpc_success_response() {
        let resp = RpcResponse::success(serde_json::json!(1), serde_json::json!({ "height": 42 }));

        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }

    #[test]
    fn rpc_error_response() {
        let resp = RpcResponse::error(
            serde_json::json!(1),
            RpcError::internal_error("something broke"),
        );

        assert!(resp.result.is_none());
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32603);
    }

    #[test]
    fn error_codes_are_correct() {
        assert_eq!(RpcError::parse_error("").code, -32700);
        assert_eq!(RpcError::invalid_request("").code, -32600);
        assert_eq!(RpcError::method_not_found("").code, -32601);
        assert_eq!(RpcError::invalid_params("").code, -32602);
        assert_eq!(RpcError::internal_error("").code, -32603);
        assert_eq!(RpcError::transaction_not_found("").code, -32000);
        assert_eq!(RpcError::block_not_found("").code, -32001);
        assert_eq!(RpcError::account_not_found("").code, -32002);
        assert_eq!(RpcError::transaction_rejected("").code, -32003);
        assert_eq!(RpcError::node_syncing().code, -32004);
    }

    #[test]
    fn all_methods_serialize_correctly() {
        let methods = vec![
            RpcMethod::GetBalance,
            RpcMethod::SendTransaction,
            RpcMethod::GetTransaction,
            RpcMethod::GetBlock,
            RpcMethod::GetBlockHeight,
            RpcMethod::GetAccountState,
            RpcMethod::GetValidators,
            RpcMethod::EstimateFee,
            RpcMethod::GetCreditOffers,
        ];

        for method in methods {
            let json = serde_json::to_string(&method).unwrap();
            assert!(
                json.contains("nova_"),
                "method {:?} should have nova_ prefix",
                method
            );
            let recovered: RpcMethod = serde_json::from_str(&json).unwrap();
            assert_eq!(method, recovered);
        }
    }
}
