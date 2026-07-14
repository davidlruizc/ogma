# Ogma

**Record in-person meetings, get speaker-labeled transcripts and AI meeting notes, and have everything land in Notion — queryable by Claude via MCP.**

[![CI](https://github.com/davidlruizc/ogma/actions/workflows/ci.yml/badge.svg)](https://github.com/davidlruizc/ogma/actions/workflows/ci.yml)
[![Website](https://img.shields.io/badge/website-ogma.my-6b5bd6)](https://ogma.my)
[![Docs](https://img.shields.io/badge/docs-ogma.my%2Fdocs-6b5bd6)](https://ogma.my/docs)
[![Status](https://img.shields.io/badge/status-alpha-orange)](#status)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%C2%B7%20macOS-informational)](#status)

🌐 **Website & docs:** [ogma.my](https://ogma.my) · 📖 **Documentation:** [ogma.my/docs](https://ogma.my/docs) · 🗺️ **Architecture:** [PLAN.md](PLAN.md)

Named after the Celtic god of eloquence and inventor of the Ogham script: he listened well and wrote things down.

---

## Contents

- [What it does](#what-it-does)
- [Why cloud processing](#why-cloud-processing)
- [Stack](#stack)
- [Status](#status)
- [Quick start](#quick-start)
- [Installing the macOS build](#installing-the-macos-build)
- [MCP: ask Claude about your meetings](#mcp-ask-claude-about-your-meetings)
- [Project layout](#project-layout)
- [Contributing](#contributing)
- [Documentation](#documentation)

## What it does

1. **Record** a 1–3 hour in-person meeting from the default mic. Crash-safe: audio is written as rotating 5-minute segments, so a crash loses minutes, not the meeting.
2. **Transcribe** via the OpenAI Whisper API, stitched into one timestamped transcript.
3. **Label & summarize** via the Claude API: speaker attribution (`Speaker A/B/C`, renameable to real names) plus structured notes — TL;DR, summary, key points, decisions, action items, open questions, and quote highlights that jump-play the audio.
4. **Sync** the finished notes and transcript to a Notion "Meetings" database — the canonical, cross-device store.
5. **Ask Claude about it**: the same binary runs as a local MCP server (`ogma --mcp`) so Claude Code / claude.ai can search transcripts and pull action items.

See it in action on the landing page: **[ogma.my](https://ogma.my)**.

## Why cloud processing

Transcription and note-taking run through the OpenAI and Anthropic APIs (roughly **$1.25 per 3-hour meeting**, paid with your own keys). That's a deliberate choice: it makes the app behave identically on desktop and, eventually, phone, and avoids shipping and maintaining local models. The local/on-device path was researched and intentionally deferred — the full reasoning is in [PLAN.md](PLAN.md).

## Stack

- **App:** [Tauri 2](https://tauri.app) — Rust backend, Vite + TypeScript frontend
- **Audio:** `cpal` capture → crash-safe 16 kHz mono WAV segments (already under Whisper's 25 MB cap, so no transcode step)
- **AI:** OpenAI Whisper (`whisper-1`) for STT, Claude (`claude-sonnet-5`) for speaker labels + notes
- **Storage:** SQLite (`rusqlite` + FTS5) locally, Notion REST API as the shared store
- **MCP:** `rmcp` crate, stdio transport

## Status

🚧 **Alpha.** Phases 1–3 are implemented and verified end-to-end against the live paid APIs on Windows (July 2026). Phase 4 (macOS, then iOS) is in progress.

| Phase | Scope | Status |
|-------|-------|--------|
| 1 | Recording core (capture, chunking, pause/resume, library UI, SQLite) | ✅ Implemented & verified |
| 2 | AI pipeline (Whisper transcription, Claude speakers + notes, retry UX) | ✅ Implemented & verified |
| 3 | Notion sync + MCP server | ✅ Implemented & verified |
| 4 | macOS, then iOS (Tauri 2 mobile) | 🚧 In progress |

**Phase 4 detail:** macOS support code is complete (wake lock, mic permission, CI job, release `.dmg`) but not yet verified on real Mac hardware; audio-file import (WAV/M4A/MP3/FLAC/OGG → the normal pipeline) ships on desktop; iOS is gated on a background-recording spike that needs a Mac + physical iPhone.

Future feature ideas (more export destinations, tray/menu-bar quick recording, Notion-optional notes) live in the [PLAN.md backlog](PLAN.md#backlog--future-feature-ideas-post-v1-unscheduled).

## Quick start

You'll need **Rust** (stable), **Node.js** (LTS), and the [Tauri 2 prerequisites](https://tauri.app/start/prerequisites/) for your platform. On Windows that's WebView2 (ships with Windows 11) and the MSVC build tools.

```sh
git clone https://github.com/davidlruizc/ogma.git
cd ogma
npm install
npm run tauri dev
```

`npm run tauri dev` starts the Vite dev server and compiles the Rust backend; the first build takes a few minutes. For a release bundle, run `npm run tauri build`.

**API keys** (OpenAI, Anthropic, Notion) are entered in the app's Settings page — nothing is hardcoded or committed. You can record without any keys; transcription and notes run once keys are configured, and any failed or skipped step can be retried later.

A short 2-person conversation is the best first smoke test: record it, stop, and watch the pipeline run transcribe → label & summarize → (optionally) sync to Notion.

Full walkthrough: **[ogma.my/docs/getting-started](https://ogma.my/docs/getting-started)**.

## Installing the macOS build

Prebuilt Apple Silicon `.dmg` files are attached to each [GitHub Release](https://github.com/davidlruizc/ogma/releases). These alpha builds are **not yet notarized**, so on first launch macOS may say *"Ogma is damaged and can't be opened"* — the download is not actually corrupted, and there is no "Open Anyway" button for this message. Clear the quarantine flag once, then open normally:

```sh
xattr -dr com.apple.quarantine /Applications/Ogma.app
```

This step goes away once releases are signed and notarized.

## MCP: ask Claude about your meetings

The same binary is an MCP server. After a `cargo build -p ogma` (or a release build), register it with Claude Code:

```sh
claude mcp add ogma -- /path/to/ogma --mcp
```

(On Windows the binary is `target\debug\ogma.exe` after a dev build, or `target\release\ogma.exe` after a release build.)

Tools: `list_meetings`, `search_transcript` (FTS5), `get_meeting_notes`, `get_transcript`, `get_action_items`. It's read-only and reads the same local SQLite library the app writes. Details: **[ogma.my/docs/mcp](https://ogma.my/docs/mcp)**.

## Project layout

A quick map for contributors — the deep dive is in [PLAN.md](PLAN.md) and [CLAUDE.md](CLAUDE.md).

```
crates/ogma-core/   The core lib: everything that matters
  recording/        cpal capture → 16 kHz mono, crash-safe rotating WAV segments, wake lock
  storage.rs        SQLite (rusqlite + FTS5)
  providers/        whisper.rs, claude.rs behind traits
  pipeline.rs       idempotent orchestrator (resume point derived from stored data)
  sync/             SyncDestination trait — Notion, Markdown/Obsidian, Apple Notes
  mcp.rs            MCP server (rmcp, stdio)
src-tauri/          thin Tauri layer: commands/events in lib.rs, --mcp branch in main.rs
src/                vanilla TS frontend: api.ts, views/ (home/detail/settings)
website/            public docs + landing page (Fumadocs on Next.js) → ogma.my
```

## Contributing

Contributions are welcome — this is an open-source project and issues, ideas, and PRs of any size are appreciated.

1. **Explore first.** [PLAN.md](PLAN.md) is the source of truth for architecture and the reasoning behind each decision; [CLAUDE.md](CLAUDE.md) captures the working conventions and non-obvious implementation notes. Skim both before a substantial change.
2. **Pick something up.** Browse [open issues](https://github.com/davidlruizc/ogma/issues), or open one to discuss a change before you start. The [PLAN.md backlog](PLAN.md#backlog--future-feature-ideas-post-v1-unscheduled) lists good future directions.
3. **Make your change** on a branch, and keep pipeline steps idempotent and retryable — source audio on disk is treated as sacred until a meeting is fully processed.
4. **Run the checks** below, then open a PR describing the what and the why.

```sh
cargo check --workspace --all-targets   # Rust builds
cargo test -p ogma-core                 # Rust unit tests
npx tsc --noEmit                        # frontend typecheck
```

Optional but appreciated: `cargo test -p ogma-core --test recorder_integration -- --ignored` exercises a real microphone. For the docs site, `cd website && npm install && npm run dev`.

Please never hardcode or commit API keys — they live in the app's settings and the OS keychain.

## Documentation

- 🌐 **[ogma.my](https://ogma.my)** — website and landing page
- 📖 **[ogma.my/docs](https://ogma.my/docs)** — user documentation (getting started, configuration, recording, Notion, MCP, FAQ)
- 🗺️ **[PLAN.md](PLAN.md)** — full architecture, decisions, phase plan, and verification checklist
- 🛠️ **[CLAUDE.md](CLAUDE.md)** — working conventions for AI-assisted development
- 📁 **[website/](website/)** — the docs site source (Fumadocs); `cd website && npm install && npm run dev`
</content>
</invoke>
