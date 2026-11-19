# CoolDesk CLI — architecture and engineering decisions

Each entry gives the decision, the context that forced it, the alternatives considered, and the evidence. Where a decision was wrong first, the entry says so; the diff between prediction and reality is in `hypothesis.md` (H-numbers).

Fork: `LeverageAISMBs/cooldesk-extension` (origin). Upstream: `abhayraghuwanshi/cooldesk-extension`.

---

## D1. Drive the app through its sidecar, never through the UI

**Decision:** the CLI talks HTTP and WebSocket to the app's embedded axum server (127.0.0.1:4545). It never automates the UI, injects keystrokes, or emits frontend events.

**Context:** CoolDesk has three control surfaces (PROPOSAL.md §3): ~90 sidecar data routes, a `/ws` event stream, and ~140 Tauri IPC commands. Only the first two could be reached from outside the process.

**Alternatives:** UI automation or keystroke injection. Both are slow, break when the layout changes, and fail silently. A CLI flag on `cooldesk.exe` was also rejected: single-instance forwarding can't return output to the caller.

**Why:** the sidecar already runs inside the app process, so it can call the same Rust functions the UI calls.

## D2. Token auth on a new `/cli/*` namespace, failing closed (P1)

**Decision:** a 256-bit token is generated from the OS RNG at every start and written atomically to `%LOCALAPPDATA%\com.cooldesk.desktop\cli-token`, but only after the listener binds. `/cli/*` requires `Authorization: Bearer <token>`, compared in constant time, and refuses any request that carries an `Origin` header. No data directory means no `/cli` routes at all.

**Context:** the sidecar admits any request without an Origin (`server.rs`: `None => true`). That was tolerable for notes; it isn't tolerable for `kill_process` or `terminal_write`.

**Alternatives:** gate every existing route. That would break the browser extension and the frontend shim, which don't send a token. Left to Mike as a separate decision.

**Why:** the token file is per-user (Windows ACLs on LOCALAPPDATA), so holding it means being the user. The Origin check stops a web page from reaching `/cli` even if it somehow learns the token. Writing the token only after the bind (H7) means the file never points at a server that failed to start.

## D3. One dispatch table, called with the same functions as the UI (P2)

**Decision:** `GET /cli/commands` returns a manifest. `POST /cli/invoke {cmd, args, confirm}` runs a command by calling the `#[tauri::command]` function directly (`crate::launch_app(...)`), with the `AppHandle` held in a `OnceLock` set in `setup`.

**Alternatives:** split every command into a core function plus a thin wrapper (the original proposal). That turned out to be unnecessary: child modules can see the crate root's private items, and Tauri async commands are already `Send` (H8). That kept the upstream diff tiny.

**Why:** the UI and the CLI run identical code, so their behaviour can't drift.

## D4. Each command declared once (`cli_commands!` macro)

**Decision:** each command is one line: name, tier, summary, typed args, call. The macro generates the manifest entry, a `deny_unknown_fields` args struct, and the dispatch arm. Arg types in the manifest come from the Rust types; `Option<T>` means optional.

**Context:** 123 commands with three hand-kept places each would be 369 chances to drift.

**Evidence:** 8 slices were added with no arg-mismatch failures. A test builds every command's args from its own manifest entry and parses them (H10).

## D5. Danger tiers enforced server-side, before parsing

**Decision:** four tiers. `read`; `act` (nothing is lost); `destructive` (kill, delete, close, run arbitrary input, change a security setting); `secret` (the vault). `destructive` and `secret` return **428** unless `confirm: true`, and the gate runs before the args are parsed or anything executes.

**Classification rules found during the work:**
- `save_backup` is destructive because it prunes old backups.
- `clipboard_set_filters` is destructive because `skip_secrets=false` starts recording passwords.
- `trust_project_dir` is destructive because it writes Claude Code's trust config.
- `terminal_write` is destructive because it types into a shell.
- `vault_generate` is read: it stores nothing.

**Why server-side:** every client (CLI, MCP, a future UI) gets the same gate. A client can't forget it.

## D6. Parity gate (P3)

**Decision:** a test parses `generate_handler![…]` in lib.rs and fails if any IPC command is neither exposed nor in `CLI_EXCLUDED` with a reason. It fails closed: fewer than 100 parsed names, or attributes in the list, fail it. It also fails on stale exclusions and on CLI-only names not declared in `CLI_ONLY`.

**Evidence:** deleting one exclusion made it fail and name that command. Today: 140 IPC = 123 exposed + 17 excluded, plus 1 CLI-only (`terminal_list`).

**Exclusions are by nature, not by effort:** commands that need the calling webview's `Window`, stream output over an IPC `Channel`, are UI handoff queues, or are pointer-driven layout.

## D7. Terminals addressed as (webview label, id)

**Decision:** the CLI resolves the `Webview` by label (`main`, `sidebar`, `bar`, `spotlight`, `app-*`) and calls the unchanged IPC functions, so their `require_main` check still applies. A read-only `terminal::session_list()` was added so the CLI can discover sessions.

**Not exposed:** `terminal_open`. Its output streams over the opener's IPC Channel, so the CLI couldn't read it. To run something, use `spotlight_run_in_terminal` (it's visible in CoolDesk) or `run_project_command` (it opens a new console).

## D8. MCP as a stdio server inside the CLI, not on the app's `/mcp` (P4)

**Decision:** `cooldesk-cli mcp serve` exposes the manifest (non-secret tiers by default) plus the app's 7 legacy `/mcp` read tools. It's registered user-scope in Claude Code.

**Why not `/mcp`:** it has no auth, and action tools there would bypass D2.

**Why not HTTP with a header:** the token rotates on every start, so a static header goes stale. The stdio server re-reads it per call.

**Why hand-rolled, not rmcp:** the protocol subset is small, and the app's own mcp.rs already hand-rolls it. rmcp brings tokio into a sync CLI and wants tools at compile time, but ours arrive at runtime.

**Details:**
- Destructive and secret tools carry `confirm: {const: true}` in their schema.
- With the app down, `tools/list` returns `[]`, and the server sends `tools/list_changed` when the app appears.
- **The client has no connection pooling.** Without that, the first call after an app restart fails (os error 10054, H14). Retrying would be wrong because `/cli/invoke` isn't idempotent.

## D9. A fork release channel

**Decision:**
- Updater endpoint: the fork's GitHub releases, signed with the fork's own minisign key.
- The key is password-protected. The key file is outside the repo (`~/.tauri/cooldesk-fork.key`), and the key and its password are in the keyring under project `cooldesk-release` (`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`).
- Build with `keyring exec -p cooldesk-release -- npm run build:tauri`.
- Don't generate keys with `--ci` and no `-p`: that key couldn't be unlocked (H16).
- The update banner reads the fork's releases.
- The analytics heartbeat and first-run ping are **off** unless a build sets `COOLDESK_ANALYTICS_ENDPOINT`.
- `run_winget_upgrade` refuses unless a build sets `COOLDESK_WINGET_UPGRADES`.

**Context:** with upstream's wiring, a fork build would install upstream's next signed release over itself and silently drop `/cli`. winget matches installs by name and publisher and would do the same. The heartbeat went to upstream's analytics worker, which isn't ours to report to.

## D10. The CLI binary is `cooldesk-cli`, and it lives in the fork

**Decision:** the crate is `cooldesk-cli/` at the repo root, a standalone package (no Cargo workspace), installed with `cargo install --path cooldesk-cli`.

**Why the name:** `cooldesk` would collide with the app's own `cooldesk.exe` on PATH.

**Why in the fork:** the app-side branches stay small, so P1–P4 can be offered upstream.

## D11. Test-binary manifest (build.rs)

**Decision:** on Windows/MSVC, the Common-Controls v6 manifest is embedded by the linker for every target, instead of by tauri-build (which does bins only). It's tauri-build's default file verbatim, with `/MANIFESTUAC:NO`.

**Context:** once a lib unit test linked window code, the test binary died before `main` with STATUS_ENTRYPOINT_NOT_FOUND (`TaskDialogIndirect` is missing from comctl32 v5).

**Evidence:** the app's PE manifest was compared before and after; it's equivalent.

## D12. Live tests: bundled UI, restore state, no keystrokes

**Rules learned the hard way:**
- **Build live-test binaries with `--features tauri/custom-protocol`.** A plain debug build loads devUrl `localhost:5173`; with no dev server, WebView2 shows Edge's error page (H11).
- **Restore every persisted change and compare against the before-state.** Never print clipboard or vault contents.
- **UI tabs persist in the WebView profile, which the installed app shares (H12).** Report them; never close them with synthetic keystrokes.
- **On a port held by a dead pid, look at the surviving children (H6).** Test on port 4546 instead.

## Known issues

- `dir_watch::tests::a_download_burst_is_one_update` (upstream code) is timing-flaky under the parallel suite. It failed 2/4 full runs on unmodified code and passes 6/6 in isolation.
- Widgets (the new-tab board) persist only in WebView or extension localStorage (`cooldesk-widget-layouts`). No IPC command or sidecar route reaches them, so they're UI-only for now.
- A workspace POSTed through the sidecar keeps only `id, name, urls, apps, shells, servers` and the timestamps (`sidecar/data.rs`). UI-only fields such as `description`, `color` and `status` are dropped on that path.
