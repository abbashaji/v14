//! `vertex_duplication_map` — render-vertex → base-mesh-vertex correspondence.
//!
//! Body-part meshes loaded by this crate from glTF/GLB files have real,
//! deliberate vertex duplication at UV seams: several render vertices can
//! share the same 3D position (e.g. the head part has 17,224 render
//! vertices but only 4,338 unique positions). A separate, sparse per-vertex
//! "morph delta" format (not implemented here) displaces vertices by an
//! original-source vertex index taken from the upstream MakeHuman base
//! mesh (`assets/upstream/base.obj`, 19,158 vertices). To apply a morph
//! delta to a render vertex, the render vertex's corresponding base-mesh
//! vertex index must first be known. [`build_duplication_map`] builds that
//! correspondence table by brute-force nearest-neighbor position matching.
//!
//! This module is intentionally standalone: it has no dependency on
//! `gltf_loader.rs`, `obj_loader.rs`, or any other module in this crate.
//! It is not declared in `lib.rs` yet; that wiring happens in a later
//! merge step, using this file as-is.

/// Sentinel value: no base-mesh vertex was found for this render vertex
/// within `tolerance`. Not expected to occur on real data (see the module
/// docs and the integration test below), but must be handled, not panicked
/// on.
pub const NO_BASE_MESH_MATCH: u32 = u32::MAX;

/// For each render vertex `i` in `render_positions`, the index into
/// `base_mesh_positions` of its nearest position match, or
/// `NO_BASE_MESH_MATCH` if the nearest match is farther than `tolerance`.
/// Returned `Vec` has exactly `render_positions.len()` entries, in the
/// same order.
///
/// Brute-force nearest-neighbor: for each render vertex, every base-mesh
/// vertex is checked and the closest (by squared distance, to avoid a
/// `sqrt` per comparison) is kept; `sqrt` is only taken once, on the
/// winning candidate, to compare against `tolerance`. This is O(render *
/// base) and is intended as an offline/pack-build-time step, not a
/// per-frame runtime path.
///
/// Ties (a base-mesh vertex exactly as close as the current best) are
/// broken by lower index, for deterministic output: since candidates are
/// scanned in increasing index order and only a strictly smaller squared
/// distance replaces the current best, the first (lowest-index) vertex
/// among any equidistant set is kept automatically.
pub fn build_duplication_map(
    render_positions: &[[f32; 3]],
    base_mesh_positions: &[[f32; 3]],
    tolerance: f32,
) -> Vec<u32> {
    let mut result = Vec::with_capacity(render_positions.len());

    for render_position in render_positions {
        let mut best_index: Option<usize> = None;
        let mut best_distance_squared = f32::INFINITY;

        for (base_index, base_position) in base_mesh_positions.iter().enumerate() {
            let dx = render_position[0] - base_position[0];
            let dy = render_position[1] - base_position[1];
            let dz = render_position[2] - base_position[2];
            let distance_squared = dx * dx + dy * dy + dz * dz;

            // Strictly less-than: on a tie, the earlier (lower-index)
            // candidate already stored is kept.
            if distance_squared < best_distance_squared {
                best_distance_squared = distance_squared;
                best_index = Some(base_index);
            }
        }

        let mapped = match best_index {
            Some(index) if best_distance_squared.sqrt() <= tolerance => {
                // `base_mesh_positions` came from a slice, so its length
                // (and thus every valid index into it) fits in a usize;
                // the only case this could exceed u32::MAX is a
                // base-mesh with more than 4 billion vertices, which is
                // not a real scenario for this crate's assets.
                u32::try_from(index).unwrap_or(NO_BASE_MESH_MATCH)
            }
            _ => NO_BASE_MESH_MATCH,
        };

        result.push(mapped);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_matches_including_duplicates_map_to_correct_base_index() {
        let base_mesh_positions = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [3.0, 0.0, 0.0],
        ];

        // Render positions simulate UV-seam duplication: base index 1
        // appears twice among the render vertices.
        let render_positions = [
            [2.0, 0.0, 0.0], // -> base 2
            [1.0, 0.0, 0.0], // -> base 1
            [1.0, 0.0, 0.0], // -> base 1 (duplicate)
            [0.0, 0.0, 0.0], // -> base 0
            [3.0, 0.0, 0.0], // -> base 3
        ];

        let result = build_duplication_map(&render_positions, &base_mesh_positions, 0.0005);

        assert_eq!(result, vec![2, 1, 1, 0, 3]);
        // Explicitly check the two duplicated render vertices landed on
        // the very same base index.
        assert_eq!(result[1], result[2]);
    }

    #[test]
    fn within_tolerance_but_not_exact_still_matches() {
        let base_mesh_positions = [[0.0, 0.0, 0.0], [5.0, 0.0, 0.0]];

        // Offset from base index 1 by less than the 0.0005 tolerance.
        let render_positions = [[5.0002, 0.0, 0.0]];

        let result = build_duplication_map(&render_positions, &base_mesh_positions, 0.0005);

        assert_eq!(result, vec![1]);
    }

    #[test]
    fn beyond_tolerance_maps_to_sentinel() {
        let base_mesh_positions = [[0.0, 0.0, 0.0], [5.0, 0.0, 0.0]];

        // 0.01 away from the nearest base vertex (index 1), well beyond
        // the 0.0005 tolerance.
        let render_positions = [[5.01, 0.0, 0.0]];

        let result = build_duplication_map(&render_positions, &base_mesh_positions, 0.0005);

        assert_eq!(result, vec![NO_BASE_MESH_MATCH]);
    }

    #[test]
    fn exact_tie_breaks_to_lower_index() {
        // Base vertices 0 and 2 are both exactly 1.0 away from the render
        // vertex at the origin; base vertex 1 is irrelevant filler.
        let base_mesh_positions = [
            [-1.0, 0.0, 0.0],
            [0.0, 100.0, 0.0],
            [1.0, 0.0, 0.0],
        ];

        let render_positions = [[0.0, 0.0, 0.0]];

        let result = build_duplication_map(&render_positions, &base_mesh_positions, 0.0005);

        // Both base 0 and base 2 are equidistant (distance 1.0, tolerance
        // is 0.0005 so this is a genuine tie *outside* tolerance — use a
        // large tolerance here to actually exercise the tie-break path).
        let result_with_large_tolerance =
            build_duplication_map(&render_positions, &base_mesh_positions, 10.0);
        assert_eq!(result_with_large_tolerance, vec![0]);
        // With the tiny tolerance neither is within range, so it's the
        // sentinel instead — sanity check the two calls aren't confused.
        assert_eq!(result, vec![NO_BASE_MESH_MATCH]);
    }

    #[test]
    fn empty_render_positions_returns_empty_vec() {
        let base_mesh_positions = [[0.0, 0.0, 0.0]];
        let render_positions: [[f32; 3]; 0] = [];

        let result = build_duplication_map(&render_positions, &base_mesh_positions, 0.0005);

        assert!(result.is_empty());
    }

    #[test]
    fn empty_base_mesh_positions_maps_everything_to_sentinel() {
        let base_mesh_positions: [[f32; 3]; 0] = [];
        let render_positions = [[0.0, 0.0, 0.0], [1.0, 2.0, 3.0]];

        let result = build_duplication_map(&render_positions, &base_mesh_positions, 0.0005);

        assert_eq!(result, vec![NO_BASE_MESH_MATCH, NO_BASE_MESH_MATCH]);
    }

    /// Minimal inline OBJ `v x y z` line reader, deliberately not sharing
    /// any code with `obj_loader.rs` — this module must stay standalone.
    fn read_obj_positions(path: &str) -> Vec<[f32; 3]> {
        let contents = std::fs::read_to_string(path).expect("failed to read base.obj fixture");
        let mut positions = Vec::new();

        for line in contents.lines() {
            let mut fields = line.split_whitespace();
            if fields.next() != Some("v") {
                continue;
            }
            let x: f32 = fields.next().expect("missing x").parse().expect("bad x");
            let y: f32 = fields.next().expect("missing y").parse().expect("bad y");
            let z: f32 = fields.next().expect("missing z").parse().expect("bad z");
            positions.push([x, y, z]);
        }

        positions
    }

    /// Minimal inline GLB reader that pulls out the `POSITION` accessor's
    /// raw `f32` triples, deliberately not sharing any code with
    /// `gltf_loader.rs` — this module must stay standalone. Only handles
    /// exactly what these fixtures need: a two-chunk GLB (JSON + BIN),
    /// with the POSITION accessor's bufferView tightly packed (no
    /// interleaving, no extra byteOffset/byteStride).
    fn read_glb_position_accessor(path: &str) -> Vec<[f32; 3]> {
        let bytes = std::fs::read(path).expect("failed to read glb fixture");

        // GLB header: magic(4) + version(4) + length(4) = 12 bytes.
        assert_eq!(&bytes[0..4], b"glTF", "not a GLB file");

        let mut offset = 12usize;
        let mut json_chunk: Option<&[u8]> = None;
        let mut bin_chunk: Option<&[u8]> = None;

        while offset + 8 <= bytes.len() {
            let chunk_length =
                u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            let chunk_type = &bytes[offset + 4..offset + 8];
            let chunk_start = offset + 8;
            let chunk_end = chunk_start + chunk_length;
            let chunk_data = &bytes[chunk_start..chunk_end];

            match chunk_type {
                b"JSON" => json_chunk = Some(chunk_data),
                b"BIN\0" => bin_chunk = Some(chunk_data),
                _ => {}
            }

            offset = chunk_end;
        }

        let json_chunk = json_chunk.expect("GLB missing JSON chunk");
        let bin_chunk = bin_chunk.expect("GLB missing BIN chunk");
        let json_text = std::str::from_utf8(json_chunk).expect("JSON chunk not valid utf-8");
        let json: serde_json::Value =
            serde_json::from_str(json_text).expect("failed to parse glTF JSON");

        // Find the POSITION attribute's accessor index on the first mesh
        // primitive — that's all these fixtures ever have.
        let accessor_index = json["meshes"][0]["primitives"][0]["attributes"]["POSITION"]
            .as_u64()
            .expect("no POSITION attribute on first primitive") as usize;

        let accessor = &json["accessors"][accessor_index];
        let count = accessor["count"].as_u64().expect("accessor missing count") as usize;
        let component_type = accessor["componentType"]
            .as_u64()
            .expect("accessor missing componentType");
        assert_eq!(component_type, 5126, "expected float32 POSITION accessor");
        assert_eq!(
            accessor["type"].as_str(),
            Some("VEC3"),
            "expected VEC3 POSITION accessor"
        );

        let buffer_view_index = accessor["bufferView"]
            .as_u64()
            .expect("accessor missing bufferView") as usize;
        let buffer_view = &json["bufferViews"][buffer_view_index];
        let byte_offset = buffer_view["byteOffset"].as_u64().unwrap_or(0) as usize;

        let mut positions = Vec::with_capacity(count);
        for i in 0..count {
            let base = byte_offset + i * 12; // tightly packed VEC3<f32>
            let x = f32::from_le_bytes(bin_chunk[base..base + 4].try_into().unwrap());
            let y = f32::from_le_bytes(bin_chunk[base + 4..base + 8].try_into().unwrap());
            let z = f32::from_le_bytes(bin_chunk[base + 8..base + 12].try_into().unwrap());
            positions.push([x, y, z]);
        }

        positions
    }

    #[test]
    fn real_head_render_vertices_all_match_real_base_mesh() {
        assert_real_part_fully_matches_base_mesh(
            "tests/fixtures/cc0_phase2_rigged_body/4001_head.glb",
            17_224,
            4_338,
        );
    }

    // Added to close an open item from `CC0_PHASE_3_MERGE_NOTES.md`:
    // Part A's own task spec proved this for head only; torso/arms/legs
    // were left "not independently spot-checked... beyond what
    // pack_builder's own build implicitly exercised." These three close
    // that gap with the same real-data assertion the head test already
    // makes, against the same real `assets/upstream/base.obj` reference
    // — not re-measured from memory, actually re-run here.
    #[test]
    fn real_torso_render_vertices_all_match_real_base_mesh() {
        assert_real_part_fully_matches_base_mesh(
            "tests/fixtures/cc0_phase2_rigged_body/4002_torso.glb",
            5_840,
            1_552,
        );
    }

    #[test]
    fn real_arms_render_vertices_all_match_real_base_mesh() {
        assert_real_part_fully_matches_base_mesh(
            "tests/fixtures/cc0_phase2_rigged_body/4003_arms.glb",
            17_280,
            4_364,
        );
    }

    #[test]
    fn real_legs_render_vertices_all_match_real_base_mesh() {
        assert_real_part_fully_matches_base_mesh(
            "tests/fixtures/cc0_phase2_rigged_body/4004_legs.glb",
            13_168,
            3_326,
        );
    }

    /// Shared body for the four real-part tests above: loads the real
    /// `base.obj` reference once per call (cheap enough not to bother
    /// caching across the four -- each call is a separate `#[test]`
    /// process-level run anyway under normal `cargo test`, so there is
    /// no shared-state benefit to caching here), builds the duplication
    /// map for the named part at the same `0.0005` tolerance the head
    /// test already established, and asserts full coverage plus the
    /// expected distinct-base-vertex count.
    fn assert_real_part_fully_matches_base_mesh(
        glb_path: &str,
        expected_render_vertex_count: usize,
        expected_distinct_base_vertex_count: usize,
    ) {
        let base_mesh_positions = read_obj_positions("assets/upstream/base.obj");
        assert_eq!(base_mesh_positions.len(), 19_158);

        let render_positions = read_glb_position_accessor(glb_path);
        assert_eq!(render_positions.len(), expected_render_vertex_count);

        let result = build_duplication_map(&render_positions, &base_mesh_positions, 0.0005);

        assert_eq!(result.len(), expected_render_vertex_count);
        assert!(
            result.iter().all(|&index| index != NO_BASE_MESH_MATCH),
            "every real render vertex in '{glb_path}' is expected to match a base-mesh vertex"
        );

        let mut distinct: Vec<u32> = result.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(
            distinct.len(),
            expected_distinct_base_vertex_count,
            "expected {expected_distinct_base_vertex_count} distinct base-mesh vertices for '{glb_path}'"
        );
    }
}
