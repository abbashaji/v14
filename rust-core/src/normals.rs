//! Carries the geometric change of a morph into the vertex normals.
//!
//! Morph targets in the packs hold a zero `normal_delta`, so a morphed body
//! keeps the normals of the unmorphed mesh while its surface moves.
//! [`transfer_morph_normals`] computes area-weighted vertex normals for the
//! unmorphed and for the morphed body (coincident positions welded), and
//! rotates each authored normal by the rotation that takes the unmorphed
//! geometric normal to the morphed one.
//!
//! Authored normals are rotated, never replaced: they are not smooth. On the
//! measurement-fit pack the welded area-weighted normals differ from the
//! authored ones by a median of 10.1 degrees (max 102.7), so replacing them
//! would change about 99.6% of all vertices on every morphed body. Rotating
//! keeps vertices whose surroundings did not move bit-identical to the
//! unmorphed output.

use crate::SkinnedVertex;

type Vec3 = [f64; 3];

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: Vec3, s: f64) -> Vec3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn widen(p: [f32; 3]) -> Vec3 {
    [p[0] as f64, p[1] as f64, p[2] as f64]
}

/// Bit-pattern key of a position, with `-0.0` mapped to `+0.0`.
fn weld_key(p: [f32; 3]) -> [u32; 3] {
    let bits = |x: f32| if x == 0.0 { 0u32 } else { x.to_bits() };
    [bits(p[0]), bits(p[1]), bits(p[2])]
}

/// One group id per vertex; vertices whose position has the identical bit
/// pattern share a group. Returns the ids and the number of groups. Sorting
/// by (key, index) keeps the result independent of any iteration order.
fn weld_groups(positions: &[[f32; 3]]) -> (Vec<usize>, usize) {
    let mut keyed: Vec<([u32; 3], usize)> = positions
        .iter()
        .enumerate()
        .map(|(i, &p)| (weld_key(p), i))
        .collect();
    keyed.sort_unstable_by_key(|&(key, i)| (key, i));

    let mut group_of = vec![0usize; positions.len()];
    let mut group_count = 0usize;
    let mut previous: Option<[u32; 3]> = None;
    for &(key, i) in &keyed {
        if previous != Some(key) {
            if previous.is_some() {
                group_count += 1;
            }
            previous = Some(key);
        }
        if let Some(slot) = group_of.get_mut(i) {
            *slot = group_count;
        }
    }
    if previous.is_some() {
        group_count += 1;
    }
    (group_of, group_count)
}

/// Sum of unnormalized triangle normals (`cross(b - a, c - a)`, so larger
/// triangles weigh more) per weld group, in `f64`. Triples with an index
/// outside `positions` are skipped.
fn accumulate(
    positions: &[[f32; 3]],
    indices: &[u32],
    group_of: &[usize],
    group_count: usize,
) -> Vec<Vec3> {
    let mut acc = vec![[0.0f64; 3]; group_count];
    let n = positions.len();
    for tri in indices.chunks_exact(3) {
        let (Some(&i0), Some(&i1), Some(&i2)) = (tri.first(), tri.get(1), tri.get(2)) else {
            continue;
        };
        let (i0, i1, i2) = (i0 as usize, i1 as usize, i2 as usize);
        if i0 >= n || i1 >= n || i2 >= n {
            continue;
        }
        let (Some(&a), Some(&b), Some(&c)) = (positions.get(i0), positions.get(i1), positions.get(i2))
        else {
            continue;
        };
        let (a, b, c) = (widen(a), widen(b), widen(c));
        let face = cross(sub(b, a), sub(c, a));
        for corner in [i0, i1, i2] {
            let Some(&g) = group_of.get(corner) else {
                continue;
            };
            if let Some(slot) = acc.get_mut(g) {
                *slot = add(*slot, face);
            }
        }
    }
    acc
}

/// Normalizes `v`; `None` if its length is not finite or is `<= 1e-12`.
fn normalized(v: Vec3) -> Option<Vec3> {
    let len = dot(v, v).sqrt();
    if !len.is_finite() || len <= 1e-12 {
        return None;
    }
    Some(scale(v, 1.0 / len))
}

/// Rotation (as `k = nb x nm`, `c = nb . nm`) taking the unmorphed group
/// normal to the morphed one, or `None` when the group keeps its authored
/// normals (degenerate, unchanged, or flipped).
fn group_rotation(base: Vec3, morphed: Vec3) -> Option<(Vec3, f64)> {
    let nb = normalized(base)?;
    let nm = normalized(morphed)?;
    if nb == nm {
        return None;
    }
    let c = dot(nb, nm);
    if c <= -1.0 + 1e-6 {
        return None;
    }
    Some((cross(nb, nm), c))
}

/// Rotates the authored normal of every vertex by the change in its welded,
/// area-weighted geometric normal between the unmorphed and morphed mesh.
///
/// `base_positions[i]` is the unmorphed position of `vertices[i]`;
/// `vertices[i].position` is the morphed position; `vertices[i].normal` is
/// the authored normal and is the only field written. Vertices whose weld
/// group is degenerate, unchanged, flipped, or non-finite keep their authored
/// normal. Never panics; a length mismatch or empty input is a no-op.
pub(crate) fn transfer_morph_normals(
    base_positions: &[[f32; 3]],
    indices: &[u32],
    vertices: &mut [SkinnedVertex],
) {
    if base_positions.len() != vertices.len() || vertices.is_empty() {
        return;
    }

    let (group_of, group_count) = weld_groups(base_positions);
    let morphed_positions: Vec<[f32; 3]> = vertices.iter().map(|v| v.position).collect();
    let base_acc = accumulate(base_positions, indices, &group_of, group_count);
    let morphed_acc = accumulate(&morphed_positions, indices, &group_of, group_count);

    let rotations: Vec<Option<(Vec3, f64)>> = base_acc
        .iter()
        .zip(morphed_acc.iter())
        .map(|(&b, &m)| group_rotation(b, m))
        .collect();

    for (vertex, &g) in vertices.iter_mut().zip(group_of.iter()) {
        let Some(&Some((k, c))) = rotations.get(g) else {
            continue;
        };
        let v = widen(vertex.normal);
        // Rodrigues form: v' = v + k x v + (k x (k x v)) / (1 + c).
        let kv = cross(k, v);
        let rotated = add(add(v, kv), scale(cross(k, kv), 1.0 / (1.0 + c)));
        if let Some(n) = normalized(rotated) {
            vertex.normal = [n[0] as f32, n[1] as f32, n[2] as f32];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vert(position: [f32; 3], normal: [f32; 3]) -> SkinnedVertex {
        SkinnedVertex {
            position,
            normal,
            uv: [0.0; 2],
            bone_indices: [0; 4],
            bone_weights: [0.0; 4],
        }
    }

    fn assert_close(actual: [f32; 3], expected: [f32; 3]) {
        for axis in 0..3 {
            assert!(
                (actual[axis] - expected[axis]).abs() <= 1e-5,
                "axis {axis}: actual {actual:?}, expected {expected:?}"
            );
        }
    }

    fn assert_bit_identical(actual: &[SkinnedVertex], authored: &[[f32; 3]]) {
        assert_eq!(actual.len(), authored.len());
        for (v, n) in actual.iter().zip(authored) {
            for axis in 0..3 {
                assert_eq!(
                    v.normal[axis].to_bits(),
                    n[axis].to_bits(),
                    "normal {:?} vs authored {:?}",
                    v.normal,
                    n
                );
            }
        }
    }

    const S: f32 = 0.866_025_4;

    fn plane_base() -> Vec<[f32; 3]> {
        vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]]
    }

    fn plane_authored() -> [[f32; 3]; 4] {
        [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.6, 0.0, 0.8]]
    }

    fn plane_vertices() -> Vec<SkinnedVertex> {
        let morphed = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, S, 0.5], [0.0, S, 0.5]];
        let authored = plane_authored();
        (0..4).map(|i| vert(morphed[i], authored[i])).collect()
    }

    const PLANE_INDICES: [u32; 6] = [0, 1, 2, 0, 2, 3];

    fn plane_expected() -> [[f32; 3]; 4] {
        [
            [0.0, -0.5, S],
            [1.0, 0.0, 0.0],
            [0.0, S, 0.5],
            [0.6, -0.4, 0.692_820_3],
        ]
    }

    fn fold_base() -> Vec<[f32; 3]> {
        vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
        ]
    }

    const FOLD_INDICES: [u32; 6] = [0, 1, 2, 3, 4, 5];

    fn fold_vertices(morphed4: [f32; 3], authored: [[f32; 3]; 6]) -> Vec<SkinnedVertex> {
        let mut positions = fold_base();
        positions[4] = morphed4;
        (0..6).map(|i| vert(positions[i], authored[i])).collect()
    }

    fn fold_authored() -> [[f32; 3]; 6] {
        let z = [0.0, 0.0, 1.0];
        let y = [0.0, 1.0, 0.0];
        [z, z, z, y, y, y]
    }

    #[test]
    fn tilted_plane_rotates_authored_normals_exactly() {
        let mut vertices = plane_vertices();
        transfer_morph_normals(&plane_base(), &PLANE_INDICES, &mut vertices);
        for (v, e) in vertices.iter().zip(plane_expected()) {
            assert_close(v.normal, e);
        }
    }

    #[test]
    fn fold_flattening_rotates_welded_twins_together() {
        let mut vertices = fold_vertices([0.0, -1.0, 0.0], fold_authored());
        transfer_morph_normals(&fold_base(), &FOLD_INDICES, &mut vertices);
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let expected = [
            [0.0, -h, h],
            [0.0, -h, h],
            [0.0, 0.0, 1.0],
            [0.0, h, h],
            [0.0, 0.0, 1.0],
            [0.0, h, h],
        ];
        for (v, e) in vertices.iter().zip(expected) {
            assert_close(v.normal, e);
        }
    }

    #[test]
    fn unchanged_positions_leave_normals_bit_identical() {
        let authored = [
            [0.3, -2.0, 5.0],
            [7.0, 0.1, 0.2],
            [0.0, 0.0, 3.5],
            [-1.5, 2.5, 0.25],
            [0.9, 0.9, 0.9],
            [-4.0, 0.0, 1.0],
        ];
        let mut vertices = fold_vertices([0.0, 0.0, 1.0], authored);
        transfer_morph_normals(&fold_base(), &FOLD_INDICES, &mut vertices);
        assert_bit_identical(&vertices, &authored);
    }

    #[test]
    fn degenerate_triangle_does_not_produce_nan() {
        let base = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        let morphed = [[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [2.0, 0.0, 1.0]];
        let authored = [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]];
        let mut vertices: Vec<SkinnedVertex> =
            (0..3).map(|i| vert(morphed[i], authored[i])).collect();
        transfer_morph_normals(&base, &[0, 1, 2], &mut vertices);
        for v in &vertices {
            assert!(v.normal.iter().all(|c| c.is_finite()));
        }
        assert_bit_identical(&vertices, &authored);
    }

    #[test]
    fn mismatched_inputs_are_a_no_op() {
        let authored = plane_authored();
        let mut vertices = plane_vertices();
        let short_base: Vec<[f32; 3]> = plane_base().into_iter().take(3).collect();
        transfer_morph_normals(&short_base, &PLANE_INDICES, &mut vertices);
        assert_bit_identical(&vertices, &authored);

        let indices = [0u32, 1, 2, 0, 2, 3, 0, 1, 9, 7];
        transfer_morph_normals(&plane_base(), &indices, &mut vertices);
        for (v, e) in vertices.iter().zip(plane_expected()) {
            assert_close(v.normal, e);
        }
    }

    #[test]
    fn antiparallel_flip_keeps_authored_normal() {
        let base = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let morphed = [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]];
        let authored = [[0.0, 0.0, 1.0], [0.6, 0.0, 0.8], [0.0, 1.0, 0.0]];
        let mut vertices: Vec<SkinnedVertex> =
            (0..3).map(|i| vert(morphed[i], authored[i])).collect();
        transfer_morph_normals(&base, &[0, 1, 2], &mut vertices);
        assert_bit_identical(&vertices, &authored);
    }

    #[test]
    fn non_finite_positions_keep_authored_normals() {
        let authored = fold_authored();
        let mut vertices = fold_vertices([f32::NAN; 3], authored);
        transfer_morph_normals(&fold_base(), &FOLD_INDICES, &mut vertices);
        assert_bit_identical(&vertices, &authored);
    }
}
