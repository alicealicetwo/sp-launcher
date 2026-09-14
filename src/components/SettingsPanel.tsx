import type { Config, HostsStatus } from "../types";
import { pickInstallFolder } from "../lib/browse";

interface Props {
  config: Config;
  hosts: HostsStatus | null;
  onConfig: (patch: Partial<Config>) => void;
  onElevate: () => void;
  onHostsRefresh: () => void;
  onOpenFolder: () => void;
}

const TOGGLES: { key: keyof Config; name: string; hint: string }[] = [
  { key: "close_on_launch", name: "Minimize to tray on launch", hint: "Send the launcher to the tray once the game starts, instead of staying open" },
];

export function SettingsPanel({ config, hosts, onConfig, onElevate, onHostsRefresh, onOpenFolder }: Props) {
  const needsAdmin = config.hosts_redirect && hosts && !hosts.writable;

  async function browse() {
    const picked = await pickInstallFolder();
    if (picked) onConfig({ install_dir: picked });
  }

  return (
    <section className="panel settings is-active">
      <div className="card">
        <h2 className="card__title">Game Files</h2>

        <span className="field__hint" style={{ marginBottom: 10, display: "block" }}>
          Download the game using the guide in the #download-game channel.
        </span>

        <div className="field">
          <span className="field__label">Install location</span>
          <div className="field__row">
            <input
              className="input"
              spellCheck={false}
              value={config.install_dir}
              placeholder="Pick a folder…"
              onChange={(e) => onConfig({ install_dir: e.target.value })}
            />
            <button className="btn" type="button" onClick={() => void browse()}>Browse</button>
            <button className="btn" type="button" onClick={onOpenFolder}>Open folder</button>
          </div>
        </div>
      </div>

      <div className="card">
        <h2 className="card__title">Connection</h2>

        <div className="toggle">
          <span className="toggle__text">
            <span className="toggle__name">Hosts redirect</span>
            <span className="toggle__hint">
              Point the game's domains at your backend while it runs
            </span>
          </span>
          <button
            className={`switch${config.hosts_redirect ? " is-on" : ""}`}
            type="button"
            role="switch"
            aria-checked={config.hosts_redirect}
            aria-label="Hosts redirect"
            onClick={() => onConfig({ hosts_redirect: !config.hosts_redirect })}
          />
        </div>

        <div className="field" style={{ marginTop: 10 }}>
          <span className="field__label">Backend IP</span>
          <input
            className="input"
            spellCheck={false}
            value={config.backend_ip}
            placeholder="127.0.0.1"
            onChange={(e) => onConfig({ backend_ip: e.target.value })}
          />
        </div>

        <div className="field">
          <span className="field__label">Redirected domains</span>
          <textarea
            className="input args"
            spellCheck={false}
            value={config.hosts_domains.join("\n")}
            onChange={(e) =>
              onConfig({
                hosts_domains: e.target.value
                  .split("\n")
                  .map((s) => s.trim())
                  .filter(Boolean),
              })
            }
          />
        </div>

        <div className="field">
          <span className="field__label">Launch arguments</span>
          <textarea
            className="input args"
            spellCheck={false}
            value={config.launch_args}
            onChange={(e) => onConfig({ launch_args: e.target.value })}
          />
        </div>
      </div>

      <div className="card">
        <h2 className="card__title">Hosts file</h2>

        <div className="hoststat">
          <div className="hoststat__row">
            <span className="field__label">Status</span>
            <span className={`hoststat__pill${hosts?.applied ? " is-on" : ""}`}>
              {hosts?.applied ? "Applied" : "Not applied"}
            </span>
          </div>
          <div className="hoststat__row">
            <span className="field__label">Writable</span>
            <span className={`hoststat__pill${hosts?.writable ? " is-on" : " is-warn"}`}>
              {hosts ? (hosts.writable ? "Yes" : "Needs admin") : "—"}
            </span>
          </div>
          <div className="hoststat__path">{hosts?.path ?? "—"}</div>
        </div>

        {needsAdmin && (
          <div className="notice">
            <p>
              The hosts file can only be written by an administrator. Restart the
              launcher elevated, or turn the redirect off above.
            </p>
            <button className="btn btn--primary" type="button" onClick={onElevate}>
              Restart as administrator
            </button>
          </div>
        )}

        {hosts && hosts.conflicts.length > 0 && (
          <div className="notice notice--warn">
            <p>
              These lines elsewhere in the hosts file map the same names. Once
              the game launches, the launcher comments them out so its own
              Backend IP always wins, and restores them exactly when it exits:
            </p>
            <pre className="notice__code">{hosts.conflicts.join("\n")}</pre>
          </div>
        )}

        <div className="field__row" style={{ marginTop: 12 }}>
          <button className="btn" type="button" onClick={onHostsRefresh}>Refresh</button>
        </div>

        <h2 className="card__title" style={{ marginTop: 18 }}>Launcher</h2>
        {TOGGLES.map((t) => (
          <div className="toggle" key={t.key}>
            <span className="toggle__text">
              <span className="toggle__name">{t.name}</span>
              <span className="toggle__hint">{t.hint}</span>
            </span>
            <button
              className={`switch${config[t.key] ? " is-on" : ""}`}
              type="button"
              role="switch"
              aria-checked={Boolean(config[t.key])}
              aria-label={t.name}
              onClick={() => onConfig({ [t.key]: !config[t.key] } as Partial<Config>)}
            />
          </div>
        ))}
      </div>
    </section>
  );
}
