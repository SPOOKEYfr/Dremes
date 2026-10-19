import type { View } from "../state";
import { playSound } from "../sound";

const NAV: { key: View; label: string; icon: string }[] = [
  { key: "dashboard", label: "Dashboard", icon: "◧" },
  { key: "gaming", label: "Gaming", icon: "▶" },
  { key: "streaming", label: "Streaming", icon: "▤" },
  { key: "waydroid", label: "Waydroid", icon: "▣" },
  { key: "capture", label: "Capture", icon: "◈" },
  { key: "wifi", label: "WiFi Booster", icon: "≋" },
  { key: "resources", label: "Resources", icon: "◫" },
  { key: "logs", label: "Logs", icon: "≣" },
  { key: "settings", label: "Settings", icon: "⚙" },
  { key: "about", label: "About", icon: "ℹ" },
];

export default function Sidebar({
  current,
  onNavigate,
}: {
  current: View;
  onNavigate: (v: View) => void;
}) {
  return (
    <aside className="sidebar">
      <div className="brand">
        <span className="brand-mark">
          <img src="/dremes-logo.png" alt="Dremes" />
        </span>
        <div className="brand-text">
          <strong>Dremes</strong>
          <small>Control Center</small>
        </div>
      </div>
      <nav className="nav">
        {NAV.map((item) => (
          <button
            key={item.key}
            className={`nav-item ${current === item.key ? "active" : ""}`}
            onMouseEnter={() => playSound("hover")}
            onClick={() => {
              playSound("page");
              onNavigate(item.key);
            }}
          >
            <span className="nav-icon">{item.icon}</span>
            <span>{item.label}</span>
          </button>
        ))}
      </nav>
      <div className="sidebar-foot">v0.2.0</div>
    </aside>
  );
}
