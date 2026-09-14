# SP Launcher

A desktop launcher for a private SUPER PEOPLE server. It points an existing
game install at your backend by redirecting the game's hostnames through a
scoped hosts-file entry, without touching Steam, DepotDownloader, or the
game's own files.

Built with [Tauri 2](https://tauri.app), React 19, and TypeScript on top of
a Rust backend.

## Features

- **Connect-to-server prompt.** Every launch asks for an `ip:port` to join,
  with a "Play without joining server" fallback — nothing is hardcoded or
  remembered between sessions.
- **Scoped hosts-file redirect.** Instead of hardcoding a backend into the
  game's config, the launcher can temporarily point the game's domains at
  your backend IP by editing the Windows hosts file, and restores it exactly
  when the game exits (including putting back any conflicting lines it had
  to comment out). This needs administrator rights only while the redirect
  is turned on; the launcher can relaunch itself elevated from Settings.
- **Install-folder detection.** No downloader is bundled — point the
  launcher at a folder that already has the game (copied from Steam, a
  previous install, wherever) and it verifies the expected executable and
  folders are present.
- **Configurable launch arguments**, editable from the Play tab.
- **System tray support.** Close-to-tray and minimize-to-tray behavior, with
  a tray icon and menu instead of the app fully quitting.
- **News carousel** on the Play tab for server announcements/events.
- Everything is stored in a single local config file — no accounts, no
  telemetry, no external services beyond the game server itself.

## Building

### Prerequisites

- [Rust](https://rustup.rs) (via rustup)
- Visual Studio Build Tools with the "Desktop development with C++"
  workload (Tauri links against it on Windows)
- [Node.js](https://nodejs.org)

### Setup

```powershell
git clone <this-repo-url>
cd sp-launcher
npm install
```

### Development

```powershell
npm run tauri dev
```

The first run compiles the full Rust dependency tree, which takes a few
minutes; after that it's fast, and the React frontend hot-reloads on save.
Rust changes trigger an automatic recompile.

### Production build

```powershell
npm run tauri build
```

This produces an optimized build of both the Rust backend and the frontend
and packages them into an NSIS installer at:

```
src-tauri/target/release/bundle/nsis/
```

That installer is what you hand to players — it installs the launcher for
the current user (no admin rights needed to install; the launcher itself
only asks for elevation later, and only if you turn on the hosts redirect
and the hosts file isn't already writable).

## How the hosts redirect works

The launcher reads a list of domains and a backend IP from Settings, and
when the redirect is turned on and the game is launched, it writes entries
for those domains into the Windows hosts file, pointing them at the backend
IP. Any existing lines in the hosts file that map the same hostnames are
commented out (not deleted) so the launcher's entry always wins, and are
restored exactly as they were once the game exits or the redirect is turned
off. Nothing is left behind if the launcher is killed unexpectedly — the
next launch reconciles the file.

## Project layout

```
src/                       React frontend
  components/               TitleBar, News, PlayPanel, SettingsPanel, LaunchArgs
  lib/                       small helpers (folder picker, formatting)
  types.ts                   mirrors the Rust structs
  news.ts                    filters news items by their active time window
  styles.css                 the whole design, one file
src-tauri/src/
  lib.rs                     Tauri commands + app state
  config.rs                  settings, atomic JSON writes
  game.rs                    locating and launching the game exe, argument parsing
  hosts.rs                   hosts-file redirect logic
  news.rs                    news feed shape
  download.rs, gateway.rs    legacy manifest-downloader / gateway code, currently unused by the UI
```

## Contributing

This project was built for a specific private server, so some assumptions
(executable name, expected install-folder layout, redirected domains) are
tailored to that game and are easiest to change in `src-tauri/src/game.rs`
and the default config in `src-tauri/src/config.rs`. Issues and PRs are
welcome.

## License

No license has been chosen yet for this repository. All rights reserved
unless a `LICENSE` file says otherwise.
