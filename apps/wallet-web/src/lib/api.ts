/**
 * API service layer for communicating with a NOVA Protocol node.
 *
 * Wraps REST and JSON-RPC calls with proper error handling and type-safe
 * responses. Base URL is read from VITE_NODE_URL or defaults to localhost.
 */

const DEFAULT_NODE_URL = 'http://localhost:9741';
const REQUEST_TIMEOUT_MS = 15_000;

function getNodeUrl(): string {
  try {
    return import.meta.env.VITE_NODE_URL || DEFAULT_NODE_URL;
  } catch {
    return DEFAULT_NODE_URL;
  }
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

export interface NodeStatus {
  version: string;
  network: string;
  blockHeight: number;
  peerCount: number;
  synced: boolean;
}

export interface AccountInfo {
  address: string;
  balance: number;
  nonce: number;
}

export interface TransactionInfo {
  hash: string;
  sender: string;
  recipient: string;
  amount: number;
  fee: number;
  blockHeight: number;
  status: string;
  timestamp: number;
}

// ---------------------------------------------------------------------------
// REST API
// ---------------------------------------------------------------------------

async function fetchWithTimeout(url: string, init?: RequestInit): Promise<Response> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS);

  try {
    const res = await fetch(url, { ...init, signal: controller.signal });
    clearTimeout(timer);
    return res;
  } catch (err) {
    clearTimeout(timer);
    if (err instanceof Error && err.name === 'AbortError') {
      throw new Error(`Request to ${url} timed out after ${REQUEST_TIMEOUT_MS}ms`);
    }
    throw err;
  }
}

/**
 * GET /status - Retrieve node status.
 */
export async function getStatus(nodeUrl?: string): Promise<NodeStatus> {
  const base = nodeUrl || getNodeUrl();
  const res = await fetchWithTimeout(`${base}/status`);

  if (!res.ok) {
    throw new Error(`GET /status failed: HTTP ${res.status}`);
  }

  const data = (await res.json()) as {
    version: string;
    network: string;
    block_height: number;
    peer_count: number;
    synced: boolean;
  };

  return {
    version: data.version,
    network: data.network,
    blockHeight: data.block_height,
    peerCount: data.peer_count,
    synced: data.synced,
  };
}

/**
 * GET /accounts/:address - Retrieve account balance and nonce.
 */
export async function getBalance(address: string, nodeUrl?: string): Promise<AccountInfo> {
  const base = nodeUrl || getNodeUrl();
  const res = await fetchWithTimeout(`${base}/accounts/${address}`);

  if (!res.ok) {
    throw new Error(`GET /accounts/${address} failed: HTTP ${res.status}`);
  }

  return (await res.json()) as AccountInfo;
}

/**
 * GET /transactions/:hash - Retrieve transaction details.
 */
export async function getTransaction(hash: string, nodeUrl?: string): Promise<TransactionInfo> {
  const base = nodeUrl || getNodeUrl();
  const res = await fetchWithTimeout(`${base}/transactions/${hash}`);

  if (!res.ok) {
    throw new Error(`GET /transactions/${hash} failed: HTTP ${res.status}`);
  }

  const data = (await res.json()) as {
    hash: string;
    sender: string;
    recipient: string;
    amount: number;
    fee: number;
    block_height: number;
    status: string;
    timestamp: number;
  };

  return {
    hash: data.hash,
    sender: data.sender,
    recipient: data.recipient,
    amount: data.amount,
    fee: data.fee,
    blockHeight: data.block_height,
    status: data.status,
    timestamp: data.timestamp,
  };
}

// ---------------------------------------------------------------------------
// JSON-RPC API
// ---------------------------------------------------------------------------

let rpcIdCounter = 0;

async function rpcCall<T>(method: string, params: unknown[] = [], nodeUrl?: string): Promise<T> {
  const base = nodeUrl || getNodeUrl();
  const id = ++rpcIdCounter;

  const res = await fetchWithTimeout(`${base}/rpc`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ jsonrpc: '2.0', id, method, params }),
  });

  if (!res.ok) {
    throw new Error(`RPC ${method} failed: HTTP ${res.status}`);
  }

  const json = (await res.json()) as {
    jsonrpc: string;
    id: number;
    result?: T;
    error?: { code: number; message: string };
  };

  if (json.error) {
    throw new Error(`RPC ${method} error ${json.error.code}: ${json.error.message}`);
  }

  return json.result as T;
}

/**
 * JSON-RPC: nova_blockHeight
 */
export async function getBlockHeight(nodeUrl?: string): Promise<number> {
  return rpcCall<number>('nova_blockHeight', [], nodeUrl);
}

/**
 * JSON-RPC: nova_getBalance
 */
export async function getRpcBalance(address: string, nodeUrl?: string): Promise<number> {
  return rpcCall<number>('nova_getBalance', [address], nodeUrl);
}

/**
 * JSON-RPC: nova_sendTransaction - broadcast a signed transaction.
 */
export async function sendTransaction(
  signedTx: Record<string, unknown>,
  nodeUrl?: string,
): Promise<{ tx_hash: string; status: string }> {
  return rpcCall<{ tx_hash: string; status: string }>('nova_sendTransaction', [signedTx], nodeUrl);
}
