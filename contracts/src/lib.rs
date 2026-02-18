//! # NOVA Protocol Smart Contracts
//!
//! On-chain logic for the NOVA payment network. These contracts implement
//! the core financial primitives that make NOVA more than just another
//! transfer-only chain:
//!
//! - **Credit Escrow** — trustless lending with time-locked fund release,
//!   automatic default detection, and multi-party dispute resolution.
//! - **Dispute Resolution** — evidence-based arbitration for escrow
//!   disagreements, driven by arbiter votes and cryptographic evidence hashes.
//! - **Token Factory** — permissionless token issuance with issuer-gated
//!   minting and verifiable burn mechanics.
//!
//! ## Design Principles
//!
//! 1. All monetary operations check for overflow — we use `checked_add` and
//!    `checked_sub` everywhere, because wrapping arithmetic and money do not
//!    mix.
//! 2. State transitions are explicit: enum variants, not boolean flags.
//! 3. Signature verification gates every privileged operation.
//! 4. Every public type is serializable (serde) for wire transport and
//!    persistent storage.

pub mod credit_escrow;
pub mod dispute_resolution;
pub mod token_factory;
