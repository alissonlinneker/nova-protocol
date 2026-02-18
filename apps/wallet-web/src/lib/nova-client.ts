/**
 * NOVA Protocol Node Client
 *
 * Communicates with a NOVA node over REST and JSON-RPC endpoints.
 * Base URL is configured via VITE_NODE_URL (default: http://localhost:9741).
 */

import type { Network } from '../stores/walletStore';

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const DEFAULT_NODE_URL = 'http://localhost:9741';

function getBaseUrl(): string {
  try {
    return import.meta.env.VITE_NODE_URL || DEFAULT_NODE_URL;
  } catch {
    return DEFAULT_NODE_URL;
  }
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

export interface StatusResponse {
  version: string;
  network: string;
  block_height: number;
  peer_count: number;
  synced: boolean;
}

export interface AccountResponse {
  address: string;
  balance: number;
  nonce: number;
}

export interface TransactionResponse {
  hash: string;
  sender: string;
  recipient: string;
  amount: number;
  fee: number;
  block_height: number;
  status: string;
  timestamp: number;
}

export interface SendTransactionResponse {
  tx_hash: string;
  status: string;
}

export interface BlockInfo {
  height: number;
  hash: string;
  timestamp: number;
  txCount: number;
  validator: string;
}

// ---------------------------------------------------------------------------
// NovaClient
// ---------------------------------------------------------------------------

interface NovaClientConfig {
  nodeUrl: string;
  network: Network;
}

class NovaClient {
  private config: NovaClientConfig;
  private rpcId = 0;

  constructor(config: NovaClientConfig) {
    this.config = config;
  }

  get nodeUrl(): string {
    return this.config.nodeUrl;
  }

  setNetwork(network: Network, nodeUrl: string): void {
    this.config = { network, nodeUrl };
  }

  // -----------------------------------------------------------------------
  // REST endpoints
  // -----------------------------------------------------------------------

  /** GET /status - node status summary. */
  async getStatus(): Promise<StatusResponse> {
    const res = await this.fetchRest('/status');
    return res as StatusResponse;
  }

  /** GET /accounts/:address - account balance and nonce. */
  async getAccount(address: string): Promise<AccountResponse> {
    const res = await this.fetchRest(`/accounts/${address}`);
    return res as AccountResponse;
  }

  /** GET /transactions/:hash - transaction details. */
  async getTransaction(hash: string): Promise<TransactionResponse> {
    const res = await this.fetchRest(`/transactions/${hash}`);
    return res as TransactionResponse;
  }

  // -----------------------------------------------------------------------
  // JSON-RPC endpoints
  // -----------------------------------------------------------------------

  /** nova_blockHeight - current block height. */
  async getBlockHeight(): Promise<number> {
    return this.rpcCall<number>('nova_blockHeight');
  }

  /** nova_getBalance - balance for a given address. */
  async getBalance(address: string): Promise<number> {
    return this.rpcCall<number>('nova_getBalance', [address]);
  }

  /** nova_sendTransaction - broadcast a signed transaction. */
  async sendTransaction(signedTx: Record<string, unknown>): Promise<SendTransactionResponse> {
    return this.rpcCall<SendTransactionResponse>('nova_sendTransaction', [signedTx]);
  }

  /** Estimate the network fee for a transfer. */
  async estimateFee(_symbol: string): Promise<number> {
    try {
      const result = await this.rpcCall<{ fee: number }>('nova_estimateFee', [{ type: 'transfer' }]);
      return result.fee;
    } catch {
      // Fallback: default fee schedule when the node doesn't support estimation.
      return _symbol === 'NOVA' ? 0.001 : 0.0005;
    }
  }

  /** Retrieve latest block info for display. */
  async getLatestBlock(): Promise<BlockInfo> {
    try {
      const height = await this.getBlockHeight();
      return {
        height,
        hash: '',
        timestamp: Date.now(),
        txCount: 0,
        validator: '',
      };
    } catch {
      throw new Error('Failed to fetch latest block');
    }
  }

  // -----------------------------------------------------------------------
  // Transport
  // -----------------------------------------------------------------------

  private async fetchRest(path: string): Promise<unknown> {
    const url = `${this.config.nodeUrl}${path}`;
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 15_000);

    try {
      const res = await fetch(url, { signal: controller.signal });
      clearTimeout(timeout);

      if (!res.ok) {
        throw new Error(`HTTP ${res.status}: ${res.statusText}`);
      }
      return await res.json();
    } catch (err) {
      clearTimeout(timeout);
      if (err instanceof Error && err.name === 'AbortError') {
        throw new Error(`Request to ${url} timed out`);
      }
      throw err;
    }
  }

  private async rpcCall<T>(method: string, params: unknown[] = []): Promise<T> {
    const id = ++this.rpcId;
    const url = `${this.config.nodeUrl}/rpc`;
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 15_000);

    try {
      const res = await fetch(url, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ jsonrpc: '2.0', id, method, params }),
        signal: controller.signal,
      });

      clearTimeout(timeout);

      if (!res.ok) {
        throw new Error(`RPC HTTP ${res.status}: ${res.statusText}`);
      }

      const json = (await res.json()) as {
        jsonrpc: string;
        id: number;
        result?: T;
        error?: { code: number; message: string };
      };

      if (json.error) {
        throw new Error(`RPC error ${json.error.code}: ${json.error.message}`);
      }

      return json.result as T;
    } catch (err) {
      clearTimeout(timeout);
      if (err instanceof Error && err.name === 'AbortError') {
        throw new Error(`RPC call to ${method} timed out`);
      }
      throw err;
    }
  }
}

// ---------------------------------------------------------------------------
// Singleton
// ---------------------------------------------------------------------------

let clientInstance: NovaClient | null = null;

export function getNovaClient(config?: NovaClientConfig): NovaClient {
  if (!clientInstance) {
    clientInstance = new NovaClient(
      config ?? {
        nodeUrl: getBaseUrl(),
        network: 'mainnet',
      },
    );
  } else if (config) {
    clientInstance.setNetwork(config.network, config.nodeUrl);
  }
  return clientInstance;
}

export type { NovaClient, NovaClientConfig };
