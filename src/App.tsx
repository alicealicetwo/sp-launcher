import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { TitleBar } from "./components/TitleBar";
import { PlayPanel } from "./components/PlayPanel";
import { SettingsPanel } from "./components/SettingsPanel";
import { activeNews, HARDCODED_NEWS } from "./news";
import { pickInstallFolder } from "./lib/browse";
import type { Config, HostsStatus, InstallState, NewsItem, Phase, Tab } from "./types";

// Re-check which items are in their [starts_at, ends_at) window every so
// often, so an event that just started (or just ended) updates without the
// user having to reopen the launcher. Unused while news is hardcoded (see
// news.ts) — goes back to use once the fetch effect below is re-enabled.
// const NEWS_REFRESH_MS = 10 * 60 * 1000;

export default function App() {
  const [tab, setTab] = useState<Tab>("play");
  const [config, setConfig] = useState<Config | null>(null);
  const [install, setInstall] = useState<InstallState>({ installed: false, exe_path: null });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [hosts, setHosts] = useState<HostsStatus | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  // No setter while news is hardcoded — `setNews` comes back once the fetch
  // effect below is re-enabled.
  const [news] = useState<NewsItem[]>(HARDCODED_NEWS);

  // Debounce config writes: the settings fields fire on every keystroke and
  // there is no reason to hit the disk that often.
  const saveTimer = useRef<number | null>(null);

  useEffect(() => {
    void (async () => {
      const cfg = await invoke<Config>("get_config");
      let effective = cfg;

      // There's no manifest to download from, so a folder with the game
      // already in it is the only way to get going — ask for it right away
      // rather than leaving the user to find the Install button on their own.
      if (!cfg.install_dir) {
        const dir = await pickInstallFolder();
        if (dir) {
          effective = { ...cfg, install_dir: dir };
          await invoke("set_config", { cfg: effective }).catch(() => {});
        }
      }

      setConfig(effective);
      setInstall(await invoke<InstallState>("install_state"));
      setHosts(await invoke<HostsStatus>("hosts_status"));
    })();
  }, []);

  // News is hardcoded for now (see news.ts) instead of coming from
  // `news_url`, so this effect is disabled rather than deleted — flip it back
  // on once there's a real feed to point at.
  //
  // useEffect(() => {
  //   // A broken or unset news feed should never block the rest of the UI —
  //   // fall back to an empty list rather than surfacing a toast for it.
  //   const refresh = () => void invoke<NewsItem[]>("fetch_news").then(setNews).catch(() => setNews([]));
  //   refresh();
  //   const id = window.setInterval(refresh, NEWS_REFRESH_MS);
  //   return () => window.clearInterval(id);
  // }, []);

  useEffect(() => {
    const unlisten: Promise<() => void>[] = [
      // The game exited: the redirect is gone, so re-read the hosts state.
      listen<number | null>("game:exited", () => {
        setBusy(false);
        void invoke<HostsStatus>("hosts_status").then(setHosts);
      }),
      listen<string>("hosts:recovered", (e) => setNotice(e.payload)),
      listen<string>("hosts:error", (e) => setError(e.payload)),
    ];
    return () => {
      unlisten.forEach((p) => void p.then((off) => off()));
    };
  }, []);

  const patchConfig = useCallback((patch: Partial<Config>) => {
    setConfig((prev) => {
      if (!prev) return prev;
      const next = { ...prev, ...patch };
      if (saveTimer.current) window.clearTimeout(saveTimer.current);
      saveTimer.current = window.setTimeout(() => {
        void invoke("set_config", { cfg: next })
          .then(() => invoke<InstallState>("install_state").then(setInstall))
          .catch((e) => setError(String(e)));
      }, 250);
      return next;
    });
  }, []);

  // Only relevant while not installed: prompts for (or complains about) the
  // install folder. Actually starting the game is `onLaunch` below.
  const onPrimary = useCallback(() => {
    if (!config) return;
    if (!config.install_dir) {
      // There's no manifest to download from — the only way to get going
      // is pointing the launcher at a folder that already has the game.
      void pickInstallFolder().then((dir) => {
        if (dir) patchConfig({ install_dir: dir });
      });
    } else {
      setError(
        "This folder doesn't look right — it needs to contain BravoHotelGame, Engine and BravoHotelClient.exe."
      );
    }
  }, [config, patchConfig]);

  // `server` is `ip:port` from the connect prompt, or null for "Play without
  // joining server" — asked fresh every launch rather than remembered.
  const onLaunch = useCallback((server: string | null) => {
    setBusy(true);
    setError(null);
    void invoke("launch_game", { server })
      .then(() => invoke<HostsStatus>("hosts_status").then(setHosts))
      .catch((e) => {
        setError(String(e));
        setBusy(false);
      });
    // `busy` is cleared by the game:exited event, not here: the launcher stays
    // in the launched state for as long as the game is up.
  }, []);

  const stopGame = useCallback(() => {
    void invoke("stop_game").catch((e) => setError(String(e)));
    // Same as onPrimary: `busy` clears on the game:exited event once the
    // process actually dies, not here.
  }, []);

  const refreshHosts = useCallback(() => {
    void invoke<HostsStatus>("hosts_status").then(setHosts).catch((e) => setError(String(e)));
  }, []);

  if (!config) {
    return (
      <div className="app">
        <div className="bg" />
      </div>
    );
  }

  const phase: Phase = install.installed ? "ready" : "not-installed";

  return (
    <div className="app">
      <div className="bg" />
      <div className="bg__slabs">
        <span className="slab slab--mint" />
        <span className="slab slab--coral" />
        <span className="slab slab--coral-2" />
        <span className="slab slab--sliver" />
      </div>
      <div className="bg__grade" />
      <div className="bg__vignette" />
      <div className="bg__grain" />

      <TitleBar tab={tab} onTab={setTab} />

      <main className="stage">
        {tab === "play" && (
          <PlayPanel
            news={activeNews(news)}
            phase={phase}
            launchArgs={config.launch_args}
            busy={busy}
            onLaunchArgs={(launch_args) => patchConfig({ launch_args })}
            onPrimary={onPrimary}
            onLaunch={onLaunch}
            onStop={stopGame}
            onError={setError}
          />
        )}

        {tab === "settings" && (
          <SettingsPanel
            config={config}
            hosts={hosts}
            onConfig={(patch) => {
              patchConfig(patch);
              // domain/IP edits change what the status means
              window.setTimeout(refreshHosts, 350);
            }}
            onElevate={() => void invoke("relaunch_elevated").catch((e) => setError(String(e)))}
            onHostsRefresh={refreshHosts}
            onOpenFolder={() => void invoke("open_install_dir").catch((e) => setError(String(e)))}
          />
        )}
      </main>

      {error && (
        <div className="toast" role="alert" onClick={() => setError(null)}>
          {error}
        </div>
      )}

      {!error && notice && (
        <div className="toast toast--info" onClick={() => setNotice(null)}>
          {notice}
        </div>
      )}
    </div>
  );
}
