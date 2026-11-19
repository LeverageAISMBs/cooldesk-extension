# cooldesk-cli — hypothesis log

think → analyze → act → observe → reason → document → proceed

## H1 — the sidecar accepts a request with no Origin
- Prediction: `is_allowed_origin` returns true when there is no Origin header (`server.rs:357`), so a plain HTTP client gets in.
- Reality (2026-11-18): `curl /health` → `{"ok":true,...}`. `/workspaces`, `/search` and `/mcp` all answered.
- Diff: none.

## H2 — POST routes merge rather than replace
- Prediction: a POST with one item adds it and keeps the rest.
- Reality from source: workspaces merge by lowercased name (`sync.rs:134`), notes and pins by `id` (`sync.rs:46`), ui-state with `extend`, settings with `merge_settings`. No route ever removes an item.
- Consequence: P0 can add and update but cannot delete. Delete needs a new route (P2).

## H3 — `watch` gets broadcasts without being identified as a browser
- Prediction: `handle_ws_connection` subscribes every client to the broadcast channel, so an unidentified client still gets `push-*` and `request-tabs` traffic.
- Reality: confirmed. The watch client received notes-, pins-, workspaces-, urls- and ui-state-updated without identifying as a browser.

## H4 — the sidecar reports failure through HTTP status
- Prediction: errors come back as 4xx/5xx.
- Reality: `/llm/v3/simple-chat` with no API key returned 200 `{"ok":false,...}`.
- Diff: failures arrive inside 200 responses. The CLI now exits 1 on `ok:false` or `success:false`.

## H5 — a path-prefix guard is enough to block the dequeue routes
- Prediction: `starts_with("/cmd/jump-next")` blocks it.
- Reality: Git Bash rewrote the argument to `C:/Program Files/Git/cmd/jump-next`, so the guard passed it through and the request 404d.
- Diff and why: shell path rewriting changes the argument before the CLI sees it. Guards now match a substring, case-insensitive.

## H6 — the live test can swap the installed app for the dev build
- Prediction: quit the installed app, run the build on 4545, test, relaunch the installed app.
- Reality: the installed app was already gone, but 4545 was still LISTENING under its dead pid 20028. The bind failed with 10048, and connections were accepted but never answered (curl exit 28).
- Why: the only surviving child of 20028 was `cmd /k claude "/cd-init"`, this session's terminal, launched from CoolDesk. It inherited the socket handle and keeps it open.
- Lesson: on Windows, a port owned by a dead pid means look at that pid's surviving children. Test on a second port instead of killing them.

## H7 — writing the token at router build time is fine
- Prediction: the token file can be written before the bind.
- Reality: the failed-bind run still rotated the token file, which then pointed at a dead server.
- Diff: the token is now written after a successful bind.

## H8 — IPC command fns are callable from the sidecar with no refactor
- Prediction: the slice-1 commands are private `async fn`s at the crate root. Child modules can see their ancestors' private items, so `crate::launch_app(...)` compiles from `sidecar::cli_invoke` with no `pub` and no core/wrapper split. Their futures are `Send` (Tauri requires that for async commands), so they're valid inside axum handlers.
- Reality (2026-11-19): compiled first try. No `pub`, no wrapper split for this slice.
- Side effect, not predicted: the lib's test binary now links window code and failed to load (STATUS_ENTRYPOINT_NOT_FOUND, `TaskDialogIndirect`). Why: tauri-build's manifest resource goes into bins only. Lesson: on Windows Tauri, the first unit test that reaches window or dialog code bricks the whole lib test binary. The fix is the linker-embedded manifest for all targets, not stubbing the code out of tests.

## H9 — focus_window from the sidecar can bring a window to the foreground
- Prediction: partial. Windows' foreground lock refuses `SetForegroundWindow` from a process that didn't get the last input. The CLI caller is the foreground process, not CoolDesk. Expect either success, if focus.rs uses the AttachThreadInput/ALT trick, or a taskbar flash.
- Reality (2026-11-19): full success. `get_focused_app` read straight after showed CoolDesk after `app_window_focus main`, then charmap after `focus_window {pid}`. No flash.
- Diff: I was too pessimistic. focus.rs already gets around the foreground lock, so the CLI inherits that for free.

## H10 — one macro declaration per command removes manifest/struct/dispatch drift
- Prediction: generating the manifest args, the typed struct and the dispatch arm from one `cli_commands!` entry makes "manifest says X, parser wants Y" impossible, and `Option<T>` → optional derives from the type text.
- Reality (2026-11-19): compiled first try. 123 IPC commands were added across 8 slices with no arg-mismatch failures. The test that builds args from the manifest and parses them passed every slice.

## H11 — CoolDesk debug builds are fine for live tests
- Prediction (implicit): a debug build behaves like the app.
- Reality: Mike saw an "Edge" window. A debug build loads devUrl `localhost:5173`, which wasn't served, so WebView2 showed Edge's "can't reach this page". Chrome is his default browser; nothing had opened a browser.
- Diff and why: the dev URL vs the bundled frontend. Build live-test binaries with `--features tauri/custom-protocol`. The screenshot then showed the real UI.

## H12 — closing a terminal session cleans up after a test
- Prediction: `terminal_close` leaves no trace.
- Reality: the shell exits, but the UI keeps the tab, and the tab list is in the WebView's localStorage (`EBWebView/Default/Local Storage`), which the installed app shares. On the next start the UI restored my slice-3 test tab and spawned a new shell for it.
- Lesson: on a Tauri app, UI-created state lives in the WebView profile, not the sidecar store. A live test that opens UI tabs leaks into the user's real app. Don't close them with synthetic keystrokes (Ctrl+W could hit the user's focused window). Report them instead.

## H13 — "120 exposed + 20 excluded"
- I wrote this coverage figure into the P3 commit message without counting.
- Reality: grep count = 123 exposed + 17 excluded (+1 CLI-only), tiers 41/51/23/9. I amended the unpushed commit's message.
- Why: I estimated instead of measuring, in the one place a number gets frozen into history. The rule already exists (claims need proof in the same turn), and I skipped it.

## H14 — a stdio proxy that reads the token per call survives app restarts
- Prediction: the server holds no token. Every /cli request re-reads `cli-token`, so restarting CoolDesk (new token) needs no restart of the MCP server or of Claude Code.
- Reality (2026-11-19): the token part held: the restart rotated the token and no 401 followed. But the first call after the restart FAILED: os error 10054, connection reset.
- Diff and why: I predicted only around the token and missed the transport. ureq pools keep-alive sockets. The pooled socket pointed at the dead process, and ureq retries only idempotent methods, so the POST failed. Fix: `max_idle_connections(0)`. After it, the same check passes.
- Lesson: a long-lived client of a restartable local server must not pool connections, or it must treat a reset on a reused socket as "never sent". Don't blindly retry non-idempotent calls.

## H15 — Claude Code shows a stdio server as Connected even when CoolDesk is down
- Prediction: initialize doesn't touch the app, so `claude mcp list` says Connected whether or not CoolDesk runs. With the app down, tools/list returns [] (or only legacy tools).
- Reality: confirmed. `claude mcp get cooldesk` showed ✔ Connected with nothing on 4545. The live test got tools/list = [] with the app down, then notifications/tools/list_changed within the poll window once it started, then 122 tools.

## H16 — `tauri signer generate --ci` with no `-p` gives an empty-password key
- Prediction: `--ci` skips the prompt, so the key has an empty password, and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""` signs with it.
- Reality (2026-11-19): the 8-minute release build compiled and bundled, then failed at signing with "incorrect updater private key password: Wrong password for that key". `tauri signer sign -f key -p ""` failed the same way. Without `-p` it waited for a prompt.
- Diff and why: `--ci` without `-p` doesn't produce a key that "" unlocks. I trusted a flag's name instead of round-tripping (generate, then sign a dummy file) before spending a full build.
- Fix and lesson: generate with an explicit random `-p` and store both the key and the password in the keyring. **Round-trip any generated credential (sign + verify a dummy) before relying on it.**

## H17 — upstream's test suite is green
- Prediction (implicit): "150 passed" means it's stable.
- Reality: `dir_watch::tests::a_download_burst_is_one_update` failed 2/4 full runs on unmodified code and passed 6/6 in isolation. Earlier green runs were luck.
- Lesson: one green run of a suite with sleep-based tests isn't evidence of stability. Re-run before attributing a failure, and before claiming "all pass".

## H18 — BrowserMCP drives the logged-in profile with sessions intact
- Prediction: unlike scrapling's persistent_profile (a fresh, logged-out Chrome) and CDP (Chrome 136+ refuses it on the default profile), the Browser MCP extension (v2.2.3, installed in Default and Profile 1) acts inside the real profile, so logged-in sites stay logged in.
- Reality: (P6.0, next session; the MCP server is configured but only loads in a fresh session)

## H19 — BrowserMCP's input on SPA controls
- Prediction: clicks work on most SPA controls. If it uses chrome.debugger, its events are isTrusted, and Chrome shows the debugging infobar.
- Reality: (P6.0)

## H20 — P6.1–P6.3 need no extension change
- Prediction: workspace artifacts, the downloads sweep, history export and workflows can all be built in the CLI/sidecar. History comes through the existing CoolDesk extension (store 2.0.19, Default profile).
- Reality: (P6.1–P6.3)
