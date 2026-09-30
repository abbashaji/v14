// AnthroForge Morph Target ("AFMT") converter.
//
// Native-only, self-contained binary, sibling to `pack_builder.rs` and
// following the same pattern: it deliberately does NOT depend on
// anthroforge_core's library code (lib.rs, gltf_loader.rs, obj_loader.rs)
// or on `pack_builder.rs` itself. It re-implements the small amount of
// text-parsing / binary-writing logic it needs directly, so it has zero
// risk of colliding with any other change happening to `lib.rs` at the
// same time, and can be built/run with a plain `cargo build`/`cargo run`
// (no wasm32 target). It is not part of the library's FFI surface.
//
// Converts an upstream MakeHuman `.target` file into this project's own
// sparse per-vertex delta format ("AFMT").
//
// ## A note on the upstream `.target` format
// This parser has since been verified against the real upstream
// MakeHuman `.target` corpus (github.com/makehumancommunity/makehuman,
// `makehuman/data/targets/**/*.target`) — all 1,280 files it ships,
// 6.1M+ non-comment/non-blank lines. Every one of those lines matched
// this parser's `vertex_index dx dy dz` / `#`-comment / blank-line
// assumptions exactly (including MakeHuman's leading-dot float style,
// e.g. `.002`, `-.133`, which `f32::from_str` parses correctly with no
// special-casing needed): zero anomalous lines found. 8 of those files
// are legitimately all-comment/header-only with no data lines at all —
// the same `delta_count = 0` case already covered by
// `empty_target_file_produces_zero_deltas_without_erroring` below.
// Vertex indices across the corpus top out at 19,157, well within
// `u32`, with no negative or duplicate indices. The tool's release
// binary was also run end-to-end against real files from that corpus
// (including the largest, `african-female-baby.target` at 19,168
// lines / 19,150 deltas), and in every case the output file's byte
// size matched `14 + delta_count * 28` exactly. (That check was made
// against the v1 layout, which this tool no longer writes -- see
// "Output format" below for the v2 equivalent.)
//
// The unit tests below still use small, hand-written synthetic
// fixtures rather than real upstream files, since embedding real
// upstream data as test fixtures is unnecessary — the corpus check
// above already establishes the parser's assumptions hold for real
// data; the unit tests exist to pin down this tool's own line-by-line
// parsing/serialization logic in isolation.
//
// ## Output format: AFMT v2 (CC0-Phase 10 Part A)
// Binary little-endian, fixed layout, no padding — same discipline as
// `pack_builder.rs`'s existing AFPP format. This tool writes AFMT v2
// only; `morph_loader.rs` reads both v2 and the legacy v1 (28-byte f32
// entries, `14 + N * 28` bytes) that earlier versions of this tool wrote.
//
//   offset 0  : magic           b"AFMT"  (4 bytes, ASCII, "AnthroForge
//                                          Morph Target")
//   offset 4  : version         u32 LE = 2
//   offset 8  : morph_id        u16 LE
//   offset 10 : delta_count     u32 LE (N)
//   offset 14 : position_scale  f32 LE
//   offset 18 : normal_scale    f32 LE
//   offset 22 : N fixed-size 16-byte delta entries, back-to-back:
//                 vertex_index       u32 LE  (4 bytes)
//                 position_delta_q   3x i16 LE (6 bytes)
//                 normal_delta_q     3x i16 LE (6 bytes)
//
// Total size is `22 + N * 16` bytes. The delta section is exactly 1.75x
// smaller than v1's (16 vs 28 bytes/entry); the whole-file ratio is a
// little under that because v2's header is 8 bytes longer.
//
// ### Quantization
// Position and normal components are quantized independently, each with
// its own scale, because their magnitude ranges differ:
//
//   position_scale = max(|every position component|) / 32767.0
//   normal_scale   = max(|every normal component|)   / 32767.0
//   q              = round(value / scale).clamp(-32767, 32767) as i16
//
// The loader reconstructs `value = q as f32 * scale`. The clamp is
// symmetric on purpose: `-32768` is never written. Rounding to the
// nearest step bounds the error at `scale / 2` per component, so
// precision is uniform in *absolute* terms over a target's own range;
// it is not uniform in relative terms (the smallest deltas in a target
// carry the same absolute error as the largest).
//
// If a target's deltas of a given kind are literally all zero (8 real
// `universal-*-averagemuscle-averageweight` targets are empty), the scale
// is written as 1.0 and every q is 0 -- this avoids a 0/0 and reconstructs
// exactly. Input containing NaN or infinity cannot be quantized and is
// rejected with an error rather than silently written as zero.
//
// `vertex_index` is written through unchanged from the upstream `.target`
// file's own vertex indices — this tool does not reorder, deduplicate, or
// otherwise renumber vertices (see "Coordination with Part B" in the task
// spec: vertex order must line up with whatever base mesh Part B produces
// from the same upstream source files, which this tool has no visibility
// into and must not second-guess).
//
// `normal_delta` is always zero in this tool's input: upstream `.target`
// files carry no normal-delta data at all, and computing/sculpting one is
// explicitly out of scope (flagged as a known gap for a later task, not an
// oversight). In practice that means `normal_scale` is always 1.0 and
// every `normal_delta_q` is 0 in this tool's output; the separate normal
// scale exists so the format doesn't need another version bump when real
// normals arrive.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("Usage: morph_converter <input.target> <morph_id> <output.afmt>");
        std::process::exit(1);
    }

    let input_path = PathBuf::from(&args[1]);
    let morph_id: u16 = match args[2].parse() {
        Ok(id) => id,
        Err(e) => {
            eprintln!(
                "error: '{}' is not a valid morph_id (expected an integer in 0..=65535): {e}",
                args[2]
            );
            std::process::exit(1);
        }
    };
    let output_path = PathBuf::from(&args[3]);

    match run(&input_path, morph_id, &output_path) {
        Ok(delta_count) => {
            println!(
                "wrote {delta_count} delta(s) to '{}'",
                output_path.display()
            );
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

/// One parsed `.target` line: `vertex_index dx dy dz`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct TargetDelta {
    vertex_index: u32,
    position_delta: [f32; 3],
}

/// All the ways parsing a single `.target` line can fail.
#[derive(Debug)]
enum TargetParseError {
    /// A non-comment, non-blank line did not split into exactly 4
    /// whitespace-separated fields (`vertex_index dx dy dz`).
    WrongFieldCount {
        line_number: usize,
        line: String,
        found: usize,
    },
    /// The first field was not a valid non-negative integer vertex index.
    BadVertexIndex {
        line_number: usize,
        line: String,
        field: String,
    },
    /// One of the three delta fields was not a valid float.
    BadDeltaComponent {
        line_number: usize,
        line: String,
        field: String,
    },
}

impl fmt::Display for TargetParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TargetParseError::WrongFieldCount {
                line_number,
                line,
                found,
            } => write!(
                f,
                "line {line_number}: expected 4 fields (vertex_index dx dy dz), found {found}: '{line}'"
            ),
            TargetParseError::BadVertexIndex {
                line_number,
                line,
                field,
            } => write!(
                f,
                "line {line_number}: '{field}' is not a valid vertex index: '{line}'"
            ),
            TargetParseError::BadDeltaComponent {
                line_number,
                line,
                field,
            } => write!(
                f,
                "line {line_number}: '{field}' is not a valid delta component float: '{line}'"
            ),
        }
    }
}

impl std::error::Error for TargetParseError {}

/// Parse the upstream MakeHuman `.target` text format.
///
/// Each line is one of:
///   - blank (whitespace-only) — skipped
///   - a comment, starting with `#` (any leading whitespace before the
///     `#` is tolerated) — skipped
///   - `vertex_index dx dy dz` — four whitespace-separated fields, parsed
///     into a [`TargetDelta`]
///
/// An empty (or all-comment/all-blank) input produces an empty `Vec`
/// without erroring — a `.target` file with no deltas at all is valid
/// input, not malformed input.
///
/// Never panics; every malformed line returns `Err(TargetParseError)`
/// naming the offending line number and content.
fn parse_target(contents: &str) -> Result<Vec<TargetDelta>, TargetParseError> {
    let mut deltas = Vec::new();

    for (zero_based_index, raw_line) in contents.lines().enumerate() {
        let line_number = zero_based_index + 1;
        let trimmed = raw_line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let fields: Vec<&str> = trimmed.split_whitespace().collect();
        if fields.len() != 4 {
            return Err(TargetParseError::WrongFieldCount {
                line_number,
                line: raw_line.to_string(),
                found: fields.len(),
            });
        }

        let vertex_index: u32 = fields[0].parse().map_err(|_| TargetParseError::BadVertexIndex {
            line_number,
            line: raw_line.to_string(),
            field: fields[0].to_string(),
        })?;

        let mut position_delta = [0.0f32; 3];
        for (component, field) in position_delta.iter_mut().zip(&fields[1..4]) {
            *component = field.parse().map_err(|_| TargetParseError::BadDeltaComponent {
                line_number,
                line: raw_line.to_string(),
                field: field.to_string(),
            })?;
        }

        deltas.push(TargetDelta {
            vertex_index,
            position_delta,
        });
    }

    Ok(deltas)
}

/// Largest magnitude an `i16` quantized component may take. Symmetric on
/// purpose (`-32768` is never produced), so `+max` and `-max` both map to
/// exactly `+-QUANT_LEVELS`.
const QUANT_LEVELS: f32 = 32767.0;

/// One delta ready to be quantized: full-precision position and normal.
#[derive(Debug, Clone, Copy)]
struct QuantizeEntry {
    vertex_index: u32,
    position_delta: [f32; 3],
    normal_delta: [f32; 3],
}

/// Scale for one group of components (all positions, or all normals) of a
/// single target: `max(|component|) / 32767.0`, or `1.0` if every
/// component is exactly zero (avoids a 0/0 and reconstructs exactly).
///
/// Errors on a non-finite component (NaN can't be ordered against a max,
/// and `NaN as i16` would silently become 0), and on a scale that isn't a
/// normal float. The latter only happens for inputs whose largest
/// component is below ~4e-34: `max / 32767` is then subnormal (or
/// underflows to zero), so the scale has lost most of its precision and
/// `value / scale` could overshoot the clamp -- better a loud error than
/// heavily distorted output. Real morph deltas are around 1e-3..1e-1.
fn quantization_scale(
    entries: &[QuantizeEntry],
    what: &str,
    pick: impl Fn(&QuantizeEntry) -> [f32; 3],
) -> Result<f32, String> {
    let mut max_abs = 0.0f32;
    for entry in entries {
        for component in pick(entry) {
            if !component.is_finite() {
                return Err(format!(
                    "vertex {}: {what} delta component is {component}, which cannot be quantized",
                    entry.vertex_index
                ));
            }
            max_abs = max_abs.max(component.abs());
        }
    }

    if max_abs == 0.0 {
        return Ok(1.0);
    }
    let scale = max_abs / QUANT_LEVELS;
    if !scale.is_normal() {
        return Err(format!(
            "{what} deltas' max magnitude {max_abs:e} yields an unusable (subnormal or zero) quantization scale {scale:e}"
        ));
    }
    Ok(scale)
}

/// `round(value / scale)` clamped to `+-32767`, as `i16`.
fn quantize(value: f32, scale: f32) -> i16 {
    (value / scale).round().clamp(-QUANT_LEVELS, QUANT_LEVELS) as i16
}

/// Serialize already-paired position/normal deltas into AFMT v2 (layout
/// and quantization rules at the top of this file). Position and normal
/// are scaled separately.
fn write_afmt_v2_entries(entries: &[QuantizeEntry], morph_id: u16) -> Result<Vec<u8>, String> {
    let delta_count = u32::try_from(entries.len()).map_err(|_| {
        format!(
            "too many deltas ({}) to represent in this format's u32 delta_count field",
            entries.len()
        )
    })?;

    let position_scale = quantization_scale(entries, "position", |e| e.position_delta)?;
    let normal_scale = quantization_scale(entries, "normal", |e| e.normal_delta)?;

    let total_len = 22 + entries.len() * 16;
    let mut out = Vec::with_capacity(total_len);

    out.extend_from_slice(b"AFMT");
    out.extend_from_slice(&2u32.to_le_bytes()); // version
    out.extend_from_slice(&morph_id.to_le_bytes());
    out.extend_from_slice(&delta_count.to_le_bytes());
    out.extend_from_slice(&position_scale.to_le_bytes());
    out.extend_from_slice(&normal_scale.to_le_bytes());

    for entry in entries {
        out.extend_from_slice(&entry.vertex_index.to_le_bytes());
        for component in entry.position_delta {
            out.extend_from_slice(&quantize(component, position_scale).to_le_bytes());
        }
        for component in entry.normal_delta {
            out.extend_from_slice(&quantize(component, normal_scale).to_le_bytes());
        }
    }

    debug_assert_eq!(out.len(), total_len);
    Ok(out)
}

/// Serialize parsed `.target` deltas into AFMT v2. `.target` files carry
/// no normal data, so every normal delta is `[0.0, 0.0, 0.0]` (see the
/// module-level doc comment).
fn write_afmt(deltas: &[TargetDelta], morph_id: u16) -> Result<Vec<u8>, String> {
    let entries: Vec<QuantizeEntry> = deltas
        .iter()
        .map(|d| QuantizeEntry {
            vertex_index: d.vertex_index,
            position_delta: d.position_delta,
            normal_delta: [0.0; 3],
        })
        .collect();
    write_afmt_v2_entries(&entries, morph_id)
}

fn run(input_path: &Path, morph_id: u16, output_path: &Path) -> Result<usize, String> {
    let contents = fs::read_to_string(input_path).map_err(|e| {
        format!(
            "failed to read input target file '{}': {e}",
            input_path.display()
        )
    })?;

    let deltas = parse_target(&contents).map_err(|e| {
        format!(
            "failed to parse '{}' as a MakeHuman .target file: {e}",
            input_path.display()
        )
    })?;

    let bytes = write_afmt(&deltas, morph_id)?;

    fs::write(output_path, &bytes).map_err(|e| {
        format!(
            "failed to write output file '{}': {e}",
            output_path.display()
        )
    })?;

    Ok(deltas.len())
}

// ============================================================================
// Unit tests.
//
// The parsing tests use small hand-written synthetic fixtures. The
// quantization tests use synthetic data where the expected values can be
// worked out by hand, plus (`real_target_*`) the real upstream `.target`
// files already checked in under `tests/fixtures/cc0_phase3_real_morphs/`.
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// v2 header length: magic + version + morph_id + delta_count +
    /// position_scale + normal_scale.
    const V2_HEADER: usize = 22;
    const V2_ENTRY: usize = 16;

    fn f32_at(b: &[u8], at: usize) -> f32 {
        f32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }
    fn i16_at(b: &[u8], at: usize) -> i16 {
        i16::from_le_bytes(b[at..at + 2].try_into().unwrap())
    }
    fn u32_at(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }

    /// Independent-of-the-loader reader for a v2 buffer: returns
    /// (position_scale, normal_scale, [(vertex_index, dequantized
    /// position, dequantized normal)]) using the documented
    /// `q as f32 * scale` rule.
    #[allow(clippy::type_complexity)]
    fn read_v2(b: &[u8]) -> (f32, f32, Vec<(u32, [f32; 3], [f32; 3])>) {
        assert_eq!(&b[0..4], b"AFMT");
        assert_eq!(u32_at(b, 4), 2);
        let n = u32_at(b, 10) as usize;
        assert_eq!(b.len(), V2_HEADER + n * V2_ENTRY, "v2 size must be 22 + N*16");
        let (ps, ns) = (f32_at(b, 14), f32_at(b, 18));
        let mut out = Vec::new();
        for i in 0..n {
            let e = V2_HEADER + i * V2_ENTRY;
            let mut pos = [0.0f32; 3];
            let mut nrm = [0.0f32; 3];
            for a in 0..3 {
                pos[a] = i16_at(b, e + 4 + a * 2) as f32 * ps;
                nrm[a] = i16_at(b, e + 10 + a * 2) as f32 * ns;
            }
            out.push((u32_at(b, e), pos, nrm));
        }
        (ps, ns, out)
    }

    fn fixture(rel: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(rel)
    }

    /// A well-formed small fixture file round-trips correctly: the parsed
    /// deltas match what was written, and the resulting AFMT v2 buffer has
    /// the right header fields, byte layout, total size
    /// (`22 + delta_count * 16`), and hand-computed quantized values.
    #[test]
    fn well_formed_fixture_round_trips() {
        let fixture = "\
# AnthroForge synthetic MakeHuman .target fixture
0 0.1 0.2 0.3
5 -1.0 0.0 2.5
";
        let deltas = parse_target(fixture).expect("well-formed fixture should parse");
        assert_eq!(
            deltas,
            vec![
                TargetDelta {
                    vertex_index: 0,
                    position_delta: [0.1, 0.2, 0.3],
                },
                TargetDelta {
                    vertex_index: 5,
                    position_delta: [-1.0, 0.0, 2.5],
                },
            ]
        );

        let bytes = write_afmt(&deltas, 42).expect("write_afmt should succeed");

        // Total size matches 22 + delta_count * 16 exactly.
        assert_eq!(bytes.len(), V2_HEADER + deltas.len() * V2_ENTRY);

        // Header.
        assert_eq!(&bytes[0..4], b"AFMT");
        assert_eq!(u32_at(&bytes, 4), 2);
        assert_eq!(u16::from_le_bytes(bytes[8..10].try_into().unwrap()), 42);
        assert_eq!(u32_at(&bytes, 10) as usize, deltas.len());
        // Largest position component is 2.5, so position_scale = 2.5 /
        // 32767. Normals are all zero, so normal_scale is the degenerate 1.0.
        assert_eq!(f32_at(&bytes, 14), 2.5f32 / 32767.0);
        assert_eq!(f32_at(&bytes, 18), 1.0);

        // Entry 0: vertex 0. Worked by hand: q = round(v * 32767 / 2.5):
        //   0.1 -> 1310.68 -> 1311, 0.2 -> 2621.36 -> 2621, 0.3 -> 3932.04 -> 3932
        let e0 = V2_HEADER;
        assert_eq!(u32_at(&bytes, e0), 0);
        assert_eq!(
            [i16_at(&bytes, e0 + 4), i16_at(&bytes, e0 + 6), i16_at(&bytes, e0 + 8)],
            [1311, 2621, 3932]
        );
        assert_eq!(
            [i16_at(&bytes, e0 + 10), i16_at(&bytes, e0 + 12), i16_at(&bytes, e0 + 14)],
            [0, 0, 0],
            "normal_delta_q is always zero"
        );

        // Entry 1: vertex 5. -1.0 -> -13106.8 -> -13107, 0.0 -> 0, and the
        // max component 2.5 maps to exactly +32767.
        let e1 = V2_HEADER + V2_ENTRY;
        assert_eq!(u32_at(&bytes, e1), 5);
        assert_eq!(
            [i16_at(&bytes, e1 + 4), i16_at(&bytes, e1 + 6), i16_at(&bytes, e1 + 8)],
            [-13107, 0, 32767]
        );
    }

    /// A file with comment lines (`#...`), blank lines, and lines with
    /// leading/trailing whitespace around a comment marker is handled:
    /// all such lines are skipped and only the real data lines produce
    /// deltas.
    #[test]
    fn comment_and_blank_lines_are_skipped() {
        let fixture = "\
# leading comment

   # indented comment
1 1.0 1.0 1.0

# trailing comment
";
        let deltas = parse_target(fixture).expect("should parse despite comments/blanks");
        assert_eq!(
            deltas,
            vec![TargetDelta {
                vertex_index: 1,
                position_delta: [1.0, 1.0, 1.0],
            }]
        );

        let bytes = write_afmt(&deltas, 7).unwrap();
        assert_eq!(bytes.len(), V2_HEADER + V2_ENTRY);
        assert_eq!(u32_at(&bytes, 10) as usize, 1);
    }

    /// An empty target file (and a file that is entirely comments/blank
    /// lines) produces `delta_count = 0` without erroring, and the
    /// resulting AFMT v2 buffer is exactly the 22-byte header with no
    /// entries (both scales the degenerate 1.0).
    #[test]
    fn empty_target_file_produces_zero_deltas_without_erroring() {
        let deltas = parse_target("").expect("empty input should not error");
        assert!(deltas.is_empty());

        let deltas_all_comments =
            parse_target("# just a comment\n\n   \n").expect("all-comment/blank input should not error");
        assert!(deltas_all_comments.is_empty());

        let bytes = write_afmt(&deltas, 1).unwrap();
        assert_eq!(bytes.len(), V2_HEADER);
        assert_eq!(&bytes[0..4], b"AFMT");
        assert_eq!(u32_at(&bytes, 4), 2);
        assert_eq!(u16::from_le_bytes(bytes[8..10].try_into().unwrap()), 1);
        assert_eq!(u32_at(&bytes, 10), 0);
        assert_eq!(f32_at(&bytes, 14), 1.0);
        assert_eq!(f32_at(&bytes, 18), 1.0);
    }

    /// A malformed line (wrong field count) is rejected with a clear
    /// error rather than silently skipped or panicking.
    #[test]
    fn malformed_line_wrong_field_count_errors() {
        let fixture = "0 1.0 2.0\n"; // only 3 fields, missing dz
        let result = parse_target(fixture);
        assert!(result.is_err());
    }

    /// A malformed vertex index (non-integer first field) is rejected
    /// with a clear error.
    #[test]
    fn malformed_vertex_index_errors() {
        let fixture = "abc 1.0 2.0 3.0\n";
        let result = parse_target(fixture);
        assert!(result.is_err());
    }

    /// End-to-end: running the tool's `run()` entry point against a
    /// fixture file on disk produces an output file whose byte size
    /// matches `22 + delta_count * 16` exactly.
    #[test]
    fn run_end_to_end_against_fixture_file_on_disk() {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "morph_converter_test_e2e_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let input_path = dir.join("fixture.target");
        fs::write(
            &input_path,
            "\
# synthetic fixture, not real upstream MakeHuman data
0 0.01 0.02 0.03
1 0.04 0.05 0.06
2 0.07 0.08 0.09
",
        )
        .unwrap();

        let output_path = dir.join("fixture.afmt");
        let delta_count = run(&input_path, 99, &output_path).expect("run should succeed");
        assert_eq!(delta_count, 3);

        let bytes = fs::read(&output_path).unwrap();
        assert_eq!(bytes.len(), V2_HEADER + delta_count * V2_ENTRY);
        assert_eq!(&bytes[0..4], b"AFMT");
        assert_eq!(u32_at(&bytes, 4), 2);
        assert_eq!(u16::from_le_bytes(bytes[8..10].try_into().unwrap()), 99);
        assert_eq!(u32_at(&bytes, 10) as usize, 3);

        let _ = fs::remove_dir_all(&dir);
    }

    // ---- quantization behaviour ----------------------------------------

    /// Position and normal are quantized with SEPARATE scales. Uses
    /// normals whose range is 1000x smaller than the positions', so a
    /// shared scale would crush the normals to (almost) all zeros.
    /// (`.target` files have no normals, so this goes through
    /// `write_afmt_v2_entries` directly.)
    #[test]
    fn position_and_normal_are_quantized_with_separate_scales() {
        let entries = [
            QuantizeEntry {
                vertex_index: 10,
                position_delta: [1.0, -0.5, 0.25],
                normal_delta: [0.001, -0.0005, 0.0],
            },
            QuantizeEntry {
                vertex_index: 11,
                position_delta: [-1.0, 0.0, 0.5],
                normal_delta: [0.0, 0.0005, -0.001],
            },
        ];
        let bytes = write_afmt_v2_entries(&entries, 3).unwrap();
        let (ps, ns, decoded) = read_v2(&bytes);

        assert_eq!(ps, 1.0f32 / 32767.0);
        assert_eq!(ns, 0.001f32 / 32767.0);

        // Each group's own largest magnitude lands on exactly +-32767.
        assert_eq!(i16_at(&bytes, V2_HEADER + 4), 32767); // pos x of entry 0 (1.0)
        assert_eq!(i16_at(&bytes, V2_HEADER + 10), 32767); // normal x of entry 0 (0.001)
        assert_eq!(i16_at(&bytes, V2_HEADER + V2_ENTRY + 4), -32767); // pos x of entry 1 (-1.0)
        assert_eq!(i16_at(&bytes, V2_HEADER + V2_ENTRY + 14), -32767); // normal z of entry 1 (-0.001)

        // Half-magnitude normals sit near +-16384 (the half-step rounding
        // makes it 16383.5 -> 16384): full-range use, not crushed.
        assert_eq!(i16_at(&bytes, V2_HEADER + 12).abs(), 16384);

        // Decoded values are within half a step of the inputs, per group.
        for (entry, (idx, pos, nrm)) in entries.iter().zip(&decoded) {
            assert_eq!(entry.vertex_index, *idx);
            for a in 0..3 {
                assert!((pos[a] - entry.position_delta[a]).abs() <= ps * 0.51);
                assert!((nrm[a] - entry.normal_delta[a]).abs() <= ns * 0.51);
            }
        }
    }

    /// A NON-empty target whose deltas are literally all zero (the real
    /// degenerate case, distinct from a header-only file): scale falls
    /// back to 1.0 instead of 0/0, and every q is 0.
    #[test]
    fn all_zero_deltas_use_unit_scale_and_quantize_to_zero() {
        let deltas = parse_target("0 0 0 0\n7 0.0 -0.0 0.0\n").unwrap();
        assert_eq!(deltas.len(), 2);

        let bytes = write_afmt(&deltas, 8).unwrap();
        let (ps, ns, decoded) = read_v2(&bytes);
        assert_eq!(ps, 1.0);
        assert_eq!(ns, 1.0);
        for (_, pos, nrm) in decoded {
            assert_eq!(pos, [0.0, 0.0, 0.0]);
            assert_eq!(nrm, [0.0, 0.0, 0.0]);
        }
    }

    /// `+max` and `-max` both encode to exactly `+-32767`, and `-32768` is
    /// never produced anywhere, even by an extreme spread of magnitudes.
    #[test]
    fn extremes_map_to_plus_minus_32767_and_never_to_minus_32768() {
        let deltas = vec![
            TargetDelta { vertex_index: 0, position_delta: [3.0, -3.0, 1e-7] },
            TargetDelta { vertex_index: 1, position_delta: [-3.0, 3.0, -1e-7] },
        ];
        let bytes = write_afmt(&deltas, 1).unwrap();
        assert_eq!(i16_at(&bytes, V2_HEADER + 4), 32767);
        assert_eq!(i16_at(&bytes, V2_HEADER + 6), -32767);
        assert_eq!(i16_at(&bytes, V2_HEADER + V2_ENTRY + 4), -32767);
        for i in 0..2 {
            for c in 0..6 {
                assert_ne!(i16_at(&bytes, V2_HEADER + i * V2_ENTRY + 4 + c * 2), i16::MIN);
            }
        }
    }

    /// Reconstruction error is bounded by half a quantization step
    /// (`scale / 2`) for EVERY component, checked over a deterministic
    /// spread that mixes large, small, and tiny magnitudes (a few
    /// thousand values from a fixed LCG, so no `rand` dependency and no
    /// run-to-run variation). Asserted per component, not averaged.
    #[test]
    fn reconstruction_error_never_exceeds_half_a_step() {
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((state >> 33) as f32) / (u32::MAX >> 1) as f32 // [0, 1)
        };
        let deltas: Vec<TargetDelta> = (0..2000u32)
            .map(|i| {
                // Magnitude spread over ~5 decades: 10^(-4..+1).
                let mut c = [0.0f32; 3];
                for v in c.iter_mut() {
                    let mag = 10f32.powf(-4.0 + 5.0 * next());
                    *v = if next() < 0.5 { -mag } else { mag };
                }
                TargetDelta { vertex_index: i, position_delta: c }
            })
            .collect();

        let bytes = write_afmt(&deltas, 1).unwrap();
        let (ps, _, decoded) = read_v2(&bytes);

        // scale/2 for round-to-nearest, plus a little slack (~1.6% of a
        // step) for f32 rounding in `value / scale` and `q * scale`.
        let bound = ps * (0.5 + 4.0 * 32767.0 * f32::EPSILON);
        let mut worst = 0.0f32;
        for (d, (_, pos, _)) in deltas.iter().zip(&decoded) {
            for a in 0..3 {
                let err = (pos[a] - d.position_delta[a]).abs();
                worst = worst.max(err);
                assert!(
                    err <= bound,
                    "vertex {}: |{} - {}| = {err} > bound {bound}",
                    d.vertex_index, pos[a], d.position_delta[a]
                );
            }
        }
        eprintln!("synthetic spread: scale={ps:e} worst error={worst:e} bound={bound:e}");

        // `-32768` must never appear in any of the 2000 x 6 quantized
        // slots (symmetric +-32767 range).
        for i in 0..deltas.len() {
            for c in 0..6 {
                assert_ne!(i16_at(&bytes, V2_HEADER + i * V2_ENTRY + 4 + c * 2), i16::MIN);
            }
        }
    }

    /// Inputs so small that `max / 32767` is subnormal can't be quantized
    /// with a usable scale; they're refused instead of being written as
    /// heavily distorted values (see `quantization_scale`).
    #[test]
    fn subnormal_range_deltas_are_rejected() {
        // 45874 smallest-subnormal steps: max / 32767 ~= 1.4 steps.
        let tiny = f32::from_bits(45874);
        assert!(tiny > 0.0 && !tiny.is_normal());
        let deltas = vec![
            TargetDelta { vertex_index: 3, position_delta: [tiny, -tiny, 0.0] },
        ];
        let err = write_afmt(&deltas, 1).expect_err("subnormal-scale input must be refused");
        assert!(err.contains("unusable"), "{err}");
    }

    /// NaN and infinity in a delta (`f32::from_str` accepts "NaN"/"inf")
    /// are refused with an error naming the vertex, not silently
    /// quantized to 0.
    #[test]
    fn non_finite_delta_components_are_rejected() {
        for bad in ["NaN", "inf", "-inf"] {
            let deltas = parse_target(&format!("12 0.1 {bad} 0.3\n")).unwrap();
            let err = write_afmt(&deltas, 1).expect_err(bad);
            assert!(err.contains("vertex 12"), "error should name the vertex: {err}");
        }
    }

    // ---- real data --------------------------------------------------------

    /// The converter's output for each real upstream `.target` is
    /// byte-identical to the v2 fixture checked in for the loader's tests
    /// (so those fixtures can't silently drift from what this tool
    /// actually writes), reconstructs the `.target` within half a step,
    /// and has the exact v2 size. Also prints the measured v1-vs-v2 file
    /// sizes (v1 sizes are read from the real v1 fixtures the previous
    /// converter wrote from the same `.target` files).
    #[test]
    fn real_target_output_matches_v2_fixture_and_reconstructs_source() {
        for (target, id, v1_name, v2_name) in [
            ("asym-ear-1-l.target", 5001u16, "5001_asym_ear_1_l.afmt", "5001_asym_ear_1_l.afmt"),
            ("asym-nose-1-l.target", 5002u16, "5002_asym_nose_1_l.afmt", "5002_asym_nose_1_l.afmt"),
        ] {
            let text = fs::read_to_string(fixture(&format!("cc0_phase3_real_morphs/{target}"))).unwrap();
            let deltas = parse_target(&text).unwrap();
            let bytes = write_afmt(&deltas, id).unwrap();

            // Byte-identical to the checked-in v2 fixture.
            let v2_fixture = fs::read(fixture(&format!("cc0_phase10_afmt_v2/{v2_name}"))).unwrap();
            assert_eq!(bytes, v2_fixture, "{target}: fixture drifted from converter output");

            // Exact sizes: v2 = 22 + N*16; v1 fixture = 14 + N*28.
            let n = deltas.len();
            assert_eq!(bytes.len(), 22 + n * 16);
            let v1_len = fs::metadata(fixture(&format!("cc0_phase3_real_morphs/{v1_name}"))).unwrap().len() as usize;
            assert_eq!(v1_len, 14 + n * 28, "{target}: v1 fixture size sanity");
            let whole_file = v1_len as f64 / bytes.len() as f64;
            let delta_section = (n * 28) as f64 / (n * 16) as f64;
            assert!(whole_file < delta_section && whole_file > 1.74);
            eprintln!(
                "{target}: N={n} v1={v1_len} B, v2={} B, whole-file ratio {whole_file:.4}x (delta section {delta_section:.2}x)",
                bytes.len()
            );

            // Reconstruction vs the source text, per component.
            let (ps, _, decoded) = read_v2(&bytes);
            let bound = ps * (0.5 + 4.0 * 32767.0 * f32::EPSILON);
            for (d, (idx, pos, _)) in deltas.iter().zip(&decoded) {
                assert_eq!(d.vertex_index, *idx);
                for a in 0..3 {
                    assert!((pos[a] - d.position_delta[a]).abs() <= bound, "{target} vertex {idx} axis {a}");
                }
            }
        }
    }
}
