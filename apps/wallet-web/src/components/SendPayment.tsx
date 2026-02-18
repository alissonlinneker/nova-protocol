import { useState, useMemo } from "react";
import { Link } from "react-router-dom";
import { useWallet } from "../hooks/useWallet";
import { useNova } from "../hooks/useNova";

type Step = "form" | "review" | "sending" | "result";

interface SendForm {
  recipient: string;
  amount: string;
  symbol: string;
  payload: string;
}

export default function SendPayment() {
  const { balances, address } = useWallet();
  const { transfer, estimateFee } = useNova();

  const [step, setStep] = useState<Step>("form");
  const [form, setForm] = useState<SendForm>({
    recipient: "",
    amount: "",
    symbol: "NOVA",
    payload: "",
  });
  const [fee, setFee] = useState<number>(0);
  const [txHash, setTxHash] = useState<string>("");
  const [error, setError] = useState<string>("");

  const selectedBalance = useMemo(
    () => balances.find((b) => b.symbol === form.symbol),
    [balances, form.symbol]
  );

  const isFormValid = useMemo(() => {
    const amount = parseFloat(form.amount);
    return (
      form.recipient.startsWith("nova1") &&
      form.recipient.length >= 20 &&
      amount > 0 &&
      selectedBalance &&
      amount <= selectedBalance.balance
    );
  }, [form, selectedBalance]);

  const handleReview = async () => {
    try {
      const estimatedFee = await estimateFee(form.symbol);
      setFee(estimatedFee);
      setStep("review");
    } catch {
      setError("Failed to estimate fee. Please try again.");
    }
  };

  const handleConfirm = async () => {
    setStep("sending");
    setError("");
    try {
      const result = await transfer(
        form.recipient,
        parseFloat(form.amount),
        form.symbol,
        form.payload || undefined
      );
      setTxHash(result.hash);
      setStep("result");
    } catch {
      setError("Transaction failed. Please try again.");
      setStep("review");
    }
  };

  const handleReset = () => {
    setForm({ recipient: "", amount: "", symbol: "NOVA", payload: "" });
    setStep("form");
    setTxHash("");
    setError("");
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
        <h1 className="text-xl font-bold text-white">Send Payment</h1>
      </div>

      {/* Step Indicator */}
      <div className="flex items-center gap-2">
        {["form", "review", "result"].map((s, i) => (
          <div key={s} className="flex items-center gap-2">
            <div
              className={`w-8 h-8 rounded-full flex items-center justify-center text-xs font-bold transition-colors ${
                step === s || (step === "sending" && s === "review")
                  ? "bg-nova-600 text-white"
                  : i < ["form", "review", "result"].indexOf(step === "sending" ? "review" : step)
                  ? "bg-nova-600/30 text-nova-300"
                  : "bg-gray-800 text-gray-500"
              }`}
            >
              {i + 1}
            </div>
            {i < 2 && (
              <div className={`w-12 h-0.5 ${
                i < ["form", "review", "result"].indexOf(step === "sending" ? "review" : step)
                  ? "bg-nova-600"
                  : "bg-gray-800"
              }`} />
            )}
          </div>
        ))}
      </div>

      {error && (
        <div className="bg-red-500/10 border border-red-500/30 rounded-xl p-4 text-sm text-red-400">
          {error}
        </div>
      )}

      {/* Form Step */}
      {step === "form" && (
        <div className="space-y-4">
          <div className="nova-card space-y-4">
            {/* Recipient */}
            <div>
              <label className="block text-sm font-medium text-gray-400 mb-2">
                Recipient Address
              </label>
              <div className="relative">
                <input
                  type="text"
                  value={form.recipient}
                  onChange={(e) => setForm({ ...form, recipient: e.target.value })}
                  placeholder="nova1..."
                  className="nova-input pr-12 font-mono text-sm"
                />
                <button
                  className="absolute right-3 top-1/2 -translate-y-1/2 text-gray-500 hover:text-nova-400 transition-colors"
                  title="Scan QR Code"
                >
                  <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                    <path strokeLinecap="round" strokeLinejoin="round" d="M3.75 4.875c0-.621.504-1.125 1.125-1.125h4.5c.621 0 1.125.504 1.125 1.125v4.5c0 .621-.504 1.125-1.125 1.125h-4.5A1.125 1.125 0 013.75 9.375v-4.5zM3.75 14.625c0-.621.504-1.125 1.125-1.125h4.5c.621 0 1.125.504 1.125 1.125v4.5c0 .621-.504 1.125-1.125 1.125h-4.5a1.125 1.125 0 01-1.125-1.125v-4.5zM13.5 4.875c0-.621.504-1.125 1.125-1.125h4.5c.621 0 1.125.504 1.125 1.125v4.5c0 .621-.504 1.125-1.125 1.125h-4.5A1.125 1.125 0 0113.5 9.375v-4.5z" />
                    <path strokeLinecap="round" strokeLinejoin="round" d="M6.75 6.75h.75v.75h-.75v-.75zM6.75 16.5h.75v.75h-.75v-.75zM16.5 6.75h.75v.75h-.75v-.75zM13.5 13.5h.75v.75h-.75v-.75zM13.5 19.5h.75v.75h-.75v-.75zM19.5 13.5h.75v.75h-.75v-.75zM19.5 19.5h.75v.75h-.75v-.75zM16.5 16.5h.75v.75h-.75v-.75z" />
                  </svg>
                </button>
              </div>
            </div>

            {/* Amount */}
            <div>
              <label className="block text-sm font-medium text-gray-400 mb-2">
                Amount
              </label>
              <div className="flex gap-3">
                <input
                  type="number"
                  value={form.amount}
                  onChange={(e) => setForm({ ...form, amount: e.target.value })}
                  placeholder="0.00"
                  min="0"
                  step="0.01"
                  className="nova-input flex-1 text-lg font-semibold"
                />
                <select
                  value={form.symbol}
                  onChange={(e) => setForm({ ...form, symbol: e.target.value })}
                  className="nova-input w-32 text-sm font-medium"
                >
                  {balances.map((b) => (
                    <option key={b.symbol} value={b.symbol}>
                      {b.symbol}
                    </option>
                  ))}
                </select>
              </div>
              {selectedBalance && (
                <p className="text-xs text-gray-500 mt-2">
                  Available: {selectedBalance.balance.toLocaleString()} {selectedBalance.symbol}
                  <button
                    onClick={() =>
                      setForm({ ...form, amount: selectedBalance.balance.toString() })
                    }
                    className="ml-2 text-nova-400 hover:text-nova-300 font-medium"
                  >
                    Max
                  </button>
                </p>
              )}
            </div>

            {/* Memo */}
            <div>
              <label className="block text-sm font-medium text-gray-400 mb-2">
                Memo <span className="text-gray-600">(optional)</span>
              </label>
              <input
                type="text"
                value={form.payload}
                onChange={(e) => setForm({ ...form, payload: e.target.value })}
                placeholder="Add a note..."
                className="nova-input text-sm"
              />
            </div>
          </div>

          <button
            onClick={handleReview}
            disabled={!isFormValid}
            className="nova-btn-primary w-full py-3.5 text-base disabled:opacity-40 disabled:cursor-not-allowed"
          >
            Review Transaction
          </button>
        </div>
      )}

      {/* Review Step */}
      {step === "review" && (
        <div className="space-y-4">
          <div className="nova-card space-y-4">
            <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider">
              Transaction Summary
            </h3>

            <div className="space-y-3">
              <div className="flex justify-between py-2 border-b border-gray-800">
                <span className="text-sm text-gray-400">From</span>
                <span className="text-sm font-mono text-gray-200">
                  {address.slice(0, 12)}...{address.slice(-8)}
                </span>
              </div>
              <div className="flex justify-between py-2 border-b border-gray-800">
                <span className="text-sm text-gray-400">To</span>
                <span className="text-sm font-mono text-gray-200">
                  {form.recipient.slice(0, 12)}...{form.recipient.slice(-8)}
                </span>
              </div>
              <div className="flex justify-between py-2 border-b border-gray-800">
                <span className="text-sm text-gray-400">Amount</span>
                <span className="text-sm font-semibold text-white">
                  {parseFloat(form.amount).toLocaleString()} {form.symbol}
                </span>
              </div>
              <div className="flex justify-between py-2 border-b border-gray-800">
                <span className="text-sm text-gray-400">Network Fee</span>
                <span className="text-sm text-gray-300">
                  {fee} {form.symbol}
                </span>
              </div>
              {form.payload && (
                <div className="flex justify-between py-2 border-b border-gray-800">
                  <span className="text-sm text-gray-400">Memo</span>
                  <span className="text-sm text-gray-300">{form.payload}</span>
                </div>
              )}
              <div className="flex justify-between py-2">
                <span className="text-sm font-semibold text-gray-300">Total</span>
                <span className="text-sm font-bold text-white">
                  {(parseFloat(form.amount) + fee).toLocaleString()} {form.symbol}
                </span>
              </div>
            </div>
          </div>

          <div className="flex gap-3">
            <button
              onClick={() => setStep("form")}
              className="nova-btn-secondary flex-1 py-3.5"
            >
              Back
            </button>
            <button
              onClick={handleConfirm}
              className="nova-btn-primary flex-1 py-3.5"
            >
              Confirm & Send
            </button>
          </div>
        </div>
      )}

      {/* Sending Step */}
      {step === "sending" && (
        <div className="nova-card flex flex-col items-center py-12">
          <div className="w-16 h-16 rounded-full border-4 border-nova-500 border-t-transparent animate-spin mb-6" />
          <h3 className="text-lg font-semibold text-white mb-2">
            Processing Transaction
          </h3>
          <p className="text-sm text-gray-400">
            Broadcasting to the NOVA network...
          </p>
        </div>
      )}

      {/* Result Step */}
      {step === "result" && (
        <div className="space-y-4">
          <div className="nova-card flex flex-col items-center py-10">
            <div className="w-16 h-16 rounded-full bg-emerald-500/20 flex items-center justify-center mb-5">
              <svg className="w-8 h-8 text-emerald-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M9 12.75L11.25 15 15 9.75M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
              </svg>
            </div>
            <h3 className="text-lg font-semibold text-white mb-1">
              Transaction Sent
            </h3>
            <p className="text-sm text-gray-400 mb-4">
              Your payment has been submitted to the network
            </p>

            <div className="w-full bg-gray-800/50 rounded-xl p-4 mt-2">
              <label className="text-[11px] uppercase tracking-wider text-gray-500 font-medium">
                Transaction Hash
              </label>
              <p className="text-xs font-mono text-gray-300 mt-1 break-all">
                {txHash}
              </p>
            </div>
          </div>

          <div className="flex gap-3">
            <button onClick={handleReset} className="nova-btn-secondary flex-1 py-3.5">
              Send Another
            </button>
            <Link to="/" className="nova-btn-primary flex-1 py-3.5 text-center">
              Done
            </Link>
          </div>
        </div>
      )}
    </div>
  );
}
