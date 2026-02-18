import { useState } from "react";
import { Link } from "react-router-dom";
import { useCredit } from "../hooks/useCredit";

export default function CreditMarket() {
  const {
    creditScore,
    scoreCategory,
    creditLines,
    availableOffers,
    totalCreditLimit,
    totalCreditUsed,
    creditUtilization,
    requestCredit,
  } = useCredit();

  const [selectedOffer, setSelectedOffer] = useState<string | null>(null);
  const [requestAmount, setRequestAmount] = useState("");
  const [isRequesting, setIsRequesting] = useState(false);
  const [requestResult, setRequestResult] = useState<"success" | "error" | null>(null);

  const handleRequest = async () => {
    if (!selectedOffer || !requestAmount) return;
    setIsRequesting(true);
    setRequestResult(null);
    try {
      await requestCredit(selectedOffer, parseFloat(requestAmount));
      setRequestResult("success");
      setSelectedOffer(null);
      setRequestAmount("");
    } catch {
      setRequestResult("error");
    } finally {
      setIsRequesting(false);
    }
  };

  const scoreColor =
    creditScore >= 750
      ? "text-emerald-400"
      : creditScore >= 700
      ? "text-accent-400"
      : creditScore >= 650
      ? "text-amber-400"
      : "text-red-400";

  const scoreRingColor =
    creditScore >= 750
      ? "stroke-emerald-400"
      : creditScore >= 700
      ? "stroke-accent-400"
      : creditScore >= 650
      ? "stroke-amber-400"
      : "stroke-red-400";

  const scorePercentage = (creditScore / 850) * 100;

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
        <h1 className="text-xl font-bold text-white">Credit Market</h1>
      </div>

      {/* Credit Score */}
      <div className="nova-card flex items-center gap-6">
        <div className="relative w-28 h-28 shrink-0">
          <svg className="w-28 h-28 -rotate-90" viewBox="0 0 100 100">
            <circle
              cx="50"
              cy="50"
              r="42"
              fill="none"
              stroke="currentColor"
              strokeWidth="8"
              className="text-gray-800"
            />
            <circle
              cx="50"
              cy="50"
              r="42"
              fill="none"
              strokeWidth="8"
              strokeLinecap="round"
              strokeDasharray={`${scorePercentage * 2.64} 264`}
              className={scoreRingColor}
            />
          </svg>
          <div className="absolute inset-0 flex flex-col items-center justify-center">
            <span className={`text-2xl font-bold ${scoreColor}`}>{creditScore}</span>
            <span className="text-[10px] text-gray-500 uppercase tracking-wider">Score</span>
          </div>
        </div>

        <div className="flex-1">
          <h3 className={`text-lg font-semibold ${scoreColor}`}>{scoreCategory}</h3>
          <p className="text-sm text-gray-400 mt-1">
            Your on-chain credit score is calculated from transaction history,
            repayment behavior, and collateral ratios.
          </p>
          <div className="flex gap-4 mt-3">
            <div>
              <p className="text-xs text-gray-500">Total Limit</p>
              <p className="text-sm font-semibold text-white">
                ${totalCreditLimit.toLocaleString()}
              </p>
            </div>
            <div>
              <p className="text-xs text-gray-500">Used</p>
              <p className="text-sm font-semibold text-white">
                ${totalCreditUsed.toLocaleString()}
              </p>
            </div>
            <div>
              <p className="text-xs text-gray-500">Utilization</p>
              <p className="text-sm font-semibold text-white">{creditUtilization}%</p>
            </div>
          </div>
        </div>
      </div>

      {/* Active Credit Lines */}
      <div className="nova-card">
        <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
          Active Credit Lines
        </h3>
        {creditLines.length === 0 ? (
          <p className="text-sm text-gray-500 py-4 text-center">
            No active credit lines
          </p>
        ) : (
          <div className="space-y-3">
            {creditLines.map((cl) => (
              <div
                key={cl.id}
                className="bg-gray-800/50 rounded-xl p-4 border border-gray-800"
              >
                <div className="flex items-center justify-between mb-3">
                  <div>
                    <p className="text-sm font-medium text-white">{cl.provider}</p>
                    <p className="text-xs text-gray-500">
                      {cl.rate}% APR &middot; {cl.term}
                    </p>
                  </div>
                  <span
                    className={`nova-badge ${
                      cl.status === "active"
                        ? "bg-emerald-500/10 text-emerald-400"
                        : cl.status === "pending"
                        ? "bg-amber-500/10 text-amber-400"
                        : "bg-gray-500/10 text-gray-400"
                    }`}
                  >
                    {cl.status}
                  </span>
                </div>

                {/* Usage bar */}
                <div className="flex items-center gap-3">
                  <div className="flex-1 h-2 bg-gray-700 rounded-full overflow-hidden">
                    <div
                      className="h-full bg-gradient-to-r from-nova-500 to-accent-500 rounded-full transition-all duration-500"
                      style={{
                        width: `${cl.limit > 0 ? (cl.used / cl.limit) * 100 : 0}%`,
                      }}
                    />
                  </div>
                  <span className="text-xs text-gray-400 shrink-0">
                    ${cl.used.toLocaleString()} / ${cl.limit.toLocaleString()}
                  </span>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Notifications */}
      {requestResult === "success" && (
        <div className="bg-emerald-500/10 border border-emerald-500/30 rounded-xl p-4 text-sm text-emerald-400">
          Credit request submitted successfully. It will be reviewed shortly.
        </div>
      )}
      {requestResult === "error" && (
        <div className="bg-red-500/10 border border-red-500/30 rounded-xl p-4 text-sm text-red-400">
          Failed to submit credit request. Please try again.
        </div>
      )}

      {/* Available Offers */}
      <div className="nova-card">
        <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
          Available Credit Offers
        </h3>
        <div className="space-y-3">
          {availableOffers.map((offer) => (
            <div
              key={offer.id}
              className={`bg-gray-800/50 rounded-xl p-4 border cursor-pointer transition-all ${
                selectedOffer === offer.id
                  ? "border-nova-500 ring-1 ring-nova-500/50"
                  : "border-gray-800 hover:border-gray-700"
              }`}
              onClick={() =>
                setSelectedOffer(selectedOffer === offer.id ? null : offer.id)
              }
            >
              <div className="flex items-center justify-between mb-2">
                <p className="text-sm font-medium text-white">{offer.provider}</p>
                <span className="text-sm font-semibold text-accent-400">
                  {offer.rate}% APR
                </span>
              </div>
              <div className="flex items-center gap-4 text-xs text-gray-500">
                <span>Up to ${offer.maxAmount.toLocaleString()}</span>
                <span>&middot;</span>
                <span>{offer.term}</span>
                <span>&middot;</span>
                <span>Min Score: {offer.minScore}</span>
              </div>

              {selectedOffer === offer.id && (
                <div className="mt-4 pt-4 border-t border-gray-700 space-y-3">
                  <div>
                    <label className="block text-sm font-medium text-gray-400 mb-2">
                      Request Amount (USDN)
                    </label>
                    <input
                      type="number"
                      value={requestAmount}
                      onChange={(e) => setRequestAmount(e.target.value)}
                      placeholder="0.00"
                      min="0"
                      max={offer.maxAmount}
                      className="nova-input"
                      onClick={(e) => e.stopPropagation()}
                    />
                    <p className="text-xs text-gray-500 mt-1">
                      Max: ${offer.maxAmount.toLocaleString()}
                    </p>
                  </div>
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      handleRequest();
                    }}
                    disabled={!requestAmount || isRequesting}
                    className="nova-btn-accent w-full py-3 disabled:opacity-40 disabled:cursor-not-allowed flex items-center justify-center gap-2"
                  >
                    {isRequesting ? (
                      <>
                        <div className="w-4 h-4 border-2 border-white border-t-transparent rounded-full animate-spin" />
                        Processing...
                      </>
                    ) : (
                      "Request Credit"
                    )}
                  </button>
                </div>
              )}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
