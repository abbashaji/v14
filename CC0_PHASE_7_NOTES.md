# CC0-Phase 7 notes — web SDK re-verified against the CC0 core

Done directly in the main context. Base: the CC0-Phase 6 core (124 native tests) and
the repo `abbashaji/wasm` as cloned (a pre-CC0 snapshot).

## Result
- `rust-core/` replaced by the CC0-Phase 6 core. Source is byte-identical to the tested base
  (`diff -rq`, ignoring `target/`); only additions are `.gitignore`, the renamed
  `CC0_PHASE_3_MERGE_NOTES.md` (was `..._updated.md`; you confirmed they are the same file)
  and `scripts/build-wasm-ubuntu-sandbox.sh`. The native suite was not re-run on this copy
  because the source is unchanged.
- `packages/web`: bridge/API/tests/bench/demo updated; `dist/` rebuilt. **15/15 tests pass**
  against the rebuilt wasm.
- `packages/web-three`: fixture builder + tests updated. **4/4 tests pass.**
- KayKit showcase, the stale root `real_test.afpp`, and the 117 MB checked-in `rust-core/target/`
  are gone; `.gitignore` added.

## What was wrong / what changed
1. **The repo's Rust core was a strictly older snapshot** (40-byte native `CharacterDNA`, pack v1,
   head+torso-only clothing). Compared with line endings ignored: 7 of 9 source files identical, and
   every repo-only line was an older version of code the CC0 base rewrote. Nothing lost by replacing it.
2. **The JS bridge was pinned to the old ABI.** `wasm-bridge.ts` hard-coded a 32-byte DNA; arms/legs
   (required) and morphs did not exist. Now 56 bytes: `arms_id`@32, `legs_id`@36, morph ids ptr@40,
   morph weights ptr@44, morph count@48. The public API gained required `armsId`/`legsId` and optional
   `morphs: {id, weight}[]` — a **breaking change** (version left at 0.1.0; bump it as you see fit).
   All allocations happen before any `DataView` is created (a `wasm_alloc` can grow memory and detach views);
   morph ids are validated as u16 in JS.
3. **Every `.afpp` was v1; the core requires v2.** `real_test.afpp` and the JS pack builder in the
   web-three tests were v1. Rebuilt with `pack_builder`; the JS builder now writes v2 by hand.
4. **`packages.zip` from your drive was not newer than the repo.** All source files identical; its
   `dist/` was stale (built 01:39 on 09-13, before the 19:58 source change: no `getSkeleton`/`toSkinnedMesh`).
5. **The repo's own web tests failed 5/5 as shipped** (`init()` got a 404: tests served only `dist/`,
   the pack lived at repo root). Packs now live in `packages/web/fixtures/`, served by a shared harness.
6. **`measure-size.mjs` pointed at `packages/core/target/...`** (nonexistent); now `rust-core/target/...`.

## Evidence
- **DNA layout, verified functionally (not just derived):** bogus `arms_id`/`legs_id` are echoed back by
  name in the error; `height=0` gives the scale error; morph 5001@1.0 displaces by 0.0480 (identical to
  the native run), @0.5 by exactly half. The struct size 56 itself is the allocation; Rust reads through
  offset 52. No wasm32 `size_of` assert exists in the crate (only native, 72).
- **Real body through the whole chain** (`src/index.cc0.test.mjs`): 53,512 vertices / 80,268 indices;
  height scales Y only (1.3x), weight scales X/Z only (1.4x); morph linear in weight; different morphs differ;
  unknown morph id skipped; morphs applied BEFORE body scale (ratio 1.15, measured through JS+wasm);
  `getSkeleton()` returns one joint per bone with a single root.
- **Mutation checks on the built bridge** (restored byte-identical afterwards): not writing `arms_id`
  fails 13 of 15 tests; writing the morph count at offset 52 fails exactly the two morph-behaviour tests
  and nothing else.
- **web-three's hand-written v2 pack builder is accepted by the real Rust loader** and yields the exact
  3-joint skeleton and a correctly bound `SkinnedMesh` (independent check of the JS layout code).
- **Benchmark** (Node, this sandbox's CPU, not a browser; `bench/benchmark.mjs`, 200 calls, 0 errors,
  deterministic): body only — first call 40.7 ms, p50 6.39, p95 14.6, p99 18.6 ms; with both morphs —
  first 47.8 ms, p50 7.68, p95 15.9, p99 18.7 ms (~1.3 ms median for morph blending). JS heap flat, but
  **RSS grew 21 MB / 30 MB over the 200 calls** (wasm memory never shrinks): could be a one-time
  high-water mark or a leak; 200 calls cannot tell. Not investigated.

## How the wasm was built (NOT the official toolchain)
Sandbox has apt + crates.io but no rustup. `rust-core/scripts/build-wasm-ubuntu-sandbox.sh` builds it
with Ubuntu's Rust 1.85.1 and `-Zbuild-std=std,panic_abort`, with two workarounds: (a) restore the
`dlmalloc` dependency Debian strips from the packaged std sources, in a private copy, **pinned `=0.2.7`**
(0.2.14 fails with "can't find crate for compiler_builtins"); (b) point the linker at apt's `wasm-ld`
(no bundled `rust-lld`). A build from this repo's own `rust-core/` takes ~2 min and behaves identically to
an earlier manual build on every check; the two are not byte-identical (816,167 vs 816,650 bytes) and I did
not determine why. `dist/anthroforge_core.wasm` = binaryen-optimized output of the script build
(816,167 -> 553,925 bytes, `validate()` passed). Imports: only `anthroforge_host.fill_random`.
Exports: 19 (the two extra vs the old build, `__data_end`/`__heap_base`, are linker defaults).
This corrects the earlier "wasm32 cannot be built in this sandbox" statement in the handoff.

## Not verified / open
- **Nothing was opened in a real browser** (no browser available here). The demo page was edited
  (real body, 4 ids, ear/nose sliders) but is untested; the Node chain is what is verified.
  `verification-evidence/demo-screenshot.png` is from the OLD demo and is now stale.
- **Official toolchain:** rebuild the wasm with your release toolchain (rustup) before publishing.
  `cargo build --target wasm32-unknown-unknown` for the CC0 core is unverified there; the Cargo.lock
  `wit-bindgen`/`edition2024` concern from the handoff is untested. The JS tests are the acceptance check.
- **Panic behaviour on wasm32:** this build uses panic-abort std; not tested. The repo's Phase 2 doc records
  the Phase 1 finding that a panic traps (no unwinding); the pasted evidence behind it is not in the repo.
- `licenseKey` is still accepted but consumed by nothing (unchanged).
- Prewarm: no JS calls `anthroforge_prewarm_clothing`; its signature is now
  `(head, torso, arms, legs, ids_ptr, count)`. The JS SDK does not expose it.

## Applying this to the repo
The zip has no `.git`. Unzip over a checkout, then
`git rm -r --cached rust-core/target` (if tracked), `git rm -r showcase-3d real_test.afpp`, and commit.
To rebuild: see `RUN_LOCALLY.md`. To test: `cd packages/web && npm install && npm test`, then the same in
`packages/web-three`.
