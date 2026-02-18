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

function formatTimestamp(ts: number): string {
  return new Date(ts).toLocaleString("en-US", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export default function TransactionFeed() {
  const { transactions } = useMerchantStore();
  const runningTotal = transactions
    .filter((tx) => tx.status === "confirmed")
    .reduce((sum, tx) => sum + tx.amount, 0);

  return (
    <div className="space-y-4">
      {/* Running Total */}
      <div className="nova-card flex items-center justify-between">
        <div>
          <p className="text-xs text-gray-500 uppercase tracking-wider">
            Total Received
          </p>
          <p className="text-2xl font-bold text-white mt-1">
            {runningTotal.toLocaleString("en-US", { minimumFractionDigits: 2 })}
            <span className="text-sm text-gray-500 ml-1">NOVA</span>
          </p>
        </div>
        <div className="text-right">
          <p className="text-xs text-gray-500 uppercase tracking-wider">
            Transactions
          </p>
          <p className="text-2xl font-bold text-white mt-1">
            {transactions.length}
          </p>
        </div>
      </div>

      {/* Transaction List */}
      <div className="nova-card">
        <div className="flex items-center justify-between mb-5">
          <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider">
            Transaction History
          </h3>
          {transactions.length > 0 && (
            <div className="flex items-center gap-2">
              <span className="w-2 h-2 rounded-full bg-emerald-400 animate-pulse" />
              <span className="text-xs text-emerald-400 font-medium">Live</span>
            </div>
          )}
        </div>

        <div className="space-y-2">
          {transactions.length === 0 ? (
            <div className="text-center py-8">
              <svg className="w-10 h-10 text-gray-700 mx-auto mb-2" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M2.25 18.75a60.07 60.07 0 0115.797 2.101c.727.198 1.453-.342 1.453-1.096V18.75M3.75 4.5v.75A.75.75 0 013 6h-.75m0 0v-.375c0-.621.504-1.125 1.125-1.125H20.25M2.25 6v9m18-10.5v.75c0 .414.336.75.75.75h.75m-1.5-1.5h.375c.621 0 1.125.504 1.125 1.125v9.75c0 .621-.504 1.125-1.125 1.125h-.375m1.5-1.5H21a.75.75 0 00-.75.75v.75m0 0H3.75m0 0h-.375a1.125 1.125 0 01-1.125-1.125V15m1.5 1.5v-.75A.75.75 0 003 15h-.75M15 10.5a3 3 0 11-6 0 3 3 0 016 0zm3 0h.008v.008H18V10.5zm-12 0h.008v.008H6V10.5z" />
              </svg>
              <p className="text-sm text-gray-600">No transactions yet</p>
              <p className="text-xs text-gray-700 mt-1">
                Payments will appear here as they are received
              </p>
            </div>
          ) : (
            transactions.map((tx, index) => (
              <div
                key={tx.id}
                className={`flex items-center justify-between py-3 px-3 rounded-xl transition-all ${
                  index === 0
                    ? "bg-emerald-500/5 border border-emerald-500/20"
                    : "hover:bg-gray-800/50"
                }`}
              >
                <div className="flex items-center gap-3">
                  <div className="w-9 h-9 rounded-full bg-emerald-500/20 flex items-center justify-center shrink-0">
                    <svg className="w-4 h-4 text-emerald-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                      <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 13.5L12 21m0 0l-7.5-7.5M12 21V3" />
                    </svg>
                  </div>
                  <div>
                    <div className="flex items-center gap-2">
                      <p className="text-sm font-medium text-white">
                        +{tx.amount.toLocaleString("en-US", { minimumFractionDigits: 2 })} NOVA
                      </p>
                      <span
                        className={`nova-badge text-[10px] ${
                          tx.status === "confirmed"
                            ? "bg-emerald-500/10 text-emerald-400"
                            : "bg-amber-500/10 text-amber-400"
                        }`}
                      >
                        {tx.status}
                      </span>
                    </div>
                    <p className="text-xs text-gray-500">
                      {tx.sender.length > 20
                        ? `${tx.sender.slice(0, 10)}...${tx.sender.slice(-6)}`
                        : tx.sender}
                      {tx.memo && (
                        <span className="text-gray-600"> &middot; {tx.memo}</span>
                      )}
                    </p>
                    {tx.hash && (
                      <p className="text-[10px] text-gray-600 font-mono mt-0.5">
                        {tx.hash.slice(0, 16)}...
                      </p>
                    )}
                  </div>
                </div>
                <div className="text-right shrink-0">
                  <span className="text-xs text-gray-500 block">
                    {timeAgo(tx.timestamp)}
                  </span>
                  <span className="text-[10px] text-gray-600 block mt-0.5">
                    {formatTimestamp(tx.timestamp)}
                  </span>
                </div>
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  );
}
