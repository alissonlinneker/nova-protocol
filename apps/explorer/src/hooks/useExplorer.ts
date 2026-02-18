export interface Block {
  height: number;
  hash: string;
  timestamp: number;
  txCount: number;
  validator: string;
  size: number;
  gasUsed: number;
}

export interface ExplorerTransaction {
  hash: string;
  blockHeight: number;
  type: "transfer" | "credit_issue" | "stake" | "unstake" | "governance";
  from: string;
  to: string;
  amount: number;
  symbol: string;
  fee: number;
  status: "confirmed" | "pending" | "failed";
  timestamp: number;
  memo?: string;
  gasUsed: number;
}

export interface AddressInfo {
  address: string;
  balance: { symbol: string; amount: number }[];
  txCount: number;
  firstSeen: number;
  lastActive: number;
  isValidator: boolean;
  isContract: boolean;
  creditScore?: number;
}

export interface NetworkStatsData {
  blockHeight: number;
  tps: number;
  avgBlockTime: number;
  totalTransactions: number;
  activeValidators: number;
  totalStaked: number;
  totalSupply: number;
  circulatingSupply: number;
  marketCap: number;
  price: number;
}

function randomHex(len: number): string {
  const chars = "0123456789abcdef";
  let r = "";
  for (let i = 0; i < len; i++) r += chars[Math.floor(Math.random() * 16)];
  return r;
}

const VALIDATORS = [
  "nova1val_alpha_prime_node_01",
  "nova1val_beta_cluster_02",
  "nova1val_gamma_sentinel_03",
  "nova1val_delta_tower_04",
  "nova1val_epsilon_core_05",
];

export function getMockBlocks(count = 15): Block[] {
  const baseHeight = 1_847_293;
  return Array.from({ length: count }, (_, i) => ({
    height: baseHeight - i,
    hash: `0x${randomHex(64)}`,
    timestamp: Date.now() - i * 6_000,
    txCount: Math.floor(Math.random() * 200) + 20,
    validator: VALIDATORS[i % VALIDATORS.length],
    size: Math.floor(Math.random() * 500_000) + 100_000,
    gasUsed: Math.floor(Math.random() * 8_000_000) + 2_000_000,
  }));
}

export function getMockTransaction(hash?: string): ExplorerTransaction {
  const types: ExplorerTransaction["type"][] = [
    "transfer",
    "credit_issue",
    "stake",
    "unstake",
    "governance",
  ];
  return {
    hash: hash || `0x${randomHex(64)}`,
    blockHeight: 1_847_293 - Math.floor(Math.random() * 100),
    type: types[Math.floor(Math.random() * types.length)],
    from: `nova1${randomHex(38)}`,
    to: `nova1${randomHex(38)}`,
    amount: Math.floor(Math.random() * 10_000) + 1,
    symbol: Math.random() > 0.5 ? "NOVA" : "USDN",
    fee: Math.random() * 0.01,
    status: "confirmed",
    timestamp: Date.now() - Math.floor(Math.random() * 86_400_000),
    memo: Math.random() > 0.7 ? "Payment for services" : undefined,
    gasUsed: Math.floor(Math.random() * 100_000) + 21_000,
  };
}

export function getMockTransactionsForBlock(count = 10): ExplorerTransaction[] {
  return Array.from({ length: count }, () => getMockTransaction());
}

export function getMockAddressInfo(addr?: string): AddressInfo {
  const address = addr || `nova1${randomHex(38)}`;
  return {
    address,
    balance: [
      { symbol: "NOVA", amount: Math.floor(Math.random() * 50_000) + 100 },
      { symbol: "USDN", amount: Math.floor(Math.random() * 20_000) },
      { symbol: "stNOVA", amount: Math.floor(Math.random() * 10_000) },
    ],
    txCount: Math.floor(Math.random() * 1_000) + 5,
    firstSeen: Date.now() - 86_400_000 * Math.floor(Math.random() * 365 + 30),
    lastActive: Date.now() - Math.floor(Math.random() * 86_400_000),
    isValidator: Math.random() > 0.9,
    isContract: Math.random() > 0.85,
    creditScore: Math.floor(Math.random() * 200) + 650,
  };
}

export function getMockNetworkStats(): NetworkStatsData {
  return {
    blockHeight: 1_847_293,
    tps: 1_247,
    avgBlockTime: 5.8,
    totalTransactions: 142_847_293,
    activeValidators: 128,
    totalStaked: 482_000_000,
    totalSupply: 1_000_000_000,
    circulatingSupply: 680_000_000,
    marketCap: 2_040_000_000,
    price: 3.0,
  };
}
