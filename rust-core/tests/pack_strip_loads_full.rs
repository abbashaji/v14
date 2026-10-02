//! A stripped `full.afpp` loads through the real loader and generates.
//!
//! Runs `pack_strip` on `packs/full.afpp`, loads the output with
//! `init_part_registry_from_pack`, and checks that permitted morphs apply
//! while a denied id is still refused by the gate. The registry is a
//! once-per-process global, so this pack has its own test file (statics and
//! helpers copied from `safety_gate_full.rs`, not imported).

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

/// Morph ids of an AFPP v2 pack, in index order.
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
static LOADED: OnceLock<bool> = OnceLock::new();

/// Runs the tool on `packs/full.afpp` into a temp directory and returns the
/// output bytes (read once, kept in a static so they outlive every use).
fn stripped_full_bytes() -> &'static [u8] {
    PACK.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("pack_strip_loads_full_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let out_path = dir.join("full.stripped.afpp");
        let output = Command::new(env!("CARGO_BIN_EXE_pack_strip"))
            .arg(manifest_dir().join("packs").join("full.afpp"))
            .arg(&out_path)
            .arg(manifest_dir().join("packs").join("CC0_NOTICE.txt"))
            .output()
            .expect("spawn pack_strip");
        assert_eq!(
            output.status.code(),
            Some(0),
            "pack_strip failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let bytes = std::fs::read(&out_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", out_path.display()));
        let _ = std::fs::remove_dir_all(&dir);
        bytes
    })
}

/// The stripped pack bytes; the registry is initialised from them once.
fn pack() -> &'static [u8] {
    let bytes = stripped_full_bytes();
    INIT.call_once(|| {
        let ok = init_part_registry_from_pack(bytes.as_ptr(), bytes.len());
        LOADED.set(ok).unwrap();
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
fn stripped_full_pack_loads_and_generates() {
    let bytes = pack();

    let ids = pack_morph_ids(bytes);
    assert_eq!(ids.len(), 1046, "stripped full pack morph count");
    assert!(
        ids.iter().all(|&id| !is_denied_morph_id(id)),
        "a denied id is still in the stripped pack"
    );
    assert_eq!(
        LOADED.get().copied(),
        Some(true),
        "init_part_registry_from_pack must accept the stripped pack"
    );

    let base = generate(&[], &[]).expect("a character with no morphs must generate");

    assert!(!is_denied_morph_id(1022));
    let old = generate(&[1022], &[1.0]).expect("permitted morph 1022 must generate");
    assert_eq!(old.len(), base.len(), "vertex count with morph 1022");
    assert!(
        old.iter().zip(&base).any(|(a, b)| a != b),
        "morph 1022 at weight 1.0 moved no vertex"
    );

    assert!(is_denied_morph_id(1020));
    assert!(generate(&[1020], &[1.0]).is_none(), "denied morph 1020 must be refused");
    let err = last_error();
    assert!(err.contains("safety:"), "error was {err:?}");
}
