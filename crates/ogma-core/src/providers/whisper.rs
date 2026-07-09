//! OpenAI Whisper API transcription (`whisper-1`, verbose_json).
//!
//! Each 5-minute WAV segment is uploaded as-is (≈9.6 MB, under the 25 MB
//! cap — no transcode step needed) and per-chunk segment timestamps are
//! offset by the chunk's position to stitch one absolute-time transcript.

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::providers::{with_retries, AudioChunk, TranscriptionProvider, Utterance};

const API_URL: &str = "https://api.openai.com/v1/audio/transcriptions";

pub struct WhisperProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    /// Optional ISO language hint; empty = auto-detect.
    language: String,
}

#[derive(Debug, Deserialize)]
struct VerboseResponse {
    #[serde(default)]
    segments: Vec<VerboseSegment>,
    #[serde(default)]
    text: String,
}

#[derive(Debug, Deserialize)]
struct VerboseSegment {
    start: f64,
    end: f64,
    text: String,
}

impl WhisperProvider {
    pub fn new(api_key: String, model: String, language: String) -> WhisperProvider {
        WhisperProvider {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(600))
                .build()
                .expect("reqwest client"),
            api_key,
            model,
            language,
        }
    }

    async fn transcribe_chunk(&self, chunk: &AudioChunk) -> Result<Vec<Utterance>> {
        let file_name = chunk
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("audio.wav")
            .to_string();
        let bytes = tokio::fs::read(&chunk.path).await?;

        let mut form = reqwest::multipart::Form::new()
            .part(
                "file",
                reqwest::multipart::Part::bytes(bytes)
                    .file_name(file_name)
                    .mime_str("audio/wav")
                    .map_err(|e| Error::Other(e.to_string()))?,
            )
            .text("model", self.model.clone())
            .text("response_format", "verbose_json");
        if !self.language.is_empty() {
            form = form.text("language", self.language.clone());
        }

        let resp = self
            .client
            .post(API_URL)
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Api {
                provider: "openai",
                status: status.as_u16(),
                message: truncate(&body, 500),
            });
        }

        let parsed: VerboseResponse = resp.json().await?;
        let mut utterances: Vec<Utterance> = parsed
            .segments
            .iter()
            .map(|s| Utterance {
                start_ms: chunk.offset_ms + (s.start * 1000.0) as i64,
                end_ms: chunk.offset_ms + (s.end * 1000.0) as i64,
                text: s.text.trim().to_string(),
            })
            .filter(|u| !u.text.is_empty())
            .collect();
        // Rare: verbose_json with no segments but non-empty text.
        if utterances.is_empty() && !parsed.text.trim().is_empty() {
            utterances.push(Utterance {
                start_ms: chunk.offset_ms,
                end_ms: chunk.offset_ms,
                text: parsed.text.trim().to_string(),
            });
        }
        Ok(utterances)
    }
}

#[async_trait::async_trait]
impl TranscriptionProvider for WhisperProvider {
    async fn transcribe(&self, chunks: &[AudioChunk]) -> Result<Vec<Utterance>> {
        let mut all = Vec::new();
        for (i, chunk) in chunks.iter().enumerate() {
            tracing::info!("transcribing chunk {}/{}", i + 1, chunks.len());
            let utterances =
                with_retries("whisper transcription", || self.transcribe_chunk(chunk)).await?;
            all.extend(utterances);
        }
        all.sort_by_key(|u| u.start_ms);
        Ok(all)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}
