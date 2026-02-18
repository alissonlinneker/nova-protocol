import { useMerchantStore } from "../hooks/useMerchant";

export default function Analytics() {
  const { todayRevenue, todayTxCount, weekRevenue, weekTxCount, dailyStats } =
    useMerchantStore();

  const maxRevenue = Math.max(...dailyStats.map((d) => d.revenue));

  const avgTransaction = weekTxCount > 0 ? weekRevenue / weekTxCount : 0;

  return (
    <div className="space-y-6">
      {/* Summary Cards */}
      <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
        <div className="nova-card">
          <p className="text-xs text-gray-500 uppercase tracking-wider mb-1">
            Today
          </p>
          <p className="text-2xl font-bold text-white">
            ${todayRevenue.toLocaleString("en-US", { minimumFractionDigits: 2 })}
          </p>
          <p className="text-xs text-emerald-400 mt-1">
            +12.5% vs yesterday
          </p>
        </div>

        <div className="nova-card">
          <p className="text-xs text-gray-500 uppercase tracking-wider mb-1">
            Today Txns
          </p>
          <p className="text-2xl font-bold text-white">{todayTxCount}</p>
          <p className="text-xs text-emerald-400 mt-1">
            +3 vs yesterday
          </p>
        </div>

        <div className="nova-card">
          <p className="text-xs text-gray-500 uppercase tracking-wider mb-1">
            This Week
          </p>
          <p className="text-2xl font-bold text-white">
            ${weekRevenue.toLocaleString("en-US", { minimumFractionDigits: 2 })}
          </p>
          <p className="text-xs text-emerald-400 mt-1">
            {weekTxCount} transactions
          </p>
        </div>

        <div className="nova-card">
          <p className="text-xs text-gray-500 uppercase tracking-wider mb-1">
            Avg. Transaction
          </p>
          <p className="text-2xl font-bold text-white">
            ${avgTransaction.toLocaleString("en-US", {
              minimumFractionDigits: 2,
              maximumFractionDigits: 2,
            })}
          </p>
          <p className="text-xs text-gray-500 mt-1">per transaction</p>
        </div>
      </div>

      {/* Weekly Revenue Chart */}
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
                  ${(day.revenue / 1_000).toFixed(1)}k
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

      {/* Quick Stats */}
      <div className="nova-card">
        <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
          Payment Methods
        </h3>
        <div className="space-y-3">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              <div className="w-8 h-8 rounded-lg bg-nova-500/20 flex items-center justify-center">
                <span className="text-xs font-bold text-nova-400">N</span>
              </div>
              <span className="text-sm text-gray-300">NOVA</span>
            </div>
            <div className="flex items-center gap-3">
              <div className="w-32 h-2 bg-gray-800 rounded-full overflow-hidden">
                <div className="w-[45%] h-full bg-nova-500 rounded-full" />
              </div>
              <span className="text-xs text-gray-400 w-10 text-right">45%</span>
            </div>
          </div>

          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              <div className="w-8 h-8 rounded-lg bg-accent-500/20 flex items-center justify-center">
                <span className="text-xs font-bold text-accent-400">U</span>
              </div>
              <span className="text-sm text-gray-300">USDN</span>
            </div>
            <div className="flex items-center gap-3">
              <div className="w-32 h-2 bg-gray-800 rounded-full overflow-hidden">
                <div className="w-[55%] h-full bg-accent-500 rounded-full" />
              </div>
              <span className="text-xs text-gray-400 w-10 text-right">55%</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
