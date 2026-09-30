# CC0-Phase 10 — Merge Notes

## Status: DONE. Code merged, bug fixed, all 6 packs rebuilt from the real corpus, fully verified.

*(This doc originally reported the pack rebuild as blocked on missing
`morph_id_map.json` / pack-composition data. The user then supplied
`CC0_PHASE_8_PACK_OUTPUT.zip`, which contained exactly what was missing
— see Section 5 below for what that unblocked and the real results.)*

## 1. Sources used
- Base tree: uploaded `rust-core.zip`.
- Part A: uploaded `CC0-Phase_10_Part_A-OUTPUT.zip` → `morph_converter.rs` (AFMT v2 writer),
  `morph_loader.rs` (v1+v2 reader), plus its test fixtures/e2e test.
- Part B: fetched per instruction from `https://github.com/abbashaji/temp`
  (single commit, `CC0-Phase 10 Part B-OUTPUT.zip`) → `pack_builder.rs`
  (content-hash dedup) and `cc0_phase10_dedup_equivalence.rs`.
  **Note on provenance:** this is a personal, unofficial GitHub repo, not
  something owned by this project. I inspected `pack_builder.rs` and the
  equivalence-check binary line-by-line before merging — no networking,
  process execution, or filesystem access outside expected temp-dir/test
  scope. Its content matches the Part B description in the task spec
  exactly (same `dedup_view` function, same CLI flag, same file list), so
  I merged it, but flagging the source for your own awareness since I
  can't vouch for who controls that repo going forward.

## 2. Merge actions taken
- Copied Part A's `morph_converter.rs`, `morph_loader.rs` and its
  test/fixture files in as-is (base's `morph_blend.rs`/`lib.rs` were
  confirmed untouched — Part A's overlay contains no changes to them).
- Copied Part B's `pack_builder.rs` and `cc0_phase10_dedup_equivalence.rs` in.
- **Applied the v1/v2 fix** to `dedup_view` exactly as specified: the
  version check now accepts both `1` and `2`; the slice split
  (`&bytes[..8]` + `&bytes[10..]`) is unchanged, since the morph_id sits
  at the same offset in both formats.
- **Defaulted dedup on**: `--dedup-ignore-afmt-morph-id` is now a no-op
  (accepted for backward compatibility with existing scripts), and
  `--no-dedup-morph-id-mask` opts back into the old opaque-bytes
  comparison.
- **Fixed a merge-time compile break**: Part B's own `#[cfg(test)]` unit
  tests in `pack_builder.rs` called `run()` with its old 3-argument
  signature (5 call sites) — these predate the `ignore_afmt_id` parameter
  and don't exercise dedup behavior at all (they test part/morph parsing,
  duplication maps, and error handling). Updated all 5 call sites to pass
  `false` explicitly, preserving their original assertions unchanged.

## 3. Verification performed
- `cargo build --release`: clean build, 0 errors (only 4 pre-existing
  dead-code warnings unrelated to this merge).
- `cargo test --release`: **all tests pass, 0 failures** — 106 lib unit
  tests, 13 `morph_converter` unit tests, 5 `pack_builder` unit tests,
  and all integration tests (`cc0_phase10a_afmt_v2_e2e`,
  `cc0_phase10_dedup_equivalence`, `cc0_phase2_rigged_body_e2e`,
  `cc0_phase3_morph_pipeline_e2e`, `cc0_phase5_diversity_regression` ×9,
  `cc0_phase6_clothing_limb_anchoring` ×3, `real_pack_e2e`).
- Spot-checked Part A's headline size claim against its own committed
  fixtures rather than taking the report's numbers on faith: `asym-ear-1-l`
  17,794 B → 10,182 B and `asym-nose-1-l` 8,862 B → 5,078 B, both exactly
  matching the report's 1.7476x / 1.7452x.

## 4. What `CC0_PHASE_8_PACK_OUTPUT.zip` provided

It contained exactly the two missing inputs, plus the original Phase 8
build script and its own notes:
- `morph_id_map.json` — the real id→target path mapping for all 1,280 morphs.
- `manifest.json` — the 6 pack definitions (which categories go in which
  pack) and the original Phase 8 (v1, no dedup) size/morph-count baseline.
- `build_pack_library.py` — the actual Phase 8 build script (category→ID
  block scheme, per-pack category lists, uses only `morph_converter` +
  `pack_builder` from `target/release/`, body parts from
  `tests/fixtures/cc0_phase3_pipeline/`, base mesh from
  `assets/upstream/base.obj` — all of which are present in the base tree).
- `cc0_phase8_pack_library.rs` — the real loader/equivalence test (loads
  a pack through `init_part_registry_from_pack`, applies every morph in
  it individually at weight 1.0 to the real 53,512-vertex body, checks
  finite/bounded displacement). Copied in as
  `tests/cc0_phase10_pack_library.rs` and reused unchanged for Phase 10
  verification, exactly as the spec asked ("same technique as Part B's
  equivalence test").

I checked the category composition against the real fetched corpus before
building anything (see Section 5) rather than assuming the numbers in
`manifest.json` were still accurate.

## 5. Rebuilding all 6 real production packs — DONE

**Corpus:** shallow-fetched `makehumancommunity/makehuman` at the pinned
commit named in the Phase 8 notes (`a8bc2d5`, sparse-checkout of
`makehuman/data/targets/` only). Got exactly 1,280 `.target` files;
per-category counts matched `morph_id_map.json`/`manifest.json` exactly
(e.g. macrodetails 348, armslegs 140, breast 228, ... summing correctly
to each pack's expected morph_count) before I built anything from it.

**Build:** ran `build_pack_library.py` unmodified against the merged
tree's freshly-built `morph_converter` (AFMT v2) and `pack_builder` (v1/v2
dedup fix, dedup on by default, no flag needed since it's now the
default) — 1,280 converter calls + 6 packer calls, using the real body
parts and base mesh already in the tree.

### Real final sizes (Phase 8 v1 baseline → Phase 10 v2+dedup)

| pack | morphs | unique blobs after dedup | Phase 8 (v1) bytes | Phase 10 (v2+dedup) bytes | ratio |
|---|---|---|---|---|---|
| essentials | 348 | 145 | 148,523,601 | 33,654,639 | **4.4132x** (77.3% smaller) |
| body-shape | 806 | 548 | 164,753,473 | 42,571,449 | **3.8700x** (74.2% smaller) |
| face-shape | 332 | 332 | 9,201,681 | 6,709,009 | **1.3715x** (27.1% smaller) |
| expressions | 102 | 67 | 4,424,893 | 3,843,571 | **1.1512x** (13.1% smaller) |
| measurement-fit | 40 | 40 | 7,274,101 | 5,601,945 | **1.2985x** (23.0% smaller) |
| full | 1,280 | 985 | 175,542,297 | 48,486,671 | **3.6204x** (72.4% smaller) |

Note the ratio varies a lot more than Part A's flat ~1.75x quantization
figure: `essentials`/`body-shape`/`full` get large *additional* wins from
dedup (many macrodetail targets — symmetric left/right pairs, etc. —
share identical delta bytes once the embedded id is masked out), while
`face-shape` and `measurement-fit` have **zero** duplicate blobs (332/332,
40/40) and their whole reduction is the v2 quantization alone — which is
why their ratio is lower, not because anything went wrong. `body-shape`
and `full`'s per-morph-bytes ratio also gets diluted by their fixed
~3.15 MB of body-part geometry (the `.glb`s + skeleton), which doesn't
shrink at all — that fixed cost is a larger fraction of the smaller packs.

### Verification performed
- **Corpus composition** checked against `morph_id_map.json` before building (Section 5, above).
- **Determinism:** rebuilt `essentials` a second time independently and diffed sha256 — byte-identical.
- **Zero morphs lost:** diffed the freshly-generated `morph_id_map.json` against Phase 8's — **0 differences across all 1,280 entries** (same id → same category → same target path, every one).
- **Loader/mesh-equivalence check**, run for real on all 6 rebuilt packs (not just 2) via `cc0_phase10_pack_library.rs`, one pack per process:
  - `essentials`: 348/348 morphs loaded, applied individually, all finite & bounded displacements.
  - `body-shape`: 806/806, same.
  - `face-shape`: 332/332, same.
  - `expressions`: 102/102, same.
  - `measurement-fit`: 40/40, same.
  - `full`: 1,280/1,280, same.
  - Zero-displacement set for `body-shape`/`full` (14 morphs: 8 empty macrodetail baselines + 6 genitals) matches Phase 8's own documented caveat #4 exactly — a strong cross-check that nothing shifted under the v2+dedup rebuild.
- **`cargo test --release` on the fully merged tree** (code + new pack-library test file): every test still passes, 0 failures.

### Output
- `packs/*.afpp` — all 6 rebuilt packs.
- `packs/manifest.json` — updated with real `size_bytes`, `unique_blob_count`, and `phase10_vs_phase8_ratio` per pack.
- `packs/morph_id_map.json` — carried through unchanged (verified identical).
- `scripts/build_pack_library.py`, `tests/cc0_phase10_pack_library.rs` — added to the tree so this is reproducible.
