# CC0-Phase 6 notes -- clothing anchors now cover all four body parts

Done directly in the main context (one source file, nothing parallelizable), not via a
fresh context. Base: the CC0-Phase 5 state (117 tests). **Final: 124 passed, 0 failed**
(`cargo test --release`, exit 0; apt cargo/rustc 1.75.0).

## The bug (confirmed on real data before touching anything)
`Registry::get_or_build_skin_tree` merged **only head + torso** to build the clothing
KD-tree, and both caches were keyed on `(head_id, torso_id)`. `fit_clothing_to_skin`
copies the anchor skin vertex's `bone_indices`/`bone_weights` onto each garment vertex, so a
garment vertex on an arm or leg inherited *torso* bone data. Measured with a throwaway probe
on the real 4-part body: a garment vertex 0.05-0.07 units from an arm vertex (bones
`[69,0,0,0]`) inherited `[1,42,41,2]` from a torso vertex ~3.9 units away. Static positions
under plain height/weight scaling are probably unaffected (fitted position comes out the same
whichever vertex is the anchor -- reasoned from the code, not tested); the visible damage is
in animation and under any non-affine deformation.

## What changed
Only `src/lib.rs` differs from the original zip in `src/` (84 lines changed-from, 381
changed-to, most of it tests/docs). `Cargo.toml`, `Cargo.lock`, `benches/` untouched.
- New `pub(crate) struct BodyPartIds { head, torso, arms, legs }` -- the cache key and the
  parameter for `get_or_build_skin_tree`, `get_or_build_clothing_anchors`,
  `prewarm_clothing_anchors_impl`. A struct because four positional `u32`s are easy to transpose.
- `get_or_build_skin_tree` merges head, torso, arms, legs **in `generate_character`'s order**.
  This is an invariant (documented in the code): anchors store indices into this merged
  buffer and are looked up in `generate_character`'s merged body skin; the index spaces only
  agree because order and per-part vertex counts are identical.
- `ClothingAnchorError` gained `UnknownArmsId` / `UnknownLegsId`.
- `generate_character` builds `BodyPartIds` from the DNA and passes all four.
- **ABI change:** `anthroforge_prewarm_clothing` is now
  `(head_id, torso_id, arms_id, legs_id, clothing_ids_ptr, clothing_count)`, changed in place
  (the product is browser-only now; there is no Unreal caller to keep compatible). The JS glue
  that calls it is NOT in this repo and must be updated. A stale caller passing the old 4
  arguments would land its pointer/count in `arms_id`/`legs_id` and the real ones would read
  as 0, making the call a no-op rather than a fault (reasoned from wasm calling convention,
  not observed). Nothing in this repo's `.mjs` files references it.
- The other two clothing exports (`build_cloth_anchors_for_part`, `fit_clothing_to_character`)
  take the skin mesh from the caller and were already body-agnostic; unchanged.

## Tests
- **New `tests/cc0_phase6_clothing_limb_anchoring.rs` (3 tests).** Real vertex positions and
  normals are read from the fixture GLBs (`cc0_phase3_pipeline/`); one probe point per region
  (extreme-X arm vertex, median-Y leg vertex, front-most torso vertex) is placed 0.03 off the
  surface along its normal; a 3-triangle garment is added to a copy of the fixture dir; a real
  pack is built with the real `pack_builder`; the real `generate_character` runs with it
  equipped. Each garment vertex must carry bone data identical to a vertex of the intended
  region within 1.0 units. A setup guard asserts each probe is genuinely nearer its intended
  region than the others, so a bad probe fails loudly instead of passing vacuously.
- **Unit tests (in `src/lib.rs`, on a private in-memory `Registry`):** unknown arms/legs id is a
  typed error; bodies differing only in arms/legs never share cache entries; the skin buffer is
  head(3)+torso(4)+arms(5)+legs(6) in that order (checked via per-part bone tags); a garment on
  the arms anchors into merged indices 7..12 and one on the legs into 12..18.

## Verification, and what each check actually proved
- Full suite on the final tree: **124 passed, 0 failed**.
- Mutation, integration test (skin tree reverted to head+torso only): arm and leg tests FAIL,
  torso test passes -- i.e. the test catches the original bug and the torso case still works.
- Mutations against the full lib suite, source restored and re-verified after each:
  - head+torso-only merge -> exactly `..._four_body_parts_in_order` and
    `..._limb_garment_bind_to_that_limbs_vertices` fail (2 of 96);
  - anchor-cache fast path never hits -> exactly `clothing_anchor_cache_repeat_call_hits_cache`
    and `prewarm_clothing_anchors_impl_populates_cache` fail;
  - skin-tree key ignores arms/legs -> exactly
    `..._distinguishes_bodies_that_differ_only_in_arms_and_legs` fails.
- Timing (native release, one sample per configuration, so indicative only): first clothing
  call 9.2 ms with the 4-part tree vs 6.9 ms with the old head+torso tree; cached calls 2.26 vs
  2.18 ms. **wasm32 cost not measured.** Prewarm runs synchronously on wasm32, so the one-time
  cost is felt on the main thread/worker there.

## FINDING: pre-existing tests that asserted nothing
`registry_with_test_clothing_parts()` builds its registry with `init_part_registry` on an
OBJ-only temp dir. That init fails the global-skeleton completeness check
(`master skeleton names 1 bone(s) that no loaded part ever contributed hierarchy/bind-pose
data for: root`), so the helper returns `None` and every test using it hits its
`let Some(..) = .. else { return; }` and passes without asserting anything.
Confirmed by instrumenting the helper (the script confirmed the edit applied) and running the
full lib suite 3 times: **10 users were no-ops in every run** (9 "no registry", 1 "missing
parts" because another test won the global-registry race). That was 7 tests that existed
before this phase -- `..._shares_tree_across_clothing_items`, `..._repeat_call_hits_cache`,
`..._distinguishes_different_bodies`, `..._unresolvable_ids_error`,
`prewarm_clothing_anchors_impl_populates_cache`,
`generate_character_unknown_torso_id_sets_specific_last_error`,
`generate_character_composes_head_torso_and_fitted_clothing` -- plus 3 new ones written this
phase before the problem was found (replaced).
So the earlier "108 passing" baseline included tests that asserted nothing, and before this
phase no unit test actually executed the clothing-fit path.
(An earlier instrumentation attempt silently failed to apply and briefly produced a wrong
"never vacuous" conclusion; the numbers above come from the rerun that confirmed application.)

What was done: added `local_registry_with_test_clothing_parts()` (private `Registry`, no global,
always `Some`) and switched the 5 pre-existing cache/prewarm tests plus this phase's new
unit tests to it. Two of those five (`repeat_call_hits_cache`, prewarm) are proven able to fail
by the M2 mutation above; the other three (`shares_tree...`, `distinguishes_different_bodies`,
`unresolvable_ids_error`) run for real now and pass, but were not shown to fail under any
mutation.

## Still open / not verified
- **The two `generate_character_*` tests above are still no-ops.** They need the process-wide
  registry, so they can't use the local helper. A fix would give the OBJ fixture a
  `root`-contributing donor part (e.g. the tests' minimal GLB), but with one global `OnceLock`
  shared by ~96 lib tests that risks turning other order-dependent tests into no-ops, so it was
  not attempted unasked.
- `concurrent_generation_and_prewarm_stress` uses a different helper and was not checked for
  the same problem.
- wasm32: still cannot be built in this sandbox (no rustup; `static.rust-lang.org` blocked). This
  phase's tests are native-only. The prewarm export's new signature is untested from JS.
- Bench comments/code (`benches/generate_character_bench.rs`, `benches/support/mod.rs`) still
  model the stride sweep on a head+torso merge; production now merges four parts, so the
  bench-derived guidance on `SKIN_KDTREE_DECIMATION_STRIDE` (currently 4) was not re-run.
- Real garments: the repo has no real clothing assets, only synthetic ones, so nothing here
  validates visual fit on a real garment.
- Many comments/docs in the repo still refer to Unreal/C++ (e.g. "C++ mirror",
  `AnthroforgeCharacterAssembler.cpp`); not touched.
- CC0-Phase 4 note: the crate's own comments (`src/lib.rs` ~lines 28-46) state that
  `panic = "unwind"` does not unwind on wasm32 (a trap), citing a `RESULTS-03.md` that is not in
  this zip; the handoff describes that item as still open. Not checked by running a wasm build.
