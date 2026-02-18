/**
 * NOVA Protocol — Identity & Cryptographic Primitives
 *
 * All key generation and signing operations use Ed25519 via `@noble/ed25519`
 * which is a well-audited, pure-JS implementation with no native dependencies.
 */

import * as ed from '@noble/ed25519';
import { sha512 } from '@noble/hashes/sha512';
import { bech32 } from '@scure/base';
import type { NovaId, PublicKey, SecretKey, Signature } from './types.js';

// @noble/ed25519 v2 requires an explicit SHA-512 configuration.
// We must set this before any operations are performed.
ed.etc.sha512Sync = (...msgs: Uint8Array[]): Uint8Array => {
  const h = sha512.create();
  for (const m of msgs) h.update(m);
  return h.digest();
};

// ---------------------------------------------------------------------------
// Key pair generation
// ---------------------------------------------------------------------------

export interface Keypair {
  publicKey: PublicKey;
  secretKey: SecretKey;
}

/**
 * Generate a fresh Ed25519 keypair from cryptographically-secure randomness.
 */
export function generateKeypair(): Keypair {
  const secretKey = ed.utils.randomPrivateKey();
  const publicKey = ed.getPublicKey(secretKey);

  return {
    publicKey: publicKey as PublicKey,
    secretKey: secretKey as SecretKey,
  };
}

/**
 * Derive an Ed25519 keypair deterministically from a 32-byte seed.
 *
 * @param seed — Exactly 32 bytes of entropy.
 * @throws {Error} If the seed is not 32 bytes.
 */
export function keypairFromSeed(seed: Uint8Array): Keypair {
  if (seed.length !== 32) {
    throw new Error(`keypairFromSeed: expected 32-byte seed, got ${seed.length}`);
  }

  const secretKey = new Uint8Array(seed);
  const publicKey = ed.getPublicKey(secretKey);

  return {
    publicKey: publicKey as PublicKey,
    secretKey: secretKey as SecretKey,
  };
}

// ---------------------------------------------------------------------------
// NOVA ID (bech32 address)
// ---------------------------------------------------------------------------

const NOVA_HRP = 'nova';
const BECH32_LIMIT = 90;

/**
 * Derive a NOVA ID address from a 32-byte Ed25519 public key.
 *
 * The address is bech32-encoded with the human-readable prefix `"nova"`.
 */
export function createNovaId(publicKey: PublicKey): NovaId {
  if (publicKey.length !== 32) {
    throw new Error(`createNovaId: expected 32-byte public key, got ${publicKey.length}`);
  }
  const words = bech32.toWords(publicKey);
  return bech32.encode(NOVA_HRP, words, BECH32_LIMIT) as NovaId;
}

/**
 * Parse a NOVA ID address and extract the raw public key bytes and HRP.
 *
 * @throws {Error} If the address cannot be decoded or has an unexpected prefix.
 */
export function parseNovaId(address: string): { publicKey: Uint8Array; hrp: string } {
  const { prefix, words } = bech32.decode(address as `${string}1${string}`, BECH32_LIMIT);
  if (prefix !== NOVA_HRP) {
    throw new Error(`parseNovaId: unexpected prefix "${prefix}", expected "${NOVA_HRP}"`);
  }
  const publicKey = new Uint8Array(bech32.fromWords(words));
  return { publicKey, hrp: prefix };
}

// ---------------------------------------------------------------------------
// Signing & verification
// ---------------------------------------------------------------------------

/**
 * Sign an arbitrary message with an Ed25519 secret key.
 *
 * @returns A 64-byte Ed25519 signature.
 */
export function signMessage(secretKey: SecretKey, message: Uint8Array): Signature {
  const sig = ed.sign(message, secretKey);
  return sig as Signature;
}

/**
 * Verify an Ed25519 signature against a public key and message.
 */
export function verifySignature(
  publicKey: PublicKey,
  message: Uint8Array,
  signature: Signature,
): boolean {
  try {
    return ed.verify(signature, message, publicKey);
  } catch {
    // Malformed inputs (wrong lengths, invalid curve points, etc.)
    return false;
  }
}
