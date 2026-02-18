import { useMemo } from "react";
import { useWalletStore } from "../stores/walletStore";
import { truncateAddress } from "../lib/crypto";

export function useWallet() {
  const store = useWalletStore();

  const totalUsdBalance = useMemo(
    () => store.balances.reduce((acc, b) => acc + b.usdValue, 0),
    [store.balances]
  );

  const recentTransactions = useMemo(
    () =>
      [...store.transactions]
        .sort((a, b) => b.timestamp - a.timestamp)
        .slice(0, 10),
    [store.transactions]
  );

  const truncatedAddress = useMemo(
    () => truncateAddress(store.address),
    [store.address]
  );

  const truncatedPublicKey = useMemo(
    () => truncateAddress(store.publicKey, 12, 10),
    [store.publicKey]
  );

  return {
    ...store,
    totalUsdBalance,
    recentTransactions,
    truncatedAddress,
    truncatedPublicKey,
  };
}
