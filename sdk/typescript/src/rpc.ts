/**
 * NOVA Protocol — Shared JSON-RPC Transport
 *
 * Thin JSON-RPC caller used by wallet and credit modules to communicate
 * with a NOVA node over HTTP.
 */

import type { RpcResponse } from './types.js';

/**
 * Send a JSON-RPC 2.0 request to a NOVA node and return the typed result.
 *
 * @param nodeUrl — Full URL of the JSON-RPC endpoint.
 * @param method  — RPC method name (e.g. `"nova_getBalance"`).
 * @param params  — Positional parameters for the method.
 *
 * @throws {Error} On HTTP-level failures or JSON-RPC error responses.
 */
export async function rpcCall<T>(nodeUrl: string, method: string, params: unknown[] = []): Promise<T> {
  const body = JSON.stringify({
    jsonrpc: '2.0',
    id: Date.now(),
    method,
    params,
  });

  const res = await fetch(nodeUrl, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body,
  });

  if (!res.ok) {
    throw new Error(`RPC HTTP error: ${res.status} ${res.statusText}`);
  }

  const json = (await res.json()) as RpcResponse<T>;
  if (json.error) {
    throw new Error(`RPC error ${json.error.code}: ${json.error.message}`);
  }
  return json.result as T;
}
