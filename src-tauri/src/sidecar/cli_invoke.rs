//! `/cli/commands` and `/cli/invoke`: one dispatch table that lets the CLI
//! (and, later, MCP) call the same Rust functions the UI calls over Tauri IPC.
//!
//! * Command names are the IPC command names, so a parity check against
//!   `generate_handler!` can match them one to one.
//! * Every command carries a danger tier. `destructive` commands are refused
//!   with 428 unless the request says `"confirm": true`. The check happens
//!   before the args are parsed or anything runs.
//! * Parsing (`parse_call`) is separate from execution (`execute`), so the
//!   manifest-vs-args agreement can be tested without launching anything.
//! * Only these routes are reachable through `cli_auth`'s token middleware.

use axum::{http::StatusCode, response::IntoResponse, response::Response, Json};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::OnceLock;

static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

/// Called from `lib.rs` setup before the sidecar is spawned.
pub fn set_app_handle(handle: tauri::AppHandle) {
    let _ = APP.set(handle);
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    /// No side effects.
    Read,
    /// Launches, focuses or opens something. Nothing is lost.
    Act,
    /// Closes, kills, or runs an arbitrary command. Needs `confirm: true`.
    Destructive,
}

#[derive(Serialize, Debug)]
pub struct ArgSpec {
    pub name: &'static str,
    /// `string`, `u32`, `i64`, `usize` or `string[]`.
    #[serde(rename = "type")]
    pub ty: &'static str,
    pub required: bool,
}

#[derive(Serialize, Debug)]
pub struct CommandSpec {
    pub name: &'static str,
    pub tier: Tier,
    pub summary: &'static str,
    pub args: &'static [ArgSpec],
}

const fn req(name: &'static str, ty: &'static str) -> ArgSpec {
    ArgSpec { name, ty, required: true }
}
const fn opt(name: &'static str, ty: &'static str) -> ArgSpec {
    ArgSpec { name, ty, required: false }
}

pub const COMMANDS: &[CommandSpec] = &[
    // ── read ──
    CommandSpec { name: "get_running_apps", tier: Tier::Read, summary: "Running apps with their windows", args: &[] },
    CommandSpec { name: "get_installed_apps", tier: Tier::Read, summary: "Installed and running apps", args: &[] },
    CommandSpec { name: "get_focused_app", tier: Tier::Read, summary: "The app that has OS focus", args: &[] },
    CommandSpec { name: "app_windows_list", tier: Tier::Read, summary: "CoolDesk's own windows", args: &[] },
    CommandSpec { name: "list_window_tabs", tier: Tier::Read, summary: "Tabs of a native window (UI Automation)", args: &[req("hwnd", "i64")] },
    // ── act ──
    CommandSpec { name: "launch_app", tier: Tier::Act, summary: "Launch an app by path, as Explorer would", args: &[req("path", "string")] },
    CommandSpec { name: "launch_app_with_args", tier: Tier::Act, summary: "Launch an executable with arguments", args: &[req("app", "string"), req("args", "string[]")] },
    CommandSpec { name: "open_url", tier: Tier::Act, summary: "Open an http(s), mailto or ms-settings link", args: &[req("url", "string")] },
    CommandSpec { name: "open_folder", tier: Tier::Act, summary: "Open a folder in the file manager", args: &[req("path", "string")] },
    CommandSpec { name: "focus_window", tier: Tier::Act, summary: "Bring an app's window to the front", args: &[req("pid", "u32"), opt("name", "string"), opt("hwnd", "i64"), opt("path", "string")] },
    CommandSpec { name: "focus_window_tab", tier: Tier::Act, summary: "Select a tab inside a native window", args: &[req("hwnd", "i64"), opt("index", "usize"), opt("title", "string")] },
    CommandSpec { name: "app_window_focus", tier: Tier::Act, summary: "Show and focus a CoolDesk window by label", args: &[req("label", "string")] },
    CommandSpec { name: "main_window_hide", tier: Tier::Act, summary: "Hide the CoolDesk main window", args: &[] },
    CommandSpec { name: "open_new_window", tier: Tier::Act, summary: "Open another CoolDesk window", args: &[] },
    // ── destructive ──
    CommandSpec { name: "close_app", tier: Tier::Destructive, summary: "Ask an app's window to close (WM_CLOSE; it may prompt to save)", args: &[req("pid", "u32"), opt("hwnd", "i64")] },
    CommandSpec { name: "app_window_close", tier: Tier::Destructive, summary: "Close a CoolDesk window (main is hidden instead)", args: &[req("label", "string")] },
    CommandSpec { name: "run_project_command", tier: Tier::Destructive, summary: "Run a shell command in a new console window", args: &[req("command", "string"), opt("cwd", "string")] },
];

/// IPC commands deliberately not reachable from the CLI, with the reason.
/// Read by the P3 parity gate.
pub const CLI_EXCLUDED: &[(&str, &str)] = &[
    ("app_window_set_space", "needs the calling webview's Window; UI-only"),
];

pub fn spec(name: &str) -> Option<&'static CommandSpec> {
    COMMANDS.iter().find(|c| c.name == name)
}

// ── typed args ──

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct NoArgs {}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Hwnd { pub hwnd: i64 }

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct PathArg { pub path: String }

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct LaunchWithArgs { pub app: String, pub args: Vec<String> }

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct UrlArg { pub url: String }

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct FocusWindow { pub pid: u32, pub name: Option<String>, pub hwnd: Option<i64>, pub path: Option<String> }

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct FocusTab { pub hwnd: i64, pub index: Option<usize>, pub title: Option<String> }

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Label { pub label: String }

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct CloseApp { pub pid: u32, pub hwnd: Option<i64> }

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct RunCommand { pub command: String, pub cwd: Option<String> }

#[derive(Debug)]
pub enum Call {
    GetRunningApps,
    GetInstalledApps,
    GetFocusedApp,
    AppWindowsList,
    ListWindowTabs(Hwnd),
    LaunchApp(PathArg),
    LaunchAppWithArgs(LaunchWithArgs),
    OpenUrl(UrlArg),
    OpenFolder(PathArg),
    FocusWindow(FocusWindow),
    FocusWindowTab(FocusTab),
    AppWindowFocus(Label),
    MainWindowHide,
    OpenNewWindow,
    CloseApp(CloseApp),
    AppWindowClose(Label),
    RunProjectCommand(RunCommand),
}

#[derive(Debug, PartialEq)]
pub enum InvokeError {
    Unknown(String),
    BadArgs(String),
    ConfirmRequired(&'static str),
    NoApp,
    Failed(String),
}

impl IntoResponse for InvokeError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            InvokeError::Unknown(n) => (StatusCode::NOT_FOUND, json!({ "ok": false, "error": format!("unknown command '{n}'; GET /cli/commands lists them") })),
            InvokeError::BadArgs(e) => (StatusCode::BAD_REQUEST, json!({ "ok": false, "error": format!("bad args: {e}") })),
            InvokeError::ConfirmRequired(n) => (
                StatusCode::PRECONDITION_REQUIRED,
                json!({ "ok": false, "tier": "destructive", "error": format!("'{n}' is destructive; resend with \"confirm\": true") }),
            ),
            InvokeError::NoApp => (StatusCode::SERVICE_UNAVAILABLE, json!({ "ok": false, "error": "app handle not ready" })),
            InvokeError::Failed(e) => (StatusCode::UNPROCESSABLE_ENTITY, json!({ "ok": false, "error": e })),
        };
        (status, Json(body)).into_response()
    }
}

fn parse<T: DeserializeOwned>(args: Value) -> Result<T, InvokeError> {
    let args = if args.is_null() { json!({}) } else { args };
    serde_json::from_value(args).map_err(|e| InvokeError::BadArgs(e.to_string()))
}

/// Turn a command name and JSON args into a typed call. Runs nothing.
pub fn parse_call(name: &str, args: Value) -> Result<Call, InvokeError> {
    Ok(match name {
        "get_running_apps" => { parse::<NoArgs>(args)?; Call::GetRunningApps }
        "get_installed_apps" => { parse::<NoArgs>(args)?; Call::GetInstalledApps }
        "get_focused_app" => { parse::<NoArgs>(args)?; Call::GetFocusedApp }
        "app_windows_list" => { parse::<NoArgs>(args)?; Call::AppWindowsList }
        "list_window_tabs" => Call::ListWindowTabs(parse(args)?),
        "launch_app" => Call::LaunchApp(parse(args)?),
        "launch_app_with_args" => Call::LaunchAppWithArgs(parse(args)?),
        "open_url" => Call::OpenUrl(parse(args)?),
        "open_folder" => Call::OpenFolder(parse(args)?),
        "focus_window" => Call::FocusWindow(parse(args)?),
        "focus_window_tab" => Call::FocusWindowTab(parse(args)?),
        "app_window_focus" => Call::AppWindowFocus(parse(args)?),
        "main_window_hide" => { parse::<NoArgs>(args)?; Call::MainWindowHide }
        "open_new_window" => { parse::<NoArgs>(args)?; Call::OpenNewWindow }
        "close_app" => Call::CloseApp(parse(args)?),
        "app_window_close" => Call::AppWindowClose(parse(args)?),
        "run_project_command" => Call::RunProjectCommand(parse(args)?),
        other => return Err(InvokeError::Unknown(other.to_string())),
    })
}

fn app() -> Result<tauri::AppHandle, InvokeError> {
    APP.get().cloned().ok_or(InvokeError::NoApp)
}

fn to_value<T: Serialize>(v: T) -> Result<Value, InvokeError> {
    serde_json::to_value(v).map_err(|e| InvokeError::Failed(e.to_string()))
}

fn unit(r: Result<(), String>) -> Result<Value, InvokeError> {
    r.map(|()| Value::Null).map_err(InvokeError::Failed)
}

/// Run a parsed call through the same function the UI's IPC command uses.
pub async fn execute(call: Call) -> Result<Value, InvokeError> {
    match call {
        Call::GetRunningApps => crate::get_running_apps(app()?).await.map_err(InvokeError::Failed),
        Call::GetInstalledApps => crate::get_installed_apps(app()?).await.map_err(InvokeError::Failed),
        Call::GetFocusedApp => to_value(crate::get_focused_app().await),
        Call::AppWindowsList => to_value(crate::app_windows_list(app()?)),
        Call::ListWindowTabs(a) => to_value(crate::list_window_tabs(a.hwnd).await),
        Call::LaunchApp(a) => unit(crate::launch_app(a.path).await),
        Call::LaunchAppWithArgs(a) => unit(crate::launch_app_with_args(a.app, a.args).await),
        Call::OpenUrl(a) => unit(crate::open_url(a.url).await),
        Call::OpenFolder(a) => unit(crate::open_folder(a.path).await),
        Call::FocusWindow(a) => unit(crate::focus_window(app()?, a.pid, a.name, a.hwnd, a.path).await),
        Call::FocusWindowTab(a) => unit(crate::focus_window_tab(a.hwnd, a.index, a.title).await),
        Call::AppWindowFocus(a) => unit(crate::app_window_focus(app()?, a.label)),
        Call::MainWindowHide => {
            crate::main_window_hide(app()?);
            Ok(Value::Null)
        }
        Call::OpenNewWindow => unit(crate::open_new_window(app()?)),
        Call::CloseApp(a) => unit(crate::close_app(app()?, a.pid, a.hwnd).await),
        Call::AppWindowClose(a) => unit(crate::app_window_close(app()?, a.label)),
        Call::RunProjectCommand(a) => unit(crate::run_project_command(a.command, a.cwd).await),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvokeRequest {
    pub cmd: String,
    #[serde(default)]
    pub args: Value,
    #[serde(default)]
    pub confirm: bool,
}

/// Tier gate, then parse, then run. The gate comes first, so an unconfirmed
/// destructive call never reaches its function.
pub async fn invoke(req: InvokeRequest) -> Result<Value, InvokeError> {
    let spec = spec(&req.cmd).ok_or_else(|| InvokeError::Unknown(req.cmd.clone()))?;
    if spec.tier == Tier::Destructive && !req.confirm {
        return Err(InvokeError::ConfirmRequired(spec.name));
    }
    let call = parse_call(spec.name, req.args)?;
    log::info!("[cli] invoke {} ({:?})", spec.name, spec.tier);
    execute(call).await
}

pub async fn commands_handler() -> Json<Value> {
    Json(json!({ "ok": true, "commands": COMMANDS, "excluded": CLI_EXCLUDED.iter().map(|(n, r)| json!({ "name": n, "reason": r })).collect::<Vec<_>>() }))
}

pub async fn invoke_handler(body: Result<Json<InvokeRequest>, axum::extract::rejection::JsonRejection>) -> Response {
    let Json(req) = match body {
        Ok(b) => b,
        Err(e) => return InvokeError::BadArgs(e.body_text()).into_response(),
    };
    let cmd = req.cmd.clone();
    match invoke(req).await {
        Ok(result) => Json(json!({ "ok": true, "cmd": cmd, "result": result })).into_response(),
        Err(e) => e.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(ty: &str) -> Value {
        match ty {
            "string" => json!("x"),
            "u32" | "usize" => json!(1),
            "i64" => json!(1),
            "string[]" => json!(["a"]),
            other => panic!("manifest uses undeclared type '{other}'"),
        }
    }

    fn full_args(c: &CommandSpec) -> Value {
        Value::Object(c.args.iter().map(|a| (a.name.to_string(), sample(a.ty))).collect())
    }

    #[test]
    fn names_are_unique_and_not_excluded() {
        let mut seen = std::collections::HashSet::new();
        for c in COMMANDS {
            assert!(seen.insert(c.name), "duplicate command {}", c.name);
            assert!(!CLI_EXCLUDED.iter().any(|(n, _)| *n == c.name), "{} is both exposed and excluded", c.name);
        }
    }

    #[test]
    fn every_manifest_entry_parses_from_its_declared_args() {
        for c in COMMANDS {
            parse_call(c.name, full_args(c)).unwrap_or_else(|e| panic!("{}: declared args rejected: {e:?}", c.name));
            // required-only also parses
            let required: serde_json::Map<_, _> =
                c.args.iter().filter(|a| a.required).map(|a| (a.name.to_string(), sample(a.ty))).collect();
            parse_call(c.name, Value::Object(required)).unwrap_or_else(|e| panic!("{}: required-only rejected: {e:?}", c.name));
        }
    }

    #[tokio::test]
    async fn invoke_only_reaches_names_in_the_manifest() {
        // invoke() looks up the spec before parse_call, so a parser arm with no
        // manifest entry is unreachable: it can't bypass the tier gate.
        let r = invoke(InvokeRequest { cmd: "kill_process".into(), args: json!({}), confirm: true }).await;
        assert_eq!(r, Err(InvokeError::Unknown("kill_process".into())));
    }

    #[test]
    fn missing_required_arg_is_rejected() {
        for c in COMMANDS.iter().filter(|c| c.args.iter().any(|a| a.required)) {
            assert!(matches!(parse_call(c.name, json!({})), Err(InvokeError::BadArgs(_))), "{} accepted empty args", c.name);
        }
    }

    #[test]
    fn unknown_field_is_rejected() {
        for c in COMMANDS {
            let mut a = full_args(c);
            a.as_object_mut().unwrap().insert("zzz".into(), json!(1));
            assert!(matches!(parse_call(c.name, a), Err(InvokeError::BadArgs(_))), "{} accepted an unknown field", c.name);
        }
    }

    #[test]
    fn null_args_mean_empty() {
        assert!(parse_call("get_focused_app", Value::Null).is_ok());
    }

    #[tokio::test]
    async fn destructive_without_confirm_is_refused_before_running() {
        for c in COMMANDS.iter().filter(|c| c.tier == Tier::Destructive) {
            // Bad args too: the refusal must come from the tier gate, not from parsing.
            let r = invoke(InvokeRequest { cmd: c.name.into(), args: json!({ "zzz": 1 }), confirm: false }).await;
            assert_eq!(r, Err(InvokeError::ConfirmRequired(c.name)));
        }
    }

    #[tokio::test]
    async fn destructive_with_confirm_reaches_parsing() {
        let r = invoke(InvokeRequest { cmd: "close_app".into(), args: json!({}), confirm: true }).await;
        assert!(matches!(r, Err(InvokeError::BadArgs(_))));
    }

    #[test]
    fn tiers_are_what_the_proposal_says() {
        for n in ["close_app", "app_window_close", "run_project_command"] {
            assert_eq!(spec(n).unwrap().tier, Tier::Destructive, "{n}");
        }
        for c in COMMANDS.iter().filter(|c| c.name.starts_with("get_") || c.name.contains("list")) {
            assert_eq!(c.tier, Tier::Read, "{}", c.name);
        }
    }
}
