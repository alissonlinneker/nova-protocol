import { useState } from 'react';
import { Link } from 'react-router-dom';
import { useWalletStore } from '../stores/walletStore';
import type { Network } from '../stores/walletStore';

export default function Settings() {
  const {
    network,
    nodeUrl,
    theme,
    secretKey,
    address,
    setNetwork,
    setNodeUrl,
    setTheme,
    lockWallet,
  } = useWalletStore();

  const [customUrl, setCustomUrl] = useState(nodeUrl);
  const [showExportModal, setShowExportModal] = useState(false);
  const [showLockModal, setShowLockModal] = useState(false);
  const [urlSaved, setUrlSaved] = useState(false);
  const [keyRevealed, setKeyRevealed] = useState(false);
  const [keyCopied, setKeyCopied] = useState(false);

  const handleNetworkChange = (newNetwork: Network) => {
    setNetwork(newNetwork);
    const urls: Record<Network, string> = {
      mainnet: 'https://rpc.nova-protocol.io',
      testnet: 'https://testnet-rpc.nova-protocol.io',
      devnet: 'https://devnet-rpc.nova-protocol.io',
    };
    setCustomUrl(urls[newNetwork]);
  };

  const handleSaveUrl = () => {
    setNodeUrl(customUrl);
    setUrlSaved(true);
    setTimeout(() => setUrlSaved(false), 2_000);
  };

  const handleCopyKey = async () => {
    await navigator.clipboard.writeText(secretKey);
    setKeyCopied(true);
    setTimeout(() => setKeyCopied(false), 2_000);
  };

  const handleLock = () => {
    lockWallet();
    setShowLockModal(false);
  };

  const networks: { id: Network; label: string; description: string }[] = [
    {
      id: 'mainnet',
      label: 'Mainnet',
      description: 'Production network with real assets',
    },
    {
      id: 'testnet',
      label: 'Testnet',
      description: 'Test network with faucet tokens',
    },
    {
      id: 'devnet',
      label: 'Devnet',
      description: 'Development network for builders',
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

      {/* Wallet Info */}
      <div className="nova-card">
        <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
          Wallet
        </h3>
        <div className="bg-gray-800/50 rounded-xl p-3 mb-3">
          <label className="text-[11px] uppercase tracking-wider text-gray-500 font-medium">
            Address
          </label>
          <p className="text-xs font-mono text-gray-300 mt-1 break-all select-all">
            {address}
          </p>
        </div>
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
                  ? 'border-nova-500 bg-nova-500/5 ring-1 ring-nova-500/30'
                  : 'border-gray-800 bg-gray-800/30 hover:border-gray-700'
              }`}
            >
              <div>
                <p className="text-sm font-medium text-white">{net.label}</p>
                <p className="text-xs text-gray-500 mt-0.5">{net.description}</p>
              </div>
              <div
                className={`w-5 h-5 rounded-full border-2 flex items-center justify-center ${
                  network === net.id
                    ? 'border-nova-500'
                    : 'border-gray-600'
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
            placeholder="http://localhost:9741"
            className="nova-input flex-1 font-mono text-sm"
          />
          <button
            onClick={handleSaveUrl}
            className="nova-btn-primary px-4 shrink-0"
          >
            {urlSaved ? 'Saved' : 'Save'}
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
            onClick={() => setTheme('dark')}
            className={`flex-1 flex items-center gap-3 p-4 rounded-xl border transition-all ${
              theme === 'dark'
                ? 'border-nova-500 bg-nova-500/5'
                : 'border-gray-800 hover:border-gray-700'
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
            onClick={() => setTheme('light')}
            className={`flex-1 flex items-center gap-3 p-4 rounded-xl border transition-all ${
              theme === 'light'
                ? 'border-nova-500 bg-nova-500/5'
                : 'border-gray-800 hover:border-gray-700'
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
            onClick={() => {
              setShowExportModal(true);
              setKeyRevealed(false);
              setKeyCopied(false);
            }}
            className="w-full flex items-center justify-between p-4 rounded-xl bg-gray-800/50 border border-gray-800 hover:border-gray-700 transition-all"
          >
            <div className="flex items-center gap-3">
              <div className="w-10 h-10 rounded-xl bg-amber-500/10 flex items-center justify-center">
                <svg className="w-5 h-5 text-amber-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                  <path strokeLinecap="round" strokeLinejoin="round" d="M15.75 5.25a3 3 0 013 3m3 0a6 6 0 01-7.029 5.912c-.563-.097-1.159.026-1.563.43L10.5 17.25H8.25v2.25H6v2.25H2.25v-2.818c0-.597.237-1.17.659-1.591l6.499-6.499c.404-.404.527-1 .43-1.563A6 6 0 1121.75 8.25z" />
                </svg>
              </div>
              <div className="text-left">
                <p className="text-sm font-medium text-white">Export Secret Key</p>
                <p className="text-xs text-gray-500">
                  Back up your Ed25519 secret key
                </p>
              </div>
            </div>
            <svg className="w-4 h-4 text-gray-500" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M8.25 4.5l7.5 7.5-7.5 7.5" />
            </svg>
          </button>

          <button
            onClick={() => setShowLockModal(true)}
            className="w-full flex items-center justify-between p-4 rounded-xl bg-gray-800/50 border border-red-900/30 hover:border-red-800/50 transition-all"
          >
            <div className="flex items-center gap-3">
              <div className="w-10 h-10 rounded-xl bg-red-500/10 flex items-center justify-center">
                <svg className="w-5 h-5 text-red-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                  <path strokeLinecap="round" strokeLinejoin="round" d="M5.636 5.636a9 9 0 1012.728 0M12 3v9" />
                </svg>
              </div>
              <div className="text-left">
                <p className="text-sm font-medium text-red-400">Lock Wallet</p>
                <p className="text-xs text-gray-500">
                  Clear keys from memory and return to setup
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

      {/* Export Key Modal */}
      {showExportModal && (
        <div className="fixed inset-0 bg-black/70 backdrop-blur-sm z-50 flex items-center justify-center p-4">
          <div className="bg-gray-900 border border-gray-800 rounded-2xl p-6 max-w-md w-full">
            <div className="flex items-center justify-between mb-4">
              <h3 className="text-lg font-semibold text-white">Export Secret Key</h3>
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
                Never share your secret key with anyone. Anyone with access to
                this key can control your funds.
              </p>
            </div>

            <div className="bg-gray-800 rounded-xl p-4 mb-4">
              <label className="text-[11px] uppercase tracking-wider text-gray-500 font-medium">
                Ed25519 Secret Key (hex)
              </label>
              {keyRevealed ? (
                <p className="text-xs font-mono text-gray-300 mt-2 break-all select-all">
                  {secretKey}
                </p>
              ) : (
                <p className="text-xs font-mono text-gray-500 mt-2">
                  {'*'.repeat(64)}
                </p>
              )}
            </div>

            <div className="flex gap-3">
              <button
                onClick={() => setShowExportModal(false)}
                className="nova-btn-secondary flex-1"
              >
                Close
              </button>
              {keyRevealed ? (
                <button onClick={handleCopyKey} className="nova-btn-primary flex-1">
                  {keyCopied ? 'Copied' : 'Copy Key'}
                </button>
              ) : (
                <button
                  onClick={() => setKeyRevealed(true)}
                  className="nova-btn-primary flex-1"
                >
                  Reveal Key
                </button>
              )}
            </div>
          </div>
        </div>
      )}

      {/* Lock Wallet Modal */}
      {showLockModal && (
        <div className="fixed inset-0 bg-black/70 backdrop-blur-sm z-50 flex items-center justify-center p-4">
          <div className="bg-gray-900 border border-gray-800 rounded-2xl p-6 max-w-md w-full">
            <div className="flex items-center justify-between mb-4">
              <h3 className="text-lg font-semibold text-white">Lock Wallet</h3>
              <button
                onClick={() => setShowLockModal(false)}
                className="w-8 h-8 rounded-lg bg-gray-800 flex items-center justify-center hover:bg-gray-700 transition-colors"
              >
                <svg className="w-4 h-4 text-gray-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                  <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
                </svg>
              </button>
            </div>

            <div className="bg-red-500/10 border border-red-500/30 rounded-xl p-4 mb-4">
              <p className="text-sm text-red-400">
                This will clear your keys from memory and local storage.
                Make sure you have backed up your secret key before proceeding.
                You will need it to re-import your wallet.
              </p>
            </div>

            <div className="flex gap-3">
              <button
                onClick={() => setShowLockModal(false)}
                className="nova-btn-secondary flex-1"
              >
                Cancel
              </button>
              <button
                onClick={handleLock}
                className="flex-1 bg-red-600 hover:bg-red-500 text-white font-medium py-2.5 px-5 rounded-xl transition-all duration-200 active:scale-95"
              >
                Lock Wallet
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
