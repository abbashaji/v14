//! Tests for the `pack_strip` tool (`src/bin/pack_strip.rs`).
//!
//! std only. The decoder, the CRLF conversion and the synthetic pack
//! builder below are this file's own, independent of the tool.

use anthroforge_core::is_denied_morph_id;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const TOOL: &str = env!("CARGO_BIN_EXE_pack_strip");

// ---------------------------------------------------------------- helpers

/// Independent copy of `MINOR_MORPH_ID_RANGES` from `src/safety.rs`.
const MINOR_RANGES: [(u16, u16); 13] = [
    (1000, 1001),
    (1004, 1005),
    (1008, 1009),
    (1012, 1013),
    (1016, 1017),
    (1020, 1021),
    (1024, 1059),
    (1096, 1131),
    (1168, 1185),
    (1222, 1239),
    (1276, 1293),
    (1312, 1329),
    (2308, 2379),
];

struct TempDir(PathBuf);

impl TempDir {
    fn new(test: &str) -> TempDir {
        let path = std::env::temp_dir().join(format!("pack_strip_{}_{}", std::process::id(), test));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp dir");
        TempDir(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn packs_dir() -> PathBuf {
    manifest_dir().join("packs")
}

fn run_tool(args: &[&Path]) -> Output {
    Command::new(TOOL).args(args).output().expect("spawn pack_strip")
}

fn read(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn write(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn crlf_to_lf(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    for (i, &b) in raw.iter().enumerate() {
        if b == b'\r' && raw.get(i + 1) == Some(&b'\n') {
            continue;
        }
        out.push(b);
    }
    out
}

fn lf_to_crlf(lf: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(lf.len() * 2);
    for &b in lf {
        if b == b'\n' {
            out.push(b'\r');
        }
        out.push(b);
    }
    out
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(b[o..o + 2].try_into().unwrap())
}

fn put_u32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

#[derive(Clone, Debug)]
struct PartE {
    part_id: u32,
    src_type: u8,
    off: u32,
    len: u32,
    dm_off: u32,
    dm_len: u32,
}

#[derive(Clone, Debug)]
struct MorphE {
    id: u16,
    reserved: u16,
    off: u32,
    len: u32,
}

struct Decoded {
    part_count: usize,
    morph_count: usize,
    skel_len: usize,
    parts: Vec<PartE>,
    morphs: Vec<MorphE>,
}

impl Decoded {
    fn part_index_start(&self) -> usize {
        20 + self.skel_len
    }
    fn morph_index_start(&self) -> usize {
        self.part_index_start() + self.part_count * 21
    }
    fn morph_index_end(&self) -> usize {
        self.morph_index_start() + self.morph_count * 12
    }
    fn ids(&self) -> Vec<u16> {
        self.morphs.iter().map(|m| m.id).collect()
    }
}

/// The test's own decoder of the AFPP v2 index (panics on a malformed pack).
fn decode(b: &[u8]) -> Decoded {
    assert_eq!(&b[0..4], b"AFPP", "magic");
    assert_eq!(u32_at(b, 4), 2, "version");
    let part_count = u32_at(b, 8) as usize;
    let morph_count = u32_at(b, 12) as usize;
    let skel_len = u32_at(b, 16) as usize;
    let part_start = 20 + skel_len;
    let morph_start = part_start + part_count * 21;
    let parts = (0..part_count)
        .map(|i| {
            let o = part_start + i * 21;
            PartE {
                part_id: u32_at(b, o),
                src_type: b[o + 4],
                off: u32_at(b, o + 5),
                len: u32_at(b, o + 9),
                dm_off: u32_at(b, o + 13),
                dm_len: u32_at(b, o + 17),
            }
        })
        .collect();
    let morphs = (0..morph_count)
        .map(|i| {
            let o = morph_start + i * 12;
            MorphE {
                id: u16_at(b, o),
                reserved: u16_at(b, o + 2),
                off: u32_at(b, o + 4),
                len: u32_at(b, o + 8),
            }
        })
        .collect();
    Decoded { part_count, morph_count, skel_len, parts, morphs }
}

fn skeleton<'a>(b: &'a [u8], d: &Decoded) -> &'a [u8] {
    &b[20..20 + d.skel_len]
}

fn part_src<'a>(b: &'a [u8], p: &PartE) -> &'a [u8] {
    &b[p.off as usize..(p.off + p.len) as usize]
}

fn part_dupmap<'a>(b: &'a [u8], p: &PartE) -> &'a [u8] {
    if p.dm_len == 0 {
        &b[0..0]
    } else {
        &b[p.dm_off as usize..(p.dm_off + p.dm_len) as usize]
    }
}

fn blob<'a>(b: &'a [u8], m: &MorphE) -> &'a [u8] {
    &b[m.off as usize..(m.off + m.len) as usize]
}

/// Splits a tool output into (data region length, notice bytes) using the
/// footer's own length field; checks the `AFNT` tag.
fn split_footer(out: &[u8]) -> (usize, Vec<u8>) {
    assert!(out.len() >= 8, "output shorter than a footer tail");
    assert_eq!(&out[out.len() - 4..], b"AFNT", "footer tag");
    let notice_len = u32_at(out, out.len() - 8) as usize;
    assert!(out.len() >= notice_len + 8, "footer length field is past the start");
    let start = out.len() - 8 - notice_len;
    (start, out[start..start + notice_len].to_vec())
}

// ------------------------------------------------------- synthetic pack

fn synthetic_ids() -> Vec<u16> {
    let mut ids: Vec<u16> = Vec::new();
    for &(lo, hi) in &MINOR_RANGES {
        assert!(is_denied_morph_id(lo), "lo edge {lo} must be denied");
        assert!(is_denied_morph_id(hi), "hi edge {hi} must be denied");
        ids.push(lo);
        ids.push(hi);
    }
    assert!(is_denied_morph_id(2600) && is_denied_morph_id(2699));
    for id in [1020u16, 1022, 1095, 2600, 2699] {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    // Just outside the genital block, and other permitted neighbours.
    for id in [2599u16, 2700, 999, 1002, 1060, 1132, 2307, 2380, 3700, 5001, 1186, 1240] {
        assert!(!is_denied_morph_id(id), "id {id} must be permitted");
        ids.push(id);
    }
    // Deterministic Fisher-Yates shuffle (xorshift), so the order is unsorted.
    let mut state: u64 = 0x9E3779B97F4A7C15;
    for i in (1..ids.len()).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let j = (state % (i as u64 + 1)) as usize;
        ids.swap(i, j);
    }
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_ne!(ids, sorted, "synthetic id order must be unsorted");
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "synthetic ids must be unique");
    assert!(ids.len() >= 30);
    ids
}

/// Blob index used by each morph id: some ids share a blob, some blobs only
/// hold equal bytes at different ranges, the rest are unique.
fn synthetic_blobs(ids: &[u16]) -> (Vec<Vec<u8>>, Vec<usize>) {
    fn make(blobs: &mut Vec<Vec<u8>>, k: usize) -> usize {
        let len = 3 + (k * 5) % 17;
        let bytes: Vec<u8> = (0..len).map(|j| (k * 31 + j * 7 + 1) as u8).collect();
        blobs.push(bytes);
        blobs.len() - 1
    }
    let mut blobs: Vec<Vec<u8>> = Vec::new();
    let shared_kept = make(&mut blobs, 100); // 1022 and 1095 (both kept) share it
    let shared_mixed = make(&mut blobs, 101); // 1020 (denied) and 1060 (kept) share it
    let equal_a = make(&mut blobs, 102); // 999 ...
    let equal_b = {
        let copy = blobs[equal_a].clone();
        blobs.push(copy); // ... and 1002: equal bytes, different range
        blobs.len() - 1
    };
    let only_denied = make(&mut blobs, 103); // 2600 and 2699: referenced by denied ids only
    let mut next = 200usize;
    let mut map = Vec::new();
    for &id in ids {
        let idx = match id {
            1022 | 1095 => shared_kept,
            1020 | 1060 => shared_mixed,
            999 => equal_a,
            1002 => equal_b,
            2600 | 2699 => only_denied,
            _ => {
                next += 1;
                make(&mut blobs, next)
            }
        };
        map.push(idx);
    }
    (blobs, map)
}

const SYNTH_SKELETON: &[u8] = b"{\"opaque\":\"skeleton bytes\"}";

/// An AFPP v2 pack with gaps between sections, the blob section before the
/// part sources, reserved fields 0xBEEF and 7 junk bytes at the end.
fn build_synthetic() -> Vec<u8> {
    let ids = synthetic_ids();
    let (blobs, blob_of) = synthetic_blobs(&ids);

    let part_src: Vec<(u32, u8, Vec<u8>, Vec<u8>)> = vec![
        (4001, 0, b"o part one\nv 0 0 0\n".to_vec(), vec![1, 0, 0, 0, 2, 0, 0, 0]),
        (4002, 1, b"glTF-ish part two bytes".to_vec(), vec![9, 0, 0, 0, 8, 0, 0, 0, 7, 0, 0, 0]),
        (4003, 0, b"o part three".to_vec(), Vec::new()),
    ];

    let index_end = 20 + SYNTH_SKELETON.len() + part_src.len() * 21 + ids.len() * 12;
    let blobs_start = index_end + 5; // gap of 5 bytes
    let mut blob_offsets = Vec::new();
    let mut pos = blobs_start;
    for b in &blobs {
        blob_offsets.push(pos);
        pos += b.len();
    }
    let mut src_offsets = Vec::new();
    pos += 3; // gap
    for p in &part_src {
        src_offsets.push(pos);
        pos += p.2.len();
    }
    pos += 2; // gap
    let mut dm_offsets = Vec::new();
    for p in &part_src {
        if p.3.is_empty() {
            dm_offsets.push(0xABCD_u32 as usize); // junk offset with dupmap_len 0
        } else {
            dm_offsets.push(pos);
            pos += p.3.len();
        }
    }

    let mut out = Vec::new();
    out.extend_from_slice(b"AFPP");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(part_src.len() as u32).to_le_bytes());
    out.extend_from_slice(&(ids.len() as u32).to_le_bytes());
    out.extend_from_slice(&(SYNTH_SKELETON.len() as u32).to_le_bytes());
    out.extend_from_slice(SYNTH_SKELETON);
    for (i, p) in part_src.iter().enumerate() {
        out.extend_from_slice(&p.0.to_le_bytes());
        out.push(p.1);
        out.extend_from_slice(&(src_offsets[i] as u32).to_le_bytes());
        out.extend_from_slice(&(p.2.len() as u32).to_le_bytes());
        out.extend_from_slice(&(dm_offsets[i] as u32).to_le_bytes());
        out.extend_from_slice(&(p.3.len() as u32).to_le_bytes());
    }
    for (i, &id) in ids.iter().enumerate() {
        let k = blob_of[i];
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&0xBEEFu16.to_le_bytes());
        out.extend_from_slice(&(blob_offsets[k] as u32).to_le_bytes());
        out.extend_from_slice(&(blobs[k].len() as u32).to_le_bytes());
    }
    assert_eq!(out.len(), index_end);
    out.extend_from_slice(&[0xEE; 5]);
    for b in &blobs {
        out.extend_from_slice(b);
    }
    out.extend_from_slice(&[0xDD; 3]);
    for p in &part_src {
        out.extend_from_slice(&p.2);
    }
    out.extend_from_slice(&[0xCC; 2]);
    for p in &part_src {
        out.extend_from_slice(&p.3);
    }
    out.extend_from_slice(&[0x77; 7]); // junk after the last section
    out
}

const SYNTH_NOTICE: &[u8] = b"synthetic notice line one\nsecond line\n";

// ------------------------------------------------------------- the tests

/// Checks a tool output against its input with the test's own decoder:
/// everything except the footer and the reserved/offset recomputation that
/// the layout dictates. Returns the decoded output.
fn check_stripped(input: &[u8], output: &[u8], notice_lf: &[u8]) -> Decoded {
    let din = decode(input);
    let dout = decode(output);

    // Footer.
    let (footer_start, footer_notice) = split_footer(output);
    assert_eq!(footer_notice, notice_lf, "footer notice");

    // Kept ids: input minus denied, in input order.
    let expected_ids: Vec<u16> = din.ids().into_iter().filter(|&id| !is_denied_morph_id(id)).collect();
    assert_eq!(dout.ids(), expected_ids, "kept id list");
    assert!(dout.ids().iter().all(|&id| !is_denied_morph_id(id)), "denied id left");
    assert_eq!(dout.morph_count, expected_ids.len());

    // Skeleton, parts, duplication maps.
    assert_eq!(dout.skel_len, din.skel_len);
    assert_eq!(skeleton(output, &dout), skeleton(input, &din), "skeleton");
    assert_eq!(dout.part_count, din.part_count);
    for (pi, po) in din.parts.iter().zip(&dout.parts) {
        assert_eq!(pi.part_id, po.part_id);
        assert_eq!(pi.src_type, po.src_type);
        assert_eq!(pi.len, po.len);
        assert_eq!(pi.dm_len, po.dm_len);
        assert_eq!(part_src(input, pi), part_src(output, po), "part {} source", pi.part_id);
        assert_eq!(part_dupmap(input, pi), part_dupmap(output, po), "part {} dupmap", pi.part_id);
        if po.dm_len == 0 {
            assert_eq!(po.dm_off, 0, "dupmap_offset must be 0 when dupmap_len is 0");
        }
    }

    // Kept blobs: reserved 0, bytes equal, sharing exactly as the input.
    let input_by_id: HashMap<u16, &MorphE> = din.morphs.iter().map(|m| (m.id, m)).collect();
    let mut in_to_out: HashMap<(u32, u32), u32> = HashMap::new();
    let mut out_to_in: HashMap<(u32, u32), (u32, u32)> = HashMap::new();
    for mo in &dout.morphs {
        assert_eq!(mo.reserved, 0, "reserved of morph {}", mo.id);
        let mi = input_by_id[&mo.id];
        assert_eq!(mi.len, mo.len, "blob length of morph {}", mo.id);
        assert_eq!(blob(input, mi), blob(output, mo), "blob bytes of morph {}", mo.id);
        let in_key = (mi.off, mi.len);
        let out_key = (mo.off, mo.len);
        if let Some(prev) = in_to_out.insert(in_key, mo.off) {
            assert_eq!(prev, mo.off, "equal input ranges must share one output blob");
        }
        if let Some(prev) = out_to_in.insert(out_key, in_key) {
            assert_eq!(prev, in_key, "different input ranges must not share an output blob");
        }
    }

    // Contiguous sections in layout order, offsets recomputed from lengths.
    let mut cursor = dout.morph_index_end();
    for (pi, po) in din.parts.iter().zip(&dout.parts) {
        assert_eq!(po.off as usize, cursor, "part {} source offset", pi.part_id);
        cursor += pi.len as usize;
    }
    for (pi, po) in din.parts.iter().zip(&dout.parts) {
        if pi.dm_len > 0 {
            assert_eq!(po.dm_off as usize, cursor, "part {} dupmap offset", pi.part_id);
            cursor += pi.dm_len as usize;
        }
    }
    let mut first_seen: HashMap<(u32, u32), usize> = HashMap::new();
    for mo in &dout.morphs {
        let mi = input_by_id[&mo.id];
        let key = (mi.off, mi.len);
        let at = *first_seen.entry(key).or_insert_with(|| {
            let here = cursor;
            cursor += mi.len as usize;
            here
        });
        assert_eq!(mo.off as usize, at, "blob offset of morph {}", mo.id);
    }
    assert_eq!(footer_start, cursor, "data region must end at the end of the last blob");
    assert_eq!(output.len(), cursor + notice_lf.len() + 8, "output length");
    dout
}

#[test]
fn synthetic_pack_drops_exactly_the_denied_ids() {
    let tmp = TempDir::new("synthetic");
    let input = build_synthetic();
    let in_path = tmp.path().join("in.afpp");
    let out_path = tmp.path().join("out.afpp");
    let notice_path = tmp.path().join("notice.txt");
    write(&in_path, &input);
    write(&notice_path, SYNTH_NOTICE);

    // Sanity of the fixture itself.
    let din = decode(&input);
    assert!(din.morph_count >= 30);
    assert!(din.ids().iter().any(|&id| is_denied_morph_id(id)));
    for id in [1020u16, 1022, 1095, 2599, 2600, 2699, 2700] {
        assert!(din.ids().contains(&id), "fixture lacks id {id}");
    }
    assert!(din.morphs.iter().all(|m| m.reserved == 0xBEEF));
    assert!(din.parts.iter().any(|p| p.dm_len == 0));
    assert!(din.parts.iter().any(|p| p.src_type == 0) && din.parts.iter().any(|p| p.src_type == 1));

    let out = run_tool(&[&in_path, &out_path, &notice_path]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr_of(&out));
    assert!(String::from_utf8_lossy(&out.stdout).lines().count() == 1, "one summary line");

    let output = read(&out_path);
    let dout = check_stripped(&input, &output, SYNTH_NOTICE);

    // The blob only denied ids reference is gone; the mixed one is kept once.
    let kept_ids: HashSet<u16> = dout.ids().into_iter().collect();
    assert!(!kept_ids.contains(&1020) && !kept_ids.contains(&2600) && !kept_ids.contains(&2699));
    assert!(kept_ids.contains(&1060) && kept_ids.contains(&1022) && kept_ids.contains(&1095));
    let by_id: HashMap<u16, &MorphE> = dout.morphs.iter().map(|m| (m.id, m)).collect();
    assert_eq!(by_id[&1022].off, by_id[&1095].off, "shared blob stays shared");
    assert_ne!(by_id[&999].off, by_id[&1002].off, "equal bytes at different ranges stay apart");
    assert_eq!(blob(&output, by_id[&999]), blob(&output, by_id[&1002]));

    // Nothing else in the output directory.
    let mut names: Vec<String> = fs::read_dir(tmp.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, vec!["in.afpp", "notice.txt", "out.afpp"]);
}

/// (name, kept, dropped, unique blobs, data region bytes, FNV-1a-64 of the data region)
const REAL_PACKS: [(&str, usize, usize, usize, usize, u64); 6] = [
    ("essentials", 192, 156, 87, 22201635, 0xa61a37e94bafa3c2),
    ("body-shape", 572, 234, 435, 30683387, 0x2a8106395f9b6b37),
    ("face-shape", 332, 0, 332, 6709009, 0xb7924df99efd687d),
    ("expressions", 102, 0, 67, 3843571, 0x61c52cb36086c5fa),
    ("measurement-fit", 40, 0, 40, 5601945, 0xf08af1f681f211d6),
    ("full", 1046, 234, 872, 36598609, 0x7b2ac4dd991021be),
];

#[test]
fn real_packs_are_stripped_exactly() {
    let tmp = TempDir::new("real");
    let notice_path = packs_dir().join("CC0_NOTICE.txt");
    let notice_lf = crlf_to_lf(&read(&notice_path));
    assert!(!notice_lf.is_empty());

    for &(name, kept, dropped, unique, data_len, fnv) in &REAL_PACKS {
        let in_path = packs_dir().join(format!("{name}.afpp"));
        let out_path = tmp.path().join(format!("{name}.out.afpp"));
        let input = read(&in_path);
        let din = decode(&input);

        let out = run_tool(&[&in_path, &out_path, &notice_path]);
        assert_eq!(out.status.code(), Some(0), "{name}: stderr: {}", stderr_of(&out));
        let output = read(&out_path);

        let dout = check_stripped(&input, &output, &notice_lf);

        let (data_end, footer_notice) = split_footer(&output);
        assert_eq!(footer_notice, notice_lf, "{name}: footer notice");
        let data = &output[..data_end];

        assert_eq!(dout.morph_count, kept, "{name}: kept");
        assert_eq!(din.morph_count - dout.morph_count, dropped, "{name}: dropped");
        let unique_out: HashSet<u32> = dout.morphs.iter().map(|m| m.off).collect();
        assert_eq!(unique_out.len(), unique, "{name}: unique blobs");
        let kept_ids: HashSet<u16> = dout.ids().into_iter().collect();
        let unique_in: HashSet<(u32, u32)> = din
            .morphs
            .iter()
            .filter(|m| kept_ids.contains(&m.id))
            .map(|m| (m.off, m.len))
            .collect();
        assert_eq!(unique_in.len(), unique, "{name}: distinct input (offset, length) pairs");
        assert_eq!(data.len(), data_len, "{name}: data region bytes");
        assert_eq!(fnv1a64(data), fnv, "{name}: FNV-1a-64 of the data region");

        if matches!(name, "face-shape" | "expressions" | "measurement-fit") {
            assert!(data == &input[..], "{name}: data region must equal the input");
        }

        if name == "measurement-fit" {
            let crlf_path = tmp.path().join("notice_crlf.txt");
            write(&crlf_path, &lf_to_crlf(&notice_lf));
            let crlf_out_path = tmp.path().join("measurement-fit.crlf.afpp");
            let out2 = run_tool(&[&in_path, &crlf_out_path, &crlf_path]);
            assert_eq!(out2.status.code(), Some(0), "stderr: {}", stderr_of(&out2));
            assert!(read(&crlf_out_path) == output, "CRLF notice must give an identical output");
        }
        fs::remove_file(&out_path).unwrap();
    }
}

#[test]
fn output_is_deterministic_and_idempotent() {
    let tmp = TempDir::new("idempotent");
    let notice_path = tmp.path().join("notice.txt");
    write(&notice_path, SYNTH_NOTICE);
    let synthetic_path = tmp.path().join("synthetic.afpp");
    write(&synthetic_path, &build_synthetic());
    let real_path = packs_dir().join("measurement-fit.afpp");

    for (label, in_path) in [("synthetic", synthetic_path), ("measurement-fit", real_path)] {
        let first = tmp.path().join(format!("{label}.1.afpp"));
        let second = tmp.path().join(format!("{label}.2.afpp"));
        let third = tmp.path().join(format!("{label}.3.afpp"));
        for target in [&first, &second] {
            let out = run_tool(&[&in_path, target, &notice_path]);
            assert_eq!(out.status.code(), Some(0), "{label}: {}", stderr_of(&out));
        }
        let a = read(&first);
        assert!(a == read(&second), "{label}: two runs must give identical bytes");
        let out = run_tool(&[&first, &third, &notice_path]);
        assert_eq!(out.status.code(), Some(0), "{label}: {}", stderr_of(&out));
        assert!(read(&third) == a, "{label}: running on its own output must reproduce it");
    }
}

/// One bad-input case: expected exit code, run with and without a
/// pre-existing output file.
fn expect_failure(
    root: &Path,
    label: &str,
    input: Option<&[u8]>,
    notice: Option<&[u8]>,
    code: i32,
) {
    for with_sentinel in [false, true] {
        let dir = root.join(format!("{label}_{}", if with_sentinel { "sentinel" } else { "fresh" }));
        fs::create_dir_all(&dir).unwrap();
        let in_path = dir.join("in.afpp");
        let out_path = dir.join("out.afpp");
        let notice_path = dir.join("notice.txt");
        if let Some(b) = input {
            write(&in_path, b);
        }
        if let Some(b) = notice {
            write(&notice_path, b);
        }
        let sentinel: &[u8] = b"SENTINEL: must stay as it is";
        if with_sentinel {
            write(&out_path, sentinel);
        }
        let out = run_tool(&[&in_path, &out_path, &notice_path]);
        assert_eq!(out.status.code(), Some(code), "{label}: stderr: {}", stderr_of(&out));
        if code == 1 {
            let err = stderr_of(&out);
            assert!(
                err.lines().next().map_or(false, |l| l.starts_with("pack_strip: error:")),
                "{label}: stderr was {err:?}"
            );
        }
        assert!(out.stdout.is_empty(), "{label}: nothing on stdout on failure");
        if with_sentinel {
            assert_eq!(read(&out_path), sentinel, "{label}: existing output was touched");
        } else {
            assert!(!out_path.exists(), "{label}: an output was created");
        }
        let mut allowed: HashSet<String> = HashSet::new();
        for name in ["in.afpp", "notice.txt"] {
            allowed.insert(name.to_string());
        }
        if with_sentinel {
            allowed.insert("out.afpp".to_string());
        }
        for entry in fs::read_dir(&dir).unwrap() {
            let name = entry.unwrap().file_name().to_string_lossy().into_owned();
            assert!(allowed.contains(&name), "{label}: unexpected file {name} left behind");
        }
    }
}

#[test]
fn bad_input_fails_cleanly() {
    let tmp = TempDir::new("bad_input");
    let root = tmp.path();
    let valid = build_synthetic();
    let d = decode(&valid);
    let notice: &[u8] = SYNTH_NOTICE;
    let len = valid.len() as u32;
    let part_at = |i: usize| d.part_index_start() + i * 21;
    let morph_at = |i: usize| d.morph_index_start() + i * 12;
    let edited = |f: &dyn Fn(&mut Vec<u8>)| -> Vec<u8> {
        let mut copy = valid.clone();
        f(&mut copy);
        copy
    };

    expect_failure(root, "missing_input", None, Some(notice), 1);
    expect_failure(root, "empty_input", Some(&[]), Some(notice), 1);
    expect_failure(root, "short_19", Some(&valid[..19]), Some(notice), 1);
    expect_failure(root, "bad_magic", Some(&edited(&|b| b[0] = b'X')), Some(notice), 1);
    expect_failure(root, "version_1", Some(&edited(&|b| put_u32(b, 4, 1))), Some(notice), 1);
    expect_failure(root, "version_3", Some(&edited(&|b| put_u32(b, 4, 3))), Some(notice), 1);
    expect_failure(root, "skel_len_past_end", Some(&edited(&|b| put_u32(b, 16, len))), Some(notice), 1);
    expect_failure(root, "part_count_max", Some(&edited(&|b| put_u32(b, 8, u32::MAX))), Some(notice), 1);
    expect_failure(root, "morph_count_max", Some(&edited(&|b| put_u32(b, 12, u32::MAX))), Some(notice), 1);
    expect_failure(
        root,
        "part_source_past_end",
        Some(&edited(&|b| put_u32(b, part_at(0) + 5, len - 1))),
        Some(notice),
        1,
    );
    expect_failure(
        root,
        "dupmap_past_end",
        Some(&edited(&|b| put_u32(b, part_at(0) + 13, len - 1))),
        Some(notice),
        1,
    );
    expect_failure(
        root,
        "dupmap_len_6",
        Some(&edited(&|b| put_u32(b, part_at(0) + 17, 6))),
        Some(notice),
        1,
    );
    expect_failure(
        root,
        "morph_blob_past_end",
        Some(&edited(&|b| {
            put_u32(b, morph_at(0) + 4, len - 1);
            put_u32(b, morph_at(0) + 8, 10);
        })),
        Some(notice),
        1,
    );
    expect_failure(
        root,
        "duplicate_morph_id_denied",
        Some(&edited(&|b| {
            b[morph_at(0)..morph_at(0) + 2].copy_from_slice(&1020u16.to_le_bytes());
            b[morph_at(1)..morph_at(1) + 2].copy_from_slice(&1020u16.to_le_bytes());
        })),
        Some(notice),
        1,
    );
    expect_failure(
        root,
        "duplicate_part_id",
        Some(&edited(&|b| {
            let first = d.parts[0].part_id;
            put_u32(b, part_at(1), first);
        })),
        Some(notice),
        1,
    );
    expect_failure(root, "missing_notice", Some(&valid), None, 1);
    expect_failure(root, "empty_notice", Some(&valid), Some(&[]), 1);
    expect_failure(root, "notice_65537", Some(&valid), Some(&vec![b'a'; 65537]), 1);
    expect_failure(root, "notice_not_utf8", Some(&valid), Some(&[0xff, 0xfe, b'\n']), 1);

    // Usage errors: wrong argument counts.
    let udir = root.join("usage");
    fs::create_dir_all(&udir).unwrap();
    let in_path = udir.join("in.afpp");
    let out_path = udir.join("out.afpp");
    let notice_path = udir.join("notice.txt");
    let extra_path = udir.join("extra");
    write(&in_path, &valid);
    write(&notice_path, notice);
    let (pi, po, pn, pe): (&Path, &Path, &Path, &Path) =
        (&in_path, &out_path, &notice_path, &extra_path);
    let arg_sets: Vec<Vec<&Path>> = vec![vec![], vec![pi], vec![pi, po], vec![pi, po, pn, pe]];
    for args in &arg_sets {
        let out = run_tool(args);
        assert_eq!(out.status.code(), Some(2), "{} args: {}", args.len(), stderr_of(&out));
        assert!(!out_path.exists() && !extra_path.exists(), "{} args created a file", args.len());
    }

    // Usage errors: the output names the input.
    let out = run_tool(&[&in_path, &in_path, &notice_path]);
    assert_eq!(out.status.code(), Some(2), "same path: {}", stderr_of(&out));
    assert_eq!(read(&in_path), valid, "input must be untouched (same path)");

    let sub = udir.join("sub");
    fs::create_dir_all(&sub).unwrap();
    let dotdot = sub.join("..").join("in.afpp");
    let out = run_tool(&[&in_path, &dotdot, &notice_path]);
    assert_eq!(out.status.code(), Some(2), "dotdot spelling: {}", stderr_of(&out));
    assert_eq!(read(&in_path), valid, "input must be untouched (dotdot)");

    let mut left: Vec<String> = fs::read_dir(&udir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, vec!["in.afpp", "notice.txt", "sub"], "usage cases left files behind");
}

fn footer_notice_for(tmp: &Path, label: &str, notice: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let in_path = tmp.join("synthetic.afpp");
    if !in_path.exists() {
        write(&in_path, &build_synthetic());
    }
    let notice_path = tmp.join(format!("{label}.notice.txt"));
    let out_path = tmp.join(format!("{label}.out.afpp"));
    write(&notice_path, notice);
    let out = run_tool(&[&in_path, &out_path, &notice_path]);
    assert_eq!(out.status.code(), Some(0), "{label}: {}", stderr_of(&out));
    let bytes = read(&out_path);
    let (_, footer) = split_footer(&bytes);
    (bytes, footer)
}

#[test]
fn notice_line_endings_do_not_change_output() {
    let tmp = TempDir::new("line_endings");
    let t = tmp.path();

    let (lf_out, lf_footer) = footer_notice_for(t, "lf", b"a\nb\n");
    let (crlf_out, crlf_footer) = footer_notice_for(t, "crlf", b"a\r\nb\r\n");
    let (mixed_out, mixed_footer) = footer_notice_for(t, "mixed", b"a\r\nb\n");
    assert_eq!(lf_footer, b"a\nb\n");
    assert_eq!(crlf_footer, b"a\nb\n");
    assert_eq!(mixed_footer, b"a\nb\n");
    assert!(lf_out == crlf_out, "CRLF notice changed the output");
    assert!(lf_out == mixed_out, "mixed notice changed the output");

    let (_, lone_cr) = footer_notice_for(t, "lone_cr", b"a\rb\n");
    assert_eq!(lone_cr, b"a\rb\n", "a lone CR must stay");

    let (_, no_newline) = footer_notice_for(t, "no_newline", b"a\nb");
    assert_eq!(no_newline, b"a\nb", "no newline is added");
    let (_, no_newline_crlf) = footer_notice_for(t, "no_newline_crlf", b"a\r\nb");
    assert_eq!(no_newline_crlf, b"a\nb");

    let (_, cr_then_crlf) = footer_notice_for(t, "cr_crlf", b"a\r\r\nb\n");
    assert_eq!(cr_then_crlf, b"a\r\nb\n", "only the CRLF pair is converted");
    let (_, only_crlf) = footer_notice_for(t, "only_crlf", b"\r\n");
    assert_eq!(only_crlf, b"\n");
}
