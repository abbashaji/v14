//! Normals of a morphed body, against the real pack `packs/measurement-fit.afpp`.
//!
//! Morph targets carry a zero `normal_delta`, so `generate_character`
//! transfers the geometric change of the morph into the authored normals.
//! These tests compute their own welded, area-weighted normals from the
//! returned buffers; they do not call into `normals.rs`.

use anthroforge_core::{
    free_mesh_buffer, generate_character, init_part_registry_from_pack, CharacterDNA,
};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Once, OnceLock};

static PACK: OnceLock<Vec<u8>> = OnceLock::new();
static INIT: Once = Once::new();

/// Reads the pack once and initialises the process-global registry from it.
/// The bytes live in a static so they outlive every use.
fn load_pack() {
    let bytes = PACK.get_or_init(|| {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("packs/measurement-fit.afpp");
        std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
    });
    INIT.call_once(|| {
        assert!(
            init_part_registry_from_pack(bytes.as_ptr(), bytes.len()),
            "loader rejected measurement-fit.afpp"
        );
    });
}

struct Out {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    indices: Vec<u32>,
}

fn generate(height: f32, weight: f32, ids: &[u16], weights: &[f32]) -> Out {
    assert_eq!(ids.len(), weights.len());
    load_pack();
    let dna = CharacterDNA {
        seed: 1,
        height_modifier: height,
        weight_modifier: weight,
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
    assert!(!p.is_null(), "generate_character returned null for {ids:?}");
    let buf = unsafe { &*p };
    let verts = unsafe { std::slice::from_raw_parts(buf.vertices_ptr, buf.vertices_count as usize) };
    let indices = unsafe { std::slice::from_raw_parts(buf.indices_ptr, buf.indices_count as usize) };
    let out = Out {
        positions: verts.iter().map(|v| v.position).collect(),
        normals: verts.iter().map(|v| v.normal).collect(),
        indices: indices.to_vec(),
    };
    free_mesh_buffer(p);
    out
}

fn no_morph() -> Out {
    generate(1.0, 1.0, &[], &[])
}

fn bits3(v: [f32; 3]) -> [u32; 3] {
    [v[0].to_bits(), v[1].to_bits(), v[2].to_bits()]
}

/// Angle in degrees, `atan2(|a x b|, a . b)` in `f64`.
fn angle_deg(a: [f64; 3], b: [f64; 3]) -> f64 {
    let c = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    let cl = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
    let d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    cl.atan2(d).to_degrees()
}

fn angle_f32(a: [f32; 3], b: [f32; 3]) -> f64 {
    angle_deg(
        [a[0] as f64, a[1] as f64, a[2] as f64],
        [b[0] as f64, b[1] as f64, b[2] as f64],
    )
}

/// Weld group id per vertex by position bit pattern (`-0.0` counts as `+0.0`).
fn weld(positions: &[[f32; 3]]) -> (Vec<usize>, usize) {
    let mut map: HashMap<[u32; 3], usize> = HashMap::new();
    let ids = positions
        .iter()
        .map(|p| {
            let key = [p[0], p[1], p[2]].map(|x| if x == 0.0 { 0 } else { x.to_bits() });
            let next = map.len();
            *map.entry(key).or_insert(next)
        })
        .collect();
    (ids, map.len())
}

/// Area-weighted (unnormalized cross products summed) normal per weld group.
fn group_normals(positions: &[[f32; 3]], indices: &[u32], groups: &[usize], n: usize) -> Vec<[f64; 3]> {
    let mut acc = vec![[0.0f64; 3]; n];
    for tri in indices.chunks_exact(3) {
        let p = |i: u32| {
            let v = positions[i as usize];
            [v[0] as f64, v[1] as f64, v[2] as f64]
        };
        let (a, b, c) = (p(tri[0]), p(tri[1]), p(tri[2]));
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let f = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        for &corner in tri {
            let g = groups[corner as usize];
            for axis in 0..3 {
                acc[g][axis] += f[axis];
            }
        }
    }
    acc
}

fn len3(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

#[test]
fn morph_changes_normals_near_moved_vertices_only() {
    let base = no_morph();
    let morphed = generate(1.0, 1.0, &[3901], &[1.0]);
    let n = base.positions.len();
    assert_eq!(morphed.positions.len(), n);
    assert_eq!(morphed.indices, base.indices);

    // M: vertices whose position differs.
    let moved: Vec<bool> = (0..n)
        .map(|i| bits3(base.positions[i]) != bits3(morphed.positions[i]))
        .collect();
    let m_count = moved.iter().filter(|&&m| m).count();
    assert!((1500..=2500).contains(&m_count), "|M| = {m_count}");

    // T: every vertex of a triangle that has a vertex in M.
    let mut in_t = vec![false; n];
    for tri in morphed.indices.chunks_exact(3) {
        if tri.iter().any(|&i| moved[i as usize]) {
            for &i in tri {
                in_t[i as usize] = true;
            }
        }
    }
    // C: vertices whose no-morph position bit pattern equals that of a vertex in T.
    let t_keys: HashSet<[u32; 3]> = (0..n)
        .filter(|&i| in_t[i])
        .map(|i| bits3(base.positions[i]))
        .collect();
    let in_c: Vec<bool> = (0..n)
        .map(|i| t_keys.contains(&bits3(base.positions[i])))
        .collect();

    let outside = in_c.iter().filter(|&&c| !c).count();
    assert!(outside > 40000, "only {outside} vertices outside C");

    let mut changed = 0usize;
    let mut max_change = 0.0f64;
    for i in 0..n {
        if !in_c[i] {
            assert_eq!(
                bits3(base.normals[i]),
                bits3(morphed.normals[i]),
                "normal of vertex {i} outside C changed"
            );
            continue;
        }
        let a = angle_f32(base.normals[i], morphed.normals[i]);
        if a > 0.5 {
            changed += 1;
        }
        max_change = max_change.max(a);
    }
    assert!((1500..=3000).contains(&changed), "{changed} vertices changed by > 0.5 deg");
    assert!((8.0..=11.0).contains(&max_change), "max change {max_change} deg");
    println!("morph 3901: |M|={m_count}, outside C={outside}, changed>0.5deg={changed}, max={max_change:.3}");
}

#[test]
fn zero_weight_morph_keeps_normals_bit_identical() {
    let base = no_morph();
    let zero = generate(1.0, 1.0, &[3901], &[0.0]);
    assert_eq!(base.positions.len(), zero.positions.len());
    for i in 0..base.positions.len() {
        assert_eq!(bits3(base.positions[i]), bits3(zero.positions[i]), "position {i}");
        assert_eq!(bits3(base.normals[i]), bits3(zero.normals[i]), "normal {i}");
    }
}

/// Rotation of `v` taking unit `nb` to unit `nm`, written in axis-angle form
/// (independent of the Rodrigues-without-normalization form in `normals.rs`).
/// Returns `v` unchanged for a flip (`nb` and `nm` antiparallel) or `nb == nm`.
fn rotate_axis_angle(nb: [f64; 3], nm: [f64; 3], v: [f64; 3]) -> [f64; 3] {
    let k = [
        nb[1] * nm[2] - nb[2] * nm[1],
        nb[2] * nm[0] - nb[0] * nm[2],
        nb[0] * nm[1] - nb[1] * nm[0],
    ];
    let c = nb[0] * nm[0] + nb[1] * nm[1] + nb[2] * nm[2];
    let s = len3(k);
    if s <= 1e-12 {
        return v;
    }
    let axis = [k[0] / s, k[1] / s, k[2] / s];
    let theta = s.atan2(c);
    let (st, ct) = (theta.sin(), theta.cos());
    let ax_v = [
        axis[1] * v[2] - axis[2] * v[1],
        axis[2] * v[0] - axis[0] * v[2],
        axis[0] * v[1] - axis[1] * v[0],
    ];
    let a_dot_v = axis[0] * v[0] + axis[1] * v[1] + axis[2] * v[2];
    [
        v[0] * ct + ax_v[0] * st + axis[0] * a_dot_v * (1.0 - ct),
        v[1] * ct + ax_v[1] * st + axis[1] * a_dot_v * (1.0 - ct),
        v[2] * ct + ax_v[2] * st + axis[2] * a_dot_v * (1.0 - ct),
    ]
}

#[test]
fn normal_change_matches_geometric_change() {
    let base = no_morph();
    let (groups, group_count) = weld(&base.positions);
    let nb_all = group_normals(&base.positions, &base.indices, &groups, group_count);

    let cases: [(&[u16], &[f32]); 3] = [
        (&[3929], &[1.0]),
        (&[3901, 3929], &[1.0, 1.0]),
        (&[3929], &[0.5]),
    ];
    let mut worst = 0.0f64;
    let mut worst_literal = 0.0f64;
    for (ids, weights) in cases {
        let morphed = generate(1.0, 1.0, ids, weights);
        let nm_all = group_normals(&morphed.positions, &morphed.indices, &groups, group_count);
        let mut significant = 0usize;
        let mut case_worst = 0.0f64;
        let mut case_literal = 0.0f64;
        for i in 0..base.positions.len() {
            let (nb, nm) = (nb_all[groups[i]], nm_all[groups[i]]);
            let usable = len3(nb) > 1e-9 && len3(nm) > 1e-9;
            // Group rotation angle (the spec's literal "expected angle").
            let group_angle = if usable { angle_deg(nb, nm) } else { 0.0 };
            // Angle the authored normal itself moves through when the group
            // rotation is applied to it: equals `group_angle` only when the
            // authored normal is perpendicular to the rotation axis.
            let expected = if usable {
                let unit = |v: [f64; 3]| {
                    let l = len3(v);
                    [v[0] / l, v[1] / l, v[2] / l]
                };
                let v = [
                    base.normals[i][0] as f64,
                    base.normals[i][1] as f64,
                    base.normals[i][2] as f64,
                ];
                let flipped = {
                    let (a, b) = (unit(nb), unit(nm));
                    a[0] * b[0] + a[1] * b[1] + a[2] * b[2] <= -1.0 + 1e-6
                };
                if flipped {
                    0.0
                } else {
                    angle_deg(v, rotate_axis_angle(unit(nb), unit(nm), v))
                }
            } else {
                0.0
            };
            let actual = angle_f32(morphed.normals[i], base.normals[i]);
            let dev = (actual - expected).abs();
            assert!(
                dev <= 0.05,
                "case {ids:?}@{weights:?} vertex {i}: actual {actual}, expected {expected}"
            );
            case_worst = case_worst.max(dev);
            case_literal = case_literal.max((actual - group_angle).abs());
            if group_angle > 0.5 {
                significant += 1;
            }
        }
        assert!(significant >= 1000, "case {ids:?}@{weights:?}: only {significant} vertices > 0.5 deg");
        println!(
            "case {ids:?}@{weights:?}: {significant} vertices > 0.5 deg, largest deviation {case_worst:.6} deg \
             (versus the plain group angle: {case_literal:.4} deg)"
        );
        worst = worst.max(case_worst);
        worst_literal = worst_literal.max(case_literal);
    }
    println!("largest deviation over all cases: {worst:.6} deg (plain group angle: {worst_literal:.4} deg)");
}

#[test]
fn registry_is_not_mutated_by_morph_generation() {
    let first = no_morph();
    let _morphed = generate(1.0, 1.0, &[3929, 3901], &[1.0, 1.0]);
    let third = no_morph();
    assert_eq!(first.positions.len(), third.positions.len());
    for i in 0..first.positions.len() {
        assert_eq!(bits3(first.positions[i]), bits3(third.positions[i]), "position {i}");
        assert_eq!(bits3(first.normals[i]), bits3(third.normals[i]), "normal {i}");
    }
}

#[test]
fn normals_stay_unit_length_with_modifiers() {
    let cases: [(f32, f32, &[u16], &[f32]); 2] = [
        (1.2, 0.8, &[3929], &[1.0]),
        (0.9, 1.1, &[3901, 3929], &[1.0, 1.0]),
    ];
    for (height, weight, ids, weights) in cases {
        let out = generate(height, weight, ids, weights);
        for (i, nrm) in out.normals.iter().enumerate() {
            assert!(nrm.iter().all(|c| c.is_finite()), "vertex {i} normal {nrm:?} not finite");
            let len = ((nrm[0] as f64).powi(2) + (nrm[1] as f64).powi(2) + (nrm[2] as f64).powi(2)).sqrt();
            assert!(
                (len - 1.0).abs() <= 1e-4,
                "h{height} w{weight} {ids:?}: vertex {i} normal length {len}"
            );
        }
    }
}
