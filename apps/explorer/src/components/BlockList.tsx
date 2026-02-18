import { useState, useMemo } from "react";
import { Link } from "react-router-dom";
import { useExplorerStore } from "../store/explorerStore";

function timeAgo(timestamp: number): string {
  const seconds = Math.floor((Date.now() - timestamp) / 1_000);
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ago`;
}

export default function BlockList() {
  const { recentBlocks, loading, error, connected } = useExplorerStore();
  const [searchQuery, setSearchQuery] = useState("");

  const filteredBlocks = useMemo(() => {
    if (!searchQuery) return recentBlocks;
    const q = searchQuery.toLowerCase();
    return recentBlocks.filter(
      (b) =>
        b.height.toString().includes(q) ||
        b.hash.toLowerCase().includes(q) ||
        b.proposer.toLowerCase().includes(q)
    );
  }, [recentBlocks, searchQuery]);

  return (
    <div className="space-y-6">
      {/* Connection Error Banner */}
      {error && (
        <div className="rounded-2xl p-4 bg-red-500/10 border border-red-500/20 flex items-center gap-3">
          <svg className="w-5 h-5 text-red-400 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126zM12 15.75h.007v.008H12v-.008z" />
          </svg>
          <div>
            <p className="text-sm font-medium text-red-400">Connection Error</p>
            <p className="text-xs text-gray-500 mt-0.5">{error}</p>
          </div>
        </div>
      )}

      {/* Search Bar */}
      <div className="relative">
        <svg
          className="absolute left-4 top-1/2 -translate-y-1/2 w-5 h-5 text-gray-500"
          fill="none"
          viewBox="0 0 24 24"
          stroke="currentColor"
          strokeWidth={2}
        >
          <path strokeLinecap="round" strokeLinejoin="round" d="M21 21l-5.197-5.197m0 0A7.5 7.5 0 105.196 5.196a7.5 7.5 0 0010.607 10.607z" />
        </svg>
        <input
          type="text"
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          placeholder="Filter by block height, hash, or proposer..."
          className="nova-input pl-12 text-sm"
        />
      </div>

      {/* Block List */}
      <div className="nova-card">
        <div className="flex items-center justify-between mb-5">
          <h2 className="text-sm font-semibold text-gray-400 uppercase tracking-wider">
            Latest Blocks
          </h2>
          <div className="flex items-center gap-2">
            <span
              className={`w-2 h-2 rounded-full ${
                connected ? "bg-emerald-400 animate-pulse" : "bg-red-400"
              }`}
            />
            <span
              className={`text-xs font-medium ${
                connected ? "text-emerald-400" : "text-red-400"
              }`}
            >
              {connected ? "Live" : "Offline"}
            </span>
          </div>
        </div>

        {/* Loading State */}
        {loading && recentBlocks.length === 0 && (
          <div className="py-12 text-center">
            <div className="inline-block w-6 h-6 border-2 border-nova-500 border-t-transparent rounded-full animate-spin mb-3" />
            <p className="text-sm text-gray-500">Connecting to node...</p>
          </div>
        )}

        {/* Empty State */}
        {!loading && recentBlocks.length === 0 && !error && (
          <div className="py-12 text-center">
            <p className="text-sm text-gray-500">No blocks found</p>
          </div>
        )}

        {/* Desktop Table */}
        {filteredBlocks.length > 0 && (
          <>
            <div className="hidden md:block overflow-x-auto">
              <table className="w-full">
                <thead>
                  <tr className="border-b border-gray-800">
                    <th className="text-left text-xs font-medium text-gray-500 uppercase tracking-wider pb-3 pr-4">
                      Height
                    </th>
                    <th className="text-left text-xs font-medium text-gray-500 uppercase tracking-wider pb-3 pr-4">
                      Hash
                    </th>
                    <th className="text-left text-xs font-medium text-gray-500 uppercase tracking-wider pb-3 pr-4">
                      Txns
                    </th>
                    <th className="text-left text-xs font-medium text-gray-500 uppercase tracking-wider pb-3 pr-4">
                      Proposer
                    </th>
                    <th className="text-right text-xs font-medium text-gray-500 uppercase tracking-wider pb-3">
                      Time
                    </th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-gray-800/50">
                  {filteredBlocks.map((block) => (
                    <tr
                      key={block.height}
                      className="hover:bg-gray-800/30 transition-colors"
                    >
                      <td className="py-3 pr-4">
                        <Link
                          to={`/block/${block.height}`}
                          className="text-sm font-semibold text-nova-400 hover:text-nova-300 transition-colors"
                        >
                          #{block.height.toLocaleString()}
                        </Link>
                      </td>
                      <td className="py-3 pr-4">
                        <Link
                          to={`/block/${block.height}`}
                          className="text-xs font-mono text-gray-400 hover:text-gray-300 transition-colors"
                        >
                          {block.hash.slice(0, 10)}...{block.hash.slice(-8)}
                        </Link>
                      </td>
                      <td className="py-3 pr-4">
                        <span className="text-sm text-gray-300">{block.tx_count}</span>
                      </td>
                      <td className="py-3 pr-4">
                        <Link
                          to={`/address/${block.proposer}`}
                          className="text-xs font-mono text-accent-400 hover:text-accent-300 transition-colors"
                        >
                          {block.proposer.length > 16
                            ? `${block.proposer.slice(0, 16)}...`
                            : block.proposer}
                        </Link>
                      </td>
                      <td className="py-3 text-right">
                        <span className="text-xs text-gray-500">
                          {timeAgo(block.timestamp)}
                        </span>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>

            {/* Mobile Cards */}
            <div className="md:hidden space-y-2">
              {filteredBlocks.map((block) => (
                <Link
                  key={block.height}
                  to={`/block/${block.height}`}
                  className="block bg-gray-800/30 rounded-xl p-4 space-y-2 hover:bg-gray-800/50 transition-colors"
                >
                  <div className="flex items-center justify-between">
                    <span className="text-sm font-semibold text-nova-400">
                      #{block.height.toLocaleString()}
                    </span>
                    <span className="text-xs text-gray-500">{timeAgo(block.timestamp)}</span>
                  </div>
                  <div className="flex items-center justify-between">
                    <code className="text-xs font-mono text-gray-400">
                      {block.hash.slice(0, 14)}...{block.hash.slice(-8)}
                    </code>
                    <span className="nova-badge bg-nova-500/10 text-nova-300 text-[10px]">
                      {block.tx_count} txns
                    </span>
                  </div>
                  <div className="flex items-center justify-between text-xs text-gray-500">
                    <span>
                      Proposer:{" "}
                      <span className="text-accent-400">
                        {block.proposer.length > 14
                          ? `${block.proposer.slice(0, 14)}...`
                          : block.proposer}
                      </span>
                    </span>
                  </div>
                </Link>
              ))}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
