// Copyright (c) 2026 ALAS Technology. MIT License.
// See LICENSE for details.

//! # NOVA Protocol — Core Library
//!
//! This is the beating heart of NOVA: an open payment protocol designed for
//! the world that actually exists, not the one crypto Twitter fantasizes about.
//!
//! NOVA takes a pragmatic stance: Ed25519 for signatures (because we're not
//! barbarians), BN254 for zero-knowledge proofs (because Groth16 is still
//! the most battle-tested SNARK), and AES-256-GCM for symmetric encryption
//! (because NIST got that one right).
//!
//! ## Architecture
//!
//! The protocol is split into modules that mirror the actual concerns of a
//! payment network:
//!
//! - **crypto** — Low-level cryptographic primitives. Don't roll your own.
//! - **identity** — DID-based identity management. Your keys, your money.
//! - **transaction** — Transaction construction, validation, and lifecycle.
//! - **zkp** — Zero-knowledge proof circuits for private transactions.
//! - **vault** — Encrypted secret storage. Because plaintext keys are a felony.
//! - **network** — P2P networking via libp2p. Gossip, but make it useful.
//! - **ntp** — NOVA Transfer Protocol for cross-network settlement.
//! - **credit** — Credit scoring and reputation (the spicy part).
//! - **storage** — Persistent storage abstraction over RocksDB.
//! - **config** — Protocol constants and network parameters.
//!
//! ## Design Philosophy
//!
//! 1. Correctness over performance (but we're still fast).
//! 2. No unsafe code in crypto paths — we sleep at night.
//! 3. Every public API is documented. Internal shame is documented too.
//! 4. If it touches money, it has tests. Plural.

pub mod config;
pub mod credit;
pub mod crypto;
pub mod identity;
pub mod network;
pub mod ntp;
pub mod storage;
pub mod transaction;
pub mod vault;
pub mod zkp;
