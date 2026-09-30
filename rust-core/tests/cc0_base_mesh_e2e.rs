//! Phase 1 fixture-wiring check for the (future) CC0 base mesh head/torso
//! parts — see `PHASE_1_TASK_SPEC_PART_B.md`.
//!
//! Modeled on the structure of `tests/real_pack_e2e.rs`, but deliberately
//! narrower in scope: this only exercises `obj_loader::load_obj_bytes`
//! directly against the two new fixture files below. It does **not**
//! call `init_part_registry_from_pack` / `generate_character`, and it
//! does not touch the global part registry at all — wiring these parts
//! into the registry with real production part IDs is explicitly out of
//! scope here and belongs to the Phase 1 merge context instead, which has
//! visibility into every existing part ID in the project to avoid
//! collisions.
//!
//! IMPORTANT — this IS the real upstream CC0 asset, reprocessed, not a
//! synthetic placeholder:
//! `tests/fixtures/cc0_base_mesh/9001_head.obj` and `.../9002_torso.obj`
//! are derived from MakeHuman's real CC0 base mesh
//! (`makehuman/data/3dobjs/base.obj` in
//! https://github.com/makehumancommunity/makehuman, commit
//! `a8bc2d54ff0ac92e78ff71431b1023eda42bf482`, CC0-licensed per that
//! repo's `LICENSE.ASSETS.md`), not a hand-authored stand-in.
//!
//! That upstream file is a single, unified full-body mesh with no
//! built-in head/torso split (its own `.obj` groups distinguish only
//! `body` vs. helper/joint-marker geometry, not body regions). To split
//! it, every vertex was assigned to whichever body region owns its
//! single highest-weighted bone in MakeHuman's own real rigging data
//! (`makehuman/data/rigs/default_weights.mhw`, also CC0): `head`/`jaw`/
//! `neck0{1,2,3}`/eye/tongue/facial-expression bones -> head;
//! `root`/`pelvis`/`spine0{1..5}`/`clavicle`/`breast` bones -> torso;
//! everything else (shoulder, arms, hands, fingers, legs, feet, toes) is
//! excluded from both. A face is kept for a part only if *every* one of
//! its vertices was assigned to that part, so the neck/shoulder seam is
//! an open boundary in both fixture files rather than stitched closed —
//! expected for a modular-part fixture, not a defect. The extraction
//! script is not checked in; the derivation is fully described here and
//! is reproducible from the two upstream files named above.
//!
//! The `9001`/`9002` numeric prefixes are placeholder IDs in the
//! `9000`+ range only (following the same numeric-prefix filename
//! convention as `tests/fixtures/real_pack_e2e/`), and are not real
//! production part IDs; final ID assignment happens at merge time.

use anthroforge_core::load_obj_bytes;

const HEAD_FIXTURE_BYTES: &[u8] =
    include_bytes!("fixtures/cc0_base_mesh/9001_head.obj");
const TORSO_FIXTURE_BYTES: &[u8] =
    include_bytes!("fixtures/cc0_base_mesh/9002_torso.obj");

/// Every index in `indices` must reference a vertex that actually exists
/// in `vertex_count`-many vertices -- i.e. no index is `>= vertex_count`.
fn assert_indices_in_range(vertex_count: usize, indices: &[u32]) {
    for (i, &index) in indices.iter().enumerate() {
        assert!(
            (index as usize) < vertex_count,
            "index {index} at position {i} is out of range for {vertex_count} vertices"
        );
    }
}

#[test]
fn cc0_head_fixture_loads_with_consistent_geometry() {
    let mesh = load_obj_bytes(HEAD_FIXTURE_BYTES)
        .expect("head fixture (derived from the real MakeHuman CC0 base mesh) must parse via load_obj_bytes");

    assert!(!mesh.vertices.is_empty(), "head mesh must have at least one vertex");
    assert!(!mesh.indices.is_empty(), "head mesh must have at least one index");
    assert_eq!(
        mesh.indices.len() % 3,
        0,
        "head mesh index count must be a whole number of triangles"
    );
    assert_indices_in_range(mesh.vertices.len(), &mesh.indices);
}

#[test]
fn cc0_torso_fixture_loads_with_consistent_geometry() {
    let mesh = load_obj_bytes(TORSO_FIXTURE_BYTES)
        .expect("torso fixture (derived from the real MakeHuman CC0 base mesh) must parse via load_obj_bytes");

    assert!(!mesh.vertices.is_empty(), "torso mesh must have at least one vertex");
    assert!(!mesh.indices.is_empty(), "torso mesh must have at least one index");
    assert_eq!(
        mesh.indices.len() % 3,
        0,
        "torso mesh index count must be a whole number of triangles"
    );
    assert_indices_in_range(mesh.vertices.len(), &mesh.indices);
}
