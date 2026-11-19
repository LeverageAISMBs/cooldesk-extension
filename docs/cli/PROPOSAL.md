# CoolDesk CLI — proposal

2026-11-18 · repo `cooldesk-extension` @ master a266014f (v2.0.22, Apache-2.0, 76 commits in 30 days)

Evidence came from the indexes, not from reading files:
- code: `C:\Users\mikes\codebase-intel-toolkit\indexes\cooldesk-extension\codebase_intel_index.json`
- docs: `C:\Users\mikes\doc-intel\indexes\cooldesk-extension.db`

## 1. What the app is

CoolDesk is a Tauri 2 desktop app (Rust, `src-tauri/`) with a React frontend (`src/`) and a Chrome extension (`src/background/background.js`, `manifest.json`). It also has a Cloudflare worker for telemetry (`worker/src/index.js`, not relevant here). The repo has 553 files and 194k LOC.

## 2. Entry points

| Entry | File | What it is |
|---|---|---|
| Native process | `src-tauri/src/main.rs` → `lib.rs::run()` (line ~2985) | Tauri builder: plugins, ~140 IPC commands, `.setup()` |
| Sidecar server | `src-tauri/src/sidecar/server.rs::start_server` (called from `lib.rs:3195`) | axum HTTP + WebSocket on **127.0.0.1:4545**, running inside the app process |
| MCP endpoint | `src-tauri/src/sidecar/mcp.rs` (`POST /mcp`) | JSON-RPC, 7 **read-only** tools |
| Main UI | `src/app/main.jsx` → `App.jsx` | React app in the main webview |
| Spotlight UI | `src/app/spotlight-main.jsx` | launcher window (Alt+K global shortcut) |
| Dock/handle UI | `src/app/handle-main.js` | edge-dock panel |
| Extension UI | `src/app/extension-main.jsx` → `ExtensionApp.jsx` | extension new-tab page |
| Extension SW | `src/background/background.js` | tab grouping and daily notes. It is a WebSocket client of the sidecar. |
| Frontend↔sidecar shim | `src/app/electron-shim.js` (`sidecarPost`, `connectWebSocket`) | how the UI calls the sidecar |
| Second launch | `tauri_plugin_single_instance` in `run()` | handles only `--quit`. Any other args just show the main window. |

## 3. The control surfaces (this decides the design)

There are three ways in. Only the first two can be reached from outside the app today.

**A. Sidecar HTTP, 127.0.0.1:4545, ~90 routes. Reachable now.** It covers workspaces, urls, tabs, settings, activity (focused, visible, app-usage, site-usage), notes, url-notes, pins, daily-memory, ui-state, dashboard, graph, search (apps), browser-history, `/cmd/jump-to-tab`, `/cmd/close-tab`, `/sync`, `/llm/*` (v1/v2/v3), `/feedback/*`, `/cooldesk/*` (project linking), and `/local/servers`. The route table is at `server.rs:213-313`.
The origin gate is `server.rs:354-358`: `None => true, // Allow requests with no Origin (internal/native calls)`. A CLI sends no Origin, so it is allowed in.
**Verified live this session:** `GET /health` → `{"ok":true,...}`, `GET /workspaces` → `[]`, `GET /search?q=code` → results, and `POST /mcp tools/list` → 7 tools.

**B. Sidecar WebSocket, `/ws`. Reachable now.** Clients send `identify`, `request-state`, `request-tabs`, `push-*` (workspaces, urls, tabs, settings, notes, pins, ui-state, dashboard, activity …), `request-native-focus`, and `llm-*`. The server re-broadcasts state changes, which gives a CLI a live event stream for `watch` and `tail`.

**C. Tauri IPC, ~140 commands. Not reachable from outside.** This is everything a launcher actually *does*:
- launching: `launch_app`, `open_url`, `open_folder`, `run_project_command`
- windows: `focus_window`, `close_app`, `focus_window_tab`
- spotlight: `toggle_spotlight`, `hide_spotlight`
- system: `kill_process`, `list_listening_ports`
- files: `search_files`, `list_dir`, `folder_index_*`
- tools: `shelf_*`, `clipboard_*`, `vault_*`, `terminal_*`, `dock_*`, `browser_pane_*`, `archive_*`, `preview_*`, `ai_cli_*`, `highlights_*`
- maintenance: backups, autostart, updates

All of these can only be called from the app's own webviews. `guard_remote_webviews` (`lib.rs:2971`) also blocks `webapp-*` and browser-pane webviews.

**So:** the data plane is already scriptable over HTTP. The action plane (C) is the gap, and closing it is the real work.

## 4. Proposed architecture

```
cooldesk-cli (separate Rust binary, shares serde types with the sidecar)
   │  HTTP + WS to 127.0.0.1:4545, Bearer token
   ▼
sidecar (axum, in the app process)
   ├── existing ~90 data routes           (A, unchanged)
   ├── /ws event stream                   (B, unchanged)
   ├── NEW  GET  /cli/commands            → command manifest (name, args schema, danger tier)
   └── NEW  POST /cli/invoke {cmd,args}   → one dispatch table
                                              │
                                   core fns (no tauri::State in the signature)
                                              ▲
                     #[tauri::command] thin wrappers (UI still calls these)
```

Key decisions:

1. **Put a bridge in the sidecar. Don't drive the UI.** The sidecar already runs in the same process as the Tauri commands. The plan: give it the `AppHandle` (store it in a `OnceCell` before `start_server`), and expose one dispatch route over the same Rust functions. Not used: UI automation, keystroke injection, or emitting events for the React app to react to. Those are slow and brittle, and they fail silently.
2. **Split each command into a core function plus a thin wrapper, one module at a time.** Each `#[tauri::command]` becomes a wrapper around a core function that takes `&AppHandle` and plain args. The UI and the CLI then call the same code, and behaviour can't drift.
3. **The server holds the command manifest.** `cooldesk-cli` builds its subcommand tree from `GET /cli/commands` at runtime and caches it. A new command in the app shows up in the CLI with no CLI release.
4. **A parity gate that fails closed.** A test enumerates `generate_handler!` and fails if any command is neither in the CLI dispatch table nor in an explicit `CLI_EXCLUDED` list with a reason. Nothing can silently go missing.
5. **The MCP endpoint and the CLI share one dispatch table.** `mcp.rs` currently exposes 7 read-only tools. If its `tools/list` and `tools/call` come from the same manifest, Claude Code and other agents can drive CoolDesk with what the CLI can do, gated by the same tiers.
6. **Keep the CLI separate from `cooldesk.exe`.** Single-instance forwarding can't return output to the caller. The forwarded-args path is still useful for one fire-and-forget fallback, `cooldesk.exe --spotlight` when the sidecar is down.

## 5. Security — must land before the action plane

Today any local process can `POST` to every sidecar route with no credential (the `None => true` above). That's tolerable for notes and pins. It isn't tolerable once `/cli/invoke` can reach `kill_process`, `terminal_write`, `run_project_command`, or `vault_get`.

- At startup the app writes a random token to the app-data dir (user-only ACL). The CLI reads it and sends `Authorization: Bearer`. `/cli/*` rejects any request without it. (Extending this to the old routes means updating the extension and shim too, so that is a separate decision.)
- Every command has a **danger tier** in the manifest:
  - `read`: no confirmation
  - `act`: launch, focus, open
  - `destructive`: kill, delete, clear, close, needs `--yes`
  - `secret`: vault, needs the vault unlocked plus `--yes`. It is excluded from MCP by default.
- **Upstream note:** the no-Origin pass-through is worth reporting to the maintainers as a hardening issue whether or not we upstream the CLI.

## 6. CLI shape

```
cooldesk status | health | version
cooldesk ws list|show|create|update|delete      # /workspaces
cooldesk tabs list | jump <id> | close <id>     # /tabs, /cmd/*
cooldesk notes|pins|urls ...                    # data routes
cooldesk search apps <q> | history <q> | files <q>
cooldesk app launch <name> [--args] | focus | close | running | installed
cooldesk spotlight toggle|hide|shortcut <keys>
cooldesk shelf|clipboard|vault|dock|terminal|pane|folders|backup ...
cooldesk llm status|models|load|chat ...
cooldesk watch [--type push-tabs,...]           # NDJSON from /ws
cooldesk invoke <cmd> --json '{...}'            # raw escape hatch to /cli/invoke
cooldesk mcp tools|call <tool> ...
```

The default output is JSON, which suits agents. `--table` gives human-readable output. Exit codes are 0 for ok, 2 for usage, 3 when the app is not running, 4 for auth, and 5 when a command is refused by its tier. It also supports `--batch` (NDJSON on stdin) and shell completions generated from the manifest.

## 7. Phased plan

- [x] **P0 — CLI over what exists (no app changes).** Data routes, `/mcp`, `/ws watch`, `invoke` stub. Ships in days and covers about 40% of the functionality.
- [x] **P1 — Token auth on `/cli/*`, failing closed.** This is a prerequisite for P2.
- [x] **P2 — `AppHandle` into the sidecar, `/cli/commands` and `/cli/invoke`, and the core-function split, one module per PR, safest first:**
  1. launcher and windows — done, branch `cli/p2-launcher-windows` (9d9378a)
  2. search and folders — done (ff2c1a9)
  3. spotlight and dock — done (b818473)
  4. shelf — done (8f68fa0)
  5. clipboard — done (347349a)
  6. browser_pane — done (b47d838)
  7. terminal — done (88b8186)
  8. processes and updates — done (28f130e)
  9. vault, last — done (5334676)
- [x] **P3 — done (b5a337f, 123 exposed + 17 excluded). The parity gate test.** `CLI_EXCLUDED` holds things that are UI-only by nature, such as `take_spotlight_edit` and `webapp_embed_set_bounds`.
- [x] **P4 — MCP backed by the same manifest.** Done as a stdio server in the CLI (`cooldesk mcp serve`), not the app's unauthenticated `/mcp`; registered user-scope in Claude Code. Register it with Claude Code so agents can drive CoolDesk.
- [ ] **P5 — Completions, `--batch`, and docs generated from the manifest.**

**Fork strategy:** upstream is very active. Keep the CLI as its own crate, and keep app-side changes small (one route file plus the wrappers). Rebase on a branch, and offer P1, P2 and P4 upstream, since they help the project too.

## 8. Open questions and unverified items

- Does the sidecar start when the app launches headless via autostart? The `start_server` call is inside `.setup()` at `lib.rs:3195`, so probably yes. Not tested.
- `/cmd/jump-to-tab` and `close-tab` need the extension connected over WS. The CLI should report "no browser client" instead of hanging.
- Some IPC commands probably depend on webview state, such as `browser_pane_*` bounds tied to a window's layout. Each one gets classified during P2. Expect a handful in `CLI_EXCLUDED`.
- Port 4545 is a hardcoded `const PORT` (`server.rs:21`). The CLI needs `--port` and `COOLDESK_PORT`.
