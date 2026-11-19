# Launch — tasks (written before the work, 2026-11-19)

Mike: "change the origin of the repo to our forked version, commit locally, then push remote. Then proceed with the next steps … run the built-from-source version. Do the Docs manually and commit them before running the docs from the command list … detailed, granular instructions so an AI model with 0 context could … operate the application … terminal and browser, all file operations, customizations, widgets … document reasoning behind architecture and engineering decisions … let's get this baby launched."

- [x] 0. Origin is already the fork (`origin` = LeverageAISMBs/cooldesk-extension, `upstream` = abhayraghuwanshi). **Push is blocked**: the fork is owned by the LeverageAISMBs user, the working credential is mikeschlottig (READ), the stored LeverageAISMBs gh token is invalid, and the SSH key isn't registered. Mike to re-auth or add a collaborator. Everything else goes ahead locally.
- [ ] 1. Bring `cooldesk-cli` into the fork as a top-level crate (`cooldesk-cli/`), and move the proposal and plans into `docs/cli/`. Archive the old standalone copy (date-time suffix). Branch `cli/p4-mcp-and-cli-crate`.
- [ ] 2. Fork release config: updater endpoints → the fork's GitHub releases, with a new signing keypair (private key never committed). Upstream endpoints removed, so a fork build can't "update" itself back to upstream and drop /cli. Branch `release/fork-launch`.
- [ ] 3. Research the UI's features (widgets, customizations, settings keys) from code and docs. Query, don't read trees.
- [ ] 4. Hand-written docs: `docs/cli/OPERATOR-GUIDE.md` (zero-context AI operator manual: terminal, browser, files, customization, widgets, safety, recipes, troubleshooting) and `docs/cli/DECISIONS.md` (architecture and engineering decisions with reasoning). Commit BEFORE any generated docs.
- [ ] 5. P5: `cooldesk docs` (a reference generated from the live manifest), `cooldesk completions <shell>`, `cooldesk batch` (NDJSON). Commit the generated reference separately.
- [ ] 6. Build the installer from source (`npm run build:tauri`, signed), install it, and run it as the daily app on 4545. Verify live: the CLI, MCP (full tool set now), the UI screenshot.
- [ ] 7. Fast-forward the fork's master to the tip. Push all branches + master when auth is fixed.
- [ ] 8. WORKLOG, hypothesis, memory.
