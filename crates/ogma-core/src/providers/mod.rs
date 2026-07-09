//! Provider traits — the seam that lets a different STT service or a local
//! backend slot in later without touching the pipeline (see PLAN.md).

pub mod claude;
pub mod whisper;

use std::path::PathBuf;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::models::{MeetingNotes, TranscriptSegment};

/// One uploadable audio chunk plus its offset into the full recording.
/// Ogma's 5-minute recording segments are used directly (16 kHz mono WAV
/// ≈ 9.6 MB per segment — under OpenAI's 25 MB cap without transcoding).
#[derive(Debug, Clone)]
pub struct AudioChunk {
    pub path: PathBuf,
    pub offset_ms: i64,
}

/// Raw (speaker-less) transcript utterance from STT.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Utterance {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

/// Output of the notes provider: speaker attribution + structured notes.
#[derive(Debug, Clone)]
pub struct NotesResult {
    /// Speaker-labeled transcript (utterances merged with attribution).
    pub segments: Vec<TranscriptSegment>,
    pub notes: MeetingNotes,
}

#[async_trait::async_trait]
pub trait TranscriptionProvider: Send + Sync {
    /// Transcribe all chunks and return utterances with absolute timestamps.
    async fn transcribe(&self, chunks: &[AudioChunk]) -> Result<Vec<Utterance>>;
}

#[async_trait::async_trait]
pub trait NotesProvider: Send + Sync {
    /// Attribute speakers and produce structured notes for a transcript.
    async fn generate(&self, title: &str, transcript: &[Utterance]) -> Result<NotesResult>;
}

/// Shared retry policy for provider HTTP calls: retries 429/5xx/529 and
/// transport errors with exponential backoff; 4xx fails immediately.
pub(crate) async fn with_retries<T, F, Fut>(op_name: &str, mut op: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    const MAX_ATTEMPTS: u32 = 4;
    let mut delay = Duration::from_secs(2);
    let mut attempt = 1;
    loop {
        match op().await {
            Ok(v) => return Ok(v),
            Err(e) => {
                let retryable = match &e {
                    Error::Api { status, .. } => matches!(status, 429 | 500..=599),
                    Error::Http(re) => re.is_timeout() || re.is_connect() || re.is_request(),
                    _ => false,
                };
                if !retryable || attempt >= MAX_ATTEMPTS {
                    return Err(e);
                }
                tracing::warn!("{op_name} attempt {attempt} failed ({e}); retrying in {delay:?}");
                tokio::time::sleep(delay).await;
                delay *= 2;
                attempt += 1;
            }
        }
    }
}
