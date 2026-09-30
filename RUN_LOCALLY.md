# Running the AnthroForge web demo locally

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
   first click downloads a ~3.4 MB pack (the real CC0 body); you should see a
   wireframe plus real generation time, vertex and index counts (53,512
   vertices / 80,268 indices).

The demo has not been opened in a real browser since the CC0 change; if
something looks off, the Node tests (`cd packages/web && npm test`) are the
reliable check that the wasm and SDK work.

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
(override with `ANTHROFORGE_WASM_SRC`), optimizes it with binaryen and refuses to
write it if validation fails.

## Rebuilding a Part Pack

```sh
cd rust-core
cargo build --release --bin pack_builder
./target/release/pack_builder <asset_dir> <out.afpp> [<base_mesh.obj>]
```
Pass `assets/upstream/base.obj` as the third argument to enable morph targets.
The pack format is v2; v1 packs are rejected.
