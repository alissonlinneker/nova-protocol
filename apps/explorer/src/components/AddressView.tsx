import { useState, useMemo } from "react";
import { useParams, Link } from "react-router-dom";
import { getMockAddressInfo, getMockTransactionsForBlock } from "../hooks/useExplorer";

function formatDate(timestamp: number): string {
  return new Date(timestamp).toLocaleDateString("en-US", {
    year: "numeric",
    month: "short",
    day: "numeric",
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

export default function AddressView() {
  const { addr } = useParams<{ addr: string }>();
  const addressInfo = useMemo(() => getMockAddressInfo(addr), [addr]);
  const transactions = useMemo(() => getMockTransactionsForBlock(12), []);
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    if (!addr) return;
    await navigator.clipboard.writeText(addr);
    setCopied(true);
    setTimeout(() => setCopied(false), 2_000);
  };

  const totalBalance = addressInfo.balance.reduce(
    (acc, b) => acc + b.amount * (b.symbol === "NOVA" ? 3.0 : b.symbol === "USDN" ? 1.0 : 3.02),
    0
  );

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
          <h1 className="text-xl font-bold text-white">Address</h1>
          <div className="flex items-center gap-2 mt-0.5">
            <code className="text-xs font-mono text-gray-500">
              {addr?.slice(0, 16)}...{addr?.slice(-10)}
            </code>
            <button
              onClick={handleCopy}
              className="text-gray-500 hover:text-nova-400 transition-colors"
            >
              {copied ? (
                <svg className="w-3.5 h-3.5 text-emerald-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                  <path strokeLinecap="round" strokeLinejoin="round" d="M5 13l4 4L19 7" />
                </svg>
              ) : (
                <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                  <path strokeLinecap="round" strokeLinejoin="round" d="M15.75 17.25v3.375c0 .621-.504 1.125-1.125 1.125h-9.75a1.125 1.125 0 01-1.125-1.125V7.875c0-.621.504-1.125 1.125-1.125H6.75a9.06 9.06 0 011.5.124m7.5 10.376h3.375c.621 0 1.125-.504 1.125-1.125V11.25c0-4.46-3.243-8.161-7.5-8.876a9.06 9.06 0 00-1.5-.124H9.375c-.621 0-1.125.504-1.125 1.125v3.5m7.5 10.375H9.375a1.125 1.125 0 01-1.125-1.125v-9.25m12 6.625v-1.875a3.375 3.375 0 00-3.375-3.375h-1.5a1.125 1.125 0 01-1.125-1.125v-1.5a3.375 3.375 0 00-3.375-3.375H9.75" />
                </svg>
              )}
            </button>
          </div>
        </div>
      </div>

      {/* Address Tags */}
      <div className="flex gap-2">
        {addressInfo.isValidator && (
          <span className="nova-badge bg-nova-500/10 text-nova-400">
            Validator
          </span>
        )}
        {addressInfo.isContract && (
          <span className="nova-badge bg-accent-500/10 text-accent-400">
            Contract
          </span>
        )}
        {addressInfo.creditScore && addressInfo.creditScore >= 750 && (
          <span className="nova-badge bg-emerald-500/10 text-emerald-400">
            High Credit Score
          </span>
        )}
      </div>

      {/* Overview Cards */}
      <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
        <div className="nova-card">
          <p className="text-xs text-gray-500 uppercase tracking-wider mb-1">
            Total Value
          </p>
          <p className="text-lg font-bold text-white">
            $
            {totalBalance.toLocaleString("en-US", {
              maximumFractionDigits: 0,
            })}
          </p>
        </div>
        <div className="nova-card">
          <p className="text-xs text-gray-500 uppercase tracking-wider mb-1">
            Transactions
          </p>
          <p className="text-lg font-bold text-white">
            {addressInfo.txCount.toLocaleString()}
          </p>
        </div>
        <div className="nova-card">
          <p className="text-xs text-gray-500 uppercase tracking-wider mb-1">
            First Seen
          </p>
          <p className="text-lg font-bold text-white">
            {formatDate(addressInfo.firstSeen)}
          </p>
        </div>
        <div className="nova-card">
          <p className="text-xs text-gray-500 uppercase tracking-wider mb-1">
            Last Active
          </p>
          <p className="text-lg font-bold text-white">
            {timeAgo(addressInfo.lastActive)}
          </p>
        </div>
      </div>

      {/* Balance Breakdown */}
      <div className="nova-card">
        <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
          Token Balances
        </h3>
        <div className="space-y-3">
          {addressInfo.balance.map((b) => (
            <div
              key={b.symbol}
              className="flex items-center justify-between py-2 px-3 rounded-xl hover:bg-gray-800/50 transition-colors"
            >
              <div className="flex items-center gap-3">
                <div className="w-9 h-9 rounded-full bg-gradient-to-br from-nova-500 to-accent-500 flex items-center justify-center text-xs font-bold text-white">
                  {b.symbol.slice(0, 2)}
                </div>
                <span className="text-sm font-medium text-white">{b.symbol}</span>
              </div>
              <span className="text-sm font-semibold text-white">
                {b.amount.toLocaleString()}
              </span>
            </div>
          ))}
        </div>
      </div>

      {/* Transaction History */}
      <div className="nova-card">
        <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
          Transaction History
        </h3>
        <div className="space-y-2">
          {transactions.map((tx) => (
            <Link
              key={tx.hash}
              to={`/tx/${tx.hash}`}
              className="flex items-center justify-between py-3 px-3 rounded-xl hover:bg-gray-800/50 transition-colors"
            >
              <div className="flex items-center gap-3">
                <div
                  className={`w-8 h-8 rounded-full flex items-center justify-center ${
                    tx.from === addr
                      ? "bg-red-500/20"
                      : "bg-emerald-500/20"
                  }`}
                >
                  {tx.from === addr ? (
                    <svg className="w-3.5 h-3.5 text-red-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                      <path strokeLinecap="round" strokeLinejoin="round" d="M4.5 10.5L12 3m0 0l7.5 7.5M12 3v18" />
                    </svg>
                  ) : (
                    <svg className="w-3.5 h-3.5 text-emerald-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                      <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 13.5L12 21m0 0l-7.5-7.5M12 21V3" />
                    </svg>
                  )}
                </div>
                <div>
                  <code className="text-xs font-mono text-gray-300">
                    {tx.hash.slice(0, 12)}...{tx.hash.slice(-6)}
                  </code>
                  <p className="text-[10px] text-gray-500">
                    {timeAgo(tx.timestamp)}
                  </p>
                </div>
              </div>
              <div className="text-right">
                <p
                  className={`text-sm font-medium ${
                    tx.from === addr ? "text-white" : "text-emerald-400"
                  }`}
                >
                  {tx.from === addr ? "-" : "+"}
                  {tx.amount.toLocaleString()} {tx.symbol}
                </p>
              </div>
            </Link>
          ))}
        </div>
      </div>
    </div>
  );
}
