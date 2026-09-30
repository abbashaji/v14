//! CC0-Phase 3 merge: end-to-end native test proving the real morph
//! pipeline -- a real AFPP v2 pack built (via the real `pack_builder`
//! binary) with a real base mesh argument (so real duplication maps get
//! computed for every part), loaded via `init_part_registry_from_pack`,
//! then `generate_character` called with a real active morph
//! (`5001_asym_ear_1_l.afmt`) on the real, full 4-part rigged body.
//!
//! Same real-binary pattern as `real_pack_e2e.rs`/
//! `cc0_phase2_rigged_body_e2e.rs`: this test builds its own pack fresh
//! every run, self-contained and portable. The fixture asset directory
//! at `tests/fixtures/cc0_phase3_pipeline/` is a fresh combined
//! directory (the same 4 renamed GLBs and `master_skeleton.json` as
//! `cc0_phase2_rigged_body`'s fixture set, plus both real `.afmt` morph
//! files from `cc0_phase3_real_morphs`) -- the existing Phase 2 fixture
//! directory itself is left untouched.

use anthroforge_core::{generate_character, init_part_registry_from_pack, CharacterDNA};
use std::path::PathBuf;
use std::process::Command;

const HEAD_ID: u32 = 4001;
const TORSO_ID: u32 = 4002;
const ARMS_ID: u32 = 4003;
const LEGS_ID: u32 = 4004;
const EAR_MORPH_ID: u16 = 5001;

// Same full-body total `cc0_phase2_rigged_body_e2e.rs` asserts: head
// 17,224 + torso 5,840 + arms 17,280 + legs 13,168 = 53,512 vertices;
// head 25,836 + torso 8,760 + arms 25,920 + legs 19,752 = 80,268 indices.
const EXPECTED_VERTS: u32 = 53_512;
const EXPECTED_INDICES: u32 = 80_268;

fn build_real_pack_bytes() -> Vec<u8> {
    let fixture_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cc0_phase3_pipeline");
    assert!(
        fixture_dir.is_dir(),
        "expected fixture asset dir at '{}' (checked into the repo) -- did the checkout lose it?",
        fixture_dir.display()
    );

    let base_mesh_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/upstream/base.obj");
    assert!(
        base_mesh_path.is_file(),
        "expected base mesh at '{}' (checked into the repo)",
        base_mesh_path.display()
    );

    let out_dir = std::env::var("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    std::fs::create_dir_all(&out_dir)
        .unwrap_or_else(|e| panic!("failed to create pack output dir '{}': {e}", out_dir.display()));
    let out_path = out_dir.join("cc0_phase3_morph_pipeline_generated.afpp");

    let pack_builder_bin = env!("CARGO_BIN_EXE_pack_builder");
    let status = Command::new(pack_builder_bin)
        .arg(&fixture_dir)
        .arg(&out_path)
        .arg(&base_mesh_path)
        .status()
        .unwrap_or_else(|e| panic!("failed to run pack_builder at '{pack_builder_bin}': {e}"));
    assert!(
        status.success(),
        "pack_builder exited with failure status building the morph-pipeline pack from '{}' \
         with base mesh '{}'",
        fixture_dir.display(),
        base_mesh_path.display()
    );

    std::fs::read(&out_path).unwrap_or_else(|e| {
        panic!(
            "failed to read freshly-built morph-pipeline pack at '{}': {e}",
            out_path.display()
        )
    })
}

#[test]
fn cc0_phase3_morph_pipeline_applies_real_morph_to_real_body() {
    let pack_bytes = build_real_pack_bytes();

    let ok = init_part_registry_from_pack(pack_bytes.as_ptr(), pack_bytes.len());
    assert!(
        ok,
        "init_part_registry_from_pack must succeed loading the real 4-part rigged body plus \
         2 real morphs, with real duplication maps computed against the real base mesh"
    );

    // Baseline: no active morphs.
    let baseline_dna = CharacterDNA {
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
    let baseline_ptr = generate_character(&baseline_dna as *const CharacterDNA);
    assert!(
        !baseline_ptr.is_null(),
        "generate_character must return non-null output for the baseline (no active morphs) call"
    );

    // Morphed: the real ear morph active at full weight.
    let morph_ids: [u16; 1] = [EAR_MORPH_ID];
    let morph_weights: [f32; 1] = [1.0];
    let morphed_dna = CharacterDNA {
        seed: 42,
        height_modifier: 1.0,
        weight_modifier: 1.0,
        head_id: HEAD_ID,
        torso_id: TORSO_ID,
        arms_id: ARMS_ID,
        legs_id: LEGS_ID,
        equipped_clothing_ids_ptr: std::ptr::null(),
        equipped_clothing_count: 0,
        active_morph_ids_ptr: morph_ids.as_ptr(),
        active_morph_weights_ptr: morph_weights.as_ptr(),
        active_morph_count: 1,
    };
    let morphed_ptr = generate_character(&morphed_dna as *const CharacterDNA);
    assert!(
        !morphed_ptr.is_null(),
        "generate_character must return non-null output for the morphed (active_morph_count = 1) call"
    );

    // SAFETY: both pointers are non-null `MeshOutputBuffer`s just
    // returned by `generate_character`, valid until freed below.
    let baseline = unsafe { &*baseline_ptr };
    let morphed = unsafe { &*morphed_ptr };

    // Morph application must not change mesh topology, only vertex
    // positions -- both calls must report the same full-body counts.
    assert_eq!(
        baseline.vertices_count, EXPECTED_VERTS,
        "baseline vertices_count must be exactly {EXPECTED_VERTS} (the real full-body total) -- \
         got {}",
        baseline.vertices_count
    );
    assert_eq!(
        baseline.indices_count, EXPECTED_INDICES,
        "baseline indices_count must be exactly {EXPECTED_INDICES} -- got {}",
        baseline.indices_count
    );
    assert_eq!(
        morphed.vertices_count, EXPECTED_VERTS,
        "morphed vertices_count must equal the baseline's {EXPECTED_VERTS} -- morph application \
         must not change mesh topology; got {}",
        morphed.vertices_count
    );
    assert_eq!(
        morphed.indices_count, EXPECTED_INDICES,
        "morphed indices_count must equal the baseline's {EXPECTED_INDICES}; got {}",
        morphed.indices_count
    );

    // The real proof: at least one vertex position genuinely differs
    // between the baseline and morphed outputs (a real displacement
    // happened, not a no-op), with a generous sanity bound on the
    // magnitude to catch a catastrophic unit/scale bug without
    // over-constraining the real morph math.
    //
    // SAFETY: both buffers report `vertices_count == EXPECTED_VERTS` and
    // `generate_character`'s own safety contract guarantees
    // `vertices_ptr` is valid for reads of `vertices_count` consecutive
    // `SkinnedVertex` values.
    let baseline_vertices = unsafe {
        std::slice::from_raw_parts(baseline.vertices_ptr, baseline.vertices_count as usize)
    };
    let morphed_vertices = unsafe {
        std::slice::from_raw_parts(morphed.vertices_ptr, morphed.vertices_count as usize)
    };

    let mut max_displacement = 0.0f32;
    let mut any_displacement = false;
    for (b, m) in baseline_vertices.iter().zip(morphed_vertices.iter()) {
        let dx = m.position[0] - b.position[0];
        let dy = m.position[1] - b.position[1];
        let dz = m.position[2] - b.position[2];
        let displacement = (dx * dx + dy * dy + dz * dz).sqrt();
        if displacement > 0.0 {
            any_displacement = true;
        }
        if displacement > max_displacement {
            max_displacement = displacement;
        }
    }

    assert!(
        any_displacement,
        "expected at least one vertex position to differ between the baseline and morphed \
         outputs -- got a perfect no-op, meaning the real morph was never actually applied"
    );
    assert!(
        max_displacement < 1.0,
        "expected max per-vertex displacement to be under 1.0 world units (a generous sanity \
         bound) -- got {max_displacement}, which suggests a unit/scale bug rather than a \
         genuine small ear-shape morph"
    );

    anthroforge_core::free_mesh_buffer(baseline_ptr);
    anthroforge_core::free_mesh_buffer(morphed_ptr);
}
