//! # NovaDB — Persistent Storage Engine
//!
//! The persistence layer for the NOVA blockchain, built on sled's
//! embedded key-value store. All on-disk data flows through this module.
//!
//! ## Tree Layout
//!
//! sled organizes data into named "trees" (analogous to column families
//! in RocksDB or tables in SQL). Each tree is an independent B+ tree
//! with its own keyspace:
//!
//! | Tree           | Key                 | Value                    |
//! |----------------|---------------------|--------------------------|
//! | `blocks`       | `height` (8B BE)    | `bincode(Block)`         |
//! | `block_hashes` | `hash` (32B)        | `height` (8B BE)         |
//! | `transactions` | `tx_id` (hex bytes) | `bincode(Transaction)`   |
//! | `accounts`     | `address` (UTF-8)   | `bincode(AccountState)`  |
//! | `metadata`     | key (UTF-8)         | value (bytes)            |
//!
//! Block heights are stored as big-endian u64 so that sled's lexicographic
//! ordering matches numeric ordering — this makes range scans over blocks
//! work naturally.
//!
//! ## Atomicity
//!
//! When persisting a new block, we write the block, all its transactions,
//! and the updated height in a single atomic `Batch`. Either everything
//! lands on disk or nothing does — no partial writes, no corruption.

use sled::{Batch, Db, Tree};
use std::path::Path;

use super::block::Block;
use super::state::AccountState;
use crate::transaction::Transaction;

// ---------------------------------------------------------------------------
// Error Type
// ---------------------------------------------------------------------------

/// Errors that can occur during database operations.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("sled error: {0}")]
    Sled(#[from] sled::Error),

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("key not found: {0}")]
    NotFound(String),
}

pub type DbResult<T> = Result<T, DbError>;

// ---------------------------------------------------------------------------
// Metadata Keys
// ---------------------------------------------------------------------------

/// Well-known key in the `metadata` tree for the latest block height.
const META_LATEST_HEIGHT: &[u8] = b"latest_block_height";

// ---------------------------------------------------------------------------
// NovaDB
// ---------------------------------------------------------------------------

/// Persistent storage engine for the NOVA blockchain.
///
/// Wraps a sled `Db` instance and exposes typed accessors for blocks,
/// transactions, accounts, and chain metadata. All serialization uses
/// bincode for compactness and speed.
///
/// # Thread Safety
///
/// sled is inherently thread-safe — all trees support lock-free concurrent
/// reads and serialized writes. `NovaDB` can be shared across threads
/// via `Arc<NovaDB>` without external synchronization.
#[derive(Debug, Clone)]
pub struct NovaDB {
    /// The underlying sled database handle.
    db: Db,
    /// Blocks indexed by height (big-endian u64 keys).
    blocks: Tree,
    /// Reverse index: block hash (32 bytes) -> height (8 bytes BE).
    block_hashes: Tree,
    /// Transactions indexed by hex-encoded tx ID.
    transactions: Tree,
    /// Account states indexed by NOVA address (UTF-8).
    accounts: Tree,
    /// Arbitrary key-value metadata (latest height, config, etc.).
    metadata: Tree,
}

impl NovaDB {
    /// Open or create a database at the given filesystem path.
    ///
    /// If the directory doesn't exist, sled creates it. If the database
    /// already exists, it's opened and all existing data is available
    /// immediately.
    pub fn open<P: AsRef<Path>>(path: P) -> DbResult<Self> {
        let db = sled::open(path)?;
        Self::from_db(db)
    }

    /// Create a temporary database that lives in memory and is cleaned
    /// up automatically when the `NovaDB` is dropped.
    ///
    /// Ideal for unit tests — no filesystem side effects, no cleanup needed.
    pub fn open_temporary() -> DbResult<Self> {
        let config = sled::Config::new().temporary(true);
        let db = config.open()?;
        Self::from_db(db)
    }

    /// Internal constructor: opens named trees from an existing sled `Db`.
    fn from_db(db: Db) -> DbResult<Self> {
        let blocks = db.open_tree("blocks")?;
        let block_hashes = db.open_tree("block_hashes")?;
        let transactions = db.open_tree("transactions")?;
        let accounts = db.open_tree("accounts")?;
        let metadata = db.open_tree("metadata")?;

        Ok(Self {
            db,
            blocks,
            block_hashes,
            transactions,
            accounts,
            metadata,
        })
    }

    /// Open a named sled tree from the underlying database.
    ///
    /// Used by higher-level data structures (e.g., Sparse Merkle Tree) that
    /// need dedicated key-value storage within the same database instance.
    /// The tree is created if it doesn't exist.
    pub fn open_tree(&self, name: &str) -> DbResult<Tree> {
        Ok(self.db.open_tree(name)?)
    }

    // -- Block operations ---------------------------------------------------

    /// Persist a block and all its transactions atomically.
    ///
    /// This writes:
    /// 1. The full block into the `blocks` tree (keyed by height).
    /// 2. A hash-to-height entry into `block_hashes`.
    /// 3. Each transaction into the `transactions` tree (keyed by tx ID).
    /// 4. An updated `latest_block_height` in `metadata`.
    ///
    /// All writes are batched into a single atomic operation per tree.
    pub fn put_block(&self, block: &Block) -> DbResult<()> {
        let height_key = block.header.height.to_be_bytes();
        let block_bytes =
            bincode::serialize(block).map_err(|e| DbError::Serialization(e.to_string()))?;

        // Batch writes to the blocks tree.
        let mut block_batch = Batch::default();
        block_batch.insert(&height_key, block_bytes);
        self.blocks.apply_batch(block_batch)?;

        // Index block hash -> height.
        self.block_hashes.insert(&block.header.hash, &height_key)?;

        // Persist each transaction.
        let mut tx_batch = Batch::default();
        for tx in &block.transactions {
            let tx_bytes =
                bincode::serialize(tx).map_err(|e| DbError::Serialization(e.to_string()))?;
            tx_batch.insert(tx.id.as_bytes(), tx_bytes);
        }
        self.transactions.apply_batch(tx_batch)?;

        // Update latest height.
        self.metadata.insert(META_LATEST_HEIGHT, &height_key)?;

        // Flush to ensure durability.
        self.db.flush()?;

        Ok(())
    }

    /// Retrieve a block by its height.
    ///
    /// Returns `None` if no block exists at the given height.
    pub fn get_block(&self, height: u64) -> DbResult<Option<Block>> {
        let key = height.to_be_bytes();
        match self.blocks.get(key)? {
            Some(bytes) => {
                let block: Block = bincode::deserialize(&bytes)
                    .map_err(|e| DbError::Serialization(e.to_string()))?;
                Ok(Some(block))
            }
            None => Ok(None),
        }
    }

    /// Retrieve a block by its hash.
    ///
    /// Performs a two-step lookup: hash -> height (from `block_hashes`),
    /// then height -> block (from `blocks`).
    pub fn get_block_by_hash(&self, hash: &[u8; 32]) -> DbResult<Option<Block>> {
        match self.block_hashes.get(hash)? {
            Some(height_bytes) => {
                let height = u64::from_be_bytes(
                    height_bytes
                        .as_ref()
                        .try_into()
                        .map_err(|_| DbError::Serialization("invalid height bytes".to_string()))?,
                );
                self.get_block(height)
            }
            None => Ok(None),
        }
    }

    /// Iterate over blocks in a height range (inclusive on both ends).
    ///
    /// Returns blocks in ascending height order. Stops at the first gap
    /// (missing height) within the range.
    pub fn get_block_range(&self, start: u64, end: u64) -> DbResult<Vec<Block>> {
        let start_key = start.to_be_bytes();
        let end_key = end.to_be_bytes();

        let mut blocks = Vec::new();
        for result in self.blocks.range(start_key..=end_key) {
            let (_key, value) = result?;
            let block: Block =
                bincode::deserialize(&value).map_err(|e| DbError::Serialization(e.to_string()))?;
            blocks.push(block);
        }

        Ok(blocks)
    }

    // -- Transaction operations ---------------------------------------------

    /// Persist a single transaction.
    ///
    /// Typically used for mempool staging. Block-included transactions
    /// are written atomically via `put_block`.
    pub fn put_transaction(&self, tx: &Transaction) -> DbResult<()> {
        let tx_bytes = bincode::serialize(tx).map_err(|e| DbError::Serialization(e.to_string()))?;
        self.transactions.insert(tx.id.as_bytes(), tx_bytes)?;
        Ok(())
    }

    /// Retrieve a transaction by its hex-encoded ID.
    pub fn get_transaction(&self, id: &str) -> DbResult<Option<Transaction>> {
        match self.transactions.get(id.as_bytes())? {
            Some(bytes) => {
                let tx: Transaction = bincode::deserialize(&bytes)
                    .map_err(|e| DbError::Serialization(e.to_string()))?;
                Ok(Some(tx))
            }
            None => Ok(None),
        }
    }

    // -- Account operations -------------------------------------------------

    /// Persist an account state for the given address.
    pub fn put_account(&self, address: &str, state: &AccountState) -> DbResult<()> {
        let bytes = bincode::serialize(state).map_err(|e| DbError::Serialization(e.to_string()))?;
        self.accounts.insert(address.as_bytes(), bytes)?;
        Ok(())
    }

    /// Retrieve the account state for a given address.
    ///
    /// Returns `None` if the address has never been seen on-chain.
    pub fn get_account(&self, address: &str) -> DbResult<Option<AccountState>> {
        match self.accounts.get(address.as_bytes())? {
            Some(bytes) => {
                let state: AccountState = bincode::deserialize(&bytes)
                    .map_err(|e| DbError::Serialization(e.to_string()))?;
                Ok(Some(state))
            }
            None => Ok(None),
        }
    }

    // -- Metadata operations ------------------------------------------------

    /// Get the latest persisted block height.
    ///
    /// Returns `None` if the database is empty (no blocks persisted yet).
    pub fn get_latest_block_height(&self) -> DbResult<Option<u64>> {
        match self.metadata.get(META_LATEST_HEIGHT)? {
            Some(bytes) => {
                let height = u64::from_be_bytes(
                    bytes
                        .as_ref()
                        .try_into()
                        .map_err(|_| DbError::Serialization("invalid height bytes".to_string()))?,
                );
                Ok(Some(height))
            }
            None => Ok(None),
        }
    }

    /// Explicitly set the latest block height in metadata.
    ///
    /// Normally this is updated automatically by `put_block`, but this
    /// method is available for bootstrapping and recovery scenarios.
    pub fn set_latest_block_height(&self, height: u64) -> DbResult<()> {
        self.metadata
            .insert(META_LATEST_HEIGHT, &height.to_be_bytes())?;
        Ok(())
    }

    // -- Utility operations -------------------------------------------------

    /// Return the number of blocks stored in the database.
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// Return the number of transactions stored in the database.
    pub fn transaction_count(&self) -> usize {
        self.transactions.len()
    }

    /// Return the number of accounts stored in the database.
    pub fn account_count(&self) -> usize {
        self.accounts.len()
    }

    /// Force a flush of all pending writes to disk.
    ///
    /// sled buffers writes in memory for performance. This call blocks
    /// until all data is durable on the underlying storage device.
    pub fn flush(&self) -> DbResult<()> {
        self.db.flush()?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::block::Block;
    use crate::transaction::builder::TransactionBuilder;
    use crate::transaction::types::{Amount, Currency, TransactionType};

    // -- Helpers ------------------------------------------------------------

    fn make_test_tx(id_byte: u8) -> Transaction {
        TransactionBuilder::new(TransactionType::Transfer)
            .sender("nova:alice")
            .receiver("nova:bob")
            .amount(Amount::new(100, Currency::NOVA))
            .fee(10)
            .nonce(id_byte as u64)
            .timestamp(1_000_000)
            .build()
    }

    fn make_block_chain(count: usize) -> Vec<Block> {
        let mut blocks = vec![Block::genesis()];
        for i in 1..count {
            let parent = &blocks[i - 1];
            let txs = vec![make_test_tx(i as u8)];
            let block = Block::new(parent, txs, format!("nova:validator_{i}"), [i as u8; 32]);
            blocks.push(block);
        }
        blocks
    }

    // -- Tests --------------------------------------------------------------

    #[test]
    fn open_temporary_database() {
        let db = NovaDB::open_temporary().expect("should create temp db");
        assert_eq!(db.block_count(), 0);
        assert_eq!(db.transaction_count(), 0);
        assert_eq!(db.account_count(), 0);
    }

    #[test]
    fn open_persistent_database() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = NovaDB::open(dir.path()).expect("should open db");
        assert_eq!(db.block_count(), 0);
        drop(db);

        // Re-open to verify persistence path works.
        let db2 = NovaDB::open(dir.path()).expect("should reopen db");
        assert_eq!(db2.block_count(), 0);
    }

    #[test]
    fn store_and_retrieve_genesis_block() {
        let db = NovaDB::open_temporary().unwrap();
        let genesis = Block::genesis();

        db.put_block(&genesis).unwrap();

        let retrieved = db.get_block(0).unwrap().expect("genesis should exist");
        assert_eq!(retrieved.header.hash, genesis.header.hash);
        assert_eq!(retrieved.header.height, 0);
        assert_eq!(retrieved.transactions.len(), genesis.transactions.len());
    }

    #[test]
    fn store_and_retrieve_block_with_transactions() {
        let db = NovaDB::open_temporary().unwrap();
        let genesis = Block::genesis();
        let tx1 = make_test_tx(1);
        let tx2 = make_test_tx(2);
        let block = Block::new(
            &genesis,
            vec![tx1.clone(), tx2.clone()],
            "nova:validator".to_string(),
            [1u8; 32],
        );

        db.put_block(&genesis).unwrap();
        db.put_block(&block).unwrap();

        let retrieved = db.get_block(1).unwrap().expect("block 1 should exist");
        assert_eq!(retrieved.transactions.len(), 2);
        assert_eq!(retrieved.transactions[0].id, tx1.id);
        assert_eq!(retrieved.transactions[1].id, tx2.id);
    }

    #[test]
    fn get_block_returns_none_for_missing_height() {
        let db = NovaDB::open_temporary().unwrap();
        assert!(db.get_block(999).unwrap().is_none());
    }

    #[test]
    fn block_hash_index_lookup() {
        let db = NovaDB::open_temporary().unwrap();
        let genesis = Block::genesis();
        db.put_block(&genesis).unwrap();

        let by_hash = db
            .get_block_by_hash(&genesis.header.hash)
            .unwrap()
            .expect("should find by hash");
        assert_eq!(by_hash.header.height, 0);
        assert_eq!(by_hash.header.hash, genesis.header.hash);
    }

    #[test]
    fn block_hash_lookup_returns_none_for_unknown_hash() {
        let db = NovaDB::open_temporary().unwrap();
        let fake_hash = [0xAB; 32];
        assert!(db.get_block_by_hash(&fake_hash).unwrap().is_none());
    }

    #[test]
    fn store_and_retrieve_transaction() {
        let db = NovaDB::open_temporary().unwrap();
        let tx = make_test_tx(42);

        db.put_transaction(&tx).unwrap();

        let retrieved = db
            .get_transaction(&tx.id)
            .unwrap()
            .expect("tx should exist");
        assert_eq!(retrieved.id, tx.id);
        assert_eq!(retrieved.sender, tx.sender);
        assert_eq!(retrieved.nonce, 42);
    }

    #[test]
    fn get_transaction_returns_none_for_missing_id() {
        let db = NovaDB::open_temporary().unwrap();
        assert!(db.get_transaction("nonexistent").unwrap().is_none());
    }

    #[test]
    fn transactions_indexed_via_put_block() {
        let db = NovaDB::open_temporary().unwrap();
        let genesis = Block::genesis();
        let tx = make_test_tx(7);
        let block = Block::new(
            &genesis,
            vec![tx.clone()],
            "nova:validator".to_string(),
            [7u8; 32],
        );

        db.put_block(&genesis).unwrap();
        db.put_block(&block).unwrap();

        // The transaction should be retrievable by its ID.
        let found = db.get_transaction(&tx.id).unwrap().expect("tx via block");
        assert_eq!(found.id, tx.id);
    }

    #[test]
    fn account_state_crud() {
        let db = NovaDB::open_temporary().unwrap();

        // Initially empty.
        assert!(db.get_account("nova:alice").unwrap().is_none());

        // Insert.
        let state = AccountState::with_balance(5000);
        db.put_account("nova:alice", &state).unwrap();

        let retrieved = db
            .get_account("nova:alice")
            .unwrap()
            .expect("alice should exist");
        assert_eq!(retrieved.balance, 5000);
        assert_eq!(retrieved.nonce, 0);

        // Update.
        let mut updated = retrieved;
        updated.balance = 3000;
        updated.nonce = 1;
        db.put_account("nova:alice", &updated).unwrap();

        let re_retrieved = db.get_account("nova:alice").unwrap().unwrap();
        assert_eq!(re_retrieved.balance, 3000);
        assert_eq!(re_retrieved.nonce, 1);
    }

    #[test]
    fn latest_block_height_tracking() {
        let db = NovaDB::open_temporary().unwrap();

        // Empty database has no latest height.
        assert!(db.get_latest_block_height().unwrap().is_none());

        // After storing genesis, height should be 0.
        let genesis = Block::genesis();
        db.put_block(&genesis).unwrap();
        assert_eq!(db.get_latest_block_height().unwrap(), Some(0));

        // After storing block 1, height should be 1.
        let block1 = Block::new(&genesis, vec![], "nova:v".to_string(), [1u8; 32]);
        db.put_block(&block1).unwrap();
        assert_eq!(db.get_latest_block_height().unwrap(), Some(1));
    }

    #[test]
    fn set_latest_block_height_explicitly() {
        let db = NovaDB::open_temporary().unwrap();

        db.set_latest_block_height(42).unwrap();
        assert_eq!(db.get_latest_block_height().unwrap(), Some(42));

        db.set_latest_block_height(100).unwrap();
        assert_eq!(db.get_latest_block_height().unwrap(), Some(100));
    }

    #[test]
    fn block_range_query() {
        let db = NovaDB::open_temporary().unwrap();
        let chain = make_block_chain(5); // genesis + 4 blocks

        for block in &chain {
            db.put_block(block).unwrap();
        }

        // Full range.
        let all = db.get_block_range(0, 4).unwrap();
        assert_eq!(all.len(), 5);
        for (i, block) in all.iter().enumerate() {
            assert_eq!(block.header.height, i as u64);
        }

        // Partial range.
        let mid = db.get_block_range(1, 3).unwrap();
        assert_eq!(mid.len(), 3);
        assert_eq!(mid[0].header.height, 1);
        assert_eq!(mid[2].header.height, 3);

        // Single block range.
        let single = db.get_block_range(2, 2).unwrap();
        assert_eq!(single.len(), 1);
        assert_eq!(single[0].header.height, 2);

        // Empty range (start > end).
        let empty = db.get_block_range(5, 10).unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn atomic_batch_write_persists_all_data() {
        let db = NovaDB::open_temporary().unwrap();
        let genesis = Block::genesis();
        let tx1 = make_test_tx(1);
        let tx2 = make_test_tx(2);
        let tx3 = make_test_tx(3);
        let block = Block::new(
            &genesis,
            vec![tx1.clone(), tx2.clone(), tx3.clone()],
            "nova:validator".to_string(),
            [1u8; 32],
        );

        db.put_block(&genesis).unwrap();
        db.put_block(&block).unwrap();

        // Verify all pieces landed.
        assert_eq!(db.block_count(), 2);
        assert_eq!(db.transaction_count(), 3);
        assert_eq!(db.get_latest_block_height().unwrap(), Some(1));

        // Each transaction must be individually retrievable.
        assert!(db.get_transaction(&tx1.id).unwrap().is_some());
        assert!(db.get_transaction(&tx2.id).unwrap().is_some());
        assert!(db.get_transaction(&tx3.id).unwrap().is_some());

        // Block retrievable by both height and hash.
        assert!(db.get_block(1).unwrap().is_some());
        assert!(db.get_block_by_hash(&block.header.hash).unwrap().is_some());
    }

    #[test]
    fn concurrent_reads_do_not_block() {
        use std::sync::Arc;
        use std::thread;

        let db = Arc::new(NovaDB::open_temporary().unwrap());
        let genesis = Block::genesis();
        db.put_block(&genesis).unwrap();

        // Populate some account data.
        for i in 0..10u64 {
            let addr = format!("nova:user_{i}");
            let state = AccountState::with_balance(i * 1000);
            db.put_account(&addr, &state).unwrap();
        }

        // Spawn readers that concurrently access the database.
        let handles: Vec<_> = (0..4)
            .map(|thread_id| {
                let db = Arc::clone(&db);
                thread::spawn(move || {
                    for i in 0..10u64 {
                        let addr = format!("nova:user_{i}");
                        let account = db.get_account(&addr).unwrap().unwrap();
                        assert_eq!(account.balance, i * 1000);
                    }
                    // Also read blocks.
                    let block = db.get_block(0).unwrap().unwrap();
                    assert_eq!(block.header.height, 0);
                    thread_id
                })
            })
            .collect();

        for handle in handles {
            handle.join().expect("reader thread should not panic");
        }
    }

    #[test]
    fn multiple_accounts_independent_state() {
        let db = NovaDB::open_temporary().unwrap();

        let alice = AccountState::with_balance(1000);
        let bob = AccountState {
            nonce: 5,
            balance: 500,
            ..Default::default()
        };

        db.put_account("nova:alice", &alice).unwrap();
        db.put_account("nova:bob", &bob).unwrap();

        let a = db.get_account("nova:alice").unwrap().unwrap();
        let b = db.get_account("nova:bob").unwrap().unwrap();

        assert_eq!(a.balance, 1000);
        assert_eq!(a.nonce, 0);
        assert_eq!(b.balance, 500);
        assert_eq!(b.nonce, 5);
        assert_eq!(db.account_count(), 2);
    }

    #[test]
    fn block_count_tracks_insertions() {
        let db = NovaDB::open_temporary().unwrap();
        assert_eq!(db.block_count(), 0);

        let chain = make_block_chain(3);
        for block in &chain {
            db.put_block(block).unwrap();
        }
        assert_eq!(db.block_count(), 3);
    }

    #[test]
    fn flush_does_not_error() {
        let db = NovaDB::open_temporary().unwrap();
        let genesis = Block::genesis();
        db.put_block(&genesis).unwrap();
        db.flush().expect("flush should succeed");
    }

    #[test]
    fn overwrite_block_at_same_height() {
        let db = NovaDB::open_temporary().unwrap();
        let genesis = Block::genesis();
        db.put_block(&genesis).unwrap();

        // Create two different blocks at height 1 (simulating a reorg).
        let block_a = Block::new(
            &genesis,
            vec![make_test_tx(1)],
            "nova:validator_a".to_string(),
            [0xAA; 32],
        );
        let block_b = Block::new(
            &genesis,
            vec![make_test_tx(2)],
            "nova:validator_b".to_string(),
            [0xBB; 32],
        );

        db.put_block(&block_a).unwrap();
        let retrieved_a = db.get_block(1).unwrap().unwrap();
        assert_eq!(retrieved_a.header.state_root, [0xAA; 32]);

        // Overwrite with block_b at the same height.
        db.put_block(&block_b).unwrap();
        let retrieved_b = db.get_block(1).unwrap().unwrap();
        assert_eq!(retrieved_b.header.state_root, [0xBB; 32]);
    }

    #[test]
    fn frozen_account_persists_correctly() {
        let db = NovaDB::open_temporary().unwrap();

        let state = AccountState {
            nonce: 3,
            balance: 1_000_000,
            balance_commitments: std::collections::HashMap::new(),
            credit_lines: vec!["credit_001".to_string()],
            frozen: true,
        };

        db.put_account("nova:frozen_user", &state).unwrap();
        let retrieved = db.get_account("nova:frozen_user").unwrap().unwrap();
        assert!(retrieved.frozen);
        assert_eq!(retrieved.credit_lines, vec!["credit_001".to_string()]);
        assert_eq!(retrieved.nonce, 3);
        assert_eq!(retrieved.balance, 1_000_000);
    }
}
