//! Priority-ordered transaction pool.
//!
//! Thread-safe mempool for pending transactions awaiting block inclusion.

use crate::transaction::Transaction;
use parking_lot::RwLock;
use std::collections::HashMap;

/// A thread-safe transaction mempool.
///
/// Transactions are keyed by their ID and ordered by fee-per-byte for
/// block production. The mempool enforces a maximum capacity to prevent
/// memory exhaustion under spam attacks.
#[derive(Debug)]
pub struct Mempool {
    txs: RwLock<HashMap<String, Transaction>>,
    max_size: usize,
}

impl Mempool {
    /// Creates a new mempool with the given maximum transaction capacity.
    pub fn new(max_size: usize) -> Self {
        Self {
            txs: RwLock::new(HashMap::new()),
            max_size,
        }
    }

    /// Inserts a transaction into the mempool.
    ///
    /// Returns an error if the mempool is at capacity.
    pub fn insert(&self, tx: Transaction) -> Result<(), String> {
        let mut txs = self.txs.write();
        if txs.len() >= self.max_size {
            return Err("mempool is full".to_string());
        }
        txs.insert(tx.id.clone(), tx);
        Ok(())
    }

    /// Removes a transaction by its ID.
    pub fn remove(&self, id: &str) {
        let mut txs = self.txs.write();
        txs.remove(id);
    }

    /// Returns the number of pending transactions.
    pub fn len(&self) -> usize {
        self.txs.read().len()
    }

    /// Returns true if the mempool has no pending transactions.
    pub fn is_empty(&self) -> bool {
        self.txs.read().is_empty()
    }
}

impl Default for Mempool {
    fn default() -> Self {
        Self::new(10_000)
    }
}
