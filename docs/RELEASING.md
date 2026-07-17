# Releasing Ogma

Releases are built by `.github/workflows/release.yml`: pushing a `v*` tag builds the
Windows installer (`.msi` + NSIS `.exe`) and the macOS Apple Silicon app (`.dmg`) and
attaches both to a single **draft** GitHub Release.

## Checklist

1. **Bump the version in three files** (keep them identical):
   - `package.json` → `"version"`
   - `Cargo.toml` (workspace root) → `[workspace.package] version` — both crates inherit it
   - `src-tauri/tauri.conf.json` → `"version"`
2. Commit the bump and land it on `main`.
3. Tag and push:
   ```
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```
4. Wait for the **Release** workflow (both matrix legs) to go green.
5. Review the draft release on GitHub — artifacts, notes — then **Publish**.
   Publishing is what turns on OTA updates for that version: installed apps poll
   `releases/latest/download/latest.json`, which only resolves for a published
   (non-draft) release.

## OTA updates

Installed apps check for updates on launch and from **Settings → Updates**
(`tauri-plugin-updater`). How it fits together:

- `src-tauri/tauri.conf.json` sets `bundle.createUpdaterArtifacts: true` (emits
  `.sig` files and the macOS `.app.tar.gz`) and `plugins.updater` with the
  minisign **public key** and the endpoint
  `https://github.com/davidlruizc/ogma/releases/latest/download/latest.json`.
- `tauri-action` builds `latest.json` from the updater artifacts and attaches it
  to the release. Windows updates point at the NSIS installer
  (`updaterJsonPreferNsis: true`) — Tauri's NSIS installer replaces an existing
  MSI install cleanly, while the reverse would leave two copies installed.
- Artifacts are signed with the private key from the repo secrets
  `TAURI_SIGNING_PRIVATE_KEY` (+ `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, empty for
  the current key). The keypair was generated with
  `npm run tauri signer generate` and the private key lives **only** in
  `~/.tauri/ogma-updater.key` on David's machine and in the GitHub secret.
  **Back it up: if it's lost, shipped apps can never accept another update**
  (the public key baked into them won't match), and users would have to
  reinstall manually.
- OTA delivers app updates only — it does not migrate between installer formats
  and does not touch the SQLite DB (schema migrations run in-app on open).

## Notes

- **Builds are unsigned.** The release body (set in the workflow) tells users how to get
  past SmartScreen (Windows) and quarantine (`xattr -cr`, macOS).
- **The macOS build is experimental** — the app compiles for macOS (wake lock has an
  IOKit implementation, keychain uses `apple-native`) but has never been run on real
  Mac hardware. Say so in the release notes until Phase 4 validates it.
- **CI minutes (private repo):** macOS runners bill at 10×, Windows at 2×, against the
  free 2,000 min/month. A full release run costs roughly 250–300 billed minutes, so
  about 5–6 releases/month stay free. Check usage under Settings → Billing → Actions.
- **Deliberately not included** (revisit later): code signing / notarization (needs a
  paid certificate; an Apple Developer account at $99/yr also unlocks iOS for Phase 4)
  and Intel macOS (add `x86_64-apple-darwin` to the workflow matrix if requested).
  Note: on macOS the updater replaces the app bundle in place, which breaks the ad-hoc
  quarantine workaround users applied on first install — until builds are notarized,
  macOS users may need to re-run `xattr -cr /Applications/Ogma.app` after an update.
