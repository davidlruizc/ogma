//! Markdown file destination — an Obsidian vault is just a folder, so this
//! covers Obsidian, Logseq and "give me my notes as files" in one go. No
//! network, no auth: render to a String and write one `.md` per meeting into
//! the folder from settings, with YAML frontmatter (title, date, attendees)
//! so Obsidian's properties/Dataview pick it up.

use std::path::PathBuf;

use crate::error::Result;
use crate::models::{Meeting, MeetingNotes, TranscriptSegment};
use crate::sync::render;
use crate::sync::SyncDestination;

pub struct MarkdownDestination {
    dir: PathBuf,
}

impl MarkdownDestination {
    pub fn new(dir: impl Into<PathBuf>) -> MarkdownDestination {
        MarkdownDestination { dir: dir.into() }
    }
}

#[async_trait::async_trait]
impl SyncDestination for MarkdownDestination {
    fn id(&self) -> &'static str {
        "markdown"
    }

    fn display_name(&self) -> &'static str {
        "Markdown folder"
    }

    async fn sync(
        &self,
        meeting: &Meeting,
        notes: &MeetingNotes,
        segments: &[TranscriptSegment],
    ) -> Result<String> {
        let content = render_markdown(meeting, notes, segments);
        let path = self.dir.join(file_name(meeting));
        tokio::fs::create_dir_all(&self.dir).await?;
        // Overwrite is safe: the name embeds the meeting's start minute, so
        // only a retry of the same meeting ever hits an existing file.
        tokio::fs::write(&path, content).await?;
        Ok(path.to_string_lossy().to_string())
    }
}

/// `YYYY-MM-DD HH.MM <title>.md` — sortable, readable in a vault, and
/// collision-free across meetings (two meetings can't share a start minute).
fn file_name(meeting: &Meeting) -> String {
    let stamp = chrono::DateTime::parse_from_rfc3339(&meeting.created_at)
        .map(|dt| dt.format("%Y-%m-%d %H.%M").to_string())
        .unwrap_or_else(|_| meeting.id.chars().take(8).collect());
    format!("{stamp} {}.md", sanitize_title(&meeting.title))
}

/// Strip filesystem-hostile characters and cap the length; never empty.
fn sanitize_title(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let truncated: String = collapsed.chars().take(80).collect();
    let trimmed = truncated.trim_matches(|c: char| c == '.' || c.is_whitespace());
    if trimmed.is_empty() {
        "Meeting".to_string()
    } else {
        trimmed.to_string()
    }
}

fn render_markdown(
    meeting: &Meeting,
    notes: &MeetingNotes,
    segments: &[TranscriptSegment],
) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("title: {}\n", yaml_quote(&meeting.title)));
    out.push_str(&format!("date: {}\n", meeting.created_at));
    if meeting.duration_ms > 0 {
        out.push_str(&format!(
            "duration_minutes: {}\n",
            meeting.duration_ms / 60_000
        ));
    }
    let attendees = render::distinct_speakers(segments);
    if !attendees.is_empty() {
        out.push_str("attendees:\n");
        for name in &attendees {
            out.push_str(&format!("  - {}\n", yaml_quote(name)));
        }
    }
    out.push_str("tags:\n  - meeting\nsource: ogma\n---\n\n");
    out.push_str(&render::blocks_to_markdown(&render::note_blocks(
        notes, segments,
    )));
    out
}

/// Double-quoted YAML scalar — the only escapes needed are `\` and `"`.
fn yaml_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::MeetingStatus;
    use crate::sync::render::tests::{sample_notes, sample_segments};

    fn meeting() -> Meeting {
        Meeting {
            id: "abcdef12-3456-7890-abcd-ef1234567890".into(),
            title: "Weekly planning".into(),
            created_at: "2026-07-09T14:32:00+02:00".into(),
            duration_ms: 3_720_000,
            status: MeetingStatus::Done,
            error: None,
            audio_dir: "/data/m1".into(),
            notion_page_id: None,
        }
    }

    #[test]
    fn file_name_is_stamped_and_sanitized() {
        let mut m = meeting();
        m.title = "Q3: budget / planning?".into();
        assert_eq!(file_name(&m), "2026-07-09 14.32 Q3 budget planning.md");
    }

    #[test]
    fn file_name_falls_back_to_id_on_bad_date() {
        let mut m = meeting();
        m.created_at = "not a date".into();
        assert_eq!(file_name(&m), "abcdef12 Weekly planning.md");
    }

    #[test]
    fn sanitize_title_never_returns_empty() {
        assert_eq!(sanitize_title("///"), "Meeting");
        assert_eq!(sanitize_title("  ..  "), "Meeting");
        assert_eq!(sanitize_title("ok"), "ok");
        assert!(sanitize_title(&"x".repeat(200)).chars().count() <= 80);
    }

    #[test]
    fn frontmatter_lists_attendees_and_meta() {
        let md = render_markdown(&meeting(), &sample_notes(), &sample_segments());
        assert!(md.starts_with("---\ntitle: \"Weekly planning\"\n"));
        assert!(md.contains("date: 2026-07-09T14:32:00+02:00\n"));
        assert!(md.contains("duration_minutes: 62\n"));
        assert!(md.contains("attendees:\n  - \"Maria\"\n  - \"Tom\"\n"));
        assert!(md.contains("## Transcript"));
    }

    #[test]
    fn yaml_quote_escapes() {
        assert_eq!(yaml_quote(r#"a "b" \c"#), r#""a \"b\" \\c""#);
    }

    #[tokio::test]
    async fn sync_writes_the_file_and_returns_its_path() {
        let dir = tempfile::tempdir().unwrap();
        let dest = MarkdownDestination::new(dir.path().join("meetings"));
        let m = meeting();
        let path = dest
            .sync(&m, &sample_notes(), &sample_segments())
            .await
            .unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("**Maria (0:00):** Let's get started."));
        // Idempotent retry: overwriting the same meeting's file succeeds.
        let again = dest
            .sync(&m, &sample_notes(), &sample_segments())
            .await
            .unwrap();
        assert_eq!(path, again);
    }
}
