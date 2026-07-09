import { api, errorMessage } from "../api";
import { armedButton, h } from "../dom";
import { formatClock, formatDuration, friendlyDate } from "../format";
import { isProcessing, statusBadge } from "../status";
import { progressByMeeting } from "../store";
import { toast } from "../toast";
import type { LevelUpdate, Meeting } from "../types";
import type { Navigate, View } from "../view";

export function renderHome(navigate: Navigate): View {
  let recording = false;
  let paused = false;

  // ── recorder card ─────────────────────────────────────────────────────────
  const titleInput = h("input", {
    class: "title-input",
    placeholder: "Meeting title (optional)",
    maxLength: 200,
  });
  const timerEl = h("div", { class: "rec-timer" }, "0:00:00");
  const meterFill = h("div", { class: "meter-fill" });
  const meterPeak = h("div", { class: "meter-peak" });
  const meter = h("div", { class: "meter" }, meterFill, meterPeak);
  const recHint = h("div", { class: "rec-hint" }, "Ready to record");

  const recordBtn = h(
    "button",
    { class: "record-btn", title: "Start recording" },
    h("span", { class: "record-btn-glyph" }),
  );

  const pauseBtn = h("button", { class: "btn" }, "Pause");
  const discardBtn = armedButton("Discard", "Discard recording?", "btn btn-danger-ghost", async () => {
    try {
      await api.discardRecording();
      setRecording(false);
      toast("Recording discarded");
      void refreshList();
    } catch (e) {
      toast(errorMessage(e), "error");
    }
  });
  const recControls = h("div", { class: "rec-buttons" }, pauseBtn, discardBtn);

  const recPanel = h(
    "div",
    { class: "rec-live" },
    timerEl,
    meter,
    recControls,
  );

  const recorderCard = h(
    "section",
    { class: "card recorder-card" },
    titleInput,
    h("div", { class: "rec-center" }, recordBtn),
    recHint,
    recPanel,
  );

  function setRecording(on: boolean) {
    recording = on;
    paused = false;
    recorderCard.classList.toggle("is-recording", on);
    recordBtn.classList.toggle("recording", on);
    recordBtn.title = on ? "Stop and process" : "Start recording";
    recHint.textContent = on ? "Recording… click to stop & process" : "Ready to record";
    pauseBtn.textContent = "Pause";
    if (!on) {
      timerEl.textContent = "0:00:00";
      meterFill.style.width = "0%";
      meterPeak.style.left = "0%";
    }
  }

  function applyLevel(level: LevelUpdate) {
    timerEl.textContent = formatClock(level.elapsed_ms);
    const scale = (v: number) => Math.min(1, v * 3.2) * 100;
    meterFill.style.width = `${scale(level.rms)}%`;
    meterPeak.style.left = `${scale(level.peak)}%`;
    if (level.paused !== paused) {
      paused = level.paused;
      pauseBtn.textContent = paused ? "Resume" : "Pause";
      recorderCard.classList.toggle("is-paused", paused);
    }
  }

  recordBtn.addEventListener("click", async () => {
    recordBtn.disabled = true;
    try {
      if (!recording) {
        const title = titleInput.value.trim();
        await api.startRecording(title.length > 0 ? title : null);
        setRecording(true);
      } else {
        recHint.textContent = "Finishing…";
        await api.stopRecording();
        setRecording(false);
        titleInput.value = "";
        toast("Recording saved — processing started", "success");
        void refreshList();
      }
    } catch (e) {
      toast(errorMessage(e), "error");
      // Re-sync with backend truth on any failure.
      void syncRecordingState();
    } finally {
      recordBtn.disabled = false;
    }
  });

  pauseBtn.addEventListener("click", async () => {
    try {
      if (paused) {
        await api.resumeRecording();
        paused = false;
      } else {
        await api.pauseRecording();
        paused = true;
      }
      pauseBtn.textContent = paused ? "Resume" : "Pause";
      recorderCard.classList.toggle("is-paused", paused);
    } catch (e) {
      toast(errorMessage(e), "error");
    }
  });

  async function syncRecordingState() {
    try {
      const state = await api.recordingState();
      const active = state.meeting_id !== null;
      if (active !== recording) setRecording(active);
      if (active) {
        timerEl.textContent = formatClock(state.elapsed_ms);
        if (state.paused !== paused) {
          paused = state.paused;
          pauseBtn.textContent = paused ? "Resume" : "Pause";
          recorderCard.classList.toggle("is-paused", paused);
        }
      }
    } catch {
      /* backend not ready yet */
    }
  }

  // ── meeting list ──────────────────────────────────────────────────────────
  const listEl = h("div", { class: "meeting-list" });
  const listSection = h(
    "section",
    { class: "meetings-section" },
    h("h2", null, "Meetings"),
    listEl,
  );

  function meetingRow(meeting: Meeting): HTMLElement {
    const progress = progressByMeeting.get(meeting.id);
    const metaBits = [friendlyDate(meeting.created_at)];
    if (meeting.duration_ms > 0) metaBits.push(formatDuration(meeting.duration_ms));

    const side = h("div", { class: "meeting-side" }, statusBadge(meeting));
    if (meeting.status === "error") {
      side.append(
        h(
          "button",
          {
            class: "btn btn-small",
            onclick: async (e: Event) => {
              e.stopPropagation();
              try {
                await api.retryPipeline(meeting.id);
                toast("Retrying…");
              } catch (err) {
                toast(errorMessage(err), "error");
              }
            },
          },
          "Retry",
        ),
      );
    }
    if (meeting.status !== "recording") {
      side.append(
        armedButton("✕", "Delete?", "btn btn-small btn-danger-ghost", async () => {
          try {
            await api.deleteMeeting(meeting.id);
            void refreshList();
          } catch (err) {
            toast(errorMessage(err), "error");
          }
        }),
      );
    }

    const subline =
      meeting.status === "error" && meeting.error
        ? h("div", { class: "meeting-error" }, meeting.error)
        : isProcessing(meeting.status) && progress?.detail
          ? h("div", { class: "meeting-progress" }, progress.detail)
          : null;

    return h(
      "div",
      {
        class: "meeting-item",
        onclick: () => navigate({ name: "detail", meetingId: meeting.id }),
      },
      h(
        "div",
        { class: "meeting-main" },
        h("div", { class: "meeting-title" }, meeting.title),
        h("div", { class: "meeting-meta" }, metaBits.join(" · ")),
        subline,
      ),
      side,
    );
  }

  async function refreshList() {
    try {
      const meetings = await api.listMeetings();
      listEl.replaceChildren();
      if (meetings.length === 0) {
        listEl.append(
          h("div", { class: "empty" }, "No meetings yet — hit record to capture your first one."),
        );
        return;
      }
      for (const meeting of meetings) listEl.append(meetingRow(meeting));
    } catch (e) {
      toast(errorMessage(e), "error");
    }
  }

  // ── wiring ────────────────────────────────────────────────────────────────
  const onLevel = (e: Event) => {
    const level = (e as CustomEvent<LevelUpdate>).detail;
    if (!recording) setRecording(true);
    applyLevel(level);
  };
  const onChanged = () => void refreshList();
  window.addEventListener("ogma:level", onLevel);
  window.addEventListener("ogma:changed", onChanged);
  window.addEventListener("ogma:progress", onChanged);
  const pollTimer = window.setInterval(() => {
    if (recording) void syncRecordingState();
  }, 1500);

  void syncRecordingState();
  void refreshList();

  const el = h("div", { class: "view view-home" }, recorderCard, listSection);
  return {
    el,
    destroy() {
      window.removeEventListener("ogma:level", onLevel);
      window.removeEventListener("ogma:changed", onChanged);
      window.removeEventListener("ogma:progress", onChanged);
      clearInterval(pollTimer);
    },
  };
}
