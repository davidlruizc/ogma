//! Claude API notes provider (raw REST — no official Rust SDK).
//!
//! One combined call does both speaker attribution and structured notes
//! (PLAN.md "Combined (default)"). To keep output small and avoid text
//! drift, Claude does NOT re-emit the transcript: the prompt numbers each
//! utterance and the response assigns speakers as index ranges
//! (`[{start_index, end_index, speaker}]`), which we merge with the stored
//! Whisper utterances locally. Output stays a few thousand tokens even for
//! a 3-hour meeting, so a plain (non-streaming) request is safe.
//!
//! Structured JSON is enforced with `output_config.format` (json_schema).

use serde::Deserialize;
use serde_json::json;

use crate::error::{Error, Result};
use crate::models::{Highlight, MeetingNotes, NoteActionItem, TranscriptSegment};
use crate::providers::{with_retries, NotesProvider, NotesResult, Utterance};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const MAX_TOKENS: u32 = 16_000;

pub struct ClaudeProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

/// Claude's response shape (schema-enforced).
#[derive(Debug, Deserialize)]
struct ClaudeNotes {
    speaker_assignments: Vec<SpeakerRange>,
    title: String,
    tldr: String,
    summary: String,
    #[serde(default)]
    key_points: Vec<String>,
    #[serde(default)]
    decisions: Vec<String>,
    #[serde(default)]
    action_items: Vec<ClaudeActionItem>,
    #[serde(default)]
    open_questions: Vec<String>,
    #[serde(default)]
    highlights: Vec<ClaudeHighlight>,
}

#[derive(Debug, Deserialize)]
struct SpeakerRange {
    start_index: usize,
    end_index: usize,
    speaker: String,
}

#[derive(Debug, Deserialize)]
struct ClaudeActionItem {
    task: String,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    due: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClaudeHighlight {
    quote: String,
    speaker: String,
    /// Index of the utterance the quote comes from (→ timestamp locally).
    utterance_index: usize,
}

impl ClaudeProvider {
    pub fn new(api_key: String, model: String) -> ClaudeProvider {
        ClaudeProvider {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(900))
                .build()
                .expect("reqwest client"),
            api_key,
            model,
        }
    }

    fn output_schema() -> serde_json::Value {
        let str_array = json!({"type": "array", "items": {"type": "string"}});
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["speaker_assignments", "title", "tldr", "summary", "key_points",
                         "decisions", "action_items", "open_questions", "highlights"],
            "properties": {
                "speaker_assignments": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["start_index", "end_index", "speaker"],
                        "properties": {
                            "start_index": {"type": "integer"},
                            "end_index": {"type": "integer"},
                            "speaker": {"type": "string"}
                        }
                    }
                },
                "title": {"type": "string"},
                "tldr": {"type": "string"},
                "summary": {"type": "string"},
                "key_points": str_array,
                "decisions": str_array,
                "action_items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["task", "owner", "due"],
                        "properties": {
                            "task": {"type": "string"},
                            "owner": {"type": ["string", "null"]},
                            "due": {"type": ["string", "null"]}
                        }
                    }
                },
                "open_questions": str_array,
                "highlights": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["quote", "speaker", "utterance_index"],
                        "properties": {
                            "quote": {"type": "string"},
                            "speaker": {"type": "string"},
                            "utterance_index": {"type": "integer"}
                        }
                    }
                }
            }
        })
    }

    fn build_prompt(title: &str, transcript: &[Utterance]) -> String {
        let mut lines = String::with_capacity(transcript.len() * 80);
        for (i, u) in transcript.iter().enumerate() {
            let mins = u.start_ms / 60_000;
            let secs = (u.start_ms % 60_000) / 1000;
            lines.push_str(&format!("[{i}] ({mins:02}:{secs:02}) {}\n", u.text));
        }
        format!(
            "Below is the timestamped transcript of an in-person meeting titled \"{title}\". \
             It was produced by speech-to-text with NO speaker labels. Each utterance is \
             numbered [n] with its (mm:ss) start time.\n\
             \n\
             Your tasks:\n\
             1. SPEAKER ATTRIBUTION: infer the distinct speakers from turn-taking, direct \
                address, self-references, and context. Label them \"Speaker A\", \"Speaker B\", \
                etc. (use a real name only when the transcript makes it certain, e.g. someone is \
                addressed by name right before responding). Cover EVERY utterance index exactly \
                once using contiguous ranges in `speaker_assignments` (start_index and end_index \
                are inclusive).\n\
             2. MEETING NOTES: a short specific `title`, a 1-2 sentence `tldr`, a `summary` \
                (2-4 paragraphs), `key_points`, `decisions` (only decisions actually made), \
                `action_items` (task + owner if identifiable + due if mentioned), \
                `open_questions`, and 3-8 `highlights` (verbatim short quotes worth revisiting, \
                each with its speaker label and the `utterance_index` it comes from).\n\
             \n\
             Attribution is best-effort: prefer fewer, consistent speakers over fragmenting; \
             if a stretch is genuinely unattributable, keep it with the likeliest speaker.\n\
             \n\
             TRANSCRIPT:\n{lines}"
        )
    }

    async fn call(&self, prompt: &str) -> Result<ClaudeNotes> {
        let body = json!({
            "model": self.model,
            "max_tokens": MAX_TOKENS,
            "output_config": {"format": {"type": "json_schema", "schema": Self::output_schema()}},
            "messages": [{"role": "user", "content": prompt}],
        });

        let resp = self
            .client
            .post(API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(Error::Api {
                provider: "anthropic",
                status: status.as_u16(),
                message: truncate(&text, 500),
            });
        }

        let value: serde_json::Value = resp.json().await?;
        let stop_reason = value["stop_reason"].as_str().unwrap_or_default();
        if stop_reason == "refusal" {
            return Err(Error::Api {
                provider: "anthropic",
                status: 200,
                message: "model declined the request (stop_reason=refusal)".into(),
            });
        }
        if stop_reason == "max_tokens" {
            return Err(Error::Api {
                provider: "anthropic",
                status: 200,
                message: "output truncated (stop_reason=max_tokens)".into(),
            });
        }
        let text = value["content"]
            .as_array()
            .and_then(|blocks| {
                blocks
                    .iter()
                    .find(|b| b["type"] == "text")
                    .and_then(|b| b["text"].as_str())
            })
            .ok_or_else(|| Error::Api {
                provider: "anthropic",
                status: 200,
                message: "no text block in response".into(),
            })?;
        Ok(serde_json::from_str(text)?)
    }
}

#[async_trait::async_trait]
impl NotesProvider for ClaudeProvider {
    async fn generate(&self, title: &str, transcript: &[Utterance]) -> Result<NotesResult> {
        if transcript.is_empty() {
            return Err(Error::InvalidState("transcript is empty".into()));
        }
        let prompt = Self::build_prompt(title, transcript);
        let raw = with_retries("claude notes", || self.call(&prompt)).await?;
        Ok(assemble(transcript, raw))
    }
}

/// Merge Claude's index-range speaker assignments back onto the utterances
/// and resolve highlight indices to timestamps. Defensive against gaps or
/// out-of-range indices — unassigned utterances get "Speaker ?".
fn assemble(transcript: &[Utterance], raw: ClaudeNotes) -> NotesResult {
    let mut speakers: Vec<Option<&str>> = vec![None; transcript.len()];
    for range in &raw.speaker_assignments {
        let end = range.end_index.min(transcript.len().saturating_sub(1));
        for slot in speakers.iter_mut().take(end + 1).skip(range.start_index) {
            if slot.is_none() {
                *slot = Some(range.speaker.as_str());
            }
        }
    }

    // Collapse consecutive same-speaker utterances into one segment.
    let mut segments: Vec<TranscriptSegment> = Vec::new();
    for (i, u) in transcript.iter().enumerate() {
        let speaker = speakers[i].unwrap_or("Speaker ?");
        match segments.last_mut() {
            Some(last) if last.speaker == speaker => {
                last.end_ms = u.end_ms;
                last.text.push(' ');
                last.text.push_str(&u.text);
            }
            _ => segments.push(TranscriptSegment {
                speaker: speaker.to_string(),
                start_ms: u.start_ms,
                end_ms: u.end_ms,
                text: u.text.clone(),
            }),
        }
    }

    let highlights = raw
        .highlights
        .into_iter()
        .map(|h| Highlight {
            timestamp_ms: transcript
                .get(h.utterance_index)
                .map(|u| u.start_ms)
                .unwrap_or(0),
            quote: h.quote,
            speaker: h.speaker,
        })
        .collect();

    NotesResult {
        segments,
        notes: MeetingNotes {
            title: raw.title,
            tldr: raw.tldr,
            summary: raw.summary,
            key_points: raw.key_points,
            decisions: raw.decisions,
            action_items: raw
                .action_items
                .into_iter()
                .map(|a| NoteActionItem {
                    task: a.task,
                    owner: a.owner,
                    due: a.due,
                })
                .collect(),
            open_questions: raw.open_questions,
            highlights,
        },
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

#[cfg(test)]
mod tests {
    use super::*;

    fn utt(start: i64, text: &str) -> Utterance {
        Utterance {
            start_ms: start,
            end_ms: start + 1000,
            text: text.into(),
        }
    }

    #[test]
    fn assemble_merges_ranges_and_collapses_runs() {
        let transcript = vec![
            utt(0, "Hi everyone."),
            utt(1000, "Let's get started."),
            utt(2000, "Sounds good."),
            utt(3000, "First item is billing."),
        ];
        let raw = ClaudeNotes {
            speaker_assignments: vec![
                SpeakerRange { start_index: 0, end_index: 1, speaker: "Speaker A".into() },
                SpeakerRange { start_index: 2, end_index: 2, speaker: "Speaker B".into() },
                SpeakerRange { start_index: 3, end_index: 3, speaker: "Speaker A".into() },
            ],
            title: "t".into(),
            tldr: "t".into(),
            summary: "s".into(),
            key_points: vec![],
            decisions: vec![],
            action_items: vec![],
            open_questions: vec![],
            highlights: vec![ClaudeHighlight {
                quote: "First item is billing.".into(),
                speaker: "Speaker A".into(),
                utterance_index: 3,
            }],
        };
        let result = assemble(&transcript, raw);
        assert_eq!(result.segments.len(), 3);
        assert_eq!(result.segments[0].speaker, "Speaker A");
        assert_eq!(result.segments[0].text, "Hi everyone. Let's get started.");
        assert_eq!(result.segments[1].speaker, "Speaker B");
        assert_eq!(result.notes.highlights[0].timestamp_ms, 3000);
    }

    #[test]
    fn assemble_handles_gaps_and_overflow() {
        let transcript = vec![utt(0, "a"), utt(1000, "b")];
        let raw = ClaudeNotes {
            speaker_assignments: vec![SpeakerRange {
                start_index: 1,
                end_index: 99, // out of range — clamped
                speaker: "Speaker A".into(),
            }],
            title: "t".into(),
            tldr: "t".into(),
            summary: "s".into(),
            key_points: vec![],
            decisions: vec![],
            action_items: vec![],
            open_questions: vec![],
            highlights: vec![],
        };
        let result = assemble(&transcript, raw);
        assert_eq!(result.segments[0].speaker, "Speaker ?"); // index 0 unassigned
        assert_eq!(result.segments[1].speaker, "Speaker A");
    }
}
