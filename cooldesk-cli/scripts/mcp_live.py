"""Run: python -I scripts/mcp_live.py   (needs the app built, see app.sh; CLI built --release)
Live stdio-protocol test of `cooldesk-cli mcp serve` against CoolDesk on port 4546.
Phases: app down at start -> list_changed after app starts -> calls -> app restart (token rotation) -> tier filters.
Prints PASS/FAIL per check; exit 1 on any FAIL."""
import json, os, queue, subprocess, sys, threading, time

CLI = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "target", "release", "cooldesk-cli.exe")
APP_SH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "app.sh").replace("\\", "/")
TOKEN = os.path.join(os.environ["LOCALAPPDATA"], "com.cooldesk.desktop", "cli-token")
BASH = r"C:\Program Files\Git\bin\bash.exe"
fails = []

def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (f"  [{detail}]" if detail else ""), flush=True)
    if not ok:
        fails.append(name)

def app(cmd):
    out = subprocess.run([BASH, APP_SH, cmd], capture_output=True, text=True).stdout.strip()
    print(f"  app {cmd}: {out}", flush=True)

class Mcp:
    def __init__(self, *extra):
        self.p = subprocess.Popen([CLI, "--port", "4546", "mcp", "serve", *extra],
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                  text=True, encoding="utf-8", bufsize=1)
        self.q, self.n = queue.Queue(), 0
        threading.Thread(target=self._read, daemon=True).start()
    def _read(self):
        for line in self.p.stdout:
            self.q.put(json.loads(line))
    def send(self, method, params=None, notify=False):
        msg = {"jsonrpc": "2.0", "method": method}
        if params is not None: msg["params"] = params
        if not notify:
            self.n += 1; msg["id"] = self.n
        self.p.stdin.write(json.dumps(msg) + "\n"); self.p.stdin.flush()
        return None if notify else self.wait(lambda m: m.get("id") == self.n)
    def wait(self, pred, timeout=60):
        end = time.time() + timeout; skipped = []
        while time.time() < end:
            try: m = self.q.get(timeout=0.5)
            except queue.Empty: continue
            if pred(m):
                for s in skipped: self.q.put(s)
                return m
            skipped.append(m)
        for s in skipped: self.q.put(s)
        return None
    def call(self, name, args=None):
        return self.send("tools/call", {"name": name, "arguments": args or {}})["result"]
    def close(self):
        self.p.stdin.close(); self.p.wait(timeout=10)

def text(r): return r["content"][0]["text"]

# ---- Phase A: app down when the server starts ----
app("stop")
m = Mcp("--poll", "2")
r = m.send("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "live-test", "version": "0"}})
check("initialize answers with app down", r["result"]["serverInfo"]["name"] == "cooldesk", r["result"]["protocolVersion"])
m.send("notifications/initialized", notify=True)
tools = m.send("tools/list")["result"]["tools"]
check("tools/list with app down is empty, not an error", tools == [], f"{len(tools)} tools")

app("start")
note = m.wait(lambda x: x.get("method") == "notifications/tools/list_changed", timeout=60)
check("list_changed notification after app starts", note is not None)
tools = m.send("tools/list")["result"]["tools"]
by = {t["name"]: t for t in tools}
check("tools/list after start", len(tools) > 100, f"{len(tools)} tools")
check("secret (vault_get) absent by default", "vault_get" not in by and "vault_status" in by)
check("legacy /mcp tool proxied (open_tabs)", "open_tabs" in by)
k = by.get("kill_process", {})
check("destructive tool requires confirm const true",
      k.get("inputSchema", {}).get("properties", {}).get("confirm", {}).get("const") is True
      and "confirm" in k["inputSchema"]["required"] and k["annotations"]["destructiveHint"] is True)
check("read tool annotated readOnly", by["get_app_version"]["annotations"]["readOnlyHint"] is True)

r = m.call("get_app_version")
check("read call get_app_version", not r["isError"], text(r))
r = m.call("list_apps", {"query": "code"})
check("legacy call list_apps", not r.get("isError"), text(r)[:80].replace("\n", " "))

before = set(subprocess.run(["tasklist", "/FI", "IMAGENAME eq charmap.exe", "/FO", "CSV", "/NH"], capture_output=True, text=True).stdout.split())
r = m.call("launch_app", {"path": "C:/Windows/System32/charmap.exe"})
check("act call launch_app", not r["isError"], text(r))
time.sleep(2)
rows = subprocess.run(["tasklist", "/FI", "IMAGENAME eq charmap.exe", "/FO", "CSV", "/NH"], capture_output=True, text=True).stdout.strip().splitlines()
pid = int(rows[0].split('","')[1]) if rows and "charmap" in rows[0].lower() else 0
check("charmap launched", pid > 0, f"pid {pid}")
r = m.call("close_app", {"pid": pid})
check("destructive without confirm -> isError, refused", r["isError"] and "confirm" in text(r), text(r)[:110])
alive = "charmap" in subprocess.run(["tasklist", "/FI", f"PID eq {pid}", "/NH"], capture_output=True, text=True).stdout.lower()
check("charmap still running after refusal", alive)
r = m.call("close_app", {"pid": pid, "confirm": True})
check("destructive with confirm=true", not r["isError"], text(r))
time.sleep(1.5)
alive = "charmap" in subprocess.run(["tasklist", "/FI", f"PID eq {pid}", "/NH"], capture_output=True, text=True).stdout.lower()
check("charmap closed", not alive)
r = m.call("vault_get", {"id": "x", "confirm": True})
check("secret tool not callable by default", r["isError"] and "unknown tool" in text(r), text(r)[:80])
r = m.call("open_url", {"url": "file:///C:/"})
check("app-side 422 surfaces as isError", r["isError"] and "web links" in text(r), text(r))

# ---- Phase B: app restart rotates the token; server keeps working ----
t0 = open(TOKEN).read().strip()
app("stop"); app("start")
t1 = open(TOKEN).read().strip()
check("token rotated by restart", t0 != t1)
r = m.call("get_app_version")
check("call after restart works without restarting the server (H14)", not r["isError"], text(r))
m.close()

# ---- Phase C: tier filters ----
m = Mcp("--tiers", "read", "--poll", "0")
m.send("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}})
tools = m.send("tools/list")["result"]["tools"]
check("--tiers read exposes only read-only tools",
      tools and all(t.get("annotations", {}).get("readOnlyHint", True) for t in tools) and not any(t["name"] == "launch_app" for t in tools),
      f"{len(tools)} tools")
r = m.call("launch_app", {"path": "x"})
check("--tiers read: act tool not callable", r["isError"])
m.close()
m = Mcp("--tiers", "read,act,destructive,secret", "--poll", "0")
m.send("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}})
names = {t["name"] for t in m.send("tools/list")["result"]["tools"]}
check("explicit secret opt-in exposes vault tools", {"vault_get", "vault_list"} <= names)
r = m.call("vault_list", {"confirm": True})
check("vault_list on a vault that doesn't exist -> isError (no secrets)", r["isError"], text(r))
m.close()
r = subprocess.run([CLI, "mcp", "serve", "--tiers", "read,bogus"], capture_output=True, text=True, stdin=subprocess.DEVNULL)
check("bad tier is a usage error (exit 2)", r.returncode == 2, r.stderr.strip())

print(f"\n{len(fails)} FAIL(S): {fails}" if fails else "\nALL PASS")
sys.exit(1 if fails else 0)
