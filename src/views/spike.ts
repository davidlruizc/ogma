// iOS background-recording spike (PLAN.md Phase 4 gate).
//
// This screen exists ONLY to answer one question: can the app keep recording a
// long meeting while the iPhone screen is locked? It drives
// tauri-plugin-audio-recorder directly (no pipeline, no storage) so the result
// is unambiguous.
//
// Method: on stop we compare the *recorded* duration reported by the native
// recorder against real wall-clock elapsed time. If the phone suspended audio
// while locked, recorded << wall-clock and the spike FAILS. If they match, iOS
// background recording works and the full iOS UI can be built on top.
//
// This view is reachable from the "Spike (iOS)" nav item on this branch; it is
// not part of the shipping desktop UI. See docs/ios-background-spike.md.
import {
  checkPermission,
  requestPermission,
  startRecording,
  stopRecording,
  getStatus,
  type RecordingResult,
} from "tauri-plugin-audio-recorder-api";
import { h } from "../dom";
import { formatClock } from "../format";
import { toast } from "../toast";
import type { View } from "../view";

/** Recorded/elapsed ratio at or above this counts as a pass. */
const PASS_RATIO = 0.95;

function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

export function renderSpike(): View {
  let recording = false;
  let startedAt = 0; // Date.now() at record start — survives screen-lock suspension.

  const timerEl = h("div", { class: "rec-timer" }, "0:00:00");
  const subEl = h("div", { class: "rec-status" }, "native recorder · idle");
  const permEl = h("div", { class: "rec-status" }, "microphone: checking…");
  const startBtn = h("button", { class: "pill pill-accent" }, "● START SPIKE");
  const stopBtn = h("button", { class: "pill" }, "■ STOP & SCORE");
  stopBtn.style.display = "none";

  const report = h("pre", {
    class: "spike-report",
    style:
      "white-space:pre-wrap;margin-top:1.5rem;padding:1rem;border:1px solid var(--line,#333);" +
      "border-radius:8px;font-family:inherit;font-size:0.85rem;line-height:1.5;display:none;",
  });

  const screen = h(
    "div",
    { class: "screen record-screen" },
    h("h2", { style: "margin-bottom:0.25rem;" }, "iOS Background Recording Spike"),
    h(
      "div",
      { class: "rec-status", style: "max-width:36rem;margin-bottom:1.5rem;" },
      "Start, lock the screen, wait 60+ min, unlock, stop. A PASS means the native " +
        "recorder captured the whole locked interval.",
    ),
    timerEl,
    h("div", { style: "display:flex;gap:0.75rem;justify-content:center;margin:1rem 0;" }, startBtn, stopBtn),
    subEl,
    permEl,
    report,
  );

  async function refreshPermission(): Promise<boolean> {
    try {
      const p = await checkPermission();
      permEl.textContent = p.granted
        ? "microphone: granted"
        : p.canRequest
          ? "microphone: not granted (will prompt on start)"
          : "microphone: DENIED — enable it in iOS Settings › Ogma";
      return p.granted;
    } catch (e) {
      permEl.textContent = `microphone: status unavailable (${String(e)})`;
      return false;
    }
  }

  // While the screen is unlocked, poll native status so the timer moves. When
  // locked, JS is suspended and this simply stops firing — the authoritative
  // number comes from stopRecording() at the end.
  const poll = window.setInterval(async () => {
    if (!recording) return;
    try {
      const s = await getStatus();
      timerEl.textContent = formatClock(s.durationMs);
      subEl.textContent = `native recorder · ${s.state} · ${formatClock(s.durationMs)} captured`;
    } catch {
      /* status not available between transitions */
    }
  }, 1000);

  startBtn.addEventListener("click", async () => {
    startBtn.disabled = true;
    try {
      const granted = (await checkPermission()).granted || (await requestPermission()).granted;
      await refreshPermission();
      if (!granted) {
        toast("Microphone permission is required for the spike", "error");
        return;
      }
      await startRecording({
        outputPath: `ogma-ios-spike-${Date.now()}.m4a`,
        quality: "low", // 16 kHz mono — matches the pipeline's target rate
        maxDuration: 0, // no cap; we control stop manually
      });
      startedAt = Date.now();
      recording = true;
      report.style.display = "none";
      startBtn.style.display = "none";
      stopBtn.style.display = "";
      subEl.textContent = "native recorder · recording — now lock the screen";
      toast("Recording started — lock the screen and wait", "success");
    } catch (e) {
      toast(`Failed to start: ${String(e)}`, "error");
    } finally {
      startBtn.disabled = false;
    }
  });

  stopBtn.addEventListener("click", async () => {
    stopBtn.disabled = true;
    try {
      const result: RecordingResult = await stopRecording();
      recording = false;
      const wallMs = Date.now() - startedAt;
      const ratio = wallMs > 0 ? result.durationMs / wallMs : 0;
      const pass = ratio >= PASS_RATIO;

      timerEl.textContent = formatClock(result.durationMs);
      subEl.textContent = "native recorder · idle";
      stopBtn.style.display = "none";
      startBtn.style.display = "";

      report.style.display = "";
      report.textContent = [
        `${pass ? "✅ PASS" : "❌ FAIL"} — recorded ${(ratio * 100).toFixed(1)}% of wall-clock time`,
        "",
        `wall-clock elapsed : ${formatClock(wallMs)}`,
        `recorded duration  : ${formatClock(result.durationMs)}`,
        `file size          : ${fmtBytes(result.fileSize)}`,
        `sample rate        : ${result.sampleRate} Hz`,
        `channels           : ${result.channels}`,
        `file path          : ${result.filePath}`,
        "",
        pass
          ? "Background recording survived the locked interval. iOS Phase 4 is viable."
          : "Recording was truncated while locked. Check UIBackgroundModes=audio and the\n" +
            "AVAudioSession category, or fall back to the import-audio path (see runbook).",
      ].join("\n");
      toast(pass ? "Spike PASSED" : "Spike FAILED", pass ? "success" : "error");
    } catch (e) {
      toast(`Failed to stop: ${String(e)}`, "error");
      stopBtn.style.display = "none";
      startBtn.style.display = "";
      recording = false;
    } finally {
      stopBtn.disabled = false;
    }
  });

  void refreshPermission();

  return {
    el: screen,
    destroy() {
      clearInterval(poll);
    },
  };
}
