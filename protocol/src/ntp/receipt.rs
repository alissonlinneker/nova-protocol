//! # NTP Step 5 -- Receipt Exchange
//!
//! The final step in the NTP flow: both parties sign a payment receipt
//! confirming that the transaction was settled. This dual-signed receipt
//! serves as non-repudiable proof of payment.
//!
//! ## Receipt Properties
//!
//! - **Non-repudiation** -- both sender and receiver sign the receipt.
//! - **Verifiability** -- any third party with both public keys can verify.
//! - **Minimal** -- contains only essential settlement data.
//!
//! ## Signing Protocol
//!
//! 1. Sender generates the receipt after confirmation and signs it.
//! 2. Sender transmits the receipt (with signature) to the receiver.
//! 3. Receiver verifies sender's signature, countersigns, returns it.
//! 4. Both parties store the dual-signed receipt.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::crypto::keys::{NovaKeypair, NovaPublicKey, NovaSignature};
use crate::transaction::types::Currency;

use super::error::NtpError;
use super::handshake::EstablishedSession;
use super::settlement::SettlementResult;

// ---------------------------------------------------------------------------
// Receipt
// ---------------------------------------------------------------------------

/// A dual-signed payment receipt proving a transaction was settled.
///
/// Both parties hold a copy. Either can present it as proof of payment.
/// Third parties verify both signatures independently.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PaymentReceipt {
    /// Unique receipt identifier (UUIDv4).
    pub receipt_id: String,
    /// The NTP session this receipt belongs to.
    pub session_id: String,
    /// Transaction hash (hex-encoded) as settled on-chain.
    pub transaction_hash: String,
    /// Block height where the transaction was confirmed.
    pub block_height: u64,
    /// Sender's NOVA ID (payer).
    pub sender: String,
    /// Sender's Ed25519 public key.
    pub sender_pubkey: NovaPublicKey,
    /// Receiver's NOVA ID (payee).
    pub receiver: String,
    /// Receiver's Ed25519 public key.
    pub receiver_pubkey: NovaPublicKey,
    /// Payment amount in smallest units.
    pub amount: u64,
    /// Currency of the payment.
    pub currency: Currency,
    /// Unix timestamp (milliseconds) of the settlement block.
    pub timestamp: u64,
    /// Sender's Ed25519 signature over the receipt body.
    pub sender_signature: Option<NovaSignature>,
    /// Receiver's Ed25519 signature over the receipt body.
    pub receiver_signature: Option<NovaSignature>,
}

impl PaymentReceipt {
    /// Compute the canonical byte representation of the receipt body.
    ///
    /// This is the message that both parties sign. It excludes the
    /// signature fields to avoid circular dependencies.
    pub fn signing_payload(&self) -> Vec<u8> {
        let canonical = format!(
            "{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
            self.receipt_id,
            self.session_id,
            self.transaction_hash,
            self.block_height,
            self.sender,
            self.receiver,
            self.amount,
            self.currency,
            self.timestamp,
            hex::encode(self.sender_pubkey.as_bytes()),
        );
        canonical.into_bytes()
    }

    /// Returns `true` if both signatures are present.
    pub fn is_fully_signed(&self) -> bool {
        self.sender_signature.is_some() && self.receiver_signature.is_some()
    }
}

// ---------------------------------------------------------------------------
// Receipt Generation
// ---------------------------------------------------------------------------

/// Generate an unsigned payment receipt from a settlement result and session.
///
/// Called by the **sender** after the transaction is confirmed on-chain.
///
/// # Errors
///
/// Returns an error if the settlement result is not `Confirmed`.
pub fn generate_receipt(
    settlement: &SettlementResult,
    session: &EstablishedSession,
) -> Result<PaymentReceipt, NtpError> {
    match settlement {
        SettlementResult::Confirmed {
            block_height,
            tx_hash,
            block_timestamp,
            ..
        } => Ok(PaymentReceipt {
            receipt_id: Uuid::new_v4().to_string(),
            session_id: session.session_id.clone(),
            transaction_hash: tx_hash.clone(),
            block_height: *block_height,
            sender: session.our_nova_id.clone(),
            sender_pubkey: session.our_pubkey.clone(),
            receiver: session.peer_nova_id.clone(),
            receiver_pubkey: session.peer_pubkey.clone(),
            amount: session.payment_params.amount,
            currency: session.payment_params.currency.clone(),
            timestamp: *block_timestamp,
            sender_signature: None,
            receiver_signature: None,
        }),
        SettlementResult::Rejected { reason, .. } => {
            Err(NtpError::SettlementRejected(reason.clone()))
        }
        SettlementResult::TimedOut {
            elapsed_ms,
            timeout_ms,
        } => Err(NtpError::SettlementTimeout {
            elapsed_ms: *elapsed_ms,
            timeout_ms: *timeout_ms,
        }),
    }
}

/// Sign a receipt as the sender.
///
/// Attaches the sender's Ed25519 signature to the receipt.
pub fn sign_receipt_as_sender(receipt: &mut PaymentReceipt, keypair: &NovaKeypair) {
    let payload = receipt.signing_payload();
    let sig = keypair.sign(&payload);
    receipt.sender_signature = Some(sig);
}

/// Countersign a receipt as the receiver.
///
/// Verifies the sender's signature first, then attaches the receiver's.
/// After this call, the receipt is fully signed.
///
/// # Errors
///
/// Returns an error if the sender's signature is missing or invalid.
pub fn countersign_receipt(
    receipt: &mut PaymentReceipt,
    receiver_keypair: &NovaKeypair,
) -> Result<(), NtpError> {
    let sender_sig = receipt
        .sender_signature
        .as_ref()
        .ok_or_else(|| NtpError::InvalidReceiptSignature("sender signature missing".to_string()))?;

    let payload = receipt.signing_payload();
    if !receipt.sender_pubkey.verify(&payload, sender_sig) {
        return Err(NtpError::InvalidReceiptSignature(
            "sender signature verification failed".to_string(),
        ));
    }

    let receiver_sig = receiver_keypair.sign(&payload);
    receipt.receiver_signature = Some(receiver_sig);
    Ok(())
}

/// Verify a fully signed receipt.
///
/// Checks both the sender's and receiver's signatures against the
/// receipt's canonical payload.
///
/// # Errors
///
/// Returns an error if either signature is missing or invalid.
pub fn verify_receipt(receipt: &PaymentReceipt) -> Result<bool, NtpError> {
    let sender_sig = receipt
        .sender_signature
        .as_ref()
        .ok_or_else(|| NtpError::InvalidReceiptSignature("sender signature missing".to_string()))?;

    let receiver_sig = receipt.receiver_signature.as_ref().ok_or_else(|| {
        NtpError::InvalidReceiptSignature("receiver signature missing".to_string())
    })?;

    let payload = receipt.signing_payload();

    if !receipt.sender_pubkey.verify(&payload, sender_sig) {
        return Err(NtpError::InvalidReceiptSignature(
            "sender signature invalid".to_string(),
        ));
    }

    if !receipt.receiver_pubkey.verify(&payload, receiver_sig) {
        return Err(NtpError::InvalidReceiptSignature(
            "receiver signature invalid".to_string(),
        ));
    }

    Ok(true)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::keys::NovaKeypair;
    use crate::identity::nova_id::NovaId;
    use crate::ntp::handshake::PaymentParams;
    use crate::ntp::settlement::ValidationStage;

    fn make_test_session(sender_kp: &NovaKeypair, receiver_kp: &NovaKeypair) -> EstablishedSession {
        EstablishedSession {
            session_id: "test-session-42".to_string(),
            shared_secret: [0u8; 32],
            peer_nova_id: NovaId::from_public_key(&receiver_kp.public_key()).to_address(),
            peer_pubkey: receiver_kp.public_key(),
            payment_params: PaymentParams {
                amount: 5000,
                currency: Currency::BRL,
                description: "Test payment".to_string(),
            },
            our_nova_id: NovaId::from_public_key(&sender_kp.public_key()).to_address(),
            our_pubkey: sender_kp.public_key(),
        }
    }

    fn make_confirmed() -> SettlementResult {
        SettlementResult::Confirmed {
            block_height: 100,
            tx_hash: "abcdef1234567890".to_string(),
            block_hash: "blockhash999".to_string(),
            tx_index: 0,
            block_timestamp: 1700000000000,
        }
    }

    #[test]
    fn generate_receipt_from_confirmed_settlement() {
        let sender_kp = NovaKeypair::generate();
        let receiver_kp = NovaKeypair::generate();
        let session = make_test_session(&sender_kp, &receiver_kp);

        let receipt = generate_receipt(&make_confirmed(), &session).unwrap();

        assert_eq!(receipt.session_id, "test-session-42");
        assert_eq!(receipt.block_height, 100);
        assert_eq!(receipt.amount, 5000);
        assert_eq!(receipt.currency, Currency::BRL);
        assert!(!receipt.is_fully_signed());
    }

    #[test]
    fn generate_receipt_from_rejected_fails() {
        let sender_kp = NovaKeypair::generate();
        let receiver_kp = NovaKeypair::generate();
        let session = make_test_session(&sender_kp, &receiver_kp);

        let settlement = SettlementResult::Rejected {
            reason: "bad sig".to_string(),
            stage: ValidationStage::Signature,
        };

        let result = generate_receipt(&settlement, &session);
        assert!(matches!(result, Err(NtpError::SettlementRejected(_))));
    }

    #[test]
    fn full_receipt_signing_and_verification() {
        let sender_kp = NovaKeypair::generate();
        let receiver_kp = NovaKeypair::generate();
        let session = make_test_session(&sender_kp, &receiver_kp);

        let mut receipt = generate_receipt(&make_confirmed(), &session).unwrap();

        // Sender signs.
        sign_receipt_as_sender(&mut receipt, &sender_kp);
        assert!(receipt.sender_signature.is_some());
        assert!(!receipt.is_fully_signed());

        // Receiver countersigns.
        countersign_receipt(&mut receipt, &receiver_kp).unwrap();
        assert!(receipt.is_fully_signed());

        // Verify.
        let valid = verify_receipt(&receipt).unwrap();
        assert!(valid);
    }

    #[test]
    fn countersign_without_sender_sig_fails() {
        let sender_kp = NovaKeypair::generate();
        let receiver_kp = NovaKeypair::generate();
        let session = make_test_session(&sender_kp, &receiver_kp);

        let mut receipt = generate_receipt(&make_confirmed(), &session).unwrap();
        let result = countersign_receipt(&mut receipt, &receiver_kp);
        assert!(matches!(result, Err(NtpError::InvalidReceiptSignature(_))));
    }

    #[test]
    fn tampered_receipt_fails_verification() {
        let sender_kp = NovaKeypair::generate();
        let receiver_kp = NovaKeypair::generate();
        let session = make_test_session(&sender_kp, &receiver_kp);

        let mut receipt = generate_receipt(&make_confirmed(), &session).unwrap();
        sign_receipt_as_sender(&mut receipt, &sender_kp);
        countersign_receipt(&mut receipt, &receiver_kp).unwrap();

        // Tamper with the amount.
        receipt.amount = 9999;

        let result = verify_receipt(&receipt);
        assert!(result.is_err());
    }

    #[test]
    fn receipt_serialization_roundtrip() {
        let sender_kp = NovaKeypair::generate();
        let receiver_kp = NovaKeypair::generate();
        let session = make_test_session(&sender_kp, &receiver_kp);

        let mut receipt = generate_receipt(&make_confirmed(), &session).unwrap();
        sign_receipt_as_sender(&mut receipt, &sender_kp);
        countersign_receipt(&mut receipt, &receiver_kp).unwrap();

        let json = serde_json::to_string(&receipt).expect("serialize");
        let recovered: PaymentReceipt = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(recovered.receipt_id, receipt.receipt_id);
        assert!(recovered.is_fully_signed());
        assert!(verify_receipt(&recovered).unwrap());
    }
}
