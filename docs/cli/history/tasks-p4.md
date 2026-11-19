# P4 — MCP backed by the same manifest (written before the work, 2026-11-19)

Goal: Claude Code (and any MCP client) can drive CoolDesk with the same commands, tiers and auth as the CLI.

Design decision: a **stdio MCP server inside cooldesk-cli** (`cooldesk mcp serve`). NOT the app's existing `/mcp`:
- `/mcp` has no auth (no-Origin pass-through, server.rs). Putting act/destructive tools there would bypass the P1 token.
- The token rotates on every app start, so a fixed `Authorization` header in Claude Code's config would go stale. The stdio server reads the token file at call time.
- No new app code: it consumes `GET /cli/commands` and `POST /cli/invoke`, so a command added to the app appears in MCP with no release.
- Hand-rolled protocol (initialize, ping, tools/list, tools/call): it's a small subset, and the app's own mcp.rs already hand-rolls it. rmcp would add tokio to a sync CLI and wants tools known at compile time, but ours come from the app at runtime.

- [x] 1. `src/mcp_serve.rs`: newline-delimited JSON-RPC on stdin/stdout. Logs go to stderr only. Unknown method → -32601. Notifications get no reply.
- [x] 2. Tools = the manifest filtered by `--tiers` (default `read,act,destructive`; `secret` only when listed explicitly) plus the app's 7 legacy `/mcp` read tools (proxied; the manifest wins on a name clash).
- [x] 3. Schemas from the manifest's arg types. Destructive/secret tools require `confirm: true` (`const: true`); the proxy moves it into `/cli/invoke`'s `confirm`. Annotations: readOnlyHint (read), destructiveHint (destructive/secret).
- [x] 4. App down: `tools/list` returns what's reachable, possibly `[]`. It advertises `listChanged`, and a poll thread sends `notifications/tools/list_changed` when the tool set changes.
- [x] 5. App-side errors (4xx/422, unreachable) → a `tools/call` result with `isError: true` and the message. JSON-RPC errors only for protocol faults.
- [x] 6. Unit tests on the pure parts: schema mapping, tier filter, confirm stripping, the secret opt-in.
- [x] 7. Live protocol test: a scripted client over stdio against the app on 4546. initialize → list → a read call → an act call → destructive without and with confirm → tier filter → secret absent by default.
- [x] 8. Install the binary at a stable path and register it user-scope (`claude mcp add cooldesk -s user -- <exe> mcp serve`). `claude mcp list` must show it Connected.
- [x] 9. End to end: a headless `claude -p` with `--mcp-config` pointed at 4546 calls a CoolDesk tool and reports the result.
- [x] 10. README, WORKLOG, hypothesis, proposal checkbox.

## Results (2026-11-19)
- Unit tests: 8 pass (`cargo test` in cooldesk-cli).
- Live protocol test (scratchpad mcp_live.py, stdio against 4546): 26 checks, ALL PASS after one fix (below).
- Registered: `claude mcp add cooldesk -s user -- C:/Users/mikes/.cargo/bin/cooldesk-cli.exe mcp serve` → `claude mcp get cooldesk`: ✔ Connected (user scope).
- E2E: `claude -p --model haiku --mcp-config <4546 config> --strict-mcp-config` answered "version: 2.0.22 / port 4546 owner: cooldesk.exe". The transcript has tool_use for get_app_version and list_listening_ports. $0.0048.

## Plan changes during the work (documented)
- A bug the live test found: after a CoolDesk restart, the first call failed with os error 10054. ureq's pooled keep-alive socket pointed at the dead server, and ureq doesn't retry POST. Fix: `max_idle_connections(0)` in Client. A retry would have been wrong, since /cli/invoke isn't idempotent.
- A harness bug, not the product: app.sh's `cd && exe &` held the caller's stdout pipe open. I fixed the redirect to cover the whole subshell.
- Not done: an opt-in to put secret tools in the registered config. Default stays off, matching the proposal; `--tiers …,secret` exists for anyone who wants it.
