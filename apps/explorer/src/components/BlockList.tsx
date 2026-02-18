import { useState, useMemo } from "react";
import { Link } from "react-router-dom";
import { getMockBlocks } from "../hooks/useExplorer";
import type { Block } from "../hooks/useExplorer";

function timeAgo(timestamp: number): string {
  const seconds = Math.floor((Date.now() - timestamp) / 1_000);
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ago`;
}

function formatBytes(bytes: number): string {
  if (bytes < 1_024) return `${bytes} B`;
  if (bytes < 1_048_576) return `${(bytes / 1_024).toFixed(1)} KB`;
  return `${(bytes / 1_048_576).toFixed(1)} MB`;
}

export default function BlockList() {
  const [blocks] = useState<Block[]>(() => getMockBlocks(20));
  const [searchQuery, setSearchQuery] = useState("");

  const filteredBlocks = useMemo(() => {
    if (!searchQuery) return blocks;
    const q = searchQuery.toLowerCase();
    return blocks.filter(
      (b) =>
        b.height.toString().includes(q) ||
        b.hash.toLowerCase().includes(q) ||
        b.validator.toLowerCase().includes(q)
    );
  }, [blocks, searchQuery]);

  return (
    <div className="space-y-6">
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
          placeholder="Search by block height, hash, or validator..."
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
            <span className="w-2 h-2 rounded-full bg-emerald-400 animate-pulse" />
            <span className="text-xs text-emerald-400 font-medium">Live</span>
          </div>
        </div>

        {/* Desktop Table */}
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
                  Validator
                </th>
                <th className="text-left text-xs font-medium text-gray-500 uppercase tracking-wider pb-3 pr-4">
                  Size
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
                    <span className="text-sm font-semibold text-nova-400">
                      #{block.height.toLocaleString()}
                    </span>
                  </td>
                  <td className="py-3 pr-4">
                    <code className="text-xs font-mono text-gray-400">
                      {block.hash.slice(0, 10)}...{block.hash.slice(-8)}
                    </code>
                  </td>
                  <td className="py-3 pr-4">
                    <span className="text-sm text-gray-300">{block.txCount}</span>
                  </td>
                  <td className="py-3 pr-4">
                    <Link
                      to={`/address/${block.validator}`}
                      className="text-xs font-mono text-accent-400 hover:text-accent-300 transition-colors"
                    >
                      {block.validator.slice(0, 16)}...
                    </Link>
                  </td>
                  <td className="py-3 pr-4">
                    <span className="text-xs text-gray-500">
                      {formatBytes(block.size)}
                    </span>
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
            <div
              key={block.height}
              className="bg-gray-800/30 rounded-xl p-4 space-y-2"
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
                  {block.txCount} txns
                </span>
              </div>
              <div className="flex items-center justify-between text-xs text-gray-500">
                <span>
                  Validator:{" "}
                  <Link
                    to={`/address/${block.validator}`}
                    className="text-accent-400"
                  >
                    {block.validator.slice(0, 14)}...
                  </Link>
                </span>
                <span>{formatBytes(block.size)}</span>
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
