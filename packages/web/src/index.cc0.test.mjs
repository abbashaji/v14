// End-to-end tests of the REAL CC0 MakeHuman body through the whole chain --
// JS API -> wasm-bridge.ts (56-byte DNA) -> the wasm32 core -> mesh back --
// using fixtures/cc0_body.afpp (a real v2 pack: 4 rigged body parts + 2 real
// morph targets, built by rust-core's pack_builder). Run with:
//   node --test src/index.cc0.test.mjs
//
// These mirror rust-core/tests/cc0_phase5_diversity_regression.rs (which runs
// natively); what they add is proof that the same behaviour survives the wasm
// build and the JS marshalling. Real numbers from the native suite that these
// are cross-checked against: 53,512 vertices / 80,268 indices; the largest
// morph-5001 vertex moves 0.0480 in Y at weight 1.0.
//
// Part ids: 4001 head, 4002 torso, 4003 arms, 4004 legs; morphs 5001
// (asym-ear-1-l) and 5002 (asym-nose-1-l).

import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { startHarness } from "./test-harness.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));

let harness;
let api;
before(async () => {
  harness = await startHarness();
  api = await import("../dist/index.js");
  await api.init({ partPackUrl: `${harness.baseUrl}/cc0_body.afpp`, licenseKey: "test" });
});
after(async () => {
  await harness.close();
});

const dna = (over = {}) => ({
  seed: 42n,
  heightModifier: 1.0,
  weightModifier: 1.0,
  headId: 4001,
  torsoId: 4002,
  armsId: 4003,
  legsId: 4004,
  clothingIds: [],
  ...over,
});
const gen = (over) => {
  const r = api.generate(dna(over));
  assert.notEqual(r, null, `generate() returned null: ${api.getLastError()}`);
  return r;
};
const extents = (pos) => {
  const lo = [Infinity, Infinity, Infinity];
  const hi = [-Infinity, -Infinity, -Infinity];
  for (let i = 0; i < pos.length; i += 3) {
    for (let c = 0; c < 3; c++) {
      lo[c] = Math.min(lo[c], pos[i + c]);
      hi[c] = Math.max(hi[c], pos[i + c]);
    }
  }
  return hi.map((h, c) => h - lo[c]);
};
const maxAbsDiff = (a, b) => {
  let m = 0;
  for (let i = 0; i < a.length; i++) m = Math.max(m, Math.abs(a[i] - b[i]));
  return m;
};

test("the real 4-part body generates with the native vertex/index counts", () => {
  const r = gen();
  assert.equal(r.positions.length / 3, 53512);
  assert.equal(r.indices.length, 80268);
  assert.equal(r.boneIndices.length, 53512 * 4);
});

test("generation is deterministic", () => {
  assert.deepEqual(gen().positions, gen().positions);
});

test("heightModifier scales Y extent only; weightModifier scales X and Z only", () => {
  const base = extents(gen().positions);
  const tall = extents(gen({ heightModifier: 1.3 }).positions);
  assert.ok(Math.abs(tall[1] / base[1] - 1.3) < 0.005, `Y ratio ${tall[1] / base[1]}`);
  assert.ok(Math.abs(tall[0] / base[0] - 1) < 0.005 && Math.abs(tall[2] / base[2] - 1) < 0.005);

  const heavy = extents(gen({ weightModifier: 1.4 }).positions);
  assert.ok(Math.abs(heavy[0] / base[0] - 1.4) < 0.005, `X ratio ${heavy[0] / base[0]}`);
  assert.ok(Math.abs(heavy[2] / base[2] - 1.4) < 0.005, `Z ratio ${heavy[2] / base[2]}`);
  assert.ok(Math.abs(heavy[1] / base[1] - 1) < 0.005);
});

test("a real morph displaces the mesh, linearly in its weight; different morphs differ", () => {
  const base = gen().positions;
  const full = gen({ morphs: [{ id: 5001, weight: 1.0 }] }).positions;
  const half = gen({ morphs: [{ id: 5001, weight: 0.5 }] }).positions;
  const d1 = maxAbsDiff(full, base);
  const d05 = maxAbsDiff(half, base);
  assert.ok(d1 > 0.04 && d1 < 0.06, `morph 5001 @1.0 max displacement ${d1}`);
  assert.ok(Math.abs(d1 / d05 - 2) < 0.01, `weight linearity: ${d1} / ${d05}`);

  const nose = gen({ morphs: [{ id: 5002, weight: 1.0 }] }).positions;
  assert.ok(maxAbsDiff(nose, base) > 0.001, "morph 5002 should displace the mesh too");
  assert.ok(maxAbsDiff(nose, full) > 0.001, "morphs 5001 and 5002 must produce different meshes");
});

test("an unknown morph id is skipped, not fatal", () => {
  const base = gen().positions;
  assert.deepEqual(gen({ morphs: [{ id: 9999, weight: 1.0 }] }).positions, base);
});

// generate_character applies morphs BEFORE the height/weight body scale, so the
// morph's own displacement is scaled too. At the most-displaced vertex the Y
// displacement therefore grows by the height factor (1.15); scale-then-morph
// would leave it at 1.0. Pinned natively too; this proves the order survives
// the wasm build.
test("morphs are applied before the body scale (displacement grows by the height factor)", () => {
  const base = gen().positions;
  const morphed = gen({ morphs: [{ id: 5001, weight: 1.0 }] }).positions;
  const scaledOnly = gen({ heightModifier: 1.15 }).positions;
  const scaledMorphed = gen({ heightModifier: 1.15, morphs: [{ id: 5001, weight: 1.0 }] }).positions;

  let best = 0;
  let bestDy = 0;
  for (let i = 1; i < base.length; i += 3) {
    const dy = Math.abs(morphed[i] - base[i]);
    if (dy > bestDy) {
      bestDy = dy;
      best = i;
    }
  }
  assert.ok(bestDy > 0.04);
  const ratio = Math.abs(scaledMorphed[best] - scaledOnly[best]) / bestDy;
  assert.ok(Math.abs(ratio - 1.15) < 0.01, `expected ~1.15 (morph before scale), got ${ratio}`);
});

test("invalid DNA is rejected with the real error, and the next valid call still works", () => {
  assert.equal(api.generate(dna({ heightModifier: 0 })), null);
  assert.match(api.getLastError() ?? "", /scale\[1\] = 0/);
  assert.equal(api.generate(dna({ weightModifier: -1 })), null);
  assert.match(api.getLastError() ?? "", /DNA mutation failed/);
  assert.notEqual(api.generate(dna()), null);
});

test("getSkeleton() returns one joint per bone in master_skeleton.json, with a single root", () => {
  const skeletonPath = path.join(
    here, "..", "..", "..", "rust-core", "tests", "fixtures", "cc0_phase3_pipeline", "master_skeleton.json",
  );
  const expectedBones = Object.keys(JSON.parse(fs.readFileSync(skeletonPath, "utf8"))).length;
  const joints = api.getSkeleton();
  assert.notEqual(joints, null, `getSkeleton() null: ${api.getLastError()}`);
  assert.equal(joints.length, expectedBones);
  assert.equal(joints.filter((j) => j.parentIndex === -1).length, 1, "exactly one root joint");
});
