import { useState } from "react";
import { Link } from "react-router-dom";
import { useWalletStore } from "../stores/walletStore";
import type { Network } from "../stores/walletStore";

export default function Settings() {
  const { network, nodeUrl, theme, setNetwork, setNodeUrl, setTheme } =
    useWalletStore();

  const [customUrl, setCustomUrl] = useState(nodeUrl);
  const [showExportModal, setShowExportModal] = useState(false);
  const [urlSaved, setUrlSaved] = useState(false);

  const handleNetworkChange = (newNetwork: Network) => {
    setNetwork(newNetwork);
    setCustomUrl(
      newNetwork === "mainnet"
        ? "https://rpc.nova-protocol.io"
        : newNetwork === "testnet"
        ? "https://testnet-rpc.nova-protocol.io"
        : "https://devnet-rpc.nova-protocol.io"
    );
  };

  const handleSaveUrl = () => {
    setNodeUrl(customUrl);
    setUrlSaved(true);
    setTimeout(() => setUrlSaved(false), 2_000);
  };

  const networks: { id: Network; label: string; description: string }[] = [
    {
      id: "mainnet",
      label: "Mainnet",
      description: "Production network with real assets",
    },
    {
      id: "testnet",
      label: "Testnet",
      description: "Test network with faucet tokens",
    },
    {
      id: "devnet",
      label: "Devnet",
      description: "Development network for builders",
    },
  ];

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
        <h1 className="text-xl font-bold text-white">Settings</h1>
      </div>

      {/* Network Selection */}
      <div className="nova-card">
        <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
          Network
        </h3>
        <div className="space-y-2">
          {networks.map((net) => (
            <button
              key={net.id}
              onClick={() => handleNetworkChange(net.id)}
              className={`w-full flex items-center justify-between p-4 rounded-xl border transition-all text-left ${
                network === net.id
                  ? "border-nova-500 bg-nova-500/5 ring-1 ring-nova-500/30"
                  : "border-gray-800 bg-gray-800/30 hover:border-gray-700"
              }`}
            >
              <div>
                <p className="text-sm font-medium text-white">{net.label}</p>
                <p className="text-xs text-gray-500 mt-0.5">{net.description}</p>
              </div>
              <div
                className={`w-5 h-5 rounded-full border-2 flex items-center justify-center ${
                  network === net.id
                    ? "border-nova-500"
                    : "border-gray-600"
                }`}
              >
                {network === net.id && (
                  <div className="w-2.5 h-2.5 rounded-full bg-nova-500" />
                )}
              </div>
            </button>
          ))}
        </div>
      </div>

      {/* Node URL */}
      <div className="nova-card">
        <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
          Node URL
        </h3>
        <div className="flex gap-3">
          <input
            type="url"
            value={customUrl}
            onChange={(e) => setCustomUrl(e.target.value)}
            placeholder="https://rpc.nova-protocol.io"
            className="nova-input flex-1 font-mono text-sm"
          />
          <button
            onClick={handleSaveUrl}
            className="nova-btn-primary px-4 shrink-0"
          >
            {urlSaved ? "Saved" : "Save"}
          </button>
        </div>
        <p className="text-xs text-gray-600 mt-2">
          Custom RPC endpoint for connecting to the NOVA network
        </p>
      </div>

      {/* Theme */}
      <div className="nova-card">
        <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
          Appearance
        </h3>
        <div className="flex gap-3">
          <button
            onClick={() => setTheme("dark")}
            className={`flex-1 flex items-center gap-3 p-4 rounded-xl border transition-all ${
              theme === "dark"
                ? "border-nova-500 bg-nova-500/5"
                : "border-gray-800 hover:border-gray-700"
            }`}
          >
            <div className="w-10 h-10 rounded-xl bg-gray-800 flex items-center justify-center">
              <svg className="w-5 h-5 text-gray-300" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M21.752 15.002A9.718 9.718 0 0118 15.75c-5.385 0-9.75-4.365-9.75-9.75 0-1.33.266-2.597.748-3.752A9.753 9.753 0 003 11.25C3 16.635 7.365 21 12.75 21a9.753 9.753 0 009.002-5.998z" />
              </svg>
            </div>
            <span className="text-sm font-medium text-white">Dark</span>
          </button>
          <button
            onClick={() => setTheme("light")}
            className={`flex-1 flex items-center gap-3 p-4 rounded-xl border transition-all ${
              theme === "light"
                ? "border-nova-500 bg-nova-500/5"
                : "border-gray-800 hover:border-gray-700"
            }`}
          >
            <div className="w-10 h-10 rounded-xl bg-gray-800 flex items-center justify-center">
              <svg className="w-5 h-5 text-amber-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M12 3v2.25m6.364.386l-1.591 1.591M21 12h-2.25m-.386 6.364l-1.591-1.591M12 18.75V21m-4.773-4.227l-1.591 1.591M5.25 12H3m4.227-4.773L5.636 5.636M15.75 12a3.75 3.75 0 11-7.5 0 3.75 3.75 0 017.5 0z" />
              </svg>
            </div>
            <span className="text-sm font-medium text-white">Light</span>
          </button>
        </div>
      </div>

      {/* Security */}
      <div className="nova-card">
        <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
          Security
        </h3>
        <div className="space-y-3">
          <button
            onClick={() => setShowExportModal(true)}
            className="w-full flex items-center justify-between p-4 rounded-xl bg-gray-800/50 border border-gray-800 hover:border-gray-700 transition-all"
          >
            <div className="flex items-center gap-3">
              <div className="w-10 h-10 rounded-xl bg-amber-500/10 flex items-center justify-center">
                <svg className="w-5 h-5 text-amber-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                  <path strokeLinecap="round" strokeLinejoin="round" d="M15.75 5.25a3 3 0 013 3m3 0a6 6 0 01-7.029 5.912c-.563-.097-1.159.026-1.563.43L10.5 17.25H8.25v2.25H6v2.25H2.25v-2.818c0-.597.237-1.17.659-1.591l6.499-6.499c.404-.404.527-1 .43-1.563A6 6 0 1121.75 8.25z" />
                </svg>
              </div>
              <div className="text-left">
                <p className="text-sm font-medium text-white">Export Keys</p>
                <p className="text-xs text-gray-500">
                  Backup your private key securely
                </p>
              </div>
            </div>
            <svg className="w-4 h-4 text-gray-500" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M8.25 4.5l7.5 7.5-7.5 7.5" />
            </svg>
          </button>

          <button className="w-full flex items-center justify-between p-4 rounded-xl bg-gray-800/50 border border-gray-800 hover:border-gray-700 transition-all">
            <div className="flex items-center gap-3">
              <div className="w-10 h-10 rounded-xl bg-nova-500/10 flex items-center justify-center">
                <svg className="w-5 h-5 text-nova-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                  <path strokeLinecap="round" strokeLinejoin="round" d="M9 12.75L11.25 15 15 9.75m-3-7.036A11.959 11.959 0 013.598 6 11.99 11.99 0 003 9.749c0 5.592 3.824 10.29 9 11.623 5.176-1.332 9-6.03 9-11.622 0-1.31-.21-2.571-.598-3.751h-.152c-3.196 0-6.1-1.248-8.25-3.285z" />
                </svg>
              </div>
              <div className="text-left">
                <p className="text-sm font-medium text-white">Recovery Phrase</p>
                <p className="text-xs text-gray-500">
                  View your 24-word recovery phrase
                </p>
              </div>
            </div>
            <svg className="w-4 h-4 text-gray-500" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M8.25 4.5l7.5 7.5-7.5 7.5" />
            </svg>
          </button>
        </div>
      </div>

      {/* Version Info */}
      <div className="text-center py-4">
        <p className="text-xs text-gray-600">
          NOVA Wallet v0.1.0 &middot; Protocol v1
        </p>
      </div>

      {/* Export Modal */}
      {showExportModal && (
        <div className="fixed inset-0 bg-black/70 backdrop-blur-sm z-50 flex items-center justify-center p-4">
          <div className="bg-gray-900 border border-gray-800 rounded-2xl p-6 max-w-md w-full">
            <div className="flex items-center justify-between mb-4">
              <h3 className="text-lg font-semibold text-white">Export Keys</h3>
              <button
                onClick={() => setShowExportModal(false)}
                className="w-8 h-8 rounded-lg bg-gray-800 flex items-center justify-center hover:bg-gray-700 transition-colors"
              >
                <svg className="w-4 h-4 text-gray-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                  <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
                </svg>
              </button>
            </div>

            <div className="bg-amber-500/10 border border-amber-500/30 rounded-xl p-4 mb-4">
              <p className="text-sm text-amber-400">
                Never share your private key with anyone. Anyone with access to
                your private key can control your funds.
              </p>
            </div>

            <div className="bg-gray-800 rounded-xl p-4 mb-4">
              <label className="text-[11px] uppercase tracking-wider text-gray-500 font-medium">
                Private Key (Encrypted)
              </label>
              <p className="text-xs font-mono text-gray-400 mt-2 break-all select-all">
                ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
              </p>
            </div>

            <div className="flex gap-3">
              <button
                onClick={() => setShowExportModal(false)}
                className="nova-btn-secondary flex-1"
              >
                Cancel
              </button>
              <button className="nova-btn-primary flex-1">
                Reveal Key
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
