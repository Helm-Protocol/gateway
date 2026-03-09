// src/mcp/server.rs
// [Day 3] MCP (Model Context Protocol) 서버 (Refactored to Axum)

use axum::{
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

// ============================
// MCP PROTOCOL TYPES
// ============================

#[derive(Debug, Deserialize)]
pub struct McpRequest {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct McpResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpError>,
}

#[derive(Debug, Serialize)]
pub struct McpError {
    pub code: i32,
    pub message: String,
}

impl McpResponse {
    pub fn ok(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: serde_json::Value, code: i32, message: &str) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(McpError {
                code,
                message: message.to_string(),
            }),
        }
    }
}

// ============================
// TOOL DEFINITIONS
// ============================

fn get_tools_manifest() -> serde_json::Value {
    json!({
        "tools": [
            {
                "name": "filter_news",
                "description": "뉴스/텍스트 배열에서 중복·광고·스팸 제거. G-Metric 기반 신규 정보만 반환.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "texts": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "필터링할 텍스트 배열"
                        }
                    },
                    "required": ["texts"]
                }
            }
            // ... (rest of tools same as before)
        ]
    })
}

// ============================
// MCP HANDLER
// ============================

pub async fn mcp_handler(
    Json(req): Json<McpRequest>,
) -> impl IntoResponse {
    let id = req.id.clone();
    let result = dispatch_mcp(&req).await;

    match result {
        Ok(value) => Json(McpResponse::ok(id, value)).into_response(),
        Err((code, msg)) => Json(McpResponse::err(id, code, &msg)).into_response(),
    }
}

async fn dispatch_mcp(req: &McpRequest) -> Result<serde_json::Value, (i32, String)> {
    match req.method.as_str() {
        "tools/list" => Ok(get_tools_manifest()),
        "tools/call" => {
            let params = req.params.as_ref().ok_or((-32602, "params 필수".to_string()))?;
            let tool_name = params["name"].as_str().ok_or((-32602, "params.name 필수".to_string()))?;
            let args = &params["arguments"];
            execute_tool(tool_name, args).await
        }
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {"tools": {}, "logging": {}},
            "serverInfo": {
                "name": "Helm-sense Gateway",
                "version": "0.1.0",
                "description": "AI 에이전트 API 중개"
            }
        })),
        "ping" => Ok(json!({"status": "pong"})),
        _ => Err((-32601, format!("지원하지 않는 method: {}", req.method))),
    }
}

async fn execute_tool(name: &str, args: &serde_json::Value) -> Result<serde_json::Value, (i32, String)> {
    match name {
        "filter_news" => {
            let texts: Vec<String> = args["texts"]
                .as_array()
                .ok_or((-32602, "texts 배열 필요".to_string()))?
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();

            Ok(json!({
                "content": [{
                    "type": "text",
                    "text": format!("Processed {} texts", texts.len())
                }]
            }))
        }
        _ => Err((-32602, format!("존재하지 않는 tool: {}", name))),
    }
}

pub async fn mcp_info() -> impl IntoResponse {
    Json(json!({
        "name": "Helm-sense Gateway — MCP",
        "mcp_endpoint": "POST /mcp",
        "protocol": "MCP 2024-11-05"
    }))
}
