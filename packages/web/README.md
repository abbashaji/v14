# @anthroforge/web

WebAssembly-powered character generation for the browser.

## Install

```sh
npm install @anthroforge/web
```

## Usage

```ts
import { init, generate, getLastError, getSkeleton } from "@anthroforge/web";

await init({
  partPackUrl: "/parts/cc0_body.afpp", // a v2 Part Pack (see below)
  licenseKey: "",
});

const character = generate({
  seed: 42n,
  heightModifier: 1.0, // scales Y; must be finite and > 0
  weightModifier: 1.0, // scales X and Z; must be finite and > 0
  // All four body parts are required -- there is no "skip" value.
  headId: 4001,
  torsoId: 4002,
  armsId: 4003,
  legsId: 4004,
  clothingIds: [],
  // Optional: morph targets by id. An id that isn't in the pack is skipped.
  morphs: [{ id: 5001, weight: 0.7 }],
});

if (!character) {
  console.error(getLastError());
}
```

`generate()` returns de-interleaved typed arrays (`positions`, `normals`, `uvs`,
`boneIndices`, `boneWeights`, `indices`). `getSkeleton()` returns the global
bone hierarchy once, after `init()`.

**Part Packs are v2.** A pack built for an earlier version of the SDK (format
v1) is rejected by `init()`. Build packs with `rust-core`'s `pack_builder`
(`pack_builder <asset_dir> <out.afpp> [<base_mesh.obj>]`; pass the base mesh to
enable morph targets). `fixtures/` has two ready-made packs: `real_test.afpp`
(a 15-vertex synthetic body for fast tests) and `cc0_body.afpp` (the real CC0
MakeHuman body, ~3.4 MB: parts 4001-4004, morphs 5001 and 5002).

**Breaking change from earlier versions:** `armsId` and `legsId` are now
required fields on `CharacterDNA`, and `morphs` is new.

When you're done with a generated character, release its underlying
wasm-side memory:

```ts
import { freeCharacter } from "@anthroforge/web";

freeCharacter(character);
```

## Size

Measured with `npm run build && npm run measure-size`. **Caveat:** the raw wasm
below was built in a sandbox with Ubuntu's repackaged Rust 1.85.1 (`-Zbuild-std`),
not the toolchain you will release with, so treat the raw/optimized figures as
indicative and re-measure from your release build.

| Artifact | Size |
|---|---|
| Raw `.wasm` (Rust build output) | 816,167 bytes (797.04 KB) |
| Optimized `.wasm` (after `optimize-wasm.mjs`) | 553,925 bytes (540.94 KB), 32.1% reduction |
| Bundled JS (`dist/index.js`) | 11,155 bytes (10.89 KB) |
| **Combined package size** (wasm + js) | **565,080 bytes (551.84 KB)** |

## Tests

`npm test` runs `src/index.test.mjs` (tiny fixture pack) and
`src/index.cc0.test.mjs` (the real CC0 body, through the JS bridge and the wasm).

## License

TBD
