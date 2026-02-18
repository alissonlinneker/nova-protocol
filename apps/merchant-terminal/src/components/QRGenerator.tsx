interface QRGeneratorProps {
  data: string;
  size?: number;
  label?: string;
}

export default function QRGenerator({ data, size = 200, label }: QRGeneratorProps) {
  return (
    <div className="flex flex-col items-center">
      <div
        className="bg-white rounded-2xl flex items-center justify-center p-4"
        style={{ width: size, height: size }}
      >
        <div className="w-full h-full relative">
          <div className="absolute inset-0 grid grid-cols-13 grid-rows-13 gap-[1.5px]">
            {Array.from({ length: 169 }).map((_, i) => {
              const row = Math.floor(i / 13);
              const col = i % 13;
              const isCorner =
                (row < 3 && col < 3) ||
                (row < 3 && col > 9) ||
                (row > 9 && col < 3);
              const isTimingH = row === 6 && col % 2 === 0;
              const isTimingV = col === 6 && row % 2 === 0;
              // Seeded pattern based on data string hash
              const seed = data.charCodeAt(i % data.length) || 0;
              const isData = (seed + row * col) % 3 === 0;
              const isFilled =
                isCorner || isTimingH || isTimingV || (row > 2 && col > 2 && row < 11 && col < 11 && isData);

              return (
                <div
                  key={i}
                  className={`rounded-[0.5px] ${
                    isFilled ? "bg-gray-900" : "bg-white"
                  }`}
                />
              );
            })}
          </div>
          {/* Center branding */}
          <div className="absolute inset-0 flex items-center justify-center">
            <div className="w-10 h-10 rounded-lg bg-gradient-to-br from-nova-600 to-accent-500 flex items-center justify-center shadow-lg">
              <span className="text-[10px] font-bold text-white">N</span>
            </div>
          </div>
        </div>
      </div>
      {label && (
        <p className="text-xs text-gray-500 mt-3 text-center">{label}</p>
      )}
    </div>
  );
}
