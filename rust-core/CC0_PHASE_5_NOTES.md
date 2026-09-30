# CC0-Phase 5 notes -- diversity regression test suite

> **Read with the appended sections at the bottom.** Everything from here to the `---` divider describes the test file *as delivered* (8 tests; test 7's ordering only printed). The final state in the base project is 9 tests with the ordering asserted -- see "Follow-up" below.

**Deliverable:** one new file, `tests/cc0_phase5_diversity_regression.rs` (8 tests).
No existing file modified (`diff -rq` of `src/` and `tests/` against the uploaded
zip shows only this new file). No new fixtures. `CharacterDNA.seed` is never
varied or read (it is only set to a constant `42`, because the struct requires it).

## Verification run 1 -- `cargo test --release cc0_phase5_diversity_regression -- --nocapture`

```
running 8 tests
wrote 4 part(s), 2 morph(s), 3397297 bytes, to '.../cc0_phase5_diversity_regression_generated.afpp'
[anthroforge] initialized part registry with 4 part(s) (from pack)
[anthroforge] generate_character: no morph loaded for active_morph_id 9999; skipping this morph
[anthroforge] generate_character: DNA mutation failed: scale[1] = 0 is invalid; ...
[anthroforge] generate_character: DNA mutation failed: scale[1] = -1 is invalid; ...
[anthroforge] generate_character: DNA mutation failed: scale[1] = NaN is invalid; ...
[t1] extents base=[9.9254, 16.6589, 4.2301] tall(1.3)=[9.9254, 21.656567, 4.2301] y-ratio=1.2999998
[t2] extents base=[9.9254, 16.6589, 4.2301] heavy(1.4)=[13.895559, 16.6589, 5.92214] x-ratio=1.4 z-ratio=1.4
[t3] base=[9.9254, 16.6589, 4.2301] combined=[8.43659, 19.99068, 3.595585]
     height-only(1.2)=[9.9254, 19.99068, 4.2301] weight-only(0.85)=[8.43659, 16.6589, 3.595585]
[t5] max displacement @0.5=0.02563217 @1.0=0.05126389 ratio=1.9999826
[t7] order probe @vertex 15792: unscaled morph dY=0.04799986, scaled morph dY=0.055200577, ratio=1.1500154
test cc0_phase5_diversity_regression_1_height_modifier_scales_y_extent_only ... ok
test cc0_phase5_diversity_regression_2_weight_modifier_scales_x_and_z_extent_only ... ok
test cc0_phase5_diversity_regression_3_height_and_weight_compose ... ok
test cc0_phase5_diversity_regression_4_invalid_height_modifier_is_rejected ... ok
test cc0_phase5_diversity_regression_5_morph_displaces_and_weight_scales_displacement ... ok
test cc0_phase5_diversity_regression_6_different_morphs_produce_different_results ... ok
test cc0_phase5_diversity_regression_7_morph_and_body_scale_compose ... ok
test cc0_phase5_diversity_regression_8_unknown_morph_id_is_skipped_not_fatal ... ok
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.97s
```

## Verification run 2 -- full `cargo test --release`

| binary | passed |
|---|---|
| unittests src/lib.rs | 92 |
| unittests morph_converter | 6 |
| unittests pack_builder | 5 |
| tests/cc0_base_mesh_e2e.rs | 2 |
| tests/cc0_phase2_rigged_body_e2e.rs | 1 |
| tests/cc0_phase3_morph_pipeline_e2e.rs | 1 |
| **tests/cc0_phase5_diversity_regression.rs** | **8** |
| tests/real_pack_e2e.rs | 1 |
| doc-tests | 0 |
| **Total** | **116 passed, 0 failed** |

## Test 7: morph vs. body-mutation ordering (found in `generate_character`)

**Morphs are applied BEFORE body mutation.** Code order in `src/lib.rs`: per-part
`morph_part()` (`apply_morph_targets` on a copy of each part's vertices) ->
`merge_parts` -> `dna_scale_from_character_dna` -> `mutate_skin_vertices`. Confirmed
empirically by the diagnostic in test 7: the largest morph vertex's Y displacement
grows by exactly the height scale (0.0480 -> 0.0552, ratio 1.1500 with
`height_modifier` 1.15), i.e. the morph delta is itself scaled. The test only asserts
that combining changes the output (as the spec asks); the ordering is printed, not asserted.

## Deviations from / contradictions with the spec

1. **One registry per process -> one shared pack init, not one pack per test.**
   `GLOBAL_REGISTRY` is a process-wide `OnceLock`; a second
   `init_part_registry_from_pack` returns `false`, and all 8 tests share one process on
   parallel threads. So `ensure_registry()` (a `std::sync::Once`) builds the pack via the
   real `pack_builder` and initializes once; each test builds its own `CharacterDNA`.
2. **Tests 1-4 use `cc0_phase3_pipeline/`, not `cc0_phase2_rigged_body/`.** Same
   consequence of (1). The four body GLBs and `master_skeleton.json` are byte-identical
   between the two fixture dirs (`cmp`), and the morphs are inert unless activated, so
   the scale tests exercise the same geometry.
3. **Test names carry the `cc0_phase5_diversity_regression_` prefix.** `cargo test
   <filter>` matches test *function* names, not file names; with short names the spec's
   verification command would have run 0 tests.
4. **Baseline is 108 pre-existing tests, not 105.** Full-suite total is 116 = 108 + 8.
   (92+6+5+2 = 105 covers only the unit tests and `cc0_base_mesh_e2e`; the spec's figure
   appears to omit the three single-test e2e files.) Nothing pre-existing failed.
5. **Slack was not adjusted.** The real numbers are essentially exact (Y 1.2999998x,
   X/Z 1.4x, morph weight ratio 1.99998x), so the spec's slack (1.25x / 1.3x / 1.8x /
   1%) is loose but valid; I left it as specified rather than tightening.

## Additions beyond the spec (small, all strictly stronger)

- Test 3 also runs height-only (1.2) and weight-only (0.85) and asserts the combined
  result differs from each, *and* that Y ~ 1.2x / X,Z ~ 0.85x base (2% tolerance).
  Comparing only against tests 1/2's 1.3/1.4 boxes would have been trivially true.
- Test 4 also asserts `anthroforge_last_error()` reports "DNA mutation failed" for each
  bad value, and that a valid call afterwards still succeeds.
- Test 7 also asserts the scaled+morphed output differs from the morph-only output.
- Test 8 also asserts vertex positions are identical to the baseline and counts equal
  53,512 / 80,268.

## Mutation sanity check (scratch copy, reverted)

Temporarily changing `dna_scale_from_character_dna` to `[height, girth, girth]` made
tests 1, 2 and 3 fail (5 passed / 3 failed), so the suite does catch the wrong-axis
regression it was written for. Reverted; `src/` verified identical to the upload.

## Environment note

No rustup access (static.rust-lang.org blocked); used apt's cargo/rustc 1.75.0 -- the
same toolchain the repo's Cargo.toml comments say it was developed against. Crates
were fetched from crates.io. `CARGO_TARGET_TMPDIR` was unset at runtime, so the
generated `.afpp` went to the system temp dir (same fallback the existing e2e tests use).

---

# Merge verification (main context) -- appended after the section above

The section above is the task-spec context's own write-up, kept verbatim. Below is
what the merge step re-checked directly, not from that write-up.

## What was done
`cc0_phase5_diversity_regression.rs` was copied byte-for-byte (`cmp`-verified) into
`tests/`, then edited in the follow-up described below. Nothing else in the tree changed: `diff -rq` against the original
`rust-core-BASE-for-cc0-phase5.zip` shows exactly one difference in `src/`, `tests/`,
`Cargo.toml`, `Cargo.lock` -- the new test file. No `[[test]]` entry is needed
(`Cargo.toml` has no `autotests` override; the file is auto-discovered).

## Verified by real runs in this sandbox (apt cargo/rustc 1.75.0)
- **Baseline before adding the file:** 92+6+5+2+1+1+1 = **108 passed, 0 failed** --
  reproduces the handoff's figure from real output.
- **New file alone, `--nocapture`:** 8 passed. Diagnostic numbers match the section
  above (Y-ratio 1.2999998, X/Z ratio 1.4, morph weight ratio 1.99998, order probe 1.1500).
- **Full suite, as delivered:** **116 passed, 0 failed**, cargo exit 0 (117 after the
  follow-up below adds test 9).
- **Mutation check reproduced independently:** `dna_scale_from_character_dna` changed
  to `[height, girth, girth]` -> tests 1, 2, 3 FAILED, tests 4-8 passed (5/3 split).
  Restored; `src/clothing_deformer.rs` verified byte-identical to the zip original,
  and the full suite was re-run afterward on the restored tree.

## Claims in the section above that were re-checked against the actual repo
- `cc0_phase3_pipeline/` exists; its 4 GLBs + `master_skeleton.json` are `cmp`-identical
  to `cc0_phase2_rigged_body/` (the file's deviation #2 is accurate).
- Morphs run before body mutation: read in `src/lib.rs` -- per-part `morph_blend`
  (lines ~1247-1271), then `merge_parts` (~1280), then `dna_scale_from_character_dna`
  + `mutate_skin_vertices` (~1296-1298). Agrees with the test-7 probe.
- `CharacterDNA.seed`: `grep -rn "\.seed\b"` over `src/ benches/ tests/ examples/` finds
  no reads. (Grep only; a field read via destructuring or a macro would not match.)
- `anthroforge_last_error()` is backed by a `thread_local!` (`src/error.rs`), so test 4's
  last-error assertions are safe under parallel test threads.

## Follow-up: the two gaps found during merge verification were closed
Both edits are in `tests/cc0_phase5_diversity_regression.rs` only; no file under `src/`
was changed (`diff -rq` against the original zip: the only difference is this test file).
1. **Test 7 now asserts the morph-then-scale order** instead of only printing it: the
   largest morph vertex's Y displacement with scale vs. without must have ratio 1.15
   +/- 0.01 (measured 1.1500154).
2. **New test 9** (`..._9_invalid_weight_modifier_is_rejected`): `weight_modifier` of
   0.0 / -1.0 / NaN each returns null with a "DNA mutation failed" last-error, and a valid
   call afterward still succeeds. Its errors name `scale[0]`; test 4's name `scale[1]`.
   Test 4 is unchanged.

Each addition was mutation-checked on the real code (source restored and verified
byte-identical to the original zip after each):
- **Order swap** (each part scaled before morphing, identity scale after the merge):
  only test 7 fails, `got ratio 1.0000099`. Before this follow-up no test failed on this.
- **Validation narrowed to component 1 only** (`body_mutation::mutate_skin_vertices`):
  only test 9 fails (`weight_modifier = 0.0 must make generate_character return null`);
  test 4 still passes, i.e. test 9 covers something test 4 does not.

Final full suite after the follow-up: **117 passed, 0 failed** (108 pre-existing + 9 here).

## Things not verified
- The original `TASK_SPEC_PHASE5_diversity_regression.md` was not attached; the file was
  judged on its own behaviour and the handoff's summary of that spec's intent.
- `wasm32-unknown-unknown` still cannot be built here. Attempted for real:
  `cargo build --release --lib --locked --target wasm32-unknown-unknown` fails at
  `error[E0463]: can't find crate for 'std'` (first crate hit: `serde_core`); `rustup` is
  not installed and `https://static.rust-lang.org` returns HTTP 403 from this sandbox.
  These tests are native-only and say nothing about wasm32.
- CC0-Phase 4's open item (does `panic=unwind` unwind for an *arbitrary* panic on wasm32)
  is untouched by this phase and remains open.

## Housekeeping
- The handoff calls the Phase 3 notes `CC0_PHASE_3_MERGE_NOTES.md`; the zip's file is
  named `CC0_PHASE_3_MERGE_NOTES_updated.md`. Contents were not re-audited here.
- Sandbox: a cold `cargo test --release` exceeds the 300 s per-command limit. A plain
  `nohup ... &` job was killed when the call returned; `setsid nohup sh -c '...' < /dev/null &`
  survived and could be polled. `time` is not available in this shell.
