//! since-until-mcp — one Model Context Protocol server, three tools (`since`,
//! `until`, `list_anchors`), over stdio via `rmcp`. Mirrors the wiring shape of
//! clock-mcp.
//!
//! This is a thin shell, exactly like the CLI binaries: every bit of real work
//! — token resolution, calendar math, the anchor store — lives in the library.
//! The server just adapts the engine's results into structured MCP responses,
//! and it reads the *same* `anchors.json` the CLIs write (same `default_path`),
//! so anchors added via `since`/`until` are visible here with no extra wiring.

use std::collections::BTreeMap;

use anyhow::Result;
use chrono::Local;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
    },
    tool, tool_handler, tool_router,
    transport::stdio,
    ErrorData as McpError, ServerHandler, ServiceExt,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use since_until::cli::{Framing, ALREADY_PASSED_NOTE};
use since_until::{measure, AnchorStore, Direction};

/// A token to resolve: either an ISO date or an anchor nickname.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[schemars(description = "Parameters for the `since` and `until` tools.")]
pub struct TokenRequest {
    /// An ISO date (`YYYY-MM-DD`) OR a saved anchor nickname (e.g. "covid").
    /// Use `list_anchors` to see which nicknames exist.
    pub token: String,
}

/// The structured result of a `since` / `until` measurement. The model gets
/// both the usable numbers and the ready-made sentence.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SpanResult {
    /// The token exactly as supplied.
    pub token: String,
    /// The date it resolved to, ISO `YYYY-MM-DD`.
    pub date: String,
    /// Whole years of difference (always non-negative; sign lives in `direction`).
    pub years: i64,
    /// Whole months past the years.
    pub months: i64,
    /// Whole days past the months.
    pub days: i64,
    /// "past", "future", or "today".
    pub direction: String,
    /// The humanized sentence, e.g. "6 years, 2 months, 30 days ago".
    pub humanized: String,
    /// Present only on the `until` tool when the date has already passed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The current anchors: nickname -> ISO date string.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AnchorsResult {
    /// Sorted map of nickname to ISO date.
    pub anchors: BTreeMap<String, String>,
    /// How many anchors are defined.
    pub count: usize,
}

#[derive(Clone)]
pub struct SinceUntilServer {
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl Default for SinceUntilServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router]
impl SinceUntilServer {
    pub fn new() -> Self {
        Self { tool_router: Self::tool_router() }
    }

    #[tool(
        description = "Measure the signed time from a date to today, past-leaning. \
            `token` is either an ISO date (YYYY-MM-DD) or a saved anchor nickname \
            (see list_anchors). Returns years/months/days, direction \
            (past/future/today), and a humanized string like \"6 years, 2 months ago\"."
    )]
    async fn since(
        &self,
        Parameters(req): Parameters<TokenRequest>,
    ) -> Result<CallToolResult, McpError> {
        respond(measure_token(&req.token, Framing::Since))
    }

    #[tool(
        description = "Measure the signed time from today to a date, future-leaning. \
            `token` is either an ISO date (YYYY-MM-DD) or a saved anchor nickname \
            (see list_anchors). Returns years/months/days, direction, and a humanized \
            string like \"in 6 months, 24 days\". If the date has already passed, the \
            result carries a `note` saying so."
    )]
    async fn until(
        &self,
        Parameters(req): Parameters<TokenRequest>,
    ) -> Result<CallToolResult, McpError> {
        respond(measure_token(&req.token, Framing::Until))
    }

    #[tool(
        description = "List all saved anchors: the nickname -> ISO date map that the \
            `since` and `until` tokens can refer to by name. Shared with the `since` \
            and `until` command-line tools."
    )]
    async fn list_anchors(&self) -> Result<CallToolResult, McpError> {
        respond(load_anchors())
    }
}

#[tool_handler]
impl ServerHandler for SinceUntilServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::V_2025_06_18)
            // Override rmcp's default identity (which reports "rmcp", since its
            // from_build_env reads rmcp's own crate name). This names us
            // distinctly in a client listing, and the version tracks our crate.
            .with_server_info(Implementation::new(
                "since-until-mcp",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "Honest, calendar-aware date differences with user-defined named anchors.\n\n\
                 Tools:\n\
                 - since(token): time from a date to today (past-leaning)\n\
                 - until(token): time from today to a date (future-leaning)\n\
                 - list_anchors(): the nickname -> ISO date map\n\n\
                 A token is an ISO date (YYYY-MM-DD) or an anchor nickname (e.g. \"covid\"). \
                 Anchors are shared with the `since`/`until` CLIs via ~/.config/since-until/anchors.json.",
            )
    }
}

/// Resolve + measure a token, projecting the engine's `Span` into the structured
/// MCP result and attaching the gentle note for past dates under `until`.
fn measure_token(token: &str, framing: Framing) -> Result<SpanResult, String> {
    let store = open_store()?;
    let now = Local::now().date_naive();
    let (date, span) = measure(token, &store, now).map_err(|e| e.to_string())?;
    let note = if framing == Framing::Until && span.direction == Direction::Past {
        Some(ALREADY_PASSED_NOTE.to_string())
    } else {
        None
    };
    Ok(SpanResult {
        token: token.to_string(),
        date: date.to_string(),
        years: span.years,
        months: span.months,
        days: span.days,
        direction: span.direction.as_str().to_string(),
        humanized: span.humanized,
        note,
    })
}

/// Read the shared anchor store as a plain nickname -> ISO map.
fn load_anchors() -> Result<AnchorsResult, String> {
    let store = open_store()?;
    let anchors: BTreeMap<String, String> = store
        .list()
        .iter()
        .map(|(name, date)| (name.clone(), date.to_string()))
        .collect();
    let count = anchors.len();
    Ok(AnchorsResult { anchors, count })
}

/// Load the same anchors.json the CLIs use (missing file = empty set).
fn open_store() -> Result<AnchorStore, String> {
    let path = AnchorStore::default_path().map_err(|e| e.to_string())?;
    AnchorStore::load_from(path).map_err(|e| e.to_string())
}

/// Adapt a `Result<T, String>` into an MCP tool result: success carries the
/// struct as JSON content; failure carries `{ "error": ... }` as a tool error.
fn respond<T: Serialize>(result: Result<T, String>) -> Result<CallToolResult, McpError> {
    match result {
        Ok(value) => {
            let content = Content::json(value)
                .map_err(|e| McpError::internal_error(format!("serialize: {e}"), None))?;
            Ok(CallToolResult::success(vec![content]))
        }
        Err(msg) => {
            let content = Content::json(serde_json::json!({ "error": msg }))
                .map_err(|e| McpError::internal_error(format!("serialize: {e}"), None))?;
            Ok(CallToolResult::error(vec![content]))
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Log to stderr — stdout is the MCP channel and must stay clean.
    eprintln!("since-until-mcp {} starting", env!("CARGO_PKG_VERSION"));

    let service = SinceUntilServer::new()
        .serve(stdio())
        .await
        .inspect_err(|e| eprintln!("failed to start stdio server: {e:?}"))?;

    service.waiting().await?;
    Ok(())
}
