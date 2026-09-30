// AnthroForge Part Pack ("AFPP") builder.
//
// Native-only, self-contained binary. Deliberately does NOT depend on
// anthroforge_core's library code (lib.rs, gltf_loader.rs, obj_loader.rs,
// vertex_duplication_map.rs, morph_loader.rs): it re-implements the small
// amount of directory-walking / numeric-prefix part-id / OBJ-and-GLB
// position parsing / nearest-neighbor duplication-map logic it needs
// directly, so it has zero risk of colliding with any other change
// happening to `lib.rs` at the same time, and can be built/run with a
// plain `cargo build`/`cargo run` (no wasm32 target). This is
// intentional, not an oversight: `vertex_duplication_map::
// build_duplication_map`'s ~20 lines are duplicated below (see
// `compute_duplication_map`) rather than pulled in as a dependency, to
// preserve this file's existing zero-library-dependency isolation rule.
// `serde_json` (used only for the tiny bit of GLB POSITION-accessor JSON
// this file's own `parse_glb_positions` needs) is fine to use directly —
// it's an ordinary external crate dependency already declared in this
// workspace's `Cargo.toml`, not a dependency on `anthroforge_core`'s own
// library code.
//
// Output format (fixed, see TASK_SPEC_PHASE3_MERGE_integration.md's
// "AFPP v2 pack format" section — this replaces the CC0-Phase 1/2 v1
// format below; per this project's own established convention, a reader
// hard-rejects a version mismatch rather than supporting both):
//
//   offset 0   : magic        b"AFPP"           (4 bytes, ASCII, no NUL)
//   offset 4   : version      u32 LE = 2
//   offset 8   : part_count   u32 LE (N, N >= 1)   -- body/clothing parts
//   offset 12  : morph_count  u32 LE (M, M >= 0)
//   offset 16  : skel_len     u32 LE
//   offset 20  : skeleton     skel_len raw bytes (master_skeleton.json,
//                             verbatim, UTF-8)
//   then N x 21-byte part index entries, back-to-back, no padding:
//                part_id       u32 LE
//                src_type      u8   (0 = OBJ, 1 = glTF/GLB)
//                data_offset   u32 LE (absolute offset from buffer start)
//                data_len      u32 LE
//                dupmap_offset u32 LE (absolute offset; 0 if dupmap_len == 0)
//                dupmap_len    u32 LE (byte length = 4 * this part's
//                                       render vertex count; 0 if no
//                                       duplication map was computed)
//   then M x 12-byte morph index entries, back-to-back, no padding:
//                morph_id      u16 LE
//                _reserved     u16 LE (= 0)
//                data_offset   u32 LE (absolute offset; points at a
//                                       complete, self-contained .afmt blob)
//                data_len      u32 LE
//   then: all N parts' raw source bytes, then all N parts' duplication-map
//   bytes (each a `Vec<u32>` LE, back-to-back, only for parts whose
//   dupmap_len > 0), then the raw .afmt bytes of each UNIQUE morph blob
//   (byte-identical morphs are stored once and share data_offset/data_len;
//   no format change -- see the morph dedup comment in `run`).
//
// CLI: `pack_builder <input_asset_dir> <output_pack_file>
// [<base_mesh_obj_path>]`. The third argument is optional; if given, it's
// used as the base mesh for every part's duplication-map computation
// (tolerance 0.0005, matching `vertex_duplication_map`'s own tests). If
// omitted, every part's dupmap_len is 0.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let mut args: Vec<String> = std::env::args().collect();
    // Morph-id-masked dedup (see `dedup_view`) is ON by default: the embedded
    // AFMT morph_id (bytes 8..10) is parsed at load time but never read by
    // anything else -- every real lookup keys off the pack index's own
    // morph_id, which dedup never touches -- so there is no correctness
    // reason to leave real duplicate morphs undetected. Legacy flag name is
    // still accepted (now a no-op, kept for scripts that pass it). Use
    // `--no-dedup-morph-id-mask` to fall back to the old opaque-bytes
    // comparison, e.g. for debugging.
    let no_mask = args.iter().any(|a| a == "--no-dedup-morph-id-mask");
    args.retain(|a| a != "--dedup-ignore-afmt-morph-id" && a != "--no-dedup-morph-id-mask");
    let ignore_afmt_id = !no_mask;
    if args.len() != 3 && args.len() != 4 {
        eprintln!(
            "Usage: pack_builder <input_asset_dir> <output_pack_file> [<base_mesh_obj_path>] [--no-dedup-morph-id-mask]"
        );
        std::process::exit(1);
    }

    let input_dir = PathBuf::from(&args[1]);
    let output_path = PathBuf::from(&args[2]);
    let base_mesh_path = args.get(3).map(PathBuf::from);

    match run(&input_dir, &output_path, base_mesh_path.as_deref(), ignore_afmt_id) {
        Ok((part_count, morph_count, total_bytes)) => {
            println!(
                "wrote {part_count} part(s), {morph_count} morph(s), {total_bytes} bytes, to '{}'",
                output_path.display()
            );
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

/// One accepted input part file, before its bytes are read.
struct PendingPart {
    part_id: u32,
    src_type: u8, // 0 = OBJ, 1 = glTF/GLB
    path: PathBuf,
}

/// One accepted input morph file (a `.afmt` blob, embedded unmodified —
/// its internal AFMT structure is `morph_loader.rs`'s job to validate at
/// load time, not this tool's).
struct PendingMorph {
    morph_id: u16,
    path: PathBuf,
}

fn run(
    input_dir: &Path,
    output_path: &Path,
    base_mesh_path: Option<&Path>,
    ignore_afmt_id: bool,
) -> Result<(usize, usize, usize), String> {
    if !input_dir.is_dir() {
        return Err(format!(
            "input asset dir '{}' does not exist or is not a directory",
            input_dir.display()
        ));
    }

    // master_skeleton.json must exist directly inside the input directory.
    let skeleton_path = input_dir.join("master_skeleton.json");
    let skeleton_bytes = fs::read(&skeleton_path).map_err(|e| {
        format!(
            "expected master_skeleton.json at '{}': {e}",
            skeleton_path.display()
        )
    })?;
    let skel_len = u32::try_from(skeleton_bytes.len()).map_err(|_| {
        format!(
            "master_skeleton.json at '{}' is too large to represent in this format's u32 length field ({} bytes)",
            skeleton_path.display(),
            skeleton_bytes.len()
        )
    })?;

    // Optional base mesh, for duplication-map computation.
    let base_mesh_positions: Option<Vec<[f32; 3]>> = match base_mesh_path {
        Some(p) => {
            let text = fs::read_to_string(p)
                .map_err(|e| format!("failed to read base mesh obj '{}': {e}", p.display()))?;
            Some(parse_obj_positions(&text))
        }
        None => None,
    };

    // Scan the directory for part files and morph files.
    let entries = fs::read_dir(input_dir)
        .map_err(|e| format!("failed to read input asset dir '{}': {e}", input_dir.display()))?;

    let mut pending: Vec<PendingPart> = Vec::new();
    let mut pending_morphs: Vec<PendingMorph> = Vec::new();
    // Tracks which file first claimed each part id, so a collision can name
    // both files. Morph ids share a separate id space (a distinct u16
    // namespace), so they're tracked independently.
    let mut seen_ids: HashMap<u32, PathBuf> = HashMap::new();
    let mut seen_morph_ids: HashMap<u16, PathBuf> = HashMap::new();

    for entry in entries {
        let entry = entry.map_err(|e| format!("failed to read a directory entry: {e}"))?;
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());

        // A `.afmt` file directly inside `input_asset_dir` is a morph, not
        // a part -- handled separately from the part `src_type` match
        // below.
        if ext.as_deref() == Some("afmt") {
            let numeric_id = match parse_part_id(&path) {
                Some(id) => id,
                None => {
                    eprintln!(
                        "warning: skipping '{}': filename does not start with a numeric morph id",
                        path.display()
                    );
                    continue;
                }
            };
            let morph_id = u16::try_from(numeric_id).map_err(|_| {
                format!(
                    "morph id {numeric_id} (from '{}') does not fit in this format's u16 morph_id field",
                    path.display()
                )
            })?;
            if let Some(prior_path) = seen_morph_ids.get(&morph_id) {
                return Err(format!(
                    "duplicate morph id {morph_id}: both '{}' and '{}' resolve to this id",
                    prior_path.display(),
                    path.display()
                ));
            }
            seen_morph_ids.insert(morph_id, path.clone());
            pending_morphs.push(PendingMorph { morph_id, path });
            continue;
        }

        let src_type: u8 = match ext.as_deref() {
            Some("obj") => 0,
            Some("glb") => 1,
            Some("gltf") => {
                eprintln!(
                    "warning: skipping '{}': .gltf not supported by this tool, convert to .glb first",
                    path.display()
                );
                continue;
            }
            Some("json") if path.file_name().and_then(|n| n.to_str()) == Some("master_skeleton.json") => {
                // The skeleton file itself; not a part.
                continue;
            }
            _ => {
                return Err(format!(
                    "unrecognized file extension for '{}' (only .obj, .glb, and .afmt part/morph files, plus master_skeleton.json, are accepted)",
                    path.display()
                ));
            }
        };

        let part_id = match parse_part_id(&path) {
            Some(id) => id,
            None => {
                eprintln!(
                    "warning: skipping '{}': filename does not start with a numeric part id",
                    path.display()
                );
                continue;
            }
        };

        if let Some(prior_path) = seen_ids.get(&part_id) {
            return Err(format!(
                "duplicate part id {part_id}: both '{}' and '{}' resolve to this id",
                prior_path.display(),
                path.display()
            ));
        }
        seen_ids.insert(part_id, path.clone());

        pending.push(PendingPart {
            part_id,
            src_type,
            path,
        });
    }

    if pending.is_empty() {
        return Err(format!(
            "no valid .obj/.glb part files were found in '{}'",
            input_dir.display()
        ));
    }

    let part_count = pending.len();
    let part_count_u32 =
        u32::try_from(part_count).map_err(|_| format!("too many parts ({part_count}) to represent in this format's u32 part_count field"))?;
    let morph_count = pending_morphs.len();
    let morph_count_u32 = u32::try_from(morph_count)
        .map_err(|_| format!("too many morphs ({morph_count}) to represent in this format's u32 morph_count field"))?;

    // Read every part's raw bytes up front, so we know each one's length
    // before laying out the index table's data_offset/data_len fields --
    // and so the same already-read bytes can be reused below to parse
    // this part's own render-vertex positions for the duplication map,
    // rather than re-reading the file from disk a second time.
    let mut part_bytes: Vec<Vec<u8>> = Vec::with_capacity(part_count);
    for p in &pending {
        let bytes = fs::read(&p.path)
            .map_err(|e| format!("failed to read part file '{}': {e}", p.path.display()))?;
        part_bytes.push(bytes);
    }

    let mut morph_bytes: Vec<Vec<u8>> = Vec::with_capacity(morph_count);
    for m in &pending_morphs {
        let bytes = fs::read(&m.path)
            .map_err(|e| format!("failed to read morph file '{}': {e}", m.path.display()))?;
        morph_bytes.push(bytes);
    }

    // Duplication maps, one per part, in the same order as `pending` --
    // `None` when no base mesh was given (dupmap_len stays 0 for every
    // part in that case).
    let mut dupmaps: Vec<Option<Vec<u32>>> = Vec::with_capacity(part_count);
    if let Some(base_positions) = &base_mesh_positions {
        for (p, bytes) in pending.iter().zip(part_bytes.iter()) {
            let render_positions = match p.src_type {
                0 => parse_obj_positions(std::str::from_utf8(bytes).map_err(|e| {
                    format!(
                        "part file '{}' (part id {}) is not valid UTF-8 OBJ text: {e}",
                        p.path.display(),
                        p.part_id
                    )
                })?),
                1 => parse_glb_positions(bytes).map_err(|e| {
                    format!(
                        "part file '{}' (part id {}): {e}",
                        p.path.display(),
                        p.part_id
                    )
                })?,
                other => {
                    return Err(format!(
                        "part id {}: internal error, unexpected src_type {other}",
                        p.part_id
                    ))
                }
            };
            let map = compute_duplication_map(&render_positions, base_positions, 0.0005);
            dupmaps.push(Some(map));
        }
    } else {
        for _ in &pending {
            dupmaps.push(None);
        }
    }

    // Header: magic(4) + version(4) + part_count(4) + morph_count(4) +
    // skel_len(4) = 20.
    let header_len: usize = 20;
    let part_index_table_len: usize = part_count * 21;
    let morph_index_table_len: usize = morph_count * 12;
    let skeleton_region_start: usize = header_len;
    let part_index_start: usize = skeleton_region_start + skeleton_bytes.len();
    let morph_index_start: usize = part_index_start + part_index_table_len;
    let part_data_region_start: usize = morph_index_start + morph_index_table_len;

    // Compute each part's absolute data_offset/data_len, laid out
    // contiguously immediately after the morph index table.
    struct PartIndexEntry {
        part_id: u32,
        src_type: u8,
        data_offset: u32,
        data_len: u32,
        dupmap_offset: u32,
        dupmap_len: u32,
    }

    let mut part_index_entries: Vec<PartIndexEntry> = Vec::with_capacity(part_count);
    let mut running_offset: usize = part_data_region_start;
    for (p, bytes) in pending.iter().zip(part_bytes.iter()) {
        let data_offset = u32::try_from(running_offset).map_err(|_| {
            format!(
                "output pack would exceed this format's u32 offset range while placing part id {}",
                p.part_id
            )
        })?;
        let data_len = u32::try_from(bytes.len()).map_err(|_| {
            format!(
                "part file '{}' (part id {}) is too large to represent in this format's u32 length field ({} bytes)",
                p.path.display(),
                p.part_id,
                bytes.len()
            )
        })?;
        part_index_entries.push(PartIndexEntry {
            part_id: p.part_id,
            src_type: p.src_type,
            data_offset,
            data_len,
            dupmap_offset: 0, // filled in below, once part data's total length is known
            dupmap_len: 0,
        });
        running_offset += bytes.len();
    }

    // Duplication-map bytes come right after all part source bytes.
    let dupmap_region_start = running_offset;
    for (entry, dupmap) in part_index_entries.iter_mut().zip(dupmaps.iter()) {
        if let Some(map) = dupmap {
            if map.is_empty() {
                continue; // dupmap_offset/dupmap_len stay 0, same as "no map"
            }
            let dupmap_offset = u32::try_from(running_offset).map_err(|_| {
                format!(
                    "output pack would exceed this format's u32 offset range while placing part id {}'s duplication map",
                    entry.part_id
                )
            })?;
            let dupmap_byte_len = map.len() * 4;
            let dupmap_len = u32::try_from(dupmap_byte_len).map_err(|_| {
                format!(
                    "part id {}'s duplication map ({dupmap_byte_len} bytes) is too large to represent in this format's u32 length field",
                    entry.part_id
                )
            })?;
            entry.dupmap_offset = dupmap_offset;
            entry.dupmap_len = dupmap_len;
            running_offset += dupmap_byte_len;
        }
    }

    // Morph bytes come right after all duplication-map bytes.
    struct MorphIndexEntry {
        morph_id: u16,
        data_offset: u32,
        data_len: u32,
    }

    // Morph blob dedup (CC0-Phase 10 Part B). Two morph ids whose blob bytes
    // are identical share one physical copy: both index entries point at the
    // same (data_offset, data_len). The AFPP format and reader are unchanged
    // (the index already stores an independent offset/len per morph). Blob
    // bytes are treated as opaque, so this works for AFMT v1 and v2 alike.
    //
    // Hash: std `DefaultHasher` (SipHash, 64-bit) over the blob bytes -- not
    // cryptographic, used only to bucket candidates. A hash match is NEVER
    // trusted on its own: candidates are confirmed by full byte-for-byte
    // comparison, so a hash collision can only cost a comparison, never
    // silently alias two different morphs.
    let mut morph_index_entries: Vec<MorphIndexEntry> = Vec::with_capacity(morph_count);
    // Indices into `morph_bytes` of the blobs actually written, in write order.
    let mut unique_morph_blobs: Vec<usize> = Vec::new();
    // hash -> (blob index into morph_bytes, data_offset, data_len) candidates.
    let mut seen_blobs: HashMap<u64, Vec<(usize, u32, u32)>> = HashMap::new();
    for (i, (m, bytes)) in pending_morphs.iter().zip(morph_bytes.iter()).enumerate() {
        let data_len = u32::try_from(bytes.len()).map_err(|_| {
            format!(
                "morph file '{}' (morph id {}) is too large to represent in this format's u32 length field ({} bytes)",
                m.path.display(),
                m.morph_id,
                bytes.len()
            )
        })?;
        let key = dedup_view(bytes, ignore_afmt_id);
        let hash = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            key.hash(&mut h);
            h.finish()
        };
        let bucket = seen_blobs.entry(hash).or_default();
        if let Some(&(_, off, len)) = bucket
            .iter()
            .find(|&&(j, _, len)| len == data_len && dedup_view(&morph_bytes[j], ignore_afmt_id) == key)
        {
            morph_index_entries.push(MorphIndexEntry {
                morph_id: m.morph_id,
                data_offset: off,
                data_len: len,
            });
            continue;
        }
        let data_offset = u32::try_from(running_offset).map_err(|_| {
            format!(
                "output pack would exceed this format's u32 offset range while placing morph id {}",
                m.morph_id
            )
        })?;
        bucket.push((i, data_offset, data_len));
        unique_morph_blobs.push(i);
        morph_index_entries.push(MorphIndexEntry {
            morph_id: m.morph_id,
            data_offset,
            data_len,
        });
        running_offset += bytes.len();
    }
    eprintln!(
        "pack_builder: morph dedup: {} morphs -> {} unique blobs",
        morph_count,
        unique_morph_blobs.len()
    );

    let total_len = running_offset;
    let mut out: Vec<u8> = Vec::with_capacity(total_len);

    // Header.
    out.extend_from_slice(b"AFPP");
    out.extend_from_slice(&2u32.to_le_bytes()); // version
    out.extend_from_slice(&part_count_u32.to_le_bytes());
    out.extend_from_slice(&morph_count_u32.to_le_bytes());
    out.extend_from_slice(&skel_len.to_le_bytes());

    // Skeleton bytes.
    out.extend_from_slice(&skeleton_bytes);
    debug_assert_eq!(out.len(), skeleton_region_start + skeleton_bytes.len());
    debug_assert_eq!(out.len(), part_index_start);

    // Part index table.
    for entry in &part_index_entries {
        out.extend_from_slice(&entry.part_id.to_le_bytes());
        out.push(entry.src_type);
        out.extend_from_slice(&entry.data_offset.to_le_bytes());
        out.extend_from_slice(&entry.data_len.to_le_bytes());
        out.extend_from_slice(&entry.dupmap_offset.to_le_bytes());
        out.extend_from_slice(&entry.dupmap_len.to_le_bytes());
    }
    debug_assert_eq!(out.len(), morph_index_start);

    // Morph index table.
    for entry in &morph_index_entries {
        out.extend_from_slice(&entry.morph_id.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // _reserved
        out.extend_from_slice(&entry.data_offset.to_le_bytes());
        out.extend_from_slice(&entry.data_len.to_le_bytes());
    }
    debug_assert_eq!(out.len(), part_data_region_start);

    // Part data, in the same order as the part index table.
    for bytes in &part_bytes {
        out.extend_from_slice(bytes);
    }
    debug_assert_eq!(out.len(), dupmap_region_start);

    // Duplication-map data, in the same order as the part index table
    // (only for parts with a non-empty map).
    for (entry, dupmap) in part_index_entries.iter().zip(dupmaps.iter()) {
        if entry.dupmap_len == 0 {
            continue;
        }
        let map = dupmap.as_ref().expect("dupmap_len > 0 implies Some(map)");
        for &v in map {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }

    // Morph data: each unique blob once, in first-occurrence order (morphs
    // that deduplicated onto an earlier blob contribute no bytes here).
    for &i in &unique_morph_blobs {
        out.extend_from_slice(&morph_bytes[i]);
    }
    debug_assert_eq!(out.len(), total_len);

    fs::write(output_path, &out)
        .map_err(|e| format!("failed to write output pack '{}': {e}", output_path.display()))?;

    Ok((part_count, morph_count, total_len))
}

/// The bytes two morph blobs must share to be considered duplicates.
///
/// Default (`ignore_afmt_id == false`): the whole blob -- fully opaque.
/// NOTE: `morph_converter` embeds the morph's own id at blob offset 8..10, so
/// under the opaque rule two morphs are never byte-identical even when their
/// deltas are, and dedup finds nothing on a real converter-produced corpus.
///
/// With `--dedup-ignore-afmt-morph-id`: for blobs that start with `AFMT` and
/// version 1 OR version 2 (and are >= 14 bytes) the 2-byte embedded morph_id
/// is excluded from the comparison. Both versions place the id at the same
/// offset (bytes 8..10), so the same slice split (`&bytes[..8]` +
/// `&bytes[10..]`) is correct for either: everything from byte 10 onward
/// (v1's f32 deltas, or v2's scales+quantized deltas) is compared as an
/// opaque tail. The returned tuple's first field records whether the blob was
/// normalized, so a normalized blob never equals a non-normalized one.
/// Consequence: an aliased morph's index entry points at a blob whose embedded
/// id is the FIRST morph's id. The current loader keys morphs by the pack
/// index's morph_id and never reads the embedded one, so behavior is
/// unchanged, but `LoadedMorph.morph_id` for aliased morphs would be the
/// canonical morph's id.
fn dedup_view(bytes: &[u8], ignore_afmt_id: bool) -> (bool, &[u8], &[u8]) {
    if ignore_afmt_id
        && bytes.len() >= 14
        && &bytes[0..4] == b"AFMT"
        && (bytes[4..8] == 1u32.to_le_bytes() || bytes[4..8] == 2u32.to_le_bytes())
    {
        (true, &bytes[..8], &bytes[10..])
    } else {
        (false, bytes, &[])
    }
}

/// Part/morph ids are taken from the leading run of ASCII digits in the
/// file stem, e.g. `1001_head_male.glb` -> `1001`. Returns `None` (skip,
/// don't error the whole run out) if the filename doesn't start with a
/// digit.
fn parse_part_id(path: &Path) -> Option<u32> {
    let stem = path.file_stem()?.to_str()?;
    let digits: String = stem.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u32>().ok()
}

/// Minimal inline OBJ `v x y z` line reader, deliberately not sharing any
/// code with `obj_loader.rs` or `vertex_duplication_map.rs`'s own
/// (test-only, file-based) equivalent -- this binary must stay
/// dependency-free of the library. Ignores every other OBJ line type
/// (`f`, `vn`, `vt`, comments); only position rows matter for duplication-
/// map computation.
fn parse_obj_positions(text: &str) -> Vec<[f32; 3]> {
    let mut positions = Vec::new();
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        if fields.next() != Some("v") {
            continue;
        }
        let (Some(x), Some(y), Some(z)) = (fields.next(), fields.next(), fields.next()) else {
            continue;
        };
        let (Ok(x), Ok(y), Ok(z)) = (x.parse::<f32>(), y.parse::<f32>(), z.parse::<f32>()) else {
            continue;
        };
        positions.push([x, y, z]);
    }
    positions
}

/// Minimal inline GLB reader that pulls out the `POSITION` accessor's raw
/// `f32` triples, deliberately not sharing any code with `gltf_loader.rs`
/// -- this binary must stay dependency-free of the library. Only handles
/// exactly what real CC0-Phase 2 GLB assets need: a two-chunk GLB (JSON +
/// BIN), with the POSITION accessor's bufferView tightly packed (no
/// interleaving, no extra byteOffset/byteStride) -- the same shape
/// `vertex_duplication_map.rs`'s own tests already assume for these
/// fixtures.
fn parse_glb_positions(bytes: &[u8]) -> Result<Vec<[f32; 3]>, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"glTF" {
        return Err("not a GLB file (bad or missing magic)".to_string());
    }

    let mut offset = 12usize;
    let mut json_chunk: Option<&[u8]> = None;
    let mut bin_chunk: Option<&[u8]> = None;

    while offset + 8 <= bytes.len() {
        let chunk_length = u32::from_le_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .map_err(|_| "truncated GLB chunk header".to_string())?,
        ) as usize;
        let chunk_type = &bytes[offset + 4..offset + 8];
        let chunk_start = offset + 8;
        let chunk_end = chunk_start
            .checked_add(chunk_length)
            .ok_or_else(|| "GLB chunk length overflows buffer offset arithmetic".to_string())?;
        if chunk_end > bytes.len() {
            return Err("GLB chunk extends past end of buffer".to_string());
        }
        let chunk_data = &bytes[chunk_start..chunk_end];

        match chunk_type {
            b"JSON" => json_chunk = Some(chunk_data),
            b"BIN\0" => bin_chunk = Some(chunk_data),
            _ => {}
        }

        offset = chunk_end;
    }

    let json_chunk = json_chunk.ok_or("GLB missing JSON chunk")?;
    let bin_chunk = bin_chunk.ok_or("GLB missing BIN chunk")?;
    let json_text =
        std::str::from_utf8(json_chunk).map_err(|e| format!("JSON chunk not valid utf-8: {e}"))?;
    let json: serde_json::Value =
        serde_json::from_str(json_text).map_err(|e| format!("failed to parse glTF JSON: {e}"))?;

    let accessor_index = json["meshes"][0]["primitives"][0]["attributes"]["POSITION"]
        .as_u64()
        .ok_or("no POSITION attribute on first mesh primitive")? as usize;

    let accessor = &json["accessors"][accessor_index];
    let count = accessor["count"].as_u64().ok_or("accessor missing count")? as usize;
    let component_type = accessor["componentType"]
        .as_u64()
        .ok_or("accessor missing componentType")?;
    if component_type != 5126 {
        return Err(format!(
            "expected float32 (componentType 5126) POSITION accessor, found {component_type}"
        ));
    }
    if accessor["type"].as_str() != Some("VEC3") {
        return Err("expected VEC3 POSITION accessor".to_string());
    }

    let buffer_view_index = accessor["bufferView"]
        .as_u64()
        .ok_or("accessor missing bufferView")? as usize;
    let buffer_view = &json["bufferViews"][buffer_view_index];
    let byte_offset = buffer_view["byteOffset"].as_u64().unwrap_or(0) as usize;

    let mut positions = Vec::with_capacity(count);
    for i in 0..count {
        let base = byte_offset + i * 12; // tightly packed VEC3<f32>
        if base + 12 > bin_chunk.len() {
            return Err("POSITION accessor reads past end of BIN chunk".to_string());
        }
        let x = f32::from_le_bytes(bin_chunk[base..base + 4].try_into().unwrap());
        let y = f32::from_le_bytes(bin_chunk[base + 4..base + 8].try_into().unwrap());
        let z = f32::from_le_bytes(bin_chunk[base + 8..base + 12].try_into().unwrap());
        positions.push([x, y, z]);
    }

    Ok(positions)
}

/// Render-vertex -> base-mesh-vertex duplication map, by brute-force
/// nearest-neighbor position matching. Deliberately duplicated from
/// `vertex_duplication_map::build_duplication_map` rather than imported
/// (see this file's header comment) -- kept behaviorally identical,
/// including the strictly-less-than tie-break-to-lower-index rule.
fn compute_duplication_map(
    render_positions: &[[f32; 3]],
    base_mesh_positions: &[[f32; 3]],
    tolerance: f32,
) -> Vec<u32> {
    const NO_BASE_MESH_MATCH: u32 = u32::MAX;

    let mut result = Vec::with_capacity(render_positions.len());

    for render_position in render_positions {
        let mut best_index: Option<usize> = None;
        let mut best_distance_squared = f32::INFINITY;

        for (base_index, base_position) in base_mesh_positions.iter().enumerate() {
            let dx = render_position[0] - base_position[0];
            let dy = render_position[1] - base_position[1];
            let dz = render_position[2] - base_position[2];
            let distance_squared = dx * dx + dy * dy + dz * dz;

            if distance_squared < best_distance_squared {
                best_distance_squared = distance_squared;
                best_index = Some(base_index);
            }
        }

        let mapped = match best_index {
            Some(index) if best_distance_squared.sqrt() <= tolerance => {
                u32::try_from(index).unwrap_or(NO_BASE_MESH_MATCH)
            }
            _ => NO_BASE_MESH_MATCH,
        };

        result.push(mapped);
    }

    result
}

// ============================================================================
// Structural self-check: writes a tiny fixture pack in-memory (via `run`,
// against a real tempdir on disk) and reads it back byte-by-byte, verifying
// magic/version/part_count/morph_count and that every index entry's
// data_offset + data_len (and, where present, dupmap_offset + dupmap_len)
// stays within bounds. Does not parse OBJ/glTF content itself for the
// *embedding* path -- this tool only needs to embed that content
// correctly -- but does exercise the real position-parsing + duplication-
// map path when a base mesh is supplied.
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn unique_tmp_dir(name: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "pack_builder_test_{name}_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn structural_round_trip_v2_no_base_mesh() {
        let dir = unique_tmp_dir("roundtrip_v2");
        fs::write(
            dir.join("master_skeleton.json"),
            br#"{"bones":{"root":0}}"#,
        )
        .unwrap();
        fs::write(dir.join("1001_head.obj"), b"v 0 0 0\n").unwrap();
        fs::write(dir.join("1002_torso.glb"), b"glTF\x02\x00\x00\x00fakebinarydata").unwrap();

        let out_path = dir.join("out.afpp");
        let (part_count, morph_count, total_len) =
            run(&dir, &out_path, None, false).expect("run should succeed");
        assert_eq!(part_count, 2);
        assert_eq!(morph_count, 0);

        let buf = fs::read(&out_path).unwrap();
        assert_eq!(buf.len(), total_len);

        assert_eq!(&buf[0..4], b"AFPP");
        let version = u32::from_le_bytes(buf[4..8].try_into().unwrap());
        assert_eq!(version, 2);
        let part_count_field = u32::from_le_bytes(buf[8..12].try_into().unwrap());
        assert_eq!(part_count_field as usize, part_count);
        let morph_count_field = u32::from_le_bytes(buf[12..16].try_into().unwrap());
        assert_eq!(morph_count_field as usize, morph_count);
        let skel_len = u32::from_le_bytes(buf[16..20].try_into().unwrap()) as usize;

        let index_start = 20 + skel_len;
        for i in 0..part_count {
            let entry_start = index_start + i * 21;
            let entry = &buf[entry_start..entry_start + 21];
            let data_offset = u32::from_le_bytes(entry[5..9].try_into().unwrap()) as usize;
            let data_len = u32::from_le_bytes(entry[9..13].try_into().unwrap()) as usize;
            let dupmap_len = u32::from_le_bytes(entry[17..21].try_into().unwrap()) as usize;
            assert!(data_offset + data_len <= buf.len(), "index entry {i} reads out of bounds");
            assert_eq!(dupmap_len, 0, "no base mesh was supplied, dupmap_len must be 0");
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn duplication_map_computed_when_base_mesh_given() {
        let dir = unique_tmp_dir("dupmap");
        fs::write(
            dir.join("master_skeleton.json"),
            br#"{"bones":{"root":0}}"#,
        )
        .unwrap();
        // A tiny OBJ part whose 2 vertices exactly match base-mesh
        // indices 2 and 0 respectively.
        fs::write(
            dir.join("1001_head.obj"),
            b"v 5.0 0.0 0.0\nv 0.0 0.0 0.0\nf 1 1 1\n",
        )
        .unwrap();

        let base_mesh_path = dir.join("base.obj");
        fs::write(
            &base_mesh_path,
            b"v 0.0 0.0 0.0\nv 1.0 0.0 0.0\nv 5.0 0.0 0.0\n",
        )
        .unwrap();

        let out_path = dir.join("out.afpp");
        let (part_count, _morph_count, _total_len) =
            run(&dir, &out_path, Some(&base_mesh_path), false).expect("run should succeed");
        assert_eq!(part_count, 1);

        let buf = fs::read(&out_path).unwrap();
        let skel_len = u32::from_le_bytes(buf[16..20].try_into().unwrap()) as usize;
        let index_start = 20 + skel_len;
        let entry = &buf[index_start..index_start + 21];
        let dupmap_offset = u32::from_le_bytes(entry[13..17].try_into().unwrap()) as usize;
        let dupmap_len = u32::from_le_bytes(entry[17..21].try_into().unwrap()) as usize;
        assert_eq!(dupmap_len, 8, "2 render vertices * 4 bytes each");

        let dupmap_bytes = &buf[dupmap_offset..dupmap_offset + dupmap_len];
        let first = u32::from_le_bytes(dupmap_bytes[0..4].try_into().unwrap());
        let second = u32::from_le_bytes(dupmap_bytes[4..8].try_into().unwrap());
        assert_eq!(first, 2, "render vertex 0 (5,0,0) should match base index 2");
        assert_eq!(second, 0, "render vertex 1 (0,0,0) should match base index 0");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn afmt_file_is_treated_as_a_morph_not_a_part() {
        let dir = unique_tmp_dir("morph");
        fs::write(
            dir.join("master_skeleton.json"),
            br#"{"bones":{"root":0}}"#,
        )
        .unwrap();
        fs::write(dir.join("1001_head.obj"), b"v 0 0 0\n").unwrap();
        let fake_afmt = b"AFMTfake-afmt-bytes-unmodified";
        fs::write(dir.join("5001_ear.afmt"), fake_afmt).unwrap();

        let out_path = dir.join("out.afpp");
        let (part_count, morph_count, _total_len) =
            run(&dir, &out_path, None, false).expect("run should succeed");
        assert_eq!(part_count, 1, "the .afmt file must not be counted as a part");
        assert_eq!(morph_count, 1);

        let buf = fs::read(&out_path).unwrap();
        let skel_len = u32::from_le_bytes(buf[16..20].try_into().unwrap()) as usize;
        let part_index_start = 20 + skel_len;
        let morph_index_start = part_index_start + part_count * 21;
        let morph_entry = &buf[morph_index_start..morph_index_start + 12];
        let morph_id = u16::from_le_bytes(morph_entry[0..2].try_into().unwrap());
        assert_eq!(morph_id, 5001);
        let data_offset = u32::from_le_bytes(morph_entry[4..8].try_into().unwrap()) as usize;
        let data_len = u32::from_le_bytes(morph_entry[8..12].try_into().unwrap()) as usize;
        assert_eq!(&buf[data_offset..data_offset + data_len], fake_afmt);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn duplicate_part_id_errors() {
        let dir = unique_tmp_dir("dupe");
        fs::write(dir.join("master_skeleton.json"), br#"{"bones":{"root":0}}"#).unwrap();
        fs::write(dir.join("1001_head.obj"), b"v 0 0 0\n").unwrap();
        fs::write(dir.join("1001_head_alt.glb"), b"glTF\x02\x00\x00\x00x").unwrap();

        let out_path = dir.join("out.afpp");
        let result = run(&dir, &out_path, None, false);
        assert!(result.is_err());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn gltf_file_is_skipped_with_warning_and_run_still_succeeds() {
        let dir = unique_tmp_dir("gltfskip");
        fs::write(dir.join("master_skeleton.json"), br#"{"bones":{"root":0}}"#).unwrap();
        fs::write(dir.join("1001_head.obj"), b"v 0 0 0\n").unwrap();
        fs::write(dir.join("2002_legacy.gltf"), b"{\"not\":\"embedded\"}").unwrap();

        let out_path = dir.join("out.afpp");
        let result = run(&dir, &out_path, None, false);
        assert!(result.is_ok());
        let (part_count, _morph_count, _) = result.unwrap();
        // Only the .obj part should have been accepted; the .gltf is skipped.
        assert_eq!(part_count, 1);

        let _ = fs::remove_dir_all(&dir);
    }
}
