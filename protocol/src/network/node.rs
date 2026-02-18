//! # Validator Node
//!
//! The `ValidatorNode` is the top-level runtime entity for a NOVA network
//! participant. It owns the local chain state, mempool, peer connections,
//! and drives the consensus engine. In production, this struct is
//! instantiated by the node binary and wired to the networking stack.
//!
//! Validator nodes go through a well-defined lifecycle:
//!
//! ```text
//! new() -> start() -> [Active | Validating | Syncing] -> stop() -> Offline
//! ```
//!
//! A node must stake the minimum required amount before transitioning
//! from `Active` to `Validating`. Nodes below the stake threshold can
//! still relay transactions and serve RPC queries, but they cannot
//! propose or vote on blocks.

use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::config;
use crate::crypto::keys::NovaKeypair;
use crate::network::consensus::{ConsensusConfig, ConsensusEngine, ValidatorSet};
use crate::network::mempool::Mempool;
use crate::storage::{Block, Chain};
use crate::transaction::Transaction;

// ---------------------------------------------------------------------------
// Node Status
// ---------------------------------------------------------------------------

/// Lifecycle state of a validator node.
///
/// Transitions are enforced by the node runtime — you cannot jump from
/// `Offline` to `Validating` without going through `Syncing` first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeStatus {
    /// Downloading and verifying blocks to catch up with the network tip.
    Syncing,
    /// Fully synced and relaying transactions, but not producing blocks.
    /// Either the stake is below the minimum or the node opted out.
    Active,
    /// Actively participating in consensus: proposing and voting on blocks.
    Validating,
    /// Gracefully shut down. Not connected to any peers.
    Offline,
}

// ---------------------------------------------------------------------------
// Validator Node
// ---------------------------------------------------------------------------

/// A NOVA network validator node.
///
/// Holds the node's identity, local chain state, mempool, and peer set.
/// The consensus engine is spun up on `start()` and torn down on `stop()`.
///
/// Thread safety: the chain, mempool, and peer set are wrapped in
/// `Arc<RwLock<_>>` so they can be shared with the networking and RPC layers.
pub struct ValidatorNode {
    /// Unique node identifier, derived from the public key.
    pub id: String,
    /// Ed25519 keypair for signing blocks and votes.
    keypair: NovaKeypair,
    /// Network address this node listens on (e.g., "/ip4/0.0.0.0/tcp/9740").
    pub address: String,
    /// Amount of NOVA staked by this validator, in photons.
    pub stake: u64,
    /// Current lifecycle status.
    pub status: NodeStatus,
    /// Connected peer IDs.
    pub peers: Arc<RwLock<HashSet<String>>>,
    /// Local copy of the blockchain.
    pub chain: Arc<RwLock<Chain>>,
    /// Transaction mempool.
    pub mempool: Arc<Mempool>,
    /// Consensus engine, initialized on start().
    consensus: Option<ConsensusEngine>,
}

impl ValidatorNode {
    /// Creates a new validator node from a keypair and consensus configuration.
    ///
    /// The node starts in `Offline` status. Call `start()` to begin syncing
    /// and participating in the network.
    pub fn new(keypair: NovaKeypair, config: &ConsensusConfig) -> Self {
        let public_key = keypair.public_key();
        let id = public_key.to_hex();
        let address = format!("/ip4/0.0.0.0/tcp/{}", config::DEFAULT_P2P_PORT);

        info!(node_id = %id, "creating validator node");

        Self {
            id,
            keypair,
            address,
            stake: 0,
            status: NodeStatus::Offline,
            peers: Arc::new(RwLock::new(HashSet::new())),
            chain: Arc::new(RwLock::new(Chain::default())),
            mempool: Arc::new(Mempool::new(config.max_block_transactions * 10)),
            consensus: None,
        }
    }

    /// Starts the node: transitions from `Offline` to `Syncing`, then to
    /// `Active` once the chain tip is reached. If the node's stake meets
    /// the minimum threshold, it transitions further to `Validating`.
    ///
    /// In a real deployment, this method spawns async tasks for the P2P
    /// listener, block sync protocol, consensus round driver, RPC server,
    /// and mempool reaper.
    pub fn start(&mut self, validator_set: ValidatorSet) {
        info!(node_id = %self.id, "starting validator node");

        self.status = NodeStatus::Syncing;

        // Initialize the consensus engine.
        let config = ConsensusConfig::default();
        self.consensus = Some(ConsensusEngine::new(config.clone(), validator_set));

        // Transition based on stake.
        self.status = if self.stake >= config.stake_requirement {
            info!(node_id = %self.id, stake = self.stake, "stake meets threshold, entering validating mode");
            NodeStatus::Validating
        } else {
            info!(node_id = %self.id, stake = self.stake, "stake below threshold, active relay only");
            NodeStatus::Active
        };
    }

    /// Gracefully shuts down the node. Flushes pending state and disconnects
    /// from all peers.
    pub fn stop(&mut self) {
        info!(node_id = %self.id, "stopping validator node");

        self.status = NodeStatus::Offline;
        self.consensus = None;

        let mut peers = self.peers.write();
        peers.clear();

        info!(node_id = %self.id, "node stopped");
    }

    /// Processes an incoming transaction: validates it and inserts it into
    /// the mempool if it passes checks.
    pub fn process_transaction(&self, tx: Transaction) -> Result<(), NodeError> {
        if self.status == NodeStatus::Offline {
            return Err(NodeError::NodeOffline);
        }

        // Stateless validation.
        crate::transaction::verify_transaction(&tx)
            .map_err(|e| NodeError::InvalidTransaction(e.to_string()))?;

        // Insert into mempool.
        self.mempool
            .insert(tx)
            .map_err(|e| NodeError::MempoolFull(e.to_string()))?;

        Ok(())
    }

    /// Processes an incoming block received from a peer.
    ///
    /// Validates the block against the current chain state and, if valid,
    /// appends it to the local chain and removes included transactions
    /// from the mempool.
    pub fn process_block(&self, block: Block) -> Result<(), NodeError> {
        if self.status == NodeStatus::Offline {
            return Err(NodeError::NodeOffline);
        }

        let consensus = self
            .consensus
            .as_ref()
            .ok_or(NodeError::ConsensusNotReady)?;
        consensus
            .validate_block(&block)
            .map_err(|e| NodeError::InvalidBlock(e.to_string()))?;

        // Remove included transactions from mempool.
        for tx in &block.transactions {
            self.mempool.remove(&tx.id);
        }

        // Append to local chain.
        let mut chain = self.chain.write();
        chain.append(block);

        Ok(())
    }

    /// Adds a peer to the connected set if below the peer limit.
    pub fn add_peer(&self, peer_id: String) {
        let mut peers = self.peers.write();
        if peers.len() < config::MAX_PEERS {
            peers.insert(peer_id);
        } else {
            warn!(node_id = %self.id, "peer limit reached, rejecting connection");
        }
    }

    /// Removes a peer from the connected set.
    pub fn remove_peer(&self, peer_id: &str) {
        let mut peers = self.peers.write();
        peers.remove(peer_id);
    }

    /// Returns the number of connected peers.
    pub fn peer_count(&self) -> usize {
        self.peers.read().len()
    }

    /// Returns a reference to the node's keypair.
    pub fn keypair(&self) -> &NovaKeypair {
        &self.keypair
    }

    /// Returns a reference to the consensus engine, if initialized.
    pub fn consensus(&self) -> Option<&ConsensusEngine> {
        self.consensus.as_ref()
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during node operations.
#[derive(Debug, thiserror::Error)]
pub enum NodeError {
    /// The node is offline and cannot process requests.
    #[error("node is offline")]
    NodeOffline,
    /// Transaction failed validation checks.
    #[error("invalid transaction: {0}")]
    InvalidTransaction(String),
    /// The mempool is full or rejected the transaction.
    #[error("mempool rejected transaction: {0}")]
    MempoolFull(String),
    /// Block failed consensus validation.
    #[error("invalid block: {0}")]
    InvalidBlock(String),
    /// Consensus engine has not been initialized.
    #[error("consensus engine not ready")]
    ConsensusNotReady,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_lifecycle() {
        let keypair = NovaKeypair::generate();
        let config = ConsensusConfig::default();
        let mut node = ValidatorNode::new(keypair, &config);

        assert_eq!(node.status, NodeStatus::Offline);

        let validator_set = ValidatorSet::new();
        node.start(validator_set);
        assert!(node.status == NodeStatus::Active || node.status == NodeStatus::Validating);

        node.stop();
        assert_eq!(node.status, NodeStatus::Offline);
        assert_eq!(node.peer_count(), 0);
    }

    #[test]
    fn peer_management() {
        let keypair = NovaKeypair::generate();
        let config = ConsensusConfig::default();
        let node = ValidatorNode::new(keypair, &config);

        node.add_peer("peer-1".to_string());
        node.add_peer("peer-2".to_string());
        assert_eq!(node.peer_count(), 2);

        node.remove_peer("peer-1");
        assert_eq!(node.peer_count(), 1);
    }

    #[test]
    fn offline_node_rejects_transactions() {
        let keypair = NovaKeypair::generate();
        let config = ConsensusConfig::default();
        let node = ValidatorNode::new(keypair, &config);

        let tx = crate::transaction::TransactionBuilder::new(
            crate::transaction::TransactionType::Transfer,
        )
        .sender("alice")
        .receiver("bob")
        .amount(crate::transaction::types::Amount::new(
            100,
            crate::transaction::Currency::NOVA,
        ))
        .fee(200)
        .nonce(1)
        .build();

        let result = node.process_transaction(tx);
        assert!(result.is_err());
    }
}
