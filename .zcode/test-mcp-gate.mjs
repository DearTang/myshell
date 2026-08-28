// Smoke test for the new MCP vault gate: initialize + tools/call list_connections.
// Expects: either the connection list (vault unlocked) or an IMMEDIATE
// '保险库未解锁' error (vault locked) — never a 30s hang.
import { spawn } from "child_process";

const exe = "F:\\workProject\\personProject\\myshell\\src-tauri\\target\\debug\\myshell-mcp.exe";
const child = spawn(exe, [], { stdio: ["pipe", "pipe", "inherit"] });

let buf = "";
const responses = [];
child.stdout.on("data", (d) => {
  buf += d.toString();
  let idx;
  while ((idx = buf.indexOf("\n")) >= 0) {
    const line = buf.slice(0, idx).trim();
    buf = buf.slice(idx + 1);
    if (line) {
      try { responses.push(JSON.parse(line)); } catch { /* ignore */ }
    }
  }
});

const send = (obj) => child.stdin.write(JSON.stringify(obj) + "\n");

send({ jsonrpc: "2.0", id: 1, method: "initialize", params: {} });
send({ jsonrpc: "2.0", method: "notifications/initialized" });

const t0 = Date.now();
send({ jsonrpc: "2.0", id: 2, method: "tools/call", params: { name: "list_connections", arguments: {} } });

const timeout = setTimeout(() => {
  console.log("TIMEOUT — tool call did not return in 60s");
  child.kill();
  process.exit(1);
}, 60000);

const wait = setInterval(() => {
  const listResp = responses.find((r) => r.id === 2);
  if (!listResp) return;
  clearTimeout(timeout);
  clearInterval(wait);
  const elapsed = Date.now() - t0;
  const initResp = responses.find((r) => r.id === 1);
  console.log("initialize ok:", !!initResp?.result?.serverInfo);
  const text = listResp.result?.content?.[0]?.text ?? "";
  const isErr = listResp.result?.isError === true;
  console.log(`list_connections returned in ${elapsed}ms, isError=${isErr}`);
  console.log("payload (first 400 chars):", text.slice(0, 400));
  if (isErr) {
    console.log(text.includes("保险库未解锁") || text.includes("保险库尚未初始化")
      ? "PASS: fail-fast vault error surfaced immediately"
      : "UNEXPECTED ERROR TYPE");
  } else {
    console.log("PASS: vault was unlocked, list returned without hanging");
  }
  child.kill();
  process.exit(0);
}, 100);
