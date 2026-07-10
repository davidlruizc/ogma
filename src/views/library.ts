import { api, errorMessage } from "../api";
import { armedButton, h } from "../dom";
import { formatDuration, friendlyDate } from "../format";
import { isProcessing, statusChip, statusDot, statusInfo } from "../status";
import { progressByMeeting } from "../store";
import { toast } from "../toast";
import type { Meeting } from "../types";
import type { Navigate, View } from "../view";

export function renderLibrary(navigate: Navigate): View {
  let query = "";
  let meetings: Meeting[] = [];

  const searchInput = h("input", {
    class: "search-input",
    placeholder: "Search meetings…",
    spellcheck: false,
  });
  searchInput.addEventListener("input", () => {
    query = searchInput.value.trim().toLowerCase();
    paint();
  });

  const listEl = h("div", { class: "meeting-list" });

  const importBtn = h(
    "button",
    {
      class: "pill",
      title: "Import an audio file (WAV, M4A, MP3, FLAC, OGG) and run the normal pipeline",
      onclick: async () => {
        try {
          importBtn.disabled = true;
          importBtn.textContent = "IMPORTING…";
          // The file picker itself runs on the Rust side; null = cancelled.
          const id = await api.importAudioFile();
          if (id !== null) toast("Imported — processing…");
        } catch (e) {
          toast(errorMessage(e), "error");
        } finally {
          importBtn.disabled = false;
          importBtn.textContent = "IMPORT AUDIO";
        }
      },
    },
    "IMPORT AUDIO",
  );

  const screen = h(
    "div",
    { class: "screen screen-pad" },
    h(
      "div",
      { class: "library-head" },
      h("div", { class: "screen-title" }, "Library"),
      h("span", { class: "flex-spacer" }),
      importBtn,
      searchInput,
    ),
    listEl,
  );

  function meetingCard(m: Meeting): HTMLElement {
    const row = h(
      "div",
      { class: "meeting-card-row" },
      statusDot(m.status),
      h(
        "div",
        { class: "meeting-main" },
        h("div", { class: "meeting-title" }, m.title),
        h("div", { class: "meeting-meta" }, friendlyDate(m.created_at)),
      ),
      h("span", { class: "flex-spacer" }),
      m.duration_ms > 0 ? h("span", { class: "meeting-dur" }, formatDuration(m.duration_ms)) : null,
      statusChip(m.status),
    );

    if (m.status === "error") {
      row.append(
        h(
          "button",
          {
            class: "pill pill-small",
            onclick: async (e: Event) => {
              e.stopPropagation();
              try {
                await api.retryPipeline(m.id);
                toast("Retrying…");
              } catch (err) {
                toast(errorMessage(err), "error");
              }
            },
          },
          "RETRY",
        ),
      );
    }
    if (m.status !== "recording") {
      row.append(
        armedButton("✕", "SURE?", "pill pill-small pill-danger", async () => {
          try {
            await api.deleteMeeting(m.id);
            void refresh();
          } catch (err) {
            toast(errorMessage(err), "error");
          }
        }),
      );
    }

    const card = h("div", { class: "meeting-card", onclick: () => navigate({ name: "detail", meetingId: m.id }) }, row);

    if (isProcessing(m.status)) {
      const detail = progressByMeeting.get(m.id)?.detail ?? statusInfo(m.status).label;
      card.append(
        h(
          "div",
          { class: "meeting-progress" },
          h("div", { class: "progress-track" }, h("div", { class: "progress-fill" })),
          h("span", { class: "progress-label" }, detail),
        ),
      );
    } else if (m.status === "error" && m.error) {
      card.append(h("div", { class: "meeting-error" }, m.error));
    }

    return card;
  }

  function paint() {
    const filtered = query ? meetings.filter((m) => m.title.toLowerCase().includes(query)) : meetings;
    listEl.replaceChildren();
    if (filtered.length === 0) {
      listEl.append(
        h(
          "div",
          { class: "empty" },
          meetings.length === 0
            ? "No meetings yet — hit Record to capture your first one."
            : "No meetings match your search.",
        ),
      );
      return;
    }
    for (const m of filtered) listEl.append(meetingCard(m));
  }

  async function refresh() {
    try {
      meetings = await api.listMeetings();
      paint();
    } catch (e) {
      toast(errorMessage(e), "error");
    }
  }

  const onChanged = () => void refresh();
  window.addEventListener("ogma:changed", onChanged);
  window.addEventListener("ogma:progress", onChanged);

  void refresh();

  return {
    el: screen,
    destroy() {
      window.removeEventListener("ogma:changed", onChanged);
      window.removeEventListener("ogma:progress", onChanged);
    },
  };
}
