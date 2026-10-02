// Browser smoke test for packages/web/demo/index.html.
// Run from the repo root: node tools/ci/demo_smoke.mjs
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const TYPES = {
  ".wasm": "application/wasm",
  ".js": "text/javascript",
  ".mjs": "text/javascript",
  ".html": "text/html",
};

function fail(reason) {
  console.error(`SMOKE FAIL: ${reason}`);
  process.exit(1);
}

const server = http.createServer((req, res) => {
  let rel;
  try {
    rel = decodeURIComponent(new URL(req.url, "http://localhost").pathname);
  } catch {
    res.writeHead(400).end();
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

await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const port = server.address().port;

let browser;
try {
  browser = await chromium.launch();
  const page = await browser.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(`pageerror: ${e.message}`));
  page.on("console", (m) => {
    if (m.type() === "error") errors.push(`console: ${m.text()}`);
  });

  await page.goto(`http://127.0.0.1:${port}/packages/web/demo/index.html`);
  await page.click("#generateBtn");
  try {
    await page.waitForFunction(
      () => /\d/.test(document.getElementById("statVerts")?.textContent || ""),
      null,
      { timeout: 60000 },
    );
  } catch {
    const shown = (await page.textContent("#statusError").catch(() => "")) || "";
    fail(`#statVerts never showed digits within 60 s (statusError: "${shown.trim()}"; ${errors[0] || "no page errors"})`);
  }

  // The demo writes plain String(n); strip any separators defensively.
  const digits = async (sel) => ((await page.textContent(sel)) || "").replace(/\D/g, "");
  const verts = await digits("#statVerts");
  const indices = await digits("#statIndices");
  const status = ((await page.textContent("#statusError")) || "").trim();

  if (verts !== "53512") fail(`vertices ${verts}, expected 53512`);
  if (indices !== "80268") fail(`indices ${indices}, expected 80268`);
  if (status !== "") fail(`#statusError not empty: ${status}`);
  if (errors.length > 0) fail(`browser errors: ${errors[0]}`);

  console.log(`SMOKE OK vertices=${verts} indices=${indices}`);
} catch (e) {
  fail(String(e && e.message ? e.message : e).split("\n")[0]);
} finally {
  if (browser) await browser.close().catch(() => {});
  server.close();
}
