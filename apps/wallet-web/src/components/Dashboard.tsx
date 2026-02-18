import { Link } from 'react-router-dom';
import { useWallet } from '../hooks/useWallet';
import { useNova } from '../hooks/useNova';
import IdentityCard from './IdentityCard';

function formatNova(value: number): string {
  return new Intl.NumberFormat('en-US', {
    minimumFractionDigits: 2,
    maximumFractionDigits: 8,
  }).format(value);
}

function formatNumber(value: number): string {
  return new Intl.NumberFormat('en-US', {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(value);
}

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
  const { balances, recentTransactions } = useWallet();
  const { isNodeConnected, blockHeight, isLoadingBalance } = useNova();

  const totalBalance = balances.reduce((acc, b) => acc + b.balance, 0);

  return (
    <div className="space-y-6">
      {/* Identity Card */}
      <IdentityCard />

      {/* Network Status */}
      <div className="nova-card flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span
            className={`w-2 h-2 rounded-full ${
              isNodeConnected ? 'bg-emerald-400 animate-pulse' : 'bg-red-400'
            }`}
          />
          <span className={`text-xs font-medium ${isNodeConnected ? 'text-emerald-400' : 'text-red-400'}`}>
            {isNodeConnected ? 'Connected' : 'Disconnected'}
          </span>
        </div>
        {blockHeight !== null && (
          <span className="text-xs text-gray-500 font-mono">
            Block #{blockHeight.toLocaleString()}
          </span>
        )}
      </div>

      {/* Total Balance */}
      <div className="nova-card text-center">
        <p className="text-sm text-gray-400 mb-1">Total Balance</p>
        {isLoadingBalance ? (
          <div className="flex items-center justify-center py-2">
            <div className="w-6 h-6 border-2 border-nova-500 border-t-transparent rounded-full animate-spin" />
          </div>
        ) : (
          <h2 className="text-4xl font-bold text-white tracking-tight">
            {formatNova(totalBalance)} <span className="text-lg text-gray-400">NOVA</span>
          </h2>
        )}
        {!isNodeConnected && balances.length === 0 && (
          <p className="text-xs text-gray-500 mt-2">
            Connect to a node to fetch your balance
          </p>
        )}
      </div>

      {/* Quick Actions */}
      <div className="grid grid-cols-3 gap-3">
        <Link
          to="/send"
          className="nova-card flex flex-col items-center gap-2 hover:border-nova-500/50 transition-colors cursor-pointer text-center py-5"
        >
          <div className="w-12 h-12 rounded-full bg-nova-500/20 flex items-center justify-center">
            <svg className="w-5 h-5 text-nova-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M6 12L3.269 3.126A59.768 59.768 0 0121.485 12 59.77 59.77 0 013.27 20.876L5.999 12zm0 0h7.5" />
            </svg>
          </div>
          <span className="text-sm font-medium text-gray-300">Send</span>
        </Link>

        <Link
          to="/receive"
          className="nova-card flex flex-col items-center gap-2 hover:border-accent-500/50 transition-colors cursor-pointer text-center py-5"
        >
          <div className="w-12 h-12 rounded-full bg-accent-500/20 flex items-center justify-center">
            <svg className="w-5 h-5 text-accent-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M3 16.5v2.25A2.25 2.25 0 005.25 21h13.5A2.25 2.25 0 0021 18.75V16.5M16.5 12L12 16.5m0 0L7.5 12m4.5 4.5V3" />
            </svg>
          </div>
          <span className="text-sm font-medium text-gray-300">Receive</span>
        </Link>

        <Link
          to="/credit"
          className="nova-card flex flex-col items-center gap-2 hover:border-emerald-500/50 transition-colors cursor-pointer text-center py-5"
        >
          <div className="w-12 h-12 rounded-full bg-emerald-500/20 flex items-center justify-center">
            <svg className="w-5 h-5 text-emerald-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M2.25 18.75a60.07 60.07 0 0115.797 2.101c.727.198 1.453-.342 1.453-1.096V18.75M3.75 4.5v.75A.75.75 0 013 6h-.75m0 0v-.375c0-.621.504-1.125 1.125-1.125H20.25M2.25 6v9m18-10.5v.75c0 .414.336.75.75.75h.75m-1.5-1.5h.375c.621 0 1.125.504 1.125 1.125v9.75c0 .621-.504 1.125-1.125 1.125h-.375m1.5-1.5H21a.75.75 0 00-.75.75v.75m0 0H3.75m0 0h-.375a1.125 1.125 0 01-1.125-1.125V15m1.5 1.5v-.75A.75.75 0 003 15h-.75M15 10.5a3 3 0 11-6 0 3 3 0 016 0zm3 0h.008v.008H18V10.5zm-12 0h.008v.008H6V10.5z" />
            </svg>
          </div>
          <span className="text-sm font-medium text-gray-300">Credit</span>
        </Link>
      </div>

      {/* Token Balances */}
      {balances.length > 0 && (
        <div className="nova-card">
          <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
            Assets
          </h3>
          <div className="space-y-3">
            {balances.map((token) => (
              <div
                key={token.symbol}
                className="flex items-center justify-between py-3 px-3 rounded-xl hover:bg-gray-800/50 transition-colors"
              >
                <div className="flex items-center gap-3">
                  <div className="w-10 h-10 rounded-full bg-gradient-to-br from-nova-500 to-accent-500 flex items-center justify-center text-xs font-bold text-white">
                    {token.symbol.slice(0, 2)}
                  </div>
                  <div>
                    <p className="text-sm font-medium text-white">{token.name}</p>
                    <p className="text-xs text-gray-500">
                      {formatNumber(token.balance)} {token.symbol}
                    </p>
                  </div>
                </div>
                <div className="text-right">
                  <p className="text-sm font-medium text-white">
                    {formatNumber(token.balance)} {token.symbol}
                  </p>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Recent Transactions */}
      <div className="nova-card">
        <div className="flex items-center justify-between mb-4">
          <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider">
            Recent Transactions
          </h3>
          <Link
            to="/history"
            className="text-xs text-nova-400 hover:text-nova-300 font-medium transition-colors"
          >
            View All
          </Link>
        </div>
        {recentTransactions.length === 0 ? (
          <div className="text-center py-8">
            <svg className="w-10 h-10 text-gray-700 mx-auto mb-2" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 14.25v-2.625a3.375 3.375 0 00-3.375-3.375h-1.5A1.125 1.125 0 0113.5 7.125v-1.5a3.375 3.375 0 00-3.375-3.375H8.25m0 12.75h7.5m-7.5 3H12M10.5 2.25H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 00-9-9z" />
            </svg>
            <p className="text-sm text-gray-500">No transactions yet</p>
            <p className="text-xs text-gray-600 mt-1">
              Send or receive NOVA to see your history
            </p>
          </div>
        ) : (
          <div className="space-y-2">
            {recentTransactions.slice(0, 5).map((tx) => (
              <div
                key={tx.id}
                className="flex items-center justify-between py-3 px-3 rounded-xl hover:bg-gray-800/50 transition-colors"
              >
                <div className="flex items-center gap-3">
                  <div
                    className={`w-9 h-9 rounded-full flex items-center justify-center ${
                      tx.type === 'receive' || tx.type === 'credit_issued'
                        ? 'bg-emerald-500/20'
                        : 'bg-red-500/20'
                    }`}
                  >
                    {tx.type === 'receive' || tx.type === 'credit_issued' ? (
                      <svg className="w-4 h-4 text-emerald-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                        <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 13.5L12 21m0 0l-7.5-7.5M12 21V3" />
                      </svg>
                    ) : (
                      <svg className="w-4 h-4 text-red-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                        <path strokeLinecap="round" strokeLinejoin="round" d="M4.5 10.5L12 3m0 0l7.5 7.5M12 3v18" />
                      </svg>
                    )}
                  </div>
                  <div>
                    <p className="text-sm font-medium text-white capitalize">
                      {tx.type === 'credit_issued'
                        ? 'Credit Issued'
                        : tx.type === 'credit_repay'
                        ? 'Credit Repay'
                        : tx.type}
                    </p>
                    <p className="text-xs text-gray-500">{timeAgo(tx.timestamp)}</p>
                  </div>
                </div>
                <div className="text-right">
                  <p
                    className={`text-sm font-medium ${
                      tx.type === 'receive' || tx.type === 'credit_issued'
                        ? 'text-emerald-400'
                        : 'text-white'
                    }`}
                  >
                    {tx.type === 'receive' || tx.type === 'credit_issued' ? '+' : '-'}
                    {formatNumber(tx.amount)} {tx.symbol}
                  </p>
                  <span
                    className={`nova-badge text-[10px] ${
                      tx.status === 'confirmed'
                        ? 'bg-emerald-500/10 text-emerald-400'
                        : tx.status === 'pending'
                        ? 'bg-amber-500/10 text-amber-400'
                        : 'bg-red-500/10 text-red-400'
                    }`}
                  >
                    {tx.status}
                  </span>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
