//! # Vault Module — Multi-Asset Wallet & Balance Management
//!
//! The vault is where money lives in NOVA. Every on-chain balance, every
//! credit line, every token position passes through this module. If the
//! transaction module is the nervous system, the vault is the circulatory
//! system — it moves value around and keeps the books straight.
//!
//! ## Architecture
//!
//! ```text
//! token.rs    — Token standard: identifiers, metadata, pre-defined tokens
//! balance.rs  — Per-wallet balance tracking with Pedersen commitments
//! wallet.rs   — Multi-asset wallet: deposits, withdrawals, transfers
//! credit.rs   — Credit line management: limits, draws, repayments
//! ```
//!
//! ## Design Principles
//!
//! 1. **All amounts are `u64` in smallest-unit denomination.** No floating
//!    point. No decimals in arithmetic. The `decimals` field in [`TokenInfo`]
//!    is for display only — the protocol never divides.
//!
//! 2. **Every balance carries a Pedersen commitment.** Even when the balance
//!    is public, we maintain the commitment so that a wallet can transition
//!    to private mode without re-initialization.
//!
//! 3. **Credit lines are first-class.** Not an afterthought bolted onto
//!    transfers — they have their own lifecycle, rate model, and state machine.
//!
//! 4. **Serializable state.** Every struct in this module derives `Serialize`
//!    and `Deserialize` so that wallet state can be persisted to RocksDB,
//!    transmitted over the wire, or snapshotted for recovery.

pub mod balance;
pub mod credit;
pub mod token;
pub mod wallet;

pub use balance::{Balance, BalanceError, BalanceSheet};
pub use credit::{CreditError, CreditLine, CreditLineManager, CreditLineStatus};
pub use token::{Token, TokenId, TokenInfo, TokenType};
pub use wallet::{Wallet, WalletError};
