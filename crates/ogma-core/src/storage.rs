//! SQLite persistence. One connection per `Storage`; callers wrap it in a
//! mutex (Tauri state, MCP server). The FTS5 index is a standalone table
//! rebuilt per meeting whenever its transcript is replaced — transcripts are
//! write-once-per-pipeline-run, so incremental sync isn't worth the pitfalls.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{Error, Result};
use crate::models::{ActionItem, Meeting, MeetingNotes, MeetingStatus, TranscriptSegment};

pub struct Storage {
    conn: Connection,
}

/// A transcript search hit.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchHit {
    pub meeting_id: String,
    pub meeting_title: String,
    pub speaker: String,
    pub start_ms: i64,
    pub snippet: String,
}

impl Storage {
    pub fn open(db_path: &Path) -> Result<Storage> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(db_path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let storage = Storage { conn };
        storage.migrate()?;
        Ok(storage)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Storage> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let storage = Storage { conn };
        storage.migrate()?;
        Ok(storage)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS meetings (
                id             TEXT PRIMARY KEY,
                title          TEXT NOT NULL,
                created_at     TEXT NOT NULL,
                duration_ms    INTEGER NOT NULL DEFAULT 0,
                status         TEXT NOT NULL,
                error          TEXT,
                audio_dir      TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS meeting_syncs (
                meeting_id   TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
                destination  TEXT NOT NULL,
                external_ref TEXT NOT NULL,
                synced_at    TEXT NOT NULL,
                PRIMARY KEY (meeting_id, destination)
            );

            CREATE TABLE IF NOT EXISTS transcript_segments (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
                idx        INTEGER NOT NULL,
                speaker    TEXT NOT NULL,
                start_ms   INTEGER NOT NULL,
                end_ms     INTEGER NOT NULL,
                text       TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_segments_meeting
                ON transcript_segments(meeting_id, idx);

            CREATE TABLE IF NOT EXISTS notes (
                meeting_id TEXT PRIMARY KEY REFERENCES meetings(id) ON DELETE CASCADE,
                json       TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS action_items (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
                task       TEXT NOT NULL,
                owner      TEXT,
                due        TEXT,
                status     TEXT NOT NULL DEFAULT 'open'
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS transcript_fts USING fts5(
                meeting_id UNINDEXED,
                seg_id UNINDEXED,
                text
            );
            "#,
        )?;
        self.migrate_notion_page_id()?;
        Ok(())
    }

    /// Legacy schema (pre-`meeting_syncs`) kept the Notion page id as a
    /// column on `meetings`. Move those values into `meeting_syncs` (as
    /// destination "notion") and drop the column. Fresh databases never had
    /// the column, so this is a no-op for them.
    fn migrate_notion_page_id(&self) -> Result<()> {
        let has_column = self
            .conn
            .prepare("SELECT 1 FROM pragma_table_info('meetings') WHERE name = 'notion_page_id'")?
            .exists([])?;
        if !has_column {
            return Ok(());
        }
        self.conn.execute_batch(
            r#"
            INSERT OR IGNORE INTO meeting_syncs (meeting_id, destination, external_ref, synced_at)
                SELECT id, 'notion', notion_page_id, created_at
                FROM meetings WHERE notion_page_id IS NOT NULL;
            ALTER TABLE meetings DROP COLUMN notion_page_id;
            "#,
        )?;
        Ok(())
    }

    // ── meetings ────────────────────────────────────────────────────────────

    /// `notion_page_id` on the passed meeting is ignored — it is derived
    /// from `meeting_syncs` on read and recorded via `record_sync`.
    pub fn create_meeting(&self, meeting: &Meeting) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meetings (id, title, created_at, duration_ms, status, error, audio_dir)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                meeting.id,
                meeting.title,
                meeting.created_at,
                meeting.duration_ms,
                meeting.status.as_str(),
                meeting.error,
                meeting.audio_dir,
            ],
        )?;
        Ok(())
    }

    pub fn list_meetings(&self) -> Result<Vec<Meeting>> {
        let mut stmt = self
            .conn
            .prepare(&format!("{MEETING_SELECT} ORDER BY created_at DESC"))?;
        let rows = stmt.query_map([], row_to_meeting)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn get_meeting(&self, id: &str) -> Result<Meeting> {
        self.conn
            .query_row(
                &format!("{MEETING_SELECT} WHERE id = ?1"),
                [id],
                row_to_meeting,
            )
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("meeting {id}")))
    }

    pub fn meetings_with_status(&self, status: MeetingStatus) -> Result<Vec<Meeting>> {
        let mut stmt = self.conn.prepare(&format!(
            "{MEETING_SELECT} WHERE status = ?1 ORDER BY created_at DESC"
        ))?;
        let rows = stmt.query_map([status.as_str()], row_to_meeting)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn set_status(&self, id: &str, status: MeetingStatus, error: Option<&str>) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE meetings SET status = ?2, error = ?3 WHERE id = ?1",
            params![id, status.as_str(), error],
        )?;
        if n == 0 {
            return Err(Error::NotFound(format!("meeting {id}")));
        }
        Ok(())
    }

    pub fn set_duration(&self, id: &str, duration_ms: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE meetings SET duration_ms = ?2 WHERE id = ?1",
            params![id, duration_ms],
        )?;
        Ok(())
    }

    pub fn set_title(&self, id: &str, title: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE meetings SET title = ?2 WHERE id = ?1",
            params![id, title],
        )?;
        Ok(())
    }

    // ── sync records ────────────────────────────────────────────────────────

    /// Record a completed sync to one destination (upsert: a forced re-sync
    /// refreshes the ref and timestamp).
    pub fn record_sync(&self, meeting_id: &str, destination: &str, external_ref: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meeting_syncs (meeting_id, destination, external_ref, synced_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(meeting_id, destination) DO UPDATE
                 SET external_ref = excluded.external_ref, synced_at = excluded.synced_at",
            params![
                meeting_id,
                destination,
                external_ref,
                chrono::Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    /// Destination ids that already have a sync record for this meeting —
    /// the pipeline syncs every enabled destination not in this list.
    pub fn synced_destinations(&self, meeting_id: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT destination FROM meeting_syncs WHERE meeting_id = ?1")?;
        let rows = stmt.query_map([meeting_id], |row| row.get(0))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn delete_meeting(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM transcript_fts WHERE meeting_id = ?1", [id])?;
        self.conn.execute("DELETE FROM meetings WHERE id = ?1", [id])?;
        Ok(())
    }

    // ── transcript ──────────────────────────────────────────────────────────

    pub fn replace_segments(&mut self, meeting_id: &str, segments: &[TranscriptSegment]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM transcript_segments WHERE meeting_id = ?1",
            [meeting_id],
        )?;
        tx.execute("DELETE FROM transcript_fts WHERE meeting_id = ?1", [meeting_id])?;
        {
            let mut ins = tx.prepare(
                "INSERT INTO transcript_segments (meeting_id, idx, speaker, start_ms, end_ms, text)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            let mut fts = tx.prepare(
                "INSERT INTO transcript_fts (meeting_id, seg_id, text) VALUES (?1, ?2, ?3)",
            )?;
            for (i, seg) in segments.iter().enumerate() {
                ins.execute(params![
                    meeting_id,
                    i as i64,
                    seg.speaker,
                    seg.start_ms,
                    seg.end_ms,
                    seg.text
                ])?;
                let seg_id = tx.last_insert_rowid();
                fts.execute(params![meeting_id, seg_id, seg.text])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get_segments(&self, meeting_id: &str) -> Result<Vec<TranscriptSegment>> {
        let mut stmt = self.conn.prepare(
            "SELECT speaker, start_ms, end_ms, text FROM transcript_segments
             WHERE meeting_id = ?1 ORDER BY idx",
        )?;
        let rows = stmt.query_map([meeting_id], |row| {
            Ok(TranscriptSegment {
                speaker: row.get(0)?,
                start_ms: row.get(1)?,
                end_ms: row.get(2)?,
                text: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Rename a speaker label everywhere it appears for one meeting:
    /// transcript segments, action-item owners, and any matching string
    /// inside the stored notes JSON (highlights carry speaker names).
    pub fn rename_speaker(&mut self, meeting_id: &str, from: &str, to: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE transcript_segments SET speaker = ?3 WHERE meeting_id = ?1 AND speaker = ?2",
            params![meeting_id, from, to],
        )?;
        tx.execute(
            "UPDATE action_items SET owner = ?3 WHERE meeting_id = ?1 AND owner = ?2",
            params![meeting_id, from, to],
        )?;
        let notes: Option<String> = tx
            .query_row(
                "SELECT json FROM notes WHERE meeting_id = ?1",
                [meeting_id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(raw) = notes {
            let mut value: serde_json::Value = serde_json::from_str(&raw)?;
            rename_in_json(&mut value, from, to);
            tx.execute(
                "UPDATE notes SET json = ?2 WHERE meeting_id = ?1",
                params![meeting_id, serde_json::to_string(&value)?],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn search_transcript(&self, query: &str, limit: u32) -> Result<Vec<SearchHit>> {
        // Quote each token so user input can't hit FTS5 syntax errors;
        // multiple tokens become an implicit AND.
        let sanitized: String = query
            .split_whitespace()
            .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" ");
        if sanitized.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT f.meeting_id, m.title, s.speaker, s.start_ms,
                    snippet(transcript_fts, 2, '[', ']', '…', 12)
             FROM transcript_fts f
             JOIN transcript_segments s ON s.id = f.seg_id
             JOIN meetings m ON m.id = f.meeting_id
             WHERE transcript_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![sanitized, limit], |row| {
            Ok(SearchHit {
                meeting_id: row.get(0)?,
                meeting_title: row.get(1)?,
                speaker: row.get(2)?,
                start_ms: row.get(3)?,
                snippet: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    // ── notes & action items ───────────────────────────────────────────────

    /// Store the notes JSON and mirror its action items into the
    /// `action_items` table (existing rows for the meeting are replaced,
    /// which resets per-item done/open status — acceptable, a pipeline rerun
    /// regenerates the items anyway).
    pub fn save_notes(&mut self, meeting_id: &str, notes: &MeetingNotes) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO notes (meeting_id, json) VALUES (?1, ?2)
             ON CONFLICT(meeting_id) DO UPDATE SET json = excluded.json",
            params![meeting_id, serde_json::to_string(notes)?],
        )?;
        tx.execute("DELETE FROM action_items WHERE meeting_id = ?1", [meeting_id])?;
        {
            let mut ins = tx.prepare(
                "INSERT INTO action_items (meeting_id, task, owner, due, status)
                 VALUES (?1, ?2, ?3, ?4, 'open')",
            )?;
            for item in &notes.action_items {
                ins.execute(params![meeting_id, item.task, item.owner, item.due])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get_notes(&self, meeting_id: &str) -> Result<Option<MeetingNotes>> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT json FROM notes WHERE meeting_id = ?1",
                [meeting_id],
                |r| r.get(0),
            )
            .optional()?;
        match raw {
            Some(json) => Ok(Some(serde_json::from_str(&json)?)),
            None => Ok(None),
        }
    }

    pub fn list_action_items(&self, status: Option<&str>) -> Result<Vec<ActionItem>> {
        let mut sql = String::from(
            "SELECT a.id, a.meeting_id, a.task, a.owner, a.due, a.status
             FROM action_items a JOIN meetings m ON m.id = a.meeting_id",
        );
        if status.is_some() {
            sql.push_str(" WHERE a.status = ?1");
        }
        sql.push_str(" ORDER BY m.created_at DESC, a.id");
        let mut stmt = self.conn.prepare(&sql)?;
        let map = |row: &rusqlite::Row| {
            Ok(ActionItem {
                id: row.get(0)?,
                meeting_id: row.get(1)?,
                task: row.get(2)?,
                owner: row.get(3)?,
                due: row.get(4)?,
                status: row.get(5)?,
            })
        };
        let rows = match status {
            Some(s) => stmt.query_map([s], map)?,
            None => stmt.query_map([], map)?,
        };
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn set_action_item_status(&self, id: i64, status: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE action_items SET status = ?2 WHERE id = ?1",
            params![id, status],
        )?;
        Ok(())
    }
}

/// Meeting rows carry a derived `notion_page_id` (the Notion sync record's
/// external ref) so the UI's "synced to Notion" link keeps working.
const MEETING_SELECT: &str = "SELECT id, title, created_at, duration_ms, status, error, audio_dir,
    (SELECT external_ref FROM meeting_syncs s
     WHERE s.meeting_id = meetings.id AND s.destination = 'notion')
 FROM meetings";

fn row_to_meeting(row: &rusqlite::Row) -> rusqlite::Result<Meeting> {
    let status: String = row.get(4)?;
    Ok(Meeting {
        id: row.get(0)?,
        title: row.get(1)?,
        created_at: row.get(2)?,
        duration_ms: row.get(3)?,
        status: MeetingStatus::parse(&status),
        error: row.get(5)?,
        audio_dir: row.get(6)?,
        notion_page_id: row.get(7)?,
    })
}

/// Recursively replace string values equal to `from` with `to`. Generic over
/// the notes shape so renames survive schema drift in provider output.
fn rename_in_json(value: &mut serde_json::Value, from: &str, to: &str) {
    match value {
        serde_json::Value::String(s) if s == from => *s = to.to_string(),
        serde_json::Value::Array(items) => {
            for v in items {
                rename_in_json(v, from, to);
            }
        }
        serde_json::Value::Object(map) => {
            for (_, v) in map.iter_mut() {
                rename_in_json(v, from, to);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Highlight, NoteActionItem};

    fn meeting(id: &str) -> Meeting {
        Meeting {
            id: id.into(),
            title: format!("Meeting {id}"),
            created_at: "2026-07-09T10:00:00Z".into(),
            duration_ms: 0,
            status: MeetingStatus::Recorded,
            error: None,
            audio_dir: format!("C:/data/{id}"),
            notion_page_id: None,
        }
    }

    fn seg(speaker: &str, start: i64, text: &str) -> TranscriptSegment {
        TranscriptSegment {
            speaker: speaker.into(),
            start_ms: start,
            end_ms: start + 1000,
            text: text.into(),
        }
    }

    #[test]
    fn meeting_crud_and_status() {
        let s = Storage::open_in_memory().unwrap();
        s.create_meeting(&meeting("m1")).unwrap();
        s.set_status("m1", MeetingStatus::Transcribing, None).unwrap();
        assert_eq!(s.get_meeting("m1").unwrap().status, MeetingStatus::Transcribing);
        s.set_status("m1", MeetingStatus::Error, Some("boom")).unwrap();
        let m = s.get_meeting("m1").unwrap();
        assert_eq!(m.error.as_deref(), Some("boom"));
        assert!(s.set_status("nope", MeetingStatus::Done, None).is_err());
    }

    #[test]
    fn fts_search_and_rename() {
        let mut s = Storage::open_in_memory().unwrap();
        s.create_meeting(&meeting("m1")).unwrap();
        s.replace_segments(
            "m1",
            &[
                seg("Speaker A", 0, "we should migrate the billing system"),
                seg("Speaker B", 1000, "agreed, billing is fragile"),
            ],
        )
        .unwrap();

        let hits = s.search_transcript("billing", 10).unwrap();
        assert_eq!(hits.len(), 2);
        // FTS5 syntax chars in user input must not error
        assert!(s.search_transcript("billing AND (", 10).is_ok());

        s.save_notes(
            "m1",
            &MeetingNotes {
                title: "Billing".into(),
                tldr: "t".into(),
                summary: "s".into(),
                key_points: vec![],
                decisions: vec![],
                action_items: vec![NoteActionItem {
                    task: "fix billing".into(),
                    owner: Some("Speaker A".into()),
                    due: None,
                }],
                open_questions: vec![],
                highlights: vec![Highlight {
                    quote: "billing is fragile".into(),
                    speaker: "Speaker B".into(),
                    timestamp_ms: 1000,
                }],
            },
        )
        .unwrap();

        s.rename_speaker("m1", "Speaker A", "Maria").unwrap();
        let segs = s.get_segments("m1").unwrap();
        assert_eq!(segs[0].speaker, "Maria");
        let notes = s.get_notes("m1").unwrap().unwrap();
        assert_eq!(notes.action_items[0].owner.as_deref(), Some("Maria"));
        let items = s.list_action_items(Some("open")).unwrap();
        assert_eq!(items[0].owner.as_deref(), Some("Maria"));
    }

    #[test]
    fn delete_cascades() {
        let mut s = Storage::open_in_memory().unwrap();
        s.create_meeting(&meeting("m1")).unwrap();
        s.replace_segments("m1", &[seg("A", 0, "hello world")]).unwrap();
        s.record_sync("m1", "markdown", "/vault/m1.md").unwrap();
        s.delete_meeting("m1").unwrap();
        assert!(s.get_meeting("m1").is_err());
        assert!(s.get_segments("m1").unwrap().is_empty());
        assert!(s.search_transcript("hello", 10).unwrap().is_empty());
        assert!(s.synced_destinations("m1").unwrap().is_empty());
    }

    #[test]
    fn sync_records_round_trip_and_derive_notion_page_id() {
        let s = Storage::open_in_memory().unwrap();
        s.create_meeting(&meeting("m1")).unwrap();
        assert!(s.get_meeting("m1").unwrap().notion_page_id.is_none());

        s.record_sync("m1", "markdown", "/vault/m1.md").unwrap();
        s.record_sync("m1", "notion", "page-123").unwrap();
        let mut synced = s.synced_destinations("m1").unwrap();
        synced.sort();
        assert_eq!(synced, vec!["markdown", "notion"]);

        // The Notion record surfaces as the meeting's derived notion_page_id.
        assert_eq!(
            s.get_meeting("m1").unwrap().notion_page_id.as_deref(),
            Some("page-123")
        );
        assert_eq!(
            s.list_meetings().unwrap()[0].notion_page_id.as_deref(),
            Some("page-123")
        );

        // Upsert: re-recording the same destination replaces the ref.
        s.record_sync("m1", "notion", "page-456").unwrap();
        assert_eq!(
            s.get_meeting("m1").unwrap().notion_page_id.as_deref(),
            Some("page-456")
        );
    }

    /// A database created by a pre-`meeting_syncs` Ogma carries the Notion
    /// page id as a `meetings` column; opening it must move those values into
    /// `meeting_syncs` and drop the column, losing nothing.
    #[test]
    fn migrates_legacy_notion_page_id_column() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("ogma.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                r#"
                CREATE TABLE meetings (
                    id             TEXT PRIMARY KEY,
                    title          TEXT NOT NULL,
                    created_at     TEXT NOT NULL,
                    duration_ms    INTEGER NOT NULL DEFAULT 0,
                    status         TEXT NOT NULL,
                    error          TEXT,
                    audio_dir      TEXT NOT NULL,
                    notion_page_id TEXT
                );
                INSERT INTO meetings VALUES
                    ('m1', 'Synced', '2026-07-01T10:00:00Z', 0, 'done', NULL, '/d/m1', 'page-legacy'),
                    ('m2', 'Unsynced', '2026-07-02T10:00:00Z', 0, 'done', NULL, '/d/m2', NULL);
                "#,
            )
            .unwrap();
        }

        let s = Storage::open(&db_path).unwrap();
        assert_eq!(
            s.get_meeting("m1").unwrap().notion_page_id.as_deref(),
            Some("page-legacy")
        );
        assert_eq!(s.synced_destinations("m1").unwrap(), vec!["notion"]);
        assert!(s.get_meeting("m2").unwrap().notion_page_id.is_none());
        assert!(s.synced_destinations("m2").unwrap().is_empty());

        // Reopening (migration already applied) must be a clean no-op.
        drop(s);
        let s = Storage::open(&db_path).unwrap();
        assert_eq!(s.synced_destinations("m1").unwrap(), vec!["notion"]);
    }
}
