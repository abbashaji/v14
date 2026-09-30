//! CC0-Phase 10 Part B: dump a per-morph fingerprint of the real generated mesh
//! (every morph applied singly at weight 1.0). Run once on the OLD pack and once
//! on the deduped pack (one pack per process; registry is a global) and diff the
//! two output files: they must be identical.
//!   AF_PACK_PATH=x.afpp AF_FP_OUT=fp.txt cargo test --release --test cc0_phase10_dedup_equivalence
use anthroforge_core::{free_mesh_buffer, generate_character, init_part_registry_from_pack, CharacterDNA};

fn morph_ids(pack: &[u8]) -> Vec<u16> {
    let u32_at = |o: usize| u32::from_le_bytes(pack[o..o + 4].try_into().unwrap()) as usize;
    let (pc, mc, sl) = (u32_at(8), u32_at(12), u32_at(16));
    let mi = 20 + sl + pc * 21;
    (0..mc).map(|i| u16::from_le_bytes(pack[mi + i * 12..mi + i * 12 + 2].try_into().unwrap())).collect()
}

#[test]
fn dump_fingerprints() {
    let (Ok(path), Ok(out)) = (std::env::var("AF_PACK_PATH"), std::env::var("AF_FP_OUT")) else { return };
    let bytes = std::fs::read(&path).unwrap();
    let mut ids = morph_ids(&bytes);
    ids.sort();
    assert!(init_part_registry_from_pack(bytes.as_ptr(), bytes.len()));
    let mut s = String::new();
    for id in ids {
        let (i, w) = ([id], [1.0f32]);
        let dna = CharacterDNA {
            seed: 42, height_modifier: 1.0, weight_modifier: 1.0,
            head_id: 4001, torso_id: 4002, arms_id: 4003, legs_id: 4004,
            equipped_clothing_ids_ptr: std::ptr::null(), equipped_clothing_count: 0,
            active_morph_ids_ptr: i.as_ptr(), active_morph_weights_ptr: w.as_ptr(), active_morph_count: 1,
        };
        let p = generate_character(&dna as *const CharacterDNA);
        assert!(!p.is_null());
        let b = unsafe { &*p };
        let v = unsafe { std::slice::from_raw_parts(b.vertices_ptr, b.vertices_count as usize) };
        // FNV-1a over exact position bits
        let mut h: u64 = 0xcbf29ce484222325;
        for x in v { for c in x.position { for byte in c.to_bits().to_le_bytes() { h ^= byte as u64; h = h.wrapping_mul(0x100000001b3); } } }
        free_mesh_buffer(p);
        s.push_str(&format!("{id} {h:016x}\n"));
    }
    std::fs::write(out, s).unwrap();
}
