//! Generate-and-free loop against the real pack `packs/measurement-fit.afpp`,
//! measured with a counting global allocator.
//!
//! The counter is process-wide, so this file holds exactly one `#[test]`:
//! the harness runs tests concurrently and a second test would move the
//! counter during the measured window. Nothing is printed between the two
//! snapshots (the harness captures prints into a growing buffer that the
//! counter would see).

use anthroforge_core::{
    anthroforge_last_error, free_mesh_buffer, generate_character, init_part_registry_from_pack,
    CharacterDNA,
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::ffi::CStr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicIsize, Ordering};

/// Live heap bytes, as seen by the global allocator.
static LIVE_BYTES: AtomicIsize = AtomicIsize::new(0);

struct CountingAlloc;

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = System.alloc(layout);
        if !p.is_null() {
            LIVE_BYTES.fetch_add(layout.size() as isize, Ordering::Relaxed);
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = System.alloc_zeroed(layout);
        if !p.is_null() {
            LIVE_BYTES.fetch_add(layout.size() as isize, Ordering::Relaxed);
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        LIVE_BYTES.fetch_sub(layout.size() as isize, Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = System.realloc(ptr, layout, new_size);
        if !p.is_null() {
            LIVE_BYTES.fetch_add(new_size as isize - layout.size() as isize, Ordering::Relaxed);
        }
        p
    }
}

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

const WARMUP_CYCLES: usize = 20;
const MAX_GROWTH_BYTES: isize = 4096;

fn live() -> isize {
    LIVE_BYTES.load(Ordering::Relaxed)
}

fn cycles_from_env() -> usize {
    match std::env::var("AF_LOOP_CYCLES") {
        Err(_) => 100,
        Ok(raw) => match raw.trim().parse::<usize>() {
            Ok(n) if n > 0 => n,
            _ => panic!("AF_LOOP_CYCLES must be a positive integer, got {raw:?}"),
        },
    }
}

/// One generate with the given morphs, freed immediately. Returns whether
/// the call produced a non-null buffer.
fn generate_and_free(ids: &[u16], weights: &[f32]) -> bool {
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
        return false;
    }
    free_mesh_buffer(p);
    true
}

fn one_cycle() {
    assert!(generate_and_free(&[], &[]), "unmorphed generate returned null");
    assert!(
        generate_and_free(&[3901, 3929], &[0.5, 0.5]),
        "morphed generate returned null"
    );
}

#[test]
fn generate_free_loop_does_not_grow_the_live_heap() {
    // Counter sanity: the allocator wrapper really sees a 1 MiB allocation.
    {
        let base = live();
        let v = vec![1u8; 1 << 20];
        let raised = live() - base;
        assert!(raised >= 1 << 20, "counter rose by only {raised} bytes for 1 MiB");
        assert_eq!(v[v.len() - 1], 1);
        drop(v);
        let fell = base + raised - live();
        assert!(fell >= 1 << 20, "counter fell by only {fell} bytes after dropping 1 MiB");
    }

    let cycles = cycles_from_env();

    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("packs/measurement-fit.afpp");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    assert!(
        init_part_registry_from_pack(bytes.as_ptr(), bytes.len()),
        "loader rejected measurement-fit.afpp"
    );

    for _ in 0..WARMUP_CYCLES {
        one_cycle();
    }

    let before = live();
    for _ in 0..cycles {
        one_cycle();
    }
    let after = live();

    let growth = after - before;
    println!("cycles={cycles} growth_bytes={growth}");
    assert!(
        growth <= MAX_GROWTH_BYTES,
        "live heap grew by {growth} bytes over {cycles} cycles (limit {MAX_GROWTH_BYTES})"
    );

    // Refusal path, deliberately outside the measured window.
    assert!(
        !generate_and_free(&[1020], &[0.5]),
        "denied morph id 1020 must return null"
    );
    let ptr = anthroforge_last_error();
    assert!(!ptr.is_null(), "refusal must set a last error");
    let message = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
    assert!(
        message.contains("safety: refused morph id 1020"),
        "unexpected last error: {message}"
    );
}
