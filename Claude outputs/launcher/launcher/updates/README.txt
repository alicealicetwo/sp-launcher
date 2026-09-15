This folder is where the auto-updater looks for new releases.

After you run a signed build (see RUNNING.md / the earlier chat instructions
for `npm run tauri build` with TAURI_SIGNING_PRIVATE_KEY set), drop two files
in here:

  - the NSIS installer .exe from src-tauri/target/release/bundle/nsis/
  - latest.json, generated with:
      node tools/make-update-manifest.mjs --version X.Y.Z --sig <path to .sig> --url http://64.226.112.204/launcher/updates/<installer>.exe --out latest.json

Both need to end up at exactly:
  http://64.226.112.204/launcher/updates/<installer>.exe
  http://64.226.112.204/launcher/updates/latest.json

This README itself doesn't need to be uploaded — delete it once you
understand the folder, or leave it, nginx won't serve .txt as anything
dangerous.
