import type { ReactNode } from "react";
import { useEngineNotices, dismissNotice, clearNotices } from "../state";
import { playSound } from "../sound";

export function Notices() {
  const list = useEngineNotices();
  if (list.length === 0) return null;
  return (
    <div className="notices" aria-live="polite">
      <div className="notices-head">
        {list.length > 1 && <span className="notices-count">{list.length} messages</span>}
        <button
          className="notices-clear"
          onClick={() => {
            clearNotices();
            playSound("tick");
          }}
        >
          Clear all
        </button>
      </div>
      {list.map((n) => (
        <div key={n.id} className={`notice ${n.kind} ${n.fading ? "leaving" : ""}`}>
          <span>{n.text}</span>
          <button
            className="notice-close"
            onClick={() => {
              dismissNotice(n.id);
              playSound("tick");
            }}
            aria-label="Dismiss"
          >
            ×
          </button>
        </div>
      ))}
    </div>
  );
}

export function Card({
  title,
  children,
  className = "",
}: {
  title?: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section className={`card ${className}`}>
      {title && <h3 className="card-title">{title}</h3>}
      <div className="card-body">{children}</div>
    </section>
  );
}

export function StatusPill({ label, status }: { label: string; status: string }) {
  const cls = status === "pass" || status === "running" || status === "connected"
    ? "ok"
    : status === "warning" || status === "starting" || status === "reconnecting"
    ? "warn"
    : "bad";
  return (
    <span className={`status-pill ${cls}`}>
      <span className="dot" />
      {label}
    </span>
  );
}

export function Button({
  children,
  onClick,
  variant = "default",
  disabled,
}: {
  children: ReactNode;
  onClick?: () => void;
  variant?: "default" | "primary" | "danger" | "ghost";
  disabled?: boolean;
}) {
  return (
    <button className={`btn ${variant}`} onClick={onClick} disabled={disabled}>
      {children}
    </button>
  );
}
