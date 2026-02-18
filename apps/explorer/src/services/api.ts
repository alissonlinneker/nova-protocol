/**
 * NOVA Explorer API service.
 *
 * Wraps the node's REST and JSON-RPC endpoints. All methods use the
 * native `fetch` API -- no extra HTTP client dependencies.
 *
 * Base URL is configurable via `VITE_NODE_URL` env var.
 * Default: http://localhost:9741
 */

const BASE_URL =
  import.meta.env.VITE_NODE_URL?.replace(/\/+$/, "") ||
  "http://localhost:9741";

// ---------------------------------------------------------------------------
// Response types (mirrors the Rust API structs in node/src/api.rs)
// ---------------------------------------------------------------------------

export interface StatusResponse {
  version: string;
  network: string;
  block_height: number;
  peer_count: number;
  synced: boolean;
  timestamp: string;
}

export interface BlockResponse {
  height: number;
  hash: string;
  parent_hash: string;
  proposer: string;
  tx_count: number;
  timestamp: number;
}

export interface TransactionResponse {
  hash: string;
  sender: string;
  recipient: string;
  amount: number;
  fee: number;
  block_height: number | null;
  status: string;
  timestamp: number;
}

export interface AccountResponse {
  address: string;
  balance: number;
  nonce: number;
  tx_count: number;
}

export interface ValidatorInfo {
  public_key: string;
  stake: number;
  active: boolean;
  last_proposed_block: number;
}

export interface NodeEvent {
  type: "new_block" | "new_transaction";
  height?: number;
  hash?: string;
  tx_count?: number;
  timestamp?: number;
  sender?: string;
  recipient?: string;
  amount?: number;
}

// ---------------------------------------------------------------------------
// JSON-RPC helpers
// ---------------------------------------------------------------------------

interface JsonRpcResponse<T = unknown> {
  jsonrpc: string;
  result?: T;
  error?: { code: number; message: string; data?: unknown };
  id: number;
}

let rpcIdCounter = 0;

async function rpcCall<T>(method: string, params: unknown[] = []): Promise<T> {
  const id = ++rpcIdCounter;
  const res = await fetch(`${BASE_URL}/rpc`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", method, params, id }),
  });
  if (!res.ok) {
    throw new Error(`RPC request failed: ${res.status} ${res.statusText}`);
  }
  const body: JsonRpcResponse<T> = await res.json();
  if (body.error) {
    throw new Error(`RPC error ${body.error.code}: ${body.error.message}`);
  }
  return body.result as T;
}

// ---------------------------------------------------------------------------
// REST endpoints
// ---------------------------------------------------------------------------

export async function fetchStatus(): Promise<StatusResponse> {
  const res = await fetch(`${BASE_URL}/status`);
  if (!res.ok) throw new Error(`GET /status failed: ${res.status}`);
  return res.json();
}

export async function fetchBlock(height: number): Promise<BlockResponse> {
  const res = await fetch(`${BASE_URL}/blocks/${height}`);
  if (!res.ok) {
    if (res.status === 404) throw new Error(`Block not found at height ${height}`);
    throw new Error(`GET /blocks/${height} failed: ${res.status}`);
  }
  return res.json();
}

export async function fetchTransaction(hash: string): Promise<TransactionResponse> {
  const res = await fetch(`${BASE_URL}/transactions/${hash}`);
  if (!res.ok) {
    if (res.status === 404) throw new Error(`Transaction not found: ${hash}`);
    throw new Error(`GET /transactions/${hash} failed: ${res.status}`);
  }
  return res.json();
}

export async function fetchAccount(address: string): Promise<AccountResponse> {
  const res = await fetch(`${BASE_URL}/accounts/${address}`);
  if (!res.ok) throw new Error(`GET /accounts/${address} failed: ${res.status}`);
  return res.json();
}

export async function fetchValidators(): Promise<ValidatorInfo[]> {
  const res = await fetch(`${BASE_URL}/validators`);
  if (!res.ok) throw new Error(`GET /validators failed: ${res.status}`);
  return res.json();
}

// ---------------------------------------------------------------------------
// JSON-RPC endpoints
// ---------------------------------------------------------------------------

export async function fetchBlockHeight(): Promise<number> {
  return rpcCall<number>("nova_blockHeight");
}

export async function fetchPeerCount(): Promise<number> {
  return rpcCall<number>("nova_peerCount");
}

export async function fetchNetworkId(): Promise<string> {
  return rpcCall<string>("nova_networkId");
}

export async function fetchVersion(): Promise<string> {
  return rpcCall<string>("nova_version");
}

export async function fetchBlockViaRpc(height: number): Promise<BlockResponse> {
  return rpcCall<BlockResponse>("nova_getBlock", [height]);
}

export async function fetchTransactionViaRpc(hash: string): Promise<TransactionResponse> {
  return rpcCall<TransactionResponse>("nova_getTransaction", [hash]);
}

// ---------------------------------------------------------------------------
// WebSocket
// ---------------------------------------------------------------------------

const WS_URL = BASE_URL.replace(/^http/, "ws") + "/ws";

export function connectWebSocket(
  onEvent: (event: NodeEvent) => void,
  onOpen?: () => void,
  onClose?: () => void,
): WebSocket {
  const ws = new WebSocket(WS_URL);

  ws.addEventListener("open", () => {
    onOpen?.();
  });

  ws.addEventListener("message", (e) => {
    try {
      const event: NodeEvent = JSON.parse(e.data);
      onEvent(event);
    } catch {
      // Ignore malformed messages.
    }
  });

  ws.addEventListener("close", () => {
    onClose?.();
  });

  ws.addEventListener("error", () => {
    // Error event is always followed by close, handled above.
  });

  return ws;
}
