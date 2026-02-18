import { useState, useEffect, useCallback, useRef } from "react";
import { useMerchantStore } from "../hooks/useMerchant";
import QRGenerator from "./QRGenerator";

type TerminalState = "input" | "waiting" | "confirmed";

const PAYMENT_TIMEOUT_MS = 5 * 60 * 1_000; // 5 minutes
const POLL_INTERVAL_MS = 3_000;

export default function Terminal() {
  const {
    address,
    pendingAmount,
    setPendingAmount,
    activePayment,
    createPaymentRequest,
    cancelPayment,
    expirePayment,
    pollForPayment,
    fetchBalance,
  } = useMerchantStore();

  const [state, setState] = useState<TerminalState>("input");
  const [error, setError] = useState<string | null>(null);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Sync terminal state with store's active payment on mount
  useEffect(() => {
    if (activePayment) {
      if (activePayment.status === "confirmed") {
        setState("confirmed");
      } else if (activePayment.status === "pending") {
        setState("waiting");
      }
    }
  }, []);

  const cleanup = useCallback(() => {
    if (pollRef.current) {
      clearInterval(pollRef.current);
      pollRef.current = null;
    }
    if (timeoutRef.current) {
      clearTimeout(timeoutRef.current);
      timeoutRef.current = null;
    }
  }, []);

  // Cleanup on unmount
  useEffect(() => cleanup, [cleanup]);

  const startPaymentPolling = useCallback(() => {
    // Poll the node for balance changes that indicate payment received
    pollRef.current = setInterval(async () => {
      const detected = await pollForPayment();
      if (detected) {
        cleanup();
        setState("confirmed");
      }
    }, POLL_INTERVAL_MS);

    // Set expiration timeout
    timeoutRef.current = setTimeout(() => {
      cleanup();
      expirePayment();
      setError("Payment request expired. No payment was detected.");
      setState("input");
    }, PAYMENT_TIMEOUT_MS);
  }, [pollForPayment, expirePayment, cleanup]);

  const handleNumpad = (value: string) => {
    setError(null);
    if (value === "clear") {
      setPendingAmount("");
      return;
    }
    if (value === "backspace") {
      setPendingAmount(pendingAmount.slice(0, -1));
      return;
    }
    if (value === "." && pendingAmount.includes(".")) return;
    if (pendingAmount.length >= 10) return;
    setPendingAmount(pendingAmount + value);
  };

  const handleCharge = async () => {
    const amount = parseFloat(pendingAmount);
    if (!pendingAmount || isNaN(amount) || amount <= 0) return;

    setError(null);

    try {
      // Snapshot the current balance before creating the payment request
      await fetchBalance();
      createPaymentRequest();
      setState("waiting");
      startPaymentPolling();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create payment request");
    }
  };

  const handleCancel = () => {
    cleanup();
    cancelPayment();
    setError(null);
    setState("input");
  };

  const handleNewCharge = () => {
    cleanup();
    cancelPayment();
    setPendingAmount("");
    setError(null);
    setState("input");
  };

  const displayAmount = pendingAmount || "0";
  const formattedAmount = parseFloat(displayAmount).toLocaleString("en-US", {
    minimumFractionDigits: pendingAmount.includes(".") ? 2 : 0,
    maximumFractionDigits: 2,
  });

  const numpadKeys = [
    "1", "2", "3",
    "4", "5", "6",
    "7", "8", "9",
    ".", "0", "backspace",
  ];

  // Build QR payload: nova:<address>?amount=<amount>
  const qrPayload = activePayment
    ? `nova:${activePayment.address}?amount=${activePayment.amount}`
    : "";

  return (
    <div className="nova-card">
      {/* Input State */}
      {state === "input" && (
        <div className="space-y-6">
          <div className="text-center">
            <p className="text-xs text-gray-500 uppercase tracking-wider mb-2">
              Amount to Charge
            </p>
            <div className="flex items-baseline justify-center gap-1">
              <span className="text-5xl font-bold text-white tracking-tight">
                {pendingAmount ? formattedAmount : "0"}
              </span>
              <span className="text-lg text-gray-500 ml-1">NOVA</span>
            </div>
          </div>

          {error && (
            <div className="bg-red-500/10 border border-red-500/20 rounded-xl px-4 py-3 text-sm text-red-400 text-center">
              {error}
            </div>
          )}

          {/* Numpad */}
          <div className="grid grid-cols-3 gap-2 max-w-xs mx-auto">
            {numpadKeys.map((key) => (
              <button
                key={key}
                onClick={() => handleNumpad(key)}
                className={`h-14 rounded-xl text-lg font-medium transition-all active:scale-95 ${
                  key === "backspace"
                    ? "bg-gray-800 text-gray-400 hover:bg-gray-700"
                    : key === "."
                    ? "bg-gray-800 text-gray-300 hover:bg-gray-700"
                    : "bg-gray-800 text-white hover:bg-gray-700"
                }`}
              >
                {key === "backspace" ? (
                  <svg className="w-5 h-5 mx-auto" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                    <path strokeLinecap="round" strokeLinejoin="round" d="M12 9.75L14.25 12m0 0l2.25 2.25M14.25 12l2.25-2.25M14.25 12L12 14.25m-2.58 4.92l-6.374-6.375a1.125 1.125 0 010-1.59L9.42 4.83c.21-.211.497-.33.795-.33H19.5a2.25 2.25 0 012.25 2.25v10.5a2.25 2.25 0 01-2.25 2.25h-9.284c-.298 0-.585-.119-.795-.33z" />
                  </svg>
                ) : (
                  key
                )}
              </button>
            ))}
          </div>

          <div className="flex gap-3">
            <button
              onClick={() => handleNumpad("clear")}
              className="nova-btn-secondary flex-1 py-3.5"
            >
              Clear
            </button>
            <button
              onClick={handleCharge}
              disabled={!pendingAmount || parseFloat(pendingAmount) <= 0}
              className="nova-btn-primary flex-1 py-3.5 text-base disabled:opacity-40 disabled:cursor-not-allowed"
            >
              Charge
            </button>
          </div>
        </div>
      )}

      {/* Waiting State */}
      {state === "waiting" && activePayment && (
        <div className="flex flex-col items-center py-8 space-y-6">
          <div className="text-center mb-2">
            <p className="text-xs text-gray-500 uppercase tracking-wider mb-2">
              Payment Requested
            </p>
            <p className="text-3xl font-bold text-white">
              {activePayment.amount.toLocaleString("en-US", {
                minimumFractionDigits: 2,
              })}
              <span className="text-base text-gray-500 ml-1">NOVA</span>
            </p>
          </div>

          <QRGenerator
            data={qrPayload}
            size={220}
            label="Scan to pay with NOVA Wallet"
          />

          {/* Destination address */}
          <div className="w-full bg-gray-800/50 rounded-xl px-4 py-3">
            <p className="text-[10px] text-gray-500 uppercase tracking-wider mb-1">
              Send to
            </p>
            <code className="text-xs font-mono text-gray-300 break-all">
              {address}
            </code>
          </div>

          {/* Waiting animation */}
          <div className="flex items-center gap-3">
            <div className="flex gap-1">
              <div className="w-2 h-2 rounded-full bg-nova-500 animate-bounce" style={{ animationDelay: "0ms" }} />
              <div className="w-2 h-2 rounded-full bg-nova-500 animate-bounce" style={{ animationDelay: "150ms" }} />
              <div className="w-2 h-2 rounded-full bg-nova-500 animate-bounce" style={{ animationDelay: "300ms" }} />
            </div>
            <p className="text-sm text-gray-400">Waiting for payment...</p>
          </div>

          <button
            onClick={handleCancel}
            className="nova-btn-secondary px-6"
          >
            Cancel
          </button>
        </div>
      )}

      {/* Confirmed State */}
      {state === "confirmed" && activePayment && (
        <div className="flex flex-col items-center py-8 space-y-6">
          {/* Success animation */}
          <div className="relative">
            <div className="w-20 h-20 rounded-full bg-emerald-500/20 flex items-center justify-center">
              <svg
                className="w-10 h-10 text-emerald-400"
                fill="none"
                viewBox="0 0 24 24"
                stroke="currentColor"
                strokeWidth={2.5}
              >
                <path strokeLinecap="round" strokeLinejoin="round" d="M4.5 12.75l6 6 9-13.5" />
              </svg>
            </div>
            <div className="absolute inset-0 rounded-full border-2 border-emerald-400/30 animate-ping" />
          </div>

          <div className="text-center">
            <h3 className="text-xl font-bold text-emerald-400 mb-1">
              Payment Confirmed
            </h3>
            <p className="text-3xl font-bold text-white mt-2">
              {activePayment.amount.toLocaleString("en-US", {
                minimumFractionDigits: 2,
              })}
              <span className="text-base text-gray-500 ml-1">NOVA</span>
            </p>
          </div>

          <div className="w-full bg-gray-800/50 rounded-xl p-4 space-y-2">
            {activePayment.sender && (
              <div className="flex justify-between">
                <span className="text-xs text-gray-500">From</span>
                <code className="text-xs font-mono text-gray-300">
                  {activePayment.sender.length > 20
                    ? `${activePayment.sender.slice(0, 12)}...${activePayment.sender.slice(-6)}`
                    : activePayment.sender}
                </code>
              </div>
            )}
            {activePayment.txHash && (
              <div className="flex justify-between">
                <span className="text-xs text-gray-500">Hash</span>
                <code className="text-xs font-mono text-gray-300">
                  {activePayment.txHash.slice(0, 12)}...{activePayment.txHash.slice(-6)}
                </code>
              </div>
            )}
            <div className="flex justify-between">
              <span className="text-xs text-gray-500">Time</span>
              <span className="text-xs text-gray-300">
                {new Date().toLocaleTimeString()}
              </span>
            </div>
          </div>

          <button
            onClick={handleNewCharge}
            className="nova-btn-primary w-full py-3.5 text-base"
          >
            New Charge
          </button>
        </div>
      )}
    </div>
  );
}
