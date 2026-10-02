// Tests of the demo sampling code's safety rule against the REAL morph map
// (rust-core/packs/morph_id_map.json) and the REAL manifest. The enforcing
// gate is rust-core/src/safety.rs; demo/identity_sampling.js mirrors its rule
// on target names so the demo never offers a denied id. Run with:
//   node --test src/demo-safety.test.mjs
//
// No wasm and no harness: identity_sampling.js is a dependency-free ES module.

import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import * as S from "../demo/identity_sampling.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const packsDir = path.resolve(here, "../../../rust-core/packs");
const morphIdMap = JSON.parse(fs.readFileSync(path.join(packsDir, "morph_id_map.json"), "utf8"));
const manifest = JSON.parse(fs.readFileSync(path.join(packsDir, "manifest.json"), "utf8"));
const essentials = manifest.packs.find((p) => p.id === "essentials");

// Written out here on purpose, NOT imported: ground truth for "a baby or
// child token in the target name", independent of S.MINOR_TARGET_RE.
const BABY_OR_CHILD_TOKEN = /(?:^|[/\-_.])(?:baby|child)(?:[/\-_.]|$)/;

/** A small seeded LCG returning floats in [0, 1). */
function lcg(seed) {
  let state = seed >>> 0;
  return () => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    return state / 4294967296;
  };
}

test("isDeniedMorphEntry denies exactly 228 minor and 6 genital morphs of 1280", () => {
  const entries = Object.entries(morphIdMap);
  assert.equal(entries.length, 1280);

  const tokenMatches = entries.filter(([, e]) => BABY_OR_CHILD_TOKEN.test(e.target));
  assert.equal(tokenMatches.length, 228);

  const genitals = entries.filter(([, e]) => e.category === "genitals");
  assert.equal(genitals.length, 6);

  const denied = entries.filter(([, e]) => S.isDeniedMorphEntry(e));
  assert.equal(denied.length, 234);

  for (const id of [1020, 1021, 2308, 2603]) {
    assert.equal(S.isDeniedMorphEntry(morphIdMap[String(id)]), true, `id ${id} must be denied`);
  }
  for (const id of [1022, 1023, 1095]) {
    assert.equal(S.isDeniedMorphEntry(morphIdMap[String(id)]), false, `id ${id} must be permitted`);
  }
});

test("default identity-corner pool is the 12 young/old corners", () => {
  assert.ok(essentials, 'manifest.json has a pack with id "essentials"');
  const pool = S.selectIdentityCorners(morphIdMap, essentials.categories);
  assert.equal(pool.length, 12);
  for (const { id, target } of pool) {
    assert.ok(
      target.endsWith("-young.target") || target.endsWith("-old.target"),
      `id ${id} (${target}) is not a young/old corner`,
    );
    assert.equal(S.isDeniedMorphEntry(morphIdMap[String(id)]), false, `id ${id} is denied`);
  }
});

test("all-macrodetails pool is 192 ids and holds no denied morph", () => {
  assert.ok(essentials, 'manifest.json has a pack with id "essentials"');
  const pool = S.selectIdentityCorners(morphIdMap, essentials.categories, S.POOL_ALL_MACRODETAILS);
  assert.equal(pool.length, 192);
  for (const { id } of pool) {
    assert.equal(S.isDeniedMorphEntry(morphIdMap[String(id)]), false, `id ${id} is denied`);
  }
});

test("randomDna never draws a denied id (5000 seeded draws from each pool)", () => {
  assert.ok(essentials, 'manifest.json has a pack with id "essentials"');
  for (const poolName of [S.POOL_IDENTITY_CORNERS, S.POOL_ALL_MACRODETAILS]) {
    const pool = S.selectIdentityCorners(morphIdMap, essentials.categories, poolName);
    const rng = lcg(12345);
    for (let i = 0; i < 5000; i++) {
      const dna = S.randomDna(pool, rng);
      assert.ok(dna.morphs.length >= 1, `${poolName} draw ${i} has no morph`);
      for (const { id } of dna.morphs) {
        const entry = morphIdMap[String(id)];
        assert.ok(entry, `${poolName} draw ${i}: id ${id} is not in the morph map`);
        assert.equal(S.isDeniedMorphEntry(entry), false, `${poolName} draw ${i}: denied id ${id}`);
      }
    }
  }
});
