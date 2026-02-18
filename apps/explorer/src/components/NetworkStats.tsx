import { useState, useEffect } from "react";
import { useExplorerStore } from "../store/explorerStore";
import { fetchValidators, type ValidatorInfo } from "../services/api";

export default function NetworkStats() {
  const { status, connected } = useExplorerStore();
  const [validators, setValidators] = useState<ValidatorInfo[]>([]);
  const [validatorsLoading, setValidatorsLoading] = useState(true);

  useEffect(() => {
    fetchValidators()
      .then((data) => {
        setValidators(data);
        setValidatorsLoading(false);
      })
      .catch(() => {
        setValidatorsLoading(false);
      });
  }, []);

  const statCards = [
    {
      label: "Block Height",
      value: status ? `#${status.block_height.toLocaleString()}` : "...",
      icon: (
        <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.8}>
          <path strokeLinecap="round" strokeLinejoin="round" d="M6.429 9.75L2.25 12l4.179 2.25m0-4.5l5.571 3 5.571-3m-11.142 0L2.25 7.5 12 2.25l9.75 5.25-4.179 2.25m0 0L21.75 12l-4.179 2.25m0 0l4.179 2.25L12 21.75 2.25 16.5l4.179-2.25m11.142 0l-5.571 3-5.571-3" />
        </svg>
      ),
      color: "text-nova-400",
      bgColor: "bg-nova-500/10",
    },
    {
      label: "Peer Count",
      value: status ? status.peer_count.toLocaleString() : "...",
      icon: (
        <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.8}>
          <path strokeLinecap="round" strokeLinejoin="round" d="M18 18.72a9.094 9.094 0 003.741-.479 3 3 0 00-4.682-2.72m.94 3.198l.001.031c0 .225-.012.447-.037.666A11.944 11.944 0 0112 21c-2.17 0-4.207-.576-5.963-1.584A6.062 6.062 0 016 18.719m12 0a5.971 5.971 0 00-.941-3.197m0 0A5.995 5.995 0 0012 12.75a5.995 5.995 0 00-5.058 2.772m0 0a3 3 0 00-4.681 2.72 8.986 8.986 0 003.74.477m.94-3.197a5.971 5.971 0 00-.94 3.197M15 6.75a3 3 0 11-6 0 3 3 0 016 0zm6 3a2.25 2.25 0 11-4.5 0 2.25 2.25 0 014.5 0zm-13.5 0a2.25 2.25 0 11-4.5 0 2.25 2.25 0 014.5 0z" />
        </svg>
      ),
      color: "text-accent-400",
      bgColor: "bg-accent-500/10",
    },
    {
      label: "Sync Status",
      value: status ? (status.synced ? "Synced" : "Syncing") : "...",
      icon: (
        <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.8}>
          <path strokeLinecap="round" strokeLinejoin="round" d="M9 12.75L11.25 15 15 9.75m-3-7.036A11.959 11.959 0 013.598 6 11.99 11.99 0 003 9.749c0 5.592 3.824 10.29 9 11.623 5.176-1.332 9-6.03 9-11.622 0-1.31-.21-2.571-.598-3.751h-.152c-3.196 0-6.1-1.248-8.25-3.285z" />
        </svg>
      ),
      color: status?.synced ? "text-emerald-400" : "text-amber-400",
      bgColor: status?.synced ? "bg-emerald-500/10" : "bg-amber-500/10",
    },
    {
      label: "Node Version",
      value: status?.version ?? "...",
      icon: (
        <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.8}>
          <path strokeLinecap="round" strokeLinejoin="round" d="M11.25 11.25l.041-.02a.75.75 0 011.063.852l-.708 2.836a.75.75 0 001.063.853l.041-.021M21 12a9 9 0 11-18 0 9 9 0 0118 0zm-9-3.75h.008v.008H12V8.25z" />
        </svg>
      ),
      color: "text-gray-300",
      bgColor: "bg-gray-700/30",
    },
  ];

  const networkDetails = status
    ? [
        { label: "Network", value: status.network },
        { label: "Version", value: status.version },
        {
          label: "Block Height",
          value: `#${status.block_height.toLocaleString()}`,
        },
        { label: "Peer Count", value: status.peer_count.toLocaleString() },
        { label: "Synced", value: status.synced ? "Yes" : "No" },
        { label: "Last Updated", value: status.timestamp },
      ]
    : [];

  const activeValidators = validators.filter((v) => v.active);
  const totalStake = activeValidators.reduce((acc, v) => acc + v.stake, 0);

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-bold text-white">Network Statistics</h1>
        <div className="flex items-center gap-2">
          <span
            className={`w-2 h-2 rounded-full ${
              connected ? "bg-emerald-400 animate-pulse" : "bg-red-400"
            }`}
          />
          <span
            className={`text-xs font-medium ${
              connected ? "text-emerald-400" : "text-red-400"
            }`}
          >
            {connected ? "Connected" : "Disconnected"}
          </span>
        </div>
      </div>

      {/* Primary Stats */}
      <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
        {statCards.map((stat) => (
          <div key={stat.label} className="nova-card">
            <div className="flex items-center gap-3 mb-3">
              <div
                className={`w-10 h-10 rounded-xl ${stat.bgColor} flex items-center justify-center ${stat.color}`}
              >
                {stat.icon}
              </div>
            </div>
            <p className="text-2xl font-bold text-white">{stat.value}</p>
            <p className="text-xs text-gray-500 mt-1">{stat.label}</p>
          </div>
        ))}
      </div>

      {/* Network Details */}
      {networkDetails.length > 0 && (
        <div className="nova-card">
          <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
            Node Status
          </h3>
          <div className="grid grid-cols-2 lg:grid-cols-3 gap-4">
            {networkDetails.map((detail) => (
              <div key={detail.label} className="space-y-1">
                <p className="text-xs text-gray-500">{detail.label}</p>
                <p className="text-sm font-semibold text-white break-all">
                  {detail.value}
                </p>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Validators */}
      <div className="nova-card">
        <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
          Validators ({activeValidators.length} active)
        </h3>

        {validatorsLoading ? (
          <div className="py-8 text-center">
            <div className="inline-block w-5 h-5 border-2 border-nova-500 border-t-transparent rounded-full animate-spin mb-2" />
            <p className="text-xs text-gray-500">Loading validators...</p>
          </div>
        ) : validators.length === 0 ? (
          <p className="text-xs text-gray-500">No validator data available.</p>
        ) : (
          <>
            {/* Desktop Table */}
            <div className="hidden md:block overflow-x-auto">
              <table className="w-full">
                <thead>
                  <tr className="border-b border-gray-800">
                    <th className="text-left text-xs font-medium text-gray-500 uppercase tracking-wider pb-3">
                      Public Key
                    </th>
                    <th className="text-right text-xs font-medium text-gray-500 uppercase tracking-wider pb-3">
                      Stake
                    </th>
                    <th className="text-right text-xs font-medium text-gray-500 uppercase tracking-wider pb-3">
                      Share
                    </th>
                    <th className="text-right text-xs font-medium text-gray-500 uppercase tracking-wider pb-3">
                      Status
                    </th>
                    <th className="text-right text-xs font-medium text-gray-500 uppercase tracking-wider pb-3">
                      Last Block
                    </th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-gray-800/50">
                  {validators.map((v, i) => (
                    <tr key={v.public_key + i} className="hover:bg-gray-800/30 transition-colors">
                      <td className="py-3">
                        <div className="flex items-center gap-3">
                          <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-nova-500 to-accent-500 flex items-center justify-center text-xs font-bold text-white">
                            {i + 1}
                          </div>
                          <code className="text-xs font-mono text-gray-300">
                            {v.public_key.length > 16
                              ? `${v.public_key.slice(0, 8)}...${v.public_key.slice(-8)}`
                              : v.public_key}
                          </code>
                        </div>
                      </td>
                      <td className="py-3 text-right text-sm text-gray-300">
                        {v.stake.toLocaleString()}
                      </td>
                      <td className="py-3 text-right text-sm text-gray-400">
                        {totalStake > 0
                          ? `${((v.stake / totalStake) * 100).toFixed(1)}%`
                          : "N/A"}
                      </td>
                      <td className="py-3 text-right">
                        <span
                          className={`nova-badge ${
                            v.active
                              ? "bg-emerald-500/10 text-emerald-400"
                              : "bg-gray-700/30 text-gray-500"
                          }`}
                        >
                          {v.active ? "Active" : "Inactive"}
                        </span>
                      </td>
                      <td className="py-3 text-right text-sm text-gray-300">
                        #{v.last_proposed_block.toLocaleString()}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>

            {/* Mobile Cards */}
            <div className="md:hidden space-y-2">
              {validators.map((v, i) => (
                <div key={v.public_key + i} className="bg-gray-800/30 rounded-xl p-4">
                  <div className="flex items-center justify-between mb-2">
                    <div className="flex items-center gap-2">
                      <div className="w-7 h-7 rounded-lg bg-gradient-to-br from-nova-500 to-accent-500 flex items-center justify-center text-xs font-bold text-white">
                        {i + 1}
                      </div>
                      <code className="text-xs font-mono text-gray-300">
                        {v.public_key.length > 12
                          ? `${v.public_key.slice(0, 6)}...${v.public_key.slice(-6)}`
                          : v.public_key}
                      </code>
                    </div>
                    <span
                      className={`nova-badge text-[10px] ${
                        v.active
                          ? "bg-emerald-500/10 text-emerald-400"
                          : "bg-gray-700/30 text-gray-500"
                      }`}
                    >
                      {v.active ? "Active" : "Inactive"}
                    </span>
                  </div>
                  <div className="flex items-center justify-between text-xs text-gray-500">
                    <span>Stake: {v.stake.toLocaleString()}</span>
                    <span>Last: #{v.last_proposed_block.toLocaleString()}</span>
                  </div>
                </div>
              ))}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
