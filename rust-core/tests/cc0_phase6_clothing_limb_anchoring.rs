//! CC0-Phase 6 regression: clothing near the arms/legs must bind to limb
//! vertices, not to the nearest head/torso vertex.
//!
//! `fit_clothing_to_skin` copies the anchor skin vertex's `bone_indices` /
//! `bone_weights` onto each garment vertex (rigid-bind inheritance). Before
//! this phase the clothing skin tree was built from head+torso only, so a
//! sleeve or trouser vertex inherited *torso* bone data instead of the
//! arm/leg bone it sits on -- on the real 4-part body, a garment vertex
//! ~0.05 units from an arm vertex (bones `[69,0,0,0]`) inherited
//! `[1,42,41,2]` from a torso vertex ~3.9 units away.
//!
//! Method: real vertex positions/normals are read straight from the
//! checked-in fixture GLBs (`tests/fixtures/cc0_phase3_pipeline/`), one
//! probe point per region is placed 0.03 units off that real surface along
//! its own normal, a one-part garment (3 tiny triangles) is added to a copy
//! of the fixture dir, a real pack is built with the real `pack_builder`,
//! and the real `generate_character` is run with the garment equipped
//! (identity scale, no morphs). Each garment vertex must then carry bone
//! data identical to some vertex of the intended body region within
//! `INHERIT_RADIUS`.
//!
//! The registry is a process-wide `OnceLock`, so the pack is built and
//! initialized once and the single generated output is shared by all tests.
//! No base-mesh argument is given to `pack_builder` (no duplication maps):
//! nothing here uses morphs, and a garment is not a subset of the body's
//! base mesh.

use anthroforge_core::{
    free_mesh_buffer, generate_character, init_part_registry_from_pack, CharacterDNA, SkinnedVertex,
};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

const CLOTHING_ID: u32 = 6001;
/// How far off the real body surface (along its normal) each probe sits.
const PROBE_OFFSET: f32 = 0.03;
/// A probe must be at least this close to a vertex of its intended region
/// (guards the test setup itself).
const SETUP_RADIUS: f32 = 0.5;
/// A garment vertex must share exact bone data with a vertex of its
/// intended region within this distance. The pre-fix failure distance was
/// ~3.9 units, so this is loose on the passing side and far below it.
const INHERIT_RADIUS: f32 = 1.0;

struct Part {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
}

fn read_part(path: &Path) -> Part {
    let gltf = gltf::Gltf::open(path).unwrap_or_else(|e| panic!("open {path:?}: {e}"));
    let blob = gltf.blob.as_deref();
    let (mut positions, mut normals) = (Vec::new(), Vec::new());
    for mesh in gltf.document.meshes() {
        for prim in mesh.primitives() {
            let reader = prim.reader(|_| blob);
            let pos: Vec<[f32; 3]> = reader.read_positions().expect("POSITION").collect();
            let nrm: Vec<[f32; 3]> = match reader.read_normals() {
                Some(it) => it.collect(),
                None => vec![[0.0; 3]; pos.len()],
            };
            positions.extend(pos);
            normals.extend(nrm);
        }
    }
    Part { positions, normals }
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// A point `PROBE_OFFSET` off the real surface vertex `i`, along its normal
/// (falling back to the radial xz direction if the normal is degenerate).
fn probe_point(part: &Part, i: usize) -> [f32; 3] {
    let p = part.positions[i];
    let mut n = part.normals[i];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len < 1e-6 {
        n = [p[0], 0.0, p[2]];
    }
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
    [
        p[0] + PROBE_OFFSET * n[0] / len,
        p[1] + PROBE_OFFSET * n[1] / len,
        p[2] + PROBE_OFFSET * n[2] / len,
    ]
}

fn argmax_by(part: &Part, key: impl Fn(&[f32; 3]) -> f32) -> usize {
    (0..part.positions.len())
        .max_by(|&a, &b| key(&part.positions[a]).partial_cmp(&key(&part.positions[b])).unwrap())
        .unwrap()
}

fn median_y_index(part: &Part) -> usize {
    let mut idx: Vec<usize> = (0..part.positions.len()).collect();
    idx.sort_by(|&a, &b| part.positions[a][1].partial_cmp(&part.positions[b][1]).unwrap());
    idx[idx.len() / 2]
}

struct Output {
    body: Vec<SkinnedVertex>,
    cloth: Vec<SkinnedVertex>,
    torso: Range<usize>,
    arms: Range<usize>,
    legs: Range<usize>,
}

fn build_output() -> Output {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fx = root.join("tests/fixtures/cc0_phase3_pipeline");
    let head = read_part(&fx.join("4001_head.glb"));
    let torso = read_part(&fx.join("4002_torso.glb"));
    let arms = read_part(&fx.join("4003_arms.glb"));
    let legs = read_part(&fx.join("4004_legs.glb"));

    // Probes: [arm, leg, torso], in that order.
    let probes = [
        probe_point(&arms, argmax_by(&arms, |p| p[0])),   // hand end of the arm
        probe_point(&legs, median_y_index(&legs)),        // mid-thigh/leg
        probe_point(&torso, argmax_by(&torso, |p| p[2])), // front-most torso point
    ];

    // Temp copy of the fixture dir + a 3-triangle garment part.
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cc0_phase6_limb_anchoring_src");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for e in std::fs::read_dir(&fx).unwrap() {
        let e = e.unwrap();
        std::fs::copy(e.path(), dir.join(e.file_name())).unwrap();
    }
    let mut obj = String::new();
    for p in &probes {
        for (dx, dy) in [(0.0, 0.0), (0.02, 0.0), (0.0, 0.02)] {
            obj.push_str(&format!("v {} {} {}\n", p[0] + dx, p[1] + dy, p[2]));
        }
    }
    obj.push_str("f 1 2 3\nf 4 5 6\nf 7 8 9\n");
    std::fs::write(dir.join(format!("{CLOTHING_ID}_limb_probe_garment.obj")), obj).unwrap();

    let afpp = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cc0_phase6_limb_anchoring.afpp");
    let status = Command::new(env!("CARGO_BIN_EXE_pack_builder"))
        .arg(&dir)
        .arg(&afpp)
        .status()
        .expect("run pack_builder");
    assert!(status.success(), "pack_builder failed");
    let bytes = std::fs::read(&afpp).unwrap();
    assert!(init_part_registry_from_pack(bytes.as_ptr(), bytes.len()), "registry init failed");

    let ids = [CLOTHING_ID];
    let dna = CharacterDNA {
        seed: 42,
        height_modifier: 1.0,
        weight_modifier: 1.0,
        head_id: 4001,
        torso_id: 4002,
        arms_id: 4003,
        legs_id: 4004,
        equipped_clothing_ids_ptr: ids.as_ptr(),
        equipped_clothing_count: 1,
        active_morph_ids_ptr: std::ptr::null(),
        active_morph_weights_ptr: std::ptr::null(),
        active_morph_count: 0,
    };
    let buf = generate_character(&dna as *const CharacterDNA);
    assert!(!buf.is_null(), "generate_character returned null");
    let all: Vec<SkinnedVertex> = unsafe {
        let b = &*buf;
        std::slice::from_raw_parts(b.vertices_ptr, b.vertices_count as usize).to_vec()
    };
    free_mesh_buffer(buf);

    // Output layout: [head | torso | arms | legs | garment].
    let torso_start = head.positions.len();
    let arms_start = torso_start + torso.positions.len();
    let legs_start = arms_start + arms.positions.len();
    let body_end = legs_start + legs.positions.len();
    assert_eq!(all.len(), body_end + 9, "expected 4 body parts + 9 garment vertices");
    Output {
        cloth: all[body_end..].to_vec(),
        body: all[..body_end].to_vec(),
        torso: torso_start..arms_start,
        arms: arms_start..legs_start,
        legs: legs_start..body_end,
    }
}

fn output() -> &'static Output {
    static OUT: OnceLock<Output> = OnceLock::new();
    OUT.get_or_init(build_output)
}

fn nearest(body: &[SkinnedVertex], r: Range<usize>, p: [f32; 3]) -> f32 {
    r.map(|i| dist(body[i].position, p)).fold(f32::INFINITY, f32::min)
}

/// Every garment vertex in `cloth[probe*3 .. probe*3+3]` must (a) be closer
/// to `region` than to the rest of the body (test-setup guard), and (b)
/// carry bone data identical to some `region` vertex within `INHERIT_RADIUS`.
fn check_probe(label: &str, probe: usize, region: Range<usize>, others: [Range<usize>; 2]) {
    let o = output();
    for k in 0..3 {
        let c = &o.cloth[probe * 3 + k];
        let d_region = nearest(&o.body, region.clone(), c.position);
        let d_other = others
            .iter()
            .map(|r| nearest(&o.body, r.clone(), c.position))
            .fold(f32::INFINITY, f32::min);
        assert!(
            d_region <= SETUP_RADIUS && d_region < d_other,
            "[{label} v{k}] test setup: garment vertex should sit on the {label} \
             (nearest {label} vertex {d_region}, nearest other-region vertex {d_other})"
        );
        let inherits = region.clone().any(|i| {
            let v = &o.body[i];
            dist(v.position, c.position) <= INHERIT_RADIUS
                && v.bone_indices == c.bone_indices
                && v.bone_weights == c.bone_weights
        });
        assert!(
            inherits,
            "[{label} v{k}] garment vertex at {:?} carries bones {:?} weights {:?}, which no {label} \
             vertex within {INHERIT_RADIUS} units has -- it was anchored to the wrong body part \
             (nearest {label} vertex is {d_region} away)",
            c.position, c.bone_indices, c.bone_weights
        );
    }
}

#[test]
fn cc0_phase6_1_garment_on_arm_inherits_arm_bones() {
    let o = output();
    check_probe("arm", 0, o.arms.clone(), [0..o.torso.end, o.legs.clone()]);
}

#[test]
fn cc0_phase6_2_garment_on_leg_inherits_leg_bones() {
    let o = output();
    check_probe("leg", 1, o.legs.clone(), [0..o.torso.end, o.arms.clone()]);
}

#[test]
fn cc0_phase6_3_garment_on_torso_still_inherits_torso_bones() {
    // Guard against the four-part tree breaking the case that already worked.
    let o = output();
    check_probe("torso", 2, o.torso.clone(), [o.arms.clone(), o.legs.clone()]);
}
