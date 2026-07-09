import { convertFileSrc } from "@tauri-apps/api/core";
import { api, errorMessage } from "../api";
import { armedButton, h } from "../dom";
import { formatDuration, friendlyDate, speakerColor } from "../format";
import { pipelineSteps } from "../status";
import { progressByMeeting } from "../store";
import { toast } from "../toast";
import type { ActionItem, Highlight, MeetingDetail, ProgressEvent, TranscriptSegment } from "../types";
import type { Navigate, View } from "../view";

export function renderDetail(navigate: Navigate, meetingId: string): View {
  const el = h("div", { class: "screen detail-screen" }, h("div", { class: "empty" }, "Loading…"));
  let audio: HTMLAudioElement | null = null;
  let lastStatus = "";
  let tab: "notes" | "transcript" = "notes";

  function seek(ms: number) {
    if (!audio) return;
    audio.currentTime = ms / 1000;
    void audio.play();
    toast(`Jumped to ${formatDuration(ms)} — playing`);
  }

  function chipStyle(name: string): string {
    const c = speakerColor(name);
    return (
      `color:${c};` +
      `background:color-mix(in oklab, ${c} 12%, transparent);` +
      `border:1px solid color-mix(in oklab, ${c} 30%, transparent)`
    );
  }

  function speakerChip(name: string, allSpeakers: string[]): HTMLElement {
    const chip = h(
      "button",
      { class: "speaker-chip", style: chipStyle(name), title: "Click to rename this speaker" },
      name,
    );
    chip.addEventListener("click", () => {
      const input = h("input", { class: "speaker-input", value: name, maxLength: 60 });
      chip.replaceWith(input);
      input.focus();
      input.select();
      let done = false;
      const finish = async (commit: boolean) => {
        if (done) return;
        done = true;
        const next = input.value.trim();
        if (commit && next && next !== name) {
          try {
            await api.renameSpeaker(meetingId, name, next);
            if (allSpeakers.includes(next)) toast(`Merged into existing speaker "${next}"`, "success");
            else toast(`Renamed to ${next} — propagated everywhere`, "success");
            void load();
            return;
          } catch (e) {
            toast(errorMessage(e), "error");
          }
        }
        input.replaceWith(chip);
      };
      input.addEventListener("keydown", (e) => {
        if (e.key === "Enter") void finish(true);
        if (e.key === "Escape") void finish(false);
      });
      input.addEventListener("blur", () => void finish(false));
    });
    return chip;
  }

  function stamp(ms: number): HTMLElement {
    return h("button", { class: "turn-stamp", title: "Jump to this moment", onclick: () => seek(ms) }, formatDuration(ms));
  }

  // ── notes / transcript bodies ─────────────────────────────────────────────
  function bulletPanel(label: string, items: string[], mark: string, markClass: string): HTMLElement | null {
    if (items.length === 0) return null;
    const list = h("div", { class: "bullet-list" });
    for (const it of items) {
      list.append(
        h("div", { class: "bullet" }, h("span", { class: `bullet-mark ${markClass}` }, mark), h("span", null, it)),
      );
    }
    return h("div", { class: "notes-panel" }, h("div", { class: "section-label" }, label), list);
  }

  function notesBody(detail: MeetingDetail): HTMLElement {
    const { notes, action_items } = detail;
    const body = h("div", { class: "notes-body" });
    if (!notes) return body;

    body.append(
      h(
        "div",
        { class: "tldr" },
        h("div", { class: "tldr-label" }, "TL;DR"),
        h("div", { class: "tldr-text" }, notes.tldr),
      ),
    );

    const keyPanel = bulletPanel("KEY POINTS", notes.key_points, "▸", "ac");
    const decisionPanel = bulletPanel("DECISIONS", notes.decisions, "✓", "ok");
    if (keyPanel && decisionPanel) {
      body.append(h("div", { class: "notes-grid" }, keyPanel, decisionPanel));
    } else if (keyPanel || decisionPanel) {
      body.append((keyPanel ?? decisionPanel)!);
    }

    if (notes.summary.trim()) {
      const summary = h("div", { class: "notes-panel" }, h("div", { class: "section-label" }, "SUMMARY"));
      const text = h("div", { class: "summary-text" });
      for (const para of notes.summary.split(/\n\n+/).filter((p) => p.trim())) {
        text.append(h("p", null, para.trim()));
      }
      summary.append(text);
      body.append(summary);
    }

    if (action_items.length > 0) {
      const list = h("div", { class: "action-items" });
      for (const item of action_items) list.append(actionRow(item));
      body.append(h("div", { class: "notes-panel" }, h("div", { class: "section-label" }, "ACTION ITEMS"), list));
    }

    const openPanel = bulletPanel("OPEN QUESTIONS", notes.open_questions, "?", "ac");
    if (openPanel) body.append(openPanel);

    if (notes.highlights.length > 0) {
      const wrap = h("div", { class: "highlights" });
      wrap.append(h("div", { class: "section-label" }, "HIGHLIGHTS — click a timestamp to seek"));
      for (const hl of notes.highlights) wrap.append(highlightRow(hl));
      body.append(wrap);
    }

    return body;
  }

  function actionRow(item: ActionItem): HTMLElement {
    const checkbox = h("input", { type: "checkbox", checked: item.status === "done" });
    checkbox.addEventListener("change", async () => {
      try {
        await api.setActionItemStatus(item.id, checkbox.checked ? "done" : "open");
      } catch (e) {
        checkbox.checked = !checkbox.checked;
        toast(errorMessage(e), "error");
      }
    });
    return h(
      "label",
      { class: "action-item" },
      checkbox,
      h("span", { class: "action-task" }, item.task),
      h("span", { class: "flex-spacer" }),
      item.owner ? h("span", { class: "action-owner" }, item.owner) : null,
      item.due ? h("span", { class: "action-due" }, item.due) : null,
    );
  }

  function highlightRow(hl: Highlight): HTMLElement {
    return h(
      "div",
      { class: "highlight" },
      h(
        "div",
        { class: "highlight-quote" },
        `“${hl.quote}” `,
        h("span", { class: "highlight-speaker" }, `— ${hl.speaker}`),
      ),
      h(
        "button",
        { class: "highlight-jump", onclick: () => seek(hl.timestamp_ms) },
        `▶ ${formatDuration(hl.timestamp_ms)}`,
      ),
    );
  }

  function transcriptBody(segments: TranscriptSegment[]): HTMLElement {
    const speakers = [...new Set(segments.map((s) => s.speaker))];
    const body = h(
      "div",
      { class: "transcript" },
      h("div", { class: "transcript-hint" }, "Speaker labels are Claude-inferred — click a chip to rename"),
    );
    for (const seg of segments) {
      body.append(
        h(
          "div",
          { class: "turn" },
          speakerChip(seg.speaker, speakers),
          stamp(seg.start_ms),
          h("span", { class: "turn-text" }, seg.text),
        ),
      );
    }
    return body;
  }

  function pipelineCard(detail: MeetingDetail): HTMLElement {
    const { meeting } = detail;
    if (meeting.status === "error") {
      return h(
        "div",
        { class: "pipeline" },
        h("div", { class: "section-label" }, "PIPELINE — failed"),
        h("div", { class: "step-name pipeline-error" }, "Processing failed"),
        h("div", { class: "step-sub pipeline-error" }, meeting.error ?? "Unknown error"),
        h(
          "button",
          {
            class: "pill pill-accent",
            style: "align-self:flex-start",
            onclick: async () => {
              try {
                await api.retryPipeline(meetingId);
                toast("Retrying…");
              } catch (e) {
                toast(errorMessage(e), "error");
              }
            },
          },
          "↻ RETRY",
        ),
      );
    }

    if (meeting.status === "recording") {
      return h(
        "div",
        { class: "pipeline" },
        h("div", { class: "section-label" }, "RECORDING IN PROGRESS"),
        h("div", { class: "step-name" }, "Capturing audio…"),
        h("div", { class: "step-sub" }, "Notes appear here once you stop & process."),
      );
    }

    const progress = progressByMeeting.get(meetingId);
    const card = h(
      "div",
      { class: "pipeline" },
      h("div", { class: "section-label" }, "PIPELINE — runs in the cloud, audio safe on disk"),
    );
    for (const step of pipelineSteps(meeting.status)) {
      const stateLabel =
        step.state === "done"
          ? "done"
          : step.state === "active"
            ? (progress?.detail ?? "running")
            : "waiting";
      card.append(
        h(
          "div",
          { class: `step-row ${step.state}` },
          h("span", { class: `step-dot ${step.state}` }),
          h("div", { class: "step-main" }, h("span", { class: "step-name" }, step.name), h("span", { class: "step-sub" }, step.sub)),
          h("span", { class: "flex-spacer" }),
          h("span", { class: "step-state" }, stateLabel),
        ),
      );
    }
    card.append(h("div", { class: "pipeline-foot" }, "est. cost ≈ $0.40 / recorded hour · retry is idempotent"));
    return card;
  }

  // ── full render ───────────────────────────────────────────────────────────
  function render(detail: MeetingDetail) {
    const { meeting, segments, notes, audio_path } = detail;
    lastStatus = meeting.status;

    // Header
    const titleInput = h("input", { class: "detail-title", value: meeting.title, maxLength: 200 });
    titleInput.addEventListener("change", async () => {
      const t = titleInput.value.trim();
      if (!t) {
        titleInput.value = meeting.title;
        return;
      }
      try {
        await api.setMeetingTitle(meetingId, t);
        toast("Title updated", "success");
      } catch (e) {
        toast(errorMessage(e), "error");
      }
    });

    const back = h("button", { class: "link-btn", onclick: () => navigate({ name: "library" }) }, "← LIBRARY");

    const metaText = `${friendlyDate(meeting.created_at)}${meeting.duration_ms > 0 ? ` · ${formatDuration(meeting.duration_ms)}` : ""}`;
    const head = h(
      "div",
      { class: "detail-head" },
      titleInput,
      h("span", { class: "detail-meta" }, metaText),
      h("span", { class: "flex-spacer" }),
      meeting.notion_page_id
        ? h(
            "a",
            {
              class: "notion-chip",
              href: `https://www.notion.so/${meeting.notion_page_id.replace(/-/g, "")}`,
              target: "_blank",
            },
            "◆ synced to Notion",
          )
        : null,
    );

    const children: (Node | null)[] = [back, head];

    // Audio player
    audio = null;
    if (audio_path) {
      audio = h("audio", { controls: true, preload: "metadata" });
      audio.src = convertFileSrc(audio_path);
      children.push(h("div", { class: "player-bar" }, audio));
    }

    // Body: tabs when we have notes; transcript-only or pipeline otherwise
    if (notes) {
      const tabNotes = h("button", { class: `tab ${tab === "notes" ? "active" : ""}` }, "Notes");
      const tabTrans = h("button", { class: `tab ${tab === "transcript" ? "active" : ""}` }, "Transcript");
      const region = h("div");
      const paintBody = () => {
        tabNotes.classList.toggle("active", tab === "notes");
        tabTrans.classList.toggle("active", tab === "transcript");
        region.replaceChildren(
          tab === "notes" ? notesBody(detail) : transcriptBody(segments),
        );
      };
      tabNotes.addEventListener("click", () => {
        tab = "notes";
        paintBody();
      });
      tabTrans.addEventListener("click", () => {
        tab = "transcript";
        paintBody();
      });
      children.push(h("div", { class: "tab-row" }, tabNotes, tabTrans));
      children.push(region);
      paintBody();
    } else {
      children.push(pipelineCard(detail));
      if (segments.length > 0) children.push(transcriptBody(segments));
    }

    // Danger zone
    children.push(
      h(
        "div",
        { class: "danger-row" },
        armedButton("DELETE MEETING", "DELETE PERMANENTLY?", "pill pill-small pill-danger", async () => {
          try {
            await api.deleteMeeting(meetingId);
            navigate({ name: "library" });
          } catch (e) {
            toast(errorMessage(e), "error");
          }
        }),
      ),
    );

    el.replaceChildren(...children.filter((c): c is Node => c != null));
  }

  async function load() {
    try {
      const detail = await api.getMeetingDetail(meetingId);
      render(detail);
    } catch (e) {
      el.replaceChildren(
        h("div", { class: "empty" }, `Could not load meeting: ${errorMessage(e)}`),
        h("button", { class: "link-btn", onclick: () => navigate({ name: "library" }) }, "← LIBRARY"),
      );
    }
  }

  // Live refresh while processing; leave the view alone once done so playback
  // isn't interrupted.
  const onProgress = (e: Event) => {
    const p = (e as CustomEvent<ProgressEvent>).detail;
    if (p.meeting_id === meetingId && lastStatus !== "done") void load();
  };
  const onChanged = () => {
    if (lastStatus !== "done") void load();
  };
  window.addEventListener("ogma:progress", onProgress);
  window.addEventListener("ogma:changed", onChanged);

  void load();

  return {
    el,
    destroy() {
      window.removeEventListener("ogma:progress", onProgress);
      window.removeEventListener("ogma:changed", onChanged);
      audio?.pause();
    },
  };
}
