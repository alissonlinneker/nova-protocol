import { useState, useEffect } from "react";
import { useParams, Link } from "react-router-dom";
import { fetchTransaction, type TransactionResponse } from "../services/api";

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

export default function TransactionDetail() {
  const { hash } = useParams<{ hash: string }>();
  const [tx, setTx] = useState<TransactionResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!hash) return;

    setLoading(true);
    setError(null);

    fetchTransaction(hash)
      .then((data) => {
        setTx(data);
        setLoading(false);
      })
      .catch((err) => {
        setError(err instanceof Error ? err.message : "Failed to fetch transaction");
        setLoading(false);
      });
  }, [hash]);

  if (loading) {
    return (
      <div className="py-16 text-center">
        <div className="inline-block w-6 h-6 border-2 border-nova-500 border-t-transparent rounded-full animate-spin mb-3" />
        <p className="text-sm text-gray-500">Loading transaction...</p>
      </div>
    );
  }

  if (error || !tx) {
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
          <h1 className="text-xl font-bold text-white">Transaction</h1>
        </div>
        <div className="rounded-2xl p-6 bg-red-500/10 border border-red-500/20 text-center">
          <p className="text-sm text-red-400">{error ?? "Transaction not found"}</p>
          {hash && (
            <code className="text-xs font-mono text-gray-500 mt-2 block break-all">
              {hash}
            </code>
          )}
        </div>
      </div>
    );
  }

  const statusColor =
    tx.status === "confirmed"
      ? "emerald"
      : tx.status === "pending"
      ? "amber"
      : "red";

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
        <div>
          <h1 className="text-xl font-bold text-white">Transaction Detail</h1>
          <code className="text-xs font-mono text-gray-500">
            {tx.hash.length > 28
              ? `${tx.hash.slice(0, 16)}...${tx.hash.slice(-12)}`
              : tx.hash}
          </code>
        </div>
      </div>

      {/* Status Banner */}
      <div
        className={`rounded-2xl p-4 flex items-center gap-3 bg-${statusColor}-500/10 border border-${statusColor}-500/20`}
      >
        {tx.status === "confirmed" ? (
          <svg className="w-6 h-6 text-emerald-400 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M9 12.75L11.25 15 15 9.75M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
          </svg>
        ) : tx.status === "pending" ? (
          <svg className="w-6 h-6 text-amber-400 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M12 6v6h4.5m4.5 0a9 9 0 11-18 0 9 9 0 0118 0z" />
          </svg>
        ) : (
          <svg className="w-6 h-6 text-red-400 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M9.75 9.75l4.5 4.5m0-4.5l-4.5 4.5M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
          </svg>
        )}
        <div>
          <p
            className={`text-sm font-semibold ${
              tx.status === "confirmed"
                ? "text-emerald-400"
                : tx.status === "pending"
                ? "text-amber-400"
                : "text-red-400"
            }`}
          >
            {tx.status === "confirmed"
              ? "Transaction Confirmed"
              : tx.status === "pending"
              ? "Transaction Pending"
              : "Transaction Failed"}
          </p>
          <p className="text-xs text-gray-500 mt-0.5">
            {tx.block_height != null
              ? `Included in block #${tx.block_height.toLocaleString()}`
              : "Waiting for block inclusion"}
          </p>
        </div>
      </div>

      {/* Transaction Details */}
      <div className="nova-card space-y-0">
        <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
          Details
        </h3>

        <div className="divide-y divide-gray-800/50">
          <div className="flex flex-col sm:flex-row sm:items-center justify-between py-3 gap-1">
            <span className="text-sm text-gray-500">Transaction Hash</span>
            <code className="text-sm font-mono text-gray-300 break-all">
              {tx.hash}
            </code>
          </div>

          {tx.block_height != null && (
            <div className="flex flex-col sm:flex-row sm:items-center justify-between py-3 gap-1">
              <span className="text-sm text-gray-500">Block Height</span>
              <Link
                to={`/block/${tx.block_height}`}
                className="text-sm font-semibold text-nova-400 hover:text-nova-300 transition-colors"
              >
                #{tx.block_height.toLocaleString()}
              </Link>
            </div>
          )}

          <div className="flex flex-col sm:flex-row sm:items-center justify-between py-3 gap-1">
            <span className="text-sm text-gray-500">Timestamp</span>
            <span className="text-sm text-gray-300">{formatDate(tx.timestamp)}</span>
          </div>

          <div className="flex flex-col sm:flex-row sm:items-center justify-between py-3 gap-1">
            <span className="text-sm text-gray-500">Sender</span>
            <Link
              to={`/address/${tx.sender}`}
              className="text-sm font-mono text-accent-400 hover:text-accent-300 transition-colors break-all"
            >
              {tx.sender}
            </Link>
          </div>

          <div className="flex flex-col sm:flex-row sm:items-center justify-between py-3 gap-1">
            <span className="text-sm text-gray-500">Recipient</span>
            <Link
              to={`/address/${tx.recipient}`}
              className="text-sm font-mono text-accent-400 hover:text-accent-300 transition-colors break-all"
            >
              {tx.recipient}
            </Link>
          </div>

          <div className="flex flex-col sm:flex-row sm:items-center justify-between py-3 gap-1">
            <span className="text-sm text-gray-500">Amount</span>
            <span className="text-sm font-semibold text-white">
              {tx.amount.toLocaleString()} photons
            </span>
          </div>

          <div className="flex flex-col sm:flex-row sm:items-center justify-between py-3 gap-1">
            <span className="text-sm text-gray-500">Fee</span>
            <span className="text-sm text-gray-300">
              {tx.fee.toLocaleString()} photons
            </span>
          </div>

          <div className="flex flex-col sm:flex-row sm:items-center justify-between py-3 gap-1">
            <span className="text-sm text-gray-500">Status</span>
            <span
              className={`nova-badge ${
                tx.status === "confirmed"
                  ? "bg-emerald-500/10 text-emerald-400"
                  : tx.status === "pending"
                  ? "bg-amber-500/10 text-amber-400"
                  : "bg-red-500/10 text-red-400"
              }`}
            >
              {tx.status.charAt(0).toUpperCase() + tx.status.slice(1)}
            </span>
          </div>
        </div>
      </div>
    </div>
  );
}
