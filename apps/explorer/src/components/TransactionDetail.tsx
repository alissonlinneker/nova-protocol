import { useMemo } from "react";
import { useParams, Link } from "react-router-dom";
import { getMockTransaction } from "../hooks/useExplorer";

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
  const tx = useMemo(() => getMockTransaction(hash), [hash]);

  const typeColors: Record<string, string> = {
    transfer: "bg-nova-500/10 text-nova-400",
    credit_issue: "bg-accent-500/10 text-accent-400",
    stake: "bg-emerald-500/10 text-emerald-400",
    unstake: "bg-amber-500/10 text-amber-400",
    governance: "bg-blue-500/10 text-blue-400",
  };

  const typeLabels: Record<string, string> = {
    transfer: "Transfer",
    credit_issue: "Credit Issue",
    stake: "Stake",
    unstake: "Unstake",
    governance: "Governance",
  };

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
            {tx.hash.slice(0, 16)}...{tx.hash.slice(-12)}
          </code>
        </div>
      </div>

      {/* Status Banner */}
      <div
        className={`rounded-2xl p-4 flex items-center gap-3 ${
          tx.status === "confirmed"
            ? "bg-emerald-500/10 border border-emerald-500/20"
            : tx.status === "pending"
            ? "bg-amber-500/10 border border-amber-500/20"
            : "bg-red-500/10 border border-red-500/20"
        }`}
      >
        {tx.status === "confirmed" ? (
          <svg className="w-6 h-6 text-emerald-400 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M9 12.75L11.25 15 15 9.75M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
          </svg>
        ) : (
          <svg className="w-6 h-6 text-amber-400 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M12 6v6h4.5m4.5 0a9 9 0 11-18 0 9 9 0 0118 0z" />
          </svg>
        )}
        <div>
          <p
            className={`text-sm font-semibold ${
              tx.status === "confirmed" ? "text-emerald-400" : "text-amber-400"
            }`}
          >
            {tx.status === "confirmed"
              ? "Transaction Confirmed"
              : "Transaction Pending"}
          </p>
          <p className="text-xs text-gray-500 mt-0.5">
            {tx.status === "confirmed"
              ? `Included in block #${tx.blockHeight.toLocaleString()}`
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

          <div className="flex flex-col sm:flex-row sm:items-center justify-between py-3 gap-1">
            <span className="text-sm text-gray-500">Type</span>
            <span className={`nova-badge ${typeColors[tx.type]}`}>
              {typeLabels[tx.type]}
            </span>
          </div>

          <div className="flex flex-col sm:flex-row sm:items-center justify-between py-3 gap-1">
            <span className="text-sm text-gray-500">Block Height</span>
            <span className="text-sm font-semibold text-nova-400">
              #{tx.blockHeight.toLocaleString()}
            </span>
          </div>

          <div className="flex flex-col sm:flex-row sm:items-center justify-between py-3 gap-1">
            <span className="text-sm text-gray-500">Timestamp</span>
            <span className="text-sm text-gray-300">{formatDate(tx.timestamp)}</span>
          </div>

          <div className="flex flex-col sm:flex-row sm:items-center justify-between py-3 gap-1">
            <span className="text-sm text-gray-500">From</span>
            <Link
              to={`/address/${tx.from}`}
              className="text-sm font-mono text-accent-400 hover:text-accent-300 transition-colors break-all"
            >
              {tx.from}
            </Link>
          </div>

          <div className="flex flex-col sm:flex-row sm:items-center justify-between py-3 gap-1">
            <span className="text-sm text-gray-500">To</span>
            <Link
              to={`/address/${tx.to}`}
              className="text-sm font-mono text-accent-400 hover:text-accent-300 transition-colors break-all"
            >
              {tx.to}
            </Link>
          </div>

          <div className="flex flex-col sm:flex-row sm:items-center justify-between py-3 gap-1">
            <span className="text-sm text-gray-500">Amount</span>
            <span className="text-sm font-semibold text-white">
              {tx.amount.toLocaleString()} {tx.symbol}
            </span>
          </div>

          <div className="flex flex-col sm:flex-row sm:items-center justify-between py-3 gap-1">
            <span className="text-sm text-gray-500">Transaction Fee</span>
            <span className="text-sm text-gray-300">
              {tx.fee.toFixed(6)} NOVA
            </span>
          </div>

          <div className="flex flex-col sm:flex-row sm:items-center justify-between py-3 gap-1">
            <span className="text-sm text-gray-500">Gas Used</span>
            <span className="text-sm text-gray-300">
              {tx.gasUsed.toLocaleString()}
            </span>
          </div>

          {tx.memo && (
            <div className="flex flex-col sm:flex-row sm:items-center justify-between py-3 gap-1">
              <span className="text-sm text-gray-500">Memo</span>
              <span className="text-sm text-gray-300">{tx.memo}</span>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
