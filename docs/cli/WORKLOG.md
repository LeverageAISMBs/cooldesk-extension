# WORKLOG

## 2026-11-18 — P0 built and verified live

Ran against CoolDesk v2.0.22 (`cooldesk.exe` pid 20028, sidecar :4545).

- **Reads, 28 commands, all exit 0 with live data:** health, ws, urls, tabs, notes, pins, settings, ui-state, all 5 activity commands, both search commands, the 7 `get` collections, the 4 llm read commands, and `mcp tools`.
- **Writes, all returned 204 and read back correctly:** notes add, pins add, ws add, urls add, ui-state set.
- **watch:** received notes-, pins-, workspaces-, urls- and ui-state-updated for each write, plus activity-updated and request-tabs. Unidentified CLI clients get the broadcasts, which confirms H3.
- **mcp:** `call list_apps` and `site_usage` both answered. An unknown tool gave the JSON-RPC error -32602 and exit 1.
- **tabs:** `jump <real id>` returned success:true. `close 99999999` also returned success:true, because success only means queued.
- **Failure paths:**
  - wrong port: exit 3
  - bad JSON: exit 2
  - 404: exit 1
  - `/cmd/*-next`: exit 2 (refused)
  - `llm chat` with no API key: exit 1
- **Bugs found and fixed during verification:**
  1. The dequeue guard used a prefix match, which Git Bash path rewriting bypassed. It now matches anywhere in the path.
  2. `ok:false` bodies exited 0. They now exit 1.
- **Not run:** `llm load/unload/download` (download pulls a multi-GB model; load needs one present) and `settings set` (I didn't want to change a real setting; the same merge path was tested through `ui-state set`).

**Test data left in the app (can't be removed over HTTP; delete it in the UI):**
- note "CLI smoke test"
- pin, URL and workspace "CLI Smoke Test" → https://example.com/cooldesk-cli-smoke
- ui-state key `cooldeskCliSmoke`

## 2026-11-19 — P1 token auth (CoolDesk branch `cli/p1-token-auth`, commit ff18c31)

- **App:** `sidecar/cli_auth.rs` adds the gated `/cli/*` group with `GET /cli/ping`.
  - The token is per start, from the OS RNG, and written after the bind.
  - The comparison is constant-time, and any request with an Origin gets 403.
  - Also added: the `COOLDESK_SIDECAR_PORT` test override, and the "Server running" log moved after the bind.
- **Tests:** 10 new cli_auth tests pass. The full lib suite passes 134, with 0 failed.
- **Live, debug build on :4546:**

  | Request | Result |
  |---|---|
  | `/cli/ping` with no header | 401 |
  | `Bearer nope` | 401 |
  | raw token without `Bearer` | 401 |
  | `Bearer <token>` | 200 |
  | token plus `Origin: https://evil.example` | 403 |
  | token plus `Origin: chrome-extension://abc` | 403 |
  | `/cli/nope` | 404 |

  - `cooldesk-cli auth status` exits 0. With a wrong `--token` or `COOLDESK_TOKEN` it exits 4.
  - P0 regression on the new build: health, ws, search, mcp, activity and notes add/list all exit 0.
- **CLI:**
  - Reads the token file on every call, and sends it only on `/cli/*`.
  - Added `auth status|path` and exit 4 on 401/403.
  - Added `--timeout` (default 30 s, was 120 s).
  - Stale-port detection: a connection that's accepted but never answered (Windows error 10060) is reported as a stale listener. A refused connection is still "Is the app running?". Both branches verified.
- **Side effects checked:**
  - The HKCU Run key is the same before and after, with no CoolDesk entry added.
  - The test instance exited through `cooldesk.exe --quit`, and 4546 is free afterwards.
  - The test data went to the scratch `sync-data`, not real data.
- **Upstream bug found:** a terminal launched from CoolDesk inherits the sidecar's listening socket. After CoolDesk exits, port 4545 stays bound (LISTEN plus 54 CLOSE_WAIT, owner pid 20028 already gone) until that terminal closes, and any restart of CoolDesk fails to bind with error 10048. The fix would be to spawn children without inheriting handles. Not fixed in P1.

## 2026-11-19 — P2.1 launcher and windows (CoolDesk branch `cli/p2-launcher-windows`, commit 9d9378a, stacked on P1)

- **App:** `sidecar/cli_invoke.rs` adds the manifest (17 commands: 5 read, 9 act, 3 destructive, plus 1 excluded) and a dispatch that calls the IPC functions directly. `GET /cli/commands` and `POST /cli/invoke` sit behind the P1 token. The AppHandle is in a OnceLock set in setup.
- **Build:** the Common-Controls manifest is now embedded by the linker for all targets, so lib tests that link window code can load. The app manifest was checked from the PE and is equivalent.
- **CLI:** `commands [--tier]`, `invoke <cmd> [--json] [--yes]`, and exit 5 on 428.
- **Tests:** 11 new. Full lib suite: 145 passed, 0 failed, 2 ignored.
- **Live (debug build, port 4546):**
  - `commands`: 17 commands, excluded `[app_window_set_space]`
  - `app_windows_list` → `[{label:main}]`, which proves the AppHandle wiring
  - `get_running_apps` → 47 apps
  - `open_url file:///…` → 422 "Only web links"
  - `nope` → 404, exit 1. `launch_app {}` → 400, exit 1. A wrong token → 401, exit 4.
  - `launch_app charmap` → opened. `focus_window {pid}` → `get_focused_app` = charmap.
  - `close_app` without `--yes` → 428, exit 5, still running. With `--yes` → closed.
  - Then `--quit`: no process left, and 4546 was free.

## 2026-11-19 — P2.2–P2.9 + P3 (branches cli/p2-search-folders … cli/p3-parity-gate, tip b5a337f)

- **Framework:** `cli_commands!` is one declaration per command. 140 IPC commands = 123 exposed + 17 excluded (with reasons), plus 1 CLI-only (`terminal_list`). Tiers: 41 read, 51 act, 23 destructive, 9 secret.
- **P3 gate:** a test parses `generate_handler!` and fails on any command that's neither exposed nor excluded. It fails closed and is mutation-checked.
- **Tests:** the full lib suite at the tip has 150 passed, 0 failed, 2 ignored.
- **Live (debug + custom-protocol, port 4546):**
  - **P2.2:** an index add/reindex/search/remove round trip, with the index restored. An archive compress/list/extract round trip. `web_suggest` and `folder_identity` returned real data.
  - **P2.3:** spotlight shows and hides. The dock round trip restored state exactly. `spotlight_run_in_terminal` ran `echo` in CoolDesk's terminal (screenshot).
  - **P2.4:** `shelf_add_text`, `pin` and `save_to` worked (the saved file's content was correct). `add_bytes`, `import` and `remove` worked. The shelf was restored to 0 items.
  - **P2.5:** status and list were checked by count only. The paused toggle was restored. Filters, clear and remove refused without --yes.
  - **P2.6:** stats, adblock_supported, dark_hosts, site_permissions_get and highlights_list worked. Nav on a missing pane → 422. Find → the Windows stub says "not available". No pane was open.
  - **P2.7:**
    - `terminal_write --yes` typed `Set-Content …` and the file appeared, containing `p27-written-via-cli`. The screenshot shows the line in CoolDesk's terminal.
    - Resize: ok. `terminal_close`: the shell pid is gone. A bad webview label → 422.
  - **P2.8:**
    - 28 listening ports, including 4546.
    - `kill_process --yes` killed a ping it had started. `kill_process_on_port 4599 --yes` killed a python http.server.
    - The `launch_at_login` on→off round trip was checked in HKCU\…\Run: entry added, then removed.
    - `read_backup` outside the backups folder → refused.
    - `ai_cli_detect`: claude and codex true.
  - **P2.9:** vault_status (no vault on this machine), generate (24 chars), lock. Secret commands → 428 without --yes. `vault_list --yes` → 422 "Vault is locked".
- **Left behind:** two test terminal tabs in CoolDesk's sidebar (WebView localStorage, shared with the installed app). Mike to close them. See H12.

## 2026-11-19 — P4 MCP server (`cooldesk mcp serve`, cooldesk-cli/src/mcp_serve.rs)

- **Design:** stdio MCP inside the CLI, built from `/cli/commands` and `/cli/invoke` at runtime, plus the 7 legacy `/mcp` read tools. No app changes.
  - Secret is opt-in. Destructive and secret tools require `confirm: true`.
  - The token is read per call. It advertises listChanged and sends it from a poll thread.
- **Tests:**
  - Unit: 8 pass.
  - Live protocol (scripts/mcp_live.py): 26/26 pass. Covered: app down → [], then list_changed when the app starts (122 tools = 115 non-secret + 7 legacy), a read call, an act call (charmap launched), destructive refused without confirm and executed with it, a 422 as isError, token rotation across a restart, the `--tiers read` filter (48 tools), the secret opt-in, and a bad tier → exit 2.
- **Bug found and fixed:** after an app restart, the first POST failed (os error 10054) because ureq's pooled socket pointed at the dead server. The fix is `max_idle_connections(0)`.
- **Registered:** user scope (`C:\Users\mikes\.claude.json`), command `C:/Users/mikes/.cargo/bin/cooldesk-cli.exe mcp serve` (installed with `cargo install --path .`). `claude mcp get cooldesk` → ✔ Connected.
- **E2E:** a headless `claude -p --model haiku` with a 4546 config answered "version: 2.0.22 / port 4546 owner: cooldesk.exe". The transcript shows the two cooldesk tool_use calls. $0.0048.
- **Caveat:** the registered server targets 4545, the installed CoolDesk 2.0.22, which has no /cli routes. Until a build from these branches runs there, agents see only the 7 legacy tools.

## 2026-11-19 — Launch prep (branch release/fork-launch)
- The CLI crate moved into the fork. All 12 cli/* branches were pushed to origin (LeverageAISMBs). The first push failed (403) because a browser was logged into the personal GitHub account.
- Fork release channel: updater, update banner, analytics off, winget refused. A new passworded signing key is in the keyring (cooldesk-release).
- Signed NSIS/MSI built from source and installed. The daily app is now this build, on 4545.
- Docs: OPERATOR-GUIDE (hand-written, verified live, passed a zero-context Haiku acceptance test, 6/6), DECISIONS (D1–D12), and the generated COMMAND-REFERENCE.
- P5: docs, completions (PowerShell verified), batch. Git Bash path-rewrite guard in `raw`.
- Known pre-existing flaky test: dir_watch::a_download_burst_is_one_update (2/4 failures on untouched code).
