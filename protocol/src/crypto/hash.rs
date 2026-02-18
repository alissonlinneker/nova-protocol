//! # Hashing Utilities
//!
//! Cryptographic hash functions used throughout NOVA. We support two primary
//! hash functions and refuse to support more without a very good reason:
//!
//! - **BLAKE3** — Our default. Fast on every platform, parallelizable,
//!   and provably secure under standard assumptions. Used for transaction IDs,
//!   Merkle trees, and anywhere performance matters (which is everywhere).
//!
//! - **SHA-256** — For interoperability with Bitcoin, Ethereum, and the rest
//!   of the "we chose SHA-256 in 2009 and now we're stuck with it" ecosystem.
//!   Also used in `double_sha256` for transaction ID compatibility.
//!
//! ## On hash function choice
//!
//! BLAKE3 is ~5x faster than SHA-256 on x86-64 and ~3x faster on ARM.
//! Both provide 128-bit collision resistance (256-bit output). There's no
//! security reason to prefer SHA-256 — only compatibility. When building
//! NOVA-native data structures, always prefer BLAKE3. When talking to
//! external systems, use whatever they expect.
//!
//! ## hash_to_field
//!
//! The `hash_to_field` function maps arbitrary data to a BN254 scalar field
//! element. This is essential for zero-knowledge proofs where we need to
//! represent arbitrary data as field elements. The construction follows a
//! hash-and-reduce approach: hash with BLAKE3, then reduce modulo the
//! field order. Simple, secure, deterministic.

use ark_bn254::Fr;
use ark_ff::PrimeField;
use sha2::{Digest, Sha256};

/// Compute the SHA-256 hash of the input data.
///
/// Returns a 32-byte digest as a `Vec<u8>`. Used primarily for cross-chain
/// compatibility and double-hashing constructions. For NOVA-internal hashing,
/// prefer `blake3_hash()`.
///
/// Why `Vec<u8>` and not `[u8; 32]`? Because half the callers immediately
/// pass it to functions that want `&[u8]`, and the other half want to
/// chain it into `double_sha256`. The heap allocation is noise compared
/// to the cost of the hash itself.
///
/// # Example
///
/// ```
/// use nova_protocol::crypto::sha256;
///
/// let hash = sha256(b"NOVA protocol");
/// assert_eq!(hash.len(), 32);
/// ```
pub fn sha256(data: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}

/// Compute the SHA-256 hash and return a fixed-size array.
///
/// Same as `sha256()` but returns `[u8; 32]` for callers that want
/// a fixed-size type without the heap allocation. Use this in hot paths
/// where the array type propagates naturally.
pub fn sha256_array(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut output = [0u8; 32];
    output.copy_from_slice(&result);
    output
}

/// Compute the BLAKE3 hash of the input data.
///
/// Returns a 32-byte digest as a fixed-size array. This is the workhorse
/// hash function of NOVA — fast, secure, and elegant. Uses the `blake3`
/// crate which automatically takes advantage of SIMD instructions on
/// supported platforms.
///
/// BLAKE3 is a Merkle-tree-based hash that can hash large inputs in parallel
/// across multiple cores. For typical transaction data (<1KB), the single-threaded
/// performance is what matters, and it's still ~5x faster than SHA-256.
///
/// # Example
///
/// ```
/// use nova_protocol::crypto::blake3_hash;
///
/// let hash = blake3_hash(b"NOVA protocol");
/// assert_eq!(hash.len(), 32);
/// ```
pub fn blake3_hash(data: &[u8]) -> [u8; 32] {
    *blake3::hash(data).as_bytes()
}

/// Compute BLAKE3 and return the digest as a `Vec<u8>`.
///
/// Use [`blake3_hash`] when you want a fixed-size array. This variant
/// exists for call sites that need a heap-allocated result (e.g., when
/// storing hashes in a `HashMap<Vec<u8>, _>` or passing to APIs that
/// want owned data).
pub fn blake3_hash_vec(data: &[u8]) -> Vec<u8> {
    blake3::hash(data).as_bytes().to_vec()
}

/// Compute the double-SHA-256 hash: `SHA-256(SHA-256(data))`.
///
/// This construction is used for transaction IDs in Bitcoin and many other
/// protocols. The double-hash provides protection against length extension
/// attacks (which SHA-256 alone is vulnerable to, though in practice this
/// matters less than people think).
///
/// We include it primarily for cross-chain transaction references. For
/// NOVA-native transaction IDs, we use BLAKE3 (which doesn't need double-
/// hashing because it's already resistant to length extension attacks —
/// it's based on a wide-pipe Merkle-Damgard variant that truncates output).
///
/// # Example
///
/// ```
/// use nova_protocol::crypto::double_sha256;
///
/// let tx_id = double_sha256(b"raw transaction bytes");
/// assert_eq!(tx_id.len(), 32);
/// ```
pub fn double_sha256(data: &[u8]) -> Vec<u8> {
    sha256(&sha256(data))
}

/// Hash arbitrary data to a BN254 scalar field element.
///
/// This function maps a byte string to an element of Fr (the scalar field of
/// BN254, a.k.a. alt_bn128). This is crucial for zero-knowledge proof circuits
/// where all computation happens over field elements.
///
/// ## Construction
///
/// 1. Hash the input with BLAKE3 to get 32 uniformly random bytes.
/// 2. Interpret those bytes as a little-endian integer.
/// 3. Reduce modulo the field order `r` to get a valid field element.
///
/// This produces a nearly uniform distribution over Fr. The bias is
/// negligible (< 2^-128) because BLAKE3's output is 256 bits and the
/// BN254 scalar field order is ~254 bits. The few extra bits of entropy
/// make the modular reduction essentially uniform.
///
/// ## Why BLAKE3 and not SHA-256?
///
/// Because this is internal to NOVA's ZKP system and doesn't need to be
/// compatible with anything external. We use the faster hash.
///
/// # Example
///
/// ```
/// use nova_protocol::crypto::hash::hash_to_field;
///
/// let field_elem = hash_to_field(b"some transaction data");
/// // field_elem is now a valid BN254 scalar that can be used in circuits.
/// ```
pub fn hash_to_field(data: &[u8]) -> Fr {
    let hash = blake3_hash(data);
    // Fr::from_le_bytes_mod_order interprets the bytes as a little-endian
    // integer and reduces mod r. This is the standard way to do it in arkworks.
    Fr::from_le_bytes_mod_order(&hash)
}

/// Compute a domain-separated hash using BLAKE3 with a context string.
///
/// Domain separation prevents hash collisions across different protocol contexts.
/// For example, `domain_separated_hash("tx-id", data)` and
/// `domain_separated_hash("block-hash", data)` will never collide even if
/// `data` is the same, because the domain tag is mixed into the hash.
///
/// This uses BLAKE3's built-in `derive_key` mode, which is the proper way
/// to do domain separation with BLAKE3. Don't try to prepend a tag manually —
/// that's what amateurs do. BLAKE3's `derive_key` uses a different internal
/// IV derived from the context string, making cross-context collisions
/// impossible by construction.
pub fn domain_separated_hash(context: &str, data: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_derive_key(context);
    hasher.update(data);
    *hasher.finalize().as_bytes()
}

/// Hash multiple byte slices together without concatenation overhead.
///
/// Instead of allocating a buffer to concatenate inputs, we feed them
/// sequentially into the hasher. Same result, less allocation. Particularly
/// useful for hashing composite structures like `(sender || receiver || amount)`
/// without the temporary buffer.
pub fn blake3_hash_multi(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    for part in parts {
        hasher.update(part);
    }
    *hasher.finalize().as_bytes()
}

/// Compute a Merkle root from a list of leaf hashes using BLAKE3.
///
/// This is a simple binary Merkle tree — nothing fancy like sparse Merkle
/// trees or Merkle Mountain Ranges. For our use case (transaction sets in a
/// block), a basic binary tree is sufficient and easier to reason about.
///
/// If the number of leaves is odd, the last leaf is duplicated. This is
/// the same approach Bitcoin uses, and while it has some known issues with
/// duplicated transactions, we handle that at a higher layer by enforcing
/// transaction uniqueness before building the tree.
///
/// Returns the 32-byte root hash. If the input is empty, returns all zeros
/// (the "empty tree" sentinel).
pub fn merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
    if leaves.is_empty() {
        return [0u8; 32];
    }

    let mut current_level: Vec<[u8; 32]> = leaves.to_vec();

    // A single leaf is paired with itself, matching Bitcoin's Merkle tree
    // behavior. This ensures the root is always the output of a hash
    // operation (never a raw leaf), which simplifies proof verification.
    if current_level.len() == 1 {
        return blake3_hash_multi(&[current_level[0].as_slice(), current_level[0].as_slice()]);
    }

    while current_level.len() > 1 {
        let mut next_level = Vec::with_capacity((current_level.len() + 1) / 2);

        for chunk in current_level.chunks(2) {
            let left = &chunk[0];
            let right = if chunk.len() == 2 {
                &chunk[1]
            } else {
                // Odd number of elements — duplicate the last one.
                // Bitcoin does this too, and yes, we know about CVE-2012-2459.
                // We prevent it by enforcing unique transaction IDs at the
                // mempool layer.
                &chunk[0]
            };

            let parent = blake3_hash_multi(&[left.as_slice(), right.as_slice()]);
            next_level.push(parent);
        }

        current_level = next_level;
    }

    current_level[0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_known_vector() {
        // SHA-256 of empty string — the canonical test vector everyone should
        // have memorized by now.
        let hash = sha256(b"");
        let expected =
            hex::decode("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
                .unwrap();
        assert_eq!(hash, expected);
    }

    #[test]
    fn sha256_deterministic() {
        let a = sha256(b"nova");
        let b = sha256(b"nova");
        assert_eq!(a, b);
        assert_eq!(a.len(), 32);
    }

    #[test]
    fn test_sha256_array_matches_vec() {
        let vec_result = sha256(b"test data");
        let arr_result = sha256_array(b"test data");
        assert_eq!(vec_result.as_slice(), arr_result.as_slice());
    }

    #[test]
    fn blake3_deterministic() {
        let a = blake3_hash(b"nova");
        let b = blake3_hash(b"nova");
        assert_eq!(a, b);
        assert_eq!(a.len(), 32);
    }

    #[test]
    fn test_blake3_different_inputs() {
        let a = blake3_hash(b"nova");
        let b = blake3_hash(b"Nova"); // case sensitive!
        assert_ne!(a, b);
    }

    #[test]
    fn double_sha256_differs_from_single() {
        let single = sha256(b"nova");
        let double = double_sha256(b"nova");
        assert_ne!(single, double);
        assert_eq!(double.len(), 32);

        // But double should equal SHA-256 of the single hash
        let manual_double = sha256(&single);
        assert_eq!(double, manual_double);
    }

    #[test]
    fn test_hash_to_field_deterministic() {
        let a = hash_to_field(b"transaction data");
        let b = hash_to_field(b"transaction data");
        assert_eq!(a, b);
    }

    #[test]
    fn test_hash_to_field_different_inputs() {
        let a = hash_to_field(b"input A");
        let b = hash_to_field(b"input B");
        assert_ne!(a, b);
    }

    #[test]
    fn test_domain_separation() {
        // Same data, different contexts = different hashes.
        // This is the whole point of domain separation.
        let data = b"same data";
        let hash_a = domain_separated_hash("context-a", data);
        let hash_b = domain_separated_hash("context-b", data);
        assert_ne!(hash_a, hash_b);
    }

    #[test]
    fn test_domain_separated_is_not_plain_blake3() {
        // Domain-separated hash should differ from a plain BLAKE3 hash.
        let data = b"test data";
        let plain = blake3_hash(data);
        let separated = domain_separated_hash("nova-test", data);
        assert_ne!(plain, separated);
    }

    #[test]
    fn test_blake3_hash_multi() {
        // Hashing parts separately via update() should equal hashing them
        // concatenated. This is a fundamental property of Merkle-Damgard
        // (and BLAKE3's equivalent construction).
        let part1 = b"hello";
        let part2 = b" world";

        let multi = blake3_hash_multi(&[part1, part2]);
        let single = blake3_hash(b"hello world");
        assert_eq!(multi, single);
    }

    #[test]
    fn test_merkle_root_empty() {
        let root = merkle_root(&[]);
        assert_eq!(root, [0u8; 32]);
    }

    #[test]
    fn test_merkle_root_single_leaf() {
        let leaf = blake3_hash(b"only child");
        let root = merkle_root(&[leaf]);
        // With one leaf, it gets paired with itself.
        let expected = blake3_hash_multi(&[leaf.as_slice(), leaf.as_slice()]);
        assert_eq!(root, expected);
    }

    #[test]
    fn test_merkle_root_two_leaves() {
        let leaf1 = blake3_hash(b"left");
        let leaf2 = blake3_hash(b"right");
        let root = merkle_root(&[leaf1, leaf2]);
        let expected = blake3_hash_multi(&[leaf1.as_slice(), leaf2.as_slice()]);
        assert_eq!(root, expected);
    }

    #[test]
    fn test_merkle_root_deterministic() {
        let leaves: Vec<[u8; 32]> = (0..8).map(|i| blake3_hash(&[i])).collect();
        let root1 = merkle_root(&leaves);
        let root2 = merkle_root(&leaves);
        assert_eq!(root1, root2);
    }

    #[test]
    fn test_merkle_root_order_matters() {
        // Merkle trees are order-dependent. Swapping leaves changes the root.
        // This is important for consensus — everyone must agree on tx ordering.
        let leaf1 = blake3_hash(b"first");
        let leaf2 = blake3_hash(b"second");
        let root_a = merkle_root(&[leaf1, leaf2]);
        let root_b = merkle_root(&[leaf2, leaf1]);
        assert_ne!(root_a, root_b);
    }

    #[test]
    fn test_blake3_hash_vec_matches_array() {
        let data = b"consistency check";
        let arr = blake3_hash(data);
        let vec = blake3_hash_vec(data);
        assert_eq!(arr.as_slice(), vec.as_slice());
    }
}
