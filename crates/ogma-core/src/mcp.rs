//! MCP server mode (`ogma --mcp`): stdio transport exposing the local
//! meeting library to AI clients (Claude Code, claude.ai).

use std::sync::{Arc, Mutex};

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::storage::Storage;

#[derive(Clone)]
pub struct OgmaMcpServer {
    storage: Arc<Mutex<Storage>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListMeetingsParams {
    /// Optional case-insensitive filter on the meeting title.
    pub query: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchTranscriptParams {
    /// Full-text query over all meeting transcripts.
    pub query: String,
    /// Maximum hits to return (default 20).
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MeetingIdParams {
    /// Meeting id (from list_meetings or search_transcript).
    pub meeting_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ActionItemsParams {
    /// Filter by status: "open" or "done". Omit for all.
    pub status: Option<String>,
}

fn json_result<T: serde::Serialize>(value: &T) -> Result<CallToolResult, McpError> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
}

fn storage_err(e: crate::error::Error) -> McpError {
    McpError::internal_error(e.to_string(), None)
}

#[tool_router]
impl OgmaMcpServer {
    pub fn new(storage: Arc<Mutex<Storage>>) -> Self {
        Self { storage }
    }

    #[tool(description = "List recorded meetings (id, title, date, duration, status), newest first. Optional title filter.")]
    async fn list_meetings(
        &self,
        Parameters(params): Parameters<ListMeetingsParams>,
    ) -> Result<CallToolResult, McpError> {
        let storage = self.storage.lock().unwrap();
        let mut meetings = storage.list_meetings().map_err(storage_err)?;
        if let Some(q) = params.query.filter(|q| !q.is_empty()) {
            let q = q.to_lowercase();
            meetings.retain(|m| m.title.to_lowercase().contains(&q));
        }
        json_result(&meetings)
    }

    #[tool(description = "Full-text search across all meeting transcripts. Returns matching utterances with meeting id/title, speaker and timestamp.")]
    async fn search_transcript(
        &self,
        Parameters(params): Parameters<SearchTranscriptParams>,
    ) -> Result<CallToolResult, McpError> {
        let storage = self.storage.lock().unwrap();
        let hits = storage
            .search_transcript(&params.query, params.limit.unwrap_or(20))
            .map_err(storage_err)?;
        json_result(&hits)
    }

    #[tool(description = "Get the AI-generated notes for a meeting: tldr, summary, key points, decisions, action items, open questions, highlights.")]
    async fn get_meeting_notes(
        &self,
        Parameters(params): Parameters<MeetingIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let storage = self.storage.lock().unwrap();
        let meeting = storage.get_meeting(&params.meeting_id).map_err(storage_err)?;
        let notes = storage.get_notes(&params.meeting_id).map_err(storage_err)?;
        json_result(&serde_json::json!({
            "meeting": meeting,
            "notes": notes,
        }))
    }

    #[tool(description = "Get the full speaker-labeled transcript of a meeting.")]
    async fn get_transcript(
        &self,
        Parameters(params): Parameters<MeetingIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let storage = self.storage.lock().unwrap();
        let segments = storage.get_segments(&params.meeting_id).map_err(storage_err)?;
        json_result(&segments)
    }

    #[tool(description = "List action items across all meetings, optionally filtered by status ('open' or 'done').")]
    async fn get_action_items(
        &self,
        Parameters(params): Parameters<ActionItemsParams>,
    ) -> Result<CallToolResult, McpError> {
        let storage = self.storage.lock().unwrap();
        let items = storage
            .list_action_items(params.status.as_deref())
            .map_err(storage_err)?;
        json_result(&items)
    }
}

#[tool_handler]
impl ServerHandler for OgmaMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "Ogma meeting library: recorded in-person meetings with transcripts and \
                 AI notes. Use search_transcript for content questions, get_action_items \
                 for todos, get_meeting_notes for summaries.",
            )
    }
}

/// Run the stdio MCP server until the client disconnects.
pub async fn serve(storage: Arc<Mutex<Storage>>) -> crate::error::Result<()> {
    let service = OgmaMcpServer::new(storage)
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| crate::error::Error::Other(format!("mcp serve: {e}")))?;
    service
        .waiting()
        .await
        .map_err(|e| crate::error::Error::Other(format!("mcp wait: {e}")))?;
    Ok(())
}
