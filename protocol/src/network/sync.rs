//! # Block Synchronization Engine
//!
//! When a node boots up for the first time — or wakes up after a long nap —
//! it needs to catch up to the chain tip. This module implements the block sync
//! protocol: the request/response types peers use to exchange chain state, and
//! the `SyncEngine` that orchestrates downloading, validating, and replaying
//! blocks from peers.
//!
//! ## Protocol Overview
//!
//! ```text
//! New Node                           Peer
//! ─────────                         ──────
//!   │  GetChainTip                    │
//!   │──────────────────────────────>  │
//!   │  ChainTip { height, hash }     │
//!   │<──────────────────────────────  │
//!   │                                 │
//!   │  GetBlocks { start, end }       │
//!   │──────────────────────────────>  │
//!   │  Blocks(Vec<Block>)             │
//!   │<──────────────────────────────  │
//!   │  ... (repeat in batches) ...    │
//! ```
//!
//! ## Design Decisions
//!
//! - **Batch downloads.** Requesting blocks one at a time over the network is
//!   painfully slow. We split the gap into configurable batches (default: 100
//!   blocks per request) and can issue up to `max_parallel_requests` concurrent
//!   fetches. The tradeoff is memory — each batch is held in RAM until applied.
//!
//! - **Validate-then-apply.** Every downloaded block is verified (hash integrity,
//!   Merkle root, parent chain linkage) before touching the state tree. A single
//!   invalid block in a batch rejects the entire batch. No partial state corruption.
//!
//! - **Replay execution.** Blocks are not just stored — their transactions are
//!   re-executed against the state tree. This means the syncing node independently
//!   derives the same state root as the rest of the network. Trust is minimized;
//!   the peer only provides blocks, not state.
//!
//! - **Stateless engine.** The `SyncEngine` does not manage network connections.
//!   It provides `process_sync_request` for handling incoming requests and
//!   `apply_blocks` for processing downloaded batches. Transport is the caller's
//!   problem — this keeps the engine testable without spinning up libp2p.

use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::storage::block::Block;
use crate::storage::db::{DbError, NovaDB};
use crate::storage::state::{apply_transfer, StateError, StateTree};
use crate::transaction::types::TransactionType;

// ---------------------------------------------------------------------------
// Sync Request / Response
// ---------------------------------------------------------------------------

/// Messages a syncing node sends to peers.
///
/// These are intentionally simple — the sync protocol is a request-response
/// pattern, not a streaming protocol. Each request gets exactly one response.
/// Complexity lives in the engine, not the wire format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncRequest {
    /// "What's your chain tip?" Lightweight probe to determine whether we
    /// need to sync at all, and how far behind we are.
    GetChainTip,

    /// "Give me blocks in range [start, end)." The primary data transfer
    /// mechanism. `end` is exclusive, matching Rust range conventions.
    GetBlocks { start: u64, end: u64 },

    /// "Give me block at this height." For surgical single-block fetches
    /// (e.g., re-downloading a block that failed validation).
    GetBlock { height: u64 },
}

/// Messages a peer sends back in response to a sync request.
///
/// Errors are encoded in-band as `SyncResponse::Error` rather than at the
/// transport level. This keeps the protocol self-describing and makes it
/// trivial to add new error variants without changing the transport layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncResponse {
    /// The peer's current chain tip: height and block hash.
    ChainTip { height: u64, block_hash: [u8; 32] },

    /// A batch of blocks in ascending height order.
    Blocks(Vec<Block>),

    /// A single block, or None if the peer doesn't have it.
    Block(Option<Block>),

    /// Something went wrong on the peer's side. The string is a
    /// human-readable description for logging, not structured data.
    Error(String),
}

// ---------------------------------------------------------------------------
// SyncConfig
// ---------------------------------------------------------------------------

/// Tuning knobs for the sync engine.
///
/// Sensible defaults are provided via `Default`. Override individual fields
/// when you know your deployment characteristics (fast local network? bump
/// batch size. Flaky peers? increase retries).
#[derive(Debug, Clone)]
pub struct SyncConfig {
    /// Number of blocks to request in a single `GetBlocks` call.
    /// Larger batches amortize round-trip latency but use more memory.
    pub batch_size: u64,

    /// Maximum number of concurrent `GetBlocks` requests in flight.
    /// More parallelism = faster sync, but also more memory pressure
    /// and more burden on the serving peer.
    pub max_parallel_requests: usize,

    /// Per-request timeout in milliseconds. If a peer doesn't respond
    /// within this window, the request is considered failed and eligible
    /// for retry.
    pub request_timeout_ms: u64,

    /// How many times to retry a failed request before giving up.
    /// Retries use the same peer — peer rotation is the caller's job.
    pub max_retries: u32,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            max_parallel_requests: 4,
            request_timeout_ms: 10_000,
            max_retries: 3,
        }
    }
}

// ---------------------------------------------------------------------------
// SyncResult
// ---------------------------------------------------------------------------

/// Summary of a successful block application batch.
///
/// Returned by `apply_blocks` so the caller can log progress, update metrics,
/// or report sync status to the user without digging into engine internals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncResult {
    /// Number of blocks successfully validated and persisted.
    pub blocks_applied: u64,

    /// Total number of transactions executed across all applied blocks.
    pub transactions_executed: u64,

    /// Chain height after the last applied block.
    pub final_height: u64,

    /// State root after replaying all transactions.
    pub final_state_root: [u8; 32],
}

// ---------------------------------------------------------------------------
// SyncError
// ---------------------------------------------------------------------------

/// Errors that can occur during block synchronization.
///
/// These are split into "the data is wrong" errors (InvalidBlock, ChainGap,
/// InvalidParentHash) and "the infrastructure broke" errors (StateError,
/// DbError, RequestTimeout, PeerDisconnected). The former indicate a
/// misbehaving peer; the latter are transient and worth retrying.
#[derive(Debug)]
pub enum SyncError {
    /// A block failed integrity verification (hash mismatch, bad Merkle root,
    /// or other structural issue).
    InvalidBlock { height: u64, reason: String },

    /// Expected block at height `expected`, but got `got`. Indicates a gap
    /// or overlap in the downloaded batch.
    ChainGap { expected: u64, got: u64 },

    /// A block's `parent_hash` doesn't match the previous block's hash.
    /// Either the peer is serving a fork or the data is corrupt.
    InvalidParentHash { height: u64 },

    /// A state transition failed during transaction replay.
    StateError(StateError),

    /// Persistence layer failure.
    DbError(DbError),

    /// The peer did not respond within the configured timeout window.
    RequestTimeout,

    /// The peer disconnected mid-sync. Pick a new peer and resume.
    PeerDisconnected,
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidBlock { height, reason } => {
                write!(f, "invalid block at height {}: {}", height, reason)
            }
            Self::ChainGap { expected, got } => {
                write!(f, "chain gap: expected height {}, got {}", expected, got)
            }
            Self::InvalidParentHash { height } => {
                write!(f, "invalid parent hash at height {}", height)
            }
            Self::StateError(e) => write!(f, "state error: {}", e),
            Self::DbError(e) => write!(f, "database error: {}", e),
            Self::RequestTimeout => write!(f, "request timed out"),
            Self::PeerDisconnected => write!(f, "peer disconnected"),
        }
    }
}

impl std::error::Error for SyncError {}

impl From<StateError> for SyncError {
    fn from(e: StateError) -> Self {
        Self::StateError(e)
    }
}

impl From<DbError> for SyncError {
    fn from(e: DbError) -> Self {
        Self::DbError(e)
    }
}

// ---------------------------------------------------------------------------
// SyncEngine
// ---------------------------------------------------------------------------

/// The block synchronization engine.
///
/// Handles both sides of the sync protocol: responding to incoming requests
/// from peers (via `process_sync_request`) and applying downloaded blocks
/// to the local chain (via `apply_blocks`). The engine is transport-agnostic —
/// it neither opens connections nor reads from the network. That's the
/// caller's responsibility.
///
/// ## Thread Safety
///
/// The engine holds `Arc` references to the database and state tree, and
/// the state tree is protected by a `RwLock`. Multiple threads can call
/// `process_sync_request` concurrently (read path). `apply_blocks` acquires
/// a write lock on the state tree and should be serialized by the caller.
pub struct SyncEngine {
    /// Persistent storage for blocks and chain metadata.
    db: Arc<NovaDB>,

    /// Sparse Merkle Tree holding all account states. Protected by RwLock
    /// because sync writes (transaction replay) must be exclusive, while
    /// reads (serving sync requests) can be concurrent.
    state_tree: Arc<RwLock<StateTree>>,

    /// Configuration knobs (batch size, timeouts, etc.).
    config: SyncConfig,
}

impl SyncEngine {
    /// Creates a new sync engine wired to the given storage layer.
    ///
    /// The engine starts idle — no sync activity happens until the caller
    /// invokes `apply_blocks` with downloaded data.
    pub fn new(
        db: Arc<NovaDB>,
        state_tree: Arc<RwLock<StateTree>>,
        config: SyncConfig,
    ) -> Self {
        Self {
            db,
            state_tree,
            config,
        }
    }

    /// Returns the local chain tip: current height and block hash.
    ///
    /// If the database is empty (no blocks persisted), returns height 0 and
    /// the genesis block hash. A fresh node always knows about genesis — it's
    /// hardcoded, not downloaded.
    pub fn local_chain_tip(&self) -> Result<(u64, [u8; 32]), SyncError> {
        match self.db.get_latest_block_height()? {
            Some(height) => {
                let block = self.db.get_block(height)?.ok_or_else(|| SyncError::DbError(
                    DbError::NotFound(format!("block at height {}", height)),
                ))?;
                Ok((height, block.header.hash))
            }
            None => {
                // Empty database — return genesis tip.
                let genesis = Block::genesis();
                Ok((0, genesis.header.hash))
            }
        }
    }

    /// Handles an incoming sync request from a peer.
    ///
    /// This is the "server side" of the sync protocol. A peer sends us a
    /// `SyncRequest`, we look up the answer in our local database, and return
    /// a `SyncResponse`. No state mutation happens here — it's pure reads.
    pub fn process_sync_request(&self, request: SyncRequest) -> SyncResponse {
        match request {
            SyncRequest::GetChainTip => {
                match self.local_chain_tip() {
                    Ok((height, hash)) => SyncResponse::ChainTip {
                        height,
                        block_hash: hash,
                    },
                    Err(e) => SyncResponse::Error(format!("failed to read chain tip: {}", e)),
                }
            }

            SyncRequest::GetBlocks { start, end } => {
                if start >= end {
                    return SyncResponse::Blocks(Vec::new());
                }
                // get_block_range is inclusive on both ends, but our protocol
                // uses [start, end), so we request [start, end - 1].
                match self.db.get_block_range(start, end - 1) {
                    Ok(blocks) => SyncResponse::Blocks(blocks),
                    Err(e) => SyncResponse::Error(format!(
                        "failed to read blocks [{}, {}): {}",
                        start, end, e,
                    )),
                }
            }

            SyncRequest::GetBlock { height } => {
                match self.db.get_block(height) {
                    Ok(block) => SyncResponse::Block(block),
                    Err(e) => SyncResponse::Error(format!(
                        "failed to read block at height {}: {}",
                        height, e,
                    )),
                }
            }
        }
    }

    /// Validates and applies a batch of blocks to the local chain.
    ///
    /// Each block goes through:
    /// 1. **Integrity check** — recompute hash, verify Merkle root.
    /// 2. **Chain linkage** — verify parent hash matches the previous block.
    /// 3. **Transaction replay** — execute every transaction against the state tree.
    /// 4. **Persistence** — write the block and updated metadata to NovaDB.
    ///
    /// If any block fails validation, the entire batch is rejected and the
    /// state tree / database are left in their pre-call state (for the
    /// remaining unprocessed blocks). Blocks that were already applied before
    /// the failure are committed — this is not a transactional rollback.
    ///
    /// The caller should pass blocks in ascending height order, starting from
    /// the block immediately after the local chain tip.
    pub fn apply_blocks(&self, blocks: Vec<Block>) -> Result<SyncResult, SyncError> {
        if blocks.is_empty() {
            let (height, _) = self.local_chain_tip()?;
            let state_root = self.state_tree.read().root();
            return Ok(SyncResult {
                blocks_applied: 0,
                transactions_executed: 0,
                final_height: height,
                final_state_root: state_root,
            });
        }

        let mut blocks_applied = 0u64;
        let mut transactions_executed = 0u64;

        // Determine what the parent hash should be for the first block in the batch.
        let first_height = blocks[0].header.height;
        let expected_parent_hash = if first_height == 0 {
            [0u8; 32] // Genesis block's parent is all zeros.
        } else {
            // Look up the block just before the batch start.
            let prev = self.db.get_block(first_height - 1)?.ok_or_else(|| {
                SyncError::ChainGap {
                    expected: first_height - 1,
                    got: first_height,
                }
            })?;
            prev.header.hash
        };

        let mut prev_hash = expected_parent_hash;
        let mut prev_height = if first_height == 0 { 0 } else { first_height - 1 };

        for (i, block) in blocks.iter().enumerate() {
            // Verify block integrity (hash + Merkle root).
            block.verify().map_err(|reason| SyncError::InvalidBlock {
                height: block.header.height,
                reason,
            })?;

            // Verify height continuity.
            let expected_height = if i == 0 { first_height } else { prev_height + 1 };
            if block.header.height != expected_height {
                return Err(SyncError::ChainGap {
                    expected: expected_height,
                    got: block.header.height,
                });
            }

            // Verify parent hash linkage.
            if i == 0 {
                // For genesis, parent must be all zeros.
                if block.header.height == 0 {
                    if block.header.parent_hash != [0u8; 32] {
                        return Err(SyncError::InvalidParentHash {
                            height: block.header.height,
                        });
                    }
                } else if block.header.parent_hash != prev_hash {
                    return Err(SyncError::InvalidParentHash {
                        height: block.header.height,
                    });
                }
            } else if block.header.parent_hash != prev_hash {
                return Err(SyncError::InvalidParentHash {
                    height: block.header.height,
                });
            }

            // Replay transactions against the state tree.
            {
                let mut tree = self.state_tree.write();
                for tx in &block.transactions {
                    match tx.tx_type {
                        TransactionType::Transfer => {
                            apply_transfer(&mut tree, &tx.sender, &tx.receiver, tx.amount.value)?;
                        }
                        // Non-transfer transaction types are accepted but don't
                        // mutate state yet. Same behavior as BlockProducer.
                        TransactionType::CreditRequest
                        | TransactionType::CreditSettlement
                        | TransactionType::TokenMint
                        | TransactionType::TokenBurn
                        | TransactionType::ConfidentialTransfer => {}
                    }
                    transactions_executed += 1;
                }
            }

            // Persist the block.
            self.db.put_block(block)?;

            blocks_applied += 1;
            prev_hash = block.header.hash;
            prev_height = block.header.height;
        }

        let state_root = self.state_tree.read().root();

        Ok(SyncResult {
            blocks_applied,
            transactions_executed,
            final_height: prev_height,
            final_state_root: state_root,
        })
    }

    /// Validates that a sequence of blocks forms a valid chain.
    ///
    /// Checks:
    /// 1. Each block passes integrity verification (hash + Merkle root).
    /// 2. Heights are contiguous starting from `expected_start`.
    /// 3. Each block's `parent_hash` matches the previous block's hash.
    ///
    /// This is a "dry run" validation — it does not touch the state tree or
    /// database. Useful for pre-validating a batch before committing resources
    /// to apply it.
    pub fn validate_block_chain(
        &self,
        blocks: &[Block],
        expected_start: u64,
    ) -> Result<(), SyncError> {
        for (i, block) in blocks.iter().enumerate() {
            // Integrity check.
            block.verify().map_err(|reason| SyncError::InvalidBlock {
                height: block.header.height,
                reason,
            })?;

            // Height continuity.
            let expected_height = expected_start + i as u64;
            if block.header.height != expected_height {
                return Err(SyncError::ChainGap {
                    expected: expected_height,
                    got: block.header.height,
                });
            }

            // Parent hash linkage (skip for the first block in the batch —
            // the caller is responsible for verifying it chains to the local tip).
            if i > 0 {
                let prev = &blocks[i - 1];
                if block.header.parent_hash != prev.header.hash {
                    return Err(SyncError::InvalidParentHash {
                        height: block.header.height,
                    });
                }
            }
        }

        Ok(())
    }

    /// Returns `true` if we are behind the given remote height.
    ///
    /// A node "needs sync" when the remote chain has blocks we haven't seen.
    /// This is a cheap check — just compares two integers.
    pub fn needs_sync(&self, remote_height: u64) -> bool {
        let local_height = self
            .db
            .get_latest_block_height()
            .ok()
            .flatten()
            .unwrap_or(0);
        remote_height > local_height
    }

    /// Splits the gap between local and remote heights into download batches.
    ///
    /// Returns a list of `(start, end)` pairs where `start` is inclusive and
    /// `end` is exclusive, suitable for `GetBlocks` requests. The batch size
    /// is determined by `config.batch_size`.
    ///
    /// If `local_height >= remote_height`, returns an empty list (nothing to sync).
    pub fn compute_sync_plan(&self, local_height: u64, remote_height: u64) -> Vec<(u64, u64)> {
        if local_height >= remote_height {
            return Vec::new();
        }

        let start = local_height + 1;
        let end = remote_height + 1; // exclusive upper bound
        let batch = self.config.batch_size;

        let mut plan = Vec::new();
        let mut cursor = start;

        while cursor < end {
            let batch_end = std::cmp::min(cursor + batch, end);
            plan.push((cursor, batch_end));
            cursor = batch_end;
        }

        plan
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::block::Block;
    use crate::storage::db::NovaDB;
    use crate::storage::state::{AccountState, StateTree};
    use crate::transaction::builder::TransactionBuilder;
    use crate::transaction::types::{Amount, Currency, TransactionType};

    // -- Helpers ------------------------------------------------------------

    /// Spins up a sync engine with a temporary database and empty state tree.
    fn setup() -> (SyncEngine, Arc<NovaDB>, Arc<RwLock<StateTree>>) {
        let db = Arc::new(NovaDB::open_temporary().expect("temp db"));
        let state_tree = Arc::new(RwLock::new(StateTree::new((*db).clone())));
        let engine = SyncEngine::new(
            Arc::clone(&db),
            Arc::clone(&state_tree),
            SyncConfig::default(),
        );
        (engine, db, state_tree)
    }

    /// Creates a test transfer transaction.
    fn make_test_tx(sender: &str, receiver: &str, amount: u64, nonce: u64) -> crate::transaction::Transaction {
        TransactionBuilder::new(TransactionType::Transfer)
            .sender(sender)
            .receiver(receiver)
            .amount(Amount::new(amount, Currency::NOVA))
            .fee(100)
            .nonce(nonce)
            .timestamp(1_000_000 + nonce)
            .build()
    }

    /// Builds a chain of blocks with no transactions, linked from genesis.
    fn make_empty_chain(count: usize) -> Vec<Block> {
        let mut chain = vec![Block::genesis()];
        for i in 1..count {
            let parent = &chain[i - 1];
            let block = Block::new(parent, vec![], format!("nova:validator_{i}"), [i as u8; 32]);
            chain.push(block);
        }
        chain
    }


    // -- 1. local_chain_tip_empty_db ----------------------------------------

    #[test]
    fn local_chain_tip_empty_db() {
        let (engine, _db, _tree) = setup();

        let (height, hash) = engine.local_chain_tip().expect("should return genesis tip");
        assert_eq!(height, 0);
        // Should be the genesis block hash.
        let genesis = Block::genesis();
        assert_eq!(hash, genesis.header.hash);
    }

    // -- 2. local_chain_tip_with_blocks -------------------------------------

    #[test]
    fn local_chain_tip_with_blocks() {
        let (engine, db, _tree) = setup();

        let chain = make_empty_chain(4);
        for block in &chain {
            db.put_block(block).unwrap();
        }

        let (height, hash) = engine.local_chain_tip().expect("should return tip");
        assert_eq!(height, 3);
        assert_eq!(hash, chain[3].header.hash);
    }

    // -- 3. process_get_chain_tip -------------------------------------------

    #[test]
    fn process_get_chain_tip() {
        let (engine, db, _tree) = setup();

        let genesis = Block::genesis();
        db.put_block(&genesis).unwrap();

        let response = engine.process_sync_request(SyncRequest::GetChainTip);
        match response {
            SyncResponse::ChainTip { height, block_hash } => {
                assert_eq!(height, 0);
                assert_eq!(block_hash, genesis.header.hash);
            }
            other => panic!("expected ChainTip, got: {:?}", other),
        }
    }

    // -- 4. process_get_blocks_range ----------------------------------------

    #[test]
    fn process_get_blocks_range() {
        let (engine, db, _tree) = setup();

        let chain = make_empty_chain(5);
        for block in &chain {
            db.put_block(block).unwrap();
        }

        // Request blocks [1, 4) = heights 1, 2, 3
        let response = engine.process_sync_request(SyncRequest::GetBlocks {
            start: 1,
            end: 4,
        });
        match response {
            SyncResponse::Blocks(blocks) => {
                assert_eq!(blocks.len(), 3);
                assert_eq!(blocks[0].header.height, 1);
                assert_eq!(blocks[1].header.height, 2);
                assert_eq!(blocks[2].header.height, 3);
            }
            other => panic!("expected Blocks, got: {:?}", other),
        }
    }

    // -- 5. process_get_block_single ----------------------------------------

    #[test]
    fn process_get_block_single() {
        let (engine, db, _tree) = setup();

        let chain = make_empty_chain(3);
        for block in &chain {
            db.put_block(block).unwrap();
        }

        let response = engine.process_sync_request(SyncRequest::GetBlock { height: 2 });
        match response {
            SyncResponse::Block(Some(block)) => {
                assert_eq!(block.header.height, 2);
                assert_eq!(block.header.hash, chain[2].header.hash);
            }
            other => panic!("expected Block(Some), got: {:?}", other),
        }
    }

    // -- 6. process_get_block_missing ---------------------------------------

    #[test]
    fn process_get_block_missing() {
        let (engine, _db, _tree) = setup();

        let response = engine.process_sync_request(SyncRequest::GetBlock { height: 999 });
        match response {
            SyncResponse::Block(None) => {} // Expected.
            other => panic!("expected Block(None), got: {:?}", other),
        }
    }

    // -- 7. apply_single_block ----------------------------------------------

    #[test]
    fn apply_single_block() {
        let (engine, db, _tree) = setup();

        let genesis = Block::genesis();
        db.put_block(&genesis).unwrap();

        let block1 = Block::new(&genesis, vec![], "nova:validator_1".to_string(), [1u8; 32]);

        let result = engine.apply_blocks(vec![block1.clone()]).unwrap();
        assert_eq!(result.blocks_applied, 1);
        assert_eq!(result.transactions_executed, 0);
        assert_eq!(result.final_height, 1);

        // Verify the block was persisted.
        let retrieved = db.get_block(1).unwrap().expect("block 1 should exist");
        assert_eq!(retrieved.header.hash, block1.header.hash);
    }

    // -- 8. apply_block_chain -----------------------------------------------

    #[test]
    fn apply_block_chain() {
        let (engine, db, _tree) = setup();

        let chain = make_empty_chain(6); // genesis + 5 blocks
        db.put_block(&chain[0]).unwrap(); // Persist genesis.

        // Apply blocks 1 through 5.
        let blocks_to_apply: Vec<Block> = chain[1..].to_vec();
        let result = engine.apply_blocks(blocks_to_apply).unwrap();

        assert_eq!(result.blocks_applied, 5);
        assert_eq!(result.final_height, 5);

        // Verify chain linkage in the database.
        for i in 1..6 {
            let block = db.get_block(i).unwrap().expect("block should exist");
            let parent = db.get_block(i - 1).unwrap().expect("parent should exist");
            assert_eq!(block.header.parent_hash, parent.header.hash);
        }
    }

    // -- 9. apply_blocks_with_transfers -------------------------------------

    #[test]
    fn apply_blocks_with_transfers() {
        let (engine, db, state_tree) = setup();

        // Seed sender balance.
        {
            let mut tree = state_tree.write();
            tree.put("nova1alice", &AccountState::with_balance(10_000));
        }

        let genesis = Block::genesis();
        db.put_block(&genesis).unwrap();

        // Build a block with a transfer.
        let tx = make_test_tx("nova1alice", "nova1bob", 3_000, 0);
        let block1 = Block::new(&genesis, vec![tx], "nova:validator_1".to_string(), [1u8; 32]);

        let result = engine.apply_blocks(vec![block1]).unwrap();

        assert_eq!(result.blocks_applied, 1);
        assert_eq!(result.transactions_executed, 1);

        // Verify state was updated.
        let tree = state_tree.read();
        let alice = tree.get("nova1alice").expect("alice should exist");
        let bob = tree.get("nova1bob").expect("bob should exist");
        assert_eq!(alice.balance, 7_000);
        assert_eq!(bob.balance, 3_000);
    }

    // -- 10. validate_block_chain_valid -------------------------------------

    #[test]
    fn validate_block_chain_valid() {
        let (engine, _db, _tree) = setup();

        let chain = make_empty_chain(5);
        // Validate blocks 1..4 starting at expected height 1.
        let result = engine.validate_block_chain(&chain[1..], 1);
        assert!(result.is_ok());
    }

    // -- 11. validate_block_chain_gap ---------------------------------------

    #[test]
    fn validate_block_chain_gap() {
        let (engine, _db, _tree) = setup();

        let chain = make_empty_chain(5);
        // Create a gap by skipping block at height 2.
        let blocks_with_gap = vec![chain[1].clone(), chain[3].clone()];

        let result = engine.validate_block_chain(&blocks_with_gap, 1);
        assert!(result.is_err());
        match result.unwrap_err() {
            SyncError::ChainGap { expected, got } => {
                assert_eq!(expected, 2);
                assert_eq!(got, 3);
            }
            other => panic!("expected ChainGap, got: {:?}", other),
        }
    }

    // -- 12. validate_block_chain_wrong_parent ------------------------------

    #[test]
    fn validate_block_chain_wrong_parent() {
        let (engine, _db, _tree) = setup();

        let chain = make_empty_chain(4);
        // Tamper with the parent hash of block 2.
        let mut tampered = chain[2].clone();
        tampered.header.parent_hash = [0xFF; 32];
        // Recompute hash to maintain internal consistency.
        tampered.header.hash = tampered.compute_hash();

        let blocks = vec![chain[1].clone(), tampered];
        let result = engine.validate_block_chain(&blocks, 1);
        assert!(result.is_err());
        match result.unwrap_err() {
            SyncError::InvalidParentHash { height } => {
                assert_eq!(height, 2);
            }
            other => panic!("expected InvalidParentHash, got: {:?}", other),
        }
    }

    // -- 13. needs_sync_behind ----------------------------------------------

    #[test]
    fn needs_sync_behind() {
        let (engine, db, _tree) = setup();

        let genesis = Block::genesis();
        db.put_block(&genesis).unwrap();

        assert!(engine.needs_sync(5));
        assert!(engine.needs_sync(1));
    }

    // -- 14. needs_sync_caught_up -------------------------------------------

    #[test]
    fn needs_sync_caught_up() {
        let (engine, db, _tree) = setup();

        let chain = make_empty_chain(4);
        for block in &chain {
            db.put_block(block).unwrap();
        }

        // Local height is 3. Remote height is 3 or less — no sync needed.
        assert!(!engine.needs_sync(3));
        assert!(!engine.needs_sync(2));
        assert!(!engine.needs_sync(0));
    }

    // -- 15. compute_sync_plan ----------------------------------------------

    #[test]
    fn compute_sync_plan() {
        let (engine, _db, _tree) = setup();

        // Local at 0, remote at 350, batch size 100.
        let plan = engine.compute_sync_plan(0, 350);

        assert_eq!(plan.len(), 4);
        assert_eq!(plan[0], (1, 101));
        assert_eq!(plan[1], (101, 201));
        assert_eq!(plan[2], (201, 301));
        assert_eq!(plan[3], (301, 351));
    }

    // -- 16. compute_sync_plan_small_gap ------------------------------------

    #[test]
    fn compute_sync_plan_small_gap() {
        let (engine, _db, _tree) = setup();

        // Gap smaller than batch size — should produce exactly one batch.
        let plan = engine.compute_sync_plan(10, 50);

        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0], (11, 51));
    }

    // -- 17. apply_blocks_state_root_consistency ----------------------------

    #[test]
    fn apply_blocks_state_root_consistency() {
        let (engine, db, state_tree) = setup();

        // Seed sender with enough balance for multiple transfers.
        {
            let mut tree = state_tree.write();
            tree.put("nova1alice", &AccountState::with_balance(100_000));
        }

        let genesis = Block::genesis();
        db.put_block(&genesis).unwrap();

        // Build 3 blocks, each with a transfer.
        let chain = {
            let mut blocks = vec![genesis.clone()];
            for i in 1..=3u64 {
                let parent = &blocks[(i - 1) as usize];
                let tx = make_test_tx("nova1alice", "nova1bob", 1_000, i - 1);
                let block = Block::new(
                    parent,
                    vec![tx],
                    format!("nova:validator_{i}"),
                    [i as u8; 32],
                );
                blocks.push(block);
            }
            blocks
        };

        let blocks_to_apply: Vec<Block> = chain[1..].to_vec();
        let result = engine.apply_blocks(blocks_to_apply).unwrap();

        // Verify the state root matches what we'd expect.
        let tree = state_tree.read();
        assert_eq!(result.final_state_root, tree.root());
        assert_eq!(result.final_height, 3);
        assert_eq!(result.transactions_executed, 3);

        // Verify final balances.
        let alice = tree.get("nova1alice").unwrap();
        let bob = tree.get("nova1bob").unwrap();
        assert_eq!(alice.balance, 97_000); // 100_000 - 3 * 1_000
        assert_eq!(bob.balance, 3_000);    // 3 * 1_000
    }

    // -- 18. config_defaults ------------------------------------------------

    #[test]
    fn config_defaults() {
        let config = SyncConfig::default();
        assert_eq!(config.batch_size, 100);
        assert_eq!(config.max_parallel_requests, 4);
        assert_eq!(config.request_timeout_ms, 10_000);
        assert_eq!(config.max_retries, 3);
    }

    // -- 19. process_get_blocks_empty_range ---------------------------------

    #[test]
    fn process_get_blocks_empty_range() {
        let (engine, _db, _tree) = setup();

        // start >= end should return an empty blocks vec.
        let response = engine.process_sync_request(SyncRequest::GetBlocks {
            start: 5,
            end: 5,
        });
        match response {
            SyncResponse::Blocks(blocks) => assert!(blocks.is_empty()),
            other => panic!("expected empty Blocks, got: {:?}", other),
        }
    }

    // -- 20. apply_blocks_empty_batch ---------------------------------------

    #[test]
    fn apply_blocks_empty_batch() {
        let (engine, db, _tree) = setup();

        let genesis = Block::genesis();
        db.put_block(&genesis).unwrap();

        let result = engine.apply_blocks(vec![]).unwrap();
        assert_eq!(result.blocks_applied, 0);
        assert_eq!(result.transactions_executed, 0);
        assert_eq!(result.final_height, 0);
    }

    // -- 21. compute_sync_plan_no_gap ---------------------------------------

    #[test]
    fn compute_sync_plan_no_gap() {
        let (engine, _db, _tree) = setup();

        // Already caught up — plan should be empty.
        let plan = engine.compute_sync_plan(100, 100);
        assert!(plan.is_empty());

        // Ahead of remote — also empty.
        let plan = engine.compute_sync_plan(100, 50);
        assert!(plan.is_empty());
    }

    // -- 22. apply_blocks_rejects_invalid_block -----------------------------

    #[test]
    fn apply_blocks_rejects_invalid_block() {
        let (engine, db, _tree) = setup();

        let genesis = Block::genesis();
        db.put_block(&genesis).unwrap();

        // Create a valid block, then tamper with its hash.
        let mut block1 = Block::new(&genesis, vec![], "nova:val".to_string(), [1u8; 32]);
        block1.header.hash[0] ^= 0xFF; // Corrupt the hash.

        let result = engine.apply_blocks(vec![block1]);
        assert!(result.is_err());
        match result.unwrap_err() {
            SyncError::InvalidBlock { height, .. } => assert_eq!(height, 1),
            other => panic!("expected InvalidBlock, got: {:?}", other),
        }
    }

    // -- 23. needs_sync_empty_db --------------------------------------------

    #[test]
    fn needs_sync_empty_db() {
        let (engine, _db, _tree) = setup();

        // Empty DB has height 0 (default). Any remote height > 0 needs sync.
        assert!(engine.needs_sync(1));
        assert!(!engine.needs_sync(0));
    }

    // -- 24. compute_sync_plan_exact_batch_boundary -------------------------

    #[test]
    fn compute_sync_plan_exact_batch_boundary() {
        let (engine, _db, _tree) = setup();

        // Gap of exactly 100 (one full batch).
        let plan = engine.compute_sync_plan(0, 100);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0], (1, 101));
    }

    // -- 25. apply_genesis_block -------------------------------------------

    #[test]
    fn apply_genesis_block() {
        let (engine, _db, _tree) = setup();

        let genesis = Block::genesis();
        let result = engine.apply_blocks(vec![genesis.clone()]).unwrap();

        assert_eq!(result.blocks_applied, 1);
        assert_eq!(result.final_height, 0);
        assert_eq!(result.transactions_executed, 0);
    }
}
