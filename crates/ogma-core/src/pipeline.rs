//! Pipeline orchestrator: recorded audio → transcript → notes → destinations.
//!
//! Every step is idempotent and the restart point is *derived from stored
//! data*, not from a remembered stage — so a retry after any failure (or a
//! crash) resumes exactly where work is missing:
//!   no transcript            → transcribe
//!   transcript, no notes     → summarize (speaker labels + notes)
//!   notes                    → for each enabled sync destination with no
//!                              sync record, sync it (one failing destination
//!                              doesn't block the others)

use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::config::Config;
use crate::error::{Error, Result};
use crate::models::MeetingStatus;
use crate::providers::claude::ClaudeProvider;
use crate::providers::whisper::WhisperProvider;
use crate::providers::{AudioChunk, NotesProvider, TranscriptionProvider, Utterance};
use crate::recording::{self, wav};
use crate::storage::Storage;
use crate::sync;

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
        let (audio_dir, has_transcript, has_notes) = {
            let storage = self.storage.lock().unwrap();
            let meeting = storage.get_meeting(meeting_id)?;
            let has_transcript = !storage.get_segments(meeting_id)?.is_empty();
            let has_notes = storage.get_notes(meeting_id)?.is_some();
            (meeting.audio_dir.clone(), has_transcript, has_notes)
        };
        let audio_dir = Path::new(&audio_dir).to_path_buf();

        if !has_transcript {
            self.transcribe(meeting_id, &audio_dir).await?;
        }
        if !has_notes || !has_labeled_transcript(&self.storage, meeting_id)? {
            self.summarize(meeting_id).await?;
        }
        self.sync_destinations(meeting_id).await?;

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
        // Offset each chunk by the *actual* summed duration of the segments
        // before it, not a nominal 5-min-per-segment count: a recovered/repaired
        // crash can leave a short segment, and using its real length keeps every
        // downstream timestamp aligned.
        let chunks = chunk_offsets(&segments)?;

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

    /// Fan the meeting out to every enabled destination that has no sync
    /// record yet. Each destination is attempted even if an earlier one
    /// failed; successes are recorded immediately, so a retry redoes only the
    /// failed ones. Any failure still fails the run (→ status Error + Retry).
    async fn sync_destinations(&self, meeting_id: &str) -> Result<()> {
        self.sync_to(meeting_id, sync::enabled_destinations(&self.config))
            .await
    }

    /// The fan-out core of `sync_destinations`, split out so tests can drive
    /// it with fake destinations instead of the config-derived real ones. It
    /// syncs only the destinations with no existing record, records each
    /// success immediately (before any later failure), and aggregates any
    /// failures into a single error.
    async fn sync_to(
        &self,
        meeting_id: &str,
        destinations: Vec<Box<dyn sync::SyncDestination>>,
    ) -> Result<()> {
        let pending = {
            let storage = self.storage.lock().unwrap();
            let synced = storage.synced_destinations(meeting_id)?;
            let mut destinations = destinations;
            destinations.retain(|d| !synced.iter().any(|s| s == d.id()));
            destinations
        };
        if pending.is_empty() {
            return Ok(());
        }

        {
            let storage = self.storage.lock().unwrap();
            storage.set_status(meeting_id, MeetingStatus::Syncing, None)?;
        }

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

        let mut failures: Vec<(&'static str, Error)> = Vec::new();
        for dest in pending {
            self.emit(
                meeting_id,
                MeetingStatus::Syncing,
                &format!("syncing to {}", dest.display_name()),
                None,
            );
            match dest.sync(&meeting, &notes, &segments).await {
                Ok(external_ref) => {
                    let storage = self.storage.lock().unwrap();
                    storage.record_sync(meeting_id, dest.id(), &external_ref)?;
                }
                Err(e) => {
                    tracing::warn!("sync to {} failed: {e}", dest.id());
                    failures.push((dest.id(), e));
                }
            }
        }
        match failures.len() {
            0 => Ok(()),
            1 => Err(failures.pop().unwrap().1),
            _ => Err(Error::Other(
                failures
                    .iter()
                    .map(|(id, e)| format!("{id}: {e}"))
                    .collect::<Vec<_>>()
                    .join("; "),
            )),
        }
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

/// Pair each segment with the running sum of the real durations of the segments
/// before it, so Whisper's per-chunk timestamps stitch into one timeline even
/// when a segment is shorter than the nominal 5 minutes.
fn chunk_offsets(segments: &[std::path::PathBuf]) -> Result<Vec<AudioChunk>> {
    let mut chunks = Vec::with_capacity(segments.len());
    let mut offset_ms = 0i64;
    for path in segments {
        chunks.push(AudioChunk {
            path: path.clone(),
            offset_ms,
        });
        offset_ms += wav::duration_ms(path)?;
    }
    Ok(chunks)
}

fn has_labeled_transcript(storage: &Arc<Mutex<Storage>>, meeting_id: &str) -> Result<bool> {
    let storage = storage.lock().unwrap();
    let segments = storage.get_segments(meeting_id)?;
    Ok(!segments.is_empty() && segments.iter().any(|s| s.speaker != UNLABELED_SPEAKER))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::wav::WavWriter;

    #[test]
    fn chunk_offsets_accumulate_actual_durations() {
        let dir = tempfile::tempdir().unwrap();
        // seg 0: 1s (16000 samples), seg 1: a short 0.5s segment, seg 2: 1s.
        let lengths = [16_000usize, 8_000, 16_000];
        let mut paths = Vec::new();
        for (i, &n) in lengths.iter().enumerate() {
            let p = dir.path().join(format!("seg-{i:03}.wav"));
            let mut w = WavWriter::create(&p).unwrap();
            w.write_samples(&vec![0i16; n]).unwrap();
            w.finalize().unwrap();
            paths.push(p);
        }
        let chunks = chunk_offsets(&paths).unwrap();
        let offsets: Vec<i64> = chunks.iter().map(|c| c.offset_ms).collect();
        // 0, then 1000ms after seg0, then 1500ms after the short seg1.
        assert_eq!(offsets, vec![0, 1000, 1500]);
    }

    // ── sync fan-out (partial failure + resume) ─────────────────────────────

    use crate::models::{Meeting, MeetingStatus};
    use crate::sync::render::tests::{sample_notes, sample_segments};
    use crate::sync::SyncDestination;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A destination that records how many times it was synced and either
    /// succeeds (returning a ref) or fails, on command.
    struct FakeDest {
        id: &'static str,
        fail: bool,
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl SyncDestination for FakeDest {
        fn id(&self) -> &'static str {
            self.id
        }
        fn display_name(&self) -> &'static str {
            self.id
        }
        async fn sync(
            &self,
            _meeting: &Meeting,
            _notes: &crate::models::MeetingNotes,
            _segments: &[crate::models::TranscriptSegment],
        ) -> Result<String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                Err(Error::Other(format!("{} boom", self.id)))
            } else {
                Ok(format!("ref-{}", self.id))
            }
        }
    }

    /// A pipeline over an in-memory meeting that already has notes + segments,
    /// i.e. parked exactly at the sync stage.
    fn pipeline_ready_to_sync() -> Pipeline {
        let mut storage = Storage::open_in_memory().unwrap();
        storage
            .create_meeting(&Meeting {
                id: "m1".into(),
                title: "Weekly planning".into(),
                created_at: "2026-07-09T14:32:00Z".into(),
                duration_ms: 0,
                status: MeetingStatus::Summarizing,
                error: None,
                audio_dir: "/data/m1".into(),
                notion_page_id: None,
            })
            .unwrap();
        storage.replace_segments("m1", &sample_segments()).unwrap();
        storage.save_notes("m1", &sample_notes()).unwrap();
        Pipeline::new(
            Arc::new(Mutex::new(storage)),
            Config::default(),
            Arc::new(|_| {}),
        )
    }

    fn synced(pipeline: &Pipeline) -> Vec<String> {
        let mut ids = pipeline
            .storage
            .lock()
            .unwrap()
            .synced_destinations("m1")
            .unwrap();
        ids.sort();
        ids
    }

    #[tokio::test]
    async fn partial_failure_records_success_and_retry_redoes_only_the_failure() {
        let pipeline = pipeline_ready_to_sync();
        let good_calls = Arc::new(AtomicUsize::new(0));
        let bad_calls = Arc::new(AtomicUsize::new(0));

        // Round 1: "good" succeeds, "bad" fails → the run fails, but "good"
        // is already recorded (success is committed before the later failure).
        let err = pipeline
            .sync_to(
                "m1",
                vec![
                    Box::new(FakeDest {
                        id: "good",
                        fail: false,
                        calls: good_calls.clone(),
                    }),
                    Box::new(FakeDest {
                        id: "bad",
                        fail: true,
                        calls: bad_calls.clone(),
                    }),
                ],
            )
            .await;
        assert!(err.is_err());
        assert_eq!(good_calls.load(Ordering::SeqCst), 1);
        assert_eq!(bad_calls.load(Ordering::SeqCst), 1);
        assert_eq!(synced(&pipeline), vec!["good"]);

        // Round 2 (retry): "good" must be skipped (already synced), only "bad"
        // — which now succeeds — is re-attempted.
        let good2 = Arc::new(AtomicUsize::new(0));
        let bad2 = Arc::new(AtomicUsize::new(0));
        pipeline
            .sync_to(
                "m1",
                vec![
                    Box::new(FakeDest {
                        id: "good",
                        fail: false,
                        calls: good2.clone(),
                    }),
                    Box::new(FakeDest {
                        id: "bad",
                        fail: false,
                        calls: bad2.clone(),
                    }),
                ],
            )
            .await
            .unwrap();
        assert_eq!(good2.load(Ordering::SeqCst), 0, "already-synced dest re-run");
        assert_eq!(bad2.load(Ordering::SeqCst), 1);
        assert_eq!(synced(&pipeline), vec!["bad", "good"]);
    }

    #[tokio::test]
    async fn multiple_failures_aggregate_into_one_error() {
        let pipeline = pipeline_ready_to_sync();
        let err = pipeline
            .sync_to(
                "m1",
                vec![
                    Box::new(FakeDest {
                        id: "one",
                        fail: true,
                        calls: Arc::new(AtomicUsize::new(0)),
                    }),
                    Box::new(FakeDest {
                        id: "two",
                        fail: true,
                        calls: Arc::new(AtomicUsize::new(0)),
                    }),
                ],
            )
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("one"), "aggregate error names all failures: {msg}");
        assert!(msg.contains("two"), "aggregate error names all failures: {msg}");
        assert!(synced(&pipeline).is_empty());
    }

    #[tokio::test]
    async fn no_pending_destinations_is_a_noop() {
        let pipeline = pipeline_ready_to_sync();
        pipeline.sync_to("m1", vec![]).await.unwrap();
        assert!(synced(&pipeline).is_empty());
    }
}
