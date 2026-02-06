//! MCP (Model Context Protocol) server for AI assistant integration
//!
//! This module implements an MCP server that exposes Yolog's memory and session
//! tools to AI assistants like Claude Code.
//!
//! # Usage
//!
//! Run yocore with the `--mcp` flag to start in MCP server mode:
//! ```text
//! yocore --mcp
//! ```
//!
//! The server communicates over stdio using JSON-RPC 2.0.

pub(crate) mod db;
mod handlers;
mod protocol;
pub(crate) mod types;

use crate::error::Result;
use crate::Core;
use db::McpDb;
use protocol::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};
use std::io::{BufRead, Write};

/// Run the MCP server over stdio
pub async fn run_mcp_server(core: Core) -> Result<()> {
    let mcp_db = McpDb::new(core.db.clone());

    tracing::info!("Starting MCP server (stdio mode)");

    // Run the blocking stdio loop in a separate thread
    let result = tokio::task::spawn_blocking(move || run_stdio_loop(&mcp_db)).await;

    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => {
            tracing::error!("MCP server error: {}", e);
            Err(crate::error::CoreError::Api(e))
        }
        Err(e) => {
            tracing::error!("MCP server task panicked: {}", e);
            Err(crate::error::CoreError::Api(e.to_string()))
        }
    }
}

/// Main stdio event loop - reads JSON-RPC requests from stdin, writes responses to stdout
fn run_stdio_loop(db: &McpDb) -> std::result::Result<(), String> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    for line in stdin.lock().lines() {
        let line = line.map_err(|e| format!("Failed to read line: {}", e))?;

        if line.trim().is_empty() {
            continue;
        }

        // Parse JSON-RPC request
        let response = match serde_json::from_str::<JsonRpcRequest>(&line) {
            Ok(request) => handle_request(request, db),
            Err(e) => JsonRpcResponse::error(
                serde_json::Value::Null,
                JsonRpcError::parse_error(format!("Invalid JSON: {}", e)),
            ),
        };

        // Write response to stdout
        let response_json =
            serde_json::to_string(&response).map_err(|e| format!("Failed to serialize: {}", e))?;

        writeln!(stdout, "{}", response_json).map_err(|e| format!("Failed to write: {}", e))?;

        stdout
            .flush()
            .map_err(|e| format!("Failed to flush: {}", e))?;
    }

    Ok(())
}

/// Handle a single JSON-RPC request
fn handle_request(request: JsonRpcRequest, db: &McpDb) -> JsonRpcResponse {
    match request.method.as_str() {
        // MCP protocol methods
        "initialize" => handlers::handle_initialize(request.id),
        "initialized" => JsonRpcResponse::success(request.id, serde_json::json!({})),
        "tools/list" => handlers::handle_tools_list(request.id),
        "tools/call" => handlers::handle_tools_call(request.id, request.params, db),
        "resources/list" => handlers::handle_resources_list(request.id),
        "ping" => JsonRpcResponse::success(request.id, serde_json::json!({})),

        // Unknown method
        _ => JsonRpcResponse::error(
            request.id,
            JsonRpcError::method_not_found(format!("Unknown method: {}", request.method)),
        ),
    }
}
