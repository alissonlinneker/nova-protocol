//! Transaction signing with Ed25519 keypairs.
//!
//! Signing is a separate step from building because the keypair may not
//! be available at construction time (e.g., hardware wallet, remote signer).
//! The signing data is the canonical [`Transaction::signable_bytes`] output,
//! which deterministically excludes the signature and ZKP proof fields.

use super::builder::Transaction;
use crate::crypto::keys::NovaKeypair;

/// Signs a transaction in place using the provided keypair.
///
/// The signing procedure:
/// 1. Compute `signable_bytes()` — the canonical binary serialization of all
///    fields except `id`, `signature`, and `zkp_proof`.
/// 2. Produce an Ed25519 signature over those bytes.
/// 3. Store the hex-encoded signature in `tx.signature`.
///
/// The transaction `id` is not affected by signing (it is derived from the
/// same signable bytes and is computed at build time).
///
/// # Arguments
///
/// * `tx` — A mutable reference to the transaction to sign. The `signature`
///   field will be overwritten.
/// * `keypair` — The sender's Ed25519 keypair. The caller is responsible for
///   ensuring this matches the `tx.sender` address.
///
/// # Returns
///
/// A reference to the (now signed) transaction, for chaining convenience.
///
/// # Example
///
/// ```rust,no_run
/// use nova_protocol::crypto::keys::NovaKeypair;
/// use nova_protocol::transaction::{TransactionBuilder, TransactionType, sign_transaction};
/// use nova_protocol::transaction::types::{Amount, Currency};
///
/// let keypair = NovaKeypair::generate();
/// let mut tx = TransactionBuilder::new(TransactionType::Transfer)
///     .sender("nova:aabb...")
///     .receiver("nova:ccdd...")
///     .amount(Amount::new(1_000, Currency::NOVA))
///     .nonce(1)
///     .build();
///
/// sign_transaction(&mut tx, &keypair);
/// assert!(tx.is_signed());
/// ```
pub fn sign_transaction<'a>(tx: &'a mut Transaction, keypair: &NovaKeypair) -> &'a Transaction {
    let signable = tx.signable_bytes();
    let signature = keypair.sign(&signable);
    tx.signature = Some(signature.to_hex());
    tx.sender_public_key = Some(keypair.public_key().to_hex());
    tx
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::keys::NovaKeypair;
    use crate::transaction::builder::TransactionBuilder;
    use crate::transaction::types::{Amount, Currency, TransactionType};

    #[test]
    fn sign_sets_signature_field() {
        let kp = NovaKeypair::generate();
        let mut tx = TransactionBuilder::new(TransactionType::Transfer)
            .sender("nova:aaaa")
            .receiver("nova:bbbb")
            .amount(Amount::new(500, Currency::NOVA))
            .nonce(1)
            .build();

        assert!(!tx.is_signed());
        sign_transaction(&mut tx, &kp);
        assert!(tx.is_signed());
    }

    #[test]
    fn signature_is_128_hex_chars() {
        // Ed25519 signatures are 64 bytes = 128 hex characters.
        let kp = NovaKeypair::generate();
        let mut tx = TransactionBuilder::new(TransactionType::Transfer)
            .sender("nova:aaaa")
            .receiver("nova:bbbb")
            .amount(Amount::new(100, Currency::NOVA))
            .nonce(1)
            .build();

        sign_transaction(&mut tx, &kp);
        let sig = tx.signature.as_ref().unwrap();
        assert_eq!(sig.len(), 128);
        assert!(sig.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn signing_does_not_change_id() {
        let kp = NovaKeypair::generate();
        let mut tx = TransactionBuilder::new(TransactionType::Transfer)
            .sender("nova:aaaa")
            .receiver("nova:bbbb")
            .amount(Amount::new(100, Currency::NOVA))
            .nonce(1)
            .build();

        let id_before = tx.id.clone();
        sign_transaction(&mut tx, &kp);
        assert_eq!(
            tx.id, id_before,
            "signing must not change the transaction ID"
        );
    }

    #[test]
    fn signing_is_deterministic() {
        let kp = NovaKeypair::generate();

        let mut tx1 = TransactionBuilder::new(TransactionType::Transfer)
            .sender("nova:aaaa")
            .receiver("nova:bbbb")
            .amount(Amount::new(100, Currency::NOVA))
            .nonce(1)
            .timestamp(1_700_000_000_000)
            .build();

        let mut tx2 = TransactionBuilder::new(TransactionType::Transfer)
            .sender("nova:aaaa")
            .receiver("nova:bbbb")
            .amount(Amount::new(100, Currency::NOVA))
            .nonce(1)
            .timestamp(1_700_000_000_000)
            .build();

        sign_transaction(&mut tx1, &kp);
        sign_transaction(&mut tx2, &kp);

        assert_eq!(
            tx1.signature, tx2.signature,
            "Ed25519 signing is deterministic for the same keypair and message"
        );
    }

    #[test]
    fn different_keypairs_produce_different_signatures() {
        let kp1 = NovaKeypair::generate();
        let kp2 = NovaKeypair::generate();

        let build = || {
            TransactionBuilder::new(TransactionType::Transfer)
                .sender("nova:aaaa")
                .receiver("nova:bbbb")
                .amount(Amount::new(100, Currency::NOVA))
                .nonce(1)
                .timestamp(1_700_000_000_000)
                .build()
        };

        let mut tx1 = build();
        let mut tx2 = build();

        sign_transaction(&mut tx1, &kp1);
        sign_transaction(&mut tx2, &kp2);

        assert_ne!(tx1.signature, tx2.signature);
    }

    #[test]
    fn re_signing_overwrites_previous_signature() {
        let kp1 = NovaKeypair::generate();
        let kp2 = NovaKeypair::generate();

        let mut tx = TransactionBuilder::new(TransactionType::Transfer)
            .sender("nova:aaaa")
            .receiver("nova:bbbb")
            .amount(Amount::new(100, Currency::NOVA))
            .nonce(1)
            .timestamp(1_700_000_000_000)
            .build();

        sign_transaction(&mut tx, &kp1);
        let sig1 = tx.signature.clone();

        sign_transaction(&mut tx, &kp2);
        let sig2 = tx.signature.clone();

        assert_ne!(
            sig1, sig2,
            "re-signing with a different key must change the signature"
        );
    }
}
