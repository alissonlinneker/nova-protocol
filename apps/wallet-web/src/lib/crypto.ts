/**
 * Cryptographic utility functions for key generation, signing, and hashing.
 *
 * Uses Ed25519 for signing (NOVA protocol standard) via a lightweight
 * implementation on top of the Web Crypto API for hashing, and raw
 * byte manipulation for key encoding.
 *
 * NOTE: In production, the actual Ed25519 operations will be delegated
 * to the TypeScript SDK (@nova-protocol/sdk) which uses @noble/ed25519.
 * This module provides the wallet-side interface and address derivation.
 */

export async function generateKeyPair(): Promise<{
  publicKey: string;
  privateKey: string;
}> {
  // Generate 32 random bytes as the Ed25519 seed (private key).
  // The public key is derived deterministically from the seed.
  // In production, this delegates to @noble/ed25519.
  const seed = new Uint8Array(32);
  window.crypto.getRandomValues(seed);

  // Derive a mock public key via SHA-512 truncation (matches Ed25519 key schedule).
  const hashBuffer = await window.crypto.subtle.digest("SHA-512", seed);
  const publicKeyBytes = new Uint8Array(hashBuffer).slice(0, 32);

  return {
    publicKey: bufferToHex(publicKeyBytes.buffer),
    privateKey: bufferToHex(seed.buffer),
  };
}

export async function signMessage(
  message: string,
  privateKeyHex: string
): Promise<string> {
  // Stub: in production, delegates to @noble/ed25519 sign().
  // For the MVP wallet UI, we produce a deterministic mock signature
  // derived from HMAC(key, message) to simulate signing behavior.
  const encoder = new TextEncoder();
  const keyBytes = hexToBuffer(privateKeyHex);

  const hmacKey = await window.crypto.subtle.importKey(
    "raw",
    keyBytes,
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"]
  );

  const signature = await window.crypto.subtle.sign(
    "HMAC",
    hmacKey,
    encoder.encode(message)
  );

  return bufferToHex(signature);
}

export async function sha256(message: string): Promise<string> {
  const encoder = new TextEncoder();
  const data = encoder.encode(message);
  const hash = await window.crypto.subtle.digest("SHA-256", data);
  return bufferToHex(hash);
}

export function deriveAddress(publicKeyHex: string): string {
  // Bech32 address derivation: take first 38 hex chars of the public key
  // as a simplified representation. In production, this uses proper
  // BLAKE3 hashing + Bech32 encoding via the SDK.
  const chars = publicKeyHex.slice(0, 38);
  return `nova1${chars}`;
}

export function truncateAddress(
  address: string,
  startChars = 10,
  endChars = 8
): string {
  if (address.length <= startChars + endChars + 3) return address;
  return `${address.slice(0, startChars)}...${address.slice(-endChars)}`;
}

export function generateRandomId(): string {
  const array = new Uint8Array(16);
  window.crypto.getRandomValues(array);
  return bufferToHex(array.buffer);
}

function bufferToHex(buffer: ArrayBuffer): string {
  return Array.from(new Uint8Array(buffer))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

function hexToBuffer(hex: string): ArrayBuffer {
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < hex.length; i += 2) {
    bytes[i / 2] = parseInt(hex.slice(i, i + 2), 16);
  }
  return bytes.buffer;
}
