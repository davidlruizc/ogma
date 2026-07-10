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
  paid certificate; an Apple Developer account at $99/yr also unlocks iOS for Phase 4),
  the Tauri auto-updater (needs the updater plugin + a signing keypair), and Intel
  macOS (add `x86_64-apple-darwin` to the workflow matrix if requested).
