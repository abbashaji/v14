//! CC0-Phase 10 Part A: end-to-end native test proving AFMT v2 (i16
//! quantized) morphs flow through the whole real pipeline --
//! `pack_builder` embeds them, `init_part_registry_from_pack` loads and
//! dequantizes them inside `morph_loader::load_afmt_bytes`, and
//! `generate_character` -> `morph_blend::apply_morph_targets` applies them
//! to the real 4-part rigged body, with `morph_blend` seeing only ordinary
//! `f32` deltas.
//!
//! Same real-binary pattern as `cc0_phase3_morph_pipeline_e2e.rs`, which
//! is left untouched and keeps covering v1 (`tests/fixtures/
//! cc0_phase3_pipeline/` still holds the v1 `.afmt` files). This test
//! assembles its asset dir at runtime from that directory's 4 GLBs +
//! `master_skeleton.json` (no 3 MB of duplicated fixtures) plus the two v2
//! `.afmt` files in `tests/fixtures/cc0_phase10_afmt_v2/`, which the real
//! `morph_converter` binary wrote from the real upstream `.target` files.
//!
//! `GLOBAL_REGISTRY` is a process-wide `OnceLock`, so a v1 and a v2 pack
//! can't be loaded side by side in one process. Instead of comparing
//! against a v1 run, the expected displacement comes from the raw
//! `.target` text, i.e. the ground truth both formats encode.

use anthroforge_core::{free_mesh_buffer, generate_character, init_part_registry_from_pack, CharacterDNA};
use std::path::{Path, PathBuf};
use std::process::Command;

const EAR_ID: u16 = 5001;
const NOSE_ID: u16 = 5002;

/// Vertices a v1 run of this same pack/base mesh displaces at weight 1.0
/// (measured once against the v1 fixtures in `cc0_phase3_pipeline`). v2
/// must move exactly the same vertices: quantization changes values, never
/// which vertices a morph touches, because the smallest real delta
/// (0.001) is ~680 quantization steps, nowhere near rounding to zero.
const EAR_DISPLACED_VERTS: usize = 2361;
const NOSE_DISPLACED_VERTS: usize = 1162;

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Assemble a fresh asset dir (4 GLBs + skeleton from the v1 pipeline
/// fixture dir, but NOT its v1 `.afmt` files; plus the v2 `.afmt` files),
/// run the real `pack_builder`, and return the pack bytes.
fn build_v2_pack_bytes() -> Vec<u8> {
    let tmp = std::env::var("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    let assets = tmp.join("cc0_phase10a_v2_assets");
    let _ = std::fs::remove_dir_all(&assets); // no stale files from a previous run
    std::fs::create_dir_all(&assets).unwrap();

    let src = manifest().join("tests/fixtures/cc0_phase3_pipeline");
    for entry in std::fs::read_dir(&src).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) == Some("afmt") {
            continue; // v1 morphs deliberately excluded
        }
        std::fs::copy(&path, assets.join(path.file_name().unwrap())).unwrap();
    }

    let v2_dir = manifest().join("tests/fixtures/cc0_phase10_afmt_v2");
    for name in ["5001_asym_ear_1_l.afmt", "5002_asym_nose_1_l.afmt"] {
        let dst = assets.join(name);
        std::fs::copy(v2_dir.join(name), &dst).unwrap();
        let bytes = std::fs::read(&dst).unwrap();
        assert_eq!(
            u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            2,
            "{name} must be an AFMT v2 file, or this test isn't testing v2"
        );
    }

    let out_path = tmp.join("cc0_phase10a_afmt_v2_generated.afpp");
    let status = Command::new(env!("CARGO_BIN_EXE_pack_builder"))
        .arg(&assets)
        .arg(&out_path)
        .arg(manifest().join("assets/upstream/base.obj"))
        .status()
        .expect("failed to run pack_builder");
    assert!(status.success(), "pack_builder failed building the v2 pack");
    std::fs::read(&out_path).unwrap()
}

/// Generate the full body with the given active morphs; returns every
/// output vertex position.
fn generate(morphs: &[(u16, f32)]) -> Vec<[f32; 3]> {
    let ids: Vec<u16> = morphs.iter().map(|m| m.0).collect();
    let weights: Vec<f32> = morphs.iter().map(|m| m.1).collect();
    let dna = CharacterDNA {
        seed: 42,
        height_modifier: 1.0,
        weight_modifier: 1.0,
        head_id: 4001,
        torso_id: 4002,
        arms_id: 4003,
        legs_id: 4004,
        equipped_clothing_ids_ptr: std::ptr::null(),
        equipped_clothing_count: 0,
        active_morph_ids_ptr: if morphs.is_empty() { std::ptr::null() } else { ids.as_ptr() },
        active_morph_weights_ptr: if morphs.is_empty() { std::ptr::null() } else { weights.as_ptr() },
        active_morph_count: morphs.len() as u32,
    };
    let ptr = generate_character(&dna as *const CharacterDNA);
    assert!(!ptr.is_null(), "generate_character returned null for morphs {morphs:?}");
    // SAFETY: non-null buffer just returned by `generate_character`; its
    // `vertices_ptr` is valid for `vertices_count` vertices until freed.
    let buf = unsafe { &*ptr };
    let verts = unsafe { std::slice::from_raw_parts(buf.vertices_ptr, buf.vertices_count as usize) };
    let positions = verts.iter().map(|v| v.position).collect();
    free_mesh_buffer(ptr);
    positions
}

/// Largest `||(dx, dy, dz)||` over the raw `.target` text file.
fn target_max_displacement(target: &str) -> f32 {
    let path = manifest().join("tests/fixtures/cc0_phase3_real_morphs").join(target);
    std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let f: Vec<f32> = l.split_whitespace().skip(1).map(|x| x.parse().unwrap()).collect();
            (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt()
        })
        .fold(0.0, f32::max)
}

fn position_scale(afmt: &str) -> f32 {
    let bytes = std::fs::read(Path::new(&manifest()).join("tests/fixtures/cc0_phase10_afmt_v2").join(afmt)).unwrap();
    f32::from_le_bytes(bytes[14..18].try_into().unwrap())
}

/// (max displacement, number of vertices displaced) between two outputs.
fn displacement(a: &[[f32; 3]], b: &[[f32; 3]]) -> (f32, usize) {
    assert_eq!(a.len(), b.len(), "morphs must not change vertex count");
    let mut max = 0.0f32;
    let mut count = 0usize;
    for (p, q) in a.iter().zip(b) {
        let d = ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) + (q[2] - p[2]).powi(2)).sqrt();
        if d > 0.0 {
            count += 1;
        }
        max = max.max(d);
    }
    (max, count)
}

#[test]
fn cc0_phase10a_v2_morphs_apply_through_the_real_pipeline() {
    let pack = build_v2_pack_bytes();
    assert!(
        init_part_registry_from_pack(pack.as_ptr(), pack.len()),
        "init_part_registry_from_pack must accept a pack whose only morphs are AFMT v2"
    );

    let baseline = generate(&[]);
    let max_coord = baseline.iter().flatten().fold(0.0f32, |m, c| m.max(c.abs()));

    for (id, target, afmt, displaced) in [
        (EAR_ID, "asym-ear-1-l.target", "5001_asym_ear_1_l.afmt", EAR_DISPLACED_VERTS),
        (NOSE_ID, "asym-nose-1-l.target", "5002_asym_nose_1_l.afmt", NOSE_DISPLACED_VERTS),
    ] {
        let want_max = target_max_displacement(target);
        let scale = position_scale(afmt);

        for weight in [1.0f32, 0.5] {
            let morphed = generate(&[(id, weight)]);
            let (got_max, got_count) = displacement(&baseline, &morphed);

            // Tolerance = worst-case Euclidean quantization error
            // (sqrt(3) * scale / 2 per vertex, scaled by the weight) +
            // f32 noise from subtracting two positions as large as
            // `max_coord` (4 ulps). This is ~1e-4 of the displacement
            // being measured, so a wrong scale or a mis-dequantized
            // component fails it.
            let tol = weight * 3f32.sqrt() * 0.5 * scale + 4.0 * f32::EPSILON * max_coord;
            let want = want_max * weight;
            eprintln!(
                "morph {id} @ {weight}: max disp {got_max:.9} vs .target-derived {want:.9} \
                 (diff {:+e}, tol {tol:e}); {got_count} vertices displaced",
                got_max - want
            );
            assert!(
                (got_max - want).abs() <= tol,
                "morph {id} @ {weight}: max displacement {got_max} differs from the source \
                 .target's {want} by more than tolerance {tol}"
            );
            assert_eq!(
                got_count, displaced,
                "morph {id} @ {weight}: v2 must displace exactly the vertices v1 did"
            );
        }
    }
}
