//! # NOVA Transfer Protocol (NTP)
//!
//! The NTP module implements the five-step payment flow that makes NOVA
//! transfers secure, private, and verifiable. Every payment — whether it's
//! a coffee purchase or a cross-border remittance — follows this protocol.
//!
//! ## The Five Steps
//!
//! ```text
//!   ┌──────────┐                              ┌──────────┐
//!   │  Sender  │                              │ Receiver │
//!   └────┬─────┘                              └────┬─────┘
//!        │                                         │
//!        │  1. HandshakeRequest (pubkey, id, caps) │
//!        ├────────────────────────────────────────►│
//!        │                                         │
//!        │  2. HandshakeResponse (session, amount) │
//!        │◄────────────────────────────────────────┤
//!        │                                         │
//!        │  3. ProofOfFunds (ZK proof)             │
//!        ├────────────────────────────────────────►│
//!        │                                         │
//!        │  4. SignedTransaction (broadcast)        │
//!        ├──────────────► NETWORK ─────────────────┤
//!        │                                         │
//!        │  5. PaymentReceipt (dual-signed)        │
//!        │◄───────────────────────────────────────►│
//!        │                                         │
//! ```
//!
//! ### Step 1 — Handshake (`handshake.rs`)
//! Devices exchange public keys and establish a session with a shared
//! secret via X25519 Diffie-Hellman. The receiver includes payment
//! parameters (amount, currency, description) in its response.
//!
//! ### Step 2 — Proof of Funds (`proof_request.rs`)
//! The receiver issues a challenge. The sender responds with a Groth16
//! zero-knowledge proof demonstrating `balance >= amount` without
//! revealing the actual balance.
//!
//! ### Step 3 — Broadcast (`broadcast.rs`)
//! The sender constructs, signs, and broadcasts the transaction to the
//! NOVA network for inclusion in a block.
//!
//! ### Step 4 — Settlement (`settlement.rs`)
//! Validators verify the transaction (signature, ZKP, balance) and
//! include it in a block. The settlement result is propagated back.
//!
//! ### Step 5 — Receipt (`receipt.rs`)
//! Both parties sign a receipt confirming the payment. This dual-signed
//! receipt serves as non-repudiable proof of payment.
//!
//! ## Session Encryption
//!
//! All messages after the handshake are encrypted with AES-256-GCM using
//! the session's shared secret. The handshake itself uses ephemeral X25519
//! keys for perfect forward secrecy — compromising a long-term key does
//! not reveal past session traffic.

pub mod broadcast;
pub mod handshake;
pub mod proof_request;
pub mod receipt;
pub mod settlement;

mod error;

pub use broadcast::{BroadcastMessage, SignedTransaction};
pub use error::NtpError;
pub use handshake::{
    EstablishedSession, HandshakeRequest, HandshakeResponse, HandshakeSession, PaymentParams,
};
pub use proof_request::{ProofOfFundsRequest, ProofOfFundsResponse};
pub use receipt::PaymentReceipt;
pub use settlement::{SettlementResult, SettlementStateMachine, ValidationRequest};
