use serde::{Deserialize, Serialize};

/// Lifecycle of a meeting through the pipeline. Stored as lowercase strings in SQLite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeetingStatus {
    Recording,
    /// Audio safely on disk, pipeline not yet finished with transcription.
    Recorded,
    Transcribing,
    /// Claude pass: speaker attribution + notes generation.
    Summarizing,
    /// Notes done locally, pushing to Notion.
    Syncing,
    Done,
    Error,
}

impl MeetingStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            MeetingStatus::Recording => "recording",
            MeetingStatus::Recorded => "recorded",
            MeetingStatus::Transcribing => "transcribing",
            MeetingStatus::Summarizing => "summarizing",
            MeetingStatus::Syncing => "syncing",
            MeetingStatus::Done => "done",
            MeetingStatus::Error => "error",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "recording" => MeetingStatus::Recording,
            "recorded" => MeetingStatus::Recorded,
            "transcribing" => MeetingStatus::Transcribing,
            "summarizing" => MeetingStatus::Summarizing,
            "syncing" => MeetingStatus::Syncing,
            "done" => MeetingStatus::Done,
            _ => MeetingStatus::Error,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meeting {
    pub id: String,
    pub title: String,
    /// RFC3339 timestamp of when recording started.
    pub created_at: String,
    pub duration_ms: i64,
    pub status: MeetingStatus,
    /// Human-readable error of the last failed pipeline step, if status == Error.
    pub error: Option<String>,
    /// Directory holding seg-*.wav chunks and the concatenated audio.wav.
    pub audio_dir: String,
    pub notion_page_id: Option<String>,
}

/// One diarized utterance of the transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub speaker: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionItem {
    pub id: i64,
    pub meeting_id: String,
    pub task: String,
    pub owner: Option<String>,
    pub due: Option<String>,
    pub status: String, // "open" | "done"
}

/// Structured notes produced by the notes provider (stored verbatim as JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingNotes {
    pub title: String,
    pub tldr: String,
    pub summary: String,
    #[serde(default)]
    pub key_points: Vec<String>,
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub action_items: Vec<NoteActionItem>,
    #[serde(default)]
    pub open_questions: Vec<String>,
    #[serde(default)]
    pub highlights: Vec<Highlight>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteActionItem {
    pub task: String,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub due: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Highlight {
    pub quote: String,
    pub speaker: String,
    pub timestamp_ms: i64,
}
