import { useState, useEffect } from "react";
import { useParams, Link } from "react-router-dom";
import { fetchAccount, type AccountResponse } from "../services/api";

export default function AddressView() {
  const { addr } = useParams<{ addr: string }>();
  const [account, setAccount] = useState<AccountResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!addr) return;

    setLoading(true);
    setError(null);

    fetchAccount(addr)
      .then((data) => {
        setAccount(data);
        setLoading(false);
      })
      .catch((err) => {
        setError(err instanceof Error ? err.message : "Failed to fetch account");
        setLoading(false);
      });
  }, [addr]);

  const handleCopy = async () => {
    if (!addr) return;
    await navigator.clipboard.writeText(addr);
    setCopied(true);
    setTimeout(() => setCopied(false), 2_000);
  };

  if (loading) {
    return (
      <div className="py-16 text-center">
        <div className="inline-block w-6 h-6 border-2 border-nova-500 border-t-transparent rounded-full animate-spin mb-3" />
        <p className="text-sm text-gray-500">Loading account...</p>
      </div>
    );
  }

  if (error || !account) {
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
          <h1 className="text-xl font-bold text-white">Address</h1>
        </div>
        <div className="rounded-2xl p-6 bg-red-500/10 border border-red-500/20 text-center">
          <p className="text-sm text-red-400">{error ?? "Account not found"}</p>
        </div>
      </div>
    );
  }

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
              {addr && addr.length > 26
                ? `${addr.slice(0, 16)}...${addr.slice(-10)}`
                : addr}
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

      {/* Overview Cards */}
      <div className="grid grid-cols-2 lg:grid-cols-3 gap-3">
        <div className="nova-card">
          <p className="text-xs text-gray-500 uppercase tracking-wider mb-1">
            Balance
          </p>
          <p className="text-lg font-bold text-white">
            {account.balance.toLocaleString()} photons
          </p>
        </div>
        <div className="nova-card">
          <p className="text-xs text-gray-500 uppercase tracking-wider mb-1">
            Nonce
          </p>
          <p className="text-lg font-bold text-white">
            {account.nonce.toLocaleString()}
          </p>
        </div>
        <div className="nova-card">
          <p className="text-xs text-gray-500 uppercase tracking-wider mb-1">
            Transactions
          </p>
          <p className="text-lg font-bold text-white">
            {account.tx_count.toLocaleString()}
          </p>
        </div>
      </div>

      {/* Account Details */}
      <div className="nova-card space-y-0">
        <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
          Account Details
        </h3>

        <div className="divide-y divide-gray-800/50">
          <div className="flex flex-col sm:flex-row sm:items-center justify-between py-3 gap-1">
            <span className="text-sm text-gray-500">Address</span>
            <code className="text-sm font-mono text-gray-300 break-all">
              {account.address}
            </code>
          </div>

          <div className="flex flex-col sm:flex-row sm:items-center justify-between py-3 gap-1">
            <span className="text-sm text-gray-500">Balance</span>
            <span className="text-sm font-semibold text-white">
              {account.balance.toLocaleString()} photons
            </span>
          </div>

          <div className="flex flex-col sm:flex-row sm:items-center justify-between py-3 gap-1">
            <span className="text-sm text-gray-500">Nonce</span>
            <span className="text-sm text-gray-300">{account.nonce}</span>
          </div>

          <div className="flex flex-col sm:flex-row sm:items-center justify-between py-3 gap-1">
            <span className="text-sm text-gray-500">Transaction Count</span>
            <span className="text-sm text-gray-300">
              {account.tx_count.toLocaleString()}
            </span>
          </div>
        </div>
      </div>
    </div>
  );
}
