// Baseline table: browser load time, first-generate time and peak wasm linear
// memory for every Part Pack in rust-core/packs/, measured in headless Chromium.
// Run from the repo root: node tools/ci/baseline.mjs [--out <file>] [--runs N] [--only id,id]
import http from "node:http";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const PACK_DIR = path.join(root, "rust-core", "packs");
const MIB = 1048576;
const RUN_LIMIT_MS = 120000;
const EXPECT_VERTICES = 53512;
const EXPECT_INDICES = 80268;
const EXPECT_PART_IDS = [4001, 4002, 4003, 4004];
const USAGE = "usage: node tools/ci/baseline.mjs [--out <file>] [--runs N] [--only id,id]   (--only cannot be combined with --out)";
const TYPES = {
  ".wasm": "application/wasm",
  ".js": "text/javascript",
  ".mjs": "text/javascript",
};

let server = null;
let browser = null;

async function closeAll() {
  if (browser) {
    const b = browser;
    browser = null;
    await Promise.race([b.close().catch(() => {}), new Promise((r) => setTimeout(r, 10000))]);
  }
  if (server) {
    const s = server;
    server = null;
    s.close();
    s.closeAllConnections?.();
  }
}

async function fail(reason) {
  console.error(`BASELINE FAIL: ${reason}`);
  await closeAll();
  process.exit(1);
}

function usageError(reason) {
  console.error(`baseline: ${reason}`);
  console.error(USAGE);
  process.exit(2);
}

// ---------------------------------------------------------------- arguments
function parseArgs(argv) {
  const opts = { out: null, runs: 3, only: null };
  const seen = new Set();
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    if (flag !== "--out" && flag !== "--runs" && flag !== "--only") {
      usageError(`unknown argument "${flag}"`);
    }
    if (seen.has(flag)) usageError(`${flag} given more than once`);
    seen.add(flag);
    if (i + 1 >= argv.length) usageError(`${flag} needs a value`);
    const value = argv[++i];
    if (flag === "--out") {
      if (value === "" || value.startsWith("--")) usageError("--out needs a file path");
      opts.out = value;
    } else if (flag === "--runs") {
      if (!/^\d+$/.test(value) || Number(value) < 1) usageError(`--runs must be an integer >= 1, got "${value}"`);
      opts.runs = Number(value);
    } else {
      const ids = value.split(",");
      if (ids.some((id) => id === "")) usageError("--only needs a comma-separated list of pack ids");
      opts.only = [...new Set(ids)];
    }
  }
  if (opts.only !== null && opts.out !== null) usageError("--only cannot be combined with --out");
  return opts;
}

// ------------------------------------------------- pack discovery/validation
function readExact(fd, length, position) {
  const buf = Buffer.alloc(length);
  const n = fs.readSync(fd, buf, 0, length, position);
  return n === length ? buf : null;
}

function validatePack(entry) {
  const name = entry.file;
  const file = path.join(PACK_DIR, name);
  const problems = [];
  const fd = fs.openSync(file, "r");
  try {
    const head = readExact(fd, 20, 0);
    if (!head) return [`pack ${name}: field header: file shorter than 20 bytes`];
    const magic = head.toString("latin1", 0, 4);
    const version = head.readUInt32LE(4);
    const partCount = head.readUInt32LE(8);
    const morphCount = head.readUInt32LE(12);
    const skelLen = head.readUInt32LE(16);
    if (magic !== "AFPP") problems.push(`pack ${name}: field magic is "${magic}", expected "AFPP"`);
    if (version !== 2) problems.push(`pack ${name}: field version is ${version}, expected 2`);
    if (partCount !== EXPECT_PART_IDS.length) {
      problems.push(`pack ${name}: field part_count is ${partCount}, expected ${EXPECT_PART_IDS.length}`);
    }
    if (morphCount !== entry.morph_count) {
      problems.push(`pack ${name}: field morph_count is ${morphCount}, manifest says ${entry.morph_count}`);
    }
    if (partCount === EXPECT_PART_IDS.length) {
      const table = readExact(fd, partCount * 21, 20 + skelLen);
      if (!table) {
        problems.push(`pack ${name}: field part index: table at offset ${20 + skelLen} is cut off`);
      } else {
        const ids = [];
        for (let i = 0; i < partCount; i++) ids.push(table.readUInt32LE(i * 21));
        const sorted = [...ids].sort((a, b) => a - b);
        if (sorted.join(",") !== EXPECT_PART_IDS.join(",")) {
          problems.push(`pack ${name}: field part ids are [${ids.join(", ")}], expected ${EXPECT_PART_IDS.join(", ")}`);
        }
      }
    }
  } finally {
    fs.closeSync(fd);
  }
  return problems;
}

function discoverPacks(only) {
  let manifest;
  try {
    manifest = JSON.parse(fs.readFileSync(path.join(PACK_DIR, "manifest.json"), "utf8"));
  } catch (e) {
    return { error: `cannot read rust-core/packs/manifest.json: ${e.message}` };
  }
  if (!Array.isArray(manifest.packs) || manifest.packs.length === 0) {
    return { error: "rust-core/packs/manifest.json has no packs array" };
  }
  const entries = manifest.packs;

  if (only !== null) {
    const known = new Set(entries.map((e) => e.id));
    for (const id of only) {
      if (!known.has(id)) {
        return { error: `--only: "${id}" is not a pack id in manifest.json (known: ${[...known].sort().join(", ")})` };
      }
    }
  }

  const onDisk = fs.readdirSync(PACK_DIR).filter((n) => n.endsWith(".afpp")).sort();
  const inManifest = entries.map((e) => e.file).sort();
  const extra = onDisk.filter((n) => !inManifest.includes(n));
  const missing = inManifest.filter((n) => !onDisk.includes(n));
  if (extra.length > 0 || missing.length > 0) {
    const parts = [];
    if (extra.length > 0) parts.push(`on disk but not in manifest: ${extra.join(", ")}`);
    if (missing.length > 0) parts.push(`in manifest but not on disk: ${missing.join(", ")}`);
    return { error: `pack files differ from manifest.json (${parts.join("; ")})` };
  }

  const problems = [];
  for (const entry of entries) problems.push(...validatePack(entry));
  if (problems.length > 0) return { error: problems.join("\n") };

  let selected = entries;
  if (only !== null) selected = entries.filter((e) => only.includes(e.id));
  selected = [...selected].sort((a, b) => (a.file < b.file ? -1 : a.file > b.file ? 1 : 0));
  return { packs: selected };
}

// ------------------------------------------------------------ static server
function startServer() {
  const srv = http.createServer((req, res) => {
    let rel;
    try {
      rel = decodeURIComponent(new URL(req.url, "http://localhost").pathname);
    } catch {
      res.writeHead(400).end();
      return;
    }
    if (rel === "/__baseline__") {
      const html = "<!doctype html><title>baseline</title>";
      res.writeHead(200, { "Content-Type": "text/html", "Content-Length": Buffer.byteLength(html) });
      res.end(html);
      return;
    }
    if (rel.includes("..")) {
      res.writeHead(400).end();
      return;
    }
    const file = path.join(root, rel);
    if (!file.startsWith(root)) {
      res.writeHead(400).end();
      return;
    }
    fs.stat(file, (err, st) => {
      if (err || !st.isFile()) {
        res.writeHead(404).end();
        return;
      }
      const type = TYPES[path.extname(file).toLowerCase()] || "application/octet-stream";
      res.writeHead(200, { "Content-Type": type, "Content-Length": st.size });
      fs.createReadStream(file).pipe(res);
    });
  });
  return new Promise((resolve) => srv.listen(0, "127.0.0.1", () => resolve(srv)));
}

// -------------------------------------------------------------- measurement
// Runs in the page before any script: keep the memory object of the one
// WebAssembly.instantiate call the SDK makes (the SDK itself does not expose it).
function installMemoryHook() {
  const original = WebAssembly.instantiate;
  WebAssembly.instantiate = async function (...args) {
    const result = await original.apply(WebAssembly, args);
    const instance = result.instance ?? result;
    window.__baselineMemory = instance.exports.memory;
    return result;
  };
}

// Runs in the page: time init() and the first generate(), then read the memory size.
async function measureInPage({ packUrl }) {
  const { init, generate } = await import("/packages/web/dist/index.js");
  const t0 = performance.now();
  await init({ partPackUrl: packUrl, licenseKey: "" });
  const t1 = performance.now();
  const character = generate({
    seed: 1n,
    heightModifier: 1,
    weightModifier: 1,
    headId: 4001,
    torsoId: 4002,
    armsId: 4003,
    legsId: 4004,
    clothingIds: [],
    morphs: [],
  });
  const t2 = performance.now();
  const memory = window.__baselineMemory;
  return {
    loadMs: t1 - t0,
    generateMs: t2 - t1,
    memoryBytes: memory ? memory.buffer.byteLength : null,
    isNull: character === null,
    vertices: character === null ? null : character.positions.length / 3,
    indices: character === null ? null : character.indices.length,
  };
}

function withTimeout(promise, ms, what) {
  let timer;
  const timeout = new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error(`${what} timed out after ${ms / 1000} s`)), ms);
  });
  return Promise.race([promise, timeout]).finally(() => clearTimeout(timer));
}

async function measureOnce(port, packFile) {
  const context = await browser.newContext();
  try {
    const page = await context.newPage();
    const errors = [];
    page.on("pageerror", (e) => errors.push(`pageerror: ${e.message}`));
    page.on("console", (m) => {
      if (m.type() === "error") errors.push(`console: ${m.text()}`);
    });
    const run = async () => {
      await page.addInitScript(installMemoryHook);
      await page.goto(`http://127.0.0.1:${port}/__baseline__`);
      return page.evaluate(measureInPage, { packUrl: `/rust-core/packs/${encodeURIComponent(packFile)}` });
    };
    const result = await withTimeout(run(), RUN_LIMIT_MS, "run");
    if (errors.length > 0) throw new Error(`browser errors: ${errors[0]}`);
    if (result.isNull) throw new Error("generate() returned null");
    if (result.memoryBytes === null) throw new Error("wasm memory export was not captured");
    if (result.vertices !== EXPECT_VERTICES || result.indices !== EXPECT_INDICES) {
      throw new Error(`got ${result.vertices} vertices / ${result.indices} indices, expected ${EXPECT_VERTICES} / ${EXPECT_INDICES}`);
    }
    return result;
  } finally {
    await context.close().catch(() => {});
  }
}

// -------------------------------------------------------------- aggregation
function median(values) {
  const s = [...values].sort((a, b) => a - b);
  const mid = s.length >> 1;
  return s.length % 2 === 1 ? s[mid] : (s[mid - 1] + s[mid]) / 2;
}

const fmt = (n) => n.toFixed(1);

function buildMarkdown(rows, browserVersion, runs) {
  const pkg = JSON.parse(fs.readFileSync(path.join(root, "tools", "ci", "package.json"), "utf8"));
  const cpus = os.cpus();
  const lines = [
    "# Baseline table",
    "",
    "Generated by `node tools/ci/baseline.mjs`; regenerate it, do not edit it by hand. The `baseline` CI job uploads the artifact `baseline-table` for a runner-side copy.",
    "",
    `- UTC timestamp: ${new Date().toISOString().replace(/\.\d{3}Z$/, "Z")}`,
    `- Browser: Chromium ${browserVersion}`,
    `- Playwright: ${pkg.dependencies.playwright}`,
    `- Node: ${process.version}`,
    `- Platform: ${process.platform} ${process.arch}`,
    `- CPU: ${cpus.length > 0 ? cpus[0].model.trim() : "unknown"}, ${cpus.length} logical CPUs`,
    `- Total memory: ${fmt(os.totalmem() / 1073741824)} GiB`,
    `- Runs per pack: ${runs}`,
    "",
    "| Pack | File MiB | Load ms | First generate ms | Peak wasm MiB |",
    "| --- | ---: | ---: | ---: | ---: |",
    ...rows.map((r) => `| ${r.id} | ${fmt(r.fileMiB)} | ${fmt(r.loadMs)} | ${fmt(r.generateMs)} | ${fmt(r.peakMiB)} |`),
    "",
    "- Load = `await init()` in the page, i.e. loopback HTTP fetch of the wasm and the pack, wasm instantiate and pack parse, and no real network transfer.",
    "- First generate = the first `generate()` after init with seed 1n, modifiers 1.0, no morphs and no clothing.",
    "- Peak wasm = size of wasm linear memory after the first generate, which only grows so it is the peak of linear memory, and excludes the JS heap and the downloaded buffers.",
    "- Each run uses a fresh browser context because the pack registry is set once per page load.",
    "- File MiB is the on-disk pack size.",
    "",
  ];
  return lines.join("\n");
}

// --------------------------------------------------------------------- main
const opts = parseArgs(process.argv.slice(2));

const found = discoverPacks(opts.only);
if (found.error) await fail(found.error);

try {
  server = await startServer();
  const port = server.address().port;
  browser = await chromium.launch();
  const browserVersion = browser.version();

  const rows = [];
  for (const entry of found.packs) {
    const loads = [];
    const gens = [];
    const mems = [];
    for (let i = 1; i <= opts.runs; i++) {
      console.error(`[${entry.id}] run ${i}/${opts.runs}`);
      let r;
      try {
        r = await measureOnce(port, entry.file);
      } catch (e) {
        await fail(`pack ${entry.id} (run ${i}/${opts.runs}): ${String(e && e.message ? e.message : e).split("\n")[0]}`);
      }
      loads.push(r.loadMs);
      gens.push(r.generateMs);
      mems.push(r.memoryBytes);
    }
    rows.push({
      id: entry.id,
      fileMiB: fs.statSync(path.join(PACK_DIR, entry.file)).size / MIB,
      loadMs: median(loads),
      generateMs: median(gens),
      peakMiB: Math.max(...mems) / MIB,
    });
  }

  const markdown = buildMarkdown(rows, browserVersion, opts.runs);
  process.stdout.write(markdown);
  if (opts.out !== null) fs.writeFileSync(opts.out, markdown);
} catch (e) {
  await fail(String(e && e.message ? e.message : e).split("\n")[0]);
}
await closeAll();
