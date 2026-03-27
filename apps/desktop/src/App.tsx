import { useState, useCallback } from "react";
import { BrowserRouter, Routes, Route } from "react-router-dom";
import { Layout } from "./components/Layout";
import { SetupOverlay } from "./components/SetupOverlay";
import { AgentsPage } from "./pages/AgentsPage";
import { MarketplacePage } from "./pages/MarketplacePage";
import { AboutPage } from "./pages/AboutPage";
import { HelpCenterPage } from "./pages/HelpCenterPage";
import { DocViewerPage } from "./pages/DocViewerPage";
import { ConfigPage } from "./pages/ConfigPage";
import { AgentDetailPage } from "./pages/AgentDetailPage";
import { WebShellPage } from "./pages/WebShellPage";
import { SettingsPage } from "./pages/SettingsPage";

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
            <Route path="/help" element={<HelpCenterPage />} />
            <Route path="/docs/:slug" element={<DocViewerPage />} />
            <Route path="/about" element={<AboutPage />} />
            <Route path="/settings" element={<SettingsPage />} />
            <Route path="/config/:id" element={<ConfigPage />} />
            <Route path="/agent/:id" element={<AgentDetailPage />} />
            <Route path="/shell/:id" element={<WebShellPage />} />
          </Route>
        </Routes>
      </BrowserRouter>
    </>
  );
}
