# AnthroForge/Web — project state snapshot

Current as of CC0-Phase 7 (see `CC0_PHASE_7_NOTES.md` for the full record,
evidence and open items).

## What's in this tree

- `rust-core/` — the CC0-Phase 6 Rust core: real 4-part CC0 MakeHuman body
  (head/torso/arms/legs, all required), morph targets, Part Pack format **v2**,
  clothing anchors that cover all four parts. 124 native tests pass.
- `packages/web/` — `@anthroforge/web`: the wasm SDK. `dist/` is a fresh build of
  the current core (wasm + JS + types). `fixtures/` holds two v2 Part Packs.
  `npm test`: 15 tests against the built wasm.
- `packages/web-three/` — `@anthroforge/web-three`: the three.js adapter
  (`toBufferGeometry`, `toSkinnedMesh`). 4 tests pass against the new wasm.

## Status

- The wasm in `packages/web/dist/` was built in a sandbox with a non-official
  toolchain (see the notes). Rebuild it with your release toolchain before
  publishing; the JS tests are the acceptance check.
- Nothing here has been opened in a real browser since the CC0 change. The
  Node-level chain (JS API -> bridge -> wasm -> mesh) is what is verified.
- KayKit demo assets were removed (no longer needed).

## Naming

`PHASE_3_MERGE_NOTES.md` and `rust-core/PHASE_2_*` are this repo's own, older
phase history (web packaging; pack loading). Everything about the CC0 body work
is prefixed **CC0-Phase N**.
