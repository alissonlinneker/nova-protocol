import { useMerchantStore } from "../hooks/useMerchant";

export default function Analytics() {
  const store = useMerchantStore();

  const todayRev = store.todayRevenue();
  const todayCount = store.todayTxCount();
  const weekRev = store.weekRevenue();
  const weekCount = store.weekTxCount();
  const { dailyStats, balance } = store;

  const maxRevenue = dailyStats.length > 0
    ? Math.max(...dailyStats.map((d) => d.revenue))
    : 0;

  const avgTransaction = weekCount > 0 ? weekRev / weekCount : 0;

  return (
    <div className="space-y-6">
      {/* Summary Cards */}
      <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
        <div className="nova-card">
          <p className="text-xs text-gray-500 uppercase tracking-wider mb-1">
            Today
          </p>
          <p className="text-2xl font-bold text-white">
            {todayRev.toLocaleString("en-US", { minimumFractionDigits: 2 })}
          </p>
          <p className="text-xs text-gray-500 mt-1">
            {todayCount} transaction{todayCount !== 1 ? "s" : ""}
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
            {weekCount} transaction{weekCount !== 1 ? "s" : ""}
          </p>
        </div>

        <div className="nova-card">
          <p className="text-xs text-gray-500 uppercase tracking-wider mb-1">
            Avg. Transaction
          </p>
          <p className="text-2xl font-bold text-white">
            {avgTransaction.toLocaleString("en-US", {
              minimumFractionDigits: 2,
              maximumFractionDigits: 2,
            })}
          </p>
          <p className="text-xs text-gray-500 mt-1">NOVA per transaction</p>
        </div>

        <div className="nova-card">
          <p className="text-xs text-gray-500 uppercase tracking-wider mb-1">
            Balance
          </p>
          <p className="text-2xl font-bold text-white">
            {balance.toLocaleString("en-US", { minimumFractionDigits: 0 })}
          </p>
          <p className="text-xs text-gray-500 mt-1">photons</p>
        </div>
      </div>

      {/* Weekly Revenue Chart */}
      {dailyStats.length > 0 && (
        <div className="nova-card">
          <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-6">
            Weekly Revenue
          </h3>

          <div className="flex items-end gap-2 h-48">
            {dailyStats.map((day) => {
              const heightPercent = maxRevenue > 0 ? (day.revenue / maxRevenue) * 100 : 0;

              return (
                <div key={day.date} className="flex-1 flex flex-col items-center gap-2">
                  <span className="text-[10px] text-gray-500 font-medium">
                    {day.revenue > 1_000
                      ? `${(day.revenue / 1_000).toFixed(1)}k`
                      : day.revenue.toFixed(0)}
                  </span>
                  <div className="w-full flex items-end" style={{ height: "160px" }}>
                    <div
                      className="w-full rounded-t-lg bg-gradient-to-t from-nova-600 to-nova-400 transition-all duration-500 hover:from-nova-500 hover:to-accent-400"
                      style={{ height: `${heightPercent}%`, minHeight: "4px" }}
                    />
                  </div>
                  <span className="text-xs text-gray-500 font-medium">
                    {day.date}
                  </span>
                </div>
              );
            })}
          </div>
        </div>
      )}

      {/* Empty state */}
      {dailyStats.length === 0 && (
        <div className="nova-card text-center py-12">
          <svg className="w-12 h-12 text-gray-700 mx-auto mb-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M3 13.125C3 12.504 3.504 12 4.125 12h2.25c.621 0 1.125.504 1.125 1.125v6.75C7.5 20.496 6.996 21 6.375 21h-2.25A1.125 1.125 0 013 19.875v-6.75zM9.75 8.625c0-.621.504-1.125 1.125-1.125h2.25c.621 0 1.125.504 1.125 1.125v11.25c0 .621-.504 1.125-1.125 1.125h-2.25a1.125 1.125 0 01-1.125-1.125V8.625zM16.5 4.125c0-.621.504-1.125 1.125-1.125h2.25C20.496 3 21 3.504 21 4.125v15.75c0 .621-.504 1.125-1.125 1.125h-2.25a1.125 1.125 0 01-1.125-1.125V4.125z" />
          </svg>
          <p className="text-sm text-gray-500">No analytics data yet</p>
          <p className="text-xs text-gray-600 mt-1">
            Analytics will populate as payments are received
          </p>
        </div>
      )}
    </div>
  );
}
