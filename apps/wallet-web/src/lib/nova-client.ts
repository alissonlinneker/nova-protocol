import type { Network } from "../stores/walletStore";

interface NovaClientConfig {
  nodeUrl: string;
  network: Network;
}

interface TransferParams {
  to: string;
  amount: number;
  symbol: string;
  payload?: string;
}

interface TransferResult {
  hash: string;
  blockHeight?: number;
  fee: number;
  status: "pending" | "confirmed";
}

interface BalanceResponse {
  symbol: string;
  balance: number;
}

interface BlockInfo {
  height: number;
  hash: string;
  timestamp: number;
  txCount: number;
  validator: string;
}

class NovaClient {
  private config: NovaClientConfig;

  constructor(config: NovaClientConfig) {
    this.config = config;
  }

  setNetwork(network: Network, nodeUrl: string): void {
    this.config = { network, nodeUrl };
  }

  async getBalance(address: string): Promise<BalanceResponse[]> {
    // Simulate network latency
    await this.delay(300);
    void address; // Used in production RPC call

    return [
      { symbol: "NOVA", balance: 12_847.35 },
      { symbol: "USDN", balance: 5_230.0 },
      { symbol: "stNOVA", balance: 3_500.0 },
      { symbol: "CRED", balance: 890.5 },
    ];
  }

  async transfer(params: TransferParams): Promise<TransferResult> {
    await this.delay(1_500);
    void params; // Used in production RPC call

    return {
      hash: `0x${this.randomHex(40)}`,
      fee: params.symbol === "NOVA" ? 0.001 : 0.0005,
      status: "pending",
    };
  }

  async getLatestBlock(): Promise<BlockInfo> {
    await this.delay(200);

    return {
      height: 1_847_293,
      hash: `0x${this.randomHex(64)}`,
      timestamp: Date.now(),
      txCount: 142,
      validator: "nova1validator_node_alpha_01",
    };
  }

  async estimateFee(symbol: string): Promise<number> {
    await this.delay(100);
    return symbol === "NOVA" ? 0.001 : 0.0005;
  }

  private delay(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }

  private randomHex(length: number): string {
    const chars = "0123456789abcdef";
    let result = "";
    for (let i = 0; i < length; i++) {
      result += chars[Math.floor(Math.random() * chars.length)];
    }
    return result;
  }
}

let clientInstance: NovaClient | null = null;

export function getNovaClient(config?: NovaClientConfig): NovaClient {
  if (!clientInstance) {
    clientInstance = new NovaClient(
      config ?? {
        nodeUrl: "https://rpc.nova-protocol.io",
        network: "mainnet",
      }
    );
  }
  return clientInstance;
}

export type { NovaClient, NovaClientConfig, TransferParams, TransferResult, BalanceResponse, BlockInfo };
