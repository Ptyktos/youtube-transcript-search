// Benchmark driver.
//
// Modes:
//   parse        — microbench parseTranscriptXml on canned input
//   parse-json   — microbench Innertube JSON parse
//   url          — microbench extractVideoId
//   e2e          — N requests against the mock server
//   cold         — single request, exit immediately (cold-start measurement)
//   memory       — N requests, then idle for 1s and exit (RSS sampled by harness)

import * as fs from "node:fs";
import { performance } from "node:perf_hooks";
import {
  extractVideoId,
  parseTranscriptXml,
  getTranscript,
  innertubeBody,
} from "./transcript.js";

const MOCK = process.env.MOCK_URL ?? "http://127.0.0.1:18080";
const SAMPLE_URL = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";

async function modeParse(iters: number) {
  const xml = fs.readFileSync("/tmp/bench/canned/caption.xml", "utf8");
  const t0 = performance.now();
  let bytesOut = 0;
  for (let i = 0; i < iters; i++) {
    bytesOut += parseTranscriptXml(xml).length;
  }
  const t1 = performance.now();
  console.log(JSON.stringify({ mode: "parse", iters, ms: t1 - t0, bytesIn: xml.length, bytesOut }));
}

async function modeParseJson(iters: number) {
  const txt = fs.readFileSync("/tmp/bench/canned/innertube.json", "utf8");
  const t0 = performance.now();
  let n = 0;
  for (let i = 0; i < iters; i++) {
    const j: any = JSON.parse(txt);
    n += j?.captions?.playerCaptionsTracklistRenderer?.captionTracks?.length ?? 0;
  }
  const t1 = performance.now();
  console.log(JSON.stringify({ mode: "parse-json", iters, ms: t1 - t0, n }));
}

async function modeUrl(iters: number) {
  const t0 = performance.now();
  let n = 0;
  for (let i = 0; i < iters; i++) {
    n += extractVideoId(SAMPLE_URL).length;
  }
  const t1 = performance.now();
  console.log(JSON.stringify({ mode: "url", iters, ms: t1 - t0, n }));
}

async function modeE2E(iters: number) {
  const innertubeUrl = `${MOCK}/youtubei/v1/player`;
  const samples: number[] = [];
  for (let i = 0; i < iters; i++) {
    const t0 = performance.now();
    const r = await getTranscript(SAMPLE_URL, "auto", { innertubeUrl });
    const t1 = performance.now();
    samples.push(t1 - t0);
    if (i === 0 && r.text.length === 0) throw new Error("empty result");
  }
  console.log(JSON.stringify({ mode: "e2e", iters, samples }));
}

async function modeCold() {
  const innertubeUrl = `${MOCK}/youtubei/v1/player`;
  const t0 = performance.now();
  const r = await getTranscript(SAMPLE_URL, "auto", { innertubeUrl });
  const t1 = performance.now();
  console.log(JSON.stringify({ mode: "cold", ms: t1 - t0, len: r.text.length }));
}

function readSelfVmHwmKib(): number {
  try {
    const s = fs.readFileSync("/proc/self/status", "utf8");
    for (const line of s.split("\n")) {
      if (line.startsWith("VmHWM:")) {
        const m = line.match(/(\d+)/);
        return m ? parseInt(m[1], 10) : 0;
      }
    }
  } catch {}
  return 0;
}

async function modeMemory(iters: number) {
  const innertubeUrl = `${MOCK}/youtubei/v1/player`;
  for (let i = 0; i < iters; i++) {
    await getTranscript(SAMPLE_URL, "auto", { innertubeUrl });
  }
  if (global.gc) global.gc();
  await new Promise((r) => setTimeout(r, 250));
  const rssKib = readSelfVmHwmKib();
  const m = process.memoryUsage();
  console.log(JSON.stringify({ mode: "memory", iters, rssKib, rss: m.rss, heapUsed: m.heapUsed }));
}

const mode = process.argv[2] ?? "cold";
const iters = parseInt(process.argv[3] ?? "100", 10);

(async () => {
  switch (mode) {
    case "parse": await modeParse(iters); break;
    case "parse-json": await modeParseJson(iters); break;
    case "url": await modeUrl(iters); break;
    case "e2e": await modeE2E(iters); break;
    case "cold": await modeCold(); break;
    case "memory": await modeMemory(iters); break;
    default: throw new Error(`unknown mode: ${mode}`);
  }
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
