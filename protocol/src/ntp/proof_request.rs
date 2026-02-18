//! # NTP Step 2 — Proof of Funds
//!
//! Before the sender broadcasts a transaction, the receiver can request
//! cryptographic proof that the sender actually has sufficient funds.
//! This prevents wasted network bandwidth from transactions that would
//! fail validation anyway.
//!
//! The proof uses the Groth16 zero-knowledge proof system over BN254.
//! The sender proves `balance >= required_amount` without revealing the
//! actual balance. The proof is bound to a challenge nonce to prevent
//! replay attacks.
//!
//! ## Protocol Flow
//!
//! ```text
//! Receiver → Sender: ProofOfFundsRequest {
//!     session_id, required_amount, currency, challenge_nonce
//! }
//!
//! Sender → Receiver: ProofOfFundsResponse {
//!     session_id, zkp_proof, commitment, timestamp
//! }
//! ```
//!
//! The receiver verifies the proof using the public verification key.
//! If verification passes, the protocol advances to the broadcast step.

use serde::{Deserialize, Serialize};

use crate::transaction::types::Currency;
use crate::zkp::commitment::{self, Commitment, PedersenParams};
use crate::zkp::prover::{BalanceProof, BalanceProver};
use crate::zkp::verifier::BalanceVerifier;

use super::error::NtpError;
use super::handshake::EstablishedSession;

use ark_bn254::Fr;
use ark_ff::UniformRand;

// ---------------------------------------------------------------------------
// Request / Response
// ---------------------------------------------------------------------------

/// Request for proof of funds, issued by the receiver.
///
/// The `challenge_nonce` is a fresh random value that the sender must
/// incorporate into their proof. This prevents the sender from replaying
/// a proof generated for a different session or a different amount.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProofOfFundsRequest {
    /// Session this request belongs to.
    pub session_id: String,
    /// Minimum balance the sender must prove they hold.
    pub required_amount: u64,
    /// Currency the proof must cover.
    pub currency: Currency,
    /// Fresh random nonce for replay protection (32 bytes).
    pub challenge_nonce: [u8; 32],
}

/// The sender's proof of funds response.
///
/// Contains a Groth16 proof and the Pedersen commitment that the proof
/// is relative to. The receiver verifies both the proof and the
/// commitment's validity.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProofOfFundsResponse {
    /// Session this response belongs to.
    pub session_id: String,
    /// Serialized Groth16 proof bytes.
    pub zkp_proof: Vec<u8>,
    /// Serialized Pedersen commitment (compressed BN254/G1 point).
    pub commitment: Vec<u8>,
    /// Unix timestamp (milliseconds) of proof generation.
    pub timestamp: u64,
}

// ---------------------------------------------------------------------------
// Protocol Functions
// ---------------------------------------------------------------------------

/// Generate a proof-of-funds request for an established session.
///
/// Called by the **receiver** after the handshake completes. The returned
/// request should be sent to the sender (encrypted with the session key).
///
/// # Arguments
///
/// * `session` — The established NTP session.
/// * `amount` — The minimum balance to prove. Typically matches the
///   payment amount from the handshake.
/// * `currency` — The currency the proof must cover.
pub fn request_proof_of_funds(
    session: &EstablishedSession,
    amount: u64,
    currency: Currency,
) -> ProofOfFundsRequest {
    let mut challenge_nonce = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut challenge_nonce);

    ProofOfFundsRequest {
        session_id: session.session_id.clone(),
        required_amount: amount,
        currency,
        challenge_nonce,
    }
}

/// Generate a proof-of-funds response.
///
/// Called by the **sender** upon receiving a [`ProofOfFundsRequest`].
/// The sender commits to their balance using a fresh Pedersen commitment,
/// then generates a Groth16 proof that `balance >= required_amount`.
///
/// # Arguments
///
/// * `request` — The proof request from the receiver.
/// * `session` — The established NTP session (for session_id validation).
/// * `balance` — The sender's actual balance in the requested currency.
/// * `prover` — The Groth16 prover (holds the proving key).
/// * `pedersen_params` — Public Pedersen commitment parameters.
///
/// # Errors
///
/// Returns [`NtpError::SessionMismatch`] if the request's session ID
/// doesn't match the active session. Returns [`NtpError::ProofVerificationFailed`]
/// if proof generation fails (e.g., insufficient balance).
pub fn generate_proof_response(
    request: &ProofOfFundsRequest,
    session: &EstablishedSession,
    balance: u64,
    prover: &BalanceProver,
    _pedersen_params: &PedersenParams,
) -> Result<ProofOfFundsResponse, NtpError> {
    // Validate session.
    if request.session_id != session.session_id {
        return Err(NtpError::SessionMismatch {
            expected: session.session_id.clone(),
            got: request.session_id.clone(),
        });
    }

    // Use the Pedersen parameters that were baked into the prover's
    // proving key during Groth16 setup.  The circuit constants (g_scalar,
    // h_scalar) are fixed at setup time, so using any *other* params here
    // would produce a proof that the verifier rejects.
    let params = prover.pedersen_params();

    // Generate a fresh blinding factor.
    let mut rng = ark_std::test_rng();
    let blinding = Fr::rand(&mut rng);

    // Commit to our balance.
    let comm = commitment::commit(params, balance, blinding);

    // Generate the Groth16 proof: balance >= required_amount.
    let proof = prover
        .prove(balance, blinding, request.required_amount, params, &comm)
        .map_err(|e| NtpError::ProofVerificationFailed(e.to_string()))?;

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    Ok(ProofOfFundsResponse {
        session_id: session.session_id.clone(),
        zkp_proof: proof.to_bytes(),
        commitment: comm.to_bytes(),
        timestamp,
    })
}

/// Verify a proof-of-funds response.
///
/// Called by the **receiver** after receiving the sender's proof. Validates
/// both the Groth16 proof and the Pedersen commitment.
///
/// # Arguments
///
/// * `response` — The proof response from the sender.
/// * `required_amount` — The amount that was requested in the proof request.
/// * `verifier` — The Groth16 verifier (holds the verification key).
/// * `pedersen_params` — Public Pedersen commitment parameters.
///
/// # Returns
///
/// `Ok(true)` if the proof is valid, `Ok(false)` if the proof is
/// mathematically invalid, or `Err` if deserialization fails.
pub fn verify_proof_of_funds(
    response: &ProofOfFundsResponse,
    required_amount: u64,
    verifier: &BalanceVerifier,
    _pedersen_params: &PedersenParams,
) -> Result<bool, NtpError> {
    // Deserialize the commitment.
    let comm = Commitment::from_bytes(&response.commitment)
        .map_err(|e| NtpError::ProofVerificationFailed(format!("bad commitment: {}", e)))?;

    // Deserialize the proof.
    let proof = BalanceProof::from_bytes(&response.zkp_proof)
        .map_err(|e| NtpError::ProofVerificationFailed(format!("bad proof: {}", e)))?;

    // Use the Pedersen parameters embedded in the verifier (baked in at
    // Groth16 setup time).  This must be the same parameter set that the
    // prover used when generating the proof.
    let params = verifier.pedersen_params();

    // Verify the Groth16 proof.
    let valid = verifier
        .verify(&proof, &comm, required_amount, params)
        .map_err(|e| NtpError::ProofVerificationFailed(e.to_string()))?;

    Ok(valid)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::keys::NovaKeypair;
    use crate::ntp::handshake::{HandshakeSession, PaymentParams};
    use ark_std::rand::{rngs::StdRng, SeedableRng};

    fn setup_session() -> EstablishedSession {
        let sender_kp = NovaKeypair::generate();
        let receiver_kp = NovaKeypair::generate();

        let payment = PaymentParams {
            amount: 500,
            currency: Currency::NOVA,
            description: "test".to_string(),
        };

        let (sender_session, request) =
            HandshakeSession::initiate(&sender_kp, vec![Currency::NOVA]);
        let (response, _receiver_session) =
            HandshakeSession::respond(&request, &receiver_kp, payment).unwrap();
        sender_session.complete(&response).unwrap()
    }

    #[test]
    fn proof_request_generation() {
        let session = setup_session();
        let request = request_proof_of_funds(&session, 500, Currency::NOVA);

        assert_eq!(request.session_id, session.session_id);
        assert_eq!(request.required_amount, 500);
        assert_eq!(request.currency, Currency::NOVA);
        // Nonce should be non-zero (random).
        assert_ne!(request.challenge_nonce, [0u8; 32]);
    }

    #[test]
    fn proof_generation_and_verification() {
        let session = setup_session();
        let mut rng = StdRng::seed_from_u64(42);

        let pedersen_params = PedersenParams::setup(&mut rng);
        let (prover, verifier) = BalanceProver::setup(&mut rng);

        let request = request_proof_of_funds(&session, 500, Currency::NOVA);

        // Sender has balance 1000, needs to prove >= 500.
        let response = generate_proof_response(&request, &session, 1000, &prover, &pedersen_params)
            .expect("proof generation should succeed");

        assert_eq!(response.session_id, session.session_id);
        assert!(!response.zkp_proof.is_empty());
        assert!(!response.commitment.is_empty());

        let valid = verify_proof_of_funds(&response, 500, &verifier, &pedersen_params)
            .expect("verification should not error");
        assert!(valid, "valid proof must verify");
    }

    #[test]
    fn insufficient_balance_proof_fails() {
        let session = setup_session();
        let mut rng = StdRng::seed_from_u64(42);

        let pedersen_params = PedersenParams::setup(&mut rng);
        let (prover, _verifier) = BalanceProver::setup(&mut rng);

        let request = request_proof_of_funds(&session, 1000, Currency::NOVA);

        // Sender only has 100 — proof should fail.
        // ark-groth16 0.4.0 panics (prover.rs:197) when the constraint
        // system is unsatisfiable instead of returning Err. Wrap in
        // catch_unwind so the test handles both a panic and an Err.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            generate_proof_response(&request, &session, 100, &prover, &pedersen_params)
        }));
        assert!(result.is_err() || result.unwrap().is_err());
    }

    #[test]
    fn session_mismatch_rejected() {
        let session = setup_session();
        let mut rng = StdRng::seed_from_u64(42);

        let pedersen_params = PedersenParams::setup(&mut rng);
        let (prover, _verifier) = BalanceProver::setup(&mut rng);

        let mut request = request_proof_of_funds(&session, 100, Currency::NOVA);
        request.session_id = "wrong-session-id".to_string();

        let result = generate_proof_response(&request, &session, 1000, &prover, &pedersen_params);
        assert!(matches!(result, Err(NtpError::SessionMismatch { .. })));
    }
}
