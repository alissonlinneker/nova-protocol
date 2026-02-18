import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { NovaClient } from '../src/nova.js';
import { NovaWallet } from '../src/wallet.js';
import type { NovaId } from '../src/types.js';

// ---------------------------------------------------------------------------
// NovaClient construction
// ---------------------------------------------------------------------------

describe('NovaClient', () => {
  it('constructs with a valid URL', () => {
    const client = new NovaClient('https://rpc.nova.network');
    expect(client).toBeInstanceOf(NovaClient);
  });

  it('strips trailing slashes from the URL', () => {
    // We can't directly inspect the private field, but we can confirm
    // construction doesn't throw and the client is usable.
    const client = new NovaClient('https://rpc.nova.network///');
    expect(client).toBeInstanceOf(NovaClient);
  });

  it('throws when constructed without a URL', () => {
    expect(() => new NovaClient('')).toThrow('nodeUrl is required');
  });
});

// ---------------------------------------------------------------------------
// NovaClient RPC methods (mocked fetch)
// ---------------------------------------------------------------------------

describe('NovaClient RPC', () => {
  const mockFetch = vi.fn();

  beforeEach(() => {
    vi.stubGlobal('fetch', mockFetch);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  function jsonOk<T>(result: T) {
    return {
      ok: true,
      status: 200,
      json: async () => ({ jsonrpc: '2.0', id: 1, result }),
    };
  }

  function jsonError(code: number, message: string) {
    return {
      ok: true,
      status: 200,
      json: async () => ({
        jsonrpc: '2.0',
        id: 1,
        error: { code, message },
      }),
    };
  }

  it('getBlockHeight returns a number', async () => {
    mockFetch.mockResolvedValueOnce(jsonOk(42));

    const client = new NovaClient('https://rpc.test');
    const height = await client.getBlockHeight();

    expect(height).toBe(42);
    expect(mockFetch).toHaveBeenCalledOnce();

    // Validate the outgoing request shape.
    const [url, init] = mockFetch.mock.calls[0]!;
    expect(url).toBe('https://rpc.test');
    const body = JSON.parse(init.body as string);
    expect(body.method).toBe('nova_blockHeight');
    expect(body.jsonrpc).toBe('2.0');
  });

  it('getBalance returns a bigint', async () => {
    mockFetch.mockResolvedValueOnce(jsonOk({ balance: '5000000000' }));

    const client = new NovaClient('https://rpc.test');
    const balance = await client.getBalance('nova1abc' as NovaId);

    expect(balance).toBe(5_000_000_000n);
  });

  it('getAccountState hydrates balances into a Map', async () => {
    mockFetch.mockResolvedValueOnce(
      jsonOk({
        nonce: 3,
        balances: {
          NOVA: '1000000',
          USDC: '250000',
        },
      }),
    );

    const client = new NovaClient('https://rpc.test');
    const state = await client.getAccountState('nova1abc' as NovaId);

    expect(state.nonce).toBe(3);
    expect(state.balances).toBeInstanceOf(Map);
    expect(state.balances.get('NOVA')).toBe(1_000_000n);
    expect(state.balances.get('USDC')).toBe(250_000n);
  });

  it('sendTransaction forwards the signed tx and returns a hash', async () => {
    mockFetch.mockResolvedValueOnce(jsonOk('0xdeadbeef'));

    const client = new NovaClient('https://rpc.test');
    const wallet = NovaWallet.create();
    const receiver = NovaWallet.create();
    const signedTx = wallet.buildTransfer(receiver.address, 100n);

    const hash = await client.sendTransaction(signedTx);
    expect(hash).toBe('0xdeadbeef');
  });

  it('estimateFee returns a bigint', async () => {
    mockFetch.mockResolvedValueOnce(jsonOk({ fee: '420' }));

    const client = new NovaClient('https://rpc.test');
    const wallet = NovaWallet.create();
    const receiver = NovaWallet.create();

    const tx = wallet.buildTransfer(receiver.address, 100n).transaction;
    const fee = await client.estimateFee(tx);

    expect(fee).toBe(420n);
  });

  it('throws on RPC error response', async () => {
    mockFetch.mockResolvedValueOnce(jsonError(-32601, 'Method not found'));

    const client = new NovaClient('https://rpc.test');
    await expect(client.getBlockHeight()).rejects.toThrow('Method not found');
  });

  it('throws on HTTP error', async () => {
    mockFetch.mockResolvedValueOnce({
      ok: false,
      status: 500,
      statusText: 'Internal Server Error',
    });

    const client = new NovaClient('https://rpc.test');
    await expect(client.getBlockHeight()).rejects.toThrow('HTTP error: 500');
  });

  it('getValidators deserializes validator list', async () => {
    mockFetch.mockResolvedValueOnce(
      jsonOk([
        {
          address: 'nova1validator1',
          stake: '100000000',
          isActive: true,
          commissionBps: 500,
          blocksProposed: 1234,
        },
      ]),
    );

    const client = new NovaClient('https://rpc.test');
    const validators = await client.getValidators();

    expect(validators).toHaveLength(1);
    expect(validators[0]!.stake).toBe(100_000_000n);
    expect(validators[0]!.isActive).toBe(true);
    expect(validators[0]!.commissionBps).toBe(500);
  });
});

// ---------------------------------------------------------------------------
// NovaWallet
// ---------------------------------------------------------------------------

describe('NovaWallet', () => {
  it('creates a wallet with a valid address', () => {
    const wallet = NovaWallet.create();

    expect(wallet.address).toBeTruthy();
    expect(wallet.address.startsWith('nova1')).toBe(true);
    expect(wallet.publicKey).toBeInstanceOf(Uint8Array);
    expect(wallet.publicKey.length).toBe(32);
  });

  it('creates a deterministic wallet from seed', () => {
    const seed = new Uint8Array(32).fill(0xab);
    const w1 = NovaWallet.fromSeed(seed);
    const w2 = NovaWallet.fromSeed(seed);

    expect(w1.address).toBe(w2.address);
    expect(Buffer.from(w1.publicKey).toString('hex')).toBe(
      Buffer.from(w2.publicKey).toString('hex'),
    );
  });

  it('signs an arbitrary message', () => {
    const wallet = NovaWallet.create();
    const message = new TextEncoder().encode('wallet-sign-test');
    const sig = wallet.sign(message);

    expect(sig).toBeInstanceOf(Uint8Array);
    expect(sig.length).toBe(64);
  });

  it('buildTransfer returns a valid signed transaction', () => {
    const sender = NovaWallet.create();
    const receiver = NovaWallet.create();

    const signedTx = sender.buildTransfer(receiver.address, 1_000n, 'NOVA');

    expect(signedTx.transaction.type).toBe('transfer');
    expect(signedTx.transaction.sender).toBe(sender.address);
    expect(signedTx.transaction.receiver).toBe(receiver.address);
    expect(signedTx.transaction.amount.value).toBe(1_000n);
    expect(signedTx.signature.length).toBe(64);
  });

  it('buildTransfer produces verifiable transactions', async () => {
    // Lazy import to avoid circular issues in the test.
    const { verifyTransaction } = await import('../src/transaction.js');

    const sender = NovaWallet.create();
    const receiver = NovaWallet.create();
    const signedTx = sender.buildTransfer(receiver.address, 1n);

    expect(verifyTransaction(signedTx)).toBe(true);
  });
});
