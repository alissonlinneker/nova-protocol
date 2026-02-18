import { useMerchantStore } from "../hooks/useMerchant";

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

export default function Dashboard() {
  const store = useMerchantStore();
  const {
    balance,
    nodeStatus,
    nodeConnected,
    wsConnected,
    transactions,
    lastBalanceCheck,
    fetchStatus,
    fetchBalance,
  } = store;

  const todayRev = store.todayRevenue();
  const todayCount = store.todayTxCount();
  const weekRev = store.weekRevenue();
  const weekCount = store.weekTxCount();

  const recentTxs = transactions.slice(0, 5);

  const handleRefresh = () => {
    fetchStatus();
    fetchBalance();
  };

  return (
    <div className="space-y-6">
      {/* Top Stats Row */}
      <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
        <div className="nova-card">
          <p className="text-xs text-gray-500 uppercase tracking-wider mb-1">
            Balance
          </p>
          <p className="text-2xl font-bold text-white">
            {balance.toLocaleString("en-US")}
          </p>
          <p className="text-xs text-gray-500 mt-1">photons</p>
        </div>

        <div className="nova-card">
          <p className="text-xs text-gray-500 uppercase tracking-wider mb-1">
            Today&apos;s Sales
          </p>
          <p className="text-2xl font-bold text-white">
            {todayRev.toLocaleString("en-US", { minimumFractionDigits: 2 })}
          </p>
          <p className="text-xs text-gray-500 mt-1">
            {todayCount} payment{todayCount !== 1 ? "s" : ""}
          </p>
        </div>

        <div className="nova-card">
          <p className="text-xs text-gray-500 uppercase tracking-wider mb-1">
            This Week
          </p>
          <p className="text-2xl font-bold text-white">
            {weekRev.toLocaleString("en-US", { minimumFractionDigits: 2 })}
          </p>
          <p className="text-xs text-gray-500 mt-1">
            {weekCount} payment{weekCount !== 1 ? "s" : ""}
          </p>
        </div>

        <div className="nova-card">
          <p className="text-xs text-gray-500 uppercase tracking-wider mb-1">
            Block Height
          </p>
          <p className="text-2xl font-bold text-white">
            {nodeStatus ? nodeStatus.block_height.toLocaleString("en-US") : "--"}
          </p>
          <p className="text-xs text-gray-500 mt-1">
            {nodeStatus?.network ?? "unknown"}
          </p>
        </div>
      </div>

      {/* Network Status */}
      <div className="nova-card">
        <div className="flex items-center justify-between mb-4">
          <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider">
            Network Status
          </h3>
          <button
            onClick={handleRefresh}
            className="text-xs text-gray-500 hover:text-gray-300 transition-colors"
          >
            Refresh
          </button>
        </div>

        <div className="grid grid-cols-2 sm:grid-cols-4 gap-4">
          {/* Connection */}
          <div className="flex items-center gap-2">
            <span
              className={`w-2.5 h-2.5 rounded-full ${
                nodeConnected ? "bg-emerald-400" : "bg-red-400"
              }`}
            />
            <div>
              <p className="text-xs text-gray-500">Node</p>
              <p className="text-sm text-gray-200 font-medium">
                {nodeConnected ? "Connected" : "Disconnected"}
              </p>
            </div>
          </div>

          {/* WebSocket */}
          <div className="flex items-center gap-2">
            <span
              className={`w-2.5 h-2.5 rounded-full ${
                wsConnected ? "bg-emerald-400" : "bg-gray-600"
              }`}
            />
            <div>
              <p className="text-xs text-gray-500">WebSocket</p>
              <p className="text-sm text-gray-200 font-medium">
                {wsConnected ? "Streaming" : "Inactive"}
              </p>
            </div>
          </div>

          {/* Peers */}
          <div className="flex items-center gap-2">
            <span
              className={`w-2.5 h-2.5 rounded-full ${
                nodeStatus && nodeStatus.peer_count > 0 ? "bg-emerald-400" : "bg-amber-400"
              }`}
            />
            <div>
              <p className="text-xs text-gray-500">Peers</p>
              <p className="text-sm text-gray-200 font-medium">
                {nodeStatus ? nodeStatus.peer_count : "--"}
              </p>
            </div>
          </div>

          {/* Sync */}
          <div className="flex items-center gap-2">
            <span
              className={`w-2.5 h-2.5 rounded-full ${
                nodeStatus?.synced ? "bg-emerald-400" : "bg-amber-400"
              }`}
            />
            <div>
              <p className="text-xs text-gray-500">Sync</p>
              <p className="text-sm text-gray-200 font-medium">
                {nodeStatus ? (nodeStatus.synced ? "Synced" : "Syncing") : "--"}
              </p>
            </div>
          </div>
        </div>

        {lastBalanceCheck > 0 && (
          <p className="text-[10px] text-gray-600 mt-4">
            Last updated: {new Date(lastBalanceCheck).toLocaleTimeString()}
          </p>
        )}
      </div>

      {/* Recent Transactions */}
      <div className="nova-card">
        <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
          Recent Payments
        </h3>

        {recentTxs.length === 0 ? (
          <div className="text-center py-6">
            <p className="text-sm text-gray-600">No payments received yet</p>
            <p className="text-xs text-gray-700 mt-1">
              Go to Terminal to start accepting payments
            </p>
          </div>
        ) : (
          <div className="space-y-2">
            {recentTxs.map((tx) => (
              <div
                key={tx.id}
                className="flex items-center justify-between py-2.5 px-3 rounded-lg hover:bg-gray-800/50 transition-all"
              >
                <div className="flex items-center gap-3">
                  <div className="w-8 h-8 rounded-full bg-emerald-500/20 flex items-center justify-center shrink-0">
                    <svg className="w-3.5 h-3.5 text-emerald-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                      <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 13.5L12 21m0 0l-7.5-7.5M12 21V3" />
                    </svg>
                  </div>
                  <div>
                    <p className="text-sm font-medium text-white">
                      +{tx.amount.toLocaleString("en-US", { minimumFractionDigits: 2 })} NOVA
                    </p>
                    <p className="text-xs text-gray-500">
                      {tx.sender.length > 20
                        ? `${tx.sender.slice(0, 10)}...${tx.sender.slice(-4)}`
                        : tx.sender}
                    </p>
                  </div>
                </div>
                <span className="text-xs text-gray-500">{timeAgo(tx.timestamp)}</span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
