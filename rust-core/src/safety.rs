//! Morph safety gate: public builds never generate a minor or a genital target.
//!
//! `generate_character` refuses any request that names a morph id in the
//! deny set below, whichever pack is loaded. Two rules make up the set:
//!
//! - `minor`: morph ids whose upstream target name carries a `baby` or
//!   `child` token (split on `/ - _ .`), in any category. The ids are
//!   derived from `packs/morph_id_map.json`: 228 ids, grouped into the 13
//!   inclusive ranges of `MINOR_MORPH_ID_RANGES`.
//! - `genital`: the id block the manifest allocates to category `genitals`
//!   (`morph_id_ranges_by_category` in `packs/manifest.json`), 2600..=2699.
//!   The whole block is denied, including ids no pack uses yet.
//!
//! A request naming a denied id is refused whatever its weight (0.0,
//! negative and NaN included): the gate looks at ids only.
//!
//! The tables are written out by hand so the gate has no runtime
//! dependency on any data file. `tests/safety_gate_unloaded.rs` re-derives
//! the set from `packs/morph_id_map.json` and fails if the tables drift.

/// The 228 morph ids whose target name carries a `baby` or `child` token,
/// as 13 inclusive `(lo, hi)` ranges, sorted and disjoint.
const MINOR_MORPH_ID_RANGES: [(u16, u16); 13] = [
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

/// The id block the manifest allocates to category `genitals`.
const GENITAL_MORPH_ID_RANGE: (u16, u16) = (2600, 2699);

/// Which rule denied an id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SafetyRule {
    Minor,
    Genital,
}

impl SafetyRule {
    /// The rule's name as it appears in the last-error text.
    pub(crate) fn name(self) -> &'static str {
        match self {
            SafetyRule::Minor => "minor",
            SafetyRule::Genital => "genital",
        }
    }
}

/// The rule that denies `id`, or `None` if the id is permitted. Minor
/// ranges are checked first, then the genital block.
pub(crate) fn denied_rule(id: u16) -> Option<SafetyRule> {
    if MINOR_MORPH_ID_RANGES
        .iter()
        .any(|&(lo, hi)| (lo..=hi).contains(&id))
    {
        return Some(SafetyRule::Minor);
    }
    let (lo, hi) = GENITAL_MORPH_ID_RANGE;
    if (lo..=hi).contains(&id) {
        return Some(SafetyRule::Genital);
    }
    None
}

/// Whether public builds refuse morph id `id`.
pub fn is_denied_morph_id(id: u16) -> bool {
    denied_rule(id).is_some()
}

/// The first denied id in slice order, with the rule that denied it.
pub(crate) fn first_denied(ids: &[u16]) -> Option<(u16, SafetyRule)> {
    ids.iter()
        .find_map(|&id| denied_rule(id).map(|rule| (id, rule)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minor_ranges_are_sorted_disjoint_and_cover_228_ids() {
        let mut total = 0usize;
        let mut prev_hi: Option<u16> = None;
        for &(lo, hi) in &MINOR_MORPH_ID_RANGES {
            assert!(lo <= hi, "range ({lo}, {hi}) is inverted");
            if let Some(p) = prev_hi {
                assert!(lo > p, "range ({lo}, {hi}) does not start after previous hi {p}");
            }
            prev_hi = Some(hi);
            total += (hi - lo) as usize + 1;
        }
        assert_eq!(total, 228);
    }

    #[test]
    fn range_edges_are_denied_and_their_neighbours_are_not() {
        for &(lo, hi) in &MINOR_MORPH_ID_RANGES {
            assert_eq!(denied_rule(lo), Some(SafetyRule::Minor), "lo edge {lo}");
            assert_eq!(denied_rule(hi), Some(SafetyRule::Minor), "hi edge {hi}");
        }
        for id in [999u16, 1002, 1003, 1060, 1095, 1132, 2307, 2380, 2599, 2700] {
            assert_eq!(denied_rule(id), None, "id {id} must be permitted");
        }
        assert_eq!(denied_rule(2600), Some(SafetyRule::Genital));
        assert_eq!(denied_rule(2699), Some(SafetyRule::Genital));
    }

    #[test]
    fn first_denied_reports_request_order() {
        assert_eq!(first_denied(&[]), None);
        assert_eq!(first_denied(&[3700, 5001]), None);
        assert_eq!(
            first_denied(&[3700, 2601, 1020]),
            Some((2601, SafetyRule::Genital))
        );
    }
}
