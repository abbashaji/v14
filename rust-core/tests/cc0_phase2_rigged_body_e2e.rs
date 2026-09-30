//! CC0-Phase 2 merge: end-to-end native test against the REAL, real-Blender
//! rigged body export (4 parts -- head/torso/arms/legs -- real skinning
//! weights, real 163-bone skeleton), not a synthetic fixture. Mirrors
//! `real_pack_e2e.rs`'s pattern: builds a real `.afpp` pack via the real
//! `pack_builder` binary from the fixture asset directory checked into
//! this repo at `tests/fixtures/cc0_phase2_rigged_body/`, then drives the
//! real FFI entry points against it.
//!
//! Fixture provenance: originally exported from a real Blender scene
//! (163-bone armature, MCP-verified live) as `head.glb`/`torso.glb`/
//! `arms.glb`/`legs.glb`. That first export had no `TEXCOORD_0` (UV)
//! accessor at all and could not be loaded by this crate's
//! `gltf_loader` -- confirmed by directly running the loader against it,
//! not assumed. Re-exported with real UV data; geometry (vertex
//! positions, indices, skin weights, normals) is byte-for-byte identical
//! to the original export -- only UVs were added. Assigned part ids
//! 4001 (head) / 4002 (torso) / 4003 (arms) / 4004 (legs): the next
//! clean, unused block, distinct from the existing `1000`/`2000` test
//! ranges, the `3001`/`3002` block CC0-Phase 1 reserved for the CC0 base
//! mesh (unrigged, used for morph blending), and the `9000`+ range
//! already used both by CC0-Phase 1's own placeholder fixtures
//! (`tests/fixtures/cc0_base_mesh/9001_head.obj`,
//! `9002_torso.obj`) and by this fixture's own as-delivered filenames
//! (`9001_head.glb`.. `9004_legs.glb`) -- reusing `9001`/`9002` here
//! would have collided with an already-established, differently-typed
//! asset under the same numeric id.
//!
//! `CharacterDNA` gained `arms_id`/`legs_id` fields in CC0-Phase 3
//! ("CharacterDNA ext"), so this test now exercises `generate_character`
//! with all 4 real rigged-body parts as a single, genuine merged body in
//! one call -- head, torso, arms, and legs together, not two separate
//! head/torso-slot calls standing in for presence checks.

use anthroforge_core::{generate_character, init_part_registry_from_pack, CharacterDNA};
use std::path::PathBuf;
use std::process::Command;

const HEAD_ID: u32 = 4001;
const TORSO_ID: u32 = 4002;
const ARMS_ID: u32 = 4003;
const LEGS_ID: u32 = 4004;

// From direct inspection of the fixture files (see this crate's own
// `gltf_loader` reading them): head 17,224 verts / 25,836 indices, torso
// 5,840 verts / 8,760 indices, arms 17,280 verts / 25,920 indices, legs
// 13,168 verts / 19,752 indices.
const HEAD_VERTS: u32 = 17_224;
const HEAD_INDICES: u32 = 25_836;
const TORSO_VERTS: u32 = 5_840;
const TORSO_INDICES: u32 = 8_760;
const ARMS_VERTS: u32 = 17_280;
const ARMS_INDICES: u32 = 25_920;
const LEGS_VERTS: u32 = 13_168;
const LEGS_INDICES: u32 = 19_752;

fn build_real_pack_bytes() -> Vec<u8> {
    let fixture_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cc0_phase2_rigged_body");
    assert!(
        fixture_dir.is_dir(),
        "expected fixture asset dir at '{}' (checked into the repo) -- did the checkout lose it?",
        fixture_dir.display()
    );

    let out_dir = std::env::var("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    std::fs::create_dir_all(&out_dir)
        .unwrap_or_else(|e| panic!("failed to create pack output dir '{}': {e}", out_dir.display()));
    let out_path = out_dir.join("cc0_phase2_rigged_body_generated.afpp");

    let pack_builder_bin = env!("CARGO_BIN_EXE_pack_builder");
    let status = Command::new(pack_builder_bin)
        .arg(&fixture_dir)
        .arg(&out_path)
        .status()
        .unwrap_or_else(|e| panic!("failed to run pack_builder at '{pack_builder_bin}': {e}"));
    assert!(
        status.success(),
        "pack_builder exited with failure status building the real rigged-body pack from '{}'",
        fixture_dir.display()
    );

    std::fs::read(&out_path).unwrap_or_else(|e| {
        panic!(
            "failed to read freshly-built rigged-body pack at '{}': {e}",
            out_path.display()
        )
    })
}

#[test]
fn cc0_phase2_rigged_body_pack_loads_and_generates_character() {
    let pack_bytes = build_real_pack_bytes();

    let ok = init_part_registry_from_pack(pack_bytes.as_ptr(), pack_bytes.len());
    assert!(
        ok,
        "init_part_registry_from_pack must succeed loading all 4 real rigged-body parts \
         (head/torso/arms/legs) -- a failure here means at least one part failed to parse \
         and/or failed skeleton contribution/resolution against the shared 163-bone skeleton"
    );

    // The real, full production-shaped body: all 4 real parts merged in
    // one `generate_character` call.
    let dna = CharacterDNA {
        seed: 42,
        height_modifier: 1.0,
        weight_modifier: 1.0,
        head_id: HEAD_ID,
        torso_id: TORSO_ID,
        arms_id: ARMS_ID,
        legs_id: LEGS_ID,
        equipped_clothing_ids_ptr: std::ptr::null(),
        equipped_clothing_count: 0,
        active_morph_ids_ptr: std::ptr::null(),
        active_morph_weights_ptr: std::ptr::null(),
        active_morph_count: 0,
    };
    let output_ptr = generate_character(&dna as *const CharacterDNA);
    assert!(
        !output_ptr.is_null(),
        "generate_character must return non-null output for the real (head={HEAD_ID}, \
         torso={TORSO_ID}, arms={ARMS_ID}, legs={LEGS_ID}) rigged body"
    );
    let output = unsafe { &*output_ptr };
    assert_eq!(
        output.vertices_count,
        HEAD_VERTS + TORSO_VERTS + ARMS_VERTS + LEGS_VERTS,
        "expected exactly {} vertices (head + torso + arms + legs) -- got {}",
        HEAD_VERTS + TORSO_VERTS + ARMS_VERTS + LEGS_VERTS,
        output.vertices_count
    );
    assert_eq!(
        output.indices_count,
        HEAD_INDICES + TORSO_INDICES + ARMS_INDICES + LEGS_INDICES,
        "expected exactly {} indices (head + torso + arms + legs) -- got {}",
        HEAD_INDICES + TORSO_INDICES + ARMS_INDICES + LEGS_INDICES,
        output.indices_count
    );
    anthroforge_core::free_mesh_buffer(output_ptr);
}
