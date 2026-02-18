import { describe, it, expect } from 'vitest';
import {
  TransactionBuilder,
  signTransaction,
  verifyTransaction,
  computeTransactionId,
} from '../src/transaction.js';
import {
  generateKeypair,
  createNovaId,
} from '../src/identity.js';
import type { NovaId } from '../src/types.js';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makePair() {
  const kp = generateKeypair();
  const addr = createNovaId(kp.publicKey);
  return { ...kp, address: addr };
}

// ---------------------------------------------------------------------------
// TransactionBuilder
// ---------------------------------------------------------------------------

describe('TransactionBuilder', () => {
  it('builds a valid unsigned transfer transaction', () => {
    const sender = makePair();
    const receiver = makePair();

    const tx = new TransactionBuilder()
      .type('transfer')
      .sender(sender.address)
      .receiver(receiver.address)
      .amount(1_000_000n, 'NOVA')
      .fee(100n)
      .nonce(1)
      .build();

    expect(tx.type).toBe('transfer');
    expect(tx.sender).toBe(sender.address);
    expect(tx.receiver).toBe(receiver.address);
    expect(tx.amount.value).toBe(1_000_000n);
    expect(tx.amount.currency).toBe('NOVA');
    expect(tx.fee).toBe(100n);
    expect(tx.nonce).toBe(1);
    expect(typeof tx.id).toBe('string');
    expect(tx.id.length).toBe(64); // SHA-256 hex = 64 chars
    expect(tx.timestamp).toBeGreaterThan(0);
  });

  it('defaults currency to NOVA if omitted', () => {
    const sender = makePair();
    const receiver = makePair();

    const tx = new TransactionBuilder()
      .sender(sender.address)
      .receiver(receiver.address)
      .amount(500n)
      .build();

    expect(tx.amount.currency).toBe('NOVA');
  });

  it('throws when sender is missing', () => {
    const receiver = makePair();

    expect(() =>
      new TransactionBuilder()
        .receiver(receiver.address)
        .amount(1n)
        .build(),
    ).toThrow('sender is required');
  });

  it('throws when receiver is missing', () => {
    const sender = makePair();

    expect(() =>
      new TransactionBuilder()
        .sender(sender.address)
        .amount(1n)
        .build(),
    ).toThrow('receiver is required');
  });

  it('accepts different transaction types', () => {
    const sender = makePair();
    const receiver = makePair();

    for (const txType of [
      'transfer',
      'credit_request',
      'credit_settlement',
      'token_mint',
      'token_burn',
    ] as const) {
      const tx = new TransactionBuilder()
        .type(txType)
        .sender(sender.address)
        .receiver(receiver.address)
        .amount(0n)
        .build();

      expect(tx.type).toBe(txType);
    }
  });

  it('attaches a payload', () => {
    const sender = makePair();
    const receiver = makePair();
    const data = new TextEncoder().encode('{"memo":"hello"}');

    const tx = new TransactionBuilder()
      .sender(sender.address)
      .receiver(receiver.address)
      .amount(0n)
      .payload(data)
      .build();

    expect(tx.payload).toEqual(data);
  });
});

// ---------------------------------------------------------------------------
// computeTransactionId
// ---------------------------------------------------------------------------

describe('computeTransactionId', () => {
  it('produces a deterministic 64-char hex hash', () => {
    const sender = makePair();
    const receiver = makePair();

    const body = {
      type: 'transfer' as const,
      sender: sender.address,
      receiver: receiver.address,
      amount: { value: 42n, currency: 'NOVA' },
      fee: 1n,
      nonce: 7,
      payload: new Uint8Array(0),
      timestamp: 1_700_000_000_000,
    };

    const id1 = computeTransactionId(body);
    const id2 = computeTransactionId(body);

    expect(id1).toBe(id2);
    expect(id1.length).toBe(64);
    expect(/^[0-9a-f]{64}$/.test(id1)).toBe(true);
  });

  it('changes when any field is different', () => {
    const sender = makePair();
    const receiver = makePair();

    const base = {
      type: 'transfer' as const,
      sender: sender.address,
      receiver: receiver.address,
      amount: { value: 100n, currency: 'NOVA' },
      fee: 0n,
      nonce: 1,
      payload: new Uint8Array(0),
      timestamp: 1_700_000_000_000,
    };

    const idBase = computeTransactionId(base);
    const idDifferentAmount = computeTransactionId({ ...base, amount: { value: 101n, currency: 'NOVA' } });
    const idDifferentNonce = computeTransactionId({ ...base, nonce: 2 });

    expect(idBase).not.toBe(idDifferentAmount);
    expect(idBase).not.toBe(idDifferentNonce);
  });
});

// ---------------------------------------------------------------------------
// signTransaction / verifyTransaction
// ---------------------------------------------------------------------------

describe('signTransaction / verifyTransaction', () => {
  it('signs a transaction and verifies it', () => {
    const sender = makePair();
    const receiver = makePair();

    const tx = new TransactionBuilder()
      .type('transfer')
      .sender(sender.address)
      .receiver(receiver.address)
      .amount(500n, 'NOVA')
      .fee(10n)
      .nonce(1)
      .build();

    const signed = signTransaction(tx, sender.secretKey, sender.publicKey);

    expect(signed.signature).toBeInstanceOf(Uint8Array);
    expect(signed.signature.length).toBe(64);
    expect(signed.signerPublicKey).toBe(sender.publicKey);

    const valid = verifyTransaction(signed);
    expect(valid).toBe(true);
  });

  it('fails verification when the transaction body is tampered', () => {
    const sender = makePair();
    const receiver = makePair();

    const tx = new TransactionBuilder()
      .sender(sender.address)
      .receiver(receiver.address)
      .amount(100n)
      .nonce(1)
      .build();

    const signed = signTransaction(tx, sender.secretKey, sender.publicKey);

    // Tamper with the amount after signing.
    const tampered = {
      ...signed,
      transaction: {
        ...signed.transaction,
        amount: { value: 999n, currency: 'NOVA' },
      },
    };

    expect(verifyTransaction(tampered)).toBe(false);
  });

  it('fails verification with a wrong signer public key', () => {
    const sender = makePair();
    const receiver = makePair();
    const imposter = makePair();

    const tx = new TransactionBuilder()
      .sender(sender.address)
      .receiver(receiver.address)
      .amount(50n)
      .nonce(1)
      .build();

    const signed = signTransaction(tx, sender.secretKey, sender.publicKey);

    // Replace the signer key with someone else's.
    const bad = { ...signed, signerPublicKey: imposter.publicKey };
    expect(verifyTransaction(bad)).toBe(false);
  });
});
