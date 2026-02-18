//! # Block Structure
//!
//! A block is the atomic unit of consensus in NOVA. Each block contains
//! an ordered list of transactions, a link to the previous block (forming
//! the chain), and cryptographic proofs of integrity.
//!
//! ## Block Layout
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │  BlockHeader                                │
//! │  ├── height: u64                            │
//! │  ├── hash: [u8; 32]       (BLAKE3 of header)│
//! │  ├── parent_hash: [u8; 32]                  │
//! │  ├── timestamp: u64                         │
//! │  ├── validator: String                      │
//! │  ├── state_root: [u8; 32]                   │
//! │  ├── tx_root: [u8; 32]   (Merkle root)      │
//! │  └── signature: Vec<u8>                     │
//! ├─────────────────────────────────────────────┤
//! │  transactions: Vec<Transaction>             │
//! └─────────────────────────────────────────────┘
//! ```
//!
//! ## Hash Computation
//!
//! The block hash covers: `height || parent_hash || timestamp || validator
//! || state_root || tx_root`. The signature is NOT included in the hash
//! (it signs the hash, not the other way around).
//!
//! ## Merkle Root
//!
//! The `tx_root` is a binary Merkle tree over the BLAKE3 hashes of each
//! transaction's canonical serialization. Empty blocks have a tx_root of
//! all zeros.

use serde::{Deserialize, Serialize};

use crate::crypto::hash::blake3_hash;
use crate::transaction::Transaction;

/// Coinbase message embedded in the genesis block state root.
/// This serves as the protocol's birth certificate — a timestamped,
/// tamper-evident record of when and why the network was created.
/// (Satoshi had "The Times 03/Jan/2009"; we have this.)
pub const GENESIS_COINBASE_MESSAGE: &[u8] =
    b"ALAS/2026: The future of payments belongs to everyone";

// ---------------------------------------------------------------------------
// BlockHeader
// ---------------------------------------------------------------------------

/// Lightweight block header — everything except the transaction list.
///
/// Light clients sync headers to verify the chain without downloading
/// full block data. The header contains the Merkle root of transactions,
/// so a client can verify individual transaction inclusion via Merkle proofs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockHeader {
    /// Block height (0-indexed, genesis = 0).
    pub height: u64,
    /// BLAKE3 hash of this block's header fields.
    pub hash: [u8; 32],
    /// Hash of the parent block's header. All zeros for genesis.
    pub parent_hash: [u8; 32],
    /// Unix timestamp (milliseconds) when this block was produced.
    pub timestamp: u64,
    /// NOVA address (hex public key) of the validator that proposed this block.
    pub validator: String,
    /// Root hash of the state tree after applying this block's transactions.
    pub state_root: [u8; 32],
    /// Merkle root of the transactions in this block.
    pub tx_root: [u8; 32],
    /// Ed25519 signature of the validator over the block hash.
    pub signature: Vec<u8>,
}

impl BlockHeader {
    /// Return the block hash as a hex string.
    pub fn hash_hex(&self) -> String {
        hex::encode(self.hash)
    }

    /// Return the parent hash as a hex string.
    pub fn parent_hash_hex(&self) -> String {
        hex::encode(self.parent_hash)
    }
}

// ---------------------------------------------------------------------------
// Block
// ---------------------------------------------------------------------------

/// A full NOVA block: header + ordered transaction list.
///
/// Blocks are immutable after construction. The hash is computed from
/// the header fields (excluding the signature), and the signature
/// covers the hash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    /// Block metadata and chain linkage.
    pub header: BlockHeader,
    /// Ordered list of transactions included in this block.
    pub transactions: Vec<Transaction>,
}

impl Block {
    /// Construct the genesis block.
    ///
    /// The genesis block has height 0, parent_hash of all zeros, an empty
    /// transaction list, and a well-known validator address. The state_root
    /// represents the initial state of the network (e.g., pre-minted supply).
    pub fn genesis() -> Self {
        let genesis_validator =
            "nova:0000000000000000000000000000000000000000000000000000000000000000".to_string();

        let timestamp = 0u64; // Epoch zero — the dawn of NOVA.

        // The genesis state root is the hash of the coinbase message,
        // anchoring the protocol's origin into the chain's cryptographic history.
        let state_root = blake3_hash(GENESIS_COINBASE_MESSAGE);
        let tx_root = [0u8; 32]; // No transactions.

        let hash = compute_header_hash(
            0,
            &[0u8; 32],
            timestamp,
            &genesis_validator,
            &state_root,
            &tx_root,
        );

        Block {
            header: BlockHeader {
                height: 0,
                hash,
                parent_hash: [0u8; 32],
                timestamp,
                validator: genesis_validator,
                state_root,
                tx_root,
                signature: Vec::new(), // Genesis block is unsigned.
            },
            transactions: Vec::new(),
        }
    }

    /// Construct a new block linked to a parent.
    ///
    /// Computes the tx Merkle root from the transaction list and the
    /// block hash from the header fields. The signature field is left
    /// empty — the validator signs separately after construction.
    ///
    /// # Arguments
    ///
    /// * `parent` — The parent block this block extends.
    /// * `transactions` — Ordered transactions to include.
    /// * `validator` — NOVA address of the block proposer.
    /// * `state_root` — Root hash of the state tree after applying transactions.
    pub fn new(
        parent: &Block,
        transactions: Vec<Transaction>,
        validator: String,
        state_root: [u8; 32],
    ) -> Self {
        let height = parent.header.height + 1;
        let parent_hash = parent.header.hash;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let tx_root = compute_merkle_root(&transactions);
        let hash = compute_header_hash(
            height,
            &parent_hash,
            timestamp,
            &validator,
            &state_root,
            &tx_root,
        );

        Block {
            header: BlockHeader {
                height,
                hash,
                parent_hash,
                timestamp,
                validator,
                state_root,
                tx_root,
                signature: Vec::new(),
            },
            transactions,
        }
    }

    /// Recompute the block hash from header fields.
    ///
    /// Use this to verify that `header.hash` matches the actual content.
    pub fn compute_hash(&self) -> [u8; 32] {
        compute_header_hash(
            self.header.height,
            &self.header.parent_hash,
            self.header.timestamp,
            &self.header.validator,
            &self.header.state_root,
            &self.header.tx_root,
        )
    }

    /// Verify block integrity: hash consistency, tx Merkle root, and
    /// structural invariants.
    ///
    /// This does NOT verify the validator's signature (that requires the
    /// validator's public key from the state tree). It checks:
    ///
    /// 1. The stored hash matches the recomputed hash.
    /// 2. The stored tx_root matches the recomputed Merkle root.
    /// 3. Genesis blocks have height 0 and zeroed parent_hash.
    ///
    /// # Errors
    ///
    /// Returns a descriptive error string on any mismatch.
    pub fn verify(&self) -> Result<(), String> {
        // 1. Verify block hash.
        let expected_hash = self.compute_hash();
        if self.header.hash != expected_hash {
            return Err(format!(
                "block {} hash mismatch: stored={}, computed={}",
                self.header.height,
                hex::encode(self.header.hash),
                hex::encode(expected_hash),
            ));
        }

        // 2. Verify tx Merkle root.
        let expected_tx_root = compute_merkle_root(&self.transactions);
        if self.header.tx_root != expected_tx_root {
            return Err(format!(
                "block {} tx_root mismatch: stored={}, computed={}",
                self.header.height,
                hex::encode(self.header.tx_root),
                hex::encode(expected_tx_root),
            ));
        }

        // 3. Genesis-specific checks.
        if self.header.height == 0 {
            if self.header.parent_hash != [0u8; 32] {
                return Err("genesis block must have zeroed parent_hash".to_string());
            }
        }

        Ok(())
    }

    /// Return the block height.
    pub fn height(&self) -> u64 {
        self.header.height
    }

    /// Return the number of transactions in this block.
    pub fn tx_count(&self) -> usize {
        self.transactions.len()
    }

    /// Return the block hash as a hex string.
    pub fn hash_hex(&self) -> String {
        hex::encode(self.header.hash)
    }
}

// ---------------------------------------------------------------------------
// Hash Computation
// ---------------------------------------------------------------------------

/// Compute the BLAKE3 hash of a block header from its constituent fields.
///
/// The hash covers: height || parent_hash || timestamp || validator ||
/// state_root || tx_root. The signature is NOT included.
fn compute_header_hash(
    height: u64,
    parent_hash: &[u8; 32],
    timestamp: u64,
    validator: &str,
    state_root: &[u8; 32],
    tx_root: &[u8; 32],
) -> [u8; 32] {
    let mut preimage = Vec::with_capacity(128);
    preimage.extend_from_slice(&height.to_le_bytes());
    preimage.extend_from_slice(parent_hash);
    preimage.extend_from_slice(&timestamp.to_le_bytes());
    preimage.extend_from_slice(validator.as_bytes());
    preimage.extend_from_slice(state_root);
    preimage.extend_from_slice(tx_root);
    blake3_hash(&preimage)
}

// ---------------------------------------------------------------------------
// Merkle Tree
// ---------------------------------------------------------------------------

/// Compute a binary Merkle tree root over a list of transactions.
///
/// Each leaf is the BLAKE3 hash of the transaction's canonical JSON
/// serialization. Internal nodes are `BLAKE3(left || right)`.
///
/// An empty list produces a root of all zeros. A single transaction
/// produces the hash of that transaction as the root.
pub fn compute_merkle_root(transactions: &[Transaction]) -> [u8; 32] {
    if transactions.is_empty() {
        return [0u8; 32];
    }

    // Compute leaf hashes.
    let mut hashes: Vec<[u8; 32]> = transactions
        .iter()
        .map(|tx| {
            let serialized = serde_json::to_vec(tx).unwrap_or_default();
            blake3_hash(&serialized)
        })
        .collect();

    // Build the tree bottom-up.
    while hashes.len() > 1 {
        let mut next_level = Vec::with_capacity((hashes.len() + 1) / 2);
        for chunk in hashes.chunks(2) {
            if chunk.len() == 2 {
                let mut combined = Vec::with_capacity(64);
                combined.extend_from_slice(&chunk[0]);
                combined.extend_from_slice(&chunk[1]);
                next_level.push(blake3_hash(&combined));
            } else {
                // Odd element — promote it unchanged (duplicate-left strategy).
                let mut combined = Vec::with_capacity(64);
                combined.extend_from_slice(&chunk[0]);
                combined.extend_from_slice(&chunk[0]);
                next_level.push(blake3_hash(&combined));
            }
        }
        hashes = next_level;
    }

    hashes[0]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::builder::TransactionBuilder;
    use crate::transaction::types::{Amount, Currency, TransactionType};

    fn make_test_tx(id_byte: u8) -> Transaction {
        TransactionBuilder::new(TransactionType::Transfer)
            .sender("nova:alice")
            .receiver("nova:bob")
            .amount(Amount::new(100, Currency::NOVA))
            .fee(100)
            .nonce(id_byte as u64)
            .timestamp(1_000_000)
            .build()
    }

    #[test]
    fn genesis_block_properties() {
        let genesis = Block::genesis();
        assert_eq!(genesis.height(), 0);
        assert_eq!(genesis.header.parent_hash, [0u8; 32]);
        assert_eq!(genesis.header.timestamp, 0);
        assert!(genesis.transactions.is_empty());
        assert!(genesis.header.signature.is_empty());
    }

    #[test]
    fn genesis_block_verifies() {
        let genesis = Block::genesis();
        assert!(genesis.verify().is_ok());
    }

    #[test]
    fn genesis_hash_is_deterministic() {
        let g1 = Block::genesis();
        let g2 = Block::genesis();
        assert_eq!(g1.header.hash, g2.header.hash);
    }

    #[test]
    fn new_block_links_to_parent() {
        let genesis = Block::genesis();
        let block1 = Block::new(&genesis, vec![], "nova:validator1".to_string(), [1u8; 32]);

        assert_eq!(block1.height(), 1);
        assert_eq!(block1.header.parent_hash, genesis.header.hash);
        assert_eq!(block1.header.state_root, [1u8; 32]);
    }

    #[test]
    fn new_block_verifies() {
        let genesis = Block::genesis();
        let txs = vec![make_test_tx(1), make_test_tx(2)];
        let block = Block::new(&genesis, txs, "nova:validator".to_string(), [42u8; 32]);

        assert!(block.verify().is_ok());
    }

    #[test]
    fn tampered_block_fails_verification() {
        let genesis = Block::genesis();
        let mut block = Block::new(&genesis, vec![], "nova:val".to_string(), [0u8; 32]);

        // Tamper with the stored hash.
        block.header.hash[0] ^= 0xFF;
        assert!(block.verify().is_err());
    }

    #[test]
    fn tampered_tx_root_fails_verification() {
        let genesis = Block::genesis();
        let txs = vec![make_test_tx(1)];
        let mut block = Block::new(&genesis, txs, "nova:val".to_string(), [0u8; 32]);

        // Tamper with the tx_root.
        block.header.tx_root[0] ^= 0xFF;
        // Also recompute hash to match the tampered root.
        block.header.hash = block.compute_hash();
        // Now the hash is consistent but the tx_root doesn't match the actual txs.
        assert!(block.verify().is_err());
    }

    #[test]
    fn merkle_root_empty() {
        assert_eq!(compute_merkle_root(&[]), [0u8; 32]);
    }

    #[test]
    fn merkle_root_single_tx() {
        let tx = make_test_tx(1);
        let root = compute_merkle_root(&[tx.clone()]);
        let expected = blake3_hash(&serde_json::to_vec(&tx).unwrap());
        assert_eq!(root, expected);
    }

    #[test]
    fn merkle_root_deterministic() {
        let txs = vec![make_test_tx(1), make_test_tx(2), make_test_tx(3)];
        let root1 = compute_merkle_root(&txs);
        let root2 = compute_merkle_root(&txs);
        assert_eq!(root1, root2);
    }

    #[test]
    fn merkle_root_order_sensitive() {
        let tx1 = make_test_tx(1);
        let tx2 = make_test_tx(2);

        let root_12 = compute_merkle_root(&[tx1.clone(), tx2.clone()]);
        let root_21 = compute_merkle_root(&[tx2, tx1]);
        assert_ne!(root_12, root_21, "Merkle root must be order-sensitive");
    }

    #[test]
    fn block_chain_of_three() {
        let b0 = Block::genesis();
        let b1 = Block::new(&b0, vec![make_test_tx(1)], "nova:v1".to_string(), [1u8; 32]);
        let b2 = Block::new(&b1, vec![make_test_tx(2)], "nova:v2".to_string(), [2u8; 32]);

        assert_eq!(b2.height(), 2);
        assert_eq!(b2.header.parent_hash, b1.header.hash);
        assert_eq!(b1.header.parent_hash, b0.header.hash);

        assert!(b0.verify().is_ok());
        assert!(b1.verify().is_ok());
        assert!(b2.verify().is_ok());
    }

    #[test]
    fn block_serialization_roundtrip() {
        let genesis = Block::genesis();
        let json = serde_json::to_string(&genesis).expect("serialize");
        let recovered: Block = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(genesis, recovered);
    }
}
