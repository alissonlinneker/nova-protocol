/**
 * NOVA Protocol — Low-level Utility Functions
 */

import { bech32 } from '@scure/base';

const HEX_CHARS = '0123456789abcdef';

/**
 * Decode a hex-encoded string into raw bytes.
 *
 * @param hex — Even-length hexadecimal string (with or without `0x` prefix).
 * @throws {Error} If the string length is odd or contains invalid characters.
 */
export function hexToBytes(hex: string): Uint8Array {
  const cleaned = hex.startsWith('0x') || hex.startsWith('0X') ? hex.slice(2) : hex;

  if (cleaned.length % 2 !== 0) {
    throw new Error(`hexToBytes: odd-length hex string (${cleaned.length} chars)`);
  }

  const bytes = new Uint8Array(cleaned.length / 2);
  for (let i = 0; i < bytes.length; i++) {
    const hi = HEX_CHARS.indexOf(cleaned[i * 2]!.toLowerCase());
    const lo = HEX_CHARS.indexOf(cleaned[i * 2 + 1]!.toLowerCase());
    if (hi === -1 || lo === -1) {
      throw new Error(`hexToBytes: invalid hex character at position ${i * 2}`);
    }
    bytes[i] = (hi << 4) | lo;
  }
  return bytes;
}

/**
 * Encode raw bytes as a lowercase hex string (no prefix).
 */
export function bytesToHex(bytes: Uint8Array): string {
  let hex = '';
  for (let i = 0; i < bytes.length; i++) {
    hex += HEX_CHARS[bytes[i]! >> 4];
    hex += HEX_CHARS[bytes[i]! & 0x0f];
  }
  return hex;
}

/**
 * Encode a 32-byte public key as a bech32-encoded NOVA address.
 *
 * Uses the `@scure/base` bech32 implementation with the "nova" HRP.
 */
export function encodeAddress(publicKey: Uint8Array): string {
  if (publicKey.length !== 32) {
    throw new Error(`encodeAddress: expected 32 bytes, got ${publicKey.length}`);
  }
  const words = bech32.toWords(publicKey);
  return bech32.encode('nova', words, 90);
}

/**
 * Decode a bech32 NOVA address back into the raw 32-byte public key.
 *
 * @throws {Error} If the address is invalid or uses an unexpected prefix.
 */
export function decodeAddress(address: string): Uint8Array {
  const { prefix, words } = bech32.decode(address as `${string}1${string}`, 90);
  if (prefix !== 'nova') {
    throw new Error(`decodeAddress: unexpected prefix "${prefix}" (expected "nova")`);
  }
  return new Uint8Array(bech32.fromWords(words));
}

/**
 * Generate a nonce from the current high-resolution timestamp.
 *
 * This is *not* cryptographically random — it is intentionally monotonic
 * so that the nonce can double as a rough ordering guarantee.
 */
export function generateNonce(): number {
  return Date.now();
}

/**
 * Promise-based sleep.
 */
export function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
