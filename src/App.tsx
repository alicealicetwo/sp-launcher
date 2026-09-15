import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import type { Update } from "@tauri-apps/plugin-updater";

import { TitleBar } from "./components/TitleBar";
import { PlayPanel } from "./components/PlayPanel";
import { SettingsPanel } from "./components/SettingsPanel";
import { activeNews } from "./news";
import { pickInstallFolder } from "./lib/browse";
import { checkForUpdate, installUpdate } from "./lib/updater";
import type { Config, HostsStatus, InstallState, NewsItem, Phase, Tab } from "./types";

// Re-check which items are in their [starts_at, ends_at) window every so
// often, so an event that just started (or just ended) updates without the
// user having to reopen the launcher.
const NEWS_REFRESH_MS = 10 * 60 * 1000;

export default function App() {
  const [tab, setTab] = useState<Tab>("play");
  const [config, setConfig] = useState<Config | null>(null);
  const [install, setInstall] = useState<InstallState>({ installed: false, exe_path: null });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [hosts, setHosts] = useState<HostsStatus | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [news, setNews] = useState<NewsItem[]>([]);

  const [appVersion, setAppVersion] = useState("");
  const [update, setUpdate] = useState<Update | null>(null);
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [updateChecked, setUpdateChecked] = useState(false);
  const [installingUpdate, setInstallingUpdate] = useState(false);
  const [updateProgress, setUpdateProgress] = useState<{ done: number; total: number | null } | null>(null);

  // Debounce config writes: the settings fields fire on every keystroke and
  // there is no reason to hit the disk that often.
  const saveTimer = useRef<number | null>(null);

  useEffect(() => {
    void (async () => {
      // Nothing below may throw uncaught: `config` staying null renders an
      // empty window forever, which looks exactly like the app not starting.
      let cfg: Config;
      try {
        cfg = await invoke<Config>("get_config");
      } catch (e) {
        setError(`Could not load settings: ${String(e)}`);
        return;
      }

      // There's no manifest to download from, so a folder with the game
      // already in it is the only way to get going — ask for it right away
      // rather than leaving the user to find the Install button on their own.
      // A picker that fails or is dismissed must not hold up the UI.
      let effective = cfg;
      if (!cfg.install_dir) {
        const dir = await pickInstallFolder().catch(() => null);
        if (dir) {
          effective = { ...cfg, install_dir: dir };
          await invoke("set_config", { cfg: effective }).catch(() => {});
        }
      }

      setConfig(effective);
      await invoke<InstallState>("install_state").then(setInstall).catch(() => {});
      await invoke<HostsStatus>("hosts_status").then(setHosts).catch(() => {});
    })();
  }, []);

  useEffect(() => {
    // A broken, unset, or empty news feed should never block the rest of the
    // UI — just show nothing rather than a placeholder.
    const refresh = () =>
      void invoke<NewsItem[]>("fetch_news")
        .then((items) => setNews(items))
        .catch(() => setNews([]));
    refresh();
    const id = window.setInterval(refresh, NEWS_REFRESH_MS);
    return () => window.clearInterval(id);
  }, []);

  useEffect(() => {
    void getVersion().then(setAppVersion);
  }, []);

  const runUpdateCheck = useCallback(() => {
    setCheckingUpdate(true);
    void checkForUpdate()
      .then((u) => {
        setUpdate(u);
        setUpdateChecked(true);
      })
      .finally(() => setCheckingUpdate(false));
  }, []);

  // One check shortly after startup — quiet on failure (see checkForUpdate),
  // so a broken/unreachable update endpoint never surfaces as an error toast.
  useEffect(() => {
    runUpdateCheck();
  }, [runUpdateCheck]);

  const runUpdateInstall = useCallback(() => {
    if (!update) return;
    setInstallingUpdate(true);
    setUpdateProgress(null);
    void installUpdate(update, (done, total) => setUpdateProgress({ done, total })).catch((e) => {
      // A successful run typically exits the process itself (see
      // lib/updater.ts) before this ever runs — only a genuine failure
      // reaches here.
      setInstallingUpdate(false);
      setError(`Update failed: ${String(e)}`);
    });
  }, [update]);

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

  // Only relevant while not installed: there's no in-launcher downloader, so
  // this just asks for the folder the game is already installed in. Actually
  // starting the game is `onLaunch` below.
  const onPrimary = useCallback(() => {
    if (!config) return;
    if (!config.install_dir) {
      void pickInstallFolder().then((dir) => {
        if (!dir) return;
        patchConfig({ install_dir: dir });
      });
    } else {
      setError(
        "This folder doesn't look right — it needs to contain BravoHotelGame, Engine and BravoHotelClient.exe."
      );
    }
  }, [config, patchConfig]);

  // `server` is `ip:port` from the connect prompt, or null for "Play without
  // joining server" — asked fresh every launch, but remembered (via
  // `last_server`) so the field is pre-filled next time instead of blank.
  const onLaunch = useCallback(
    (server: string | null) => {
      if (server) patchConfig({ last_server: server });
      setBusy(true);
      setError(null);
      void invoke("launch_game", { server })
        .then(() => invoke<HostsStatus>("hosts_status").then(setHosts))
        .catch((e) => {
          setError(String(e));
          setBusy(false);
        });
      // `busy` is cleared by the game:exited event, not here: the launcher
      // stays in the launched state for as long as the game is up.
    },
    [patchConfig]
  );

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
        {error && (
          <div className="toast" role="alert">
            {error}
          </div>
        )}
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
            lastServer={config.last_server}
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
            appVersion={appVersion}
            update={update}
            checkingUpdate={checkingUpdate}
            updateChecked={updateChecked}
            onCheckUpdate={runUpdateCheck}
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

      {!error && !notice && update && (
        <div className="toast toast--info" role="status">
          {installingUpdate ? (
            <span>
              Installing v{update.version}
              {updateProgress?.total
                ? ` — ${Math.min(100, Math.round((updateProgress.done / updateProgress.total) * 100))}%`
                : "…"}
            </span>
          ) : (
            <span className="field__row" style={{ alignItems: "center" }}>
              <span style={{ marginRight: 10 }}>Update available: v{update.version}</span>
              <button className="btn btn--primary" type="button" onClick={runUpdateInstall}>
                Install &amp; Restart
              </button>
              <button className="btn" type="button" onClick={() => setUpdate(null)}>
                Later
              </button>
            </span>
          )}
        </div>
      )}
    </div>
  );
}
