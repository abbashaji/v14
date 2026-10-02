//! `pack_strip <input.afpp> <output.afpp> <notice.txt>`
//!
//! Rewrites an AFPP v2 pack without its denied morph ids (see
//! `anthroforge_core::is_denied_morph_id`) and appends a notice footer.
//!
//! Exit codes: 0 success; 1 input, format or I/O error (stderr line starts
//! `pack_strip: error:`, an existing output file is left untouched); 2 usage.
//!
//! Output layout, no gap and no padding, all integers little-endian:
//!
//! (a) header: `AFPP`, version 2, part_count, kept morph count, skel_len
//! (b) skeleton bytes, copied verbatim (never parsed)
//! (c) part index, input order, new absolute offsets
//! (d) kept morph index, input order, reserved 0, new blob offset
//! (e) part sources, part order
//! (f) duplication maps of the parts whose dupmap_len > 0, part order
//! (g) unique kept morph blobs, first-reference order over the kept index
//! (h) footer: normalized notice bytes, notice length (u32), `AFNT`

use anthroforge_core::is_denied_morph_id;
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const HEADER_LEN: usize = 20;
const PART_ENTRY_LEN: usize = 21;
const MORPH_ENTRY_LEN: usize = 12;
const MAX_NOTICE_LEN: usize = 65536;
const FOOTER_TAG: &[u8; 4] = b"AFNT";
const USAGE: &str = "usage: pack_strip <input.afpp> <output.afpp> <notice.txt>";

struct PartEntry {
    part_id: u32,
    src_type: u8,
    data_offset: u32,
    data_len: u32,
    dupmap_offset: u32,
    dupmap_len: u32,
}

struct MorphEntry {
    morph_id: u16,
    data_offset: u32,
    data_len: u32,
}

struct Pack<'a> {
    skel_len: u32,
    skeleton: &'a [u8],
    parts: Vec<PartEntry>,
    morphs: Vec<MorphEntry>,
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();
    if args.len() != 4 {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let input = Path::new(&args[1]);
    let output = Path::new(&args[2]);
    let notice = Path::new(&args[3]);

    if same_file(input, output) {
        eprintln!("pack_strip: usage: the output path names the same file as the input");
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }

    match run(input, output, notice) {
        Ok(summary) => {
            let _ = writeln!(std::io::stdout(), "{summary}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            let _ = writeln!(std::io::stderr(), "pack_strip: error: {message}");
            ExitCode::from(1)
        }
    }
}

/// Whether `output` names the same file as `input`, comparing canonical
/// forms. A not-yet-existing output is canonicalized through its parent
/// directory joined with its file name. When either side cannot be
/// canonicalized the answer is `false`; a real I/O problem then surfaces
/// later as an ordinary error.
fn same_file(input: &Path, output: &Path) -> bool {
    let Ok(canonical_input) = fs::canonicalize(input) else {
        return false;
    };
    let canonical_output = match fs::canonicalize(output) {
        Ok(p) => p,
        Err(_) => {
            let Some(name) = output.file_name() else {
                return false;
            };
            let Ok(parent) = fs::canonicalize(parent_dir(output)) else {
                return false;
            };
            parent.join(name)
        }
    };
    canonical_input == canonical_output
}

fn parent_dir(path: &Path) -> PathBuf {
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// CRLF pairs become LF; a lone CR stays; nothing is added or removed.
fn normalize_notice(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let byte = raw[i];
        if byte == b'\r' && raw.get(i + 1) == Some(&b'\n') {
            // Drop the CR, keep the LF on the next turn.
            i += 1;
            continue;
        }
        out.push(byte);
        i += 1;
    }
    out
}

fn read_notice(path: &Path) -> Result<Vec<u8>, String> {
    let raw = fs::read(path).map_err(|e| format!("cannot read notice {}: {e}", path.display()))?;
    let notice = normalize_notice(&raw);
    if notice.is_empty() {
        return Err(format!("notice {} is empty", path.display()));
    }
    if notice.len() > MAX_NOTICE_LEN {
        return Err(format!(
            "notice {} is {} bytes, more than the {MAX_NOTICE_LEN} byte limit",
            path.display(),
            notice.len()
        ));
    }
    if std::str::from_utf8(&notice).is_err() {
        return Err(format!("notice {} is not valid UTF-8", path.display()));
    }
    Ok(notice)
}

fn le_u32(bytes: &[u8], at: usize) -> Result<u32, String> {
    let end = at.checked_add(4).ok_or_else(|| "offset overflow".to_string())?;
    let slice = bytes
        .get(at..end)
        .ok_or_else(|| format!("read of 4 bytes at offset {at} is past the end"))?;
    let array: [u8; 4] = slice
        .try_into()
        .map_err(|_| "internal error: 4-byte slice conversion".to_string())?;
    Ok(u32::from_le_bytes(array))
}

fn le_u16(bytes: &[u8], at: usize) -> Result<u16, String> {
    let end = at.checked_add(2).ok_or_else(|| "offset overflow".to_string())?;
    let slice = bytes
        .get(at..end)
        .ok_or_else(|| format!("read of 2 bytes at offset {at} is past the end"))?;
    let array: [u8; 2] = slice
        .try_into()
        .map_err(|_| "internal error: 2-byte slice conversion".to_string())?;
    Ok(u16::from_le_bytes(array))
}

/// `bytes[offset..offset + len]`, or an error naming `what`.
fn range<'a>(bytes: &'a [u8], offset: u32, len: u32, what: &str) -> Result<&'a [u8], String> {
    let start = offset as usize;
    let end = start
        .checked_add(len as usize)
        .ok_or_else(|| format!("{what}: offset + length overflows"))?;
    bytes.get(start..end).ok_or_else(|| {
        format!(
            "{what}: range {start}..{end} exceeds the input's {} byte(s)",
            bytes.len()
        )
    })
}

/// A table of `count` entries of `entry_len` bytes starting at `start`.
fn table<'a>(
    bytes: &'a [u8],
    start: usize,
    count: usize,
    entry_len: usize,
    what: &str,
) -> Result<&'a [u8], String> {
    let len = count
        .checked_mul(entry_len)
        .ok_or_else(|| format!("{what} length overflows"))?;
    let end = start
        .checked_add(len)
        .ok_or_else(|| format!("{what} length overflows"))?;
    bytes.get(start..end).ok_or_else(|| {
        format!(
            "{what} (offset {start}..{end}, {count} entries) exceeds the input's {} byte(s)",
            bytes.len()
        )
    })
}

fn parse(bytes: &[u8]) -> Result<Pack<'_>, String> {
    if bytes.len() < HEADER_LEN {
        return Err(format!(
            "input is {} byte(s), shorter than the {HEADER_LEN} byte header",
            bytes.len()
        ));
    }
    if bytes.get(0..4) != Some(&b"AFPP"[..]) {
        return Err("bad magic: the input is not an AFPP pack".to_string());
    }
    let version = le_u32(bytes, 4)?;
    if version != 2 {
        return Err(format!("unsupported pack version {version}, expected 2"));
    }
    let part_count = le_u32(bytes, 8)? as usize;
    let morph_count = le_u32(bytes, 12)? as usize;
    let skel_len = le_u32(bytes, 16)?;

    let skeleton_end = HEADER_LEN
        .checked_add(skel_len as usize)
        .ok_or_else(|| "skeleton length overflows".to_string())?;
    let skeleton = bytes.get(HEADER_LEN..skeleton_end).ok_or_else(|| {
        format!(
            "skeleton (offset {HEADER_LEN}..{skeleton_end}) exceeds the input's {} byte(s)",
            bytes.len()
        )
    })?;

    let part_table = table(bytes, skeleton_end, part_count, PART_ENTRY_LEN, "part index table")?;
    let morph_start = skeleton_end + part_table.len();
    let morph_table = table(bytes, morph_start, morph_count, MORPH_ENTRY_LEN, "morph index table")?;

    let mut parts = Vec::with_capacity(part_count);
    let mut seen_parts: HashSet<u32> = HashSet::new();
    for raw in part_table.chunks_exact(PART_ENTRY_LEN) {
        let entry = PartEntry {
            part_id: le_u32(raw, 0)?,
            src_type: *raw.get(4).ok_or_else(|| "short part entry".to_string())?,
            data_offset: le_u32(raw, 5)?,
            data_len: le_u32(raw, 9)?,
            dupmap_offset: le_u32(raw, 13)?,
            dupmap_len: le_u32(raw, 17)?,
        };
        range(
            bytes,
            entry.data_offset,
            entry.data_len,
            &format!("part_id {} source", entry.part_id),
        )?;
        if entry.dupmap_len != 0 {
            if entry.dupmap_len % 4 != 0 {
                return Err(format!(
                    "part_id {}: dupmap_len {} is not a multiple of 4",
                    entry.part_id, entry.dupmap_len
                ));
            }
            range(
                bytes,
                entry.dupmap_offset,
                entry.dupmap_len,
                &format!("part_id {} duplication map", entry.part_id),
            )?;
        }
        if !seen_parts.insert(entry.part_id) {
            return Err(format!("duplicate part_id {}", entry.part_id));
        }
        parts.push(entry);
    }

    let mut morphs = Vec::with_capacity(morph_count);
    let mut seen_morphs: HashSet<u16> = HashSet::new();
    for raw in morph_table.chunks_exact(MORPH_ENTRY_LEN) {
        let entry = MorphEntry {
            morph_id: le_u16(raw, 0)?,
            // raw[2..4] is the reserved field: never read, never copied.
            data_offset: le_u32(raw, 4)?,
            data_len: le_u32(raw, 8)?,
        };
        range(
            bytes,
            entry.data_offset,
            entry.data_len,
            &format!("morph_id {} blob", entry.morph_id),
        )?;
        if !seen_morphs.insert(entry.morph_id) {
            return Err(format!("duplicate morph_id {}", entry.morph_id));
        }
        morphs.push(entry);
    }

    Ok(Pack {
        skel_len,
        skeleton,
        parts,
        morphs,
    })
}

fn add(a: u64, b: u64) -> Result<u64, String> {
    a.checked_add(b)
        .ok_or_else(|| "output length overflows".to_string())
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn build(bytes: &[u8], pack: &Pack<'_>, notice: &[u8]) -> Result<(Vec<u8>, usize), String> {
    let kept: Vec<&MorphEntry> = pack
        .morphs
        .iter()
        .filter(|m| !is_denied_morph_id(m.morph_id))
        .collect();
    let dropped = pack.morphs.len() - kept.len();

    // Plan: every section start, in the order of the layout.
    let part_index_start = add(HEADER_LEN as u64, u64::from(pack.skel_len))?;
    let morph_index_start = add(part_index_start, pack.parts.len() as u64 * PART_ENTRY_LEN as u64)?;
    let sources_start = add(morph_index_start, kept.len() as u64 * MORPH_ENTRY_LEN as u64)?;

    let mut source_offsets: Vec<u64> = Vec::with_capacity(pack.parts.len());
    let mut cursor = sources_start;
    for part in &pack.parts {
        source_offsets.push(cursor);
        cursor = add(cursor, u64::from(part.data_len))?;
    }

    let mut dupmap_offsets: Vec<u64> = Vec::with_capacity(pack.parts.len());
    for part in &pack.parts {
        if part.dupmap_len > 0 {
            dupmap_offsets.push(cursor);
            cursor = add(cursor, u64::from(part.dupmap_len))?;
        } else {
            dupmap_offsets.push(0);
        }
    }

    // Unique blobs by input (offset, length), first reference first.
    let mut blob_slot: HashMap<(u32, u32), usize> = HashMap::new();
    let mut blobs: Vec<(u32, u32)> = Vec::new();
    let mut blob_offsets: Vec<u64> = Vec::new();
    let mut kept_blob_offsets: Vec<u64> = Vec::with_capacity(kept.len());
    for morph in &kept {
        let key = (morph.data_offset, morph.data_len);
        let slot = match blob_slot.get(&key) {
            Some(&slot) => slot,
            None => {
                let slot = blobs.len();
                blob_slot.insert(key, slot);
                blobs.push(key);
                blob_offsets.push(cursor);
                cursor = add(cursor, u64::from(morph.data_len))?;
                slot
            }
        };
        let offset = *blob_offsets
            .get(slot)
            .ok_or_else(|| "internal error: blob slot".to_string())?;
        kept_blob_offsets.push(offset);
    }
    let blobs_end = cursor;

    let footer_len = notice.len() as u64 + 8;
    let total = add(blobs_end, footer_len)?;
    if total > u64::from(u32::MAX) {
        return Err(format!(
            "output would be {total} bytes, longer than the u32 limit of {} bytes",
            u32::MAX
        ));
    }

    // Write.
    let mut out: Vec<u8> = Vec::with_capacity(total.min(1 << 28) as usize);
    out.extend_from_slice(b"AFPP");
    push_u32(&mut out, 2);
    push_u32(&mut out, pack.parts.len() as u32);
    push_u32(&mut out, kept.len() as u32);
    push_u32(&mut out, pack.skel_len);
    out.extend_from_slice(pack.skeleton);

    for (i, part) in pack.parts.iter().enumerate() {
        push_u32(&mut out, part.part_id);
        out.push(part.src_type);
        push_u32(&mut out, *source_offsets.get(i).ok_or("internal error: source offset")? as u32);
        push_u32(&mut out, part.data_len);
        push_u32(&mut out, *dupmap_offsets.get(i).ok_or("internal error: dupmap offset")? as u32);
        push_u32(&mut out, part.dupmap_len);
    }

    for (morph, offset) in kept.iter().zip(&kept_blob_offsets) {
        out.extend_from_slice(&morph.morph_id.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        push_u32(&mut out, *offset as u32);
        push_u32(&mut out, morph.data_len);
    }

    for part in &pack.parts {
        let src = range(bytes, part.data_offset, part.data_len, "part source")?;
        out.extend_from_slice(src);
    }

    for part in &pack.parts {
        if part.dupmap_len > 0 {
            let map = range(bytes, part.dupmap_offset, part.dupmap_len, "duplication map")?;
            out.extend_from_slice(map);
        }
    }

    for &(offset, len) in &blobs {
        let blob = range(bytes, offset, len, "morph blob")?;
        out.extend_from_slice(blob);
    }

    out.extend_from_slice(notice);
    push_u32(&mut out, notice.len() as u32);
    out.extend_from_slice(FOOTER_TAG);

    if out.len() as u64 != total {
        return Err(format!(
            "internal error: wrote {} bytes, planned {total}",
            out.len()
        ));
    }
    Ok((out, dropped))
}

/// Writes `data` to a temporary file next to `output`, then renames it over
/// `output`. On any failure the temporary file is removed and an existing
/// output is left as it was.
fn write_output(output: &Path, data: &[u8]) -> Result<(), String> {
    let name = output
        .file_name()
        .ok_or_else(|| format!("output path {} has no file name", output.display()))?;
    let mut tmp_name = name.to_os_string();
    tmp_name.push(format!(".tmp{}", std::process::id()));
    let tmp = parent_dir(output).join(tmp_name);

    let result = (|| -> std::io::Result<()> {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(data)?;
        drop(file);
        fs::rename(&tmp, output)
    })();

    if let Err(e) = result {
        let _ = fs::remove_file(&tmp);
        return Err(format!("cannot write output {}: {e}", output.display()));
    }
    Ok(())
}

fn run(input: &Path, output: &Path, notice_path: &Path) -> Result<String, String> {
    let bytes = fs::read(input).map_err(|e| format!("cannot read input {}: {e}", input.display()))?;
    let notice = read_notice(notice_path)?;
    let pack = parse(&bytes)?;
    let kept_before = pack.morphs.len();
    let (out, dropped) = build(&bytes, &pack, &notice)?;
    write_output(output, &out)?;
    Ok(format!(
        "pack_strip: kept {} dropped {} output {} bytes",
        kept_before - dropped,
        dropped,
        out.len()
    ))
}
