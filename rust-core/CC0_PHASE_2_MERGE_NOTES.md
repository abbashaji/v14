# CC0-Phase 2 Merge Notes — rigged body (head/torso/arms/legs) fold-in

(Named `CC0_PHASE_2_*` deliberately, matching `CC0_PHASE_1_MERGE_NOTES.md`'s
own convention — not to be confused with this repo's own unrelated
`PHASE_2_MERGE_NOTES.md`, which is the pack-loading/`pack_builder`
feature, already done, nothing to do with body diversity.)

## What was merged
The real, real-Blender-rigged 4-part body export
(`cc0-phase2_6-complete-human.zip`'s `head.glb`/`torso.glb`/`arms.glb`/
`legs.glb` + `master_skeleton.json`, later re-exported with UV data — see
below) is now a checked-in test fixture,
`tests/fixtures/cc0_phase2_rigged_body/`, proven to load through this
crate's real, **unmodified** `gltf_loader`/`skeleton_resolver`/
`pack_builder`/`init_part_registry_from_pack` pipeline, via a new
integration test, `tests/cc0_phase2_rigged_body_e2e.rs`.

## Blocker found, then resolved (not silently)
The original export had no `TEXCOORD_0` accessor at all in any of the 4
files. `gltf_loader.rs` hard-requires it and returned a real `Err` for
all 4 — confirmed directly against the compiled loader, not assumed from
reading the glTF spec. Reported back rather than routed around (the two
fixes — re-export with real UVs, vs. loosening the loader to tolerate a
missing UV — are not equivalent, and picking one silently would have been
a real design decision made without the person who owns the asset
pipeline).

The person re-exported the 4 files with real UV data and handed them back
as `9001_head.glb`/`9002_torso.glb`/`9003_arms.glb`/`9004_legs.glb`.
Before touching anything else, re-verified (not assumed) that this
actually fixed it and changed nothing else:

- **Geometry unchanged**: every vertex position in the re-export is
  byte-identical (max diff `0.0`) to the original export, for all 4
  parts. Only UV data was added.
- **UVs now present and sane**: `TEXCOORD_0` present on all 4, every
  value in `[0, 1]`, zero `NaN`.
- **Everything previously verified still holds**: 163-bone joint-name
  completeness against `master_skeleton.json` (no missing, no
  duplicates), weight sums (all 1.0 ± 1e-6), unit-length normals, and the
  same adjacent-part boundary-seam vertex overlap counts as before
  (head↔torso 52, torso↔arms 74, torso↔legs 64; non-adjacent pairs 0–14).
- **Loads via the real compiled loader**: ran `gltf_loader::load_gltf_file`
  against all 4 re-exported files directly (a throwaway `#[cfg(test)]`
  probe, added and then reverted — not part of this delivery) — all 4
  now return `Ok`, matching the original files' vertex/index counts
  exactly (head 17,224/25,836; torso 5,840/8,760; arms 17,280/25,920;
  legs 13,168/19,752).

## Part-id assignment, and a naming collision avoided
The person's re-exported filenames used `9001`–`9004`. Did **not** use
those numeric ids for the actual fixture files in the repo: `9001` and
`9002` already mean something else here — `tests/fixtures/cc0_base_mesh/
9001_head.obj` / `9002_torso.obj` (CC0-Phase 1's *unrigged* base-mesh
fixtures, used for morph-target work, explicitly documented in
`CC0_PHASE_1_MERGE_NOTES.md` as "Part B's placeholder IDs... explicitly
not production IDs"). Two different assets — one an unrigged morph-target
base mesh, one a rigged skinned body — sharing part id `9001` would be a
real collision the moment both ever needed to live in the same asset
directory or pack, not just a cosmetic mismatch.

Assigned the next clean, unused block instead — distinct from `1000`/
`2000` (existing tiny synthetic test fixtures), `3001`/`3002` (CC0-Phase
1's reserved-but-unwired base-mesh ids), and `9000`+ (already spoken for,
twice over, as above):

| Part | Assigned id | Source file |
|---|---|---|
| Head | **4001** | re-exported `9001_head.glb` |
| Torso | **4002** | re-exported `9002_torso.glb` |
| Arms | **4003** | re-exported `9003_arms.glb` |
| Legs | **4004** | re-exported `9004_legs.glb` |

The glTF files themselves are byte-identical to what was handed back —
only the checked-in filenames changed, since `parse_part_id` (`lib.rs`)
reads the id from the filename prefix, not file contents.

## What the new test actually proves
`tests/cc0_phase2_rigged_body_e2e.rs` mirrors `real_pack_e2e.rs`'s
pattern (builds a real `.afpp` via the real `pack_builder` binary against
the fixture dir, then drives the real FFI):

1. `init_part_registry_from_pack` on all 4 parts together succeeds —
   this fails on the *first* bad part (per that function's own doc
   comment), so a pass here is proof all 4 parsed and skeleton-resolved
   cleanly against the shared 163-bone master skeleton, not just that
   *some* of them did.
2. `generate_character(head=4001, torso=4002)` — the real body pair —
   returns non-null with exactly 23,064 vertices / 34,596 indices (head +
   torso, concatenated).
3. `generate_character(head=4003, torso=4004)` — arms/legs fed through
   the same two slots, not a real body shape — returns non-null with
   exactly 30,448 vertices / 45,672 indices (arms + legs). This is
   deliberately not a meaningful body; it exists only because there is no
   other FFI surface to directly query which part ids are loaded in the
   registry, and this is direct proof `4003`/`4004` are genuinely present
   and retrievable, not silently dropped.

Ran the **full** suite after adding this, not just the new test: 86
tests total (was 85 before this merge), 0 failed. Same 3 pre-existing
`dead_code` warnings in `texture_atlas.rs` as every prior phase — this
merge doesn't touch that file.

## What this merge deliberately did NOT do
- Did not extend `CharacterDNA` to add `arms_id`/`legs_id` fields, and
  did not touch `generate_character`'s merge logic to include arms/legs
  in real output. Per the handoff's own phase table, that's "CharacterDNA
  ext" — explicitly CC0-Phase 3 work, blocked on this merge being done
  first. Confirmed by reading `generate_character` directly:
  `CharacterDNA` only has `head_id`/`torso_id` right now, so arms/legs
  parts are loaded and skeleton-resolved, but have no consumer yet — that
  is expected, not a gap in this merge.
- Did not touch `gltf_loader.rs`, `skeleton_resolver.rs`, or
  `pack_builder.rs` at all. The UV blocker was resolved on the asset
  side (re-export), not the code side, so the loader's `TEXCOORD_0`
  requirement is unchanged and still applies to any future part.
- Did not attempt a `wasm32-unknown-unknown` build in this sandbox. Same
  standing limitation `The_Fresh-Context_Breakdown_V2.md` already
  documents for this project (`rustup`'s target-add needs
  `static.rust-lang.org`, blocked by this sandbox's network policy) —
  not a new finding, and this merge only added test-only files plus one
  new integration test, nothing in `src/`, so it carries no new wasm32
  risk beyond what CC0-Phase 1/this repo's own Phase 2 already verified
  end-to-end on a real machine.
- Did not do an exhaustive seam-loop correspondence proof (exact 1:1
  vertex-to-vertex matching around each boundary loop) — only a
  boundary-position-overlap count, both before and after the UV
  re-export. That's corroborating evidence the seams are real, not a
  full proof they're geometrically watertight.

## Immediate next step
CC0-Phase 3 (engine: morph blend, `CharacterDNA` ext, AFPP v2) is now
unblocked. Per the handoff's own note, resolve the vertex-duplication
design issue (render-vertex → base-mesh-vertex map) as part of that
phase's design, before writing its task specs — nothing in this merge
changes that finding or its fix.
