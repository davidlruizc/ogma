import { api, errorMessage } from "../api";
import { armedButton, h } from "../dom";
import { formatClock } from "../format";
import { toast } from "../toast";
import type { LevelUpdate } from "../types";
import type { Navigate, View } from "../view";

const BAR_COUNT = 32;

export function renderRecord(navigate: Navigate): View {
  let recording = false;
  let paused = false;
  let inputDevice = "";

  const titleInput = h("input", {
    class: "title-input",
    placeholder: "Meeting title…",
    maxLength: 200,
  });

  // Live equalizer — each bar is a slice of recent mic loudness, scrolling
  // left as new level samples arrive so the shape tracks the actual voice.
  const bars: HTMLElement[] = [];
  const amps = new Array<number>(BAR_COUNT).fill(0);
  const waveform = h("div", { class: "waveform" });
  for (let i = 0; i < BAR_COUNT; i++) {
    const bar = h("span", { class: "wbar" });
    bars.push(bar);
    waveform.append(bar);
  }

  function paintBars() {
    for (let i = 0; i < BAR_COUNT; i++) {
      bars[i].style.transform = `scaleY(${(0.08 + amps[i] * 0.92).toFixed(3)})`;
    }
  }

  /** Push one loudness sample onto the scrolling buffer. */
  function pushLevel(rms: number, peak: number) {
    // rms is the loudness envelope; a touch of peak adds transient snap.
    const amp = Math.min(1, rms * 3.2 + peak * 0.25);
    amps.push(amp);
    amps.shift();
    paintBars();
  }

  const timerEl = h("div", { class: "rec-timer" }, "0:00:00");

  const recGlyph = h("span", { class: "rec-glyph" });
  const recordBtn = h("button", { class: "record-btn", title: "Start recording" }, recGlyph);

  const pauseBtn = h("button", { class: "pill" }, "⏸ PAUSE");
  const stopBtn = h("button", { class: "pill pill-accent" }, "■ STOP & PROCESS");
  const discardBtn = armedButton("DISCARD", "SURE?", "pill pill-small pill-danger", async () => {
    try {
      await api.discardRecording();
      setRecording(false);
      toast("Recording discarded");
    } catch (e) {
      toast(errorMessage(e), "error");
    }
  });
  const controls = h("div", { class: "rec-controls" }, pauseBtn, stopBtn, discardBtn);

  const statusEl = h("div", { class: "rec-status" });

  function renderStatus() {
    statusEl.replaceChildren();
    let head: string;
    if (!recording) {
      head = `ready · input: ${inputDevice || "system default"}`;
    } else if (paused) {
      head = "paused — segments safe on disk";
    } else {
      const seg = Math.floor(parseElapsed(timerEl.textContent) / 300);
      head = `recording — seg-${String(seg).padStart(3, "0")}.wav`;
    }
    statusEl.append(
      head,
      h("br"),
      "crash-safe · rotating 5-min WAV segments · 16 kHz mono",
    );
  }

  function parseElapsed(text: string | null): number {
    if (!text) return 0;
    const [h1, m1, s1] = text.split(":").map(Number);
    return (h1 || 0) * 3600 + (m1 || 0) * 60 + (s1 || 0);
  }

  const screen = h(
    "div",
    { class: "screen record-screen" },
    titleInput,
    waveform,
    timerEl,
    recordBtn,
    controls,
    statusEl,
  );

  function setRecording(on: boolean) {
    recording = on;
    paused = false;
    screen.classList.toggle("recording", on);
    screen.classList.remove("paused");
    recordBtn.classList.toggle("recording", on);
    recordBtn.title = on ? "Recording…" : "Start recording";
    controls.style.display = on ? "flex" : "none";
    pauseBtn.textContent = "⏸ PAUSE";
    if (!on) timerEl.textContent = "0:00:00";
    // Flatten the equalizer whenever we enter or leave a recording.
    amps.fill(0);
    paintBars();
    renderStatus();
  }

  function applyLevel(level: LevelUpdate) {
    timerEl.textContent = formatClock(level.elapsed_ms);
    if (level.paused !== paused) {
      paused = level.paused;
      screen.classList.toggle("paused", paused);
      pauseBtn.textContent = paused ? "▶ RESUME" : "⏸ PAUSE";
    }
    // Drive the equalizer from the real mic level; freeze it while paused.
    if (!level.paused) pushLevel(level.rms, level.peak);
    renderStatus();
  }

  recordBtn.addEventListener("click", async () => {
    recordBtn.disabled = true;
    try {
      if (!recording) {
        const title = titleInput.value.trim();
        await api.startRecording(title.length > 0 ? title : null);
        setRecording(true);
      } else {
        await api.stopRecording();
        setRecording(false);
        titleInput.value = "";
        toast("Recording saved — pipeline started", "success");
        navigate({ name: "library" });
      }
    } catch (e) {
      toast(errorMessage(e), "error");
      void syncRecordingState();
    } finally {
      recordBtn.disabled = false;
    }
  });

  stopBtn.addEventListener("click", () => recordBtn.click());

  pauseBtn.addEventListener("click", async () => {
    try {
      if (paused) {
        await api.resumeRecording();
        paused = false;
      } else {
        await api.pauseRecording();
        paused = true;
      }
      pauseBtn.textContent = paused ? "▶ RESUME" : "⏸ PAUSE";
      screen.classList.toggle("paused", paused);
      renderStatus();
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
          pauseBtn.textContent = paused ? "▶ RESUME" : "⏸ PAUSE";
          screen.classList.toggle("paused", paused);
        }
        renderStatus();
      }
    } catch {
      /* backend not ready yet */
    }
  }

  // ── wiring ──────────────────────────────────────────────────────────────
  const onLevel = (e: Event) => {
    const level = (e as CustomEvent<LevelUpdate>).detail;
    if (!recording) setRecording(true);
    applyLevel(level);
  };
  window.addEventListener("ogma:level", onLevel);
  const pollTimer = window.setInterval(() => {
    if (recording) void syncRecordingState();
  }, 1500);

  setRecording(false);
  void syncRecordingState();
  void api
    .getSettings()
    .then((c) => {
      inputDevice = c.input_device;
      if (!recording) renderStatus();
    })
    .catch(() => {});

  return {
    el: screen,
    destroy() {
      window.removeEventListener("ogma:level", onLevel);
      clearInterval(pollTimer);
    },
  };
}
