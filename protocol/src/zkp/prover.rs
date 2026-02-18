//! # Groth16 Proof Generation
//!
//! This module wraps `ark-groth16` to provide a high-level API for generating
//! balance proofs. The workflow is:
//!
//! 1. **Setup**: Run `BalanceProver::setup(rng)` once per circuit shape.
//!    This produces a proving key and a verification key (returned as
//!    `BalanceVerifier`). In production, replace this with an MPC ceremony.
//!
//! 2. **Prove**: Call `BalanceProver::prove(...)` with the private witness
//!    and public parameters. Internally this populates a
//!    [`BalanceProofCircuit`] and invokes `Groth16::prove`.
//!
//! 3. The resulting [`BalanceProof`] is a compact (~192 bytes) serializable
//!    blob that can be attached to a transaction.

use anyhow::{Context, Result};
use ark_bn254::{Bn254, Fr};
use ark_groth16::{Groth16, ProvingKey};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use ark_snark::SNARK;
use ark_std::rand::{CryptoRng, Rng};

use super::circuit::BalanceProofCircuit;
use super::commitment::{Commitment, PedersenParams};
use super::verifier::BalanceVerifier;

// ---------------------------------------------------------------------------
// BalanceProver
// ---------------------------------------------------------------------------

/// Holds the Groth16 proving key for the balance-proof circuit.
///
/// Instances are created via [`BalanceProver::setup`] and should be kept
/// in memory for the lifetime of the node (they are large but immutable).
pub struct BalanceProver {
    pk: ProvingKey<Bn254>,
    /// The Pedersen parameters used during setup. The scalar generators
    /// are baked into the circuit as constants, so the proving key is
    /// only valid for this specific parameter set.
    params: PedersenParams,
}

impl BalanceProver {
    /// Run the Groth16 trusted setup for the balance-proof circuit.
    ///
    /// Returns both the prover and verifier halves. The verifier should be
    /// distributed to all validators; the prover is kept by the wallet/client.
    ///
    /// # Panics
    ///
    /// Panics if CRS generation fails (indicates a bug in the circuit).
    pub fn setup<R: Rng + CryptoRng>(rng: &mut R) -> (Self, BalanceVerifier) {
        // Generate Pedersen parameters. The scalar generators are embedded
        // as constants in the constraint system, so the CRS is bound to
        // this specific parameter set.
        let params = PedersenParams::setup(rng);

        let blank_circuit = BalanceProofCircuit::blank(&params);

        let (pk, vk) = Groth16::<Bn254>::circuit_specific_setup(blank_circuit, rng)
            .expect("Groth16 setup must succeed for a well-formed circuit");

        let prover = Self {
            pk,
            params: params.clone(),
        };
        let verifier = BalanceVerifier::from_vk(vk, params);

        (prover, verifier)
    }

    /// Return a reference to the Pedersen parameters embedded in this prover.
    pub fn pedersen_params(&self) -> &PedersenParams {
        &self.params
    }

    /// Generate a Groth16 proof for the balance statement.
    ///
    /// # Arguments
    ///
    /// * `balance` — The prover's actual balance (private).
    /// * `blinding` — The blinding factor used in the commitment (private).
    /// * `required_amount` — The minimum balance being proved (public).
    /// * `params` — The Pedersen parameters (must match setup).
    /// * `commitment` — The commitment (public).
    ///
    /// # Errors
    ///
    /// Returns an error if the witness does not satisfy the circuit (e.g.,
    /// balance < required_amount) or if serialization fails.
    pub fn prove(
        &self,
        balance: u64,
        blinding: Fr,
        required_amount: u64,
        params: &PedersenParams,
        commitment: &Commitment,
    ) -> Result<BalanceProof> {
        let circuit =
            BalanceProofCircuit::new(params, balance, blinding, commitment, required_amount);

        let mut rng = ark_std::rand::thread_rng();

        let proof = Groth16::<Bn254>::prove(&self.pk, circuit, &mut rng)
            .context("Groth16 proof generation failed (witness likely unsatisfiable)")?;

        let mut proof_bytes = Vec::new();
        proof
            .serialize_compressed(&mut proof_bytes)
            .context("proof serialization failed")?;

        Ok(BalanceProof { bytes: proof_bytes })
    }
}

// ---------------------------------------------------------------------------
// BalanceProof
// ---------------------------------------------------------------------------

/// A serialized Groth16 proof for the balance statement.
///
/// This is the artifact that gets attached to a NOVA transaction and
/// broadcast to the network. Validators deserialize it and pass it to
/// [`BalanceVerifier::verify`].
#[derive(Clone, Debug)]
pub struct BalanceProof {
    bytes: Vec<u8>,
}

impl BalanceProof {
    /// Raw compressed proof bytes (suitable for on-chain storage).
    pub fn to_bytes(&self) -> Vec<u8> {
        self.bytes.clone()
    }

    /// Reconstruct a proof from compressed bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        // Validate that the bytes actually decode to a valid Groth16 proof.
        let _proof = ark_groth16::Proof::<Bn254>::deserialize_compressed(data)
            .context("invalid Groth16 proof bytes")?;

        Ok(Self {
            bytes: data.to_vec(),
        })
    }

    /// Deserialize into the arkworks proof struct (used internally by the verifier).
    pub(crate) fn to_ark_proof(&self) -> Result<ark_groth16::Proof<Bn254>> {
        ark_groth16::Proof::<Bn254>::deserialize_compressed(&self.bytes[..])
            .map_err(|e| anyhow::anyhow!("proof deserialization failed: {}", e))
    }

    /// Size of the proof in bytes.
    pub fn size(&self) -> usize {
        self.bytes.len()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zkp::commitment;
    use ark_ff::UniformRand;
    use ark_std::rand::{rngs::StdRng, SeedableRng};

    #[test]
    fn prove_valid_balance() {
        let mut rng = StdRng::seed_from_u64(42);
        let (prover, _verifier) = BalanceProver::setup(&mut rng);
        let params = prover.pedersen_params();

        let balance = 1_000u64;
        let blinding = Fr::rand(&mut rng);
        let c = commitment::commit(params, balance, blinding);

        let proof = prover.prove(balance, blinding, 500, params, &c);
        assert!(proof.is_ok(), "valid witness must produce a proof");

        // Groth16 proofs on BN254 are ~192 bytes compressed.
        let proof = proof.unwrap();
        assert!(proof.size() > 100, "proof should be non-trivial in size");
        assert!(proof.size() < 400, "proof should be compact");
    }

    #[test]
    fn prove_insufficient_balance_fails() {
        let mut rng = StdRng::seed_from_u64(42);
        let (prover, _verifier) = BalanceProver::setup(&mut rng);
        let params = prover.pedersen_params();

        let balance = 10u64;
        let blinding = Fr::rand(&mut rng);
        let c = commitment::commit(params, balance, blinding);

        // ark-groth16 0.4.0 panics (prover.rs:197) when the constraint
        // system is unsatisfiable instead of returning Err. Wrap in
        // catch_unwind so the test handles both a panic and an Err.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            prover.prove(balance, blinding, 100, params, &c)
        }));
        assert!(
            result.is_err() || result.unwrap().is_err(),
            "insufficient balance must not produce a proof",
        );
    }

    #[test]
    fn proof_bytes_round_trip() {
        let mut rng = StdRng::seed_from_u64(42);
        let (prover, _verifier) = BalanceProver::setup(&mut rng);
        let params = prover.pedersen_params();

        let balance = 500u64;
        let blinding = Fr::rand(&mut rng);
        let c = commitment::commit(params, balance, blinding);

        let proof = prover.prove(balance, blinding, 100, params, &c).unwrap();
        let bytes = proof.to_bytes();
        let restored = BalanceProof::from_bytes(&bytes).unwrap();

        assert_eq!(proof.bytes, restored.bytes);
    }
}
