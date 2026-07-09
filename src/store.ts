import type { ProgressEvent } from "./types";

/** Latest pipeline progress per meeting, fed by the "meeting:progress" event. */
export const progressByMeeting = new Map<string, ProgressEvent>();
