export type MeetingStatus =
  | "recording"
  | "recorded"
  | "transcribing"
  | "summarizing"
  | "syncing"
  | "done"
  | "error";

export interface Meeting {
  id: string;
  title: string;
  created_at: string; // RFC3339
  duration_ms: number;
  status: MeetingStatus;
  error: string | null;
  audio_dir: string;
  notion_page_id: string | null;
}

export interface TranscriptSegment {
  speaker: string;
  start_ms: number;
  end_ms: number;
  text: string;
}

export interface NoteActionItem {
  task: string;
  owner: string | null;
  due: string | null;
}

export interface Highlight {
  quote: string;
  speaker: string;
  timestamp_ms: number;
}

export interface MeetingNotes {
  title: string;
  tldr: string;
  summary: string;
  key_points: string[];
  decisions: string[];
  action_items: NoteActionItem[];
  open_questions: string[];
  highlights: Highlight[];
}

export interface ActionItem {
  id: number;
  meeting_id: string;
  task: string;
  owner: string | null;
  due: string | null;
  status: string; // "open" | "done"
}

export interface MeetingDetail {
  meeting: Meeting;
  segments: TranscriptSegment[];
  notes: MeetingNotes | null;
  action_items: ActionItem[];
  audio_path: string | null;
}

export interface Config {
  openai_api_key: string;
  anthropic_api_key: string;
  notion_api_key: string;
  notion_database_id: string;
  notes_model: string;
  whisper_model: string;
  language: string;
  input_device: string;
}

export interface RecordingState {
  meeting_id: string | null;
  elapsed_ms: number;
  paused: boolean;
}

export interface LevelUpdate {
  rms: number;
  peak: number;
  elapsed_ms: number;
  paused: boolean;
}

export interface ProgressEvent {
  meeting_id: string;
  status: MeetingStatus;
  detail: string;
  error: string | null;
}
