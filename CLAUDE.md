# CLAUDE.md

Guidance for Claude Code when working in this repository.

## What Ogma is

A Tauri 2 desktop app (Rust backend + Vite/TypeScript UI) that records in-person meetings (1–3 hours), transcribes them via the OpenAI Whisper API, generates speaker-labeled transcripts and structured notes via the Claude API, pushes everything to Notion, and exposes the meeting library to AI clients through a local MCP server.

**`PLAN.md` is the source of truth** for architecture, phase breakdown, and the reasoning behind every decision below. Read it before making design changes. Keep this file updated as the codebase takes shape (build commands, crate layout, etc.).

## Current state

Phases 1–3 implemented (Windows desktop): recording core, SQLite storage, Whisper + Claude pipeline, Notion sync, MCP server, full UI. Not yet exercised against the live paid APIs end-to-end; Phase 4 (macOS/iOS) not started.

## Commands

- `npm run tauri dev` — run the app (Vite dev server + Rust build)
- `cargo check --workspace --all-targets` / `cargo test -p ogma-core` — Rust checks and unit tests
- `cargo test -p ogma-core --test recorder_integration -- --ignored` — real-microphone integration test (needs audio hardware)
- `npx tsc --noEmit` / `npm run build` — frontend typecheck / bundle
- `npm run tauri build` — release bundle
- MCP smoke test: pipe JSON-RPC lines into `target/debug/ogma.exe --mcp`

## Layout

- `crates/ogma-core/` — everything that matters: `recording/` (cpal capture → 16kHz mono, crash-safe rotating WAV segments with header `repair()`, wake lock), `storage.rs` (rusqlite + FTS5), `providers/` (`whisper.rs`, `claude.rs` behind traits), `pipeline.rs` (idempotent orchestrator, resume point derived from stored data), `notion.rs`, `mcp.rs`, `config.rs`
- `src-tauri/` — thin Tauri layer: commands/events in `lib.rs`, `--mcp` branch in `main.rs`
- `src/` — vanilla TS frontend: `api.ts` (typed invoke wrappers), `views/` (home/detail/settings); events re-dispatched as `ogma:*` CustomEvents

## Implementation notes (non-obvious)

- Whisper chunks are the raw 5-min recording segments — 16kHz mono WAV ≈ 9.6MB is already under the 25MB cap, so there is **no ffmpeg/transcode step** (deviation from the original plan, deliberate).
- The Claude call does NOT re-emit transcript text: it returns speaker labels as utterance **index ranges** (`speaker_assignments`) plus notes, enforced via `output_config.format` json_schema. `assemble()` merges labels onto stored utterances locally. Keeps output a few k tokens → no streaming needed.
- Pipeline resume points are derived from what's in SQLite (no transcript → transcribe; no notes → summarize; no notion page → sync), so retry-after-crash needs no remembered stage.
- Unlabeled speaker sentinel is `"Speaker ?"` (`pipeline::UNLABELED_SPEAKER`).
- MCP mode must never print to stdout except JSON-RPC (logging goes to stderr).
- `Recorder` uses a dedicated audio thread because cpal streams are `!Send`; `stop()` is called via `spawn_blocking`.

## Locked-in decisions (don't relitigate without asking the user)

- **Cloud-only AI for v1.** STT = OpenAI Whisper API (`whisper-1`, `verbose_json`). Notes + speaker attribution = Claude API (`claude-sonnet-5`). Local/on-device AI was researched and deliberately deferred — see "Why cloud" in PLAN.md.
- **No acoustic diarization.** Whisper gives no speaker info; Claude infers `Speaker A/B/C` labels from the transcript text (combined with notes generation in one call by default). This is best-effort by design.
- **Providers behind Rust traits** (`TranscriptionProvider`, `NotesProvider`) so alternative STT or local backends can slot in later without touching the pipeline.
- **Notion is the canonical cross-device store** (direct REST API from Rust); local SQLite is the per-device cache/index. MCP is only for AI clients, not for the app's own Notion writes.
- **Crash-safe recording:** rotating 5-minute WAV segments (16kHz mono 16-bit PCM), concatenated on finalize. A crash must never lose more than ~5 minutes.
- **MCP server is the same binary** run as `ogma --mcp` (stdio, `rmcp` crate), reading local SQLite.
- **Platform order:** Windows → macOS → iOS (Tauri 2 mobile, gated on a background-recording spike).

## Architecture (target)

```
Rust workspace:
  ogma (Tauri app)  ──uses──►  core (lib crate: pipeline, storage, providers)
                                 └── also used by the --mcp stdio mode
Pipeline: finalize audio → raw 5-min WAV chunks (already <25MB, no transcode) →
          Whisper per chunk → stitch timestamps → Claude (speakers + notes JSON)
          → SQLite → Notion
Frontend: Vite + vanilla TypeScript (no framework unless UI outgrows it)
```

Key crates: `cpal`, `rusqlite` (+FTS5), `reqwest`, `rmcp`, `keyring`, `tokio`, `serde_json`. WAV read/write is hand-rolled (`recording/wav.rs`) for crash-safe header repair — no `hound`. No ffmpeg: raw 16kHz mono WAV chunks are already under the 25MB Whisper cap.

## Conventions & constraints

- API keys (OpenAI, Anthropic, Notion) live in the app's settings/config — never hardcode or commit them.
- Every pipeline step must be idempotent and retryable; source audio on disk is sacred until the meeting is fully processed.
- SQLite schema: `meetings`, `transcript_segments` (speaker, start_ms, end_ms, text), `notes` (JSON), `action_items`. Timestamps are `_ms` integers throughout.
- Whisper chunk uploads must stay under the 25MB API cap (5-min segments at ~32kbps Opus/MP3 ≈ 1–2MB).
- Prevent system sleep while recording (`SetThreadExecutionState` on Windows; IOKit equivalent on macOS).

## Verification

Per-phase acceptance checks are listed under "Verification" in PLAN.md — notably the kill-the-app-mid-recording recovery test and the end-to-end pipeline test on a short 2-person recording. Run the relevant ones before declaring a phase done.
