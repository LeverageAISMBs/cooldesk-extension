# cooldesk-cli — P0 tasks

Goal: a CLI that drives CoolDesk through what the app already exposes (sidecar HTTP on 127.0.0.1:4545, `/ws`, and `/mcp`), with no changes to CoolDesk. Proposal: `../CLI-PROPOSAL.md`.

- [x] 1. Scaffold the Rust crate outside the clone: clap, ureq, serde_json, tungstenite.
- [x] 2. Global options: `--port` / `COOLDESK_PORT`, `--compact`. Exit codes: 0 ok, 1 HTTP error, 2 usage, 3 app not running.
- [x] 3. Read commands: health, ws, urls, tabs, notes, url-notes, pins, settings, activity (list, focused, visible, apps, sites), search (apps, history), graph, dashboard, ui-state, daily-memory, servers, llm (status, models, config).
- [x] 4. Write commands (the server merges, nothing is removed): ws add, urls add, notes add, pins add, settings set, tabs jump/close, llm load/unload/chat.
- [x] 5. mcp tools / mcp call.
- [x] 6. `watch`: WS event stream as NDJSON, with a type filter.
- [x] 7. `raw get|post <path>` escape hatch.
- [x] 8. Live verification of every command against the running app. Record the output in WORKLOG.md.
- [x] 9. README with the command list, known gaps (no delete, no IPC actions), and the exit codes.

Deliberately excluded: `GET /cmd/jump-next` and `/cmd/close-next`. They remove items from the extension's poll queue (`handlers.rs:1352`), and calling them would steal jumps meant for the extension.
