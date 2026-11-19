# CoolDesk — operator guide for AI agents

You are an AI agent with no prior context. This guide is enough to operate CoolDesk on this machine: launch and manage apps, drive the terminal and the browser, work with files, organize projects, and customize the app. Every command here has been run against a real CoolDesk. When something is not possible, this guide says so instead of guessing.

**Read sections 1–4 before doing anything. Then jump to the recipe you need (section 6).** Every command with its exact args: [COMMAND-REFERENCE.md](COMMAND-REFERENCE.md) (generated).

---

## 1. What CoolDesk is

CoolDesk is a desktop launcher and workspace manager (Tauri: Rust backend + React UI). It organizes work into **spaces** (called *workspaces* in the API). A space holds the links, apps, folders, shells, dev servers, notes and to-dos for one project.

It has four parts. You need to know which part does what:

| Part | What it is | How you reach it |
|---|---|---|
| **Desktop app** (`cooldesk.exe`) | The windows: main window, **Spotlight** (Alt+K search), dock **bar** / **sidebar**, file manager, terminals, browser panes | Through its sidecar (below) |
| **Sidecar** | An HTTP + WebSocket server *inside* the app, on `127.0.0.1:4545` | The `cooldesk-cli` command, or MCP |
| **Chrome extension** | Reports the browser's tabs and history to the app, and draws the new-tab dashboard with widgets | Indirectly: tab commands go app → extension |
| **`cooldesk-cli`** | The command-line client you use. It also contains an MCP server | `cooldesk-cli …` in any shell |

**Mental model:** you never click. You send commands to the sidecar. The sidecar runs the same Rust functions the UI's buttons run.

## 2. Pre-flight: run these first, every session

```sh
cooldesk-cli health                 # is the app running?  exit 3 = not running
cooldesk-cli auth status            # is the action API reachable with the token?  exit 4 = no
cooldesk-cli invoke get_app_version # which build?
cooldesk-cli tabs list              # is the Chrome extension connected? [] = no browser connected
```

| Result | Meaning | Fix |
|---|---|---|
| `health` exit 3 | The app isn't running | Start it (section 5.1) |
| `health` ok, `auth status` exit 4 | A build without the CLI API, or the token file is missing or stale | Use the build from this fork (section 5.2), then restart the app |
| `tabs list` is `[]` | No browser extension connected | Browser-tab recipes won't work. See 6.3 |

## 3. How to call CoolDesk

### 3.1 The CLI

- **Binary:** `cooldesk-cli` (installed at `%USERPROFILE%\.cargo\bin\cooldesk-cli.exe`, on PATH).
- **Output:** JSON on stdout. Errors go to stderr. Add `--compact` for single-line JSON.
- **Global options:**
  - `--port <n>` / env `COOLDESK_PORT` (default 4545)
  - `--timeout <s>` / env `COOLDESK_TIMEOUT` (default 30; raise it for `llm`)
  - `--token <t>` / env `COOLDESK_TOKEN` (normally automatic)

There are two families of commands:

1. **Data commands:** `ws`, `urls`, `notes`, `pins`, `tabs`, `settings`, `ui-state`, `activity`, `search`, `get`, `llm`, `watch`, `raw`. These read and write the app's stored data.
2. **Actions:** `cooldesk-cli invoke <command> --json '<args>' [--yes]`. These run 124 app functions: launching, windows, files, terminal, clipboard, processes, vault…
   - List them with `cooldesk-cli commands` (all) or `cooldesk-cli commands --tier read|act|destructive|secret`.
   - Each entry shows `name`, `tier`, `summary` and `args` (`name`, `type`, `required`).

**Rules for `invoke`:**
- `--json` must be a JSON **object**. Arg names are **snake_case** exactly as listed (`dest_dir`, not `destDir`).
- Unknown args are rejected (400). Missing required args are rejected (400).
- Arg types: `string`, `string[]`, `bool`, `u16`/`u32`/`usize` (non-negative integers), `i32`/`i64`, `f64`, `json` (any JSON value), `object` (a structured object; see the summary).
- The result is `{"ok":true,"cmd":…,"result":…}`. `result` is `null` for actions that return nothing.

> **Git Bash rewrites paths that start with `/`.** `raw post /workspaces` arrives as `C:/Program Files/Git/workspaces` (HTTP 404). Leave the slash off: `raw post workspaces …` (the CLI adds it back), or set `MSYS_NO_PATHCONV=1`.
>
> **Shell quoting:** in bash or Git Bash, wrap `--json` in single quotes. In PowerShell, use single quotes too, and double any `"` inside if needed. Or build the JSON with a script.

### 3.2 Exit codes

| Code | Meaning | What to do |
|---|---|---|
| 0 | ok | — |
| 1 | HTTP error, or a 200 whose body says `ok:false`. For `invoke`: 400 bad args, 404 unknown command, 422 the command itself failed (the message says why) | Read stderr |
| 2 | Usage error, or a refused route | Fix the command line |
| 3 | App not reachable | Start the app |
| 4 | Token missing or rejected | Restart the app; check `auth path` |
| 5 | **Destructive/secret command refused: you didn't pass `--yes`** | Pass `--yes` only if the user asked for this action |

### 3.3 MCP (if you are Claude Code or another MCP client)

The same actions are available as MCP tools from the server `cooldesk` (registered user-scope: `cooldesk-cli.exe mcp serve`). Tool names are the command names, e.g. `mcp__cooldesk__launch_app`.

- **Tiers:** destructive tools have a required `confirm` argument that must be `true`. Vault (secret) tools are **not exposed** over MCP unless the server was started with `--tiers read,act,destructive,secret`.
- **Extra tools:** the app's 7 legacy read tools are also there: `open_tabs`, `visited_sites`, `unfiled_pages`, `site_usage`, `app_usage`, `list_apps`, `suggest_workspace`.
- **App not running:** the tool list is empty. It fills in automatically when the app starts.

## 4. Safety rules (follow them)

1. **Tiers:**
   - `read`: run freely.
   - `act`: run when it serves the user's request. Nothing is lost, but things open on their screen.
   - `destructive` and `secret`: **only when the user asked for that specific action.** Never add `--yes` (or `confirm: true`) to get past an error.
2. **Never print secrets.** Don't echo clipboard history text or vault entries unless the user asked to see that item.
3. **Restore what you change for testing.** Read the state first (`dock_get_state`, `folder_index_list`, `clipboard_status`…), change it, then put it back and compare.
4. **Things you open in the UI persist.** Terminal tabs and browser panes stay in CoolDesk's saved UI state, and closing the shell doesn't remove the tab. Tell the user what you left open.
5. **Prefer reversible actions:**
   - `close_app` (asks the window to close and lets it prompt to save) before `kill_process` (force).
   - `shelf_save_to` (moves) before `shelf_remove` (deletes).
6. **Don't use winget or updates on this build.** `run_winget_upgrade` is disabled in fork builds. `install_native_update` restarts the app.

---

## 5. Running the app

### 5.1 Start, stop, check
- **Start:** run the installed app from the Start menu, or `"%LOCALAPPDATA%\CoolDesk\cooldesk.exe"` (the per-user NSIS install location).
- **Stop:** `cooldesk.exe --quit`.
- **Running?** `cooldesk-cli health`.
- **Start at login:** `cooldesk-cli invoke set_launch_at_login --json '{"enabled":true}'`.

### 5.2 Build and install from source (this fork)
```sh
git clone https://github.com/LeverageAISMBs/cooldesk-extension && cd cooldesk-extension
npm install
# The release build is signed with the fork's updater key. The key and its password
# are in the keyring (project cooldesk-release); exec injects them as env vars:
node C:/Users/mikes/keyring/src/cli.js exec -p cooldesk-release -- npm run build:tauri
# Output: src-tauri/target/release/bundle/nsis/CoolDesk_<version>_x64-setup.exe (+ .sig for the updater)
#         src-tauri/target/release/bundle/msi/CoolDesk_<version>_x64_en-US.msi (+ .sig)
# Silent install: CoolDesk_<version>_x64-setup.exe /S   → %LOCALAPPDATA%\CoolDesk
cargo install --path cooldesk-cli          # the CLI + MCP server
```
For a quick dev build without an installer: `cd src-tauri && cargo build --features tauri/custom-protocol`. The feature is required. Without it the window tries to load `localhost:5173` and shows a browser error page.

---

## 6. Recipes

Each recipe shows the commands in the order to run them. `$C` stands for `cooldesk-cli`.

### 6.1 Apps and windows

| Goal | Command |
|---|---|
| What's running (with windows, pids, hwnds) | `$C invoke get_running_apps` |
| What's installed | `$C invoke get_installed_apps` |
| Find an app by name (fast, ranked) | `$C search apps "code" --limit 5` |
| Which app has focus | `$C invoke get_focused_app` |
| Launch by path (.exe, .lnk, Store apps, .cpl) | `$C invoke launch_app --json '{"path":"C:/Windows/System32/notepad.exe"}'` |
| Launch with arguments | `$C invoke launch_app_with_args --json '{"app":"C:/path/app.exe","args":["--flag","x"]}'` |
| Bring an app to the front | `$C invoke focus_window --json '{"pid":1234}'` (optionally `hwnd`, `name`, `path`) |
| Tabs inside a native window (e.g. Windows Terminal, Explorer) | `$C invoke list_window_tabs --json '{"hwnd":123456}'`, then `focus_window_tab` with `index` or `title` |
| Close an app politely (it may prompt to save) | `$C invoke close_app --json '{"pid":1234}' --yes` |
| Force-kill a process tree | `$C invoke kill_process --json '{"pid":1234}' --yes` |

**Find pid and hwnd:** `get_running_apps` returns them. Filter its JSON by `name`/`title`.

**CoolDesk's own windows:**
- `app_windows_list`, `app_window_focus {"label":"main"}`, `main_window_hide`, `open_new_window`
- `app_window_close {"label":…}` (destructive; for `main` it hides the window instead)

**Verify a launch worked:** call `get_running_apps` again, or `get_focused_app`.

### 6.2 The terminal

CoolDesk has its own terminal (PowerShell on Windows) inside its windows. There are three ways to run things:

**A. Run a command visibly in CoolDesk's terminal** (opens the main window on a terminal tab):
```sh
$C invoke spotlight_run_in_terminal --json '{"command":"npm test"}' --yes
```

**B. Type into a terminal that's already open:**
```sh
$C invoke terminal_list                     # → [{"webview":"main","id":"mwnb…-1","pid":8732}, …]
$C invoke terminal_write --json '{"webview":"main","id":"mwnb…-1","data":"git status\r"}' --yes
```
`\r` presses Enter. Send `\u0003` for Ctrl+C. Other calls:
- `terminal_resize {webview,id,cols,rows}`
- `terminal_close {webview,id}` (destructive: it ends the shell)
- `terminal_focus {webview}`

**C. Run a command in a new standalone console window** (good for dev servers):
```sh
$C invoke run_project_command --json '{"command":"npm run dev","cwd":"C:/code/myapp"}' --yes
```

**Reading output:** the CLI **cannot read terminal output**. The stream goes only to the UI. To capture output, redirect it to a file and read the file:
```sh
$C invoke terminal_write --json '{"webview":"main","id":"<id>","data":"npm test *> C:/Temp/out.txt\r"}' --yes
# wait for it to finish, then:
$C invoke preview_text_file --json '{"path":"C:/Temp/out.txt"}'
```
If you have your own shell, running the command there yourself is simpler. Use CoolDesk's terminal when the user wants to *see* it in CoolDesk.

**Dev servers and ports:**
- `$C invoke list_listening_ports`: every listening TCP port with its process.
- `$C get servers`: the dev servers CoolDesk detected.
- `$C invoke kill_process_on_port --json '{"port":3000}' --yes`: frees a port.

### 6.3 The browser

There are two different browsers. Don't confuse them:

**(a) The user's real browser (Chrome)**, through the CoolDesk Chrome extension:

| Goal | Command |
|---|---|
| Open a link in the default browser (Chrome) | `$C invoke open_url --json '{"url":"https://example.com"}'`. Only `http`, `https`, `mailto` and `ms-settings` links are allowed |
| List open tabs (all connected browsers) | `$C tabs list` |
| Switch to a tab | `$C tabs jump <tabId> --window-id <id> --browser <b> --device-id <d>` (values from `tabs list`) |
| Close a tab | `$C tabs close <tabId> …` (same flags) |
| Search history | `$C search history "invoice" --limit 10` |
| Which sites got time today | `$C activity sites --days 1` |

- **Tab control needs the extension.** Install it from the Chrome Web Store ("CoolDesk"), or from this repo:
  1. Run `npm run build`.
  2. Open `chrome://extensions` and turn on **Developer mode**.
  3. Click **Load unpacked** and select the **repo root** (it uses `manifest.json`). Source: the repo README, "Load the extension".

  If `tabs list` is `[]`, tab commands can't work. Say so; don't retry.
- **Don't** use `raw get /cmd/jump-next` or `/cmd/close-next`. They steal the extension's queue, and the CLI refuses them.

**(b) Browser panes inside CoolDesk** are embedded web views the user opened in CoolDesk. You can act on an open pane by its id, but you **cannot open a new pane** (the UI places panes inside a window):
- Read: `browser_pane_stats` (lists open panes and their ids), `site_permissions_get {host}`, `page_dark_hosts`.
- Act: `browser_pane_nav {"id":…,"action":"back"|"forward"|"reload"}`, `browser_pane_set_zoom {id,zoom}`, `browser_pane_set_adblock {id,enabled}`, `browser_pane_stop_capture {id,camera,microphone}`.
- Destructive: `browser_pane_close {id}`, `browser_pane_kill {id}`, `site_permissions_set {host,settings}`.
- **On Windows, these are not available yet:** `browser_pane_find`, `browser_pane_set_dark`, `browser_pane_screenshot` and `browser_pane_clip` return "not available on this platform yet".

**Web app windows:** `open_webapp_window {"id":"gmail","url":"https://mail.google.com","title":"Gmail"}` opens a site as its own CoolDesk window.

**Highlights** (saved page highlights): `highlights_list {url}`, `highlights_update {url,id,color?,note?}`, `highlights_delete {url,id}` (destructive).

**(c) Inside a page: click, type, scroll, read.** CoolDesk can't do this; its browser reach stops at tabs. Use the **Scrapling Agent Suite** bridge instead. It's an MCP server named `scrapling`, registered user-scope in Claude Code at `http://127.0.0.1:8765/mcp`. Input is **humanized by default**:
- mouse moves along a jittered path to the target before clicking;
- typing has human timing;
- scrolling is split into uneven wheel steps with random 30–80 ms pauses.

1. **Is the bridge running?** `curl -s http://127.0.0.1:8765/health`. If it isn't:
   ```
   cd C:\Dev\browser-extensions\scrapling-agent-suite\bridge-python && uv run scrapling-bridge
   ```
   Run it in the background; it serves on 127.0.0.1:8765.
2. **Connect:** `browser_connect`.
   - The default `mode:"persistent_profile"` launches **real Chrome** with the suite's login-once profile (`%LOCALAPPDATA%\ScraplingAgentSuite\BrowserProfile`). The user signs in there once, and the sessions persist.
   - `mode:"attach_cdp", cdp_url:"http://127.0.0.1:9333"` drives the dedicated debugging Chrome instead.
   - **Either way, this is a different Chrome instance from the user's everyday Chrome.** CoolDesk's `tabs` commands don't see its tabs, and its tools don't see CoolDesk's.
3. **Act.** The tools:
   - Tabs and navigation: `browser_navigate {url}`, `browser_list_tabs`, `browser_new_tab`, `browser_switch_tab`.
   - Reading the page: `browser_map_dom` (interactive elements with selectors), `browser_screenshot {annotate}` (numbered element overlay), `browser_extract` (clean content).
   - Input: `browser_click {selector, humanized}`, `browser_type {selector, text, press_enter, humanized}`, `browser_scroll {direction: down|up|left|right, amount: pixels, humanized}`.
   - Other: `browser_auto_login` (password from the OS keyring, in memory only), `browser_status`.
4. **Verify.** `browser_scroll` returns `scroll_before` and `scroll_after` measured from the page. `browser_click` returns the URL and `healed` (whether the selector had to be relocated). `browser_status` shows the active URL.
   - Checked 2026-11-19: a scroll down 1200 gave y 0 → 1200; a click on example.com's link navigated to iana.org.

**Typical combined flow:** use CoolDesk for the desktop (launch apps, terminals, files, the user's everyday tabs) and `scrapling` for anything inside a page.

### 6.4 Files and folders

| Goal | Command |
|---|---|
| List a folder | `$C invoke list_dir --json '{"path":"C:/Users/me/Downloads"}'` |
| Does it exist? File or folder? | `$C invoke path_kinds --json '{"paths":["C:/a","C:/b.txt"]}'` |
| Read a text file | `$C invoke preview_text_file --json '{"path":"C:/notes.txt"}'` (returns content, line count, truncated flag) |
| Image / PDF preview | `preview_image_file`, `preview_pdf_file` |
| Open in Explorer | `$C invoke open_folder --json '{"path":"C:/code"}'` |
| Search files by name (indexed folders only) | `$C invoke search_files --json '{"query":"invoice"}'` |
| Which folders are indexed | `$C invoke folder_index_list` |
| Index a folder (makes it searchable) | `folder_index_add {path}`, then `folder_index_reindex {path}` |
| Index options | `folder_index_set_options {path, enabled?, auto_reindex_minutes?, exclude?, include_apps?}` |
| Stop indexing a folder | `folder_index_remove {path}` (destructive; it only removes it from the index, no files are deleted) |
| Is this a project folder? Repo info | `classify_project_folders {paths}`, `folder_identity {path}` (git branch, dirty, stack) |
| User places / frequent folders | `get_user_places`, `get_frequent_folders` |
| Zip / list / extract | `archive_compress {paths}` → zip path. `archive_list {path}`. `archive_extract {path}` → folder next to it. `archive_extract_entry {path,entry}` |

**The shelf** (a tray for files and text the user is carrying between apps):
- Read: `shelf_list`.
- Add: `shelf_add_text {text,name?}`, `shelf_import {paths}` (copies files in), `shelf_add_bytes {name,data(base64)}`.
- Pin: `shelf_pin {id,pinned}`.
- Move out: `shelf_save_to {id,dest_dir?}` (moves the item out; the default is Downloads).
- Delete: `shelf_remove {id}`, `shelf_clear {include_pinned}` (both destructive).

**Clipboard history** (off unless the user enabled it):
- Read: `clipboard_status`, `clipboard_list` (previews), `clipboard_text {id}` (full text; secrets rule applies).
- Act: `clipboard_copy {id}` (puts it back on the system clipboard), `clipboard_pin`, `clipboard_save_to_shelf`, `clipboard_set_enabled`, `clipboard_set_paused`.
- Destructive: `clipboard_remove`, `clipboard_clear`, `clipboard_set_filters` (because `skip_secrets:false` records passwords).

**Delete or move arbitrary files:** CoolDesk has no general delete/move command. Use your own shell.

### 6.5 Spaces (workspaces), links, notes, pins

```sh
$C ws list
$C ws show "My Project"
$C ws add "My Project" --url https://github.com/me/repo --url https://linear.app/me   # creates, or merges into the space with that name
```
**Spaces hold more than links.** Write the full object with `raw post`:
- `apps` is an array of `{name, path, appType, addedAt}`.
- `appType` is `"folder"` for folders, `"file"` for files, `"vscode"` / `"cursor"` / `"windsurf"` / `"idea"` to open in that editor, and `"default"` for apps.

```sh
$C raw post workspaces '[{"id":"ws_myproj","name":"My Project","urls":[{"url":"https://github.com/me/repo"}],"apps":[{"name":"repo","path":"C:/code/repo","appType":"folder"}]}]'
```
- POST **merges by name** (case-insensitive) and never removes items.
- The UI picks it up: the sidecar broadcasts `workspaces-updated` (`handlers.rs`, `save_and_broadcast`), which the UI subscribes to (`src/services/syncOrchestrator.js`). Confirm with `$C ws show "<name>"`.
- Only `id, name, urls, apps, shells, servers, createdAt, updatedAt` survive this path. UI-only fields such as `description` or `color` are dropped.
- **There is no delete command for spaces, links, notes or pins.** Removal needs the UI.

**Other data commands:**
- Notes: `$C notes list`, `$C notes add "text" --title T --folder F --tag x`.
- Saved links: `$C urls list|add <url> --title T`.
- Pins: `$C pins list|add <url> --title T`.

### 6.6 Customization

| Goal | Command |
|---|---|
| Spotlight hotkey | **Two calls.** Apply now: `$C invoke set_spotlight_shortcut --json '{"shortcut":"Alt+Space"}'`. Keep it after restarts: `$C settings set spotlightShortcut Alt+Space` (read at startup; the default is Alt+K). An invalid key returns 422 and reverts to Alt+K |
| Show/hide Spotlight | `toggle_spotlight`, `hide_spotlight` |
| Dock bar / sidebar | `dock_get_state`. Then `dock_enable {"surface":"bar"\|"sidebar", side?, size?, mode?, expand?}`, `dock_disable {surface}`, `dock_expand`, `dock_collapse`, `dock_set_size {surface,size}` |
| Start at login | `get_launch_at_login`, `set_launch_at_login {enabled}` |
| Analytics | `set_analytics_enabled {enabled}`. Fork builds send no analytics anyway |
| Clipboard history | `clipboard_set_enabled {enabled}`, `clipboard_set_paused {paused}` |
| Sidecar settings | `$C settings get`, `$C settings get <key>`, `$C settings set <key> <value>` (VALUE is parsed as JSON, otherwise stored as a string; POST merges keys). Keys that exist: `spotlightShortcut`, and under `data`: `historyDays`, `visitCountThreshold`, `modelName`, `geminiApiKey` (**secret: never print it**) |
| Language, theme, layouts | **UI-only.** They live in the window's local storage (`src/shared/i18n`, `cooldesk-theme`), not in the sidecar. Use Settings → General → Language in the app, or Customize on the Chrome new tab |

### 6.7 Widgets

Widgets are small tiles on the **new-tab dashboard** of the Chrome extension, and on the board in the desktop app's overview. There are 39 of them in five categories (CoolDesk, Time & Focus, Productivity, Data & Life, Dev Tools): clock, weather, pomodoro, tasks, calendar, GitHub graph, JSON formatter, regex tester and more (`src/data/widgetCatalog.js`). They're served from `cool-desk.com/widgets` as embedded pages.

**There is no CLI or MCP command for widgets.** The board layout is saved only in the browser's or app window's local storage (`cooldesk-widget-layouts`), which the sidecar can't reach. To add, remove, resize or recolor widgets, use the UI:
- **In Chrome:** open a new tab → Customize → widgets.
- **In the desktop app:** the overview board.

An agent with browser automation can drive the new-tab page in Chrome directly. Report this limit instead of attempting it through the CLI.

### 6.8 Spotlight (for reference, if the user asks how to do something by hand)
**Alt+K** opens Spotlight. It searches apps, windows, tabs, history, bookmarks, spaces and files. Slash commands:
- Scoped search: `/a` apps, `/u` URLs, `/f` files.
- Notes and to-dos: `/todo`, `/notes`.
- Terminal: `/shell`.
- Dock: `/dock on|off`, `/sidebar on|off`.
- Agent: `/agent` (the AI agent).
- Spaces: `/new-workspace`.
- Help: `/help`.

**Ctrl+Shift+D** cycles full window / sidebar / bar.

### 6.9 Activity, insight, AI
- Activity:
  - `$C activity focused`: the focused app.
  - `$C activity visible`: visible windows.
  - `$C activity apps --days 7`: per-app time.
  - `$C activity sites --days 7`: per-site time.
- Data: `$C get dashboard`, `$C get graph`, `$C get daily-memory`.
- AI CLIs: `$C invoke ai_cli_detect --json '{"bins":["claude","codex"]}'` checks which are on PATH.
- Local LLM: `$C llm status`, `$C llm chat "…"` (raise `--timeout`).

### 6.10 Password vault (secret tier)
- `vault_status` shows enabled / exists / unlocked.
- `vault_generate {length?,symbols?}` makes a password and stores nothing.
- `vault_lock` locks the vault.
- **Everything else needs `--yes` and the user's explicit request:**
  - `vault_list` and `vault_get {id}` read entries.
  - `vault_copy {id,field}` copies a field to the clipboard, which auto-clears.
  - `vault_unlock {master}` unlocks.
  - `vault_save {entry:{title,username?,password?,url?,notes?,id?}}` adds or updates an entry.
  - `vault_delete {id}` deletes one.
- Never ask the user for the master password just to explore.

### 6.11 System and backups
- Version and folders: `get_app_version`, `get_backups_dir`, `get_agent_workspace_dir`.
- Backups:
  - `list_backups`.
  - `read_backup {path}` (inside the backups folder only).
  - `save_backup {contents}` (destructive: it prunes old backups).
  - `delete_backup {path}`.

### 6.12 Watching events
`$C watch --type workspaces-updated,notes-updated` prints one JSON line per change. Use `--count N` to stop after N lines.

---

## 7. Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| exit 3, "connected but got no response" | The port is held by a stale socket (a terminal launched from an old CoolDesk keeps it open) | Close that terminal, or run on another port with `COOLDESK_SIDECAR_PORT` (test builds only) |
| exit 4 | The app restarted and the token rotated, or the build has no CLI | Retry once (the CLI re-reads the token). Check `auth path` |
| 422 "Browser pane is not open" | Wrong pane id | `browser_pane_stats` |
| 422 "no CoolDesk webview labelled …" | Wrong `webview` for a terminal | `terminal_list` |
| 422 "Only web links can be opened" | `open_url` with a file: or app link | Use `open_folder` / `launch_app` |
| `tabs list` = `[]` | No extension connected | Install or enable the Chrome extension |
| The window shows "can't reach this page" | A debug build without `custom-protocol` | Rebuild with the feature |

## 8. What you cannot do (by design or not yet)

- Open a CoolDesk terminal session directly and read its output stream. Open one with `spotlight_run_in_terminal`; read output through a file.
- Open a new browser pane, or place panes and embedded web apps. They're positioned by the UI's layout.
- Click, type, scroll or read inside web pages through CoolDesk. Use the `scrapling` MCP server for that (6.3 c).
- Change widgets, or delete spaces, links, notes or pins. Only the UI can.
- Run AI-CLI sessions inside CoolDesk (`ai_cli_run`): their output streams to the UI. Run the AI CLI directly.
- The full list of excluded commands, with reasons: `$C commands` → `excluded`.
