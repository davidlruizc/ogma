---
name: pr-autopilot
description: Fully automated PR review cycle for Ogma — run /pr-reviews in auto mode, if findings come back run /address-review to fix/push/reply, then re-review in verification rounds until the PR is clean, disputed, or the round cap is hit. If a PR is clean on round 1, post the why-it's-clean comment and stop. No user interaction until the final summary.
metadata:
  short-description: Review → fix → re-review loop until an Ogma PR converges; escalates disputes
---

# PR Autopilot

## When to use

- `/pr-autopilot <pr-number>` — run the full cycle on one PR.
- `/pr-autopilot` — run it on every open PR in the queue, sequentially.

This skill is **autonomous**: it never asks the user anything mid-run. All state lives on GitHub (review threads, labels, marker comments), so a killed run can simply be re-invoked and resumes from the unresolved threads.

Note: this repo (`davidlruizc/ogma`) is single-author — every PR is the user's own, so all review outcomes are COMMENT reviews + `autopilot:*` labels (GitHub blocks formal approve/request-changes on own PRs). The orchestrator reads those labels, not GitHub review states. The branches are **flat** (all base `main`, not stacked), so PRs are independent — but still run them sequentially to keep output readable.

## Constants

- `MAX_ROUNDS = 3`
- Progress rule: the medium+ finding count must **strictly decrease** each round. If round N has ≥ as many findings as round N−1, stop and escalate — the loop is churning, not converging.

## The loop (per PR)

```text
round = resume point (1 + highest round marker found on the PR, else 1)
while round <= MAX_ROUNDS:
    1. REVIEW:   /pr-reviews --auto <pr> --round <round>
                 → parse AUTOPILOT_RESULT (verdict, findings, disputed)
    2. if verdict == clean:
         → done. PR has the clean comment/label (round 1: "nothing to change";
           round ≥2: all findings verified fixed). END.
    3. if disputed > 0 and findings == disputed:
         → nothing left but disagreements. END as ESCALATED.
    4. if findings did not strictly decrease vs previous round:
         → END as STALLED (escalate).
    5. ADDRESS:  /address-review <pr>
                 → parse ADDRESS_RESULT (fixed, disputed, pushed)
                 if pushed == none and fixed == 0: END as ESCALATED
                 (nothing was fixable — all disputed — re-reviewing is pointless)
    6. round += 1
if round > MAX_ROUNDS: END as ROUND-CAP (escalate)
```

### Resume behavior

Before starting round 1, check the PR for existing `<!-- pr-autopilot:review round=N -->` / `<!-- pr-autopilot:clean round=N -->` markers and unresolved threads:

- `autopilot:clean` label present and no unresolved threads → already converged; report and skip.
- Unresolved threads exist → a previous run stopped mid-cycle; start at the ADDRESS step, then continue the loop at the next round number.

### Invocation mechanics

Run each step via the Skill tool (`pr-reviews` with args `--auto <pr> --round <N>`, then `address-review` with args `<pr>`). Parse the trailing `AUTOPILOT_RESULT` / `ADDRESS_RESULT` lines from each step's output to drive the loop. If a step's output is missing its result line, treat the step as failed, retry it once, then escalate.

## End states

Every PR ends in exactly one:

| State         | Meaning                                                        | GitHub artifact                                  |
| ------------- | -------------------------------------------------------------- | ------------------------------------------------ |
| **CLEAN**     | No findings, or all findings fixed and verified                 | `autopilot:clean` label + clean comment          |
| **ESCALATED** | Only disputed findings remain — human judgment needed           | Unresolved threads with ⚠️ pushback replies      |
| **STALLED**   | Findings stopped decreasing — reviewer/fixer are ping-ponging   | `autopilot:changes-requested` label still set    |
| **ROUND-CAP** | `MAX_ROUNDS` exhausted without convergence                      | `autopilot:changes-requested` label still set    |

For ESCALATED / STALLED / ROUND-CAP, post one comment on the PR:

```bash
gh pr comment <number> --body "<!-- pr-autopilot:escalated -->
🔺 Autopilot stopped after round <N>: <reason>. Human review needed on: <list of unresolved thread links>"
```

## Final summary (printed to the user)

```text
## Autopilot session complete

| PR  | Rounds | End state | Fixed | Disputed | Notes |
| --- | ------ | --------- | ----- | -------- | ----- |
| #6  | 2      | CLEAN     | 3     | 0        | —     |

<one line per ESCALATED/STALLED PR explaining exactly what the human must decide>
```

## Lessons baked in

- **Converge, don't re-litigate.** Rounds ≥ 2 verify prior findings and inspect only the fix commits (enforced in pr-reviews Phase 3b). A fresh full review every round never reaches consensus.
- **Disputes are an end state, not a loop state.** A finding the fixer pushed back on is never re-posted by a later round; it stays as an unresolved thread for the human.
- **GitHub is the source of truth.** Labels + marker comments + thread resolution state make every run resumable and idempotent; conversation memory is never load-bearing.
- **Own-PR account can't formally approve/request-changes** — the pr-reviews skill substitutes COMMENT reviews + `autopilot:*` labels; this orchestrator reads those labels, not review states.
