//! `cooldesk mcp serve`: an MCP server on stdio, backed by the app's own
//! command manifest (`GET /cli/commands`) and dispatch (`POST /cli/invoke`).
//!
//! Why stdio in the CLI, not the app's `/mcp` route:
//! * `/mcp` has no auth. Exposing actions there would bypass the /cli token.
//! * The token rotates at every app start. This server holds no token: each
//!   /cli request re-reads the token file, so an app restart needs no restart here.
//! * Tools come from the running app, so a new app command appears with no release.
//!
//! Policy:
//! * `--tiers` picks what's exposed (default read,act,destructive). `secret` (the vault)
//!   is only exposed when it's listed explicitly.
//! * Destructive and secret tools require `confirm: true` in their input schema.
//!   The server moves it into /cli/invoke's `confirm`, which the app checks.
//! * The app's 7 legacy read-only `/mcp` tools are proxied too. The manifest wins on a name clash.
//! * Protocol: newline-delimited JSON-RPC 2.0. stdout carries protocol only, logs go to stderr.

use crate::{Client, Failure};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};

pub const ALL_TIERS: &[&str] = &["read", "act", "destructive", "secret"];
const DEFAULT_PROTOCOL: &str = "2025-06-18";

pub fn needs_confirm(tier: &str) -> bool {
    tier == "destructive" || tier == "secret"
}

/// JSON Schema for one manifest arg type (see the app's `cli_invoke::ArgSpec`).
pub fn arg_schema(ty: &str) -> Value {
    match ty {
        "string" => json!({ "type": "string" }),
        "string[]" => json!({ "type": "array", "items": { "type": "string" } }),
        "bool" => json!({ "type": "boolean" }),
        "u16" | "u32" | "usize" => json!({ "type": "integer", "minimum": 0 }),
        "i32" | "i64" => json!({ "type": "integer" }),
        "f64" => json!({ "type": "number" }),
        "json" => json!({}),
        _ => json!({ "type": "object" }),
    }
}

/// One manifest command → one MCP tool definition.
pub fn manifest_tool(cmd: &Value) -> Option<Value> {
    let name = cmd["name"].as_str()?;
    let tier = cmd["tier"].as_str()?;
    let summary = cmd["summary"].as_str().unwrap_or("");
    let mut props = Map::new();
    let mut required = Vec::new();
    for a in cmd["args"].as_array().into_iter().flatten() {
        let (Some(an), Some(ty)) = (a["name"].as_str(), a["type"].as_str()) else { continue };
        props.insert(an.to_string(), arg_schema(ty));
        if a["required"].as_bool() == Some(true) {
            required.push(json!(an));
        }
    }
    let mut description = format!("{summary} [tier: {tier}]");
    if needs_confirm(tier) {
        props.insert(
            "confirm".into(),
            json!({ "type": "boolean", "const": true, "description": "Must be true. Set it only when the user asked for this action." }),
        );
        required.push(json!("confirm"));
        description.push_str(if tier == "secret" {
            " Touches the password vault; requires confirm=true."
        } else {
            " Destructive; requires confirm=true."
        });
    }
    Some(json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": props, "required": required, "additionalProperties": false },
        "annotations": {
            "title": name,
            "readOnlyHint": tier == "read",
            "destructiveHint": needs_confirm(tier),
        },
    }))
}

/// The tool set: manifest commands whose tier is enabled, then legacy tools
/// whose names don't clash. Returns (tools, name → Route).
pub fn build_tools(manifest: &Value, legacy: &Value, tiers: &[String]) -> (Vec<Value>, BTreeMap<String, Route>) {
    let mut tools = Vec::new();
    let mut routes = BTreeMap::new();
    for cmd in manifest["commands"].as_array().into_iter().flatten() {
        let tier = cmd["tier"].as_str().unwrap_or("");
        if !tiers.iter().any(|t| t == tier) {
            continue;
        }
        if let Some(t) = manifest_tool(cmd) {
            routes.insert(t["name"].as_str().unwrap().to_string(), Route::Invoke { tier: tier.to_string() });
            tools.push(t);
        }
    }
    for t in legacy["tools"].as_array().into_iter().flatten() {
        let Some(name) = t["name"].as_str() else { continue };
        if routes.contains_key(name) {
            eprintln!("[cooldesk mcp] legacy tool '{name}' shadowed by the manifest");
            continue;
        }
        routes.insert(name.to_string(), Route::Legacy);
        tools.push(t.clone());
    }
    (tools, routes)
}

#[derive(Clone, Debug, PartialEq)]
pub enum Route {
    /// POST /cli/invoke (token-gated).
    Invoke { tier: String },
    /// POST /mcp tools/call (the app's legacy read-only tools).
    Legacy,
}

/// Split `confirm` out of the tool arguments: (args for the command, confirm).
pub fn split_confirm(arguments: &Value) -> (Value, bool) {
    let mut args = arguments.as_object().cloned().unwrap_or_default();
    let confirm = args.remove("confirm").and_then(|v| v.as_bool()).unwrap_or(false);
    (Value::Object(args), confirm)
}

fn text_result(text: String, is_error: bool) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

fn failure_text(f: &Failure) -> String {
    match f {
        Failure::NotRunning(m) => format!("CoolDesk is not reachable: {m}"),
        Failure::Http(m) | Failure::Usage(m) => m.clone(),
        Failure::Auth(m) => format!("CLI token refused or missing: {m}"),
        Failure::Refused(m) => format!("{m} (set confirm=true only if the user asked for this)"),
    }
}

pub struct Server {
    client: Client,
    tiers: Vec<String>,
    routes: BTreeMap<String, Route>,
}

impl Server {
    pub fn new(client: Client, tiers: Vec<String>) -> Self {
        Server { client, tiers, routes: BTreeMap::new() }
    }

    /// Fetch the manifest and legacy tools from the running app. Anything
    /// unreachable contributes nothing (logged to stderr).
    fn fetch(&self) -> (Vec<Value>, BTreeMap<String, Route>) {
        let manifest = self.client.get("/cli/commands", &[]).unwrap_or_else(|e| {
            eprintln!("[cooldesk mcp] /cli/commands unavailable: {}", failure_text(&e));
            Value::Null
        });
        let legacy = crate::mcp(&self.client, "tools/list", json!({})).unwrap_or_else(|e| {
            eprintln!("[cooldesk mcp] legacy /mcp unavailable: {}", failure_text(&e));
            Value::Null
        });
        build_tools(&manifest, &legacy, &self.tiers)
    }

    pub fn tool_names(&self) -> BTreeSet<String> {
        self.fetch().1.into_keys().collect()
    }

    fn list(&mut self) -> Value {
        let (tools, routes) = self.fetch();
        self.routes = routes;
        json!({ "tools": tools })
    }

    fn call(&mut self, params: &Value) -> Value {
        let name = params["name"].as_str().unwrap_or("").to_string();
        if !self.routes.contains_key(&name) {
            // The app may have started (or changed) since the last list.
            self.routes = self.fetch().1;
        }
        let arguments = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
        match self.routes.get(&name).cloned() {
            Some(Route::Invoke { tier }) => {
                let (args, confirm) = split_confirm(&arguments);
                eprintln!("[cooldesk mcp] call {name} ({tier})");
                match self.client.post("/cli/invoke", &json!({ "cmd": name, "args": args, "confirm": confirm })) {
                    Ok(v) => {
                        let r = v.get("result").cloned().unwrap_or(Value::Null);
                        text_result(serde_json::to_string_pretty(&r).unwrap_or_default(), false)
                    }
                    Err(e) => text_result(failure_text(&e), true),
                }
            }
            Some(Route::Legacy) => match crate::mcp(&self.client, "tools/call", json!({ "name": name, "arguments": arguments })) {
                Ok(v) => v,
                Err(e) => text_result(failure_text(&e), true),
            },
            None => text_result(
                format!("unknown tool '{name}'. Either CoolDesk isn't running, this tool's tier isn't enabled (--tiers), or it doesn't exist."),
                true,
            ),
        }
    }

    /// Handle one JSON-RPC message. Returns the reply (None for notifications).
    pub fn handle(&mut self, msg: &Value, initialized: &mut bool) -> Option<Value> {
        let id = msg.get("id").cloned();
        let method = msg["method"].as_str().unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        if id.is_none() {
            if method == "notifications/initialized" {
                *initialized = true;
            }
            return None;
        }
        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": params["protocolVersion"].as_str().unwrap_or(DEFAULT_PROTOCOL),
                "capabilities": { "tools": { "listChanged": true } },
                "serverInfo": { "name": "cooldesk", "version": env!("CARGO_PKG_VERSION") },
                "instructions": "Controls the CoolDesk desktop launcher on this machine: apps, windows, files, spotlight, dock, shelf, clipboard history, terminals, processes. Read-tier tools are safe. Destructive tools require confirm=true; set it only when the user asked for that action. If tools are missing, CoolDesk isn't running or is a build without the /cli API.",
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(self.list()),
            "tools/call" => Ok(self.call(&params)),
            other => Err(json!({ "code": -32601, "message": format!("method not found: {other}") })),
        };
        Some(match result {
            Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
            Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": e }),
        })
    }
}

fn write_line(out: &Mutex<std::io::Stdout>, v: &Value) {
    let mut o = out.lock().unwrap();
    let _ = writeln!(o, "{v}");
    let _ = o.flush();
}

/// Run until stdin closes.
pub fn serve(make_client: impl Fn() -> Client + Send + 'static, tiers: Vec<String>, poll_secs: u64) {
    let out = Arc::new(Mutex::new(std::io::stdout()));
    let initialized = Arc::new(Mutex::new(false));

    // Poll the app and tell the client when the tool set changes (e.g. CoolDesk
    // started after the session did, or was updated).
    if poll_secs > 0 {
        let out = out.clone();
        let initialized = initialized.clone();
        let poller = Server::new(make_client(), tiers.clone());
        std::thread::spawn(move || {
            let mut last = poller.tool_names();
            loop {
                std::thread::sleep(std::time::Duration::from_secs(poll_secs));
                let now = poller.tool_names();
                if now != last && *initialized.lock().unwrap() {
                    eprintln!("[cooldesk mcp] tool set changed ({} → {}); notifying", last.len(), now.len());
                    write_line(&out, &json!({ "jsonrpc": "2.0", "method": "notifications/tools/list_changed" }));
                }
                last = now;
            }
        });
    }

    let mut server = Server::new(make_client(), tiers);
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                write_line(&out, &json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": format!("parse error: {e}") } }));
                continue;
            }
        };
        let mut init = *initialized.lock().unwrap();
        let reply = server.handle(&msg, &mut init);
        *initialized.lock().unwrap() = init;
        if let Some(r) = reply {
            write_line(&out, &r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Value {
        json!({ "commands": [
            { "name": "get_focused_app", "tier": "read", "summary": "focused", "args": [] },
            { "name": "launch_app", "tier": "act", "summary": "launch", "args": [{ "name": "path", "type": "string", "required": true }] },
            { "name": "kill_process", "tier": "destructive", "summary": "kill", "args": [{ "name": "pid", "type": "u32", "required": true }] },
            { "name": "vault_get", "tier": "secret", "summary": "get", "args": [{ "name": "id", "type": "string", "required": true }] },
            { "name": "focus_window", "tier": "act", "summary": "focus", "args": [
                { "name": "pid", "type": "u32", "required": true }, { "name": "hwnd", "type": "i64", "required": false } ] }
        ]})
    }

    fn tiers(s: &str) -> Vec<String> {
        s.split(',').map(String::from).collect()
    }

    #[test]
    fn default_tiers_exclude_secret() {
        let (_, routes) = build_tools(&manifest(), &Value::Null, &tiers("read,act,destructive"));
        assert!(routes.contains_key("kill_process"));
        assert!(!routes.contains_key("vault_get"), "secret must be opt-in");
        let (_, routes) = build_tools(&manifest(), &Value::Null, &tiers("read,act,destructive,secret"));
        assert!(routes.contains_key("vault_get"));
    }

    #[test]
    fn read_only_filter() {
        let (tools, _) = build_tools(&manifest(), &Value::Null, &tiers("read"));
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["annotations"]["readOnlyHint"], json!(true));
    }

    #[test]
    fn destructive_requires_confirm_in_schema() {
        let t = manifest_tool(&manifest()["commands"][2]).unwrap();
        assert_eq!(t["inputSchema"]["properties"]["confirm"]["const"], json!(true));
        assert!(t["inputSchema"]["required"].as_array().unwrap().contains(&json!("confirm")));
        assert_eq!(t["annotations"]["destructiveHint"], json!(true));
        let t = manifest_tool(&manifest()["commands"][1]).unwrap();
        assert!(t["inputSchema"]["properties"].get("confirm").is_none(), "act tools have no confirm");
    }

    #[test]
    fn schema_types_and_required() {
        let t = manifest_tool(&manifest()["commands"][4]).unwrap();
        assert_eq!(t["inputSchema"]["properties"]["pid"], json!({ "type": "integer", "minimum": 0 }));
        assert_eq!(t["inputSchema"]["properties"]["hwnd"], json!({ "type": "integer" }));
        assert_eq!(t["inputSchema"]["required"], json!(["pid"]));
        assert_eq!(t["inputSchema"]["additionalProperties"], json!(false));
        assert_eq!(arg_schema("string[]"), json!({ "type": "array", "items": { "type": "string" } }));
    }

    #[test]
    fn confirm_is_split_out_of_args() {
        let (a, c) = split_confirm(&json!({ "pid": 5, "confirm": true }));
        assert_eq!((a, c), (json!({ "pid": 5 }), true));
        let (a, c) = split_confirm(&json!({ "pid": 5 }));
        assert_eq!((a, c), (json!({ "pid": 5 }), false));
        let (_, c) = split_confirm(&json!({ "confirm": "yes" }));
        assert!(!c, "only a JSON true confirms");
    }

    #[test]
    fn manifest_wins_name_clash_with_legacy() {
        let legacy = json!({ "tools": [ { "name": "launch_app", "description": "old" }, { "name": "open_tabs", "description": "tabs" } ] });
        let (tools, routes) = build_tools(&manifest(), &legacy, &tiers("read,act"));
        assert_eq!(routes["launch_app"], Route::Invoke { tier: "act".into() });
        assert_eq!(routes["open_tabs"], Route::Legacy);
        assert_eq!(tools.iter().filter(|t| t["name"] == "launch_app").count(), 1);
    }

    #[test]
    fn notifications_get_no_reply_and_unknown_methods_error() {
        let mut s = Server::new(Client::new(1, Some("t".into()), 1), tiers("read"));
        let mut init = false;
        assert!(s.handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }), &mut init).is_none());
        assert!(init);
        let r = s.handle(&json!({ "jsonrpc": "2.0", "id": 7, "method": "nope" }), &mut init).unwrap();
        assert_eq!(r["error"]["code"], json!(-32601));
        assert_eq!(r["id"], json!(7));
        let r = s.handle(&json!({ "jsonrpc": "2.0", "id": 8, "method": "initialize", "params": { "protocolVersion": "2025-03-26" } }), &mut init).unwrap();
        assert_eq!(r["result"]["protocolVersion"], json!("2025-03-26"));
        assert_eq!(r["result"]["capabilities"]["tools"]["listChanged"], json!(true));
    }

    #[test]
    fn app_unreachable_lists_nothing_and_calls_error_cleanly() {
        // Port 1: nothing listens. tools/list must still answer, and a call must be an isError result.
        let mut s = Server::new(Client::new(1, Some("t".into()), 1), tiers("read,act,destructive"));
        let mut init = true;
        let r = s.handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }), &mut init).unwrap();
        assert_eq!(r["result"]["tools"], json!([]));
        let r = s.handle(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "get_focused_app", "arguments": {} } }), &mut init).unwrap();
        assert_eq!(r["result"]["isError"], json!(true));
    }
}
