//! Chain state synchronization protocol. Placeholder.
use serde::{Deserialize, Serialize};

pub struct SyncProtocol;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRequest {
    pub from_height: u64,
    pub to_height: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResponse {
    pub blocks: Vec<Vec<u8>>,
}
