//! Simple MCP test server for testing.
//! Implements basic JSON-RPC endpoints for tool discovery and tool execution.

use axum::{
    extract::MatchedPath,
    middleware::{self, Next},
    response::Response,
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::info;

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: String,
    method: String,
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().init();

    let app = Router::new()
        .route("/", post(handle_rpc))
        .layer(middleware::from_fn(track_metrics));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3031")
        .await
        .unwrap();

    println!("[+] MCP Test Server listening on http://127.0.0.1:3031");

    axum::serve(listener, app).await.unwrap();
}

async fn track_metrics(
    _matched_path: Option<MatchedPath>,
    req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Response {
    let method = req.method().clone();
    let response = next.run(req).await;
    tracing::debug!("{} - {}", method, response.status());
    response
}

async fn handle_rpc(Json(req): Json<JsonRpcRequest>) -> Json<JsonRpcResponse> {
    info!("MCP RPC: {} (id: {})", req.method, req.id);

    let (result, error) = match req.method.as_str() {
        "tools/list" => {
            let tools = json!([
                {
                    "name": "read_file",
                    "description": "Read a file from the filesystem",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "File path to read" }
                        },
                        "required": ["path"]
                    }
                },
                {
                    "name": "list_files",
                    "description": "List files in a directory",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "directory": { "type": "string", "description": "Directory to list" }
                        },
                        "required": ["directory"]
                    }
                },
                {
                    "name": "write_file",
                    "description": "Write content to a file",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "File path to write" },
                            "content": { "type": "string", "description": "Content to write" }
                        },
                        "required": ["path", "content"]
                    }
                }
            ]);
            (Some(tools), None)
        }
        "tools/call" => {
            let tool_name = req
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let params_copy = req.params.clone();

            let result = match tool_name.as_str() {
                "read_file" => {
                    let path = params_copy
                        .get("arguments")
                        .and_then(|a| a.get("path"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("(no path)");
                    json!({
                        "success": true,
                        "content": format!("Mock file content from: {}", path)
                    })
                }
                "list_files" => {
                    json!({
                        "success": true,
                        "files": ["file1.txt", "file2.txt", "file3.txt"]
                    })
                }
                "write_file" => {
                    let path = params_copy
                        .get("arguments")
                        .and_then(|a| a.get("path"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("(no path)");
                    json!({
                        "success": true,
                        "message": format!("Wrote {} bytes to {}", 42, path)
                    })
                }
                _ => {
                    let error = JsonRpcError {
                        code: -32601,
                        message: format!("Unknown tool: {}", tool_name),
                    };
                    return Json(JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: req.id,
                        result: None,
                        error: Some(error),
                    });
                }
            };

            (Some(result), None)
        }
        _ => {
            let error = JsonRpcError {
                code: -32601,
                message: format!("Unknown method: {}", req.method),
            };
            (None, Some(error))
        }
    };

    Json(JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: req.id,
        result,
        error,
    })
}
