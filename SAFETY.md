# Morph safety gate

Public builds never generate a minor or a genital target. `generate_character`
refuses any request naming a morph id in a fixed deny set, whichever pack is
loaded, and the demo sampling code never offers those ids.

## What is denied

- **minor**: every morph whose upstream target name has a `baby` or `child`
  token (split on `/ - _ .`), in any category: 228 ids in 13 inclusive ranges
  (156 `macrodetails`, 72 `breast`).
- **genital**: the id block the manifest allocates to category `genitals`,
  2600..=2699, including ids no pack uses yet (the map holds 6).

A request is refused whatever the weight (0.0, negative and NaN included).
Ground truth: `rust-core/packs/morph_id_map.json` (id to category and target)
and `rust-core/packs/manifest.json` (`morph_id_ranges_by_category`).

## Where it is enforced

`rust-core/src/safety.rs`, called inside `generate_character` before morph id
resolution (resolution silently skips ids the pack lacks, so the gate comes
first). The call returns null and `anthroforge_last_error` (JS `getLastError()`)
carries `safety: refused morph id <id> (minor|genital)`. The demo mirrors the
rule on target names in `isDeniedMorphEntry`
(`packages/web/demo/identity_sampling.js`, also used by `demo/crowd.html`).

## How to extend it

Edit the tables in `safety.rs` and `isDeniedMorphEntry`. The test
`denied_set_equals_upstream_ground_truth` fails until the tables match the map.

## Packs

- The distributed packs in `rust-core/packs/` are the output of `pack_strip`:
  ids denied by `is_denied_morph_id` are removed and a notice footer is added.
- `morph_id_map.json` stays complete; `manifest.json` describes the stripped files.
- The tests `shipped_packs_*` check the manifest, the held ids and the footer.
- A pack made with `pack_builder` or `rust-core/scripts/build_pack_library.py`
  holds the denied morphs until `pack_strip` is run on it. The gate in
  `generate_character` is what enforces the rule for any pack.

## Not covered yet

- The wasm in `packages/web/dist/` is not rebuilt, so the shipped browser build
  does not enforce this until a later rebuild.
- No clothed-output check exists; height/weight modifiers are not range-checked in the core.
- Breast targets named `nipple` (ids 2524..=2527) are not denied.
