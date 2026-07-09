# Ogma

Record in-person meetings, get speaker-labeled transcripts and AI meeting notes, and have everything land in Notion — queryable by Claude via MCP.

Named after the Celtic god of eloquence and inventor of the Ogham script: he listened well and wrote things down.

## What it does

1. **Record** a 1–3 hour in-person meeting from the default mic (crash-safe: audio is written as rotating 5-minute segments, so a crash loses minutes, not the meeting).
2. **Transcribe** via the OpenAI Whisper API, stitched into one timestamped transcript.
3. **Label & summarize** via the Claude API: speaker attribution (`Speaker A/B/C`, renameable to real names) plus structured notes — TL;DR, summary, key points, decisions, action items, open questions, and quote highlights that jump-play the audio.
4. **Sync** the finished notes and transcript to a Notion "Meetings" database — the canonical, cross-device store.
5. **Ask Claude about it**: the same binary runs as a local MCP server (`ogma --mcp`) so Claude Code / claude.ai can search transcripts and pull action items.

Cloud processing (~$1.25 per 3-hour meeting) is a deliberate choice: it makes the app work identically on desktop and phone. See [PLAN.md](PLAN.md) for the full reasoning and the deferred local-AI path.

## Stack

- **App:** [Tauri 2](https://tauri.app) — Rust backend, Vite + TypeScript frontend
- **Audio:** `cpal` capture → crash-safe 16kHz mono WAV segments (already under Whisper's 25MB cap, so no transcode step)
- **AI:** OpenAI Whisper (`whisper-1`) for STT, Claude (`claude-sonnet-5`) for speaker labels + notes
- **Storage:** SQLite (`rusqlite` + FTS5) locally, Notion REST API as the shared store
- **MCP:** `rmcp` crate, stdio transport

## Status

🚧 **Alpha — Phases 1–3 implemented on Windows; live-API end-to-end run pending.**

| Phase | Scope | Status |
|-------|-------|--------|
| 1 | Recording core on Windows (capture, chunking, pause/resume, library UI, SQLite) | ✅ Implemented (real-mic test passing) |
| 2 | AI pipeline (Whisper transcription, Claude speakers + notes, retry UX) | ✅ Implemented (needs live-API shakedown) |
| 3 | Notion sync + MCP server | ✅ Implemented (MCP stdio smoke-tested) |
| 4 | macOS, then iOS (Tauri 2 mobile) | — |

## Development

Requires Rust (stable), Node.js, and the [Tauri 2 prerequisites](https://tauri.app/start/prerequisites/) for your platform.

```sh
npm install
npm run tauri dev
```

Tests: `cargo test -p ogma-core` (unit), plus `cargo test -p ogma-core --test recorder_integration -- --ignored` to exercise the real microphone.

API keys (OpenAI, Anthropic, Notion) are entered in the app's settings page — nothing is hardcoded or committed.

### MCP: ask Claude about your meetings

The same binary is an MCP server. After a `cargo build -p ogma` (or release build), register it:

```sh
claude mcp add ogma -- E:\Work\ogma\target\debug\ogma.exe --mcp
```

Tools: `list_meetings`, `search_transcript` (FTS5), `get_meeting_notes`, `get_transcript`, `get_action_items`.

## Documents

- [website/](website/) — public documentation site (Fumadocs); `cd website && npm install && npm run dev`
- [PLAN.md](PLAN.md) — full architecture, decisions, phase plan, and verification checklist
- [CLAUDE.md](CLAUDE.md) — working conventions for AI-assisted development
