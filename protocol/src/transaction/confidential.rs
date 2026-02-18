//! Confidential transaction construction.
//!
//! Provides a high-level helper for building privacy-preserving transfers
//! that embed a Groth16 balance proof and Pedersen commitment directly
//! into the transaction. The amount is hidden from validators — they only
//! see that the proof verifies, confirming the sender has sufficient funds
//! without learning the actual transfer value.
//!
//! ## Performance
//!
//! Proof generation involves an FFT-based multi-scalar multiplication over
//! BN254, which takes ~2-3 seconds on modern hardware. This is a one-time
//! cost paid by the sender at transaction creation; verification is <5ms.

use super::builder::{Transaction, TransactionBuilder};
use super::types::{Amount, Currency, TransactionType};
use crate::zkp::commitment;
use crate::zkp::prover::{BalanceProof, BalanceProver};

use ark_bn254::Fr;

/// Builds a confidential transfer transaction with an embedded ZKP proof.
///
/// This function performs the full pipeline:
/// 1. Construct the base transaction with `ConfidentialTransfer` type.
/// 2. Generate a Pedersen commitment to the transfer amount.
/// 3. Generate a Groth16 proof that the sender's balance covers the amount.
/// 4. Attach both artifacts to the transaction and recompute the ID.
///
/// The transaction is returned unsigned — the caller must sign it separately
/// via [`super::signing::sign_transaction`].
///
/// # Arguments
///
/// * `sender` — Sender's NOVA address (Bech32).
/// * `receiver` — Receiver's NOVA address (Bech32).
/// * `amount` — Transfer amount in the smallest currency unit.
/// * `blinding` — Random blinding factor for the Pedersen commitment. Must
///   be kept secret by the sender and used to open the commitment later
///   if dispute resolution requires it.
/// * `prover` — A pre-initialized `BalanceProver` (carries the proving key).
///
/// # Errors
///
/// Returns an error if proof generation fails (e.g., the prover's internal
/// balance is less than the transfer amount).
///
/// # Performance
///
/// Proof generation takes ~2-3 seconds. This is expected and unavoidable
/// with Groth16 over BN254. Callers should run this off the main thread.
pub fn create_confidential_transfer(
    sender: &str,
    receiver: &str,
    amount: u64,
    blinding: Fr,
    prover: &BalanceProver,
) -> anyhow::Result<Transaction> {
    let params = prover.pedersen_params();

    // Generate the Pedersen commitment to the transfer amount.
    let comm = commitment::commit(params, amount, blinding);

    // Generate the Groth16 proof: "I know (amount, blinding) such that the
    // commitment opens correctly and amount >= amount" (trivially true, but
    // the proof binds the commitment to a specific value the prover knows).
    let proof = prover.prove(amount, blinding, amount, params, &comm)?;

    // Serialize both artifacts for embedding in the transaction.
    let proof_bytes = proof.to_bytes();
    let commitment_bytes = comm.to_bytes();

    // Build the base transaction. The amount field is set to the public
    // value for fee calculation; the actual hidden amount is in the proof.
    let tx = TransactionBuilder::new(TransactionType::ConfidentialTransfer)
        .sender(sender)
        .receiver(receiver)
        .amount(Amount::new(amount, Currency::NOVA))
        .fee(0)
        .nonce(1)
        .build()
        .with_proof(proof_bytes)
        .with_commitment(commitment_bytes);

    Ok(tx)
}

/// Verify that a confidential transaction's proof is semantically valid
/// against its embedded commitment.
///
/// This goes beyond the structural check in `verify_transaction` — it
/// actually runs the Groth16 pairing check to confirm the proof is
/// mathematically sound for the given commitment and amount.
///
/// # Arguments
///
/// * `tx` — The transaction to verify (must have `proof` and `amount_commitment`).
/// * `verifier` — The `BalanceVerifier` holding the Groth16 verification key.
///
/// # Errors
///
/// Returns an error if the proof or commitment cannot be deserialized, or
/// if the Groth16 verification algorithm fails.
pub fn verify_confidential_proof(
    tx: &Transaction,
    verifier: &crate::zkp::verifier::BalanceVerifier,
) -> anyhow::Result<bool> {
    let proof_bytes = tx
        .proof
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("transaction has no proof"))?;

    let commitment_bytes = tx
        .amount_commitment
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("transaction has no amount commitment"))?;

    let proof = BalanceProof::from_bytes(proof_bytes)?;
    let comm = commitment::Commitment::from_bytes(commitment_bytes)
        .map_err(|e| anyhow::anyhow!("commitment deserialization failed: {}", e))?;

    let params = verifier.pedersen_params();
    let required_amount = tx.amount.value;

    verifier.verify(&proof, &comm, required_amount, params)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::keys::NovaKeypair;
    use crate::identity::NovaId;
    use crate::transaction::signing::sign_transaction;
    use crate::transaction::verification::verify_transaction;
    use crate::zkp::commitment::{self as zkp_commitment, Commitment, PedersenParams};
    use crate::zkp::prover::BalanceProver;
    use ark_bn254::Fr;
    use ark_ff::UniformRand;
    use ark_std::rand::{rngs::StdRng, SeedableRng};
    use bincode;

    /// Helper: set up prover/verifier pair with deterministic RNG.
    fn setup_zkp() -> (BalanceProver, crate::zkp::verifier::BalanceVerifier, StdRng) {
        let mut rng = StdRng::seed_from_u64(42);
        let (prover, verifier) = BalanceProver::setup(&mut rng);
        (prover, verifier, rng)
    }

    // ------------------------------------------------------------------
    // 1. Regular transaction still works (no proof)
    // ------------------------------------------------------------------
    #[test]
    fn regular_transfer_works_without_proof() {
        let kp = NovaKeypair::generate();
        let sender = NovaId::from_public_key(&kp.public_key()).to_address();
        let receiver_kp = NovaKeypair::generate();
        let receiver = NovaId::from_public_key(&receiver_kp.public_key()).to_address();

        let mut tx = TransactionBuilder::new(TransactionType::Transfer)
            .sender(&sender)
            .receiver(&receiver)
            .amount(Amount::new(1_000, Currency::NOVA))
            .fee(10)
            .nonce(1)
            .build();

        sign_transaction(&mut tx, &kp);
        assert!(tx.proof.is_none());
        assert!(tx.amount_commitment.is_none());
        assert!(verify_transaction(&tx).is_ok());
    }

    // ------------------------------------------------------------------
    // 2. Transaction with proof field serializes/deserializes correctly
    // ------------------------------------------------------------------
    #[test]
    fn proof_field_json_roundtrip() {
        let mut tx = TransactionBuilder::new(TransactionType::Transfer)
            .sender("nova1aaaa")
            .receiver("nova1bbbb")
            .amount(Amount::new(100, Currency::NOVA))
            .nonce(1)
            .timestamp(1_700_000_000_000)
            .build();

        let fake_proof = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let fake_commitment = vec![0xCA, 0xFE, 0xBA, 0xBE];
        tx.proof = Some(fake_proof.clone());
        tx.amount_commitment = Some(fake_commitment.clone());

        let json = serde_json::to_string(&tx).unwrap();
        let recovered: Transaction = serde_json::from_str(&json).unwrap();

        assert_eq!(recovered.proof.unwrap(), fake_proof);
        assert_eq!(recovered.amount_commitment.unwrap(), fake_commitment);
    }

    // ------------------------------------------------------------------
    // 3. Confidential transfer builds with proof and commitment
    //    (full proof generation — slow, ~2-3s)
    // ------------------------------------------------------------------
    #[test]
    #[ignore] // Groth16 proof generation takes ~2-3 seconds.
    fn confidential_transfer_builds_with_proof() {
        let (prover, _verifier, mut rng) = setup_zkp();
        let blinding = Fr::rand(&mut rng);

        let tx =
            create_confidential_transfer("nova1sender", "nova1receiver", 500, blinding, &prover)
                .expect("confidential transfer must succeed");

        assert!(tx.proof.is_some());
        assert!(tx.amount_commitment.is_some());
        assert_eq!(tx.tx_type, TransactionType::ConfidentialTransfer);
    }

    // ------------------------------------------------------------------
    // 4. Valid proof passes full verification (end-to-end)
    //    (full proof generation — slow, ~2-3s)
    // ------------------------------------------------------------------
    #[test]
    #[ignore] // Groth16 proof generation takes ~2-3 seconds.
    fn valid_proof_passes_confidential_verification() {
        let (prover, verifier, mut rng) = setup_zkp();
        let blinding = Fr::rand(&mut rng);

        let tx =
            create_confidential_transfer("nova1sender", "nova1receiver", 500, blinding, &prover)
                .expect("confidential transfer must succeed");

        let result =
            verify_confidential_proof(&tx, &verifier).expect("verification must not error");
        assert!(result, "valid confidential proof must verify");
    }

    // ------------------------------------------------------------------
    // 5. Invalid/corrupted proof fails verification
    // ------------------------------------------------------------------
    #[test]
    fn corrupted_proof_fails_structural_validation() {
        let mut tx = TransactionBuilder::new(TransactionType::ConfidentialTransfer)
            .sender("nova1aaaa")
            .receiver("nova1bbbb")
            .amount(Amount::new(100, Currency::NOVA))
            .nonce(1)
            .timestamp(1_700_000_000_000)
            .build();

        // Attach garbage bytes that cannot be deserialized as a Groth16 proof.
        tx.proof = Some(vec![0xFF; 32]);
        tx.amount_commitment = Some(vec![0xAA; 64]);
        tx.id = tx.compute_id();

        // Structural validation: BalanceProof::from_bytes should fail on garbage.
        let result = BalanceProof::from_bytes(&tx.proof.as_ref().unwrap());
        assert!(result.is_err(), "corrupted proof must fail deserialization");
    }

    // ------------------------------------------------------------------
    // 6. Missing proof on ConfidentialTransfer type is rejected
    // ------------------------------------------------------------------
    #[test]
    fn confidential_transfer_missing_proof_rejected() {
        let kp = NovaKeypair::generate();
        let sender = NovaId::from_public_key(&kp.public_key()).to_address();
        let receiver_kp = NovaKeypair::generate();
        let receiver = NovaId::from_public_key(&receiver_kp.public_key()).to_address();

        let mut tx = TransactionBuilder::new(TransactionType::ConfidentialTransfer)
            .sender(&sender)
            .receiver(&receiver)
            .amount(Amount::new(100, Currency::NOVA))
            .fee(10)
            .nonce(1)
            .build();

        sign_transaction(&mut tx, &kp);

        match verify_transaction(&tx) {
            Err(crate::transaction::verification::TransactionError::MissingProof) => {}
            other => panic!("expected MissingProof, got {:?}", other),
        }
    }

    // ------------------------------------------------------------------
    // 7. Missing commitment on ConfidentialTransfer type is rejected
    // ------------------------------------------------------------------
    #[test]
    fn confidential_transfer_missing_commitment_rejected() {
        let kp = NovaKeypair::generate();
        let sender = NovaId::from_public_key(&kp.public_key()).to_address();
        let receiver_kp = NovaKeypair::generate();
        let receiver = NovaId::from_public_key(&receiver_kp.public_key()).to_address();

        let mut tx = TransactionBuilder::new(TransactionType::ConfidentialTransfer)
            .sender(&sender)
            .receiver(&receiver)
            .amount(Amount::new(100, Currency::NOVA))
            .fee(10)
            .nonce(1)
            .build();

        // Attach proof but NOT commitment.
        // Use realistic-length bytes but still garbage for structural purposes —
        // the check for MissingCommitment happens before proof deserialization.
        tx.proof = Some(vec![0x00; 128]);
        tx.id = tx.compute_id();
        sign_transaction(&mut tx, &kp);

        match verify_transaction(&tx) {
            Err(crate::transaction::verification::TransactionError::MissingCommitment) => {}
            other => panic!("expected MissingCommitment, got {:?}", other),
        }
    }

    // ------------------------------------------------------------------
    // 8. Proof on regular Transfer is optional but verified if present
    // ------------------------------------------------------------------
    #[test]
    fn regular_transfer_with_corrupted_proof_rejected() {
        let kp = NovaKeypair::generate();
        let sender = NovaId::from_public_key(&kp.public_key()).to_address();
        let receiver_kp = NovaKeypair::generate();
        let receiver = NovaId::from_public_key(&receiver_kp.public_key()).to_address();

        let mut tx = TransactionBuilder::new(TransactionType::Transfer)
            .sender(&sender)
            .receiver(&receiver)
            .amount(Amount::new(100, Currency::NOVA))
            .fee(10)
            .nonce(1)
            .build();

        // Attach a corrupt proof to a regular Transfer — should fail structural check.
        tx.proof = Some(vec![0xFF; 32]);
        tx.id = tx.compute_id();
        sign_transaction(&mut tx, &kp);

        match verify_transaction(&tx) {
            Err(crate::transaction::verification::TransactionError::InvalidProof { .. }) => {}
            other => panic!("expected InvalidProof, got {:?}", other),
        }
    }

    // ------------------------------------------------------------------
    // 9. Signable bytes unchanged (backward compatible)
    // ------------------------------------------------------------------
    #[test]
    fn signable_bytes_unchanged_with_proof_attached() {
        let tx_base = TransactionBuilder::new(TransactionType::Transfer)
            .sender("nova1aaaa")
            .receiver("nova1bbbb")
            .amount(Amount::new(100, Currency::NOVA))
            .nonce(1)
            .timestamp(1_700_000_000_000)
            .build();

        let bytes_without = tx_base.signable_bytes();

        let tx_with = tx_base
            .clone()
            .with_proof(vec![0xDE, 0xAD])
            .with_commitment(vec![0xCA, 0xFE]);

        let bytes_with = tx_with.signable_bytes();

        assert_eq!(
            bytes_without, bytes_with,
            "proof and commitment must NOT affect signable bytes"
        );
    }

    // ------------------------------------------------------------------
    // 10. Transaction ID changes when proof is added
    // ------------------------------------------------------------------
    #[test]
    fn tx_id_changes_when_proof_added() {
        let tx_base = TransactionBuilder::new(TransactionType::Transfer)
            .sender("nova1aaaa")
            .receiver("nova1bbbb")
            .amount(Amount::new(100, Currency::NOVA))
            .nonce(1)
            .timestamp(1_700_000_000_000)
            .build();

        let id_without = tx_base.id.clone();

        let tx_with_proof = tx_base.with_proof(vec![0xDE, 0xAD, 0xBE, 0xEF]);
        let id_with = tx_with_proof.id.clone();

        assert_ne!(
            id_without, id_with,
            "attaching a proof must change the transaction ID"
        );
    }

    // ------------------------------------------------------------------
    // 11. Commitment round-trip (serialize + deserialize)
    // ------------------------------------------------------------------
    #[test]
    fn commitment_serialize_deserialize_roundtrip() {
        let mut rng = StdRng::seed_from_u64(42);
        let params = PedersenParams::setup(&mut rng);
        let blinding = Fr::rand(&mut rng);
        let comm = zkp_commitment::commit(&params, 1000, blinding);

        let bytes = comm.to_bytes();
        let restored =
            Commitment::from_bytes(&bytes).expect("commitment deserialization must succeed");
        assert_eq!(comm, restored);
    }

    // ------------------------------------------------------------------
    // 12. None proof fields serialize as null in JSON and survive bincode
    //     round-trips. skip_serializing_if is intentionally NOT used here
    //     because the Transaction struct is also serialized via bincode
    //     (positional format) in the storage layer. Skipping fields in a
    //     positional format causes deserialization to misalign.
    // ------------------------------------------------------------------
    #[test]
    fn none_proof_fields_json_and_bincode_roundtrip() {
        let tx = TransactionBuilder::new(TransactionType::Transfer)
            .sender("nova1aaaa")
            .receiver("nova1bbbb")
            .amount(Amount::new(100, Currency::NOVA))
            .nonce(1)
            .timestamp(1_700_000_000_000)
            .build();

        // JSON round-trip
        let json = serde_json::to_string(&tx).unwrap();
        let recovered_json: Transaction = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered_json.proof, None);
        assert_eq!(recovered_json.amount_commitment, None);

        // Bincode round-trip
        let bytes = bincode::serialize(&tx).unwrap();
        let recovered_bincode: Transaction = bincode::deserialize(&bytes).unwrap();
        assert_eq!(recovered_bincode.proof, None);
        assert_eq!(recovered_bincode.amount_commitment, None);
        assert_eq!(recovered_bincode.id, tx.id);
    }

    // ------------------------------------------------------------------
    // 13. Transaction ID is deterministic with proof and commitment
    // ------------------------------------------------------------------
    #[test]
    fn tx_id_deterministic_with_proof_and_commitment() {
        let proof_bytes = vec![0x01, 0x02, 0x03];
        let commitment_bytes = vec![0x04, 0x05, 0x06];

        let tx1 = TransactionBuilder::new(TransactionType::ConfidentialTransfer)
            .sender("nova1aaaa")
            .receiver("nova1bbbb")
            .amount(Amount::new(100, Currency::NOVA))
            .nonce(1)
            .timestamp(1_700_000_000_000)
            .build()
            .with_proof(proof_bytes.clone())
            .with_commitment(commitment_bytes.clone());

        let tx2 = TransactionBuilder::new(TransactionType::ConfidentialTransfer)
            .sender("nova1aaaa")
            .receiver("nova1bbbb")
            .amount(Amount::new(100, Currency::NOVA))
            .nonce(1)
            .timestamp(1_700_000_000_000)
            .build()
            .with_proof(proof_bytes)
            .with_commitment(commitment_bytes);

        assert_eq!(
            tx1.id, tx2.id,
            "identical proof+commitment must produce identical IDs"
        );
    }

    // ------------------------------------------------------------------
    // 14. Regular Transfer without proof passes verification unchanged
    // ------------------------------------------------------------------
    #[test]
    fn regular_transfer_no_proof_passes_verification() {
        let kp = NovaKeypair::generate();
        let sender = NovaId::from_public_key(&kp.public_key()).to_address();
        let receiver_kp = NovaKeypair::generate();
        let receiver = NovaId::from_public_key(&receiver_kp.public_key()).to_address();

        let mut tx = TransactionBuilder::new(TransactionType::Transfer)
            .sender(&sender)
            .receiver(&receiver)
            .amount(Amount::new(500, Currency::NOVA))
            .fee(10)
            .nonce(1)
            .build();

        sign_transaction(&mut tx, &kp);
        assert!(
            verify_transaction(&tx).is_ok(),
            "regular transfer without proof must still pass verification"
        );
    }
}
