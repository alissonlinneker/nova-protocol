//! # Storage Module
//!
//! Persistent storage for the NOVA blockchain. This module provides the
//! data structures and persistence layer that make NOVA a real chain,
//! not just a fancy calculator.
//!
//! ## Architecture
//!
//! ```text
//! block.rs  — Block structure, genesis block, hash/verify operations
//! state.rs  — Sparse Merkle Tree for account state (256-bit keyspace, BLAKE3)
//! chain.rs  — In-memory chain management with validation
//! db.rs     — sled-backed persistence with separate trees per data type
//! ```
//!
//! ## Data Flow
//!
//! ```text
//! Transaction → Block → Chain → StateTree
//!                 ↓        ↓        ↓
//!              NovaDB   NovaDB   NovaDB
//!              (blocks) (chain)  (state)
//! ```
//!
//! Every block updates the state tree. The chain enforces ordering and
//! hash-chain integrity. The DB persists everything to disk so we survive
//! restarts without resyncing from peers.
//!
//! ## Design Decisions
//!
//! 1. **BLAKE3 for everything.** Block hashes, Merkle roots, state roots —
//!    all BLAKE3. It's faster than SHA-256 on every architecture that matters,
//!    and security margins are comparable.
//!
//! 2. **Separate sled trees.** Blocks, state, transactions, and metadata
//!    each live in their own tree. sled's lock-free B+ tree gives us
//!    concurrent reads without contention, and atomic batches span trees.
//!
//! 3. **Bincode for on-disk serialization.** Compact, fast, deterministic.
//!    JSON is for APIs and debugging; bincode is for storage.

pub mod block;
pub mod chain;
pub mod db;
pub mod state;

pub use block::{Block, BlockHeader};
pub use chain::Chain;
pub use db::{DbError, DbResult, NovaDB};
pub use state::{apply_transfer, AccountState, MerkleProof, StateError, StateTree};
