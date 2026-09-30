// Shared test harness for @anthroforge/web's Node tests.
//
// Why this exists (carried over from the original index.test.mjs): `init()`
// uses `fetch()` for both the bundled `.wasm` and the caller's Part Pack,
// because that is the one code path that also works in a browser. Node's
// `fetch` (undici) does not implement the `file:` scheme, and the built
// `dist/index.js` is loaded as a normal file-based ES module, so the wasm URL
// `init()` derives from `import.meta.url` is unavoidably a `file://` URL.
// This harness starts a real local HTTP server and wraps `globalThis.fetch`
// so a `file://.../<name>` URL is rewritten to `http://127.0.0.1:<port>/<name>`
// before it reaches the *real* fetch. Every byte still travels over a genuine
// HTTP request/response; the shim only bridges the URL-scheme gap.
//
// The server serves two directories, looked up by base file name only (no
// path traversal): `../fixtures/` (test Part Packs) first, then `../dist/`
// (the built wasm). Packs live in `fixtures/`, not `dist/`, because `dist/` is
// build output.

import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
export const fixturesDir = path.join(here, "..", "fixtures");
export const distDir = path.join(here, "..", "dist");

export async function startHarness() {
  const server = http.createServer((req, res) => {
    const name = path.basename(decodeURIComponent(req.url ?? "/"));
    for (const dir of [fixturesDir, distDir]) {
      const filePath = path.join(dir, name);
      if (fs.existsSync(filePath) && fs.statSync(filePath).isFile()) {
        res.writeHead(200);
        res.end(fs.readFileSync(filePath));
        return;
      }
    }
    res.writeHead(404);
    res.end();
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const baseUrl = `http://127.0.0.1:${server.address().port}`;

  const realFetch = globalThis.fetch;
  globalThis.fetch = (input, init) => {
    const url = typeof input === "string" ? new URL(input) : new URL(input.url ?? input);
    if (url.protocol === "file:") {
      return realFetch(`${baseUrl}/${path.basename(url.pathname)}`, init);
    }
    return realFetch(input, init);
  };

  return {
    baseUrl,
    async close() {
      globalThis.fetch = realFetch;
      await new Promise((resolve) => server.close(resolve));
    },
  };
}
