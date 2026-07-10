# Ogma — Record, Transcribe & Note-Take for In-Person Meetings

## Context

The goal is an app to record in-person meetings (1–3 hours), then automatically produce a full transcript with speaker labels, AI-generated meeting notes (summary, action items, decisions, highlights), and push everything to Notion. Notes must also be queryable by Claude via MCP. Priorities: practicality, cross-platform (Windows, macOS, iOS).

**Decisions made:**
- **Stack:** Tauri 2 (Rust backend + web UI) — matches existing experience (Rust from coffee-terminal, web from image-studio). Desktop first (Windows → macOS), iOS as a later phase via Tauri 2 mobile.
- **Transcription & notes: cloud-only for v1** using **existing API keys (OpenAI + Anthropic)** — no third-party STT service. Cloud processing is what makes the app work identically on desktop *and* phone: the recording device uploads audio and finished notes sync everywhere via Notion. Local/on-device AI would tie processing to one powered-on machine and break the mobile story.
  - **STT: OpenAI Whisper API** (`whisper-1`, `verbose_json` for segment timestamps). ~$0.36/hr audio. 25MB/file limit is handled by our 5-min compressed chunks. **Caveat:** Whisper has *no* speaker diarization — see "Speaker labeling" for how we get per-speaker output without it.
  - **Notes: Claude API** (`claude-sonnet-5`). ~$0.10–0.20 per 3hr meeting.
  - **~$1.25 per 3hr meeting.**
- **Providers still sit behind a Rust trait** (`TranscriptionProvider` / `NotesProvider`) purely as a seam, so a different STT (e.g. one with native diarization) or a local backend can slot in later without touching the pipeline.
- **Destinations:** Notion (canonical, shared across devices) + a local MCP server so Claude Code / claude.ai can search meetings.

## Why cloud (deep-research summary — local deferred, not dismissed)

The research confirms fully-local is *technically* viable, but the mobile requirement makes cloud the right v1 call. Captured here so the local path is ready if priorities change:
- **Local STT is fast enough on a good GPU:** `faster-whisper` int8 `large-v3` ≈ 7–8x real-time on an RTX 3060/4060 (3hr → ~25 min), ~2.5 GB VRAM; **NVIDIA Parakeet** is ~40x Whisper speed at comparable WER; Apple Silicon MLX Whisper does 1hr in ~2 min. CPU-only is slow (not interactive). [verified: SYSTRAN/faster-whisper, HF Open ASR Leaderboard]
- **Diarization is the local pain point:** `pyannote` 3.1 (~12–19% DER) is the standard but its models are HF-gated (token + license acceptance) and officially Linux/macOS — Windows setup is the #1 reported failure. This alone makes cloud diarization the pragmatic choice. [verified: HF pyannote/speaker-diarization-3.1]
- **Local notes LLM:** Qwen3-14B (16GB VRAM) / 27–30B at 4-bit (24GB) via Ollama handles a 25–35k-token transcript, but structured action-item quality trails Claude.
- **The blocker for this use case isn't capability — it's topology:** local processing can't serve a phone that records away from the desktop. Cloud does, trivially.
- **Prior art / fallback:** [Meetily](https://github.com/Zackriya-Solutions/meetily) is an existing fully-local meeting-notes desktop app (whisper.cpp/Parakeet + Ollama) — worth a look as reference, but it's desktop-only-processing, which is exactly the constraint we're avoiding. [verified]
- **When to revisit local:** if a capable GPU becomes the always-on hub, if per-meeting cost adds up (break-even on a used RTX 3090 is thousands of hours — so the real driver would be privacy/offline, not money), or if specific meetings legally can't leave the machine.

## Architecture

```
┌─ Tauri 2 app (Rust + web UI) ─────────────────────────────┐
│  Record (cpal → chunked WAV segments on disk)             │
│  Meeting library + notes viewer (web UI)                  │
│  Pipeline (Rust, async):                                  │
│    1. finalize audio → compressed chunks (<25MB each)     │
│    2. OpenAI Whisper per chunk → stitched transcript      │
│    3. Claude API → speaker labels + structured notes JSON │
│    4. save to SQLite  5. push page to Notion              │
└───────────────────────────────────────────────────────────┘
        │                                   │
   SQLite + audio files              Notion "Meetings" DB
        │                            (canonical, cross-device)
   MCP server (same binary, `--mcp` stdio mode)
   tools: list_meetings, search_transcript,
          get_notes, get_action_items
```

## Project setup

- New project at `E:\Work\ogma`
- `create-tauri-app` with vanilla TypeScript + Vite frontend (keep it light, like image-studio; no heavy framework needed — can add one later if UI grows)
- Rust workspace: main Tauri app + `core` lib crate (pipeline, storage) shared by the MCP mode

## Phase 1 — Recording core (Windows desktop)

**Recording (Rust, `cpal` crate):**
- Capture default input device, downmix to 16kHz mono 16-bit PCM (ideal for STT, small files: ~115MB/hr WAV)
- **Crash-safe chunking:** write rotating 5-minute WAV segments (`meeting-id/seg-000.wav`, …). A crash at 2h50m loses ≤5 min, not the meeting. On finalize, concatenate segments.
- Pause/resume, elapsed timer, live input-level meter (emit RMS via Tauri events) so you can see it's actually capturing
- Prevent system sleep while recording (Windows `SetThreadExecutionState`, macOS `caffeinate`-equivalent via IOKit)

**UI:** big Record button, timer, level meter, meeting title field; meeting list with status (recorded / transcribing / done)

**Storage:** SQLite via `rusqlite` — tables `meetings`, `transcript_segments` (speaker, start_ms, end_ms, text), `notes` (JSON), `action_items`. Audio + DB under the app data dir. Settings (API keys) in a config file, entered via a settings page.

## Phase 2 — AI pipeline (cloud)

Define two Rust traits as a seam (one cloud impl each in v1; local can slot in later):
- `TranscriptionProvider` → diarized utterances `[{speaker, start_ms, end_ms, text}]`
- `NotesProvider` → structured notes JSON

**Transcription (OpenAI Whisper `whisper-1`):** transcode each 5-min WAV segment to a compressed format (e.g. 32kbps Opus/MP3, ~1–2MB, well under the 25MB cap), POST to `/v1/audio/transcriptions` with `response_format=verbose_json` to get per-segment timestamps, and stitch segments back into one timestamped transcript (offset each chunk's timestamps by its start time). Store as transcript segments.

**Speaker labeling (Claude-inferred):** Whisper returns no speaker info, so Claude does attribution. Two workable shapes:
- **Combined (default, one call):** send the full timestamped transcript to Claude and ask it to both (a) segment utterances by speaker (`Speaker A/B/C…` from turn-taking, direct address, and context) and (b) produce the notes — one structured JSON response covering both. Cheapest and simplest; the notes and the labeled transcript stay consistent.
- **Two-pass (fallback if quality needs it):** a dedicated attribution call first, then notes. Only if the combined prompt underperforms on longer meetings.
- Best-effort by nature (no acoustic signal) — good for clear 2–4 person meetings; degrades on heavy cross-talk. The provider trait leaves room to bolt on real diarization later without reworking the pipeline.

**Speaker rename UI:** click "Speaker A" → type "Maria"; propagates through the stored transcript.

**Notes (Claude API `claude-sonnet-5`):** input the timestamped transcript (3hr ≈ 25–35k tokens, one call, no chunking). Force structured JSON via tool-use schema — in the combined approach this same call also carries the speaker-segmented transcript: `speaker_segments[{speaker, start_ms, end_ms, text}], title, tldr, summary, key_points[], decisions[], action_items[{task, owner, due}], open_questions[], highlights[{quote, speaker, timestamp_ms}]`. Highlights carry timestamps → clickable to jump-play audio.

**Pipeline UX:** runs automatically when recording stops; progress surfaced in the meeting list; retry on failure (audio safe on disk, every step idempotent). Because processing is server-side, a meeting recorded on the phone is transcribed and written to Notion without the desktop being involved at all.

## Phase 3 — Notion + MCP

**Notion (direct REST API from Rust, not MCP — the app is a service, MCP is for AI clients):**
- One-time setup: create/select a "Meetings" database (Name, Date, Duration, Attendees, Action Items rollup, Status)
- Per meeting: page with notes sections + toggle block containing the full speaker-labeled transcript
- Notion is the canonical cross-device store; local SQLite is the per-device cache/index

**MCP server:** same binary run as `ogma --mcp` (stdio transport, `rmcp` crate), reading the local SQLite:
- `list_meetings(query?, date_range?)`, `search_transcript(query)` (SQLite FTS5), `get_meeting_notes(id)`, `get_action_items(status?)`
- Register in Claude Code: `claude mcp add ogma -- E:\Work\ogma\...\ogma.exe --mcp`

## Phase 4 — macOS, then iOS

- **macOS:** same codebase; verify cpal input + mic permission (`NSMicrophoneUsageDescription`), sleep prevention. Expected near-free.
- **iOS (Tauri 2 mobile):** recording UI + pipeline (upload directly from phone; Notion is the shared store so notes appear everywhere). Native recording via `tauri-plugin-audio-recorder` (M4A on mobile) + `UIBackgroundModes: audio` for screen-off recording. **Known risk:** Tauri iOS + long background recording is the least-proven part — validate with a spike (1hr locked-screen recording) before building the full iOS UI. Fallback: keep-screen-on foreground recording, or record with Voice Memos and add an "import audio file" path (worth having on desktop anyway).

## Key crates/libs

`cpal` (audio capture), `hound` (WAV), audio transcode to compressed for the 25MB cap — bundle **ffmpeg as a Tauri sidecar** (simplest, also reused to concat segments) or the `opus`/`audiopus` crate, `rusqlite` + FTS5, `reqwest` (OpenAI/Claude/Notion REST), `rmcp` (MCP server), `serde_json`, `tokio`. Frontend: Vite + TS, plain or lightweight components. API keys (OpenAI + Anthropic + Notion token) entered in settings, stored in the app config/OS keychain.

## Verification

1. **Recording robustness:** record 10-min test with pause/resume; kill the app mid-recording → confirm segments survive and meeting is recoverable. Confirm no sleep interruption.
2. **Pipeline:** record a short 2-person conversation → verify chunk transcode stays <25MB, Whisper transcript stitches with correct timestamps, Claude speaker attribution is sensible, speaker rename works, notes JSON renders (summary/action items/highlights), highlight click seeks audio.
3. **Notion:** page appears in the Meetings DB with correct properties + transcript toggle.
4. **MCP:** `claude mcp add` the server, then from Claude Code ask "what were the action items from my last meeting" → correct results via `get_action_items`.
5. **Long-run test:** one real 1hr+ meeting end-to-end; check memory stays flat during recording and total cost ≈ **~$0.40/hr** (Whisper ~$0.36/hr + notes). Sanity-check Claude speaker attribution accuracy on a real multi-person meeting — this is the weakest link.

## Suggested build order

Phase 1 → 2 gives a fully useful single-machine product fast; 3 adds the integrations; 4 extends platforms. Each phase is independently shippable.

## Backlog — future feature ideas (post-v1, unscheduled)

Ideas being tracked but not yet designed or committed. Order is rough priority; none block Phase 4.

1. **More note-taking destinations beyond Notion.** Investigate which popular note-taking apps people would want transcripts/meeting notes pushed to (e.g. Google Docs, Obsidian, OneNote, Apple Notes, Evernote) and offer a selectable set of export destinations. Architecturally this means generalizing the Notion sync into a `SyncDestination`-style trait, mirroring how `TranscriptionProvider`/`NotesProvider` already abstract the AI side. Starts with a research task: shortlist candidates by API availability and user demand.

2. **Quick-start recording from the menu bar / system tray.** On macOS, a persistent top-bar (menu bar) icon that starts/stops recording in one click; same idea on Windows via the system tray. Optionally a global keyboard shortcut/command to start recording without opening the main window. Tauri 2 has tray + global-shortcut APIs, so this is likely a thin layer over the existing `Recorder`. **Implemented** (`src-tauri/src/tray.rs`): tray icon with Start/Stop toggle (red-dot icon while recording), Open/Quit menu, `Ctrl+Shift+R` (`Cmd+Shift+R` on macOS) global toggle; closing the window now hides to the tray, Quit finalizes any active recording first.

3. **Make Notion optional — in-app notes first, external link second.** Today a Notion connection is effectively required to have a canonical home for notes. Transition to: step 1, notes/transcripts are fully first-class *in the app* (SQLite already stores everything — this is mostly UX: viewing, editing, exporting without Notion configured); step 2, linking to Notion (or any destination from idea #1) becomes an optional "connect your favorite app" integration. **Note:** this revises the "Notion is the canonical cross-device store" locked-in decision — cross-device sync strategy needs a rethink when it lands.
