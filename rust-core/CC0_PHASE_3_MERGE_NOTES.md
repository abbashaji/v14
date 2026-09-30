# CC0-Phase 3 Merge Notes — CharacterDNA ext, AFPP v2, morph blending

## What was merged

Three independently-delivered pieces, folded into one working pipeline:

- **Part A** (`vertex_duplication_map.rs`) — render-vertex → base-mesh-vertex
  correspondence, by brute-force nearest-neighbor position matching.
- **Part B** (`morph_loader.rs`, `morph_blend.rs`) — the AFMT binary
  morph-delta format loader, and applying one or more loaded morphs at
  arbitrary weights onto a render-vertex buffer via a duplication map.
- **This integration**: wiring both parts into the real crate —
  `CharacterDNA` extended with `arms_id`/`legs_id`/active-morph fields,
  `generate_character` merging all 4 body parts and applying morph
  blending, the AFPP pack format bumped to v2 (adds a morph section and
  per-part duplication maps), and `pack_builder` rewritten to emit it.

## Step 0 — re-verifying Part A/B before building on them

Ran `cargo test --release` filtered to each of the three new files
individually (`cargo test` only accepts one `TESTNAME` filter, so this was
three separate invocations, not the one combined command the task spec's
example command implied):

```
vertex_duplication_map: 7 passed; 0 failed
morph_loader:            6 passed; 0 failed
morph_blend:             3 passed; 0 failed
```

All 16 passed against the real fixtures (`assets/upstream/base.obj`,
`tests/fixtures/cc0_phase2_rigged_body/4001_head.glb`,
`tests/fixtures/cc0_phase3_real_morphs/5001_asym_ear_1_l.afmt`), including
the real head-mesh duplication-map test (17,224 render vertices, all
matched, 4,338 distinct base-mesh vertices) and the real AFMT fixture
parse (635 deltas). No mismatch found between either part's own doc
comments and its delivered code — both matched their own task specs as
written, nothing needed correcting before integration.

## Environment note (not a code issue, but real friction)

The sandbox's pre-installed `cargo`/`rustc` was 1.75.0 (apt's default
`cargo`/`rustc` packages), which could not even `cargo fetch` this
project's `Cargo.lock` — one locked dependency (`wit-bindgen`, a
transitive wasm32-only dependency) requires the `edition2024` Cargo
feature, unavailable before Rust 1.85. Installed `rustc-1.91`/`cargo-1.91`
(available as separate apt packages on this distro) and symlinked them
ahead of the defaults on `PATH`. Separately, `rustdoc` stayed pinned at
the old 1.75 binary even after that (it's a distinct apt package,
`rustc-1.91` doesn't pull in a matching `rustdoc`), which made `cargo test
--release`'s doctest pass fail with an unrelated-looking `-Z
unstable-options`/`check-cfg` error — symlinked `rustdoc-1.91` over it too
once traced back to a version mismatch rather than a real doctest
failure.

## Step 2 — `CharacterDNA` extended

Five fields appended after the existing `equipped_clothing_count` (not
interleaved near `head_id`/`torso_id`, so every pre-existing field keeps
its exact offset): `arms_id: u32`, `legs_id: u32`,
`active_morph_ids_ptr: *const u16`, `active_morph_weights_ptr: *const
f32`, `active_morph_count: u32`. Struct grew from 40 to 72 bytes (worked
out by hand from `repr(C)` field alignment, then confirmed by the crate's
own compile-time `size_of` assertion — the pointer fields force 8-byte
alignment, which pads `legs_id`'s end at offset 44 up to 48 before
`active_morph_ids_ptr`, and pads the whole struct's end at 68 up to 72).

Found and updated **every** `CharacterDNA { ... }` construction site in
the tree (11 total): `benches/support/mod.rs`,
`benches/generate_character_bench.rs` (6 call sites),
`src/clothing_deformer.rs` (2, test-only), `src/lib.rs` (8, test-only),
`tests/real_pack_e2e.rs`, and `tests/cc0_phase2_rigged_body_e2e.rs` (2).
`arms_id`/`legs_id` are required, exactly like `head_id`/`torso_id` — no
"0 means skip" sentinel was invented for them, per the task spec's
explicit instruction. Where a test didn't care about real arms/legs
geometry, it reuses an already-known-valid id (documented inline at each
site, e.g. `arms_id: head_id` for a torso-lookup-failure test that never
reaches the arms/legs check anyway).

`benches/support/mod.rs` needed more than a mechanical field addition:
`make_dna`'s benchmark bodies previously only had head+torso synthetic
assets. Added `generate_arms`/`generate_legs` mesh generators (same
tapered-tube shape family as the existing `generate_torso`),
`FIXED_ARMS_ID`/`FIXED_LEGS_ID` constants, and extended the cold-cache
pool from `(head, torso)` pairs to `(head, torso, arms, legs)` quads —
otherwise every bench iteration would fail to resolve `arms_id`/`legs_id`
against the registry and `generate_character` would return null for the
whole benchmark.

## Step 3 — AFPP v2 pack format

`pack_builder.rs` rewritten (CLI: `pack_builder <input_dir> <output_pack>
[<base_mesh_obj_path>]`, the third argument now optional):

- A `.afmt` file directly inside the input directory is now treated as a
  morph, not a part — its numeric filename prefix becomes the `morph_id`
  (returns a clear error if it doesn't fit in `u16`), its bytes are
  embedded unmodified (no AFMT-structure validation at build time — that
  stays `morph_loader.rs`'s job at load time, per the task spec).
- New v2 byte layout: `morph_count` added to the 20-byte header, part
  index entries grew from 13 to 21 bytes (added `dupmap_offset`/
  `dupmap_len`), a new 12-byte-entry morph index table. Every
  offset/length is written and read as explicit little-endian bytes, no
  `#[repr(C)]` pointer-cast — same convention the v1 format and
  `morph_loader.rs`'s own AFMT format already use.
- When a base mesh path is given, each part's own render-vertex positions
  are parsed (a standalone OBJ `v`-line reader and a standalone
  GLB-POSITION-accessor reader, both duplicated rather than shared with
  `obj_loader.rs`/`gltf_loader.rs`/`vertex_duplication_map.rs`, preserving
  this binary's existing zero-library-dependency isolation) and fed
  through a duplicated copy of `vertex_duplication_map::
  build_duplication_map`'s brute-force nearest-neighbor logic (kept
  behaviorally identical, including the tie-break rule). Without a base
  mesh, every part's `dupmap_len` is 0 (no duplication map computed) —
  not an error.
- Added 5 tests of its own: a structural round-trip with no base mesh, a
  hand-checked 2-vertex duplication-map computation against a 3-vertex
  synthetic base mesh (confirms the actual index values, not just
  presence), `.afmt`-is-a-morph-not-a-part, duplicate-part-id rejection,
  and `.gltf`-is-skipped-with-warning. All 5 pass.
- `PartData` gained `duplication_map: Option<Vec<u32>>`; `Registry` gained
  `morphs: HashMap<u16, LoadedMorph>`. Both plain-directory-scan
  (`init_part_registry_impl`) and v2-pack (`init_part_registry_from_pack_impl`)
  construction sites updated — the plain-directory path always sets
  `duplication_map: None` and `morphs: HashMap::new()` (no concept of
  either), matching that path's existing scope.
- `init_part_registry_from_pack_impl` rewritten for the v2 layout: rejects
  `version != 2` outright (no v1/v2 dual support, per the project's
  established one-version convention — same policy `morph_loader.rs`'s
  own `UnsupportedVersion` already follows), parses the new header/index
  tables, validates every part's duplication map length against that
  part's own vertex count (a length mismatch is a hard error, not a
  silent truncation), and loads every morph via the real
  `morph_loader::load_afmt_bytes` — a morph that fails to parse fails the
  whole call, same fail-fast policy as a bad part.

## Step 4 — `generate_character` wiring

**4a (arms/legs merge)**: two more `let Some(x) = registry.parts.get(...)
else { ...error... }` blocks added, copying the existing head/torso
blocks' exact structure and error-message style. The first
`mesh_merge::merge_parts` call now takes all 4 parts, in
head/torso/arms/legs order (matching what the updated
`real_pack_e2e.rs`/`cc0_phase2_rigged_body_e2e.rs` expected counts
assume). Confirmed by reading it (not assumed) that DNA mutation and
clothing fitting/merging downstream of that call already operate on the
merged buffer generically — neither needed a code change for this part.

**4b (morph blending)**: after resolving all 4 parts and before merging
them, if `dna.active_morph_count > 0`, each active
`(morph_id, weight)` pair is resolved against `registry.morphs` (an
unknown id is logged and skipped, same policy as an unknown clothing id).
Then, independently per part, `morph_blend::apply_morph_targets` is
called on a **clone** of that part's vertex buffer using that part's own
`duplication_map` — a part with `duplication_map: None` has morph
application skipped for it, not an error. The registry's own stored
vertices are never mutated (same pattern `mutate_skin_vertices`'s
existing height/weight step already follows — read directly to confirm,
not assumed).

## Step 5 — new end-to-end test

`tests/cc0_phase3_morph_pipeline_e2e.rs`: builds a real AFPP v2 pack (via
the real `pack_builder` binary) from a new combined fixture directory,
`tests/fixtures/cc0_phase3_pipeline/` (the same 4 renamed GLBs +
`master_skeleton.json` as `cc0_phase2_rigged_body`'s fixture set, plus
both real `.afmt` files from `cc0_phase3_real_morphs/` — the existing
Phase 2 fixture directory itself was left untouched), with
`assets/upstream/base.obj` as the base-mesh argument so real duplication
maps get computed for all 4 real body parts.

Calls `generate_character` twice on the real, full 4-part body
(`head=4001, torso=4002, arms=4003, legs=4004`): once with no active
morphs, once with the real `5001` ear morph active at weight `1.0`. Both
return non-null with the same **53,512 vertices / 80,268 indices** (the
same full-body total Step 2's updated `cc0_phase2_rigged_body_e2e.rs`
now asserts) — proof morph application doesn't change topology. Then
reads both output vertex buffers directly and confirms at least one
position genuinely differs (not a no-op) with max per-vertex displacement
under the 1.0-world-unit sanity bound (no exact-position assertion, to
avoid re-deriving the morph math in the test itself). Passes.

Building this pack (computing 4 real duplication maps by brute force
against the 19,158-vertex base mesh — roughly 53,512 × 19,158 ≈ 1.03
billion squared-distance comparisons total) took under 2 seconds in
release mode; not a bottleneck.

## Step 6 — full-suite verification

Real pass/fail counts, `cargo test --release` from `rust-core/`, after
fixing the `rustdoc` version mismatch noted above:

```
running 89 tests   (src/lib.rs unit tests)
test result: ok. 89 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

running 6 tests    (src/bin/morph_converter.rs)
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

running 5 tests    (src/bin/pack_builder.rs — new in this phase)
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

running 2 tests    (tests/cc0_base_mesh_e2e.rs)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

running 1 test     (tests/cc0_phase2_rigged_body_e2e.rs — updated this phase)
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

running 1 test     (tests/cc0_phase3_morph_pipeline_e2e.rs — new this phase)
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

running 1 test     (tests/real_pack_e2e.rs — updated this phase)
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

Doc-tests anthroforge_core: running 0 tests, ok
```

**105 tests total, 0 failed** (86 before this phase per
`CC0_PHASE_2_MERGE_NOTES.md`, +16 from Part A/B's own `vertex_duplication_map`/
`morph_loader`/`morph_blend` modules now folded into the 89-test `lib.rs`
count, +5 new `pack_builder` tests, +1 new `cc0_phase3_morph_pipeline_e2e`
test; `cc0_base_mesh_e2e`'s 2 and the other e2e tests' counts are
unchanged in count, though `real_pack_e2e`/`cc0_phase2_rigged_body_e2e`'s
single tests now exercise real 4-part bodies with updated expected
counts).

`cargo build --target wasm32-unknown-unknown --release`: **not buildable
in this environment**, confirmed directly (not skipped silently) — fails
with `error[E0463]: can't find crate for std`, because no
`wasm32-unknown-unknown` standard library target is installed, and
there's no `rustup` available to install one (`rustup target add` needs
`static.rust-lang.org`, blocked by this sandbox's network policy — the
exact limitation the task spec anticipated). Same standing limitation
`CC0_PHASE_2_MERGE_NOTES.md` already documented; not a new finding, and
nothing in this phase's changes (the `CharacterDNA` size assertion is
already `#[cfg(not(target_arch = "wasm32"))]`-gated, unchanged in that
respect) is expected to newly break a wasm32 build once one is
reachable — that's an assumption, though, not something this environment
could actually verify.

- **Vertex-duplication-map accuracy on non-head parts**: **closed** — see
  the addendum at the end of this file.
- **Clothing anchors still only consider head+torso**: read
  `Registry::get_or_build_skin_tree` and `get_or_build_clothing_anchors`
  directly (not assumed) — both still build the shared KD-tree from only
  the `(head_id, torso_id)` bind-pose pair, exactly as before this phase.
  This was already true before CC0-Phase 2's arms/legs parts existed as
  loadable parts at all, so it's not a regression this merge introduced,
  but it does mean an equipped clothing item's fitting/clearance logic
  has no awareness of the arms/legs geometry now present in the full
  merged+mutated skin buffer `generate_character` actually outputs — a
  jacket sleeve or pant leg fitted this way could clip through (or leave
  a visible gap from) the real arm/leg mesh, since the anchors it was
  built against never saw that geometry. Left unchanged per the task
  spec's "don't touch `clothing_deformer.rs`... unless reading shows it
  genuinely needs a change" instruction — this is a real design gap
  worth its own scoped follow-up (probably: build the KD-tree from all 4
  parts, not just 2), not a quick fix bolted onto this merge.
- **AFMT's always-zero `normal_delta`**: `morph_blend::apply_morph_targets`
  does apply `delta.normal_delta * weight` to each morphed vertex's
  normal exactly like it does for position, and the real
  `5001_asym_ear_1_l.afmt` fixture's normal deltas were not specifically
  inspected for whether they're actually always zero (that was Part B's
  own task-spec framing, not independently re-checked here). If they
  are, the morphed output's normals never move even though positions do —
  which would eventually show up as visibly wrong lighting on a morphed
  ear once this is renderer-side, since normals meant to be
  re-oriented by a shape change would still point the old direction. Not
  a bug in the code delivered here (it applies whatever's in the file
  correctly either way), but worth resolving whether real production AFMT
  morphs are expected to ship nonzero normal deltas, or whether normals
  need a recompute-from-topology step after morphing, before this is
  relied on for real rendered output.

## What this merge deliberately did NOT do

- Did not make `init_part_registry_from_pack_impl` accept both v1 and v2
  packs — one supported version, hard rejection of anything else, per
  this project's established convention.
- Did not invent a "0 means skip" sentinel for `arms_id`/`legs_id` — both
  required, exactly like `head_id`/`torso_id`.
- Did not touch `body_mutation.rs`, `texture_atlas.rs`, or
  `skeleton_resolver.rs` at all. Did touch `clothing_deformer.rs` only
  for the two required `CharacterDNA` construction-site field additions
  (test-only helpers) — no behavioral change, see the open item above for
  why a real behavioral change there was read about but deliberately not
  made in this merge.

## Addendum — duplication-map accuracy on non-head parts (closed)
The open item above was closed directly, without a new fresh context —
it only needed the same real-data assertion the head test already made,
applied to the other three real parts:
`real_torso_render_vertices_all_match_real_base_mesh`,
`real_arms_render_vertices_all_match_real_base_mesh`, and
`real_legs_render_vertices_all_match_real_base_mesh` (all in
`src/vertex_duplication_map.rs`'s test module). Run for real, not
asserted from memory: torso 5,840/5,840 render vertices matched (1,552
distinct base-mesh vertices), arms 17,280/17,280 (4,364 distinct), legs
13,168/13,168 (3,326 distinct) — same `0.0005` tolerance, same 100%
coverage the head test already established. Full suite re-run after:
**108 tests, 0 failed** (was 105).
