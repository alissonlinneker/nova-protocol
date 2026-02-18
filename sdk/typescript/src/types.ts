/**
 * NOVA Protocol — Core Type Definitions
 *
 * All shared types used throughout the SDK. Branded types enforce
 * domain correctness at the type level without runtime overhead.
 */

// ---------------------------------------------------------------------------
// Branded / Nominal helpers
// ---------------------------------------------------------------------------

declare const __brand: unique symbol;

/** A bech32-encoded NOVA address (human-readable prefix "nova"). */
export type NovaId = string & { readonly [__brand]: 'NovaId' };

// ---------------------------------------------------------------------------
// Cryptographic primitives
// ---------------------------------------------------------------------------

/** Ed25519 public key — 32 bytes. */
export type PublicKey = Uint8Array & { readonly [__brand]: 'PublicKey' };

/** Ed25519 secret key — 32 bytes (seed form). */
export type SecretKey = Uint8Array & { readonly [__brand]: 'SecretKey' };

/** Ed25519 signature — 64 bytes. */
export type Signature = Uint8Array & { readonly [__brand]: 'Signature' };

// ---------------------------------------------------------------------------
// Transaction types
// ---------------------------------------------------------------------------

/** Supported on-chain transaction types. */
export type TransactionType =
  | 'transfer'
  | 'credit_request'
  | 'credit_settlement'
  | 'token_mint'
  | 'token_burn';

/** Lifecycle status of a transaction. */
export type TransactionStatus = 'pending' | 'confirmed' | 'failed' | 'expired';

/** Precise monetary amount with an associated currency identifier. */
export interface Amount {
  /** Atomic units (e.g. 1 NOVA = 1_000_000_000 units). */
  value: bigint;
  /** Currency / token identifier. "NOVA" for the native token. */
  currency: string;
}

/** An unsigned transaction ready for signing. */
export interface Transaction {
  /** Unique hash computed from the transaction body. */
  id: string;
  /** Protocol version (default: 1). Allows validators to apply the correct rule set. */
  version: number;
  type: TransactionType;
  sender: NovaId;
  receiver: NovaId;
  amount: Amount;
  /** Fee in atomic NOVA units. */
  fee: bigint;
  /** Sender-scoped monotonic counter (replay protection). */
  nonce: number;
  /** Arbitrary payload — contract call data, memo, etc. */
  payload: Uint8Array;
  /** Unix-millisecond timestamp of creation. */
  timestamp: number;
}

/** A transaction bundled with its Ed25519 signature. */
export interface SignedTransaction {
  transaction: Transaction;
  signature: Signature;
  /** The public key of the signer (for quick verification). */
  signerPublicKey: PublicKey;
}

/** Server-issued receipt after a transaction is finalized. */
export interface TransactionReceipt {
  transactionId: string;
  blockHeight: number;
  blockHash: string;
  status: TransactionStatus;
  /** Gas / compute units consumed (if applicable). */
  gasUsed: bigint;
  /** Unix-millisecond timestamp of confirmation. */
  timestamp: number;
}

// ---------------------------------------------------------------------------
// Block types
// ---------------------------------------------------------------------------

export interface BlockHeader {
  height: number;
  hash: string;
  previousHash: string;
  /** Merkle root of all transactions in the block. */
  transactionsRoot: string;
  /** Merkle root of the world state after applying this block. */
  stateRoot: string;
  /** Unix-millisecond timestamp. */
  timestamp: number;
  /** NOVA ID of the block proposer. */
  proposer: NovaId;
}

export interface Block {
  header: BlockHeader;
  transactions: Transaction[];
}

// ---------------------------------------------------------------------------
// Account & wallet types
// ---------------------------------------------------------------------------

export interface AccountState {
  /** Next expected nonce for this account. */
  nonce: number;
  /** Currency identifier -> balance in atomic units. */
  balances: Map<string, bigint>;
}

export interface WalletState {
  address: NovaId;
  publicKey: PublicKey;
  nonce: number;
  balances: Map<string, bigint>;
}

// ---------------------------------------------------------------------------
// Credit marketplace types
// ---------------------------------------------------------------------------

export interface CreditOffer {
  id: string;
  lender: NovaId;
  /** Maximum amount the lender is willing to extend. */
  maxAmount: Amount;
  /** Annual interest rate in basis points (1 bp = 0.01 %). */
  interestRateBps: number;
  /** Loan term in seconds. */
  termSeconds: number;
  /** Minimum credit score required. */
  minCreditScore: number;
  /** Unix-millisecond expiry of the offer. */
  expiresAt: number;
}

export interface CreditScore {
  address: NovaId;
  /** Numeric score (higher is better, range 0 – 1000). */
  score: number;
  /** Number of on-time repayments. */
  totalRepayments: number;
  /** Number of defaults. */
  totalDefaults: number;
  /** Unix-millisecond timestamp of last update. */
  lastUpdated: number;
}

export interface CreditRequestParams {
  borrower: NovaId;
  amount: Amount;
  /** Maximum acceptable annual interest rate (basis points). */
  maxInterestRateBps: number;
  /** Desired loan term in seconds. */
  termSeconds: number;
}

// ---------------------------------------------------------------------------
// Network / RPC types
// ---------------------------------------------------------------------------

export interface NodeInfo {
  nodeId: string;
  version: string;
  networkId: string;
  blockHeight: number;
  peerCount: number;
  /** Unix-millisecond uptime start. */
  uptimeSince: number;
}

export interface ValidatorInfo {
  address: NovaId;
  /** Staked NOVA in atomic units. */
  stake: bigint;
  /** Whether the validator is currently in the active set. */
  isActive: boolean;
  /** Commission rate in basis points. */
  commissionBps: number;
  /** Blocks proposed in the current epoch. */
  blocksProposed: number;
}

// ---------------------------------------------------------------------------
// JSON-RPC transport
// ---------------------------------------------------------------------------

export interface RpcRequest {
  jsonrpc: '2.0';
  id: number | string;
  method: string;
  params?: unknown[];
}

export interface RpcResponse<T = unknown> {
  jsonrpc: '2.0';
  id: number | string;
  result?: T;
  error?: RpcError;
}

export interface RpcError {
  code: number;
  message: string;
  data?: unknown;
}
