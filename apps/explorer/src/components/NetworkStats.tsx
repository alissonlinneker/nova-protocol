import { useMemo } from "react";
import { getMockNetworkStats } from "../hooks/useExplorer";

export default function NetworkStats() {
  const stats = useMemo(() => getMockNetworkStats(), []);

  const statCards = [
    {
      label: "Block Height",
      value: `#${stats.blockHeight.toLocaleString()}`,
      icon: (
        <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.8}>
          <path strokeLinecap="round" strokeLinejoin="round" d="M6.429 9.75L2.25 12l4.179 2.25m0-4.5l5.571 3 5.571-3m-11.142 0L2.25 7.5 12 2.25l9.75 5.25-4.179 2.25m0 0L21.75 12l-4.179 2.25m0 0l4.179 2.25L12 21.75 2.25 16.5l4.179-2.25m11.142 0l-5.571 3-5.571-3" />
        </svg>
      ),
      color: "text-nova-400",
      bgColor: "bg-nova-500/10",
    },
    {
      label: "Transactions/sec",
      value: stats.tps.toLocaleString(),
      icon: (
        <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.8}>
          <path strokeLinecap="round" strokeLinejoin="round" d="M3.75 13.5l10.5-11.25L12 10.5h8.25L9.75 21.75 12 13.5H3.75z" />
        </svg>
      ),
      color: "text-accent-400",
      bgColor: "bg-accent-500/10",
    },
    {
      label: "Avg Block Time",
      value: `${stats.avgBlockTime}s`,
      icon: (
        <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.8}>
          <path strokeLinecap="round" strokeLinejoin="round" d="M12 6v6h4.5m4.5 0a9 9 0 11-18 0 9 9 0 0118 0z" />
        </svg>
      ),
      color: "text-emerald-400",
      bgColor: "bg-emerald-500/10",
    },
    {
      label: "Active Validators",
      value: stats.activeValidators.toString(),
      icon: (
        <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.8}>
          <path strokeLinecap="round" strokeLinejoin="round" d="M9 12.75L11.25 15 15 9.75m-3-7.036A11.959 11.959 0 013.598 6 11.99 11.99 0 003 9.749c0 5.592 3.824 10.29 9 11.623 5.176-1.332 9-6.03 9-11.622 0-1.31-.21-2.571-.598-3.751h-.152c-3.196 0-6.1-1.248-8.25-3.285z" />
        </svg>
      ),
      color: "text-amber-400",
      bgColor: "bg-amber-500/10",
    },
  ];

  const tokenMetrics = [
    {
      label: "Total Supply",
      value: `${(stats.totalSupply / 1_000_000).toFixed(0)}M NOVA`,
    },
    {
      label: "Circulating Supply",
      value: `${(stats.circulatingSupply / 1_000_000).toFixed(0)}M NOVA`,
    },
    {
      label: "Total Staked",
      value: `${(stats.totalStaked / 1_000_000).toFixed(0)}M NOVA`,
    },
    {
      label: "Staking Ratio",
      value: `${((stats.totalStaked / stats.circulatingSupply) * 100).toFixed(1)}%`,
    },
    {
      label: "Market Cap",
      value: `$${(stats.marketCap / 1_000_000_000).toFixed(2)}B`,
    },
    {
      label: "NOVA Price",
      value: `$${stats.price.toFixed(2)}`,
    },
    {
      label: "Total Transactions",
      value: `${(stats.totalTransactions / 1_000_000).toFixed(1)}M`,
    },
  ];

  // Mock validator data
  const validators = [
    { name: "Alpha Prime", stake: 38_000_000, uptime: 99.98, blocks: 284_102 },
    { name: "Beta Cluster", stake: 31_500_000, uptime: 99.95, blocks: 241_847 },
    { name: "Gamma Sentinel", stake: 28_200_000, uptime: 99.91, blocks: 219_384 },
    { name: "Delta Tower", stake: 25_800_000, uptime: 99.89, blocks: 198_472 },
    { name: "Epsilon Core", stake: 22_100_000, uptime: 99.87, blocks: 172_039 },
    { name: "Zeta Node", stake: 19_400_000, uptime: 99.82, blocks: 154_281 },
  ];

  const totalValidatorStake = validators.reduce((acc, v) => acc + v.stake, 0);

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-bold text-white">Network Statistics</h1>

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

      {/* Token Metrics */}
      <div className="nova-card">
        <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
          Token Metrics
        </h3>
        <div className="grid grid-cols-2 lg:grid-cols-4 gap-4">
          {tokenMetrics.map((metric) => (
            <div key={metric.label} className="space-y-1">
              <p className="text-xs text-gray-500">{metric.label}</p>
              <p className="text-sm font-semibold text-white">{metric.value}</p>
            </div>
          ))}
        </div>
      </div>

      {/* Staking Distribution */}
      <div className="nova-card">
        <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-2">
          Supply Distribution
        </h3>
        <p className="text-xs text-gray-600 mb-4">
          How NOVA tokens are distributed across the network
        </p>

        <div className="flex h-4 rounded-full overflow-hidden gap-0.5 mb-4">
          <div
            className="bg-nova-500 rounded-l-full"
            style={{
              width: `${(stats.totalStaked / stats.totalSupply) * 100}%`,
            }}
            title="Staked"
          />
          <div
            className="bg-accent-500"
            style={{
              width: `${
                ((stats.circulatingSupply - stats.totalStaked) /
                  stats.totalSupply) *
                100
              }%`,
            }}
            title="Circulating"
          />
          <div
            className="bg-gray-700 rounded-r-full"
            style={{
              width: `${
                ((stats.totalSupply - stats.circulatingSupply) /
                  stats.totalSupply) *
                100
              }%`,
            }}
            title="Locked"
          />
        </div>

        <div className="flex gap-6 text-xs">
          <div className="flex items-center gap-2">
            <div className="w-3 h-3 rounded-full bg-nova-500" />
            <span className="text-gray-400">
              Staked ({((stats.totalStaked / stats.totalSupply) * 100).toFixed(1)}%)
            </span>
          </div>
          <div className="flex items-center gap-2">
            <div className="w-3 h-3 rounded-full bg-accent-500" />
            <span className="text-gray-400">
              Circulating ({(((stats.circulatingSupply - stats.totalStaked) / stats.totalSupply) * 100).toFixed(1)}%)
            </span>
          </div>
          <div className="flex items-center gap-2">
            <div className="w-3 h-3 rounded-full bg-gray-700" />
            <span className="text-gray-400">
              Locked ({(((stats.totalSupply - stats.circulatingSupply) / stats.totalSupply) * 100).toFixed(1)}%)
            </span>
          </div>
        </div>
      </div>

      {/* Validators */}
      <div className="nova-card">
        <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
          Top Validators
        </h3>

        {/* Desktop Table */}
        <div className="hidden md:block overflow-x-auto">
          <table className="w-full">
            <thead>
              <tr className="border-b border-gray-800">
                <th className="text-left text-xs font-medium text-gray-500 uppercase tracking-wider pb-3">
                  Validator
                </th>
                <th className="text-right text-xs font-medium text-gray-500 uppercase tracking-wider pb-3">
                  Stake
                </th>
                <th className="text-right text-xs font-medium text-gray-500 uppercase tracking-wider pb-3">
                  Share
                </th>
                <th className="text-right text-xs font-medium text-gray-500 uppercase tracking-wider pb-3">
                  Uptime
                </th>
                <th className="text-right text-xs font-medium text-gray-500 uppercase tracking-wider pb-3">
                  Blocks Produced
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-800/50">
              {validators.map((v, i) => (
                <tr key={v.name} className="hover:bg-gray-800/30 transition-colors">
                  <td className="py-3">
                    <div className="flex items-center gap-3">
                      <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-nova-500 to-accent-500 flex items-center justify-center text-xs font-bold text-white">
                        {i + 1}
                      </div>
                      <span className="text-sm font-medium text-white">
                        {v.name}
                      </span>
                    </div>
                  </td>
                  <td className="py-3 text-right text-sm text-gray-300">
                    {(v.stake / 1_000_000).toFixed(1)}M NOVA
                  </td>
                  <td className="py-3 text-right text-sm text-gray-400">
                    {((v.stake / totalValidatorStake) * 100).toFixed(1)}%
                  </td>
                  <td className="py-3 text-right">
                    <span
                      className={`text-sm font-medium ${
                        v.uptime >= 99.9 ? "text-emerald-400" : "text-amber-400"
                      }`}
                    >
                      {v.uptime}%
                    </span>
                  </td>
                  <td className="py-3 text-right text-sm text-gray-300">
                    {v.blocks.toLocaleString()}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        {/* Mobile Cards */}
        <div className="md:hidden space-y-2">
          {validators.map((v, i) => (
            <div key={v.name} className="bg-gray-800/30 rounded-xl p-4">
              <div className="flex items-center justify-between mb-2">
                <div className="flex items-center gap-2">
                  <div className="w-7 h-7 rounded-lg bg-gradient-to-br from-nova-500 to-accent-500 flex items-center justify-center text-xs font-bold text-white">
                    {i + 1}
                  </div>
                  <span className="text-sm font-medium text-white">{v.name}</span>
                </div>
                <span className="text-sm font-medium text-emerald-400">
                  {v.uptime}%
                </span>
              </div>
              <div className="flex items-center justify-between text-xs text-gray-500">
                <span>{(v.stake / 1_000_000).toFixed(1)}M NOVA</span>
                <span>{v.blocks.toLocaleString()} blocks</span>
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
