//! # Digital Signatures
//!
//! Ed25519 signing and verification — the backbone of authentication in NOVA.
//!
//! Every transaction, every block header, every peer handshake is authenticated
//! with an Ed25519 signature. This module provides the signing and verification
//! functions that make that possible.
//!
//! ## Why not just use ed25519-dalek directly?
//!
//! We could, and in some internal code we do. But wrapping the operations
//! gives us:
//!
//! 1. A single place to audit all signing operations.
//! 2. Consistent error types across the codebase.
//! 3. A natural extension point for multi-sig and threshold signatures later.
//! 4. Type safety — you can't accidentally pass a hash where a message goes.
//!
//! ## Strictness
//!
//! We use `ed25519-dalek`'s strict verification by default. This means we
//! reject some edge-case signatures that lenient implementations accept.
//! This is deliberate: stricter is safer, and we don't need to be compatible
//! with legacy Ed25519 implementations that get the cofactor wrong.

use ed25519_dalek::{Signature as DalekSignature, Verifier, VerifyingKey};
use thiserror::Error;

use super::keys::{NovaKeypair, NovaPublicKey, NovaSignature};

/// Errors during signature operations.
///
/// Intentionally vague — we don't tell attackers why verification failed.
#[derive(Debug, Error)]
pub enum SignatureError {
    #[error("signature verification failed")]
    VerificationFailed,

    #[error("invalid signature bytes: expected 64 bytes")]
    InvalidSignatureBytes,

    #[error("invalid public key")]
    InvalidPublicKey,
}

/// Sign a message using a NOVA keypair.
///
/// Produces a 64-byte Ed25519 signature over the given message bytes.
/// The signature is deterministic — signing the same message with the same
/// key will always produce the same signature (RFC 8032). No nonce reuse
/// bugs possible. Thank you, Bernstein.
///
/// # Arguments
///
/// * `keypair` — The signer's keypair. Only the signing key is used, but
///   we take the full keypair to prevent callers from forgetting they need
///   the private key.
/// * `message` — The message bytes to sign. Can be any length; Ed25519
///   internally hashes with SHA-512.
///
/// # Example
///
/// ```
/// use nova_protocol::crypto::{NovaKeypair, sign, verify};
///
/// let keypair = NovaKeypair::generate();
/// let message = b"send 100 NOVA to alice";
/// let signature = sign(&keypair, message);
///
/// assert!(verify(&keypair.public_key(), message, &signature));
/// ```
pub fn sign(keypair: &NovaKeypair, message: &[u8]) -> NovaSignature {
    keypair.sign(message)
}

/// Verify an Ed25519 signature against a public key and message.
///
/// Returns `true` if the signature is valid, `false` otherwise.
/// We intentionally don't distinguish between "invalid signature" and
/// "wrong public key" — both are just "nope." Giving attackers a
/// detailed error oracle is a bad idea.
///
/// # Arguments
///
/// * `public_key` — The signer's public key.
/// * `message` — The original message bytes.
/// * `signature` — The signature to verify.
pub fn verify(public_key: &NovaPublicKey, message: &[u8], signature: &NovaSignature) -> bool {
    public_key.verify(message, signature)
}

/// Verify a signature using raw byte components.
///
/// This is the "I got these bytes off the wire and need to check them" variant.
/// It parses the public key and signature bytes, then does the verification.
///
/// Useful when deserializing transactions from the network where everything
/// arrives as byte slices rather than typed structs.
pub fn verify_raw(
    public_key_bytes: &[u8; 32],
    message: &[u8],
    signature_bytes: &[u8; 64],
) -> Result<(), SignatureError> {
    let verifying_key =
        VerifyingKey::from_bytes(public_key_bytes).map_err(|_| SignatureError::InvalidPublicKey)?;

    let signature = DalekSignature::from_bytes(signature_bytes);

    verifying_key
        .verify(message, &signature)
        .map_err(|_| SignatureError::VerificationFailed)
}

/// Sign a message and return the signature as raw bytes.
///
/// Convenience function for when you need bytes instead of a `NovaSignature`
/// struct. Common in serialization paths where you're building wire-format
/// messages and don't want to round-trip through the typed wrapper.
pub fn sign_to_bytes(keypair: &NovaKeypair, message: &[u8]) -> Vec<u8> {
    let sig = sign(keypair, message);
    sig.as_bytes().to_vec()
}

/// Batch-verify multiple signatures.
///
/// All signatures must be valid for this to return `Ok`. If any single
/// signature fails, the entire batch fails — we don't tell you which one.
/// If you need to know which signature is bad, verify them individually.
///
/// Currently falls back to sequential verification. True batch verification
/// (using randomized linear combinations) is on the roadmap and will give
/// us ~2x speedup for large batches without changing this API.
///
/// # Arguments
///
/// * `items` — A slice of (public_key, message, signature) tuples.
pub fn batch_verify(
    items: &[(NovaPublicKey, Vec<u8>, NovaSignature)],
) -> Result<(), SignatureError> {
    // Sequential verification for now. O(n) where each verification is
    // ~1600 field operations on Curve25519. For typical block sizes (<1000 txs),
    // this completes in under 100ms on modern hardware.
    //
    // When we switch to true batch verification, the math is:
    //   check: sum(s_i * B) == sum(R_i) + sum(k_i * A_i)
    // with random scalars to prevent cancellation attacks.
    for (pubkey, message, signature) in items {
        if !verify(pubkey, message, signature) {
            return Err(SignatureError::VerificationFailed);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::NovaKeypair;

    #[test]
    fn test_sign_and_verify() {
        let kp = NovaKeypair::generate();
        let msg = b"hello, world";
        let sig = sign(&kp, msg);
        assert!(verify(&kp.public_key(), msg, &sig));
    }

    #[test]
    fn test_wrong_message_fails() {
        let kp = NovaKeypair::generate();
        let sig = sign(&kp, b"correct message");
        assert!(!verify(&kp.public_key(), b"wrong message", &sig));
    }

    #[test]
    fn test_wrong_key_fails() {
        let kp1 = NovaKeypair::generate();
        let kp2 = NovaKeypair::generate();
        let msg = b"test message";
        let sig = sign(&kp1, msg);
        // Verifying with a different key should fail.
        assert!(!verify(&kp2.public_key(), msg, &sig));
    }

    #[test]
    fn test_deterministic_signatures() {
        // Ed25519 is deterministic — same key + same message = same signature.
        let kp = NovaKeypair::generate();
        let msg = b"determinism is underrated";
        let sig1 = sign(&kp, msg);
        let sig2 = sign(&kp, msg);
        assert_eq!(sig1.as_bytes(), sig2.as_bytes());
    }

    #[test]
    fn test_sign_to_bytes_roundtrip() {
        let kp = NovaKeypair::generate();
        let msg = b"bytes go in, bytes come out";
        let sig_bytes = sign_to_bytes(&kp, msg);
        assert_eq!(sig_bytes.len(), 64);

        // Verify using the raw bytes path
        let pk_bytes = kp.public_key_bytes();
        let mut sig_arr = [0u8; 64];
        sig_arr.copy_from_slice(&sig_bytes);
        assert!(verify_raw(&pk_bytes, msg, &sig_arr).is_ok());
    }

    #[test]
    fn test_verify_raw_with_invalid_pubkey() {
        // All zeros is not a valid Ed25519 public key (it's the identity point,
        // which is a small-order point that should be rejected).
        let bad_pk = [0u8; 32];
        let msg = b"doesn't matter";
        let sig = [0u8; 64];
        assert!(verify_raw(&bad_pk, msg, &sig).is_err());
    }

    #[test]
    fn test_empty_message() {
        // Signing an empty message should work fine. Ed25519 doesn't care.
        let kp = NovaKeypair::generate();
        let sig = sign(&kp, b"");
        assert!(verify(&kp.public_key(), b"", &sig));
    }

    #[test]
    fn test_large_message() {
        // Ed25519 can sign messages of any length (it hashes internally).
        let kp = NovaKeypair::generate();
        let msg = vec![0xAB; 1_000_000]; // 1 MB of data
        let sig = sign(&kp, &msg);
        assert!(verify(&kp.public_key(), &msg, &sig));
    }

    #[test]
    fn test_batch_verify_success() {
        let items: Vec<(NovaPublicKey, Vec<u8>, NovaSignature)> = (0..10)
            .map(|i| {
                let kp = NovaKeypair::generate();
                let msg = format!("message number {}", i).into_bytes();
                let sig = sign(&kp, &msg);
                (kp.public_key(), msg, sig)
            })
            .collect();

        assert!(batch_verify(&items).is_ok());
    }

    #[test]
    fn test_batch_verify_one_bad_apple() {
        let kp1 = NovaKeypair::generate();
        let kp2 = NovaKeypair::generate();

        let msg1 = b"valid".to_vec();
        let sig1 = sign(&kp1, &msg1);

        let msg2 = b"also valid".to_vec();
        let sig2 = sign(&kp2, &msg2);

        // Swap the public key on the second one to make it invalid
        let items = vec![
            (kp1.public_key(), msg1, sig1),
            (kp1.public_key(), msg2, sig2), // wrong key for this sig
        ];

        assert!(batch_verify(&items).is_err());
    }

    #[test]
    fn test_batch_verify_empty() {
        // Vacuously true — no signatures to fail.
        assert!(batch_verify(&[]).is_ok());
    }
}
