//! # AES-256-GCM Encryption
//!
//! Authenticated encryption for NOVA. Used for encrypting private transaction
//! data, vault secrets, and P2P communication payloads.
//!
//! We use AES-256-GCM (Galois/Counter Mode) because:
//!
//! - It's an AEAD cipher — authentication and encryption in one operation.
//!   No "encrypt-then-MAC" vs "MAC-then-encrypt" debates. It just works.
//! - AES-NI hardware acceleration is available on every modern x86 CPU and
//!   most ARM chips. Performance is essentially free.
//! - 256-bit keys provide a comfortable security margin. Even if quantum
//!   computers arrive tomorrow (they won't), Grover's algorithm only halves
//!   the effective key length, leaving us with 128-bit security.
//!
//! ## Nonce management
//!
//! GCM is notoriously unforgiving about nonce reuse. If you encrypt two
//! different messages with the same key and nonce, an attacker can recover
//! the XOR of the plaintexts AND forge authentication tags. Game over.
//!
//! Our strategy: random 96-bit nonces from a CSPRNG. The birthday bound
//! for 96-bit nonces is ~2^48 messages per key. We rotate keys long before
//! that via the PFS module. Don't try to be clever with counter-based nonces
//! unless you have a very good reason and a very good implementation.
//!
//! ## Wire format
//!
//! The `encrypt()` function returns `nonce || ciphertext` as a single `Vec<u8>`.
//! The first 12 bytes are the nonce, the rest is the ciphertext + auth tag.
//! The `decrypt()` function expects this same format.

use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use rand::RngCore;
use thiserror::Error;

use crate::config::{AES_KEY_LENGTH, AES_NONCE_LENGTH};

/// Errors that can occur during encryption/decryption.
///
/// We intentionally keep these vague. Detailed error messages about
/// cryptographic failures are a gift to attackers. The difference between
/// "wrong key" and "corrupted ciphertext" is none of their business.
#[derive(Debug, Error)]
pub enum EncryptionError {
    #[error("encryption failed")]
    EncryptFailed,

    #[error("decryption failed -- wrong key or corrupted ciphertext")]
    DecryptFailed,

    #[error("invalid key length: expected {AES_KEY_LENGTH} bytes")]
    InvalidKeyLength,

    #[error("invalid nonce length: expected {AES_NONCE_LENGTH} bytes")]
    InvalidNonceLength,

    #[error("ciphertext too short: must be at least {AES_NONCE_LENGTH} bytes")]
    CiphertextTooShort,
}

/// Encrypt plaintext with AES-256-GCM using a random nonce.
///
/// Returns `nonce || ciphertext` as a single `Vec<u8>`. The first 12 bytes
/// are the random nonce, followed by the ciphertext (which includes the
/// 16-byte GCM authentication tag appended by AES-GCM internally).
///
/// ## Additional Authenticated Data (AAD)
///
/// This basic variant does NOT use AAD. If you need to authenticate metadata
/// alongside the ciphertext (e.g., transaction IDs, sender addresses), use
/// [`encrypt_with_aad`] instead.
///
/// # Arguments
///
/// * `key` — 32-byte AES-256 key. Must be cryptographically random.
/// * `plaintext` — The data to encrypt. Can be any length.
///
/// # Example
///
/// ```
/// use nova_protocol::crypto::encryption::{encrypt, decrypt};
///
/// let key = [0x42u8; 32]; // In real code, use a properly derived key!
/// let plaintext = b"secret transaction details";
///
/// let sealed = encrypt(&key, plaintext).unwrap();
/// let recovered = decrypt(&key, &sealed).unwrap();
/// assert_eq!(recovered, plaintext);
/// ```
pub fn encrypt(key: &[u8; AES_KEY_LENGTH], plaintext: &[u8]) -> Result<Vec<u8>, EncryptionError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| EncryptionError::EncryptFailed)?;

    // Generate a random 96-bit nonce. This is the standard nonce size for
    // AES-GCM and the only one you should use. Larger nonces exist in theory
    // but aren't widely supported and have different security properties.
    let mut nonce_bytes = [0u8; AES_NONCE_LENGTH];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| EncryptionError::EncryptFailed)?;

    // Pack nonce || ciphertext into a single buffer.
    // This is the simplest wire format and the caller doesn't have to
    // manage the nonce separately.
    let mut out = Vec::with_capacity(AES_NONCE_LENGTH + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypt data previously encrypted with [`encrypt`].
///
/// Expects `nonce || ciphertext` format (12-byte nonce prefix followed by
/// ciphertext + auth tag).
///
/// # Errors
///
/// Returns `EncryptionError::DecryptFailed` if:
/// - The key is wrong.
/// - The ciphertext has been modified (bit flip, truncation, etc.).
/// - The nonce doesn't match (impossible if using our `encrypt` format).
///
/// We don't distinguish between these cases on purpose.
pub fn decrypt(key: &[u8; AES_KEY_LENGTH], data: &[u8]) -> Result<Vec<u8>, EncryptionError> {
    if data.len() < AES_NONCE_LENGTH {
        return Err(EncryptionError::CiphertextTooShort);
    }

    let (nonce_bytes, ciphertext) = data.split_at(AES_NONCE_LENGTH);
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| EncryptionError::DecryptFailed)?;
    let nonce = Nonce::from_slice(nonce_bytes);

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| EncryptionError::DecryptFailed)
}

/// Encrypt with Additional Authenticated Data (AAD).
///
/// The AAD is authenticated but NOT encrypted. Use it for metadata that
/// needs integrity protection but doesn't need to be secret — transaction
/// IDs, sender addresses, timestamps, etc.
///
/// Returns a tuple of `(nonce, ciphertext)` where the nonce is 12 bytes
/// and the ciphertext includes the 16-byte auth tag.
///
/// The caller MUST provide the same AAD at decryption time, or authentication
/// will fail. This is the "A" in AEAD doing its job.
pub fn encrypt_with_aad(
    key: &[u8; AES_KEY_LENGTH],
    plaintext: &[u8],
    aad: &[u8],
) -> Result<([u8; AES_NONCE_LENGTH], Vec<u8>), EncryptionError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| EncryptionError::EncryptFailed)?;

    let mut nonce_bytes = [0u8; AES_NONCE_LENGTH];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let payload = Payload {
        msg: plaintext,
        aad,
    };

    let ciphertext = cipher
        .encrypt(nonce, payload)
        .map_err(|_| EncryptionError::EncryptFailed)?;

    Ok((nonce_bytes, ciphertext))
}

/// Decrypt ciphertext that was encrypted with AAD.
///
/// The nonce and AAD must match the values used during encryption, or
/// decryption will fail with an authentication error. This is by design —
/// any mismatch means tampering.
pub fn decrypt_with_aad(
    key: &[u8; AES_KEY_LENGTH],
    nonce: &[u8; AES_NONCE_LENGTH],
    ciphertext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, EncryptionError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| EncryptionError::DecryptFailed)?;
    let nonce = Nonce::from_slice(nonce);

    let payload = Payload {
        msg: ciphertext,
        aad,
    };

    cipher
        .decrypt(nonce, payload)
        .map_err(|_| EncryptionError::DecryptFailed)
}

/// Encrypt with a key provided as a byte slice (length-checked at runtime).
///
/// Convenience wrapper for when the key comes from an untrusted source
/// (e.g., deserialized from config) and might not be the right length.
pub fn encrypt_checked(key: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, EncryptionError> {
    let key: &[u8; AES_KEY_LENGTH] = key
        .try_into()
        .map_err(|_| EncryptionError::InvalidKeyLength)?;
    encrypt(key, plaintext)
}

/// Decrypt with a key provided as a byte slice (length-checked at runtime).
pub fn decrypt_checked(key: &[u8], data: &[u8]) -> Result<Vec<u8>, EncryptionError> {
    let key: &[u8; AES_KEY_LENGTH] = key
        .try_into()
        .map_err(|_| EncryptionError::InvalidKeyLength)?;
    decrypt(key, data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> [u8; 32] {
        // A fixed key for testing. Never use a predictable key in production.
        // But you knew that. Right?
        let mut key = [0u8; 32];
        for (i, byte) in key.iter_mut().enumerate() {
            *byte = i as u8;
        }
        key
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = test_key();
        let plaintext = b"the quick brown fox jumps over the lazy dog";

        let sealed = encrypt(&key, plaintext).unwrap();
        let recovered = decrypt(&key, &sealed).unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn test_encrypt_empty_plaintext() {
        // Encrypting nothing is valid — you get just the nonce + auth tag.
        let key = test_key();
        let sealed = encrypt(&key, b"").unwrap();
        // 12 bytes nonce + 16 bytes auth tag = 28 bytes minimum
        assert_eq!(sealed.len(), AES_NONCE_LENGTH + 16);
        let recovered = decrypt(&key, &sealed).unwrap();
        assert!(recovered.is_empty());
    }

    #[test]
    fn test_wrong_key_fails_decryption() {
        let key = test_key();
        let sealed = encrypt(&key, b"secret").unwrap();

        let mut wrong_key = test_key();
        wrong_key[0] ^= 0xFF; // Flip one byte

        assert!(decrypt(&wrong_key, &sealed).is_err());
    }

    #[test]
    fn test_modified_ciphertext_fails_decryption() {
        let key = test_key();
        let mut sealed = encrypt(&key, b"secret").unwrap();
        // Corrupt a byte in the ciphertext portion (after the nonce)
        sealed[AES_NONCE_LENGTH] ^= 0xFF;

        assert!(decrypt(&key, &sealed).is_err());
    }

    #[test]
    fn test_unique_nonces() {
        // Two encryptions with the same key should produce different nonces.
        // If this fails, the RNG is broken and we need to burn everything down.
        let key = test_key();
        let sealed1 = encrypt(&key, b"message").unwrap();
        let sealed2 = encrypt(&key, b"message").unwrap();
        // Compare the nonce portions (first 12 bytes)
        assert_ne!(&sealed1[..AES_NONCE_LENGTH], &sealed2[..AES_NONCE_LENGTH]);
    }

    #[test]
    fn test_ciphertext_length() {
        // Sealed output should be nonce (12) + plaintext length + auth tag (16).
        let key = test_key();
        let plaintext = b"exactly 26 bytes of input!";
        let sealed = encrypt(&key, plaintext).unwrap();
        assert_eq!(sealed.len(), AES_NONCE_LENGTH + plaintext.len() + 16);
    }

    #[test]
    fn test_aad_encrypt_decrypt_roundtrip() {
        let key = test_key();
        let plaintext = b"private payment data";
        let aad = b"tx-id:abc123";

        let (nonce, ciphertext) = encrypt_with_aad(&key, plaintext, aad).unwrap();
        let recovered = decrypt_with_aad(&key, &nonce, &ciphertext, aad).unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn test_wrong_aad_fails_decryption() {
        let key = test_key();
        let (nonce, ciphertext) = encrypt_with_aad(&key, b"secret", b"correct-aad").unwrap();

        // Changing the AAD should cause authentication failure.
        // This is the whole point of "authenticated" in AEAD.
        assert!(decrypt_with_aad(&key, &nonce, &ciphertext, b"wrong-aad").is_err());
    }

    #[test]
    fn test_encrypt_checked_rejects_short_key() {
        let short_key = [0u8; 16]; // AES-128, not AES-256
        assert!(encrypt_checked(&short_key, b"test").is_err());
    }

    #[test]
    fn test_decrypt_too_short() {
        let key = test_key();
        let too_short = [0u8; 4]; // Way too short to contain a nonce
        assert!(decrypt(&key, &too_short).is_err());
    }

    #[test]
    fn test_large_plaintext() {
        // AES-GCM can handle messages up to 2^36 - 32 bytes per NIST SP 800-38D.
        // We won't test that limit, but 1MB should be fine.
        let key = test_key();
        let plaintext = vec![0xAB; 1_000_000];
        let sealed = encrypt(&key, &plaintext).unwrap();
        let recovered = decrypt(&key, &sealed).unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn test_encrypt_checked_with_valid_key() {
        let key = test_key();
        let sealed = encrypt_checked(&key, b"hello").unwrap();
        let recovered = decrypt_checked(&key, &sealed).unwrap();
        assert_eq!(recovered, b"hello");
    }
}
