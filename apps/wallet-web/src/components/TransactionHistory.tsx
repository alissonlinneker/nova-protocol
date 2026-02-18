import { useState, useMemo } from 'react';
import { Link } from 'react-router-dom';
import { useWallet } from '../hooks/useWallet';
import type { Transaction } from '../stores/walletStore';

type FilterType = 'all' | 'send' | 'receive' | 'credit_issued' | 'credit_repay';
type FilterStatus = 'all' | 'pending' | 'confirmed' | 'failed';

function formatDate(timestamp: number): string {
  return new Date(timestamp).toLocaleDateString('en-US', {
    month: 'short',
    day: 'numeric',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

function truncateHash(hash: string): string {
  if (hash.length <= 18) return hash;
  return `${hash.slice(0, 10)}...${hash.slice(-8)}`;
}

export default function TransactionHistory() {
  const { transactions, address } = useWallet();

  const [typeFilter, setTypeFilter] = useState<FilterType>('all');
  const [statusFilter, setStatusFilter] = useState<FilterStatus>('all');
  const [selectedTx, setSelectedTx] = useState<Transaction | null>(null);

  const filteredTransactions = useMemo(() => {
    return transactions
      .filter((tx) => typeFilter === 'all' || tx.type === typeFilter)
      .filter((tx) => statusFilter === 'all' || tx.status === statusFilter)
      .sort((a, b) => b.timestamp - a.timestamp);
  }, [transactions, typeFilter, statusFilter]);

  const typeLabels: Record<FilterType, string> = {
    all: 'All',
    send: 'Sent',
    receive: 'Received',
    credit_issued: 'Credit',
    credit_repay: 'Repay',
  };

  const statusLabels: Record<FilterStatus, string> = {
    all: 'All Status',
    pending: 'Pending',
    confirmed: 'Confirmed',
    failed: 'Failed',
  };

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
        <h1 className="text-xl font-bold text-white">Transaction History</h1>
      </div>

      {/* Filters */}
      <div className="space-y-3">
        {/* Type Filter */}
        <div className="flex gap-2 overflow-x-auto pb-1 scrollbar-none">
          {(Object.keys(typeLabels) as FilterType[]).map((type) => (
            <button
              key={type}
              onClick={() => setTypeFilter(type)}
              className={`px-3.5 py-1.5 rounded-lg text-xs font-medium whitespace-nowrap transition-colors ${
                typeFilter === type
                  ? 'bg-nova-600 text-white'
                  : 'bg-gray-800 text-gray-400 hover:bg-gray-700'
              }`}
            >
              {typeLabels[type]}
            </button>
          ))}
        </div>

        {/* Status Filter */}
        <div className="flex gap-2">
          {(Object.keys(statusLabels) as FilterStatus[]).map((status) => (
            <button
              key={status}
              onClick={() => setStatusFilter(status)}
              className={`px-3.5 py-1.5 rounded-lg text-xs font-medium whitespace-nowrap transition-colors ${
                statusFilter === status
                  ? 'bg-gray-700 text-white'
                  : 'bg-gray-800/50 text-gray-500 hover:bg-gray-800'
              }`}
            >
              {statusLabels[status]}
            </button>
          ))}
        </div>
      </div>

      {/* Results count */}
      <p className="text-xs text-gray-500">
        {filteredTransactions.length} transaction{filteredTransactions.length !== 1 ? 's' : ''}
      </p>

      {/* Transaction List */}
      <div className="space-y-2">
        {filteredTransactions.length === 0 ? (
          <div className="nova-card text-center py-12">
            <svg className="w-12 h-12 text-gray-700 mx-auto mb-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 14.25v-2.625a3.375 3.375 0 00-3.375-3.375h-1.5A1.125 1.125 0 0113.5 7.125v-1.5a3.375 3.375 0 00-3.375-3.375H8.25m0 12.75h7.5m-7.5 3H12M10.5 2.25H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 00-9-9z" />
            </svg>
            <p className="text-sm text-gray-500">No transactions match your filters</p>
          </div>
        ) : (
          filteredTransactions.map((tx) => (
            <div
              key={tx.id}
              onClick={() => setSelectedTx(selectedTx?.id === tx.id ? null : tx)}
              className={`nova-card cursor-pointer transition-all ${
                selectedTx?.id === tx.id
                  ? 'border-nova-500/50 ring-1 ring-nova-500/20'
                  : 'hover:border-gray-700'
              }`}
            >
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-3">
                  <div
                    className={`w-9 h-9 rounded-full flex items-center justify-center shrink-0 ${
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
                        ? 'Credit Repayment'
                        : tx.type}
                    </p>
                    <p className="text-xs text-gray-500">{formatDate(tx.timestamp)}</p>
                  </div>
                </div>
                <div className="text-right">
                  <p
                    className={`text-sm font-semibold ${
                      tx.type === 'receive' || tx.type === 'credit_issued'
                        ? 'text-emerald-400'
                        : 'text-white'
                    }`}
                  >
                    {tx.type === 'receive' || tx.type === 'credit_issued' ? '+' : '-'}
                    {tx.amount.toLocaleString(undefined, { maximumFractionDigits: 8 })} {tx.symbol}
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

              {/* Expanded Detail */}
              {selectedTx?.id === tx.id && (
                <div className="mt-4 pt-4 border-t border-gray-800 space-y-2.5">
                  <div className="flex justify-between">
                    <span className="text-xs text-gray-500">Hash</span>
                    <code className="text-xs font-mono text-gray-300 select-all">
                      {truncateHash(tx.hash)}
                    </code>
                  </div>
                  <div className="flex justify-between">
                    <span className="text-xs text-gray-500">From</span>
                    <code className="text-xs font-mono text-gray-300">
                      {tx.from === address ? 'You' : truncateHash(tx.from)}
                    </code>
                  </div>
                  <div className="flex justify-between">
                    <span className="text-xs text-gray-500">To</span>
                    <code className="text-xs font-mono text-gray-300">
                      {tx.to === address ? 'You' : truncateHash(tx.to)}
                    </code>
                  </div>
                  <div className="flex justify-between">
                    <span className="text-xs text-gray-500">Fee</span>
                    <span className="text-xs text-gray-300">
                      {tx.fee} {tx.symbol}
                    </span>
                  </div>
                  {tx.blockHeight && (
                    <div className="flex justify-between">
                      <span className="text-xs text-gray-500">Block</span>
                      <span className="text-xs text-gray-300">
                        #{tx.blockHeight.toLocaleString()}
                      </span>
                    </div>
                  )}
                  {tx.payload && (
                    <div className="flex justify-between">
                      <span className="text-xs text-gray-500">Payload</span>
                      <span className="text-xs text-gray-300">{tx.payload}</span>
                    </div>
                  )}
                </div>
              )}
            </div>
          ))
        )}
      </div>
    </div>
  );
}
