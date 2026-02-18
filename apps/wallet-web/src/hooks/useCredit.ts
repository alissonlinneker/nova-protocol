import { useMemo, useCallback } from "react";
import { useWalletStore } from "../stores/walletStore";
import type { CreditLine } from "../stores/walletStore";

interface CreditOffer {
  id: string;
  provider: string;
  maxAmount: number;
  rate: number;
  term: string;
  minScore: number;
}

const AVAILABLE_OFFERS: CreditOffer[] = [
  {
    id: "offer-001",
    provider: "Nova Prime Pool",
    maxAmount: 25_000,
    rate: 4.5,
    term: "90 days",
    minScore: 700,
  },
  {
    id: "offer-002",
    provider: "DeFi Credit DAO",
    maxAmount: 10_000,
    rate: 6.2,
    term: "30 days",
    minScore: 650,
  },
  {
    id: "offer-003",
    provider: "Stellar Lending",
    maxAmount: 50_000,
    rate: 3.8,
    term: "180 days",
    minScore: 750,
  },
  {
    id: "offer-004",
    provider: "Nexus Finance",
    maxAmount: 15_000,
    rate: 5.5,
    term: "60 days",
    minScore: 680,
  },
];

export function useCredit() {
  const { creditScore, creditLines } = useWalletStore();

  const availableOffers = useMemo(
    () => AVAILABLE_OFFERS.filter((offer) => creditScore >= offer.minScore),
    [creditScore]
  );

  const totalCreditLimit = useMemo(
    () => creditLines.reduce((acc, cl) => acc + cl.limit, 0),
    [creditLines]
  );

  const totalCreditUsed = useMemo(
    () => creditLines.reduce((acc, cl) => acc + cl.used, 0),
    [creditLines]
  );

  const creditUtilization = useMemo(
    () =>
      totalCreditLimit > 0
        ? Math.round((totalCreditUsed / totalCreditLimit) * 100)
        : 0,
    [totalCreditLimit, totalCreditUsed]
  );

  const scoreCategory = useMemo((): string => {
    if (creditScore >= 750) return "Excellent";
    if (creditScore >= 700) return "Good";
    if (creditScore >= 650) return "Fair";
    return "Building";
  }, [creditScore]);

  const requestCredit = useCallback(
    async (_offerId: string, _amount: number): Promise<CreditLine> => {
      // Simulated credit issuance
      await new Promise((resolve) => setTimeout(resolve, 2_000));

      const offer = AVAILABLE_OFFERS.find((o) => o.id === _offerId);
      if (!offer) throw new Error("Offer not found");

      return {
        id: `cl-${Date.now()}`,
        provider: offer.provider,
        limit: _amount,
        used: 0,
        rate: offer.rate,
        term: offer.term,
        status: "pending",
      };
    },
    []
  );

  return {
    creditScore,
    scoreCategory,
    creditLines,
    availableOffers,
    totalCreditLimit,
    totalCreditUsed,
    creditUtilization,
    requestCredit,
  };
}

export type { CreditOffer };
