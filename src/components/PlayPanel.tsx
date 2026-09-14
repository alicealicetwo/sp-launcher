import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { News } from "./News";
import { LaunchArgs } from "./LaunchArgs";
import type { NewsItem, Phase } from "../types";

const DISCORD_URL = "https://discord.gg/superpeopleofficial";

interface Props {
  news: NewsItem[];
  phase: Phase;
  launchArgs: string;
  busy: boolean;
  onLaunchArgs: (next: string) => void;
  onPrimary: () => void;
  onLaunch: (server: string | null) => void;
  onStop: () => void;
  onError: (message: string) => void;
}

// Loose on purpose: a hostname works as well as an IP, only the ":port" part
// is actually load-bearing for the "did they finish typing something" check.
const SERVER_PATTERN = /^[^\s:]+:\d{1,5}$/;

/** Text field + Play, plus the "skip it" fallback — asked fresh every launch
 * rather than remembered, since the server to join can change launch to
 * launch. */
function ConnectPlay({ onLaunch }: { onLaunch: (server: string | null) => void }) {
  const [server, setServer] = useState("");
  const canJoin = SERVER_PATTERN.test(server.trim());

  return (
    <div className="connect">
      <div className="connect__row">
        <input
          className="connect__input"
          spellCheck={false}
          placeholder="ip:port"
          value={server}
          onChange={(e) => setServer(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && canJoin) onLaunch(server.trim());
          }}
        />
        <button
          className="connect__play"
          type="button"
          disabled={!canJoin}
          onClick={() => onLaunch(server.trim())}
        >
          Play
        </button>
      </div>
      <button className="connect__skip" type="button" onClick={() => onLaunch(null)}>
        Play without joining server →
      </button>
    </div>
  );
}

export function PlayPanel(props: Props) {
  const { phase, busy } = props;

  return (
    <section className="panel play is-active">
      <News items={props.news} onError={props.onError} />

      <div className="action">
        <div className="action__left">
          <LaunchArgs value={props.launchArgs} onSave={props.onLaunchArgs} />

          {busy ? (
            <button className="cta cta--stop" type="button" onClick={props.onStop}>
              <span className="cta__inner">
                <span className="cta__label">Close Game</span>
                <span className="cta__icon">
                  <svg viewBox="0 0 14 14" fill="currentColor"><rect x="2.5" y="2.5" width="9" height="9" /></svg>
                </span>
              </span>
            </button>
          ) : phase === "not-installed" ? (
            <button className="cta" type="button" onClick={props.onPrimary}>
              <span className="cta__inner">
                <span className="cta__label">Install</span>
                <span className="cta__icon">
                  <svg viewBox="0 0 14 14" fill="currentColor"><path d="M3 1.5l9 5.5-9 5.5z" /></svg>
                </span>
              </span>
            </button>
          ) : (
            <ConnectPlay onLaunch={props.onLaunch} />
          )}
        </div>

        {/* opener plugin hands the URL to the default browser, not the webview.
            The href stays as a fallback for middle-click/"open in new tab", but
            the real navigation goes through openUrl so it never tries (and
            fails, silently, under the CSP) to load discord.gg inside the
            frameless window itself. */}
        <a
          className="social"
          href={DISCORD_URL}
          title="Join the Discord"
          onClick={(e) => {
            e.preventDefault();
            openUrl(DISCORD_URL).catch((err) => props.onError(`Could not open Discord: ${String(err)}`));
          }}
        >
          <span className="social__label">Community</span>
          <span className="discord">
            <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
              <path d="M20.317 4.369a19.79 19.79 0 0 0-4.885-1.515.074.074 0 0 0-.079.037c-.211.375-.445.865-.608 1.25a18.27 18.27 0 0 0-5.487 0 12.64 12.64 0 0 0-.617-1.25.077.077 0 0 0-.079-.037A19.736 19.736 0 0 0 3.677 4.37a.07.07 0 0 0-.032.027C.533 9.046-.32 13.58.099 18.057a.082.082 0 0 0 .031.057 19.9 19.9 0 0 0 5.993 3.03.078.078 0 0 0 .084-.028c.462-.63.874-1.295 1.226-1.994a.076.076 0 0 0-.041-.106 13.107 13.107 0 0 1-1.872-.892.077.077 0 0 1-.008-.128c.126-.094.252-.192.372-.291a.074.074 0 0 1 .077-.01c3.928 1.793 8.18 1.793 12.062 0a.074.074 0 0 1 .078.009c.12.099.246.198.373.292a.077.077 0 0 1-.006.127c-.598.35-1.22.644-1.873.891a.077.077 0 0 0-.041.107c.36.698.772 1.362 1.225 1.993a.076.076 0 0 0 .84.028 19.839 19.839 0 0 0 6.002-3.03.077.077 0 0 0 .032-.056c.5-5.177-.838-9.674-3.549-13.66a.061.061 0 0 0-.031-.028ZM8.02 15.331c-1.183 0-2.157-1.086-2.157-2.419 0-1.333.955-2.419 2.157-2.419 1.211 0 2.176 1.096 2.157 2.42 0 1.332-.955 2.418-2.157 2.418Zm7.975 0c-1.183 0-2.157-1.086-2.157-2.419 0-1.333.955-2.419 2.157-2.419 1.211 0 2.176 1.096 2.157 2.42 0 1.332-.946 2.418-2.157 2.418Z" />
            </svg>
          </span>
        </a>
      </div>
    </section>
  );
}
