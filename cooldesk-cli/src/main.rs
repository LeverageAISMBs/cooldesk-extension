//! cooldesk-cli — drive a running CoolDesk through its local sidecar (127.0.0.1:<port>).
//!
//! P0 runs on the surfaces the app already exposes: HTTP data routes, `/ws`
//! and `/mcp`. Every route used here maps to a line in
//! `src-tauri/src/sidecar/server.rs` (the route table at lines 213-313).

mod mcp_serve;
mod p5;

use clap::{Args, Parser, Subcommand};
use serde_json::{Value, json};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

const EXIT_HTTP: u8 = 1;
const EXIT_NOT_RUNNING: u8 = 3;

#[derive(Parser)]
#[command(name = "cooldesk-cli", version, about = "Drive a running CoolDesk from the command line")]
struct Cli {
    /// Sidecar port (server.rs: `const PORT: u16 = 4545`)
    #[arg(long, env = "COOLDESK_PORT", default_value_t = 4545, global = true)]
    port: u16,
    /// Single-line JSON instead of pretty-printed
    #[arg(long, global = true)]
    compact: bool,
    /// Override the CLI token. By default it's read from
    /// <local data dir>/com.cooldesk.desktop/cli-token, which the app rewrites at every start.
    #[arg(long, env = "COOLDESK_TOKEN", global = true, hide_env_values = true)]
    token: Option<String>,
    /// Seconds to wait for a response. Raise it for `llm` calls.
    #[arg(long, env = "COOLDESK_TIMEOUT", global = true, default_value_t = 30)]
    timeout: u64,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Sidecar liveness: GET /health
    Health,
    /// Workspaces (GET/POST /workspaces; POST merges by name)
    #[command(subcommand)]
    Ws(WsCmd),
    /// Saved URLs (GET/POST /urls)
    #[command(subcommand)]
    Urls(UrlsCmd),
    /// Browser tabs reported by connected extensions
    #[command(subcommand)]
    Tabs(TabsCmd),
    /// Notes (GET/POST /notes; POST merges by id)
    #[command(subcommand)]
    Notes(NotesCmd),
    /// Pins (GET/POST /pins)
    #[command(subcommand)]
    Pins(PinsCmd),
    /// Settings map (GET/POST /settings; POST merges keys)
    #[command(subcommand)]
    Settings(KvCmd),
    /// UI state map (GET/POST /ui-state; POST merges keys)
    #[command(name = "ui-state", subcommand)]
    UiState(KvCmd),
    /// Activity and usage
    #[command(subcommand)]
    Activity(ActivityCmd),
    /// Search installed/running apps or browser history
    #[command(subcommand)]
    Search(SearchCmd),
    /// Read-only collections: url-notes, daily-memory, dashboard, graph, scraped-chats, scraped-configs, servers
    Get {
        #[arg(value_parser = ["url-notes", "daily-memory", "dashboard", "graph", "scraped-chats", "scraped-configs", "servers"])]
        what: String,
    },
    /// Local LLM (/llm/*, /llm/v3/*)
    #[command(subcommand)]
    Llm(LlmCmd),
    /// The app's MCP endpoint (POST /mcp, JSON-RPC)
    #[command(subcommand)]
    Mcp(McpCmd),
    /// Stream sidecar WebSocket events as NDJSON (one JSON object per line)
    Watch {
        /// Only print these message types (comma-separated), e.g. push-tabs,push-notes
        #[arg(long, value_delimiter = ',')]
        r#type: Vec<String>,
        /// Ask for a full state push on connect
        #[arg(long)]
        initial: bool,
        /// Stop after N printed messages
        #[arg(long)]
        count: Option<usize>,
    },
    /// Escape hatch: call any sidecar route directly
    #[command(subcommand)]
    Raw(RawCmd),
    /// Check the token-gated /cli namespace (GET /cli/ping)
    #[command(subcommand)]
    Auth(AuthCmd),
    /// List the app actions /cli/invoke can run, with their tier and args (GET /cli/commands)
    Commands {
        /// Only commands of this tier: read, act, destructive, secret
        #[arg(long)]
        tier: Option<String>,
    },
    /// Run an app action through POST /cli/invoke. `cooldesk-cli commands` lists them.
    Invoke {
        /// Command name, e.g. launch_app
        cmd: String,
        /// Arguments as a JSON object with snake_case names, e.g. '{"path":"C:/Windows/notepad.exe"}'
        #[arg(long, default_value = "{}")]
        json: String,
        /// Confirm a destructive or secret command. Without it the app refuses (exit 5).
        #[arg(long)]
        yes: bool,
    },
    /// Generate the Markdown command reference from the live manifest
    Docs {
        /// Write to this file instead of stdout
        #[arg(long)]
        out: Option<String>,
    },
    /// Print shell completions (bash, zsh, fish, powershell, elvish). `invoke <cmd>` (not elvish)
    /// completes command names when CoolDesk is running at generation time.
    Completions { shell: clap_complete::Shell },
    /// Run many actions: NDJSON on stdin, one {"cmd","args","confirm"} per line; NDJSON
    /// results on stdout. Blank lines and lines starting with # are skipped.
    Batch {
        /// Allow lines to confirm destructive/secret commands ("confirm": true). Without it
        /// every line's confirm is ignored, so destructive lines are refused.
        #[arg(long)]
        yes: bool,
        /// Stop at the first failed line
        #[arg(long)]
        stop_on_error: bool,
    },
}

#[derive(Subcommand)]
enum AuthCmd {
    /// Ping /cli with the token. Exit 4 if it's refused.
    Status,
    /// Print where the token file is expected
    Path,
}

#[derive(Subcommand)]
enum WsCmd {
    List,
    /// Show one workspace by name (case-insensitive)
    Show { name: String },
    /// Create or update a workspace (merged by name)
    Add {
        name: String,
        /// URL to include (repeatable)
        #[arg(long = "url")]
        urls: Vec<String>,
    },
}

#[derive(Subcommand)]
enum UrlsCmd {
    List,
    Add { url: String, #[arg(long)] title: Option<String> },
}

#[derive(Subcommand)]
enum TabsCmd {
    List,
    /// Ask the owning browser extension to focus a tab (POST /cmd/jump-to-tab)
    Jump(TabTarget),
    /// Ask the owning browser extension to close a tab (POST /cmd/close-tab)
    Close(TabTarget),
}

#[derive(Args)]
struct TabTarget {
    tab_id: i64,
    #[arg(long)]
    window_id: Option<i64>,
    #[arg(long)]
    browser: Option<String>,
    #[arg(long)]
    device_id: Option<String>,
}

#[derive(Subcommand)]
enum NotesCmd {
    List,
    Add {
        text: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        folder: Option<String>,
        #[arg(long = "tag")]
        tags: Vec<String>,
    },
}

#[derive(Subcommand)]
enum PinsCmd {
    List,
    Add { url: String, #[arg(long)] title: Option<String> },
}

#[derive(Subcommand)]
enum KvCmd {
    /// Print the whole map, or one key
    Get { key: Option<String> },
    /// Set a key. VALUE is parsed as JSON and falls back to a string.
    Set { key: String, value: String },
}

#[derive(Subcommand)]
enum ActivityCmd {
    List { #[arg(long)] since: Option<i64> },
    Focused,
    Visible,
    Apps { #[arg(long)] date: Option<String>, #[arg(long)] days: Option<u32> },
    Sites { #[arg(long)] days: Option<i64> },
}

#[derive(Subcommand)]
enum SearchCmd {
    Apps { query: String, #[arg(long)] limit: Option<usize> },
    History { query: String, #[arg(long)] limit: Option<usize> },
}

#[derive(Subcommand)]
enum LlmCmd {
    Status,
    Models,
    Load { model: String, #[arg(long)] gpu_layers: Option<u32> },
    Unload,
    /// One-shot chat through the v3 agent (POST /llm/v3/simple-chat)
    Chat { message: String },
    /// v3 agent status and config
    V3Status,
    Config,
}

#[derive(Subcommand)]
enum McpCmd {
    /// List the app's legacy read-only /mcp tools
    Tools,
    /// Run an MCP server on stdio that exposes the app's /cli commands (and the
    /// legacy /mcp tools) to an MCP client such as Claude Code. Register with:
    /// claude mcp add cooldesk -s user -- <path-to>/cooldesk-cli.exe mcp serve
    Serve {
        /// Tiers to expose. `secret` (the vault) is only exposed when listed here.
        #[arg(long, value_delimiter = ',', default_value = "read,act,destructive")]
        tiers: Vec<String>,
        /// Seconds between checks for a changed tool set (0 = off)
        #[arg(long, default_value_t = 5)]
        poll: u64,
    },
    /// Call a tool. ARGS is a JSON object.
    Call { tool: String, #[arg(default_value = "{}")] args: String },
}

#[derive(Subcommand)]
enum RawCmd {
    Get { path: String },
    Post { path: String, #[arg(default_value = "{}")] body: String },
}

// ---------------------------------------------------------------------------

enum Failure {
    NotRunning(String),
    Http(String),
    Usage(String),
    Auth(String),
    /// The app refused a destructive command that wasn't confirmed (HTTP 428).
    Refused(String),
}

const EXIT_AUTH: u8 = 4;
const EXIT_REFUSED: u8 = 5;

type R<T> = Result<T, Failure>;

/// Must match the app's `cli_auth::token_path()`.
fn token_path() -> Option<std::path::PathBuf> {
    dirs::data_local_dir().map(|d| d.join("com.cooldesk.desktop").join("cli-token"))
}

struct Client {
    base: String,
    agent: ureq::Agent,
    token_override: Option<String>,
}

impl Client {
    fn new(port: u16, token_override: Option<String>, timeout_s: u64) -> Self {
        // No connection pooling: `mcp serve` is long-lived, and a pooled socket to a
        // CoolDesk that has since restarted gets reset (os error 10054). ureq only
        // retries idempotent methods, so the next POST /cli/invoke would fail.
        // Localhost connections are cheap.
        let agent = ureq::AgentBuilder::new()
            .max_idle_connections(0)
            .timeout_connect(std::time::Duration::from_secs(2))
            .timeout(std::time::Duration::from_secs(timeout_s))
            .build();
        Client { base: format!("http://127.0.0.1:{port}"), agent, token_override }
    }

    /// Read the token at call time. The app rotates it on every start, so a cached value would go stale.
    fn token(&self) -> R<String> {
        if let Some(t) = &self.token_override {
            return Ok(t.trim().to_string());
        }
        let p = token_path().ok_or_else(|| Failure::Auth("no local data directory to read the CLI token from".into()))?;
        std::fs::read_to_string(&p)
            .map(|s| s.trim().to_string())
            .map_err(|e| Failure::Auth(format!("cannot read CLI token at {} ({e}). Is this CoolDesk build new enough to have /cli?", p.display())))
    }

    fn authorize(&self, path: &str, req: ureq::Request) -> R<ureq::Request> {
        if path.starts_with("/cli") {
            Ok(req.set("Authorization", &format!("Bearer {}", self.token()?)))
        } else {
            Ok(req)
        }
    }

    fn get(&self, path: &str, query: &[(&str, String)]) -> R<Value> {
        let mut req = self.authorize(path, self.agent.get(&format!("{}{}", self.base, path)))?;
        for (k, v) in query {
            req = req.query(k, v);
        }
        self.finish(path, req.call())
    }

    fn post(&self, path: &str, body: &Value) -> R<Value> {
        let req = self.authorize(path, self.agent.post(&format!("{}{}", self.base, path)))?;
        self.finish(path, req.send_json(body))
    }

    fn finish(&self, path: &str, res: Result<ureq::Response, ureq::Error>) -> R<Value> {
        match res {
            Ok(resp) => {
                let status = resp.status();
                let text = resp.into_string().map_err(|e| Failure::Http(e.to_string()))?;
                if text.trim().is_empty() {
                    // 204 from merge-style POSTs
                    return Ok(json!({ "ok": true, "status": status }));
                }
                Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
            }
            Err(ureq::Error::Status(code, resp)) => {
                let body = resp.into_string().unwrap_or_default();
                let msg = format!("{path} -> HTTP {code}: {body}");
                match code {
                    401 | 403 => Err(Failure::Auth(msg)),
                    428 => Err(Failure::Refused(format!("{msg}
Pass --yes to confirm."))),
                    _ => Err(Failure::Http(msg)),
                }
            }
            Err(ureq::Error::Transport(t)) => {
                let s = t.to_string().to_lowercase();
                // "Connect error" = nothing listening. Anything else that times out after
                // connecting (Windows says "status line" / os error 10060) = stale listener.
                let stale = !s.contains("connect error")
                    && (s.contains("timed out") || s.contains("status line") || s.contains("10060"));
                let hint = if stale {
                    "connected but got no response. The port may be held by a stale socket (on Windows, a terminal launched from an old CoolDesk keeps it open)"
                } else {
                    "Is the app running?"
                };
                Err(Failure::NotRunning(format!("cannot reach CoolDesk at {} ({t}). {hint}", self.base)))
            }
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn new_id(prefix: &str) -> String {
    format!("{prefix}-{}", now_ms())
}

fn parse_json_arg(s: &str, what: &str) -> R<Value> {
    serde_json::from_str(s).map_err(|e| Failure::Usage(format!("{what} is not valid JSON: {e}")))
}

fn opt_q<T: ToString>(k: &'static str, v: &Option<T>) -> Vec<(&'static str, String)> {
    v.as_ref().map(|x| vec![(k, x.to_string())]).unwrap_or_default()
}

fn tab_body(t: &TabTarget) -> Value {
    let mut b = json!({ "tabId": t.tab_id });
    if let Some(w) = t.window_id { b["windowId"] = json!(w); }
    if let Some(s) = &t.browser { b["browser"] = json!(s); }
    if let Some(s) = &t.device_id { b["deviceId"] = json!(s); }
    b
}

fn kv(c: &Client, path: &str, cmd: &KvCmd) -> R<Value> {
    match cmd {
        KvCmd::Get { key: None } => c.get(path, &[]),
        KvCmd::Get { key: Some(k) } => Ok(c.get(path, &[])?.get(k).cloned().unwrap_or(Value::Null)),
        KvCmd::Set { key, value } => {
            let v = serde_json::from_str(value).unwrap_or(Value::String(value.clone()));
            c.post(path, &json!({ key: v }))
        }
    }
}

fn mcp(c: &Client, method: &str, params: Value) -> R<Value> {
    let resp = c.post("/mcp", &json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }))?;
    if let Some(err) = resp.get("error") {
        return Err(Failure::Http(format!("MCP error: {err}")));
    }
    Ok(resp.get("result").cloned().unwrap_or(resp))
}

fn watch(port: u16, types: &[String], initial: bool, count: Option<usize>) -> R<Value> {
    use std::io::Write;
    use tungstenite::Message;
    let url = format!("ws://127.0.0.1:{port}/ws");
    let (mut sock, _) = tungstenite::connect(&url)
        .map_err(|e| Failure::NotRunning(format!("cannot open {url} ({e}). Is the app running?")))?;
    let send = |s: &mut tungstenite::WebSocket<_>, v: Value| {
        s.send(Message::Text(v.to_string())).map_err(|e| Failure::Http(e.to_string()))
    };
    send(&mut sock, json!({ "type": "identify", "payload": { "client": "cooldesk-cli" } }))?;
    if initial {
        send(&mut sock, json!({ "type": "request-state", "payload": {} }))?;
    }
    let mut printed = 0usize;
    let stdout = std::io::stdout();
    loop {
        let msg = sock.read().map_err(|e| Failure::Http(format!("websocket closed: {e}")))?;
        let Message::Text(text) = msg else { continue };
        let v: Value = serde_json::from_str(&text).unwrap_or(Value::String(text.to_string()));
        let ty = v.get("type").and_then(Value::as_str).unwrap_or("");
        if !types.is_empty() && !types.iter().any(|t| t == ty) {
            continue;
        }
        let mut out = stdout.lock();
        let _ = writeln!(out, "{v}");
        let _ = out.flush();
        printed += 1;
        if count.is_some_and(|n| printed >= n) {
            return Ok(Value::Null);
        }
    }
}

fn run(cli: &Cli) -> R<Value> {
    let c = Client::new(cli.port, cli.token.clone(), cli.timeout);
    match &cli.cmd {
        Cmd::Health => c.get("/health", &[]),
        Cmd::Auth(AuthCmd::Status) => c.get("/cli/ping", &[]),
        Cmd::Auth(AuthCmd::Path) => Ok(json!({
            "path": token_path().map(|p| p.display().to_string()),
            "exists": token_path().is_some_and(|p| p.exists()),
        })),

        Cmd::Commands { tier } => {
            let mut v = c.get("/cli/commands", &[])?;
            if let (Some(t), Some(list)) = (tier, v.get_mut("commands").and_then(Value::as_array_mut)) {
                list.retain(|c| c["tier"].as_str() == Some(t.as_str()));
            }
            Ok(v)
        }
        Cmd::Invoke { cmd, json: args, yes } => {
            let args = parse_json_arg(args, "--json")?;
            if !args.is_object() {
                return Err(Failure::Usage("--json must be a JSON object".into()));
            }
            c.post("/cli/invoke", &json!({ "cmd": cmd, "args": args, "confirm": yes }))
        }

        Cmd::Docs { out } => p5::docs(&c, out.as_deref()),
        Cmd::Completions { shell } => {
            print!("{}", p5::completions(*shell, p5::manifest_entries(&c)));
            Ok(Value::Null)
        }
        Cmd::Batch { yes, stop_on_error } => match p5::batch(&c, *yes, *stop_on_error) {
            0 => Ok(Value::Null),
            n => Err(Failure::Http(format!("{n} batch line(s) failed"))),
        },

        Cmd::Ws(WsCmd::List) => c.get("/workspaces", &[]),
        Cmd::Ws(WsCmd::Show { name }) => {
            let all = c.get("/workspaces", &[])?;
            let want = name.trim().to_lowercase();
            all.as_array()
                .and_then(|a| a.iter().find(|w| w["name"].as_str().map(|n| n.trim().to_lowercase()) == Some(want.clone())))
                .cloned()
                .ok_or_else(|| Failure::Usage(format!("no workspace named {name:?}")))
        }
        Cmd::Ws(WsCmd::Add { name, urls }) => {
            let now = now_ms();
            let urls: Vec<Value> = urls.iter().map(|u| json!({ "url": u, "addedAt": now })).collect();
            c.post("/workspaces", &json!([{ "id": new_id("ws"), "name": name, "urls": urls, "createdAt": now, "updatedAt": now }]))
        }

        Cmd::Urls(UrlsCmd::List) => c.get("/urls", &[]),
        Cmd::Urls(UrlsCmd::Add { url, title }) => {
            let now = now_ms();
            c.post("/urls", &json!([{ "id": new_id("url"), "url": url, "title": title, "createdAt": now, "updatedAt": now }]))
        }

        Cmd::Tabs(TabsCmd::List) => c.get("/tabs", &[]),
        Cmd::Tabs(TabsCmd::Jump(t)) => c.post("/cmd/jump-to-tab", &tab_body(t)),
        Cmd::Tabs(TabsCmd::Close(t)) => c.post("/cmd/close-tab", &tab_body(t)),

        Cmd::Notes(NotesCmd::List) => c.get("/notes", &[]),
        Cmd::Notes(NotesCmd::Add { text, title, folder, tags }) => {
            let now = now_ms();
            let mut n = json!({ "id": new_id("note"), "text": text, "content": text, "type": "text", "createdAt": now, "updatedAt": now });
            if let Some(t) = title { n["title"] = json!(t); }
            if let Some(f) = folder { n["folder"] = json!(f); }
            if !tags.is_empty() { n["tags"] = json!(tags); }
            c.post("/notes", &json!([n]))
        }

        Cmd::Pins(PinsCmd::List) => c.get("/pins", &[]),
        Cmd::Pins(PinsCmd::Add { url, title }) => {
            let now = now_ms();
            c.post("/pins", &json!([{ "id": new_id("pin"), "url": url, "title": title, "createdAt": now, "updatedAt": now }]))
        }

        Cmd::Settings(k) => kv(&c, "/settings", k),
        Cmd::UiState(k) => kv(&c, "/ui-state", k),

        Cmd::Activity(a) => match a {
            ActivityCmd::List { since } => c.get("/activity", &opt_q("since", since)),
            ActivityCmd::Focused => c.get("/activity/focused", &[]),
            ActivityCmd::Visible => c.get("/activity/visible", &[]),
            ActivityCmd::Apps { date, days } => {
                let mut q = opt_q("date", date);
                q.extend(opt_q("days", days));
                c.get("/activity/app-usage", &q)
            }
            ActivityCmd::Sites { days } => c.get("/activity/site-usage", &opt_q("days", days)),
        },

        Cmd::Search(s) => {
            let (path, query, limit) = match s {
                SearchCmd::Apps { query, limit } => ("/search", query, limit),
                SearchCmd::History { query, limit } => ("/browser-history", query, limit),
            };
            let mut q = vec![("q", query.clone())];
            q.extend(opt_q("limit", limit));
            c.get(path, &q)
        }

        Cmd::Get { what } => {
            let path = if what == "servers" { "/local/servers".to_string() } else { format!("/{what}") };
            c.get(&path, &[])
        }

        Cmd::Llm(l) => match l {
            LlmCmd::Status => c.get("/llm/status", &[]),
            LlmCmd::Models => c.get("/llm/models", &[]),
            LlmCmd::Load { model, gpu_layers } => c.post("/llm/load", &json!({ "modelName": model, "gpuLayers": gpu_layers })),
            LlmCmd::Unload => c.post("/llm/unload", &json!({})),
            LlmCmd::Chat { message } => c.post("/llm/v3/simple-chat", &json!({ "message": message })),
            LlmCmd::V3Status => c.get("/llm/v3/status", &[]),
            LlmCmd::Config => c.get("/llm/v3/config", &[]),
        },

        Cmd::Mcp(McpCmd::Tools) => mcp(&c, "tools/list", json!({})),
        Cmd::Mcp(McpCmd::Serve { tiers, poll }) => {
            if let Some(bad) = tiers.iter().find(|t| !mcp_serve::ALL_TIERS.contains(&t.as_str())) {
                return Err(Failure::Usage(format!("unknown tier '{bad}'; use {}", mcp_serve::ALL_TIERS.join(","))));
            }
            let (port, token, timeout) = (cli.port, cli.token.clone(), cli.timeout);
            eprintln!("[cooldesk mcp] serving on stdio for 127.0.0.1:{port}, tiers {}", tiers.join(","));
            mcp_serve::serve(move || Client::new(port, token.clone(), timeout), tiers.clone(), *poll);
            Ok(Value::Null)
        }
        Cmd::Mcp(McpCmd::Call { tool, args }) => {
            let args = parse_json_arg(args, "ARGS")?;
            mcp(&c, "tools/call", json!({ "name": tool, "arguments": args }))
        }

        Cmd::Watch { r#type, initial, count } => watch(cli.port, r#type, *initial, *count),

        Cmd::Raw(RawCmd::Get { path }) | Cmd::Raw(RawCmd::Post { path, .. }) if mangled_path_hint(path).is_some() => {
            Err(Failure::Usage(mangled_path_hint(path).unwrap()))
        }
        Cmd::Raw(RawCmd::Get { path }) => c.get(&norm_path(path), &[]),
        Cmd::Raw(RawCmd::Post { path, body }) => {
            let p = norm_path(path);
            if is_dequeue_route(&p) {
                return Err(Failure::Usage(
                    "refused: /cmd/*-next dequeues the browser extension's poll queue (handlers.rs:1352)".into(),
                ));
            }
            c.post(&p, &parse_json_arg(body, "BODY")?)
        }
    }
}

fn norm_path(p: &str) -> String {
    if p.starts_with('/') { p.to_string() } else { format!("/{p}") }
}

/// Git Bash turns `/workspaces` into `C:/Program Files/Git/workspaces` before we see it.
/// Catch that and say how to fix it, instead of sending it and getting a 404.
fn mangled_path_hint(p: &str) -> Option<String> {
    let b = p.as_bytes();
    let drive = b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'/' || b[2] == b'\\');
    drive.then(|| format!(
        "path {p:?} looks rewritten by Git Bash (MSYS path conversion). Drop the leading slash, \
         e.g. `raw post workspaces …`, or set MSYS_NO_PATHCONV=1"
    ))
}

#[cfg(test)]
mod path_tests {
    use super::*;

    #[test]
    fn detects_git_bash_rewrite_only() {
        assert!(mangled_path_hint("C:/Program Files/Git/workspaces").is_some());
        assert!(mangled_path_hint("c:\\x").is_some());
        assert!(mangled_path_hint("/workspaces").is_none());
        assert!(mangled_path_hint("workspaces").is_none());
        assert!(mangled_path_hint("cli/ping").is_none());
    }
}

/// Matches anywhere in the path. Git Bash rewrites `/cmd/x` to `C:/Program Files/Git/cmd/x`,
/// so a prefix check would let it through.
fn is_dequeue_route(p: &str) -> bool {
    let p = p.to_ascii_lowercase();
    p.contains("cmd/jump-next") || p.contains("cmd/close-next")
}

/// The sidecar reports many failures as HTTP 200 with `ok:false` / `success:false`.
fn reports_failure(v: &Value) -> bool {
    v.get("ok") == Some(&Value::Bool(false)) || v.get("success") == Some(&Value::Bool(false))
}

/// Human-readable message for a failure (used by batch and mcp serve).
pub(crate) fn failure_message(f: &Failure) -> String {
    match f {
        Failure::NotRunning(m) | Failure::Http(m) | Failure::Usage(m) | Failure::Auth(m) | Failure::Refused(m) => m.clone(),
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Cmd::Raw(RawCmd::Get { path }) = &cli.cmd {
        let p = norm_path(path);
        if is_dequeue_route(&p) {
            eprintln!("refused: {p} dequeues the browser extension's poll queue (handlers.rs:1352)");
            return ExitCode::from(2);
        }
    }
    match run(&cli) {
        // Null = the command already wrote its own output (watch, mcp serve, docs, completions, batch).
        Ok(Value::Null) => ExitCode::SUCCESS,
        Ok(v) => {
            let out = if cli.compact { v.to_string() } else { serde_json::to_string_pretty(&v).unwrap_or_default() };
            println!("{out}");
            if reports_failure(&v) { ExitCode::from(EXIT_HTTP) } else { ExitCode::SUCCESS }
        }
        Err(Failure::NotRunning(m)) => { eprintln!("{m}"); ExitCode::from(EXIT_NOT_RUNNING) }
        Err(Failure::Http(m)) => { eprintln!("{m}"); ExitCode::from(EXIT_HTTP) }
        Err(Failure::Usage(m)) => { eprintln!("{m}"); ExitCode::from(2) }
        Err(Failure::Auth(m)) => { eprintln!("{m}"); ExitCode::from(EXIT_AUTH) }
        Err(Failure::Refused(m)) => { eprintln!("{m}"); ExitCode::from(EXIT_REFUSED) }
    }
}
