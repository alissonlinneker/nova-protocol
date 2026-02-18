import { create } from "zustand";
import { persist } from "zustand/middleware";

export type Network = "mainnet" | "testnet" | "devnet";

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
  type: "send" | "receive" | "credit_issued" | "credit_repay";
  amount: number;
  symbol: string;
  from: string;
  to: string;
  fee: number;
  status: "pending" | "confirmed" | "failed";
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
  status: "active" | "pending" | "expired";
}

interface WalletState {
  address: string;
  publicKey: string;
  createdAt: string;
  network: Network;
  nodeUrl: string;
  theme: "dark" | "light";
  balances: TokenBalance[];
  transactions: Transaction[];
  creditScore: number;
  creditLines: CreditLine[];

  setWallet: (address: string, publicKey: string) => void;
  updateBalances: (balances: TokenBalance[]) => void;
  addTransaction: (tx: Transaction) => void;
  setNetwork: (network: Network) => void;
  setNodeUrl: (url: string) => void;
  setTheme: (theme: "dark" | "light") => void;
}

const MOCK_ADDRESS = "nova1q9g7f3k2x8p4m5n6j7h8d9s0a2w3e4r5t6y7u";
const MOCK_PUBLIC_KEY =
  "04a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f9";

const MOCK_BALANCES: TokenBalance[] = [
  {
    symbol: "NOVA",
    name: "Nova Token",
    balance: 12_847.35,
    usdValue: 38_542.05,
    change24h: 3.42,
  },
  {
    symbol: "USDN",
    name: "Nova USD",
    balance: 5_230.0,
    usdValue: 5_230.0,
    change24h: 0.01,
  },
  {
    symbol: "stNOVA",
    name: "Staked Nova",
    balance: 3_500.0,
    usdValue: 10_570.0,
    change24h: 3.51,
  },
  {
    symbol: "CRED",
    name: "Credit Token",
    balance: 890.5,
    usdValue: 1_425.8,
    change24h: -1.23,
  },
];

const MOCK_TRANSACTIONS: Transaction[] = [
  {
    id: "tx-001",
    hash: "0xab3f91c7d8e2a4b56c1d9e8f7a6b5c4d3e2f1a0b",
    type: "receive",
    amount: 1_250.0,
    symbol: "NOVA",
    from: "nova1m8k3j2h1g6f5d4s3a2p1o0i9u8y7t6r5e4w3q",
    to: MOCK_ADDRESS,
    fee: 0.001,
    status: "confirmed",
    timestamp: Date.now() - 3_600_000,
    blockHeight: 1_847_293,
  },
  {
    id: "tx-002",
    hash: "0xcd5e82b1a9f3c4d67e8f0a1b2c3d4e5f6a7b8c9d",
    type: "send",
    amount: 500.0,
    symbol: "USDN",
    from: MOCK_ADDRESS,
    to: "nova1z9x8c7v6b5n4m3a2s1d0f9g8h7j6k5l4p3o2i",
    fee: 0.0005,
    status: "confirmed",
    timestamp: Date.now() - 7_200_000,
    blockHeight: 1_847_201,
  },
  {
    id: "tx-003",
    hash: "0xef1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a",
    type: "send",
    amount: 75.0,
    symbol: "NOVA",
    from: MOCK_ADDRESS,
    to: "nova1p2o3i4u5y6t7r8e9w0q1a2s3d4f5g6h7j8k9l",
    fee: 0.001,
    status: "pending",
    timestamp: Date.now() - 300_000,
    payload: "Coffee subscription",
  },
  {
    id: "tx-004",
    hash: "0x1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b",
    type: "credit_issued",
    amount: 2_000.0,
    symbol: "USDN",
    from: "nova1credit_pool_alpha",
    to: MOCK_ADDRESS,
    fee: 0.002,
    status: "confirmed",
    timestamp: Date.now() - 86_400_000,
    blockHeight: 1_846_100,
  },
  {
    id: "tx-005",
    hash: "0x2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c",
    type: "receive",
    amount: 3_200.0,
    symbol: "NOVA",
    from: "nova1w2e3r4t5y6u7i8o9p0a1s2d3f4g5h6j7k8l9z",
    to: MOCK_ADDRESS,
    fee: 0.001,
    status: "confirmed",
    timestamp: Date.now() - 172_800_000,
    blockHeight: 1_845_022,
  },
];

const MOCK_CREDIT_LINES: CreditLine[] = [
  {
    id: "cl-001",
    provider: "Nova Prime Pool",
    limit: 10_000,
    used: 2_000,
    rate: 4.5,
    term: "90 days",
    status: "active",
  },
  {
    id: "cl-002",
    provider: "DeFi Credit DAO",
    limit: 5_000,
    used: 0,
    rate: 6.2,
    term: "30 days",
    status: "active",
  },
];

export const useWalletStore = create<WalletState>()(
  persist(
    (set) => ({
      address: MOCK_ADDRESS,
      publicKey: MOCK_PUBLIC_KEY,
      createdAt: "2025-09-15T10:30:00Z",
      network: "mainnet",
      nodeUrl: "https://rpc.nova-protocol.io",
      theme: "dark",
      balances: MOCK_BALANCES,
      transactions: MOCK_TRANSACTIONS,
      creditScore: 782,
      creditLines: MOCK_CREDIT_LINES,

      setWallet: (address, publicKey) => set({ address, publicKey }),
      updateBalances: (balances) => set({ balances }),
      addTransaction: (tx) =>
        set((state) => ({ transactions: [tx, ...state.transactions] })),
      setNetwork: (network) => {
        const urls: Record<Network, string> = {
          mainnet: "https://rpc.nova-protocol.io",
          testnet: "https://testnet-rpc.nova-protocol.io",
          devnet: "https://devnet-rpc.nova-protocol.io",
        };
        set({ network, nodeUrl: urls[network] });
      },
      setNodeUrl: (nodeUrl) => set({ nodeUrl }),
      setTheme: (theme) => set({ theme }),
    }),
    {
      name: "nova-wallet-storage",
      partialize: (state) => ({
        network: state.network,
        nodeUrl: state.nodeUrl,
        theme: state.theme,
      }),
    }
  )
);
