//! # Perfect Forward Secrecy (PFS)
//!
//! Ephemeral key exchange for NOVA's secure communication channels.
//!
//! Perfect Forward Secrecy means that even if a long-term key is compromised,
//! past session keys cannot be recovered. We achieve this by generating fresh
//! X25519 keypairs for every session and deriving session keys via
//! Diffie-Hellman + a BLAKE3-based KDF.
//!
//! ## Why this matters for a payment protocol
//!
//! Payment metadata is sensitive — who paid whom, when, how much. Even if
//! transaction amounts are hidden via ZKPs, network-layer metadata can leak
//! information. PFS ensures that intercepted encrypted traffic can't be
//! retroactively decrypted if a node's long-term key is later compromised.
//!
//! ## Protocol Flow
//!
//! 1. Alice generates an ephemeral X25519 keypair and sends her public key.
//! 2. Bob generates his own ephemeral X25519 keypair, computes the shared
//!    secret, derives a session key, and sends his public key back.
//! 3. Alice computes the same shared secret and derives the same session key.
//! 4. Both sides now have a shared AES-256 key that only they know.
//! 5. After the session ends, ephemeral keys are discarded. Gone. Forever.
//!
//! ## Key Derivation
//!
//! The raw Diffie-Hellman output is NOT used directly as an encryption key.
//! That would be a textbook mistake — DH outputs are not uniformly random,
//! they're points on an elliptic curve with algebraic structure. Instead, we
//! run the shared secret through a BLAKE3-based KDF with a domain-separation
//! context string. This is functionally equivalent to HKDF but uses BLAKE3's
//! native `derive_key` mode, which is purpose-built for exactly this use case.

use rand::rngs::OsRng;
use thiserror::Error;
use x25519_dalek::{EphemeralSecret, PublicKey, SharedSecret};

use crate::config::AES_KEY_LENGTH;

/// Errors in the PFS key exchange protocol.
#[derive(Debug, Error)]
pub enum PfsError {
    #[error("key exchange failed: received invalid public key")]
    InvalidPublicKey,

    #[error("session key derivation failed")]
    KeyDerivationFailed,

    #[error("session already completed -- ephemeral key consumed")]
    SessionAlreadyCompleted,
}

/// An ongoing PFS session managing ephemeral X25519 keys.
///
/// This struct represents one side of an ephemeral Diffie-Hellman key exchange.
/// It generates a fresh X25519 keypair on creation and can compute a shared
/// secret when the peer's public key is received.
///
/// ## Lifecycle
///
/// A `PfsSession` is created, the public key is sent to the peer, and then
/// `derive_shared_secret()` is called with the peer's public key. After that,
/// the ephemeral secret is consumed and cannot be used again. Rust's type
/// system enforces this via `Option::take()`.
///
/// ## Important
///
/// The ephemeral secret is consumed on derivation — by design.
/// X25519's `EphemeralSecret` enforces single-use semantics at the type level.
/// You literally cannot accidentally reuse an ephemeral key. This is Rust's
/// type system doing what it does best: making wrong code fail to compile.
pub struct PfsSession {
    /// Our ephemeral secret key. `Option` because it's consumed on completion.
    /// Once it's `None`, the DH exchange is done and the key material is gone.
    secret: Option<EphemeralSecret>,
    /// The corresponding ephemeral public key. Stored separately because
    /// we need it even after the secret is consumed.
    pub public_key: PublicKey,
}

/// A completed PFS session. Contains the derived session key ready for use.
///
/// This is the "happy ending" of a key exchange. Both sides have one of these,
/// the ephemeral keys are gone, and the derived key is ready for AES-256-GCM.
#[derive(Debug)]
pub struct CompletedPfsSession {
    /// The derived 256-bit session key, suitable for AES-256-GCM.
    session_key: [u8; AES_KEY_LENGTH],
    /// Our ephemeral public key (for protocol message references).
    our_public_key: [u8; 32],
    /// The peer's ephemeral public key.
    peer_public_key: [u8; 32],
}

impl PfsSession {
    /// Create a new PFS session with a fresh ephemeral X25519 keypair.
    ///
    /// The keypair is generated using `OsRng`. The public key should be sent
    /// to the peer as part of the handshake. Don't delay — the sooner you
    /// exchange keys and derive the shared secret, the sooner the ephemeral
    /// key can be dropped.
    ///
    /// # Example
    ///
    /// ```
    /// use nova_protocol::crypto::PfsSession;
    ///
    /// let session = PfsSession::new();
    /// let public_bytes = session.public_key_bytes();
    /// assert_eq!(public_bytes.len(), 32);
    /// // Send public_bytes to the peer...
    /// ```
    pub fn new() -> Self {
        let secret = EphemeralSecret::random_from_rng(OsRng);
        let public_key = PublicKey::from(&secret);
        Self {
            secret: Some(secret),
            public_key,
        }
    }

    /// Return the ephemeral public key bytes to send to the peer.
    ///
    /// These 32 bytes are what you send over the wire. They're public —
    /// no need to encrypt them (that would be a chicken-and-egg problem).
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.public_key.to_bytes()
    }

    /// Derive the shared secret from the peer's public key.
    ///
    /// Consumes the ephemeral secret — this can only be called once.
    /// The raw DH output is fed through BLAKE3's `derive_key` mode to
    /// produce a uniform 32-byte session key.
    ///
    /// ## Why BLAKE3 and not raw DH output?
    ///
    /// Raw X25519 output is a point on Curve25519, which has algebraic
    /// structure. It's not uniformly random over {0,1}^256. Running it
    /// through a KDF extracts the entropy and produces a key that's
    /// indistinguishable from random — which is what AES-GCM needs.
    ///
    /// # Panics
    ///
    /// Panics if called twice on the same session. This is a programming
    /// error, not a runtime condition. If you're unsure whether you've
    /// already called this, use [`try_derive_shared_secret`] instead.
    pub fn derive_shared_secret(&mut self, peer_public: &[u8; 32]) -> [u8; 32] {
        let secret = self
            .secret
            .take()
            .expect("PfsSession::derive_shared_secret called twice -- this is a bug");
        let peer_pk = PublicKey::from(*peer_public);
        let raw: SharedSecret = secret.diffie_hellman(&peer_pk);

        // Use BLAKE3's derive_key mode for proper domain-separated key derivation.
        // We include both public keys in the input to bind the derived key to
        // this specific session. This prevents an attacker from replaying a
        // handshake with a different peer.
        derive_session_key(raw.as_bytes(), &self.public_key.to_bytes(), peer_public)
    }

    /// Non-panicking version of `derive_shared_secret`.
    ///
    /// Returns `Err(PfsError::SessionAlreadyCompleted)` if the secret has
    /// already been consumed.
    pub fn try_derive_shared_secret(
        &mut self,
        peer_public: &[u8; 32],
    ) -> Result<[u8; 32], PfsError> {
        let secret = self
            .secret
            .take()
            .ok_or(PfsError::SessionAlreadyCompleted)?;
        let peer_pk = PublicKey::from(*peer_public);
        let raw: SharedSecret = secret.diffie_hellman(&peer_pk);

        Ok(derive_session_key(
            raw.as_bytes(),
            &self.public_key.to_bytes(),
            peer_public,
        ))
    }

    /// Complete the key exchange and return a `CompletedPfsSession`.
    ///
    /// This is the high-level API that combines derivation with session
    /// metadata. Use this when you want a self-contained session object
    /// rather than just the raw key bytes.
    pub fn complete(
        mut self,
        peer_public_key_bytes: &[u8; 32],
    ) -> Result<CompletedPfsSession, PfsError> {
        let our_pub = self.public_key.to_bytes();
        let session_key = self.try_derive_shared_secret(peer_public_key_bytes)?;

        Ok(CompletedPfsSession {
            session_key,
            our_public_key: our_pub,
            peer_public_key: *peer_public_key_bytes,
        })
    }
}

impl Default for PfsSession {
    fn default() -> Self {
        Self::new()
    }
}

impl CompletedPfsSession {
    /// Get the derived 256-bit session key.
    ///
    /// This key is suitable for direct use with AES-256-GCM. Both sides
    /// of the exchange derive the same key (that's the magic of Diffie-Hellman).
    pub fn session_key(&self) -> &[u8; AES_KEY_LENGTH] {
        &self.session_key
    }

    /// Get our public key bytes (for protocol message construction).
    pub fn our_public_key_bytes(&self) -> [u8; 32] {
        self.our_public_key
    }

    /// Get the peer's public key bytes.
    pub fn peer_public_key_bytes(&self) -> [u8; 32] {
        self.peer_public_key
    }
}

/// Derive a session key from the DH shared secret and both public keys.
///
/// We include both public keys in the derivation to bind the key to this
/// specific session. This prevents an attacker from replaying a handshake
/// with a different peer and getting the same session key.
///
/// The construction is:
///
///   session_key = BLAKE3-derive-key(
///     context = "nova-protocol v1 pfs session key",
///     input   = shared_secret || min(pub_a, pub_b) || max(pub_a, pub_b)
///   )
///
/// The two public keys are sorted into canonical (lexicographic) order so
/// that both sides of the exchange derive the same session key regardless
/// of which is "ours" vs "peer".
///
/// BLAKE3's `derive_key` mode is specifically designed for this. It uses a
/// different internal IV derived from the context string, making it impossible
/// for outputs to collide with any other use of BLAKE3 in the protocol.
fn derive_session_key(
    shared_secret: &[u8; 32],
    our_public: &[u8; 32],
    peer_public: &[u8; 32],
) -> [u8; AES_KEY_LENGTH] {
    let mut hasher = blake3::Hasher::new_derive_key("nova-protocol v1 pfs session key");
    hasher.update(shared_secret);

    // Use canonical (sorted) ordering of the two public keys so that both
    // parties derive the same session key regardless of who is "ours" vs
    // "peer". Without this, Alice would compute KDF(secret, A, B) while
    // Bob computes KDF(secret, B, A), producing different keys.
    let (first, second) = if our_public <= peer_public {
        (our_public, peer_public)
    } else {
        (peer_public, our_public)
    };
    hasher.update(first);
    hasher.update(second);

    let mut session_key = [0u8; AES_KEY_LENGTH];
    let mut output_reader = hasher.finalize_xof();
    output_reader.fill(&mut session_key);
    session_key
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::encryption;

    /// Helper: perform a complete key exchange between two parties.
    fn do_key_exchange() -> (CompletedPfsSession, CompletedPfsSession) {
        let alice = PfsSession::new();
        let bob = PfsSession::new();

        let alice_pub = alice.public_key_bytes();
        let bob_pub = bob.public_key_bytes();

        let alice_completed = alice.complete(&bob_pub).unwrap();
        let bob_completed = bob.complete(&alice_pub).unwrap();

        (alice_completed, bob_completed)
    }

    #[test]
    fn test_key_exchange_produces_same_key() {
        let (alice, bob) = do_key_exchange();
        assert_eq!(alice.session_key(), bob.session_key());
    }

    #[test]
    fn test_different_sessions_different_keys() {
        // Two independent key exchanges should produce different session keys.
        // If they don't, something is horribly wrong with the RNG.
        let (alice1, _bob1) = do_key_exchange();
        let (alice2, _bob2) = do_key_exchange();
        assert_ne!(alice1.session_key(), alice2.session_key());
    }

    #[test]
    fn test_session_key_length() {
        let (alice, _bob) = do_key_exchange();
        assert_eq!(alice.session_key().len(), 32);
    }

    #[test]
    fn test_public_keys_stored_correctly() {
        let alice_session = PfsSession::new();
        let bob_session = PfsSession::new();

        let alice_pub = alice_session.public_key_bytes();
        let bob_pub = bob_session.public_key_bytes();

        let alice_completed = alice_session.complete(&bob_pub).unwrap();
        let bob_completed = bob_session.complete(&alice_pub).unwrap();

        assert_eq!(alice_completed.peer_public_key_bytes(), bob_pub);
        assert_eq!(alice_completed.our_public_key_bytes(), alice_pub);
        assert_eq!(bob_completed.peer_public_key_bytes(), alice_pub);
        assert_eq!(bob_completed.our_public_key_bytes(), bob_pub);
    }

    #[test]
    fn test_end_to_end_encryption() {
        // The real test: can Alice encrypt something that Bob can decrypt
        // using session keys derived from an ephemeral key exchange?
        let (alice, bob) = do_key_exchange();

        let plaintext = b"send 500 NOVA to treasury";
        let sealed = encryption::encrypt(alice.session_key(), plaintext).unwrap();
        let recovered = encryption::decrypt(bob.session_key(), &sealed).unwrap();

        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn test_derive_shared_secret_basic() {
        // Test the lower-level derive_shared_secret API
        let mut alice = PfsSession::new();
        let mut bob = PfsSession::new();

        let alice_pub = alice.public_key_bytes();
        let bob_pub = bob.public_key_bytes();

        let alice_key = alice.derive_shared_secret(&bob_pub);
        let bob_key = bob.derive_shared_secret(&alice_pub);

        // Both sides derive the same key because derive_session_key uses
        // canonical (sorted) ordering of the two public keys, ensuring
        // Alice and Bob always feed the keys in the same order to the KDF.
        assert_eq!(alice_key, bob_key);
        assert_eq!(alice_key.len(), 32);
    }

    #[test]
    fn test_try_derive_returns_error_on_reuse() {
        let mut session = PfsSession::new();
        let peer_pub = [0xAA; 32];

        // First derivation should succeed.
        assert!(session.try_derive_shared_secret(&peer_pub).is_ok());
        // Second derivation should fail — ephemeral secret is consumed.
        assert!(session.try_derive_shared_secret(&peer_pub).is_err());
    }

    #[test]
    fn test_key_derivation_is_deterministic() {
        // Same inputs should always produce the same session key.
        let shared_secret = [0xAA; 32];
        let our_public = [0xBB; 32];
        let peer_public = [0xCC; 32];

        let key1 = derive_session_key(&shared_secret, &our_public, &peer_public);
        let key2 = derive_session_key(&shared_secret, &our_public, &peer_public);
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_key_derivation_canonical_order() {
        // Swapping our_public and peer_public should produce the SAME key
        // because derive_session_key uses canonical (sorted) ordering.
        // This is essential for both sides of the exchange to derive
        // identical session keys.
        let shared_secret = [0xAA; 32];
        let key_a = [0xBB; 32];
        let key_b = [0xCC; 32];

        let derived1 = derive_session_key(&shared_secret, &key_a, &key_b);
        let derived2 = derive_session_key(&shared_secret, &key_b, &key_a);
        assert_eq!(derived1, derived2);

        // But a different shared secret must produce a different key.
        let different_secret = [0xDD; 32];
        let derived3 = derive_session_key(&different_secret, &key_a, &key_b);
        assert_ne!(derived1, derived3);
    }

    #[test]
    fn test_unique_ephemeral_keys() {
        // Every new session should have a different public key.
        // If two consecutive sessions produce the same key, the entropy
        // source is broken.
        let session1 = PfsSession::new();
        let session2 = PfsSession::new();
        assert_ne!(session1.public_key_bytes(), session2.public_key_bytes());
    }
}
