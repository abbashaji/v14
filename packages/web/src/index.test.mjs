// Verification tests for the AnthroForge/Web SDK core against the tiny
// 4-part fixture pack (fixtures/real_test.afpp), run with:
//   node --test src/index.test.mjs
// (See test-harness.mjs for how these tests reach fetch(), and
// index.cc0.test.mjs for the real CC0 body.)
//
// Fixture part ids (rust-core/tests/fixtures/real_pack_e2e/): every
// character needs all four body parts, so tests always pass all four ids.
//   head  1001 (OBJ, 3 verts / 3 indices)
//   torso 2002 (GLB, 4 verts / 6 indices)
//   arms  2001 (GLB, 3 verts / 3 indices)
//   legs  1002 (OBJ, 5 verts / 9 indices)

import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import { startHarness } from "./test-harness.mjs";

let harness;
before(async () => {
  harness = await startHarness();
});
after(async () => {
  await harness.close();
});

const BODY = { headId: 1001, torsoId: 2002, armsId: 2001, legsId: 1002 };
const dna = (over = {}) => ({
  seed: 12345n,
  heightModifier: 1.0,
  weightModifier: 1.0,
  ...BODY,
  clothingIds: [],
  ...over,
});

test("init() loads the real wasm module and the real v2 .afpp pack", async () => {
  const { init } = await import("../dist/index.js");
  await assert.doesNotReject(
    init({ partPackUrl: `${harness.baseUrl}/real_test.afpp`, licenseKey: "test-license-key" }),
  );
});

test("generate() with all four real part ids returns the merged 4-part mesh", async () => {
  const { generate } = await import("../dist/index.js");
  const result = generate(dna());

  assert.notEqual(result, null);
  // 3 + 4 + 3 + 5 vertices and 3 + 6 + 3 + 9 indices, per the fixture parts.
  assert.equal(result.positions.length / 3, 15);
  assert.equal(result.indices.length, 21);
  assert.equal(result.normals.length, result.positions.length);
  assert.equal(result.uvs.length, (result.positions.length / 3) * 2);
  assert.equal(result.boneIndices.length, (result.positions.length / 3) * 4);
  assert.equal(result.boneWeights.length, (result.positions.length / 3) * 4);
  assert.equal(result.atlasBytes.length, 0);
  assert.equal(result.atlasWidth, 0);
  assert.equal(result.atlasHeight, 0);
});

test("generate() with invalid DNA (heightModifier: 0) returns null and sets a real error", async () => {
  const { generate, getLastError } = await import("../dist/index.js");
  assert.equal(generate(dna({ heightModifier: 0 })), null);
  const err = getLastError();
  assert.ok(err && err.includes("scale[1] = 0"), `unexpected error: ${err}`);
});

// armsId/legsId are required and read from their own DNA fields (offsets 32
// and 36). The id echoed back in the error is the proof that the bridge wrote
// each one to the right offset -- a swapped or missing write would name a
// different id or succeed.
test("an unknown armsId / legsId is reported by name, with the id that was sent", async () => {
  const { generate, getLastError } = await import("../dist/index.js");

  assert.equal(generate(dna({ armsId: 999999 })), null);
  assert.match(getLastError() ?? "", /no part loaded for arms_id 999999/);

  assert.equal(generate(dna({ legsId: 888888 })), null);
  assert.match(getLastError() ?? "", /no part loaded for legs_id 888888/);

  assert.equal(generate(dna({ headId: 777777 })), null);
  assert.match(getLastError() ?? "", /no part loaded for head_id 777777/);

  assert.equal(generate(dna({ torsoId: 666666 })), null);
  assert.match(getLastError() ?? "", /no part loaded for torso_id 666666/);

  // and a valid call afterwards still works (no poisoned state)
  assert.notEqual(generate(dna()), null);
});

test("freeCharacter() is a callable no-op", async () => {
  const { generate, freeCharacter } = await import("../dist/index.js");
  const result = generate(dna({ seed: 999n }));
  assert.notEqual(result, null);
  assert.doesNotThrow(() => freeCharacter(result));
});

// The tiny fixture pack contains no morphs, which makes it a clean test of the
// morph marshalling path itself: an id with no loaded morph must be skipped
// (not an error), leaving the output identical to a no-morph call; and a
// morph id outside u16 range must be rejected in JS rather than silently
// truncated.
test("morph weights are marshalled: an unloaded morph id is skipped, an out-of-range id is rejected", async () => {
  const { generate } = await import("../dist/index.js");
  const baseline = generate(dna());
  const withUnknownMorph = generate(dna({ morphs: [{ id: 5001, weight: 1.0 }] }));
  assert.notEqual(withUnknownMorph, null);
  assert.deepEqual(withUnknownMorph.positions, baseline.positions);

  assert.throws(() => generate(dna({ morphs: [{ id: 70000, weight: 1.0 }] })), RangeError);
  assert.throws(() => generate(dna({ morphs: [{ id: -1, weight: 1.0 }] })), RangeError);
});

// generate_character's clothing integration (equipped_clothing_ids_ptr -> fit
// -> merge into one output mesh) must actually change the output, not just
// fail to error. Part id 1002 ("legs") is reused as a stand-in equippable
// item: the point is to prove the pipeline runs and changes the output, not to
// validate real garment fitting quality.
test("generate() with a real equipped clothingId actually merges it into the output", async () => {
  const { generate, getLastError } = await import("../dist/index.js");

  const bodyOnly = generate(dna({ seed: 42n }));
  assert.notEqual(bodyOnly, null, `body-only generate() failed: ${getLastError()}`);

  const withClothing = generate(dna({ seed: 42n, clothingIds: [1002] }));
  assert.notEqual(withClothing, null, `clothing generate() returned null: ${getLastError()}`);
  assert.ok(
    withClothing.positions.length > bodyOnly.positions.length,
    `expected clothing to add vertices (body ${bodyOnly.positions.length / 3}, ` +
      `with clothing ${withClothing.positions.length / 3}); equal means it was silently skipped`,
  );
  assert.ok(withClothing.indices.length > bodyOnly.indices.length);
});
