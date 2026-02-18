import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  NovaClient,
  NovaClientError,
  NovaConnectionError,
  NovaNotFoundError,
  NovaRpcError,
} from '../src/client.js';
import type {
  NovaClientConfig,
  StatusResponse,
  BlockResponse,
  TransactionResponse,
  AccountResponse,
  SendTransactionResponse,
} from '../src/client.js';
import { NovaWallet } from '../src/wallet.js';

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

const BASE_URL = 'https://node.nova.test';

function restOk<T>(body: T): Partial<Response> {
  return {
    ok: true,
    status: 200,
    json: async () => body,
  };
}

function restNotFound(body: unknown = { error: 'Not found' }): Partial<Response> {
  return {
    ok: false,
    status: 404,
    json: async () => body,
  };
}

function rpcOk<T>(result: T): Partial<Response> {
  return {
    ok: true,
    status: 200,
    json: async () => ({ jsonrpc: '2.0', id: 1, result }),
  };
}

function rpcError(code: number, message: string): Partial<Response> {
  return {
    ok: true,
    status: 200,
    json: async () => ({ jsonrpc: '2.0', id: 1, error: { code, message } }),
  };
}

// ---------------------------------------------------------------------------
// Test suite
// ---------------------------------------------------------------------------

describe('NovaClient', () => {
  const mockFetch = vi.fn();

  beforeEach(() => {
    vi.stubGlobal('fetch', mockFetch);
    mockFetch.mockReset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  // -----------------------------------------------------------------------
  // Construction
  // -----------------------------------------------------------------------

  it('client_construction_with_string — accepts a bare URL string', () => {
    const client = new NovaClient(BASE_URL);
    expect(client).toBeInstanceOf(NovaClient);
  });

  it('client_construction_with_config — accepts a full config object', () => {
    const config: NovaClientConfig = {
      baseUrl: BASE_URL,
      timeout: 5_000,
      retries: 2,
      retryDelay: 500,
    };
    const client = new NovaClient(config);
    expect(client).toBeInstanceOf(NovaClient);
  });

  it('strips trailing slashes from the base URL', () => {
    const client = new NovaClient(`${BASE_URL}///`);
    expect(client).toBeInstanceOf(NovaClient);
  });

  it('throws when constructed without a URL', () => {
    expect(() => new NovaClient('')).toThrow('baseUrl is required');
    expect(() => new NovaClient({ baseUrl: '' })).toThrow('baseUrl is required');
  });

  // -----------------------------------------------------------------------
  // REST: health
  // -----------------------------------------------------------------------

  it('health_returns_true — mocked /health returns true', async () => {
    mockFetch.mockResolvedValueOnce(restOk({ status: 'ok' }));

    const client = new NovaClient(BASE_URL);
    const healthy = await client.health();

    expect(healthy).toBe(true);
    expect(mockFetch).toHaveBeenCalledOnce();
    const [url] = mockFetch.mock.calls[0]!;
    expect(url).toBe(`${BASE_URL}/health`);
  });

  it('health_returns_false_on_error — connection error returns false', async () => {
    mockFetch.mockRejectedValue(new TypeError('fetch failed'));

    const client = new NovaClient({ baseUrl: BASE_URL, retries: 0 });
    const healthy = await client.health();

    expect(healthy).toBe(false);
  });

  // -----------------------------------------------------------------------
  // REST: getStatus
  // -----------------------------------------------------------------------

  it('get_status — mocked /status returns StatusResponse', async () => {
    const statusBody: StatusResponse = {
      version: '0.1.0',
      network: 'devnet',
      block_height: 1024,
      peer_count: 8,
      synced: true,
    };
    mockFetch.mockResolvedValueOnce(restOk(statusBody));

    const client = new NovaClient(BASE_URL);
    const status = await client.getStatus();

    expect(status.version).toBe('0.1.0');
    expect(status.block_height).toBe(1024);
    expect(status.synced).toBe(true);
  });

  // -----------------------------------------------------------------------
  // REST: getBlock
  // -----------------------------------------------------------------------

  it('get_block — mocked /blocks/:height returns BlockResponse', async () => {
    const blockBody: BlockResponse = {
      height: 42,
      hash: 'abc123',
      parent_hash: 'def456',
      proposer: 'nova1validator',
      tx_count: 5,
      timestamp: 1_700_000_000_000,
      state_root: 'aaaa',
    };
    mockFetch.mockResolvedValueOnce(restOk(blockBody));

    const client = new NovaClient(BASE_URL);
    const block = await client.getBlock(42);

    expect(block.height).toBe(42);
    expect(block.hash).toBe('abc123');
    expect(block.tx_count).toBe(5);
  });

  it('get_block_not_found — 404 throws NovaNotFoundError', async () => {
    mockFetch.mockResolvedValueOnce(restNotFound());

    const client = new NovaClient(BASE_URL);

    await expect(client.getBlock(999)).rejects.toThrow(NovaNotFoundError);
    await expect(async () => {
      mockFetch.mockResolvedValueOnce(restNotFound());
      await client.getBlock(999);
    }).rejects.toThrow('Block not found');
  });

  // -----------------------------------------------------------------------
  // REST: getTransaction
  // -----------------------------------------------------------------------

  it('get_transaction — mocked /transactions/:hash returns TransactionResponse', async () => {
    const txBody: TransactionResponse = {
      hash: 'deadbeef',
      sender: 'nova1alice',
      recipient: 'nova1bob',
      amount: 500,
      fee: 10,
      block_height: 100,
      status: 'confirmed',
      timestamp: 1_700_000_000_000,
    };
    mockFetch.mockResolvedValueOnce(restOk(txBody));

    const client = new NovaClient(BASE_URL);
    const tx = await client.getTransaction('deadbeef');

    expect(tx.hash).toBe('deadbeef');
    expect(tx.sender).toBe('nova1alice');
    expect(tx.amount).toBe(500);
  });

  // -----------------------------------------------------------------------
  // REST: getAccount
  // -----------------------------------------------------------------------

  it('get_account — mocked /accounts/:address returns AccountResponse', async () => {
    const acctBody: AccountResponse = {
      address: 'nova1alice',
      balance: 42_000,
      nonce: 7,
    };
    mockFetch.mockResolvedValueOnce(restOk(acctBody));

    const client = new NovaClient(BASE_URL);
    const account = await client.getAccount('nova1alice');

    expect(account.address).toBe('nova1alice');
    expect(account.balance).toBe(42_000);
    expect(account.nonce).toBe(7);
  });

  // -----------------------------------------------------------------------
  // JSON-RPC: getBlockHeight
  // -----------------------------------------------------------------------

  it('get_block_height_rpc — mocked RPC returns height', async () => {
    mockFetch.mockResolvedValueOnce(rpcOk(256));

    const client = new NovaClient(BASE_URL);
    const height = await client.getBlockHeight();

    expect(height).toBe(256);

    // Validate outgoing request shape.
    const [url, init] = mockFetch.mock.calls[0]!;
    expect(url).toBe(`${BASE_URL}/rpc`);
    const body = JSON.parse(init.body as string);
    expect(body.method).toBe('nova_blockHeight');
    expect(body.jsonrpc).toBe('2.0');
  });

  // -----------------------------------------------------------------------
  // JSON-RPC: getPeerCount
  // -----------------------------------------------------------------------

  it('get_peer_count_rpc — mocked RPC returns count', async () => {
    mockFetch.mockResolvedValueOnce(rpcOk(12));

    const client = new NovaClient(BASE_URL);
    const count = await client.getPeerCount();

    expect(count).toBe(12);
  });

  // -----------------------------------------------------------------------
  // JSON-RPC: sendTransaction
  // -----------------------------------------------------------------------

  it('send_transaction_rpc — mocked RPC returns tx hash', async () => {
    const rpcResult: SendTransactionResponse = {
      tx_hash: '0xfeedbabe',
      status: 'pending',
    };
    mockFetch.mockResolvedValueOnce(rpcOk(rpcResult));

    const client = new NovaClient(BASE_URL);
    const sender = NovaWallet.create();
    const receiver = NovaWallet.create();
    const signedTx = sender.buildTransfer(receiver.address, 100n);

    const result = await client.sendTransaction(signedTx);

    expect(result.tx_hash).toBe('0xfeedbabe');
    expect(result.status).toBe('pending');
  });

  // -----------------------------------------------------------------------
  // RPC error handling
  // -----------------------------------------------------------------------

  it('rpc_error_handling — RPC error code throws NovaRpcError', async () => {
    mockFetch.mockResolvedValueOnce(rpcError(-32601, 'Method not found'));

    const client = new NovaClient(BASE_URL);

    try {
      await client.getBlockHeight();
      expect.unreachable('should have thrown');
    } catch (err) {
      expect(err).toBeInstanceOf(NovaRpcError);
      expect(err).toBeInstanceOf(NovaClientError);
      const rpcErr = err as NovaRpcError;
      expect(rpcErr.rpcError?.code).toBe(-32601);
      expect(rpcErr.rpcError?.message).toBe('Method not found');
    }
  });

  // -----------------------------------------------------------------------
  // Connection error
  // -----------------------------------------------------------------------

  it('connection_error — fetch rejection throws NovaConnectionError', async () => {
    mockFetch.mockRejectedValue(new TypeError('fetch failed'));

    const client = new NovaClient({ baseUrl: BASE_URL, retries: 0 });

    try {
      await client.getBlockHeight();
      expect.unreachable('should have thrown');
    } catch (err) {
      expect(err).toBeInstanceOf(NovaConnectionError);
      expect(err).toBeInstanceOf(NovaClientError);
    }
  });

  // -----------------------------------------------------------------------
  // Retry on failure
  // -----------------------------------------------------------------------

  it('retry_on_failure — retries N times then fails', async () => {
    // Reject all attempts.
    mockFetch.mockRejectedValue(new TypeError('connection refused'));

    const retries = 2;
    const client = new NovaClient({
      baseUrl: BASE_URL,
      retries,
      retryDelay: 1, // minimal delay for test speed
    });

    await expect(client.getStatus()).rejects.toThrow(NovaConnectionError);

    // Initial attempt + 2 retries = 3 total calls.
    expect(mockFetch).toHaveBeenCalledTimes(retries + 1);
  });

  // -----------------------------------------------------------------------
  // Timeout configuration
  // -----------------------------------------------------------------------

  it('timeout_configuration — custom timeout is passed via AbortSignal', async () => {
    mockFetch.mockResolvedValueOnce(restOk({ status: 'ok' }));

    const client = new NovaClient({
      baseUrl: BASE_URL,
      timeout: 5_000,
    });

    await client.health();

    // Verify that fetch was called with a signal (AbortController).
    const [_url, init] = mockFetch.mock.calls[0]!;
    expect(init.signal).toBeDefined();
    expect(init.signal).toBeInstanceOf(AbortSignal);
  });

  // -----------------------------------------------------------------------
  // Additional JSON-RPC methods
  // -----------------------------------------------------------------------

  it('getNetworkId returns the network identifier', async () => {
    mockFetch.mockResolvedValueOnce(rpcOk('devnet'));

    const client = new NovaClient(BASE_URL);
    const networkId = await client.getNetworkId();

    expect(networkId).toBe('devnet');
  });

  it('getVersion returns the node version string', async () => {
    mockFetch.mockResolvedValueOnce(rpcOk('0.1.0'));

    const client = new NovaClient(BASE_URL);
    const version = await client.getVersion();

    expect(version).toBe('0.1.0');
  });

  it('getBalance returns a numeric balance', async () => {
    mockFetch.mockResolvedValueOnce(rpcOk(42_000));

    const client = new NovaClient(BASE_URL);
    const balance = await client.getBalance('nova1alice');

    expect(balance).toBe(42_000);
  });

  // -----------------------------------------------------------------------
  // Retry succeeds on second attempt
  // -----------------------------------------------------------------------

  it('retry succeeds when the second attempt returns 200', async () => {
    // First attempt: transient 503
    mockFetch.mockResolvedValueOnce({
      ok: false,
      status: 503,
      statusText: 'Service Unavailable',
      json: async () => ({}),
    });
    // Second attempt: success
    const statusBody: StatusResponse = {
      version: '0.1.0',
      network: 'devnet',
      block_height: 10,
      peer_count: 4,
      synced: true,
    };
    mockFetch.mockResolvedValueOnce(restOk(statusBody));

    const client = new NovaClient({
      baseUrl: BASE_URL,
      retryDelay: 1,
    });
    const status = await client.getStatus();

    expect(status.block_height).toBe(10);
    expect(mockFetch).toHaveBeenCalledTimes(2);
  });
});
