# cooldesk-cli

**AI agents: start with [docs/cli/OPERATOR-GUIDE.md](../docs/cli/OPERATOR-GUIDE.md).** Design decisions: [docs/cli/DECISIONS.md](../docs/cli/DECISIONS.md).

Drives a running CoolDesk through its local sidecar (`127.0.0.1:4545`). This is P0 of `../CLI-PROPOSAL.md`: it uses only what the app already exposes and makes no changes to CoolDesk.

```
cargo build --release      # -> target/release/cooldesk-cli.exe
```

## Commands

| Command | Route |
|---|---|
| `health` | GET /health |
| `ws list` · `ws show <name>` · `ws add <name> [--url U]...` | /workspaces (POST merges by name) |
| `urls list` · `urls add <url> [--title]` | /urls |
| `tabs list` · `tabs jump <id>` · `tabs close <id>` `[--browser --window-id --device-id]` | /tabs, /cmd/jump-to-tab, /cmd/close-tab |
| `notes list` · `notes add <text> [--title --folder --tag T...]` | /notes (merges by id) |
| `pins list` · `pins add <url> [--title]` | /pins |
| `settings get [key]` · `settings set <key> <json-or-string>` | /settings (merges keys) |
| `ui-state get [key]` · `ui-state set <key> <value>` | /ui-state (merges keys) |
| `activity list [--since ms]` · `focused` · `visible` · `apps [--date --days]` · `sites [--days]` | /activity/* |
| `search apps <q> [--limit]` · `search history <q> [--limit]` | /search, /browser-history |
| `get url-notes\|daily-memory\|dashboard\|graph\|scraped-chats\|scraped-configs\|servers` | read-only collections |
| `llm status\|models\|v3-status\|config` · `llm load <model> [--gpu-layers]` · `llm unload` · `llm chat <msg>` | /llm/*, /llm/v3/* |
| `mcp tools` · `mcp call <tool> ['{json}']` | POST /mcp (7 read-only tools) |
| `watch [--type a,b] [--initial] [--count N]` | /ws → NDJSON on stdout |
| `raw get <path>` · `raw post <path> ['{json}']` | any route |

Global options:
- `--port` / `COOLDESK_PORT`
- `--token` / `COOLDESK_TOKEN` (default: read from `%LOCALAPPDATA%\com.cooldesk.desktop\cli-token` on every call; the app rotates it at every start)
- `--timeout` / `COOLDESK_TIMEOUT` (default 30 s)
- `--compact` for single-line JSON

`auth status` pings the token-gated `/cli/ping` (CoolDesk branch `cli/p1-token-auth`). `auth path` shows where the token file should be.

## App actions (`/cli/invoke`, needs the token)

```
cooldesk-cli commands [--tier read|act|destructive]
cooldesk-cli invoke get_running_apps
cooldesk-cli invoke launch_app --json '{"path":"C:/Windows/System32/charmap.exe"}'
cooldesk-cli invoke focus_window --json '{"pid":1234}'
cooldesk-cli invoke close_app --json '{"pid":1234}' --yes     # destructive: refused (exit 5) without --yes
```

Command names are the app's IPC command names, and arg names are the Rust parameter names (snake_case, e.g. `dest_dir`, not `destDir`). Bad args → 400, unknown command → 404, command error → 422 (all exit 1).

Tiers: `read` and `act` run directly. `destructive` (kill, delete, close, run arbitrary input, security settings) and `secret` (the vault) need `--yes`; otherwise the app refuses with exit 5. Examples:

```
cooldesk-cli commands --tier secret
cooldesk-cli invoke list_listening_ports
cooldesk-cli invoke kill_process_on_port --json '{"port":3000}' --yes
cooldesk-cli invoke terminal_list                                     # then:
cooldesk-cli invoke terminal_write --json '{"webview":"main","id":"<id>","data":"npm test
"}' --yes
cooldesk-cli invoke shelf_add_text --json '{"text":"remember this"}'
```

## MCP server (Claude Code and other agents)

`cooldesk-cli mcp serve` runs an MCP server on stdio. Its tools are the app's `/cli/commands` manifest plus the app's 7 legacy read-only `/mcp` tools. It's registered user-scope on this machine:

```
claude mcp add cooldesk -s user -- C:/Users/mikes/.cargo/bin/cooldesk-cli.exe mcp serve
```

- **Tiers:** `--tiers read,act,destructive` is the default. `secret` (the vault) is only exposed when listed explicitly. `--tiers read` gives a read-only agent.
- **Confirmation:** destructive and secret tools have a required `confirm` argument that must be `true`. Claude Code's own per-tool permission prompt also applies.
- **Auth:** the token is re-read on every call, so restarting CoolDesk needs no restart of the server or of Claude Code.
- **CoolDesk not running:** the server still shows Connected, with no tools. It polls (`--poll 5` seconds) and sends `tools/list_changed` when the app appears.
- **Errors:** they come back as tool results with `isError: true`. stdout carries protocol only; logs go to stderr.
- **Why not the app's `/mcp`:** it has no auth, so action tools must not go there.
- **Live test:** `python -I scripts/mcp_live.py` (26 checks; uses `scripts/app.sh` on port 4546).

## Generated docs, completions, batch (P5)

```
cooldesk-cli docs --out docs/cli/COMMAND-REFERENCE.md   # reference generated from the live manifest
cooldesk-cli completions powershell >> $PROFILE          # also bash, zsh, fish, elvish
printf '%s
' '{"cmd":"get_app_version"}' '{"cmd":"list_dir","args":{"path":"C:/"}}' | cooldesk-cli batch
```

- **completions:** `invoke <TAB>` completes the app's command names (with summaries) in bash, zsh, fish and PowerShell when CoolDesk is running at generation time. Not in elvish. `commands --tier <TAB>` completes in bash and zsh only (clap_complete doesn't emit option values for PowerShell or fish).
- **batch:** NDJSON in, one `{"cmd","args","confirm"}` per line; NDJSON out, `{"line","ok","cmd","result"|"error"}`. Blank lines and `#` lines are skipped. A line's `confirm` counts only when the batch runs with `--yes`. `--stop-on-error` stops at the first failed line. Exit 1 if any line failed.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | ok |
| 1 | HTTP error, **or** an HTTP 200 whose body says `ok:false` / `success:false` (the sidecar reports many failures that way) |
| 2 | usage error or refused route |
| 3 | app not reachable. If the app is running but never answers, the hint says the port is held by a stale socket |
| 4 | CLI token missing or rejected (401/403) |
| 5 | destructive or secret command refused because `--yes` wasn't given (428) |

## Known gaps (P0)

- **No delete.** Every POST route merges and none removes (`sync.rs:46`, `sync.rs:134`). Delete needs a new route (P2).
- **Actions:** `cooldesk-cli commands` lists 124 actions (every IPC command except 17 UI-only ones, which are listed with reasons under `excluded`). Not reachable: opening terminals and browser panes (the UI owns their windows and output streams), and the AI-CLI runner.
- **`tabs jump/close` return `success` once the request is queued** for the browser extension, not when it's done. A tab ID that doesn't exist still returns success.
- **`/cmd/jump-next` and `/cmd/close-next` are refused.** They remove items from the extension's poll queue (`handlers.rs:1352`).
- **Git Bash rewrites `/paths`** into Windows paths. For `raw`, drop the leading slash (`raw get health`) or set `MSYS_NO_PATHCONV=1`.
- **Only `/cli/*` is token-gated.** The old data routes still accept any request with no Origin (`server.rs:357`). Gating them too would mean changing the extension and the frontend shim, and that decision is yours.
- **On Windows, a terminal launched from CoolDesk keeps port 4545 bound after CoolDesk exits** (inherited socket). Restarts fail until that terminal closes. This is an upstream bug.
