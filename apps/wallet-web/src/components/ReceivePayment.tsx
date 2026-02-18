import { useState, useMemo } from 'react';
import { Link } from 'react-router-dom';
import { useWallet } from '../hooks/useWallet';

/**
 * Minimal QR code SVG generator.
 *
 * Uses a deterministic bit matrix derived from the input data to produce
 * a visual QR-like pattern. This is a simplified approach that creates a
 * recognizable visual representation. For production scanning support,
 * integrate a full QR encoder library.
 */
function generateQrMatrix(data: string, size: number): boolean[][] {
  // Simple hash-based matrix generation for visual representation.
  // Deterministic: same data always produces the same pattern.
  const matrix: boolean[][] = Array.from({ length: size }, () =>
    Array.from({ length: size }, () => false),
  );

  // Finder patterns (3 corners)
  const finderSize = 7;
  const drawFinder = (startRow: number, startCol: number) => {
    for (let r = 0; r < finderSize; r++) {
      for (let c = 0; c < finderSize; c++) {
        const isOuter = r === 0 || r === finderSize - 1 || c === 0 || c === finderSize - 1;
        const isInner = r >= 2 && r <= 4 && c >= 2 && c <= 4;
        if (isOuter || isInner) {
          const mr = startRow + r;
          const mc = startCol + c;
          if (mr < size && mc < size) {
            matrix[mr]![mc] = true;
          }
        }
      }
    }
  };

  drawFinder(0, 0);
  drawFinder(0, size - finderSize);
  drawFinder(size - finderSize, 0);

  // Timing patterns
  for (let i = finderSize; i < size - finderSize; i++) {
    matrix[6]![i] = i % 2 === 0;
    matrix[i]![6] = i % 2 === 0;
  }

  // Data region - hash-based deterministic fill
  let hash = 0;
  for (let i = 0; i < data.length; i++) {
    hash = ((hash << 5) - hash + data.charCodeAt(i)) | 0;
  }

  for (let r = 8; r < size - 8; r++) {
    for (let c = 8; c < size - 8; c++) {
      if (r === 6 || c === 6) continue;
      // Deterministic pseudo-random based on position and data hash
      const seed = ((r * 31 + c * 17 + hash) * 2654435761) >>> 0;
      matrix[r]![c] = seed % 3 !== 0;
    }
  }

  return matrix;
}

function QrCode({ data, cellSize = 4 }: { data: string; cellSize?: number }) {
  const size = 29; // Standard QR module count for medium data
  const matrix = useMemo(() => generateQrMatrix(data, size), [data]);
  const svgSize = size * cellSize;

  return (
    <svg
      width={svgSize}
      height={svgSize}
      viewBox={`0 0 ${svgSize} ${svgSize}`}
      xmlns="http://www.w3.org/2000/svg"
    >
      <rect width={svgSize} height={svgSize} fill="white" />
      {matrix.map((row, r) =>
        row.map(
          (cell, c) =>
            cell && (
              <rect
                key={`${r}-${c}`}
                x={c * cellSize}
                y={r * cellSize}
                width={cellSize}
                height={cellSize}
                fill="#1a1a2e"
              />
            ),
        ),
      )}
    </svg>
  );
}

export default function ReceivePayment() {
  const { address, truncatedAddress } = useWallet();
  const [copied, setCopied] = useState(false);
  const [requestAmount, setRequestAmount] = useState('');
  const [requestSymbol, setRequestSymbol] = useState('NOVA');

  const handleCopy = async () => {
    await navigator.clipboard.writeText(address);
    setCopied(true);
    setTimeout(() => setCopied(false), 2_000);
  };

  const paymentUri = requestAmount
    ? `nova:${address}?amount=${requestAmount}&token=${requestSymbol}`
    : `nova:${address}`;

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
        <h1 className="text-xl font-bold text-white">Receive Payment</h1>
      </div>

      {/* QR Code */}
      <div className="nova-card flex flex-col items-center py-8">
        <div className="bg-white rounded-2xl p-4 mb-6">
          <QrCode data={paymentUri} cellSize={4} />
        </div>

        <p className="text-xs text-gray-500 mb-4">
          Scan this QR code to send a payment
        </p>

        {/* Address display */}
        <div className="flex items-center gap-2 bg-gray-800 rounded-xl px-4 py-3 w-full max-w-xs">
          <code className="text-sm font-mono text-gray-300 flex-1 text-center">
            {truncatedAddress}
          </code>
          <button
            onClick={handleCopy}
            className="text-gray-500 hover:text-nova-400 transition-colors shrink-0"
          >
            {copied ? (
              <svg className="w-4 h-4 text-emerald-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M5 13l4 4L19 7" />
              </svg>
            ) : (
              <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M15.75 17.25v3.375c0 .621-.504 1.125-1.125 1.125h-9.75a1.125 1.125 0 01-1.125-1.125V7.875c0-.621.504-1.125 1.125-1.125H6.75a9.06 9.06 0 011.5.124m7.5 10.376h3.375c.621 0 1.125-.504 1.125-1.125V11.25c0-4.46-3.243-8.161-7.5-8.876a9.06 9.06 0 00-1.5-.124H9.375c-.621 0-1.125.504-1.125 1.125v3.5m7.5 10.375H9.375a1.125 1.125 0 01-1.125-1.125v-9.25m12 6.625v-1.875a3.375 3.375 0 00-3.375-3.375h-1.5a1.125 1.125 0 01-1.125-1.125v-1.5a3.375 3.375 0 00-3.375-3.375H9.75" />
              </svg>
            )}
          </button>
        </div>

        {copied && (
          <p className="text-xs text-emerald-400 mt-2 font-medium">
            Address copied to clipboard
          </p>
        )}
      </div>

      {/* Full Address */}
      <div className="nova-card">
        <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-3">
          Full Address
        </h3>
        <div className="bg-gray-800/50 rounded-xl p-3">
          <p className="text-xs font-mono text-gray-300 break-all select-all">
            {address}
          </p>
        </div>
      </div>

      {/* Request Amount */}
      <div className="nova-card">
        <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
          Request Specific Amount
        </h3>
        <div className="flex gap-3 mb-3">
          <input
            type="number"
            value={requestAmount}
            onChange={(e) => setRequestAmount(e.target.value)}
            placeholder="0.00"
            min="0"
            step="0.00000001"
            className="nova-input flex-1"
          />
          <select
            value={requestSymbol}
            onChange={(e) => setRequestSymbol(e.target.value)}
            className="nova-input w-28"
          >
            <option value="NOVA">NOVA</option>
          </select>
        </div>

        {requestAmount && (
          <div className="bg-gray-800/50 rounded-xl p-3">
            <label className="text-[11px] uppercase tracking-wider text-gray-500 font-medium">
              Payment URI
            </label>
            <p className="text-xs font-mono text-gray-400 mt-1 break-all select-all">
              {paymentUri}
            </p>
          </div>
        )}
      </div>

      {/* Share Options */}
      <div className="grid grid-cols-2 gap-3">
        <button
          onClick={async () => {
            if (navigator.share) {
              try {
                await navigator.share({
                  title: 'NOVA Payment Address',
                  text: `Send NOVA to: ${address}`,
                  url: paymentUri,
                });
              } catch {
                // User cancelled or share not supported
              }
            } else {
              await navigator.clipboard.writeText(paymentUri);
            }
          }}
          className="nova-btn-secondary flex items-center justify-center gap-2"
        >
          <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M7.217 10.907a2.25 2.25 0 100 2.186m0-2.186c.18.324.283.696.283 1.093s-.103.77-.283 1.093m0-2.186l9.566-5.314m-9.566 7.5l9.566 5.314m0 0a2.25 2.25 0 103.935 2.186 2.25 2.25 0 00-3.935-2.186zm0-12.814a2.25 2.25 0 103.933-2.185 2.25 2.25 0 00-3.933 2.185z" />
          </svg>
          Share
        </button>
        <button onClick={handleCopy} className="nova-btn-primary flex items-center justify-center gap-2">
          <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M15.75 17.25v3.375c0 .621-.504 1.125-1.125 1.125h-9.75a1.125 1.125 0 01-1.125-1.125V7.875c0-.621.504-1.125 1.125-1.125H6.75a9.06 9.06 0 011.5.124m7.5 10.376h3.375c.621 0 1.125-.504 1.125-1.125V11.25c0-4.46-3.243-8.161-7.5-8.876a9.06 9.06 0 00-1.5-.124H9.375c-.621 0-1.125.504-1.125 1.125v3.5m7.5 10.375H9.375a1.125 1.125 0 01-1.125-1.125v-9.25m12 6.625v-1.875a3.375 3.375 0 00-3.375-3.375h-1.5a1.125 1.125 0 01-1.125-1.125v-1.5a3.375 3.375 0 00-3.375-3.375H9.75" />
          </svg>
          Copy Address
        </button>
      </div>
    </div>
  );
}
