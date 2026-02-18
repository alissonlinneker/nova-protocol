//! # State Management -- Simplified Merkle State Tree
//!
//! The state tree maps NOVA addresses to account states. After each block,
//! the tree is updated by applying that block's transactions, and the new
//! root hash is stored in the block header.
//!
//! ## Current Implementation
//!
//! For the initial protocol version we use a flat `HashMap` with a Merkle
//! root computed by sorting entries and hashing the concatenation. This is
//! sufficient for correctness and testing. A production-grade Merkle
//! Patricia Trie will replace this once the state outgrows memory.
//!
//! ## State Transitions
//!
//! A transaction `T: sender -> recipient` for amount `A` with fee `F`:
//!
//! 1. Verify `sender.nonce == T.nonce`.
//! 2. Verify `sender.balance >= A + F`.
//! 3. `sender.balance -= A + F`
//! 4. `sender.nonce += 1`
//! 5. `recipient.balance += A`
//! 6. Recompute the state root.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

use crate::crypto::hash::blake3_hash;
use crate::transaction::builder::Transaction;

// ---------------------------------------------------------------------------
// AccountState
// ---------------------------------------------------------------------------

/// The on-chain state of a single account.
///
/// Stored in the state tree and persisted to RocksDB. Every field here
/// is consensus-critical -- validators must agree on every byte.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

impl Default for AccountState {
    fn default() -> Self {
        Self {
            nonce: 0,
            balance: 0,
            balance_commitments: HashMap::new(),
            credit_lines: Vec::new(),
            frozen: false,
        }
    }
}

impl AccountState {
    /// Create a new account with the given initial balance.
    pub fn with_balance(balance: u64) -> Self {
        Self {
            balance,
            ..Default::default()
        }
    }

    /// Serialize this account state to bytes for hashing.
    fn to_hash_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// StateTree
// ---------------------------------------------------------------------------

/// In-memory state tree mapping addresses to account states.
///
/// The root hash is a Merkle root computed over sorted entries:
///
/// ```text
/// leaves = sort([ BLAKE3(addr || account_bytes) for (addr, account) in tree ])
/// root   = merkle_root(leaves)
/// ```
///
/// Sorting ensures deterministic root computation regardless of insertion order.
pub struct StateTree {
    /// Account states keyed by NOVA address.
    accounts: HashMap<String, AccountState>,
}

impl StateTree {
    /// Create a new empty state tree.
    pub fn new() -> Self {
        Self {
            accounts: HashMap::new(),
        }
    }

    /// Retrieve the account state for an address.
    pub fn get(&self, address: &str) -> Option<&AccountState> {
        self.accounts.get(address)
    }

    /// Retrieve a mutable reference to an account state.
    pub fn get_mut(&mut self, address: &str) -> Option<&mut AccountState> {
        self.accounts.get_mut(address)
    }

    /// Insert or update an account state.
    pub fn insert(&mut self, address: String, state: AccountState) {
        self.accounts.insert(address, state);
    }

    /// Remove an account from the tree.
    pub fn remove(&mut self, address: &str) -> Option<AccountState> {
        self.accounts.remove(address)
    }

    /// Return the number of accounts in the tree.
    pub fn len(&self) -> usize {
        self.accounts.len()
    }

    /// Return `true` if the tree has no accounts.
    pub fn is_empty(&self) -> bool {
        self.accounts.is_empty()
    }

    /// Compute the Merkle root hash of the entire state tree.
    ///
    /// 1. For each (address, state), compute `BLAKE3(address || serialized_state)`.
    /// 2. Sort hashes lexicographically (via BTreeMap ordering on keys).
    /// 3. Build a binary Merkle tree over the sorted hashes.
    /// 4. Return the root.
    ///
    /// An empty tree returns `[0u8; 32]`.
    pub fn root_hash(&self) -> [u8; 32] {
        if self.accounts.is_empty() {
            return [0u8; 32];
        }

        let sorted: BTreeMap<&String, &AccountState> = self.accounts.iter().collect();

        let mut leaf_hashes: Vec<[u8; 32]> = sorted
            .iter()
            .map(|(addr, state)| {
                let mut preimage = Vec::new();
                preimage.extend_from_slice(addr.as_bytes());
                preimage.extend_from_slice(&state.to_hash_bytes());
                blake3_hash(&preimage)
            })
            .collect();

        while leaf_hashes.len() > 1 {
            let mut next_level = Vec::with_capacity((leaf_hashes.len() + 1) / 2);
            for chunk in leaf_hashes.chunks(2) {
                let mut combined = Vec::with_capacity(64);
                combined.extend_from_slice(&chunk[0]);
                if chunk.len() == 2 {
                    combined.extend_from_slice(&chunk[1]);
                } else {
                    combined.extend_from_slice(&chunk[0]);
                }
                next_level.push(blake3_hash(&combined));
            }
            leaf_hashes = next_level;
        }

        leaf_hashes[0]
    }

    /// Apply a transaction to the state tree.
    ///
    /// Performs the full state transition: nonce check, balance debit,
    /// nonce increment, balance credit.
    pub fn apply_transaction(&mut self, tx: &Transaction) -> Result<(), String> {
        if !self.accounts.contains_key(&tx.sender) {
            self.accounts
                .insert(tx.sender.clone(), AccountState::default());
        }

        let amount_value = tx.amount.value;

        {
            let sender_state = self.accounts.get(&tx.sender).unwrap();

            if sender_state.frozen {
                return Err(format!("sender {} is frozen", tx.sender));
            }

            if sender_state.nonce != tx.nonce {
                return Err(format!(
                    "nonce mismatch for {}: expected {}, got {}",
                    tx.sender, sender_state.nonce, tx.nonce
                ));
            }

            let total_debit = amount_value
                .checked_add(tx.fee)
                .ok_or_else(|| "amount + fee overflow".to_string())?;

            if sender_state.balance < total_debit {
                return Err(format!(
                    "insufficient balance for {}: have {}, need {}",
                    tx.sender, sender_state.balance, total_debit
                ));
            }
        }

        {
            let sender_state = self.accounts.get_mut(&tx.sender).unwrap();
            sender_state.balance -= amount_value + tx.fee;
            sender_state.nonce += 1;
        }

        let recipient_state = self
            .accounts
            .entry(tx.receiver.clone())
            .or_insert_with(AccountState::default);
        recipient_state.balance += amount_value;

        Ok(())
    }

    /// Return an iterator over all (address, account_state) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &AccountState)> {
        self.accounts.iter()
    }
}

impl Default for StateTree {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::builder::TransactionBuilder;
    use crate::transaction::types::{Amount, Currency, TransactionType};

    fn make_tx(sender: &str, recipient: &str, amount: u64, fee: u64, nonce: u64) -> Transaction {
        TransactionBuilder::new(TransactionType::Transfer)
            .sender(sender)
            .receiver(recipient)
            .amount(Amount::new(amount, Currency::NOVA))
            .fee(fee)
            .nonce(nonce)
            .timestamp(1_000_000)
            .build()
    }

    #[test]
    fn empty_tree_root_is_zero() {
        let tree = StateTree::new();
        assert_eq!(tree.root_hash(), [0u8; 32]);
    }

    #[test]
    fn insert_and_get() {
        let mut tree = StateTree::new();
        let state = AccountState::with_balance(1000);
        tree.insert("nova:alice".to_string(), state.clone());

        assert_eq!(tree.get("nova:alice"), Some(&state));
        assert_eq!(tree.get("nova:bob"), None);
    }

    #[test]
    fn root_hash_is_deterministic() {
        let mut tree1 = StateTree::new();
        let mut tree2 = StateTree::new();

        tree1.insert("nova:alice".to_string(), AccountState::with_balance(100));
        tree1.insert("nova:bob".to_string(), AccountState::with_balance(200));

        tree2.insert("nova:bob".to_string(), AccountState::with_balance(200));
        tree2.insert("nova:alice".to_string(), AccountState::with_balance(100));

        assert_eq!(tree1.root_hash(), tree2.root_hash());
    }

    #[test]
    fn different_states_different_roots() {
        let mut tree1 = StateTree::new();
        let mut tree2 = StateTree::new();

        tree1.insert("nova:alice".to_string(), AccountState::with_balance(100));
        tree2.insert("nova:alice".to_string(), AccountState::with_balance(200));

        assert_ne!(tree1.root_hash(), tree2.root_hash());
    }

    #[test]
    fn apply_transaction_success() {
        let mut tree = StateTree::new();
        tree.insert("nova:alice".to_string(), AccountState::with_balance(1000));

        let tx = make_tx("nova:alice", "nova:bob", 500, 100, 0);
        tree.apply_transaction(&tx).unwrap();

        let alice = tree.get("nova:alice").unwrap();
        assert_eq!(alice.balance, 400);
        assert_eq!(alice.nonce, 1);

        let bob = tree.get("nova:bob").unwrap();
        assert_eq!(bob.balance, 500);
    }

    #[test]
    fn apply_transaction_insufficient_balance() {
        let mut tree = StateTree::new();
        tree.insert("nova:alice".to_string(), AccountState::with_balance(100));

        let tx = make_tx("nova:alice", "nova:bob", 500, 100, 0);
        assert!(tree.apply_transaction(&tx).is_err());
    }

    #[test]
    fn apply_transaction_nonce_mismatch() {
        let mut tree = StateTree::new();
        tree.insert("nova:alice".to_string(), AccountState::with_balance(1000));

        let tx = make_tx("nova:alice", "nova:bob", 100, 10, 5);
        assert!(tree.apply_transaction(&tx).is_err());
    }

    #[test]
    fn state_root_changes_after_transaction() {
        let mut tree = StateTree::new();
        tree.insert("nova:alice".to_string(), AccountState::with_balance(1000));
        let root_before = tree.root_hash();

        let tx = make_tx("nova:alice", "nova:bob", 100, 10, 0);
        tree.apply_transaction(&tx).unwrap();

        assert_ne!(root_before, tree.root_hash());
    }
}
