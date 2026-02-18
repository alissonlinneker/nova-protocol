//! # NOVA ID — Sovereign Identity Addresses
//!
//! A NOVA ID is the human-facing representation of a participant's identity
//! on the network. It is derived from the participant's Ed25519 public key
//! via BLAKE3 hashing and Bech32 encoding:
//!
//! ```text
//! public_key (32 bytes)
//!     -> BLAKE3(public_key) -> 32 bytes
//!     -> Bech32("nova", hash) -> nova1qw508d6qe...
//! ```
//!
//! The `nova` human-readable prefix (HRP) makes addresses immediately
//! recognizable. Bech32 encoding provides built-in error detection — it
//! can detect up to 4 character errors — which matters when users are
//! copy-pasting addresses into payment forms.
//!
//! ## Why BLAKE3 instead of raw public key?
//!
//! - Provides a layer of indirection (quantum resistance hedge).
//! - Consistent 32-byte output regardless of future key scheme changes.
//! - BLAKE3 is faster than SHA-256 and produces higher-quality digests.

use crate::crypto::keys::{NovaPublicKey, NovaSignature};
use bech32::{Bech32, Hrp};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;
use uuid::Uuid;

/// The human-readable prefix for all NOVA addresses.
const NOVA_HRP: &str = "nova";

/// Current identity document schema version.
const DOCUMENT_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during NOVA ID operations.
#[derive(Debug, Error)]
pub enum NovaIdError {
    /// The Bech32 string could not be decoded.
    #[error("bech32 decode error: {0}")]
    Bech32Decode(String),

    /// The decoded address has an unexpected human-readable prefix.
    #[error("invalid HRP: expected '{expected}', got '{got}'")]
    InvalidHrp {
        /// The expected HRP.
        expected: String,
        /// The HRP that was actually found.
        got: String,
    },

    /// The decoded data has an unexpected length.
    #[error("invalid address data length: expected {expected} bytes, got {got}")]
    InvalidDataLength {
        /// Expected number of bytes.
        expected: usize,
        /// Actual number of bytes.
        got: usize,
    },

    /// Signature verification failed during identity assertion.
    #[error("signature verification failed")]
    SignatureVerificationFailed,

    /// The operation requires an attached public key but none is present.
    #[error("no public key attached to this NovaId (address-only mode)")]
    NoPublicKey,

    /// The provided public key does not match the address hash.
    #[error("public key hash does not match the stored address hash")]
    PublicKeyMismatch,
}

// ---------------------------------------------------------------------------
// NovaId
// ---------------------------------------------------------------------------

/// A NOVA identity — the primary address format used across the protocol.
///
/// Internally stores the BLAKE3 hash of the originating public key (32 bytes)
/// and optionally the public key itself for signature verification. The Bech32
/// address is computed on-the-fly from the hash.
///
/// # Examples
///
/// ```
/// use nova_protocol::identity::{NovaKeypair, NovaId};
///
/// let kp = NovaKeypair::generate();
/// let id = NovaId::from_public_key(&kp.public_key());
/// let address = id.to_address();
/// assert!(address.starts_with("nova1"));
///
/// let recovered = NovaId::from_address(&address).unwrap();
/// assert_eq!(id, recovered);
/// ```
#[derive(Clone, Eq)]
pub struct NovaId {
    /// BLAKE3 hash of the public key (32 bytes). This is what gets
    /// Bech32-encoded into the address string.
    key_hash: [u8; 32],

    /// The original public key, retained for signature verification
    /// without requiring a separate lookup. `None` when the ID was
    /// parsed from an address string.
    public_key: Option<NovaPublicKey>,
}

impl NovaId {
    /// Create a NOVA ID from a public key.
    ///
    /// Hashes the public key bytes with BLAKE3 and stores both the
    /// hash (for address derivation) and the key (for verification).
    pub fn from_public_key(pk: &NovaPublicKey) -> Self {
        let key_hash = blake3::hash(pk.as_bytes());
        Self {
            key_hash: *key_hash.as_bytes(),
            public_key: Some(pk.clone()),
        }
    }

    /// Encode this identity as a Bech32 address string.
    ///
    /// The output has the form `nova1<bech32-encoded-hash>` and includes
    /// a checksum for error detection.
    pub fn to_address(&self) -> String {
        let hrp = Hrp::parse(NOVA_HRP).expect("static HRP is valid");
        bech32::encode::<Bech32>(hrp, &self.key_hash)
            .expect("encoding a 32-byte payload should never fail")
    }

    /// Parse a Bech32-encoded NOVA address back into a [`NovaId`].
    ///
    /// Validates the HRP, checksum, and data length. Note that the
    /// resulting `NovaId` will **not** have a public key attached —
    /// only the hash is recoverable from the address. Signature
    /// verification requires calling [`attach_public_key`](Self::attach_public_key).
    pub fn from_address(addr: &str) -> Result<Self, NovaIdError> {
        let (hrp, data) =
            bech32::decode(addr).map_err(|e| NovaIdError::Bech32Decode(e.to_string()))?;

        let expected_hrp = Hrp::parse(NOVA_HRP).expect("static HRP is valid");
        if hrp != expected_hrp {
            return Err(NovaIdError::InvalidHrp {
                expected: NOVA_HRP.to_string(),
                got: hrp.to_string(),
            });
        }

        if data.len() != 32 {
            return Err(NovaIdError::InvalidDataLength {
                expected: 32,
                got: data.len(),
            });
        }

        let mut key_hash = [0u8; 32];
        key_hash.copy_from_slice(&data);

        Ok(Self {
            key_hash,
            public_key: None,
        })
    }

    /// Verify a signature against this identity.
    ///
    /// Requires that this `NovaId` was created via [`from_public_key`](Self::from_public_key)
    /// or has had a key attached via [`attach_public_key`](Self::attach_public_key).
    ///
    /// Returns `Ok(())` on success, or an appropriate error if the key
    /// is missing or the signature is invalid.
    pub fn verify_signature(
        &self,
        message: &[u8],
        signature: &NovaSignature,
    ) -> Result<(), NovaIdError> {
        let pk = self.public_key.as_ref().ok_or(NovaIdError::NoPublicKey)?;
        if pk.verify(message, signature) {
            Ok(())
        } else {
            Err(NovaIdError::SignatureVerificationFailed)
        }
    }

    /// Attach a public key to a NovaId recovered from an address.
    ///
    /// Validates that the key's BLAKE3 hash matches the stored hash.
    /// This is required before calling [`verify_signature`](Self::verify_signature)
    /// on an address-derived ID.
    pub fn attach_public_key(&mut self, pk: &NovaPublicKey) -> Result<(), NovaIdError> {
        let expected_hash = blake3::hash(pk.as_bytes());
        if expected_hash.as_bytes() != &self.key_hash {
            return Err(NovaIdError::PublicKeyMismatch);
        }
        self.public_key = Some(pk.clone());
        Ok(())
    }

    /// Return the raw 32-byte BLAKE3 hash underlying this address.
    pub fn key_hash(&self) -> &[u8; 32] {
        &self.key_hash
    }

    /// Return the attached public key, if any.
    pub fn public_key(&self) -> Option<&NovaPublicKey> {
        self.public_key.as_ref()
    }
}

impl PartialEq for NovaId {
    fn eq(&self, other: &Self) -> bool {
        // Two NovaIds are equal if they represent the same address, regardless
        // of whether a public key is attached. The key_hash is the canonical
        // identity; the optional public_key is auxiliary metadata that may or
        // may not be present depending on how the NovaId was constructed
        // (from_public_key retains it, from_address does not).
        self.key_hash == other.key_hash
    }
}

impl std::hash::Hash for NovaId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Must be consistent with PartialEq: only hash the key_hash field.
        self.key_hash.hash(state);
    }
}

impl fmt::Display for NovaId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_address())
    }
}

impl fmt::Debug for NovaId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NovaId({})", self.to_address())
    }
}

impl Serialize for NovaId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serializer.serialize_str(&self.to_address())
        } else {
            serializer.serialize_bytes(&self.key_hash)
        }
    }
}

impl<'de> Deserialize<'de> for NovaId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let s = String::deserialize(deserializer)?;
            NovaId::from_address(&s).map_err(serde::de::Error::custom)
        } else {
            let bytes = <Vec<u8>>::deserialize(deserializer)?;
            if bytes.len() != 32 {
                return Err(serde::de::Error::custom(format!(
                    "expected 32-byte key hash, got {}",
                    bytes.len()
                )));
            }
            let mut key_hash = [0u8; 32];
            key_hash.copy_from_slice(&bytes);
            Ok(NovaId {
                key_hash,
                public_key: None,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// NovaIdDocument
// ---------------------------------------------------------------------------

/// A full identity document containing metadata about a NOVA identity.
///
/// This is the authoritative record of a participant's identity on the
/// network. It binds a NOVA ID to its public key, creation timestamp,
/// and a unique document identifier. Documents are versioned to support
/// future schema evolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NovaIdDocument {
    /// Unique document identifier (UUID v4).
    pub id: Uuid,

    /// The NOVA ID this document describes.
    pub nova_id: NovaId,

    /// The public key backing this identity.
    pub public_key: NovaPublicKey,

    /// When this identity document was created (UTC).
    pub created_at: DateTime<Utc>,

    /// When this document was last updated (UTC).
    pub updated_at: DateTime<Utc>,

    /// Document schema version for forward compatibility.
    pub version: u32,

    /// Optional human-readable label for the identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    /// Optional metadata key-value pairs.
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub metadata: std::collections::HashMap<String, String>,
}

impl NovaIdDocument {
    /// Create a new identity document from a public key.
    ///
    /// Sets `created_at` and `updated_at` to the current UTC time,
    /// generates a fresh UUID, and uses the current schema version.
    pub fn new(public_key: &NovaPublicKey) -> Self {
        let nova_id = NovaId::from_public_key(public_key);
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            nova_id,
            public_key: public_key.clone(),
            created_at: now,
            updated_at: now,
            version: DOCUMENT_VERSION,
            label: None,
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Create a new identity document with a label.
    pub fn with_label(public_key: &NovaPublicKey, label: impl Into<String>) -> Self {
        let mut doc = Self::new(public_key);
        doc.label = Some(label.into());
        doc
    }

    /// Return the Bech32-encoded address for this identity.
    pub fn address(&self) -> String {
        self.nova_id.to_address()
    }

    /// Verify a signature against this document's identity.
    pub fn verify_signature(
        &self,
        message: &[u8],
        signature: &NovaSignature,
    ) -> Result<(), NovaIdError> {
        self.nova_id.verify_signature(message, signature)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::NovaKeypair;

    #[test]
    fn address_starts_with_nova1() {
        let kp = NovaKeypair::generate();
        let id = NovaId::from_public_key(&kp.public_key());
        let addr = id.to_address();
        assert!(addr.starts_with("nova1"), "address was: {}", addr);
    }

    #[test]
    fn address_roundtrip() {
        let kp = NovaKeypair::generate();
        let id = NovaId::from_public_key(&kp.public_key());
        let addr = id.to_address();
        let recovered = NovaId::from_address(&addr).unwrap();
        assert_eq!(id.key_hash(), recovered.key_hash());
    }

    #[test]
    fn different_keys_different_addresses() {
        let kp1 = NovaKeypair::generate();
        let kp2 = NovaKeypair::generate();
        let addr1 = NovaId::from_public_key(&kp1.public_key()).to_address();
        let addr2 = NovaId::from_public_key(&kp2.public_key()).to_address();
        assert_ne!(addr1, addr2);
    }

    #[test]
    fn deterministic_address_from_same_key() {
        let seed = [7u8; 32];
        let kp = NovaKeypair::from_seed(&seed);
        let addr1 = NovaId::from_public_key(&kp.public_key()).to_address();
        let addr2 = NovaId::from_public_key(&kp.public_key()).to_address();
        assert_eq!(addr1, addr2);
    }

    #[test]
    fn invalid_hrp_rejected() {
        let hrp = Hrp::parse("btc").unwrap();
        let data = [0u8; 32];
        let encoded = bech32::encode::<Bech32>(hrp, &data).unwrap();
        let err = NovaId::from_address(&encoded).unwrap_err();
        assert!(matches!(err, NovaIdError::InvalidHrp { .. }));
    }

    #[test]
    fn corrupted_address_rejected() {
        let kp = NovaKeypair::generate();
        let mut addr = NovaId::from_public_key(&kp.public_key()).to_address();
        // Corrupt a character in the middle of the data part.
        let mid = addr.len() / 2;
        let original = addr.as_bytes()[mid];
        let replacement = if original == b'q' { b'p' } else { b'q' };
        unsafe {
            addr.as_bytes_mut()[mid] = replacement;
        }
        assert!(NovaId::from_address(&addr).is_err());
    }

    #[test]
    fn verify_signature_via_nova_id() {
        let kp = NovaKeypair::generate();
        let id = NovaId::from_public_key(&kp.public_key());
        let msg = b"transfer 50 NOVA";
        let sig = kp.sign(msg);
        assert!(id.verify_signature(msg, &sig).is_ok());
    }

    #[test]
    fn verify_fails_without_public_key() {
        let kp = NovaKeypair::generate();
        let id = NovaId::from_public_key(&kp.public_key());
        let addr = id.to_address();
        let recovered = NovaId::from_address(&addr).unwrap();
        let sig = kp.sign(b"msg");
        assert!(matches!(
            recovered.verify_signature(b"msg", &sig),
            Err(NovaIdError::NoPublicKey)
        ));
    }

    #[test]
    fn attach_public_key_and_verify() {
        let kp = NovaKeypair::generate();
        let id = NovaId::from_public_key(&kp.public_key());
        let addr = id.to_address();
        let mut recovered = NovaId::from_address(&addr).unwrap();
        recovered.attach_public_key(&kp.public_key()).unwrap();

        let msg = b"authenticated message";
        let sig = kp.sign(msg);
        assert!(recovered.verify_signature(msg, &sig).is_ok());
    }

    #[test]
    fn attach_wrong_public_key_rejected() {
        let kp1 = NovaKeypair::generate();
        let kp2 = NovaKeypair::generate();
        let id = NovaId::from_public_key(&kp1.public_key());
        let addr = id.to_address();
        let mut recovered = NovaId::from_address(&addr).unwrap();
        assert!(matches!(
            recovered.attach_public_key(&kp2.public_key()),
            Err(NovaIdError::PublicKeyMismatch)
        ));
    }

    #[test]
    fn nova_id_serde_json_roundtrip() {
        let kp = NovaKeypair::generate();
        let id = NovaId::from_public_key(&kp.public_key());
        let json = serde_json::to_string(&id).unwrap();
        let recovered: NovaId = serde_json::from_str(&json).unwrap();
        assert_eq!(id.key_hash(), recovered.key_hash());
    }

    #[test]
    fn identity_document_creation() {
        let kp = NovaKeypair::generate();
        let doc = NovaIdDocument::new(&kp.public_key());
        assert_eq!(doc.version, DOCUMENT_VERSION);
        assert!(doc.label.is_none());
        assert!(doc.address().starts_with("nova1"));
    }

    #[test]
    fn identity_document_with_label() {
        let kp = NovaKeypair::generate();
        let doc = NovaIdDocument::with_label(&kp.public_key(), "alice-primary");
        assert_eq!(doc.label.as_deref(), Some("alice-primary"));
    }

    #[test]
    fn identity_document_serde_roundtrip() {
        let kp = NovaKeypair::generate();
        let doc = NovaIdDocument::with_label(&kp.public_key(), "test");
        let json = serde_json::to_string_pretty(&doc).unwrap();
        let recovered: NovaIdDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.address(), doc.address());
        assert_eq!(recovered.label, doc.label);
    }

    #[test]
    fn identity_document_verify_signature() {
        let kp = NovaKeypair::generate();
        let doc = NovaIdDocument::new(&kp.public_key());
        let msg = b"document assertion";
        let sig = kp.sign(msg);
        assert!(doc.verify_signature(msg, &sig).is_ok());
    }
}
