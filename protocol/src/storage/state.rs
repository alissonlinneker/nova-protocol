//! # Sparse Merkle Tree — State Management
//!
//! The state tree maps NOVA addresses to account states using a Sparse Merkle
//! Tree (SMT) with a 256-bit key space. Every leaf is the serialized
//! `AccountState` for an address, and every internal node is
//! `BLAKE3(left_child || right_child)`. Empty subtrees collapse to
//! precomputed "default hashes" at each depth level, so the tree is always
//! exactly 256 levels deep but only materializes nodes along populated paths.
//!
//! ## Why a Sparse Merkle Tree?
//!
//! Classic dense Merkle trees grow linearly with the number of accounts. An
//! SMT is defined over the entire 2^256 keyspace, but stores only the paths
//! that contain data. This gives us:
//!
//! 1. **O(256) proof size** — proofs are always exactly 256 sibling hashes,
//!    regardless of how many accounts exist.
//! 2. **Exclusion proofs** — proving an account does NOT exist is as cheap as
//!    proving one does. The proof walks down to an empty leaf whose default
//!    hash is verifiable.
//! 3. **Deterministic root** — the root depends only on the set of (key, value)
//!    pairs, not on insertion order.
//!
//! ## Persistence
//!
//! Nodes are persisted to sled via `NovaDB`. Each node is keyed by its
//! position in the tree (level + path prefix). On update, only the 256 nodes
//! along the affected path are written — the rest of the tree remains
//! untouched on disk.
//!
//! ## State Transitions
//!
//! A transfer `sender -> recipient` for amount `A`:
//!
//! 1. Verify `sender.balance >= A`.
//! 2. `sender.balance -= A`
//! 3. `sender.nonce += 1`
//! 4. `recipient.balance += A`
//! 5. Recompute the state root.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::crypto::hash::blake3_hash;

use super::db::NovaDB;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Depth of the sparse Merkle tree. Matches the output size of BLAKE3 (256 bits).
const TREE_DEPTH: usize = 256;

/// sled tree name for SMT node data.
const SMT_TREE_NAME: &str = "smt_nodes";

// ---------------------------------------------------------------------------
// Precomputed Default Hashes
// ---------------------------------------------------------------------------

/// Default hashes at each level, computed bottom-up.
///
/// `DEFAULTS[0]` is the default hash for an empty leaf (the bottom).
/// `DEFAULTS[i]` = BLAKE3(DEFAULTS[i-1] || DEFAULTS[i-1]).
///
/// Level 0 is the leaf level. Level 256 is the root level.
fn compute_default_hashes() -> Vec<[u8; 32]> {
    let mut defaults = vec![[0u8; 32]; TREE_DEPTH + 1];
    defaults[0] = [0u8; 32];
    for i in 1..=TREE_DEPTH {
        let mut combined = [0u8; 64];
        combined[..32].copy_from_slice(&defaults[i - 1]);
        combined[32..].copy_from_slice(&defaults[i - 1]);
        defaults[i] = blake3_hash(&combined);
    }
    defaults
}

/// Thread-safe, lazily-initialized cache of default hashes.
fn default_hashes() -> &'static Vec<[u8; 32]> {
    use std::sync::OnceLock;
    static DEFAULTS: OnceLock<Vec<[u8; 32]>> = OnceLock::new();
    DEFAULTS.get_or_init(compute_default_hashes)
}

// ---------------------------------------------------------------------------
// AccountState
// ---------------------------------------------------------------------------

/// The on-chain state of a single account.
///
/// Stored in the state tree and persisted to sled. Every field here
/// is consensus-critical — validators must agree on every byte.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountState {
    /// Next expected transaction nonce (monotonically increasing).
    pub nonce: u64,
    /// Native token balance (photons).
    pub balance: u64,
    /// Per-token balance commitments (serialized Pedersen commitment bytes).
    /// Keyed by token ID (hex-encoded).
    pub balance_commitments: HashMap<String, Vec<u8>>,
    /// Active credit line IDs associated with this account.
    pub credit_lines: Vec<String>,
    /// Whether this account is frozen (compliance hold, dispute, etc.).
    pub frozen: bool,
}

impl AccountState {
    /// Create a new account with the given initial balance.
    pub fn with_balance(balance: u64) -> Self {
        Self {
            balance,
            ..Default::default()
        }
    }

    /// Serialize this account state to bytes for hashing / storage.
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("AccountState serialization should never fail")
    }

    /// Deserialize an account state from bytes.
    pub fn from_bytes(data: &[u8]) -> Option<AccountState> {
        bincode::deserialize(data).ok()
    }
}

// ---------------------------------------------------------------------------
// MerkleProof
// ---------------------------------------------------------------------------

/// Merkle inclusion / exclusion proof for a single key in the SMT.
///
/// Contains the sibling hashes along the path from leaf to root, plus the
/// direction bits that tell the verifier whether each sibling was on the
/// left or right. For a 256-bit keyspace, proofs are always exactly 256
/// siblings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MerkleProof {
    /// Sibling hashes along the path from leaf to root. Index 0 corresponds
    /// to level 1 (just above the leaf), index 255 to level 256 (root level).
    pub siblings: Vec<[u8; 32]>,
    /// Direction at each level: `false` = key bit 0 (current is left child),
    /// `true` = key bit 1 (current is right child).
    pub path_bits: Vec<bool>,
}

// ---------------------------------------------------------------------------
// StateError
// ---------------------------------------------------------------------------

/// Errors that can occur during state transitions.
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("insufficient balance: have {have}, need {need}")]
    InsufficientBalance { have: u64, need: u64 },

    #[error("account is frozen: {0}")]
    AccountFrozen(String),

    #[error("database error: {0}")]
    Db(#[from] super::db::DbError),

    #[error("serialization error: {0}")]
    Serialization(String),
}

// ---------------------------------------------------------------------------
// Key Bit Addressing
// ---------------------------------------------------------------------------
//
// The SMT has 256 levels. Level 0 is the leaf, level 256 is the root.
//
// At each internal level `l` (1..=256), we use one bit of the 256-bit key
// to decide whether the path goes left (bit=0) or right (bit=1).
//
// Convention: at level `l`, we use bit index `256 - l` of the key.
//   - Level 256 (root): bit index 0 (MSB of byte 0)
//   - Level 1 (just above leaf): bit index 255 (LSB of byte 31)
//   - Level 0 (leaf): no branching, the leaf is fully identified by all 256 bits

/// Extract bit at position `bit_index` from a 256-bit key.
/// Bit 0 is the MSB of byte 0.
fn get_bit(key: &[u8; 32], bit_index: usize) -> bool {
    let byte_idx = bit_index / 8;
    let bit_idx = 7 - (bit_index % 8);
    (key[byte_idx] >> bit_idx) & 1 == 1
}

/// Get the bit that determines left/right at a given tree level.
fn bit_at_level(key: &[u8; 32], level: usize) -> bool {
    let bit_index = TREE_DEPTH - level;
    get_bit(key, bit_index)
}

// ---------------------------------------------------------------------------
// Node Storage Keys
// ---------------------------------------------------------------------------
//
// Each node is stored in sled keyed by its position in the tree:
//   `<level:u16be><path_prefix>`
//
// The path prefix for a node at level `l` is the first `256 - l` bits of
// the key, packed into bytes, with trailing bits in the last byte zeroed.
// For the root (level 256), the prefix is empty (0 bits).
// For a leaf (level 0), the prefix is the full 32-byte key.

/// Build the sled storage key for a node at a given level along the path
/// determined by `key`.
fn storage_key_for_node(key: &[u8; 32], level: usize) -> Vec<u8> {
    let prefix_bits = TREE_DEPTH - level;
    let prefix_bytes_len = prefix_bits.div_ceil(8);

    let mut skey = Vec::with_capacity(2 + prefix_bytes_len);
    skey.extend_from_slice(&(level as u16).to_be_bytes());

    if prefix_bytes_len > 0 {
        let mut prefix = [0u8; 32];
        prefix[..prefix_bytes_len].copy_from_slice(&key[..prefix_bytes_len]);

        // Zero out bits beyond prefix_bits in the last byte.
        let remainder = prefix_bits % 8;
        if remainder != 0 {
            let mask = 0xFFu8 << (8 - remainder);
            prefix[prefix_bytes_len - 1] &= mask;
        }

        skey.extend_from_slice(&prefix[..prefix_bytes_len]);
    }

    skey
}

/// Build the sled storage key for the sibling of the node at a given level.
fn storage_key_for_sibling(key: &[u8; 32], level: usize) -> Vec<u8> {
    // The sibling shares the same parent, so its path prefix is the same
    // for bits 0..bit_at_level-1, but has the opposite bit at the branching
    // point. We flip the bit at bit_index = TREE_DEPTH - level.
    let mut flipped = *key;
    let bit_index = TREE_DEPTH - level;
    let byte_idx = bit_index / 8;
    let bit_idx = 7 - (bit_index % 8);
    flipped[byte_idx] ^= 1 << bit_idx;
    storage_key_for_node(&flipped, level - 1)
}

/// Storage key for leaf values (the serialized AccountState, not the hash).
fn leaf_value_key(key: &[u8; 32]) -> Vec<u8> {
    let mut k = Vec::with_capacity(2 + 32);
    k.extend_from_slice(b"v:");
    k.extend_from_slice(key);
    k
}

// ---------------------------------------------------------------------------
// Hash Helpers
// ---------------------------------------------------------------------------

/// Compute the hash of a leaf node: BLAKE3(key || serialized_value).
fn leaf_hash(key: &[u8; 32], value_bytes: &[u8]) -> [u8; 32] {
    let mut preimage = Vec::with_capacity(32 + value_bytes.len());
    preimage.extend_from_slice(key);
    preimage.extend_from_slice(value_bytes);
    blake3_hash(&preimage)
}

/// Combine two child hashes into a parent hash: BLAKE3(left || right).
fn combine_hashes(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut combined = [0u8; 64];
    combined[..32].copy_from_slice(left);
    combined[32..].copy_from_slice(right);
    blake3_hash(&combined)
}

// ---------------------------------------------------------------------------
// StateTree (Sparse Merkle Tree)
// ---------------------------------------------------------------------------

/// Sparse Merkle Tree backed by sled for persistent state storage.
///
/// The tree has a fixed depth of 256 levels, matching the BLAKE3 output
/// size used to hash account addresses into tree keys. Only paths with
/// actual data are materialized — empty subtrees are represented by
/// precomputed default hashes at each level.
///
/// ## Level Numbering
///
/// - Level 0 = leaf (stores hash of account state)
/// - Level 256 = root
///
/// At level `l`, the branching decision uses bit index `256 - l` of the key.
pub struct StateTree {
    db: NovaDB,
    root: [u8; 32],
}

impl StateTree {
    /// Create a new empty state tree.
    ///
    /// The initial root is the default hash at level 256 (the root level),
    /// representing a tree where every account has the default empty state.
    pub fn new(db: NovaDB) -> Self {
        let defaults = default_hashes();
        Self {
            db,
            root: defaults[TREE_DEPTH],
        }
    }

    /// Load an existing state tree from a known root hash.
    ///
    /// The caller is responsible for ensuring the root is valid and that
    /// the corresponding nodes exist in the database. Used when resuming
    /// from a persisted state after a node restart.
    pub fn from_root(db: NovaDB, root: [u8; 32]) -> Self {
        Self { db, root }
    }

    /// Return the current state root hash.
    pub fn root(&self) -> [u8; 32] {
        self.root
    }

    /// Retrieve the account state for an address.
    ///
    /// Returns `None` if the address has never been written to the tree.
    pub fn get(&self, address: &str) -> Option<AccountState> {
        let key = address_to_key(address);
        let tree = self.smt_tree();
        let vkey = leaf_value_key(&key);
        match tree.get(vkey).ok()? {
            Some(bytes) => AccountState::from_bytes(&bytes),
            None => None,
        }
    }

    /// Insert or update an account state, recomputing the root hash.
    ///
    /// The algorithm:
    /// 1. Walk from root to leaf, collecting the current sibling hash at
    ///    each level. Siblings are read from sled, falling back to default
    ///    hashes for empty subtrees.
    /// 2. Compute the new leaf hash.
    /// 3. Walk from leaf to root, combining the new hash with each collected
    ///    sibling, and writing every updated node to sled.
    pub fn put(&mut self, address: &str, state: &AccountState) {
        let key = address_to_key(address);
        let tree = self.smt_tree();
        let defaults = default_hashes();

        // Step 1: Collect sibling hashes top-down (root to leaf).
        // We iterate from level TREE_DEPTH down to level 1.
        // At each level, the sibling is the child of the parent on the
        // opposite side of our path.
        let mut siblings_top_down = Vec::with_capacity(TREE_DEPTH);
        for level in (1..=TREE_DEPTH).rev() {
            let sib_key = storage_key_for_sibling(&key, level);
            let sib_hash = match tree.get(&sib_key).ok().flatten() {
                Some(bytes) if bytes.len() == 32 => {
                    let mut h = [0u8; 32];
                    h.copy_from_slice(&bytes);
                    h
                }
                _ => defaults[level - 1],
            };
            siblings_top_down.push(sib_hash);
        }
        // siblings_top_down[0] = sibling at level TREE_DEPTH
        // siblings_top_down[TREE_DEPTH - 1] = sibling at level 1
        // Reverse so index 0 = level 1, index i = level i+1
        siblings_top_down.reverse();
        // Now: siblings_top_down[i] = sibling at level (i+1)

        // Step 2: Store the leaf value.
        let value_bytes = state.to_bytes();
        let vkey = leaf_value_key(&key);
        tree.insert(vkey, value_bytes.as_slice())
            .expect("sled write should not fail");

        // Step 3: Compute new leaf hash (level 0) and store it.
        let mut current_hash = leaf_hash(&key, &value_bytes);
        let leaf_skey = storage_key_for_node(&key, 0);
        tree.insert(leaf_skey, &current_hash)
            .expect("sled write should not fail");

        // Step 4: Walk from level 1 to TREE_DEPTH, recomputing parent hashes.
        for level in 1..=TREE_DEPTH {
            let bit = bit_at_level(&key, level);
            let sibling = siblings_top_down[level - 1];

            let (left, right) = if bit {
                (sibling, current_hash)
            } else {
                (current_hash, sibling)
            };

            current_hash = combine_hashes(&left, &right);

            let node_skey = storage_key_for_node(&key, level);
            tree.insert(node_skey, &current_hash)
                .expect("sled write should not fail");
        }

        self.root = current_hash;
    }

    /// Generate a Merkle proof for the given address.
    ///
    /// Works for both existing accounts (inclusion proof) and non-existent
    /// accounts (exclusion proof). The proof contains 256 sibling hashes.
    pub fn get_proof(&self, address: &str) -> MerkleProof {
        let key = address_to_key(address);
        let tree = self.smt_tree();
        let defaults = default_hashes();

        let mut siblings = Vec::with_capacity(TREE_DEPTH);
        let mut path_bits = Vec::with_capacity(TREE_DEPTH);

        for level in 1..=TREE_DEPTH {
            let bit = bit_at_level(&key, level);
            path_bits.push(bit);

            let sib_key = storage_key_for_sibling(&key, level);
            let sib_hash = match tree.get(&sib_key).ok().flatten() {
                Some(bytes) if bytes.len() == 32 => {
                    let mut h = [0u8; 32];
                    h.copy_from_slice(&bytes);
                    h
                }
                _ => defaults[level - 1],
            };
            siblings.push(sib_hash);
        }

        MerkleProof {
            siblings,
            path_bits,
        }
    }

    /// Verify a Merkle proof against a known root hash.
    ///
    /// If `value` is `Some`, this verifies an inclusion proof (the account
    /// exists with the given state). If `value` is `None`, this verifies an
    /// exclusion proof (the account does not exist in the tree).
    pub fn verify_proof(
        root: &[u8; 32],
        address: &str,
        value: Option<&AccountState>,
        proof: &MerkleProof,
    ) -> bool {
        if proof.siblings.len() != TREE_DEPTH || proof.path_bits.len() != TREE_DEPTH {
            return false;
        }

        let key = address_to_key(address);
        let defaults = default_hashes();

        let mut current_hash = match value {
            Some(state) => {
                let value_bytes = state.to_bytes();
                leaf_hash(&key, &value_bytes)
            }
            None => defaults[0],
        };

        for i in 0..TREE_DEPTH {
            let bit = proof.path_bits[i];
            let sibling = &proof.siblings[i];

            let (left, right) = if bit {
                (*sibling, current_hash)
            } else {
                (current_hash, *sibling)
            };

            current_hash = combine_hashes(&left, &right);
        }

        current_hash == *root
    }

    // -- Internal helpers ---------------------------------------------------

    fn smt_tree(&self) -> sled::Tree {
        self.db
            .open_tree(SMT_TREE_NAME)
            .expect("opening smt_nodes tree should not fail")
    }
}

// ---------------------------------------------------------------------------
// Transaction Execution
// ---------------------------------------------------------------------------

/// Apply a balance transfer between two accounts in the state tree.
///
/// Validates that the sender has sufficient balance, decrements sender balance,
/// increments sender nonce, and credits the receiver.
///
/// This is the fundamental state transition for NOVA transfers. Higher-level
/// transaction types (credit requests, token mints, etc.) build on top of this
/// primitive.
pub fn apply_transfer(
    tree: &mut StateTree,
    sender: &str,
    receiver: &str,
    amount: u64,
) -> Result<(), StateError> {
    let mut sender_state = tree.get(sender).unwrap_or_default();

    if sender_state.frozen {
        return Err(StateError::AccountFrozen(sender.to_string()));
    }

    if sender_state.balance < amount {
        return Err(StateError::InsufficientBalance {
            have: sender_state.balance,
            need: amount,
        });
    }

    sender_state.balance -= amount;
    sender_state.nonce += 1;
    tree.put(sender, &sender_state);

    let mut receiver_state = tree.get(receiver).unwrap_or_default();
    receiver_state.balance += amount;
    tree.put(receiver, &receiver_state);

    Ok(())
}

// ---------------------------------------------------------------------------
// Utility Functions
// ---------------------------------------------------------------------------

/// Hash an address string into a 256-bit key for the SMT.
fn address_to_key(address: &str) -> [u8; 32] {
    blake3_hash(address.as_bytes())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_tree() -> StateTree {
        let db = NovaDB::open_temporary().expect("should create temp db");
        StateTree::new(db)
    }

    // -- 1. Empty tree has known default root --------------------------------

    #[test]
    fn empty_tree_has_known_default_root() {
        let tree = temp_tree();
        let defaults = default_hashes();
        assert_eq!(tree.root(), defaults[TREE_DEPTH]);
        assert_ne!(tree.root(), [0u8; 32]);
    }

    // -- 2. Insert single account, verify root changes -----------------------

    #[test]
    fn insert_single_account_changes_root() {
        let mut tree = temp_tree();
        let initial_root = tree.root();

        let state = AccountState::with_balance(1000);
        tree.put("nova1alice", &state);

        assert_ne!(tree.root(), initial_root);
    }

    // -- 3. Insert and retrieve account state --------------------------------

    #[test]
    fn insert_and_retrieve_account_state() {
        let mut tree = temp_tree();
        let state = AccountState {
            nonce: 5,
            balance: 42_000,
            balance_commitments: HashMap::new(),
            credit_lines: vec!["credit_001".to_string()],
            frozen: false,
        };

        tree.put("nova1bob", &state);
        let retrieved = tree.get("nova1bob").expect("bob should exist");
        assert_eq!(retrieved, state);
    }

    // -- 4. Multiple accounts, independent state -----------------------------

    #[test]
    fn multiple_accounts_independent_state() {
        let mut tree = temp_tree();

        let alice = AccountState::with_balance(1000);
        let bob = AccountState::with_balance(2000);
        let charlie = AccountState::with_balance(3000);

        tree.put("nova1alice", &alice);
        tree.put("nova1bob", &bob);
        tree.put("nova1charlie", &charlie);

        assert_eq!(tree.get("nova1alice").unwrap().balance, 1000);
        assert_eq!(tree.get("nova1bob").unwrap().balance, 2000);
        assert_eq!(tree.get("nova1charlie").unwrap().balance, 3000);
    }

    // -- 5. Update existing account, root changes ----------------------------

    #[test]
    fn update_existing_account_changes_root() {
        let mut tree = temp_tree();

        let state_v1 = AccountState::with_balance(1000);
        tree.put("nova1alice", &state_v1);
        let root_v1 = tree.root();

        let state_v2 = AccountState::with_balance(2000);
        tree.put("nova1alice", &state_v2);
        let root_v2 = tree.root();

        assert_ne!(root_v1, root_v2);
        assert_eq!(tree.get("nova1alice").unwrap().balance, 2000);
    }

    // -- 6. Merkle proof generation and verification -------------------------

    #[test]
    fn merkle_proof_generation_and_verification() {
        let mut tree = temp_tree();
        let state = AccountState::with_balance(5000);
        tree.put("nova1alice", &state);

        let proof = tree.get_proof("nova1alice");
        assert_eq!(proof.siblings.len(), TREE_DEPTH);
        assert_eq!(proof.path_bits.len(), TREE_DEPTH);

        let valid = StateTree::verify_proof(&tree.root(), "nova1alice", Some(&state), &proof);
        assert!(valid, "valid inclusion proof should verify");
    }

    // -- 7. Invalid proof rejected -------------------------------------------

    #[test]
    fn invalid_proof_rejected() {
        let mut tree = temp_tree();
        let state = AccountState::with_balance(5000);
        tree.put("nova1alice", &state);

        let proof = tree.get_proof("nova1alice");

        // Wrong value.
        let wrong_state = AccountState::with_balance(9999);
        let result =
            StateTree::verify_proof(&tree.root(), "nova1alice", Some(&wrong_state), &proof);
        assert!(!result, "proof with wrong value should not verify");

        // Wrong root.
        let fake_root = [0xAB; 32];
        let result = StateTree::verify_proof(&fake_root, "nova1alice", Some(&state), &proof);
        assert!(!result, "proof with wrong root should not verify");

        // Wrong address.
        let result = StateTree::verify_proof(&tree.root(), "nova1bob", Some(&state), &proof);
        assert!(!result, "proof with wrong address should not verify");
    }

    // -- 8. apply_transfer: successful transfer ------------------------------

    #[test]
    fn apply_transfer_success() {
        let mut tree = temp_tree();

        let alice = AccountState::with_balance(10_000);
        tree.put("nova1alice", &alice);

        apply_transfer(&mut tree, "nova1alice", "nova1bob", 3_000).unwrap();

        let alice_after = tree.get("nova1alice").unwrap();
        let bob_after = tree.get("nova1bob").unwrap();

        assert_eq!(alice_after.balance, 7_000);
        assert_eq!(bob_after.balance, 3_000);
    }

    // -- 9. apply_transfer: insufficient balance rejected --------------------

    #[test]
    fn apply_transfer_insufficient_balance() {
        let mut tree = temp_tree();

        let alice = AccountState::with_balance(500);
        tree.put("nova1alice", &alice);

        let result = apply_transfer(&mut tree, "nova1alice", "nova1bob", 1_000);
        assert!(result.is_err());

        match result.unwrap_err() {
            StateError::InsufficientBalance { have, need } => {
                assert_eq!(have, 500);
                assert_eq!(need, 1_000);
            }
            other => panic!("expected InsufficientBalance, got: {:?}", other),
        }
    }

    // -- 10. apply_transfer: nonce incremented -------------------------------

    #[test]
    fn apply_transfer_increments_nonce() {
        let mut tree = temp_tree();

        let alice = AccountState::with_balance(10_000);
        tree.put("nova1alice", &alice);

        apply_transfer(&mut tree, "nova1alice", "nova1bob", 1_000).unwrap();
        assert_eq!(tree.get("nova1alice").unwrap().nonce, 1);

        apply_transfer(&mut tree, "nova1alice", "nova1bob", 1_000).unwrap();
        assert_eq!(tree.get("nova1alice").unwrap().nonce, 2);

        apply_transfer(&mut tree, "nova1alice", "nova1bob", 1_000).unwrap();
        assert_eq!(tree.get("nova1alice").unwrap().nonce, 3);
    }

    // -- 11. State root is deterministic -------------------------------------

    #[test]
    fn state_root_is_deterministic() {
        let mut tree1 = temp_tree();
        let mut tree2 = temp_tree();

        let alice = AccountState::with_balance(100);
        let bob = AccountState::with_balance(200);
        let charlie = AccountState::with_balance(300);

        tree1.put("nova1alice", &alice);
        tree1.put("nova1bob", &bob);
        tree1.put("nova1charlie", &charlie);

        tree2.put("nova1charlie", &charlie);
        tree2.put("nova1alice", &alice);
        tree2.put("nova1bob", &bob);

        assert_eq!(tree1.root(), tree2.root());
    }

    // -- 12. Large tree (100+ accounts) --------------------------------------

    #[test]
    fn large_tree_100_plus_accounts() {
        let mut tree = temp_tree();

        for i in 0..150u64 {
            let address = format!("nova1user_{i:04}");
            let state = AccountState::with_balance(i * 1000);
            tree.put(&address, &state);
        }

        assert_eq!(tree.get("nova1user_0000").unwrap().balance, 0);
        assert_eq!(tree.get("nova1user_0050").unwrap().balance, 50_000);
        assert_eq!(tree.get("nova1user_0099").unwrap().balance, 99_000);
        assert_eq!(tree.get("nova1user_0149").unwrap().balance, 149_000);

        let defaults = default_hashes();
        assert_ne!(tree.root(), defaults[TREE_DEPTH]);

        let state = tree.get("nova1user_0075").unwrap();
        let proof = tree.get_proof("nova1user_0075");
        assert!(StateTree::verify_proof(
            &tree.root(),
            "nova1user_0075",
            Some(&state),
            &proof
        ));
    }

    // -- 13. Proof for non-existent key (exclusion proof) --------------------

    #[test]
    fn exclusion_proof_for_nonexistent_key() {
        let mut tree = temp_tree();

        let alice = AccountState::with_balance(1000);
        tree.put("nova1alice", &alice);

        let proof = tree.get_proof("nova1nonexistent");
        let valid = StateTree::verify_proof(&tree.root(), "nova1nonexistent", None, &proof);
        assert!(valid, "exclusion proof should verify for nonexistent key");

        let fake = AccountState::with_balance(999);
        let invalid =
            StateTree::verify_proof(&tree.root(), "nova1nonexistent", Some(&fake), &proof);
        assert!(
            !invalid,
            "inclusion proof for nonexistent key should not verify"
        );
    }

    // -- 14. Empty tree exclusion proof --------------------------------------

    #[test]
    fn empty_tree_exclusion_proof() {
        let tree = temp_tree();
        let proof = tree.get_proof("nova1anyone");
        let valid = StateTree::verify_proof(&tree.root(), "nova1anyone", None, &proof);
        assert!(valid, "exclusion proof in empty tree should verify");
    }

    // -- 15. Proof after multiple updates ------------------------------------

    #[test]
    fn proof_valid_after_multiple_updates() {
        let mut tree = temp_tree();

        let alice = AccountState::with_balance(1000);
        tree.put("nova1alice", &alice);

        let bob = AccountState::with_balance(2000);
        tree.put("nova1bob", &bob);

        let alice_v2 = AccountState::with_balance(500);
        tree.put("nova1alice", &alice_v2);

        let proof = tree.get_proof("nova1alice");
        assert!(StateTree::verify_proof(
            &tree.root(),
            "nova1alice",
            Some(&alice_v2),
            &proof
        ));

        let proof_bob = tree.get_proof("nova1bob");
        assert!(StateTree::verify_proof(
            &tree.root(),
            "nova1bob",
            Some(&bob),
            &proof_bob
        ));
    }

    // -- 16. From_root constructor preserves state ---------------------------

    #[test]
    fn from_root_preserves_state() {
        let db = NovaDB::open_temporary().expect("temp db");
        let mut tree = StateTree::new(db.clone());

        let alice = AccountState::with_balance(7777);
        tree.put("nova1alice", &alice);
        let saved_root = tree.root();

        let tree2 = StateTree::from_root(db, saved_root);
        assert_eq!(tree2.root(), saved_root);
        assert_eq!(tree2.get("nova1alice").unwrap().balance, 7777);
    }

    // -- 17. Transfer rejects frozen account ---------------------------------

    #[test]
    fn apply_transfer_rejects_frozen_sender() {
        let mut tree = temp_tree();

        let alice = AccountState {
            balance: 10_000,
            frozen: true,
            ..Default::default()
        };
        tree.put("nova1alice", &alice);

        let result = apply_transfer(&mut tree, "nova1alice", "nova1bob", 1_000);
        assert!(result.is_err());
        match result.unwrap_err() {
            StateError::AccountFrozen(addr) => assert_eq!(addr, "nova1alice"),
            other => panic!("expected AccountFrozen, got: {:?}", other),
        }
    }

    // -- 18. Get returns None for missing account ----------------------------

    #[test]
    fn get_returns_none_for_missing_account() {
        let tree = temp_tree();
        assert!(tree.get("nova1nobody").is_none());
    }

    // -- 19. Different values produce different roots ------------------------

    #[test]
    fn different_values_produce_different_roots() {
        let mut tree1 = temp_tree();
        let mut tree2 = temp_tree();

        tree1.put("nova1alice", &AccountState::with_balance(100));
        tree2.put("nova1alice", &AccountState::with_balance(200));

        assert_ne!(tree1.root(), tree2.root());
    }

    // -- 20. Proof with tampered sibling rejected ----------------------------

    #[test]
    fn proof_with_tampered_sibling_rejected() {
        let mut tree = temp_tree();
        let state = AccountState::with_balance(5000);
        tree.put("nova1alice", &state);

        let mut proof = tree.get_proof("nova1alice");
        proof.siblings[0] = [0xFF; 32];

        let valid = StateTree::verify_proof(&tree.root(), "nova1alice", Some(&state), &proof);
        assert!(!valid, "tampered proof should not verify");
    }

    // -- 21. Transfer to self ------------------------------------------------

    #[test]
    fn transfer_to_self() {
        let mut tree = temp_tree();
        let alice = AccountState::with_balance(5_000);
        tree.put("nova1alice", &alice);

        apply_transfer(&mut tree, "nova1alice", "nova1alice", 1_000).unwrap();
        let after = tree.get("nova1alice").unwrap();
        // Sender debit: balance=4000, nonce=1. Receiver credit: balance=5000, nonce=1.
        assert_eq!(after.balance, 5_000);
        assert_eq!(after.nonce, 1);
    }
}
