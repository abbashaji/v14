//! CC0-Phase 8: load a built library pack through the REAL loader and apply
//! every morph in it, one at a time, to the real 4-part body.
//!
//! Skipped (passes trivially) unless `AF_PACK_PATH` is set, because the packs
//! are large build outputs, not checked-in fixtures. The part/morph registry
//! is a once-per-process global, so run ONE pack per invocation:
//!
//!   AF_PACK_PATH=/path/to/essentials.afpp \
//!     cargo test --release --test cc0_phase8_pack_library -- --nocapture
//!
//! Optional: `AF_EXPECT_MORPHS=<n>` asserts the pack's morph count.
//! Prints how many morphs produced zero displacement (legitimately possible
//! for header-only/empty targets) and the max displacement seen.

use anthroforge_core::{generate_character, free_mesh_buffer, init_part_registry_from_pack, CharacterDNA};
use std::time::Instant;

fn morph_ids(pack: &[u8]) -> Vec<u16> {
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

fn dna(ids: *const u16, weights: *const f32, n: u32) -> CharacterDNA {
    CharacterDNA {
        seed: 42,
        height_modifier: 1.0,
        weight_modifier: 1.0,
        head_id: 4001,
        torso_id: 4002,
        arms_id: 4003,
        legs_id: 4004,
        equipped_clothing_ids_ptr: std::ptr::null(),
        equipped_clothing_count: 0,
        active_morph_ids_ptr: ids,
        active_morph_weights_ptr: weights,
        active_morph_count: n,
    }
}

fn positions(dna: &CharacterDNA) -> Vec<[f32; 3]> {
    let p = generate_character(dna as *const CharacterDNA);
    assert!(!p.is_null(), "generate_character returned null");
    let b = unsafe { &*p };
    assert_eq!(b.vertices_count, 53_512);
    let v = unsafe { std::slice::from_raw_parts(b.vertices_ptr, b.vertices_count as usize) };
    let out = v.iter().map(|x| x.position).collect();
    free_mesh_buffer(p);
    out
}

#[test]
fn every_morph_in_pack_loads_and_applies() {
    let Ok(path) = std::env::var("AF_PACK_PATH") else {
        eprintln!("AF_PACK_PATH not set; skipping");
        return;
    };
    let bytes = std::fs::read(&path).unwrap();
    let ids = morph_ids(&bytes);
    if let Ok(n) = std::env::var("AF_EXPECT_MORPHS") {
        assert_eq!(ids.len(), n.parse::<usize>().unwrap());
    }

    let t = Instant::now();
    assert!(init_part_registry_from_pack(bytes.as_ptr(), bytes.len()), "loader rejected {path}");
    eprintln!("loaded {path}: {} morphs, {} bytes, {:.2}s", ids.len(), bytes.len(), t.elapsed().as_secs_f32());

    let base = positions(&dna(std::ptr::null(), std::ptr::null(), 0));
    // Sanity bound tied to the real body: no single morph at weight 1.0 may move
    // a vertex farther than the body is tall (catches unit/scale blow-ups).
    // Macrodetails targets are whole-population deltas (e.g. baby <-> adult) and
    // legitimately move vertices by a large fraction of body height.
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for p in &base { lo = lo.min(p[1]); hi = hi.max(p[1]); }
    let body_h = hi - lo;
    eprintln!("base body height (Y extent): {body_h:.3}");
    let (mut zero, mut max_d, mut refused) = (Vec::new(), 0.0f32, 0usize);
    for &id in &ids {
        // `generate_character` refuses denied ids by design (src/safety.rs),
        // so there is nothing to apply for them: count and skip.
        if anthroforge_core::is_denied_morph_id(id) {
            refused += 1;
            continue;
        }
        let (i, w) = ([id], [1.0f32]);
        let pos = positions(&dna(i.as_ptr(), w.as_ptr(), 1));
        let d = pos.iter().zip(&base).map(|(a, b)| {
            ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
        }).fold(0.0f32, f32::max);
        if d == 0.0 { zero.push(id); }
        max_d = max_d.max(d);
        assert!(d.is_finite() && d < body_h, "morph {id}: displacement {d} exceeds body height {body_h}");
    }
    eprintln!("applied {} morphs individually ({} denied ids skipped); zero-displacement: {} {:?}; max displacement {:.4}",
              ids.len() - refused, refused, zero.len(), zero, max_d);
}
