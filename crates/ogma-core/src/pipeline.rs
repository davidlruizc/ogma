//! Pipeline orchestrator: recorded audio → transcript → notes → Notion.
//!
//! Every step is idempotent and the restart point is *derived from stored
//! data*, not from a remembered stage — so a retry after any failure (or a
//! crash) resumes exactly where work is missing:
//!   no transcript            → transcribe
//!   transcript, no notes     → summarize (speaker labels + notes)
//!   notes, no notion page    → sync (skipped when Notion isn't configured)

use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::config::Config;
use crate::error::{Error, Result};
use crate::models::MeetingStatus;
use crate::notion::NotionClient;
use crate::providers::claude::ClaudeProvider;
use crate::providers::whisper::WhisperProvider;
use crate::providers::{AudioChunk, NotesProvider, TranscriptionProvider, Utterance};
use crate::recording::{self, SEGMENT_SECONDS};
use crate::storage::Storage;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProgressEvent {
    pub meeting_id: String,
    pub status: MeetingStatus,
    pub detail: String,
    pub error: Option<String>,
}

pub type ProgressCallback = Arc<dyn Fn(ProgressEvent) + Send + Sync>;

pub struct Pipeline {
    storage: Arc<Mutex<Storage>>,
    config: Config,
    on_progress: ProgressCallback,
}

impl Pipeline {
    pub fn new(storage: Arc<Mutex<Storage>>, config: Config, on_progress: ProgressCallback) -> Pipeline {
        Pipeline {
            storage,
            config,
            on_progress,
        }
    }

    /// Run (or resume) the pipeline for one meeting. Sets status to `Error`
    /// with a message on failure; safe to call again to retry.
    pub async fn run(&self, meeting_id: &str) -> Result<()> {
        match self.run_inner(meeting_id).await {
            Ok(()) => Ok(()),
            Err(e) => {
                let msg = e.to_string();
                {
                    let storage = self.storage.lock().unwrap();
                    let _ = storage.set_status(meeting_id, MeetingStatus::Error, Some(&msg));
                }
                self.emit(meeting_id, MeetingStatus::Error, "pipeline failed", Some(&msg));
                Err(e)
            }
        }
    }

    async fn run_inner(&self, meeting_id: &str) -> Result<()> {
        let (audio_dir, has_transcript, has_notes, notion_done) = {
            let storage = self.storage.lock().unwrap();
            let meeting = storage.get_meeting(meeting_id)?;
            let has_transcript = !storage.get_segments(meeting_id)?.is_empty();
            let has_notes = storage.get_notes(meeting_id)?.is_some();
            (
                meeting.audio_dir.clone(),
                has_transcript,
                has_notes,
                meeting.notion_page_id.is_some(),
            )
        };
        let audio_dir = Path::new(&audio_dir).to_path_buf();

        if !has_transcript {
            self.transcribe(meeting_id, &audio_dir).await?;
        }
        if !has_notes || !has_labeled_transcript(&self.storage, meeting_id)? {
            self.summarize(meeting_id).await?;
        }
        if !notion_done && self.notion_configured() {
            self.sync_notion(meeting_id).await?;
        }

        {
            let storage = self.storage.lock().unwrap();
            storage.set_status(meeting_id, MeetingStatus::Done, None)?;
        }
        self.emit(meeting_id, MeetingStatus::Done, "complete", None);
        Ok(())
    }

    async fn transcribe(&self, meeting_id: &str, audio_dir: &Path) -> Result<()> {
        if self.config.openai_api_key.is_empty() {
            return Err(Error::Config(
                "OpenAI API key is not set (Settings → OpenAI API key)".into(),
            ));
        }
        {
            let storage = self.storage.lock().unwrap();
            storage.set_status(meeting_id, MeetingStatus::Transcribing, None)?;
        }
        self.emit(meeting_id, MeetingStatus::Transcribing, "uploading audio to Whisper", None);

        let segments = recording::list_segments(audio_dir)?;
        if segments.is_empty() {
            return Err(Error::InvalidState("no audio segments on disk".into()));
        }
        let chunks: Vec<AudioChunk> = segments
            .iter()
            .enumerate()
            .map(|(i, path)| AudioChunk {
                path: path.clone(),
                offset_ms: (i as i64) * (SEGMENT_SECONDS as i64) * 1000,
            })
            .collect();

        let provider = WhisperProvider::new(
            self.config.openai_api_key.clone(),
            self.config.whisper_model.clone(),
            self.config.language.clone(),
        );
        let utterances = provider.transcribe(&chunks).await?;
        if utterances.is_empty() {
            return Err(Error::InvalidState(
                "transcription returned no speech (empty recording?)".into(),
            ));
        }

        let segments: Vec<crate::models::TranscriptSegment> = utterances
            .iter()
            .map(|u| crate::models::TranscriptSegment {
                speaker: UNLABELED_SPEAKER.to_string(),
                start_ms: u.start_ms,
                end_ms: u.end_ms,
                text: u.text.clone(),
            })
            .collect();
        let mut storage = self.storage.lock().unwrap();
        storage.replace_segments(meeting_id, &segments)?;
        Ok(())
    }

    async fn summarize(&self, meeting_id: &str) -> Result<()> {
        if self.config.anthropic_api_key.is_empty() {
            return Err(Error::Config(
                "Anthropic API key is not set (Settings → Anthropic API key)".into(),
            ));
        }
        {
            let storage = self.storage.lock().unwrap();
            storage.set_status(meeting_id, MeetingStatus::Summarizing, None)?;
        }
        self.emit(meeting_id, MeetingStatus::Summarizing, "generating speaker labels and notes", None);

        let (title, utterances) = {
            let storage = self.storage.lock().unwrap();
            let meeting = storage.get_meeting(meeting_id)?;
            let utterances: Vec<Utterance> = storage
                .get_segments(meeting_id)?
                .into_iter()
                .map(|s| Utterance {
                    start_ms: s.start_ms,
                    end_ms: s.end_ms,
                    text: s.text,
                })
                .collect();
            (meeting.title, utterances)
        };

        let provider = ClaudeProvider::new(
            self.config.anthropic_api_key.clone(),
            self.config.notes_model.clone(),
        );
        let result = provider.generate(&title, &utterances).await?;

        let mut storage = self.storage.lock().unwrap();
        storage.replace_segments(meeting_id, &result.segments)?;
        storage.save_notes(meeting_id, &result.notes)?;
        // Adopt Claude's title when the user left the default.
        if title.trim().is_empty() || title.starts_with("Meeting ") {
            storage.set_title(meeting_id, &result.notes.title)?;
        }
        Ok(())
    }

    async fn sync_notion(&self, meeting_id: &str) -> Result<()> {
        {
            let storage = self.storage.lock().unwrap();
            storage.set_status(meeting_id, MeetingStatus::Syncing, None)?;
        }
        self.emit(meeting_id, MeetingStatus::Syncing, "pushing to Notion", None);

        let (meeting, notes, segments) = {
            let storage = self.storage.lock().unwrap();
            (
                storage.get_meeting(meeting_id)?,
                storage
                    .get_notes(meeting_id)?
                    .ok_or_else(|| Error::InvalidState("notes missing before sync".into()))?,
                storage.get_segments(meeting_id)?,
            )
        };

        let client = NotionClient::new(
            self.config.notion_api_key.clone(),
            self.config.notion_database_id.clone(),
        );
        let page_id = client.create_meeting_page(&meeting, &notes, &segments).await?;

        let storage = self.storage.lock().unwrap();
        storage.set_notion_page(meeting_id, &page_id)?;
        Ok(())
    }

    fn notion_configured(&self) -> bool {
        !self.config.notion_api_key.is_empty() && !self.config.notion_database_id.is_empty()
    }

    fn emit(&self, meeting_id: &str, status: MeetingStatus, detail: &str, error: Option<&str>) {
        (self.on_progress)(ProgressEvent {
            meeting_id: meeting_id.to_string(),
            status,
            detail: detail.to_string(),
            error: error.map(String::from),
        });
    }
}

pub const UNLABELED_SPEAKER: &str = "Speaker ?";

fn has_labeled_transcript(storage: &Arc<Mutex<Storage>>, meeting_id: &str) -> Result<bool> {
    let storage = storage.lock().unwrap();
    let segments = storage.get_segments(meeting_id)?;
    Ok(!segments.is_empty() && segments.iter().any(|s| s.speaker != UNLABELED_SPEAKER))
}
