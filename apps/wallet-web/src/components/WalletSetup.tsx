import { useState } from 'react';
import { useWalletStore } from '../stores/walletStore';

type SetupStep = 'welcome' | 'create' | 'import' | 'backup';

export default function WalletSetup() {
  const { createWallet, importWallet } = useWalletStore();

  const [step, setStep] = useState<SetupStep>('welcome');
  const [secretKeyHex, setSecretKeyHex] = useState('');
  const [newAddress, setNewAddress] = useState('');
  const [importKey, setImportKey] = useState('');
  const [importError, setImportError] = useState('');
  const [backedUp, setBackedUp] = useState(false);
  const [showKey, setShowKey] = useState(false);

  const handleCreate = () => {
    const result = createWallet();
    setSecretKeyHex(result.secretKeyHex);
    setNewAddress(result.address);
    setStep('backup');
  };

  const handleImport = () => {
    setImportError('');
    const cleaned = importKey.trim().replace(/^0x/, '');

    if (cleaned.length !== 64) {
      setImportError('Secret key must be exactly 64 hex characters (32 bytes).');
      return;
    }

    if (!/^[0-9a-fA-F]+$/.test(cleaned)) {
      setImportError('Secret key must contain only hexadecimal characters (0-9, a-f).');
      return;
    }

    try {
      importWallet(cleaned);
    } catch (err) {
      setImportError(
        err instanceof Error ? err.message : 'Failed to import key. Please check the format.',
      );
    }
  };

  const handleCopyKey = async () => {
    await navigator.clipboard.writeText(secretKeyHex);
  };

  // ---------------------------------------------------------------------------
  // Welcome screen
  // ---------------------------------------------------------------------------

  if (step === 'welcome') {
    return (
      <div className="min-h-screen flex items-center justify-center bg-gray-950 px-4">
        <div className="max-w-md w-full space-y-8">
          {/* Logo */}
          <div className="text-center">
            <div className="w-20 h-20 rounded-2xl bg-gradient-to-br from-nova-500 to-accent-500 flex items-center justify-center mx-auto mb-6">
              <span className="text-3xl font-bold text-white">N</span>
            </div>
            <h1 className="text-3xl font-bold nova-gradient-text mb-2">
              NOVA Wallet
            </h1>
            <p className="text-gray-400 text-sm">
              Sovereign payments on the NOVA Protocol
            </p>
          </div>

          {/* Actions */}
          <div className="space-y-3 pt-4">
            <button
              onClick={handleCreate}
              className="nova-btn-primary w-full py-4 text-base font-semibold"
            >
              Create New Wallet
            </button>
            <button
              onClick={() => setStep('import')}
              className="nova-btn-secondary w-full py-4 text-base"
            >
              Import Existing Wallet
            </button>
          </div>

          <p className="text-center text-xs text-gray-600 pt-4">
            NOVA Protocol v1.0
          </p>
        </div>
      </div>
    );
  }

  // ---------------------------------------------------------------------------
  // Backup screen (after create)
  // ---------------------------------------------------------------------------

  if (step === 'backup') {
    return (
      <div className="min-h-screen flex items-center justify-center bg-gray-950 px-4">
        <div className="max-w-md w-full space-y-6">
          <div className="text-center">
            <div className="w-16 h-16 rounded-full bg-emerald-500/20 flex items-center justify-center mx-auto mb-4">
              <svg
                className="w-8 h-8 text-emerald-400"
                fill="none"
                viewBox="0 0 24 24"
                stroke="currentColor"
                strokeWidth={2}
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  d="M9 12.75L11.25 15 15 9.75M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
                />
              </svg>
            </div>
            <h2 className="text-2xl font-bold text-white mb-1">
              Wallet Created
            </h2>
            <p className="text-sm text-gray-400">
              Back up your secret key now. You will not be able to see it again.
            </p>
          </div>

          {/* Address */}
          <div className="nova-card">
            <label className="text-[11px] uppercase tracking-wider text-gray-500 font-medium">
              Your NOVA Address
            </label>
            <p className="text-sm font-mono text-gray-200 mt-1 break-all select-all">
              {newAddress}
            </p>
          </div>

          {/* Secret key */}
          <div className="bg-amber-500/10 border border-amber-500/30 rounded-xl p-4">
            <div className="flex items-start gap-3">
              <svg
                className="w-5 h-5 text-amber-400 shrink-0 mt-0.5"
                fill="none"
                viewBox="0 0 24 24"
                stroke="currentColor"
                strokeWidth={2}
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126zM12 15.75h.007v.008H12v-.008z"
                />
              </svg>
              <div>
                <p className="text-sm font-semibold text-amber-400 mb-1">
                  Save your secret key
                </p>
                <p className="text-xs text-amber-400/80">
                  Anyone with this key can access your funds. Store it securely
                  and never share it with anyone.
                </p>
              </div>
            </div>
          </div>

          <div className="nova-card">
            <label className="text-[11px] uppercase tracking-wider text-gray-500 font-medium">
              Secret Key
            </label>
            <div className="mt-2 relative">
              {showKey ? (
                <p className="text-xs font-mono text-gray-300 break-all select-all pr-20">
                  {secretKeyHex}
                </p>
              ) : (
                <p className="text-xs font-mono text-gray-500 pr-20">
                  {'*'.repeat(64)}
                </p>
              )}
              <div className="absolute top-0 right-0 flex gap-2">
                <button
                  onClick={() => setShowKey(!showKey)}
                  className="text-xs text-nova-400 hover:text-nova-300 font-medium"
                >
                  {showKey ? 'Hide' : 'Reveal'}
                </button>
                <button
                  onClick={handleCopyKey}
                  className="text-xs text-nova-400 hover:text-nova-300 font-medium"
                >
                  Copy
                </button>
              </div>
            </div>
          </div>

          <label className="flex items-center gap-3 cursor-pointer">
            <input
              type="checkbox"
              checked={backedUp}
              onChange={(e) => setBackedUp(e.target.checked)}
              className="w-4 h-4 rounded border-gray-600 bg-gray-800 text-nova-500 focus:ring-nova-500"
            />
            <span className="text-sm text-gray-300">
              I have saved my secret key in a secure location
            </span>
          </label>

          <button
            disabled={!backedUp}
            onClick={() => {
              // Wallet is already initialized from handleCreate,
              // just clear the sensitive data from local component state.
              setSecretKeyHex('');
            }}
            className="nova-btn-primary w-full py-4 text-base font-semibold disabled:opacity-40 disabled:cursor-not-allowed"
          >
            Continue to Wallet
          </button>
        </div>
      </div>
    );
  }

  // ---------------------------------------------------------------------------
  // Import screen
  // ---------------------------------------------------------------------------

  return (
    <div className="min-h-screen flex items-center justify-center bg-gray-950 px-4">
      <div className="max-w-md w-full space-y-6">
        <div className="text-center">
          <h2 className="text-2xl font-bold text-white mb-1">
            Import Wallet
          </h2>
          <p className="text-sm text-gray-400">
            Enter your 32-byte Ed25519 secret key in hex format
          </p>
        </div>

        {importError && (
          <div className="bg-red-500/10 border border-red-500/30 rounded-xl p-4 text-sm text-red-400">
            {importError}
          </div>
        )}

        <div className="nova-card space-y-4">
          <div>
            <label className="block text-sm font-medium text-gray-400 mb-2">
              Secret Key (hex)
            </label>
            <textarea
              value={importKey}
              onChange={(e) => setImportKey(e.target.value)}
              placeholder="Enter 64-character hex string..."
              rows={3}
              className="nova-input font-mono text-sm resize-none"
            />
            <p className="text-xs text-gray-600 mt-1">
              64 hex characters (32 bytes). With or without 0x prefix.
            </p>
          </div>
        </div>

        <div className="flex gap-3">
          <button
            onClick={() => {
              setStep('welcome');
              setImportKey('');
              setImportError('');
            }}
            className="nova-btn-secondary flex-1 py-3.5"
          >
            Back
          </button>
          <button
            onClick={handleImport}
            disabled={!importKey.trim()}
            className="nova-btn-primary flex-1 py-3.5 disabled:opacity-40 disabled:cursor-not-allowed"
          >
            Import
          </button>
        </div>
      </div>
    </div>
  );
}
