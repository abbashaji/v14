//! Safety gate, pack WITHOUT the denied morphs.
//!
//! Loads `packs/expressions.afpp` (102 ids in 3700..=3801, none denied) and
//! proves `generate_character` refuses every denied morph id even though
//! this pack does not hold any of them: the gate must run before the
//! registry lookup, which silently skips ids the pack lacks.
//!
//! The ground truth is the real `packs/morph_id_map.json`: the denied set
//! is re-derived from it here (a `baby` or `child` token in the target
//! name, or category `genitals`), so `src/safety.rs` cannot drift from the
//! upstream map unnoticed.
//!
//! The registry is a once-per-process global, so a test that needs a
//! different pack lives in its own file (see `safety_gate_full.rs`).

use anthroforge_core::{
    anthroforge_last_error, free_mesh_buffer, generate_character, init_part_registry_from_pack,
    is_denied_morph_id, CharacterDNA,
};
use std::collections::BTreeMap;
use std::ffi::CStr;
use std::path::PathBuf;
use std::sync::{Once, OnceLock};

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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

/// The pack bytes, read once; the registry is initialised from them once.
/// The buffer lives in a static so it outlives every use.
fn pack() -> &'static [u8] {
    let bytes = PACK.get_or_init(|| {
        let path = manifest_dir().join("packs/expressions.afpp");
        std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
    });
    INIT.call_once(|| {
        assert!(
            init_part_registry_from_pack(bytes.as_ptr(), bytes.len()),
            "loader rejected expressions.afpp"
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

// ---------------------------------------------------------------------
// Ground truth: the real morph map.
// ---------------------------------------------------------------------

/// `packs/morph_id_map.json` as id -> (category, target).
fn morph_map() -> BTreeMap<u16, (String, String)> {
    let path = manifest_dir().join("packs/morph_id_map.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let json: serde_json::Value = serde_json::from_str(&text).expect("morph_id_map.json is JSON");
    json.as_object()
        .expect("morph_id_map.json is an object")
        .iter()
        .map(|(k, v)| {
            let id: u16 = k.parse().unwrap_or_else(|_| panic!("map key {k:?} is not a u16"));
            let field = |name: &str| {
                v[name]
                    .as_str()
                    .unwrap_or_else(|| panic!("map entry {k} has no string field {name:?}"))
                    .to_string()
            };
            (id, (field("category"), field("target")))
        })
        .collect()
}

/// Whether a target name carries a `baby` or `child` token, splitting on
/// `/`, `-`, `_` and `.`.
fn has_minor_token(target: &str) -> bool {
    target
        .split(|c| matches!(c, '/' | '-' | '_' | '.'))
        .any(|t| t == "baby" || t == "child")
}

/// Every id the gate must refuse: the 228 token ids, the 6 genital ids and
/// the whole genital block 2600..=2699, sorted and de-duplicated.
fn expected_denied_ids(map: &BTreeMap<u16, (String, String)>) -> Vec<u16> {
    let mut ids: Vec<u16> = map
        .iter()
        .filter(|(_, (cat, target))| has_minor_token(target) || cat == "genitals")
        .map(|(&id, _)| id)
        .collect();
    ids.extend(2600u16..=2699);
    ids.sort_unstable();
    ids.dedup();
    ids
}

#[test]
fn denied_set_equals_upstream_ground_truth() {
    let map = morph_map();
    assert_eq!(map.len(), 1280, "morph_id_map.json entry count");

    let token_matches = map.values().filter(|(_, t)| has_minor_token(t)).count();
    assert_eq!(token_matches, 228, "baby/child token matches");
    let genitals = map.values().filter(|(c, _)| c == "genitals").count();
    assert_eq!(genitals, 6, "entries of category genitals");

    for (&id, (category, target)) in &map {
        let expected = has_minor_token(target) || category == "genitals";
        assert_eq!(
            is_denied_morph_id(id),
            expected,
            "id {id} ({category}, {target}): gate disagrees with the map"
        );
    }

    for id in 2600u16..=2699 {
        assert!(is_denied_morph_id(id), "genital block id {id} must be denied");
    }
    for id in [0u16, 999, 2599, 2700, 3700, 5001, 5002, 9999, 65535] {
        assert!(!is_denied_morph_id(id), "id {id} must be permitted");
    }
}

#[test]
fn refuses_every_denied_id_even_when_pack_lacks_it() {
    let map = morph_map();
    let denied = expected_denied_ids(&map);
    assert_eq!(denied.len(), 328, "228 token ids + 6 genital ids, plus the rest of 2600..=2699");

    // The test relies on this: none of the denied ids is in the loaded pack,
    // so only a gate that runs before id resolution can refuse them.
    let held = pack_morph_ids(pack());
    for id in &denied {
        assert!(!held.contains(id), "expressions.afpp unexpectedly holds denied id {id}");
    }

    for &id in &denied {
        for w in [1.0f32, 0.5, 0.0, -1.0, f32::NAN] {
            assert!(
                generate(&[id], &[w]).is_none(),
                "generate_character must refuse morph id {id} at weight {w}"
            );
            let err = last_error();
            assert!(err.contains("safety:"), "id {id} weight {w}: error was {err:?}");
            assert!(
                err.contains(&format!("morph id {id} ")),
                "id {id} weight {w}: error was {err:?}"
            );
        }
    }
}

#[test]
fn refuses_a_denied_id_hidden_among_permitted_ones() {
    let ok = pack_morph_ids(pack())[0];
    assert!(!is_denied_morph_id(ok));

    assert!(generate(&[ok, 1021, ok], &[0.3, 0.3, 0.3]).is_none());
    let err = last_error();
    assert!(err.contains("morph id 1021 (minor)"), "error was {err:?}");

    assert!(generate(&[ok, ok, 2603], &[0.3, 0.3, 0.3]).is_none());
    let err = last_error();
    assert!(err.contains("morph id 2603 (genital)"), "error was {err:?}");
}

#[test]
fn permitted_requests_still_generate() {
    let base = generate(&[], &[]).expect("a character with no morphs must generate");
    assert!(!base.is_empty(), "no-morph character has no vertices");

    let first = *pack_morph_ids(pack()).iter().min().expect("pack has morphs");
    let morphed = generate(&[first], &[1.0]).expect("a permitted morph from the pack must generate");
    assert_eq!(morphed.len(), base.len(), "vertex count with morph {first}");
    assert!(
        morphed.iter().zip(&base).any(|(a, b)| a != b),
        "morph {first} at weight 1.0 changed no vertex"
    );

    // An unknown but permitted id is skipped by id resolution, not refused.
    let unknown = generate(&[9999], &[1.0]).expect("an unknown permitted id must be skipped, not refused");
    assert_eq!(unknown, base, "an unknown id must leave the positions unchanged");

    assert_eq!(last_error(), "", "a successful call must leave no error behind");
}
