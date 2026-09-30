//! `morph_blend` — applies one or more loaded morph targets, each at its
//! own weight, onto a render-vertex buffer in place, via a
//! render-vertex -> base-mesh-vertex duplication map.

use std::fmt;

use crate::morph_loader::LoadedMorph;
use crate::SkinnedVertex;

/// All the ways applying morph targets to a vertex buffer can fail.
#[derive(Debug)]
pub enum MorphBlendError {
    /// `duplication_map.len()` didn't match `vertices.len()`.
    DuplicationMapLengthMismatch { vertices_len: usize, map_len: usize },
}

impl fmt::Display for MorphBlendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MorphBlendError::DuplicationMapLengthMismatch {
                vertices_len,
                map_len,
            } => write!(
                f,
                "duplication map length ({map_len}) does not match vertex buffer length ({vertices_len})"
            ),
        }
    }
}

/// Applies every `(morph, weight)` pair in `active_morphs`, in order, to
/// `vertices` in place.
///
/// For each vertex `i`:
///   - look up `duplication_map[i]` -- if it's
///     `crate::vertex_duplication_map::NO_BASE_MESH_MATCH`, skip this
///     vertex for every morph (nothing to blend against).
///   - otherwise, for each `(morph, weight)`, look up
///     `morph.delta_for_vertex(base_index)`. If present, add
///     `delta.position_delta * weight` to `vertices[i].position` and
///     `delta.normal_delta * weight` to `vertices[i].normal`. If absent,
///     that particular morph simply doesn't affect this vertex -- not an
///     error, not a partial skip of the whole vertex.
///
/// Normals are never renormalized here -- that's a rendering-layer
/// concern, out of scope for this function.
pub fn apply_morph_targets(
    vertices: &mut [SkinnedVertex],
    duplication_map: &[u32],
    active_morphs: &[(&LoadedMorph, f32)],
) -> Result<(), MorphBlendError> {
    if duplication_map.len() != vertices.len() {
        return Err(MorphBlendError::DuplicationMapLengthMismatch {
            vertices_len: vertices.len(),
            map_len: duplication_map.len(),
        });
    }

    for (i, vertex) in vertices.iter_mut().enumerate() {
        let base_index = duplication_map[i];
        if base_index == crate::vertex_duplication_map::NO_BASE_MESH_MATCH {
            continue;
        }

        for (morph, weight) in active_morphs {
            if let Some(delta) = morph.delta_for_vertex(base_index) {
                for axis in 0..3 {
                    vertex.position[axis] += delta.position_delta[axis] * weight;
                    vertex.normal[axis] += delta.normal_delta[axis] * weight;
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::morph_loader::MorphDelta;

    // Local stand-in for `crate::vertex_duplication_map::
    // NO_BASE_MESH_MATCH` for this test module's own compilation only.
    // The delivered, non-test code above references the real external
    // symbol -- this stub exists solely so this file's tests can compile
    // and run standalone, since `vertex_duplication_map` is built
    // elsewhere in this phase and isn't present in this context. Remove
    // if/when that module is actually present and this stub would
    // otherwise shadow it (it won't, this is scoped inside `mod tests`).
    #[allow(dead_code)]
    const NO_BASE_MESH_MATCH: u32 = u32::MAX;

    fn make_vertex(position: [f32; 3], normal: [f32; 3]) -> SkinnedVertex {
        SkinnedVertex {
            position,
            normal,
            uv: [0.0, 0.0],
            bone_indices: [0, 0, 0, 0],
            bone_weights: [1.0, 0.0, 0.0, 0.0],
        }
    }

    #[test]
    fn applies_single_morph_and_skips_no_match_vertex() {
        // 3 render vertices: 0 -> base 10, 1 -> base 20, 2 -> no match.
        let duplication_map = vec![10u32, 20u32, NO_BASE_MESH_MATCH];

        let morph = LoadedMorph::from_parts_for_test(
            1,
            vec![
                MorphDelta {
                    vertex_index: 10,
                    position_delta: [1.0, 0.0, 0.0],
                    normal_delta: [0.0, 1.0, 0.0],
                },
                MorphDelta {
                    vertex_index: 20,
                    position_delta: [0.0, 2.0, 0.0],
                    normal_delta: [0.0, 0.0, 1.0],
                },
            ],
        );

        let mut vertices = vec![
            make_vertex([0.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
            make_vertex([5.0, 5.0, 5.0], [1.0, 1.0, 1.0]),
            make_vertex([9.0, 9.0, 9.0], [2.0, 2.0, 2.0]),
        ];

        let weight = 0.5f32;
        apply_morph_targets(&mut vertices, &duplication_map, &[(&morph, weight)])
            .expect("well-formed inputs should not error");

        assert_eq!(vertices[0].position, [0.5, 0.0, 0.0]);
        assert_eq!(vertices[0].normal, [0.0, 0.5, 0.0]);

        assert_eq!(vertices[1].position, [5.0, 6.0, 5.0]);
        assert_eq!(vertices[1].normal, [1.0, 1.0, 1.5]);

        // Vertex 2's duplication-map entry is the sentinel -- untouched.
        assert_eq!(vertices[2].position, [9.0, 9.0, 9.0]);
        assert_eq!(vertices[2].normal, [2.0, 2.0, 2.0]);
    }

    #[test]
    fn blends_multiple_active_morphs_on_same_vertex() {
        let duplication_map = vec![10u32];

        let morph_a = LoadedMorph::from_parts_for_test(
            1,
            vec![MorphDelta {
                vertex_index: 10,
                position_delta: [1.0, 0.0, 0.0],
                normal_delta: [0.0, 0.0, 0.0],
            }],
        );
        let morph_b = LoadedMorph::from_parts_for_test(
            2,
            vec![MorphDelta {
                vertex_index: 10,
                position_delta: [0.0, 4.0, 0.0],
                normal_delta: [0.0, 0.0, 0.0],
            }],
        );

        let mut vertices = vec![make_vertex([0.0, 0.0, 0.0], [0.0, 0.0, 0.0])];

        apply_morph_targets(
            &mut vertices,
            &duplication_map,
            &[(&morph_a, 2.0), (&morph_b, 0.25)],
        )
        .expect("well-formed inputs should not error");

        // weight1*delta1 + weight2*delta2 = (2.0*[1,0,0]) + (0.25*[0,4,0])
        //                                  = [2,0,0] + [0,1,0] = [2,1,0]
        assert_eq!(vertices[0].position, [2.0, 1.0, 0.0]);
    }

    #[test]
    fn rejects_duplication_map_length_mismatch() {
        let duplication_map = vec![10u32, 20u32]; // 2 entries
        let mut vertices = vec![make_vertex([0.0, 0.0, 0.0], [0.0, 0.0, 0.0])]; // 1 vertex

        let morph = LoadedMorph::from_parts_for_test(1, vec![]);

        match apply_morph_targets(&mut vertices, &duplication_map, &[(&morph, 1.0)]) {
            Err(MorphBlendError::DuplicationMapLengthMismatch {
                vertices_len,
                map_len,
            }) => {
                assert_eq!(vertices_len, 1);
                assert_eq!(map_len, 2);
            }
            other => panic!("expected DuplicationMapLengthMismatch, got {other:?}"),
        }
    }
}
