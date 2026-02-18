import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import {
  generateKeypair,
  keypairFromSecretKey,
  deriveAddress,
  bytesToHex,
  buildAndSignTransaction,
  hexToBytes,
} from '../lib/crypto';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type Network = 'mainnet' | 'testnet' | 'devnet';

export interface TokenBalance {
  symbol: string;
  name: string;
  balance: number;
  usdValue: number;
  change24h: number;
}

export interface Transaction {
  id: string;
  hash: string;
  type: 'send' | 'receive' | 'credit_issued' | 'credit_repay';
  amount: number;
  symbol: string;
  from: string;
  to: string;
  fee: number;
  status: 'pending' | 'confirmed' | 'failed';
  timestamp: number;
  blockHeight?: number;
  payload?: string;
}

export interface CreditLine {
  id: string;
  provider: string;
  limit: number;
  used: number;
  rate: number;
  term: string;
  status: 'active' | 'pending' | 'expired';
}

// ---------------------------------------------------------------------------
// Store interface
// ---------------------------------------------------------------------------

interface WalletState {
  // Wallet identity
  address: string;
  publicKey: string;
  secretKey: string;
  isWalletInitialized: boolean;
  createdAt: string;

  // Network config
  network: Network;
  nodeUrl: string;
  theme: 'dark' | 'light';

  // Node status
  networkConnected: boolean;
  blockHeight: number;

  // Balances & transactions
  balances: TokenBalance[];
  transactions: Transaction[];
  creditScore: number;
  creditLines: CreditLine[];

  // Actions - wallet lifecycle
  createWallet: () => { secretKeyHex: string; address: string };
  importWallet: (secretKeyHex: string) => void;
  lockWallet: () => void;

  // Actions - data
  setWallet: (address: string, publicKey: string) => void;
  updateBalances: (balances: TokenBalance[]) => void;
  addTransaction: (tx: Transaction) => void;
  updateTransactionStatus: (id: string, status: Transaction['status'], blockHeight?: number) => void;
  setNetworkStatus: (connected: boolean, blockHeight?: number) => void;
  setNetwork: (network: Network) => void;
  setNodeUrl: (url: string) => void;
  setTheme: (theme: 'dark' | 'light') => void;
}

// ---------------------------------------------------------------------------
// Network URL mapping
// ---------------------------------------------------------------------------

const NETWORK_URLS: Record<Network, string> = {
  mainnet: 'https://rpc.nova-protocol.io',
  testnet: 'https://testnet-rpc.nova-protocol.io',
  devnet: 'https://devnet-rpc.nova-protocol.io',
};

function getDefaultNodeUrl(): string {
  try {
    return import.meta.env.VITE_NODE_URL || 'http://localhost:9741';
  } catch {
    return 'http://localhost:9741';
  }
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

export const useWalletStore = create<WalletState>()(
  persist(
    (set) => ({
      // Initial state - no wallet
      address: '',
      publicKey: '',
      secretKey: '',
      isWalletInitialized: false,
      createdAt: '',
      network: 'mainnet',
      nodeUrl: getDefaultNodeUrl(),
      theme: 'dark',
      networkConnected: false,
      blockHeight: 0,
      balances: [],
      transactions: [],
      creditScore: 0,
      creditLines: [],

      // -------------------------------------------------------------------
      // Wallet lifecycle
      // -------------------------------------------------------------------

      createWallet: () => {
        const keypair = generateKeypair();
        const address = deriveAddress(keypair.publicKey);
        const publicKeyHex = bytesToHex(keypair.publicKey);
        const secretKeyHex = bytesToHex(keypair.secretKey);

        set({
          address,
          publicKey: publicKeyHex,
          secretKey: secretKeyHex,
          isWalletInitialized: true,
          createdAt: new Date().toISOString(),
          balances: [],
          transactions: [],
          creditScore: 0,
          creditLines: [],
        });

        return { secretKeyHex, address };
      },

      importWallet: (secretKeyHex: string) => {
        const keypair = keypairFromSecretKey(secretKeyHex);
        const address = deriveAddress(keypair.publicKey);
        const publicKeyHex = bytesToHex(keypair.publicKey);

        set({
          address,
          publicKey: publicKeyHex,
          secretKey: secretKeyHex,
          isWalletInitialized: true,
          createdAt: new Date().toISOString(),
          balances: [],
          transactions: [],
          creditScore: 0,
          creditLines: [],
        });
      },

      lockWallet: () => {
        set({
          address: '',
          publicKey: '',
          secretKey: '',
          isWalletInitialized: false,
          createdAt: '',
          balances: [],
          transactions: [],
          creditScore: 0,
          creditLines: [],
          networkConnected: false,
          blockHeight: 0,
        });
      },

      // -------------------------------------------------------------------
      // Data actions
      // -------------------------------------------------------------------

      setWallet: (address, publicKey) => set({ address, publicKey }),

      updateBalances: (balances) => set({ balances }),

      addTransaction: (tx) =>
        set((state) => ({ transactions: [tx, ...state.transactions] })),

      updateTransactionStatus: (id, status, blockHeight) =>
        set((state) => ({
          transactions: state.transactions.map((tx) =>
            tx.id === id ? { ...tx, status, blockHeight: blockHeight ?? tx.blockHeight } : tx,
          ),
        })),

      setNetworkStatus: (connected, blockHeight) =>
        set({
          networkConnected: connected,
          ...(blockHeight !== undefined ? { blockHeight } : {}),
        }),

      setNetwork: (network) => {
        set({ network, nodeUrl: NETWORK_URLS[network] });
      },

      setNodeUrl: (nodeUrl) => set({ nodeUrl }),

      setTheme: (theme) => set({ theme }),
    }),
    {
      name: 'nova-wallet-storage',
      partialize: (state) => ({
        address: state.address,
        publicKey: state.publicKey,
        secretKey: state.secretKey,
        isWalletInitialized: state.isWalletInitialized,
        createdAt: state.createdAt,
        network: state.network,
        nodeUrl: state.nodeUrl,
        theme: state.theme,
        transactions: state.transactions,
      }),
    },
  ),
);

// ---------------------------------------------------------------------------
// Transaction signing helper (exported for use in hooks)
// ---------------------------------------------------------------------------

export function signAndBuildTx(params: {
  recipient: string;
  amount: number;
  currency: string;
  payload?: string;
}): { txId: string; signedTx: Record<string, unknown> } {
  const state = useWalletStore.getState();

  if (!state.secretKey || !state.publicKey) {
    throw new Error('Wallet not initialized');
  }

  const encoder = new TextEncoder();
  const payloadBytes = params.payload
    ? encoder.encode(params.payload)
    : new Uint8Array(0);

  // Convert human-readable amount to atomic units (1 NOVA = 1e8 units).
  const atomicAmount = BigInt(Math.round(params.amount * 1e8));
  const atomicFee = params.currency === 'NOVA' ? 100_000n : 50_000n; // 0.001 / 0.0005

  return buildAndSignTransaction({
    sender: state.address,
    receiver: params.recipient,
    amount: atomicAmount,
    currency: params.currency,
    fee: atomicFee,
    payload: payloadBytes,
    secretKey: hexToBytes(state.secretKey),
    publicKey: hexToBytes(state.publicKey),
  });
}
