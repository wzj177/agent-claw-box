import { useState, useCallback } from "react";
import { BrowserRouter, Routes, Route } from "react-router-dom";
import { Layout } from "./components/Layout";
import { SetupOverlay } from "./components/SetupOverlay";
import { AgentsPage } from "./pages/AgentsPage";
import { MarketplacePage } from "./pages/MarketplacePage";
import { ConfigPage } from "./pages/ConfigPage";
import { AgentDetailPage } from "./pages/AgentDetailPage";
import { WebShellPage } from "./pages/WebShellPage";

export default function App() {
  const [ready, setReady] = useState(false);
  const handleReady = useCallback(() => setReady(true), []);

  return (
    <>
      {!ready && <SetupOverlay onReady={handleReady} />}
      <BrowserRouter>
        <Routes>
          <Route element={<Layout />}>
            <Route path="/" element={<AgentsPage />} />
            <Route path="/marketplace" element={<MarketplacePage />} />
            <Route path="/config/:id" element={<ConfigPage />} />
            <Route path="/agent/:id" element={<AgentDetailPage />} />
            <Route path="/shell/:id" element={<WebShellPage />} />
          </Route>
        </Routes>
      </BrowserRouter>
    </>
  );
}
