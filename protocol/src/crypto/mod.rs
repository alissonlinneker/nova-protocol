//! # Cryptographic Primitives for NOVA
//!
//! This module is the foundation of everything security-related in the protocol.
//! Every signing operation, every hash, every encrypted payload flows through here.
//!
//! We deliberately chose boring, well-audited cryptography:
//!
//! - **Ed25519** for signatures — fast, deterministic, and nobody has broken it.
//! - **X25519** for key exchange — same curve, different clothes.
//! - **AES-256-GCM** for symmetric encryption — AEAD done right.
//! - **BLAKE3** for hashing — because we live in the future.
//! - **SHA-256** for compatibility — because the rest of the world doesn't.
//!
//! ## A note on "rolling your own crypto"
//!
//! We don't. Everything here is a thin, type-safe wrapper around audited
//! implementations. If you're tempted to optimize these functions, please
//! reconsider. Then reconsider again. Then go read about timing attacks
//! and come back when you've lost the urge.

pub mod encryption;
pub mod hash;
pub mod keys;
pub mod pfs;
pub mod signatures;

// Re-export the things people actually need so they don't have to memorize
// our module hierarchy. Life's too short for five levels of `use` statements.
pub use encryption::{decrypt, encrypt};
pub use hash::{blake3_hash, double_sha256, hash_to_field, sha256};
pub use keys::{NovaKeypair, NovaPublicKey, NovaSignature};
pub use pfs::PfsSession;
pub use signatures::{sign, verify};
