//! # Gossip Protocol
//!
//! Two-layer message propagation for the NOVA P2P network:
//!
//! **Layer 1 — Epidemic Gossip** (`GossipProtocol`):
//! Application-level gossip with TTL-bounded epidemic propagation and
//! BLAKE3-based deduplication. When a node receives a new transaction or
//! block, it forwards the message to a subset of its peers (fanout). Each
//! peer does the same, resulting in O(log N) propagation across the network.
//! This layer is transport-agnostic and returns `GossipAction` values that
//! the node runtime dispatches.
//!
//! **Layer 2 — libp2p Gossipsub** (`GossipService`):
//! The actual network transport built on libp2p's gossipsub protocol. Handles
//! topic-based pub/sub, peer mesh management, and wire-level encryption via
//! Noise. The service constructs a configured `Swarm` and provides publish
//! helpers for transactions, blocks, and votes. The event loop itself runs
//! in the node binary — this module stays focused on setup and serialization.
//!
//! ## Why two layers?
//!
//! Gossipsub handles the mesh topology and message routing at the network
//! level. The epidemic gossip layer adds application-specific deduplication,
//! TTL enforcement, and action dispatch. Separation of concerns: gossipsub
//! doesn't know about mempools, and `GossipProtocol` doesn't know about TCP.
//!
//! ## Message Encoding
//!
//! libp2p messages are bincode-encoded `P2pGossipMessage` enums. Bincode was
//! chosen over JSON for the wire format because gossip messages are hot-path
//! and we don't need human readability on the wire. The size difference is
//! roughly 3-4x smaller for typical transaction payloads.

use std::fmt;
use std::time::Duration;

use dashmap::DashMap;
use libp2p::gossipsub::{self, IdentTopic, MessageAuthenticity};
use libp2p::identity::Keypair;
use libp2p::swarm::NetworkBehaviour;
use libp2p::{identify, PeerId, Swarm};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, trace};

use crate::network::consensus::Vote;
use crate::storage::Block;
use crate::transaction::Transaction;

// ===========================================================================
// Layer 1: Epidemic Gossip (application-level)
// ===========================================================================

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the epidemic gossip protocol layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GossipConfig {
    /// Maximum number of peers to connect to.
    pub max_peers: usize,
    /// Heartbeat interval in milliseconds. Peers that miss 2x this interval
    /// are considered disconnected.
    pub heartbeat_interval_ms: u64,
    /// Maximum number of hops a message can travel before being dropped.
    pub message_ttl: u8,
    /// Number of peers to forward each message to (fanout).
    pub fanout: usize,
    /// Maximum number of message hashes to keep in the deduplication cache.
    pub seen_cache_size: usize,
}

impl Default for GossipConfig {
    fn default() -> Self {
        Self {
            max_peers: crate::config::MAX_PEERS,
            heartbeat_interval_ms: crate::config::PEER_HEARTBEAT_INTERVAL.as_millis() as u64,
            message_ttl: 10,
            fanout: crate::config::GOSSIP_FANOUT,
            seen_cache_size: 100_000,
        }
    }
}

// ---------------------------------------------------------------------------
// Peer Info
// ---------------------------------------------------------------------------

/// Information about a connected peer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerInfo {
    /// Unique peer identifier (typically the hex-encoded public key).
    pub peer_id: String,
    /// Multiaddr-style address string (e.g., "/ip4/1.2.3.4/tcp/9740").
    pub address: String,
    /// Unix timestamp (milliseconds) when this peer connected.
    pub connected_at: u64,
    /// Last time a heartbeat was received from this peer (Unix ms).
    pub last_seen: u64,
}

// ---------------------------------------------------------------------------
// Gossip Messages (epidemic layer)
// ---------------------------------------------------------------------------

/// Messages propagated through the epidemic gossip layer.
///
/// Each variant wraps the actual payload plus a TTL counter. The TTL is
/// decremented on every hop and the message is dropped when it hits zero.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GossipMessage {
    /// A new transaction to be added to the mempool.
    NewTransaction {
        /// The transaction being gossiped.
        transaction: Transaction,
        /// Remaining hops before this message is dropped.
        ttl: u8,
    },
    /// A new block proposed by a validator.
    NewBlock {
        /// The proposed block.
        block: Block,
        /// Remaining hops.
        ttl: u8,
    },
    /// Peer discovery announcement. Nodes periodically broadcast their
    /// known peers so that new nodes can bootstrap their connection set.
    PeerDiscovery {
        /// Information about the announcing peer.
        peer: PeerInfo,
        /// Known peers being shared.
        known_peers: Vec<PeerInfo>,
        /// Remaining hops.
        ttl: u8,
    },
}

impl GossipMessage {
    /// Returns the TTL of this message.
    pub fn ttl(&self) -> u8 {
        match self {
            Self::NewTransaction { ttl, .. } => *ttl,
            Self::NewBlock { ttl, .. } => *ttl,
            Self::PeerDiscovery { ttl, .. } => *ttl,
        }
    }

    /// Decrements the TTL. Returns `None` if the message has expired.
    pub fn decrement_ttl(self) -> Option<Self> {
        match self {
            Self::NewTransaction { transaction, ttl } if ttl > 1 => Some(Self::NewTransaction {
                transaction,
                ttl: ttl - 1,
            }),
            Self::NewBlock { block, ttl } if ttl > 1 => Some(Self::NewBlock {
                block,
                ttl: ttl - 1,
            }),
            Self::PeerDiscovery {
                peer,
                known_peers,
                ttl,
            } if ttl > 1 => Some(Self::PeerDiscovery {
                peer,
                known_peers,
                ttl: ttl - 1,
            }),
            _ => None,
        }
    }

    /// Computes the BLAKE3 hash of the message for deduplication.
    pub fn content_hash(&self) -> [u8; 32] {
        let serialized = serde_json::to_vec(self).unwrap_or_default();
        *blake3::hash(&serialized).as_bytes()
    }
}

// ---------------------------------------------------------------------------
// Gossip Actions
// ---------------------------------------------------------------------------

/// Actions the gossip protocol handler returns after processing a message.
/// The node runtime executes these actions against the network layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GossipAction {
    /// Forward the message to the specified peers.
    Forward {
        /// The message to forward (with decremented TTL).
        message: GossipMessage,
        /// Peer IDs to forward to. If empty, forward to all connected peers.
        target_peers: Vec<String>,
    },
    /// Add a transaction to the local mempool.
    AddToMempool(Transaction),
    /// Process a received block (validate + potentially append to chain).
    ProcessBlock(Block),
    /// Add discovered peers to the connection set.
    AddPeers(Vec<PeerInfo>),
    /// Drop the message (duplicate or expired TTL).
    Drop,
}

// ---------------------------------------------------------------------------
// Gossip Protocol (epidemic layer engine)
// ---------------------------------------------------------------------------

/// The epidemic gossip protocol engine.
///
/// Manages message deduplication, TTL enforcement, and determines which
/// actions to take when a message arrives. Does not perform network I/O
/// directly — it returns `GossipAction` values that the node runtime
/// dispatches to the appropriate subsystems.
pub struct GossipProtocol {
    /// Protocol configuration.
    config: GossipConfig,
    /// Set of recently seen message hashes for deduplication.
    seen_messages: DashMap<[u8; 32], u64>,
    /// Connected peers.
    peers: RwLock<Vec<PeerInfo>>,
}

impl GossipProtocol {
    /// Creates a new gossip protocol instance with the given configuration.
    pub fn new(config: GossipConfig) -> Self {
        Self {
            config,
            seen_messages: DashMap::new(),
            peers: RwLock::new(Vec::new()),
        }
    }

    /// Broadcasts a message to all connected peers (up to fanout limit).
    ///
    /// Returns the list of actions to execute. Typically this is a single
    /// `Forward` action targeting `min(fanout, peer_count)` peers.
    pub fn broadcast(&self, message: GossipMessage) -> Vec<GossipAction> {
        let hash = message.content_hash();

        // Mark as seen so we don't process our own broadcast.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.seen_messages.insert(hash, now);

        // Select target peers (up to fanout).
        let peers = self.peers.read();
        let target_peers: Vec<String> = peers
            .iter()
            .take(self.config.fanout)
            .map(|p| p.peer_id.clone())
            .collect();

        if target_peers.is_empty() {
            debug!("no peers connected, message will not be forwarded");
            return vec![];
        }

        vec![GossipAction::Forward {
            message,
            target_peers,
        }]
    }

    /// Handles an incoming gossip message from a peer.
    ///
    /// Returns a list of actions to execute. The actions may include
    /// forwarding the message, adding transactions to the mempool,
    /// processing blocks, or adding newly discovered peers.
    pub fn handle_message(&self, peer_id: &str, message: GossipMessage) -> Vec<GossipAction> {
        let hash = message.content_hash();

        // Deduplication: drop if already seen.
        if self.seen_messages.contains_key(&hash) {
            trace!(peer = peer_id, "dropping duplicate gossip message");
            return vec![GossipAction::Drop];
        }

        // TTL check.
        if message.ttl() == 0 {
            trace!(peer = peer_id, "dropping expired gossip message (TTL=0)");
            return vec![GossipAction::Drop];
        }

        // Mark as seen.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.seen_messages.insert(hash, now);

        // Evict old entries if cache is full.
        self.maybe_evict_seen_cache();

        let mut actions = Vec::new();

        // Process based on message type.
        match &message {
            GossipMessage::NewTransaction { transaction, .. } => {
                actions.push(GossipAction::AddToMempool(transaction.clone()));
            }
            GossipMessage::NewBlock { block, .. } => {
                actions.push(GossipAction::ProcessBlock(block.clone()));
            }
            GossipMessage::PeerDiscovery { known_peers, .. } => {
                actions.push(GossipAction::AddPeers(known_peers.clone()));
            }
        }

        // Forward with decremented TTL.
        if let Some(forwarded) = message.decrement_ttl() {
            let peers = self.peers.read();
            let target_peers: Vec<String> = peers
                .iter()
                .filter(|p| p.peer_id != peer_id) // Don't send back to sender.
                .take(self.config.fanout)
                .map(|p| p.peer_id.clone())
                .collect();

            if !target_peers.is_empty() {
                actions.push(GossipAction::Forward {
                    message: forwarded,
                    target_peers,
                });
            }
        }

        actions
    }

    /// Adds a peer to the gossip protocol's peer set.
    pub fn add_peer(&self, peer: PeerInfo) {
        let mut peers = self.peers.write();
        if peers.len() < self.config.max_peers && !peers.iter().any(|p| p.peer_id == peer.peer_id) {
            peers.push(peer);
        }
    }

    /// Removes a peer from the gossip protocol's peer set.
    pub fn remove_peer(&self, peer_id: &str) {
        let mut peers = self.peers.write();
        peers.retain(|p| p.peer_id != peer_id);
    }

    /// Returns the number of connected peers.
    pub fn peer_count(&self) -> usize {
        self.peers.read().len()
    }

    /// Returns the number of messages in the deduplication cache.
    pub fn seen_count(&self) -> usize {
        self.seen_messages.len()
    }

    /// Evicts the oldest entries from the seen cache if it exceeds capacity.
    fn maybe_evict_seen_cache(&self) {
        if self.seen_messages.len() <= self.config.seen_cache_size {
            return;
        }

        // Simple eviction: remove entries until we're at 75% capacity.
        let target = self.config.seen_cache_size * 3 / 4;
        let mut entries: Vec<([u8; 32], u64)> = self
            .seen_messages
            .iter()
            .map(|entry| (*entry.key(), *entry.value()))
            .collect();

        entries.sort_by_key(|(_, ts)| *ts);

        let to_remove = entries.len().saturating_sub(target);
        for (hash, _) in entries.iter().take(to_remove) {
            self.seen_messages.remove(hash);
        }
    }
}

// ===========================================================================
// Layer 2: libp2p Gossipsub Service
// ===========================================================================

// ---------------------------------------------------------------------------
// P2P Gossip Message (wire format)
// ---------------------------------------------------------------------------

/// Messages sent over the libp2p gossipsub wire protocol.
///
/// Unlike the epidemic `GossipMessage` which carries TTL metadata for
/// application-level forwarding, `P2pGossipMessage` is a clean envelope
/// for the three types of data that flow through the network. Gossipsub
/// handles deduplication and mesh propagation at the transport level, so
/// we don't need TTL here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum P2pGossipMessage {
    /// A new transaction broadcast by a client or relayed by a peer.
    NewTransaction(Transaction),
    /// A new block proposed by a validator during consensus.
    NewBlock(Block),
    /// A consensus vote (prevote or precommit) from a validator.
    BlockVote(Vote),
}

// ---------------------------------------------------------------------------
// Gossip Topics
// ---------------------------------------------------------------------------

/// Topic strings for the three gossipsub channels.
///
/// Each message type gets its own topic so that nodes can subscribe
/// selectively. Light clients might subscribe only to blocks, while
/// validators need all three. Separate topics also let us tune gossipsub
/// parameters (mesh size, scoring) per message type if needed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GossipTopics {
    /// Topic for transaction broadcasts.
    pub transactions: String,
    /// Topic for new block announcements.
    pub blocks: String,
    /// Topic for consensus votes.
    pub votes: String,
}

impl Default for GossipTopics {
    fn default() -> Self {
        Self {
            transactions: "nova-transactions".to_string(),
            blocks: "nova-blocks".to_string(),
            votes: "nova-votes".to_string(),
        }
    }
}

impl GossipTopics {
    /// Returns the transaction topic as a gossipsub `IdentTopic`.
    pub fn transactions_topic(&self) -> IdentTopic {
        IdentTopic::new(&self.transactions)
    }

    /// Returns the blocks topic as a gossipsub `IdentTopic`.
    pub fn blocks_topic(&self) -> IdentTopic {
        IdentTopic::new(&self.blocks)
    }

    /// Returns the votes topic as a gossipsub `IdentTopic`.
    pub fn votes_topic(&self) -> IdentTopic {
        IdentTopic::new(&self.votes)
    }
}

// ---------------------------------------------------------------------------
// Service Configuration
// ---------------------------------------------------------------------------

/// Configuration for the libp2p gossipsub service.
///
/// Controls the listening address, topic names, and gossipsub mesh parameters.
/// Defaults are tuned for a network of 50-200 validators with commodity
/// hardware — adjust `mesh_n` and friends for larger deployments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GossipServiceConfig {
    /// Multiaddr to listen on (e.g., "/ip4/0.0.0.0/tcp/9740").
    pub listen_addr: String,
    /// Topic configuration.
    pub topics: GossipTopics,
    /// Target number of peers in the gossipsub mesh per topic.
    /// Too low and messages propagate slowly; too high and you're
    /// wasting bandwidth on redundant copies.
    pub mesh_n: usize,
    /// Mesh lower bound. Gossipsub will graft new peers when the mesh
    /// drops below this threshold.
    pub mesh_n_low: usize,
    /// Mesh upper bound. Gossipsub will prune excess peers above this.
    pub mesh_n_high: usize,
    /// Heartbeat interval in milliseconds. Gossipsub uses heartbeats to
    /// maintain mesh health — grafting, pruning, and opportunistic relaying.
    pub heartbeat_interval_ms: u64,
    /// Maximum gossip message size in bytes. Messages exceeding this are
    /// dropped at the transport level before deserialization.
    pub max_message_size: usize,
}

impl Default for GossipServiceConfig {
    fn default() -> Self {
        Self {
            listen_addr: format!("/ip4/0.0.0.0/tcp/{}", crate::config::DEFAULT_P2P_PORT),
            topics: GossipTopics::default(),
            mesh_n: 6,
            mesh_n_low: 4,
            mesh_n_high: 12,
            heartbeat_interval_ms: 1000,
            max_message_size: 1024 * 1024, // 1 MiB — enough for the largest blocks.
        }
    }
}

// ---------------------------------------------------------------------------
// Gossip Error
// ---------------------------------------------------------------------------

/// Errors originating from the gossip service layer.
///
/// Intentionally coarse-grained — callers care about *what* failed, not the
/// exact libp2p error variant three layers deep. The inner `String` carries
/// enough context for debugging without leaking implementation details.
#[derive(Debug)]
pub enum GossipError {
    /// Bincode serialization or deserialization failed.
    Serialization(String),
    /// Failed to publish a message to a gossipsub topic.
    PublishError(String),
    /// Failed to subscribe to a gossipsub topic.
    SubscriptionError(String),
    /// Transport-level error (TCP, Noise, Yamux).
    TransportError(String),
    /// Received message that could not be decoded into a known type.
    InvalidMessage(String),
}

impl fmt::Display for GossipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Serialization(msg) => write!(f, "serialization error: {}", msg),
            Self::PublishError(msg) => write!(f, "publish error: {}", msg),
            Self::SubscriptionError(msg) => write!(f, "subscription error: {}", msg),
            Self::TransportError(msg) => write!(f, "transport error: {}", msg),
            Self::InvalidMessage(msg) => write!(f, "invalid message: {}", msg),
        }
    }
}

impl std::error::Error for GossipError {}

// ---------------------------------------------------------------------------
// Combined Network Behaviour
// ---------------------------------------------------------------------------

/// The combined libp2p behaviour for NOVA nodes.
///
/// Gossipsub handles pub/sub message propagation. Identify lets peers
/// exchange metadata (protocol version, listen addresses) on connection,
/// which is essential for NAT traversal and peer discovery.
#[derive(NetworkBehaviour)]
pub struct GossipBehaviour {
    /// Gossipsub protocol for topic-based message propagation.
    pub gossipsub: gossipsub::Behaviour,
    /// Identify protocol for peer metadata exchange.
    pub identify: identify::Behaviour,
}

// ---------------------------------------------------------------------------
// Message Encoding / Decoding
// ---------------------------------------------------------------------------

/// Serialize a `P2pGossipMessage` to bincode bytes for wire transmission.
///
/// Bincode is deterministic for the same input, compact, and fast. The
/// encoded output is suitable for publishing directly to a gossipsub topic.
pub fn encode_message(msg: &P2pGossipMessage) -> Vec<u8> {
    // bincode::serialize returns Result but should never fail for our types
    // (no unsupported types like maps with non-string keys). Unwrap is safe.
    bincode::serialize(msg).expect("P2pGossipMessage serialization should never fail")
}

/// Deserialize bincode bytes back into a `P2pGossipMessage`.
///
/// Returns a `GossipError::Serialization` if the bytes are malformed or
/// truncated. This is expected for messages from misbehaving peers — the
/// caller should log and drop, not panic.
pub fn decode_message(data: &[u8]) -> Result<P2pGossipMessage, GossipError> {
    bincode::deserialize(data).map_err(|e| GossipError::Serialization(e.to_string()))
}

// ---------------------------------------------------------------------------
// Swarm Construction
// ---------------------------------------------------------------------------

/// Build a fully configured libp2p `Swarm` with gossipsub and identify.
///
/// The returned swarm is ready to listen and dial but is NOT yet running
/// its event loop. The caller (node binary) is responsible for:
///
/// 1. Calling `swarm.listen_on(...)` with the configured address.
/// 2. Subscribing to topics via `swarm.behaviour_mut().gossipsub.subscribe(...)`.
/// 3. Driving the swarm in a `tokio::select!` loop.
///
/// This function handles the fiddly libp2p plumbing so the node binary
/// doesn't have to care about Noise handshakes and Yamux configuration.
pub fn build_swarm(
    config: &GossipServiceConfig,
    keypair: &Keypair,
) -> Result<Swarm<GossipBehaviour>, GossipError> {
    // Gossipsub configuration with NOVA-specific tuning.
    let gossipsub_config = gossipsub::ConfigBuilder::default()
        .heartbeat_interval(Duration::from_millis(config.heartbeat_interval_ms))
        .mesh_n(config.mesh_n)
        .mesh_n_low(config.mesh_n_low)
        .mesh_n_high(config.mesh_n_high)
        .max_transmit_size(config.max_message_size)
        .validation_mode(gossipsub::ValidationMode::Strict)
        .build()
        .map_err(|e| GossipError::TransportError(format!("gossipsub config: {}", e)))?;

    // Messages are signed with the node's identity keypair. This prevents
    // message spoofing and enables gossipsub's peer scoring to attribute
    // messages correctly.
    let gossipsub_behaviour = gossipsub::Behaviour::new(
        MessageAuthenticity::Signed(keypair.clone()),
        gossipsub_config,
    )
    .map_err(|e| GossipError::TransportError(format!("gossipsub behaviour: {}", e)))?;

    // Identify protocol — exchange metadata on every new connection.
    let identify_config = identify::Config::new(
        format!("/nova/{}", crate::config::PROTOCOL_VERSION),
        keypair.public(),
    );
    let identify_behaviour = identify::Behaviour::new(identify_config);

    let behaviour = GossipBehaviour {
        gossipsub: gossipsub_behaviour,
        identify: identify_behaviour,
    };

    let swarm = libp2p::SwarmBuilder::with_existing_identity(keypair.clone())
        .with_tokio()
        .with_tcp(
            libp2p::tcp::Config::default(),
            libp2p::noise::Config::new,
            libp2p::yamux::Config::default,
        )
        .map_err(|e| GossipError::TransportError(format!("tcp transport: {}", e)))?
        .with_behaviour(|_| behaviour)
        .map_err(|e| GossipError::TransportError(format!("behaviour: {}", e)))?
        .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    Ok(swarm)
}

// ---------------------------------------------------------------------------
// Gossip Service
// ---------------------------------------------------------------------------

/// High-level gossip service that owns the outbound message channel.
///
/// The service is the public-facing API for broadcasting messages to the
/// network. Internally, it pushes `P2pGossipMessage` values onto an
/// unbounded channel. The node binary reads from the receiver side and
/// publishes to the gossipsub swarm.
///
/// This design decouples message creation (which can happen on any thread)
/// from the async swarm event loop (which owns the `Swarm`).
pub struct GossipService {
    /// Service configuration.
    config: GossipServiceConfig,
    /// The local peer identity. Derived deterministically from the node's
    /// Ed25519 keypair — same key, same PeerId, every time.
    local_peer_id: PeerId,
    /// Outbound message sender. Messages pushed here are consumed by the
    /// swarm event loop for publication to the appropriate gossipsub topic.
    tx_sender: mpsc::UnboundedSender<P2pGossipMessage>,
}

impl GossipService {
    /// Create a new gossip service.
    ///
    /// Derives the `PeerId` from the keypair and opens the outbound
    /// message channel. The returned receiver should be consumed by
    /// the swarm event loop in the node binary.
    pub fn new(
        config: GossipServiceConfig,
        keypair: &Keypair,
    ) -> (Self, mpsc::UnboundedReceiver<P2pGossipMessage>) {
        let local_peer_id = PeerId::from(keypair.public());
        let (tx_sender, rx_receiver) = mpsc::unbounded_channel();

        let service = Self {
            config,
            local_peer_id,
            tx_sender,
        };

        (service, rx_receiver)
    }

    /// Returns the local peer ID.
    pub fn local_peer_id(&self) -> &PeerId {
        &self.local_peer_id
    }

    /// Returns a reference to the service configuration.
    pub fn config(&self) -> &GossipServiceConfig {
        &self.config
    }

    /// Publish a transaction to the network.
    ///
    /// The message is queued for publication on the `nova-transactions`
    /// topic. Actual network I/O happens asynchronously in the swarm loop.
    pub fn publish_transaction(&self, tx: &Transaction) -> Result<(), GossipError> {
        let msg = P2pGossipMessage::NewTransaction(tx.clone());
        self.tx_sender
            .send(msg)
            .map_err(|e| GossipError::PublishError(format!("channel closed: {}", e)))
    }

    /// Publish a block to the network.
    ///
    /// The message is queued for publication on the `nova-blocks` topic.
    pub fn publish_block(&self, block: &Block) -> Result<(), GossipError> {
        let msg = P2pGossipMessage::NewBlock(block.clone());
        self.tx_sender
            .send(msg)
            .map_err(|e| GossipError::PublishError(format!("channel closed: {}", e)))
    }

    /// Publish a consensus vote to the network.
    ///
    /// The message is queued for publication on the `nova-votes` topic.
    pub fn publish_vote(&self, vote: &Vote) -> Result<(), GossipError> {
        let msg = P2pGossipMessage::BlockVote(vote.clone());
        self.tx_sender
            .send(msg)
            .map_err(|e| GossipError::PublishError(format!("channel closed: {}", e)))
    }

    /// Determine which topic a `P2pGossipMessage` should be published to.
    ///
    /// Used by the swarm event loop to route outbound messages to the
    /// correct gossipsub topic.
    pub fn topic_for_message(&self, msg: &P2pGossipMessage) -> IdentTopic {
        match msg {
            P2pGossipMessage::NewTransaction(_) => self.config.topics.transactions_topic(),
            P2pGossipMessage::NewBlock(_) => self.config.topics.blocks_topic(),
            P2pGossipMessage::BlockVote(_) => self.config.topics.votes_topic(),
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::keys::NovaKeypair;
    use crate::storage::Block;
    use crate::transaction::builder::TransactionBuilder;
    use crate::transaction::types::{Amount, Currency, TransactionType};

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    fn make_config() -> GossipConfig {
        GossipConfig {
            max_peers: 10,
            seen_cache_size: 100,
            ..GossipConfig::default()
        }
    }

    fn make_peer(id: &str) -> PeerInfo {
        PeerInfo {
            peer_id: id.to_string(),
            address: format!("/ip4/127.0.0.1/tcp/9740"),
            connected_at: 1000,
            last_seen: 1000,
        }
    }

    fn make_test_tx(nonce: u64) -> Transaction {
        TransactionBuilder::new(TransactionType::Transfer)
            .sender("nova1alice")
            .receiver("nova1bob")
            .amount(Amount::new(1_000_000, Currency::NOVA))
            .fee(100)
            .nonce(nonce)
            .timestamp(1_700_000_000_000)
            .build()
    }

    fn make_test_block() -> Block {
        Block::genesis()
    }

    fn make_test_vote() -> Vote {
        let keypair = NovaKeypair::generate();
        Vote::new(&keypair, [42u8; 32], 1)
    }

    // -----------------------------------------------------------------------
    // Layer 1: Epidemic gossip tests (preserved from original)
    // -----------------------------------------------------------------------

    #[test]
    fn duplicate_messages_are_dropped() {
        let proto = GossipProtocol::new(make_config());
        proto.add_peer(make_peer("peer-1"));

        let msg = GossipMessage::PeerDiscovery {
            peer: make_peer("sender"),
            known_peers: vec![],
            ttl: 5,
        };

        let actions1 = proto.handle_message("peer-1", msg.clone());
        assert!(!actions1.iter().any(|a| matches!(a, GossipAction::Drop)));

        let actions2 = proto.handle_message("peer-1", msg);
        assert!(actions2.iter().any(|a| matches!(a, GossipAction::Drop)));
    }

    #[test]
    fn expired_ttl_is_dropped() {
        let proto = GossipProtocol::new(make_config());
        proto.add_peer(make_peer("peer-1"));

        let msg = GossipMessage::PeerDiscovery {
            peer: make_peer("sender"),
            known_peers: vec![],
            ttl: 0,
        };

        let actions = proto.handle_message("peer-1", msg);
        assert!(actions.iter().any(|a| matches!(a, GossipAction::Drop)));
    }

    #[test]
    fn message_is_forwarded_with_decremented_ttl() {
        let proto = GossipProtocol::new(make_config());
        proto.add_peer(make_peer("peer-1"));
        proto.add_peer(make_peer("peer-2"));

        let msg = GossipMessage::PeerDiscovery {
            peer: make_peer("sender"),
            known_peers: vec![],
            ttl: 5,
        };

        let actions = proto.handle_message("peer-1", msg);

        let forward_action = actions
            .iter()
            .find(|a| matches!(a, GossipAction::Forward { .. }));
        assert!(forward_action.is_some());

        if let Some(GossipAction::Forward {
            message,
            target_peers,
        }) = forward_action
        {
            assert_eq!(message.ttl(), 4);
            // Should not forward back to sender.
            assert!(!target_peers.contains(&"peer-1".to_string()));
        }
    }

    #[test]
    fn ttl_1_message_is_not_forwarded() {
        let proto = GossipProtocol::new(make_config());
        proto.add_peer(make_peer("peer-1"));
        proto.add_peer(make_peer("peer-2"));

        let msg = GossipMessage::PeerDiscovery {
            peer: make_peer("sender"),
            known_peers: vec![],
            ttl: 1,
        };

        let actions = proto.handle_message("peer-1", msg);

        // Should process the message but NOT forward (TTL would be 0).
        let has_forward = actions
            .iter()
            .any(|a| matches!(a, GossipAction::Forward { .. }));
        assert!(!has_forward);
    }

    #[test]
    fn peer_management() {
        let proto = GossipProtocol::new(make_config());
        assert_eq!(proto.peer_count(), 0);

        proto.add_peer(make_peer("p1"));
        proto.add_peer(make_peer("p2"));
        assert_eq!(proto.peer_count(), 2);

        // Duplicate peer is not added.
        proto.add_peer(make_peer("p1"));
        assert_eq!(proto.peer_count(), 2);

        proto.remove_peer("p1");
        assert_eq!(proto.peer_count(), 1);
    }

    // -----------------------------------------------------------------------
    // Layer 2: libp2p gossipsub tests
    // -----------------------------------------------------------------------

    #[test]
    fn encode_decode_transaction_message() {
        let tx = make_test_tx(1);
        let msg = P2pGossipMessage::NewTransaction(tx.clone());

        let encoded = encode_message(&msg);
        let decoded = decode_message(&encoded).expect("should decode");

        match decoded {
            P2pGossipMessage::NewTransaction(decoded_tx) => {
                assert_eq!(decoded_tx.id, tx.id);
                assert_eq!(decoded_tx.sender, tx.sender);
                assert_eq!(decoded_tx.receiver, tx.receiver);
                assert_eq!(decoded_tx.fee, tx.fee);
                assert_eq!(decoded_tx.nonce, tx.nonce);
            }
            other => panic!("expected NewTransaction, got {:?}", other),
        }
    }

    #[test]
    fn encode_decode_block_message() {
        let block = make_test_block();
        let msg = P2pGossipMessage::NewBlock(block.clone());

        let encoded = encode_message(&msg);
        let decoded = decode_message(&encoded).expect("should decode");

        match decoded {
            P2pGossipMessage::NewBlock(decoded_block) => {
                assert_eq!(decoded_block.header.height, block.header.height);
                assert_eq!(decoded_block.header.hash, block.header.hash);
                assert_eq!(decoded_block.transactions.len(), block.transactions.len());
            }
            other => panic!("expected NewBlock, got {:?}", other),
        }
    }

    #[test]
    fn encode_decode_vote_message() {
        let vote = make_test_vote();
        let msg = P2pGossipMessage::BlockVote(vote.clone());

        let encoded = encode_message(&msg);
        let decoded = decode_message(&encoded).expect("should decode");

        match decoded {
            P2pGossipMessage::BlockVote(decoded_vote) => {
                assert_eq!(decoded_vote.validator, vote.validator);
                assert_eq!(decoded_vote.block_hash, vote.block_hash);
                assert_eq!(decoded_vote.round, vote.round);
            }
            other => panic!("expected BlockVote, got {:?}", other),
        }
    }

    #[test]
    fn gossip_service_config_defaults() {
        let config = GossipServiceConfig::default();
        assert_eq!(config.mesh_n, 6);
        assert_eq!(config.mesh_n_low, 4);
        assert_eq!(config.mesh_n_high, 12);
        assert_eq!(config.heartbeat_interval_ms, 1000);
        assert_eq!(config.max_message_size, 1024 * 1024);
        assert!(config.listen_addr.contains("9740"));
    }

    #[test]
    fn gossip_topics_correct() {
        let topics = GossipTopics::default();
        assert_eq!(topics.transactions, "nova-transactions");
        assert_eq!(topics.blocks, "nova-blocks");
        assert_eq!(topics.votes, "nova-votes");
    }

    #[test]
    fn invalid_message_decode_fails() {
        let garbage = vec![0xFF, 0xFE, 0xFD, 0xFC, 0x00, 0x01, 0x02, 0x03];
        let result = decode_message(&garbage);
        assert!(result.is_err());

        match result {
            Err(GossipError::Serialization(msg)) => {
                assert!(!msg.is_empty(), "error message should be descriptive");
            }
            other => panic!("expected Serialization error, got {:?}", other),
        }
    }

    #[test]
    fn message_size_reasonable() {
        let tx = make_test_tx(1);
        let msg = P2pGossipMessage::NewTransaction(tx);
        let encoded = encode_message(&msg);

        // A single transaction message should be well under the 1 MiB limit.
        let max_size = GossipServiceConfig::default().max_message_size;
        assert!(
            encoded.len() < max_size,
            "encoded size {} exceeds max {}",
            encoded.len(),
            max_size
        );
        // Sanity: it should be at least a few hundred bytes (not empty).
        assert!(encoded.len() > 50, "encoded message suspiciously small");
    }

    #[test]
    fn block_message_with_transactions() {
        let genesis = Block::genesis();
        let txs = vec![make_test_tx(1), make_test_tx(2), make_test_tx(3)];
        let block = Block::new(&genesis, txs, "nova:validator".to_string(), [99u8; 32]);

        let msg = P2pGossipMessage::NewBlock(block.clone());
        let encoded = encode_message(&msg);
        let decoded = decode_message(&encoded).expect("should decode");

        match decoded {
            P2pGossipMessage::NewBlock(decoded_block) => {
                assert_eq!(decoded_block.transactions.len(), 3);
                assert_eq!(decoded_block.header.height, 1);
                assert_eq!(decoded_block.header.parent_hash, genesis.header.hash);
            }
            other => panic!("expected NewBlock, got {:?}", other),
        }
    }

    #[test]
    fn empty_block_message() {
        let block = Block::genesis();
        let msg = P2pGossipMessage::NewBlock(block.clone());

        let encoded = encode_message(&msg);
        let decoded = decode_message(&encoded).expect("should decode");

        match decoded {
            P2pGossipMessage::NewBlock(decoded_block) => {
                assert!(decoded_block.transactions.is_empty());
                assert_eq!(decoded_block.header.height, 0);
            }
            other => panic!("expected NewBlock, got {:?}", other),
        }
    }

    #[test]
    fn vote_message_fields_preserved() {
        let keypair = NovaKeypair::generate();
        let block_hash = [0xAB; 32];
        let round = 42;
        let vote = Vote::new(&keypair, block_hash, round);

        let msg = P2pGossipMessage::BlockVote(vote.clone());
        let encoded = encode_message(&msg);
        let decoded = decode_message(&encoded).expect("should decode");

        match decoded {
            P2pGossipMessage::BlockVote(v) => {
                assert_eq!(v.block_hash, block_hash);
                assert_eq!(v.round, round);
                assert_eq!(v.validator, vote.validator);
                // Signature bytes should survive the roundtrip.
                assert_eq!(v.signature.as_bytes(), vote.signature.as_bytes());
            }
            other => panic!("expected BlockVote, got {:?}", other),
        }
    }

    #[test]
    fn large_block_serialization() {
        let genesis = Block::genesis();
        let txs: Vec<Transaction> = (0..100).map(|i| make_test_tx(i)).collect();
        let block = Block::new(&genesis, txs, "nova:validator".to_string(), [1u8; 32]);

        let msg = P2pGossipMessage::NewBlock(block.clone());
        let encoded = encode_message(&msg);

        // Should still fit under the 1 MiB max.
        let max_size = GossipServiceConfig::default().max_message_size;
        assert!(
            encoded.len() < max_size,
            "block with 100 txs ({} bytes) exceeds max {}",
            encoded.len(),
            max_size
        );

        let decoded = decode_message(&encoded).expect("should decode large block");
        match decoded {
            P2pGossipMessage::NewBlock(b) => {
                assert_eq!(b.transactions.len(), 100);
            }
            other => panic!("expected NewBlock, got {:?}", other),
        }
    }

    #[test]
    fn gossip_service_new() {
        let keypair = Keypair::generate_ed25519();
        let config = GossipServiceConfig::default();
        let (service, _rx) = GossipService::new(config, &keypair);

        let expected_peer_id = PeerId::from(keypair.public());
        assert_eq!(*service.local_peer_id(), expected_peer_id);
    }

    #[test]
    fn peer_id_deterministic() {
        let keypair = Keypair::generate_ed25519();
        let config = GossipServiceConfig::default();

        let (service1, _rx1) = GossipService::new(config.clone(), &keypair);
        let (service2, _rx2) = GossipService::new(config, &keypair);

        assert_eq!(
            service1.local_peer_id(),
            service2.local_peer_id(),
            "same keypair must produce the same PeerId"
        );
    }

    #[test]
    fn config_custom_values() {
        let config = GossipServiceConfig {
            listen_addr: "/ip4/127.0.0.1/tcp/12345".to_string(),
            topics: GossipTopics {
                transactions: "custom-tx".to_string(),
                blocks: "custom-blocks".to_string(),
                votes: "custom-votes".to_string(),
            },
            mesh_n: 8,
            mesh_n_low: 5,
            mesh_n_high: 15,
            heartbeat_interval_ms: 2000,
            max_message_size: 2 * 1024 * 1024,
        };

        assert_eq!(config.listen_addr, "/ip4/127.0.0.1/tcp/12345");
        assert_eq!(config.topics.transactions, "custom-tx");
        assert_eq!(config.topics.blocks, "custom-blocks");
        assert_eq!(config.topics.votes, "custom-votes");
        assert_eq!(config.mesh_n, 8);
        assert_eq!(config.mesh_n_low, 5);
        assert_eq!(config.mesh_n_high, 15);
        assert_eq!(config.heartbeat_interval_ms, 2000);
        assert_eq!(config.max_message_size, 2 * 1024 * 1024);
    }

    #[test]
    fn topics_struct_creation() {
        let topics = GossipTopics {
            transactions: "test-tx-topic".to_string(),
            blocks: "test-block-topic".to_string(),
            votes: "test-vote-topic".to_string(),
        };

        // Verify the IdentTopic conversion works.
        let tx_topic = topics.transactions_topic();
        let block_topic = topics.blocks_topic();
        let vote_topic = topics.votes_topic();

        // IdentTopic hashes should be different for different topic strings.
        assert_ne!(tx_topic.hash(), block_topic.hash());
        assert_ne!(tx_topic.hash(), vote_topic.hash());
        assert_ne!(block_topic.hash(), vote_topic.hash());
    }

    #[test]
    fn gossip_service_publish_transaction() {
        let keypair = Keypair::generate_ed25519();
        let config = GossipServiceConfig::default();
        let (service, mut rx) = GossipService::new(config, &keypair);

        let tx = make_test_tx(42);
        service.publish_transaction(&tx).expect("publish should succeed");

        let received = rx.try_recv().expect("should receive message");
        match received {
            P2pGossipMessage::NewTransaction(received_tx) => {
                assert_eq!(received_tx.id, tx.id);
            }
            other => panic!("expected NewTransaction, got {:?}", other),
        }
    }

    #[test]
    fn gossip_service_publish_block() {
        let keypair = Keypair::generate_ed25519();
        let config = GossipServiceConfig::default();
        let (service, mut rx) = GossipService::new(config, &keypair);

        let block = make_test_block();
        service.publish_block(&block).expect("publish should succeed");

        let received = rx.try_recv().expect("should receive message");
        match received {
            P2pGossipMessage::NewBlock(received_block) => {
                assert_eq!(received_block.header.hash, block.header.hash);
            }
            other => panic!("expected NewBlock, got {:?}", other),
        }
    }

    #[test]
    fn gossip_service_publish_vote() {
        let keypair = Keypair::generate_ed25519();
        let config = GossipServiceConfig::default();
        let (service, mut rx) = GossipService::new(config, &keypair);

        let vote = make_test_vote();
        service.publish_vote(&vote).expect("publish should succeed");

        let received = rx.try_recv().expect("should receive message");
        match received {
            P2pGossipMessage::BlockVote(received_vote) => {
                assert_eq!(received_vote.round, vote.round);
                assert_eq!(received_vote.block_hash, vote.block_hash);
            }
            other => panic!("expected BlockVote, got {:?}", other),
        }
    }

    #[test]
    fn gossip_service_topic_routing() {
        let keypair = Keypair::generate_ed25519();
        let config = GossipServiceConfig::default();
        let (service, _rx) = GossipService::new(config, &keypair);

        let tx_msg = P2pGossipMessage::NewTransaction(make_test_tx(1));
        let block_msg = P2pGossipMessage::NewBlock(make_test_block());
        let vote_msg = P2pGossipMessage::BlockVote(make_test_vote());

        let tx_topic = service.topic_for_message(&tx_msg);
        let block_topic = service.topic_for_message(&block_msg);
        let vote_topic = service.topic_for_message(&vote_msg);

        // Each message type should route to a different topic.
        assert_ne!(tx_topic.hash(), block_topic.hash());
        assert_ne!(tx_topic.hash(), vote_topic.hash());
        assert_ne!(block_topic.hash(), vote_topic.hash());
    }

    #[test]
    fn gossip_error_display() {
        let errors = vec![
            GossipError::Serialization("bad bytes".to_string()),
            GossipError::PublishError("channel full".to_string()),
            GossipError::SubscriptionError("topic not found".to_string()),
            GossipError::TransportError("connection refused".to_string()),
            GossipError::InvalidMessage("unknown variant".to_string()),
        ];

        for err in &errors {
            let display = format!("{}", err);
            assert!(!display.is_empty(), "error display should not be empty");
        }

        // Verify each variant produces a distinct prefix.
        assert!(format!("{}", errors[0]).starts_with("serialization"));
        assert!(format!("{}", errors[1]).starts_with("publish"));
        assert!(format!("{}", errors[2]).starts_with("subscription"));
        assert!(format!("{}", errors[3]).starts_with("transport"));
        assert!(format!("{}", errors[4]).starts_with("invalid"));
    }

    #[test]
    fn empty_data_decode_fails() {
        let result = decode_message(&[]);
        assert!(result.is_err());
    }
}
