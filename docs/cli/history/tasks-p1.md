# P1 — token auth for `/cli/*` (CoolDesk branch `cli/p1-token-auth`)

Scope: protect the new `/cli/*` namespace that P2 will build on. The existing ~90 routes are left as they are, because tightening them would also mean changing the extension and the frontend shim. That decision is listed separately at the end.

- [x] 1. Branch `cli/p1-token-auth` off master in the clone.
- [x] 2. `sidecar/cli_auth.rs`:
  - generate a 256-bit token at startup with the OS RNG
  - write it atomically to `<local data dir>/com.cooldesk.desktop/cli-token` (0600 on unix; Windows `%LOCALAPPDATA%` is already per-user)
  - a middleware that requires `Authorization: Bearer`, compares in constant time, and rejects any request that carries an Origin
- [x] 3. Fail closed: if the token can't be created, the `/cli/*` routes are never mounted. An empty expected token never matches.
- [x] 4. Add `GET /cli/ping`, the first gated route, so the gate can be tested.
- [x] 5. Unit tests (tower oneshot):
  - no header → 401
  - wrong token → 401
  - right token → 200
  - right token with an Origin → 403
  - empty expected token → 401
  - `/health` is still open
- [x] 6. CLI: read the token file, send the header on `/cli/*`, `auth status` command, exit 4 on 401/403.
- [x] 7. Build the app, swap it for the installed instance, verify live, then restore the installed instance.
- [x] 8. Commit on the branch, update WORKLOG and hypothesis.

Deferred (Mike's call): putting the old data routes behind the same token. The extension and `electron-shim.js` would need the token too.

## Plan changes during the work (documented)
- Step 7: there was no installed instance to swap. The old CoolDesk had exited at about 23:34, but its listening socket on 4545 was still held by the terminal it had launched (this Claude session). Added a `COOLDESK_SIDECAR_PORT` test override and ran the build on 4546 instead.
- Step 3: tightened it. The token is now written only after the bind succeeds (`PendingToken::persist`), because the first live run wrote a token for a server whose bind had failed.
