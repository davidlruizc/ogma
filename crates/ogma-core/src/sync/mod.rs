//! Pluggable sync destinations (see docs/sync-destinations-spike.md).
//!
//! One meeting fans out to every enabled destination after notes exist. The
//! pipeline derives what is pending from the `meeting_syncs` table — for each
//! enabled destination with no sync record, sync it — so a failing
//! destination never blocks the others and retries only redo what's missing.

#[cfg(target_os = "macos")]
pub mod apple_notes;
pub mod markdown;
pub mod render;

use crate::config::Config;
use crate::error::Result;
use crate::models::{Meeting, MeetingNotes, TranscriptSegment};
use crate::notion::NotionClient;

/// One sync target. Implementations render MeetingNotes + transcript into
/// their native format. Mirrors TranscriptionProvider/NotesProvider.
#[async_trait::async_trait]
pub trait SyncDestination: Send + Sync {
    /// Stable id stored in `meeting_syncs.destination`, e.g. "notion".
    fn id(&self) -> &'static str;

    /// Human-readable name for progress messages.
    fn display_name(&self) -> &'static str;

    /// Idempotent per meeting: called only when no sync record exists.
    /// Returns an external ref (page id, file path) stored in the record.
    async fn sync(
        &self,
        meeting: &Meeting,
        notes: &MeetingNotes,
        segments: &[TranscriptSegment],
    ) -> Result<String>;
}

/// The destinations the current config turns on, in sync order. Notion stays
/// first: it is the canonical cross-device store, the others are fan-out.
pub fn enabled_destinations(config: &Config) -> Vec<Box<dyn SyncDestination>> {
    let mut out: Vec<Box<dyn SyncDestination>> = Vec::new();
    if !config.notion_api_key.is_empty() && !config.notion_database_id.is_empty() {
        out.push(Box::new(NotionClient::new(
            config.notion_api_key.clone(),
            config.notion_database_id.clone(),
        )));
    }
    if !config.markdown_dir.trim().is_empty() {
        out.push(Box::new(markdown::MarkdownDestination::new(
            config.markdown_dir.trim(),
        )));
    }
    #[cfg(target_os = "macos")]
    if config.apple_notes_enabled {
        out.push(Box::new(apple_notes::AppleNotesDestination));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabled_destinations_follow_config() {
        let mut config = Config::default();
        assert!(enabled_destinations(&config).is_empty());

        config.markdown_dir = "/vault/meetings".into();
        let ids: Vec<&str> = enabled_destinations(&config).iter().map(|d| d.id()).collect();
        assert_eq!(ids, vec!["markdown"]);

        config.notion_api_key = "secret".into();
        config.notion_database_id = "db".into();
        let ids: Vec<&str> = enabled_destinations(&config).iter().map(|d| d.id()).collect();
        assert_eq!(ids, vec!["notion", "markdown"]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn apple_notes_follows_toggle() {
        let config = Config {
            apple_notes_enabled: true,
            ..Config::default()
        };
        let ids: Vec<&str> = enabled_destinations(&config).iter().map(|d| d.id()).collect();
        assert_eq!(ids, vec!["apple_notes"]);
    }

    #[test]
    fn notion_needs_both_key_and_database() {
        let config = Config {
            notion_api_key: "secret".into(),
            ..Config::default()
        };
        assert!(enabled_destinations(&config).is_empty());
    }
}
