import { Card } from "../components/ui";
import { useAppState } from "../state";

export default function About() {
  const { version, connected } = useAppState();
  const INFO: { label: string; value: string }[] = [
    { label: "Version", value: version },
    { label: "Technology", value: "Tauri • React • TypeScript • Rust" },
    { label: "Platforms", value: "Linux" },
  ];
  return (
    <div className="page">
      <header className="page-head">
        <h1>About</h1>
      </header>

      <div className="about-hero card">
        <div className="about-mark">
          <img src="/dremes-logo.png" alt="Dremes" />
        </div>
        <h2 className="about-title">Dremes</h2>
        <p className="about-sub">Waydroid &amp; Streaming Control Center</p>
        <p className="about-desc">
          A modern desktop application for managing Waydroid gaming, direct capture, OBS
          integration, streaming resources, and recovery from one unified interface.
        </p>
      </div>

      <div className="about-grid">
        <Card className="about-creator" title="Created By">
          <div className="about-profile">
            <div className="about-avatar-wrap">
              <img src="/profile.png" alt="SPOOKEYfr" className="about-avatar" />
            </div>
            <div className="about-name">
              <strong>SPOOKEYfr</strong>
              <span>Creator &amp; Developer</span>
            </div>
          </div>
        </Card>

        <Card className="about-details" title="Information">
          <dl className="about-info">
            {INFO.map((row) => (
              <div className="about-info-row" key={row.label}>
                <dt>{row.label}</dt>
                <dd>{row.value}</dd>
              </div>
            ))}
          </dl>
          <p className="about-copyright">© 2026 SPOOKEYfr · GPL-3.0-or-later</p>
        </Card>
      </div>

      <div className="about-engine">
        {connected ? (
          <span className="about-engine-pill ok">
            <span className="dot" /> Engine daemon: ONLINE
          </span>
        ) : (
          <span className="about-engine-pill bad">
            <span className="dot" /> Engine daemon: OFFLINE
          </span>
        )}
        <span className="about-engine-hint">Direct Waydroid → scrcpy → V4L2 → OBS capture.</span>
      </div>
    </div>
  );
}
