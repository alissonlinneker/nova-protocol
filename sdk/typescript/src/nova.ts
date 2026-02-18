/**
 * NOVA Protocol — Main Client
 *
 * `NovaClient` is the primary entry point for interacting with a NOVA
 * Protocol node over JSON-RPC.  All public methods are thin wrappers
 * around RPC calls with proper TypeScript typing and bigint hydration.
 */

import { sleep } from './utils.js';
import type {
  AccountState,
  Block,
  BlockHeader,
  NovaId,
  RpcResponse,
  SignedTransaction,
  Transaction,
  TransactionReceipt,
  TransactionStatus,
  ValidatorInfo,
} from './types.js';

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

function serializeSignedTx(stx: SignedTransaction): Record<string, unknown> {
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

// ---------------------------------------------------------------------------
// NovaClient
// ---------------------------------------------------------------------------

export class NovaClient {
  private readonly _nodeUrl: string;
  private _rpcId = 0;

  /**
   * @param nodeUrl — Full URL of a NOVA JSON-RPC endpoint
   *                  (e.g. `"https://rpc.nova.network"`).
   */
  constructor(nodeUrl: string) {
    if (!nodeUrl) {
      throw new Error('NovaClient: nodeUrl is required');
    }
    // Strip trailing slash for consistency.
    this._nodeUrl = nodeUrl.replace(/\/+$/, '');
  }

  // -----------------------------------------------------------------------
  // JSON-RPC transport
  // -----------------------------------------------------------------------

  private async rpc<T>(method: string, params: unknown[] = []): Promise<T> {
    const id = ++this._rpcId;
    const body = JSON.stringify({
      jsonrpc: '2.0',
      id,
      method,
      params,
    });

    const res = await fetch(this._nodeUrl, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body,
    });

    if (!res.ok) {
      throw new Error(`NovaClient RPC HTTP error: ${res.status} ${res.statusText}`);
    }

    const json = (await res.json()) as RpcResponse<T>;

    if (json.error) {
      throw new Error(`NovaClient RPC error ${json.error.code}: ${json.error.message}`);
    }

    return json.result as T;
  }

  // -----------------------------------------------------------------------
  // Chain queries
  // -----------------------------------------------------------------------

  /** Return the current block height of the chain. */
  async getBlockHeight(): Promise<number> {
    return this.rpc<number>('nova_blockHeight');
  }

  /** Fetch a full block (header + transactions) at a given height. */
  async getBlock(height: number): Promise<Block> {
    const raw = await this.rpc<{
      header: BlockHeader & { timestamp: number };
      transactions: Array<{
        id: string;
        type: string;
        sender: string;
        receiver: string;
        amount: { value: string; currency: string };
        fee: string;
        nonce: number;
        payload: string;
        timestamp: number;
      }>;
    }>('nova_getBlock', [height]);

    return {
      header: raw.header,
      transactions: raw.transactions.map((t) => ({
        id: t.id,
        type: t.type as Transaction['type'],
        sender: t.sender as NovaId,
        receiver: t.receiver as NovaId,
        amount: { value: BigInt(t.amount.value), currency: t.amount.currency },
        fee: BigInt(t.fee),
        nonce: t.nonce,
        payload: Uint8Array.from(Buffer.from(t.payload, 'base64')),
        timestamp: t.timestamp,
      })),
    };
  }

  /** Fetch a single transaction by its hash. */
  async getTransaction(hash: string): Promise<Transaction> {
    const t = await this.rpc<{
      id: string;
      type: string;
      sender: string;
      receiver: string;
      amount: { value: string; currency: string };
      fee: string;
      nonce: number;
      payload: string;
      timestamp: number;
    }>('nova_getTransaction', [hash]);

    return {
      id: t.id,
      type: t.type as Transaction['type'],
      sender: t.sender as NovaId,
      receiver: t.receiver as NovaId,
      amount: { value: BigInt(t.amount.value), currency: t.amount.currency },
      fee: BigInt(t.fee),
      nonce: t.nonce,
      payload: Uint8Array.from(Buffer.from(t.payload, 'base64')),
      timestamp: t.timestamp,
    };
  }

  // -----------------------------------------------------------------------
  // Account queries
  // -----------------------------------------------------------------------

  /** Fetch account state (nonce + all token balances). */
  async getAccountState(address: NovaId): Promise<AccountState> {
    const raw = await this.rpc<{
      nonce: number;
      balances: Record<string, string>;
    }>('nova_getAccountState', [address]);

    const balances = new Map<string, bigint>();
    for (const [token, val] of Object.entries(raw.balances)) {
      balances.set(token, BigInt(val));
    }

    return { nonce: raw.nonce, balances };
  }

  /** Fetch the balance of a specific token for the given address. */
  async getBalance(address: NovaId, tokenId = 'NOVA'): Promise<bigint> {
    const result = await this.rpc<{ balance: string }>('nova_getBalance', [address, tokenId]);
    return BigInt(result.balance);
  }

  // -----------------------------------------------------------------------
  // Transaction submission
  // -----------------------------------------------------------------------

  /**
   * Broadcast a signed transaction and return the transaction hash.
   *
   * @returns The transaction hash (hex).
   */
  async sendTransaction(signedTx: SignedTransaction): Promise<string> {
    return this.rpc<string>('nova_sendTransaction', [serializeSignedTx(signedTx)]);
  }

  /** Estimate the fee (in atomic NOVA) that a transaction would cost. */
  async estimateFee(tx: Transaction): Promise<bigint> {
    const raw = await this.rpc<{ fee: string }>('nova_estimateFee', [
      {
        type: tx.type,
        sender: tx.sender,
        receiver: tx.receiver,
        amount: { value: tx.amount.value.toString(), currency: tx.amount.currency },
        payloadSize: tx.payload.length,
      },
    ]);
    return BigInt(raw.fee);
  }

  /**
   * Poll the node until the given transaction is confirmed (or times out).
   *
   * @param txHash  — Transaction hash to watch.
   * @param timeout — Maximum wait in milliseconds (default 30 000).
   */
  async waitForConfirmation(txHash: string, timeout = 30_000): Promise<TransactionReceipt> {
    const deadline = Date.now() + timeout;

    while (Date.now() < deadline) {
      try {
        const receipt = await this.rpc<{
          transactionId: string;
          blockHeight: number;
          blockHash: string;
          status: string;
          gasUsed: string;
          timestamp: number;
        } | null>('nova_getTransactionReceipt', [txHash]);

        if (receipt && receipt.status !== 'pending') {
          return {
            transactionId: receipt.transactionId,
            blockHeight: receipt.blockHeight,
            blockHash: receipt.blockHash,
            status: receipt.status as TransactionStatus,
            gasUsed: BigInt(receipt.gasUsed),
            timestamp: receipt.timestamp,
          };
        }
      } catch {
        // Transaction may not exist in the mempool yet — keep polling.
      }

      await sleep(1_000);
    }

    throw new Error(
      `NovaClient: timed out waiting for confirmation of ${txHash} after ${timeout} ms`,
    );
  }

  // -----------------------------------------------------------------------
  // Network queries
  // -----------------------------------------------------------------------

  /** List current validators (active set + standby). */
  async getValidators(): Promise<ValidatorInfo[]> {
    const raw = await this.rpc<
      Array<{
        address: string;
        stake: string;
        isActive: boolean;
        commissionBps: number;
        blocksProposed: number;
      }>
    >('nova_getValidators');

    return raw.map((v) => ({
      address: v.address as NovaId,
      stake: BigInt(v.stake),
      isActive: v.isActive,
      commissionBps: v.commissionBps,
      blocksProposed: v.blocksProposed,
    }));
  }

  // -----------------------------------------------------------------------
  // WebSocket subscriptions
  // -----------------------------------------------------------------------

  /**
   * Open a WebSocket connection and stream new blocks as they are finalized.
   *
   * @param callback — Invoked for each new block header.
   * @returns A cleanup function that closes the connection.
   */
  subscribeToBlocks(callback: (header: BlockHeader) => void): () => void {
    // Derive the WS URL from the HTTP URL.
    const wsUrl = this._nodeUrl.replace(/^http/, 'ws') + '/ws';

    const ws = new WebSocket(wsUrl);
    let closed = false;

    ws.addEventListener('open', () => {
      ws.send(
        JSON.stringify({
          jsonrpc: '2.0',
          id: ++this._rpcId,
          method: 'nova_subscribe',
          params: ['newBlocks'],
        }),
      );
    });

    ws.addEventListener('message', (event) => {
      try {
        const data = JSON.parse(String(event.data)) as {
          params?: { result?: BlockHeader };
        };
        if (data.params?.result) {
          callback(data.params.result);
        }
      } catch {
        // Ignore malformed frames.
      }
    });

    ws.addEventListener('error', () => {
      // Errors will also trigger 'close'; nothing to do here.
    });

    return () => {
      if (!closed) {
        closed = true;
        ws.close();
      }
    };
  }
}
