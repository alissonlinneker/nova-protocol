//! # Key Recovery via Shamir's Secret Sharing
//!
//! Implements Shamir's Secret Sharing Scheme (SSSS) over GF(256) for
//! splitting Ed25519 seed material into `n` shares with a reconstruction
//! threshold of `t`. Any `t` shares can recover the original secret;
//! fewer than `t` shares reveal zero information about it.
//!
//! ## Finite Field Arithmetic
//!
//! All operations are performed in GF(2^8) with the irreducible polynomial
//! `x^8 + x^4 + x^3 + x + 1` (0x11B) — the same field used by AES.
//! Multiplication uses log/exp tables for fast evaluation. The generator
//! element is 3 (0x03), which generates the full multiplicative group
//! of order 255.
//!
//! ## Security Model
//!
//! - Shares are generated using OS CSPRNG for polynomial coefficients.
//! - The scheme is information-theoretically secure: `t-1` shares give
//!   an attacker exactly zero bits of information about the secret.
//! - Share indices are 1-based (x=0 is reserved for the secret itself).
//!
//! ## Usage
//!
//! ```
//! use nova_protocol::identity::recovery::{split_secret, recover_secret, ShamirConfig};
//!
//! let secret = b"this is a 32-byte seed value!!!!";
//! let config = ShamirConfig::new(3, 5).unwrap();
//! let shares = split_secret(secret, &config).unwrap();
//!
//! // Any 3 of the 5 shares can recover the secret
//! let recovered = recover_secret(&shares[..3]).unwrap();
//! assert_eq!(secret.as_slice(), &recovered);
//! ```

use rand::RngCore;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during secret sharing operations.
#[derive(Debug, Error)]
pub enum ShamirError {
    /// The threshold must be at least 2 (1-of-n is just copies).
    #[error("threshold must be >= 2, got {0}")]
    ThresholdTooLow(u8),

    /// The number of shares must be at least equal to the threshold.
    #[error("total shares ({total}) must be >= threshold ({threshold})")]
    InsufficientShares {
        /// The configured threshold.
        threshold: u8,
        /// The configured total.
        total: u8,
    },

    /// Cannot create more than 255 shares (x-coordinates are in GF(256)\{0}).
    #[error("cannot create more than 255 shares, got {0}")]
    TooManyShares(u8),

    /// The secret is empty — nothing to split.
    #[error("secret must not be empty")]
    EmptySecret,

    /// Not enough shares provided for reconstruction.
    #[error("need at least 2 shares for reconstruction, got {0}")]
    NotEnoughShares(usize),

    /// Shares have inconsistent data lengths.
    #[error("share data lengths are inconsistent: expected {expected}, got {got}")]
    InconsistentShareLengths {
        /// Expected length from the first share.
        expected: usize,
        /// Actual length of the offending share.
        got: usize,
    },

    /// Duplicate share indices were provided.
    #[error("duplicate share index: {0}")]
    DuplicateShareIndex(u8),
}

// ---------------------------------------------------------------------------
// GF(256) Arithmetic
// ---------------------------------------------------------------------------

/// GF(256) with irreducible polynomial x^8 + x^4 + x^3 + x + 1 (0x11B).
///
/// We use log/exp lookup tables for fast multiplication and division.
/// The generator element is 3 (0x03), which generates the full
/// multiplicative group of order 255.
mod gf256 {
    /// Irreducible polynomial: x^8 + x^4 + x^3 + x + 1.
    const MODULUS: u16 = 0x11B;

    /// Exponentiation table: EXP[i] = g^i mod p, where g = 0x03.
    /// Length 512 to handle wraparound during multiplication without
    /// needing modular reduction on the log sum.
    const fn build_exp_table() -> [u8; 512] {
        let mut table = [0u8; 512];
        let mut val: u16 = 1;
        let mut i = 0;
        while i < 255 {
            table[i] = val as u8;
            table[i + 255] = val as u8;
            // Multiply by generator (3): val * 3 = val * 2 + val
            val = (val << 1) ^ val;
            if val >= 256 {
                val ^= MODULUS;
            }
            i += 1;
        }
        table[255] = table[0];
        table
    }

    /// Logarithm table: LOG[EXP[i]] = i.
    const fn build_log_table() -> [u8; 256] {
        let exp = build_exp_table();
        let mut table = [0u8; 256];
        let mut i = 0;
        while i < 255 {
            table[exp[i] as usize] = i as u8;
            i += 1;
        }
        table
    }

    static EXP: [u8; 512] = build_exp_table();
    static LOG: [u8; 256] = build_log_table();

    /// Add two elements in GF(256). Addition is XOR.
    #[inline]
    pub fn add(a: u8, b: u8) -> u8 {
        a ^ b
    }

    /// Subtract two elements in GF(256). Same as addition (characteristic 2).
    #[inline]
    pub fn sub(a: u8, b: u8) -> u8 {
        a ^ b
    }

    /// Multiply two elements in GF(256) using log/exp tables.
    #[inline]
    pub fn mul(a: u8, b: u8) -> u8 {
        if a == 0 || b == 0 {
            return 0;
        }
        let log_sum = LOG[a as usize] as usize + LOG[b as usize] as usize;
        EXP[log_sum]
    }

    /// Divide a by b in GF(256). Panics if b is zero.
    #[inline]
    pub fn div(a: u8, b: u8) -> u8 {
        assert!(b != 0, "division by zero in GF(256)");
        if a == 0 {
            return 0;
        }
        let log_diff = 255 + LOG[a as usize] as usize - LOG[b as usize] as usize;
        EXP[log_diff]
    }

    /// Evaluate a polynomial at point `x` using Horner's method.
    ///
    /// `coefficients[0]` is the constant term (the secret), `coefficients[1]`
    /// is the x^1 coefficient, etc.
    pub fn eval_polynomial(coefficients: &[u8], x: u8) -> u8 {
        let mut result = 0u8;
        for &coeff in coefficients.iter().rev() {
            result = add(mul(result, x), coeff);
        }
        result
    }

    /// Lagrange interpolation at x=0 to recover the secret.
    ///
    /// Given points (x_i, y_i), computes the polynomial value at x=0
    /// using the Lagrange basis polynomials.
    pub fn lagrange_interpolate_at_zero(xs: &[u8], ys: &[u8]) -> u8 {
        assert_eq!(xs.len(), ys.len());
        let n = xs.len();
        let mut secret = 0u8;

        for i in 0..n {
            let mut numerator = 1u8;
            let mut denominator = 1u8;

            for j in 0..n {
                if i == j {
                    continue;
                }
                // numerator *= (0 - x_j) = x_j  (since -x = x in GF(256))
                numerator = mul(numerator, xs[j]);
                // denominator *= (x_i - x_j)
                denominator = mul(denominator, sub(xs[i], xs[j]));
            }

            // Lagrange basis polynomial L_i(0) = numerator / denominator
            let lagrange = div(numerator, denominator);
            secret = add(secret, mul(ys[i], lagrange));
        }

        secret
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn add_is_xor() {
            assert_eq!(add(0xFF, 0xFF), 0);
            assert_eq!(add(0xAB, 0x00), 0xAB);
        }

        #[test]
        fn mul_identity() {
            for i in 0..=255u8 {
                assert_eq!(mul(i, 1), i);
                assert_eq!(mul(1, i), i);
            }
        }

        #[test]
        fn mul_zero() {
            for i in 0..=255u8 {
                assert_eq!(mul(i, 0), 0);
                assert_eq!(mul(0, i), 0);
            }
        }

        #[test]
        fn mul_div_inverse() {
            for a in 1..=255u8 {
                for b in 1..=255u8 {
                    let product = mul(a, b);
                    assert_eq!(div(product, b), a);
                }
            }
        }

        #[test]
        fn polynomial_eval_constant() {
            assert_eq!(eval_polynomial(&[42], 1), 42);
            assert_eq!(eval_polynomial(&[42], 100), 42);
        }

        #[test]
        fn lagrange_recovers_constant() {
            let xs = [1, 2, 3];
            let ys = [42, 42, 42];
            assert_eq!(lagrange_interpolate_at_zero(&xs, &ys), 42);
        }
    }
}

// ---------------------------------------------------------------------------
// Public Types
// ---------------------------------------------------------------------------

/// Configuration for Shamir's Secret Sharing.
///
/// Defines the threshold (minimum shares for recovery) and total number
/// of shares to generate. The scheme is `(threshold, total_shares)` — any
/// `threshold` shares can reconstruct the secret, but `threshold - 1`
/// shares reveal nothing.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ShamirConfig {
    /// Minimum number of shares required to reconstruct the secret.
    pub threshold: u8,
    /// Total number of shares to generate.
    pub total_shares: u8,
}

impl ShamirConfig {
    /// Create a new configuration, validating the parameters.
    ///
    /// # Constraints
    ///
    /// - `threshold >= 2` (1-of-n is just copying)
    /// - `total_shares >= threshold`
    /// - `total_shares <= 255` (share indices are non-zero bytes in GF(256))
    pub fn new(threshold: u8, total_shares: u8) -> Result<Self, ShamirError> {
        if threshold < 2 {
            return Err(ShamirError::ThresholdTooLow(threshold));
        }
        if total_shares < threshold {
            return Err(ShamirError::InsufficientShares {
                threshold,
                total: total_shares,
            });
        }
        Ok(Self {
            threshold,
            total_shares,
        })
    }
}

/// A single share of a split secret.
///
/// Each share has a unique index (1-255) and a data vector the same
/// length as the original secret. Shares are meaningless in isolation —
/// you need at least `threshold` of them to recover anything.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Share {
    /// The x-coordinate of this share's evaluation point (1-based).
    pub index: u8,
    /// The share data — one byte per byte of the original secret.
    pub data: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Split and Recover
// ---------------------------------------------------------------------------

/// Split a secret into shares using Shamir's Secret Sharing.
///
/// For each byte of the secret, generates a random polynomial of degree
/// `threshold - 1` with the secret byte as the constant term, then
/// evaluates it at points x = 1, 2, ..., total_shares.
///
/// # Arguments
///
/// - `secret` — The secret bytes to split (e.g., a 32-byte Ed25519 seed).
/// - `config` — Threshold and total share count.
///
/// # Returns
///
/// A vector of [`Share`] structs, each containing the share index and data.
pub fn split_secret(secret: &[u8], config: &ShamirConfig) -> Result<Vec<Share>, ShamirError> {
    if secret.is_empty() {
        return Err(ShamirError::EmptySecret);
    }

    let threshold = config.threshold as usize;
    let total = config.total_shares as usize;

    let mut shares: Vec<Share> = (1..=total)
        .map(|i| Share {
            index: i as u8,
            data: Vec::with_capacity(secret.len()),
        })
        .collect();

    let mut rng = rand::rngs::OsRng;

    // For each byte of the secret, construct a random polynomial and evaluate.
    for &secret_byte in secret {
        // Polynomial coefficients: [secret_byte, c_1, c_2, ..., c_{t-1}]
        // Degree = threshold - 1
        let mut coefficients = vec![0u8; threshold];
        coefficients[0] = secret_byte;

        // Fill higher-degree coefficients with CSPRNG output.
        let mut random_bytes = vec![0u8; threshold - 1];
        rng.fill_bytes(&mut random_bytes);
        coefficients[1..].copy_from_slice(&random_bytes);

        // Evaluate polynomial at each share's x-coordinate.
        for share in shares.iter_mut() {
            let y = gf256::eval_polynomial(&coefficients, share.index);
            share.data.push(y);
        }
    }

    Ok(shares)
}

/// Recover a secret from a sufficient number of shares.
///
/// Uses Lagrange interpolation over GF(256) to reconstruct the original
/// polynomial and evaluate it at x=0 (recovering the constant terms,
/// which are the secret bytes).
///
/// # Arguments
///
/// - `shares` — At least `threshold` shares from a previous [`split_secret`] call.
///
/// # Returns
///
/// The reconstructed secret as a `Vec<u8>`.
///
/// # Errors
///
/// Returns an error if:
/// - Fewer than 2 shares are provided
/// - Share data lengths are inconsistent
/// - Duplicate share indices are found
///
/// **Note**: If you provide fewer shares than the original threshold but
/// more than 1, the function will return *incorrect* data without error.
/// There is no way to detect this purely from the shares themselves.
pub fn recover_secret(shares: &[Share]) -> Result<Vec<u8>, ShamirError> {
    if shares.len() < 2 {
        return Err(ShamirError::NotEnoughShares(shares.len()));
    }

    // Validate consistent lengths.
    let expected_len = shares[0].data.len();
    for share in &shares[1..] {
        if share.data.len() != expected_len {
            return Err(ShamirError::InconsistentShareLengths {
                expected: expected_len,
                got: share.data.len(),
            });
        }
    }

    // Check for duplicate or zero indices.
    let mut seen = [false; 256];
    for share in shares {
        if share.index == 0 {
            return Err(ShamirError::DuplicateShareIndex(0));
        }
        if seen[share.index as usize] {
            return Err(ShamirError::DuplicateShareIndex(share.index));
        }
        seen[share.index as usize] = true;
    }

    let xs: Vec<u8> = shares.iter().map(|s| s.index).collect();
    let mut secret = Vec::with_capacity(expected_len);

    // Reconstruct each byte of the secret independently via Lagrange.
    for byte_idx in 0..expected_len {
        let ys: Vec<u8> = shares.iter().map(|s| s.data[byte_idx]).collect();
        let recovered_byte = gf256::lagrange_interpolate_at_zero(&xs, &ys);
        secret.push(recovered_byte);
    }

    Ok(secret)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_2_of_3_split_and_recover() {
        let secret = b"attack at dawn!!";
        let config = ShamirConfig::new(2, 3).unwrap();
        let shares = split_secret(secret, &config).unwrap();
        assert_eq!(shares.len(), 3);

        // Any 2 of 3 should reconstruct.
        let recovered = recover_secret(&shares[..2]).unwrap();
        assert_eq!(secret.as_slice(), &recovered);

        let recovered = recover_secret(&shares[1..]).unwrap();
        assert_eq!(secret.as_slice(), &recovered);

        let recovered = recover_secret(&[shares[0].clone(), shares[2].clone()]).unwrap();
        assert_eq!(secret.as_slice(), &recovered);
    }

    #[test]
    fn threshold_3_of_5() {
        let secret = vec![0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE];
        let config = ShamirConfig::new(3, 5).unwrap();
        let shares = split_secret(&secret, &config).unwrap();
        assert_eq!(shares.len(), 5);

        // Exhaustively test all 3-of-5 combinations.
        let combos: Vec<Vec<usize>> = vec![
            vec![0, 1, 2],
            vec![0, 1, 3],
            vec![0, 1, 4],
            vec![0, 2, 3],
            vec![0, 2, 4],
            vec![0, 3, 4],
            vec![1, 2, 3],
            vec![1, 2, 4],
            vec![1, 3, 4],
            vec![2, 3, 4],
        ];

        for combo in combos {
            let subset: Vec<Share> = combo.iter().map(|&i| shares[i].clone()).collect();
            let recovered = recover_secret(&subset).unwrap();
            assert_eq!(secret, recovered, "failed for combo {:?}", combo);
        }
    }

    #[test]
    fn full_32_byte_seed_recovery() {
        let seed = [42u8; 32];
        let config = ShamirConfig::new(3, 5).unwrap();
        let shares = split_secret(&seed, &config).unwrap();
        let recovered = recover_secret(&shares[0..3]).unwrap();
        assert_eq!(seed.as_slice(), &recovered);
    }

    #[test]
    fn ed25519_key_split_and_recover() {
        use crate::identity::NovaKeypair;

        let kp = NovaKeypair::generate();
        let secret = kp.secret_key_bytes();

        let config = ShamirConfig::new(3, 5).unwrap();
        let shares = split_secret(&secret, &config).unwrap();

        let recovered = recover_secret(&shares[1..4]).unwrap();
        let mut recovered_seed = [0u8; 32];
        recovered_seed.copy_from_slice(&recovered);

        let restored_kp = NovaKeypair::from_seed(&recovered_seed);
        assert_eq!(kp.public_key(), restored_kp.public_key());

        // Verify the restored key produces valid signatures.
        let msg = b"recovery test";
        let sig = restored_kp.sign(msg);
        assert!(kp.public_key().verify(msg, &sig));
    }

    #[test]
    fn insufficient_shares_produces_wrong_result() {
        let secret = b"secret";
        let config = ShamirConfig::new(3, 5).unwrap();
        let shares = split_secret(secret, &config).unwrap();

        // 2 shares when threshold is 3 will produce garbage.
        let recovered = recover_secret(&shares[..2]).unwrap();
        assert_ne!(secret.as_slice(), &recovered);
    }

    #[test]
    fn config_validation() {
        assert!(ShamirConfig::new(1, 3).is_err()); // threshold too low
        assert!(ShamirConfig::new(5, 3).is_err()); // total < threshold
        assert!(ShamirConfig::new(2, 3).is_ok());
        assert!(ShamirConfig::new(2, 255).is_ok()); // max shares
    }

    #[test]
    fn empty_secret_rejected() {
        let config = ShamirConfig::new(2, 3).unwrap();
        assert!(split_secret(&[], &config).is_err());
    }

    #[test]
    fn duplicate_share_indices_rejected() {
        let share = Share {
            index: 1,
            data: vec![42],
        };
        let result = recover_secret(&[share.clone(), share]);
        assert!(matches!(result, Err(ShamirError::DuplicateShareIndex(1))));
    }

    #[test]
    fn inconsistent_share_lengths_rejected() {
        let shares = vec![
            Share {
                index: 1,
                data: vec![1, 2, 3],
            },
            Share {
                index: 2,
                data: vec![4, 5],
            },
        ];
        assert!(matches!(
            recover_secret(&shares),
            Err(ShamirError::InconsistentShareLengths { .. })
        ));
    }

    #[test]
    fn single_byte_secret() {
        let secret = [0xAB];
        let config = ShamirConfig::new(2, 3).unwrap();
        let shares = split_secret(&secret, &config).unwrap();
        let recovered = recover_secret(&shares[..2]).unwrap();
        assert_eq!(secret.as_slice(), &recovered);
    }

    #[test]
    fn all_zeros_secret() {
        let secret = [0u8; 32];
        let config = ShamirConfig::new(3, 5).unwrap();
        let shares = split_secret(&secret, &config).unwrap();
        let recovered = recover_secret(&shares[2..5]).unwrap();
        assert_eq!(secret.as_slice(), &recovered);
    }

    #[test]
    fn all_ones_secret() {
        let secret = [0xFF; 32];
        let config = ShamirConfig::new(3, 5).unwrap();
        let shares = split_secret(&secret, &config).unwrap();
        let recovered = recover_secret(&shares[0..3]).unwrap();
        assert_eq!(secret.as_slice(), &recovered);
    }

    #[test]
    fn share_serialization_roundtrip() {
        let secret = b"serde test";
        let config = ShamirConfig::new(2, 3).unwrap();
        let shares = split_secret(secret, &config).unwrap();

        let json = serde_json::to_string(&shares).unwrap();
        let recovered_shares: Vec<Share> = serde_json::from_str(&json).unwrap();

        let recovered = recover_secret(&recovered_shares[..2]).unwrap();
        assert_eq!(secret.as_slice(), &recovered);
    }
}
