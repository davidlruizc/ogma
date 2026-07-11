import { invoke } from "@tauri-apps/api/core";
import type {
  ActionItem as _ActionItem,
  Config,
  Meeting,
  MeetingDetail,
  RecordingState,
} from "./types";

export const api = {
  listMeetings: () => invoke<Meeting[]>("list_meetings"),

  getMeetingDetail: (meetingId: string) =>
    invoke<MeetingDetail>("get_meeting_detail", { meetingId }),

  setMeetingTitle: (meetingId: string, title: string) =>
    invoke<void>("set_meeting_title", { meetingId, title }),

  renameSpeaker: (meetingId: string, from: string, to: string) =>
    invoke<void>("rename_speaker", { meetingId, from, to }),

  deleteMeeting: (meetingId: string) =>
    invoke<void>("delete_meeting", { meetingId }),

  setActionItemStatus: (id: number, status: "open" | "done") =>
    invoke<void>("set_action_item_status", { id, status }),

  startRecording: (title: string | null) =>
    invoke<Meeting>("start_recording", { title }),

  pauseRecording: () => invoke<void>("pause_recording"),
  resumeRecording: () => invoke<void>("resume_recording"),
  listInputDevices: () => invoke<string[]>("list_input_devices"),
  recordingState: () => invoke<RecordingState>("recording_state"),
  stopRecording: () => invoke<string>("stop_recording"),
  discardRecording: () => invoke<void>("discard_recording"),

  /** Opens a native file picker (Rust-side); resolves null if cancelled. */
  importAudioFile: (title: string | null = null) =>
    invoke<string | null>("import_audio_file", { title }),

  retryPipeline: (meetingId: string) =>
    invoke<void>("retry_pipeline", { meetingId }),

  /** Opens a native folder picker (Rust-side); resolves null if cancelled. */
  pickFolder: () => invoke<string | null>("pick_folder"),

  getPlatform: () => invoke<string>("get_platform"),

  getSettings: () => invoke<Config>("get_settings"),
  saveSettings: (settings: Config) =>
    invoke<void>("save_settings", { settings }),

  notionSetup: (parentPageId: string) =>
    invoke<string>("notion_setup", { parentPageId }),
};

/** Normalize any thrown value (backend rejects with strings) to a message. */
export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return JSON.stringify(e);
}
