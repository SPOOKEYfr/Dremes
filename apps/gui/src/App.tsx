import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { applyTheme, type View } from "./state";
import { setSoundVolume } from "./sound";
import Sidebar from "./components/Sidebar";
import { Notices } from "./components/ui";
import Dashboard from "./pages/Dashboard";
import Gaming from "./pages/Gaming";
import Streaming from "./pages/Streaming";
import Waydroid from "./pages/Waydroid";
import Wifi from "./pages/Wifi";
import Capture from "./pages/Capture";
import Resources from "./pages/Resources";
import Logs from "./pages/Logs";
import SettingsPage from "./pages/SettingsPage";
import About from "./pages/About";

interface SavedSettings {
  theme?: string;
  sound_volume?: number;
}

export default function App() {
  const [view, setView] = useState<View>("dashboard");

  // Apply the persisted theme + sound volume on startup.
  useEffect(() => {
    invoke<SavedSettings>("get_settings")
      .then((s) => {
        applyTheme(s.theme);
        if (typeof s.sound_volume === "number") setSoundVolume(s.sound_volume);
      })
      .catch(() => applyTheme(undefined));
  }, []);

  return (
    <div className="app">
      <Sidebar current={view} onNavigate={setView} />
      <main className="content">
        <Notices />
        {view === "dashboard" && <Dashboard />}
        {view === "gaming" && <Gaming />}
        {view === "streaming" && <Streaming />}
        {view === "waydroid" && <Waydroid />}
        {view === "wifi" && <Wifi />}
        {view === "capture" && <Capture />}
        {view === "resources" && <Resources />}
        {view === "logs" && <Logs />}
        {view === "settings" && <SettingsPage />}
        {view === "about" && <About />}
      </main>
    </div>
  );
}
