# Spike — more note-taking destinations beyond Notion (backlog #1)

Research spike (July 2026) for generalizing the Notion sync into pluggable
destinations. Scope: API feasibility per candidate app, the `SyncDestination`
trait shape, and a suggested build order. No code changes yet.

**Decision (July 2026):** build **Markdown/Obsidian** and **Apple Notes
(macOS only)**. **Google Docs and OneNote are skipped** — both require
registering the app in Google Cloud / Azure (Entra ID), and we don't plan to
create or maintain those developer accounts for this project. The
`SyncDestination` trait leaves the door open if that changes; the survey and
OAuth notes below are kept for that eventuality.

## What exists today

`notion.rs` already has the right surface for a trait: the pipeline calls one
method, `create_meeting_page(&Meeting, &MeetingNotes, &[TranscriptSegment]) ->
page_id`, and stores the returned id in `meetings.notion_page_id`. The resume
rule is "notes exist + no notion page id + Notion configured → sync". All the
Notion-specific work (block rendering, 100-block/2000-char batching, schema
backfill) is private to the module. Generalizing this is mostly mechanical.

## Candidate survey

| Destination | API | Auth | Effort | Verdict |
|---|---|---|---|---|
| **Markdown file / Obsidian** | none — write `.md` into a user-chosen folder (an Obsidian vault is just a folder) | none | S | **Do first** |
| **Apple Notes** | no public API; AppleScript via `osascript` can create a note with an HTML body | none (Automation permission prompt) | S | **Do second** — macOS only |
| **Google Docs** | Docs REST API (`documents.create` + `batchUpdate`) | OAuth 2.0 desktop (loopback) — `drive.file` scope is non-sensitive, so only basic app verification | M | **Skipped** — needs a Google Cloud account we don't plan to maintain |
| **OneNote** | Microsoft Graph (`POST /me/onenote/pages`, body is XHTML) | OAuth 2.0 delegated (app-only auth was retired March 2025) | M | **Skipped** — needs an Azure (Entra ID) app registration we don't plan to maintain |
| **Joplin** | local REST API (Web Clipper service, `localhost:41184`, token auth), markdown body | local token | S | Cheap, niche |
| **Evernote** | Thrift-based API, OAuth 1.0; API keys granted only by manual review ("proven necessity") | OAuth 1.0 | L | **Reject** — access is effectively closed and the API is stagnant |

Notes on the interesting ones:

- **Markdown/Obsidian** is the highest value-per-effort: no network, no auth,
  no rate limits; it also covers Logseq and "just give me my notes as files",
  and it is the natural first step toward backlog #3 (Notion optional). The
  destination is a directory path in settings; the renderer is a pure
  `MeetingNotes + segments → String` function with YAML frontmatter (title,
  date, attendees) so Obsidian's Dataview/properties pick it up.
- **Apple Notes** has no API at all; `osascript -e 'tell application "Notes"
  to make new note … with properties {body: <html>}'` works, gated by a
  one-time macOS Automation permission prompt. **iOS caveat:** AppleScript
  doesn't exist on iOS and Notes has no public API there either, so this
  destination is macOS-only by nature. iCloud softens it: notes Ogma creates
  on the Mac sync to iPhone automatically — but a future iOS-recorded meeting
  can't write to Apple Notes from the phone.
- **Google Docs** (skipped) with the `drive.file` scope would avoid the
  sensitive-scope verification process — the app only touches documents it
  created. The real cost is OAuth infrastructure (below) plus a Google Cloud
  project registration owned by the developer's personal account.
- **OneNote** (skipped) would reuse the same OAuth infrastructure with
  MSAL-style endpoints; page creation is a single multipart XHTML POST —
  simpler rendering than Notion blocks. Requires an Entra ID app
  registration.

## The real shared cost: OAuth (reference only — moot while Google/OneNote are skipped)

Notion uses a static integration token; Google and Microsoft both require a
real OAuth 2.0 authorization-code flow with PKCE, a loopback redirect
(`http://127.0.0.1:<port>/callback`), refresh-token storage in the OS keyring
(the `keyring` plumbing in `config.rs` already exists), and silent refresh.
That's one `oauth.rs` helper built once for Google and reused for Microsoft —
it is the bulk of the work for those two destinations, not the per-API code.

## Proposed design

```rust
/// One sync target. Implementations render MeetingNotes + transcript into
/// their native format. Mirrors TranscriptionProvider/NotesProvider.
#[async_trait]
pub trait SyncDestination: Send + Sync {
    /// Stable id stored in the DB, e.g. "notion", "markdown", "gdocs".
    fn id(&self) -> &'static str;
    /// Idempotent per meeting: called only when no sync record exists.
    /// Returns an external ref (page id, file path, doc URL) for the record.
    async fn sync(
        &self,
        meeting: &Meeting,
        notes: &MeetingNotes,
        segments: &[TranscriptSegment],
    ) -> Result<String>;
}
```

- **Storage:** replace the single `meetings.notion_page_id` column with a
  `meeting_syncs` table (`meeting_id`, `destination`, `external_ref`,
  `synced_at`). Migration copies existing `notion_page_id` values into it as
  `destination = "notion"`. The pipeline resume rule generalizes cleanly:
  *for each enabled destination with no sync record → sync it* — same
  derive-from-stored-data philosophy, now per destination, and one failing
  destination no longer blocks the others.
- **Rendering:** keep renderers per destination (Notion blocks, Markdown,
  XHTML, HTML). A shared Markdown renderer covers the file/Obsidian/Joplin
  destinations and can seed the OneNote/Apple Notes HTML via a trivial
  markdown→HTML pass; don't force a common intermediate format on Notion,
  whose block model is the odd one out and already works.
- **Config/UI:** settings gains a "Destinations" section with per-destination
  enable + credentials (folder picker for Markdown, Connect button for
  OAuth ones). `notion_configured()` becomes `enabled_destinations()`.

**Locked-in-decision check:** "Notion is the canonical cross-device store"
stays true — additional destinations are additive fan-out, not a new
canonical home. That only changes if/when backlog #3 lands.

## Action items (decided order)

1. **Trait + `meeting_syncs` migration + Markdown/Obsidian destination** — one
   PR, no new dependencies, immediately useful, and de-risks the refactor
   because Notion just moves behind the trait unchanged.
2. **Apple Notes destination (macOS only, via `osascript`)** — reuses the
   shared markdown renderer through a markdown→HTML pass; compiled/enabled
   only on macOS.

Not planned: Google Docs and OneNote (no Google Cloud/Azure accounts for this
project — revisit only if that stance changes), Joplin (niche, wait for
demand), Evernote (rejected outright).

## Sources

- Evernote API access: [dev.evernote.com FAQ](https://dev.evernote.com/support/faq.php),
  [developer tokens](https://dev.evernote.com/doc/articles/dev_tokens.php)
- OneNote: [create pages via Graph](https://learn.microsoft.com/en-us/graph/onenote-create-page),
  [Graph OneNote API overview](https://learn.microsoft.com/en-us/graph/api/resources/onenote-api-overview?view=graph-rest-1.0)
- Google Docs scopes: [docs API auth](https://developers.google.com/workspace/docs/api/auth),
  [drive.file guidance](https://developers.google.com/workspace/drive/api/guides/api-specific-auth)
