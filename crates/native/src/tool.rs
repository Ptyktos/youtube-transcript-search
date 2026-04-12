use reqwest::Client;
use rmcp::{
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    tool, ServerHandler,
};
use std::sync::Arc;

use crate::fetch::get_transcript;

#[derive(Debug, Clone)]
pub struct TranscriptServer {
    client: Arc<Client>,
}

impl TranscriptServer {
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self {
            client: Arc::new(client),
        }
    }
}

#[tool(tool_box)]
impl TranscriptServer {
    /// Extract the full transcript from a `YouTube` video.
    #[tool(description = "Extract the full transcript from a YouTube video URL")]
    async fn get_transcript(
        &self,
        #[tool(param)]
        #[schemars(
            description = "YouTube video URL (any format: watch, youtu.be, shorts, live, embed)"
        )]
        url: String,
        #[tool(param)]
        #[schemars(
            description = "Language code (e.g. 'en', 'es', 'fr'). Omit or use 'auto' for automatic detection."
        )]
        language: Option<String>,
    ) -> Result<CallToolResult, rmcp::Error> {
        let lang = language.as_deref().unwrap_or("auto");
        get_transcript(&self.client, &url, lang)
            .await
            .map(|r| CallToolResult::success(vec![Content::text(r.text)]))
            .map_err(|e| rmcp::Error::invalid_params(e.to_string(), None))
    }
}

#[tool(tool_box)]
impl ServerHandler for TranscriptServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: rmcp::model::Implementation {
                name: "youtube-transcript-mcp".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            ..Default::default()
        }
    }
}
