//! Safety gate, pack WITH the denied morphs.
//!
//! The distributed packs are the output of `pack_strip` and no longer hold
//! the denied morphs, so this test builds its own pack: the real 4-part
//! body of `tests/fixtures/cc0_phase3_pipeline/` plus one morph for every id
//! of `packs/morph_id_map.json` (all 1280, including the 228 minor and 6
//! genital ones), each morph a copy of one real `.afmt` file, built with the
//! real `pack_builder` binary. Here the registry DOES contain the denied
//! morphs, so refusal is the gate's doing and nothing else's: every denied
//! id the pack holds must be refused, and the permitted ones must still
//! generate and apply.
//!
//! The registry is a once-per-process global, so this pack has its own
//! file (see `safety_gate_unloaded.rs` for the pack that lacks the ids).

use anthroforge_core::{
    anthroforge_last_error, free_mesh_buffer, generate_character, init_part_registry_from_pack,
    is_denied_morph_id, CharacterDNA,
};
use std::ffi::CStr;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Once, OnceLock};

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every morph id of `packs/morph_id_map.json`, sorted.
fn map_morph_ids() -> Vec<u16> {
    let path = manifest_dir().join("packs/morph_id_map.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let map: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", path.display()));
    let mut ids: Vec<u16> = map
        .as_object()
        .unwrap_or_else(|| panic!("{} must be a JSON object", path.display()))
        .keys()
        .map(|k| k.parse().unwrap_or_else(|_| panic!("map key {k:?} is not a u16")))
        .collect();
    ids.sort_unstable();
    ids
}

/// Builds the pack with `pack_builder` in a directory under
/// `CARGO_TARGET_TMPDIR` that is deleted and recreated on every run, and
/// returns its bytes. Every id of the map gets one morph file
/// `<id>_morph.afmt` (`pack_builder` takes the id from the numeric prefix).
fn build_pack() -> Vec<u8> {
    let fixture = manifest_dir().join("tests/fixtures/cc0_phase3_pipeline");
    let base_mesh = manifest_dir().join("assets/upstream/base.obj");
    assert!(base_mesh.is_file(), "expected base mesh at '{}'", base_mesh.display());

    let work = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("safety_gate_full_pack");
    if work.exists() {
        std::fs::remove_dir_all(&work)
            .unwrap_or_else(|e| panic!("failed to delete {}: {e}", work.display()));
    }
    let assets = work.join("assets");
    std::fs::create_dir_all(&assets)
        .unwrap_or_else(|e| panic!("failed to create {}: {e}", assets.display()));

    for name in [
        "4001_head.glb",
        "4002_torso.glb",
        "4003_arms.glb",
        "4004_legs.glb",
        "master_skeleton.json",
    ] {
        std::fs::copy(fixture.join(name), assets.join(name))
            .unwrap_or_else(|e| panic!("failed to copy fixture file {name}: {e}"));
    }
    let afmt = std::fs::read(fixture.join("5001_asym_ear_1_l.afmt"))
        .unwrap_or_else(|e| panic!("failed to read the fixture morph: {e}"));
    let ids = map_morph_ids();
    for id in &ids {
        std::fs::write(assets.join(format!("{id}_morph.afmt")), &afmt)
            .unwrap_or_else(|e| panic!("failed to write the morph file of id {id}: {e}"));
    }

    let out_path = work.join("with_denied_morphs.afpp");
    let out = Command::new(env!("CARGO_BIN_EXE_pack_builder"))
        .arg(&assets)
        .arg(&out_path)
        .arg(&base_mesh)
        .output()
        .unwrap_or_else(|e| panic!("failed to run pack_builder: {e}"));
    assert!(
        out.status.success(),
        "pack_builder failed building the pack with {} morphs: {}",
        ids.len(),
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::read(&out_path)
        .unwrap_or_else(|e| panic!("failed to read the built pack {}: {e}", out_path.display()))
}

/// Morph ids of an AFPP v2 pack, in index order (same decoding as
/// `morph_ids` in `cc0_phase10_pack_library.rs`).
fn pack_morph_ids(pack: &[u8]) -> Vec<u16> {
    assert_eq!(&pack[0..4], b"AFPP");
    let u32_at = |o: usize| u32::from_le_bytes(pack[o..o + 4].try_into().unwrap()) as usize;
    let (part_count, morph_count, skel_len) = (u32_at(8), u32_at(12), u32_at(16));
    let morph_index = 20 + skel_len + part_count * 21;
    (0..morph_count)
        .map(|i| {
            let o = morph_index + i * 12;
            u16::from_le_bytes(pack[o..o + 2].try_into().unwrap())
        })
        .collect()
}

static PACK: OnceLock<Vec<u8>> = OnceLock::new();
static INIT: Once = Once::new();

/// The pack bytes, built once; the registry is initialised from them once.
/// The buffer lives in a static so it outlives every use.
fn pack() -> &'static [u8] {
    let bytes = PACK.get_or_init(build_pack);
    INIT.call_once(|| {
        assert!(
            init_part_registry_from_pack(bytes.as_ptr(), bytes.len()),
            "loader rejected the pack built with the denied morphs"
        );
    });
    bytes
}

/// Generates the real 4-part body with the given morphs. `None` when
/// `generate_character` returns null; otherwise every vertex position.
fn generate(ids: &[u16], weights: &[f32]) -> Option<Vec<[f32; 3]>> {
    assert_eq!(ids.len(), weights.len());
    pack();
    let dna = CharacterDNA {
        seed: 1,
        height_modifier: 1.0,
        weight_modifier: 1.0,
        head_id: 4001,
        torso_id: 4002,
        arms_id: 4003,
        legs_id: 4004,
        equipped_clothing_ids_ptr: std::ptr::null(),
        equipped_clothing_count: 0,
        active_morph_ids_ptr: if ids.is_empty() { std::ptr::null() } else { ids.as_ptr() },
        active_morph_weights_ptr: if ids.is_empty() { std::ptr::null() } else { weights.as_ptr() },
        active_morph_count: ids.len() as u32,
    };
    let p = generate_character(&dna as *const CharacterDNA);
    if p.is_null() {
        return None;
    }
    let buf = unsafe { &*p };
    let verts = unsafe { std::slice::from_raw_parts(buf.vertices_ptr, buf.vertices_count as usize) };
    let positions = verts.iter().map(|v| v.position).collect();
    free_mesh_buffer(p);
    Some(positions)
}

/// The calling thread's last error text; empty when none is set.
fn last_error() -> String {
    let p = anthroforge_last_error();
    if p.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

#[test]
fn loaded_denied_morphs_are_refused_and_loaded_permitted_ones_apply() {
    let mut ids = pack_morph_ids(pack());
    ids.sort_unstable();
    assert_eq!(ids.len(), 1280, "built pack morph count");
    assert_eq!(ids, map_morph_ids(), "the built pack must hold every id of morph_id_map.json");

    // Every denied id the pack holds: 228 minor + 6 genital.
    let denied: Vec<u16> = ids.iter().copied().filter(|&id| is_denied_morph_id(id)).collect();
    assert_eq!(denied.len(), 234, "denied ids held by the built pack");
    for &id in &denied {
        assert!(
            generate(&[id], &[1.0]).is_none(),
            "generate_character must refuse loaded morph id {id}"
        );
        let err = last_error();
        assert!(err.contains("safety:"), "id {id}: error was {err:?}");
        assert!(err.contains(&format!("morph id {id} ")), "id {id}: error was {err:?}");
    }

    // 1022 (macrodetails/caucasian-male-old) is permitted and really applies.
    assert!(!is_denied_morph_id(1022));
    let base = generate(&[], &[]).expect("a character with no morphs must generate");
    let old = generate(&[1022], &[1.0]).expect("permitted morph 1022 must generate");
    assert_eq!(old.len(), base.len(), "vertex count with morph 1022");
    assert!(
        old.iter().zip(&base).any(|(a, b)| a != b),
        "morph 1022 at weight 1.0 moved no vertex"
    );

    // A spread of the permitted ids all generate.
    let permitted: Vec<u16> = ids.iter().copied().filter(|&id| !is_denied_morph_id(id)).collect();
    assert_eq!(permitted.len(), 1046, "permitted ids held by the built pack");
    for &id in permitted.iter().step_by(permitted.len() / 20).take(20) {
        assert!(
            generate(&[id], &[1.0]).is_some(),
            "permitted morph id {id} must generate"
        );
    }
}
