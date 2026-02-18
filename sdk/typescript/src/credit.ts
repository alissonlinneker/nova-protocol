/**
 * NOVA Protocol — Credit Marketplace
 *
 * Functions for interacting with the decentralized credit marketplace:
 * requesting credit, browsing offers, accepting offers, and querying
 * on-chain credit scores.
 */

import type {
  CreditOffer,
  CreditRequestParams,
  CreditScore,
  NovaId,
  TransactionReceipt,
} from './types.js';
import type { NovaWallet } from './wallet.js';
import { signTransaction, TransactionBuilder } from './transaction.js';
import { rpcCall } from './rpc.js';

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/**
 * Submit a credit request to the marketplace.
 *
 * The request is broadcast to all participating lenders. Matching offers
 * will be returned by {@link getOffers}.
 *
 * @returns The unique request ID assigned by the network.
 */
export async function requestCredit(
  nodeUrl: string,
  params: CreditRequestParams,
): Promise<string> {
  return rpcCall<string>(nodeUrl, 'nova_requestCredit', [
    {
      borrower: params.borrower,
      amount: {
        value: params.amount.value.toString(),
        currency: params.amount.currency,
      },
      maxInterestRateBps: params.maxInterestRateBps,
      termSeconds: params.termSeconds,
    },
  ]);
}

/**
 * Retrieve all offers that have been submitted for a given credit request.
 */
export async function getOffers(nodeUrl: string, requestId: string): Promise<CreditOffer[]> {
  const raw = await rpcCall<Array<{
    id: string;
    lender: string;
    maxAmount: { value: string; currency: string };
    interestRateBps: number;
    termSeconds: number;
    minCreditScore: number;
    expiresAt: number;
  }>>(nodeUrl, 'nova_getCreditOffers', [requestId]);

  // Hydrate bigint fields from their JSON-safe string representations.
  return raw.map((o) => ({
    id: o.id,
    lender: o.lender as NovaId,
    maxAmount: {
      value: BigInt(o.maxAmount.value),
      currency: o.maxAmount.currency,
    },
    interestRateBps: o.interestRateBps,
    termSeconds: o.termSeconds,
    minCreditScore: o.minCreditScore,
    expiresAt: o.expiresAt,
  }));
}

/**
 * Accept a specific credit offer by building and submitting a
 * `credit_request` transaction signed by the borrower's wallet.
 *
 * @returns The finalized transaction receipt.
 */
export async function acceptOffer(
  nodeUrl: string,
  requestId: string,
  offerId: string,
  wallet: NovaWallet,
): Promise<TransactionReceipt> {
  const encoder = new TextEncoder();
  const payloadData = JSON.stringify({ requestId, offerId });

  const tx = new TransactionBuilder()
    .type('credit_request')
    .sender(wallet.address)
    .receiver(wallet.address) // self-referencing; the lender is encoded in the offer
    .amount(0n, 'NOVA')
    .payload(encoder.encode(payloadData))
    .build();

  const signedTx = signTransaction(tx, wallet['_secretKey'], wallet.publicKey);

  const txHash = await rpcCall<string>(nodeUrl, 'nova_sendTransaction', [
    serializeSignedTx(signedTx),
  ]);

  // Poll until confirmation.
  return pollForReceipt(nodeUrl, txHash);
}

/**
 * Fetch the on-chain credit score for an address.
 */
export async function getCreditScore(
  nodeUrl: string,
  address: NovaId,
): Promise<CreditScore> {
  const raw = await rpcCall<{
    address: string;
    score: number;
    totalRepayments: number;
    totalDefaults: number;
    lastUpdated: number;
  }>(nodeUrl, 'nova_getCreditScore', [address]);

  return {
    address: raw.address as NovaId,
    score: raw.score,
    totalRepayments: raw.totalRepayments,
    totalDefaults: raw.totalDefaults,
    lastUpdated: raw.lastUpdated,
  };
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

function serializeSignedTx(stx: ReturnType<typeof signTransaction>): Record<string, unknown> {
  const tx = stx.transaction;
  return {
    transaction: {
      id: tx.id,
      type: tx.type,
      sender: tx.sender,
      receiver: tx.receiver,
      amount: { value: tx.amount.value.toString(), currency: tx.amount.currency },
      fee: tx.fee.toString(),
      nonce: tx.nonce,
      payload: Buffer.from(tx.payload).toString('base64'),
      timestamp: tx.timestamp,
    },
    signature: Buffer.from(stx.signature).toString('hex'),
    signerPublicKey: Buffer.from(stx.signerPublicKey).toString('hex'),
  };
}

const POLL_INTERVAL_MS = 1_000;
const POLL_TIMEOUT_MS = 30_000;

async function pollForReceipt(nodeUrl: string, txHash: string): Promise<TransactionReceipt> {
  const deadline = Date.now() + POLL_TIMEOUT_MS;

  while (Date.now() < deadline) {
    try {
      const receipt = await rpcCall<{
        transactionId: string;
        blockHeight: number;
        blockHash: string;
        status: string;
        gasUsed: string;
        timestamp: number;
      } | null>(nodeUrl, 'nova_getTransactionReceipt', [txHash]);

      if (receipt && receipt.status !== 'pending') {
        return {
          transactionId: receipt.transactionId,
          blockHeight: receipt.blockHeight,
          blockHash: receipt.blockHash,
          status: receipt.status as TransactionReceipt['status'],
          gasUsed: BigInt(receipt.gasUsed),
          timestamp: receipt.timestamp,
        };
      }
    } catch {
      // Node may return an error while the tx is still in the mempool.
    }

    await new Promise((r) => setTimeout(r, POLL_INTERVAL_MS));
  }

  throw new Error(`Timed out waiting for transaction ${txHash} after ${POLL_TIMEOUT_MS} ms`);
}
