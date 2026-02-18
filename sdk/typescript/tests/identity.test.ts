import { describe, it, expect } from 'vitest';
import {
  generateKeypair,
  keypairFromSeed,
  createNovaId,
  parseNovaId,
  signMessage,
  verifySignature,
} from '../src/identity.js';
import type { PublicKey, SecretKey } from '../src/types.js';

describe('generateKeypair', () => {
  it('produces a 32-byte public key and a 32-byte secret key', () => {
    const kp = generateKeypair();

    expect(kp.publicKey).toBeInstanceOf(Uint8Array);
    expect(kp.secretKey).toBeInstanceOf(Uint8Array);
    expect(kp.publicKey.length).toBe(32);
    expect(kp.secretKey.length).toBe(32);
  });

  it('generates distinct keypairs on successive calls', () => {
    const a = generateKeypair();
    const b = generateKeypair();

    // The probability of a collision is negligible.
    expect(Buffer.from(a.publicKey).toString('hex')).not.toBe(
      Buffer.from(b.publicKey).toString('hex'),
    );
  });
});

describe('keypairFromSeed', () => {
  it('derives a deterministic keypair from a 32-byte seed', () => {
    const seed = new Uint8Array(32);
    seed[0] = 0xde;
    seed[1] = 0xad;
    seed[31] = 0xff;

    const kp1 = keypairFromSeed(seed);
    const kp2 = keypairFromSeed(seed);

    expect(Buffer.from(kp1.publicKey).toString('hex')).toBe(
      Buffer.from(kp2.publicKey).toString('hex'),
    );
    expect(Buffer.from(kp1.secretKey).toString('hex')).toBe(
      Buffer.from(kp2.secretKey).toString('hex'),
    );
  });

  it('throws on an invalid seed length', () => {
    expect(() => keypairFromSeed(new Uint8Array(16))).toThrow('expected 32-byte seed');
    expect(() => keypairFromSeed(new Uint8Array(64))).toThrow('expected 32-byte seed');
  });
});

describe('createNovaId / parseNovaId roundtrip', () => {
  it('encodes a public key to a bech32 address and decodes it back', () => {
    const kp = generateKeypair();
    const novaId = createNovaId(kp.publicKey);

    // Should start with the "nova" prefix.
    expect(novaId.startsWith('nova1')).toBe(true);

    const parsed = parseNovaId(novaId);
    expect(parsed.hrp).toBe('nova');
    expect(Buffer.from(parsed.publicKey).toString('hex')).toBe(
      Buffer.from(kp.publicKey).toString('hex'),
    );
  });

  it('rejects an address with a wrong prefix', () => {
    // Manually encode with a different HRP.
    expect(() => parseNovaId('bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4')).toThrow(
      'unexpected prefix',
    );
  });

  it('produces a deterministic address for a deterministic keypair', () => {
    const seed = new Uint8Array(32).fill(0x42);
    const kp = keypairFromSeed(seed);
    const addr1 = createNovaId(kp.publicKey);
    const addr2 = createNovaId(kp.publicKey);
    expect(addr1).toBe(addr2);
  });
});

describe('signMessage / verifySignature', () => {
  it('signs a message and verifies successfully', () => {
    const kp = generateKeypair();
    const message = new TextEncoder().encode('hello nova protocol');

    const sig = signMessage(kp.secretKey, message);

    expect(sig).toBeInstanceOf(Uint8Array);
    expect(sig.length).toBe(64);

    const valid = verifySignature(kp.publicKey, message, sig);
    expect(valid).toBe(true);
  });

  it('fails verification with a different message', () => {
    const kp = generateKeypair();
    const message = new TextEncoder().encode('original');
    const sig = signMessage(kp.secretKey, message);

    const tampered = new TextEncoder().encode('tampered');
    expect(verifySignature(kp.publicKey, tampered, sig)).toBe(false);
  });

  it('fails verification with a different public key', () => {
    const kp1 = generateKeypair();
    const kp2 = generateKeypair();
    const message = new TextEncoder().encode('test');

    const sig = signMessage(kp1.secretKey, message);
    expect(verifySignature(kp2.publicKey, message, sig)).toBe(false);
  });

  it('fails verification with a corrupted signature', () => {
    const kp = generateKeypair();
    const message = new TextEncoder().encode('test');
    const sig = signMessage(kp.secretKey, message);

    // Flip a byte.
    const corrupted = new Uint8Array(sig);
    corrupted[0] ^= 0xff;

    expect(verifySignature(kp.publicKey, message, corrupted as unknown as typeof sig)).toBe(false);
  });

  it('produces deterministic signatures for the same key and message', () => {
    const seed = new Uint8Array(32).fill(0x01);
    const kp = keypairFromSeed(seed);
    const message = new TextEncoder().encode('deterministic');

    const sig1 = signMessage(kp.secretKey, message);
    const sig2 = signMessage(kp.secretKey, message);

    expect(Buffer.from(sig1).toString('hex')).toBe(Buffer.from(sig2).toString('hex'));
  });
});
