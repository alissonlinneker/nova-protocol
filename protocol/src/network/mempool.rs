//! Priority-ordered transaction pool.
//!
//! Thread-safe mempool for pending transactions awaiting block inclusion.
//! Transactions are indexed by ID for O(1) lookups, and sorted by fee-per-byte
//! in a B-tree for efficient block proposal selection. Per-sender tracking
//! prevents any single address from monopolizing pool capacity.
//!
//! ## Design
//!
//! - `DashMap` provides lock-free concurrent reads for the hot path (RPC
//!   queries, duplicate detection during gossip).
//! - `parking_lot::RwLock<BTreeMap>` protects the fee index. Writers are rare
//!   (new transactions, evictions) compared to readers (block proposers
//!   scanning the top-N entries).
//! - Eviction targets the lowest fee-per-byte transaction when the pool is
//!   full and an incoming transaction offers a higher fee density.

use std::collections::BTreeMap;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use dashmap::DashMap;
use parking_lot::RwLock;

use crate::transaction::Transaction;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Tunable parameters for mempool behaviour.
///
/// Defaults are tuned for a devnet/testnet environment where fee enforcement
/// is relaxed. Production deployments should raise `min_fee` and lower
/// `max_per_sender` to mitigate spam.
#[derive(Debug, Clone)]
pub struct MempoolConfig {
    /// Maximum number of transactions the pool will hold.
    pub max_size: usize,

    /// Maximum pending transactions allowed per sender address.
    pub max_per_sender: usize,

    /// Seconds after which a transaction is considered stale and eligible
    /// for garbage collection via [`Mempool::expire_old`].
    pub expiry_seconds: u64,

    /// Minimum acceptable fee in photons. Transactions below this threshold
    /// are rejected outright (set to 0 on devnet for convenience).
    pub min_fee: u64,
}

impl Default for MempoolConfig {
    fn default() -> Self {
        Self {
            max_size: 10_000,
            max_per_sender: 100,
            expiry_seconds: 3600,
            min_fee: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// FeeKey — B-tree ordering key
// ---------------------------------------------------------------------------

/// Composite key for the fee-priority index.
///
/// Transactions are sorted by fee-per-byte descending. When two transactions
/// have the same fee density, the one added first (lower timestamp) wins.
/// The transaction ID is included as a final tiebreaker to guarantee
/// total ordering — `BTreeMap` requires unique keys.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FeeKey {
    /// Fee per byte, stored inverted (u64::MAX - fee_per_byte) so that
    /// the default ascending BTreeMap order yields highest-fee-first
    /// iteration.
    inverted_fee: u64,

    /// Insertion timestamp in seconds since UNIX epoch. Earlier entries
    /// are preferred when fees are tied.
    added_at: u64,

    /// Transaction ID for uniqueness.
    tx_id: String,
}

impl Ord for FeeKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.inverted_fee
            .cmp(&other.inverted_fee)
            .then_with(|| self.added_at.cmp(&other.added_at))
            .then_with(|| self.tx_id.cmp(&other.tx_id))
    }
}

impl PartialOrd for FeeKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

// ---------------------------------------------------------------------------
// MempoolEntry
// ---------------------------------------------------------------------------

/// A transaction together with pool-management metadata.
#[derive(Debug, Clone)]
pub struct MempoolEntry {
    /// The transaction itself.
    pub transaction: Transaction,

    /// Unix timestamp (seconds) when the transaction was added to the pool.
    pub added_at: u64,

    /// Pre-computed fee density used for priority ordering.
    pub fee_per_byte: u64,
}

// ---------------------------------------------------------------------------
// MempoolError
// ---------------------------------------------------------------------------

/// Errors returned by mempool operations.
#[derive(Debug)]
pub enum MempoolError {
    /// A transaction with the same ID is already in the pool.
    DuplicateTransaction,

    /// The offered fee does not meet the minimum threshold.
    FeeTooLow { min: u64, got: u64 },

    /// The sender already has too many pending transactions.
    SenderLimitExceeded { sender: String, limit: usize },

    /// The pool is at capacity and the incoming transaction does not outbid
    /// the lowest-fee entry.
    MempoolFull { size: usize },
}

impl fmt::Display for MempoolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateTransaction => write!(f, "transaction already exists in mempool"),
            Self::FeeTooLow { min, got } => {
                write!(f, "fee too low: minimum {}, got {}", min, got)
            }
            Self::SenderLimitExceeded { sender, limit } => {
                write!(
                    f,
                    "sender {} exceeded per-sender limit of {}",
                    sender, limit
                )
            }
            Self::MempoolFull { size } => {
                write!(f, "mempool is full ({} transactions)", size)
            }
        }
    }
}

impl std::error::Error for MempoolError {}

// ---------------------------------------------------------------------------
// Mempool
// ---------------------------------------------------------------------------

/// A thread-safe transaction mempool.
///
/// Transactions are keyed by their ID and ordered by fee-per-byte for
/// block production. The mempool enforces maximum capacity, per-sender
/// limits, minimum fee thresholds, and time-based expiry to prevent
/// memory exhaustion under spam attacks.
pub struct Mempool {
    /// Pending transactions indexed by ID for O(1) lookups.
    transactions: DashMap<String, MempoolEntry>,

    /// Transactions ordered by fee density (highest first) for block
    /// proposal selection.
    fee_index: RwLock<BTreeMap<FeeKey, String>>,

    /// Per-sender transaction count for rate limiting.
    sender_counts: DashMap<String, usize>,

    /// Configuration knobs.
    config: MempoolConfig,
}

impl fmt::Debug for Mempool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mempool")
            .field("size", &self.transactions.len())
            .field("config", &self.config)
            .finish()
    }
}

impl Mempool {
    /// Creates a new mempool with the given configuration.
    pub fn new(config: MempoolConfig) -> Self {
        Self {
            transactions: DashMap::new(),
            fee_index: RwLock::new(BTreeMap::new()),
            sender_counts: DashMap::new(),
            config,
        }
    }

    /// Adds a validated transaction to the mempool.
    ///
    /// The following checks are applied in order:
    ///
    /// 1. **Duplicate** — reject if a transaction with the same ID already exists.
    /// 2. **Minimum fee** — reject if `tx.fee < config.min_fee`.
    /// 3. **Per-sender limit** — reject if the sender already has
    ///    `config.max_per_sender` pending transactions.
    /// 4. **Capacity** — if the pool is full, attempt to evict the lowest-fee
    ///    transaction. If the incoming transaction does not outbid it, reject.
    ///
    /// On success the transaction is inserted into all indices atomically.
    pub fn add(&self, tx: Transaction) -> Result<(), MempoolError> {
        // 1. Duplicate check.
        if self.transactions.contains_key(&tx.id) {
            return Err(MempoolError::DuplicateTransaction);
        }

        // 2. Minimum fee enforcement.
        if tx.fee < self.config.min_fee {
            return Err(MempoolError::FeeTooLow {
                min: self.config.min_fee,
                got: tx.fee,
            });
        }

        // 3. Per-sender limit.
        let sender = tx.sender.clone();
        let sender_count = self.sender_counts.get(&sender).map(|v| *v).unwrap_or(0);

        if sender_count >= self.config.max_per_sender {
            return Err(MempoolError::SenderLimitExceeded {
                sender,
                limit: self.config.max_per_sender,
            });
        }

        // 4. Capacity check with eviction.
        if self.transactions.len() >= self.config.max_size {
            let incoming_fpb = tx.fee_per_byte();
            let evicted = self.try_evict_lowest(incoming_fpb);
            if !evicted {
                return Err(MempoolError::MempoolFull {
                    size: self.config.max_size,
                });
            }
        }

        // Build the entry and insert into all indices.
        let now = current_timestamp_secs();
        let fee_per_byte = tx.fee_per_byte();
        let tx_id = tx.id.clone();

        let entry = MempoolEntry {
            transaction: tx,
            added_at: now,
            fee_per_byte,
        };

        let fee_key = FeeKey {
            inverted_fee: u64::MAX - fee_per_byte,
            added_at: now,
            tx_id: tx_id.clone(),
        };

        self.transactions.insert(tx_id.clone(), entry);
        self.fee_index.write().insert(fee_key, tx_id);
        *self.sender_counts.entry(sender).or_insert(0) += 1;

        Ok(())
    }

    /// Removes a transaction by its ID and returns it, or `None` if not found.
    pub fn remove(&self, tx_id: &str) -> Option<Transaction> {
        let (_, entry) = self.transactions.remove(tx_id)?;
        self.remove_from_indices(&entry);
        Some(entry.transaction)
    }

    /// Batch-removes transactions by their IDs.
    ///
    /// Typically called after a block is finalized to clear included
    /// transactions. Missing IDs are silently ignored.
    pub fn remove_batch(&self, tx_ids: &[String]) {
        for id in tx_ids {
            self.remove(id);
        }
    }

    /// Returns a clone of the transaction with the given ID, if present.
    pub fn get(&self, tx_id: &str) -> Option<Transaction> {
        self.transactions.get(tx_id).map(|e| e.transaction.clone())
    }

    /// Returns `true` if the mempool contains a transaction with the given ID.
    pub fn contains(&self, tx_id: &str) -> bool {
        self.transactions.contains_key(tx_id)
    }

    /// Selects up to `max_count` transactions ordered by fee density
    /// (highest first) for block proposal.
    ///
    /// The returned vector is ordered from highest to lowest fee-per-byte.
    /// This is the primary interface used by the consensus engine during
    /// block production.
    pub fn select_transactions(&self, max_count: usize) -> Vec<Transaction> {
        let index = self.fee_index.read();
        let mut result = Vec::with_capacity(max_count.min(index.len()));

        for (_key, tx_id) in index.iter() {
            if result.len() >= max_count {
                break;
            }
            if let Some(entry) = self.transactions.get(tx_id) {
                result.push(entry.transaction.clone());
            }
        }

        result
    }

    /// Returns the current number of transactions in the pool.
    pub fn size(&self) -> usize {
        self.transactions.len()
    }

    /// Returns `true` if the mempool has no pending transactions.
    pub fn is_empty(&self) -> bool {
        self.transactions.is_empty()
    }

    /// Removes all transactions from the pool.
    pub fn clear(&self) {
        self.transactions.clear();
        self.fee_index.write().clear();
        self.sender_counts.clear();
    }

    /// Removes transactions that have been in the pool longer than
    /// `config.expiry_seconds`.
    ///
    /// Intended to be called periodically by a background timer in the
    /// validator node. Returns the number of expired transactions removed.
    pub fn expire_old(&self) -> usize {
        let now = current_timestamp_secs();
        let cutoff = now.saturating_sub(self.config.expiry_seconds);

        // Collect expired IDs first to avoid holding a DashMap iterator
        // while mutating.
        let expired_ids: Vec<String> = self
            .transactions
            .iter()
            .filter(|entry| entry.value().added_at < cutoff)
            .map(|entry| entry.key().clone())
            .collect();

        let count = expired_ids.len();
        for id in &expired_ids {
            self.remove(id);
        }

        count
    }

    /// Returns all pending transactions for a given sender address.
    pub fn pending_for_sender(&self, sender: &str) -> Vec<Transaction> {
        self.transactions
            .iter()
            .filter(|entry| entry.value().transaction.sender == sender)
            .map(|entry| entry.value().transaction.clone())
            .collect()
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Attempts to evict the lowest-fee transaction to make room for an
    /// incoming one with `incoming_fpb` fee-per-byte. Returns `true` if
    /// eviction succeeded.
    fn try_evict_lowest(&self, incoming_fpb: u64) -> bool {
        let mut index = self.fee_index.write();

        // The last entry in our inverted B-tree is the one with the
        // highest inverted_fee, i.e. the lowest actual fee-per-byte.
        let lowest_key = match index.keys().next_back() {
            Some(k) => k.clone(),
            None => return false,
        };

        let lowest_fpb = u64::MAX - lowest_key.inverted_fee;
        if incoming_fpb <= lowest_fpb {
            // The incoming transaction does not outbid the worst entry.
            return false;
        }

        // Evict the lowest-fee transaction.
        let evicted_id = index.remove(&lowest_key).unwrap();
        drop(index); // release the write lock before touching DashMap

        if let Some((_, entry)) = self.transactions.remove(&evicted_id) {
            self.decrement_sender_count(&entry.transaction.sender);
        }

        true
    }

    /// Removes an entry's metadata from the fee index and sender counts.
    fn remove_from_indices(&self, entry: &MempoolEntry) {
        // Remove from fee index.
        let fee_key = FeeKey {
            inverted_fee: u64::MAX - entry.fee_per_byte,
            added_at: entry.added_at,
            tx_id: entry.transaction.id.clone(),
        };
        self.fee_index.write().remove(&fee_key);

        // Decrement sender count.
        self.decrement_sender_count(&entry.transaction.sender);
    }

    /// Decrements the sender's pending transaction count, removing the
    /// entry entirely when it reaches zero.
    fn decrement_sender_count(&self, sender: &str) {
        if let Some(mut count) = self.sender_counts.get_mut(sender) {
            if *count <= 1 {
                drop(count);
                self.sender_counts.remove(sender);
            } else {
                *count -= 1;
            }
        }
    }
}

impl Default for Mempool {
    fn default() -> Self {
        Self::new(MempoolConfig::default())
    }
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

/// Returns the current time as seconds since the UNIX epoch.
fn current_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_secs()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::builder::TransactionBuilder;
    use crate::transaction::types::{Amount, Currency, TransactionType};

    /// Builds a test transaction with the given parameters.
    fn make_tx(sender: &str, receiver: &str, fee: u64, nonce: u64) -> Transaction {
        TransactionBuilder::new(TransactionType::Transfer)
            .sender(sender)
            .receiver(receiver)
            .amount(Amount::new(1_000, Currency::NOVA))
            .fee(fee)
            .nonce(nonce)
            .timestamp(1_700_000_000_000 + nonce)
            .build()
    }

    /// Builds a test transaction with only a fee differentiator.
    fn make_tx_with_fee(fee: u64, nonce: u64) -> Transaction {
        make_tx("nova1sender_a", "nova1receiver_b", fee, nonce)
    }

    // -- Basic add / get / contains -----------------------------------------

    #[test]
    fn add_and_retrieve_transaction() {
        let pool = Mempool::default();
        let tx = make_tx_with_fee(100, 1);
        let tx_id = tx.id.clone();

        pool.add(tx.clone()).unwrap();

        let retrieved = pool.get(&tx_id).unwrap();
        assert_eq!(retrieved.id, tx.id);
        assert_eq!(retrieved.fee, 100);
        assert_eq!(pool.size(), 1);
    }

    #[test]
    fn contains_returns_true_for_existing() {
        let pool = Mempool::default();
        let tx = make_tx_with_fee(100, 1);
        let tx_id = tx.id.clone();

        assert!(!pool.contains(&tx_id));
        pool.add(tx).unwrap();
        assert!(pool.contains(&tx_id));
    }

    #[test]
    fn get_returns_none_for_missing() {
        let pool = Mempool::default();
        assert!(pool.get("nonexistent_id").is_none());
    }

    // -- Duplicate rejection ------------------------------------------------

    #[test]
    fn rejects_duplicate_transaction() {
        let pool = Mempool::default();
        let tx = make_tx_with_fee(100, 1);

        pool.add(tx.clone()).unwrap();
        let result = pool.add(tx);

        assert!(matches!(result, Err(MempoolError::DuplicateTransaction)));
        assert_eq!(pool.size(), 1);
    }

    // -- Fee too low --------------------------------------------------------

    #[test]
    fn rejects_fee_too_low() {
        let config = MempoolConfig {
            min_fee: 500,
            ..Default::default()
        };
        let pool = Mempool::new(config);
        let tx = make_tx_with_fee(100, 1);

        let result = pool.add(tx);
        assert!(matches!(
            result,
            Err(MempoolError::FeeTooLow { min: 500, got: 100 })
        ));
        assert_eq!(pool.size(), 0);
    }

    #[test]
    fn accepts_fee_at_minimum() {
        let config = MempoolConfig {
            min_fee: 100,
            ..Default::default()
        };
        let pool = Mempool::new(config);
        let tx = make_tx_with_fee(100, 1);

        assert!(pool.add(tx).is_ok());
        assert_eq!(pool.size(), 1);
    }

    // -- Sender limit -------------------------------------------------------

    #[test]
    fn enforces_sender_limit() {
        let config = MempoolConfig {
            max_per_sender: 3,
            ..Default::default()
        };
        let pool = Mempool::new(config);

        for nonce in 1..=3 {
            let tx = make_tx("nova1alice", "nova1bob", 100, nonce);
            pool.add(tx).unwrap();
        }

        let tx4 = make_tx("nova1alice", "nova1bob", 100, 4);
        let result = pool.add(tx4);
        assert!(matches!(
            result,
            Err(MempoolError::SenderLimitExceeded { limit: 3, .. })
        ));
        assert_eq!(pool.size(), 3);
    }

    #[test]
    fn sender_limit_is_per_sender() {
        let config = MempoolConfig {
            max_per_sender: 2,
            ..Default::default()
        };
        let pool = Mempool::new(config);

        // Alice fills her quota.
        pool.add(make_tx("nova1alice", "nova1bob", 100, 1)).unwrap();
        pool.add(make_tx("nova1alice", "nova1bob", 100, 2)).unwrap();

        // Bob can still submit.
        pool.add(make_tx("nova1bob", "nova1alice", 100, 1)).unwrap();
        assert_eq!(pool.size(), 3);
    }

    // -- Mempool full / eviction --------------------------------------------

    #[test]
    fn mempool_full_evicts_lowest_fee() {
        let config = MempoolConfig {
            max_size: 3,
            ..Default::default()
        };
        let pool = Mempool::new(config);

        let tx_low = make_tx("nova1a", "nova1b", 10, 1);
        let tx_mid = make_tx("nova1c", "nova1d", 500, 2);
        let tx_high = make_tx("nova1e", "nova1f", 1_000, 3);

        let low_id = tx_low.id.clone();

        pool.add(tx_low).unwrap();
        pool.add(tx_mid).unwrap();
        pool.add(tx_high).unwrap();
        assert_eq!(pool.size(), 3);

        // Adding a higher-fee tx should evict the lowest.
        let tx_incoming = make_tx("nova1g", "nova1h", 5_000, 4);
        pool.add(tx_incoming).unwrap();

        assert_eq!(pool.size(), 3);
        assert!(
            !pool.contains(&low_id),
            "lowest-fee transaction should have been evicted"
        );
    }

    #[test]
    fn mempool_full_rejects_when_incoming_is_lowest() {
        let config = MempoolConfig {
            max_size: 2,
            ..Default::default()
        };
        let pool = Mempool::new(config);

        pool.add(make_tx("nova1a", "nova1b", 1_000, 1)).unwrap();
        pool.add(make_tx("nova1c", "nova1d", 2_000, 2)).unwrap();

        // Incoming fee is lower than anything in the pool — should be rejected.
        let tx_low = make_tx("nova1e", "nova1f", 1, 3);
        let result = pool.add(tx_low);
        assert!(matches!(result, Err(MempoolError::MempoolFull { size: 2 })));
    }

    // -- select_transactions ------------------------------------------------

    #[test]
    fn select_transactions_returns_highest_fee_first() {
        let pool = Mempool::default();

        let tx_low = make_tx("nova1a", "nova1b", 100, 1);
        let tx_mid = make_tx("nova1c", "nova1d", 500, 2);
        let tx_high = make_tx("nova1e", "nova1f", 10_000, 3);

        // Add in mixed order.
        pool.add(tx_mid.clone()).unwrap();
        pool.add(tx_low.clone()).unwrap();
        pool.add(tx_high.clone()).unwrap();

        let selected = pool.select_transactions(10);
        assert_eq!(selected.len(), 3);
        // Highest fee first.
        assert_eq!(selected[0].id, tx_high.id);
        // Lowest fee last.
        assert_eq!(selected[2].id, tx_low.id);
    }

    #[test]
    fn select_transactions_respects_max_count() {
        let pool = Mempool::default();
        for nonce in 1..=5 {
            pool.add(make_tx_with_fee(nonce * 100, nonce)).unwrap();
        }

        let selected = pool.select_transactions(3);
        assert_eq!(selected.len(), 3);
    }

    #[test]
    fn select_transactions_empty_pool() {
        let pool = Mempool::default();
        let selected = pool.select_transactions(10);
        assert!(selected.is_empty());
    }

    // -- remove / remove_batch ----------------------------------------------

    #[test]
    fn remove_returns_transaction() {
        let pool = Mempool::default();
        let tx = make_tx_with_fee(100, 1);
        let tx_id = tx.id.clone();

        pool.add(tx).unwrap();
        let removed = pool.remove(&tx_id).unwrap();

        assert_eq!(removed.id, tx_id);
        assert_eq!(pool.size(), 0);
        assert!(!pool.contains(&tx_id));
    }

    #[test]
    fn remove_nonexistent_returns_none() {
        let pool = Mempool::default();
        assert!(pool.remove("does_not_exist").is_none());
    }

    #[test]
    fn remove_batch_after_block_inclusion() {
        let pool = Mempool::default();

        let tx1 = make_tx_with_fee(100, 1);
        let tx2 = make_tx_with_fee(200, 2);
        let tx3 = make_tx_with_fee(300, 3);

        let ids: Vec<String> = vec![tx1.id.clone(), tx2.id.clone()];

        pool.add(tx1).unwrap();
        pool.add(tx2).unwrap();
        pool.add(tx3).unwrap();
        assert_eq!(pool.size(), 3);

        pool.remove_batch(&ids);

        assert_eq!(pool.size(), 1);
        assert!(!pool.contains(&ids[0]));
        assert!(!pool.contains(&ids[1]));
    }

    #[test]
    fn remove_batch_ignores_missing_ids() {
        let pool = Mempool::default();
        let tx = make_tx_with_fee(100, 1);
        pool.add(tx).unwrap();

        // One real ID, one fake — should not panic.
        pool.remove_batch(&["nonexistent".to_string()]);
        assert_eq!(pool.size(), 1);
    }

    // -- expire_old ---------------------------------------------------------

    #[test]
    fn expire_old_removes_stale_transactions() {
        let config = MempoolConfig {
            expiry_seconds: 1,
            ..Default::default()
        };
        let pool = Mempool::new(config);

        pool.add(make_tx_with_fee(100, 1)).unwrap();
        pool.add(make_tx_with_fee(200, 2)).unwrap();

        // Wait long enough for the entries to age past the 1-second TTL.
        // Timestamps have second-level granularity, so we need a full
        // second boundary to pass.
        std::thread::sleep(std::time::Duration::from_secs(2));

        let expired = pool.expire_old();
        assert_eq!(expired, 2);
        assert_eq!(pool.size(), 0);
    }

    #[test]
    fn expire_old_keeps_fresh_transactions() {
        let config = MempoolConfig {
            expiry_seconds: 3600, // 1 hour — nothing should expire
            ..Default::default()
        };
        let pool = Mempool::new(config);

        pool.add(make_tx_with_fee(100, 1)).unwrap();
        pool.add(make_tx_with_fee(200, 2)).unwrap();

        let expired = pool.expire_old();
        assert_eq!(expired, 0);
        assert_eq!(pool.size(), 2);
    }

    // -- pending_for_sender -------------------------------------------------

    #[test]
    fn pending_for_sender_returns_correct_set() {
        let pool = Mempool::default();

        pool.add(make_tx("nova1alice", "nova1bob", 100, 1)).unwrap();
        pool.add(make_tx("nova1alice", "nova1bob", 200, 2)).unwrap();
        pool.add(make_tx("nova1bob", "nova1alice", 300, 1)).unwrap();

        let alice_txs = pool.pending_for_sender("nova1alice");
        assert_eq!(alice_txs.len(), 2);
        assert!(alice_txs.iter().all(|tx| tx.sender == "nova1alice"));

        let bob_txs = pool.pending_for_sender("nova1bob");
        assert_eq!(bob_txs.len(), 1);

        let empty = pool.pending_for_sender("nova1nobody");
        assert!(empty.is_empty());
    }

    // -- clear --------------------------------------------------------------

    #[test]
    fn clear_empties_the_mempool() {
        let pool = Mempool::default();

        pool.add(make_tx_with_fee(100, 1)).unwrap();
        pool.add(make_tx_with_fee(200, 2)).unwrap();
        assert_eq!(pool.size(), 2);

        pool.clear();

        assert_eq!(pool.size(), 0);
        assert!(pool.is_empty());
        assert!(pool.select_transactions(10).is_empty());
    }

    // -- size tracking ------------------------------------------------------

    #[test]
    fn size_tracks_correctly() {
        let pool = Mempool::default();
        assert_eq!(pool.size(), 0);
        assert!(pool.is_empty());

        let tx1 = make_tx_with_fee(100, 1);
        let tx1_id = tx1.id.clone();
        pool.add(tx1).unwrap();
        assert_eq!(pool.size(), 1);
        assert!(!pool.is_empty());

        pool.add(make_tx_with_fee(200, 2)).unwrap();
        assert_eq!(pool.size(), 2);

        pool.remove(&tx1_id);
        assert_eq!(pool.size(), 1);
    }

    // -- Default config values ----------------------------------------------

    #[test]
    fn default_config_values() {
        let config = MempoolConfig::default();
        assert_eq!(config.max_size, 10_000);
        assert_eq!(config.max_per_sender, 100);
        assert_eq!(config.expiry_seconds, 3600);
        assert_eq!(config.min_fee, 0);
    }

    #[test]
    fn default_mempool_uses_default_config() {
        let pool = Mempool::default();
        // Should accept a 0-fee transaction on devnet defaults.
        let tx = make_tx_with_fee(0, 1);
        assert!(pool.add(tx).is_ok());
    }

    // -- Thread safety ------------------------------------------------------

    #[test]
    fn concurrent_add_and_remove() {
        use std::sync::Arc;
        use std::thread;

        let pool = Arc::new(Mempool::default());
        let mut handles = vec![];

        // Spawn writers that add transactions.
        for i in 0..10 {
            let pool = Arc::clone(&pool);
            handles.push(thread::spawn(move || {
                for nonce in 1..=20u64 {
                    let tx = make_tx(
                        &format!("nova1sender_{}", i),
                        "nova1receiver",
                        100 + nonce,
                        i as u64 * 1000 + nonce,
                    );
                    let _ = pool.add(tx);
                }
            }));
        }

        // Spawn readers that query and remove.
        for _ in 0..5 {
            let pool = Arc::clone(&pool);
            handles.push(thread::spawn(move || {
                for _ in 0..50 {
                    let _ = pool.size();
                    let _ = pool.select_transactions(5);
                    let _ = pool.pending_for_sender("nova1sender_0");
                }
            }));
        }

        for h in handles {
            h.join().expect("thread panicked");
        }

        // Pool should be in a consistent state — no panics, no deadlocks.
        assert!(pool.size() <= 200);
    }

    // -- Sender count bookkeeping -------------------------------------------

    #[test]
    fn sender_count_decrements_on_remove() {
        let config = MempoolConfig {
            max_per_sender: 2,
            ..Default::default()
        };
        let pool = Mempool::new(config);

        let tx1 = make_tx("nova1alice", "nova1bob", 100, 1);
        let tx2 = make_tx("nova1alice", "nova1bob", 200, 2);
        let tx1_id = tx1.id.clone();

        pool.add(tx1).unwrap();
        pool.add(tx2).unwrap();

        // Alice is at limit.
        let tx3 = make_tx("nova1alice", "nova1bob", 300, 3);
        assert!(matches!(
            pool.add(tx3),
            Err(MempoolError::SenderLimitExceeded { .. })
        ));

        // Remove one — should free up a slot.
        pool.remove(&tx1_id);

        let tx4 = make_tx("nova1alice", "nova1bob", 400, 4);
        assert!(pool.add(tx4).is_ok());
        assert_eq!(pool.size(), 2);
    }

    // -- Fee index consistency after remove ---------------------------------

    #[test]
    fn fee_index_consistent_after_removal() {
        let pool = Mempool::default();

        let tx1 = make_tx("nova1a", "nova1b", 100, 1);
        let tx2 = make_tx("nova1c", "nova1d", 500, 2);
        let tx3 = make_tx("nova1e", "nova1f", 1_000, 3);

        let tx2_id = tx2.id.clone();

        pool.add(tx1.clone()).unwrap();
        pool.add(tx2).unwrap();
        pool.add(tx3.clone()).unwrap();

        // Remove the middle-fee transaction.
        pool.remove(&tx2_id);

        let selected = pool.select_transactions(10);
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].id, tx3.id);
        assert_eq!(selected[1].id, tx1.id);
    }
}
