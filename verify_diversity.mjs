// CC0-Phase 11b / 11b-2 verification. Run from the repo root:
//     node verify_diversity.mjs            (BATCHES=100 by default)
//     BATCHES=300 node verify_diversity.mjs
//
// Part 1 — one fresh 16-character batch, generated through the SAME
//   sampling code diversity.html imports (packages/web/demo/
//   identity_sampling.js), against the real rebuilt dist/index.js and the
//   real essentials.afpp. Reports real bounding boxes and checksums.
// Part 2 — distribution comparison over many batches (reproducible, seeded
//   RNG) of: the Phase 11 sampler (kept below ONLY for comparison), the
//   literal "one file from any of the 348", and the 11b default pool
//   (identity corners) at 11b's original 0.5-1.5 modifier range.
// Part 3 — grid-layout overlap under the OLD (character-#0-only) spacing
//   rule, for each Part 2 sampler — this is the 11b baseline being fixed.
// Part 4 (11b-2) — same identity-corner pool, A/B on modifier range alone
//   (0.5-1.5 vs 0.85-1.15): real squat-rate (h:w<1) effect, isolated from
//   everything else.
// Part 5 (11b-2) — same identity-corner pool + narrowed 0.85-1.15 range
//   (i.e. diversity.html's actual new narrowedRandomDna), A/B on spacing
//   rule alone (OLD char-#0-only vs NEW batch-max, i.e. the page's actual
//   new two-pass generateGrid): real overlap-rate effect, isolated from
//   everything else, plus the one number that matters — the real shipped
//   combination (new range + new spacing) vs the 11b baseline from Part 3.
//
// This cannot confirm three.js pixels / WebGL / OrbitControls / the button.

import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const root = process.cwd();
const { init, generate, getSkeleton, getLastError } = await import(
  pathToFileURL(`${root}/packages/web/dist/index.js`).href
);
const S = await import(pathToFileURL(`${root}/packages/web/demo/identity_sampling.js`).href);

// Node's fetch has no file:// support; serve the local files the way a
// static server at the repo root would. No product code changes.
const REAL_FETCH = globalThis.fetch;
globalThis.fetch = async (url) => {
  const href = typeof url === "string" ? url : url.href;
  if (href.startsWith("file://")) return new Response(readFileSync(new URL(href).pathname), { status: 200 });
  return REAL_FETCH(url);
};

const PART_PACK_URL = pathToFileURL(`${root}/rust-core/packs/essentials.afpp`).href;
const manifest = JSON.parse(readFileSync(`${root}/rust-core/packs/manifest.json`, "utf8"));
const morphIdMap = JSON.parse(readFileSync(`${root}/rust-core/packs/morph_id_map.json`, "utf8"));
const essentials = manifest.packs.find((p) => p.id === "essentials");
if (!essentials) throw new Error('no "essentials" pack in manifest.json');

const GRID_COUNT = 16, GRID_COLS = 4;
const BATCHES = Number(process.env.BATCHES ?? 100);
const DM = 0.1; // MakeHuman-internal unit is the decimeter: 1 unit = 0.1 m

// ---- pools ---------------------------------------------------------------
const identityCorners = S.selectIdentityCorners(morphIdMap, essentials.categories, S.POOL_IDENTITY_CORNERS);
const allMacro = S.selectIdentityCorners(morphIdMap, essentials.categories, S.POOL_ALL_MACRODETAILS);
const targetById = new Map(allMacro.map((c) => [c.id, c.target.replace(/^macrodetails\//, "").replace(/\.target$/, "")]));

console.log(`essentials categories: [${essentials.categories.join(", ")}]`);
console.log(`macrodetails ids in pack: ${allMacro.length} (range ${Math.min(...allMacro.map((c) => c.id))}..${Math.max(...allMacro.map((c) => c.id))})`);
const nTop = allMacro.filter((c) => c.target.split("/").length === 2).length;
const nSub = (d) => allMacro.filter((c) => c.target.startsWith(`macrodetails/${d}/`)).length;
console.log(`  top-level ${nTop} + height/ ${nSub("height")} + proportions/ ${nSub("proportions")} = ${nTop + nSub("height") + nSub("proportions")}`);
console.log(`  of the ${nTop} top-level: ${identityCorners.length} match {ethnicity}-{gender}-{age} (default pool), ${nTop - identityCorners.length} are universal-*-{muscle}-{weight}`);

await init({ partPackUrl: PART_PACK_URL, licenseKey: "" });
const skeleton = getSkeleton();
console.log(`getSkeleton(): ${skeleton ? skeleton.length + " joints" : "NULL — " + getLastError()}`);

// ---- helpers ----------------------------------------------------------------
function mulberry32(a) {
  return () => {
    a |= 0; a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function stats(positions) {
  let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity, minZ = Infinity, maxZ = -Infinity, sum = 0;
  for (let i = 0; i < positions.length; i += 3) {
    const x = positions[i], y = positions[i + 1], z = positions[i + 2];
    if (x < minX) minX = x; if (x > maxX) maxX = x;
    if (y < minY) minY = y; if (y > maxY) maxY = y;
    if (z < minZ) minZ = z; if (z > maxZ) maxZ = z;
    sum += x + y * 1.31 + z * 7.77; // cheap order-sensitive checksum
  }
  return { height: maxY - minY, width: maxX - minX, depth: maxZ - minZ, checksum: sum, vertCount: positions.length / 3 };
}

// Phase 11 sampler, copied verbatim (rng injectable) for COMPARISON ONLY.
function legacyRandomDna(ids, rng) {
  const pool = ids.slice(), picked = [];
  const morphCount = 3 + Math.floor(rng() * 4);
  for (let i = 0; i < morphCount && pool.length > 0; i++) {
    const idx = Math.floor(rng() * pool.length);
    picked.push(pool[idx]); pool.splice(idx, 1);
  }
  return {
    seed: BigInt(Math.floor(rng() * 1_000_000_000_000)),
    heightModifier: S.HEIGHT_MIN + rng() * (S.HEIGHT_MAX - S.HEIGHT_MIN),
    weightModifier: S.WEIGHT_MIN + rng() * (S.WEIGHT_MAX - S.WEIGHT_MIN),
    headId: S.HEAD_ID, torsoId: S.TORSO_ID, armsId: S.ARMS_ID, legsId: S.LEGS_ID,
    clothingIds: [],
    morphs: picked.map((id) => ({ id, weight: rng() })),
  };
}

function quantile(sorted, q) {
  const pos = (sorted.length - 1) * q, lo = Math.floor(pos), hi = Math.ceil(pos);
  return sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo);
}
const f = (x, n = 2) => x.toFixed(n);

// ============================================================================
// Part 1 — fresh 16-character batch through the page's own sampling code
// ============================================================================
console.log("\n=== PART 1: fresh 16-character batch (new sampler, default pool, Math.random) ===");
const errors = [], results = [];
let generated = 0;
for (let i = 0; i < GRID_COUNT; i++) {
  const dna = S.randomDna(identityCorners);
  if (dna.morphs.length !== 1 || dna.morphs[0].weight !== 1.0) throw new Error(`character ${i}: not exactly one morph at weight 1.0`);
  const character = generate(dna);
  if (character === null) { errors.push(`character ${i}: ${getLastError() ?? "null, no error message"}`); continue; }
  const errAfter = getLastError();
  if (errAfter) errors.push(`character ${i}: succeeded but getLastError() fired: ${errAfter}`);
  const s = stats(character.positions);
  results.push({ i, dna, corner: targetById.get(dna.morphs[0].id), ...s });
  generated++;
}
console.log(`generated ${generated}/${GRID_COUNT}, errors: ${errors.length}`);
if (errors.length) console.log(errors.join("\n"));

console.log("\nper character. bbox in MakeHuman decimeter units (x0.1 = meters); 'identity' = bbox with the");
console.log("height/weight modifiers divided back out (exact: the engine scales Y by heightModifier, X/Z by weightModifier).");
for (const r of results) {
  const hm = r.dna.heightModifier, wm = r.dna.weightModifier;
  console.log(
    `  #${String(r.i).padStart(2)} ${r.corner.padEnd(24)} morphs=[${r.dna.morphs.map((m) => `${m.id}@${m.weight}`)}] hm=${f(hm)} wm=${f(wm)}  ` +
    `bbox h/w/d=${f(r.height)}/${f(r.width)}/${f(r.depth)} (${f(r.height * DM)} m tall)  identity h/w=${f(r.height / hm)}/${f(r.width / wm)}  h:w=${f(r.height / r.width)}  ` +
    `verts=${r.vertCount} checksum=${f(r.checksum)}`,
  );
}
const uniqueChecksums = new Set(results.map((r) => r.checksum.toFixed(6)));
const uniqueBBoxes = new Set(results.map((r) => `${r.height.toFixed(4)}/${r.width.toFixed(4)}/${r.depth.toFixed(4)}`));
const uniqueCorners = new Set(results.map((r) => r.corner));
console.log(`\nunique geometry checksums: ${uniqueChecksums.size}/${results.length}`);
console.log(`unique bounding boxes:     ${uniqueBBoxes.size}/${results.length}`);
console.log(`distinct identity corners drawn: ${uniqueCorners.size} of ${identityCorners.length}`);
const H = results.map((r) => r.height), W = results.map((r) => r.width);
console.log(`height range across the 16: ${f(Math.min(...H))} .. ${f(Math.max(...H))}  (${f(Math.min(...H) * DM)} .. ${f(Math.max(...H) * DM)} m)`);
console.log(`width  range across the 16: ${f(Math.min(...W))} .. ${f(Math.max(...W))}`);
console.log(`squat (h:w < 1.0) in this batch: ${results.filter((r) => r.height / r.width < 1).length}/16`);

// ============================================================================
// Part 2 — distributions over many batches (seeded => reproducible)
// ============================================================================
console.log(`\n=== PART 2: distribution over ${BATCHES} batches x ${GRID_COUNT} = ${BATCHES * GRID_COUNT} characters per sampler (seeded, reproducible) ===`);
const SAMPLERS = [
  { name: "OLD  Phase 11: 3-6 random of all 348, random weights", make: (rng) => legacyRandomDna(allMacro.map((c) => c.id), rng), seed: 0xa11 },
  { name: "ONE  literal: 1 file of any of the 348, weight 1.0   ", make: (rng) => S.randomDna(allMacro, rng), seed: 0xb22 },
  { name: "NEW  default: 1 of the 24 identity corners, weight 1.0", make: (rng) => S.randomDna(identityCorners, rng), seed: 0xc33 },
];
const THRESH = { squat: 1.0, tallDm: 24, shortDm: 5 }; // arbitrary, fixed before looking at results
const perSampler = [];
for (const sp of SAMPLERS) {
  const rng = mulberry32(sp.seed);
  const hs = [], ws = [], ratios = [], ident = [], identRatios = [], batchRanges = [], sizes = [];
  let bad = 0;
  for (let b = 0; b < BATCHES; b++) {
    const bh = [];
    const batch = [];
    for (let i = 0; i < GRID_COUNT; i++) {
      const dna = sp.make(rng);
      const c = generate(dna);
      if (c === null) { bad++; continue; }
      const s = stats(c.positions);
      hs.push(s.height); ws.push(s.width); ratios.push(s.height / s.width);
      ident.push(s.height / dna.heightModifier);
      identRatios.push((s.height / dna.heightModifier) / (s.width / dna.weightModifier));
      bh.push(s.height); batch.push(s);
    }
    batchRanges.push([Math.min(...bh), Math.max(...bh)]);
    sizes.push(batch);
  }
  perSampler.push({ sp, hs, ws, ratios, ident, identRatios, batchRanges, sizes, bad });
}

const pct = (arr, pred) => (100 * arr.filter(pred).length / arr.length).toFixed(1) + "%";
for (const { sp, hs, ratios, ident, identRatios, batchRanges, bad } of perSampler) {
  const sh = hs.slice().sort((a, b) => a - b), si = ident.slice().sort((a, b) => a - b);
  const meanSpan = batchRanges.reduce((a, [lo, hi]) => a + (hi - lo), 0) / batchRanges.length;
  console.log(`\n${sp.name}   (generate() failures: ${bad})`);
  console.log(`  height (dm)         min ${f(sh[0])}  p5 ${f(quantile(sh, 0.05))}  median ${f(quantile(sh, 0.5))}  p95 ${f(quantile(sh, 0.95))}  max ${f(sh[sh.length - 1])}`);
  console.log(`  identity-only h(dm) min ${f(si[0])}  p5 ${f(quantile(si, 0.05))}  median ${f(quantile(si, 0.5))}  p95 ${f(quantile(si, 0.95))}  max ${f(si[si.length - 1])}   (modifiers divided out)`);
  console.log(`  per-batch height span, mean of (max-min) over batches: ${f(meanSpan)} dm`);
  console.log(`  share wider-than-tall (h:w < ${THRESH.squat}): ${pct(ratios, (r) => r < THRESH.squat)}   taller than ${THRESH.tallDm} dm (${f(THRESH.tallDm * DM, 1)} m): ${pct(hs, (h) => h > THRESH.tallDm)}   shorter than ${THRESH.shortDm} dm: ${pct(hs, (h) => h < THRESH.shortDm)}`);
  console.log(`  same wider-than-tall share with modifiers divided out (identity shape alone): ${pct(identRatios, (r) => r < THRESH.squat)}   identity-only h:w min ${f(Math.min(...identRatios))}`);
}

// ============================================================================
// Part 3 — does the page's (unchanged) grid spacing rule overlap neighbours?
// ============================================================================
console.log("\n=== PART 3: grid-layout overlap check — 11b BASELINE (char-#0-only spacing rule, pre-11b-2) ===");
console.log("spacingX = max(w0,d0)*1.6+0.4, spacingZ = d0*2.2+0.6 from character #0's bbox; a pair of");
console.log("grid neighbours 'overlaps' if their bbox extents (centered in cell) cross the cell spacing.");
for (const { sp, sizes } of perSampler) {
  let batchesWithOverlap = 0, pairsOver = 0, pairsTotal = 0;
  for (const batch of sizes) {
    if (batch.length < GRID_COUNT) continue;
    const spX = Math.max(batch[0].width, batch[0].depth) * 1.6 + 0.4;
    const spZ = batch[0].depth * 2.2 + 0.6;
    let any = false;
    for (let i = 0; i < GRID_COUNT; i++) {
      const col = i % GRID_COLS, row = Math.floor(i / GRID_COLS);
      if (col < GRID_COLS - 1) { pairsTotal++; if (batch[i].width / 2 + batch[i + 1].width / 2 > spX) { pairsOver++; any = true; } }
      if (row < GRID_COLS - 1) { pairsTotal++; if (batch[i].depth / 2 + batch[i + GRID_COLS].depth / 2 > spZ) { pairsOver++; any = true; } }
    }
    if (any) batchesWithOverlap++;
  }
  console.log(`  ${sp.name}  batches with >=1 overlapping neighbour pair: ${(100 * batchesWithOverlap / BATCHES).toFixed(0)}%  (pairs: ${(100 * pairsOver / pairsTotal).toFixed(1)}%)`);
}

// ============================================================================
// 11b-2 additions below. Same identity-corner pool throughout (sampling
// itself is out of scope for 11b-2) — only the modifier range and the
// spacing rule vary, isolated one at a time, matching diversity.html's real
// narrowedRandomDna()/two-pass generateGrid() exactly.
// ============================================================================

// Mirrors diversity.html's narrowedRandomDna(), generalized over a range so
// both the old (0.5-1.5) and new (0.85-1.15) range can be run through the
// exact same code path for a fair A/B.
function rangedRandomDna(corners, rng, lo, hi) {
  const dna = S.randomDna(corners, rng);
  dna.heightModifier = lo + rng() * (hi - lo);
  dna.weightModifier = lo + rng() * (hi - lo);
  return dna;
}
const RANGE_OLD = [0.5, 1.5]; // 11b's range (identity_sampling.js's own, unchanged)
const RANGE_NEW = [0.85, 1.15]; // 11b-2's range, applied in diversity.html only

function runBatches(seed, makeDna) {
  const rng = mulberry32(seed);
  const all = [], batches = [];
  let bad = 0;
  for (let b = 0; b < BATCHES; b++) {
    const batch = [];
    for (let i = 0; i < GRID_COUNT; i++) {
      const dna = makeDna(rng);
      const c = generate(dna);
      if (c === null) { bad++; continue; }
      const s = stats(c.positions);
      all.push(s);
      batch.push(s);
    }
    batches.push(batch);
  }
  return { all, batches, bad };
}

function overlapRate(batches, spacingRule) {
  let batchesWithOverlap = 0, pairsOver = 0, pairsTotal = 0;
  for (const batch of batches) {
    if (batch.length < GRID_COUNT) continue;
    const { spX, spZ } = spacingRule(batch);
    let any = false;
    for (let i = 0; i < GRID_COUNT; i++) {
      const col = i % GRID_COLS, row = Math.floor(i / GRID_COLS);
      if (col < GRID_COLS - 1) { pairsTotal++; if (batch[i].width / 2 + batch[i + 1].width / 2 > spX) { pairsOver++; any = true; } }
      if (row < GRID_COLS - 1) { pairsTotal++; if (batch[i].depth / 2 + batch[i + GRID_COLS].depth / 2 > spZ) { pairsOver++; any = true; } }
    }
    if (any) batchesWithOverlap++;
  }
  return { batchPct: 100 * batchesWithOverlap / BATCHES, pairPct: 100 * pairsOver / pairsTotal };
}
// OLD (11b) rule: spacing from character #0 alone.
const oldSpacingRule = (batch) => ({ spX: Math.max(batch[0].width, batch[0].depth) * 1.6 + 0.4, spZ: batch[0].depth * 2.2 + 0.6 });
// NEW (11b-2) rule: spacing from the whole batch's max width/depth — this is
// diversity.html's real new generateGrid() pass-2 formula, copied exactly.
const newSpacingRule = (batch) => {
  const maxWidth = Math.max(...batch.map((c) => c.width));
  const maxDepth = Math.max(...batch.map((c) => c.depth));
  return { spX: Math.max(maxWidth, maxDepth) * 1.6 + 0.4, spZ: maxDepth * 2.2 + 0.6 };
};

// ============================================================================
// Part 4 (11b-2, fix 1) — modifier range A/B, isolated: same pool, same
// spacing question set aside, only the range changes.
// ============================================================================
console.log("\n=== PART 4 (11b-2): modifier-range A/B on the SAME identity-corner pool, isolated ===");
const oldRangeRun = runBatches(0xd44, (rng) => rangedRandomDna(identityCorners, rng, ...RANGE_OLD));
const newRangeRun = runBatches(0xd44, (rng) => rangedRandomDna(identityCorners, rng, ...RANGE_NEW)); // same seed: same corner/order draws, only the range differs
for (const [label, run] of [["OLD range 0.50-1.50 (11b)      ", oldRangeRun], ["NEW range 0.85-1.15 (11b-2)    ", newRangeRun]]) {
  const ratios = run.all.map((c) => c.height / c.width);
  const hs = run.all.map((c) => c.height).sort((a, b) => a - b);
  console.log(`  ${label} generate() failures: ${run.bad}   squat (h:w<1): ${pct(ratios, (r) => r < 1)}   height p5/median/p95: ${f(quantile(hs, 0.05))}/${f(quantile(hs, 0.5))}/${f(quantile(hs, 0.95))} dm`);
}
console.log(`  -> real effect of narrowing the range alone on squat rate: ${pct(oldRangeRun.all.map((c) => c.height / c.width), (r) => r < 1)} -> ${pct(newRangeRun.all.map((c) => c.height / c.width), (r) => r < 1)}. As 11b's own re-analysis predicted (squatness tracks the modifiers, not identity selection), this is NOT expected to meaningfully change — reported here as measured, not assumed.`);

// ============================================================================
// Part 5 (11b-2, fix 2) — spacing-rule A/B, isolated (both under the NEW
// 0.85-1.15 range, i.e. diversity.html's real shipped modifier draw), plus
// the one number that matters: real shipped combo vs the Part-3 11b baseline.
// ============================================================================
console.log("\n=== PART 5 (11b-2): spacing-rule A/B under the NEW 0.85-1.15 range, isolated ===");
const newRangeForSpacing = runBatches(0xe55, (rng) => rangedRandomDna(identityCorners, rng, ...RANGE_NEW));
const oldSpacingOnNewRange = overlapRate(newRangeForSpacing.batches, oldSpacingRule);
const newSpacingOnNewRange = overlapRate(newRangeForSpacing.batches, newSpacingRule);
console.log(`  OLD spacing (char #0 only)  on NEW range: batches with overlap ${oldSpacingOnNewRange.batchPct.toFixed(0)}%  (pairs ${oldSpacingOnNewRange.pairPct.toFixed(1)}%)`);
console.log(`  NEW spacing (batch max)     on NEW range: batches with overlap ${newSpacingOnNewRange.batchPct.toFixed(0)}%  (pairs ${newSpacingOnNewRange.pairPct.toFixed(1)}%)`);

const baselineSampler = perSampler.find((p) => p.sp.name.startsWith("NEW  default"));
const baselineOverlap = overlapRate(baselineSampler.sizes, oldSpacingRule);
console.log(`\n  REAL SHIPPED COMBO (11b-2: new range + new spacing) vs 11b BASELINE (old range + old spacing, same as Part 3 above):`);
console.log(`    11b baseline    : batches with overlap ${baselineOverlap.batchPct.toFixed(0)}%  (pairs ${baselineOverlap.pairPct.toFixed(1)}%)`);
console.log(`    11b-2 shipped   : batches with overlap ${newSpacingOnNewRange.batchPct.toFixed(0)}%  (pairs ${newSpacingOnNewRange.pairPct.toFixed(1)}%)`);
