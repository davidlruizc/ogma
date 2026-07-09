---
name: address-review
description: Address the review comments on an Ogma PR — fetch unresolved review threads, fix each finding in an isolated git worktree, verify (cargo check + cargo test + tsc), commit, push, reply to every thread with what was done and the commit SHA, and resolve the threads. Findings the fixer disagrees with get a pushback reply and stay unresolved (escalation signal). Used standalone (`/address-review <pr>`) or by /pr-autopilot.
metadata:
  short-description: Fix an Ogma PR's review comments, push, reply to and resolve each thread
---

# Address Review

## When to use

- `/address-review <pr-number>` — the user wants the review comments on a PR fixed.
- Invoked by `/pr-autopilot` between review rounds.

Takes exactly one PR number. If none is given, ask (interactive) or fail loudly (when invoked by the orchestrator — the orchestrator always passes one).

## Outcome contract

Every unresolved review thread on the PR ends in exactly one of these states:

1. **Fixed** — code changed, committed, pushed; thread has a reply naming the fix + commit SHA; thread **resolved**.
2. **Disputed** — fixer concluded the finding is wrong or not worth doing; thread has a reply explaining why; thread left **unresolved**. Never resolve a thread you didn't fix.
3. **Already addressed** — the concern no longer applies (fixed by a later commit, or the flagged code was removed); reply says so with evidence; thread **resolved**.

No thread is ever silently skipped, and nothing is force-pushed.

## Workflow

### Phase 1: Collect the work list

Unresolved review threads are the durable work list (survives session restarts — never track this in conversation state):

```bash
gh api graphql -f query='
query($owner:String!, $repo:String!, $pr:Int!) {
  repository(owner:$owner, name:$repo) {
    pullRequest(number:$pr) {
      headRefName
      baseRefName
      reviewThreads(first:100) {
        nodes {
          id
          isResolved
          isOutdated
          path
          line
          comments(first:20) {
            nodes { databaseId body author { login } url }
          }
        }
      }
    }
  }
}' -f owner=davidlruizc -f repo=ogma -F pr=<number>
```

Filter to `isResolved: false`. If the list is empty, report "nothing to address" and stop — do not invent work.

Parse each thread's first comment for the severity tag, focus area, and suggestion (the pr-reviews skill writes them in a consistent format).

### Phase 2: Isolated worktree

Never touch the user's working tree — they may be mid-work on another branch. Use the session scratchpad directory for the worktree path.

```bash
git fetch origin <headRefName>
git worktree add "$SCRATCHPAD/pr-<number>" <headRefName>
```

(If a stale worktree from a previous run exists at that path, `git worktree remove --force` it first, then re-add.) On Windows, `$SCRATCHPAD` is the session scratchpad dir given in the environment; use a path under it.

All fixing, verification, and pushing happens inside this worktree. Remove it (`git worktree remove`) when the run ends, success or failure.

### Phase 3: Fix

Work through the findings grouped by file. For each finding, read the surrounding code first — the review comment describes the symptom; fix the cause, in keeping with the project conventions (`CLAUDE.md` "Locked-in decisions" + "Implementation notes", `PLAN.md`): keep the provider-trait seam, keep pipeline stages idempotent and the resume point derived from SQLite, keep WAV rotation at 5-min crash-safe segments, keep `_ms` integer timestamps, never log/commit a key, keep MCP stdout JSON-RPC-only, keep cpal stream handling on the dedicated `!Send` audio thread.

The security-critical modules — `config.rs` (keychain/secrets), `storage.rs` (SQL), `notion.rs`, `mcp.rs`, and the recording FFI in `recording/wake.rs` — should be fixed directly and carefully, never rushed.

If a finding is wrong (misread the code, flags a decision that doesn't apply, the "simpler" suggestion breaks a documented constraint like the trait seam or crash-safety): don't fix it. Record it as **disputed** with a concrete justification for Phase 6.

### Phase 4: Verify

Inside the worktree, run the checks relevant to what changed:

```bash
cargo check --workspace --all-targets      # Rust changes
cargo test -p ogma-core                     # if the fix touched ogma-core logic
npx tsc --noEmit                            # frontend/TS changes
npm run build                               # if the fix touched the frontend bundle
```

If a fix touched the recording robustness path, also consider the ignored real-hardware test (`cargo test -p ogma-core --test recorder_integration -- --ignored`) — but that needs audio hardware, so only run it when the environment has a mic and note in the output if it was skipped. If verification fails because of a fix, repair the fix — never push red.

### Phase 5: Commit and push

- One commit per logical group of findings (a single commit for the whole batch is fine when the fixes are small). Message format: `review: <short description> (addresses PR #<number> review)`. End the commit message with the project's required co-author trailer:

  ```
  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  ```

- `git push origin <headRefName>` from the worktree. **Never force-push** — the review rounds rely on the commit history to scope verification.
- These branches are **not stacked** (all base off `main`), so there are no child branches to worry about after a push. (If that ever changes, list trailing children as "needs rebase" for the human instead of rebasing automatically.)

### Phase 6: Reply and resolve

For each thread, reply to its first comment:

```bash
gh api repos/davidlruizc/ogma/pulls/<number>/comments/<first_comment_databaseId>/replies \
  -f body="✅ Fixed in <short-sha>: <one-to-three sentences on what was changed and why it addresses the finding>"
```

Then resolve fixed / already-addressed threads:

```bash
gh api graphql -f query='
mutation($threadId:ID!) {
  resolveReviewThread(input:{threadId:$threadId}) { thread { isResolved } }
}' -f threadId=<thread id>
```

Disputed threads get a reply (`⚠️ Not changing this: <justification>`) and are **left unresolved**.

## Output expected at end of run

```text
ADDRESS_RESULT pr=<number> fixed=<count> disputed=<count> already_addressed=<count> pushed=<sha|none>

| Thread (file:line)            | Outcome  | Commit |
| ----------------------------- | -------- | ------ |
| crates/ogma-core/src/x.rs:42  | fixed    | abc123 |
| src/views/settings.ts:10      | disputed | —      |
```

The `ADDRESS_RESULT` line is machine-readable for `/pr-autopilot`.
