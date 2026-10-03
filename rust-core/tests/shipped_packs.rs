//! The distributed packs in `packs/` are the output of `pack_strip`: the
//! morph ids denied by `safety.rs` are removed and a CC0/MakeHuman notice
//! footer follows the last section. These tests read `packs/manifest.json`
//! and every pack it lists and check that the manifest describes the files,
//! that the files hold exactly the permitted ids, and that each ends with the
//! notice.
//!
//! std, `serde_json` and `is_denied_morph_id` only. The AFPP v2 decoder and
//! every helper below are this file's own.
//!
//! Layout decoded here: a 20-byte header (`AFPP`, version, part_count,
//! morph_count, skel_len, all u32 LE), the skeleton, the part index
//! (21-byte entries), then the morph index (12-byte entries: u16 id, u16
//! reserved, u32 offset, u32 length). A footer is the notice bytes, the
//! notice length as u32 LE, then `AFNT`; the loader ignores it.

use anthroforge_core::is_denied_morph_id;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

/// (id, file bytes, morphs, unique blobs): the measured values of the six
/// stripped packs, written here so the manifest and the packs cannot drift
/// together.
const EXPECTED: [(&str, u64, u64, u64); 6] = [
    ("essentials", 22_202_221, 192, 87),
    ("body-shape", 30_683_973, 572, 435),
    ("face-shape", 6_709_595, 332, 332),
    ("expressions", 3_844_157, 102, 67),
    ("measurement-fit", 5_602_531, 40, 40),
    ("full", 36_599_195, 1046, 872),
];

// ---------------------------------------------------------------- helpers

fn packs_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("packs")
}

fn read_bytes(name: &str) -> Vec<u8> {
    let path = packs_dir().join(name);
    fs::read(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

fn read_text(name: &str) -> String {
    String::from_utf8(read_bytes(name)).unwrap_or_else(|e| panic!("{name} is not UTF-8: {e}"))
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

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(b[o..o + 2].try_into().unwrap())
}

struct Decoded {
    file_len: u64,
    version: u32,
    part_count: u64,
    morph_count: u64,
    /// (id, data_offset, data_len) in index order.
    morphs: Vec<(u16, u32, u32)>,
}

impl Decoded {
    fn ids(&self) -> BTreeSet<u16> {
        self.morphs.iter().map(|m| m.0).collect()
    }
    fn unique_blobs(&self) -> BTreeSet<(u32, u32)> {
        self.morphs.iter().map(|m| (m.1, m.2)).collect()
    }
}

/// Decodes the header, part index and morph index. Panics with `what` on a
/// pack that is too short for what its header claims.
fn decode(what: &str, b: &[u8]) -> Decoded {
    assert!(b.len() >= 20, "{what}: shorter than the 20-byte header");
    assert_eq!(&b[0..4], b"AFPP", "{what}: magic");
    let version = u32_at(b, 4);
    let part_count = u32_at(b, 8) as u64;
    let morph_count = u32_at(b, 12) as u64;
    let skel_len = u32_at(b, 16) as u64;
    let morph_index = 20 + skel_len + part_count * 21;
    let morph_index_end = morph_index + morph_count * 12;
    assert!(
        morph_index_end <= b.len() as u64,
        "{what}: the morph index ends at {morph_index_end}, past the file length {}",
        b.len()
    );
    let morphs = (0..morph_count as usize)
        .map(|i| {
            let o = morph_index as usize + i * 12;
            (u16_at(b, o), u32_at(b, o + 4), u32_at(b, o + 8))
        })
        .collect();
    Decoded { file_len: b.len() as u64, version, part_count, morph_count, morphs }
}

fn json(name: &str) -> Value {
    serde_json::from_str(&read_text(name)).unwrap_or_else(|e| panic!("{name}: invalid JSON: {e}"))
}

fn as_u64(v: &Value, what: &str) -> u64 {
    v.as_u64().unwrap_or_else(|| panic!("{what}: expected an unsigned integer, got {v}"))
}

fn as_str<'a>(v: &'a Value, what: &str) -> &'a str {
    v.as_str().unwrap_or_else(|| panic!("{what}: expected a string, got {v}"))
}

/// One `packs` entry of the manifest.
struct Entry {
    id: String,
    file: String,
    part_count: u64,
    size_bytes: u64,
    morph_count: u64,
    unique_blob_count: u64,
    categories: Vec<String>,
    morph_id_range: (u64, u64),
}

fn manifest_entries(manifest: &Value) -> Vec<Entry> {
    let packs = manifest["packs"].as_array().expect("manifest.packs must be an array");
    packs
        .iter()
        .map(|p| {
            let id = as_str(&p["id"], "pack id").to_string();
            let range = p["morph_id_range"].as_array().unwrap_or_else(|| panic!("{id}: morph_id_range"));
            assert_eq!(range.len(), 2, "{id}: morph_id_range must have two numbers");
            let categories = p["categories"]
                .as_array()
                .unwrap_or_else(|| panic!("{id}: categories"))
                .iter()
                .map(|c| as_str(c, "category").to_string())
                .collect();
            Entry {
                file: as_str(&p["file"], &format!("{id}: file")).to_string(),
                part_count: as_u64(&p["part_count"], &format!("{id}: part_count")),
                size_bytes: as_u64(&p["size_bytes"], &format!("{id}: size_bytes")),
                morph_count: as_u64(&p["morph_count"], &format!("{id}: morph_count")),
                unique_blob_count: as_u64(&p["unique_blob_count"], &format!("{id}: unique_blob_count")),
                categories,
                morph_id_range: (
                    as_u64(&range[0], &format!("{id}: morph_id_range[0]")),
                    as_u64(&range[1], &format!("{id}: morph_id_range[1]")),
                ),
                id,
            }
        })
        .collect()
}

/// Category blocks of `morph_id_ranges_by_category` as (name, start, end), in
/// the order they appear in the manifest text. `serde_json` sorts object keys
/// alphabetically (no `preserve_order`), so the order is taken from the text:
/// the entries of that object are the lines `    "<name>": [`.
fn category_blocks(manifest_text: &str, manifest: &Value) -> Vec<(String, u64, u64)> {
    let ranges = manifest["morph_id_ranges_by_category"]
        .as_object()
        .expect("manifest.morph_id_ranges_by_category must be an object");
    let key_line = "  \"morph_id_ranges_by_category\": {";
    let start = manifest_text.find(key_line).expect("morph_id_ranges_by_category not found in the text");
    let body = &manifest_text[start + key_line.len()..];
    let end = body.find("\n  }").expect("end of morph_id_ranges_by_category not found");
    let mut order: Vec<String> = Vec::new();
    for line in body[..end].lines() {
        if let Some(rest) = line.strip_prefix("    \"") {
            if let Some(name_end) = rest.find("\": [") {
                order.push(rest[..name_end].to_string());
            }
        }
    }
    assert_eq!(order.len(), ranges.len(), "category order scan must find every category");
    order
        .into_iter()
        .map(|name| {
            let r = ranges[&name].as_array().unwrap_or_else(|| panic!("range of {name}"));
            assert_eq!(r.len(), 2, "range of {name}");
            let (lo, hi) = (as_u64(&r[0], &name), as_u64(&r[1], &name));
            (name, lo, hi)
        })
        .collect()
}

// ------------------------------------------------------------- the tests

#[test]
fn shipped_packs_match_the_manifest() {
    let manifest = json("manifest.json");
    let entries = manifest_entries(&manifest);

    let ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
    let want: Vec<&str> = EXPECTED.iter().map(|e| e.0).collect();
    assert_eq!(ids, want, "the manifest must list exactly these packs, in this order");

    let listed: BTreeSet<String> = entries.iter().map(|e| e.file.clone()).collect();
    assert_eq!(listed.len(), entries.len(), "manifest files must be distinct");
    for e in &entries {
        assert_eq!(e.file, format!("{}.afpp", e.id), "{}: file name", e.id);
    }
    let on_disk: BTreeSet<String> = fs::read_dir(packs_dir())
        .expect("read packs dir")
        .map(|d| d.expect("dir entry").file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".afpp"))
        .collect();
    assert_eq!(on_disk, listed, "the *.afpp files in packs/ must be exactly the ones the manifest lists");

    for (e, &(id, size, morphs, unique)) in entries.iter().zip(EXPECTED.iter()) {
        assert_eq!(e.id, id);
        let bytes = read_bytes(&e.file);
        let d = decode(&e.id, &bytes);

        assert_eq!(d.file_len, e.size_bytes, "{id}: file length vs manifest size_bytes");
        assert_eq!(d.version, 2, "{id}: version");
        assert_eq!(d.part_count, e.part_count, "{id}: header part_count vs manifest");
        assert_eq!(d.morph_count, e.morph_count, "{id}: header morph count vs manifest morph_count");
        assert_eq!(
            d.unique_blobs().len() as u64,
            e.unique_blob_count,
            "{id}: distinct (offset, length) pairs vs manifest unique_blob_count"
        );
        assert_eq!(d.ids().len() as u64, d.morph_count, "{id}: morph ids must be unique");

        // The same three numbers against constants, so manifest and pack
        // cannot drift together.
        assert_eq!(d.file_len, size, "{id}: file length vs pinned value");
        assert_eq!(d.morph_count, morphs, "{id}: morph count vs pinned value");
        assert_eq!(d.unique_blobs().len() as u64, unique, "{id}: unique blobs vs pinned value");
    }
}

#[test]
fn shipped_packs_hold_exactly_the_permitted_ids() {
    let manifest_text = read_text("manifest.json");
    let manifest: Value = serde_json::from_str(&manifest_text).expect("manifest.json: invalid JSON");
    let entries = manifest_entries(&manifest);
    let blocks = category_blocks(&manifest_text, &manifest);
    assert_eq!(blocks.len(), 23, "category blocks");
    assert!(
        blocks.windows(2).all(|w| w[0].2 < w[1].1),
        "category blocks must be listed in ascending id order"
    );

    let map_json = json("morph_id_map.json");
    let map_obj = map_json.as_object().expect("morph_id_map.json must be an object");
    let mut category_of: BTreeMap<u16, String> = BTreeMap::new();
    for (k, v) in map_obj {
        let id: u16 = k.parse().unwrap_or_else(|_| panic!("map key {k:?} is not a u16"));
        category_of.insert(id, as_str(&v["category"], &format!("category of {k}")).to_string());
    }
    // The filter must not pass vacuously.
    assert_eq!(category_of.len(), 1280, "morph_id_map.json entries");
    let denied_in_map = category_of.keys().filter(|&&id| is_denied_morph_id(id)).count();
    assert_eq!(denied_in_map, 234, "ids of morph_id_map.json denied by is_denied_morph_id");

    assert_eq!(entries.len(), EXPECTED.len(), "number of manifest packs");
    for e in &entries {
        let bytes = read_bytes(&e.file);
        let held = decode(&e.id, &bytes).ids();

        for &id in &held {
            assert!(!is_denied_morph_id(id), "{}: holds denied morph id {id}", e.id);
            assert!(category_of.contains_key(&id), "{}: holds id {id}, which is not in morph_id_map.json", e.id);
        }

        let expected: BTreeSet<u16> = category_of
            .iter()
            .filter(|(id, cat)| e.categories.contains(cat) && !is_denied_morph_id(**id))
            .map(|(id, _)| *id)
            .collect();
        let missing: Vec<u16> = expected.difference(&held).copied().collect();
        let extra: Vec<u16> = held.difference(&expected).copied().collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "{}: held ids differ from the permitted ids of its categories; missing {:?}, unexpected {:?}",
            e.id,
            &missing[..missing.len().min(10)],
            &extra[..extra.len().min(10)]
        );

        // The categories that still hold an id, in manifest order.
        let holding: BTreeSet<&str> = held.iter().map(|id| category_of[id].as_str()).collect();
        let want_categories: Vec<String> = blocks
            .iter()
            .filter(|b| holding.contains(b.0.as_str()))
            .map(|b| b.0.clone())
            .collect();
        assert!(!want_categories.is_empty(), "{}: holds no id", e.id);
        assert_eq!(e.categories, want_categories, "{}: manifest categories vs the categories holding an id", e.id);

        let lo = blocks.iter().filter(|b| holding.contains(b.0.as_str())).map(|b| b.1).min().unwrap();
        let hi = blocks.iter().filter(|b| holding.contains(b.0.as_str())).map(|b| b.2).max().unwrap();
        assert_eq!(e.morph_id_range, (lo, hi), "{}: morph_id_range vs the block span of its categories", e.id);
    }
}

#[test]
fn shipped_packs_end_with_the_notice() {
    let notice_lf = crlf_to_lf(&read_bytes("CC0_NOTICE.txt"));
    assert!(!notice_lf.is_empty(), "CC0_NOTICE.txt is empty");

    let manifest = json("manifest.json");
    let entries = manifest_entries(&manifest);
    assert_eq!(entries.len(), EXPECTED.len(), "number of manifest packs");
    for e in &entries {
        let b = read_bytes(&e.file);
        assert!(b.len() >= 8, "{}: shorter than a footer tail", e.id);
        assert_eq!(&b[b.len() - 4..], b"AFNT", "{}: footer tag", e.id);
        let n = u32_at(&b, b.len() - 8) as usize;
        assert!(n > 0, "{}: footer notice length is zero", e.id);
        assert!(n + 8 <= b.len(), "{}: footer notice length {n} does not fit the file", e.id);
        let start = b.len() - 8 - n;
        assert!(
            b[start..start + n] == notice_lf[..],
            "{}: footer notice differs from CC0_NOTICE.txt (CRLF read as LF)",
            e.id
        );
    }
}
