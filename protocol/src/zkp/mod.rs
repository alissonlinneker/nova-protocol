//! # Zero-Knowledge Proof Module
//!
//! Implements the ZKP subsystem for NOVA private transactions using Groth16
//! over the BN254 curve. The core primitive is a balance proof: given a
//! Pedersen commitment `C = balance * G + r * H`, the prover demonstrates
//! knowledge of `(balance, r)` such that the commitment opens correctly
//! **and** `balance >= required_amount`, without revealing either witness.
//!
//! ## Architecture
//!
//! ```text
//! commitment.rs   — Pedersen commitment scheme (setup, commit, verify)
//! circuit.rs      — R1CS arithmetic circuit (BalanceProofCircuit)
//! prover.rs       — Groth16 proof generation (BalanceProver, BalanceProof)
//! verifier.rs     — Groth16 proof verification (BalanceVerifier)
//! ```
//!
//! ## Security Model
//!
//! - **Commitment hiding**: information-theoretically hiding under DLOG.
//! - **Commitment binding**: computationally binding under DLOG on BN254/G1.
//! - **Soundness**: Groth16 knowledge-soundness in the generic group model.
//! - **Range check**: bit-decomposition to 64 bits with boolean enforcement
//!   on every limb — no overflow, no wrap-around.
//!
//! The trusted setup is per-circuit. In production, replace the local
//! ceremony with an MPC-generated SRS (see `prover::BalanceProver::setup`).

pub mod circuit;
pub mod commitment;
pub mod prover;
pub mod verifier;

// Re-export the public API so callers can do `use nova_protocol::zkp::*`.
pub use circuit::BalanceProofCircuit;
pub use commitment::{Commitment, PedersenParams};
pub use prover::{BalanceProof, BalanceProver};
pub use verifier::BalanceVerifier;

/// Number of bits used for range proofs. 64 bits covers the full u64 domain,
/// which is more than sufficient for any sane monetary denomination.
pub const RANGE_BITS: usize = 64;

#[cfg(test)]
mod tests {
    use super::*;
    use ark_bn254::Fr;
    use ark_ff::UniformRand;
    use ark_std::rand::{rngs::StdRng, SeedableRng};

    /// End-to-end: setup -> commit -> prove -> verify.
    #[test]
    fn end_to_end_balance_proof() {
        let mut rng = StdRng::seed_from_u64(42);

        // 1. Trusted setup (generates Pedersen params + Groth16 CRS)
        let (prover, verifier) = BalanceProver::setup(&mut rng);
        let params = prover.pedersen_params();

        // 2. Prover knows: balance = 1000, blinding = random scalar
        let balance: u64 = 1000;
        let blinding = Fr::rand(&mut rng);
        let comm = commitment::commit(params, balance, blinding);

        // 3. They want to prove balance >= 200
        let required_amount: u64 = 200;

        // 4. Generate proof
        let proof = prover
            .prove(balance, blinding, required_amount, params, &comm)
            .expect("proof generation must succeed");

        // 5. Verify
        let ok = verifier
            .verify(&proof, &comm, required_amount, params)
            .expect("verification must not error");
        assert!(ok, "valid proof must verify");
    }

    /// A proof for an insufficient balance must not verify.
    #[test]
    fn insufficient_balance_rejected() {
        let mut rng = StdRng::seed_from_u64(42);

        let (prover, _verifier) = BalanceProver::setup(&mut rng);
        let params = prover.pedersen_params();

        let balance: u64 = 50;
        let blinding = Fr::rand(&mut rng);
        let comm = commitment::commit(params, balance, blinding);

        // Try to prove balance >= 100 — this is false.
        let required_amount: u64 = 100;

        // ark-groth16 0.4.0 panics (prover.rs:197) when the constraint
        // system is unsatisfiable instead of returning Err. Wrap in
        // catch_unwind so the test handles both a panic and an Err.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            prover.prove(balance, blinding, required_amount, params, &comm)
        }));
        assert!(
            result.is_err() || result.unwrap().is_err(),
            "proof for insufficient balance must fail",
        );
    }

    /// Commitment round-trip: commit then verify opening.
    #[test]
    fn commitment_open_verify() {
        let mut rng = StdRng::seed_from_u64(42);
        let params = PedersenParams::setup(&mut rng);

        let value: u64 = 42;
        let blinding = Fr::rand(&mut rng);
        let c = commitment::commit(&params, value, blinding);

        assert!(
            commitment::verify_commitment(&params, &c, value, blinding),
            "valid opening must verify"
        );
        assert!(
            !commitment::verify_commitment(&params, &c, value + 1, blinding),
            "wrong value must fail"
        );
    }

    /// Proof serialization round-trip.
    #[test]
    fn proof_serialization_round_trip() {
        let mut rng = StdRng::seed_from_u64(42);

        let (prover, verifier) = BalanceProver::setup(&mut rng);
        let params = prover.pedersen_params();

        let balance: u64 = 500;
        let blinding = Fr::rand(&mut rng);
        let comm = commitment::commit(params, balance, blinding);

        let proof = prover
            .prove(balance, blinding, 100, params, &comm)
            .expect("proof generation");

        let bytes = proof.to_bytes();
        let restored = BalanceProof::from_bytes(&bytes).expect("deserialization");

        let ok = verifier
            .verify(&restored, &comm, 100, params)
            .expect("verification");
        assert!(ok, "deserialized proof must still verify");
    }
}
