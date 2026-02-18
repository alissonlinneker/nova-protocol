//! # Transaction Module
//!
//! Construction, signing, verification, and lifecycle management for NOVA
//! protocol transactions. Every value transfer, credit operation, and token
//! action on the network is represented as a [`Transaction`].
//!
//! ## Architecture
//!
//! ```text
//! types.rs        — Core enums and value types (TransactionType, Amount, Currency)
//! builder.rs      — Fluent TransactionBuilder for constructing unsigned transactions
//! signing.rs      — Transaction signing with Ed25519 keypairs
//! verification.rs — Structural and cryptographic verification of signed transactions
//! receipt.rs      — Immutable post-confirmation receipts for audit trails
//! ```
//!
//! ## Transaction Lifecycle
//!
//! 1. **Build** — Use [`TransactionBuilder`] to assemble the transaction fields.
//! 2. **Sign** — Call [`sign_transaction`] with the sender's keypair.
//! 3. **Broadcast** — Submit the signed transaction to the mempool.
//! 4. **Verify** — Validators run [`verify_transaction`] before inclusion.
//! 5. **Receipt** — After block confirmation, a [`TransactionReceipt`] is generated.
//!
//! ## Design Decisions
//!
//! - Transaction IDs are `double_sha256` of the canonical byte representation
//!   (excluding signature and ZKP proof), matching Bitcoin's approach to
//!   prevent length-extension attacks on the hash.
//! - All amounts are `u64` in the smallest denomination. No floating point
//!   anywhere near monetary values.
//! - The `payload` and `zkp_proof` fields are optional byte vectors, keeping
//!   the base transaction lean while supporting extensibility.
//! - Timestamps are checked against a 5-minute future window to prevent
//!   clock-skew attacks without rejecting legitimate transactions.

pub mod builder;
pub mod receipt;
pub mod signing;
pub mod types;
pub mod verification;

pub use builder::{Transaction, TransactionBuilder};
pub use receipt::TransactionReceipt;
pub use signing::sign_transaction;
pub use types::{Amount, Currency, TransactionStatus, TransactionType};
pub use verification::{verify_transaction, TransactionError};
