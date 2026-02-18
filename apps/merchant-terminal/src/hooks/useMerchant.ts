import { create } from "zustand";

export interface MerchantTransaction {
  id: string;
  hash: string;
  amount: number;
  symbol: string;
  from: string;
  status: "pending" | "confirmed";
  timestamp: number;
  memo?: string;
}

interface DailyStats {
  date: string;
  revenue: number;
  txCount: number;
}

interface MerchantState {
  merchantId: string;
  merchantName: string;
  address: string;
  todayRevenue: number;
  todayTxCount: number;
  weekRevenue: number;
  weekTxCount: number;
  transactions: MerchantTransaction[];
  dailyStats: DailyStats[];
  pendingAmount: string;
  pendingPayment: boolean;

  setPendingAmount: (amount: string) => void;
  setPendingPayment: (pending: boolean) => void;
  addTransaction: (tx: MerchantTransaction) => void;
  simulatePayment: () => Promise<MerchantTransaction>;
}

const MOCK_TRANSACTIONS: MerchantTransaction[] = [
  {
    id: "mtx-001",
    hash: "0xf1a2b3c4d5e6f7890a1b2c3d4e5f6789",
    amount: 42.5,
    symbol: "USDN",
    from: "nova1q9g7f3k2x8p4m5n6j7h8d9s0a2w3e4r5t6y7u",
    status: "confirmed",
    timestamp: Date.now() - 180_000,
    memo: "Order #1847",
  },
  {
    id: "mtx-002",
    hash: "0xa2b3c4d5e6f7890a1b2c3d4e5f67890a",
    amount: 156.0,
    symbol: "NOVA",
    from: "nova1m8k3j2h1g6f5d4s3a2p1o0i9u8y7t6r5e4w3q",
    status: "confirmed",
    timestamp: Date.now() - 720_000,
  },
  {
    id: "mtx-003",
    hash: "0xb3c4d5e6f7890a1b2c3d4e5f67890a1b",
    amount: 23.99,
    symbol: "USDN",
    from: "nova1z9x8c7v6b5n4m3a2s1d0f9g8h7j6k5l4p3o2i",
    status: "confirmed",
    timestamp: Date.now() - 1_800_000,
    memo: "Order #1846",
  },
  {
    id: "mtx-004",
    hash: "0xc4d5e6f7890a1b2c3d4e5f67890a1b2c",
    amount: 89.0,
    symbol: "NOVA",
    from: "nova1p2o3i4u5y6t7r8e9w0q1a2s3d4f5g6h7j8k9l",
    status: "confirmed",
    timestamp: Date.now() - 3_600_000,
  },
  {
    id: "mtx-005",
    hash: "0xd5e6f7890a1b2c3d4e5f67890a1b2c3d",
    amount: 310.0,
    symbol: "USDN",
    from: "nova1w2e3r4t5y6u7i8o9p0a1s2d3f4g5h6j7k8l9z",
    status: "confirmed",
    timestamp: Date.now() - 7_200_000,
    memo: "Order #1845",
  },
];

const MOCK_DAILY_STATS: DailyStats[] = [
  { date: "Mon", revenue: 1_247.5, txCount: 18 },
  { date: "Tue", revenue: 982.3, txCount: 14 },
  { date: "Wed", revenue: 1_563.0, txCount: 22 },
  { date: "Thu", revenue: 2_105.8, txCount: 31 },
  { date: "Fri", revenue: 1_890.2, txCount: 27 },
  { date: "Sat", revenue: 2_341.0, txCount: 34 },
  { date: "Sun", revenue: 621.49, txCount: 9 },
];

export const useMerchantStore = create<MerchantState>((set, get) => ({
  merchantId: "MERCH-NV-0042",
  merchantName: "Nova Coffee Co.",
  address: "nova1merchant_q9g7f3k2x8p4m5n6j7h8d9s0a2w3",
  todayRevenue: 621.49,
  todayTxCount: 9,
  weekRevenue: 10_751.29,
  weekTxCount: 155,
  transactions: MOCK_TRANSACTIONS,
  dailyStats: MOCK_DAILY_STATS,
  pendingAmount: "",
  pendingPayment: false,

  setPendingAmount: (amount) => set({ pendingAmount: amount }),
  setPendingPayment: (pending) => set({ pendingPayment: pending }),

  addTransaction: (tx) =>
    set((state) => ({
      transactions: [tx, ...state.transactions],
      todayRevenue: state.todayRevenue + tx.amount,
      todayTxCount: state.todayTxCount + 1,
    })),

  simulatePayment: async () => {
    const state = get();
    const amount = parseFloat(state.pendingAmount);
    if (isNaN(amount) || amount <= 0) throw new Error("Invalid amount");

    set({ pendingPayment: true });

    // Simulate waiting for payment
    await new Promise((resolve) => setTimeout(resolve, 3_000));

    const tx: MerchantTransaction = {
      id: `mtx-${Date.now()}`,
      hash: `0x${Array.from({ length: 40 }, () => Math.floor(Math.random() * 16).toString(16)).join("")}`,
      amount,
      symbol: "USDN",
      from: `nova1${Array.from({ length: 38 }, () => Math.floor(Math.random() * 36).toString(36)).join("")}`,
      status: "confirmed",
      timestamp: Date.now(),
      memo: `Order #${1847 + state.transactions.length}`,
    };

    set((prev) => ({
      transactions: [tx, ...prev.transactions],
      todayRevenue: prev.todayRevenue + amount,
      todayTxCount: prev.todayTxCount + 1,
      pendingPayment: false,
      pendingAmount: "",
    }));

    return tx;
  },
}));
