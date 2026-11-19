# P6 — the browser plane: logged-in Chrome, organized per workspace

Written 2026-11-19, before the work. Mike's goal, in his words:

> "Drive the browser directly through the same interface that's being used locally, extract docs, do research, do our SpyFu workflow … you have access to the full history and can search, index it, document workflows then pass them off to fast inexpensive models — and we can save it all, and organize it all within the same, smaller easy to navigate workspace."

## The problem today
- Browser work happens in a fresh, logged-out profile (scrapling `persistent_profile`, the 9333 CDP Chrome), not in Mike's real one.
- Screenshots and SpyFu exports land in random folders. Nothing ties them to a project.
- Browser history isn't available to agents, so sequences can't be saved and repeated.
- Context doesn't persist across sessions for browser work. doc-intel fixed this for session transcripts, but not for the browser.

## Facts this design rests on (checked 2026-11-19)
- **CDP can't attach to the real profile.** Chrome 136+ ignores `--remote-debugging-port` on the default user-data dir. Driving the logged-in profile therefore needs an **extension inside it**.
- **Extensions in Mike's Chrome** (`%LOCALAPPDATA%\Google\Chrome\User Data\<Profile>\Extensions\`):
  - "New Tab by Cooldesk" `ioggffobciopdddacpclplkeodllhjko` v2.0.19, **Default profile only**. Its source is this fork, at 2.0.22.
  - "Browser MCP" `bjfgambnhccakkhmkepdoekmckoijdlc` v2.2.3, in Default and Profile 1.
  - Also installed: Jan Browser MCP 0.14.4 (Default) and Algonius Browser MCP 0.5.6 (Profile 1).
- **The CoolDesk extension's permissions** (manifest.json): `history`, `sessions`, `bookmarks`, `tabs`, `scripting`, `topSites`, plus host access to `https://*/*` and `http://*/*`. No `downloads` and no `debugger` permission.
- **The sidecar → extension channel exists**: `jump-to-tab` and `close-tab` over `/ws` (syncWebSocket.js). They're fire-and-forget; there's no request/response.
- **Already working from the CLI:** `search history`, `tabs list|jump|close`, `activity sites`, and `open_url` (opens in Chrome). All verified live this session.

## Architecture

```
Claude Code / cheap model
   ├── MCP cooldesk  ──► CoolDesk sidecar ──► apps, terminals, files, workspaces, history (via the CoolDesk ext)
   └── MCP browsermcp ─► Browser MCP extension (inside the logged-in Chrome profile) ──► click/type/scroll/snapshot
                                     ▲
             launched by CoolDesk: launch_app_with_args chrome.exe --profile-directory="Default"
Artifacts, history exports, workflows ──► <workspace root>/.cooldesk/ ──► doc-intel index
```

**The workspace root** is the workspace's first `apps` entry with `appType:"folder"`. With no folder, it falls back to `%USERPROFILE%\CoolDesk\workspaces\<slug>`.

## Tasks

### P6.0 — evaluate BrowserMCP in the real profile (next session; the server is configured, so it needs a fresh session)
- [ ] Launch Chrome through CoolDesk: `launch_app_with_args {"app":"C:/Program Files/Google/Chrome/Application/chrome.exe","args":["--profile-directory=Default"]}`.
- [ ] Connect BrowserMCP (its toolbar button connects the tab), then list its tools and record them here.
- [ ] Check a logged-in page loads as logged in.
- [ ] Run a SpyFu flow end to end: navigate, interact, export.
- [ ] Measure: does a click land on React/SPA controls, and do input events arrive as `isTrusted`?
- [ ] Decide: use BrowserMCP as-is, or build page commands into our CoolDesk extension (P6.4).

### P6.1 — workspace artifacts (CLI + sidecar; no extension change)
- [ ] `cooldesk-cli ws root <name>`: resolve or create the workspace's root as described above.
- [ ] `cooldesk-cli save <file|--text|--url> --ws <name> [--kind screenshot|extract|export|note] [--title] [--source-url]`: copy into `<root>/.cooldesk/artifacts/YYYY-MM-DD/`, then append a line to `<root>/.cooldesk/index.jsonl` with `{ts, kind, title, source_url, path, sha256, session?}`.
- [ ] `cooldesk-cli artifacts list|search <q> --ws <name>`.
- [ ] Tests: path resolution, never overwriting (date-time suffix on a clash), index append.

### P6.2 — downloads into the workspace (SpyFu exports)
- [ ] `cooldesk-cli sweep-downloads --ws <name> --since <10m|ISO> [--match spyfu]`: **move** new files out of the Downloads folder into the workspace artifacts (kind `export`), then index them. Use the user's real Downloads folder (`get_user_places`). Report each move, and never touch files older than `--since`.
- [ ] Optional later: add the `downloads` permission to our extension so files route automatically.

### P6.3 — history and workflows
- [ ] `cooldesk-cli history export --ws <name> --days N`: the history entries whose domains match the workspace's URLs (plus `--domain` extras), written as `<root>/.cooldesk/history/<date>.md`.
- [ ] `cooldesk-cli workflow save <name> --ws <ws> --from <file.md|ndjson>` and `workflow list|show|run`.
  - A workflow is markdown (steps for a model to follow, such as the BrowserMCP calls) plus an optional `steps.ndjson` that `cooldesk-cli batch` can run.
  - Stored in `<root>/.cooldesk/workflows/`.
- [ ] Index `<root>/.cooldesk/` in doc-intel (its own index root, outside the tree, per house rules) so artifacts, history and workflows are searchable across sessions.

### P6.4 — only if P6.0 says BrowserMCP isn't enough: page commands in our extension
- [ ] The sidecar gets request/response over `/ws` (request ids, timeouts, replies) and `/cli/invoke` commands `page_*`. They're added to the command table, so the CLI, MCP and parity gate pick them up.
- [ ] Extension handlers (`scripting.executeScript`):
  - `page_map_dom`, `page_click`, `page_type`, `page_scroll` (humanized: the scrapling algorithms);
  - `page_extract` (markdown), `page_screenshot` (`captureVisibleTab`), `page_navigate`, `page_wait`.
- [ ] Optional `trusted:true` via `chrome.debugger` (`Input.dispatchMouseEvent`) for sites that check `isTrusted`. This adds the `debugger` permission, and Chrome shows a "being debugged" bar.
- [ ] Ship our extension build in place of the store 2.0.19: run `npm run build`, then **Load unpacked → `dist/`** (not the repo root; the root page loads raw JSX), and disable the store copy. Do this in both profiles as needed.
  - Source map: `manifest.json`, `src/background/` (the WS bridge is `bridge.js`), `src/content-scripts/`, `src/app/ExtensionApp.jsx`.

## Predictions (logged in hypothesis.md as H18–H20)
- H18: BrowserMCP drives the logged-in Default profile with sessions intact, unlike scrapling's fresh profile.
- H19: on at least some SPA controls, BrowserMCP's clicks work. If it uses `chrome.debugger`, its events are `isTrusted`, and that will show up as the debugging infobar.
- H20: P6.1–P6.3 need no extension change: history comes through the existing CoolDesk extension, and files are handled by the CLI.
