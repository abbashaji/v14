# Phase 1 Merge Notes — CC0 asset acquisition/conversion

## Most important fact, up front: this is the real upstream CC0 asset, not a placeholder

Part B's fixture files (`9001_head.obj`, `9002_torso.obj`) are genuinely
derived from MakeHuman's real CC0 base mesh, not synthetic stand-ins.
This was independently re-verified, not taken on the write-up's word
(see "Independent verification" below): a fresh sparse clone of
`makehuman/data/3dobjs/base.obj` at the exact commit the fixture's
header cites (`a8bc2d54ff0ac92e78ff71431b1023eda42bf482`) shows **every
single vertex** in both delivered `.obj` files — 4,284 in the head,
1,320 in the torso, zero unmatched — is an exact-coordinate match to a
vertex in that real upstream mesh, and the two vertex sets are disjoint
(consistent with an exclusive per-bone split, not overlapping/duplicated
data). The file's CC0 licensing claim was also checked directly against
that commit's `LICENSE.ASSETS.md`.

Part A's morph-target converter's format-verification claim was
similarly re-verified independently against the real upstream
`.target` corpus (see below) and found to be accurate in every specific
number it cited.

## Independent verification (both parts' write-ups were checked against actual files, not trusted)

### Part A — `morph_converter.rs` / upstream `.target` format
The write-up (in the file's own header comment) claims the parser was
verified against MakeHuman's real `.target` corpus: 1,280 files, 6.1M+
data lines, zero anomalies, 8 header-only files, max vertex index
19,157, no negative/duplicate indices, and a specific file
(`african-female-baby.target`) at 19,168 lines / 19,150 deltas.

This claim was independently re-run against a fresh clone of
`github.com/makehumancommunity/makehuman` (current `master`), applying
the same `vertex_index dx dy dz` / `#`-comment / blank-line parsing
rules described in the file. Every number matched exactly:

| Claim | Independently measured |
|---|---|
| 1,280 `.target` files | 1,280 |
| 6.1M+ non-comment/blank data lines | 6,147,800 |
| 0 anomalous lines | 0 |
| 8 header-only (zero-delta) files | 8 |
| max vertex index 19,157 | 19,157 |
| 0 negative / duplicate indices | 0 |
| `african-female-baby.target`: 19,168 lines / 19,150 deltas | 19,168 / 19,150 |

**Conclusion: Part A's write-up claims real-format verification, and
that claim checks out.** It is not working from an assumed format.

### Part B — base mesh provenance
The write-up (in `cc0_base_mesh_e2e.rs`'s header comment) claims the
fixture meshes are the real upstream CC0 base mesh, split by real
skinning-weight data, not hand-authored placeholders.

Independently re-verified by fetching the exact cited commit
(`a8bc2d54ff0ac92e78ff71431b1023eda42bf482`) of
`makehuman/data/3dobjs/base.obj` (19,158 vertices, CC0-licensed per
that commit's `LICENSE.ASSETS.md`) and comparing vertex coordinates
against the delivered fixtures:

| Fixture | Vertices | Matched in real base.obj | Unmatched |
|---|---|---|---|
| `9001_head.obj` | 4,284 | 4,284 | 0 |
| `9002_torso.obj` | 1,320 | 1,320 | 0 |

Head/torso vertex sets are disjoint (0 overlap), consistent with the
described exclusive-highest-weighted-bone split. The `.obj` face
indices were also confirmed in-range by the crate's own
`cc0_base_mesh_e2e.rs` tests (see test results below).

**Conclusion: Part B's write-up claims this is the real upstream asset,
reprocessed, not a synthetic placeholder — and that claim checks out.**

### `lib.rs` addendum-authorized change — verified exact
Per the task spec, Part B's original spec forbade touching `lib.rs`,
overridden by a signed addendum authorizing exactly one `pub use`
addition. Diffing Part B's delivered `lib.rs` against the base
project's:

```
84a85
> pub use obj_loader::{load_obj_bytes, LoadedObjMesh, ObjLoadError};
```

This is the **only** change, placed in the existing `pub use` block a
few lines below the `mod obj_loader;` declaration it corresponds to.
No `gltf_loader` equivalent was added, which is correct — Part B's test
only exercises `load_obj_bytes`, so a `gltf_loader` re-export wasn't
needed. No scope violation.

**Caveat:** the addendum document (`PHASE_1_PART_B_ADDENDUM.md`) itself
was not included among the files handed to this merge task — only the
task spec's description of what it authorized. The diff matches that
description exactly, but its existence/signing could not be verified
directly since the document wasn't provided. Worth tracking down and
attaching to the project record if it isn't already filed somewhere.

## Merge: clean union confirmed, not assumed
Part A and Part B touch disjoint files:
- Part A adds `src/bin/morph_converter.rs` (new file).
- Part B adds `tests/cc0_base_mesh_e2e.rs`,
  `tests/fixtures/cc0_base_mesh/{9001_head.obj,9002_torso.obj}` (new
  files) and makes the single-line `lib.rs` change above.

No file is touched by both parts. Applying both onto the base project
tree was a literal union with zero conflicts — confirmed by diffing
each delivered file against the base tree and against each other's
file lists, not assumed from the task descriptions.

## Final assigned production part IDs
Existing numeric part IDs in use anywhere in the project (grepped
across `src/lib.rs`, `benches/`, and every `tests/` fixture directory,
including doc-comment examples in `lib.rs`):

- `1001` (head, OBJ, `tests/fixtures/real_pack_e2e/1001_head.obj`)
- `1002` (legs, OBJ, `tests/fixtures/real_pack_e2e/1002_legs.obj`)
- `2001` (arms, glTF, `tests/fixtures/real_pack_e2e/2001_arms.glb`)
- `2002` (torso, glTF, `tests/fixtures/real_pack_e2e/2002_torso.glb`)
- `9001` / `9002` — Part B's placeholder IDs (`9000`+ range, explicitly
  not production IDs per its own header comment)

No existing convention ties a numeric range to a body-part type (`1001`
is a head, `2002` is a torso — the prefixes are just catalog IDs).

**Assigned final production IDs for this phase's new parts:**

| Part | Assigned ID |
|---|---|
| Head (from `9001_head.obj`) | **3001** |
| Torso (from `9002_torso.obj`) | **3002** |

`3001`/`3002` were chosen as the next clean, unused block — distinct
from both the existing `1000`/`2000` ranges and Part B's `9000`+
placeholder range, so there's no ambiguity later about which IDs are
"real."

**These IDs are recorded here only.** Per the task spec, this merge
does **not** wire them into `init_part_registry`, the registry init
code, `generate_character`, or `CharacterDNA` — the fixture filenames
remain `9001_head.obj` / `9002_torso.obj` as delivered by Part B.
Actually assigning `3001`/`3002` to real pack files/registry entries is
Phase 3 work.

## Full test suite result

Toolchain: `rustc`/`cargo` 1.75.0 (installed fresh into this
environment via `apt`, since none was preinstalled).

```
cargo test --release
```

All suites passed, **85 tests total, 0 failed**:

| Test binary | Result |
|---|---|
| `src/lib.rs` unit tests | 73 passed |
| `src/bin/morph_converter.rs` unit tests (Part A's own) | 6 passed |
| `src/bin/pack_builder.rs` unit tests | 3 passed |
| `tests/cc0_base_mesh_e2e.rs` (Part B's own) | 2 passed |
| `tests/real_pack_e2e.rs` | 1 passed |
| Doc-tests | 0 (none defined) |

Part A's and Part B's own tests were confirmed passing **in the merged
tree** (all six of `morph_converter.rs`'s tests, both of
`cc0_base_mesh_e2e.rs`'s tests), not just re-run in isolation from
their original delivery zips.

Build produced 3 pre-existing warnings, all `dead_code` in
`texture_atlas.rs` (`uv_bounds`, `blit_to_quadrant`,
`remap_uvs_for_quadrant` never used) — unrelated to this merge; neither
part touches `texture_atlas.rs`.

## Known, expected characteristics of the fixture geometry (not defects)
- The head/torso split leaves an open (unstitched) neck/shoulder
  boundary, since a face is only kept if every one of its vertices
  landed in that part's bone group. This is called out in Part B's own
  comments and is expected for a modular-part fixture at this stage.
- Shoulders, arms, hands, fingers, legs, feet, and toes are excluded
  from both fixtures (correctly — those aren't head or torso).
- Morph-target `normal_delta` is always `[0, 0, 0]` in this format;
  upstream `.target` files carry no normal-delta data, and computing
  one is out of scope for this task (flagged as a known gap for later,
  not an oversight, per Part A's own note).

## What this merge deliberately did NOT do
Per the task spec's "What NOT to do": the new parts are not wired into
`generate_character` or `CharacterDNA`, and no part output was "fixed
up" to look more complete than delivered. The base mesh is reported
here plainly as real upstream data (verified), and the assigned
`3001`/`3002` IDs are recorded but not yet applied anywhere in code.
