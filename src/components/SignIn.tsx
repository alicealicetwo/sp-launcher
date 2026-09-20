import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";

const DISCORD_URL = "https://discord.gg/superpeopleofficial";

interface Props {
  busy: boolean;
  error: string | null;
  onRedeem: (key: string) => void;
}

/** Looks like a key once enough characters are there. Deliberately loose —
 * the backend decides what is valid, this only stops the button being live
 * over an obviously-unfinished entry. */
function looksLikeKey(raw: string): boolean {
  const cleaned = raw.replace(/[^0-9a-zA-Z]/g, "").toUpperCase();
  return cleaned.length === 14 && cleaned.startsWith("SP");
}

/** Formats as the player types: sp ab12cd34ef56 -> SP-AB12-CD34-EF56.
 * Cheaper than telling someone off for pasting it in the wrong shape. */
function pretty(raw: string): string {
  const cleaned = raw.replace(/[^0-9a-zA-Z]/g, "").toUpperCase().slice(0, 14);
  if (!cleaned.startsWith("SP")) return cleaned;
  const body = cleaned.slice(2);
  const groups = [body.slice(0, 4), body.slice(4, 8), body.slice(8, 12)].filter(Boolean);
  return groups.length ? `SP-${groups.join("-")}` : cleaned;
}

/** The gate in front of Play. Everything else in the launcher stays reachable
 * — settings, news — because someone whose key is suspended still needs to be
 * able to read why and find the Discord link. */
export function SignIn({ busy, error, onRedeem }: Props) {
  const [key, setKey] = useState("");
  const ready = looksLikeKey(key) && !busy;

  return (
    <div className="signin">
      <div className="signin__card">
        <h2 className="signin__title">Sign in</h2>
        <p className="signin__lead">
          Enter your launcher key. You only do this once — the launcher stays signed in
          afterwards, and the game signs in with it.
        </p>

        <form
          className="signin__form"
          onSubmit={(e) => {
            e.preventDefault();
            if (ready) onRedeem(key);
          }}
        >
          <input
            className="signin__input"
            value={key}
            onChange={(e) => setKey(pretty(e.target.value))}
            placeholder="SP-XXXX-XXXX-XXXX"
            spellCheck={false}
            autoComplete="off"
            autoFocus
            aria-label="Launcher key"
          />
          <button className="signin__submit" type="submit" disabled={!ready}>
            {busy ? "Checking…" : "Sign in"}
          </button>
        </form>

        {error && (
          <p className="signin__error" role="alert">
            {error}
          </p>
        )}

        <p className="signin__hint">
          No key yet? Run{" "}
          <code>/authkey</code> in{" "}
          <button
            type="button"
            className="signin__link"
            onClick={() => void openUrl(DISCORD_URL).catch(() => {})}
          >
            Discord
          </button>
          . Keep it to yourself — anyone who has it can play as you.
        </p>
      </div>
    </div>
  );
}
