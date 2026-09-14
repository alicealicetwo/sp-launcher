import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import logo from "../assets/sp-logo.png";
import type { Tab } from "../types";

const TABS: { id: Tab; label: string }[] = [
  { id: "play", label: "Play" },
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
