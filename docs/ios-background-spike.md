# iOS Background-Recording Spike

**Goal:** answer one yes/no question before any iOS UI is built —
**can the app keep recording a 1–3 hour meeting while the iPhone screen is locked?**

This is the Phase 4 gate called out in `PLAN.md`. Everything about the phone
story (record on the phone → upload → Notion sync) depends on it. If it fails,
the plan's documented fallback is the *import-audio-file* path (record with
Voice Memos, import the file) — worth having on desktop anyway.

> **Why a spike and not just "build it":** the recording plugin
> (`tauri-plugin-audio-recorder`) documents mic permission but says nothing about
> background audio. iOS silently suspends capture when an app is backgrounded
> *unless* the app declares the `audio` background mode **and** the native
> recorder activates an `AVAudioSession` with a record-capable category. Whether
> the plugin does the latter is unknown — that's what we're testing.

---

## ⚠️ Run the spike in a STANDALONE app, not the ogma workspace

Adding `tauri-plugin-audio-recorder` to `src-tauri/` **breaks `cargo` resolution
for the whole workspace.** Confirmed on this branch:

```
error: failed to select a version for `alsa-sys`.
  ... cpal v0.15  (tauri-plugin-audio-recorder → desktop backend) → alsa-sys 0.3
  ... cpal v0.18  (ogma-core recorder)                            → alsa-sys 0.4
  package `alsa-sys` links to the native library `alsa`, but it conflicts ...
```

The plugin depends on `cpal 0.15` for its **desktop** backend
(`cfg(not(any(target_os="ios", target_os="android")))`). `ogma-core` uses
`cpal 0.18`. Cargo builds one unified lockfile across all targets, so the two
`cpal` majors — each linking the native `alsa` lib on Linux — can't coexist.
**cfg-gating the plugin to mobile does not help:** once the plugin is in the
manifest, Cargo resolves its desktop-only `cpal 0.15` for desktop targets anyway.

Reconciling this (fork the plugin onto `cpal 0.18`, or split `ogma-core`'s
recorder out of the mobile graph, or write our own thin Swift recording plugin)
is **real Phase-4 integration work** — deliberately out of scope for a spike
whose whole point is to isolate the background-recording question. So:

**Prove background recording in a throwaway Tauri app first. Only if it PASSES do
we pay the integration cost above.**

The scaffolding staged on this branch (see below) is ready to copy into that
standalone app.

---

## What's already staged on this branch (`ios-background-spike`)

Done from Windows, desktop workspace still green (`cargo check`, `tsc`, `npm run
build` all pass):

| Artifact | File | Notes |
| --- | --- | --- |
| Spike screen with automatic PASS/FAIL scoring | `src/views/spike.ts` | The core reusable artifact — copy into the standalone app. |
| "Spike (iOS)" nav item + route | `index.html`, `src/main.ts`, `src/view.ts` | Lets you preview the UI on desktop (plugin calls no-op off-iOS). |
| JS API dependency | `package.json` → `tauri-plugin-audio-recorder-api` | Pure JS bindings; installs cross-platform. |
| This runbook | `docs/ios-background-spike.md` | — |

The Rust-side plugin registration and mobile capability are **intentionally not
committed** (they break the workspace, see above). Add them in the standalone
app per steps 2–3.

The spike screen records, then compares the recorder's **reported duration**
against **wall-clock elapsed** (`Date.now()` deltas survive screen-lock JS
suspension). Recorded ≥ 95% of wall-clock → **PASS**.

---

## Run it (macOS only)

### 0. Prerequisites
- macOS with **Xcode** + Command Line Tools, an Apple Developer account (a free
  one works for on-device testing), and the Rust iOS targets:
  ```sh
  rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
  ```
- A **physical iPhone**. The Simulator does **not** faithfully reproduce
  background-audio suspension — a Simulator pass proves nothing. Use a real device.

### 1. Create the standalone spike app
```sh
npm create tauri-app@latest ogma-ios-spike -- --template vanilla-ts
cd ogma-ios-spike
npm install
npm install tauri-plugin-audio-recorder-api
cargo add tauri-plugin-audio-recorder --manifest-path src-tauri/Cargo.toml
```
Then copy in the UI from this branch:
`src/views/spike.ts` plus its tiny helpers `src/dom.ts`, `src/format.ts`,
`src/toast.ts`, and the `View` type from `src/view.ts`. Render `renderSpike()`
as the app's only screen (replace the template's `main.ts` body).

### 2. Register the plugin (Rust)
In `src-tauri/src/lib.rs` (or `main.rs`):
```rust
tauri::Builder::default()
    .plugin(tauri_plugin_audio_recorder::init())
    // ...
```

### 3. Grant the capability
In `src-tauri/capabilities/default.json`, add to `permissions`:
```json
"audio-recorder:default"
```

### 4. Initialize the iOS project
```sh
npm run tauri ios init
```

### 5. Add the two Info.plist keys
Open `src-tauri/gen/apple/<project>_iOS/Info.plist` and add inside the top-level
`<dict>`:
```xml
<key>NSMicrophoneUsageDescription</key>
<string>Ogma records your in-person meetings to transcribe and summarize them.</string>

<key>UIBackgroundModes</key>
<array>
  <string>audio</string>
</array>
```
`UIBackgroundModes = audio` is the entitlement that keeps the audio session alive
with the screen locked. Without it the spike **will** fail.

> If the plugin turns out **not** to activate a record-capable `AVAudioSession`
> category, add this at launch in the generated Xcode project — the single most
> likely fix if step 8 fails:
> ```swift
> try? AVAudioSession.sharedInstance().setCategory(.playAndRecord, mode: .default)
> try? AVAudioSession.sharedInstance().setActive(true)
> ```

### 6. Build & run on the device
```sh
npm run tauri ios dev     # pick your connected iPhone
```
Trust the developer profile if prompted (Settings › General › VPN & Device
Management).

### 7. Perform the spike
1. Tap **● START SPIKE** and grant the mic prompt.
2. **Lock the screen.** Put the phone down.
3. Wait **at least 60 minutes** (real target is 1–3 h; 60 min is the minimum
   convincing run). Optional stress: take an incoming call, switch apps, return.
4. Unlock, reopen the app, tap **■ STOP & SCORE**.

### 8. Read the verdict
The screen prints PASS/FAIL plus `wall-clock elapsed` vs `recorded duration`
(the core comparison), `file size`, `sample rate`, `channels`, `file path`.

---

## Pass / fail criteria

| Outcome | Meaning | Next step |
| --- | --- | --- |
| **PASS** — recorded ≥ 95% of wall-clock | Background recording works | Pay the integration cost (reconcile the cpal clash) and build the real iOS recording UI, feeding the plugin's M4A into the pipeline (transcode → Whisper). |
| **FAIL** — recording truncates near lock time | iOS suspended capture | Retry with the `AVAudioSession` Swift snippet (step 5 note). Still failing → fallback below. |
| **FAIL** — app killed / crashes when locked | Process not surviving background | Same: try the session fix, then fallback. |

### Fallback if it can't be made to pass
Per `PLAN.md`, don't fight iOS background audio indefinitely. Pivot to:
- **Import-audio-file path** — record with Apple **Voice Memos** (rock-solid
  background recording), then import the file into Ogma and run the normal
  pipeline. Also useful on desktop.
- Or keep-screen-on foreground recording as a stopgap.

---

## Notes / gotchas
- **Real device only** for the verdict (see step 0).
- The spike is intentionally isolated: no storage, no pipeline, no Notion.
- Mobile output is **M4A/AAC**, not the desktop's WAV segments. Integrating it
  later means an import/transcode step feeding Whisper — note the returned
  `sampleRate`/`channels` so we know what we'll get.
- Record the actual step-8 numbers in the PR so the Phase-4 go/no-go is documented.
- If you later decide to fold the plugin into the main workspace, budget for the
  cpal/alsa conflict described above before anything else.
