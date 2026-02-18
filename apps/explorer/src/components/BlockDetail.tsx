import { useState, useEffect } from "react";
import { useParams, Link } from "react-router-dom";
import { fetchBlock, type BlockResponse } from "../services/api";

function formatDate(timestamp: number): string {
  return new Date(timestamp).toLocaleString("en-US", {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function timeAgo(timestamp: number): string {
  const seconds = Math.floor((Date.now() - timestamp) / 1_000);
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

export default function BlockDetail() {
  const { height } = useParams<{ height: string }>();
  const [block, setBlock] = useState<BlockResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!height) return;

    const h = parseInt(height, 10);
    if (isNaN(h) || h < 0) {
      setError("Invalid block height");
      setLoading(false);
      return;
    }

    setLoading(true);
    setError(null);

    fetchBlock(h)
      .then((data) => {
        setBlock(data);
        setLoading(false);
      })
      .catch((err) => {
        setError(err instanceof Error ? err.message : "Failed to fetch block");
        setLoading(false);
      });
  }, [height]);

  if (loading) {
    return (
      <div className="py-16 text-center">
        <div className="inline-block w-6 h-6 border-2 border-nova-500 border-t-transparent rounded-full animate-spin mb-3" />
        <p className="text-sm text-gray-500">Loading block #{height}...</p>
      </div>
    );
  }

  if (error || !block) {
    return (
      <div className="space-y-6">
        <div className="flex items-center gap-3">
          <Link
            to="/"
            className="w-9 h-9 rounded-xl bg-gray-800 flex items-center justify-center hover:bg-gray-700 transition-colors"
          >
            <svg className="w-4 h-4 text-gray-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M15.75 19.5L8.25 12l7.5-7.5" />
            </svg>
          </Link>
          <h1 className="text-xl font-bold text-white">Block #{height}</h1>
        </div>
        <div className="rounded-2xl p-6 bg-red-500/10 border border-red-500/20 text-center">
          <p className="text-sm text-red-400">{error ?? "Block not found"}</p>
        </div>
      </div>
    );
  }

  const parentHeight = block.height > 0 ? block.height - 1 : null;

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center gap-3">
        <Link
          to="/"
          className="w-9 h-9 rounded-xl bg-gray-800 flex items-center justify-center hover:bg-gray-700 transition-colors"
        >
          <svg className="w-4 h-4 text-gray-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M15.75 19.5L8.25 12l7.5-7.5" />
          </svg>
        </Link>
        <div className="flex-1">
          <h1 className="text-xl font-bold text-white">
            Block #{block.height.toLocaleString()}
          </h1>
          <p className="text-xs text-gray-500 mt-0.5">{timeAgo(block.timestamp)}</p>
        </div>

        {/* Block navigation */}
        <div className="flex items-center gap-2">
          {parentHeight !== null && (
            <Link
              to={`/block/${parentHeight}`}
              className="w-9 h-9 rounded-xl bg-gray-800 flex items-center justify-center hover:bg-gray-700 transition-colors"
              title={`Block #${parentHeight}`}
            >
              <svg className="w-4 h-4 text-gray-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M15.75 19.5L8.25 12l7.5-7.5" />
              </svg>
            </Link>
          )}
          <Link
            to={`/block/${block.height + 1}`}
            className="w-9 h-9 rounded-xl bg-gray-800 flex items-center justify-center hover:bg-gray-700 transition-colors"
            title={`Block #${block.height + 1}`}
          >
            <svg className="w-4 h-4 text-gray-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M8.25 4.5l7.5 7.5-7.5 7.5" />
            </svg>
          </Link>
        </div>
      </div>

      {/* Block Summary Cards */}
      <div className="grid grid-cols-2 lg:grid-cols-3 gap-3">
        <div className="nova-card">
          <p className="text-xs text-gray-500 uppercase tracking-wider mb-1">Height</p>
          <p className="text-lg font-bold text-nova-400">
            #{block.height.toLocaleString()}
          </p>
        </div>
        <div className="nova-card">
          <p className="text-xs text-gray-500 uppercase tracking-wider mb-1">Transactions</p>
          <p className="text-lg font-bold text-white">{block.tx_count}</p>
        </div>
        <div className="nova-card">
          <p className="text-xs text-gray-500 uppercase tracking-wider mb-1">Timestamp</p>
          <p className="text-sm font-semibold text-white">{formatDate(block.timestamp)}</p>
        </div>
      </div>

      {/* Block Details */}
      <div className="nova-card space-y-0">
        <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
          Block Header
        </h3>

        <div className="divide-y divide-gray-800/50">
          <div className="flex flex-col sm:flex-row sm:items-center justify-between py-3 gap-1">
            <span className="text-sm text-gray-500">Block Hash</span>
            <code className="text-sm font-mono text-gray-300 break-all">{block.hash}</code>
          </div>

          <div className="flex flex-col sm:flex-row sm:items-center justify-between py-3 gap-1">
            <span className="text-sm text-gray-500">Parent Hash</span>
            {parentHeight !== null ? (
              <Link
                to={`/block/${parentHeight}`}
                className="text-sm font-mono text-accent-400 hover:text-accent-300 transition-colors break-all"
              >
                {block.parent_hash}
              </Link>
            ) : (
              <code className="text-sm font-mono text-gray-300 break-all">
                {block.parent_hash}
              </code>
            )}
          </div>

          <div className="flex flex-col sm:flex-row sm:items-center justify-between py-3 gap-1">
            <span className="text-sm text-gray-500">Proposer</span>
            <Link
              to={`/address/${block.proposer}`}
              className="text-sm font-mono text-accent-400 hover:text-accent-300 transition-colors break-all"
            >
              {block.proposer}
            </Link>
          </div>

          <div className="flex flex-col sm:flex-row sm:items-center justify-between py-3 gap-1">
            <span className="text-sm text-gray-500">Transaction Count</span>
            <span className="text-sm font-semibold text-white">{block.tx_count}</span>
          </div>

          <div className="flex flex-col sm:flex-row sm:items-center justify-between py-3 gap-1">
            <span className="text-sm text-gray-500">Timestamp</span>
            <span className="text-sm text-gray-300">{formatDate(block.timestamp)}</span>
          </div>
        </div>
      </div>

      {/* Transactions in Block */}
      {block.tx_count > 0 && (
        <div className="nova-card">
          <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
            Transactions ({block.tx_count})
          </h3>
          <p className="text-xs text-gray-500">
            This block contains {block.tx_count} transaction{block.tx_count !== 1 ? "s" : ""}.
            Individual transaction data can be queried by hash.
          </p>
        </div>
      )}

      {block.tx_count === 0 && (
        <div className="nova-card">
          <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
            Transactions
          </h3>
          <p className="text-xs text-gray-500">This block contains no transactions.</p>
        </div>
      )}
    </div>
  );
}
