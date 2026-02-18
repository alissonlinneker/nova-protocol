/**
 * NOVA Protocol — Transaction Building, Signing & Verification
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
// Transaction ID computation
// ---------------------------------------------------------------------------

/**
 * Deterministically compute a transaction ID by SHA-256 hashing the
 * canonical byte representation of the transaction body.
 *
 * The canonical form is:
 *   type | sender | receiver | amount.value (BE u64) | amount.currency |
 *   fee (BE u64) | nonce (BE u32) | timestamp (BE u64) | payload
 */
export function computeTransactionId(tx: Omit<Transaction, 'id'>): string {
  const encoder = new TextEncoder();

  const typeBytes = encoder.encode(tx.type);
  const senderBytes = encoder.encode(tx.sender);
  const receiverBytes = encoder.encode(tx.receiver);
  const currencyBytes = encoder.encode(tx.amount.currency);

  // Encode numeric fields as big-endian fixed-width buffers.
  const amountBuf = new ArrayBuffer(8);
  new DataView(amountBuf).setBigUint64(0, BigInt(tx.amount.value));

  const feeBuf = new ArrayBuffer(8);
  new DataView(feeBuf).setBigUint64(0, BigInt(tx.fee));

  const nonceBuf = new ArrayBuffer(4);
  new DataView(nonceBuf).setUint32(0, tx.nonce);

  const timestampBuf = new ArrayBuffer(8);
  new DataView(timestampBuf).setBigUint64(0, BigInt(tx.timestamp));

  // Concatenate all segments.
  const parts: Uint8Array[] = [
    typeBytes,
    senderBytes,
    receiverBytes,
    new Uint8Array(amountBuf),
    currencyBytes,
    new Uint8Array(feeBuf),
    new Uint8Array(nonceBuf),
    new Uint8Array(timestampBuf),
    tx.payload,
  ];

  const totalLength = parts.reduce((sum, p) => sum + p.length, 0);
  const buf = new Uint8Array(totalLength);
  let offset = 0;
  for (const part of parts) {
    buf.set(part, offset);
    offset += part.length;
  }

  const hash = sha256(buf);
  return bytesToHex(hash);
}

// ---------------------------------------------------------------------------
// Canonical signing message
// ---------------------------------------------------------------------------

/**
 * Build the message that is actually signed:
 *   SHA-256( "nova-tx:" | txId )
 */
function signingMessage(txId: string): Uint8Array {
  const encoder = new TextEncoder();
  const prefix = encoder.encode('nova-tx:');
  const id = encoder.encode(txId);
  const combined = new Uint8Array(prefix.length + id.length);
  combined.set(prefix, 0);
  combined.set(id, prefix.length);
  return sha256(combined);
}

// ---------------------------------------------------------------------------
// TransactionBuilder — fluent API
// ---------------------------------------------------------------------------

export class TransactionBuilder {
  private _type: TransactionType = 'transfer';
  private _sender: NovaId | undefined;
  private _receiver: NovaId | undefined;
  private _amount: Amount = { value: 0n, currency: 'NOVA' };
  private _fee: bigint = 0n;
  private _nonce: number | undefined;
  private _payload: Uint8Array = new Uint8Array(0);
  private _timestamp: number | undefined;

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
 */
export function signTransaction(
  tx: Transaction,
  secretKey: SecretKey,
  signerPublicKey: PublicKey,
): SignedTransaction {
  const msg = signingMessage(tx.id);
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
  // Re-derive the transaction ID to make sure it was not tampered with.
  const { id: _original, ...body } = signedTx.transaction;
  const recomputedId = computeTransactionId(body);

  if (recomputedId !== signedTx.transaction.id) {
    return false;
  }

  const msg = signingMessage(signedTx.transaction.id);
  return verifySignature(signedTx.signerPublicKey, msg, signedTx.signature);
}
