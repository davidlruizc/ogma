//! Notion REST client (direct API, not MCP — the app is a service).
//!
//! One-time setup creates a "Meetings" database under a page the user
//! shares with the integration; per meeting we create a page with the notes
//! sections plus a toggle holding the full speaker-labeled transcript.
//! Notion caps: ≤100 blocks per request, ≤2000 chars per rich_text item —
//! the transcript is appended to the toggle in batches.

use serde_json::{json, Value};

use crate::error::{Error, Result};
use crate::models::{Meeting, MeetingNotes, TranscriptSegment};
use crate::providers::with_retries;
use crate::sync::render::format_ms;
use crate::sync::SyncDestination;

const API: &str = "https://api.notion.com/v1";
const NOTION_VERSION: &str = "2022-06-28";
const MAX_TEXT: usize = 1900; // margin under the 2000-char rich_text cap
const BATCH: usize = 90; // margin under the 100-block cap

pub struct NotionClient {
    client: reqwest::Client,
    token: String,
    database_id: String,
}

impl NotionClient {
    pub fn new(token: String, database_id: String) -> NotionClient {
        NotionClient {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .expect("reqwest client"),
            token,
            database_id,
        }
    }

    async fn request(&self, method: reqwest::Method, path: &str, body: Value) -> Result<Value> {
        let url = format!("{API}{path}");
        let mut req = self
            .client
            .request(method, &url)
            .bearer_auth(&self.token)
            .header("Notion-Version", NOTION_VERSION);
        if !body.is_null() {
            req = req.json(&body);
        }
        let resp = req.send().await?;
        let status = resp.status();
        let value: Value = resp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            let message = value["message"].as_str().unwrap_or("unknown error");
            return Err(Error::Api {
                provider: "notion",
                status: status.as_u16(),
                message: message.to_string(),
            });
        }
        Ok(value)
    }

    /// One-time setup: create a "Meetings" database under `parent_page_id`
    /// (the page must be shared with the integration). Returns database id.
    pub async fn create_meetings_database(&self, parent_page_id: &str) -> Result<String> {
        let body = json!({
            "parent": {"type": "page_id", "page_id": parent_page_id},
            "title": [{"type": "text", "text": {"content": "Meetings"}}],
            "properties": {
                "Name": {"title": {}},
                "Date": {"date": {}},
                "Duration (min)": {"number": {}},
                "Attendees": {"multi_select": {}},
                // A true Notion rollup needs a separate related database; the
                // per-meeting action-item *count* as a number property gives the
                // same at-a-glance/sortable signal without that extra structure.
                "Action Items": {"number": {}},
                "Status": {"select": {"options": [
                    {"name": "Done", "color": "green"},
                    {"name": "Processing", "color": "yellow"}
                ]}}
            }
        });
        let resp = with_retries("notion create database", || {
            self.request(reqwest::Method::POST, "/databases", body.clone())
        })
        .await?;
        resp["id"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| Error::Other("notion: database response missing id".into()))
    }

    /// Backfill the properties each page sets onto a database created by an
    /// older Ogma version that predates them. Notion's database update is
    /// idempotent — it adds missing properties and leaves existing ones
    /// untouched — so this is safe to call before every sync.
    async fn ensure_schema(&self) -> Result<()> {
        let body = json!({
            "properties": {
                "Attendees": {"multi_select": {}},
                "Action Items": {"number": {}}
            }
        });
        let path = format!("/databases/{}", self.database_id);
        with_retries("notion ensure schema", || {
            self.request(reqwest::Method::PATCH, &path, body.clone())
        })
        .await
        .map(|_| ())
    }

    /// Create the meeting page and append the transcript. Returns page id.
    pub async fn create_meeting_page(
        &self,
        meeting: &Meeting,
        notes: &MeetingNotes,
        segments: &[TranscriptSegment],
    ) -> Result<String> {
        // Ensure the newer Attendees/Action Items columns exist before we set
        // them, so a database created by an older version doesn't 400 into the
        // degraded fallback. Best-effort: if this fails the surgical fallback
        // below still preserves the core properties and the full transcript.
        if let Err(e) = self.ensure_schema().await {
            tracing::warn!("notion ensure schema failed: {e}");
        }

        // Notes blocks plus the toggle that holds the full transcript. Rebuilt
        // for the fallback body too (`json!` moves the Vec), so the transcript
        // toggle is present on both the primary and the degraded page.
        let transcript_children = || {
            let mut children = notes_blocks(notes);
            children.push(json!({
                "object": "block",
                "type": "toggle",
                "toggle": {"rich_text": [text_rt("Full transcript")]}
            }));
            children
        };

        let attendees: Vec<Value> = distinct_speakers(segments)
            .into_iter()
            .map(|name| json!({"name": name}))
            .collect();

        let body = json!({
            "parent": {"database_id": self.database_id},
            "properties": {
                "Name": {"title": [text_rt(&meeting.title)]},
                "Date": {"date": {"start": meeting.created_at}},
                "Duration (min)": {"number": meeting.duration_ms / 60_000},
                "Attendees": {"multi_select": attendees},
                "Action Items": {"number": notes.action_items.len()},
                "Status": {"select": {"name": "Done"}}
            },
            "children": transcript_children()
        });

        let resp = match with_retries("notion create page", || {
            self.request(reqwest::Method::POST, "/pages", body.clone())
        })
        .await
        {
            Ok(v) => v,
            Err(Error::Api { status: 400, .. }) => {
                // A property we set is still missing on an older database (e.g.
                // ensure_schema couldn't run). Retry without the newer
                // Attendees/Action Items properties, but keep the core
                // properties AND the transcript toggle so the full transcript is
                // never silently dropped.
                let fallback = json!({
                    "parent": {"database_id": self.database_id},
                    "properties": {
                        "Name": {"title": [text_rt(&meeting.title)]},
                        "Date": {"date": {"start": meeting.created_at}},
                        "Duration (min)": {"number": meeting.duration_ms / 60_000},
                        "Status": {"select": {"name": "Done"}}
                    },
                    "children": transcript_children()
                });
                with_retries("notion create page (fallback)", || {
                    self.request(reqwest::Method::POST, "/pages", fallback.clone())
                })
                .await?
            }
            Err(e) => return Err(e),
        };
        let page_id = resp["id"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| Error::Other("notion: page response missing id".into()))?;

        if let Err(e) = self.append_transcript(&page_id, segments).await {
            // The page exists and carries the notes — a transcript append
            // failure shouldn't fail the whole sync.
            tracing::warn!("notion transcript append failed: {e}");
        }
        Ok(page_id)
    }

    async fn append_transcript(&self, page_id: &str, segments: &[TranscriptSegment]) -> Result<()> {
        if segments.is_empty() {
            return Ok(());
        }
        // Find the toggle block we created on the page.
        let children_path = format!("/blocks/{page_id}/children?page_size=100");
        let children = with_retries("notion list children", || {
            self.request(reqwest::Method::GET, &children_path, Value::Null)
        })
        .await?;
        let toggle_id = children["results"]
            .as_array()
            .and_then(|blocks| {
                blocks
                    .iter()
                    .find(|b| b["type"] == "toggle")
                    .and_then(|b| b["id"].as_str())
            })
            .map(String::from);
        let Some(toggle_id) = toggle_id else {
            return Ok(()); // minimal-page fallback has no toggle
        };

        let paragraphs: Vec<Value> = segments
            .iter()
            .flat_map(|seg| {
                let stamp = format_ms(seg.start_ms);
                split_text(&seg.text, MAX_TEXT)
                    .into_iter()
                    .enumerate()
                    .map(move |(i, piece)| {
                        let prefix = if i == 0 {
                            format!("{} ({stamp}): ", seg.speaker)
                        } else {
                            String::new()
                        };
                        json!({
                            "object": "block",
                            "type": "paragraph",
                            "paragraph": {"rich_text": [text_rt(&format!("{prefix}{piece}"))]}
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        let append_path = format!("/blocks/{toggle_id}/children");
        for batch in paragraphs.chunks(BATCH) {
            let body = json!({"children": batch});
            with_retries("notion append transcript", || {
                self.request(reqwest::Method::PATCH, &append_path, body.clone())
            })
            .await?;
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl SyncDestination for NotionClient {
    fn id(&self) -> &'static str {
        "notion"
    }

    fn display_name(&self) -> &'static str {
        "Notion"
    }

    async fn sync(
        &self,
        meeting: &Meeting,
        notes: &MeetingNotes,
        segments: &[TranscriptSegment],
    ) -> Result<String> {
        self.create_meeting_page(meeting, notes, segments).await
    }
}

/// Distinct speaker labels in first-seen order, dropping the unlabeled
/// sentinel. Notion multi_select option names can't contain commas, so those
/// are swapped for spaces.
fn distinct_speakers(segments: &[TranscriptSegment]) -> Vec<String> {
    let mut seen = Vec::new();
    for seg in segments {
        if seg.speaker == crate::pipeline::UNLABELED_SPEAKER {
            continue;
        }
        let name = seg.speaker.replace(',', " ");
        let name = name.trim();
        if !name.is_empty() && !seen.iter().any(|s: &String| s == name) {
            seen.push(name.to_string());
        }
    }
    seen
}

fn text_rt(content: &str) -> Value {
    json!({"type": "text", "text": {"content": content}})
}

fn heading(text: &str) -> Value {
    json!({
        "object": "block",
        "type": "heading_2",
        "heading_2": {"rich_text": [text_rt(text)]}
    })
}

fn paragraph(text: &str) -> Value {
    json!({
        "object": "block",
        "type": "paragraph",
        "paragraph": {"rich_text": [text_rt(text)]}
    })
}

fn bullet(text: &str) -> Value {
    json!({
        "object": "block",
        "type": "bulleted_list_item",
        "bulleted_list_item": {"rich_text": [text_rt(text)]}
    })
}

fn notes_blocks(notes: &MeetingNotes) -> Vec<Value> {
    let mut blocks = Vec::new();
    blocks.push(json!({
        "object": "block",
        "type": "callout",
        "callout": {
            "rich_text": [text_rt(&clamp(&notes.tldr))],
            "icon": {"type": "emoji", "emoji": "🗒️"}
        }
    }));

    blocks.push(heading("Summary"));
    for para in notes.summary.split("\n\n").filter(|p| !p.trim().is_empty()) {
        for piece in split_text(para.trim(), MAX_TEXT) {
            blocks.push(paragraph(&piece));
        }
    }

    if !notes.key_points.is_empty() {
        blocks.push(heading("Key points"));
        for point in &notes.key_points {
            blocks.push(bullet(&clamp(point)));
        }
    }
    if !notes.decisions.is_empty() {
        blocks.push(heading("Decisions"));
        for decision in &notes.decisions {
            blocks.push(bullet(&clamp(decision)));
        }
    }
    if !notes.action_items.is_empty() {
        blocks.push(heading("Action items"));
        for item in &notes.action_items {
            let mut text = item.task.clone();
            if let Some(owner) = &item.owner {
                text.push_str(&format!(" — {owner}"));
            }
            if let Some(due) = &item.due {
                text.push_str(&format!(" (due {due})"));
            }
            blocks.push(json!({
                "object": "block",
                "type": "to_do",
                "to_do": {"rich_text": [text_rt(&clamp(&text))], "checked": false}
            }));
        }
    }
    if !notes.open_questions.is_empty() {
        blocks.push(heading("Open questions"));
        for q in &notes.open_questions {
            blocks.push(bullet(&clamp(q)));
        }
    }
    if !notes.highlights.is_empty() {
        blocks.push(heading("Highlights"));
        for h in &notes.highlights {
            blocks.push(json!({
                "object": "block",
                "type": "quote",
                "quote": {"rich_text": [text_rt(&clamp(&format!(
                    "\u{201c}{}\u{201d} — {} ({})",
                    h.quote, h.speaker, format_ms(h.timestamp_ms)
                )))]}
            }));
        }
    }
    // Page create allows ≤100 children; the transcript toggle is added by
    // the caller, so keep headroom.
    blocks.truncate(95);
    blocks
}

fn clamp(s: &str) -> String {
    split_text(s, MAX_TEXT).into_iter().next().unwrap_or_default()
}

fn split_text(s: &str, max_chars: usize) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    if chars.is_empty() {
        return vec![String::new()];
    }
    chars
        .chunks(max_chars)
        .map(|c| c.iter().collect::<String>())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_text_respects_char_boundaries() {
        let s = "é".repeat(4000);
        let pieces = split_text(&s, 1900);
        assert_eq!(pieces.len(), 3);
        assert!(pieces.iter().all(|p| p.chars().count() <= 1900));
    }

    #[test]
    fn distinct_speakers_dedupes_and_drops_unlabeled() {
        let seg = |speaker: &str| TranscriptSegment {
            speaker: speaker.to_string(),
            start_ms: 0,
            end_ms: 0,
            text: String::new(),
        };
        let segments = vec![
            seg("Maria"),
            seg(crate::pipeline::UNLABELED_SPEAKER),
            seg("Tom, Jr."),
            seg("Maria"),
        ];
        assert_eq!(
            distinct_speakers(&segments),
            vec!["Maria".to_string(), "Tom  Jr.".to_string()]
        );
    }
}
