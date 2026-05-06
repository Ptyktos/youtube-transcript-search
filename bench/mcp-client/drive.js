// Generic MCP-stdio client benchmark driver.
//
// Spawns the target MCP server as a child, performs the standard MCP
// handshake (initialize → notifications/initialized → tools/list), then
// fires N `tools/call` requests for a given tool with a given URL.
//
// Outputs: cold-start (handshake), per-request samples, peak RSS of the
// child (read from /proc/<pid>/status:VmHWM right before kill).
//
// Usage:
//   node drive.js <mode:cold|e2e|memory> <iters> <toolName> <url> -- <child argv...>

import { spawn } from "node:child_process";
import { performance } from "node:perf_hooks";
import * as fs from "node:fs";

const args = process.argv.slice(2);
const dashIdx = args.indexOf("--");
if (dashIdx < 0) {
  console.error("usage: drive.js <mode> <iters> <toolName> <url> [--arg-name <name>] -- <child argv...>");
  process.exit(2);
}
const head = args.slice(0, dashIdx);
const [mode, itersStr, toolName, callUrl] = head.slice(0, 4);
let urlArgName = "url";
for (let i = 4; i < head.length; i++) {
  if (head[i] === "--arg-name" && head[i + 1]) urlArgName = head[i + 1];
}
const childArgv = args.slice(dashIdx + 1);
const iters = parseInt(itersStr, 10);

function readChildVmHwmKib(pid) {
  try {
    const s = fs.readFileSync(`/proc/${pid}/status`, "utf8");
    for (const line of s.split("\n")) {
      if (line.startsWith("VmHWM:")) {
        const m = line.match(/(\d+)/);
        return m ? parseInt(m[1], 10) : 0;
      }
    }
  } catch {}
  return 0;
}

function startServer(env = {}) {
  const child = spawn(childArgv[0], childArgv.slice(1), {
    stdio: ["pipe", "pipe", "pipe"],
    env: { ...process.env, ...env },
  });
  let buf = "";
  const queue = [];
  let resolveNext = null;
  child.stdout.on("data", (chunk) => {
    buf += chunk.toString();
    let idx;
    while ((idx = buf.indexOf("\n")) >= 0) {
      const line = buf.slice(0, idx).trim();
      buf = buf.slice(idx + 1);
      if (!line) continue;
      try {
        const msg = JSON.parse(line);
        if (resolveNext) {
          const r = resolveNext; resolveNext = null;
          r(msg);
        } else queue.push(msg);
      } catch {}
    }
  });
  child.stderr.on("data", () => {}); // discard
  return {
    child,
    send(obj) {
      child.stdin.write(JSON.stringify(obj) + "\n");
    },
    next() {
      if (queue.length) return Promise.resolve(queue.shift());
      return new Promise((res) => { resolveNext = res; });
    },
  };
}

let _id = 0;
function nextId() { return ++_id; }

async function handshake(srv) {
  srv.send({
    jsonrpc: "2.0", id: nextId(), method: "initialize",
    params: {
      protocolVersion: "2024-11-05",
      capabilities: {},
      clientInfo: { name: "bench-driver", version: "0.0.1" },
    },
  });
  const initResp = await srv.next();
  if (!initResp.result) throw new Error("initialize failed: " + JSON.stringify(initResp));
  srv.send({ jsonrpc: "2.0", method: "notifications/initialized" });
}

async function callTool(srv, name, args) {
  const id = nextId();
  srv.send({
    jsonrpc: "2.0", id, method: "tools/call",
    params: { name, arguments: args },
  });
  const resp = await srv.next();
  if (resp.error) throw new Error("tool error: " + JSON.stringify(resp.error));
  return resp;
}

async function runCold() {
  const t0 = performance.now();
  const srv = startServer();
  await handshake(srv);
  const r = await callTool(srv, toolName, { [urlArgName]: callUrl });
  const t1 = performance.now();
  srv.child.kill();
  // Detect server-reported errors (isError:true) so we don't accidentally
  // measure a fast error-return path as if it were a successful fetch.
  const isErr = !!(r.result && r.result.isError);
  console.log(JSON.stringify({ mode: "cold", ms: t1 - t0, len: JSON.stringify(r.result).length, error: isErr }));
}

async function runE2E() {
  const srv = startServer();
  await handshake(srv);
  const samples = [];
  for (let i = 0; i < iters; i++) {
    const t0 = performance.now();
    await callTool(srv, toolName, { [urlArgName]: callUrl });
    samples.push(performance.now() - t0);
  }
  srv.child.kill();
  console.log(JSON.stringify({ mode: "e2e", iters, samples }));
}

async function runMemory() {
  const srv = startServer();
  await handshake(srv);
  for (let i = 0; i < iters; i++) await callTool(srv, toolName, { [urlArgName]: callUrl });
  await new Promise((r) => setTimeout(r, 250));
  const rssKib = readChildVmHwmKib(srv.child.pid);
  srv.child.kill();
  console.log(JSON.stringify({ mode: "memory", iters, rssKib }));
}

(async () => {
  if (mode === "cold") await runCold();
  else if (mode === "e2e") await runE2E();
  else if (mode === "memory") await runMemory();
  else { console.error("unknown mode " + mode); process.exit(2); }
})().catch((e) => { console.error(e); process.exit(1); });
