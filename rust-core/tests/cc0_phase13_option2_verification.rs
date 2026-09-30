//! CC0-Phase 13, Option 2 — verification against real `essentials.afpp`
//! data.
//!
//! Reruns CC0-Phase 12's own elbow-bend displacement check (§3c in
//! CC0_PHASE_12_POSE_DIAGNOSIS_RESULTS.md) for the baby character, but
//! now with the scaled bind pose (this phase's fix) applied to the fixed
//! global skeleton before posing, and reports the real before/after
//! numbers side by side. Also re-checks the adult character doesn't
//! regress, and measures the real per-call cost of the Rust-side fit.
//!
//! This test deliberately reimplements a small amount of forward-
//! kinematics / linear-blend-skinning math rather than depending on a
//! new math crate (none is a dependency of this crate) or exposing
//! internal engine functions — it plays the same role as CC0-Phase 12's
//! own throwaway diagnosis code (a numerical reconstruction of what
//! `packages/web-three`'s `toSkinnedMesh` does in TS, since no browser
//! is available in this environment either). The pivot-anchored scaling
//! applied to the bind pose here matches exactly what
//! `packages/web-three/src/index.ts`'s `toSkinnedMesh` now does (see
//! that file): scale every joint's WORLD bind position about
//! `rest_pivot` by `rest_scale`, keep rotations unchanged, then recover
//! each non-root joint's new LOCAL translation from its (unchanged)
//! parent world rotation. Applying the naive "scale local translation
//! directly, pivot and all" version the task doc's one-line formula
//! reads as (if taken completely literally, ignoring hierarchy) is
//! wrong for any non-root bone -- see the CC0-Phase 13 delivery notes.

use anthroforge_core::{generate_character, get_skeleton, init_part_registry_from_pack, CharacterDNA, FfiJoint};
use std::path::PathBuf;
use std::time::Instant;

// ---------------------------------------------------------------------
// Minimal Vec3 / quaternion math (x, y, z, w order, matching FfiJoint's
// documented rotation layout and three.js's Quaternion.fromArray).
// ---------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct Vec3 {
    x: f32,
    y: f32,
    z: f32,
}
impl Vec3 {
    fn new(a: [f32; 3]) -> Self {
        Vec3 { x: a[0], y: a[1], z: a[2] }
    }
    fn add(self, o: Vec3) -> Vec3 {
        Vec3 { x: self.x + o.x, y: self.y + o.y, z: self.z + o.z }
    }
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3 { x: self.x - o.x, y: self.y - o.y, z: self.z - o.z }
    }
    fn len(self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }
}

#[derive(Clone, Copy, Debug)]
struct Quat {
    x: f32,
    y: f32,
    z: f32,
    w: f32,
}
impl Quat {
    fn new(a: [f32; 4]) -> Self {
        Quat { x: a[0], y: a[1], z: a[2], w: a[3] }
    }
    /// Standard quaternion-vector rotation: q * v * q^-1, optimized form.
    fn rotate(self, v: Vec3) -> Vec3 {
        let q = self;
        let uv_x = q.y * v.z - q.z * v.y;
        let uv_y = q.z * v.x - q.x * v.z;
        let uv_z = q.x * v.y - q.y * v.x;
        let uuv_x = q.y * uv_z - q.z * uv_y;
        let uuv_y = q.z * uv_x - q.x * uv_z;
        let uuv_z = q.x * uv_y - q.y * uv_x;
        Vec3 {
            x: v.x + 2.0 * (q.w * uv_x + uuv_x),
            y: v.y + 2.0 * (q.w * uv_y + uuv_y),
            z: v.z + 2.0 * (q.w * uv_z + uuv_z),
        }
    }
    /// Hamilton product self * other.
    fn mul(self, o: Quat) -> Quat {
        Quat {
            w: self.w * o.w - self.x * o.x - self.y * o.y - self.z * o.z,
            x: self.w * o.x + self.x * o.w + self.y * o.z - self.z * o.y,
            y: self.w * o.y - self.x * o.z + self.y * o.w + self.z * o.x,
            z: self.w * o.z + self.x * o.y - self.y * o.x + self.z * o.w,
        }
    }
    fn conjugate(self) -> Quat {
        Quat { x: -self.x, y: -self.y, z: -self.z, w: self.w }
    }
    /// Rotation about the local Z axis by `angle_rad`.
    fn from_z_rotation(angle_rad: f32) -> Quat {
        Quat { x: 0.0, y: 0.0, z: (angle_rad / 2.0).sin(), w: (angle_rad / 2.0).cos() }
    }
}

/// One joint's bind-pose local transform, decoupled from `FfiJoint` so
/// this file can freely construct "scaled" copies.
#[derive(Clone, Copy)]
struct LocalJoint {
    parent_index: i32,
    translation: Vec3,
    rotation: Quat,
}

/// Forward kinematics: world (position, rotation) per joint, assuming
/// `joints[i].parent_index < i` for every non-root joint (verified true
/// for the real skeleton by CC0-Phase 12's own diagnosis).
fn forward_kinematics(joints: &[LocalJoint]) -> Vec<(Vec3, Quat)> {
    let mut world: Vec<Option<(Vec3, Quat)>> = vec![None; joints.len()];
    for (i, j) in joints.iter().enumerate() {
        if j.parent_index == -1 {
            world[i] = Some((j.translation, j.rotation));
        } else {
            let (pp, pr) = world[j.parent_index as usize]
                .expect("parent must be computed before child (parent_index < i)");
            let wp = pp.add(pr.rotate(j.translation));
            let wr = pr.mul(j.rotation);
            world[i] = Some((wp, wr));
        }
    }
    world.into_iter().map(|x| x.unwrap()).collect()
}

/// CC0-Phase 13 Option 2's bind-pose scaling, applied to the WHOLE
/// hierarchy: matches `toSkinnedMesh`'s TS implementation exactly (see
/// its doc comment). Returns new `LocalJoint`s (rotations unchanged).
fn scale_bind_pose(joints: &[LocalJoint], scale: [f32; 3], pivot: [f32; 3]) -> Vec<LocalJoint> {
    let old_world = forward_kinematics(joints);

    let scale_pt = |p: Vec3| -> Vec3 {
        Vec3 {
            x: pivot[0] + scale[0] * (p.x - pivot[0]),
            y: pivot[1] + scale[1] * (p.y - pivot[1]),
            z: pivot[2] + scale[2] * (p.z - pivot[2]),
        }
    };
    let new_world_pos: Vec<Vec3> = old_world.iter().map(|(p, _)| scale_pt(*p)).collect();

    joints
        .iter()
        .enumerate()
        .map(|(i, j)| {
            let new_translation = if j.parent_index == -1 {
                new_world_pos[i]
            } else {
                let parent_idx = j.parent_index as usize;
                let (_, parent_old_rot) = old_world[parent_idx];
                let delta_world = new_world_pos[i].sub(new_world_pos[parent_idx]);
                parent_old_rot.conjugate().rotate(delta_world)
            };
            LocalJoint {
                parent_index: j.parent_index,
                translation: new_translation,
                rotation: j.rotation, // unchanged
            }
        })
        .collect()
}

fn load_joints() -> Vec<LocalJoint> {
    let skel_ptr = get_skeleton();
    assert!(!skel_ptr.is_null(), "get_skeleton must succeed");
    let skel = unsafe { &*skel_ptr };
    let raw: &[FfiJoint] = unsafe { std::slice::from_raw_parts(skel.joints_ptr, skel.joint_count as usize) };
    raw.iter()
        .map(|j| LocalJoint {
            parent_index: j.parent_index,
            translation: Vec3::new(j.translation),
            rotation: Quat::new(j.rotation),
        })
        .collect()
}

struct GeneratedMesh {
    positions: Vec<[f32; 3]>,
    bone_indices: Vec<[u16; 4]>,
    bone_weights: Vec<[f32; 4]>,
    rest_scale: [f32; 3],
    rest_pivot: [f32; 3],
    vertex_count: usize,
}

fn generate(morph_id: u16) -> GeneratedMesh {
    let ids = [morph_id];
    let weights = [1.0f32];
    let dna = CharacterDNA {
        seed: 1,
        height_modifier: 1.0,
        weight_modifier: 1.0,
        head_id: 4001,
        torso_id: 4002,
        equipped_clothing_ids_ptr: std::ptr::null(),
        equipped_clothing_count: 0,
        arms_id: 4003,
        legs_id: 4004,
        active_morph_ids_ptr: ids.as_ptr(),
        active_morph_weights_ptr: weights.as_ptr(),
        active_morph_count: 1,
    };

    let start = Instant::now();
    let out_ptr = generate_character(&dna as *const CharacterDNA);
    let elapsed = start.elapsed();
    assert!(!out_ptr.is_null(), "generate_character must succeed for morph {morph_id}");
    let out = unsafe { &*out_ptr };
    println!(
        "  generate_character(morph {morph_id}) took {:.3} ms ({} vertices)",
        elapsed.as_secs_f64() * 1000.0,
        out.vertices_count
    );

    let verts = unsafe { std::slice::from_raw_parts(out.vertices_ptr, out.vertices_count as usize) };
    let positions = verts.iter().map(|v| v.position).collect();
    let bone_indices = verts.iter().map(|v| v.bone_indices).collect();
    let bone_weights = verts.iter().map(|v| v.bone_weights).collect();

    GeneratedMesh {
        positions,
        bone_indices,
        bone_weights,
        rest_scale: out.rest_scale,
        rest_pivot: out.rest_pivot,
        vertex_count: out.vertices_count as usize,
    }
}

/// Reimplements CC0-Phase 12 §3c: bend bone `bend_bone` by `angle_rad`
/// about its local Z axis, hold every other joint at bind pose, and
/// return mean/max `|v_posed - v_bind|` over vertices with >0.9 combined
/// weight concentrated on that single bone (matches "the 1,448 vertices
/// with nonzero weight on that bone" methodology -- this reimplementation
/// uses the same >0.9-single-bone-weight selection Phase 12's §3a used,
/// which is the same real subset for a bend that should only visibly
/// affect that bone's own skin).
fn elbow_bend_displacement(
    mesh: &GeneratedMesh,
    bind_joints: &[LocalJoint],
    bend_bone: usize,
    angle_rad: f32,
) -> (f32, f32, usize) {
    let bind_world = forward_kinematics(bind_joints);

    // Posed world: identical to bind world except `bend_bone` gets an
    // extra local Z rotation composed in before FK propagates to its
    // descendants.
    let mut posed_joints = bind_joints.to_vec();
    posed_joints[bend_bone].rotation = posed_joints[bend_bone].rotation.mul(Quat::from_z_rotation(angle_rad));
    let posed_world = forward_kinematics(&posed_joints);

    // Precompute each bone's inverse bind matrix as (inv_rot, inv_translate-equivalent):
    // v_bind_space = inv_rot_k * (v_world - bind_pos_k)
    // v_posed = pose_pos_k + pose_rot_k * v_bind_space
    let mut total_disp = 0.0f64;
    let mut max_disp = 0.0f32;
    let mut n = 0usize;

    for i in 0..mesh.vertex_count {
        let bi = mesh.bone_indices[i];
        let bw = mesh.bone_weights[i];
        // Find if this vertex has >0.9 weight concentrated on `bend_bone`.
        let mut w_on_bend_bone = 0.0f32;
        for k in 0..4 {
            if bi[k] as usize == bend_bone {
                w_on_bend_bone += bw[k];
            }
        }
        if w_on_bend_bone <= 0.9 {
            continue;
        }

        let v_world = Vec3::new(mesh.positions[i]);

        // Linear blend skinning across this vertex's actual (up to 4)
        // influencing bones, exactly as three.js's SkinnedMesh does.
        let mut v_posed = Vec3 { x: 0.0, y: 0.0, z: 0.0 };
        let mut weight_sum = 0.0f32;
        for k in 0..4 {
            let w = bw[k];
            if w <= 0.0 {
                continue;
            }
            let bone = bi[k] as usize;
            let (bind_pos, bind_rot) = bind_world[bone];
            let (pose_pos, pose_rot) = posed_world[bone];
            let v_bind_space = bind_rot.conjugate().rotate(v_world.sub(bind_pos));
            let v_k = pose_pos.add(pose_rot.rotate(v_bind_space));
            v_posed = v_posed.add(Vec3 { x: v_k.x * w, y: v_k.y * w, z: v_k.z * w });
            weight_sum += w;
        }
        // Sanity, matching Phase 12: weight sums should be ~1.0.
        debug_assert!((weight_sum - 1.0).abs() < 0.01, "weight sum {weight_sum} far from 1.0");

        let disp = v_posed.sub(v_world).len();
        total_disp += disp as f64;
        max_disp = max_disp.max(disp);
        n += 1;
    }

    let mean = if n > 0 { (total_disp / n as f64) as f32 } else { 0.0 };
    (mean, max_disp, n)
}

fn bbox_height(positions: &[[f32; 3]]) -> f32 {
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for p in positions {
        min_y = min_y.min(p[1]);
        max_y = max_y.max(p[1]);
    }
    max_y - min_y
}

const LOWERARM01_L: usize = 52; // per CC0-Phase 12 §3a/§3c.

#[test]
fn cc0_phase13_option2_verification() {
    let pack_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("packs/essentials.afpp");
    let pack_bytes = std::fs::read(&pack_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", pack_path.display()));
    let ok = init_part_registry_from_pack(pack_bytes.as_ptr(), pack_bytes.len());
    assert!(ok, "init_part_registry_from_pack must succeed");

    let bind_joints = load_joints();
    assert_eq!(bind_joints.len(), 163, "expected the real 163-joint skeleton");

    println!("\n=== CC0-Phase 13 Option 2 verification (real essentials.afpp) ===\n");

    println!("-- generate_character timing / rest_scale,rest_pivot --");
    let baby = generate(1020);
    let adult = generate(1023);

    println!(
        "  baby:  rest_scale = {:?}, rest_pivot = {:?}, bbox height = {:.3}",
        baby.rest_scale,
        baby.rest_pivot,
        bbox_height(&baby.positions)
    );
    println!(
        "  adult: rest_scale = {:?}, rest_pivot = {:?}, bbox height = {:.3}",
        adult.rest_scale,
        adult.rest_pivot,
        bbox_height(&adult.positions)
    );

    // Cross-check against CC0-Phase 12 §3b's baby-vs-adult-corner fit
    // (k≈0.32-0.35 on that pairwise comparison). This phase's fit is a
    // DIFFERENT comparison (this character's own pre- vs. post-morph
    // vertices, not baby-vs-adult), so an exact match isn't expected,
    // but the same ballpark for the baby corner is a sanity check that
    // nothing is wildly broken.
    for axis in 0..3 {
        assert!(
            baby.rest_scale[axis] > 0.05 && baby.rest_scale[axis] < 0.9,
            "baby rest_scale[{axis}] = {} is outside the sane range for a shrink-morph fit",
            baby.rest_scale[axis]
        );
    }

    println!("\n-- scaling the bind pose per character --");
    let baby_bind = scale_bind_pose(&bind_joints, baby.rest_scale, baby.rest_pivot);
    let adult_bind = scale_bind_pose(&bind_joints, adult.rest_scale, adult.rest_pivot);

    // Sanity: root joint's NEW world Y should be pivot[1] + k*(old - pivot).
    let old_world = forward_kinematics(&bind_joints);
    let new_baby_world = forward_kinematics(&baby_bind);
    println!(
        "  baby root: old world = {:?}, new (scaled) world = {:?}",
        old_world[0].0, new_baby_world[0].0
    );

    println!("\n-- elbow-bend displacement, BEFORE (fixed/unscaled bind pose) --");
    for angle_deg in [15.0f32, 30.0, 60.0] {
        let angle_rad = angle_deg.to_radians();
        let (mean_baby, max_baby, n_baby) =
            elbow_bend_displacement(&baby, &bind_joints, LOWERARM01_L, angle_rad);
        let baby_height = bbox_height(&baby.positions);
        let (mean_adult, _max_adult, _n_adult) =
            elbow_bend_displacement(&adult, &bind_joints, LOWERARM01_L, angle_rad);
        let adult_height = bbox_height(&adult.positions);
        println!(
            "  {angle_deg}°  baby: mean={:.3} ({:.1}% height, n={n_baby}, max={:.3})   adult: mean={:.3} ({:.1}% height)",
            mean_baby, 100.0 * mean_baby / baby_height, max_baby,
            mean_adult, 100.0 * mean_adult / adult_height
        );
    }

    println!("\n-- elbow-bend displacement, AFTER (CC0-Phase 13 Option 2 scaled bind pose) --");
    for angle_deg in [15.0f32, 30.0, 60.0] {
        let angle_rad = angle_deg.to_radians();
        let (mean_baby, max_baby, n_baby) =
            elbow_bend_displacement(&baby, &baby_bind, LOWERARM01_L, angle_rad);
        let baby_height = bbox_height(&baby.positions);
        let (mean_adult, _max_adult, _n_adult) =
            elbow_bend_displacement(&adult, &adult_bind, LOWERARM01_L, angle_rad);
        let adult_height = bbox_height(&adult.positions);
        println!(
            "  {angle_deg}°  baby: mean={:.3} ({:.1}% height, n={n_baby}, max={:.3})   adult: mean={:.3} ({:.1}% height)",
            mean_baby, 100.0 * mean_baby / baby_height, max_baby,
            mean_adult, 100.0 * mean_adult / adult_height
        );
    }

    // Real assertion, not just printed numbers: the 60-degree baby
    // displacement must have dropped substantially (order-of-magnitude,
    // matching the diagnosis's "roughly 10-13x amplification" framing) --
    // not merely "not worse".
    let (mean_before, _, _) = elbow_bend_displacement(&baby, &bind_joints, LOWERARM01_L, 60f32.to_radians());
    let (mean_after, _, _) = elbow_bend_displacement(&baby, &baby_bind, LOWERARM01_L, 60f32.to_radians());
    println!("\n  60° baby mean displacement: before={mean_before:.3}, after={mean_after:.3}, ratio={:.2}x", mean_before / mean_after);
    assert!(
        mean_after < mean_before / 3.0,
        "expected the scaled bind pose to cut 60-degree baby displacement by at least 3x, got before={mean_before} after={mean_after}"
    );

    println!("\n-- fit cost measurement (Rust-side, full ~53k vertices, baby morph) --");
    let mut durations = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        let out_ptr = generate_character(&CharacterDNA {
            seed: 1,
            height_modifier: 1.0,
            weight_modifier: 1.0,
            head_id: 4001,
            torso_id: 4002,
            equipped_clothing_ids_ptr: std::ptr::null(),
            equipped_clothing_count: 0,
            arms_id: 4003,
            legs_id: 4004,
            active_morph_ids_ptr: [1020u16].as_ptr(),
            active_morph_weights_ptr: [1.0f32].as_ptr(),
            active_morph_count: 1,
        } as *const CharacterDNA);
        let elapsed = start.elapsed();
        assert!(!out_ptr.is_null());
        durations.push(elapsed.as_secs_f64() * 1000.0);
    }
    println!("  5 runs (ms, full generate_character incl. fit): {:?}", durations);
    println!("  mean: {:.3} ms", durations.iter().sum::<f64>() / durations.len() as f64);
}
