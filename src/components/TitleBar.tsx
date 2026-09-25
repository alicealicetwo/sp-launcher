import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import logo from "../assets/sp-logo.png";
import type { Tab } from "../types";

const TABS: { id: Tab; label: string }[] = [
  { id: "play", label: "Play" },
  { id: "download", label: "Download" },
  { id: "settings", label: "Settings" },
];

interface Props {
  tab: Tab;
  onTab: (tab: Tab) => void;
}

export function TitleBar({ tab, onTab }: Props) {
  // Minimize behaves like a normal window: it drops to the taskbar, not the
  // tray. Only the X button (and Alt+F4, handled on the Rust side) sends the
  // window to the tray — the tray icon's "Open" item and the taskbar Quit
  // are the two ways back, and the tray also has a "Quit" for actually
  // ending the process.
  const minimize = () => void getCurrentWindow().minimize();
  const hideToTray = () => void invoke("hide_to_tray");

  return (
    <header className="topbar" data-tauri-drag-region>
      <div className="brand" data-tauri-drag-region>
        <img className="brand__logo" src={logo} alt="SP" draggable={false} />
      </div>

      <nav className="nav">
        {TABS.map((t) => (
          <button
            key={t.id}
            className={`tab${tab === t.id ? " is-active" : ""}`}
            onClick={() => onTab(t.id)}
            type="button"
          >
            {t.label}
          </button>
        ))}
      </nav>

      <ServerPill />

      <div className="winbtns">
        <button className="winbtn" title="Minimize" type="button" onClick={minimize}>
          <svg viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.4">
            <path d="M2 6h8" />
          </svg>
        </button>
        <button className="winbtn winbtn--close" title="Minimize to tray" type="button" onClick={hideToTray}>
          <svg viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.4">
            <path d="M2.5 2.5l7 7M9.5 2.5l-7 7" />
          </svg>
        </button>
      </div>
    </header>
  );
}

// Backend status next to the window buttons: green dot + "N playing" while the
// backend answers, red dot + "Offline" when it does not. Polled every 30 s; the
// Rust side does the request (the webview's CSP allows no outside hosts).
const STATUS_POLL_MS = 30_000;

function ServerPill() {
  const [st, setSt] = useState<{ online: boolean; players: number } | null>(null);

  useEffect(() => {
    let alive = true;
    const poll = () =>
      void invoke<{ online: boolean; players: number }>("server_status")
        .then((s) => alive && setSt(s))
        .catch(() => alive && setSt({ online: false, players: 0 }));
    poll();
    const id = window.setInterval(poll, STATUS_POLL_MS);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, []);

  if (!st) return null;
  return (
    <div className={`srvpill${st.online ? " is-online" : " is-offline"}`} title={st.online ? "Backend online" : "Backend offline"}>
      <span className="srvpill__dot" />
      <span className="srvpill__text">{st.online ? `${st.players} playing` : "Offline"}</span>
    </div>
  );
}
