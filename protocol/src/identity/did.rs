//! # DID (Decentralized Identifier) Compatibility
//!
//! Maps NOVA identities to the W3C DID specification (DID Core v1.0),
//! enabling interoperability with the broader Self-Sovereign Identity
//! ecosystem. The `did:nova:` method encodes the Bech32 NOVA address
//! directly into the DID string.
//!
//! ## DID Format
//!
//! ```text
//! did:nova:<bech32-address>
//! ```
//!
//! Example: `did:nova:nova1qw508d6qejxtdg4y5r3zarvary0c5xw7k...`
//!
//! ## DID Document
//!
//! The generated DID Document follows the W3C DID Core specification and
//! includes:
//!
//! - `id` — The DID string
//! - `verificationMethod` — Ed25519 public key in multibase format
//! - `authentication` — References the verification method
//! - `assertionMethod` — References the verification method
//!
//! ## Standards References
//!
//! - [DID Core v1.0](https://www.w3.org/TR/did-core/)
//! - [DID Specification Registries](https://www.w3.org/TR/did-spec-registries/)
//! - [Ed25519VerificationKey2020](https://w3c-ccg.github.io/di-eddsa-2020/)

use crate::crypto::keys::NovaPublicKey;
use crate::identity::nova_id::NovaId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// DID method name for NOVA.
const DID_METHOD: &str = "nova";

/// Context URI for the W3C DID Core specification.
const DID_CONTEXT: &str = "https://www.w3.org/ns/did/v1";

/// Context URI for the Ed25519 verification key suite.
const ED25519_CONTEXT: &str = "https://w3id.org/security/suites/ed25519-2020/v1";

/// Verification method type for Ed25519 public keys.
const VERIFICATION_KEY_TYPE: &str = "Ed25519VerificationKey2020";

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during DID operations.
#[derive(Debug, Error)]
pub enum DidError {
    /// The DID string does not match the expected format.
    #[error("invalid DID format: {0}")]
    InvalidFormat(String),

    /// The DID method is not "nova".
    #[error("unsupported DID method: expected 'nova', got '{0}'")]
    UnsupportedMethod(String),

    /// The method-specific identifier could not be parsed as a NOVA address.
    #[error("invalid NOVA address in DID: {0}")]
    InvalidAddress(String),

    /// Serialization error during document generation.
    #[error("serialization error: {0}")]
    Serialization(String),
}

// ---------------------------------------------------------------------------
// NovaDid
// ---------------------------------------------------------------------------

/// A Decentralized Identifier for a NOVA identity.
///
/// Maps a NOVA ID to the `did:nova:<bech32-address>` format defined by
/// the W3C DID Core specification. Stores both the NOVA ID and the
/// associated public key (needed for DID Document generation).
///
/// # Examples
///
/// ```
/// use nova_protocol::identity::{NovaKeypair, NovaId};
/// use nova_protocol::identity::did::NovaDid;
///
/// let kp = NovaKeypair::generate();
/// let nova_id = NovaId::from_public_key(&kp.public_key());
/// let did = NovaDid::from_nova_id(&nova_id, &kp.public_key());
/// assert!(did.to_string().starts_with("did:nova:nova1"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NovaDid {
    /// The NOVA ID this DID represents.
    nova_id: NovaId,
    /// The public key associated with this DID (needed for document generation).
    public_key: NovaPublicKey,
}

impl NovaDid {
    /// Create a DID from a NOVA ID and its associated public key.
    pub fn from_nova_id(nova_id: &NovaId, public_key: &NovaPublicKey) -> Self {
        Self {
            nova_id: nova_id.clone(),
            public_key: public_key.clone(),
        }
    }

    /// Create a DID directly from a public key.
    ///
    /// Derives the NOVA ID internally, so the caller doesn't need to
    /// construct it separately.
    pub fn from_public_key(public_key: &NovaPublicKey) -> Self {
        let nova_id = NovaId::from_public_key(public_key);
        Self {
            nova_id,
            public_key: public_key.clone(),
        }
    }

    /// Return the full DID string: `did:nova:<bech32-address>`.
    pub fn to_did_string(&self) -> String {
        format!("did:{}:{}", DID_METHOD, self.nova_id.to_address())
    }

    /// Parse a DID string back into a [`NovaDid`].
    ///
    /// Requires the public key separately since it cannot be recovered
    /// from the Bech32 address alone (the address contains a hash, not
    /// the raw key).
    pub fn from_did_string(did: &str, public_key: &NovaPublicKey) -> Result<Self, DidError> {
        let parts: Vec<&str> = did.splitn(3, ':').collect();
        if parts.len() != 3 {
            return Err(DidError::InvalidFormat(
                "DID must have format 'did:<method>:<identifier>'".into(),
            ));
        }

        if parts[0] != "did" {
            return Err(DidError::InvalidFormat(format!(
                "expected 'did' prefix, got '{}'",
                parts[0]
            )));
        }

        if parts[1] != DID_METHOD {
            return Err(DidError::UnsupportedMethod(parts[1].to_string()));
        }

        let nova_id =
            NovaId::from_address(parts[2]).map_err(|e| DidError::InvalidAddress(e.to_string()))?;

        Ok(Self {
            nova_id,
            public_key: public_key.clone(),
        })
    }

    /// Return the underlying NOVA ID.
    pub fn nova_id(&self) -> &NovaId {
        &self.nova_id
    }

    /// Return the public key associated with this DID.
    pub fn public_key(&self) -> &NovaPublicKey {
        &self.public_key
    }

    /// Generate a W3C DID Document for this identity.
    ///
    /// The document includes:
    /// - `@context` — DID Core + Ed25519 suite contexts
    /// - `id` — The DID string
    /// - `verificationMethod` — Ed25519 public key encoded as multibase (base58btc)
    /// - `authentication` — Reference to the verification method
    /// - `assertionMethod` — Reference to the verification method
    /// - `created` — Timestamp of document generation
    pub fn to_did_document(&self) -> DidDocument {
        let did_string = self.to_did_string();
        let key_id = format!("{}#key-1", did_string);

        // Multibase encoding: 'z' prefix indicates base58btc.
        // For Ed25519VerificationKey2020, the public key is encoded as
        // multicodec(0xed, 0x01) + raw_key_bytes, then base58btc-encoded.
        let mut multicodec_bytes = vec![0xed, 0x01];
        multicodec_bytes.extend_from_slice(self.public_key.as_bytes());
        let public_key_multibase = format!("z{}", bs58::encode(&multicodec_bytes).into_string());

        DidDocument {
            context: vec![DID_CONTEXT.to_string(), ED25519_CONTEXT.to_string()],
            id: did_string.clone(),
            verification_method: vec![VerificationMethod {
                id: key_id.clone(),
                type_: VERIFICATION_KEY_TYPE.to_string(),
                controller: did_string,
                public_key_multibase,
            }],
            authentication: vec![key_id.clone()],
            assertion_method: vec![key_id],
            created: Utc::now(),
        }
    }

    /// Generate the DID Document as a pretty-printed JSON string.
    pub fn to_did_document_json(&self) -> Result<String, DidError> {
        let doc = self.to_did_document();
        serde_json::to_string_pretty(&doc).map_err(|e| DidError::Serialization(e.to_string()))
    }
}

impl std::fmt::Display for NovaDid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_did_string())
    }
}

// ---------------------------------------------------------------------------
// DID Document Types
// ---------------------------------------------------------------------------

/// A W3C DID Document describing a NOVA identity.
///
/// Follows the structure defined in [DID Core v1.0](https://www.w3.org/TR/did-core/).
/// All verification relationships (authentication, assertion) reference
/// the same Ed25519 key since NOVA identities are single-key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DidDocument {
    /// JSON-LD context URIs.
    #[serde(rename = "@context")]
    pub context: Vec<String>,

    /// The DID string this document describes.
    pub id: String,

    /// Verification methods (cryptographic keys) associated with this DID.
    #[serde(rename = "verificationMethod")]
    pub verification_method: Vec<VerificationMethod>,

    /// References to verification methods usable for authentication.
    pub authentication: Vec<String>,

    /// References to verification methods usable for issuing assertions.
    #[serde(rename = "assertionMethod")]
    pub assertion_method: Vec<String>,

    /// When this document was created.
    pub created: DateTime<Utc>,
}

impl DidDocument {
    /// Serialize this document to a pretty-printed JSON string.
    pub fn to_json(&self) -> Result<String, DidError> {
        serde_json::to_string_pretty(self).map_err(|e| DidError::Serialization(e.to_string()))
    }

    /// Parse a DID Document from a JSON string.
    pub fn from_json(json: &str) -> Result<Self, DidError> {
        serde_json::from_str(json).map_err(|e| DidError::Serialization(e.to_string()))
    }

    /// Validate that the document has the required fields and structure.
    ///
    /// Checks:
    /// - The `id` field starts with `did:nova:`
    /// - At least one verification method is present
    /// - At least one authentication reference is present
    /// - The DID Core context is included
    pub fn validate(&self) -> Result<(), DidError> {
        if !self.id.starts_with("did:nova:") {
            return Err(DidError::InvalidFormat(
                "document ID must start with 'did:nova:'".into(),
            ));
        }

        if self.verification_method.is_empty() {
            return Err(DidError::InvalidFormat(
                "document must have at least one verification method".into(),
            ));
        }

        if self.authentication.is_empty() {
            return Err(DidError::InvalidFormat(
                "document must have at least one authentication method".into(),
            ));
        }

        if !self.context.contains(&DID_CONTEXT.to_string()) {
            return Err(DidError::InvalidFormat(
                "document must include DID Core context".into(),
            ));
        }

        Ok(())
    }
}

/// A verification method entry in a DID Document.
///
/// Describes a single cryptographic key that can be used for
/// authentication, assertion, or other verification relationships.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationMethod {
    /// Unique identifier for this verification method (DID URL fragment).
    pub id: String,

    /// The type of cryptographic key (e.g., "Ed25519VerificationKey2020").
    #[serde(rename = "type")]
    pub type_: String,

    /// The DID that controls this verification method.
    pub controller: String,

    /// The public key material in multibase encoding (base58btc with 'z' prefix).
    #[serde(rename = "publicKeyMultibase")]
    pub public_key_multibase: String,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::NovaKeypair;

    #[test]
    fn did_string_format() {
        let kp = NovaKeypair::generate();
        let nova_id = NovaId::from_public_key(&kp.public_key());
        let did = NovaDid::from_nova_id(&nova_id, &kp.public_key());
        let did_str = did.to_did_string();
        assert!(did_str.starts_with("did:nova:nova1"), "got: {}", did_str);
    }

    #[test]
    fn did_display_matches_to_did_string() {
        let kp = NovaKeypair::generate();
        let did = NovaDid::from_public_key(&kp.public_key());
        assert_eq!(did.to_string(), did.to_did_string());
    }

    #[test]
    fn did_roundtrip_via_string() {
        let kp = NovaKeypair::generate();
        let did = NovaDid::from_public_key(&kp.public_key());
        let did_str = did.to_did_string();
        let recovered = NovaDid::from_did_string(&did_str, &kp.public_key()).unwrap();
        assert_eq!(did, recovered);
    }

    #[test]
    fn invalid_did_prefix_rejected() {
        let kp = NovaKeypair::generate();
        let result = NovaDid::from_did_string("notadid:nova:nova1abc", &kp.public_key());
        assert!(matches!(result, Err(DidError::InvalidFormat(_))));
    }

    #[test]
    fn wrong_method_rejected() {
        let kp = NovaKeypair::generate();
        let result = NovaDid::from_did_string("did:ethr:0xabc123", &kp.public_key());
        assert!(matches!(result, Err(DidError::UnsupportedMethod(_))));
    }

    #[test]
    fn did_document_has_required_fields() {
        let kp = NovaKeypair::generate();
        let did = NovaDid::from_public_key(&kp.public_key());
        let doc = did.to_did_document();

        assert!(doc.context.contains(&DID_CONTEXT.to_string()));
        assert!(doc.context.contains(&ED25519_CONTEXT.to_string()));
        assert_eq!(doc.id, did.to_did_string());
        assert_eq!(doc.verification_method.len(), 1);
        assert_eq!(doc.authentication.len(), 1);
        assert_eq!(doc.assertion_method.len(), 1);
    }

    #[test]
    fn verification_method_structure() {
        let kp = NovaKeypair::generate();
        let did = NovaDid::from_public_key(&kp.public_key());
        let doc = did.to_did_document();

        let vm = &doc.verification_method[0];
        let expected_key_id = format!("{}#key-1", did.to_did_string());
        assert_eq!(vm.id, expected_key_id);
        assert_eq!(vm.type_, VERIFICATION_KEY_TYPE);
        assert_eq!(vm.controller, did.to_did_string());
        assert!(vm.public_key_multibase.starts_with('z'));
    }

    #[test]
    fn authentication_references_key() {
        let kp = NovaKeypair::generate();
        let did = NovaDid::from_public_key(&kp.public_key());
        let doc = did.to_did_document();

        let key_id = &doc.verification_method[0].id;
        assert!(doc.authentication.contains(key_id));
        assert!(doc.assertion_method.contains(key_id));
    }

    #[test]
    fn did_document_json_roundtrip() {
        let kp = NovaKeypair::generate();
        let did = NovaDid::from_public_key(&kp.public_key());
        let json = did.to_did_document_json().unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.get("@context").is_some());
        assert!(parsed.get("id").is_some());
        assert!(parsed.get("verificationMethod").is_some());

        let doc = DidDocument::from_json(&json).unwrap();
        assert_eq!(doc.id, did.to_did_string());
    }

    #[test]
    fn did_document_validation_passes() {
        let kp = NovaKeypair::generate();
        let did = NovaDid::from_public_key(&kp.public_key());
        let doc = did.to_did_document();
        assert!(doc.validate().is_ok());
    }

    #[test]
    fn invalid_document_rejected() {
        let doc = DidDocument {
            context: vec![],
            id: "not-a-did".to_string(),
            verification_method: vec![],
            authentication: vec![],
            assertion_method: vec![],
            created: Utc::now(),
        };
        assert!(doc.validate().is_err());
    }

    #[test]
    fn multibase_key_encodes_multicodec_prefix() {
        let kp = NovaKeypair::generate();
        let did = NovaDid::from_public_key(&kp.public_key());
        let doc = did.to_did_document();

        let multibase = &doc.verification_method[0].public_key_multibase;
        let decoded = bs58::decode(&multibase[1..]).into_vec().unwrap();
        // First two bytes: Ed25519 multicodec prefix (0xed, 0x01).
        assert_eq!(decoded[0], 0xed);
        assert_eq!(decoded[1], 0x01);
        // Remaining 32 bytes: the raw public key.
        assert_eq!(&decoded[2..], kp.public_key().as_bytes());
    }

    #[test]
    fn deterministic_did_from_same_key() {
        let seed = [99u8; 32];
        let kp = NovaKeypair::from_seed(&seed);
        let did1 = NovaDid::from_public_key(&kp.public_key());
        let did2 = NovaDid::from_public_key(&kp.public_key());
        assert_eq!(did1.to_did_string(), did2.to_did_string());
    }

    #[test]
    fn did_serde_json_roundtrip() {
        let kp = NovaKeypair::generate();
        let did = NovaDid::from_public_key(&kp.public_key());
        let json = serde_json::to_string(&did).unwrap();
        let recovered: NovaDid = serde_json::from_str(&json).unwrap();
        assert_eq!(did.to_did_string(), recovered.to_did_string());
    }
}
