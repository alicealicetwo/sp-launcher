# Shipping updates through the auto-updater

This is the reference for the update system that's already wired into the
launcher (Tauri's official updater plugin). It covers what's already set up,
and the steps to run every time you want to push a new version to players.

## How it actually works

Every time the launcher starts, it fetches a small JSON file — `latest.json`
— from a URL baked into the app at build time (see "One-time setup" below).
That file says what the newest version is, where to download the installer,
and carries a cryptographic signature of that installer.

The launcher compares `latest.json`'s version against its own. If it's
newer, it shows a toast on the Play screen: "Update available: vX.Y.Z" with
an **Install & Restart** button. Settings also has a **Check for updates**
button for a manual check, plus the current version number.

When someone clicks Install & Restart, the launcher downloads the new
installer and verifies its signature against the public key that was baked
into the app when *that copy* of the launcher was built. If the signature
doesn't match — file got corrupted, `latest.json` got tampered with, wrong
key — it refuses and shows an error instead of running something unverified.
This is why the server hosting `latest.json` doesn't need to be trusted with
HTTPS or anything special: the signature is what actually protects players,
not the transport.

Once verified, it launches the new NSIS installer, which reinstalls over the
current install and restarts the app.

## One-time setup (already done)

This part is finished — it's here so you know what exists and where, not
because you need to redo it.

- **Signing keypair**: `%USERPROFILE%\.tauri\sp-launcher-2.key` (private —
  never shared, never committed) and `sp-launcher-2.key.pub` (public, on the
  same machine, not sensitive but not needed after step below).

  > **Rotated 20.09.2026.** The original `sp-launcher.key` had a password
  > that was lost, so it could no longer sign anything. The old pair is still
  > in `.tauri` in case the password ever resurfaces, but it is dead for
  > practical purposes. The replacement has NO password: the file itself is
  > the secret, and an unprotected key you still have beats a protected one
  > you cannot open. **Back it up** — a password manager entry, not the repo
  > and not the VPS.
  >
  > Consequence: every launcher built before 0.2.7 carries the OLD public key
  > and rejects anything signed with the new one. Those installs had to be
  > updated by hand once. From 0.2.7 onward the updater works normally again.
- **Public key** is baked into `src-tauri/tauri.conf.json` under
  `plugins.updater.pubkey`. It's what every future build ships with, so it
  only needs to be set once — until you deliberately rotate keys (see
  "Losing the private key" below).
- **Update endpoint**: also in `tauri.conf.json`, under
  `plugins.updater.endpoints` —
  `http://64.226.112.204/launcher/updates/latest.json`. The launcher polls
  exactly this URL on every startup.
- **Server folder**: `/var/www/html/launcher/updates/` on the Ubuntu box,
  served by nginx at `http://64.226.112.204/launcher/updates/`.

If any of these three ever need to change (new server, new domain, rotated
key), the app has to be rebuilt and redistributed — they're compiled into
the binary, not read from a config file at runtime, since letting the
update source be user-editable would defeat the point of signing.

## Releasing a new version — do this every time

> `powershell -ExecutionPolicy Bypass -File tools\publish-update.ps1` runs
> steps 1b to 3 for you, asking for the key password up front (Tauri's own
> mid-build prompt fails under a script) and refusing to build if the DLL or
> the key is missing. The manual steps below remain the reference.

**1. Bump the version number.** Keep these three in step (they don't have
to match by a hard requirement, but it avoids confusion):
- `src-tauri/tauri.conf.json` → `"version"`
- `src-tauri/Cargo.toml` → `[package] version`
- `package.json` → `"version"`

**1b. Make sure the no-Steam DLL is bundled.** `src-tauri/resources/XAPOFX1_5.dll`
has to exist, or the launcher ships without it — and since the launcher passes
`-ServicePlatform=`, every player who does not already have that DLL by hand
gets a game that never leaves the loading screen.

```powershell
cd ..\sp-listen-patch
.\build_sp_proxy.bat
copy dist\XAPOFX1_5.dll ..\sp-launcher\src-tauri\resources\
```

`build.rs` warns when it is missing and the launcher logs `this launcher has
no DLL bundled` at launch; `tools\publish-update.ps1` refuses to build at all.

**1c. Build the separate client fixes DLL.** The Settings toggle loads
`src-tauri/resources/SPClientFixes.dll`; it does not affect the no-Steam
proxy. Follow [client-fixes/README.md](client-fixes/README.md) to build and
copy it. The release script refuses to publish without this payload.

**2. Build with signing enabled.** The private key has to be available as an
environment variable during the build — this is what makes `tauri build` sign
the installer automatically instead of just building it. The current key
(`sp-launcher-2.key`, since 20.09.2026) has no password:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content "$env:USERPROFILE\.tauri\sp-launcher-2.key" -Raw
npm run tauri build
```

This produces, in `src-tauri\target\release\bundle\nsis\`:
- `SP Launcher_X.Y.Z_x64-setup.exe` — the installer
- `SP Launcher_X.Y.Z_x64-setup.exe.sig` — its signature (plain text, base64)

If you forget to set the env vars, the build still succeeds but produces no
`.sig` file — that's the tell that signing didn't happen.

**3. Generate `latest.json`:**

```powershell
node tools/make-update-manifest.mjs `
  --version X.Y.Z `
  --sig "src-tauri\target\release\bundle\nsis\SP Launcher_X.Y.Z_x64-setup.exe.sig" `
  --url "http://64.226.112.204/launcher/updates/SP Launcher_X.Y.Z_x64-setup.exe" `
  --notes "One or two lines describing what changed" `
  --out latest.json
```

`--notes` isn't shown anywhere in the current UI yet — it's just carried in
the JSON for future use — but fill it in anyway so you have a record.

**4. Upload both files to the server**, so they land at exactly the URLs the
launcher expects:

```powershell
scp "src-tauri\target\release\bundle\nsis\SP Launcher_X.Y.Z_x64-setup.exe" youruser@64.226.112.204:/var/www/html/launcher/updates/
scp latest.json youruser@64.226.112.204:/var/www/html/launcher/updates/latest.json
```

**5. Verify it's actually live** before telling anyone to update:

```powershell
curl http://64.226.112.204/launcher/updates/latest.json
```

You should see the JSON you just generated, with the new version number.

That's it — every running launcher will pick this up on its next startup
(or immediately if someone clicks Check for updates) and offer to install
it.

## A version you upload replaces the previous one for everybody

There's only ever one `latest.json` at that URL. Uploading a new version
overwrites what every player's launcher will see next time it checks — you
can't have some players offered v1.2 and others v1.3 from the same
endpoint. If you need to roll back a bad release, re-upload the previous
version's installer and a `latest.json` pointing at it (with the old, lower
version number) — though note the updater compares versions and normally
won't offer a downgrade; you'd need `allowDowngrades` for that, which isn't
currently enabled anywhere in this app.

## Losing the private key

If `%USERPROFILE%\.tauri\sp-launcher.key` (or wherever you end up backing it
up to) is lost, you cannot sign any future update that existing installs
will trust — every copy of the launcher already out there has today's
public key baked in and will reject anything signed by a different one.
Recovering from that means generating a new keypair, putting the new public
key in `tauri.conf.json`, and shipping that build to everyone through some
channel *other* than the auto-updater (since the old installs can't verify
it) — e.g. posting a fresh installer link in Discord. Back the private key
up somewhere durable (a password manager, an encrypted drive) — losing it
isn't catastrophic, but it does mean burning the auto-updater for everyone
currently on an older version.

## Troubleshooting

- **"Check for updates" never finds anything, even after uploading a newer
  version.** Check the endpoint URL is reachable and returns the file you
  expect: `curl http://64.226.112.204/launcher/updates/latest.json`. A 404
  means it's not at that exact path; nginx returning the wrong folder means
  the `launcher/updates/` structure under `/var/www/html/` doesn't match.
- **Update found, but install fails with a signature error.** Almost always
  means the `.sig` file used in `make-update-manifest.mjs --sig` doesn't
  match the `.exe` you actually uploaded — rebuild and regenerate both
  together, don't mix an old `.sig` with a new `.exe` or vice versa.
- **Build succeeds but no `.sig` file appears.** `TAURI_SIGNING_PRIVATE_KEY`
  wasn't set in that shell session — environment variables set with `$env:`
  in PowerShell only last for that window; set them again if you open a new
  one before building.
