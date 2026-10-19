import { Card, Button } from "../components/ui";
import { useAppState } from "../state";
import { playSound } from "../sound";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";

function levelClass(level: string): string {
  const l = level.toLowerCase();
  if (l === "error" || l === "fatal") return "log-error";
  if (l === "warn" || l === "warning") return "log-warn";
  return "log-info";
}

export default function Logs() {
  const { logs, connected } = useAppState();

  const copyLogs = () => {
    playSound("tick");
    const text = logs
      .map((l) => `[${l.timestamp}] [${l.level.toUpperCase()}] [${l.component}] ${l.message}`)
      .join("\n");
    if (text) void navigator.clipboard?.writeText(text);
  };

  const exportLog = async () => {
    playSound("save");
    const text = logs
      .map((l) => `[${l.timestamp}] [${l.level.toUpperCase()}] [${l.component}] ${l.message}`)
      .join("\n");
    const stamp = new Date().toISOString().slice(0, 19).replace(/[T:]/g, "-");
    const filePath = await save({
      title: "Save Diagnostic Log",
      defaultPath: `dremes-logs-${stamp}.txt`,
      filters: [{ name: "Log file", extensions: ["txt"] }],
    });
    if (!filePath) return;
    try {
      await invoke("write_text_file", { path: filePath, contents: text || "No logs.\n" });
      alert(`Log saved to:\n${filePath}`);
    } catch (e) {
      alert(String(e));
    }
  };

  return (
    <div className="page">
      <header className="page-head">
        <h1>Logs</h1>
        <p>Application and system events.</p>
      </header>
      <Card>
        <div className="log-toolbar">
          <Button variant="ghost" onClick={copyLogs} disabled={logs.length === 0}>
            Copy
          </Button>
          <Button variant="ghost" onClick={exportLog}>
            Export Diagnostic Log
          </Button>
        </div>
        <div className="log-panel">
          {!connected && logs.length === 0 ? (
            <div className="log-line muted">Engine offline — no logs available.</div>
          ) : logs.length === 0 ? (
            <div className="log-line muted">No logs yet.</div>
          ) : (
            [...logs].reverse().map((l, i) => (
              <div key={i} className={`log-line ${levelClass(l.level)}`}>
                <span className="log-time">{l.timestamp}</span>{" "}
                <span className="log-lvl">{l.level.toUpperCase()}</span>{" "}
                <span className="log-comp">[{l.component}]</span> {l.message}
              </div>
            ))
          )}
        </div>
      </Card>
    </div>
  );
}