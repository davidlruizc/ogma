import { listen } from "@tauri-apps/api/event";
import { renderDetail } from "./views/detail";
import { renderHome } from "./views/home";
import { renderSettings } from "./views/settings";
import { progressByMeeting } from "./store";
import type { LevelUpdate, ProgressEvent } from "./types";
import type { Route, View } from "./view";

const app = document.getElementById("app")!;
let current: View | null = null;

function navigate(route: Route): void {
  current?.destroy?.();
  const view: View =
    route.name === "detail"
      ? renderDetail(navigate, route.meetingId)
      : route.name === "settings"
        ? renderSettings(navigate)
        : renderHome(navigate);
  current = view;
  app.replaceChildren(view.el);
  app.scrollTop = 0;
  document.documentElement.scrollTop = 0;
}

// Header nav
document.getElementById("nav-home")?.addEventListener("click", () => navigate({ name: "home" }));
document.getElementById("nav-settings")?.addEventListener("click", () => navigate({ name: "settings" }));

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

navigate({ name: "home" });
