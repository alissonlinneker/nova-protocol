import { useCallback, useEffect } from 'react';
import { useQuery } from '@tanstack/react-query';
import { getNovaClient } from '../lib/nova-client';
import { useWalletStore, signAndBuildTx } from '../stores/walletStore';
import type { Transaction } from '../stores/walletStore';

export function useNova() {
  const {
    address,
    network,
    nodeUrl,
    isWalletInitialized,
    addTransaction,
    updateBalances,
    setNetworkStatus,
  } = useWalletStore();

  const client = getNovaClient({ nodeUrl, network });

  // ---------------------------------------------------------------------------
  // Network status polling
  // ---------------------------------------------------------------------------

  const statusQuery = useQuery({
    queryKey: ['nodeStatus', nodeUrl],
    queryFn: async () => {
      try {
        const status = await client.getStatus();
        return status;
      } catch {
        return null;
      }
    },
    refetchInterval: 10_000,
    enabled: isWalletInitialized,
  });

  // Update connection status in store
  useEffect(() => {
    if (statusQuery.data) {
      setNetworkStatus(true, statusQuery.data.block_height);
    } else if (statusQuery.isError || statusQuery.data === null) {
      setNetworkStatus(false);
    }
  }, [statusQuery.data, statusQuery.isError, setNetworkStatus]);

  // ---------------------------------------------------------------------------
  // Balance polling
  // ---------------------------------------------------------------------------

  const balanceQuery = useQuery({
    queryKey: ['balance', address, nodeUrl],
    queryFn: async () => {
      if (!address) return null;
      try {
        const account = await client.getAccount(address);
        return account;
      } catch {
        // Try JSON-RPC fallback
        try {
          const balance = await client.getBalance(address);
          return { address, balance, nonce: 0 };
        } catch {
          return null;
        }
      }
    },
    refetchInterval: 15_000,
    enabled: isWalletInitialized && !!address,
  });

  // Sync balances to store
  useEffect(() => {
    if (balanceQuery.data) {
      const novaBalance = balanceQuery.data.balance;
      updateBalances([
        {
          symbol: 'NOVA',
          name: 'Nova Token',
          balance: novaBalance,
          usdValue: novaBalance * 3.0, // Placeholder price
          change24h: 0,
        },
      ]);
    }
  }, [balanceQuery.data, updateBalances]);

  // ---------------------------------------------------------------------------
  // Block height polling
  // ---------------------------------------------------------------------------

  const blockHeightQuery = useQuery({
    queryKey: ['blockHeight', nodeUrl],
    queryFn: async () => {
      try {
        return await client.getBlockHeight();
      } catch {
        return null;
      }
    },
    refetchInterval: 5_000,
    enabled: isWalletInitialized,
  });

  // ---------------------------------------------------------------------------
  // Fee estimation
  // ---------------------------------------------------------------------------

  const estimateFee = useCallback(
    async (symbol: string) => {
      return client.estimateFee(symbol);
    },
    [client],
  );

  // ---------------------------------------------------------------------------
  // Transfer (build, sign, submit)
  // ---------------------------------------------------------------------------

  const transfer = useCallback(
    async (
      to: string,
      amount: number,
      symbol: string,
      payload?: string,
    ): Promise<{ hash: string }> => {
      // Build and sign the transaction using real Ed25519 keys
      const { txId, signedTx } = signAndBuildTx({
        recipient: to,
        amount,
        currency: symbol,
        payload,
      });

      // Submit to the node via JSON-RPC
      try {
        const result = await client.sendTransaction(signedTx);

        const txRecord: Transaction = {
          id: txId,
          hash: result.tx_hash || txId,
          type: 'send',
          amount,
          symbol,
          from: address,
          to,
          fee: symbol === 'NOVA' ? 0.001 : 0.0005,
          status: 'pending',
          timestamp: Date.now(),
          payload,
        };

        addTransaction(txRecord);

        return { hash: result.tx_hash || txId };
      } catch (err) {
        // Even if the node is unreachable, record the attempt locally.
        // The tx was validly signed and can be resubmitted later.
        const txRecord: Transaction = {
          id: txId,
          hash: txId,
          type: 'send',
          amount,
          symbol,
          from: address,
          to,
          fee: symbol === 'NOVA' ? 0.001 : 0.0005,
          status: 'pending',
          timestamp: Date.now(),
          payload,
        };

        addTransaction(txRecord);

        // Re-throw with the original tx hash so the UI can show it
        const error = err instanceof Error ? err : new Error('Transaction submission failed');
        (error as Error & { txHash?: string }).txHash = txId;
        throw error;
      }
    },
    [address, client, addTransaction],
  );

  return {
    // Queries
    nodeStatus: statusQuery.data,
    isNodeConnected: statusQuery.data !== null && statusQuery.data !== undefined,
    isLoadingStatus: statusQuery.isLoading,
    balance: balanceQuery.data?.balance ?? null,
    isLoadingBalance: balanceQuery.isLoading,
    blockHeight: blockHeightQuery.data ?? null,

    // Actions
    estimateFee,
    transfer,
    network,

    // Refresh
    refetchBalance: balanceQuery.refetch,
    refetchStatus: statusQuery.refetch,
  };
}
