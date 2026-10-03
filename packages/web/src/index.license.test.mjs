// `licenseKey` is optional: init() must work when it is omitted. Run with:
//   node --test src/index.license.test.mjs
// Same harness and fixture pack as index.test.mjs (see that file for the
// fixture part ids).

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

test("init() works without a licenseKey", async () => {
  const { init, generate } = await import("../dist/index.js");
  await assert.doesNotReject(init({ partPackUrl: `${harness.baseUrl}/real_test.afpp` }));

  const result = generate({
    seed: 12345n,
    heightModifier: 1.0,
    weightModifier: 1.0,
    ...BODY,
    clothingIds: [],
  });
  assert.notEqual(result, null);
  assert.equal(result.positions.length / 3, 15);
  assert.equal(result.indices.length, 21);
});
