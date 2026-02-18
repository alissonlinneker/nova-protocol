/**
 * Zustand store for the NOVA Block Explorer.
 *
 * Manages network status, cached blocks, and connection state.
 * Polls the node REST API every 2 seconds for new blocks and
 * optionally subscribes to the WebSocket for live events.
 */

import { create } from "zustand";
import {
  fetchStatus,
  fetchBlock,
  connectWebSocket,
  type StatusResponse,
  type BlockResponse,
  type NodeEvent,
} from "../services/api";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface ExplorerState {
  /** Current network status from GET /status. Null until first fetch. */
  status: StatusResponse | null;

  /** Ordered list of recently fetched blocks (newest first). */
  recentBlocks: BlockResponse[];

  /** Whether we have successfully connected to the node at least once. */
  connected: boolean;

  /** If the most recent fetch attempt failed, this holds the error message. */
  error: string | null;

  /** Whether a fetch is currently in flight (first load only). */
  loading: boolean;

  /** Internal polling interval handle. */
  _pollHandle: ReturnType<typeof setInterval> | null;

  /** Active WebSocket connection. */
  _ws: WebSocket | null;

  // Actions
  /** Fetch the latest status and recent blocks from the node. */
  refresh: () => Promise<void>;

  /** Start automatic polling (every 2s). Idempotent. */
  startPolling: () => void;

  /** Stop automatic polling. */
  stopPolling: () => void;

  /** Connect to the WebSocket for live events. */
  connectWs: () => void;

  /** Disconnect the WebSocket. */
  disconnectWs: () => void;
}

// Maximum number of recent blocks to cache in-memory.
const MAX_RECENT_BLOCKS = 30;

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

export const useExplorerStore = create<ExplorerState>((set, get) => ({
  status: null,
  recentBlocks: [],
  connected: false,
  error: null,
  loading: true,
  _pollHandle: null,
  _ws: null,

  refresh: async () => {
    try {
      const status = await fetchStatus();

      // Determine which blocks we need to fetch. We always keep
      // the latest `MAX_RECENT_BLOCKS` blocks cached.
      const existing = get().recentBlocks;
      const highestCached = existing.length > 0 ? existing[0].height : -1;

      const newBlocks: BlockResponse[] = [];
      const fetchFrom = Math.max(0, status.block_height - MAX_RECENT_BLOCKS + 1);
      const fetchUpTo = status.block_height;

      if (highestCached < fetchFrom) {
        // Cache is entirely stale — fetch the full window.
        const promises: Promise<BlockResponse | null>[] = [];
        for (let h = fetchUpTo; h >= fetchFrom; h--) {
          promises.push(fetchBlock(h).catch(() => null));
        }
        const results = await Promise.all(promises);
        for (const b of results) {
          if (b) newBlocks.push(b);
        }
      } else if (fetchUpTo > highestCached) {
        // Only fetch new blocks since last poll.
        const promises: Promise<BlockResponse | null>[] = [];
        for (let h = fetchUpTo; h > highestCached; h--) {
          promises.push(fetchBlock(h).catch(() => null));
        }
        const results = await Promise.all(promises);
        for (const b of results) {
          if (b) newBlocks.push(b);
        }
      }

      set((state) => {
        let merged: BlockResponse[];
        if (newBlocks.length > 0) {
          // Merge new blocks with existing, deduplicate, sort descending.
          const blockMap = new Map<number, BlockResponse>();
          for (const b of state.recentBlocks) blockMap.set(b.height, b);
          for (const b of newBlocks) blockMap.set(b.height, b);
          merged = Array.from(blockMap.values())
            .sort((a, b) => b.height - a.height)
            .slice(0, MAX_RECENT_BLOCKS);
        } else {
          merged = state.recentBlocks;
        }

        return {
          status,
          recentBlocks: merged,
          connected: true,
          error: null,
          loading: false,
        };
      });
    } catch (err) {
      set({
        connected: false,
        error: err instanceof Error ? err.message : "Connection failed",
        loading: false,
      });
    }
  },

  startPolling: () => {
    const state = get();
    if (state._pollHandle) return; // Already polling.

    // Immediate first fetch.
    state.refresh();

    const handle = setInterval(() => {
      get().refresh();
    }, 2_000);

    set({ _pollHandle: handle });
  },

  stopPolling: () => {
    const handle = get()._pollHandle;
    if (handle) {
      clearInterval(handle);
      set({ _pollHandle: null });
    }
  },

  connectWs: () => {
    if (get()._ws) return; // Already connected.

    const ws = connectWebSocket(
      (event: NodeEvent) => {
        if (event.type === "new_block" && event.height != null) {
          // A new block was broadcast — fetch its full data.
          fetchBlock(event.height)
            .then((block) => {
              set((state) => {
                const exists = state.recentBlocks.some(
                  (b) => b.height === block.height,
                );
                if (exists) return state;

                const merged = [block, ...state.recentBlocks]
                  .sort((a, b) => b.height - a.height)
                  .slice(0, MAX_RECENT_BLOCKS);

                return {
                  recentBlocks: merged,
                  status: state.status
                    ? { ...state.status, block_height: block.height }
                    : state.status,
                };
              });
            })
            .catch(() => {
              // Block fetch failed, will be picked up by next poll.
            });
        }
      },
      () => set({ connected: true }),
      () => set({ _ws: null }),
    );

    set({ _ws: ws });
  },

  disconnectWs: () => {
    const ws = get()._ws;
    if (ws) {
      ws.close();
      set({ _ws: null });
    }
  },
}));
