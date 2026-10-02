# AnthroForge/Web — project file map

How to build, run and test: see `RUN_LOCALLY.md`. CI is `.github/workflows/ci.yml`.

## Top-level folders

- `rust-core/` — Rust crate `anthroforge-core` (`cdylib` + `rlib`). Procedural
  character generation, Part Pack format v2 (`src/`), the `pack_builder` binary,
  integration tests (`tests/`, fixtures in `tests/fixtures/`), benches, and
  the tracked packs in `packs/`. Compiles to native and to `wasm32-unknown-unknown`.
- `packages/web/` — `@anthroforge/web`, the wasm SDK. `src/` is the TypeScript
  source, `dist/` the tracked build output (wasm, JS, types), `fixtures/` the
  Part Packs the tests and demo load, `demo/index.html` the browser demo.
- `packages/web-three/` — `@anthroforge/web-three`, the three.js adapter. It
  depends on `@anthroforge/web` through `file:../web`; `dist/` is tracked.
- `tools/` — offline Node scripts (`bake_dance.mjs`, `dump_skeleton.mjs`) with
  their own `package.json`. `tools/ci/` holds the CI browser smoke test.
- `.github/workflows/` — the `ci` workflow: Rust tests, the two npm suites, a
  browser smoke test of the demo, and a non-gating `dist/` rebuild.

## Test commands

- `cargo test --locked` in `rust-core/`.
- `npm ci && npm test` in `packages/web/` and in `packages/web-three/`.
- `node tools/ci/demo_smoke.mjs` from the repo root (needs Playwright's Chromium).
- `node tools/ci/baseline.mjs --out BASELINE.md` from the repo root (regenerates the baseline table; needs Playwright's Chromium).

## Root files

- `start_demo.bat`, `verify_diversity.mjs` — helper scripts at the root.
- `anthroforge-wasm-sdk-product-spec-v2.md` — the SDK product spec.
- `BASELINE.md` — per-pack load time, first-generate time and peak wasm memory (generated).

## Notes files (history, not current instructions)

- `CC0_PHASE_7_NOTES.md`, `PHASE_3_MERGE_NOTES.md`, `SNAPSHOT_README.md`,
  `ANIMATION_BRANCH_README.txt` — records of earlier work.
- `rust-core/CC0_PHASE_*_NOTES.md`, `rust-core/CC0_PHASE_*_MERGE_NOTES.md`,
  `rust-core/PHASE_*` — older notes inside the crate.
- `rust-core/PROVENANCE.md` — where the bundled assets came from.

## Known caveats

- The wasm committed in `packages/web/dist/` came from a non-official
  toolchain (see `CC0_PHASE_7_NOTES.md`). Rebuild it with your release toolchain
  before publishing.
- The `rust-core/Cargo.toml` comments say some dev-dependency versions are
  pinned for an old rustc via `Cargo.lock`.
