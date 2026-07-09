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

    /// Create the meeting page and append the transcript. Returns page id.
    pub async fn create_meeting_page(
        &self,
        meeting: &Meeting,
        notes: &MeetingNotes,
        segments: &[TranscriptSegment],
    ) -> Result<String> {
        let mut children = notes_blocks(notes);
        children.push(json!({
            "object": "block",
            "type": "toggle",
            "toggle": {"rich_text": [text_rt("Full transcript")]}
        }));

        let body = json!({
            "parent": {"database_id": self.database_id},
            "properties": {
                "Name": {"title": [text_rt(&meeting.title)]},
                "Date": {"date": {"start": meeting.created_at}},
                "Duration (min)": {"number": meeting.duration_ms / 60_000},
                "Status": {"select": {"name": "Done"}}
            },
            "children": children
        });

        let resp = match with_retries("notion create page", || {
            self.request(reqwest::Method::POST, "/pages", body.clone())
        })
        .await
        {
            Ok(v) => v,
            Err(Error::Api { status: 400, .. }) => {
                // Existing databases may not have our property names — retry
                // with just the title so sync still succeeds.
                let minimal = json!({
                    "parent": {"database_id": self.database_id},
                    "properties": {"Name": {"title": [text_rt(&meeting.title)]}},
                    "children": notes_blocks(notes)
                });
                with_retries("notion create page (minimal)", || {
                    self.request(reqwest::Method::POST, "/pages", minimal.clone())
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

fn format_ms(ms: i64) -> String {
    let total_secs = ms / 1000;
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    let s = total_secs % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
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
    fn format_ms_variants() {
        assert_eq!(format_ms(65_000), "1:05");
        assert_eq!(format_ms(3_725_000), "1:02:05");
    }
}
