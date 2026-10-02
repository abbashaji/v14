# Running AnthroForge locally

## Run the demo

`packages/web/dist/` ships a pre-built wasm + JS, so you don't need Rust to try it.
You only need a static file server, because browsers block `fetch()` of the
`.wasm`/`.afpp` files from a bare `file://` page.

1. From the **repo root**, start a server:
   ```sh
   python3 -m http.server 8000
   # or: npx serve .
   ```
2. Open `http://localhost:8000/packages/web/demo/index.html`.
3. Move the Height / Weight / Ear / Nose sliders and click **Generate**. The
   first click downloads a ~3.4 MB pack; you should see a wireframe plus
   generation time, vertex and index counts (53,512 vertices / 80,268 indices).

The CI job `demo-smoke` is what checks the demo in a real browser, and it has
not run yet unless this task ran it.

## Run the tests

```sh
sh -c 'cd rust-core && cargo test --locked'
sh -c 'cd packages/web && npm ci && npm test'
sh -c 'cd packages/web-three && npm ci && npm test'
```

## Rebuilding the wasm

```sh
cd rust-core
cargo build --release --lib --target wasm32-unknown-unknown   # needs rustup + the wasm32 target
cd ../packages/web && npm install && npm run build && npm test
```
Note: this exact command has been verified on the OLDER (pre-CC0) core on a Windows machine, but not yet
on the CC0 core with an official toolchain; the wasm currently in `dist/` was built with a sandbox workaround
(see `CC0_PHASE_7_NOTES.md`). If it fails on a dependency needing a newer Rust (`edition2024`), use a newer toolchain.

`npm run build` reads `../../rust-core/target/wasm32-unknown-unknown/release/anthroforge_core.wasm`
(override with `ANTHROFORGE_WASM_SRC`), optimizes it with binaryen and refuses to write it if validation fails.

## Rebuilding a Part Pack

```sh
cd rust-core
cargo build --release --bin pack_builder
./target/release/pack_builder <asset_dir> <out.afpp> [<base_mesh.obj>]
```
Pass `assets/upstream/base.obj` as the third argument to enable morph targets.
The pack format is v2; v1 packs are rejected.

## dist/ policy

- `dist/` stays tracked so the demo works without Rust.
- The gating test commands never run `npm run build`.
- Rebuilding is done only by the `dist-rebuild` CI job (artifact, not committed)
  or by hand.
- A rebuilt `dist/` is committed only in a phase that says so.

## Baseline table

```sh
node tools/ci/baseline.mjs --out BASELINE.md   # from the repo root
```
- Needs Playwright's Chromium: `cd tools/ci && npm install && npx playwright install chromium`.
- Load ms is the time `init()` takes in the page (loopback fetch, wasm instantiate, pack parse).
- First generate ms is the time of the first `generate()` after init.
- Peak wasm MiB is the size of the wasm linear memory after that generate.
- `BASELINE.md` is the committed copy; the `baseline` CI job produces a runner-side copy as artifact `baseline-table`.
