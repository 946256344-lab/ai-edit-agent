//! newline 分隔的 JSON-RPC 2.0 MCP 循环；也接受 LSP 风格的 Content-Length 帧。

use serde_json::{json, Map, Value};
use std::io::{self, BufRead, Write};
use tauri::AppHandle;

use super::tools;

const PROTOCOL_VERSION: &str = "2024-11-05";

pub(crate) fn serve(app: AppHandle) -> Result<(), String> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    loop {
        let Some(message) = read_message(&mut reader).map_err(|error| error.to_string())? else {
            return Ok(());
        };
        if message.get("method").and_then(Value::as_str) == Some("notifications/initialized") {
            continue;
        }
        if message.get("id").is_none() {
            continue;
        }
        let response = handle_request(&app, &message);
        write_message(&response).map_err(|error| error.to_string())?;
    }
}

fn handle_request(app: &AppHandle, request: &Value) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let params = request.get("params").cloned().unwrap_or(json!({}));
    match method {
        "initialize" => ok(id, initialize_result(&params)),
        "ping" => ok(id, json!({})),
        "tools/list" => ok(id, json!({ "tools": tools::catalog() })),
        "tools/call" => match call_tool(app, &params) {
            Ok(result) => ok(id, result),
            Err(error) => ok(
                id,
                json!({
                    "content": [{ "type": "text", "text": error }],
                    "isError": true
                }),
            ),
        },
        _ => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": format!("Method not found: {method}") }
        }),
    }
}

fn initialize_result(params: &Value) -> Value {
    let requested = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(PROTOCOL_VERSION);
    let version = if requested.starts_with("2024-") || requested.starts_with("2025-") {
        requested
    } else {
        PROTOCOL_VERSION
    };
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": {
            "name": "voycut",
            "version": env!("CARGO_PKG_VERSION"),
            "title": "Voycut"
        },
        "instructions": "Voycut MCP reads the local analyzed footage library and writes new editor drafts. It does not run model-paid analysis, expose credentials, or overwrite existing editor projects."
    })
}

fn call_tool(app: &AppHandle, params: &Value) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "Tool call is missing a name.".to_owned())?;
    let arguments = match params.get("arguments") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(map)) => map.clone(),
        Some(_) => return Err("Tool arguments must be an object.".to_owned()),
    };
    let value = tools::dispatch(app, name, Value::Object(arguments))?;
    let text = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
    Ok(json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": value,
        "isError": false
    }))
}

fn ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn read_message(reader: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut header = String::new();
    let mut content_length = None;
    loop {
        header.clear();
        let n = reader.read_line(&mut header)?;
        if n == 0 {
            return Ok(None);
        }
        let line = header.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            if let Some(length) = content_length {
                let mut buf = vec![0_u8; length];
                reader.read_exact(&mut buf)?;
                let value = serde_json::from_slice(&buf).map_err(json_error)?;
                return Ok(Some(value));
            }
            continue;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
            );
            continue;
        }
        if line.starts_with("Content-Type:") {
            continue;
        }
        if content_length.is_none() {
            if line.starts_with('{') {
                let value = serde_json::from_str(line).map_err(json_error)?;
                return Ok(Some(value));
            }
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported MCP frame: {line}"),
            ));
        }
    }
}

fn write_message(value: &Value) -> io::Result<()> {
    let payload = serde_json::to_string(value)?;
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{payload}")?;
    stdout.flush()
}

fn json_error(error: serde_json::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}
