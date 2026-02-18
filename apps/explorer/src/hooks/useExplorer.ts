/**
 * Re-exports the explorer store for convenience.
 *
 * All mock data generators have been removed. The explorer now
 * fetches real data from the NOVA node REST/JSON-RPC API.
 */

export { useExplorerStore } from "../store/explorerStore";
