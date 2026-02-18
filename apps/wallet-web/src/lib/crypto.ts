/**
 * Cryptographic utility functions for key generation, signing, and address
 * derivation.
 *
 * Uses Ed25519 via @noble/ed25519 (the same library used by the TS SDK) for
 * all signing operations, and @scure/base for bech32 address encoding.
 */

import * as ed from '@noble/ed25519';
import { sha512 } from '@noble/hashes/sha512';
import { sha256 } from '@noble/hashes/sha256';
import { bech32 } from '@scure/base';

// @noble/ed25519 v2 requires an explicit SHA-512 backend.
ed.etc.sha512Sync = (...msgs: Uint8Array[]): Uint8Array => {
  const h = sha512.create();
  for (const m of msgs) h.update(m);
  return h.digest();
};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface Keypair {
  publicKey: Uint8Array;
  secretKey: Uint8Array;
}

// ---------------------------------------------------------------------------
// Key generation & import
// ---------------------------------------------------------------------------

/**
 * Generate a fresh Ed25519 keypair from cryptographically-secure randomness.
 */
export function generateKeypair(): Keypair {
  const secretKey = ed.utils.randomPrivateKey();
  const publicKey = ed.getPublicKey(secretKey);
  return { publicKey, secretKey };
}

/**
 * Derive a keypair from a 32-byte secret key (seed).
 */
export function keypairFromSecretKey(secretKeyHex: string): Keypair {
  const secretKey = hexToBytes(secretKeyHex);
  if (secretKey.length !== 32) {
    throw new Error(`Expected 32-byte secret key, got ${secretKey.length}`);
  }
  const publicKey = ed.getPublicKey(secretKey);
  return { publicKey, secretKey };
}

// ---------------------------------------------------------------------------
// NOVA address (bech32 with "nova" HRP)
// ---------------------------------------------------------------------------

const NOVA_HRP = 'nova';
const BECH32_LIMIT = 90;

/**
 * Derive a NOVA bech32 address from a 32-byte Ed25519 public key.
 */
export function deriveAddress(publicKey: Uint8Array): string {
  if (publicKey.length !== 32) {
    throw new Error(`deriveAddress: expected 32-byte public key, got ${publicKey.length}`);
  }
  const words = bech32.toWords(publicKey);
  return bech32.encode(NOVA_HRP, words, BECH32_LIMIT);
}

/**
 * Validate that a string is a valid NOVA bech32 address.
 */
export function isValidAddress(address: string): boolean {
  try {
    const { prefix } = bech32.decode(address as `${string}1${string}`, BECH32_LIMIT);
    return prefix === NOVA_HRP;
  } catch {
    return false;
  }
}

// ---------------------------------------------------------------------------
// Signing
// ---------------------------------------------------------------------------

/**
 * Sign raw bytes with an Ed25519 secret key.
 * Returns a 64-byte signature.
 */
export function sign(message: Uint8Array, secretKey: Uint8Array): Uint8Array {
  return ed.sign(message, secretKey);
}

/**
 * Verify an Ed25519 signature.
 */
export function verify(
  message: Uint8Array,
  signature: Uint8Array,
  publicKey: Uint8Array,
): boolean {
  try {
    return ed.verify(signature, message, publicKey);
  } catch {
    return false;
  }
}

// ---------------------------------------------------------------------------
// Transaction signing helpers
// ---------------------------------------------------------------------------

/**
 * Build canonical signable bytes for a NOVA transaction. This layout must match
 * `Transaction::signable_bytes()` in the Rust protocol crate byte-for-byte.
 *
 * Layout:
 *   version        - 2 bytes LE u16
 *   tx_type        - UTF-8 PascalCase + 0x00
 *   sender         - UTF-8 address + 0x00
 *   receiver       - UTF-8 address + 0x00
 *   amount.value   - 8 bytes LE u64
 *   amount.currency- UTF-8 + 0x00
 *   fee            - 8 bytes LE u64
 *   nonce          - 8 bytes LE u64
 *   timestamp      - 8 bytes LE u64
 *   payload        - 0x01 + 4-byte LE u32 length + bytes (or 0x00 if empty)
 */

const TX_TYPE_WIRE: Record<string, string> = {
  transfer: 'Transfer',
  credit_request: 'CreditRequest',
  credit_settlement: 'CreditSettlement',
  token_mint: 'TokenMint',
  token_burn: 'TokenBurn',
};

export interface TransactionFields {
  version: number;
  type: string;
  sender: string;
  receiver: string;
  amountValue: bigint;
  amountCurrency: string;
  fee: bigint;
  nonce: number;
  timestamp: number;
  payload: Uint8Array;
}

export function signableBytes(tx: TransactionFields): Uint8Array {
  const encoder = new TextEncoder();
  const parts: Uint8Array[] = [];

  const versionBuf = new Uint8Array(2);
  new DataView(versionBuf.buffer).setUint16(0, tx.version, true);
  parts.push(versionBuf);

  parts.push(encoder.encode(TX_TYPE_WIRE[tx.type] ?? tx.type));
  parts.push(new Uint8Array([0x00]));

  parts.push(encoder.encode(tx.sender));
  parts.push(new Uint8Array([0x00]));

  parts.push(encoder.encode(tx.receiver));
  parts.push(new Uint8Array([0x00]));

  const amountBuf = new Uint8Array(8);
  new DataView(amountBuf.buffer).setBigUint64(0, tx.amountValue, true);
  parts.push(amountBuf);

  parts.push(encoder.encode(tx.amountCurrency));
  parts.push(new Uint8Array([0x00]));

  const feeBuf = new Uint8Array(8);
  new DataView(feeBuf.buffer).setBigUint64(0, tx.fee, true);
  parts.push(feeBuf);

  const nonceBuf = new Uint8Array(8);
  new DataView(nonceBuf.buffer).setBigUint64(0, BigInt(tx.nonce), true);
  parts.push(nonceBuf);

  const timestampBuf = new Uint8Array(8);
  new DataView(timestampBuf.buffer).setBigUint64(0, BigInt(tx.timestamp), true);
  parts.push(timestampBuf);

  if (tx.payload.length > 0) {
    parts.push(new Uint8Array([0x01]));
    const lenBuf = new Uint8Array(4);
    new DataView(lenBuf.buffer).setUint32(0, tx.payload.length, true);
    parts.push(lenBuf);
    parts.push(tx.payload);
  } else {
    parts.push(new Uint8Array([0x00]));
  }

  const totalLength = parts.reduce((sum, p) => sum + p.length, 0);
  const buf = new Uint8Array(totalLength);
  let offset = 0;
  for (const part of parts) {
    buf.set(part, offset);
    offset += part.length;
  }
  return buf;
}

/**
 * Compute a transaction ID: hex(double_sha256(signable_bytes)).
 */
export function computeTransactionId(tx: TransactionFields): string {
  const bytes = signableBytes(tx);
  const hash = sha256(sha256(bytes));
  return bytesToHex(hash);
}

/**
 * Build, sign, and return a complete signed transaction payload ready for
 * JSON-RPC submission.
 */
export function buildAndSignTransaction(params: {
  sender: string;
  receiver: string;
  amount: bigint;
  currency: string;
  fee: bigint;
  payload: Uint8Array;
  secretKey: Uint8Array;
  publicKey: Uint8Array;
}): {
  txId: string;
  signedTx: Record<string, unknown>;
} {
  const nonce = Date.now();
  const timestamp = Date.now();

  const txFields: TransactionFields = {
    version: 1,
    type: 'transfer',
    sender: params.sender,
    receiver: params.receiver,
    amountValue: params.amount,
    amountCurrency: params.currency,
    fee: params.fee,
    nonce,
    timestamp,
    payload: params.payload,
  };

  const txId = computeTransactionId(txFields);
  const msg = signableBytes(txFields);
  const signature = sign(msg, params.secretKey);

  const signedTx = {
    transaction: {
      id: txId,
      type: 'transfer',
      sender: params.sender,
      receiver: params.receiver,
      amount: { value: params.amount.toString(), currency: params.currency },
      fee: params.fee.toString(),
      nonce,
      payload: uint8ToBase64(params.payload),
      timestamp,
    },
    signature: bytesToHex(signature),
    signerPublicKey: bytesToHex(params.publicKey),
  };

  return { txId, signedTx };
}

// ---------------------------------------------------------------------------
// Encoding utilities
// ---------------------------------------------------------------------------

export function bytesToHex(bytes: Uint8Array): string {
  let hex = '';
  for (let i = 0; i < bytes.length; i++) {
    hex += (bytes[i]! >> 4).toString(16);
    hex += (bytes[i]! & 0x0f).toString(16);
  }
  return hex;
}

export function hexToBytes(hex: string): Uint8Array {
  const cleaned = hex.startsWith('0x') || hex.startsWith('0X') ? hex.slice(2) : hex;
  if (cleaned.length % 2 !== 0) {
    throw new Error(`hexToBytes: odd-length hex string (${cleaned.length} chars)`);
  }
  const bytes = new Uint8Array(cleaned.length / 2);
  for (let i = 0; i < bytes.length; i++) {
    bytes[i] = parseInt(cleaned.slice(i * 2, i * 2 + 2), 16);
  }
  return bytes;
}

export function truncateAddress(
  address: string,
  startChars = 10,
  endChars = 8,
): string {
  if (address.length <= startChars + endChars + 3) return address;
  return `${address.slice(0, startChars)}...${address.slice(-endChars)}`;
}

function uint8ToBase64(bytes: Uint8Array): string {
  let binary = '';
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i]!);
  }
  return btoa(binary);
}
