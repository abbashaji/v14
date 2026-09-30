#!/usr/bin/env node
/**
 * packages/web/bench/benchmark.mjs
 *
 * Timing/memory/determinism benchmark for `generate()` per
 * TASK-PHASE3-PART-C, §Part 1.
 *
 * Runs against the REAL CC0 MakeHuman body (fixtures/cc0_body.afpp: 4 rigged
 * parts, 53,512 vertices / 80,268 indices) -- production-scale geometry, unlike
 * the earlier runs against a 6-vertex fixture, whose numbers said nothing about
 * a real character. Set BENCH_MORPHS=1 to blend both real morph targets on
 * every call and measure their cost.
 *
 *   node --expose-gc bench/benchmark.mjs            # body only
 *   BENCH_MORPHS=1 node --expose-gc bench/benchmark.mjs
 *
 * Numbers are from Node on the machine running it, NOT a browser; see
 * PROJECT_STATE.md for the recorded results and that caveat. (A small
 * Node file://->http fetch shim is used below, the same technique
 * src/test-harness.mjs uses, since Node's fetch doesn't support file: URLs.)
 */

// "@anthroforge/web" is not published and there's no npm workspace set
// up in packages/web/ to make that bare specifier resolve locally, so
// (per the merge — see PHASE_3_MERGE_NOTES.md) this imports the real
// built output directly instead of the placeholder bare specifier.
import { init, generate, getLastError, freeCharacter } from "../dist/index.js";
import http from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";

// --- Node file:// fetch shim --------------------------------------------
//
// init() uses fetch() for both the bundled wasm module and the Part Pack
// (the one code path that also works unmodified in a browser). This
// Node version's fetch (undici) does not implement the `file:` scheme at
// all ("not implemented... yet..."), and when this script is run the
// normal way (`node bench/benchmark.mjs`), `dist/index.js`'s
// `import.meta.url` — and therefore the wasm URL it derives from it — is
// unavoidably a `file://` URL. src/index.test.mjs hit this identical
// problem and solved it by serving files over a real local HTTP server
// and shimming fetch to rewrite `file://` to `http://` before handing
// off to the real fetch. Reused here verbatim so this script is actually
// runnable with a plain `node bench/benchmark.mjs`, not just in theory.
const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..");

const fileServer = http.createServer((req, res) => {
  const filePath = path.join(repoRoot, decodeURIComponent(req.url ?? "/"));
  import("node:fs").then(({ readFile }) =>
    readFile(filePath, (err, data) => {
      if (err) {
        res.writeHead(404);
        res.end();
        return;
      }
      res.writeHead(200);
      res.end(data);
    }),
  );
});
const fileServerReady = new Promise((resolve) => fileServer.listen(0, "127.0.0.1", resolve));
const realFetch = globalThis.fetch;
globalThis.fetch = async (input, init) => {
  const url = typeof input === "string" ? new URL(input) : new URL(input.url ?? input);
  if (url.protocol === "file:") {
    await fileServerReady;
    const { port } = fileServer.address();
    const relative = path.relative(repoRoot, url.pathname);
    return realFetch(`http://127.0.0.1:${port}/${relative.split(path.sep).join("/")}`, init);
  }
  return realFetch(input, init);
};

// --- Configuration -----------------------------------------------------

// The real CC0 body pack (v2). Served by the repo-root file server above.
const PART_PACK_URL = new URL("../fixtures/cc0_body.afpp", import.meta.url).href;

const LICENSE_KEY = process.env.ANTHROFORGE_LICENSE_KEY ?? "";

// Part ids in fixtures/cc0_body.afpp: 4001 head, 4002 torso, 4003 arms, 4004 legs
// (all four are required); morphs 5001 (asym-ear-1-l) and 5002 (asym-nose-1-l).
const HEAD_ID = 4001;
const TORSO_ID = 4002;
const ARMS_ID = 4003;
const LEGS_ID = 4004;
const MORPHS = process.env.BENCH_MORPHS
  ? [
      { id: 5001, weight: 0.7 },
      { id: 5002, weight: 0.4 },
    ]
  : [];

const TOTAL_CALLS = 200;
const WARMUP_CALLS = 10;

// --- Helpers -------------------------------------------------------------

function makeDNA(callIndex) {
  return {
    seed: BigInt(callIndex + 1), // 200 distinct seeds, 1..200
    heightModifier: 1.0,
    weightModifier: 1.0,
    headId: HEAD_ID,
    torsoId: TORSO_ID,
    armsId: ARMS_ID,
    legsId: LEGS_ID,
    clothingIds: [],
    morphs: MORPHS,
  };
}

function percentile(sortedAsc, p) {
  const idx = Math.min(
    sortedAsc.length - 1,
    Math.max(0, Math.ceil((p / 100) * sortedAsc.length) - 1)
  );
  return sortedAsc[idx];
}

function mb(bytes) {
  return (bytes / (1024 * 1024)).toFixed(2);
}

// GeneratedCharacter's real field names, confirmed against the merged
// src/index.ts: positions (Float32Array, 3 per vertex) and indices
// (Uint32Array). The vertexCount fallback below is kept only in case a
// future SDK version adds a precomputed count field.
function readMeshCounts(result) {
  const vertexCount =
    result.vertexCount ??
    (Array.isArray(result.positions) || result.positions?.length !== undefined
      ? result.positions.length / 3
      : undefined);
  const indexCount = result.indices?.length;
  return { vertexCount, indexCount };
}

// --- Main ------------------------------------------------------------

async function main() {
  await init({ partPackUrl: PART_PACK_URL, licenseKey: LICENSE_KEY });

  const heapBefore = process.memoryUsage().heapUsed;
  const rssBefore = process.memoryUsage().rss;

  let firstCallMs = null;
  const steadyStateTimings = []; // calls 11-200 only
  let referenceVertexCount = null;
  let referenceIndexCount = null;
  let meshMismatch = false;
  let errorCount = 0;

  for (let i = 0; i < TOTAL_CALLS; i++) {
    const dna = makeDNA(i);

    const t0 = performance.now();
    const result = generate(dna);
    const t1 = performance.now();
    const elapsedMs = t1 - t0;

    if (result === null) {
      errorCount++;
      console.error(`  call ${i + 1}: generate() returned null: ${getLastError()}`);
      continue;
    }

    if (i === 0) {
      firstCallMs = elapsedMs;
    } else if (i >= WARMUP_CALLS) {
      // calls 11..200 (i is 0-indexed, so i >= 10 means call number >= 11)
      steadyStateTimings.push(elapsedMs);
    }
    // i = 1..9 (calls 2-10): warm-up, intentionally discarded from stats

    const { vertexCount, indexCount } = readMeshCounts(result);
    if (referenceVertexCount === null) {
      referenceVertexCount = vertexCount;
      referenceIndexCount = indexCount;
    } else if (vertexCount !== referenceVertexCount || indexCount !== referenceIndexCount) {
      meshMismatch = true;
    }

    freeCharacter(result);
  }

  if (typeof global.gc === "function") {
    global.gc();
  }

  const heapAfter = process.memoryUsage().heapUsed;
  const rssAfter = process.memoryUsage().rss;

  steadyStateTimings.sort((a, b) => a - b);

  console.log("=== AnthroForge generate() benchmark ===");
  console.log(`Total calls attempted: ${TOTAL_CALLS}`);
  console.log(`Errors: ${errorCount}`);
  console.log("");

  console.log(`First call time: ${firstCallMs !== null ? firstCallMs.toFixed(3) + " ms" : "N/A (first call errored)"}`);
  console.log("");

  console.log(`Steady-state timing, calls 11-${TOTAL_CALLS} (n=${steadyStateTimings.length}):`);
  if (steadyStateTimings.length > 0) {
    console.log(`  min: ${steadyStateTimings[0].toFixed(3)} ms`);
    console.log(`  p50: ${percentile(steadyStateTimings, 50).toFixed(3)} ms`);
    console.log(`  p95: ${percentile(steadyStateTimings, 95).toFixed(3)} ms`);
    console.log(`  p99: ${percentile(steadyStateTimings, 99).toFixed(3)} ms`);
    console.log(`  max: ${steadyStateTimings[steadyStateTimings.length - 1].toFixed(3)} ms`);
  } else {
    console.log("  no successful steady-state calls");
  }
  console.log("");

  console.log("JS-side memory (process.memoryUsage()):");
  console.log(`  heapUsed before: ${mb(heapBefore)} MB`);
  console.log(`  heapUsed after:  ${mb(heapAfter)} MB`);
  console.log(`  heapUsed delta:  ${mb(heapAfter - heapBefore)} MB`);
  console.log(`  rss before:      ${mb(rssBefore)} MB`);
  console.log(`  rss after:       ${mb(rssAfter)} MB`);
  console.log(`  rss delta:       ${mb(rssAfter - rssBefore)} MB`);
  console.log(
    typeof global.gc === "function"
      ? "  (heapUsed/rss 'after' measured following an explicit global.gc() call)"
      : "  (run with `node --expose-gc bench/benchmark.mjs` for a cleaner reading — no GC was forced here)"
  );
  console.log("");

  console.log("Mesh determinism check (fixed body ids across all calls):");
  console.log(`  vertex count: ${referenceVertexCount}`);
  console.log(`  index count:  ${referenceIndexCount}`);
  console.log(`  identical across all successful calls: ${meshMismatch ? "NO — MISMATCH DETECTED" : "yes"}`);
}

main()
  .catch((err) => {
    console.error("Benchmark failed to run:", err);
    process.exit(1);
  })
  .finally(() => {
    fileServer.close();
  });

/*
 * History: this script was first written (Phase 3, Part C) before a real wasm
 * or pack existed, then run against a 6-vertex fixture. It now targets the real
 * CC0 body. Those earlier timing numbers were for a triangle-scale mesh and
 * should not be quoted.
 */
