import { useCallback } from "react";
import { useQuery } from "@tanstack/react-query";
import { getNovaClient } from "../lib/nova-client";
import { useWalletStore } from "../stores/walletStore";

export function useNova() {
  const { address, network, nodeUrl } = useWalletStore();
  const client = getNovaClient({ nodeUrl, network });

  const balancesQuery = useQuery({
    queryKey: ["balances", address, network],
    queryFn: () => client.getBalance(address),
    refetchInterval: 30_000,
  });

  const latestBlockQuery = useQuery({
    queryKey: ["latestBlock", network],
    queryFn: () => client.getLatestBlock(),
    refetchInterval: 10_000,
  });

  const estimateFee = useCallback(
    async (symbol: string) => {
      return client.estimateFee(symbol);
    },
    [client]
  );

  const transfer = useCallback(
    async (to: string, amount: number, symbol: string, payload?: string) => {
      return client.transfer({ to, amount, symbol, payload });
    },
    [client]
  );

  return {
    balances: balancesQuery.data,
    isLoadingBalances: balancesQuery.isLoading,
    latestBlock: latestBlockQuery.data,
    estimateFee,
    transfer,
    network,
  };
}
