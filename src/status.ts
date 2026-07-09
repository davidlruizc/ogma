import { h } from "./dom";
import type { MeetingStatus } from "./types";

interface StatusInfo {
  label: string;
  /** CSS custom-property reference for the accent color of this state. */
  color: string;
}

const STATUS: Record<MeetingStatus, StatusInfo> = {
  recording: { label: "recording", color: "var(--rec)" },
  recorded: { label: "queued", color: "var(--tx3)" },
  transcribing: { label: "transcribing", color: "var(--ac)" },
  summarizing: { label: "generating notes", color: "var(--vi)" },
  syncing: { label: "syncing to Notion", color: "var(--ok)" },
  done: { label: "done", color: "var(--ok)" },
  error: { label: "error", color: "var(--rec)" },
};

export const PROCESSING_STATUSES: MeetingStatus[] = ["transcribing", "summarizing", "syncing"];

export function isProcessing(status: MeetingStatus): boolean {
  return PROCESSING_STATUSES.includes(status);
}

export function statusInfo(status: MeetingStatus): StatusInfo {
  return STATUS[status];
}

/** Small colored dot used in the meeting list. */
export function statusDot(status: MeetingStatus): HTMLElement {
  const c = STATUS[status].color;
  const glow = status === "recorded" ? "transparent" : `color-mix(in oklab, ${c} 60%, transparent)`;
  return h("span", {
    class: "meeting-dot",
    style: `background:${c};box-shadow:0 0 10px ${glow}`,
  });
}

/** Pill status chip, colored per state. */
export function statusChip(status: MeetingStatus): HTMLElement {
  const { label, color } = STATUS[status];
  return h(
    "span",
    {
      class: "status-chip",
      style:
        `color:${color};` +
        `background:color-mix(in oklab, ${color} 9%, transparent);` +
        `border:1px solid color-mix(in oklab, ${color} 40%, transparent)`,
    },
    label,
  );
}

export interface PipelineStep {
  name: string;
  sub: string;
  state: "done" | "active" | "waiting";
}

/** Where a meeting sits on the transcribe → notes → notion track. */
const STEP_ORDER: MeetingStatus[] = [
  "recorded",
  "transcribing",
  "summarizing",
  "syncing",
  "done",
];

const STEP_DEFS = [
  { name: "Transcribe", sub: "OpenAI Whisper · 5-min chunks", at: "transcribing" },
  { name: "Speakers + notes", sub: "Claude · one structured call", at: "summarizing" },
  { name: "Push to Notion", sub: "page + transcript toggle block", at: "syncing" },
] as const;

export function pipelineSteps(status: MeetingStatus): PipelineStep[] {
  const cur = STEP_ORDER.indexOf(status);
  return STEP_DEFS.map((def) => {
    const idx = STEP_ORDER.indexOf(def.at);
    const state: PipelineStep["state"] = cur > idx ? "done" : cur === idx ? "active" : "waiting";
    return { name: def.name, sub: def.sub, state };
  });
}

export { statusChip as badge };
