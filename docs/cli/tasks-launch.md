# Launch — tasks (written before the work, 2026-11-19)

Mike: "change the origin of the repo to our forked version, commit locally, then push remote. Then proceed with the next steps … run the built-from-source version. Do the Docs manually and commit them before running the docs from the command list … detailed, granular instructions so an AI model with 0 context could … operate the application … terminal and browser, all file operations, customizations, widgets … document reasoning behind architecture and engineering decisions … let's get this baby launched."

- [x] 0. Origin is already the fork (`origin` = LeverageAISMBs/cooldesk-extension, `upstream` = abhayraghuwanshi). **Push is blocked**: the fork is owned by the LeverageAISMBs user, the working credential is mikeschlottig (READ), the stored LeverageAISMBs gh token is invalid, and the SSH key isn't registered. Mike to re-auth or add a collaborator. Everything else goes ahead locally.
- [x] 1. Bring `cooldesk-cli` into the fork as a top-level crate (`cooldesk-cli/`), and move the proposal and plans into `docs/cli/`. Archive the old standalone copy (date-time suffix). Branch `cli/p4-mcp-and-cli-crate`.
- [x] 2. Fork release config: updater endpoints → the fork's GitHub releases, with a new signing keypair (private key never committed). Upstream endpoints removed, so a fork build can't "update" itself back to upstream and drop /cli. Branch `release/fork-launch`.
- [x] 3. Research the UI's features (widgets, customizations, settings keys) from code and docs. Query, don't read trees.
- [x] 4. Hand-written docs: `docs/cli/OPERATOR-GUIDE.md` (zero-context AI operator manual: terminal, browser, files, customization, widgets, safety, recipes, troubleshooting) and `docs/cli/DECISIONS.md` (architecture and engineering decisions with reasoning). Commit BEFORE any generated docs.
- [x] 5. P5: `cooldesk docs` (a reference generated from the live manifest), `cooldesk completions <shell>`, `cooldesk batch` (NDJSON). Commit the generated reference separately.
- [x] 6. Build the installer from source (`npm run build:tauri`, signed), install it, and run it as the daily app on 4545. Verify live: the CLI, MCP (full tool set now), the UI screenshot.
- [ ] 7. Fast-forward the fork's master to the tip. Push all branches + master when auth is fixed.
- [ ] 8. WORKLOG, hypothesis, memory.

## Results (2026-11-19)
- 1: crate and records in the repo (261faf9); the standalone copy archived in C:\Dev\_archive.
- 2: fork release channel (2db88d0). The first key was unusable (`--ci` with no `-p`, H16), so it was replaced with a passworded key in the keyring (5efd070).
- 3: research done. Widgets and language/theme are localStorage-only. Sidecar settings hold `spotlightShortcut` plus `data.{historyDays, visitCountThreshold, modelName, geminiApiKey}`.
- 4: OPERATOR-GUIDE.md + DECISIONS.md committed before any generated docs (8df8006).
  - **Zero-context acceptance test:** headless `claude -p --model haiku`, given only the guide and a `cooldesk-cli`-only tool allowance. It got 6/6 tasks right, including refusing the widget trap with the right reason. 7 turns, $0.0136, 0 denials, nothing destructive run. Its answers were checked against reality.
- 5: P5 (94aba28): docs, completions (verified in real PowerShell with TabExpansion2), batch. The generated reference is committed separately (43c87da).
- 6: signed installer built from source (NSIS + MSI, plus updater .sig files), installed silently to %LOCALAPPDATA%\CoolDesk, running on 4545.
  - Pre-flight, the browser open→jump→close round trip, the spotlight-hotkey round trip and the winget refusal were verified on it.
- Added (Apache-2.0 compliance): FORK-CHANGES.md plus §4(b) notices in the modified upstream files.

## Plan changes (documented)
- The installer was built before the docs were finished, so the guide could be verified against the real daily app. The plan order allowed that.
- Left in the user's data on purpose (no delete API exists): the space "CLI guide check (safe to delete)" and the older "CLI Smoke Test" (P0 session). Also two restored test terminal tabs (H12).
