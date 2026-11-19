# Changes in this fork

This repository, `LeverageAISMBs/cooldesk-extension`, is an **unofficial fork** of [CoolDesk](https://github.com/abhayraghuwanshi/cooldesk-extension) by Abhay Raghuwanshi, licensed under the Apache License 2.0 (see [LICENSE](LICENSE)). It isn't affiliated with or endorsed by the original author. "CoolDesk" is the original project's name.

Per Apache-2.0 §4(b), this file states what the fork changed. Each modified upstream source file also carries a one-line notice at the top.

## What the fork adds

A command-line and MCP control surface for the desktop app, so people and AI agents can operate it without the UI. Full design record: [docs/cli/DECISIONS.md](docs/cli/DECISIONS.md). Operator manual: [docs/cli/OPERATOR-GUIDE.md](docs/cli/OPERATOR-GUIDE.md).

## Modified upstream files

| File | Change |
|---|---|
| `src-tauri/src/lib.rs` | Stores the `AppHandle` for the CLI dispatch. Fork release channel: the update check reads the fork's releases, the analytics heartbeat is off unless `COOLDESK_ANALYTICS_ENDPOINT` is set at build time, and `run_winget_upgrade` is refused unless `COOLDESK_WINGET_UPGRADES` is set |
| `src-tauri/src/sidecar/server.rs` | Mounts the token-gated `/cli/*` routes; `COOLDESK_SIDECAR_PORT` test override; the token is written after the bind |
| `src-tauri/src/sidecar/mod.rs` | Registers `cli_auth` and `cli_invoke` |
| `src-tauri/src/terminal.rs` | Adds read-only `session_list()` so the CLI can address open terminals |
| `src-tauri/build.rs` | On Windows/MSVC, embeds the app manifest through the linker for every target (so unit tests that link window code can load) |
| `src-tauri/tauri.conf.json` | Updater endpoint and public key point at the fork's releases and signing key |
| `src-tauri/Cargo.toml`, `Cargo.lock` | Dependencies for the CLI auth (OS RNG) |
| `src-tauri/windows-app-manifest.xml` | New file: tauri-build 2.5.5's default manifest, verbatim |

## New files

- `src-tauri/src/sidecar/cli_auth.rs`, `src-tauri/src/sidecar/cli_invoke.rs`: the `/cli` API (P1–P3)
- `cooldesk-cli/`: the CLI and its MCP server (P0, P4, P5)
- `docs/cli/`: proposal, decisions, operator guide, generated reference, hypothesis log, worklog
- `FORK-CHANGES.md`: this file

## Upstream contributions

The app-side changes are kept small and live on separate branches (`cli/p1-token-auth` … `cli/p3-parity-gate`) so they can be offered upstream.
