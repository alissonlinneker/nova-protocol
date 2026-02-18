//! # Identity Module
//!
//! Sovereign identity management for the NOVA protocol. Every participant
//! on the network is identified by an Ed25519 keypair, from which we derive
//! a Bech32-encoded NOVA ID (human-readable, checksummed, hard to fat-finger).
//!
//! The identity stack is layered:
//!
//! 1. **Keypair** — Raw Ed25519 key material. Signs things, proves ownership.
//! 2. **NOVA ID** — Bech32-encoded public key with `nova` HRP. This is what
//!    users see, share, and paste into payment fields.
//! 3. **Recovery** — Shamir's Secret Sharing over GF(256) for social/custodial
//!    key recovery. Split your seed across trusted parties.
//! 4. **DID** — W3C Decentralized Identifier compatibility layer. Maps NOVA
//!    identities into the `did:nova:` method for interop with the broader
//!    SSI ecosystem.
//!
//! ## Design Decisions
//!
//! - Ed25519 was chosen for its speed, small key/signature sizes, and
//!   resistance to timing side-channels. We use the `ed25519-dalek` crate
//!   (RFC 8032 compliant).
//! - Bech32 (not Bech32m) for addresses — we're encoding raw pubkey hashes,
//!   not witness programs. The error-detection properties of Bech32 are
//!   sufficient for our use case.
//! - Shamir's implementation operates over GF(256) with irreducible polynomial
//!   x^8 + x^4 + x^3 + x + 1 (0x11B), same as AES. No external dependencies.

pub mod did;
pub mod keypair;
pub mod nova_id;
pub mod recovery;

pub use did::{DidDocument, NovaDid, VerificationMethod};
pub use keypair::{NovaKeypair, NovaPublicKey, NovaSignature};
pub use nova_id::{NovaId, NovaIdDocument};
pub use recovery::{recover_secret, split_secret, ShamirConfig, Share};
