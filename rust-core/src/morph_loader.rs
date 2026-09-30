//! `morph_loader` — parses this project's own binary "AFMT" morph-delta
//! format (produced by `src/bin/morph_converter.rs` from upstream
//! MakeHuman `.target` files) into an in-memory [`LoadedMorph`].
//!
//! Two on-disk versions are readable. Both produce the same
//! [`LoadedMorph`] with real `f32` deltas, so nothing downstream of this
//! module (`morph_blend`, `lib.rs`) can tell which one a file was.
//!
//! All integers little-endian. The first 14 bytes are identical in both
//! versions.
//!
//! ## AFMT v1 (legacy, `f32` deltas)
//! ```text
//! offset 0  : magic       b"AFMT"  (4 bytes, ASCII)
//! offset 4  : version     u32 LE = 1
//! offset 8  : morph_id    u16 LE
//! offset 10 : delta_count u32 LE (N)
//! offset 14 : N fixed-size 28-byte delta entries, back-to-back:
//!               vertex_index      u32 LE  (4 bytes)
//!               position_delta    3x f32 LE (12 bytes)
//!               normal_delta      3x f32 LE (12 bytes)
//! ```
//!
//! ## AFMT v2 (`i16`-quantized deltas)
//! ```text
//! offset 0  : magic           b"AFMT"
//! offset 4  : version         u32 LE = 2
//! offset 8  : morph_id        u16 LE
//! offset 10 : delta_count     u32 LE (N)
//! offset 14 : position_scale  f32 LE
//! offset 18 : normal_scale    f32 LE
//! offset 22 : N fixed-size 16-byte delta entries, back-to-back:
//!               vertex_index       u32 LE  (4 bytes)
//!               position_delta_q   3x i16 LE (6 bytes)
//!               normal_delta_q     3x i16 LE (6 bytes)
//! ```
//! Dequantization, done here at load time and nowhere else:
//! `value = q as f32 * scale`, with `position_scale` applied to the three
//! position components and `normal_scale` to the three normal components.
//! The converter clamps to `+-32767`, so it never writes `-32768`; this
//! loader still accepts it (it just dequantizes like any other `i16`).
//!
//! `vertex_index` refers to a vertex in the real upstream base mesh, not
//! to any particular part's render mesh — see `vertex_duplication_map`
//! (built elsewhere in this phase) for how the two are connected.

use std::collections::HashMap;
use std::fmt;

const MAGIC: [u8; 4] = *b"AFMT";
const VERSION_1: u32 = 1;
const VERSION_2: u32 = 2;

/// magic + version + morph_id + delta_count: the prefix both versions
/// share, and all of v1's header.
const COMMON_HEADER_LEN: usize = 14;
/// v2 appends `position_scale` + `normal_scale` (2x f32) to the common
/// prefix.
const V2_HEADER_LEN: usize = COMMON_HEADER_LEN + 8;
const V1_ENTRY_LEN: usize = 28;
const V2_ENTRY_LEN: usize = 16;

#[derive(Debug, Clone, Copy)]
pub struct MorphDelta {
    pub vertex_index: u32,
    pub position_delta: [f32; 3],
    pub normal_delta: [f32; 3],
}

#[derive(Debug)]
pub struct LoadedMorph {
    pub morph_id: u16,
    pub deltas: Vec<MorphDelta>,
    // Internal index for O(1) lookup by vertex_index. Populated eagerly in
    // `load_afmt_bytes`, not lazily. Real AFMT files never share a
    // `vertex_index` across two deltas (each upstream `.target` line is a
    // distinct vertex), but that's not enforced here as an invariant — if
    // it ever happens, last-one-wins, deterministically, keyed on file
    // order (a later entry overwrites an earlier one's index slot).
    index_by_vertex: HashMap<u32, usize>,
}

impl LoadedMorph {
    /// O(1) lookup. Returns `None` if this morph has no delta for that
    /// base-mesh vertex index.
    pub fn delta_for_vertex(&self, base_vertex_index: u32) -> Option<&MorphDelta> {
        self.index_by_vertex
            .get(&base_vertex_index)
            .map(|&i| &self.deltas[i])
    }

    /// Test-only constructor: builds a `LoadedMorph` directly from parts,
    /// deriving `index_by_vertex` the same way `load_afmt_bytes` does
    /// (later entry wins on a duplicate `vertex_index`). Exists because
    /// `index_by_vertex` is a private field, so `morph_blend`'s tests (in
    /// a different file/module) have no other way to construct a
    /// `LoadedMorph` by hand.
    #[cfg(test)]
    pub(crate) fn from_parts_for_test(morph_id: u16, deltas: Vec<MorphDelta>) -> Self {
        let mut index_by_vertex = HashMap::with_capacity(deltas.len());
        for (i, d) in deltas.iter().enumerate() {
            index_by_vertex.insert(d.vertex_index, i);
        }
        LoadedMorph {
            morph_id,
            deltas,
            index_by_vertex,
        }
    }
}

/// All the ways parsing an AFMT byte buffer can fail.
#[derive(Debug)]
pub enum MorphLoadError {
    /// The buffer is shorter than the 14-byte prefix every AFMT version
    /// starts with, so not even magic/version/morph_id/delta_count can be
    /// read.
    TooShortForHeader { len: usize },
    /// The first 4 bytes aren't exactly `b"AFMT"`.
    BadMagic { found: [u8; 4] },
    /// The version field is neither `1` nor `2`. This loader deliberately
    /// doesn't try to be forward compatible with hypothetical future
    /// versions.
    UnsupportedVersion { found: u32 },
    /// The version field says `2`, but the buffer is shorter than v2's
    /// 22-byte header (it has the 14-byte common prefix but not both
    /// scale fields).
    TooShortForV2Header { len: usize },
    /// A v2 scale field is NaN, infinite, or negative. Any of those would
    /// silently poison every dequantized delta (NaN/inf) or flip its sign
    /// (negative), so it's rejected up front. `field` is
    /// `"position_scale"` or `"normal_scale"`.
    InvalidV2Scale { field: &'static str, value: f32 },
    /// The buffer's length doesn't exactly match the header size plus
    /// `delta_count * entry_size` for its version (`14 + N * 28` for v1,
    /// `22 + N * 16` for v2).
    TruncatedDeltas {
        expected_count: u32,
        expected_bytes: usize,
        actual_bytes: usize,
    },
}

impl fmt::Display for MorphLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MorphLoadError::TooShortForHeader { len } => write!(
                f,
                "AFMT buffer too short for header: got {len} bytes, need at least {COMMON_HEADER_LEN}"
            ),
            MorphLoadError::BadMagic { found } => write!(
                f,
                "bad AFMT magic: expected {MAGIC:?}, found {found:?}"
            ),
            MorphLoadError::UnsupportedVersion { found } => write!(
                f,
                "unsupported AFMT version: supported versions are {VERSION_1} and {VERSION_2}, found {found}"
            ),
            MorphLoadError::TooShortForV2Header { len } => write!(
                f,
                "AFMT v2 buffer too short for header: got {len} bytes, need at least {V2_HEADER_LEN}"
            ),
            MorphLoadError::InvalidV2Scale { field, value } => write!(
                f,
                "AFMT v2 {field} must be finite and non-negative, found {value}"
            ),
            MorphLoadError::TruncatedDeltas {
                expected_count,
                expected_bytes,
                actual_bytes,
            } => write!(
                f,
                "truncated AFMT delta section: delta_count={expected_count} implies {expected_bytes} bytes, but buffer has {actual_bytes}"
            ),
        }
    }
}

/// `header_len + delta_count * entry_len`, saturating instead of
/// overflowing. `usize` is only 32 bits on the wasm32 target this crate
/// also ships for, where a corrupt `delta_count` near `u32::MAX` would
/// overflow the multiplication (a panic in debug, a silent wrap in
/// release). Saturating turns that into an ordinary length mismatch, i.e.
/// a `TruncatedDeltas` error.
fn expected_len(header_len: usize, delta_count: u32, entry_len: usize) -> usize {
    header_len.saturating_add((delta_count as usize).saturating_mul(entry_len))
}

pub fn load_afmt_bytes(bytes: &[u8]) -> Result<LoadedMorph, MorphLoadError> {
    if bytes.len() < COMMON_HEADER_LEN {
        return Err(MorphLoadError::TooShortForHeader { len: bytes.len() });
    }

    let mut magic = [0u8; 4];
    magic.copy_from_slice(&bytes[0..4]);
    if magic != MAGIC {
        return Err(MorphLoadError::BadMagic { found: magic });
    }

    let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    let morph_id = u16::from_le_bytes(bytes[8..10].try_into().unwrap());
    let delta_count = u32::from_le_bytes(bytes[10..14].try_into().unwrap());

    match version {
        VERSION_1 => load_v1(bytes, morph_id, delta_count),
        VERSION_2 => load_v2(bytes, morph_id, delta_count),
        found => Err(MorphLoadError::UnsupportedVersion { found }),
    }
}

fn load_v1(bytes: &[u8], morph_id: u16, delta_count: u32) -> Result<LoadedMorph, MorphLoadError> {
    let expected_bytes = expected_len(COMMON_HEADER_LEN, delta_count, V1_ENTRY_LEN);
    if bytes.len() != expected_bytes {
        return Err(MorphLoadError::TruncatedDeltas {
            expected_count: delta_count,
            expected_bytes,
            actual_bytes: bytes.len(),
        });
    }

    let mut deltas = Vec::with_capacity(delta_count as usize);
    let mut index_by_vertex = HashMap::with_capacity(delta_count as usize);

    for i in 0..delta_count as usize {
        let base = COMMON_HEADER_LEN + i * V1_ENTRY_LEN;
        let entry = &bytes[base..base + V1_ENTRY_LEN];

        let vertex_index = u32::from_le_bytes(entry[0..4].try_into().unwrap());

        let mut position_delta = [0f32; 3];
        for (c, chunk) in position_delta.iter_mut().zip(entry[4..16].chunks_exact(4)) {
            *c = f32::from_le_bytes(chunk.try_into().unwrap());
        }

        let mut normal_delta = [0f32; 3];
        for (c, chunk) in normal_delta.iter_mut().zip(entry[16..28].chunks_exact(4)) {
            *c = f32::from_le_bytes(chunk.try_into().unwrap());
        }

        index_by_vertex.insert(vertex_index, i);
        deltas.push(MorphDelta {
            vertex_index,
            position_delta,
            normal_delta,
        });
    }

    Ok(LoadedMorph {
        morph_id,
        deltas,
        index_by_vertex,
    })
}

fn load_v2(bytes: &[u8], morph_id: u16, delta_count: u32) -> Result<LoadedMorph, MorphLoadError> {
    if bytes.len() < V2_HEADER_LEN {
        return Err(MorphLoadError::TooShortForV2Header { len: bytes.len() });
    }

    let position_scale = f32::from_le_bytes(bytes[14..18].try_into().unwrap());
    let normal_scale = f32::from_le_bytes(bytes[18..22].try_into().unwrap());
    for (field, value) in [
        ("position_scale", position_scale),
        ("normal_scale", normal_scale),
    ] {
        if !value.is_finite() || value < 0.0 {
            return Err(MorphLoadError::InvalidV2Scale { field, value });
        }
    }

    let expected_bytes = expected_len(V2_HEADER_LEN, delta_count, V2_ENTRY_LEN);
    if bytes.len() != expected_bytes {
        return Err(MorphLoadError::TruncatedDeltas {
            expected_count: delta_count,
            expected_bytes,
            actual_bytes: bytes.len(),
        });
    }

    let mut deltas = Vec::with_capacity(delta_count as usize);
    let mut index_by_vertex = HashMap::with_capacity(delta_count as usize);

    for i in 0..delta_count as usize {
        let base = V2_HEADER_LEN + i * V2_ENTRY_LEN;
        let entry = &bytes[base..base + V2_ENTRY_LEN];

        let vertex_index = u32::from_le_bytes(entry[0..4].try_into().unwrap());

        // Dequantize here, and only here: `value = q as f32 * scale`.
        let mut position_delta = [0f32; 3];
        for (c, chunk) in position_delta.iter_mut().zip(entry[4..10].chunks_exact(2)) {
            *c = i16::from_le_bytes(chunk.try_into().unwrap()) as f32 * position_scale;
        }

        let mut normal_delta = [0f32; 3];
        for (c, chunk) in normal_delta.iter_mut().zip(entry[10..16].chunks_exact(2)) {
            *c = i16::from_le_bytes(chunk.try_into().unwrap()) as f32 * normal_scale;
        }

        index_by_vertex.insert(vertex_index, i);
        deltas.push(MorphDelta {
            vertex_index,
            position_delta,
            normal_delta,
        });
    }

    Ok(LoadedMorph {
        morph_id,
        deltas,
        index_by_vertex,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-builds a small synthetic **v1** AFMT byte buffer: magic +
    /// version + morph_id + `deltas.len()` 28-byte entries, in the exact
    /// on-disk layout `load_afmt_bytes` expects.
    fn build_afmt_bytes(morph_id: u16, deltas: &[MorphDelta]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(COMMON_HEADER_LEN + deltas.len() * V1_ENTRY_LEN);
        bytes.extend_from_slice(&MAGIC);
        bytes.extend_from_slice(&VERSION_1.to_le_bytes());
        bytes.extend_from_slice(&morph_id.to_le_bytes());
        bytes.extend_from_slice(&(deltas.len() as u32).to_le_bytes());
        for d in deltas {
            bytes.extend_from_slice(&d.vertex_index.to_le_bytes());
            for c in d.position_delta {
                bytes.extend_from_slice(&c.to_le_bytes());
            }
            for c in d.normal_delta {
                bytes.extend_from_slice(&c.to_le_bytes());
            }
        }
        bytes
    }

    /// One already-quantized v2 entry: (vertex_index, position_q, normal_q).
    type V2Entry = (u32, [i16; 3], [i16; 3]);

    /// Hand-builds a synthetic **v2** AFMT byte buffer in the exact
    /// on-disk layout documented at the top of this file.
    fn build_afmt_v2_bytes(
        morph_id: u16,
        position_scale: f32,
        normal_scale: f32,
        entries: &[V2Entry],
    ) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(V2_HEADER_LEN + entries.len() * V2_ENTRY_LEN);
        bytes.extend_from_slice(&MAGIC);
        bytes.extend_from_slice(&VERSION_2.to_le_bytes());
        bytes.extend_from_slice(&morph_id.to_le_bytes());
        bytes.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&position_scale.to_le_bytes());
        bytes.extend_from_slice(&normal_scale.to_le_bytes());
        for (vertex_index, pos_q, nrm_q) in entries {
            bytes.extend_from_slice(&vertex_index.to_le_bytes());
            for q in pos_q {
                bytes.extend_from_slice(&q.to_le_bytes());
            }
            for q in nrm_q {
                bytes.extend_from_slice(&q.to_le_bytes());
            }
        }
        bytes
    }

    /// Minimal parser for the upstream MakeHuman `.target` text format
    /// (`vertex_index dx dy dz`, `#` comments, blank lines), used only to
    /// get an independent ground truth for the real-fixture accuracy
    /// tests. Deliberately separate from the converter's own parser.
    fn parse_target_ground_truth(path: &str) -> Vec<(u32, [f32; 3])> {
        std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("failed to read '{path}': {e}"))
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| {
                let f: Vec<&str> = l.split_whitespace().collect();
                assert_eq!(f.len(), 4, "bad .target line: '{l}'");
                (
                    f[0].parse().unwrap(),
                    [
                        f[1].parse().unwrap(),
                        f[2].parse().unwrap(),
                        f[3].parse().unwrap(),
                    ],
                )
            })
            .collect()
    }

    fn fixture(rel: &str) -> String {
        format!("{}/tests/fixtures/{rel}", env!("CARGO_MANIFEST_DIR"))
    }

    // ------------------------------------------------------------------
    // v1 (regression: everything here predates v2 and must keep passing)
    // ------------------------------------------------------------------

    #[test]
    fn round_trips_synthetic_buffer() {
        let deltas = vec![
            MorphDelta {
                vertex_index: 7,
                position_delta: [0.1, -0.2, 0.3],
                normal_delta: [0.0, 0.0, 0.0],
            },
            MorphDelta {
                vertex_index: 42,
                position_delta: [1.5, 0.0, -1.5],
                normal_delta: [0.0, 0.0, 0.0],
            },
        ];
        let bytes = build_afmt_bytes(9001, &deltas);

        let morph = load_afmt_bytes(&bytes).expect("well-formed synthetic buffer should parse");

        assert_eq!(morph.morph_id, 9001);
        assert_eq!(morph.deltas.len(), 2);
        assert_eq!(morph.deltas[0].vertex_index, 7);
        assert_eq!(morph.deltas[0].position_delta, [0.1, -0.2, 0.3]);
        assert_eq!(morph.deltas[0].normal_delta, [0.0, 0.0, 0.0]);
        assert_eq!(morph.deltas[1].vertex_index, 42);
        assert_eq!(morph.deltas[1].position_delta, [1.5, 0.0, -1.5]);

        let found = morph.delta_for_vertex(42).expect("vertex 42 should be present");
        assert_eq!(found.position_delta, [1.5, 0.0, -1.5]);
        assert!(morph.delta_for_vertex(999).is_none());
    }

    #[test]
    fn parses_real_fixture_5001_asym_ear() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/cc0_phase3_real_morphs/5001_asym_ear_1_l.afmt"
        );
        let bytes = std::fs::read(path).expect("real AFMT fixture should be readable");

        let morph = load_afmt_bytes(&bytes).expect("real AFMT fixture should parse");

        assert_eq!(morph.morph_id, 5001);
        assert_eq!(morph.deltas.len(), 635);

        // Don't guess a vertex index -- read one out of the parsed data
        // itself, then assert on it.
        let real_index = morph.deltas[0].vertex_index;
        assert!(morph.delta_for_vertex(real_index).is_some());

        // u32::MAX - 1 is not a plausible base.obj vertex index (the base
        // mesh only has 19,158 vertices), so it should not be present.
        assert!(morph.delta_for_vertex(u32::MAX - 1).is_none());
    }

    /// v1 regression against the real nose fixture too (the ear one above
    /// was the only real v1 file this module exercised before v2).
    #[test]
    fn parses_real_v1_fixture_5002_asym_nose() {
        let bytes =
            std::fs::read(fixture("cc0_phase3_real_morphs/5002_asym_nose_1_l.afmt")).unwrap();
        let morph = load_afmt_bytes(&bytes).expect("real v1 nose fixture should parse");
        assert_eq!(morph.morph_id, 5002);
        assert_eq!(morph.deltas.len(), 316);
    }

    #[test]
    fn rejects_bad_magic() {
        let deltas = vec![MorphDelta {
            vertex_index: 0,
            position_delta: [0.0, 0.0, 0.0],
            normal_delta: [0.0, 0.0, 0.0],
        }];
        let mut bytes = build_afmt_bytes(1, &deltas);
        bytes[0..4].copy_from_slice(b"XXXX");

        match load_afmt_bytes(&bytes) {
            Err(MorphLoadError::BadMagic { found }) => assert_eq!(found, *b"XXXX"),
            other => panic!("expected BadMagic, got {other:?}"),
        }
    }

    /// Versions other than 1 and 2 are still rejected. (This test used
    /// `2` as its example bad version before v2 existed; `2` is valid now,
    /// so it uses `3` and `0`.)
    #[test]
    fn rejects_bad_version() {
        let deltas = vec![MorphDelta {
            vertex_index: 0,
            position_delta: [0.0, 0.0, 0.0],
            normal_delta: [0.0, 0.0, 0.0],
        }];
        for bad in [0u32, 3, u32::MAX] {
            let mut bytes = build_afmt_bytes(1, &deltas);
            bytes[4..8].copy_from_slice(&bad.to_le_bytes());

            match load_afmt_bytes(&bytes) {
                Err(MorphLoadError::UnsupportedVersion { found }) => assert_eq!(found, bad),
                other => panic!("version {bad}: expected UnsupportedVersion, got {other:?}"),
            }
        }
    }

    #[test]
    fn rejects_truncated_buffer() {
        let deltas = vec![
            MorphDelta {
                vertex_index: 0,
                position_delta: [0.0, 0.0, 0.0],
                normal_delta: [0.0, 0.0, 0.0],
            },
            MorphDelta {
                vertex_index: 1,
                position_delta: [0.0, 0.0, 0.0],
                normal_delta: [0.0, 0.0, 0.0],
            },
        ];
        let mut bytes = build_afmt_bytes(1, &deltas);
        // delta_count says 2 (56 bytes of deltas) but chop off the last
        // entry's worth of bytes so the buffer is short.
        bytes.truncate(bytes.len() - V1_ENTRY_LEN);

        match load_afmt_bytes(&bytes) {
            Err(MorphLoadError::TruncatedDeltas {
                expected_count,
                expected_bytes,
                actual_bytes,
            }) => {
                assert_eq!(expected_count, 2);
                assert_eq!(expected_bytes, COMMON_HEADER_LEN + 2 * V1_ENTRY_LEN);
                assert_eq!(actual_bytes, COMMON_HEADER_LEN + 1 * V1_ENTRY_LEN);
            }
            other => panic!("expected TruncatedDeltas, got {other:?}"),
        }
    }

    #[test]
    fn rejects_buffer_too_short_for_header() {
        let bytes = vec![b'A', b'F', b'M', b'T', 1, 0, 0]; // 7 bytes, < 14
        match load_afmt_bytes(&bytes) {
            Err(MorphLoadError::TooShortForHeader { len }) => assert_eq!(len, 7),
            other => panic!("expected TooShortForHeader, got {other:?}"),
        }
    }

    // ------------------------------------------------------------------
    // v2
    // ------------------------------------------------------------------

    /// Hand-computed, exactly-representable dequantization with two
    /// *different* scales, so a loader that swapped the two scales or
    /// shared one between position and normal would fail here.
    #[test]
    fn v2_dequantizes_position_and_normal_with_their_own_scales() {
        // 0.5 and 0.25 are powers of two, so every product below is exact
        // in f32 (no tolerance needed): 32767 * 0.5 = 16383.5, 32767 *
        // 0.25 = 8191.75.
        let bytes = build_afmt_v2_bytes(
            9002,
            0.5,
            0.25,
            &[(7, [2, -4, 32767], [4, -8, -32767]), (42, [0, 0, 0], [1, 1, 1])],
        );

        let morph = load_afmt_bytes(&bytes).expect("well-formed v2 buffer should parse");

        assert_eq!(morph.morph_id, 9002);
        assert_eq!(morph.deltas.len(), 2);
        assert_eq!(morph.deltas[0].vertex_index, 7);
        assert_eq!(morph.deltas[0].position_delta, [1.0, -2.0, 16383.5]);
        assert_eq!(morph.deltas[0].normal_delta, [1.0, -2.0, -8191.75]);
        assert_eq!(morph.deltas[1].position_delta, [0.0, 0.0, 0.0]);
        assert_eq!(morph.deltas[1].normal_delta, [0.25, 0.25, 0.25]);

        // The O(1) index is built for v2 loads too.
        assert_eq!(morph.delta_for_vertex(42).unwrap().normal_delta, [0.25, 0.25, 0.25]);
        assert!(morph.delta_for_vertex(999).is_none());
    }

    /// The converter's degenerate encoding for a target whose deltas are
    /// literally all zero (scale = 1.0, every q = 0) loads as exact zeros,
    /// and so does a v2 file with no entries at all (22-byte header only).
    #[test]
    fn v2_all_zero_and_empty_targets_load_as_zeros() {
        let bytes = build_afmt_v2_bytes(1, 1.0, 1.0, &[(3, [0, 0, 0], [0, 0, 0])]);
        let morph = load_afmt_bytes(&bytes).unwrap();
        assert_eq!(morph.deltas[0].position_delta, [0.0, 0.0, 0.0]);
        assert_eq!(morph.deltas[0].normal_delta, [0.0, 0.0, 0.0]);

        let empty = build_afmt_v2_bytes(1, 1.0, 1.0, &[]);
        assert_eq!(empty.len(), 22);
        let morph = load_afmt_bytes(&empty).expect("header-only v2 buffer is valid");
        assert!(morph.deltas.is_empty());
    }

    /// `-32768` is never written by the converter (it clamps to +-32767)
    /// but is a legal `i16`; the loader dequantizes it like any other.
    #[test]
    fn v2_accepts_i16_min_without_special_casing() {
        let bytes = build_afmt_v2_bytes(1, 0.5, 1.0, &[(0, [-32768, 0, 0], [0, 0, 0])]);
        let morph = load_afmt_bytes(&bytes).unwrap();
        assert_eq!(morph.deltas[0].position_delta, [-16384.0, 0.0, 0.0]);
    }

    #[test]
    fn v2_rejects_buffer_shorter_than_v2_header() {
        // 14..=21 bytes: enough for the common prefix (so it is not a
        // TooShortForHeader) but missing part or all of the two scales.
        let full = build_afmt_v2_bytes(1, 1.0, 1.0, &[]);
        for len in [14usize, 17, 21] {
            match load_afmt_bytes(&full[..len]) {
                Err(MorphLoadError::TooShortForV2Header { len: got }) => assert_eq!(got, len),
                other => panic!("len {len}: expected TooShortForV2Header, got {other:?}"),
            }
        }
    }

    #[test]
    fn v2_rejects_truncated_and_oversized_delta_section() {
        let entries = [(0, [1, 2, 3], [0, 0, 0]), (1, [4, 5, 6], [0, 0, 0])];
        let bytes = build_afmt_v2_bytes(1, 1.0, 1.0, &entries);

        // One entry short.
        let short = &bytes[..bytes.len() - V2_ENTRY_LEN];
        match load_afmt_bytes(short) {
            Err(MorphLoadError::TruncatedDeltas {
                expected_count,
                expected_bytes,
                actual_bytes,
            }) => {
                assert_eq!(expected_count, 2);
                assert_eq!(expected_bytes, V2_HEADER_LEN + 2 * V2_ENTRY_LEN);
                assert_eq!(actual_bytes, V2_HEADER_LEN + V2_ENTRY_LEN);
            }
            other => panic!("expected TruncatedDeltas, got {other:?}"),
        }

        // Trailing garbage is rejected too (exact length match, same
        // policy as v1).
        let mut long = bytes.clone();
        long.push(0);
        assert!(matches!(
            load_afmt_bytes(&long),
            Err(MorphLoadError::TruncatedDeltas { .. })
        ));
    }

    /// A corrupt `delta_count` of `u32::MAX` must produce an ordinary
    /// error in both versions, not an arithmetic-overflow panic or a
    /// multi-gigabyte allocation attempt.
    #[test]
    fn huge_delta_count_is_an_error_not_a_panic() {
        let mut v2 = build_afmt_v2_bytes(1, 1.0, 1.0, &[]);
        v2[10..14].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            load_afmt_bytes(&v2),
            Err(MorphLoadError::TruncatedDeltas { .. })
        ));

        let mut v1 = build_afmt_bytes(1, &[]);
        v1[10..14].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            load_afmt_bytes(&v1),
            Err(MorphLoadError::TruncatedDeltas { .. })
        ));
    }

    #[test]
    fn v2_rejects_non_finite_or_negative_scales() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, -f32::MIN_POSITIVE] {
            let bytes = build_afmt_v2_bytes(1, bad, 1.0, &[(0, [1, 1, 1], [0, 0, 0])]);
            match load_afmt_bytes(&bytes) {
                Err(MorphLoadError::InvalidV2Scale { field, .. }) => {
                    assert_eq!(field, "position_scale", "scale {bad}")
                }
                other => panic!("position_scale {bad}: expected InvalidV2Scale, got {other:?}"),
            }

            let bytes = build_afmt_v2_bytes(1, 1.0, bad, &[(0, [1, 1, 1], [0, 0, 0])]);
            match load_afmt_bytes(&bytes) {
                Err(MorphLoadError::InvalidV2Scale { field, .. }) => {
                    assert_eq!(field, "normal_scale", "scale {bad}")
                }
                other => panic!("normal_scale {bad}: expected InvalidV2Scale, got {other:?}"),
            }
        }
    }

    /// The real accuracy check, against ground truth: load each real v2
    /// fixture (written by the real `morph_converter` from the real
    /// upstream `.target`) and compare every delta against the raw
    /// `.target` text values.
    ///
    /// Bound: rounding to the nearest of 32767 levels gives at most
    /// `scale / 2` absolute error per component; `+ 4 * 32767 * f32::EPSILON
    /// * scale` (~1.6% of `scale`) covers f32 rounding in `q * scale` and
    /// in the converter's `value / scale`. Asserting this per component
    /// (not on an average) means one bad delta fails the test.
    #[test]
    fn real_v2_fixtures_reconstruct_the_source_target_within_quantization_error() {
        for (afmt, target, id, count) in [
            (
                "cc0_phase10_afmt_v2/5001_asym_ear_1_l.afmt",
                "cc0_phase3_real_morphs/asym-ear-1-l.target",
                5001u16,
                635usize,
            ),
            (
                "cc0_phase10_afmt_v2/5002_asym_nose_1_l.afmt",
                "cc0_phase3_real_morphs/asym-nose-1-l.target",
                5002u16,
                316usize,
            ),
        ] {
            let bytes = std::fs::read(fixture(afmt)).unwrap();
            assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), VERSION_2);
            let position_scale = f32::from_le_bytes(bytes[14..18].try_into().unwrap());
            let normal_scale = f32::from_le_bytes(bytes[18..22].try_into().unwrap());
            assert_eq!(bytes.len(), V2_HEADER_LEN + count * V2_ENTRY_LEN, "{afmt}");
            // The converter has no normal data, so it takes the all-zero
            // degenerate branch: scale 1.0, every q = 0.
            assert_eq!(normal_scale, 1.0, "{afmt}");

            let morph = load_afmt_bytes(&bytes).unwrap_or_else(|e| panic!("{afmt}: {e}"));
            assert_eq!(morph.morph_id, id, "{afmt}");
            assert_eq!(morph.deltas.len(), count, "{afmt}");

            let truth = parse_target_ground_truth(&fixture(target));
            assert_eq!(truth.len(), count, "{target}");

            let bound = position_scale * (0.5 + 4.0 * 32767.0 * f32::EPSILON);
            let mut worst = 0.0f32;
            for (loaded, (want_index, want)) in morph.deltas.iter().zip(&truth) {
                // Same vertex order as the source file, unchanged.
                assert_eq!(loaded.vertex_index, *want_index, "{afmt}");
                assert_eq!(loaded.normal_delta, [0.0, 0.0, 0.0], "{afmt}");
                for axis in 0..3 {
                    let err = (loaded.position_delta[axis] - want[axis]).abs();
                    worst = worst.max(err);
                    assert!(
                        err <= bound,
                        "{afmt}: vertex {want_index} axis {axis}: |{} - {}| = {err} exceeds bound {bound}",
                        loaded.position_delta[axis],
                        want[axis]
                    );
                }
            }
            eprintln!("{afmt}: scale={position_scale:e}, worst component error={worst:e}, bound={bound:e}");
        }
    }

    /// v1 and v2 fixtures built from the same `.target` must describe the
    /// same vertices in the same order (v2 changes value precision only).
    #[test]
    fn real_v2_fixtures_have_the_same_vertex_layout_as_the_v1_fixtures() {
        for name in ["5001_asym_ear_1_l.afmt", "5002_asym_nose_1_l.afmt"] {
            let v1 = load_afmt_bytes(
                &std::fs::read(fixture(&format!("cc0_phase3_real_morphs/{name}"))).unwrap(),
            )
            .unwrap();
            let v2 = load_afmt_bytes(
                &std::fs::read(fixture(&format!("cc0_phase10_afmt_v2/{name}"))).unwrap(),
            )
            .unwrap();
            assert_eq!(v1.morph_id, v2.morph_id, "{name}");
            let v1_indices: Vec<u32> = v1.deltas.iter().map(|d| d.vertex_index).collect();
            let v2_indices: Vec<u32> = v2.deltas.iter().map(|d| d.vertex_index).collect();
            assert_eq!(v1_indices, v2_indices, "{name}");
        }
    }
}
