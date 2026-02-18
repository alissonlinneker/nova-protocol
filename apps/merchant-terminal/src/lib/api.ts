import type {
  NodeStatus,
  AccountInfo,
  TransactionInfo,
  JsonRpcResponse,
} from "./types";

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const DEFAULT_NODE_URL = "http://localhost:9741";

function getNodeUrl(): string {
  // Check localStorage first (user-configured), then env variable, then default.
  const stored = localStorage.getItem("nova_node_url");
  if (stored) return stored.replace(/\/+$/, "");

  const envUrl = import.meta.env.VITE_NODE_URL;
  if (envUrl) return envUrl.replace(/\/+$/, "");

  return DEFAULT_NODE_URL;
}

export function setNodeUrl(url: string): void {
  localStorage.setItem("nova_node_url", url.replace(/\/+$/, ""));
}

export function clearNodeUrl(): void {
  localStorage.removeItem("nova_node_url");
}

// ---------------------------------------------------------------------------
// REST Endpoints
// ---------------------------------------------------------------------------

async function fetchJson<T>(path: string): Promise<T> {
  const base = getNodeUrl();
  const resp = await fetch(`${base}${path}`, {
    headers: { Accept: "application/json" },
  });

  if (!resp.ok) {
    const body = await resp.text().catch(() => "");
    throw new ApiError(resp.status, body || resp.statusText);
  }

  return resp.json() as Promise<T>;
}

/** GET /status -- Node status summary (version, height, peers, sync state) */
export async function getStatus(): Promise<NodeStatus> {
  return fetchJson<NodeStatus>("/status");
}

/** GET /accounts/:address -- Account balance and nonce */
export async function getBalance(address: string): Promise<AccountInfo> {
  return fetchJson<AccountInfo>(`/accounts/${encodeURIComponent(address)}`);
}

/** GET /transactions/:hash -- Transaction details by hash */
export async function getTransaction(hash: string): Promise<TransactionInfo> {
  return fetchJson<TransactionInfo>(`/transactions/${encodeURIComponent(hash)}`);
}

// ---------------------------------------------------------------------------
// JSON-RPC Gateway
// ---------------------------------------------------------------------------

let rpcIdCounter = 0;

async function rpcCall<T>(method: string, params: unknown[] = []): Promise<T> {
  const base = getNodeUrl();
  const id = ++rpcIdCounter;

  const resp = await fetch(`${base}/rpc`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Accept: "application/json",
    },
    body: JSON.stringify({
      jsonrpc: "2.0",
      method,
      params,
      id,
    }),
  });

  if (!resp.ok) {
    const body = await resp.text().catch(() => "");
    throw new ApiError(resp.status, body || resp.statusText);
  }

  const envelope = (await resp.json()) as JsonRpcResponse<T>;

  if (envelope.error) {
    throw new RpcError(envelope.error.code, envelope.error.message);
  }

  return envelope.result as T;
}

/** JSON-RPC nova_blockHeight -- Current finalized block height */
export async function getBlockHeight(): Promise<number> {
  return rpcCall<number>("nova_blockHeight");
}

/** JSON-RPC nova_getBalance -- Account balance via RPC */
export async function getRpcBalance(address: string): Promise<number> {
  const account = await getBalance(address);
  return account.balance;
}

/** JSON-RPC nova_sendTransaction -- Submit a signed transaction */
export async function sendTransaction(txHex: string): Promise<string> {
  return rpcCall<string>("nova_sendTransaction", [txHex]);
}

// ---------------------------------------------------------------------------
// WebSocket
// ---------------------------------------------------------------------------

export type WsEventHandler = (event: MessageEvent) => void;

/**
 * Opens a WebSocket connection to the node for live block/tx event streaming.
 * Returns a cleanup function to close the socket.
 */
export function connectWebSocket(
  onMessage: WsEventHandler,
  onOpen?: () => void,
  onClose?: () => void,
  onError?: (err: Event) => void,
): () => void {
  const base = getNodeUrl().replace(/^http/, "ws");
  const ws = new WebSocket(`${base}/ws`);

  ws.addEventListener("message", onMessage);
  if (onOpen) ws.addEventListener("open", onOpen);
  if (onClose) ws.addEventListener("close", onClose);
  if (onError) ws.addEventListener("error", onError);

  return () => {
    ws.close();
  };
}

// ---------------------------------------------------------------------------
// Error Types
// ---------------------------------------------------------------------------

export class ApiError extends Error {
  constructor(
    public readonly status: number,
    public readonly body: string,
  ) {
    super(`API error ${status}: ${body}`);
    this.name = "ApiError";
  }
}

export class RpcError extends Error {
  constructor(
    public readonly code: number,
    message: string,
  ) {
    super(message);
    this.name = "RpcError";
  }
}
