//! Shared document renderer for file-ish destinations: one block model
//! rendered to Markdown (file/Obsidian) and, later, HTML (Apple Notes).
//! Notion keeps its own private block renderer — its block model is the odd
//! one out and already works (see docs/sync-destinations-spike.md).

use crate::models::{MeetingNotes, TranscriptSegment};

/// One rendered element of a meeting note, format-agnostic.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// The TL;DR line, rendered with emphasis (Obsidian callout / HTML box).
    Callout(String),
    Heading(String),
    Paragraph(String),
    Bullet(String),
    /// Unchecked task item.
    Todo(String),
    Quote(String),
    /// One transcript utterance with a bolded speaker/timestamp prefix.
    Utterance {
        speaker: String,
        stamp: String,
        text: String,
    },
}

/// The full note body: notes sections followed by the transcript.
pub fn note_blocks(notes: &MeetingNotes, segments: &[TranscriptSegment]) -> Vec<Block> {
    let mut blocks = Vec::new();
    blocks.push(Block::Callout(notes.tldr.clone()));

    blocks.push(Block::Heading("Summary".into()));
    for para in notes.summary.split("\n\n").filter(|p| !p.trim().is_empty()) {
        blocks.push(Block::Paragraph(para.trim().to_string()));
    }

    let bullet_section = |blocks: &mut Vec<Block>, title: &str, items: &[String]| {
        if !items.is_empty() {
            blocks.push(Block::Heading(title.into()));
            for item in items {
                blocks.push(Block::Bullet(item.clone()));
            }
        }
    };
    bullet_section(&mut blocks, "Key points", &notes.key_points);
    bullet_section(&mut blocks, "Decisions", &notes.decisions);

    if !notes.action_items.is_empty() {
        blocks.push(Block::Heading("Action items".into()));
        for item in &notes.action_items {
            let mut text = item.task.clone();
            if let Some(owner) = &item.owner {
                text.push_str(&format!(" — {owner}"));
            }
            if let Some(due) = &item.due {
                text.push_str(&format!(" (due {due})"));
            }
            blocks.push(Block::Todo(text));
        }
    }

    bullet_section(&mut blocks, "Open questions", &notes.open_questions);

    if !notes.highlights.is_empty() {
        blocks.push(Block::Heading("Highlights".into()));
        for h in &notes.highlights {
            blocks.push(Block::Quote(format!(
                "\u{201c}{}\u{201d} — {} ({})",
                h.quote,
                h.speaker,
                format_ms(h.timestamp_ms)
            )));
        }
    }

    if !segments.is_empty() {
        blocks.push(Block::Heading("Transcript".into()));
        for seg in segments {
            blocks.push(Block::Utterance {
                speaker: seg.speaker.clone(),
                stamp: format_ms(seg.start_ms),
                text: seg.text.clone(),
            });
        }
    }
    blocks
}

/// Render blocks as Markdown (no frontmatter — the Markdown destination
/// prepends that; Apple Notes converts these same blocks to HTML instead).
pub fn blocks_to_markdown(blocks: &[Block]) -> String {
    let mut out = String::new();
    for block in blocks {
        match block {
            Block::Callout(text) => {
                out.push_str("> [!summary] TL;DR\n");
                for line in text.lines() {
                    out.push_str("> ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
            Block::Heading(text) => {
                out.push_str("## ");
                out.push_str(text);
                out.push('\n');
            }
            Block::Paragraph(text) => {
                out.push_str(text);
                out.push('\n');
            }
            Block::Bullet(text) => {
                out.push_str("- ");
                out.push_str(text);
                out.push('\n');
            }
            Block::Todo(text) => {
                out.push_str("- [ ] ");
                out.push_str(text);
                out.push('\n');
            }
            Block::Quote(text) => {
                for line in text.lines() {
                    out.push_str("> ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
            Block::Utterance { speaker, stamp, text } => {
                out.push_str(&format!("**{speaker} ({stamp}):** {text}\n"));
            }
        }
        out.push('\n');
    }
    // Consecutive bullets/todos shouldn't be separated by blank lines.
    collapse_list_gaps(&out)
}

/// Remove the blank line between consecutive list items so they render as
/// one list; everything else keeps its paragraph spacing.
fn collapse_list_gaps(md: &str) -> String {
    let lines: Vec<&str> = md.lines().collect();
    let is_item = |l: &str| l.starts_with("- ");
    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        if line.is_empty()
            && i > 0
            && is_item(lines[i - 1])
            && lines.get(i + 1).is_some_and(|next| is_item(next))
        {
            continue;
        }
        out.push(line);
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// Distinct speaker labels in first-seen order, dropping the unlabeled
/// sentinel — the shared attendee list for destination frontmatter/properties.
pub fn distinct_speakers(segments: &[TranscriptSegment]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for seg in segments {
        if seg.speaker == crate::pipeline::UNLABELED_SPEAKER {
            continue;
        }
        let name = seg.speaker.trim();
        if !name.is_empty() && !seen.iter().any(|s| s == name) {
            seen.push(name.to_string());
        }
    }
    seen
}

/// `h:mm:ss` above an hour, `m:ss` below.
pub fn format_ms(ms: i64) -> String {
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
pub(crate) mod tests {
    use super::*;
    use crate::models::{Highlight, NoteActionItem};

    pub(crate) fn sample_notes() -> MeetingNotes {
        MeetingNotes {
            title: "Weekly planning".into(),
            tldr: "We agreed to ship v1 this month.".into(),
            summary: "First paragraph.\n\nSecond paragraph.".into(),
            key_points: vec!["Point one".into()],
            decisions: vec!["Ship it".into()],
            action_items: vec![NoteActionItem {
                task: "Write the release notes".into(),
                owner: Some("Maria".into()),
                due: Some("2026-07-15".into()),
            }],
            open_questions: vec!["Budget?".into()],
            highlights: vec![Highlight {
                quote: "This is the one".into(),
                speaker: "Tom".into(),
                timestamp_ms: 65_000,
            }],
        }
    }

    pub(crate) fn sample_segments() -> Vec<TranscriptSegment> {
        vec![
            TranscriptSegment {
                speaker: "Maria".into(),
                start_ms: 0,
                end_ms: 4_000,
                text: "Let's get started.".into(),
            },
            TranscriptSegment {
                speaker: "Tom".into(),
                start_ms: 4_000,
                end_ms: 9_000,
                text: "Agenda first.".into(),
            },
        ]
    }

    #[test]
    fn markdown_covers_all_sections() {
        let md = blocks_to_markdown(&note_blocks(&sample_notes(), &sample_segments()));
        assert!(md.contains("> [!summary] TL;DR\n> We agreed to ship v1 this month."));
        assert!(md.contains("## Summary\n"));
        assert!(md.contains("First paragraph.\n\nSecond paragraph."));
        assert!(md.contains("## Key points\n\n- Point one"));
        assert!(md.contains("- [ ] Write the release notes — Maria (due 2026-07-15)"));
        assert!(md.contains("> \u{201c}This is the one\u{201d} — Tom (1:05)"));
        assert!(md.contains("**Maria (0:00):** Let's get started."));
        assert!(md.contains("**Tom (0:04):** Agenda first."));
    }

    #[test]
    fn empty_sections_are_omitted() {
        let notes = MeetingNotes {
            key_points: vec![],
            decisions: vec![],
            action_items: vec![],
            open_questions: vec![],
            highlights: vec![],
            ..sample_notes()
        };
        let md = blocks_to_markdown(&note_blocks(&notes, &[]));
        assert!(!md.contains("## Key points"));
        assert!(!md.contains("## Action items"));
        assert!(!md.contains("## Transcript"));
    }

    #[test]
    fn consecutive_bullets_form_one_list() {
        let blocks = vec![
            Block::Bullet("one".into()),
            Block::Bullet("two".into()),
            Block::Heading("Next".into()),
        ];
        let md = blocks_to_markdown(&blocks);
        assert!(md.contains("- one\n- two\n\n## Next"));
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
            seg("Tom"),
            seg("Maria"),
        ];
        assert_eq!(distinct_speakers(&segments), vec!["Maria", "Tom"]);
    }

    #[test]
    fn format_ms_variants() {
        assert_eq!(format_ms(65_000), "1:05");
        assert_eq!(format_ms(3_725_000), "1:02:05");
    }
}
