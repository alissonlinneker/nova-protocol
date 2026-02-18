import { useState } from "react";
import { useWallet } from "../hooks/useWallet";

export default function IdentityCard() {
  const { address, createdAt, truncatedAddress, truncatedPublicKey, network } =
    useWallet();
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    await navigator.clipboard.writeText(address);
    setCopied(true);
    setTimeout(() => setCopied(false), 2_000);
  };

  const formattedDate = new Date(createdAt).toLocaleDateString("en-US", {
    year: "numeric",
    month: "short",
    day: "numeric",
  });

  return (
    <div className="relative overflow-hidden rounded-2xl bg-gradient-to-br from-nova-700 via-nova-600 to-accent-600 p-[1px]">
      <div className="relative rounded-2xl bg-gray-950/80 backdrop-blur-xl p-6">
        {/* Decorative elements */}
        <div className="absolute top-0 right-0 w-40 h-40 bg-nova-500/10 rounded-full -translate-y-1/2 translate-x-1/2" />
        <div className="absolute bottom-0 left-0 w-24 h-24 bg-accent-500/10 rounded-full translate-y-1/2 -translate-x-1/2" />

        <div className="relative z-10">
          {/* Header */}
          <div className="flex items-center justify-between mb-5">
            <div className="flex items-center gap-3">
              <div className="w-10 h-10 rounded-xl bg-gradient-to-br from-nova-500 to-accent-500 flex items-center justify-center">
                <svg
                  className="w-5 h-5 text-white"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                  strokeWidth={2}
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    d="M15 9h3.75M15 12h3.75M15 15h3.75M4.5 19.5h15a2.25 2.25 0 002.25-2.25V6.75A2.25 2.25 0 0019.5 4.5h-15a2.25 2.25 0 00-2.25 2.25v10.5A2.25 2.25 0 004.5 19.5zm6-10.125a1.875 1.875 0 11-3.75 0 1.875 1.875 0 013.75 0zm1.294 6.336a6.721 6.721 0 01-3.17.789 6.721 6.721 0 01-3.168-.789 3.376 3.376 0 016.338 0z"
                  />
                </svg>
              </div>
              <div>
                <h3 className="text-sm font-semibold text-white">NOVA ID</h3>
                <span className="text-xs text-nova-300 capitalize">{network}</span>
              </div>
            </div>

            <div className="flex items-center gap-2">
              <span className="w-2 h-2 rounded-full bg-emerald-400 animate-pulse" />
              <span className="text-xs text-emerald-400 font-medium">Connected</span>
            </div>
          </div>

          {/* Address */}
          <div className="mb-4">
            <label className="text-[11px] uppercase tracking-wider text-gray-500 font-medium">
              Address
            </label>
            <div className="flex items-center gap-2 mt-1">
              <code className="text-sm font-mono text-gray-200">{truncatedAddress}</code>
              <button
                onClick={handleCopy}
                className="text-gray-500 hover:text-nova-400 transition-colors"
                title="Copy address"
              >
                {copied ? (
                  <svg className="w-4 h-4 text-emerald-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                    <path strokeLinecap="round" strokeLinejoin="round" d="M5 13l4 4L19 7" />
                  </svg>
                ) : (
                  <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                    <path strokeLinecap="round" strokeLinejoin="round" d="M15.666 3.888A2.25 2.25 0 0013.5 2.25h-3c-1.03 0-1.9.693-2.166 1.638m7.332 0c.055.194.084.4.084.612v0a.75.75 0 01-.75.75H9.75a.75.75 0 01-.75-.75v0c0-.212.03-.418.084-.612m7.332 0c.646.049 1.288.11 1.927.184 1.1.128 1.907 1.077 1.907 2.185V19.5a2.25 2.25 0 01-2.25 2.25H6.75A2.25 2.25 0 014.5 19.5V6.257c0-1.108.806-2.057 1.907-2.185a48.208 48.208 0 011.927-.184" />
                  </svg>
                )}
              </button>
            </div>
          </div>

          {/* Public Key */}
          <div className="mb-4">
            <label className="text-[11px] uppercase tracking-wider text-gray-500 font-medium">
              Public Key
            </label>
            <code className="block text-xs font-mono text-gray-400 mt-1">
              {truncatedPublicKey}
            </code>
          </div>

          {/* Footer */}
          <div className="flex items-center justify-between pt-3 border-t border-gray-800/50">
            <div>
              <label className="text-[11px] uppercase tracking-wider text-gray-500 font-medium">
                Created
              </label>
              <p className="text-xs text-gray-400 mt-0.5">{formattedDate}</p>
            </div>
            <div className="text-right">
              <label className="text-[11px] uppercase tracking-wider text-gray-500 font-medium">
                Protocol
              </label>
              <p className="text-xs font-semibold nova-gradient-text mt-0.5">NOVA v1</p>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
