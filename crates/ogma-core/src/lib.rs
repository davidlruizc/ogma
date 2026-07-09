//! Ogma core: recording, storage, AI pipeline, Notion sync, MCP server.
//! Shared by the Tauri app and the `ogma --mcp` stdio mode.

pub mod config;
pub mod error;
pub mod mcp;
pub mod models;
pub mod notion;
pub mod pipeline;
pub mod providers;
pub mod recording;
pub mod storage;

pub use config::Config;
pub use error::{Error, Result};
