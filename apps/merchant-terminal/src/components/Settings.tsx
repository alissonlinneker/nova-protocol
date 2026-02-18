import { useState, useCallback } from "react";
import { useMerchantStore } from "../hooks/useMerchant";
import { clearNodeUrl } from "../lib/api";

export default function Settings() {
  const {
    merchantName,
    merchantId,
    address,
    nodeUrl,
    nodeConnected,
    nodeStatus,
    setMerchantName,
    setMerchantId,
    setAddress,
    setNodeUrl,
    exportTransactions,
    clearTransactions,
    transactions,
    fetchStatus,
  } = useMerchantStore();

  const [editNodeUrl, setEditNodeUrl] = useState(nodeUrl);
  const [urlSaved, setUrlSaved] = useState(false);
  const [showClearConfirm, setShowClearConfirm] = useState(false);
  const [copied, setCopied] = useState(false);

  const handleSaveUrl = useCallback(() => {
    setNodeUrl(editNodeUrl);
    setUrlSaved(true);
    fetchStatus();
    setTimeout(() => setUrlSaved(false), 2_000);
  }, [editNodeUrl, setNodeUrl, fetchStatus]);

  const handleResetUrl = useCallback(() => {
    clearNodeUrl();
    const defaultUrl = import.meta.env.VITE_NODE_URL || "http://localhost:9741";
    setEditNodeUrl(defaultUrl);
    setNodeUrl(defaultUrl);
    fetchStatus();
  }, [setNodeUrl, fetchStatus]);

  const handleCopyAddress = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(address);
      setCopied(true);
      setTimeout(() => setCopied(false), 2_000);
    } catch {
      // Clipboard API may not be available in insecure contexts
    }
  }, [address]);

  const handleExport = useCallback(() => {
    const csv = exportTransactions();
    const blob = new Blob([csv], { type: "text/csv;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `nova-transactions-${new Date().toISOString().slice(0, 10)}.csv`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
  }, [exportTransactions]);

  const handleClearHistory = useCallback(() => {
    clearTransactions();
    setShowClearConfirm(false);
  }, [clearTransactions]);

  return (
    <div className="space-y-6 max-w-2xl mx-auto">
      {/* Merchant Identity */}
      <div className="nova-card">
        <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
          Merchant Identity
        </h3>

        <div className="space-y-4">
          <div>
            <label className="block text-xs text-gray-500 mb-1">
              Merchant Name
            </label>
            <input
              type="text"
              value={merchantName}
              onChange={(e) => setMerchantName(e.target.value)}
              className="nova-input"
              placeholder="Your business name"
            />
          </div>

          <div>
            <label className="block text-xs text-gray-500 mb-1">
              Merchant ID
            </label>
            <input
              type="text"
              value={merchantId}
              onChange={(e) => setMerchantId(e.target.value)}
              className="nova-input"
              placeholder="MERCH-NV-0001"
            />
          </div>
        </div>
      </div>

      {/* NOVA Address */}
      <div className="nova-card">
        <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
          NOVA Address
        </h3>

        <div className="space-y-4">
          <div>
            <label className="block text-xs text-gray-500 mb-1">
              Receiving Address
            </label>
            <div className="flex gap-2">
              <input
                type="text"
                value={address}
                onChange={(e) => setAddress(e.target.value)}
                className="nova-input font-mono text-sm"
                placeholder="nova1..."
              />
              <button
                onClick={handleCopyAddress}
                className="nova-btn-secondary shrink-0 px-3"
                title="Copy address"
              >
                {copied ? (
                  <svg className="w-4 h-4 text-emerald-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                    <path strokeLinecap="round" strokeLinejoin="round" d="M4.5 12.75l6 6 9-13.5" />
                  </svg>
                ) : (
                  <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                    <path strokeLinecap="round" strokeLinejoin="round" d="M15.666 3.888A2.25 2.25 0 0013.5 2.25h-3c-1.03 0-1.9.693-2.166 1.638m7.332 0c.055.194.084.4.084.612v0a.75.75 0 01-.75.75H9.75a.75.75 0 01-.75-.75v0c0-.212.03-.418.084-.612m7.332 0c.646.049 1.288.11 1.927.184 1.1.128 1.907 1.077 1.907 2.185V19.5a2.25 2.25 0 01-2.25 2.25H6.75A2.25 2.25 0 014.5 19.5V6.257c0-1.108.806-2.057 1.907-2.185a48.208 48.208 0 011.927-.184" />
                  </svg>
                )}
              </button>
            </div>
            <p className="text-[10px] text-gray-600 mt-1">
              All payment requests will be directed to this address
            </p>
          </div>
        </div>
      </div>

      {/* Node Configuration */}
      <div className="nova-card">
        <div className="flex items-center justify-between mb-4">
          <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider">
            Node Connection
          </h3>
          <div className="flex items-center gap-2">
            <span
              className={`w-2 h-2 rounded-full ${
                nodeConnected ? "bg-emerald-400" : "bg-red-400"
              }`}
            />
            <span
              className={`text-xs font-medium ${
                nodeConnected ? "text-emerald-400" : "text-red-400"
              }`}
            >
              {nodeConnected ? "Connected" : "Disconnected"}
            </span>
          </div>
        </div>

        <div className="space-y-4">
          <div>
            <label className="block text-xs text-gray-500 mb-1">
              Node URL
            </label>
            <div className="flex gap-2">
              <input
                type="text"
                value={editNodeUrl}
                onChange={(e) => {
                  setEditNodeUrl(e.target.value);
                  setUrlSaved(false);
                }}
                className="nova-input font-mono text-sm"
                placeholder="http://localhost:9741"
              />
              <button
                onClick={handleSaveUrl}
                className="nova-btn-primary shrink-0"
              >
                {urlSaved ? "Saved" : "Save"}
              </button>
            </div>
            <div className="flex items-center gap-3 mt-2">
              <button
                onClick={handleResetUrl}
                className="text-[10px] text-gray-600 hover:text-gray-400 transition-colors"
              >
                Reset to default
              </button>
            </div>
          </div>

          {nodeStatus && (
            <div className="bg-gray-800/50 rounded-xl p-4 space-y-2 text-sm">
              <div className="flex justify-between">
                <span className="text-gray-500">Version</span>
                <span className="text-gray-300 font-mono">{nodeStatus.version}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-gray-500">Network</span>
                <span className="text-gray-300">{nodeStatus.network}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-gray-500">Block Height</span>
                <span className="text-gray-300 font-mono">
                  {nodeStatus.block_height.toLocaleString("en-US")}
                </span>
              </div>
              <div className="flex justify-between">
                <span className="text-gray-500">Peers</span>
                <span className="text-gray-300">{nodeStatus.peer_count}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-gray-500">Synced</span>
                <span className={nodeStatus.synced ? "text-emerald-400" : "text-amber-400"}>
                  {nodeStatus.synced ? "Yes" : "No"}
                </span>
              </div>
            </div>
          )}
        </div>
      </div>

      {/* Transaction History Management */}
      <div className="nova-card">
        <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
          Transaction History
        </h3>

        <div className="space-y-3">
          <div className="flex items-center justify-between bg-gray-800/50 rounded-xl p-4">
            <div>
              <p className="text-sm text-gray-300">
                {transactions.length} transaction{transactions.length !== 1 ? "s" : ""}
              </p>
              <p className="text-xs text-gray-500">stored locally</p>
            </div>
            <button
              onClick={handleExport}
              disabled={transactions.length === 0}
              className="nova-btn-secondary text-sm disabled:opacity-40 disabled:cursor-not-allowed"
            >
              <span className="flex items-center gap-2">
                <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                  <path strokeLinecap="round" strokeLinejoin="round" d="M3 16.5v2.25A2.25 2.25 0 005.25 21h13.5A2.25 2.25 0 0021 18.75V16.5M16.5 12L12 16.5m0 0L7.5 12m4.5 4.5V3" />
                </svg>
                Export CSV
              </span>
            </button>
          </div>

          {!showClearConfirm ? (
            <button
              onClick={() => setShowClearConfirm(true)}
              disabled={transactions.length === 0}
              className="text-xs text-red-400/60 hover:text-red-400 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
            >
              Clear transaction history
            </button>
          ) : (
            <div className="bg-red-500/10 border border-red-500/20 rounded-xl p-4">
              <p className="text-sm text-red-400 mb-3">
                This will permanently delete all {transactions.length} local transaction records. This action cannot be undone.
              </p>
              <div className="flex gap-2">
                <button
                  onClick={handleClearHistory}
                  className="bg-red-600 hover:bg-red-500 text-white text-sm font-medium py-2 px-4 rounded-lg transition-colors"
                >
                  Clear All
                </button>
                <button
                  onClick={() => setShowClearConfirm(false)}
                  className="nova-btn-secondary text-sm"
                >
                  Cancel
                </button>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
