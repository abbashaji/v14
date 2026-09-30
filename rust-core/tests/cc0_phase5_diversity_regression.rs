//! CC0-Phase 5: diversity regression test suite.
//!
//! Locks in the two *real* body-diversity mechanisms this crate has today
//! (and nothing else -- `CharacterDNA.seed` is a dead field and is
//! deliberately never read or varied here):
//!
//! 1. `body_mutation::mutate_skin_vertices`, driven by
//!    `[weight_modifier, height_modifier, weight_modifier]`.
//! 2. `morph_blend::apply_morph_targets`, driven by
//!    `active_morph_ids_ptr` / `active_morph_weights_ptr`.
//!
//! Same real-pipeline pattern as `real_pack_e2e.rs` /
//! `cc0_phase2_rigged_body_e2e.rs` / `cc0_phase3_morph_pipeline_e2e.rs`:
//! the `.afpp` is built fresh through the real `pack_builder` binary
//! (`CARGO_BIN_EXE_pack_builder`) with the real base mesh, never
//! hand-written. This file is self-contained -- it does not import from
//! those files.
//!
//! ## One pack per *process*, not per test (deviation from the spec)
//!
//! `GLOBAL_REGISTRY` is a process-wide `OnceLock`; a second
//! `init_part_registry_from_pack` in the same process returns `false`.
//! All tests in one integration-test file share one process and run on
//! parallel threads, so "each test builds and inits its own pack" is not
//! possible. `ensure_registry()` below builds the pack and initializes
//! the registry exactly once (`std::sync::Once`); every test calls it and
//! then only varies the `CharacterDNA` it passes to `generate_character`.
//!
//! Because the registry is shared, every test -- including the scale
//! tests the spec says should use `cc0_phase2_rigged_body/` -- runs
//! against `tests/fixtures/cc0_phase3_pipeline/`. Its four body GLBs and
//! `master_skeleton.json` are byte-identical to the phase 2 fixture's
//! (verified with `cmp`); the only difference is the two extra `.afmt`
//! morph files, which have no effect unless a test activates them.

use anthroforge_core::{
    anthroforge_last_error, free_mesh_buffer, generate_character, init_part_registry_from_pack,
    CharacterDNA,
};
use std::ffi::CStr;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Once;

const HEAD_ID: u32 = 4001;
const TORSO_ID: u32 = 4002;
const ARMS_ID: u32 = 4003;
const LEGS_ID: u32 = 4004;
const EAR_MORPH_ID: u16 = 5001;
const NOSE_MORPH_ID: u16 = 5002;
const UNKNOWN_MORPH_ID: u16 = 9999;

// Full-body totals asserted by the existing e2e tests: head 17,224 +
// torso 5,840 + arms 17,280 + legs 13,168 vertices; head 25,836 + torso
// 8,760 + arms 25,920 + legs 19,752 indices.
const EXPECTED_VERTS: u32 = 53_512;
const EXPECTED_INDICES: u32 = 80_268;

/// Builds the real pack via the real `pack_builder` binary and returns its bytes.
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
    let out_path = out_dir.join("cc0_phase5_diversity_regression_generated.afpp");

    let pack_builder_bin = env!("CARGO_BIN_EXE_pack_builder");
    let status = Command::new(pack_builder_bin)
        .arg(&fixture_dir)
        .arg(&out_path)
        .arg(&base_mesh_path)
        .status()
        .unwrap_or_else(|e| panic!("failed to run pack_builder at '{pack_builder_bin}': {e}"));
    assert!(
        status.success(),
        "pack_builder exited with failure status building the diversity-regression pack from \
         '{}' with base mesh '{}'",
        fixture_dir.display(),
        base_mesh_path.display()
    );

    std::fs::read(&out_path).unwrap_or_else(|e| {
        panic!(
            "failed to read freshly-built diversity-regression pack at '{}': {e}",
            out_path.display()
        )
    })
}

/// Builds the real pack and initializes the global registry exactly once
/// per process (see the module doc comment for why). Safe to call from
/// every test; concurrent callers block until the first one finishes.
fn ensure_registry() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let pack_bytes = build_real_pack_bytes();
        let ok = init_part_registry_from_pack(pack_bytes.as_ptr(), pack_bytes.len());
        assert!(
            ok,
            "init_part_registry_from_pack must succeed loading the real 4-part rigged body plus \
             2 real morphs"
        );
    });
}

/// Owned copy of the parts of a `generate_character` result these tests
/// care about, so the C-style buffer can be freed immediately.
struct Generated {
    positions: Vec<[f32; 3]>,
    vertices_count: u32,
    indices_count: u32,
}

/// Runs `generate_character` on the real 4-part body with the given
/// modifiers and active morphs (no clothing). Returns `None` when
/// `generate_character` returns null. Never touches `seed` beyond the
/// required constant field initializer (it is a dead field).
fn generate(height: f32, weight: f32, morphs: &[(u16, f32)]) -> Option<Generated> {
    ensure_registry();

    let ids: Vec<u16> = morphs.iter().map(|m| m.0).collect();
    let weights: Vec<f32> = morphs.iter().map(|m| m.1).collect();
    let dna = CharacterDNA {
        seed: 42,
        height_modifier: height,
        weight_modifier: weight,
        head_id: HEAD_ID,
        torso_id: TORSO_ID,
        arms_id: ARMS_ID,
        legs_id: LEGS_ID,
        equipped_clothing_ids_ptr: std::ptr::null(),
        equipped_clothing_count: 0,
        active_morph_ids_ptr: if morphs.is_empty() { std::ptr::null() } else { ids.as_ptr() },
        active_morph_weights_ptr: if morphs.is_empty() {
            std::ptr::null()
        } else {
            weights.as_ptr()
        },
        active_morph_count: morphs.len() as u32,
    };

    let ptr = generate_character(&dna as *const CharacterDNA);
    if ptr.is_null() {
        return None;
    }

    // SAFETY: `ptr` is a non-null `MeshOutputBuffer` just returned by
    // `generate_character`; its `vertices_ptr` is valid for reads of
    // `vertices_count` consecutive `SkinnedVertex` values until freed.
    let buffer = unsafe { &*ptr };
    let vertices =
        unsafe { std::slice::from_raw_parts(buffer.vertices_ptr, buffer.vertices_count as usize) };
    let result = Generated {
        positions: vertices.iter().map(|v| v.position).collect(),
        vertices_count: buffer.vertices_count,
        indices_count: buffer.indices_count,
    };
    free_mesh_buffer(ptr);
    Some(result)
}

/// Like `generate`, but panics with a clear message on a null return.
fn generate_ok(height: f32, weight: f32, morphs: &[(u16, f32)]) -> Generated {
    generate(height, weight, morphs).unwrap_or_else(|| {
        panic!(
            "generate_character returned null for height={height}, weight={weight}, \
             morphs={morphs:?}"
        )
    })
}

/// Axis-aligned bounding-box extent `[x, y, z]` (max - min per axis).
fn extents(positions: &[[f32; 3]]) -> [f32; 3] {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in positions {
        for axis in 0..3 {
            min[axis] = min[axis].min(p[axis]);
            max[axis] = max[axis].max(p[axis]);
        }
    }
    [max[0] - min[0], max[1] - min[1], max[2] - min[2]]
}

/// Relative difference `|a - b| / max(|a|, |b|)`.
fn rel_diff(a: f32, b: f32) -> f32 {
    let denom = a.abs().max(b.abs());
    if denom == 0.0 {
        0.0
    } else {
        (a - b).abs() / denom
    }
}

/// Euclidean per-vertex displacement `b[i] - a[i]`.
fn displacements(a: &Generated, b: &Generated) -> Vec<f32> {
    assert_eq!(a.positions.len(), b.positions.len(), "vertex counts must match to diff");
    a.positions
        .iter()
        .zip(b.positions.iter())
        .map(|(p, q)| {
            let (dx, dy, dz) = (q[0] - p[0], q[1] - p[1], q[2] - p[2]);
            (dx * dx + dy * dy + dz * dz).sqrt()
        })
        .collect()
}

fn max_displacement(a: &Generated, b: &Generated) -> f32 {
    displacements(a, b).into_iter().fold(0.0, f32::max)
}

fn any_vertex_differs(a: &Generated, b: &Generated) -> bool {
    assert_eq!(a.positions.len(), b.positions.len(), "vertex counts must match to diff");
    a.positions.iter().zip(b.positions.iter()).any(|(p, q)| p != q)
}

fn last_error_string() -> Option<String> {
    let p = anthroforge_last_error();
    if p.is_null() {
        None
    } else {
        // SAFETY: borrowed, NUL-terminated pointer valid until the next
        // library call on this thread; copied out immediately.
        Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }
}

// ---------------------------------------------------------------------
// 1. Height actually changes height.
// ---------------------------------------------------------------------
#[test]
fn cc0_phase5_diversity_regression_1_height_modifier_scales_y_extent_only() {
    let base = generate_ok(1.0, 1.0, &[]);
    let tall = generate_ok(1.3, 1.0, &[]);
    let (eb, et) = (extents(&base.positions), extents(&tall.positions));
    eprintln!("[t1] extents base={eb:?} tall(1.3)={et:?} y-ratio={}", et[1] / eb[1]);

    assert!(
        et[1] >= 1.25 * eb[1],
        "height_modifier 1.3 must grow the Y extent by at least 1.25x: base {} -> tall {} \
         (ratio {})",
        eb[1],
        et[1],
        et[1] / eb[1]
    );
    for (axis, name) in [(0usize, "X"), (2usize, "Z")] {
        assert!(
            rel_diff(eb[axis], et[axis]) <= 0.01,
            "height_modifier must not stretch the {name} extent: base {} vs tall {} (rel diff {})",
            eb[axis],
            et[axis],
            rel_diff(eb[axis], et[axis])
        );
    }
}

// ---------------------------------------------------------------------
// 2. Weight actually changes girth, not height.
// ---------------------------------------------------------------------
#[test]
fn cc0_phase5_diversity_regression_2_weight_modifier_scales_x_and_z_extent_only() {
    let base = generate_ok(1.0, 1.0, &[]);
    let heavy = generate_ok(1.0, 1.4, &[]);
    let (eb, eh) = (extents(&base.positions), extents(&heavy.positions));
    eprintln!(
        "[t2] extents base={eb:?} heavy(1.4)={eh:?} x-ratio={} z-ratio={}",
        eh[0] / eb[0],
        eh[2] / eb[2]
    );

    for (axis, name) in [(0usize, "X"), (2usize, "Z")] {
        assert!(
            eh[axis] >= 1.3 * eb[axis],
            "weight_modifier 1.4 must grow the {name} extent by at least 1.3x: base {} -> heavy \
             {} (ratio {})",
            eb[axis],
            eh[axis],
            eh[axis] / eb[axis]
        );
    }
    assert!(
        rel_diff(eb[1], eh[1]) <= 0.01,
        "weight_modifier must not change the Y extent: base {} vs heavy {} (rel diff {})",
        eb[1],
        eh[1],
        rel_diff(eb[1], eh[1])
    );
}

// ---------------------------------------------------------------------
// 3. Height + weight compose (neither overrides the other).
// ---------------------------------------------------------------------
#[test]
fn cc0_phase5_diversity_regression_3_height_and_weight_compose() {
    let combined = generate_ok(1.2, 0.85, &[]);
    let height_only = generate_ok(1.2, 1.0, &[]);
    let weight_only = generate_ok(1.0, 0.85, &[]);
    let base = generate_ok(1.0, 1.0, &[]);

    let ec = extents(&combined.positions);
    let eh = extents(&height_only.positions);
    let ew = extents(&weight_only.positions);
    let eb = extents(&base.positions);
    eprintln!("[t3] base={eb:?} combined={ec:?} height-only(1.2)={eh:?} weight-only(0.85)={ew:?}");

    // Neither pure-height-only nor pure-weight-only.
    assert!(
        rel_diff(ec[0], eh[0]) > 0.01 && rel_diff(ec[2], eh[2]) > 0.01,
        "combined X/Z extents ({}, {}) must differ from height-only ({}, {}) -- weight_modifier \
         was silently ignored",
        ec[0], ec[2], eh[0], eh[2]
    );
    assert!(
        rel_diff(ec[1], ew[1]) > 0.01,
        "combined Y extent ({}) must differ from weight-only ({}) -- height_modifier was \
         silently ignored",
        ec[1], ew[1]
    );

    // And both are genuinely applied, in the right direction and amount
    // (extents scale linearly under positive per-axis scale; 2% slack).
    assert!(
        rel_diff(ec[1], 1.2 * eb[1]) <= 0.02,
        "combined Y extent {} should be ~1.2x base {}",
        ec[1], eb[1]
    );
    for (axis, name) in [(0usize, "X"), (2usize, "Z")] {
        assert!(
            rel_diff(ec[axis], 0.85 * eb[axis]) <= 0.02,
            "combined {name} extent {} should be ~0.85x base {}",
            ec[axis], eb[axis]
        );
    }
}

// ---------------------------------------------------------------------
// 4. Invalid scale is rejected (null), not clamped / NaN'd / panicking.
// ---------------------------------------------------------------------
#[test]
fn cc0_phase5_diversity_regression_4_invalid_height_modifier_is_rejected() {
    for (label, bad) in [("0.0", 0.0f32), ("-1.0", -1.0f32), ("NaN", f32::NAN)] {
        let result = generate(bad, 1.0, &[]);
        assert!(
            result.is_none(),
            "height_modifier = {label} must make generate_character return null, not a mesh"
        );
        // The failure must be reported, not silent.
        let err = last_error_string()
            .unwrap_or_else(|| panic!("height_modifier = {label}: expected a last-error message"));
        assert!(
            err.contains("DNA mutation failed"),
            "height_modifier = {label}: last-error should name the DNA mutation failure, got: {err}"
        );
    }
    // A valid call right after still works (no poisoned state).
    assert!(generate(1.0, 1.0, &[]).is_some());
}

// ---------------------------------------------------------------------
// 5. A morph displaces vertices, and weight scales the displacement.
// ---------------------------------------------------------------------
#[test]
fn cc0_phase5_diversity_regression_5_morph_displaces_and_weight_scales_displacement() {
    let baseline = generate_ok(1.0, 1.0, &[]);
    let half = generate_ok(1.0, 1.0, &[(EAR_MORPH_ID, 0.5)]);
    let full = generate_ok(1.0, 1.0, &[(EAR_MORPH_ID, 1.0)]);

    assert!(any_vertex_differs(&baseline, &half), "morph 5001 @0.5 must move at least one vertex");
    assert!(any_vertex_differs(&baseline, &full), "morph 5001 @1.0 must move at least one vertex");

    let max_half = max_displacement(&baseline, &half);
    let max_full = max_displacement(&baseline, &full);
    eprintln!(
        "[t5] max displacement @0.5={max_half} @1.0={max_full} ratio={}",
        max_full / max_half
    );
    assert!(max_half > 0.0, "half-weight max displacement must be > 0");
    assert!(
        max_full >= 1.8 * max_half,
        "weight 1.0 max displacement ({max_full}) must be >= 1.8x weight 0.5's ({max_half}) -- \
         weight is not being applied proportionally (ratio {})",
        max_full / max_half
    );
}

// ---------------------------------------------------------------------
// 6. Two different real morphs give two different results.
// ---------------------------------------------------------------------
#[test]
fn cc0_phase5_diversity_regression_6_different_morphs_produce_different_results() {
    let ear = generate_ok(1.0, 1.0, &[(EAR_MORPH_ID, 1.0)]);
    let nose = generate_ok(1.0, 1.0, &[(NOSE_MORPH_ID, 1.0)]);
    assert!(
        any_vertex_differs(&ear, &nose),
        "morph 5001 alone and morph 5002 alone produced identical vertex buffers -- the two \
         morphs are resolving to the same data (registry key collision?)"
    );
}

// ---------------------------------------------------------------------
// 7. Morphs and body-scale compose.
// ---------------------------------------------------------------------
#[test]
fn cc0_phase5_diversity_regression_7_morph_and_body_scale_compose() {
    let scaled_only = generate_ok(1.15, 1.0, &[]);
    let scaled_morphed = generate_ok(1.15, 1.0, &[(EAR_MORPH_ID, 1.0)]);
    assert!(
        any_vertex_differs(&scaled_only, &scaled_morphed),
        "height 1.15 + morph 5001 must differ from height 1.15 alone in at least one vertex"
    );

    // Also still differs from the unscaled morph-only result (i.e. the
    // scale was not lost either).
    let morphed_only = generate_ok(1.0, 1.0, &[(EAR_MORPH_ID, 1.0)]);
    assert!(
        any_vertex_differs(&scaled_morphed, &morphed_only),
        "height 1.15 + morph 5001 must differ from morph 5001 alone (scale was lost)"
    );

    // Order is pinned, not just printed: `generate_character` applies morphs
    // BEFORE body scale (per-part `apply_morph_targets`, then `merge_parts`,
    // then `mutate_skin_vertices`; see its doc comment in src/lib.rs). Find
    // the vertex with the largest morph displacement and compare its Y
    // displacement with vs. without scale. Morph-then-scale => the morph's
    // own displacement is scaled too, so the ratio is the height factor
    // (1.15); scale-then-morph => the morph delta is unscaled, ratio 1.0.
    let baseline = generate_ok(1.0, 1.0, &[]);
    let (mut best_i, mut best_dy) = (0usize, 0.0f32);
    for i in 0..baseline.positions.len() {
        let dy = (morphed_only.positions[i][1] - baseline.positions[i][1]).abs();
        if dy > best_dy {
            best_dy = dy;
            best_i = i;
        }
    }
    let dy_scaled =
        (scaled_morphed.positions[best_i][1] - scaled_only.positions[best_i][1]).abs();
    assert!(
        best_dy > 0.0,
        "morph 5001 @1.0 must displace at least one vertex in Y for the order probe to mean anything"
    );
    let ratio = dy_scaled / best_dy;
    eprintln!(
        "[t7] order probe @vertex {best_i}: unscaled morph dY={best_dy}, scaled morph dY={dy_scaled}, \
         ratio={ratio} (1.15 => morph applied BEFORE scale; 1.0 => AFTER)"
    );
    // Measured 1.1500154 (f32 noise). 1.0 (scale-then-morph) is far outside
    // this window, so a swap in order fails here.
    assert!(
        (ratio - 1.15).abs() <= 0.01,
        "morph must be applied BEFORE body scale: expected the morph's Y displacement to grow \
         by the height factor 1.15 (got ratio {ratio}; ~1.0 would mean scale-then-morph)"
    );
}

// ---------------------------------------------------------------------
// 8. Unknown morph id is skipped, not fatal.
// ---------------------------------------------------------------------
#[test]
fn cc0_phase5_diversity_regression_8_unknown_morph_id_is_skipped_not_fatal() {
    let baseline = generate_ok(1.0, 1.0, &[]);
    let with_unknown = generate(1.0, 1.0, &[(UNKNOWN_MORPH_ID, 1.0)])
        .expect("an unknown morph id must be skipped, not make generate_character return null");

    assert_eq!(with_unknown.vertices_count, baseline.vertices_count, "vertex count must match baseline");
    assert_eq!(with_unknown.indices_count, baseline.indices_count, "index count must match baseline");
    assert_eq!(baseline.vertices_count, EXPECTED_VERTS);
    assert_eq!(baseline.indices_count, EXPECTED_INDICES);
    assert!(
        !any_vertex_differs(&baseline, &with_unknown),
        "an unknown morph id must be a no-op: positions must be identical to the baseline"
    );
}

// ---------------------------------------------------------------------
// 9. Invalid weight_modifier is rejected too (scale components 0 and 2).
//    Test 4 only covers height_modifier (component 1).
// ---------------------------------------------------------------------
#[test]
fn cc0_phase5_diversity_regression_9_invalid_weight_modifier_is_rejected() {
    for (label, bad) in [("0.0", 0.0f32), ("-1.0", -1.0f32), ("NaN", f32::NAN)] {
        let result = generate(1.0, bad, &[]);
        assert!(
            result.is_none(),
            "weight_modifier = {label} must make generate_character return null, not a mesh"
        );
        let err = last_error_string()
            .unwrap_or_else(|| panic!("weight_modifier = {label}: expected a last-error message"));
        assert!(
            err.contains("DNA mutation failed"),
            "weight_modifier = {label}: last-error should name the DNA mutation failure, got: {err}"
        );
    }
    // A valid call right after still works (no poisoned state).
    assert!(generate(1.0, 1.0, &[]).is_some());
}
