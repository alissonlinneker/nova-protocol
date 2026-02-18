//! # Gossip Protocol
//!
//! Epidemic-style message propagation for the NOVA P2P network. When a node
//! receives a new transaction or block, it forwards the message to a subset
//! of its peers (fanout). Each peer does the same, resulting in O(log N)
//! propagation across the network.
//!
//! ## Deduplication
//!
//! Every message is identified by its BLAKE3 content hash. Nodes maintain a
//! bounded set of recently seen message hashes (capped at `seen_cache_size`).
//! If a message hash is already in the set, the message is dropped instead
//! of being forwarded again. This prevents broadcast storms.
//!
//! ## TTL (Time-to-Live)
//!
//! Each gossip message carries a TTL counter that decrements on every hop.
//! When TTL reaches zero, the message is dropped regardless of whether the
//! node has seen it before. This bounds the propagation diameter and prevents
//! messages from circulating indefinitely in partitioned subgraphs.

use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{debug, trace};

use crate::storage::Block;
use crate::transaction::Transaction;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the gossip protocol layer.
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
// Gossip Messages
// ---------------------------------------------------------------------------

/// Messages propagated through the gossip layer.
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
// Gossip Protocol
// ---------------------------------------------------------------------------

/// The gossip protocol engine.
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
