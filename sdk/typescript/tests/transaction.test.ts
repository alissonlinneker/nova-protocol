import { describe, it, expect } from 'vitest';
import {
  TransactionBuilder,
  signTransaction,
  verifyTransaction,
  computeTransactionId,
  signableBytes,
} from '../src/transaction.js';
import {
  generateKeypair,
  createNovaId,
  keypairFromSeed,
} from '../src/identity.js';
import type { NovaId } from '../src/types.js';
import { bytesToHex } from '../src/utils.js';

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
    expect(tx.version).toBe(1);
    expect(tx.sender).toBe(sender.address);
    expect(tx.receiver).toBe(receiver.address);
    expect(tx.amount.value).toBe(1_000_000n);
    expect(tx.amount.currency).toBe('NOVA');
    expect(tx.fee).toBe(100n);
    expect(tx.nonce).toBe(1);
    expect(typeof tx.id).toBe('string');
    expect(tx.id.length).toBe(64); // double-SHA-256 hex = 64 chars
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

  it('defaults version to 1', () => {
    const sender = makePair();
    const receiver = makePair();

    const tx = new TransactionBuilder()
      .sender(sender.address)
      .receiver(receiver.address)
      .amount(500n)
      .build();

    expect(tx.version).toBe(1);
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

  it('allows overriding the version', () => {
    const sender = makePair();
    const receiver = makePair();

    const tx = new TransactionBuilder()
      .version(2)
      .sender(sender.address)
      .receiver(receiver.address)
      .amount(0n)
      .build();

    expect(tx.version).toBe(2);
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
      version: 1,
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
      version: 1,
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
// signableBytes — canonical binary format
// ---------------------------------------------------------------------------

describe('signableBytes', () => {
  it('matches the documented Rust layout for a no-payload transaction', () => {
    const body = {
      version: 1,
      type: 'transfer' as const,
      sender: 'nova1aaaa' as NovaId,
      receiver: 'nova1bbbb' as NovaId,
      amount: { value: 1_000_000n, currency: 'NOVA' },
      fee: 100n,
      nonce: 1,
      payload: new Uint8Array(0),
      timestamp: 1_700_000_000_000,
    };

    const bytes = signableBytes(body);

    // Verify the structure manually.
    let offset = 0;

    // version: LE u16 = 1 => [0x01, 0x00]
    expect(bytes[offset]).toBe(0x01);
    expect(bytes[offset + 1]).toBe(0x00);
    offset += 2;

    // tx_type: "Transfer" + 0x00
    const encoder = new TextEncoder();
    const typeStr = encoder.encode('Transfer');
    for (let i = 0; i < typeStr.length; i++) {
      expect(bytes[offset + i]).toBe(typeStr[i]);
    }
    offset += typeStr.length;
    expect(bytes[offset]).toBe(0x00);
    offset += 1;

    // sender: "nova1aaaa" + 0x00
    const senderStr = encoder.encode('nova1aaaa');
    for (let i = 0; i < senderStr.length; i++) {
      expect(bytes[offset + i]).toBe(senderStr[i]);
    }
    offset += senderStr.length;
    expect(bytes[offset]).toBe(0x00);
    offset += 1;

    // receiver: "nova1bbbb" + 0x00
    const receiverStr = encoder.encode('nova1bbbb');
    for (let i = 0; i < receiverStr.length; i++) {
      expect(bytes[offset + i]).toBe(receiverStr[i]);
    }
    offset += receiverStr.length;
    expect(bytes[offset]).toBe(0x00);
    offset += 1;

    // amount.value: LE u64 = 1_000_000
    const amountView = new DataView(bytes.buffer, offset, 8);
    expect(amountView.getBigUint64(0, true)).toBe(1_000_000n);
    offset += 8;

    // amount.currency: "NOVA" + 0x00
    const currStr = encoder.encode('NOVA');
    for (let i = 0; i < currStr.length; i++) {
      expect(bytes[offset + i]).toBe(currStr[i]);
    }
    offset += currStr.length;
    expect(bytes[offset]).toBe(0x00);
    offset += 1;

    // fee: LE u64 = 100
    const feeView = new DataView(bytes.buffer, offset, 8);
    expect(feeView.getBigUint64(0, true)).toBe(100n);
    offset += 8;

    // nonce: LE u64 = 1
    const nonceView = new DataView(bytes.buffer, offset, 8);
    expect(nonceView.getBigUint64(0, true)).toBe(1n);
    offset += 8;

    // timestamp: LE u64 = 1_700_000_000_000
    const tsView = new DataView(bytes.buffer, offset, 8);
    expect(tsView.getBigUint64(0, true)).toBe(1_700_000_000_000n);
    offset += 8;

    // no-payload flag: 0x00
    expect(bytes[offset]).toBe(0x00);
    offset += 1;

    // Should have consumed the entire buffer.
    expect(offset).toBe(bytes.length);
  });

  it('encodes payload with length prefix when present', () => {
    const payload = new TextEncoder().encode('hello');

    const body = {
      version: 1,
      type: 'transfer' as const,
      sender: 'nova1aaaa' as NovaId,
      receiver: 'nova1bbbb' as NovaId,
      amount: { value: 0n, currency: 'NOVA' },
      fee: 0n,
      nonce: 1,
      payload,
      timestamp: 1_700_000_000_000,
    };

    const bytes = signableBytes(body);

    // Find the payload section at the end: 0x01 + LE u32 len + payload bytes.
    const payloadStart = bytes.length - 1 - 4 - payload.length;
    expect(bytes[payloadStart]).toBe(0x01); // present flag
    const lenView = new DataView(bytes.buffer, payloadStart + 1, 4);
    expect(lenView.getUint32(0, true)).toBe(payload.length);

    const extractedPayload = bytes.slice(payloadStart + 5, payloadStart + 5 + payload.length);
    expect(extractedPayload).toEqual(payload);
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

// ---------------------------------------------------------------------------
// Cross-language test vector
// ---------------------------------------------------------------------------

describe('cross-language test vector', () => {
  it('produces the same signable bytes and transaction ID as Rust', () => {
    // Use the same hardcoded address strings as the Rust test to ensure
    // the binary serialization is identical byte-for-byte.
    const senderAddr = 'nova1sender_test_vector' as NovaId;
    const receiverAddr = 'nova1receiver_test_vector' as NovaId;

    const tx = new TransactionBuilder()
      .version(1)
      .type('transfer')
      .sender(senderAddr)
      .receiver(receiverAddr)
      .amount(1_000_000n, 'NOVA')
      .fee(100n)
      .nonce(42)
      .timestamp(1_700_000_000_000)
      .build();

    const { id: _id, ...body } = tx;
    const canonical = signableBytes(body);
    const canonicalHex = bytesToHex(canonical);

    // These must match the values pinned in the Rust cross_language_test_vector test.
    expect(canonicalHex).toBe(
      '01005472616e73666572006e6f76613173656e6465725f746573745f766563746f72006e6f76613172656365697665725f746573745f766563746f720040420f00000000004e4f56410064000000000000002a000000000000000068e5cf8b01000000'
    );

    expect(tx.id).toBe(
      'a8c099ee823f352281802881bf6b55008b4a0f8813808426fe83017e20a5d147'
    );

    console.log('--- Cross-language test vector (TypeScript) ---');
    console.log('signable_bytes_hex:', canonicalHex);
    console.log('tx_id:', tx.id);
  });

  it('signing round-trips with a deterministic keypair', () => {
    const seed = new Uint8Array(32);
    seed[0] = 0x01;
    const kp = keypairFromSeed(seed);
    const senderAddr = createNovaId(kp.publicKey);

    const receiverSeed = new Uint8Array(32);
    receiverSeed[0] = 0x02;
    const receiverKp = keypairFromSeed(receiverSeed);
    const receiverAddr = createNovaId(receiverKp.publicKey);

    const tx = new TransactionBuilder()
      .version(1)
      .type('transfer')
      .sender(senderAddr)
      .receiver(receiverAddr)
      .amount(1_000_000n, 'NOVA')
      .fee(100n)
      .nonce(42)
      .timestamp(1_700_000_000_000)
      .build();

    const signed = signTransaction(tx, kp.secretKey, kp.publicKey);
    expect(verifyTransaction(signed)).toBe(true);
  });
});
