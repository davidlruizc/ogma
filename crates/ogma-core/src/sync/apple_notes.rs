//! Apple Notes destination (macOS only — see docs/sync-destinations-spike.md).
//!
//! Notes has no public API; `osascript` can create a note with an HTML body.
//! Notes are created in an "Ogma" folder (created on first sync) in the
//! default account, so iCloud carries them to iPhone/iPad automatically. The
//! first sync triggers macOS's one-time Automation permission prompt; a
//! denial surfaces as a settings-hint error, not a silent failure.
//!
//! AppleScript doesn't exist on iOS, so this destination is macOS-only by
//! nature (`#[cfg(target_os = "macos")]` in `sync/mod.rs`).

use crate::error::{Error, Result};
use crate::models::{Meeting, MeetingNotes, TranscriptSegment};
use crate::sync::render;
use crate::sync::SyncDestination;

pub struct AppleNotesDestination;

/// Title and body arrive via `argv`, never spliced into the script source —
/// meeting titles and transcripts can contain anything.
const SCRIPT: &str = r#"
on run argv
    set noteBody to item 1 of argv
    tell application "Notes"
        if not (exists folder "Ogma") then
            make new folder with properties {name:"Ogma"}
        end if
        set theNote to make new note at folder "Ogma" with properties {body:noteBody}
        return id of theNote
    end tell
end run
"#;

#[async_trait::async_trait]
impl SyncDestination for AppleNotesDestination {
    fn id(&self) -> &'static str {
        "apple_notes"
    }

    fn display_name(&self) -> &'static str {
        "Apple Notes"
    }

    async fn sync(
        &self,
        meeting: &Meeting,
        notes: &MeetingNotes,
        segments: &[TranscriptSegment],
    ) -> Result<String> {
        let body = note_html(meeting, notes, segments);
        // std::process on a blocking thread: the workspace tokio build has no
        // "process" feature, and this runs at most once per meeting.
        let output = tokio::task::spawn_blocking(move || {
            std::process::Command::new("osascript")
                .arg("-e")
                .arg(SCRIPT)
                .arg(&body)
                .output()
        })
        .await
        .map_err(|e| Error::Other(format!("osascript task failed: {e}")))??;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            // -1743 = errAEEventNotPermitted: the user declined the one-time
            // Automation prompt (or it was revoked in System Settings).
            let hint = if stderr.contains("-1743") {
                " — allow Ogma to control Notes in System Settings → Privacy & Security → Automation"
            } else {
                ""
            };
            return Err(Error::Other(format!(
                "Apple Notes sync failed: {stderr}{hint}"
            )));
        }
        let note_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if note_id.is_empty() {
            return Err(Error::Other(
                "Apple Notes sync returned no note id".into(),
            ));
        }
        Ok(note_id)
    }
}

/// The note's HTML body. Notes takes the first line as the note title, so it
/// starts with an `<h1>` of the meeting title, then a meta line, then the
/// shared blocks rendered as HTML.
fn note_html(meeting: &Meeting, notes: &MeetingNotes, segments: &[TranscriptSegment]) -> String {
    let mut out = format!("<h1>{}</h1>\n", render::html_escape(&meeting.title));
    let mut meta = vec![friendly_date(&meeting.created_at)];
    if meeting.duration_ms > 0 {
        meta.push(format!("{} min", meeting.duration_ms / 60_000));
    }
    let attendees = render::distinct_speakers(segments);
    if !attendees.is_empty() {
        meta.push(attendees.join(", "));
    }
    out.push_str(&format!(
        "<div><i>{}</i></div>\n",
        render::html_escape(&meta.join(" · "))
    ));
    out.push_str(&render::blocks_to_html(&render::note_blocks(
        notes, segments,
    )));
    out
}

/// `2026-07-09 14:32` from the stored RFC3339 stamp; the raw stamp if it
/// doesn't parse (never fails the sync over cosmetics).
fn friendly_date(created_at: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(created_at)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|_| created_at.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::MeetingStatus;
    use crate::sync::render::tests::{sample_notes, sample_segments};

    fn meeting() -> Meeting {
        Meeting {
            id: "m1".into(),
            title: "Q3 <planning> & review".into(),
            created_at: "2026-07-09T14:32:00+02:00".into(),
            duration_ms: 3_720_000,
            status: MeetingStatus::Done,
            error: None,
            audio_dir: "/data/m1".into(),
            notion_page_id: None,
        }
    }

    #[test]
    fn note_html_has_title_meta_and_body() {
        let html = note_html(&meeting(), &sample_notes(), &sample_segments());
        assert!(html.starts_with("<h1>Q3 &lt;planning&gt; &amp; review</h1>"));
        assert!(html.contains("<div><i>2026-07-09 14:32 · 62 min · Maria, Tom</i></div>"));
        assert!(html.contains("<h2>Transcript</h2>"));
    }

    #[test]
    fn friendly_date_falls_back_to_raw() {
        assert_eq!(friendly_date("2026-07-09T14:32:00+02:00"), "2026-07-09 14:32");
        assert_eq!(friendly_date("garbage"), "garbage");
    }

    /// Real end-to-end check on a Mac: creates an actual note in Apple Notes
    /// (and triggers the Automation prompt on first run). Opt-in only:
    /// `cargo test -p ogma-core apple_notes -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn creates_a_real_note() {
        let id = AppleNotesDestination
            .sync(&meeting(), &sample_notes(), &sample_segments())
            .await
            .unwrap();
        assert!(!id.is_empty());
    }
}
