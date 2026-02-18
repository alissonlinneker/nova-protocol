//! # Consensus-Driven Block Production Loop
//!
//! The `ConsensusLoop` is the heartbeat of a NOVA validator. It ties together
//! the consensus engine, block producer, mempool, and persistence layer into
//! a single async loop that proposes, votes on, and finalizes blocks.
//!
//! ## How it works
//!
//! Each iteration ("round") of the loop:
//!
//! 1. Check if we are the designated proposer for the current consensus round.
//! 2. If yes: produce a block, self-vote (sufficient for single-validator devnet),
//!    finalize via the consensus engine, and commit to persistent storage.
//! 3. If no: skip the round. In a production deployment with gossip, we would
//!    wait for the proposer's block over the network. That plumbing comes later.
//! 4. Sleep for `block_time_ms` before starting the next round.
//! 5. If the mempool is empty, add `empty_block_delay_ms` to the sleep — no
//!    point burning cycles producing blocks that carry no transactions.
//!
//! ## Shutdown
//!
//! The loop monitors a `tokio::sync::watch` channel. When the sender drops or
//! sends `true`, the loop exits cleanly after finishing its current round.
//! No in-flight blocks are left half-committed.
//!
//! ## Single-Validator Mode
//!
//! For devnet and testing, a single validator is both proposer and sole voter.
//! The quorum threshold is 1, so a self-vote is sufficient for finalization.
//! Multi-validator gossip-based voting is a separate concern handled by the
//! networking layer (see `gossip.rs`).

use std::fmt;
use std::sync::Arc;

use parking_lot::RwLock;
use tracing::{debug, info, warn};

use crate::crypto::keys::NovaKeypair;
use crate::network::consensus::{ConsensusEngine, ConsensusError, FinalizedBlock, Vote};
use crate::network::mempool::Mempool;
use crate::network::producer::{BlockProducer, BlockProductionError};
use crate::storage::db::{DbError, NovaDB};
use crate::storage::state::StateTree;
use crate::storage::Block;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Tunable parameters for the consensus loop.
///
/// These control timing, throughput, and liveness. Defaults are tuned for
/// a responsive devnet — production deployments will want to increase
/// `max_txs_per_block` and decrease `empty_block_delay_ms` once the
/// network has real traffic.
#[derive(Debug, Clone)]
pub struct ConsensusLoopConfig {
    /// Target time between blocks, in milliseconds. The loop sleeps for
    /// this duration after each round (successful or not). Setting this
    /// too low burns CPU; too high hurts perceived transaction latency.
    pub block_time_ms: u64,

    /// Maximum number of transactions pulled from the mempool per block.
    /// Acts as a throughput cap — the block producer will never include
    /// more than this many transactions regardless of mempool depth.
    pub max_txs_per_block: usize,

    /// Extra delay (in milliseconds) added to the sleep when the mempool
    /// is empty. Prevents the validator from spinning on empty blocks when
    /// there is no demand. Set to 0 if you want empty blocks at full cadence
    /// (useful for testing, wasteful in production).
    pub empty_block_delay_ms: u64,

    /// Maximum consecutive rounds without producing a block before the
    /// proposer slot advances. Guards against a validator that is "online"
    /// but failing to produce (e.g., persistent state corruption).
    pub max_rounds_without_block: u64,
}

impl Default for ConsensusLoopConfig {
    fn default() -> Self {
        Self {
            block_time_ms: 1500,
            max_txs_per_block: 1000,
            empty_block_delay_ms: 5000,
            max_rounds_without_block: 3,
        }
    }
}

// ---------------------------------------------------------------------------
// Error Type
// ---------------------------------------------------------------------------

/// Errors that can occur during the consensus loop lifecycle.
///
/// These map cleanly to the subsystem that failed. The caller (typically the
/// validator node runtime) can decide whether to retry, log, or shut down
/// based on the variant.
#[derive(Debug)]
pub enum ConsensusLoopError {
    /// This node is not in the active validator set. It cannot propose or
    /// vote, so running the consensus loop is pointless.
    NotValidator,

    /// The block production pipeline failed (state error, signing error, etc.).
    ProductionError(BlockProductionError),

    /// The consensus engine rejected the block or votes (invalid proposer,
    /// insufficient quorum, height mismatch, etc.).
    ConsensusError(ConsensusError),

    /// Database persistence failed.
    DbError(DbError),

    /// The shutdown signal was received. This is the happy path — the loop
    /// exited because someone asked it to, not because something broke.
    Shutdown,
}

impl fmt::Display for ConsensusLoopError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotValidator => write!(f, "node is not in the active validator set"),
            Self::ProductionError(e) => write!(f, "block production failed: {}", e),
            Self::ConsensusError(e) => write!(f, "consensus error: {}", e),
            Self::DbError(e) => write!(f, "database error: {}", e),
            Self::Shutdown => write!(f, "consensus loop received shutdown signal"),
        }
    }
}

impl std::error::Error for ConsensusLoopError {}

impl From<BlockProductionError> for ConsensusLoopError {
    fn from(e: BlockProductionError) -> Self {
        Self::ProductionError(e)
    }
}

impl From<ConsensusError> for ConsensusLoopError {
    fn from(e: ConsensusError) -> Self {
        Self::ConsensusError(e)
    }
}

impl From<DbError> for ConsensusLoopError {
    fn from(e: DbError) -> Self {
        Self::DbError(e)
    }
}

// ---------------------------------------------------------------------------
// ConsensusLoop
// ---------------------------------------------------------------------------

/// The async consensus loop that drives block production for a validator node.
///
/// Owns shared references to every subsystem involved in block production:
/// the consensus engine (proposer selection + finalization), block producer
/// (transaction execution + block construction), database (persistence),
/// state tree (account balances), and mempool (pending transactions).
///
/// Thread safety: all shared state is behind `Arc<RwLock<_>>` or `Arc<_>`.
/// The loop itself runs on a single tokio task but the referenced data
/// structures are accessed concurrently by the RPC and networking layers.
pub struct ConsensusLoop {
    /// Consensus engine — manages validator set, round tracking, and
    /// block finalization rules.
    engine: Arc<RwLock<ConsensusEngine>>,

    /// Block producer — transaction execution and block construction pipeline.
    producer: Arc<BlockProducer>,

    /// Persistent storage for blocks and chain metadata.
    db: Arc<NovaDB>,

    /// Sparse Merkle Tree for account state. Held here for state queries
    /// during epoch boundary evaluation and validator set recalculation.
    #[allow(dead_code)]
    state_tree: Arc<RwLock<StateTree>>,

    /// Priority-ordered transaction pool.
    mempool: Arc<Mempool>,

    /// This validator's signing keypair.
    keypair: NovaKeypair,

    /// Loop timing and throughput configuration.
    config: ConsensusLoopConfig,
}

impl ConsensusLoop {
    /// Creates a new consensus loop wired to the given infrastructure.
    ///
    /// Does not start the loop — call [`run`](Self::run) or
    /// [`run_single_round`](Self::run_single_round) to begin producing blocks.
    pub fn new(
        engine: Arc<RwLock<ConsensusEngine>>,
        producer: Arc<BlockProducer>,
        db: Arc<NovaDB>,
        state_tree: Arc<RwLock<StateTree>>,
        mempool: Arc<Mempool>,
        keypair: NovaKeypair,
        config: ConsensusLoopConfig,
    ) -> Self {
        Self {
            engine,
            producer,
            db,
            state_tree,
            mempool,
            keypair,
            config,
        }
    }

    /// Runs the consensus loop until a shutdown signal is received.
    ///
    /// This is the main entry point for block production. It runs indefinitely,
    /// producing blocks when it is this validator's turn and sleeping between
    /// rounds. The loop checks the shutdown channel at every iteration boundary.
    ///
    /// Returns `Ok(())` if the loop exits cleanly via shutdown, or an error
    /// if a fatal (non-retryable) condition is encountered.
    pub async fn run(
        &self,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> Result<(), ConsensusLoopError> {
        info!("consensus loop starting");

        loop {
            // Check shutdown signal before each round.
            if *shutdown.borrow() {
                info!("consensus loop received shutdown signal, exiting cleanly");
                return Err(ConsensusLoopError::Shutdown);
            }

            // Run one round of consensus.
            match self.run_single_round() {
                Ok(Some(finalized)) => {
                    info!(
                        height = finalized.block.header.height,
                        txs = finalized.block.transactions.len(),
                        round = finalized.round,
                        "block finalized in consensus loop"
                    );
                }
                Ok(None) => {
                    debug!("not our turn to propose, skipping round");
                }
                Err(ConsensusLoopError::ProductionError(BlockProductionError::EmptyMempool)) => {
                    debug!("mempool empty, delaying next round");
                }
                Err(e) => {
                    warn!(error = %e, "consensus round failed");
                }
            }

            // Determine sleep duration. If the mempool is empty, add extra delay
            // to avoid churning on empty blocks.
            let sleep_ms = if self.mempool.is_empty() {
                self.config.block_time_ms + self.config.empty_block_delay_ms
            } else {
                self.config.block_time_ms
            };

            // Sleep with shutdown awareness — wake up early if shutdown fires.
            tokio::select! {
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(sleep_ms)) => {}
                _ = shutdown.changed() => {
                    info!("consensus loop shutting down during sleep");
                    return Err(ConsensusLoopError::Shutdown);
                }
            }
        }
    }

    /// Executes a single round of the consensus protocol.
    ///
    /// If this validator is the designated proposer for the current round:
    /// 1. Retrieve the latest block from the database (chain tip).
    /// 2. Produce a new block via the block producer pipeline.
    /// 3. Cast a self-vote on the produced block.
    /// 4. Finalize the block through the consensus engine.
    /// 5. Commit the finalized block to persistent storage.
    ///
    /// If this validator is NOT the proposer, returns `Ok(None)`. In a
    /// multi-validator deployment, we would wait for the proposer's block
    /// via gossip — that coordination layer is not yet wired in.
    pub fn run_single_round(&self) -> Result<Option<FinalizedBlock>, ConsensusLoopError> {
        if !self.is_our_turn() {
            return Ok(None);
        }

        let engine = self.engine.read();
        let current_round = engine.current_round();
        drop(engine);

        // Step 1: Get the chain tip as parent block.
        let parent = self.get_latest_block()?;

        // Step 2: Produce a block from the current mempool.
        let produced = self
            .producer
            .produce_block(&parent, self.config.max_txs_per_block)?;

        // Step 3: Self-vote on the block we just produced.
        let block_hash = produced.block.header.hash;
        let vote = self.self_vote(block_hash, current_round);

        // Step 4: Finalize the block through the consensus engine.
        let finalized = {
            let mut engine = self.engine.write();
            engine.finalize_block(produced.block, vec![vote])?
        };

        // Step 5: Commit to persistent storage and drain mempool.
        self.producer.commit_block(&finalized.block)?;

        debug!(
            height = finalized.block.header.height,
            round = finalized.round,
            txs = finalized.block.transactions.len(),
            "round completed successfully"
        );

        Ok(Some(finalized))
    }

    /// Returns `true` if this validator is the designated proposer for the
    /// current consensus round.
    ///
    /// Compares our validator address (hex-encoded public key) against the
    /// proposer selected by the round-robin algorithm in the validator set.
    pub fn is_our_turn(&self) -> bool {
        let engine = self.engine.read();
        let round = engine.current_round();
        let validator_set = engine.validator_set();

        let proposer = match validator_set.proposer_for_round(round) {
            Some(p) => p,
            None => return false,
        };

        let our_address = self.keypair.public_key().to_hex();
        proposer.address == our_address
    }

    /// Creates a signed vote from this validator for the given block hash
    /// and round number.
    ///
    /// The vote signature covers `block_hash || round.to_le_bytes()`, which
    /// prevents cross-round replay attacks.
    pub fn self_vote(&self, block_hash: [u8; 32], round: u64) -> Vote {
        Vote::new(&self.keypair, block_hash, round)
    }

    /// Returns a reference to the loop configuration.
    pub fn config(&self) -> &ConsensusLoopConfig {
        &self.config
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Retrieves the latest block from the database.
    ///
    /// If the DB has a recorded latest height, fetches that block. Otherwise,
    /// falls back to the genesis block. This handles both fresh starts
    /// (genesis only) and restarts after producing blocks.
    fn get_latest_block(&self) -> Result<Block, ConsensusLoopError> {
        let height = self
            .db
            .get_latest_block_height()
            .map_err(ConsensusLoopError::DbError)?;

        match height {
            Some(h) => {
                let block = self
                    .db
                    .get_block(h)
                    .map_err(ConsensusLoopError::DbError)?
                    .ok_or_else(|| {
                        ConsensusLoopError::DbError(DbError::NotFound(format!(
                            "block at height {} not found despite metadata claiming it exists",
                            h
                        )))
                    })?;
                Ok(block)
            }
            None => {
                // No blocks in DB — use genesis as the parent.
                let genesis = Block::genesis();
                Ok(genesis)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::keys::NovaKeypair;
    use crate::network::consensus::{ConsensusConfig, ConsensusEngine, ValidatorSet};
    use crate::network::mempool::{Mempool, MempoolConfig};
    use crate::network::producer::BlockProducer;
    use crate::storage::db::NovaDB;
    use crate::storage::state::{AccountState, StateTree};
    use crate::storage::Block;
    use crate::transaction::builder::TransactionBuilder;
    use crate::transaction::types::{Amount, Currency, TransactionType};

    // -----------------------------------------------------------------------
    // Test Helpers
    // -----------------------------------------------------------------------

    /// Spins up a complete consensus loop environment with temporary storage,
    /// a single validator, and matching consensus/producer configuration.
    /// Returns everything needed to drive the loop and inspect results.
    struct TestHarness {
        consensus_loop: ConsensusLoop,
        engine: Arc<RwLock<ConsensusEngine>>,
        db: Arc<NovaDB>,
        state_tree: Arc<RwLock<StateTree>>,
        mempool: Arc<Mempool>,
        keypair: NovaKeypair,
    }

    fn setup() -> TestHarness {
        setup_with_config(ConsensusLoopConfig::default())
    }

    fn setup_with_config(loop_config: ConsensusLoopConfig) -> TestHarness {
        let keypair = NovaKeypair::generate();
        let address = keypair.public_key().to_hex();

        // Single-validator set: quorum = 1, so self-vote is sufficient.
        let mut validator_set = ValidatorSet::new();
        validator_set.add_validator(address, 10_000_000_000);

        let consensus_config = ConsensusConfig {
            min_validators: 1,
            ..ConsensusConfig::default()
        };

        let engine = Arc::new(RwLock::new(ConsensusEngine::new(
            consensus_config,
            validator_set,
        )));

        let db = Arc::new(NovaDB::open_temporary().expect("temp db"));
        let state_tree = Arc::new(RwLock::new(StateTree::new((*db).clone())));
        let mempool = Arc::new(Mempool::new(MempoolConfig::default()));

        // Persist genesis and sync the engine to start at height 1.
        // This mirrors what a real validator does on first boot: the genesis
        // block is committed to storage and the engine tracks chain state
        // from that point forward.
        let genesis = Block::genesis();
        db.put_block(&genesis).unwrap();
        {
            let mut eng = engine.write();
            eng.set_chain_state(1, genesis.header.hash);
        }

        let producer = Arc::new(BlockProducer::new(
            Arc::clone(&db),
            Arc::clone(&state_tree),
            Arc::clone(&mempool),
            keypair.clone(),
        ));

        let consensus_loop = ConsensusLoop::new(
            Arc::clone(&engine),
            Arc::clone(&producer),
            Arc::clone(&db),
            Arc::clone(&state_tree),
            Arc::clone(&mempool),
            keypair.clone(),
            loop_config,
        );

        TestHarness {
            consensus_loop,
            engine,
            db,
            state_tree,
            mempool,
            keypair,
        }
    }

    /// Creates a transfer transaction with explicit parameters.
    fn make_transfer(
        sender: &str,
        receiver: &str,
        amount: u64,
        fee: u64,
        nonce: u64,
    ) -> crate::transaction::Transaction {
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

    // -----------------------------------------------------------------------
    // 1. Single round produces a block when we have transactions
    // -----------------------------------------------------------------------

    #[test]
    fn single_round_produces_block() {
        let h = setup();

        seed_balance(&h.state_tree, "nova1alice", 100_000);

        let tx = make_transfer("nova1alice", "nova1bob", 5_000, 100, 0);
        h.mempool.add(tx).unwrap();

        let result = h.consensus_loop.run_single_round();
        assert!(result.is_ok());

        let finalized = result.unwrap();
        assert!(finalized.is_some());

        let block = finalized.unwrap();
        // Genesis is at height 0; the first produced block is at height 1.
        assert_eq!(block.block.header.height, 1);
        assert_eq!(block.block.transactions.len(), 1);
    }

    // -----------------------------------------------------------------------
    // 2. Single round with empty mempool produces an empty block
    // -----------------------------------------------------------------------

    #[test]
    fn single_round_empty_mempool() {
        let h = setup();

        let result = h.consensus_loop.run_single_round();
        assert!(result.is_ok());

        let finalized = result.unwrap();
        assert!(finalized.is_some());

        let block = finalized.unwrap();
        assert_eq!(block.block.transactions.len(), 0);
    }

    // -----------------------------------------------------------------------
    // 3. is_our_turn returns true when we are the proposer
    // -----------------------------------------------------------------------

    #[test]
    fn is_our_turn_correct() {
        let h = setup();

        // We are the only validator, round 0 — must be our turn.
        assert!(h.consensus_loop.is_our_turn());
    }

    // -----------------------------------------------------------------------
    // 4. is_our_turn returns false for a non-proposer
    // -----------------------------------------------------------------------

    #[test]
    fn is_our_turn_wrong_validator() {
        // Create a loop where the consensus engine has a DIFFERENT validator
        // than the one holding the keypair.
        let keypair = NovaKeypair::generate();
        let other_keypair = NovaKeypair::generate();
        let other_address = other_keypair.public_key().to_hex();

        let mut validator_set = ValidatorSet::new();
        validator_set.add_validator(other_address, 10_000_000_000);

        let consensus_config = ConsensusConfig {
            min_validators: 1,
            ..ConsensusConfig::default()
        };

        let engine = Arc::new(RwLock::new(ConsensusEngine::new(
            consensus_config,
            validator_set,
        )));

        let db = Arc::new(NovaDB::open_temporary().expect("temp db"));
        let state_tree = Arc::new(RwLock::new(StateTree::new((*db).clone())));
        let mempool = Arc::new(Mempool::new(MempoolConfig::default()));

        let producer = Arc::new(BlockProducer::new(
            Arc::clone(&db),
            Arc::clone(&state_tree),
            Arc::clone(&mempool),
            keypair.clone(),
        ));

        let consensus_loop = ConsensusLoop::new(
            engine,
            producer,
            db,
            state_tree,
            mempool,
            keypair,
            ConsensusLoopConfig::default(),
        );

        assert!(!consensus_loop.is_our_turn());
    }

    // -----------------------------------------------------------------------
    // 5. Self-vote passes signature verification
    // -----------------------------------------------------------------------

    #[test]
    fn self_vote_valid() {
        let h = setup();

        let block_hash = [42u8; 32];
        let vote = h.consensus_loop.self_vote(block_hash, 0);

        assert!(vote.verify());
        assert_eq!(vote.block_hash, block_hash);
        assert_eq!(vote.round, 0);
        assert_eq!(vote.validator, h.keypair.public_key().to_hex());
    }

    // -----------------------------------------------------------------------
    // 6. Block is finalized after production + self-vote
    // -----------------------------------------------------------------------

    #[test]
    fn finalize_after_production() {
        let h = setup();

        let result = h.consensus_loop.run_single_round().unwrap();
        let finalized = result.expect("should produce a finalized block");

        assert_eq!(finalized.votes.len(), 1);
        assert!(finalized.votes[0].verify());
        assert_eq!(finalized.round, 0);
    }

    // -----------------------------------------------------------------------
    // 7. Sequential rounds produce blocks at incrementing heights
    // -----------------------------------------------------------------------

    #[test]
    fn sequential_rounds_increment_height() {
        let h = setup();

        seed_balance(&h.state_tree, "nova1alice", 1_000_000);

        let mut heights = Vec::new();

        for i in 0..3u64 {
            let tx = make_transfer("nova1alice", "nova1bob", 100, 50, i);
            h.mempool.add(tx).unwrap();

            let result = h.consensus_loop.run_single_round().unwrap();
            let finalized = result.expect("should produce block");
            heights.push(finalized.block.header.height);
        }

        assert_eq!(heights, vec![1, 2, 3]);
    }

    // -----------------------------------------------------------------------
    // 8. Engine round increments after finalization
    // -----------------------------------------------------------------------

    #[test]
    fn round_advances_after_finalization() {
        let h = setup();

        let round_before = h.engine.read().current_round();
        assert_eq!(round_before, 0);

        h.consensus_loop.run_single_round().unwrap();

        let round_after = h.engine.read().current_round();
        assert_eq!(round_after, 1);
    }

    // -----------------------------------------------------------------------
    // 9. Default config values are sane
    // -----------------------------------------------------------------------

    #[test]
    fn config_defaults_are_sane() {
        let config = ConsensusLoopConfig::default();

        assert_eq!(config.block_time_ms, 1500);
        assert_eq!(config.max_txs_per_block, 1000);
        assert_eq!(config.empty_block_delay_ms, 5000);
        assert_eq!(config.max_rounds_without_block, 3);

        // Block time should be under the 2-second finality target.
        assert!(config.block_time_ms < 2000);

        // Max transactions should be a power-of-10 or at least reasonable.
        assert!(config.max_txs_per_block >= 100);
        assert!(config.max_txs_per_block <= 10_000);
    }

    // -----------------------------------------------------------------------
    // 10. Produced block has a non-zero state root after transfers
    // -----------------------------------------------------------------------

    #[test]
    fn produced_block_has_state_root() {
        let h = setup();

        seed_balance(&h.state_tree, "nova1alice", 50_000);

        let tx = make_transfer("nova1alice", "nova1bob", 10_000, 100, 0);
        h.mempool.add(tx).unwrap();

        let result = h.consensus_loop.run_single_round().unwrap();
        let finalized = result.expect("should finalize");

        // State root should be non-zero — transfers mutate the state tree.
        assert_ne!(finalized.block.header.state_root, [0u8; 32]);
    }

    // -----------------------------------------------------------------------
    // 11. Produced block is persisted to NovaDB after round
    // -----------------------------------------------------------------------

    #[test]
    fn produced_block_persisted() {
        let h = setup();

        let result = h.consensus_loop.run_single_round().unwrap();
        let finalized = result.expect("should finalize");
        let height = finalized.block.header.height;

        let retrieved = h.db.get_block(height).unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().header.height, height);
    }

    // -----------------------------------------------------------------------
    // 12. Included transactions are removed from mempool after round
    // -----------------------------------------------------------------------

    #[test]
    fn mempool_drained_after_round() {
        let h = setup();

        seed_balance(&h.state_tree, "nova1alice", 100_000);

        let tx1 = make_transfer("nova1alice", "nova1bob", 1_000, 100, 0);
        let tx2 = make_transfer("nova1alice", "nova1bob", 2_000, 200, 1);

        let tx1_id = tx1.id.clone();
        let tx2_id = tx2.id.clone();

        h.mempool.add(tx1).unwrap();
        h.mempool.add(tx2).unwrap();
        assert_eq!(h.mempool.size(), 2);

        h.consensus_loop.run_single_round().unwrap();

        // Both transactions should have been drained from the mempool.
        assert!(!h.mempool.contains(&tx1_id));
        assert!(!h.mempool.contains(&tx2_id));
        assert_eq!(h.mempool.size(), 0);
    }

    // -----------------------------------------------------------------------
    // 13. Shutdown signal stops the loop
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn shutdown_signal_stops_loop() {
        let h = setup_with_config(ConsensusLoopConfig {
            block_time_ms: 50,
            empty_block_delay_ms: 50,
            ..ConsensusLoopConfig::default()
        });

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        // Signal shutdown after a brief delay.
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            let _ = shutdown_tx.send(true);
        });

        let result = h.consensus_loop.run(shutdown_rx).await;

        // The loop should exit with a Shutdown error (the clean exit path).
        assert!(matches!(result, Err(ConsensusLoopError::Shutdown)));
    }

    // -----------------------------------------------------------------------
    // 14. State tree is consistent after multiple rounds
    // -----------------------------------------------------------------------

    #[test]
    fn multiple_rounds_state_consistency() {
        let h = setup();

        seed_balance(&h.state_tree, "nova1alice", 1_000_000);

        // Run 5 rounds, each transferring 10,000 from alice to bob.
        for i in 0..5u64 {
            let tx = make_transfer("nova1alice", "nova1bob", 10_000, 100, i);
            h.mempool.add(tx).unwrap();

            let result = h.consensus_loop.run_single_round().unwrap();
            assert!(result.is_some());
        }

        // Verify final balances.
        let tree = h.state_tree.read();
        let alice = tree.get("nova1alice").unwrap();
        let bob = tree.get("nova1bob").unwrap();

        // Alice: 1,000,000 - (5 * 10,000) = 950,000
        assert_eq!(alice.balance, 950_000);
        // Bob: 5 * 10,000 = 50,000
        assert_eq!(bob.balance, 50_000);

        // State root should be consistent with the tree.
        let root = tree.root();
        assert_ne!(root, [0u8; 32]);
    }

    // -----------------------------------------------------------------------
    // 15. Non-validator cannot produce blocks
    // -----------------------------------------------------------------------

    #[test]
    fn non_validator_cannot_produce() {
        // Set up a loop where the keypair holder is NOT in the validator set.
        let our_keypair = NovaKeypair::generate();
        let other_keypair = NovaKeypair::generate();
        let other_address = other_keypair.public_key().to_hex();

        let mut validator_set = ValidatorSet::new();
        validator_set.add_validator(other_address, 10_000_000_000);

        let consensus_config = ConsensusConfig {
            min_validators: 1,
            ..ConsensusConfig::default()
        };

        let engine = Arc::new(RwLock::new(ConsensusEngine::new(
            consensus_config,
            validator_set,
        )));

        let db = Arc::new(NovaDB::open_temporary().expect("temp db"));
        let state_tree = Arc::new(RwLock::new(StateTree::new((*db).clone())));
        let mempool = Arc::new(Mempool::new(MempoolConfig::default()));

        let producer = Arc::new(BlockProducer::new(
            Arc::clone(&db),
            Arc::clone(&state_tree),
            Arc::clone(&mempool),
            our_keypair.clone(),
        ));

        let consensus_loop = ConsensusLoop::new(
            engine,
            producer,
            db,
            state_tree,
            mempool,
            our_keypair,
            ConsensusLoopConfig::default(),
        );

        // is_our_turn should return false.
        assert!(!consensus_loop.is_our_turn());

        // run_single_round should return None (not our turn, skip).
        let result = consensus_loop.run_single_round().unwrap();
        assert!(result.is_none());
    }

    // -----------------------------------------------------------------------
    // 16. Config is accessible from the loop
    // -----------------------------------------------------------------------

    #[test]
    fn config_accessible() {
        let custom_config = ConsensusLoopConfig {
            block_time_ms: 2000,
            max_txs_per_block: 500,
            empty_block_delay_ms: 3000,
            max_rounds_without_block: 5,
        };

        let h = setup_with_config(custom_config.clone());

        let config = h.consensus_loop.config();
        assert_eq!(config.block_time_ms, 2000);
        assert_eq!(config.max_txs_per_block, 500);
        assert_eq!(config.empty_block_delay_ms, 3000);
        assert_eq!(config.max_rounds_without_block, 5);
    }

    // -----------------------------------------------------------------------
    // 17. Self-vote round number matches engine round
    // -----------------------------------------------------------------------

    #[test]
    fn self_vote_round_matches_engine() {
        let h = setup();

        let engine_round = h.engine.read().current_round();
        let vote = h.consensus_loop.self_vote([0xAB; 32], engine_round);

        assert_eq!(vote.round, engine_round);
        assert!(vote.verify());
    }

    // -----------------------------------------------------------------------
    // 18. Latest block height updates in DB after each round
    // -----------------------------------------------------------------------

    #[test]
    fn db_height_updates_each_round() {
        let h = setup();

        h.consensus_loop.run_single_round().unwrap();
        assert_eq!(h.db.get_latest_block_height().unwrap(), Some(1));

        h.consensus_loop.run_single_round().unwrap();
        assert_eq!(h.db.get_latest_block_height().unwrap(), Some(2));
    }

    // -----------------------------------------------------------------------
    // 19. Block validator field matches our address
    // -----------------------------------------------------------------------

    #[test]
    fn block_validator_is_ours() {
        let h = setup();

        let result = h.consensus_loop.run_single_round().unwrap();
        let finalized = result.expect("should produce block");

        // The block's validator field should be our public key hex.
        let our_address = h.keypair.public_key().to_hex();
        assert_eq!(finalized.block.header.validator, our_address);
    }
}
