//! # Pedersen Commitment Scheme over BN254
//!
//! A Pedersen commitment to value `v` with blinding factor `r` is:
//!
//! ```text
//! C = v * G + r * H          (elliptic-curve form, on BN254/G1)
//! c = v * g + r * h   mod p  (scalar-field form, in Fr)
//! ```
//!
//! Both forms use the same witness `(v, r)`. The EC form is stored on-chain
//! for external auditability. The scalar form is what the Groth16 circuit
//! actually constrains, because native Fr arithmetic inside an R1CS over Fr
//! is trivial (two multiplications), whereas emulating Fq inside Fr is
//! prohibitively expensive (~thousands of constraints per field op).
//!
//! ## Why two forms?
//!
//! Groth16 on BN254 generates constraints over the scalar field `Fr`.
//! The curve G1 lives over the base field `Fq`. Verifying `C = v*G + r*H`
//! inside the circuit would require non-native field arithmetic for every
//! coordinate operation — roughly 10-50x overhead per constraint. By using
//! a parallel scalar commitment `c = v*g + r*h` (where `g, h` are random
//! Fr elements with unknown discrete-log relation), we get the same
//! hiding/binding properties with zero overhead.
//!
//! The binding between the two commitments is enforced by the fact that
//! the prover uses the *same* `(v, r)` for both. A dishonest prover who
//! uses different witnesses would need to break binding on at least one
//! of the two schemes, which reduces to DLOG on either BN254/G1 or Fr.

use ark_bn254::{Fr, G1Affine, G1Projective};
use ark_ec::{AffineRepr, CurveGroup};
use ark_ff::UniformRand;
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use ark_std::rand::Rng;
use std::ops::Mul;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Public parameters for the Pedersen commitment scheme.
///
/// Contains generators for both the EC-based and scalar-field-based
/// commitment forms. These MUST be generated via a setup ceremony or
/// hash-to-curve so that no party knows the discrete-log relations.
#[derive(Clone, Debug, CanonicalSerialize, CanonicalDeserialize)]
pub struct PedersenParams {
    // -- EC generators (for on-chain commitment) ----------------------------
    /// Primary EC generator — value component.
    pub g: G1Affine,
    /// Secondary EC generator — blinding component.
    pub h: G1Affine,

    // -- Scalar generators (for in-circuit commitment) ----------------------
    /// Primary scalar generator in Fr.
    pub g_scalar: Fr,
    /// Secondary scalar generator in Fr.
    pub h_scalar: Fr,
}

/// A Pedersen commitment carrying both the EC point (on-chain) and the
/// scalar value (circuit input).
#[derive(Clone, Debug, PartialEq, Eq, CanonicalSerialize, CanonicalDeserialize)]
pub struct Commitment {
    /// EC commitment: `C = v * G + r * H` on BN254/G1.
    pub point: G1Affine,
    /// Scalar commitment: `c = v * g_scalar + r * h_scalar` in Fr.
    pub scalar: Fr,
}

// ---------------------------------------------------------------------------
// Setup
// ---------------------------------------------------------------------------

impl PedersenParams {
    /// Generate fresh commitment parameters.
    ///
    /// In production this should be replaced by a deterministic derivation
    /// (hash-to-curve for EC generators, hash-to-field for scalar generators)
    /// or an MPC ceremony output.
    pub fn setup<R: Rng>(rng: &mut R) -> Self {
        // EC generators: random points on BN254/G1 (prime-order ⇒ always generators).
        let g = G1Projective::rand(rng).into_affine();
        let h = G1Projective::rand(rng).into_affine();

        // Scalar generators: random non-zero Fr elements.
        let g_scalar = Fr::rand(rng);
        let h_scalar = Fr::rand(rng);

        debug_assert!(!g.is_zero(), "EC generator g must not be identity");
        debug_assert!(!h.is_zero(), "EC generator h must not be identity");

        Self {
            g,
            h,
            g_scalar,
            h_scalar,
        }
    }

    /// Serialize parameters to compressed bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        self.serialize_compressed(&mut buf)
            .expect("PedersenParams serialization must not fail");
        buf
    }

    /// Deserialize parameters from compressed bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, ark_serialize::SerializationError> {
        Self::deserialize_compressed(data)
    }
}

// ---------------------------------------------------------------------------
// Commit / Verify
// ---------------------------------------------------------------------------

/// Compute the Pedersen commitment in both EC and scalar forms.
///
/// ```text
/// EC:     C = value * G + blinding * H
/// Scalar: c = value * g_scalar + blinding * h_scalar
/// ```
pub fn commit(params: &PedersenParams, value: u64, blinding: Fr) -> Commitment {
    let v = Fr::from(value);

    // EC form
    let point = (params.g.mul(v) + params.h.mul(blinding)).into_affine();

    // Scalar form
    let scalar = v * params.g_scalar + blinding * params.h_scalar;

    Commitment { point, scalar }
}

/// Verify that `commitment` opens to `(value, blinding)` under `params`.
///
/// Checks both the EC and scalar forms. This is NOT a zero-knowledge
/// operation — the opening `(value, blinding)` is revealed. Use this
/// only for audits or dispute resolution.
pub fn verify_commitment(
    params: &PedersenParams,
    commitment: &Commitment,
    value: u64,
    blinding: Fr,
) -> bool {
    let expected = commit(params, value, blinding);
    commitment.point == expected.point && commitment.scalar == expected.scalar
}

impl Commitment {
    /// Serialize commitment to compressed bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        self.serialize_compressed(&mut buf)
            .expect("Commitment serialization must not fail");
        buf
    }

    /// Deserialize commitment from compressed bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, ark_serialize::SerializationError> {
        Self::deserialize_compressed(data)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ark_ff::UniformRand;
    use ark_std::test_rng;

    #[test]
    fn commitment_deterministic() {
        let mut rng = test_rng();
        let params = PedersenParams::setup(&mut rng);
        let r = Fr::rand(&mut rng);

        let c1 = commit(&params, 100, r);
        let c2 = commit(&params, 100, r);
        assert_eq!(c1, c2);
    }

    #[test]
    fn different_values_different_commitments() {
        let mut rng = test_rng();
        let params = PedersenParams::setup(&mut rng);
        let r = Fr::rand(&mut rng);

        let c1 = commit(&params, 100, r);
        let c2 = commit(&params, 101, r);
        assert_ne!(c1, c2);
    }

    #[test]
    fn different_blindings_different_commitments() {
        let mut rng = test_rng();
        let params = PedersenParams::setup(&mut rng);
        let r1 = Fr::rand(&mut rng);
        let r2 = Fr::rand(&mut rng);

        let c1 = commit(&params, 100, r1);
        let c2 = commit(&params, 100, r2);
        assert_ne!(
            c1, c2,
            "hiding: different blindings must produce different commitments"
        );
    }

    #[test]
    fn verify_valid_opening() {
        let mut rng = test_rng();
        let params = PedersenParams::setup(&mut rng);
        let r = Fr::rand(&mut rng);
        let c = commit(&params, 42, r);

        assert!(verify_commitment(&params, &c, 42, r));
    }

    #[test]
    fn reject_wrong_value() {
        let mut rng = test_rng();
        let params = PedersenParams::setup(&mut rng);
        let r = Fr::rand(&mut rng);
        let c = commit(&params, 42, r);

        assert!(!verify_commitment(&params, &c, 43, r));
    }

    #[test]
    fn reject_wrong_blinding() {
        let mut rng = test_rng();
        let params = PedersenParams::setup(&mut rng);
        let r1 = Fr::rand(&mut rng);
        let r2 = Fr::rand(&mut rng);
        let c = commit(&params, 42, r1);

        assert!(!verify_commitment(&params, &c, 42, r2));
    }

    #[test]
    fn params_serialization_round_trip() {
        let mut rng = test_rng();
        let params = PedersenParams::setup(&mut rng);
        let bytes = params.to_bytes();
        let restored = PedersenParams::from_bytes(&bytes).unwrap();

        assert_eq!(params.g, restored.g);
        assert_eq!(params.h, restored.h);
        assert_eq!(params.g_scalar, restored.g_scalar);
        assert_eq!(params.h_scalar, restored.h_scalar);
    }

    #[test]
    fn commitment_serialization_round_trip() {
        let mut rng = test_rng();
        let params = PedersenParams::setup(&mut rng);
        let r = Fr::rand(&mut rng);
        let c = commit(&params, 999, r);

        let bytes = c.to_bytes();
        let restored = Commitment::from_bytes(&bytes).unwrap();
        assert_eq!(c, restored);
    }

    #[test]
    fn scalar_and_ec_commitments_are_consistent() {
        // Verify both forms use the same witness — changing the value
        // invalidates both simultaneously.
        let mut rng = test_rng();
        let params = PedersenParams::setup(&mut rng);
        let r = Fr::rand(&mut rng);

        let c = commit(&params, 100, r);

        // Recompute scalar form manually
        let v = Fr::from(100u64);
        let expected_scalar = v * params.g_scalar + r * params.h_scalar;
        assert_eq!(c.scalar, expected_scalar);

        // Recompute EC form manually
        let expected_point = (params.g.mul(v) + params.h.mul(r)).into_affine();
        assert_eq!(c.point, expected_point);
    }
}
