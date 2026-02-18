import { useState, useEffect } from "react";
import { Routes, Route, NavLink, useNavigate } from "react-router-dom";
import BlockList from "./components/BlockList";
import BlockDetail from "./components/BlockDetail";
import TransactionDetail from "./components/TransactionDetail";
import AddressView from "./components/AddressView";
import NetworkStats from "./components/NetworkStats";
import { useExplorerStore } from "./store/explorerStore";

export default function App() {
  const navigate = useNavigate();
  const [globalSearch, setGlobalSearch] = useState("");
  const { status, connected, startPolling, stopPolling, connectWs, disconnectWs } =
    useExplorerStore();

  // Start polling and WebSocket on mount.
  useEffect(() => {
    startPolling();
    connectWs();
    return () => {
      stopPolling();
      disconnectWs();
    };
  }, [startPolling, stopPolling, connectWs, disconnectWs]);

  const handleGlobalSearch = (e: React.FormEvent) => {
    e.preventDefault();
    const q = globalSearch.trim();
    if (!q) return;

    if (q.startsWith("nova1")) {
      navigate(`/address/${q}`);
    } else if (q.startsWith("0x") || /^[0-9a-fA-F]{16,}$/.test(q)) {
      navigate(`/tx/${q}`);
    } else if (/^\d+$/.test(q)) {
      navigate(`/block/${q}`);
    }
    setGlobalSearch("");
  };

  return (
    <div className="min-h-screen bg-gray-950">
      {/* Header */}
      <header className="border-b border-gray-800/50 bg-gray-950/90 backdrop-blur-xl sticky top-0 z-30">
        <div className="max-w-6xl mx-auto px-4 py-4">
          <div className="flex items-center justify-between gap-4">
            {/* Logo */}
            <NavLink to="/" className="flex items-center gap-3 shrink-0">
              <div className="w-10 h-10 rounded-xl bg-gradient-to-br from-nova-500 to-accent-500 flex items-center justify-center">
                <span className="text-sm font-bold text-white">N</span>
              </div>
              <div className="hidden sm:block">
                <h1 className="text-base font-bold nova-gradient-text">NOVA Explorer</h1>
                <p className="text-[10px] text-gray-500 uppercase tracking-widest">
                  Block Explorer
                </p>
              </div>
            </NavLink>

            {/* Global Search */}
            <form onSubmit={handleGlobalSearch} className="flex-1 max-w-xl">
              <div className="relative">
                <svg
                  className="absolute left-4 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-500"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                  strokeWidth={2}
                >
                  <path strokeLinecap="round" strokeLinejoin="round" d="M21 21l-5.197-5.197m0 0A7.5 7.5 0 105.196 5.196a7.5 7.5 0 0010.607 10.607z" />
                </svg>
                <input
                  type="text"
                  value={globalSearch}
                  onChange={(e) => setGlobalSearch(e.target.value)}
                  placeholder="Search by address, tx hash, or block height..."
                  className="w-full bg-gray-800/80 border border-gray-700/50 rounded-xl pl-11 pr-4 py-2.5 text-sm text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-nova-500 focus:border-transparent transition-all"
                />
              </div>
            </form>

            {/* Network Badge */}
            <div className="hidden sm:flex items-center gap-2 shrink-0">
              <span
                className={`w-2 h-2 rounded-full ${
                  connected ? "bg-emerald-400 animate-pulse" : "bg-red-400"
                }`}
              />
              <span
                className={`text-xs font-medium ${
                  connected ? "text-emerald-400" : "text-red-400"
                }`}
              >
                {connected
                  ? status?.network
                    ? status.network.charAt(0).toUpperCase() + status.network.slice(1)
                    : "Connected"
                  : "Disconnected"}
              </span>
            </div>
          </div>

          {/* Navigation */}
          <nav className="flex gap-1 mt-3 -mb-[1px]">
            <NavLink
              to="/"
              end
              className={({ isActive }) =>
                `flex items-center gap-2 px-4 py-2 text-sm font-medium rounded-t-lg transition-all border-b-2 ${
                  isActive
                    ? "text-nova-400 border-nova-500 bg-gray-900/50"
                    : "text-gray-500 border-transparent hover:text-gray-300"
                }`
              }
            >
              <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.8}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M6.429 9.75L2.25 12l4.179 2.25m0-4.5l5.571 3 5.571-3m-11.142 0L2.25 7.5 12 2.25l9.75 5.25-4.179 2.25m0 0L21.75 12l-4.179 2.25m0 0l4.179 2.25L12 21.75 2.25 16.5l4.179-2.25m11.142 0l-5.571 3-5.571-3" />
              </svg>
              Blocks
            </NavLink>
            <NavLink
              to="/stats"
              className={({ isActive }) =>
                `flex items-center gap-2 px-4 py-2 text-sm font-medium rounded-t-lg transition-all border-b-2 ${
                  isActive
                    ? "text-nova-400 border-nova-500 bg-gray-900/50"
                    : "text-gray-500 border-transparent hover:text-gray-300"
                }`
              }
            >
              <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.8}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M3 13.125C3 12.504 3.504 12 4.125 12h2.25c.621 0 1.125.504 1.125 1.125v6.75C7.5 20.496 6.996 21 6.375 21h-2.25A1.125 1.125 0 013 19.875v-6.75zM9.75 8.625c0-.621.504-1.125 1.125-1.125h2.25c.621 0 1.125.504 1.125 1.125v11.25c0 .621-.504 1.125-1.125 1.125h-2.25a1.125 1.125 0 01-1.125-1.125V8.625zM16.5 4.125c0-.621.504-1.125 1.125-1.125h2.25C20.496 3 21 3.504 21 4.125v15.75c0 .621-.504 1.125-1.125 1.125h-2.25a1.125 1.125 0 01-1.125-1.125V4.125z" />
              </svg>
              Stats
            </NavLink>
          </nav>
        </div>
      </header>

      {/* Main Content */}
      <main className="max-w-6xl mx-auto px-4 py-6">
        <Routes>
          <Route path="/" element={<BlockList />} />
          <Route path="/block/:height" element={<BlockDetail />} />
          <Route path="/tx/:hash" element={<TransactionDetail />} />
          <Route path="/address/:addr" element={<AddressView />} />
          <Route path="/stats" element={<NetworkStats />} />
        </Routes>
      </main>

      {/* Footer */}
      <footer className="border-t border-gray-800/50 mt-12">
        <div className="max-w-6xl mx-auto px-4 py-6 flex flex-col sm:flex-row items-center justify-between gap-3">
          <p className="text-xs text-gray-600">
            NOVA Protocol Explorer &middot; Powered by NOVA{" "}
            {status?.version ? `v${status.version}` : ""}
          </p>
          <div className="flex items-center gap-4 text-xs text-gray-600">
            <span>
              Block Height: #{status?.block_height?.toLocaleString() ?? "..."}
            </span>
            <span>&middot;</span>
            <span>
              Peers: {status?.peer_count?.toLocaleString() ?? "..."}
            </span>
            <span>&middot;</span>
            <span>
              {status?.synced ? "Synced" : "Syncing..."}
            </span>
          </div>
        </div>
      </footer>
    </div>
  );
}
