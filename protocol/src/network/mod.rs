//! # Network Module
//!
//! P2P networking layer for the NOVA protocol. Handles validator node
//! lifecycle, consensus (hybrid PoS+PoA), transaction mempool management,
//! gossip-based message propagation, RPC API definitions, and state
//! synchronization between peers.
//!
//! ## Architecture
//!
//! ```text
//! node.rs       — Validator node lifecycle and peer management
//! consensus.rs  — Hybrid PoS+PoA consensus engine with BFT finality
//! mempool.rs    — Priority-ordered transaction pool with thread-safe access
//! gossip.rs     — Gossip protocol for block/transaction propagation
//! rpc.rs        — JSON-RPC method definitions and request/response types
//! sync.rs       — Chain state synchronization protocol
//! ```
//!
//! ## Design Decisions
//!
//! - Consensus uses round-robin proposer selection weighted by stake (PoS)
//!   with an authority set for block signing (PoA). This gives us fast
//!   finality without the energy waste of pure PoW.
//! - The mempool is protected by `parking_lot::RwLock` rather than `tokio::Mutex`
//!   because mempool reads vastly outnumber writes, and we want zero-cost
//!   reads on the hot path (block production).
//! - Gossip deduplication uses a bounded seen-message cache. Messages are
//!   identified by their BLAKE3 hash, and TTL prevents indefinite propagation.
//! - The RPC layer defines types only — actual HTTP serving happens in the
//!   node binary via axum. The protocol crate stays transport-agnostic.

pub mod consensus;
pub mod consensus_loop;
pub mod gossip;
pub mod mempool;
pub mod node;
pub mod producer;
pub mod rpc;
pub mod sync;

pub use consensus::{
    ConsensusConfig, ConsensusEngine, ConsensusRound, FinalizedBlock, ValidatorInfo, ValidatorSet,
    Vote,
};
pub use consensus_loop::{ConsensusLoop, ConsensusLoopConfig, ConsensusLoopError};
pub use gossip::{
    GossipAction, GossipBehaviour, GossipConfig, GossipError, GossipMessage, GossipProtocol,
    GossipService, GossipServiceConfig, GossipTopics, P2pGossipMessage, PeerInfo,
};
pub use mempool::{Mempool, MempoolConfig, MempoolEntry, MempoolError};
pub use node::{NodeStatus, ValidatorNode};
pub use producer::{BlockProducer, BlockProductionError, ProducedBlock, TxResult};
pub use rpc::{RpcError, RpcMethod, RpcRequest, RpcResponse};
pub use sync::{SyncProtocol, SyncRequest, SyncResponse};
