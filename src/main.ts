import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api } from "./api";
import { toastImportSummary } from "./toast";
import { renderDetail } from "./views/detail";
import { renderLibrary } from "./views/library";
import { renderRecord } from "./views/record";
import { renderSettings } from "./views/settings";
import { renderSpike } from "./views/spike";
import { progressByMeeting } from "./store";
import type { ImportSummary, LevelUpdate, ProgressEvent } from "./types";
import type { Route, View } from "./view";

const app = document.getElementById("app")!;
let current: View | null = null;

/** Which sidebar tab lights up for a given route. */
function navKey(route: Route): string {
  return route.name === "detail" ? "library" : route.name;
}

function navigate(route: Route): void {
  current?.destroy?.();
  const view: View =
    route.name === "detail"
      ? renderDetail(navigate, route.meetingId)
      : route.name === "settings"
        ? renderSettings(navigate)
        : route.name === "library"
          ? renderLibrary(navigate)
          : route.name === "spike"
            ? renderSpike()
            : renderRecord(navigate);
  current = view;
  app.replaceChildren(view.el);
  app.scrollTop = 0;

  const active = navKey(route);
  for (const btn of document.querySelectorAll<HTMLElement>(".nav-item")) {
    btn.classList.toggle("active", btn.dataset.nav === active);
  }
}

// ── sidebar nav ─────────────────────────────────────────────────────────────
document.getElementById("nav")?.addEventListener("click", (e) => {
  const btn = (e.target as HTMLElement).closest<HTMLElement>(".nav-item");
  const key = btn?.dataset.nav;
  if (key === "record") navigate({ name: "record" });
  else if (key === "library") navigate({ name: "library" });
  else if (key === "settings") navigate({ name: "settings" });
  else if (key === "spike") navigate({ name: "spike" });
});

// ── theme toggle (persisted) ────────────────────────────────────────────────
const themeBtn = document.getElementById("theme-toggle")!;
function applyTheme(theme: "dark" | "light"): void {
  document.body.dataset.theme = theme;
  themeBtn.textContent = theme === "dark" ? "◑ DARK" : "◐ LIGHT";
  localStorage.setItem("ogma-theme", theme);
}
themeBtn.addEventListener("click", () => {
  applyTheme(document.body.dataset.theme === "dark" ? "light" : "dark");
});
applyTheme(localStorage.getItem("ogma-theme") === "light" ? "light" : "dark");

// ── sidebar badges + Notion status ──────────────────────────────────────────
function setBadge(key: string, text: string, rec = false): void {
  const el = document.querySelector<HTMLElement>(`.nav-badge[data-badge="${key}"]`);
  if (!el) return;
  el.textContent = text;
  el.classList.toggle("rec", rec);
}

async function refreshBadges(): Promise<void> {
  try {
    const meetings = await api.listMeetings();
    setBadge("library", String(meetings.length));
  } catch {
    /* backend not ready */
  }
  try {
    const state = await api.recordingState();
    setBadge("record", state.meeting_id ? "REC" : "", state.meeting_id !== null);
  } catch {
    /* backend not ready */
  }
}

async function refreshNotionStatus(): Promise<void> {
  const line = document.getElementById("notion-status");
  if (!line) return;
  try {
    const config = await api.getSettings();
    const connected = config.notion_api_key.trim() !== "" && config.notion_database_id.trim() !== "";
    line.innerHTML = "";
    const dot = document.createElement("span");
    dot.className = `status-dot ${connected ? "ok" : "muted"}`;
    line.append(dot, connected ? "Notion · connected" : "Notion · not connected");
  } catch {
    /* leave default */
  }
}

// Bridge Tauri events onto window CustomEvents so views can subscribe with
// plain DOM listeners (and clean up synchronously on teardown).
void listen<LevelUpdate>("recording:level", (event) => {
  window.dispatchEvent(new CustomEvent("ogma:level", { detail: event.payload }));
});
void listen<ProgressEvent>("meeting:progress", (event) => {
  progressByMeeting.set(event.payload.meeting_id, event.payload);
  window.dispatchEvent(new CustomEvent("ogma:progress", { detail: event.payload }));
});
void listen("meetings:changed", () => {
  window.dispatchEvent(new CustomEvent("ogma:changed"));
});
// Rust imports dropped audio (paths come from the OS, never the webview) and
// reports the outcome here.
void listen<ImportSummary>("import:done", (event) => {
  toastImportSummary(event.payload);
});

// ── drag-and-drop to import audio ─────────────────────────────────────────────
// The actual decode/import happens Rust-side on the OS drop event; here we only
// show a drop affordance. Not importing from JS keeps the "webview never
// supplies a path" security invariant intact.
const dropOverlay = document.createElement("div");
dropOverlay.className = "drop-overlay";
dropOverlay.innerHTML = `<div class="drop-overlay-card">Drop audio files to import</div>`;
document.body.append(dropOverlay);
void getCurrentWebview().onDragDropEvent((event) => {
  const t = event.payload.type;
  dropOverlay.classList.toggle("visible", t === "enter" || t === "over");
});

window.addEventListener("ogma:changed", () => void refreshBadges());
window.addEventListener("ogma:progress", () => void refreshBadges());
window.addEventListener("ogma:settings-saved", () => void refreshNotionStatus());
window.setInterval(() => void refreshBadges(), 2500);

void refreshBadges();
void refreshNotionStatus();

navigate({ name: "record" });
