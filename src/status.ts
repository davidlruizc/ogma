import { h } from "./dom";
import { progressByMeeting } from "./store";
import type { Meeting, MeetingStatus } from "./types";

const LABELS: Record<MeetingStatus, string> = {
  recording: "Recording",
  recorded: "Recorded",
  transcribing: "Transcribing",
  summarizing: "Summarizing",
  syncing: "Syncing",
  done: "Done",
  error: "Error",
};

export const PROCESSING_STATUSES: MeetingStatus[] = [
  "transcribing",
  "summarizing",
  "syncing",
];

export function isProcessing(status: MeetingStatus): boolean {
  return PROCESSING_STATUSES.includes(status);
}

/** Color-coded status badge; processing states get a spinner + live detail. */
export function statusBadge(meeting: Meeting): HTMLElement {
  const status = meeting.status;
  const badge = h(
    "span",
    { class: `badge badge-${status}`, title: meeting.error ?? "" },
    isProcessing(status) ? h("span", { class: "spinner" }) : null,
    LABELS[status],
  );
  if (isProcessing(status)) {
    const progress = progressByMeeting.get(meeting.id);
    if (progress && progress.detail) {
      badge.title = progress.detail;
    }
  }
  return badge;
}
