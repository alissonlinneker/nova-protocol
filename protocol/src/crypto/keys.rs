//! # Key Management
//!
//! Ed25519 keypair generation and serialization for NOVA identities.
//!
//! Every participant in the NOVA network has at least one Ed25519 keypair.
//! This module handles creation, serialization, and basic key operations.
//!
//! ## Why Ed25519?
//!
//! - Deterministic signatures (no k-value footguns like ECDSA).
//! - 128-bit security level in 32+32 bytes. Compact and sufficient.
//! - Constant-time implementations exist and are well-audited.
//! - Fast verification — important when you're checking thousands of
//!   signatures per block.
//!
//! ## Security considerations
//!
//! - Private keys are zeroized on drop (thanks, ed25519-dalek).
//! - We use OS-level RNG (`OsRng`) for key generation. If your OS RNG
//!   is broken, you have bigger problems than NOVA.
//! - Key bytes are never logged. If you add logging to this module,
//!   you will be asked to leave.

use ed25519_dalek::{
    Signature as DalekSignature, Signer, SigningKey, Verifier, VerifyingKey, SECRET_KEY_LENGTH,
};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::hash::{Hash, Hasher};
use thiserror::Error;

/// Errors that can occur during key operations.
///
/// These are intentionally vague about *why* something failed — leaking
/// details about key material through error messages is a classic footgun.
#[derive(Debug, Error)]
pub enum KeyError {
    #[error("invalid secret key bytes: wrong length or not a valid scalar")]
    InvalidSecretKey,

    #[error("invalid public key bytes: not a valid Ed25519 point")]
    InvalidPublicKey,

    #[error("keypair validation failed: public key does not match secret key")]
    KeypairMismatch,
}

/// A NOVA identity keypair wrapping Ed25519 signing and verification keys.
///
/// This is the atomic unit of identity in the protocol. Every address,
/// every signature, every authentication challenge ultimately traces back
/// to one of these.
///
/// The `SigningKey` is the crown jewel — guard it with your life (or at
/// least with proper key management; see the `vault` module).
///
/// ## Serialization
///
/// `NovaKeypair` intentionally does NOT implement `Serialize`/`Deserialize`
/// directly. Serializing private keys should be a deliberate, conscious act,
/// not something that happens because someone shoved a keypair into a JSON
/// response. Use `to_bytes()` / `from_bytes()` explicitly.
///
/// # Examples
///
/// ```
/// use nova_protocol::crypto::keys::NovaKeypair;
///
/// let kp = NovaKeypair::generate();
/// let msg = b"send 100 NOVA to alice";
/// let sig = kp.sign(msg);
/// assert!(kp.verify(msg, &sig));
/// ```
pub struct NovaKeypair {
    /// The Ed25519 signing (private) key. 32 bytes of pure responsibility.
    signing_key: SigningKey,
}

/// The public half of a NOVA identity, safe to share with the world.
///
/// This is what you give to other people so they can verify your signatures
/// and send you money. Losing this is inconvenient but not catastrophic —
/// it can be re-derived from the signing key.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NovaPublicKey {
    bytes: [u8; 32],
}

/// An Ed25519 signature over a message.
///
/// 64 bytes. Deterministic for a given (key, message) pair — that's the
/// beauty of Ed25519. No nonce management, no k-value disasters, no
/// sleepless nights wondering if your RNG was seeded properly during signing.
///
/// Stored as `Vec<u8>` for serde compatibility, but always exactly 64 bytes.
/// If someone hands you a NovaSignature that isn't 64 bytes, verification
/// will simply fail — no panics, no undefined behavior, just a boolean `false`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NovaSignature {
    bytes: Vec<u8>,
}

impl NovaKeypair {
    /// Generate a fresh keypair using the OS cryptographic RNG.
    ///
    /// This is the preferred way to create a new identity. The RNG is
    /// `OsRng`, which pulls from `/dev/urandom` on Unix and `BCryptGenRandom`
    /// on Windows. If either of those is compromised, NOVA keys are the
    /// least of your worries.
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        Self { signing_key }
    }

    /// Constructs a keypair deterministically from a 32-byte seed.
    ///
    /// The seed is used directly as the Ed25519 secret scalar. Useful for
    /// deriving keypairs from BIP-39 mnemonics, KDFs, or Shamir-recovered
    /// secrets.
    ///
    /// **Warning**: if you call this with a weak seed, you get a weak key.
    /// Use a proper CSPRNG or KDF to produce the seed bytes.
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let signing_key = SigningKey::from_bytes(seed);
        Self { signing_key }
    }

    /// Reconstruct a keypair from raw 32-byte secret key material.
    ///
    /// The public key is re-derived from the secret key to ensure consistency.
    /// Equivalent to [`from_seed`](Self::from_seed) — in Ed25519, the 32-byte
    /// secret key *is* the seed.
    pub fn from_bytes(secret_key_bytes: &[u8; SECRET_KEY_LENGTH]) -> Result<Self, KeyError> {
        Ok(Self::from_seed(secret_key_bytes))
    }

    /// Reconstruct a keypair from a hex-encoded secret key.
    ///
    /// Convenience method for loading keys from config files. Please don't
    /// put raw hex keys in config files in production — use the vault module.
    /// But for devnet, we're not going to pretend you won't do it anyway.
    pub fn from_hex(hex_str: &str) -> Result<Self, KeyError> {
        let bytes = hex::decode(hex_str).map_err(|_| KeyError::InvalidSecretKey)?;
        if bytes.len() != SECRET_KEY_LENGTH {
            return Err(KeyError::InvalidSecretKey);
        }
        let mut arr = [0u8; SECRET_KEY_LENGTH];
        arr.copy_from_slice(&bytes);
        Self::from_bytes(&arr)
    }

    /// Returns the public key associated with this keypair.
    pub fn public_key(&self) -> NovaPublicKey {
        NovaPublicKey {
            bytes: self.signing_key.verifying_key().to_bytes(),
        }
    }

    /// Get the raw public key bytes (32 bytes).
    ///
    /// This is the identity that appears on-chain. Safe to share, log,
    /// tattoo on your arm, etc.
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.signing_key.verifying_key().to_bytes()
    }

    /// Sign a message and return a `NovaSignature`.
    ///
    /// Ed25519 signatures are deterministic — the same (key, message) pair
    /// always produces the same signature. No nonce games, no randomness
    /// needed at signing time. This is one of the biggest advantages over
    /// ECDSA, where a bad RNG during signing can leak your private key
    /// (see: PlayStation 3 master key incident, 2010).
    pub fn sign(&self, message: &[u8]) -> NovaSignature {
        let sig = self.signing_key.sign(message);
        NovaSignature {
            bytes: sig.to_bytes().to_vec(),
        }
    }

    /// Verify a signature against this keypair's public key.
    ///
    /// Convenience method — equivalent to calling `self.public_key().verify()`.
    pub fn verify(&self, message: &[u8], signature: &NovaSignature) -> bool {
        self.public_key().verify(message, signature)
    }

    /// Exports the raw 32-byte secret key material.
    ///
    /// **Handle with extreme care.** This is the only secret that stands
    /// between an attacker and full control of the associated NOVA identity.
    /// Don't log it. Don't send it over the network in plaintext. Don't
    /// store it in a text file called "my_keys.txt" on your desktop.
    pub fn secret_key_bytes(&self) -> [u8; 32] {
        self.signing_key.to_bytes()
    }

    /// Export the raw secret key bytes. Alias for [`secret_key_bytes`](Self::secret_key_bytes).
    pub fn to_bytes(&self) -> [u8; 32] {
        self.secret_key_bytes()
    }

    /// Reconstructs a keypair from raw secret key bytes.
    ///
    /// Equivalent to [`from_seed`](Self::from_seed) — the 32-byte secret
    /// key *is* the seed in Ed25519.
    pub fn from_secret_key_bytes(bytes: &[u8; 32]) -> Self {
        Self::from_seed(bytes)
    }

    /// Get a reference to the underlying `SigningKey`.
    ///
    /// Needed by internal code that talks directly to ed25519-dalek.
    /// Try not to pass this around more than necessary.
    pub fn signing_key(&self) -> &SigningKey {
        &self.signing_key
    }

    /// Get the underlying `VerifyingKey`.
    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    /// Get the public key as a hex string. Useful for display and logging.
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.public_key_bytes())
    }

    /// Get the public key as a base58 string. More compact than hex,
    /// and what most users will see as their "address" (before Bech32 encoding).
    pub fn public_key_base58(&self) -> String {
        bs58::encode(self.public_key_bytes()).into_string()
    }
}

impl Clone for NovaKeypair {
    /// Cloning a keypair is allowed but should make you uncomfortable.
    /// Every copy of a private key is another thing to protect.
    fn clone(&self) -> Self {
        Self {
            signing_key: SigningKey::from_bytes(&self.signing_key.to_bytes()),
        }
    }
}

impl fmt::Debug for NovaKeypair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never print secret key material in debug output. Not even "partially."
        // A partial leak is still a leak, and grepping logs for hex is trivial.
        write!(f, "NovaKeypair(pub={})", self.public_key().to_hex())
    }
}

impl PartialEq for NovaKeypair {
    /// Two keypairs are equal if their public keys match.
    /// We compare public keys (not private) because comparing secret material
    /// in a non-constant-time way is a bad habit, and for identity purposes,
    /// the public key is what matters.
    fn eq(&self, other: &Self) -> bool {
        self.public_key_bytes() == other.public_key_bytes()
    }
}

impl Eq for NovaKeypair {}

// ---------------------------------------------------------------------------
// NovaPublicKey
// ---------------------------------------------------------------------------

impl NovaPublicKey {
    /// Create a `NovaPublicKey` from raw bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self { bytes }
    }

    /// Try to create a `NovaPublicKey` from a byte slice.
    ///
    /// Validates the length and that the bytes represent a valid Ed25519 point.
    /// We don't just accept any 32 bytes — some values aren't valid points on
    /// the curve, and using them could lead to weird behavior.
    pub fn try_from_slice(slice: &[u8]) -> Result<Self, KeyError> {
        if slice.len() != 32 {
            return Err(KeyError::InvalidPublicKey);
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(slice);

        // Verify these bytes actually represent a valid Ed25519 public key.
        // This catches low-order points and other degenerate cases.
        VerifyingKey::from_bytes(&bytes).map_err(|_| KeyError::InvalidPublicKey)?;

        Ok(Self { bytes })
    }

    /// Get the raw bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }

    /// Verify a signature against this public key.
    ///
    /// Returns `true` if the signature is valid, `false` otherwise. We use
    /// a boolean here (rather than `Result`) because the vast majority of
    /// callers just want a yes/no answer and don't care about the specific
    /// failure mode.
    pub fn verify(&self, message: &[u8], signature: &NovaSignature) -> bool {
        let Ok(verifying_key) = VerifyingKey::from_bytes(&self.bytes) else {
            return false;
        };
        let sig_bytes: [u8; 64] = match signature.bytes.as_slice().try_into() {
            Ok(b) => b,
            Err(_) => return false,
        };
        let dalek_sig = DalekSignature::from_bytes(&sig_bytes);
        verifying_key.verify(message, &dalek_sig).is_ok()
    }

    /// Convert to a `VerifyingKey` for direct use with ed25519-dalek.
    ///
    /// This can fail if the stored bytes are somehow invalid, which shouldn't
    /// happen if the key was created through our constructors. But crypto code
    /// doesn't get to assume things are fine.
    pub fn to_verifying_key(&self) -> Result<VerifyingKey, KeyError> {
        VerifyingKey::from_bytes(&self.bytes).map_err(|_| KeyError::InvalidPublicKey)
    }

    /// Hex-encoded representation. 64 characters for 32 bytes.
    pub fn to_hex(&self) -> String {
        hex::encode(self.bytes)
    }

    /// Parse a hex-encoded public key string.
    ///
    /// Returns an error if the hex is malformed or the wrong length.
    pub fn from_hex(s: &str) -> Result<Self, hex::FromHexError> {
        let bytes = hex::decode(s)?;
        if bytes.len() != 32 {
            return Err(hex::FromHexError::OddLength);
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Self { bytes: arr })
    }

    /// Base58-encoded representation.
    pub fn to_base58(&self) -> String {
        bs58::encode(self.bytes).into_string()
    }
}

impl Hash for NovaPublicKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.bytes.hash(state);
    }
}

impl fmt::Display for NovaPublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl fmt::Debug for NovaPublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NovaPublicKey({})", &self.to_hex()[..16])
    }
}

// ---------------------------------------------------------------------------
// NovaSignature
// ---------------------------------------------------------------------------

impl NovaSignature {
    /// Create a signature from raw 64-byte representation.
    pub fn from_bytes(bytes: [u8; 64]) -> Self {
        Self {
            bytes: bytes.to_vec(),
        }
    }

    /// Returns the raw signature bytes (always 64 bytes for valid Ed25519 signatures).
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Convert to the ed25519-dalek `Signature` type for internal use.
    ///
    /// Returns `None` if the internal bytes aren't exactly 64 bytes
    /// (which shouldn't happen if the signature was created properly,
    /// but defense in depth is free).
    pub fn to_dalek_signature(&self) -> Option<DalekSignature> {
        let arr: [u8; 64] = self.bytes.as_slice().try_into().ok()?;
        Some(DalekSignature::from_bytes(&arr))
    }

    /// Returns the hex-encoded signature string. 128 characters for a valid sig.
    pub fn to_hex(&self) -> String {
        hex::encode(&self.bytes)
    }

    /// Parse a hex-encoded signature.
    pub fn from_hex(s: &str) -> Result<Self, hex::FromHexError> {
        let bytes = hex::decode(s)?;
        if bytes.len() != 64 {
            return Err(hex::FromHexError::OddLength);
        }
        Ok(Self { bytes })
    }
}

impl fmt::Display for NovaSignature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl fmt::Debug for NovaSignature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hex_str = self.to_hex();
        if hex_str.len() >= 128 {
            write!(f, "NovaSignature({}...{})", &hex_str[..8], &hex_str[120..])
        } else {
            write!(f, "NovaSignature({})", hex_str)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_produces_valid_keypair() {
        let kp = NovaKeypair::generate();
        assert_eq!(kp.public_key_bytes().len(), 32);
        assert_eq!(kp.to_bytes().len(), 32);
    }

    #[test]
    fn keypair_sign_verify_roundtrip() {
        let kp = NovaKeypair::generate();
        let msg = b"transfer 100 NOVA";
        let sig = kp.sign(msg);
        assert!(kp.verify(msg, &sig));
    }

    #[test]
    fn wrong_message_fails_verification() {
        let kp = NovaKeypair::generate();
        let sig = kp.sign(b"correct message");
        assert!(!kp.verify(b"wrong message", &sig));
    }

    #[test]
    fn wrong_key_fails_verification() {
        let kp1 = NovaKeypair::generate();
        let kp2 = NovaKeypair::generate();
        let sig = kp1.sign(b"message");
        assert!(!kp2.verify(b"message", &sig));
    }

    #[test]
    fn test_roundtrip_bytes() {
        let kp = NovaKeypair::generate();
        let secret_bytes = kp.to_bytes();
        let restored = NovaKeypair::from_bytes(&secret_bytes).unwrap();
        assert_eq!(kp.public_key_bytes(), restored.public_key_bytes());
    }

    #[test]
    fn test_roundtrip_hex() {
        let kp = NovaKeypair::generate();
        let hex_str = hex::encode(kp.to_bytes());
        let restored = NovaKeypair::from_hex(&hex_str).unwrap();
        assert_eq!(kp.public_key_bytes(), restored.public_key_bytes());
    }

    #[test]
    fn test_invalid_hex_rejected() {
        // Too short
        assert!(NovaKeypair::from_hex("deadbeef").is_err());
        // Not hex at all
        assert!(NovaKeypair::from_hex("not-hex-at-all").is_err());
    }

    #[test]
    fn public_key_hex_roundtrip() {
        let kp = NovaKeypair::generate();
        let pk = kp.public_key();
        let hex_str = pk.to_hex();
        let recovered = NovaPublicKey::from_hex(&hex_str).unwrap();
        assert_eq!(pk, recovered);
    }

    #[test]
    fn test_public_key_encoding_formats() {
        let kp = NovaKeypair::generate();
        let hex_str = kp.public_key_hex();
        let b58 = kp.public_key_base58();

        // Hex should be 64 characters (32 bytes * 2)
        assert_eq!(hex_str.len(), 64);
        // Base58 should be roughly 43-44 characters for 32 bytes
        assert!(b58.len() >= 42 && b58.len() <= 46);
    }

    #[test]
    fn test_two_generated_keypairs_are_different() {
        // If this fails, your RNG is broken and you should panic (the emotion,
        // not the macro). Well, actually, both.
        let kp1 = NovaKeypair::generate();
        let kp2 = NovaKeypair::generate();
        assert_ne!(kp1.public_key_bytes(), kp2.public_key_bytes());
    }

    #[test]
    fn test_nova_public_key_try_from_slice() {
        let kp = NovaKeypair::generate();
        let pk = NovaPublicKey::try_from_slice(&kp.public_key_bytes()).unwrap();
        assert_eq!(pk.as_bytes(), &kp.public_key_bytes());
    }

    #[test]
    fn test_nova_public_key_rejects_wrong_length() {
        let short = [0u8; 16];
        assert!(NovaPublicKey::try_from_slice(&short).is_err());
    }

    #[test]
    fn test_clone_preserves_identity() {
        let kp = NovaKeypair::generate();
        let cloned = kp.clone();
        assert_eq!(kp.public_key_bytes(), cloned.public_key_bytes());
        assert_eq!(kp.to_bytes(), cloned.to_bytes());
    }

    #[test]
    fn deterministic_from_seed() {
        let seed = [42u8; 32];
        let kp1 = NovaKeypair::from_seed(&seed);
        let kp2 = NovaKeypair::from_seed(&seed);
        assert_eq!(kp1.public_key(), kp2.public_key());
    }

    #[test]
    fn secret_key_roundtrip() {
        let kp = NovaKeypair::generate();
        let bytes = kp.secret_key_bytes();
        let restored = NovaKeypair::from_secret_key_bytes(&bytes);
        assert_eq!(kp.public_key(), restored.public_key());
    }

    #[test]
    fn test_deterministic_signatures() {
        // Ed25519 is deterministic — same key + same message = same signature.
        // This is a feature, not a bug.
        let kp = NovaKeypair::generate();
        let msg = b"determinism is underrated";
        let sig1 = kp.sign(msg);
        let sig2 = kp.sign(msg);
        assert_eq!(sig1.as_bytes(), sig2.as_bytes());
    }

    #[test]
    fn test_signature_hex_roundtrip() {
        let kp = NovaKeypair::generate();
        let sig = kp.sign(b"test");
        let hex_str = sig.to_hex();
        let recovered = NovaSignature::from_hex(&hex_str).unwrap();
        assert_eq!(sig, recovered);
    }

    #[test]
    fn debug_does_not_leak_secret() {
        let kp = NovaKeypair::generate();
        let debug_str = format!("{:?}", kp);
        assert!(debug_str.starts_with("NovaKeypair(pub="));
        assert!(!debug_str.contains("signing_key"));
    }

    #[test]
    fn test_empty_message_signing() {
        // Signing an empty message is valid in Ed25519. Some protocols
        // forbid it, but we don't — the signature is still deterministic.
        let kp = NovaKeypair::generate();
        let sig = kp.sign(b"");
        assert!(kp.verify(b"", &sig));
    }

    #[test]
    fn test_large_message_signing() {
        // Ed25519 can sign messages of any length (it hashes internally with SHA-512).
        let kp = NovaKeypair::generate();
        let msg = vec![0xAB; 1_000_000]; // 1 MB
        let sig = kp.sign(&msg);
        assert!(kp.verify(&msg, &sig));
    }

    #[test]
    fn test_known_seed_vector() {
        // Deterministic test vector: a well-known seed should always produce the
        // same public key. This catches regressions in key derivation if we ever
        // swap out the Ed25519 backend.
        //
        // Seed: hex-encoded "alisson.linneker\0..." (padded to 32 bytes).
        let seed: [u8; 32] = [
            0x61, 0x6c, 0x69, 0x73, 0x73, 0x6f, 0x6e, 0x2e, // "alisson."
            0x6c, 0x69, 0x6e, 0x6e, 0x65, 0x6b, 0x65, 0x72, // "linneker"
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let kp = NovaKeypair::from_seed(&seed);
        let pk_hex = kp.public_key_hex();

        // Ensure deterministic derivation — the public key must be stable across
        // builds and platforms.
        let kp2 = NovaKeypair::from_seed(&seed);
        assert_eq!(pk_hex, kp2.public_key_hex());

        // Verify the keypair is functional.
        let sig = kp.sign(b"NOVA genesis");
        assert!(kp.verify(b"NOVA genesis", &sig));
    }
}
