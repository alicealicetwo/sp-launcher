# Running & building SP Launcher

Quick reference for working on the launcher day-to-day, and for producing an
installer to hand to players. All commands run in PowerShell from the
project folder:

```powershell
cd path\to\sp-launcher
```

## First-time setup

You need Rust and the C++ build tools once per machine:

1. Install Rust via [rustup](https://rustup.rs).
2. Install Visual Studio Build Tools with the "Desktop development with
   C++" workload (Tauri links against it on Windows).
3. Install the Node dependencies:

   ```powershell
   npm install
   ```

## Running it while developing

```powershell
npm run tauri dev
```

The first run compiles the whole Rust dependency tree, which takes a few
minutes. After that it's fast, and the React frontend hot-reloads on save —
Rust changes still need a recompile, which `tauri dev` does automatically
when you save a `.rs` file.

## Building the installer

```powershell
npm run tauri build
```

This produces a release build of both the Rust backend and the frontend,
then packages them. The NSIS installer lands in:

```
src-tauri\target\release\bundle\nsis\
```

That `.exe` is the one thing you hand to players — it installs the
launcher for the current user (no admin needed to install; the launcher
itself will ask for elevation later, but only if you turn on the hosts
redirect and it's not writable).

A release build takes noticeably longer than `tauri dev` (optimized Rust
codegen). Only run it when you actually need an installer to distribute or
test the packaged app — use `tauri dev` for everyday iteration.

## Changing the app icon

If you swap `src-tauri/icons/icon.png` (the 512×512 source) for a new
design, regenerate every derived size from it — don't hand-edit
`icon.ico` or the individual PNGs, they're generated. See
`src-tauri/icons/` for what's currently there; ask for the icon to be
rebuilt from a new source if you change the artwork.

## Getting the game files hosted, downloadable, and verified (legacy/unused)

**This section describes a manifest-based downloader that still exists in
the Rust backend (`download.rs`, `start_download`/`cancel_download`
commands, `tools/make-manifest.mjs`, `tools/serve-files.mjs`) but is no
longer wired into the UI.** The Download tab was removed — players are now
expected to already have a game install and point the launcher at it from
Settings → Game Files (Install location / Browse / Open folder). The notes
below are kept for whoever wants to resurrect in-launcher downloading later;
skip this section for day-to-day building.

The downloader never talks to Steam or DepotDownloader — it only ever
downloads plain files over HTTPS from a host you control, checking each
one's SHA-256 against a manifest you generate once. Here is the sequence it
was designed around:

1. **Get the install files once**, however you already do (DepotDownloader,
   a manual copy, whatever). End state: a normal folder on disk that looks
   like a real SUPER PEOPLE install, e.g. `C:\Games\SUPER PEOPLE`.

2. **Generate the manifest** — this is what makes files "verifiable": it
   walks the folder and writes a SHA-256 hash for every file.

   ```powershell
   node tools/make-manifest.mjs `
     --dir "C:/Games/SUPER PEOPLE" `
     --base-url "https://files.example.com/sp/1.3.0.0" `
     --version 1.3.0.0 `
     --out manifest.json
   ```

   `--base-url` must be the URL the folder will actually be reachable at
   once uploaded — the manifest bakes a full download URL into every file
   entry by joining `--base-url` with that file's relative path. Hashing
   ~28 GB takes a few minutes; it runs 8 files at a time.

3. **Pick a host and upload the folder, keeping the same layout.** It
   **must** support HTTP range requests or resume silently turns into
   "start over on every drop". Object storage with cheap/free egress
   (Cloudflare R2, Backblaze B2 + Bunny) is worth it over a plain VPS at
   28 GB/player — a normal nginx/Apache static host also works fine. Check
   range support before trusting it:

   ```powershell
   curl.exe -I -H "Range: bytes=0-1023" https://files.example.com/sp/1.3.0.0/some.pak
   ```

   You want `206 Partial Content` back. `200 OK` means no range support —
   don't use that host.

4. **Upload `manifest.json` itself too**, anywhere reachable (can be the
   same host, doesn't have to sit inside the game folder).

5. **Point the launcher at it**: Settings → Manifest URL → the
   `manifest.json` URL from step 4. That's the only launcher-side
   configuration needed — `download_threads` (also in Settings) controls
   how many files it pulls in parallel, default 4.

That's it — verification isn't a separate step you run. Every file's hash
is checked as it streams in (it's only renamed into place on a match), and
re-running a download or turning on **Verify before launch** in Settings
re-checks whatever's already on disk, so a player re-downloading after an
interruption is also implicitly a repair pass.

**Test this without hosting anything first.** Serve your own game folder
locally and point a *second, empty* install folder at it:

```powershell
# terminal 1
node tools/serve-files.mjs --dir "C:/Games/SUPER PEOPLE" --port 8080

# terminal 2 — small manifest, seconds instead of an evening
node tools/make-manifest.mjs `
  --dir "C:/Games/SUPER PEOPLE" `
  --base-url "http://localhost:8080" `
  --limit 20 --max-size 50MB `
  --out public/manifest.test.json
```

Put `manifest.test.json` inside the served folder, set Settings → Manifest
URL to `http://localhost:8080/manifest.test.json`, pick a different install
folder in the Download tab, hit Download. Worth breaking on purpose:

| To test | Do this |
|---|---|
| Resume | Ctrl-C the server mid-download, restart it, download again — the `.part` picks up where it left off. |
| No range support | `node tools/serve-files.mjs --dir ... --no-range` — launcher should restart the file cleanly, not corrupt it. |
| Cancel | `--throttle 2` to slow it to 2 MB/s, then hit Cancel. |
| Corrupt file caught | Edit one `sha256` in the manifest — the run must fail and leave nothing behind. |

Drop `--limit`/`--max-size` and swap in the real `--base-url` when you're
ready to go live with the actual 28 GB.

## Troubleshooting

- **Taskbar/title bar icon still looks low-res after a rebuild**: two
  separate things have to happen, in order:

  1. **The app actually has to be rebuilt.** Editing the icon files on
     disk does nothing to an `.exe` that was already built — it has the
     old icon baked into it until you rebuild. Cargo also doesn't always
     notice that only an icon *asset* changed (no `.rs` file changed), so
     the safest way to force it to re-embed the icon is a clean rebuild:

     ```powershell
     cd src-tauri
     cargo clean
     cd ..
     npm run tauri build
     ```

     (Slower than an incremental build, but guarantees the new icon is
     actually linked into the new `.exe`, not just present in `icons/`.)

  2. **Pinned taskbar shortcuts cache their own icon bitmap**, separate
     from the icon cache and separate from the `.exe` resource — logging
     out/in does not touch this. Unpin the launcher from the taskbar,
     install/run the freshly rebuilt version, and pin it again.

  If it's still stale after both of those, force the system icon cache
  itself to rebuild:

  ```powershell
  taskkill /f /im explorer.exe
  Remove-Item "$env:LOCALAPPDATA\IconCache.db" -Force -ErrorAction SilentlyContinue
  Remove-Item "$env:LOCALAPPDATA\Microsoft\Windows\Explorer\iconcache*" -Force -ErrorAction SilentlyContinue
  Start-Process explorer.exe
  ```

- **`cargo` not found**: the Rust install didn't add itself to PATH for the
  current shell — open a new PowerShell window (or restart) after
  installing rustup.
- **A `link.exe` / MSVC error during build**: the C++ Build Tools
  workload above is missing or incomplete.
