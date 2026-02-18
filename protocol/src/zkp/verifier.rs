//! # Groth16 Proof Verification
//!
//! The verifier side of the balance proof. In the NOVA network, every
//! validator holds a copy of the [`BalanceVerifier`] (i.e., the Groth16
//! verification key) and checks incoming transaction proofs before accepting
//! them into the mempool.
//!
//! Groth16 verification is three pairings + a multi-scalar multiplication,
//! so it runs in constant time regardless of circuit size — well under 5ms
//! on commodity hardware.

use anyhow::{Context, Result};
use ark_bn254::Bn254;
use ark_groth16::{Groth16, VerifyingKey};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use ark_snark::SNARK;

use super::circuit;
use super::commitment::{Commitment, PedersenParams};
use super::prover::BalanceProof;

// ---------------------------------------------------------------------------
// BalanceVerifier
// ---------------------------------------------------------------------------

/// Holds the Groth16 verification key for the balance-proof circuit.
///
/// This is small (~1 KB) and can be freely distributed to all validators.
/// Verification is a constant-time operation dominated by pairing checks.
pub struct BalanceVerifier {
    vk: VerifyingKey<Bn254>,
    /// The Pedersen parameters baked into the circuit at setup time.
    /// Needed to reconstruct the public input vector during verification.
    params: PedersenParams,
}

impl BalanceVerifier {
    /// Construct from an arkworks verification key (called by `BalanceProver::setup`).
    pub(crate) fn from_vk(vk: VerifyingKey<Bn254>, params: PedersenParams) -> Self {
        Self { vk, params }
    }

    /// Return a reference to the Pedersen parameters embedded in this verifier.
    pub fn pedersen_params(&self) -> &PedersenParams {
        &self.params
    }

    /// Verify a balance proof against the given public inputs.
    ///
    /// # Arguments
    ///
    /// * `proof` — The serialized Groth16 proof.
    /// * `commitment` — The Pedersen commitment the proof is bound to.
    /// * `required_amount` — The minimum balance threshold being proved.
    /// * `params` — The Pedersen parameters (must match setup).
    ///
    /// # Returns
    ///
    /// `Ok(true)` if the proof verifies, `Ok(false)` if it does not, or
    /// `Err(...)` if deserialization or the verification algorithm itself fails.
    pub fn verify(
        &self,
        proof: &BalanceProof,
        commitment: &Commitment,
        required_amount: u64,
        _params: &PedersenParams,
    ) -> Result<bool> {
        let ark_proof = proof
            .to_ark_proof()
            .context("failed to deserialize proof")?;

        let public_inputs = circuit::public_inputs(commitment, required_amount);

        let valid = Groth16::<Bn254>::verify(&self.vk, &public_inputs, &ark_proof)
            .context("Groth16 verification algorithm failed")?;

        Ok(valid)
    }

    /// Serialize the verification key to bytes (for persistent storage or
    /// distribution to validators).
    pub fn vk_to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        self.vk
            .serialize_compressed(&mut buf)
            .expect("VK serialization must not fail");
        buf
    }

    /// Deserialize a verification key from bytes.
    pub fn vk_from_bytes(data: &[u8], params: PedersenParams) -> Result<Self> {
        let vk = VerifyingKey::<Bn254>::deserialize_compressed(&data[..])
            .context("failed to deserialize verification key")?;
        Ok(Self { vk, params })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zkp::commitment;
    use crate::zkp::prover::BalanceProver;
    use ark_bn254::Fr;
    use ark_ff::UniformRand;
    use ark_std::rand::{rngs::StdRng, SeedableRng};

    #[test]
    fn verify_valid_proof() {
        let mut rng = StdRng::seed_from_u64(42);
        let (prover, verifier) = BalanceProver::setup(&mut rng);
        let params = prover.pedersen_params();

        let balance = 1000u64;
        let blinding = Fr::rand(&mut rng);
        let c = commitment::commit(params, balance, blinding);

        let proof = prover.prove(balance, blinding, 200, params, &c).unwrap();

        let result = verifier.verify(&proof, &c, 200, params).unwrap();
        assert!(result, "valid proof must verify");
    }

    #[test]
    fn reject_wrong_amount() {
        let mut rng = StdRng::seed_from_u64(42);
        let (prover, verifier) = BalanceProver::setup(&mut rng);
        let params = prover.pedersen_params();

        let balance = 1000u64;
        let blinding = Fr::rand(&mut rng);
        let c = commitment::commit(params, balance, blinding);

        // Prove for amount = 200
        let proof = prover.prove(balance, blinding, 200, params, &c).unwrap();

        // Verify against amount = 999 — must fail
        let result = verifier.verify(&proof, &c, 999, params).unwrap();
        assert!(!result, "proof for different amount must not verify");
    }

    #[test]
    fn reject_wrong_commitment() {
        let mut rng = StdRng::seed_from_u64(42);
        let (prover, verifier) = BalanceProver::setup(&mut rng);
        let params = prover.pedersen_params();

        let balance = 1000u64;
        let blinding = Fr::rand(&mut rng);
        let c = commitment::commit(params, balance, blinding);

        let proof = prover.prove(balance, blinding, 200, params, &c).unwrap();

        // Verify against a different commitment
        let other_blinding = Fr::rand(&mut rng);
        let other_c = commitment::commit(params, 1000, other_blinding);

        let result = verifier.verify(&proof, &other_c, 200, params).unwrap();
        assert!(!result, "proof for different commitment must not verify");
    }

    #[test]
    fn verify_exact_balance() {
        let mut rng = StdRng::seed_from_u64(42);
        let (prover, verifier) = BalanceProver::setup(&mut rng);
        let params = prover.pedersen_params();

        let balance = 500u64;
        let blinding = Fr::rand(&mut rng);
        let c = commitment::commit(params, balance, blinding);

        let proof = prover.prove(balance, blinding, 500, params, &c).unwrap();
        let result = verifier.verify(&proof, &c, 500, params).unwrap();
        assert!(result, "exact balance proof must verify");
    }

    #[test]
    fn vk_serialization_round_trip() {
        let mut rng = StdRng::seed_from_u64(42);
        let (prover, verifier) = BalanceProver::setup(&mut rng);
        let params = prover.pedersen_params().clone();

        let bytes = verifier.vk_to_bytes();
        let restored = BalanceVerifier::vk_from_bytes(&bytes, params.clone()).unwrap();

        // Verify a proof with the restored verifier.
        let balance = 100u64;
        let blinding = Fr::rand(&mut rng);
        let c = commitment::commit(&params, balance, blinding);
        let proof = prover.prove(balance, blinding, 50, &params, &c).unwrap();

        let ok = restored.verify(&proof, &c, 50, &params).unwrap();
        assert!(ok, "restored VK must verify valid proofs");
    }
}
