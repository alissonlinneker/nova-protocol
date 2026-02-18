//! # Hybrid PoS+PoA Consensus Engine
//!
//! NOVA uses a hybrid Proof-of-Stake / Proof-of-Authority consensus mechanism
//! that combines the economic security of staking with the operational efficiency
//! of a known validator set.
//!
//! ## How it works
//!
//! 1. **Proposer selection (PoS)**: Validators are sorted by stake. Block
//!    production rotates round-robin among the top `max_validators` stakers.
//!    Higher stake = you get a slot sooner, but everyone in the active set
//!    gets their turn. No grinding, no VRF lottery — deterministic and auditable.
//!
//! 2. **Block signing (PoA)**: Only validators in the active authority set can
//!    sign blocks. The set is updated at epoch boundaries (every N blocks).
//!
//! 3. **BFT finality**: A block is finalized when 2/3 + 1 of the active
//!    validator set has voted for it. This follows the standard PBFT quorum
//!    threshold — fewer votes and you can't guarantee safety under Byzantine
//!    faults.
//!
//! ## Consensus Round State Machine
//!
//! ```text
//! Propose -> Prevote -> Precommit -> Commit
//!    ^                                 |
//!    +----------- (next round) --------+
//! ```
//!
//! Each round has a designated proposer. If the proposer fails to produce a
//! block within the timeout, the round advances and the next proposer takes
//! over. Liveness is guaranteed as long as > 2/3 of validators are honest
//! and online.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::crypto::keys::{NovaKeypair, NovaPublicKey, NovaSignature};
use crate::storage::{Block, BlockHeader};
use crate::transaction::Transaction;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration parameters for the consensus engine.
///
/// These values are set at genesis and should not change without a
/// coordinated hard fork. Changing `block_time_ms` or `stake_requirement`
/// mid-chain will cause consensus failures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusConfig {
    /// Target time between blocks, in milliseconds.
    pub block_time_ms: u64,
    /// Minimum number of validators required for the network to produce blocks.
    pub min_validators: usize,
    /// Maximum number of validators in the active set.
    pub max_validators: usize,
    /// Minimum stake required to enter the active validator set, in photons.
    pub stake_requirement: u64,
    /// Number of blocks per epoch. The validator set is re-evaluated at
    /// each epoch boundary.
    pub epoch_length: u64,
    /// Maximum number of transactions per block.
    pub max_block_transactions: usize,
    /// Timeout for a consensus round before advancing to the next proposer,
    /// in milliseconds.
    pub round_timeout_ms: u64,
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        Self {
            block_time_ms: crate::config::BLOCK_TIME_MS,
            min_validators: 4,
            max_validators: 100,
            stake_requirement: 1_000_000_000, // 10 NOVA (at 8 decimals)
            epoch_length: 100,
            max_block_transactions: 1_000,
            round_timeout_ms: 5_000,
        }
    }
}

// ---------------------------------------------------------------------------
// Validator Info & Set
// ---------------------------------------------------------------------------

/// Information about a single validator in the active set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatorInfo {
    /// Hex-encoded public key of the validator.
    pub address: String,
    /// Amount staked, in photons.
    pub stake: u64,
    /// Whether this validator is currently active (online and participating).
    pub active: bool,
    /// Total number of blocks proposed by this validator.
    pub blocks_proposed: u64,
    /// Total number of blocks where this validator voted.
    pub blocks_voted: u64,
}

/// The current set of active validators, sorted by stake (descending).
///
/// The validator set determines who can propose and vote on blocks.
/// It is recalculated at each epoch boundary based on the current
/// stake distribution in the state tree.
#[derive(Debug, Clone, Default)]
pub struct ValidatorSet {
    /// Validators sorted by stake, highest first.
    validators: Vec<ValidatorInfo>,
}

impl ValidatorSet {
    /// Creates an empty validator set.
    pub fn new() -> Self {
        Self {
            validators: Vec::new(),
        }
    }

    /// Adds a validator to the set. The set is re-sorted after insertion
    /// to maintain stake-descending order.
    pub fn add_validator(&mut self, address: String, stake: u64) {
        self.validators.push(ValidatorInfo {
            address,
            stake,
            active: true,
            blocks_proposed: 0,
            blocks_voted: 0,
        });
        self.validators.sort_by(|a, b| b.stake.cmp(&a.stake));
    }

    /// Removes a validator by address.
    pub fn remove_validator(&mut self, address: &str) {
        self.validators.retain(|v| v.address != address);
    }

    /// Returns the number of validators in the set.
    pub fn len(&self) -> usize {
        self.validators.len()
    }

    /// Returns true if the validator set is empty.
    pub fn is_empty(&self) -> bool {
        self.validators.is_empty()
    }

    /// Returns the list of all validators, sorted by stake (descending).
    pub fn validators(&self) -> &[ValidatorInfo] {
        &self.validators
    }

    /// Returns the validator at a given index.
    pub fn get(&self, index: usize) -> Option<&ValidatorInfo> {
        self.validators.get(index)
    }

    /// Returns the proposer for a given round number.
    ///
    /// Selection is round-robin among the active set, ordered by stake.
    /// Round 0 goes to the highest-staked validator, round 1 to the
    /// second-highest, and so on, wrapping around.
    pub fn proposer_for_round(&self, round: u64) -> Option<&ValidatorInfo> {
        let active: Vec<&ValidatorInfo> = self.validators.iter().filter(|v| v.active).collect();
        if active.is_empty() {
            return None;
        }
        let index = (round as usize) % active.len();
        Some(active[index])
    }

    /// Computes the BFT quorum threshold: 2/3 + 1 of the active validator count.
    ///
    /// This is the minimum number of votes required to finalize a block.
    /// With N validators, the threshold is `(2 * N / 3) + 1`.
    pub fn quorum_threshold(&self) -> usize {
        let active_count = self.validators.iter().filter(|v| v.active).count();
        if active_count == 0 {
            return 0;
        }
        (2 * active_count / 3) + 1
    }

    /// Returns the total stake across all active validators.
    pub fn total_stake(&self) -> u64 {
        self.validators
            .iter()
            .filter(|v| v.active)
            .map(|v| v.stake)
            .sum()
    }

    /// Checks if an address is in the active validator set.
    pub fn contains(&self, address: &str) -> bool {
        self.validators
            .iter()
            .any(|v| v.address == address && v.active)
    }
}

// ---------------------------------------------------------------------------
// Consensus Round
// ---------------------------------------------------------------------------

/// State machine phases for a single consensus round.
///
/// Each round progresses linearly through these phases. If a timeout
/// occurs at any phase, the round is abandoned and a new round begins
/// with the next proposer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsensusRound {
    /// Waiting for the designated proposer to broadcast a block.
    Propose,
    /// Validators examine the proposed block and broadcast prevotes.
    Prevote,
    /// After receiving 2/3+ prevotes, validators broadcast precommits.
    Precommit,
    /// After receiving 2/3+ precommits, the block is committed to the chain.
    Commit,
}

impl ConsensusRound {
    /// Advances to the next phase. Returns `None` if already at `Commit`.
    pub fn next(self) -> Option<Self> {
        match self {
            Self::Propose => Some(Self::Prevote),
            Self::Prevote => Some(Self::Precommit),
            Self::Precommit => Some(Self::Commit),
            Self::Commit => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Vote
// ---------------------------------------------------------------------------

/// A validator's vote on a proposed block.
///
/// Votes are broadcast during the Prevote and Precommit phases. The
/// signature covers `(block_hash || round)` to prevent replay across rounds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    /// Hex-encoded public key of the voting validator.
    pub validator: String,
    /// Hash of the block being voted on.
    pub block_hash: [u8; 32],
    /// Ed25519 signature over `(block_hash || round.to_le_bytes())`.
    pub signature: NovaSignature,
    /// Consensus round number this vote belongs to.
    pub round: u64,
}

impl Vote {
    /// Creates a new signed vote.
    ///
    /// The signature covers the concatenation of the block hash and the
    /// round number (little-endian u64), preventing cross-round replay.
    pub fn new(keypair: &NovaKeypair, block_hash: [u8; 32], round: u64) -> Self {
        let mut message = Vec::with_capacity(40);
        message.extend_from_slice(&block_hash);
        message.extend_from_slice(&round.to_le_bytes());

        let signature = keypair.sign(&message);

        Self {
            validator: keypair.public_key().to_hex(),
            block_hash,
            signature,
            round,
        }
    }

    /// Verifies this vote's signature against the validator's public key.
    pub fn verify(&self) -> bool {
        let pk = match NovaPublicKey::from_hex(&self.validator) {
            Ok(pk) => pk,
            Err(_) => return false,
        };

        let mut message = Vec::with_capacity(40);
        message.extend_from_slice(&self.block_hash);
        message.extend_from_slice(&self.round.to_le_bytes());

        pk.verify(&message, &self.signature)
    }
}

// ---------------------------------------------------------------------------
// Finalized Block
// ---------------------------------------------------------------------------

/// A block that has achieved consensus finality.
///
/// Once a block reaches this state, it is irreversible. The votes serve as
/// a cryptographic proof of finality that light clients can verify without
/// replaying the full consensus protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalizedBlock {
    /// The block itself.
    pub block: Block,
    /// Votes that finalized this block (at least quorum threshold count).
    pub votes: Vec<Vote>,
    /// The consensus round in which finality was achieved.
    pub round: u64,
}

// ---------------------------------------------------------------------------
// Consensus Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during consensus operations.
#[derive(Debug, thiserror::Error)]
pub enum ConsensusError {
    /// Not enough validators to meet the minimum quorum.
    #[error("insufficient validators: have {have}, need {need}")]
    InsufficientValidators {
        /// Number of validators currently available.
        have: usize,
        /// Minimum validators required.
        need: usize,
    },
    /// The block proposer is not in the active validator set.
    #[error("unauthorized proposer: {0}")]
    UnauthorizedProposer(String),
    /// Block height does not match expected next height.
    #[error("unexpected block height: expected {expected}, got {got}")]
    UnexpectedHeight {
        /// The height the engine was expecting.
        expected: u64,
        /// The height the block claims.
        got: u64,
    },
    /// Block's previous_hash does not match the chain tip.
    #[error("block does not extend the chain tip")]
    InvalidParentHash,
    /// Block contains more transactions than the maximum.
    #[error("block exceeds maximum transaction count: {0}")]
    TooManyTransactions(usize),
    /// Block timestamp is invalid.
    #[error("invalid block timestamp: {0}")]
    InvalidTimestamp(u64),
    /// Not enough votes to reach finality quorum.
    #[error("insufficient votes for finality: have {have}, need {need}")]
    InsufficientVotes {
        /// Number of valid votes received.
        have: usize,
        /// Quorum threshold required.
        need: usize,
    },
    /// A vote's signature failed verification.
    #[error("invalid vote from validator {0}")]
    InvalidVote(String),
    /// A voter is not in the active validator set.
    #[error("vote from non-validator: {0}")]
    VoteFromNonValidator(String),
    /// Duplicate vote from the same validator in the same round.
    #[error("duplicate vote from {0}")]
    DuplicateVote(String),
}

// ---------------------------------------------------------------------------
// Consensus Engine
// ---------------------------------------------------------------------------

/// The consensus engine drives block production and finalization.
///
/// It manages the validator set, tracks the current round and phase,
/// and enforces the consensus rules. The engine does not own the chain —
/// it receives the current state as input and returns validated blocks
/// as output.
pub struct ConsensusEngine {
    /// Consensus parameters.
    config: ConsensusConfig,
    /// The active validator set.
    validator_set: ValidatorSet,
    /// Current consensus round number.
    current_round: u64,
    /// Current phase within the round.
    current_phase: ConsensusRound,
    /// Height of the next block to be produced.
    next_height: u64,
    /// Hash of the most recent finalized block.
    last_block_hash: [u8; 32],
}

impl ConsensusEngine {
    /// Creates a new consensus engine with the given configuration and
    /// initial validator set.
    pub fn new(config: ConsensusConfig, validator_set: ValidatorSet) -> Self {
        info!(
            validators = validator_set.len(),
            quorum = validator_set.quorum_threshold(),
            "consensus engine initialized"
        );

        Self {
            config,
            validator_set,
            current_round: 0,
            current_phase: ConsensusRound::Propose,
            next_height: 0,
            last_block_hash: [0u8; 32],
        }
    }

    /// Returns a reference to the consensus configuration.
    pub fn config(&self) -> &ConsensusConfig {
        &self.config
    }

    /// Returns a reference to the current validator set.
    pub fn validator_set(&self) -> &ValidatorSet {
        &self.validator_set
    }

    /// Returns the current consensus round number.
    pub fn current_round(&self) -> u64 {
        self.current_round
    }

    /// Returns the current consensus phase.
    pub fn current_phase(&self) -> ConsensusRound {
        self.current_phase
    }

    /// Proposes a new block from the given transactions.
    ///
    /// The proposer must be the designated validator for the current round
    /// (round-robin by stake). The block is constructed but not yet finalized —
    /// it must go through the vote collection process.
    pub fn propose_block(
        &self,
        transactions: Vec<Transaction>,
        proposer_keypair: &NovaKeypair,
    ) -> Result<Block, ConsensusError> {
        let proposer_address = proposer_keypair.public_key().to_hex();

        // Verify the proposer is authorized for this round.
        let expected_proposer = self
            .validator_set
            .proposer_for_round(self.current_round)
            .ok_or(ConsensusError::InsufficientValidators {
                have: self.validator_set.len(),
                need: self.config.min_validators,
            })?;

        if expected_proposer.address != proposer_address {
            return Err(ConsensusError::UnauthorizedProposer(proposer_address));
        }

        // Enforce transaction limit.
        if transactions.len() > self.config.max_block_transactions {
            return Err(ConsensusError::TooManyTransactions(transactions.len()));
        }

        // Compute the transactions Merkle root.
        let tx_root = Self::compute_transactions_root(&transactions);

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let mut header = BlockHeader {
            height: self.next_height,
            hash: [0u8; 32], // Computed below.
            parent_hash: self.last_block_hash,
            tx_root,
            state_root: [0u8; 32], // Filled by the state transition engine.
            timestamp,
            validator: proposer_address,
            signature: Vec::new(),
        };

        // Compute the block hash from header fields.
        let block_for_hash = Block {
            header: header.clone(),
            transactions: transactions.clone(),
        };
        header.hash = block_for_hash.compute_hash();

        // Sign the header.
        let header_bytes = serde_json::to_vec(&header).unwrap_or_default();
        let sig = proposer_keypair.sign(&header_bytes);
        header.signature = sig.as_bytes().to_vec();

        let block = Block {
            header,
            transactions,
        };

        debug!(
            height = self.next_height,
            round = self.current_round,
            "block proposed"
        );
        Ok(block)
    }

    /// Validates a block against the consensus rules.
    ///
    /// Checks height, parent hash, proposer authorization, transaction count,
    /// and proposer signature. Does not execute transactions — that is the
    /// responsibility of the state transition engine.
    pub fn validate_block(&self, block: &Block) -> Result<bool, ConsensusError> {
        if block.header.height != self.next_height {
            return Err(ConsensusError::UnexpectedHeight {
                expected: self.next_height,
                got: block.header.height,
            });
        }

        if block.header.parent_hash != self.last_block_hash {
            return Err(ConsensusError::InvalidParentHash);
        }

        if !self.validator_set.contains(&block.header.validator) {
            return Err(ConsensusError::UnauthorizedProposer(
                block.header.validator.clone(),
            ));
        }

        if block.transactions.len() > self.config.max_block_transactions {
            return Err(ConsensusError::TooManyTransactions(
                block.transactions.len(),
            ));
        }

        // Verify the proposer's signature.
        let proposer_pk = NovaPublicKey::from_hex(&block.header.validator)
            .map_err(|_| ConsensusError::UnauthorizedProposer(block.header.validator.clone()))?;

        let mut header_for_sig = block.header.clone();
        header_for_sig.signature = Vec::new();
        let header_bytes = serde_json::to_vec(&header_for_sig).unwrap_or_default();

        if block.header.signature.len() != 64 {
            return Err(ConsensusError::UnauthorizedProposer(
                block.header.validator.clone(),
            ));
        }

        let mut sig_bytes = [0u8; 64];
        sig_bytes.copy_from_slice(&block.header.signature);
        let signature = NovaSignature::from_bytes(sig_bytes);

        if !proposer_pk.verify(&header_bytes, &signature) {
            return Err(ConsensusError::UnauthorizedProposer(
                block.header.validator.clone(),
            ));
        }

        // Verify transactions root.
        let expected_root = Self::compute_transactions_root(&block.transactions);
        if block.header.tx_root != expected_root {
            warn!(height = block.header.height, "transactions root mismatch");
        }

        debug!(height = block.header.height, "block validated");
        Ok(true)
    }

    /// Finalizes a block given a set of votes from validators.
    ///
    /// The block is finalized if and only if:
    /// - All votes are for the correct block hash.
    /// - All votes have valid signatures.
    /// - All voters are in the active validator set.
    /// - The number of valid votes meets the quorum threshold (2/3 + 1).
    /// - No duplicate votes from the same validator.
    pub fn finalize_block(
        &mut self,
        block: Block,
        votes: Vec<Vote>,
    ) -> Result<FinalizedBlock, ConsensusError> {
        let block_hash = block.header.hash;
        let quorum = self.validator_set.quorum_threshold();
        let mut seen_validators: HashMap<String, bool> = HashMap::new();
        let mut valid_votes = Vec::new();

        for vote in &votes {
            if vote.block_hash != block_hash {
                continue;
            }

            if seen_validators.contains_key(&vote.validator) {
                return Err(ConsensusError::DuplicateVote(vote.validator.clone()));
            }

            if !self.validator_set.contains(&vote.validator) {
                return Err(ConsensusError::VoteFromNonValidator(vote.validator.clone()));
            }

            if !vote.verify() {
                return Err(ConsensusError::InvalidVote(vote.validator.clone()));
            }

            seen_validators.insert(vote.validator.clone(), true);
            valid_votes.push(vote.clone());
        }

        if valid_votes.len() < quorum {
            return Err(ConsensusError::InsufficientVotes {
                have: valid_votes.len(),
                need: quorum,
            });
        }

        let finalized = FinalizedBlock {
            block: block.clone(),
            votes: valid_votes,
            round: self.current_round,
        };

        self.last_block_hash = block_hash;
        self.next_height += 1;
        self.current_round += 1;
        self.current_phase = ConsensusRound::Propose;

        info!(
            height = finalized.block.header.height,
            round = finalized.round,
            votes = finalized.votes.len(),
            "block finalized"
        );

        Ok(finalized)
    }

    /// Advances to the next round (e.g., after a proposer timeout).
    pub fn advance_round(&mut self) {
        self.current_round += 1;
        self.current_phase = ConsensusRound::Propose;
        debug!(round = self.current_round, "advanced to next round");
    }

    /// Advances to the next consensus phase within the current round.
    pub fn advance_phase(&mut self) -> Option<ConsensusRound> {
        if let Some(next) = self.current_phase.next() {
            self.current_phase = next;
            Some(next)
        } else {
            None
        }
    }

    /// Updates the validator set (typically at epoch boundaries).
    pub fn update_validator_set(&mut self, new_set: ValidatorSet) {
        info!(
            old_count = self.validator_set.len(),
            new_count = new_set.len(),
            "validator set updated"
        );
        self.validator_set = new_set;
    }

    /// Sets the chain state for the engine (used during sync/initialization).
    pub fn set_chain_state(&mut self, height: u64, last_hash: [u8; 32]) {
        self.next_height = height;
        self.last_block_hash = last_hash;
    }

    /// Computes a simplified transactions root from a list of transactions.
    ///
    /// Concatenates all transaction IDs and hashes the result with BLAKE3.
    /// A proper binary Merkle tree implementation replaces this in production,
    /// but the critical property — determinism — is preserved either way.
    fn compute_transactions_root(transactions: &[Transaction]) -> [u8; 32] {
        let mut data = Vec::with_capacity(transactions.len() * 64);
        for tx in transactions {
            data.extend_from_slice(tx.id.as_bytes());
        }
        *blake3::hash(&data).as_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::keys::NovaKeypair;

    fn setup_engine() -> (ConsensusEngine, NovaKeypair) {
        let keypair = NovaKeypair::generate();
        let address = keypair.public_key().to_hex();

        let mut validator_set = ValidatorSet::new();
        validator_set.add_validator(address, 10_000_000_000);

        let config = ConsensusConfig {
            min_validators: 1,
            ..ConsensusConfig::default()
        };

        let engine = ConsensusEngine::new(config, validator_set);
        (engine, keypair)
    }

    #[test]
    fn propose_and_validate_block() {
        let (engine, keypair) = setup_engine();

        let block = engine
            .propose_block(vec![], &keypair)
            .expect("block proposal should succeed");

        assert_eq!(block.header.height, 0);
        assert!(engine.validate_block(&block).is_ok());
    }

    #[test]
    fn unauthorized_proposer_rejected() {
        let (engine, _) = setup_engine();
        let rogue = NovaKeypair::generate();

        let result = engine.propose_block(vec![], &rogue);
        assert!(result.is_err());
    }

    #[test]
    fn quorum_threshold_calculation() {
        let mut vs = ValidatorSet::new();
        for i in 0..4u32 {
            vs.add_validator(format!("validator-{}", i), 1000);
        }
        // 4 validators: (2*4/3) + 1 = 3
        assert_eq!(vs.quorum_threshold(), 3);

        for i in 4..7u32 {
            vs.add_validator(format!("validator-{}", i), 1000);
        }
        // 7 validators: (2*7/3) + 1 = 5
        assert_eq!(vs.quorum_threshold(), 5);
    }

    #[test]
    fn round_robin_proposer_selection() {
        let mut vs = ValidatorSet::new();
        vs.add_validator("high-stake".to_string(), 3000);
        vs.add_validator("mid-stake".to_string(), 2000);
        vs.add_validator("low-stake".to_string(), 1000);

        assert_eq!(vs.proposer_for_round(0).unwrap().address, "high-stake");
        assert_eq!(vs.proposer_for_round(1).unwrap().address, "mid-stake");
        assert_eq!(vs.proposer_for_round(2).unwrap().address, "low-stake");
        assert_eq!(vs.proposer_for_round(3).unwrap().address, "high-stake");
    }

    #[test]
    fn finalize_block_with_quorum() {
        let keypair = NovaKeypair::generate();
        let address = keypair.public_key().to_hex();

        let mut validator_set = ValidatorSet::new();
        validator_set.add_validator(address, 10_000_000_000);

        let config = ConsensusConfig {
            min_validators: 1,
            ..ConsensusConfig::default()
        };

        let mut engine = ConsensusEngine::new(config, validator_set);

        let block = engine
            .propose_block(vec![], &keypair)
            .expect("proposal should succeed");

        let block_hash = block.header.hash;
        let vote = Vote::new(&keypair, block_hash, 0);

        let finalized = engine
            .finalize_block(block, vec![vote])
            .expect("finalization should succeed");

        assert_eq!(finalized.round, 0);
        assert_eq!(finalized.votes.len(), 1);
        assert_eq!(engine.next_height, 1);
    }

    #[test]
    fn consensus_round_state_machine() {
        let mut round = ConsensusRound::Propose;
        round = round.next().unwrap();
        assert_eq!(round, ConsensusRound::Prevote);
        round = round.next().unwrap();
        assert_eq!(round, ConsensusRound::Precommit);
        round = round.next().unwrap();
        assert_eq!(round, ConsensusRound::Commit);
        assert!(round.next().is_none());
    }

    #[test]
    fn vote_signature_verification() {
        let keypair = NovaKeypair::generate();
        let block_hash = [42u8; 32];
        let vote = Vote::new(&keypair, block_hash, 5);
        assert!(vote.verify());
    }

    #[test]
    fn insufficient_votes_rejected() {
        let kp1 = NovaKeypair::generate();
        let kp2 = NovaKeypair::generate();
        let kp3 = NovaKeypair::generate();
        let addr1 = kp1.public_key().to_hex();
        let addr2 = kp2.public_key().to_hex();
        let addr3 = kp3.public_key().to_hex();

        let mut vs = ValidatorSet::new();
        vs.add_validator(addr1, 3000);
        vs.add_validator(addr2, 2000);
        vs.add_validator(addr3, 1000);
        // Quorum for 3 validators = (2*3/3)+1 = 3

        let config = ConsensusConfig {
            min_validators: 1,
            ..ConsensusConfig::default()
        };

        let mut engine = ConsensusEngine::new(config, vs);

        let block = engine
            .propose_block(vec![], &kp1)
            .expect("proposal should succeed");

        let block_hash = block.header.hash;
        // Only 1 vote — not enough.
        let vote = Vote::new(&kp1, block_hash, 0);

        let result = engine.finalize_block(block, vec![vote]);
        assert!(matches!(
            result,
            Err(ConsensusError::InsufficientVotes { .. })
        ));
    }
}
