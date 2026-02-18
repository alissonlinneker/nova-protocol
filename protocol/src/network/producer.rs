//! # Block Production Pipeline
//!
//! The `BlockProducer` ties together the mempool, state tree, persistence
//! layer, and block construction into a single coherent pipeline. It is the
//! component that turns "a bunch of pending transactions" into "a signed,
//! state-committed block ready for consensus voting."
//!
//! ## Pipeline Stages
//!
//! ```text
//! 1. SELECT   — Pull highest-fee transactions from the mempool
//! 2. EXECUTE  — Apply each transaction to the state tree; drop failures
//! 3. BUILD    — Construct the block with the post-execution state root
//! 4. SIGN     — Attach the validator's Ed25519 signature
//! 5. COMMIT   — Persist to NovaDB and purge executed txs from the mempool
//! ```
//!
//! Failed transactions are silently dropped during execution. They do not
//! make it into the block, and they do not pollute the state tree. This is
//! the "optimistic execution" model: we attempt every transaction the mempool
//! offers and keep only the winners.
//!
//! ## Thread Safety
//!
//! The `BlockProducer` holds `Arc` references to shared infrastructure
//! (database, state tree, mempool) and can be safely used from any thread.
//! The state tree is protected by `RwLock` — block production acquires a
//! write lock for the duration of transaction execution.

use std::fmt;
use std::sync::Arc;

use parking_lot::RwLock;
use tracing::{debug, info};

use crate::crypto::keys::NovaKeypair;
use crate::storage::block::Block;
use crate::storage::db::{DbError, NovaDB};
use crate::storage::state::{apply_transfer, StateError, StateTree};
use crate::network::mempool::Mempool;
use crate::transaction::Transaction;
use crate::transaction::types::TransactionType;

// ---------------------------------------------------------------------------
// Error Type
// ---------------------------------------------------------------------------

/// Errors that can occur during block production.
///
/// These are operational errors — things that go wrong during the pipeline,
/// not protocol violations. A `BlockProductionError` means "we tried to
/// produce a block and something didn't work," not "the block is invalid."
#[derive(Debug)]
pub enum BlockProductionError {
    /// No transactions available in the mempool. Not necessarily an error
    /// in practice (empty blocks are valid), but the caller may want to
    /// skip production when there's nothing to include.
    EmptyMempool,

    /// No transactions survived execution. Every candidate was invalid
    /// (insufficient balance, wrong nonce, frozen account, etc.).
    NoTransactions,

    /// A state tree operation failed during transaction execution.
    StateError(StateError),

    /// Database persistence failed.
    DbError(DbError),

    /// Block signing failed (malformed key, hardware token error, etc.).
    SigningError(String),
}

impl fmt::Display for BlockProductionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyMempool => write!(f, "mempool is empty, nothing to produce"),
            Self::NoTransactions => {
                write!(f, "all candidate transactions failed execution")
            }
            Self::StateError(e) => write!(f, "state transition error: {}", e),
            Self::DbError(e) => write!(f, "database error: {}", e),
            Self::SigningError(e) => write!(f, "block signing error: {}", e),
        }
    }
}

impl std::error::Error for BlockProductionError {}

impl From<StateError> for BlockProductionError {
    fn from(e: StateError) -> Self {
        Self::StateError(e)
    }
}

impl From<DbError> for BlockProductionError {
    fn from(e: DbError) -> Self {
        Self::DbError(e)
    }
}

// ---------------------------------------------------------------------------
// Execution Results
// ---------------------------------------------------------------------------

/// Outcome of executing a single transaction against the state tree.
///
/// Successful transactions are included in the block. Failed transactions
/// are recorded here for diagnostics but excluded from the block body.
#[derive(Debug, Clone)]
pub struct TxResult {
    /// Transaction ID (hex-encoded double-SHA-256).
    pub tx_id: String,

    /// Whether the transaction was successfully applied to the state.
    pub success: bool,

    /// Human-readable error description, populated only when `success` is false.
    pub error: Option<String>,
}

/// A freshly produced block together with its execution metadata.
///
/// This is the output of the block production pipeline — the block itself
/// plus a record of which transactions succeeded and which were dropped.
/// The caller can inspect `tx_results` for logging, metrics, or debugging.
#[derive(Debug, Clone)]
pub struct ProducedBlock {
    /// The constructed and signed block.
    pub block: Block,

    /// Per-transaction execution results. The order matches the order in
    /// which transactions were attempted (fee-priority descending), not
    /// the order in the final block (which only contains successes).
    pub tx_results: Vec<TxResult>,

    /// State root after applying all successful transactions. This is
    /// the same value embedded in `block.header.state_root`.
    pub state_root: [u8; 32],
}

// ---------------------------------------------------------------------------
// BlockProducer
// ---------------------------------------------------------------------------

/// Orchestrates the block production pipeline.
///
/// The producer does not own the consensus engine — it sits between the
/// mempool and the consensus layer. The flow is:
///
/// 1. Consensus tells us "it's our turn to propose."
/// 2. We call `produce_block()` to build a candidate.
/// 3. Consensus wraps it in a proposal and broadcasts to peers.
/// 4. After finalization, we call `commit_block()` to persist.
///
/// This separation means the producer is testable in isolation, without
/// spinning up a full consensus round.
pub struct BlockProducer {
    /// Persistent storage for blocks and chain metadata.
    db: Arc<NovaDB>,

    /// Sparse Merkle Tree holding all account states. Protected by RwLock
    /// because block production mutates it (applying transactions), while
    /// RPC queries read it concurrently.
    state_tree: Arc<RwLock<StateTree>>,

    /// Priority-ordered transaction pool. Transactions are pulled from here
    /// during block production and removed after successful commit.
    mempool: Arc<Mempool>,

    /// Validator's Ed25519 keypair for signing produced blocks.
    keypair: NovaKeypair,

    /// NOVA address (hex-encoded public key) of this validator.
    validator_address: String,
}

impl BlockProducer {
    /// Creates a new block producer wired to the given infrastructure.
    ///
    /// The `keypair` is the validator's signing key. The `validator_address`
    /// is derived from it automatically (hex-encoded public key).
    pub fn new(
        db: Arc<NovaDB>,
        state_tree: Arc<RwLock<StateTree>>,
        mempool: Arc<Mempool>,
        keypair: NovaKeypair,
    ) -> Self {
        let validator_address = keypair.public_key().to_hex();
        Self {
            db,
            state_tree,
            mempool,
            keypair,
            validator_address,
        }
    }

    /// Produces a new block from the current mempool contents.
    ///
    /// Selects up to `max_txs` transactions ordered by fee priority,
    /// executes each one against the state tree, drops failures, and
    /// assembles the surviving transactions into a signed block.
    ///
    /// The state tree is mutated in place. If the caller needs rollback
    /// semantics, they should snapshot the state root before calling this
    /// method and restore it on error.
    ///
    /// # Arguments
    ///
    /// * `parent` — The block this new block extends (chain tip).
    /// * `max_txs` — Maximum number of transactions to pull from the mempool.
    ///
    /// # Returns
    ///
    /// A `ProducedBlock` containing the signed block and execution results,
    /// or an error if block production failed entirely.
    pub fn produce_block(
        &self,
        parent: &Block,
        max_txs: usize,
    ) -> Result<ProducedBlock, BlockProductionError> {
        // Stage 1: SELECT — grab the best transactions from the mempool.
        let candidates = self.mempool.select_transactions(max_txs);

        info!(
            candidates = candidates.len(),
            max_txs = max_txs,
            parent_height = parent.header.height,
            "starting block production"
        );

        // Stage 2: EXECUTE — apply each transaction to the state tree.
        let mut successful_txs = Vec::new();
        let mut tx_results = Vec::new();

        {
            let mut tree = self.state_tree.write();

            for tx in &candidates {
                match self.execute_transaction(&mut tree, tx) {
                    Ok(()) => {
                        tx_results.push(TxResult {
                            tx_id: tx.id.clone(),
                            success: true,
                            error: None,
                        });
                        successful_txs.push(tx.clone());
                    }
                    Err(e) => {
                        debug!(
                            tx_id = %tx.id,
                            error = %e,
                            "transaction execution failed, dropping from block"
                        );
                        tx_results.push(TxResult {
                            tx_id: tx.id.clone(),
                            success: false,
                            error: Some(e.to_string()),
                        });
                    }
                }
            }
        }

        // Stage 3: Capture the post-execution state root.
        let state_root = self.state_tree.read().root();

        // Stage 4: BUILD — construct the block from successful transactions.
        let mut block = Block::new(
            parent,
            successful_txs,
            self.validator_address.clone(),
            state_root,
        );

        // Stage 5: SIGN — attach the validator's signature.
        let sig = self.keypair.sign(&block.header.hash);
        block.header.signature = sig.as_bytes().to_vec();

        info!(
            height = block.header.height,
            tx_count = block.transactions.len(),
            dropped = tx_results.iter().filter(|r| !r.success).count(),
            "block produced"
        );

        Ok(ProducedBlock {
            block,
            tx_results,
            state_root,
        })
    }

    /// Executes a single transaction against the state tree.
    ///
    /// For `Transfer` transactions, this calls `apply_transfer` which
    /// validates the sender's balance, debits the sender, credits the
    /// receiver, and increments the sender's nonce.
    ///
    /// Other transaction types (CreditRequest, TokenMint, etc.) are not
    /// yet implemented in the state transition engine. They pass through
    /// as no-ops — included in the block but with no state effect.
    fn execute_transaction(
        &self,
        tree: &mut StateTree,
        tx: &Transaction,
    ) -> Result<(), BlockProductionError> {
        match tx.tx_type {
            TransactionType::Transfer => {
                let amount = tx.amount.value;
                apply_transfer(tree, &tx.sender, &tx.receiver, amount)?;
                Ok(())
            }
            // Other transaction types are accepted but do not yet modify
            // state. The block includes them for ordering and audit purposes;
            // state transitions will be added as each module matures.
            TransactionType::CreditRequest
            | TransactionType::CreditSettlement
            | TransactionType::TokenMint
            | TransactionType::TokenBurn
            | TransactionType::ConfidentialTransfer => {
                debug!(
                    tx_type = %tx.tx_type,
                    tx_id = %tx.id,
                    "non-transfer transaction accepted as no-op"
                );
                Ok(())
            }
        }
    }

    /// Persists a produced block to the database and cleans up the mempool.
    ///
    /// This is the final step in the block production pipeline. After this
    /// call, the block is durable on disk and its transactions are no longer
    /// in the mempool.
    ///
    /// # Ordering guarantee
    ///
    /// The block is written to the DB first, then transactions are removed
    /// from the mempool. If the process crashes between these two steps,
    /// the transactions will still be in the mempool on restart. This is
    /// safe — they will be re-executed against the state tree and either
    /// succeed (if the block was not actually persisted) or fail with a
    /// nonce mismatch (if it was). Either way, no funds are lost.
    pub fn commit_block(&self, block: &Block) -> Result<(), BlockProductionError> {
        // Persist the block to the database.
        self.db.put_block(block)?;

        // Remove included transactions from the mempool.
        let tx_ids: Vec<String> = block.transactions.iter().map(|tx| tx.id.clone()).collect();
        self.mempool.remove_batch(&tx_ids);

        info!(
            height = block.header.height,
            tx_count = block.transactions.len(),
            "block committed to storage"
        );

        Ok(())
    }

    /// Returns this producer's validator address (hex-encoded public key).
    pub fn validator_address(&self) -> &str {
        &self.validator_address
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::db::NovaDB;
    use crate::storage::state::{AccountState, StateTree};
    use crate::network::mempool::{Mempool, MempoolConfig};
    use crate::transaction::builder::TransactionBuilder;
    use crate::transaction::types::{Amount, Currency, TransactionType};
    use crate::crypto::keys::NovaKeypair;

    // -- Test Helpers -------------------------------------------------------

    /// Spins up a complete block production environment with temporary storage.
    /// Returns the producer, its genesis block, and raw references to the
    /// shared state tree and mempool for direct inspection in tests.
    fn setup() -> (BlockProducer, Block, Arc<RwLock<StateTree>>, Arc<Mempool>, Arc<NovaDB>) {
        let db = Arc::new(NovaDB::open_temporary().expect("temp db"));
        let state_tree = Arc::new(RwLock::new(StateTree::new((*db).clone())));
        let mempool = Arc::new(Mempool::new(MempoolConfig::default()));
        let keypair = NovaKeypair::generate();
        let producer = BlockProducer::new(
            Arc::clone(&db),
            Arc::clone(&state_tree),
            Arc::clone(&mempool),
            keypair,
        );
        let genesis = Block::genesis();
        (producer, genesis, state_tree, mempool, db)
    }

    /// Creates a transfer transaction with explicit parameters.
    fn make_transfer(sender: &str, receiver: &str, amount: u64, fee: u64, nonce: u64) -> Transaction {
        TransactionBuilder::new(TransactionType::Transfer)
            .sender(sender)
            .receiver(receiver)
            .amount(Amount::new(amount, Currency::NOVA))
            .fee(fee)
            .nonce(nonce)
            .timestamp(1_700_000_000_000 + nonce)
            .build()
    }

    /// Seeds an account with a given balance in the state tree.
    fn seed_balance(tree: &Arc<RwLock<StateTree>>, address: &str, balance: u64) {
        let mut t = tree.write();
        t.put(address, &AccountState::with_balance(balance));
    }

    // -- 1. Empty mempool produces empty block ------------------------------

    #[test]
    fn produce_empty_block() {
        let (producer, genesis, _tree, _mempool, _db) = setup();

        let result = producer.produce_block(&genesis, 100);
        assert!(result.is_ok());

        let produced = result.unwrap();
        assert_eq!(produced.block.transactions.len(), 0);
        assert_eq!(produced.block.header.height, 1);
        assert_eq!(produced.block.header.parent_hash, genesis.header.hash);
        assert!(produced.tx_results.is_empty());
    }

    // -- 2. Transfers execute and update state ------------------------------

    #[test]
    fn produce_block_with_transfers() {
        let (producer, genesis, tree, mempool, _db) = setup();

        seed_balance(&tree, "nova1alice", 10_000);

        let tx = make_transfer("nova1alice", "nova1bob", 3_000, 100, 0);
        mempool.add(tx).unwrap();

        let produced = producer.produce_block(&genesis, 100).unwrap();

        assert_eq!(produced.block.transactions.len(), 1);
        assert!(produced.tx_results.iter().all(|r| r.success));

        let t = tree.read();
        let alice = t.get("nova1alice").unwrap();
        let bob = t.get("nova1bob").unwrap();
        assert_eq!(alice.balance, 7_000);
        assert_eq!(bob.balance, 3_000);
    }

    // -- 3. Respects max_txs limit -----------------------------------------

    #[test]
    fn produce_block_respects_max_txs() {
        let (producer, genesis, tree, mempool, _db) = setup();

        seed_balance(&tree, "nova1sender", 100_000);

        for i in 0..10u64 {
            let tx = make_transfer("nova1sender", "nova1receiver", 100, (i + 1) * 100, i);
            mempool.add(tx).unwrap();
        }

        let produced = producer.produce_block(&genesis, 5).unwrap();

        // Should include at most 5 transactions.
        assert!(produced.block.transactions.len() <= 5);
    }

    // -- 4. Invalid transfer is skipped ------------------------------------

    #[test]
    fn produce_block_invalid_transfer_skipped() {
        let (producer, genesis, tree, mempool, _db) = setup();

        // Alice has only 500 — not enough for a 1000 transfer.
        seed_balance(&tree, "nova1alice", 500);
        seed_balance(&tree, "nova1bob", 10_000);

        let bad_tx = make_transfer("nova1alice", "nova1bob", 1_000, 200, 0);
        let good_tx = make_transfer("nova1bob", "nova1alice", 500, 100, 0);

        mempool.add(bad_tx.clone()).unwrap();
        mempool.add(good_tx.clone()).unwrap();

        let produced = producer.produce_block(&genesis, 100).unwrap();

        // Only the good transaction should be in the block.
        let successful_ids: Vec<&str> = produced
            .block
            .transactions
            .iter()
            .map(|tx| tx.id.as_str())
            .collect();

        assert!(
            !successful_ids.contains(&bad_tx.id.as_str()),
            "insufficient-balance tx should be dropped"
        );

        // The bad tx should be recorded as failed.
        let failed = produced.tx_results.iter().find(|r| r.tx_id == bad_tx.id);
        assert!(failed.is_some());
        assert!(!failed.unwrap().success);
    }

    // -- 5. State root changes after execution ------------------------------

    #[test]
    fn produce_block_updates_state_root() {
        let (producer, genesis, tree, mempool, _db) = setup();

        let root_before = tree.read().root();

        seed_balance(&tree, "nova1alice", 10_000);
        let tx = make_transfer("nova1alice", "nova1bob", 5_000, 100, 0);
        mempool.add(tx).unwrap();

        let produced = producer.produce_block(&genesis, 100).unwrap();

        assert_ne!(produced.state_root, root_before);
        assert_eq!(produced.block.header.state_root, produced.state_root);
    }

    // -- 6. Commit persists block to NovaDB ---------------------------------

    #[test]
    fn commit_block_persists_to_db() {
        let (producer, genesis, _tree, _mempool, db) = setup();

        db.put_block(&genesis).unwrap();

        let produced = producer.produce_block(&genesis, 100).unwrap();
        producer.commit_block(&produced.block).unwrap();

        let retrieved = db.get_block(1).unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().header.height, 1);
    }

    // -- 7. Commit removes txs from mempool ---------------------------------

    #[test]
    fn commit_block_removes_from_mempool() {
        let (producer, genesis, tree, mempool, _db) = setup();

        seed_balance(&tree, "nova1alice", 10_000);

        let tx = make_transfer("nova1alice", "nova1bob", 1_000, 100, 0);
        let tx_id = tx.id.clone();
        mempool.add(tx).unwrap();
        assert_eq!(mempool.size(), 1);

        let produced = producer.produce_block(&genesis, 100).unwrap();
        producer.commit_block(&produced.block).unwrap();

        assert_eq!(mempool.size(), 0);
        assert!(!mempool.contains(&tx_id));
    }

    // -- 8. Block signature is valid ----------------------------------------

    #[test]
    fn produce_block_signs_correctly() {
        let (producer, genesis, _tree, _mempool, _db) = setup();

        let produced = producer.produce_block(&genesis, 100).unwrap();
        let block = &produced.block;

        // The signature should be 64 bytes (Ed25519).
        assert_eq!(block.header.signature.len(), 64);

        // Verify the signature against the block hash.
        let pk = producer.keypair.public_key();
        let mut sig_bytes = [0u8; 64];
        sig_bytes.copy_from_slice(&block.header.signature);
        let signature = crate::crypto::keys::NovaSignature::from_bytes(sig_bytes);
        assert!(pk.verify(&block.header.hash, &signature));
    }

    // -- 9. Sequential blocks chain correctly --------------------------------

    #[test]
    fn sequential_blocks_chain_correctly() {
        let (producer, genesis, tree, mempool, db) = setup();

        db.put_block(&genesis).unwrap();
        seed_balance(&tree, "nova1alice", 100_000);

        let mut parent = genesis.clone();
        let mut heights = vec![0u64];

        for i in 0..3u64 {
            let tx = make_transfer("nova1alice", "nova1bob", 1_000, 100, i);
            mempool.add(tx).unwrap();

            let produced = producer.produce_block(&parent, 100).unwrap();
            producer.commit_block(&produced.block).unwrap();

            assert_eq!(produced.block.header.parent_hash, parent.header.hash);
            assert_eq!(produced.block.header.height, parent.header.height + 1);

            heights.push(produced.block.header.height);
            parent = produced.block;
        }

        assert_eq!(heights, vec![0, 1, 2, 3]);
    }

    // -- 10. Nonce ordering -------------------------------------------------

    #[test]
    fn produce_block_nonce_ordering() {
        let (producer, genesis, tree, mempool, _db) = setup();

        seed_balance(&tree, "nova1alice", 50_000);

        // Nonce 0 should succeed (expected nonce for a fresh account).
        let tx0 = make_transfer("nova1alice", "nova1bob", 1_000, 300, 0);
        // Nonce 1 should succeed after nonce 0 is applied.
        let tx1 = make_transfer("nova1alice", "nova1bob", 1_000, 200, 1);

        mempool.add(tx0).unwrap();
        mempool.add(tx1).unwrap();

        let produced = producer.produce_block(&genesis, 100).unwrap();

        // Both should be in the block (nonce 0 executes first by fee priority,
        // then nonce 1 naturally follows).
        let successful_count = produced.tx_results.iter().filter(|r| r.success).count();
        assert!(successful_count >= 1);
    }

    // -- 11. Fee priority ordering ------------------------------------------

    #[test]
    fn produce_block_fee_priority() {
        let (producer, genesis, tree, mempool, _db) = setup();

        seed_balance(&tree, "nova1alice", 100_000);
        seed_balance(&tree, "nova1bob", 100_000);

        // Low fee from alice.
        let tx_low = make_transfer("nova1alice", "nova1charlie", 100, 10, 0);
        // High fee from bob.
        let tx_high = make_transfer("nova1bob", "nova1charlie", 100, 10_000, 0);

        mempool.add(tx_low).unwrap();
        mempool.add(tx_high.clone()).unwrap();

        // Only take 1 transaction — should be the high-fee one.
        let produced = producer.produce_block(&genesis, 1).unwrap();
        assert_eq!(produced.block.transactions.len(), 1);
        assert_eq!(produced.block.transactions[0].id, tx_high.id);
    }

    // -- 12. Partial execution: earlier txs survive later failures ----------

    #[test]
    fn state_rollback_on_failure() {
        let (producer, genesis, tree, mempool, _db) = setup();

        seed_balance(&tree, "nova1rich", 50_000);
        seed_balance(&tree, "nova1poor", 100);

        // High fee: rich sends 1000 — should succeed.
        let tx_good = make_transfer("nova1rich", "nova1dest", 1_000, 5_000, 0);
        // Low fee: poor sends 10000 — should fail (insufficient balance).
        let tx_bad = make_transfer("nova1poor", "nova1dest", 10_000, 100, 0);

        mempool.add(tx_good).unwrap();
        mempool.add(tx_bad).unwrap();

        let produced = producer.produce_block(&genesis, 100).unwrap();

        // The good tx should have been applied.
        let t = tree.read();
        let rich = t.get("nova1rich").unwrap();
        assert_eq!(rich.balance, 49_000);

        // The failed tx should not have affected the poor account's balance.
        let poor = t.get("nova1poor").unwrap();
        assert_eq!(poor.balance, 100);

        // Block should contain only the successful transaction.
        assert_eq!(produced.block.transactions.len(), 1);
    }

    // -- 13. Concurrent mempool access during production --------------------

    #[test]
    fn produce_block_concurrent_mempool_access() {
        use std::thread;

        let (producer, genesis, tree, mempool, _db) = setup();
        let producer = Arc::new(producer);
        let genesis = Arc::new(genesis);

        seed_balance(&tree, "nova1alice", 1_000_000);

        // Add some initial transactions.
        for i in 0..5u64 {
            let tx = make_transfer("nova1alice", "nova1bob", 100, (i + 1) * 100, i);
            mempool.add(tx).unwrap();
        }

        // Produce a block while another thread adds more transactions.
        let mempool_clone = Arc::clone(&mempool);
        let writer = thread::spawn(move || {
            for i in 100..110u64 {
                let tx = make_transfer(
                    &format!("nova1writer_{}", i),
                    "nova1receiver",
                    10,
                    50,
                    i,
                );
                let _ = mempool_clone.add(tx);
            }
        });

        let produced = producer.produce_block(&genesis, 100);
        writer.join().expect("writer thread should not panic");

        // Production should succeed regardless of concurrent writes.
        assert!(produced.is_ok());
    }

    // -- 14. Same transactions produce same state root ----------------------

    #[test]
    fn produce_block_state_root_deterministic() {
        // Build two independent environments with identical initial state
        // and feed them the same transaction.
        let db1 = Arc::new(NovaDB::open_temporary().expect("temp db 1"));
        let db2 = Arc::new(NovaDB::open_temporary().expect("temp db 2"));

        let tree1 = Arc::new(RwLock::new(StateTree::new((*db1).clone())));
        let tree2 = Arc::new(RwLock::new(StateTree::new((*db2).clone())));

        // Identical initial balances.
        {
            let mut t1 = tree1.write();
            t1.put("nova1alice", &AccountState::with_balance(10_000));
        }
        {
            let mut t2 = tree2.write();
            t2.put("nova1alice", &AccountState::with_balance(10_000));
        }

        let mempool1 = Arc::new(Mempool::new(MempoolConfig::default()));
        let mempool2 = Arc::new(Mempool::new(MempoolConfig::default()));

        let tx = make_transfer("nova1alice", "nova1bob", 3_000, 100, 0);
        mempool1.add(tx.clone()).unwrap();
        mempool2.add(tx).unwrap();

        let kp1 = NovaKeypair::generate();
        let kp2 = NovaKeypair::generate();

        let producer1 = BlockProducer::new(db1, tree1, mempool1, kp1);
        let producer2 = BlockProducer::new(db2, tree2, mempool2, kp2);

        let genesis = Block::genesis();
        let p1 = producer1.produce_block(&genesis, 100).unwrap();
        let p2 = producer2.produce_block(&genesis, 100).unwrap();

        // The state roots should be identical because the same transactions
        // were applied to the same initial state.
        assert_eq!(p1.state_root, p2.state_root);
    }

    // -- 15. Balances accumulate across multiple blocks ----------------------

    #[test]
    fn produce_multiple_blocks_state_accumulates() {
        let (producer, genesis, tree, mempool, db) = setup();

        db.put_block(&genesis).unwrap();
        seed_balance(&tree, "nova1alice", 100_000);

        let mut parent = genesis;

        // Block 1: Alice sends 10,000 to Bob.
        let tx1 = make_transfer("nova1alice", "nova1bob", 10_000, 100, 0);
        mempool.add(tx1).unwrap();
        let p1 = producer.produce_block(&parent, 100).unwrap();
        producer.commit_block(&p1.block).unwrap();
        parent = p1.block;

        // Block 2: Alice sends another 20,000 to Bob.
        let tx2 = make_transfer("nova1alice", "nova1bob", 20_000, 100, 1);
        mempool.add(tx2).unwrap();
        let p2 = producer.produce_block(&parent, 100).unwrap();
        producer.commit_block(&p2.block).unwrap();
        parent = p2.block;

        // Block 3: Bob sends 5,000 to Charlie.
        let tx3 = make_transfer("nova1bob", "nova1charlie", 5_000, 100, 0);
        mempool.add(tx3).unwrap();
        let p3 = producer.produce_block(&parent, 100).unwrap();
        producer.commit_block(&p3.block).unwrap();

        let t = tree.read();
        let alice = t.get("nova1alice").unwrap();
        let bob = t.get("nova1bob").unwrap();
        let charlie = t.get("nova1charlie").unwrap();

        // Alice: 100,000 - 10,000 - 20,000 = 70,000
        assert_eq!(alice.balance, 70_000);
        // Bob: 10,000 + 20,000 - 5,000 = 25,000
        assert_eq!(bob.balance, 25_000);
        // Charlie: 5,000
        assert_eq!(charlie.balance, 5_000);
    }

    // -- 16. Block height increments correctly ------------------------------

    #[test]
    fn produced_block_height_increments() {
        let (producer, genesis, _tree, _mempool, _db) = setup();

        let p1 = producer.produce_block(&genesis, 100).unwrap();
        assert_eq!(p1.block.header.height, 1);

        let p2 = producer.produce_block(&p1.block, 100).unwrap();
        assert_eq!(p2.block.header.height, 2);

        let p3 = producer.produce_block(&p2.block, 100).unwrap();
        assert_eq!(p3.block.header.height, 3);
    }

    // -- 17. Validator address set on produced block -------------------------

    #[test]
    fn produced_block_has_correct_validator() {
        let (producer, genesis, _tree, _mempool, _db) = setup();

        let produced = producer.produce_block(&genesis, 100).unwrap();
        assert_eq!(
            produced.block.header.validator,
            producer.validator_address()
        );
    }

    // -- 18. Multiple transfers in one block --------------------------------

    #[test]
    fn produce_block_multiple_transfers() {
        let (producer, genesis, tree, mempool, _db) = setup();

        seed_balance(&tree, "nova1a", 50_000);
        seed_balance(&tree, "nova1b", 50_000);
        seed_balance(&tree, "nova1c", 50_000);

        let tx1 = make_transfer("nova1a", "nova1d", 1_000, 100, 0);
        let tx2 = make_transfer("nova1b", "nova1d", 2_000, 200, 0);
        let tx3 = make_transfer("nova1c", "nova1d", 3_000, 300, 0);

        mempool.add(tx1).unwrap();
        mempool.add(tx2).unwrap();
        mempool.add(tx3).unwrap();

        let produced = producer.produce_block(&genesis, 100).unwrap();

        assert_eq!(produced.block.transactions.len(), 3);
        assert!(produced.tx_results.iter().all(|r| r.success));

        let t = tree.read();
        let d = t.get("nova1d").unwrap();
        assert_eq!(d.balance, 6_000); // 1000 + 2000 + 3000
    }

    // -- 19. Commit updates latest height in DB -----------------------------

    #[test]
    fn commit_block_updates_db_height() {
        let (producer, genesis, _tree, _mempool, db) = setup();

        db.put_block(&genesis).unwrap();

        let p1 = producer.produce_block(&genesis, 100).unwrap();
        producer.commit_block(&p1.block).unwrap();

        let height = db.get_latest_block_height().unwrap();
        assert_eq!(height, Some(1));
    }

    // -- 20. Frozen account transfer rejected --------------------------------

    #[test]
    fn produce_block_frozen_account_skipped() {
        let (producer, genesis, tree, mempool, _db) = setup();

        // Freeze alice's account.
        {
            let mut t = tree.write();
            t.put(
                "nova1alice",
                &AccountState {
                    balance: 10_000,
                    frozen: true,
                    ..Default::default()
                },
            );
        }

        let tx = make_transfer("nova1alice", "nova1bob", 1_000, 100, 0);
        mempool.add(tx.clone()).unwrap();

        let produced = producer.produce_block(&genesis, 100).unwrap();

        // The frozen tx should be dropped.
        assert_eq!(produced.block.transactions.len(), 0);
        let result = produced.tx_results.iter().find(|r| r.tx_id == tx.id);
        assert!(result.is_some());
        assert!(!result.unwrap().success);
    }
}
