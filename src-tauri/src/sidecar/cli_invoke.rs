//! `/cli/commands` and `/cli/invoke`: one dispatch table that lets the CLI
//! (and, later, MCP) call the same Rust functions the UI calls over Tauri IPC.
//!
//! * Each command is declared ONCE in the `cli_commands!` table below: name,
//!   tier, summary, typed args, call. The manifest's arg list, the
//!   `deny_unknown_fields` struct the request is parsed into, and the dispatch
//!   arm are all generated from that one declaration, so they can't drift.
//! * Command names are the IPC command names. `CLI_EXCLUDED` names every IPC
//!   command left out, with the reason, and the parity test fails if a command
//!   in `generate_handler!` is in neither list.
//! * Arg names are the Rust parameter names (snake_case). Tauri's JS bridge
//!   uses camelCase, and the CLI doesn't go through that bridge.
//! * Tiers: `destructive` and `secret` commands are refused with 428 unless the
//!   request says `"confirm": true`. The check happens before the args are
//!   parsed or anything runs. `secret` (the vault) will also be kept out of MCP.
//! * Only reachable through `cli_auth`'s token middleware.

use axum::{http::StatusCode, response::IntoResponse, response::Response, Json};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{LazyLock, OnceLock};

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
    /// Launches, focuses, opens, adds or toggles something. Nothing is lost.
    Act,
    /// Closes, kills, deletes, runs arbitrary input, or changes a security
    /// setting. Needs `confirm: true`.
    Destructive,
    /// Touches the password vault. Needs `confirm: true`. Kept out of MCP.
    Secret,
}

impl Tier {
    pub fn needs_confirm(self) -> bool {
        matches!(self, Tier::Destructive | Tier::Secret)
    }
}

#[derive(Serialize, Debug)]
pub struct ArgSpec {
    pub name: &'static str,
    /// `string`, `string[]`, `bool`, `u16`, `u32`, `usize`, `i32`, `i64`, `f64`,
    /// `json` (any JSON value) or `object` (a structured object; see the summary).
    #[serde(rename = "type")]
    pub ty: String,
    pub required: bool,
}

#[derive(Serialize, Debug)]
pub struct CommandSpec {
    pub name: &'static str,
    pub tier: Tier,
    pub summary: &'static str,
    pub args: Vec<ArgSpec>,
}

/// Derive the manifest entry for one arg from its Rust type, as written in the table.
fn arg_spec(name: &'static str, rust_ty: &str) -> ArgSpec {
    let t: String = rust_ty.chars().filter(|c| !c.is_whitespace()).collect();
    let (required, inner) = match t.strip_prefix("Option<").and_then(|s| s.strip_suffix('>')) {
        Some(inner) => (false, inner.to_string()),
        None => (true, t),
    };
    let ty = match inner.as_str() {
        "String" => "string".into(),
        "Vec<String>" => "string[]".into(),
        "bool" | "u16" | "u32" | "usize" | "i32" | "i64" | "f64" => inner,
        "Value" | "serde_json::Value" => "json".into(),
        _ => "object".into(),
    };
    ArgSpec { name, ty, required }
}

#[derive(Debug, PartialEq)]
pub enum InvokeError {
    Unknown(String),
    BadArgs(String),
    ConfirmRequired(&'static str, Tier),
    NoApp,
    Failed(String),
}

impl IntoResponse for InvokeError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            InvokeError::Unknown(n) => (StatusCode::NOT_FOUND, json!({ "ok": false, "error": format!("unknown command '{n}'; GET /cli/commands lists them") })),
            InvokeError::BadArgs(e) => (StatusCode::BAD_REQUEST, json!({ "ok": false, "error": format!("bad args: {e}") })),
            InvokeError::ConfirmRequired(n, tier) => (
                StatusCode::PRECONDITION_REQUIRED,
                json!({ "ok": false, "tier": tier, "error": format!("'{n}' is {}; resend with \"confirm\": true", serde_json::to_value(tier).unwrap().as_str().unwrap_or("")) }),
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

fn app() -> Result<tauri::AppHandle, InvokeError> {
    APP.get().cloned().ok_or(InvokeError::NoApp)
}

/// A plain return value.
fn val<T: Serialize>(v: T) -> Result<Value, InvokeError> {
    serde_json::to_value(v).map_err(|e| InvokeError::Failed(e.to_string()))
}

/// A `Result<T, String>` return value; `Err` becomes 422.
fn res<T: Serialize>(r: Result<T, String>) -> Result<Value, InvokeError> {
    r.map_err(InvokeError::Failed).and_then(val)
}

macro_rules! cli_commands {
    ($( $name:ident : $tier:ident , $summary:literal , { $($arg:ident : $ty:ty),* $(,)? } => |$a:ident| $body:expr ; )*) => {
        static SPECS: LazyLock<Vec<CommandSpec>> = LazyLock::new(|| vec![
            $( CommandSpec {
                name: stringify!($name),
                tier: Tier::$tier,
                summary: $summary,
                args: vec![ $( arg_spec(stringify!($arg), stringify!($ty)) ),* ],
            }, )*
        ]);

        /// Parse `args` for `name` without running anything.
        pub fn check_args(name: &str, args: Value) -> Result<(), InvokeError> {
            $( if name == stringify!($name) {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                #[allow(dead_code)]
                struct A { $( $arg: $ty ),* }
                parse::<A>(args)?;
                return Ok(());
            } )*
            Err(InvokeError::Unknown(name.to_string()))
        }

        async fn dispatch(name: &str, args: Value) -> Result<Value, InvokeError> {
            $( if name == stringify!($name) {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct A { $( $arg: $ty ),* }
                #[allow(unused_variables)]
                let $a: A = parse(args)?;
                return $body;
            } )*
            Err(InvokeError::Unknown(name.to_string()))
        }
    };
}

cli_commands! {
    // ── P2.1 launcher and windows ──
    get_running_apps: Read, "Running apps with their windows", {} => |a| res(crate::get_running_apps(app()?).await);
    get_installed_apps: Read, "Installed and running apps", {} => |a| res(crate::get_installed_apps(app()?).await);
    get_focused_app: Read, "The app that has OS focus", {} => |a| val(crate::get_focused_app().await);
    app_windows_list: Read, "CoolDesk's own windows", {} => |a| val(crate::app_windows_list(app()?));
    list_window_tabs: Read, "Tabs of a native window (UI Automation)", { hwnd: i64 } => |a| val(crate::list_window_tabs(a.hwnd).await);
    launch_app: Act, "Launch an app by path, as Explorer would", { path: String } => |a| res(crate::launch_app(a.path).await);
    launch_app_with_args: Act, "Launch an executable with arguments", { app: String, args: Vec<String> } => |a| res(crate::launch_app_with_args(a.app, a.args).await);
    open_url: Act, "Open an http(s), mailto or ms-settings link", { url: String } => |a| res(crate::open_url(a.url).await);
    open_folder: Act, "Open a folder in the file manager", { path: String } => |a| res(crate::open_folder(a.path).await);
    focus_window: Act, "Bring an app's window to the front", { pid: u32, name: Option<String>, hwnd: Option<i64>, path: Option<String> } => |a| res(crate::focus_window(app()?, a.pid, a.name, a.hwnd, a.path).await);
    focus_window_tab: Act, "Select a tab inside a native window", { hwnd: i64, index: Option<usize>, title: Option<String> } => |a| res(crate::focus_window_tab(a.hwnd, a.index, a.title).await);
    app_window_focus: Act, "Show and focus a CoolDesk window by label", { label: String } => |a| res(crate::app_window_focus(app()?, a.label));
    main_window_hide: Act, "Hide the CoolDesk main window", {} => |a| val(crate::main_window_hide(app()?));
    open_new_window: Act, "Open another CoolDesk window", {} => |a| res(crate::open_new_window(app()?));
    close_app: Destructive, "Ask an app's window to close (WM_CLOSE; it may prompt to save)", { pid: u32, hwnd: Option<i64> } => |a| res(crate::close_app(app()?, a.pid, a.hwnd).await);
    app_window_close: Destructive, "Close a CoolDesk window (main is hidden instead)", { label: String } => |a| res(crate::app_window_close(app()?, a.label));
    run_project_command: Destructive, "Run a shell command in a new console window", { command: String, cwd: Option<String> } => |a| res(crate::run_project_command(a.command, a.cwd).await);

    // ── P2.2 search and folders ──
    web_suggest: Read, "Search-engine autocomplete suggestions (network)", { query: String, engine: Option<String> } => |a| val(crate::web_suggest(a.query, a.engine).await);
    categorize_app: Read, "Category CoolDesk assigns to an app", { name: String, path: String } => |a| val(crate::categorize_app(a.name, a.path));
    folder_display_name: Read, "Display name for a folder path", { path: String } => |a| val(crate::folder_display_name(a.path));
    folder_identity: Read, "What kind of project a folder is (repo, language, icon)", { path: String } => |a| val(crate::folder_identity(a.path).await);
    get_frequent_folders: Read, "Frequently used folders", {} => |a| val(crate::get_frequent_folders().await);
    get_user_places: Read, "User places (Desktop, Documents, Downloads, …)", {} => |a| val(crate::get_user_places().await);
    search_files: Read, "Search files in the indexed folders", { query: String } => |a| res(crate::search_files(app()?, a.query).await);
    list_dir: Read, "List a directory", { path: String } => |a| res(crate::list_dir(a.path).await);
    path_kinds: Read, "File, folder or missing, for each path", { paths: Vec<String> } => |a| val(crate::path_kinds(a.paths));
    classify_project_folders: Read, "Whether each path looks like a project folder", { paths: Vec<String> } => |a| val(crate::classify_project_folders(a.paths));
    folder_index_list: Read, "Folders in the file-search index", {} => |a| val(crate::folder_index::folder_index_list(app()?));
    folder_index_add: Act, "Add a folder to the file-search index", { path: String } => |a| res(crate::folder_index::folder_index_add(app()?, a.path));
    folder_index_set_options: Act, "Change an indexed folder's options", { path: String, enabled: Option<bool>, auto_reindex_minutes: Option<u32>, exclude: Option<Vec<String>>, include_apps: Option<bool> } => |a| res(crate::folder_index::folder_index_set_options(app()?, a.path, a.enabled, a.auto_reindex_minutes, a.exclude, a.include_apps));
    folder_index_reindex: Act, "Re-index one folder", { path: String } => |a| res(crate::folder_index::folder_index_reindex(app()?, a.path).await);
    folder_index_reindex_all: Act, "Re-index every folder", {} => |a| val(crate::folder_index::folder_index_reindex_all(app()?).await);
    folder_index_remove: Destructive, "Remove a folder from the file-search index", { path: String } => |a| val(crate::folder_index::folder_index_remove(app()?, a.path));
    trust_project_dir: Destructive, "Pre-trust a folder for Claude Code (writes its trust config)", { path: String } => |a| res(crate::trust_project_dir(a.path));
    preview_text_file: Read, "Text preview of a file", { path: String } => |a| res(crate::preview::preview_text_file(a.path).await);
    preview_image_file: Read, "Image preview (data URL) of a file", { path: String } => |a| res(crate::preview::preview_image_file(a.path).await);
    preview_pdf_file: Read, "PDF preview of a file", { path: String } => |a| res(crate::preview::preview_pdf_file(a.path).await);
    archive_list: Read, "List an archive's entries", { path: String } => |a| res(crate::archive::archive_list(a.path).await);
    archive_extract: Act, "Extract an archive next to itself; returns the folder", { path: String } => |a| res(crate::archive::archive_extract(a.path).await);
    archive_extract_entry: Act, "Extract one archive entry to a temp file; returns its path", { path: String, entry: String } => |a| res(crate::archive::archive_extract_entry(app()?, a.path, a.entry).await);
    archive_compress: Act, "Zip paths next to the first one; returns the zip path", { paths: Vec<String> } => |a| res(crate::archive::archive_compress(a.paths).await);

    // ── P2.3 spotlight and dock ──
    toggle_spotlight: Act, "Show or hide Spotlight", {} => |a| val(crate::toggle_spotlight(app()?));
    hide_spotlight: Act, "Hide Spotlight", {} => |a| val(crate::hide_spotlight(app()?));
    set_spotlight_shortcut: Act, "Change Spotlight's global shortcut, e.g. \"Alt+Space\"", { shortcut: String } => |a| res(crate::set_spotlight_shortcut(app()?, a.shortcut));
    spotlight_run_in_terminal: Destructive, "Open Spotlight's terminal and run a command in it", { command: String } => |a| res(crate::spotlight_run_in_terminal(app()?, a.command));
    dock_get_state: Read, "State of the docked surfaces (sidebar, bar)", {} => |a| val(crate::surfaces::dock_get_state(app()?));
    dock_enable: Act, "Dock a surface to a screen edge", { surface: String, side: Option<String>, size: Option<u32>, mode: Option<String>, expand: Option<bool> } => |a| res(crate::surfaces::dock_enable(app()?, a.surface, a.side, a.size, a.mode, a.expand).await);
    dock_disable: Act, "Undock a surface", { surface: String } => |a| res(crate::surfaces::dock_disable(app()?, a.surface).await);
    dock_expand: Act, "Expand a docked surface", { surface: String } => |a| res(crate::surfaces::dock_expand(app()?, a.surface).await);
    dock_collapse: Act, "Collapse a docked surface", { surface: String } => |a| res(crate::surfaces::dock_collapse(app()?, a.surface));
    dock_set_size: Act, "Set a docked surface's size in pixels", { surface: String, size: u32 } => |a| res(crate::surfaces::dock_set_size(app()?, a.surface, a.size).await);
    dock_open_main: Act, "Open the main window from the dock", { from: Option<String> } => |a| val(crate::surfaces::dock_open_main(app()?, a.from).await);

    // ── P2.4 shelf ──
    shelf_list: Read, "Items on the shelf", {} => |a| res(crate::shelf::shelf_list(app()?));
    shelf_add_text: Act, "Put text on the shelf", { text: String, name: Option<String> } => |a| res(crate::shelf::shelf_add_text(app()?, a.text, a.name));
    shelf_add_bytes: Act, "Put a file on the shelf from base64 data", { name: String, data: String, source: Option<String> } => |a| res(crate::shelf::shelf_add_bytes(app()?, a.name, a.data, a.source));
    shelf_import: Act, "Copy files onto the shelf", { paths: Vec<String> } => |a| res(crate::shelf::shelf_import(app()?, a.paths));
    shelf_pin: Act, "Pin or unpin a shelf item", { id: String, pinned: bool } => |a| res(crate::shelf::shelf_pin(app()?, a.id, a.pinned));
    shelf_save_to: Act, "Move a shelf item into a folder (default Downloads); returns its path", { id: String, dest_dir: Option<String> } => |a| res(crate::shelf::shelf_save_to(app()?, a.id, a.dest_dir));
    shelf_remove: Destructive, "Delete a shelf item", { id: String } => |a| res(crate::shelf::shelf_remove(app()?, a.id));
    shelf_clear: Destructive, "Delete all shelf items (pinned too if include_pinned)", { include_pinned: bool } => |a| res(crate::shelf::shelf_clear(app()?, a.include_pinned));
}

/// IPC commands deliberately not reachable from the CLI, with the reason.
/// The parity test checks this list plus the table against `generate_handler!`.
pub const CLI_EXCLUDED: &[(&str, &str)] = &[
    ("app_window_set_space", "needs the calling webview's Window; UI-only"),
    ("watch_dir", "pushes change events to the calling webview; use `cooldesk watch` for state events"),
    ("unwatch_dir", "pairs with watch_dir"),
    ("open_spotlight_edit", "hands a workspace from one window to Spotlight's editor; UI handoff"),
    ("take_spotlight_edit", "Spotlight's side of that handoff queue"),
    ("take_terminal_run", "Spotlight's side of the run-in-terminal handoff; use spotlight_run_in_terminal"),
    ("dock_resize", "live drag resize driven by pointer events; use dock_set_size"),
    ("dock_set_bar_content_width", "layout measurement reported by the bar's own webview"),
];

pub fn specs() -> &'static [CommandSpec] {
    &SPECS
}

pub fn spec(name: &str) -> Option<&'static CommandSpec> {
    SPECS.iter().find(|c| c.name == name)
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
/// destructive or secret call never reaches its function.
pub async fn invoke(req: InvokeRequest) -> Result<Value, InvokeError> {
    let spec = spec(&req.cmd).ok_or_else(|| InvokeError::Unknown(req.cmd.clone()))?;
    if spec.tier.needs_confirm() && !req.confirm {
        return Err(InvokeError::ConfirmRequired(spec.name, spec.tier));
    }
    // Name only. Args can hold secrets (vault master, clipboard text).
    log::info!("[cli] invoke {} ({:?})", spec.name, spec.tier);
    dispatch(spec.name, req.args).await
}

pub async fn commands_handler() -> Json<Value> {
    Json(json!({
        "ok": true,
        "commands": specs(),
        "excluded": CLI_EXCLUDED.iter().map(|(n, r)| json!({ "name": n, "reason": r })).collect::<Vec<_>>(),
    }))
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

    /// Commands whose args include a structured object. They can't be
    /// sampled generically, so they're listed here; a new one fails the test until
    /// it's added.
    const OBJECT_ARG_COMMANDS: &[&str] = &[];

    fn sample(ty: &str) -> Option<Value> {
        Some(match ty {
            "string" => json!("x"),
            "string[]" => json!(["a"]),
            "bool" => json!(true),
            "u16" | "u32" | "usize" | "i32" | "i64" => json!(1),
            "f64" => json!(1.5),
            "json" => json!({}),
            "object" => return None,
            other => panic!("manifest uses undeclared type '{other}'"),
        })
    }

    fn args_from(c: &CommandSpec, only_required: bool) -> Option<Value> {
        let mut m = serde_json::Map::new();
        for a in c.args.iter().filter(|a| a.required || !only_required) {
            m.insert(a.name.to_string(), sample(&a.ty)?);
        }
        Some(Value::Object(m))
    }

    #[test]
    fn names_are_unique_and_not_excluded() {
        let mut seen = std::collections::HashSet::new();
        for c in specs() {
            assert!(seen.insert(c.name), "duplicate command {}", c.name);
            assert!(!CLI_EXCLUDED.iter().any(|(n, _)| *n == c.name), "{} is both exposed and excluded", c.name);
        }
    }

    #[test]
    fn every_manifest_entry_parses_from_its_declared_args() {
        for c in specs() {
            match (args_from(c, false), args_from(c, true)) {
                (Some(full), Some(req)) => {
                    check_args(c.name, full).unwrap_or_else(|e| panic!("{}: declared args rejected: {e:?}", c.name));
                    check_args(c.name, req).unwrap_or_else(|e| panic!("{}: required-only rejected: {e:?}", c.name));
                }
                _ => assert!(OBJECT_ARG_COMMANDS.contains(&c.name), "{} has an object arg; add it to OBJECT_ARG_COMMANDS", c.name),
            }
        }
    }

    #[test]
    fn missing_required_arg_is_rejected() {
        for c in specs().iter().filter(|c| c.args.iter().any(|a| a.required)) {
            assert!(matches!(check_args(c.name, json!({})), Err(InvokeError::BadArgs(_))), "{} accepted empty args", c.name);
        }
    }

    #[test]
    fn unknown_field_is_rejected() {
        for c in specs() {
            let mut a = args_from(c, false).unwrap_or_else(|| json!({}));
            a.as_object_mut().unwrap().insert("zzz".into(), json!(1));
            assert!(matches!(check_args(c.name, a), Err(InvokeError::BadArgs(_))), "{} accepted an unknown field", c.name);
        }
    }

    #[test]
    fn null_args_mean_empty() {
        assert!(check_args("get_focused_app", Value::Null).is_ok());
    }

    #[test]
    fn arg_types_derive_from_rust_types() {
        let a = arg_spec("x", "Option < Vec < String > >");
        assert_eq!((a.ty.as_str(), a.required), ("string[]", false));
        let a = arg_spec("x", "u32");
        assert_eq!((a.ty.as_str(), a.required), ("u32", true));
        assert_eq!(arg_spec("x", "serde_json :: Value").ty, "json");
        assert_eq!(arg_spec("x", "crate :: vault :: EntryInput").ty, "object");
    }

    #[tokio::test]
    async fn confirm_tiers_are_refused_before_running() {
        for c in specs().iter().filter(|c| c.tier.needs_confirm()) {
            // Bad args too: the refusal must come from the tier gate, not from parsing.
            let r = invoke(InvokeRequest { cmd: c.name.into(), args: json!({ "zzz": 1 }), confirm: false }).await;
            assert_eq!(r, Err(InvokeError::ConfirmRequired(c.name, c.tier)));
        }
    }

    #[tokio::test]
    async fn confirmed_call_reaches_parsing() {
        let r = invoke(InvokeRequest { cmd: "close_app".into(), args: json!({}), confirm: true }).await;
        assert!(matches!(r, Err(InvokeError::BadArgs(_))));
    }

    #[tokio::test]
    async fn invoke_only_reaches_names_in_the_manifest() {
        let r = invoke(InvokeRequest { cmd: "no_such_command".into(), args: json!({}), confirm: true }).await;
        assert_eq!(r, Err(InvokeError::Unknown("no_such_command".into())));
    }

    #[test]
    fn tiers_for_known_dangerous_commands() {
        for n in ["close_app", "app_window_close", "run_project_command", "folder_index_remove", "trust_project_dir"] {
            assert_eq!(spec(n).map(|s| s.tier), Some(Tier::Destructive), "{n}");
        }
    }
}
