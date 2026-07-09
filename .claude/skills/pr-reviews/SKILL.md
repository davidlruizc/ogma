---
name: pr-reviews
description: Walk through my open Ogma PRs one by one. For each PR, runs parallel review agents checking architecture (the locked-in decisions in CLAUDE.md/PLAN.md), Rust/TS code quality, test depth, and security (API-key handling). Presents medium+ severity findings interactively — I decide to approve, request changes (inline comments), or skip. Supports `--auto [pr-number] [--round N]` for headless runs (used by /pr-autopilot): posts findings as inline comments automatically instead of asking.
metadata:
  short-description: Review my open Ogma PRs — architecture, Rust/TS quality, tests, secrets
---

# PR Reviews

## When to use

Trigger when the user says any of:

- "Review my PRs", "go through my PRs", "pr-reviews", "/pr-reviews"
- "What PRs do I have open?"
- "Check my PR queue"

## Modes

- **Interactive (default)** — runs in the main conversation thread, presents findings one PR at a time, and waits for user action before moving on.
- **Auto (`--auto [pr-number] [--round N]`)** — headless; never asks. Reviews the given PR (or the whole queue if no number), posts findings automatically, and applies state labels. This is the mode `/pr-autopilot` invokes. `--round N` (default 1) controls scope: round 1 is a full review of the diff vs `main`; rounds ≥ 2 are **verification rounds** (see Phase 3b).

### Own-PR constraint (the normal case here, applies to both modes)

This repo (`davidlruizc/ogma`) is single-author: **every open PR is authored by the account running the review** (`davidlruizc`). GitHub rejects `APPROVE` and `REQUEST_CHANGES` review events on a PR authored by the authenticated account, so the own-PR path below is the *default*, not an edge case. Confirm with `gh api user --jq .login` vs the PR author, but expect them to match:

- Instead of `REQUEST_CHANGES` → post the review with `"event": "COMMENT"` (inline comments work identically) and add label `autopilot:changes-requested`.
- Instead of `--approve` → post a short comment summarizing what was checked and why it passes, and add label `autopilot:clean` (remove `autopilot:changes-requested` if present).

Ensure labels exist (idempotent): `gh label create "autopilot:changes-requested" --color D93F0B --force` and `gh label create "autopilot:clean" --color 0E8A16 --force`.

## Outcome contract

Every in-scope PR ends in exactly one of these states:

1. **Approved / clean** — on own PRs: the clean comment + `autopilot:clean` label. (`gh pr review --approve` only if a PR is ever authored by someone else.)
2. **Changes requested** — inline review comments posted on specific file:line for each finding, with `COMMENT` event + `autopilot:changes-requested` label (own-PR path); `REQUEST_CHANGES` only for other-authored PRs.
3. **Skipped** — moved to next PR without any GitHub action (interactive mode only).

The loop ends when all queued PRs have been processed.

## Focus areas

Ogma is a **Tauri 2 desktop app**: a Rust core crate (`crates/ogma-core/`) that records in-person meetings, transcribes them via the OpenAI Whisper API, generates speaker labels + notes via the Claude API, syncs to Notion, and exposes a local MCP server — plus a thin Tauri layer (`src-tauri/`) and a vanilla-TypeScript frontend (`src/`). This skill targets **judgment-heavy review areas** that `cargo clippy`, `tsc`, and CodeRabbit miss. Each finding must be classified into one of these focus areas.

The invariants this project protects (from `CLAUDE.md` "Locked-in decisions" + "Implementation notes" and `PLAN.md`):

1. **Providers sit behind Rust traits** (`TranscriptionProvider`, `NotesProvider`) — the pipeline talks to the traits, never to Whisper/Claude HTTP directly.
2. **The pipeline is idempotent and crash-safe.** Resume points are derived from what's in SQLite (no transcript → transcribe; no notes → summarize; no Notion page → sync) — never from a remembered stage. Source audio on disk is sacred until the meeting is fully processed.
3. **Recording never loses more than ~5 minutes.** Rotating 5-min 16kHz-mono-16-bit WAV segments with crash-safe header `repair()`.
4. **API keys (OpenAI, Anthropic, Notion) never get hardcoded, committed, or logged** — they live in app config / the OS keychain.
5. **MCP stdio mode prints nothing to stdout except JSON-RPC** — all logging goes to stderr.

### 1. Architecture

Wrong approach to the problem. The implementation works but the design fights a locked-in decision.

Flag when:

- **Bypassing the provider traits** — `pipeline.rs` (or anything else) calling `whisper.rs` / `claude.rs` HTTP concretely instead of through `TranscriptionProvider` / `NotesProvider`, or new STT/notes logic wired in without a trait seam.
- **Breaking pipeline idempotency / resume** — a stage that remembers "where it left off" in memory or a status column instead of deriving the resume point from stored data (transcript/notes/notion-page presence); a step that isn't safely re-runnable after a crash; deleting or mutating source audio before the meeting is fully processed.
- **Weakening crash-safety** — enlarging the WAV segment rotation window (must stay ~5 min), buffering more than one segment in memory, skipping the header `repair()` path, or finalize logic that can lose a segment on kill.
- **Re-emitting transcript text through Claude** — the Claude call returns speaker labels as utterance **index ranges** (`speaker_assignments`) + notes via `output_config.format` json_schema, and `assemble()` merges labels onto stored utterances **locally**. A change that makes Claude re-output the full transcript (blowing up tokens / forcing streaming) is an architecture regression.
- **Reintroducing a transcode/ffmpeg step** — 16kHz mono WAV segments (~9.6MB) are already under the 25MB Whisper cap; there is deliberately **no ffmpeg/transcode step**. Adding one back (or an `opus`/`audiopus` dependency for it) contradicts a documented deviation — flag unless the PR explicitly justifies crossing the 25MB cap.
- **MCP stdout pollution** — anything in `--mcp` mode writing to stdout other than JSON-RPC (a stray `println!`, a progress bar, a `dbg!`). Logging must go to stderr.
- **Notion/SQLite role confusion** — treating local SQLite as canonical, or routing the app's own Notion writes through MCP (MCP is read-only, for AI clients; the app writes Notion via the direct REST client in `notion.rs`).
- **Audio-thread / Send violations** — `Recorder` uses a dedicated audio thread because cpal streams are `!Send`; `stop()` runs via `spawn_blocking`. Moving cpal stream handling onto an async task or across an `await` is wrong.
- **Relitigating a locked decision** — reintroducing acoustic diarization, local/on-device AI, or a non-cloud STT for v1. These were deliberately deferred (see PLAN.md "Why cloud"); flag, don't silently accept.
- **Over-engineering** — a new abstraction where the pipeline / storage / provider traits already cover it; a framework added to the vanilla-TS frontend without cause.

### 2. Code quality

Spaghetti code, vibe-coded helpers, code smells — in Rust or TS.

Flag when:

- **Panics on the library path** — `.unwrap()` / `.expect()` / `panic!` / array-index panics in `ogma-core` code paths that can fail at runtime (I/O, HTTP, parsing, audio), instead of returning the crate's error type (`error.rs`). Tests and truly-infallible invariants are fine.
- **Blocking in async** — sync file/network/`std::thread::sleep` or other blocking calls inside async fns without `spawn_blocking` (the recorder's `stop()` is the established pattern).
- **Timestamp unit drift** — timestamps must be `_ms` integer throughout (`start_ms`, `end_ms`). Introducing seconds/floats, or mixing units when stitching Whisper chunk offsets, is a correctness smell.
- **Secret leakage into logs/errors** — an API key or token echoed in a `log`/`eprintln!`/error `Display`/`Debug`, a response body, or a panic message.
- **Config read scattered** — API keys / endpoints read via ad-hoc `std::env`/hardcoded literals instead of the `config.rs` (+ keychain) path.
- **God functions / copy-paste** — a pipeline stage or command handler doing too many things; duplicated blocks that should be a shared fn; deeply nested conditionals that should be early returns / `?`.
- **Vibe-coded utility layers** — wrapper modules that add indirection without value, or a heavy dependency added for something the std lib / an existing crate already does.
- **Frontend smells** — untyped `invoke` calls bypassing the typed wrappers in `api.ts`; DOM/business logic tangled into event handlers; `any` where `types.ts` has a type.

### 3. Test quality

Happy-path-only tests that give false confidence. (The core has real tests — `recorder_integration.rs`, storage/pipeline units — so a change to a robustness-critical module with no test is itself a medium finding.)

Flag when:

- **Crash/robustness paths changed without tests** — WAV segment rotation, header `repair()`, finalize/concat, or the kill-mid-recording recovery path touched with no test exercising a truncated/partial segment.
- **Pipeline idempotency untested** — resume/retry logic changed without a test that re-runs a stage and asserts it's a no-op / produces the same result (the "derive resume point from SQLite" contract).
- **Timestamp-stitching untested** — chunk-offset math changed with no test asserting stitched `start_ms`/`end_ms` are monotonic and correctly offset (directly relevant to the "offset Whisper chunks" work).
- **Only the success case** — no error paths: a failing Whisper/Claude/Notion HTTP call, a malformed provider response, a `failed` stage.
- **Over-mocked** — mocking the provider trait so completely the pipeline's own stitching/assemble/resume logic is never exercised.
- **Tautological / implementation-detail assertions** — asserting on log strings or internal state rather than behavior; assertions that would pass even if the feature were broken.

### 4. Security

Spawn the `Security Engineer` subagent type for an independent review. The agent reviews the diff and returns security findings.

Additionally flag:

- **Secrets in the repo** — an OpenAI/Anthropic/Notion key or token committed in code, config defaults, fixtures, or a `.env` instead of OS keychain / user-entered settings. (PR "Store API secrets in the OS keychain" is exactly this surface — verify the migration doesn't leave a plaintext copy behind in the old config file or logs.)
- **Secret exposure at rest / in transit to the frontend** — a decrypted key crossing a Tauri command boundary to the webview, serialized into an event payload, or written to a world-readable config path.
- **Keychain misuse** — storing the key under a guessable/shared service name without user scoping; not removing the old plaintext value on migration; swallowing keychain errors so the app silently falls back to an insecure store.
- **Path / input injection** — a meeting id, filename, or MCP tool argument used to build a filesystem path or SQL without validation (path traversal into the audio/DB dir; SQL built by string concat instead of bound params in `storage.rs`).
- **MCP surface** — an MCP tool returning more than the caller should see, or taking an unvalidated argument that reaches the filesystem/DB.
- **Unsafe / FFI on the platform paths** — `unsafe` blocks for `SetThreadExecutionState` (Windows) or IOKit/`caffeinate` (macOS) sleep prevention, or mic-permission FFI, that mishandle handles/lifetimes or ignore error returns.

## Severity classification

Only surface **medium and above**. Low-severity findings are noise — clippy/CodeRabbit handle nits.

| Severity     | Criteria                                                        | Examples                                                                                              |
| ------------ | -------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| **Critical** | Security exposure, data/recording loss risk                    | API key committed or logged, decrypted secret sent to the webview, a change that can lose recorded audio, SQL injection via a meeting id |
| **High**     | Fundamentally wrong architecture, broken core contract         | Pipeline calling Whisper/Claude around the provider traits, resume point no longer derived from SQLite, MCP writing non-JSON-RPC to stdout, WAV rotation window enlarged past 5 min |
| **Medium**   | Code smells, shallow tests, unnecessary deps, unit drift       | `.unwrap()` on a fallible core path, blocking call in async, `_ms`→seconds drift, robustness path changed with no test, reintroduced ffmpeg step |

**Do not report low-severity findings.** Skip: naming preferences, minor style, "nice to have" suggestions, roughly-equivalent alternative approaches.

**Only assess introduced code changes.** Do not flag pre-existing issues the PR did not introduce or modify. The goal is to make things better one PR at a time — not to demand a refactor of everything a PR touches. Exception: if the new code _relies on_ a pre-existing broken/insecure pattern such that the new call site is specifically wrong, flag the new call site, not the pre-existing function.

**Docs-only / asset-only PRs** (e.g. a logo image, a README/PLAN edit): there is usually nothing at medium+. Verify the change doesn't contradict a locked-in decision or leak a secret, then take the clean path. Don't manufacture findings.

## Workflow

### Phase 1: Discover queue

**Auto mode with an explicit PR number:** skip discovery — the queue is just that PR. Otherwise:

This repo is `davidlruizc/ogma`, single-author, **not stacked** — every PR bases off `main`, so review each PR's own diff against `main`. Because the PRs are the user's own, discover by author, not by `review-requested:@me` (you can't request review from yourself):

```bash
gh pr list --author @me --state open \
  --json number,title,author,url,additions,deletions,changedFiles,baseRefName
```

Display the numbered queue to the user:

```text
## Review queue (N PRs)

| #   | PR                                        | Base | Size        |
| --- | ----------------------------------------- | ---- | ----------- |
| 1   | #6 Store API secrets in the OS keychain   | main | +219 / -7   |
| 2   | #4 Offset Whisper chunks by segment dur.  | main | +47 / -9    |
```

If empty: report "No open PRs" and stop.

### Phase 2: Fetch PR context

For the current PR:

1. Fetch metadata: `gh pr view <number> --json body,files,labels,headRefName,baseRefName`
2. Fetch diff: `gh pr diff <number>` (diff vs `main`).
3. Classify which layer(s) the PR touches, to focus the review:
   - **Recording core** — `crates/ogma-core/src/recording/` (`mod.rs`, `wav.rs`, `wake.rs`) — the crash-safety / audio-thread / sleep-prevention critical code.
   - **Pipeline & providers** — `pipeline.rs`, `providers/` (`whisper.rs`, `claude.rs`, `mod.rs`) — idempotency, trait seam, timestamp stitching, speaker assemble.
   - **Storage & config** — `storage.rs` (rusqlite + FTS5), `config.rs` (keys/keychain), `models.rs`, `error.rs`.
   - **Integrations** — `notion.rs`, `mcp.rs`.
   - **Tauri layer** — `src-tauri/src/lib.rs` (commands/events), `main.rs` (`--mcp` branch).
   - **Frontend** — `src/` (`api.ts`, `views/`, `types.ts`).
   - **Docs / assets** — `PLAN.md`, `CLAUDE.md`, images, config files.
4. If the diff is large (>2000 lines), fetch individual changed files with `gh api` / read them in context rather than relying on the raw diff alone.

### Phase 3: Parallel review

Launch **2 agents in parallel** (single message, multiple Agent tool calls):

#### a) Code review agent (`subagent_type: general-purpose`)

Prompt must include:

- The full PR diff (or file-by-file diffs for large PRs)
- The PR description/body
- The list of changed files with their paths
- The layer classification (recording / pipeline+providers / storage+config / integrations / Tauri / frontend / docs)
- **Focus areas 1–3** (Architecture, Code Quality, Test Quality) with the concrete examples above
- A pointer to the project conventions in `CLAUDE.md` (the "Locked-in decisions" + "Implementation notes (non-obvious)" sections) and `PLAN.md`: provider traits, idempotent/resume-from-SQLite pipeline, crash-safe 5-min WAV segments, Claude returns index ranges not transcript text, no ffmpeg step, MCP stdout purity, `_ms` timestamps, secrets never logged, `!Send` cpal audio thread.
- Instruction to classify each finding with severity (critical/high/medium) and focus area
- Instruction to skip low-severity findings entirely
- Instruction to return findings as a structured list with: severity, focus area, file:line, one-line summary, 2-3 sentence explanation with concrete suggestion

The agent should also read the actual source files (not just the diff) when it needs surrounding context to judge architecture or code quality — especially `pipeline.rs`, the provider traits in `providers/mod.rs`, and the recording modules, to catch bypasses of the trait seam or the resume/crash-safety contracts.

#### b) Security review agent (`subagent_type: Security Engineer`)

Prompt must include:

- The full PR diff
- The list of changed files
- The branch name
- The project's security-relevant invariants from focus area 4 (API keys never hardcoded/committed/logged; secrets live in config/OS keychain and must never cross the Tauri boundary into the webview; keychain migration must not leave plaintext behind; no path traversal via meeting id/filename; bound params not string-concat SQL in `storage.rs`; MCP tools validate arguments; `unsafe`/FFI on the sleep-prevention and mic-permission paths handled correctly)
- Instruction to review for security issues aligned with OWASP-style categories adapted to a local desktop app (secret management, local file/DB access, injection, unsafe FFI)
- Instruction to only report medium+ severity findings

### Phase 3b: Verification rounds (auto mode, `--round` ≥ 2)

A verification round exists to **converge**, not to re-litigate. Do NOT re-review the full diff. Instead:

1. Fetch this PR's review threads and prior autopilot review bodies (marker: `<!-- pr-autopilot:review round=N -->`):

   ```bash
   gh api graphql -f query='
   query($owner:String!, $repo:String!, $pr:Int!) {
     repository(owner:$owner, name:$repo) {
       pullRequest(number:$pr) {
         reviewThreads(first:100) {
           nodes { id isResolved isOutdated path line
             comments(first:20) { nodes { databaseId body author { login } } } } } } } }' \
     -f owner=davidlruizc -f repo=ogma -F pr=<number>
   ```

2. Determine the fix commits: commits pushed after the previous round's review timestamp. Fetch only their diff (`gh pr diff` limited to those files, or `gh api .../commits/<sha>`).
3. Launch ONE review agent whose scope is exactly:
   - For each finding from the previous round: is it actually fixed by the fix commits (not just claimed fixed in the thread reply)?
   - Do the fix commits themselves introduce any new medium+ issue? (Only the changed lines of the fix commits — findings on lines the fixes did not touch are **out of scope and must be discarded**.)
4. Include the Security Engineer agent only if the fix commits touch `config.rs`, `storage.rs`, `notion.rs`, `mcp.rs`, or the recording FFI paths (`recording/wake.rs`).
5. Verdict:
   - All prior findings fixed, no new findings → the PR is **clean** (Phase 5 auto: clean path).
   - Anything unfixed or newly introduced → post ONLY those as a new round of inline comments (Phase 5 auto: changes-requested path).
   - A prior finding whose thread has a pushback reply (fixer disagreed, thread left unresolved): do not repeat it as a new comment. Report it in the run output as **disputed** — the orchestrator escalates it to the human.

### Phase 4: Consolidate and present

Merge findings from both agents. Filter to medium+ severity (agents should already have done this, but double-check). Group by focus area.

**Auto mode:** print the findings block below for the transcript but omit the "What would you like to do?" section entirely — go straight to Phase 5's auto path.

Present using this template:

```text
## PR #<number>: <title> (@<author>)
<additions>+ / <deletions>- across <changedFiles> files | Touches: <recording|pipeline+providers|storage+config|integrations|Tauri|frontend|docs>

### Findings (<count> total — <critical_count> critical, <high_count> high, <medium_count> medium)

1. **[CRITICAL] Security: <One-line summary>**
   `<file>:<line>`
   <2-3 sentence explanation with concrete suggestion>

2. **[MEDIUM] Code Quality: <One-line summary>**
   `<file>:<line>`
   <2-3 sentence explanation with concrete suggestion>

### What would you like to do?
- **approve** — mark this PR clean (posts a clean comment + label; you author it, so no formal approval)
- **request-changes** — post inline comments on each finding
- **skip** — move to next PR without acting
```

If there are **zero findings** at medium+, present:

```text
## PR #<number>: <title> (@<author>)
<additions>+ / <deletions>- across <changedFiles> files | Touches: <...>

No medium+ findings. Looks clean.

### What would you like to do?
- **approve** — mark this PR clean
- **skip** — move to next PR without acting
```

### Phase 5: Act

**Auto mode — no user prompt.** Decide from the findings:

- **Zero medium+ findings (clean path):** since the PR author is the authenticated account, post a clean comment (do not attempt `gh pr review --approve` — GitHub will reject it):

  ```bash
  gh pr comment <number> --body "<!-- pr-autopilot:clean round=N -->
  ✅ Automated review round N: no medium+ findings. Checked: <layers touched> against the Ogma locked-in decisions (provider traits, idempotent/crash-safe pipeline, 5-min WAV segments, MCP stdout purity), test depth, and secret handling."
  ```

  Then swap labels: add `autopilot:clean`, remove `autopilot:changes-requested`.

- **Findings (changes-requested path):** post the inline review with `"event": "COMMENT"` and the body prefixed with the marker line `<!-- pr-autopilot:review round=N -->`. Then add label `autopilot:changes-requested` (remove `autopilot:clean` if present).

At the end of an auto run, output a machine-readable result block for the orchestrator:

```text
AUTOPILOT_RESULT pr=<number> round=<N> verdict=<clean|changes-requested> findings=<count> disputed=<count>
```

**Interactive mode — act on user decision:**

- **approve** (own PR): post the clean comment + `autopilot:clean` label as above. (`gh pr review <number> --approve` only if the PR is ever authored by someone else.)
- **request-changes**: Post inline review comments via the GitHub API using `--input` JSON, with `"event": "COMMENT"` (own PR) and `autopilot:changes-requested` label:

  ```bash
  gh api repos/davidlruizc/ogma/pulls/{number}/reviews \
    --method POST --input - <<'EOF'
  {
    "event": "COMMENT",
    "body": "<summary of findings>",
    "comments": [
      {"path": "<file>", "line": <line>, "body": "<finding>"}
    ]
  }
  EOF
  ```

  For each finding, construct a comment with the severity tag, explanation, and suggestion.

- **skip**: Move to next PR, no GitHub action.

### Phase 6: Next PR or summary

Loop back to Phase 2 for the next PR in the queue.

After all PRs are processed, print a summary:

```text
## Review session complete

| Action            | Count | PRs                |
| ----------------- | ----- | ------------------ |
| Clean             | 2     | #7, #8             |
| Changes requested | 1     | #6                 |
| Skipped           | 1     | #5                 |
| **Total**         | **4** |                    |
```

## Lessons baked in

- **Single-author repo — the own-PR path is the default.** All PRs are `davidlruizc`'s, so GitHub blocks formal approve/request-changes; every outcome is a COMMENT review + an `autopilot:*` label. Discover by `--author @me`, not `review-requested:@me`.
- **Flat branches, not stacked.** Every PR bases off `main`; review each PR's own diff against `main`.
- **`CLAUDE.md` "Locked-in decisions" + "Implementation notes" is the rubric.** Most "wrong approach" findings are violations of an already-documented decision — provider traits, resume-from-SQLite idempotency, 5-min crash-safe segments, Claude returning index ranges not transcript, no ffmpeg step, MCP stdout purity. Read them into the review agent's context.
- **The highest-value catches are recording-loss and secret-leak.** A change that can lose recorded audio, or an API key reaching logs / the repo / the webview, is critical. The keychain PR is the sharpest instance of the latter.

## Output expected at end of run

The summary table from Phase 6, plus a one-line note if any PRs had critical findings that need urgent attention.
