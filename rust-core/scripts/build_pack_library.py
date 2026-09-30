#!/usr/bin/env python3
"""CC0-Phase 8: build the library of selectable morph packs.

Usage:
  build_pack_library.py --targets <makehuman/data/targets> --out <packs_dir> \
      [--work <scratch_dir>]

Uses only the two existing tools, `morph_converter` (one call per .target)
and `pack_builder` (one call per pack), from rust-core/target/release/.

Morph ID scheme (FROZEN -- do not renumber; a future live-mix phase refers
to morphs by these IDs regardless of which pack they shipped in):
  * Each category owns a fixed numeric block (CATEGORY_RANGES below).
  * Within a category, targets are sorted by relative path (bytewise) and
    numbered sequentially from the start of the block.
  * The ID is therefore a pure function of (category, sorted position) and
    is identical in every pack that contains that morph.
"""
import argparse, json, os, re, shutil, subprocess, sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
CORE = HERE.parent
BIN = CORE / "target" / "release"
BODY_DIR = CORE / "tests" / "fixtures" / "cc0_phase3_pipeline"   # 4 rigged .glb + master_skeleton.json
BASE_OBJ = CORE / "assets" / "upstream" / "base.obj"

# Allocated block per category: (first_id, last_id) inclusive. Assigned in
# the order the categories are first listed by the pack definitions below
# (essentials, body-shape, face-shape, expressions, measurement-fit), each
# block sized generously above the category's real count.
CATEGORY_RANGES = {
    # essentials
    "macrodetails": (1000, 1399),
    # body-shape
    "armslegs":     (1400, 1599),
    "bodyshapes":   (1600, 1699),
    "torso":        (1700, 1799),
    "neck":         (1800, 1899),
    "hip":          (1900, 1999),
    "stomach":      (2000, 2099),
    "pelvis":       (2100, 2199),
    "buttocks":     (2200, 2299),
    "breast":       (2300, 2599),
    "genitals":     (2600, 2699),
    # face-shape
    "head":         (2700, 2799),
    "nose":         (2800, 2899),
    "ears":         (2900, 2999),
    "mouth":        (3000, 3099),
    "cheek":        (3100, 3199),
    "chin":         (3200, 3299),
    "forehead":     (3300, 3399),
    "eyebrows":     (3400, 3499),
    "eyes":         (3500, 3599),
    "asym":         (3600, 3699),
    # expressions
    "expression":   (3700, 3899),
    # measurement-fit
    "measure":      (3900, 3999),
}

PACKS = [
    dict(id="essentials", file="essentials.afpp", label="Essentials",
         description="Core body macro sliders (age, weight, muscle, gender, height family): MakeHuman's "
                     "whole-population targets. Fewest morphs, but NOT small in bytes -- these are "
                     "full-body deltas (~417 KB each), ~84% of the entire corpus's morph data. "
                     "Choose a default with that cost in mind.",
         categories=["macrodetails"]),
    dict(id="body-shape", file="body-shape.afpp", label="Body shape",
         description="Body proportion and build variety (macro sliders, arms/legs, body shapes, "
                     "torso, neck, hip, stomach, pelvis, buttocks, breast, genitals). No face.",
         categories=["macrodetails", "armslegs", "bodyshapes", "torso", "neck", "hip",
                     "stomach", "pelvis", "buttocks", "breast", "genitals"]),
    dict(id="face-shape", file="face-shape.afpp", label="Face shape",
         description="Facial identity variety (head, nose, ears, mouth, cheek, chin, forehead, "
                     "eyebrows, eyes, asymmetry).",
         categories=["head", "nose", "ears", "mouth", "cheek", "chin", "forehead",
                     "eyebrows", "eyes", "asym"]),
    dict(id="expressions", file="expressions.afpp", label="Expressions",
         description="POSED FACIAL EXPRESSIONS (smile, frown, etc.). Not shape/identity diversity: "
                     "a different purpose from the body-shape and face-shape packs -- present it "
                     "as its own picker category, not blended with them.",
         categories=["expression"]),
    dict(id="measurement-fit", file="measurement-fit.afpp", label="Measurement fit",
         description="Targets suited to a future 'type in real height/waist/chest, reverse-fit' "
                     "feature. Kept separate so that feature can load just this small pack.",
         categories=["measure"]),
    dict(id="full", file="full.afpp", label="Full library",
         description="The entire MakeHuman CC0 target corpus, unfiltered (all 23 categories). "
                     "Power-user / offline option; not a default download.",
         categories=list(CATEGORY_RANGES)),
]


def slug(rel: Path) -> str:
    s = "_".join(rel.with_suffix("").parts)
    return re.sub(r"[^A-Za-z0-9_.-]+", "-", s)


def run(cmd):
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode != 0:
        sys.exit(f"FAILED: {' '.join(map(str, cmd))}\n{r.stdout}\n{r.stderr}")
    return r.stdout


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--targets", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--work", default="/tmp/pack_library_work")
    a = ap.parse_args()
    targets, out, work = Path(a.targets), Path(a.out), Path(a.work)
    conv, packer = BIN / "morph_converter", BIN / "pack_builder"
    for p in (conv, packer, BASE_OBJ, BODY_DIR / "master_skeleton.json"):
        if not p.exists():
            sys.exit(f"missing required input: {p}")

    # ---- 1. discover + assign IDs -------------------------------------
    by_cat = {}
    for f in targets.rglob("*.target"):
        rel = f.relative_to(targets)
        by_cat.setdefault(rel.parts[0], []).append(rel)
    if set(by_cat) != set(CATEGORY_RANGES):
        sys.exit(f"category mismatch: corpus-only={set(by_cat)-set(CATEGORY_RANGES)} "
                 f"table-only={set(CATEGORY_RANGES)-set(by_cat)}")
    morphs = []  # dicts: id, category, rel, afmt_name
    for cat, (lo, hi) in CATEGORY_RANGES.items():
        rels = sorted(by_cat[cat], key=lambda p: p.as_posix().encode())
        if len(rels) > hi - lo + 1:
            sys.exit(f"category {cat}: {len(rels)} targets exceed block {lo}-{hi}")
        for i, rel in enumerate(rels):
            mid = lo + i
            morphs.append(dict(id=mid, category=cat, path=rel.as_posix(),
                               afmt=f"{mid}_{slug(rel)}.afmt"))
    total = len(morphs)
    assert len({m["id"] for m in morphs}) == total
    print(f"corpus: {total} targets in {len(by_cat)} categories")

    # ---- 2. convert each .target once with morph_converter -------------
    if work.exists():
        shutil.rmtree(work)
    stage = work / "afmt"
    stage.mkdir(parents=True)
    for n, m in enumerate(morphs, 1):
        run([conv, targets / m["path"], str(m["id"]), stage / m["afmt"]])
        if n % 200 == 0:
            print(f"  converted {n}/{total}")
    print(f"  converted {total}/{total}")

    # ---- 3. per-pack directory + pack_builder ---------------------------
    out.mkdir(parents=True, exist_ok=True)
    manifest_packs = []
    by_id = {m["id"]: m for m in morphs}
    for pack in PACKS:
        d = work / f"dir_{pack['id']}"
        d.mkdir()
        for f in BODY_DIR.iterdir():          # 4 rigged body .glb + master_skeleton.json only
            if f.suffix == ".glb" or f.name == "master_skeleton.json":
                shutil.copy2(f, d / f.name)
        members = [m for m in morphs if m["category"] in pack["categories"]]
        for m in members:
            os.link(stage / m["afmt"], d / m["afmt"])
        dest = out / pack["file"]
        stdout = run([packer, d, dest, BASE_OBJ]).strip()
        mt = re.match(r"wrote (\d+) part\(s\), (\d+) morph\(s\), (\d+) bytes", stdout)
        if not mt:
            sys.exit(f"unparseable pack_builder output: {stdout}")
        parts, nm, tb = map(int, mt.groups())
        assert nm == len(members), (pack["id"], nm, len(members))
        assert tb == dest.stat().st_size, (pack["id"], tb, dest.stat().st_size)
        print(f"{pack['id']:16s} parts={parts} morphs={nm} total_bytes={tb}")
        cats = pack["categories"]
        manifest_packs.append(dict(
            id=pack["id"], file=pack["file"], label=pack["label"],
            description=pack["description"],
            size_bytes=dest.stat().st_size,
            part_count=parts, morph_count=nm,
            morph_id_range=[min(CATEGORY_RANGES[c][0] for c in cats),
                            max(CATEGORY_RANGES[c][1] for c in cats)],
            categories=cats,
        ))

    # ---- 4. manifest + id map ------------------------------------------
    manifest = dict(
        manifest_version=1,
        note=("morph_id is globally unique and identical in every pack that contains the morph "
              "(so packs that are subsets of one another share IDs by design). morph_id_range is "
              "the min/max of the ALLOCATED blocks of the pack's categories; use "
              "morph_id_ranges_by_category for the exact blocks and morph_id_map.json for "
              "id->source-target."),
        morph_id_ranges_by_category={c: list(r) for c, r in CATEGORY_RANGES.items()},
        packs=manifest_packs,
    )
    (out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    (out / "morph_id_map.json").write_text(json.dumps(
        {str(m["id"]): dict(category=m["category"], target=m["path"]) for m in morphs},
        indent=1) + "\n")
    print("wrote manifest.json, morph_id_map.json")


if __name__ == "__main__":
    main()
