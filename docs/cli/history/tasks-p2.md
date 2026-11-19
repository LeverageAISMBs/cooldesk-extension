# P2.1 — action plane, slice 1: launcher and windows (CoolDesk branch `cli/p2-launcher-windows`, stacked on `cli/p1-token-auth`)

Scope: proposal §7 P2 item 1 only. Later slices (search/folders, spotlight/dock, shelf, clipboard, browser_pane, terminal, processes/updates, vault) are separate branches. Nothing in this slice kills processes, writes to terminals or reads the vault.

- [x] 1. Branch `cli/p2-launcher-windows` off `cli/p1-token-auth`.
- [x] 2. `sidecar/cli_invoke.rs`:
  - `OnceLock<AppHandle>`, set in `lib.rs` setup before `start_server` is spawned
  - static manifest: name (= the IPC command name, so the P3 parity gate can match `generate_handler!`), tier, summary, args
  - `parse_call(name, args) -> Call` (typed args, `deny_unknown_fields`) kept separate from `execute(Call)`, so tests can check the parsing without launching anything
  - tier gate before parsing: `destructive` needs `"confirm": true`, otherwise 428
  - statuses: 200 ok · 400 bad args · 404 unknown cmd · 422 command returned Err · 428 confirmation required · 503 no AppHandle
- [x] 3. Mount `GET /cli/commands` and `POST /cli/invoke` behind the P1 token middleware.
- [x] 4. Commands:
  - read: get_running_apps, get_installed_apps, get_focused_app, app_windows_list, list_window_tabs
  - act: launch_app, launch_app_with_args, open_url, open_folder, focus_window, focus_window_tab, app_window_focus, main_window_hide, open_new_window
  - destructive: close_app, app_window_close, run_project_command
  - excluded (UI-only): app_window_set_space (needs the caller's webview `Window`)
- [x] 5. Tests: every manifest entry parses from args built from its own declared schema; missing required → 400; unknown field → 400; unknown cmd → 404; destructive without confirm → 428 (before any execution); names unique; the route sits behind the token.
- [x] 6. CLI: `commands` (manifest), `invoke <cmd> [--json '{..}'] [--yes]`, exit 5 on 428.
- [x] 7. Build the app, run it on test port 4546, and verify live: the manifest, one read, one act (open_url / launch notepad), and destructive with and without --yes (close the notepad it launched).
- [x] 8. Full app test suite, then commit, then update WORKLOG, hypothesis, and the proposal checkbox.

## Plan changes during the work (documented)
- Step 5: the first test run died before main with STATUS_ENTRYPOINT_NOT_FOUND. Not a test failure: tauri-build links the Common-Controls v6 manifest into bins only, and once the lib test binary linked window code (through `execute`) it imported `TaskDialogIndirect`, which comctl32 v5 doesn't have. `cargo:rustc-link-arg-tests` is invalid without a `tests/` target. Fix: `build.rs` now embeds tauri-build's default manifest (copied verbatim) through the linker for every target, with `/MANIFESTUAC:NO`. The app's PE manifest was compared before and after and is equivalent. That's a change to the app's build and is called out in the commit.
- Step 7: used Character Map rather than Notepad. Windows 11 Notepad can reuse an existing process, so `close_app` by pid could have closed one of Mike's windows.
