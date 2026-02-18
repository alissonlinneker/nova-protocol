/**
 * NOVA Protocol — HTTP Client
 *
 * `NovaClient` provides a unified interface for communicating with a NOVA
 * Protocol node over both REST and JSON-RPC transports. All network calls
 * use the built-in `fetch` API (Node 18+) with configurable timeout and
 * automatic retry on transient failures.
 */

import type { SignedTransaction } from './types.js';

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

export interface NovaClientConfig {
  /** Base URL of the NOVA node (e.g. "https://rpc.nova.network"). */
  baseUrl: string;
  /** Request timeout in milliseconds. Defaults to 30 000. */
  timeout?: number;
  /** Number of retry attempts on transient failures. Defaults to 3. */
  retries?: number;
  /** Delay between retries in milliseconds. Defaults to 1 000. */
  retryDelay?: number;
}

// ---------------------------------------------------------------------------
// Response types — REST and JSON-RPC
// ---------------------------------------------------------------------------

export interface StatusResponse {
  version: string;
  network: string;
  block_height: number;
  peer_count: number;
  synced: boolean;
}

export interface BlockResponse {
  height: number;
  hash: string;
  parent_hash: string;
  proposer: string;
  tx_count: number;
  timestamp: number;
  state_root: string;
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

export interface AccountResponse {
  address: string;
  balance: number;
  nonce: number;
}

export interface SendTransactionResponse {
  tx_hash: string;
  status: string;
}

// ---------------------------------------------------------------------------
// Error hierarchy
// ---------------------------------------------------------------------------

/**
 * Base error for all `NovaClient` failures. Carries optional HTTP status
 * and JSON-RPC error details for programmatic inspection.
 */
export class NovaClientError extends Error {
  constructor(
    message: string,
    public readonly statusCode?: number,
    public readonly rpcError?: { code: number; message: string },
  ) {
    super(message);
    this.name = 'NovaClientError';
  }
}

/** Thrown when the node is unreachable or the connection is refused. */
export class NovaConnectionError extends NovaClientError {
  constructor(message: string) {
    super(message);
    this.name = 'NovaConnectionError';
  }
}

/** Thrown when a requested resource does not exist (HTTP 404). */
export class NovaNotFoundError extends NovaClientError {
  constructor(message: string, statusCode?: number) {
    super(message, statusCode);
    this.name = 'NovaNotFoundError';
  }
}

/** Thrown when the JSON-RPC layer returns an error object. */
export class NovaRpcError extends NovaClientError {
  constructor(
    message: string,
    rpcError: { code: number; message: string },
  ) {
    super(message, undefined, rpcError);
    this.name = 'NovaRpcError';
  }
}

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
  private readonly _baseUrl: string;
  private readonly _timeout: number;
  private readonly _retries: number;
  private readonly _retryDelay: number;
  private _rpcId = 0;

  /**
   * @param config — Either a full {@link NovaClientConfig} object or a bare
   *                 URL string. When a string is provided, default timeout
   *                 and retry settings are used.
   */
  constructor(config: NovaClientConfig | string) {
    if (typeof config === 'string') {
      if (!config) {
        throw new Error('NovaClient: baseUrl is required');
      }
      this._baseUrl = config.replace(/\/+$/, '');
      this._timeout = 30_000;
      this._retries = 3;
      this._retryDelay = 1_000;
    } else {
      if (!config.baseUrl) {
        throw new Error('NovaClient: baseUrl is required');
      }
      this._baseUrl = config.baseUrl.replace(/\/+$/, '');
      this._timeout = config.timeout ?? 30_000;
      this._retries = config.retries ?? 3;
      this._retryDelay = config.retryDelay ?? 1_000;
    }
  }

  // -----------------------------------------------------------------------
  // REST methods
  // -----------------------------------------------------------------------

  /**
   * Probe the node's liveness endpoint.
   *
   * Returns `true` if the node responds to `GET /health` with a 200
   * status, and `false` on any error (network, timeout, non-200, etc.).
   */
  async health(): Promise<boolean> {
    try {
      const res = await this.fetchWithRetry(`${this._baseUrl}/health`);
      return res.ok;
    } catch {
      return false;
    }
  }

  /** Retrieve the node's current status summary. */
  async getStatus(): Promise<StatusResponse> {
    const res = await this.fetchWithRetry(`${this._baseUrl}/status`);
    if (!res.ok) {
      throw new NovaClientError(
        `GET /status failed with HTTP ${res.status}`,
        res.status,
      );
    }
    return (await res.json()) as StatusResponse;
  }

  /** Fetch a block by its height. */
  async getBlock(height: number): Promise<BlockResponse> {
    const res = await this.fetchWithRetry(`${this._baseUrl}/blocks/${height}`);
    if (res.status === 404) {
      throw new NovaNotFoundError(
        `Block not found at height ${height}`,
        404,
      );
    }
    if (!res.ok) {
      throw new NovaClientError(
        `GET /blocks/${height} failed with HTTP ${res.status}`,
        res.status,
      );
    }
    return (await res.json()) as BlockResponse;
  }

  /** Fetch a transaction by its hex-encoded hash. */
  async getTransaction(hash: string): Promise<TransactionResponse> {
    const res = await this.fetchWithRetry(`${this._baseUrl}/transactions/${hash}`);
    if (res.status === 404) {
      throw new NovaNotFoundError(
        `Transaction not found: ${hash}`,
        404,
      );
    }
    if (!res.ok) {
      throw new NovaClientError(
        `GET /transactions/${hash} failed with HTTP ${res.status}`,
        res.status,
      );
    }
    return (await res.json()) as TransactionResponse;
  }

  /** Fetch account state for the given address. */
  async getAccount(address: string): Promise<AccountResponse> {
    const res = await this.fetchWithRetry(`${this._baseUrl}/accounts/${address}`);
    if (!res.ok) {
      throw new NovaClientError(
        `GET /accounts/${address} failed with HTTP ${res.status}`,
        res.status,
      );
    }
    return (await res.json()) as AccountResponse;
  }

  // -----------------------------------------------------------------------
  // JSON-RPC methods
  // -----------------------------------------------------------------------

  /** Return the current block height via JSON-RPC. */
  async getBlockHeight(): Promise<number> {
    return this.rpcCall<number>('nova_blockHeight');
  }

  /** Return the number of connected peers. */
  async getPeerCount(): Promise<number> {
    return this.rpcCall<number>('nova_peerCount');
  }

  /** Return the network identifier (e.g. "devnet", "testnet"). */
  async getNetworkId(): Promise<string> {
    return this.rpcCall<string>('nova_networkId');
  }

  /** Return the node software version. */
  async getVersion(): Promise<string> {
    return this.rpcCall<string>('nova_version');
  }

  /** Fetch the balance for the given address via JSON-RPC. */
  async getBalance(address: string): Promise<number> {
    return this.rpcCall<number>('nova_getBalance', [address]);
  }

  /**
   * Broadcast a signed transaction to the network.
   *
   * @returns The transaction hash and submission status.
   */
  async sendTransaction(tx: SignedTransaction): Promise<SendTransactionResponse> {
    return this.rpcCall<SendTransactionResponse>('nova_sendTransaction', [
      serializeSignedTx(tx),
    ]);
  }

  // -----------------------------------------------------------------------
  // Internal: JSON-RPC transport
  // -----------------------------------------------------------------------

  /**
   * Execute a single JSON-RPC 2.0 call against the node's `/rpc` endpoint.
   *
   * @param method — RPC method name (e.g. "nova_blockHeight").
   * @param params — Positional parameters.
   * @returns The typed `result` field from the RPC response.
   */
  private async rpcCall<T>(method: string, params: unknown[] = []): Promise<T> {
    const id = ++this._rpcId;
    const body = JSON.stringify({
      jsonrpc: '2.0',
      id,
      method,
      params,
    });

    let res: Response;
    try {
      res = await this.fetchWithRetry(`${this._baseUrl}/rpc`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body,
      });
    } catch (err) {
      if (err instanceof NovaClientError) throw err;
      throw new NovaConnectionError(
        `Failed to connect to ${this._baseUrl}/rpc: ${(err as Error).message}`,
      );
    }

    if (!res.ok) {
      throw new NovaClientError(
        `RPC HTTP error: ${res.status} ${res.statusText}`,
        res.status,
      );
    }

    const json = (await res.json()) as {
      jsonrpc: string;
      id: number | string;
      result?: T;
      error?: { code: number; message: string };
    };

    if (json.error) {
      throw new NovaRpcError(
        `RPC error ${json.error.code}: ${json.error.message}`,
        json.error,
      );
    }

    return json.result as T;
  }

  // -----------------------------------------------------------------------
  // Internal: HTTP transport with timeout & retry
  // -----------------------------------------------------------------------

  /**
   * Wrapper around `fetch` that applies an `AbortController` timeout and
   * retries the request on transient network failures.
   *
   * Retries are attempted for:
   *   - Network errors (DNS resolution, connection refused, etc.)
   *   - HTTP 502, 503, 504 (gateway / temporary unavailability)
   *
   * Non-retryable responses (4xx, other 5xx) are returned immediately so
   * the caller can inspect the status code.
   */
  private async fetchWithRetry(url: string, options?: RequestInit): Promise<Response> {
    let lastError: Error | undefined;

    for (let attempt = 0; attempt <= this._retries; attempt++) {
      const controller = new AbortController();
      const timer = setTimeout(() => controller.abort(), this._timeout);

      try {
        const res = await fetch(url, {
          ...options,
          signal: controller.signal,
        });

        clearTimeout(timer);

        // Retry on gateway-level transient errors.
        if (res.status === 502 || res.status === 503 || res.status === 504) {
          lastError = new NovaClientError(
            `HTTP ${res.status} from ${url}`,
            res.status,
          );
          if (attempt < this._retries) {
            await this.delay(this._retryDelay);
            continue;
          }
          return res;
        }

        return res;
      } catch (err) {
        clearTimeout(timer);
        lastError = err as Error;

        // On the last attempt, throw immediately.
        if (attempt < this._retries) {
          await this.delay(this._retryDelay);
        }
      }
    }

    throw new NovaConnectionError(
      `Failed to fetch ${url} after ${this._retries + 1} attempts: ${lastError?.message ?? 'unknown error'}`,
    );
  }

  /** Promise-based delay for retry back-off. */
  private delay(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }
}
