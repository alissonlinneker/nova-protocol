//! # Balance Proof R1CS Circuit
//!
//! This module defines the arithmetic circuit used inside the Groth16 SNARK.
//! The statement being proved is:
//!
//! ```text
//! "I know (balance, r) such that:
//!     1. balance * g + r * h = c          (scalar commitment correctness)
//!     2. balance >= required_amount       (solvency / range proof)"
//! ```
//!
//! ## Constraint breakdown
//!
//! ### Commitment correctness (native Fr arithmetic)
//!
//! The scalar-field Pedersen commitment `c = balance * g_scalar + r * h_scalar`
//! is verified using two multiplications and one addition in Fr. The generators
//! `g_scalar` and `h_scalar` are baked into the circuit as constants. The
//! commitment value `c` is a public input.
//!
//! This is O(1) constraints — the entire commitment check costs exactly
//! 2 R1CS constraints (one per multiplication gate).
//!
//! ### Range proof (balance >= amount)
//!
//! Let `delta = balance - required_amount`. We need `delta >= 0` in the
//! integers (not modular). We bit-decompose `delta` into [`RANGE_BITS`] (64)
//! bits and enforce:
//!
//! 1. Each bit `b_i` is boolean: `b_i * (1 - b_i) = 0`.
//! 2. `sum(b_i * 2^i) = delta`.
//!
//! Because `2^64 < |Fr|`, this decomposition is unique, so a satisfied
//! constraint system implies `0 <= delta < 2^64` in the integers, which
//! in turn implies `balance >= required_amount` (assuming both fit in u64).
//!
//! Total constraint count: ~2 (commitment) + 64 (boolean) + 1 (sum) = ~67.
//!
//! ## Public inputs (in order)
//!
//! | index | value |
//! |-------|-------|
//! | 0     | scalar commitment `c` (Fr element) |
//! | 1     | Fr::from(required_amount) |

use ark_bn254::Fr;
use ark_ff::PrimeField;
use ark_r1cs_std::{
    alloc::AllocVar,
    boolean::Boolean,
    eq::EqGadget,
    fields::{fp::FpVar, FieldVar},
};
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};

use super::commitment::PedersenParams;
use super::RANGE_BITS;

// ---------------------------------------------------------------------------
// Circuit definition
// ---------------------------------------------------------------------------

/// Groth16 R1CS circuit proving balance solvency against a Pedersen commitment.
///
/// All fields are `Option<_>` so the struct can be constructed with `None`
/// values during Groth16 key generation (where the constraint topology is
/// determined but no witness is available yet).
#[derive(Clone)]
pub struct BalanceProofCircuit {
    // -- Commitment parameters (constants baked into the circuit) ------------
    /// Scalar generator g (constant in Fr).
    pub g_scalar: Fr,
    /// Scalar generator h (constant in Fr).
    pub h_scalar: Fr,

    // -- Private witness ----------------------------------------------------
    /// The actual balance (u64, encoded as Fr).
    pub balance: Option<Fr>,
    /// Blinding factor used in the commitment.
    pub blinding: Option<Fr>,

    // -- Public inputs ------------------------------------------------------
    /// Scalar commitment value: `c = balance * g_scalar + blinding * h_scalar`.
    pub commitment_scalar: Option<Fr>,
    /// Minimum amount the balance must cover.
    pub required_amount: Option<Fr>,
}

impl BalanceProofCircuit {
    /// Construct a fully-populated circuit for proof generation.
    pub fn new(
        params: &PedersenParams,
        balance: u64,
        blinding: Fr,
        commitment: &super::commitment::Commitment,
        required_amount: u64,
    ) -> Self {
        Self {
            g_scalar: params.g_scalar,
            h_scalar: params.h_scalar,
            balance: Some(Fr::from(balance)),
            blinding: Some(blinding),
            commitment_scalar: Some(commitment.scalar),
            required_amount: Some(Fr::from(required_amount)),
        }
    }

    /// Construct a blank circuit (for CRS generation). The constraint
    /// topology is identical — only the witness slots are empty.
    pub fn blank(params: &PedersenParams) -> Self {
        Self {
            g_scalar: params.g_scalar,
            h_scalar: params.h_scalar,
            balance: None,
            blinding: None,
            commitment_scalar: None,
            required_amount: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Constraint synthesizer
// ---------------------------------------------------------------------------

impl ConstraintSynthesizer<Fr> for BalanceProofCircuit {
    fn generate_constraints(self, cs: ConstraintSystemRef<Fr>) -> Result<(), SynthesisError> {
        // ===================================================================
        // 1. Allocate public inputs
        // ===================================================================

        // Scalar commitment value — public so verifier can bind the proof
        // to a specific on-chain commitment.
        let commitment_var =
            FpVar::<Fr>::new_input(ark_relations::ns!(cs, "commitment_scalar"), || {
                self.commitment_scalar
                    .ok_or(SynthesisError::AssignmentMissing)
            })?;

        // Required amount — public so the verifier knows what threshold
        // is being proved.
        let required_amount_var =
            FpVar::<Fr>::new_input(ark_relations::ns!(cs, "required_amount"), || {
                self.required_amount
                    .ok_or(SynthesisError::AssignmentMissing)
            })?;

        // ===================================================================
        // 2. Allocate private witnesses
        // ===================================================================

        let balance_var = FpVar::<Fr>::new_witness(ark_relations::ns!(cs, "balance"), || {
            self.balance.ok_or(SynthesisError::AssignmentMissing)
        })?;

        let blinding_var = FpVar::<Fr>::new_witness(ark_relations::ns!(cs, "blinding"), || {
            self.blinding.ok_or(SynthesisError::AssignmentMissing)
        })?;

        // ===================================================================
        // 3. Allocate generators as constants
        // ===================================================================

        let g_var = FpVar::<Fr>::new_constant(ark_relations::ns!(cs, "g_scalar"), self.g_scalar)?;

        let h_var = FpVar::<Fr>::new_constant(ark_relations::ns!(cs, "h_scalar"), self.h_scalar)?;

        // ===================================================================
        // 4. Commitment correctness: c == balance * g + blinding * h
        // ===================================================================

        let computed = &balance_var * &g_var + &blinding_var * &h_var;
        computed.enforce_equal(&commitment_var)?;

        // ===================================================================
        // 5. Range proof: balance >= required_amount
        //    <=> delta = balance - required_amount >= 0
        //    <=> delta in [0, 2^RANGE_BITS)
        // ===================================================================

        let delta_var = &balance_var - &required_amount_var;

        // Compute the concrete delta value for the witness assignment.
        let delta_bits = delta_to_bits(self.balance, self.required_amount);

        // Allocate each bit as a private Boolean witness, enforce
        // boolean-ness, and reconstruct the field element from bits.
        let mut reconstructed = FpVar::<Fr>::zero();
        let mut power_of_two = FpVar::<Fr>::one();
        let two = FpVar::<Fr>::constant(Fr::from(2u64));

        for i in 0..RANGE_BITS {
            let bit = Boolean::<Fr>::new_witness(ark_relations::ns!(cs, "delta_bit"), || {
                delta_bits
                    .as_ref()
                    .map(|bits| bits[i])
                    .ok_or(SynthesisError::AssignmentMissing)
            })?;

            // Accumulate: reconstructed += bit_as_field * 2^i
            reconstructed += FpVar::<Fr>::from(bit) * &power_of_two;
            power_of_two *= &two;
        }

        // Enforce: reconstructed == delta
        // This guarantees:
        //   (a) delta is non-negative (has a valid 64-bit binary decomposition)
        //   (b) delta < 2^64 (only 64 bits are used)
        reconstructed.enforce_equal(&delta_var)?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compute the little-endian bit decomposition of `balance - amount`.
/// Returns `None` if either input is `None` (key-gen mode).
fn delta_to_bits(balance: Option<Fr>, amount: Option<Fr>) -> Option<Vec<bool>> {
    let balance = balance?;
    let amount = amount?;
    let delta = balance - amount;

    let bigint = delta.into_bigint();
    let bits: Vec<bool> = bigint
        .0
        .iter()
        .flat_map(|limb| (0..64).map(move |i| (limb >> i) & 1 == 1))
        .take(RANGE_BITS)
        .collect();

    Some(bits)
}

/// Build the vector of public inputs that the Groth16 verifier expects.
///
/// The ordering MUST match `generate_constraints` — the first `new_input`
/// allocation becomes public_inputs[0], the second becomes [1], etc.
pub fn public_inputs(commitment: &super::commitment::Commitment, required_amount: u64) -> Vec<Fr> {
    vec![commitment.scalar, Fr::from(required_amount)]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zkp::commitment;
    use ark_ff::UniformRand;
    use ark_relations::r1cs::ConstraintSystem;
    use ark_std::rand::{rngs::StdRng, SeedableRng};

    #[test]
    fn circuit_satisfiable_valid_witness() {
        let mut rng = StdRng::seed_from_u64(42);
        let params = PedersenParams::setup(&mut rng);

        let balance = 1000u64;
        let blinding = Fr::rand(&mut rng);
        let c = commitment::commit(&params, balance, blinding);
        let amount = 500u64;

        let circuit = BalanceProofCircuit::new(&params, balance, blinding, &c, amount);

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();

        let satisfied = cs.is_satisfied().unwrap();
        let num_constraints = cs.num_constraints();

        assert!(satisfied, "circuit must be satisfied for valid witness");
        // Sanity: ~67 constraints (2 commitment + 64 boolean + 1 sum recomposition)
        assert!(
            num_constraints < 200,
            "circuit should be compact, got {} constraints",
            num_constraints
        );
    }

    #[test]
    fn circuit_unsatisfied_insufficient_balance() {
        let mut rng = StdRng::seed_from_u64(42);
        let params = PedersenParams::setup(&mut rng);

        let balance = 50u64;
        let blinding = Fr::rand(&mut rng);
        let c = commitment::commit(&params, balance, blinding);
        let amount = 100u64;

        let circuit = BalanceProofCircuit::new(&params, balance, blinding, &c, amount);

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();

        assert!(
            !cs.is_satisfied().unwrap(),
            "circuit must NOT be satisfied when balance < amount"
        );
    }

    #[test]
    fn circuit_satisfiable_exact_balance() {
        let mut rng = StdRng::seed_from_u64(42);
        let params = PedersenParams::setup(&mut rng);

        let balance = 100u64;
        let blinding = Fr::rand(&mut rng);
        let c = commitment::commit(&params, balance, blinding);

        let circuit = BalanceProofCircuit::new(&params, balance, blinding, &c, 100);

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();

        assert!(cs.is_satisfied().unwrap(), "exact balance must pass");
    }

    #[test]
    fn circuit_satisfiable_zero_zero() {
        let mut rng = StdRng::seed_from_u64(42);
        let params = PedersenParams::setup(&mut rng);

        let blinding = Fr::rand(&mut rng);
        let c = commitment::commit(&params, 0, blinding);

        let circuit = BalanceProofCircuit::new(&params, 0, blinding, &c, 0);

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();

        assert!(cs.is_satisfied().unwrap());
    }

    #[test]
    fn circuit_unsatisfied_wrong_commitment() {
        let mut rng = StdRng::seed_from_u64(42);
        let params = PedersenParams::setup(&mut rng);

        let balance = 100u64;
        let blinding = Fr::rand(&mut rng);
        // Commit to a DIFFERENT value
        let wrong_c = commitment::commit(&params, 999, blinding);

        let circuit = BalanceProofCircuit::new(&params, balance, blinding, &wrong_c, 50);

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();

        assert!(
            !cs.is_satisfied().unwrap(),
            "wrong commitment must not pass"
        );
    }

    #[test]
    fn public_inputs_match_expected_values() {
        let mut rng = StdRng::seed_from_u64(42);
        let params = PedersenParams::setup(&mut rng);

        let balance = 500u64;
        let blinding = Fr::rand(&mut rng);
        let c = commitment::commit(&params, balance, blinding);
        let amount = 100u64;

        // Verify that public_inputs() produces the correct values matching
        // the circuit's new_input allocation order: [commitment_scalar, required_amount].
        let inputs = public_inputs(&c, amount);
        assert_eq!(inputs.len(), 2, "circuit expects exactly 2 public inputs");
        assert_eq!(
            inputs[0], c.scalar,
            "first public input must be the commitment scalar"
        );
        assert_eq!(
            inputs[1],
            Fr::from(amount),
            "second public input must be the required amount"
        );

        // Also verify the circuit is satisfied with these inputs.
        let circuit = BalanceProofCircuit::new(&params, balance, blinding, &c, amount);
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(cs.is_satisfied().unwrap(), "circuit must be satisfied");
    }

    #[test]
    fn circuit_constraint_count() {
        let mut rng = StdRng::seed_from_u64(42);
        let params = PedersenParams::setup(&mut rng);

        let blinding = Fr::rand(&mut rng);
        let c = commitment::commit(&params, 1000, blinding);

        let circuit = BalanceProofCircuit::new(&params, 1000, blinding, &c, 500);

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();

        let n = cs.num_constraints();
        // Expected: ~66-70 constraints (depends on arkworks internals).
        // We assert a sane upper bound to catch regressions.
        assert!(n > 50, "too few constraints ({}), something is wrong", n);
        assert!(n < 200, "too many constraints ({}), circuit bloat", n);
    }
}
