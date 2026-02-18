/**
 * NOVA Protocol — Wallet Management
 *
 * High-level wallet abstraction that wraps key management, address
 * derivation, transaction building, and RPC interaction in a single
 * ergonomic class.
 */

import {
  createNovaId,
  generateKeypair,
  keypairFromSeed,
  signMessage as signRaw,
} from './identity.js';
import { signTransaction, TransactionBuilder } from './transaction.js';
import type {
  NovaId,
  PublicKey,
  SecretKey,
  Signature,
  SignedTransaction,
  Transaction,
} from './types.js';
import { rpcCall } from './rpc.js';

// ---------------------------------------------------------------------------
// NovaWallet
// ---------------------------------------------------------------------------

export class NovaWallet {
  /** The bech32-encoded NOVA ID of this wallet. */
  public readonly address: NovaId;
  /** The Ed25519 public key of this wallet. */
  public readonly publicKey: PublicKey;

  private readonly _secretKey: SecretKey;

  private constructor(publicKey: PublicKey, secretKey: SecretKey) {
    this.publicKey = publicKey;
    this._secretKey = secretKey;
    this.address = createNovaId(publicKey);
  }

  // -----------------------------------------------------------------------
  // Factory methods
  // -----------------------------------------------------------------------

  /** Create a brand-new wallet with a random keypair. */
  static create(): NovaWallet {
    const { publicKey, secretKey } = generateKeypair();
    return new NovaWallet(publicKey, secretKey);
  }

  /** Derive a wallet deterministically from a 32-byte seed. */
  static fromSeed(seed: Uint8Array): NovaWallet {
    const { publicKey, secretKey } = keypairFromSeed(seed);
    return new NovaWallet(publicKey, secretKey);
  }

  // -----------------------------------------------------------------------
  // Signing
  // -----------------------------------------------------------------------

  /** Sign an arbitrary message with the wallet's secret key. */
  sign(message: Uint8Array): Signature {
    return signRaw(this._secretKey, message);
  }

  // -----------------------------------------------------------------------
  // Transaction helpers
  // -----------------------------------------------------------------------

  /**
   * Build and sign a simple transfer transaction.
   *
   * @param to       — Recipient NOVA ID.
   * @param amount   — Amount in atomic units.
   * @param currency — Token identifier (default "NOVA").
   */
  buildTransfer(to: NovaId, amount: bigint, currency = 'NOVA'): SignedTransaction {
    const tx = new TransactionBuilder()
      .type('transfer')
      .sender(this.address)
      .receiver(to)
      .amount(amount, currency)
      .build();

    return signTransaction(tx, this._secretKey, this.publicKey);
  }

  // -----------------------------------------------------------------------
  // RPC queries
  // -----------------------------------------------------------------------

  /**
   * Fetch the balance of a specific token from a NOVA node.
   *
   * @param nodeUrl — JSON-RPC endpoint (e.g. "https://rpc.nova.network").
   * @param tokenId — Token identifier; defaults to "NOVA".
   */
  async getBalance(nodeUrl: string, tokenId = 'NOVA'): Promise<bigint> {
    const result = await rpcCall<{ balance: string }>(nodeUrl, 'nova_getBalance', [
      this.address,
      tokenId,
    ]);
    return BigInt(result.balance);
  }

  /**
   * Fetch the recent transaction history for this address.
   *
   * @param nodeUrl — JSON-RPC endpoint.
   */
  async getTransactionHistory(nodeUrl: string): Promise<Transaction[]> {
    return rpcCall<Transaction[]>(nodeUrl, 'nova_getTransactionHistory', [this.address]);
  }
}
