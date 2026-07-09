import { convertFileSrc } from "@tauri-apps/api/core";
import { api, errorMessage } from "../api";
import { armedButton, h } from "../dom";
import { formatDuration, friendlyDate, speakerColor } from "../format";
import { isProcessing, statusBadge } from "../status";
import { progressByMeeting } from "../store";
import { toast } from "../toast";
import type { MeetingDetail, ProgressEvent } from "../types";
import type { Navigate, View } from "../view";

export function renderDetail(navigate: Navigate, meetingId: string): View {
  const el = h("div", { class: "view view-detail" }, h("div", { class: "empty" }, "Loading…"));
  let audio: HTMLAudioElement | null = null;
  let lastStatus = "";

  function seek(ms: number) {
    if (!audio) return;
    audio.currentTime = ms / 1000;
    void audio.play();
  }

  function speakerEl(name: string, allSpeakers: string[]): HTMLElement {
    const span = h(
      "button",
      {
        class: "speaker",
        style: `color: ${speakerColor(name)}`,
        title: "Click to rename this speaker",
      },
      name,
    );
    span.addEventListener("click", () => {
      const input = h("input", { class: "speaker-input", value: name, maxLength: 60 });
      span.replaceWith(input);
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
            if (allSpeakers.includes(next)) {
              toast(`Merged into existing speaker "${next}"`, "success");
            }
            void load();
            return;
          } catch (e) {
            toast(errorMessage(e), "error");
          }
        }
        input.replaceWith(span);
      };
      input.addEventListener("keydown", (e) => {
        if (e.key === "Enter") void finish(true);
        if (e.key === "Escape") void finish(false);
      });
      input.addEventListener("blur", () => void finish(false));
    });
    return span;
  }

  function timestampBtn(ms: number): HTMLElement {
    return h(
      "button",
      { class: "stamp", title: "Jump to this moment", onclick: () => seek(ms) },
      formatDuration(ms),
    );
  }

  function render(detail: MeetingDetail) {
    const { meeting, segments, notes, action_items, audio_path } = detail;
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

    const header = h(
      "div",
      { class: "detail-header" },
      h(
        "button",
        { class: "btn btn-small", onclick: () => navigate({ name: "home" }) },
        "← Back",
      ),
      titleInput,
      statusBadge(meeting),
    );
    const meta = h(
      "div",
      { class: "detail-meta" },
      `${friendlyDate(meeting.created_at)}${meeting.duration_ms > 0 ? ` · ${formatDuration(meeting.duration_ms)}` : ""}`,
      meeting.notion_page_id
        ? h(
            "a",
            {
              class: "notion-chip",
              href: `https://www.notion.so/${meeting.notion_page_id.replace(/-/g, "")}`,
              target: "_blank",
            },
            "Open in Notion ↗",
          )
        : null,
    );

    // Audio player
    audio = null;
    let player: HTMLElement | null = null;
    if (audio_path) {
      audio = h("audio", { controls: true, preload: "metadata" });
      audio.src = convertFileSrc(audio_path);
      player = h("div", { class: "player-bar" }, audio);
    }

    const body = h("div", { class: "detail-body" });

    // Notes (or processing/error placeholder)
    if (notes) {
      body.append(h("div", { class: "callout" }, notes.tldr));

      const summary = h("section", { class: "notes-section" }, h("h2", null, "Summary"));
      for (const para of notes.summary.split(/\n\n+/).filter((p) => p.trim())) {
        summary.append(h("p", null, para.trim()));
      }
      body.append(summary);

      const bulletSection = (title: string, items: string[]) => {
        if (items.length === 0) return;
        const ul = h("ul", { class: "notes-list" });
        for (const item of items) ul.append(h("li", null, item));
        body.append(h("section", { class: "notes-section" }, h("h2", null, title), ul));
      };
      bulletSection("Key points", notes.key_points);
      bulletSection("Decisions", notes.decisions);

      if (action_items.length > 0) {
        const list = h("div", { class: "action-items" });
        for (const item of action_items) {
          const checkbox = h("input", {
            type: "checkbox",
            checked: item.status === "done",
          });
          checkbox.addEventListener("change", async () => {
            try {
              await api.setActionItemStatus(item.id, checkbox.checked ? "done" : "open");
            } catch (e) {
              checkbox.checked = !checkbox.checked;
              toast(errorMessage(e), "error");
            }
          });
          const label = h(
            "label",
            { class: "action-item" },
            checkbox,
            h(
              "span",
              { class: "action-text" },
              item.task,
              item.owner ? h("span", { class: "action-owner" }, ` — ${item.owner}`) : null,
              item.due ? h("span", { class: "action-due" }, ` (due ${item.due})`) : null,
            ),
          );
          list.append(label);
        }
        body.append(h("section", { class: "notes-section" }, h("h2", null, "Action items"), list));
      }

      bulletSection("Open questions", notes.open_questions);

      if (notes.highlights.length > 0) {
        const cards = h("div", { class: "highlights" });
        for (const hl of notes.highlights) {
          cards.append(
            h(
              "blockquote",
              {
                class: "highlight-card",
                title: "Click to play from here",
                onclick: () => seek(hl.timestamp_ms),
              },
              h("p", null, `“${hl.quote}”`),
              h(
                "footer",
                null,
                h("span", { style: `color: ${speakerColor(hl.speaker)}` }, hl.speaker),
                ` · ${formatDuration(hl.timestamp_ms)}`,
              ),
            ),
          );
        }
        body.append(h("section", { class: "notes-section" }, h("h2", null, "Highlights"), cards));
      }
    } else {
      const progress = progressByMeeting.get(meetingId);
      const placeholder = h("div", { class: "processing-card" });
      if (meeting.status === "error") {
        placeholder.append(
          h("div", { class: "processing-title" }, "Processing failed"),
          h("div", { class: "processing-detail error-text" }, meeting.error ?? "Unknown error"),
          h(
            "button",
            {
              class: "btn",
              onclick: async () => {
                try {
                  await api.retryPipeline(meetingId);
                  toast("Retrying…");
                } catch (e) {
                  toast(errorMessage(e), "error");
                }
              },
            },
            "Retry",
          ),
        );
      } else if (isProcessing(meeting.status) || meeting.status === "recorded") {
        placeholder.append(
          h("span", { class: "spinner spinner-lg" }),
          h("div", { class: "processing-title" }, "Working on it…"),
          h(
            "div",
            { class: "processing-detail" },
            progress?.detail ?? "Notes will appear here when processing finishes.",
          ),
        );
      } else if (meeting.status === "recording") {
        placeholder.append(h("div", { class: "processing-title" }, "Recording in progress…"));
      }
      body.append(placeholder);
    }

    // Transcript
    if (segments.length > 0) {
      const speakers = [...new Set(segments.map((s) => s.speaker))];
      const transcript = h("section", { class: "notes-section transcript" }, h("h2", null, "Transcript"));
      for (const seg of segments) {
        transcript.append(
          h(
            "div",
            { class: "turn" },
            h("div", { class: "turn-head" }, speakerEl(seg.speaker, speakers), timestampBtn(seg.start_ms)),
            h("div", { class: "turn-text" }, seg.text),
          ),
        );
      }
      body.append(transcript);
    }

    // Danger zone
    body.append(
      h(
        "div",
        { class: "danger-row" },
        armedButton("Delete meeting", "Delete permanently?", "btn btn-danger-ghost", async () => {
          try {
            await api.deleteMeeting(meetingId);
            navigate({ name: "home" });
          } catch (e) {
            toast(errorMessage(e), "error");
          }
        }),
      ),
    );

    el.replaceChildren(header, meta, ...(player ? [player] : []), body);
  }

  async function load() {
    try {
      const detail = await api.getMeetingDetail(meetingId);
      render(detail);
    } catch (e) {
      el.replaceChildren(
        h("div", { class: "empty" }, `Could not load meeting: ${errorMessage(e)}`),
        h("button", { class: "btn", onclick: () => navigate({ name: "home" }) }, "← Back"),
      );
    }
  }

  // Live refresh while this meeting is still being processed; leave the view
  // alone once it's done so playback isn't interrupted.
  const onProgress = (e: Event) => {
    const p = (e as CustomEvent<ProgressEvent>).detail;
    if (p.meeting_id === meetingId) void load();
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
