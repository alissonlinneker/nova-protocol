/**
 * NOVA Protocol — Transaction Building, Signing & Verification
 *
 * Wire format is aligned with the Rust protocol crate so that transactions
 * built in any SDK can be verified by the Rust validator node. The canonical
 * binary serialization (signable bytes) uses little-endian integers and null-
 * byte separators — see {@link signableBytes} for the exact layout.
 */

import { sha256 } from '@noble/hashes/sha256';
import { signMessage, verifySignature } from './identity.js';
import type {
  Amount,
  NovaId,
  PublicKey,
  SecretKey,
  Signature,
  SignedTransaction,
  Transaction,
  TransactionType,
} from './types.js';
import { bytesToHex, generateNonce } from './utils.js';

// ---------------------------------------------------------------------------
// Transaction type mapping
// ---------------------------------------------------------------------------

/**
 * Map SDK transaction type strings to the Rust `TransactionType::Display`
 * format used in the canonical byte serialization. The Rust side uses
 * PascalCase (e.g. "Transfer"), while the SDK uses snake_case for JSON
 * ergonomics.
 */
const TX_TYPE_WIRE: Record<TransactionType, string> = {
  transfer: 'Transfer',
  credit_request: 'CreditRequest',
  credit_settlement: 'CreditSettlement',
  token_mint: 'TokenMint',
  token_burn: 'TokenBurn',
};

// ---------------------------------------------------------------------------
// Canonical signable bytes — matches Rust Transaction::signable_bytes()
// ---------------------------------------------------------------------------

/**
 * Produce the canonical byte representation used for signing and transaction
 * ID computation. This must match `Transaction::signable_bytes()` in the Rust
 * protocol crate byte-for-byte.
 *
 * Layout:
 *   version        — 2 bytes, little-endian u16
 *   tx_type        — UTF-8 PascalCase string + 0x00 separator
 *   sender         — UTF-8 address string + 0x00 separator
 *   receiver       — UTF-8 address string + 0x00 separator
 *   amount.value   — 8 bytes, little-endian u64
 *   amount.currency— UTF-8 string + 0x00 separator
 *   fee            — 8 bytes, little-endian u64
 *   nonce          — 8 bytes, little-endian u64
 *   timestamp      — 8 bytes, little-endian u64
 *   payload        — if present: 0x01 + 4-byte LE u32 length + raw bytes
 *                    if absent:  0x00
 *
 * Fields excluded: id, signature, sender_public_key, zkp_proof.
 */
export function signableBytes(tx: Omit<Transaction, 'id'>): Uint8Array {
  const encoder = new TextEncoder();
  const parts: Uint8Array[] = [];

  // Protocol version (2 bytes, LE).
  const versionBuf = new Uint8Array(2);
  new DataView(versionBuf.buffer).setUint16(0, tx.version, true);
  parts.push(versionBuf);

  // Transaction type discriminant (PascalCase) + null separator.
  parts.push(encoder.encode(TX_TYPE_WIRE[tx.type]));
  parts.push(new Uint8Array([0x00]));

  // Sender address + null separator.
  parts.push(encoder.encode(tx.sender));
  parts.push(new Uint8Array([0x00]));

  // Receiver address + null separator.
  parts.push(encoder.encode(tx.receiver));
  parts.push(new Uint8Array([0x00]));

  // Amount value (8 bytes, LE u64).
  const amountBuf = new Uint8Array(8);
  new DataView(amountBuf.buffer).setBigUint64(0, BigInt(tx.amount.value), true);
  parts.push(amountBuf);

  // Amount currency string + null separator.
  parts.push(encoder.encode(tx.amount.currency));
  parts.push(new Uint8Array([0x00]));

  // Fee (8 bytes, LE u64).
  const feeBuf = new Uint8Array(8);
  new DataView(feeBuf.buffer).setBigUint64(0, BigInt(tx.fee), true);
  parts.push(feeBuf);

  // Nonce (8 bytes, LE u64).
  const nonceBuf = new Uint8Array(8);
  new DataView(nonceBuf.buffer).setBigUint64(0, BigInt(tx.nonce), true);
  parts.push(nonceBuf);

  // Timestamp (8 bytes, LE u64).
  const timestampBuf = new Uint8Array(8);
  new DataView(timestampBuf.buffer).setBigUint64(0, BigInt(tx.timestamp), true);
  parts.push(timestampBuf);

  // Payload: length-prefixed if present, single 0x00 flag if absent.
  if (tx.payload.length > 0) {
    parts.push(new Uint8Array([0x01]));
    const lenBuf = new Uint8Array(4);
    new DataView(lenBuf.buffer).setUint32(0, tx.payload.length, true);
    parts.push(lenBuf);
    parts.push(tx.payload);
  } else {
    parts.push(new Uint8Array([0x00]));
  }

  // Concatenate all segments into a single buffer.
  const totalLength = parts.reduce((sum, p) => sum + p.length, 0);
  const buf = new Uint8Array(totalLength);
  let offset = 0;
  for (const part of parts) {
    buf.set(part, offset);
    offset += part.length;
  }

  return buf;
}

// ---------------------------------------------------------------------------
// Transaction ID computation
// ---------------------------------------------------------------------------

/**
 * Compute the canonical transaction ID: `hex(double_sha256(signable_bytes))`.
 *
 * This matches the Rust `Transaction::compute_id()` implementation, which
 * applies SHA-256 twice to protect against length-extension attacks.
 */
export function computeTransactionId(tx: Omit<Transaction, 'id'>): string {
  const bytes = signableBytes(tx);
  const hash = sha256(sha256(bytes));
  return bytesToHex(hash);
}

// ---------------------------------------------------------------------------
// TransactionBuilder — fluent API
// ---------------------------------------------------------------------------

export class TransactionBuilder {
  private _version: number = 1;
  private _type: TransactionType = 'transfer';
  private _sender: NovaId | undefined;
  private _receiver: NovaId | undefined;
  private _amount: Amount = { value: 0n, currency: 'NOVA' };
  private _fee: bigint = 0n;
  private _nonce: number | undefined;
  private _payload: Uint8Array = new Uint8Array(0);
  private _timestamp: number | undefined;

  /** Set the protocol version (default: 1). */
  version(v: number): this {
    this._version = v;
    return this;
  }

  /** Set the transaction type. */
  type(txType: TransactionType): this {
    this._type = txType;
    return this;
  }

  /** Set the sender address. */
  sender(address: NovaId): this {
    this._sender = address;
    return this;
  }

  /** Set the receiver address. */
  receiver(address: NovaId): this {
    this._receiver = address;
    return this;
  }

  /** Set the transfer amount and currency. */
  amount(value: bigint, currency = 'NOVA'): this {
    this._amount = { value, currency };
    return this;
  }

  /** Set the transaction fee (in atomic NOVA units). */
  fee(value: bigint): this {
    this._fee = value;
    return this;
  }

  /** Set the nonce explicitly. If omitted, `build()` generates one. */
  nonce(n: number): this {
    this._nonce = n;
    return this;
  }

  /** Attach an arbitrary payload. */
  payload(data: Uint8Array): this {
    this._payload = data;
    return this;
  }

  /** Override the timestamp. If omitted, `build()` uses `Date.now()`. */
  timestamp(ts: number): this {
    this._timestamp = ts;
    return this;
  }

  /**
   * Finalize and return an unsigned {@link Transaction}.
   *
   * @throws {Error} If required fields (sender, receiver) are missing.
   */
  build(): Transaction {
    if (!this._sender) throw new Error('TransactionBuilder: sender is required');
    if (!this._receiver) throw new Error('TransactionBuilder: receiver is required');

    const partial = {
      version: this._version,
      type: this._type,
      sender: this._sender,
      receiver: this._receiver,
      amount: this._amount,
      fee: this._fee,
      nonce: this._nonce ?? generateNonce(),
      payload: this._payload,
      timestamp: this._timestamp ?? Date.now(),
    };

    const id = computeTransactionId(partial);

    return { ...partial, id };
  }
}

// ---------------------------------------------------------------------------
// Signing & verification
// ---------------------------------------------------------------------------

/**
 * Sign a transaction with the given secret key and return a
 * {@link SignedTransaction} bundle.
 *
 * The signing message is the raw canonical signable bytes (matching the Rust
 * validator), NOT a hash of the transaction ID.
 */
export function signTransaction(
  tx: Transaction,
  secretKey: SecretKey,
  signerPublicKey: PublicKey,
): SignedTransaction {
  const { id: _id, ...body } = tx;
  const msg = signableBytes(body);
  const signature: Signature = signMessage(secretKey, msg);

  return {
    transaction: tx,
    signature,
    signerPublicKey,
  };
}

/**
 * Verify that a signed transaction's signature is valid for the
 * embedded signer public key, and that the transaction ID is consistent.
 */
export function verifyTransaction(signedTx: SignedTransaction): boolean {
  // Re-derive the transaction ID to detect tampering.
  const { id: _original, ...body } = signedTx.transaction;
  const recomputedId = computeTransactionId(body);

  if (recomputedId !== signedTx.transaction.id) {
    return false;
  }

  // Verify the Ed25519 signature over the raw signable bytes.
  const msg = signableBytes(body);
  return verifySignature(signedTx.signerPublicKey, msg, signedTx.signature);
}
