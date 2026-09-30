/**
 * packages/web/demo/identity_sampling.js
 *
 * CC0-Phase 11b — macrodetail sampling: ONE identity per character.
 *
 * Plain ES module, no dependencies, no DOM, no wasm. Imported by both
 * diversity.html (browser) and verify_diversity.mjs (Node), so the Node
 * verification exercises the exact sampling code the page runs rather than
 * a hand-copied mirror of it.
 *
 * ---------------------------------------------------------------------
 * What a "macrodetails" file is, measured against the real corpus
 * ---------------------------------------------------------------------
 * The 348 `macrodetails` targets in essentials.afpp are 96 + 144 + 108:
 *
 *    24  macrodetails/{african|asian|caucasian}-{female|male}-{baby|child|young|old}
 *    72  macrodetails/universal-{female|male}-{age}-{muscle}-{weight}
 *   144  macrodetails/height/{female|male}-{age}-{muscle}-{weight}-{min|max}height
 *   108  macrodetails/proportions/{female|male}-{age}-{muscle}-{weight}-{ideal|uncommon}proportions
 *
 * Only the first 24 are standalone identities. Generated one at a time at
 * weight 1.0 on the real pack (all modifiers 1.0), the other 324 behave
 * like refinement deltas meant to be layered on an identity, not
 * identities themselves:
 *   - universal-*   : bbox height 16.63..16.70 vs 16.66 for the no-morph
 *                     base mesh, i.e. ~the base mesh again;
 *   - proportions/* : bbox height 16.59..16.75, same story;
 *   - height/*      : bbox height 12.99..23.89; the 36 *baby* height files
 *                     alone give 15.0..19.6 (adult-sized) because they are
 *                     deltas meant to sit on a baby identity, applied here
 *                     to the adult base mesh.
 * The 24 identity corners give baby ~6.0, child 12.2-14.0, young/old adult
 * 14.9-18.2 (MakeHuman is internally in decimeters, so ~0.6 m, ~1.2-1.4 m,
 * ~1.5-1.8 m).
 *
 * So the default pool is the 24 identity corners. The pool is a parameter
 * so the literal "any of the 348" alternative stays one argument away
 * (verify_diversity.mjs runs both and reports the difference).
 *
 * Deliberately NOT here: blending several corners (e.g. 70% young + 30%
 * old) — that is barycentric interpolation across a multi-axis simplex,
 * a separate, larger piece of work.
 */

// Part IDs: confirmed by parsing essentials.afpp's AFPP v2 part index.
export const HEAD_ID = 4001;
export const TORSO_ID = 4002;
export const ARMS_ID = 4003;
export const LEGS_ID = 4004;

// Copied from demo/index.html's #heightSlider / #weightSlider min/max.
// Unchanged from the Phase 11 demo. NB: the engine applies these as a
// whole-body scale (Y by height, X/Z by weight), so they multiply
// whatever the identity corner is.
export const HEIGHT_MIN = 0.5, HEIGHT_MAX = 1.5;
export const WEIGHT_MIN = 0.5, WEIGHT_MAX = 1.5;

/** The 24 real standalone identity corners. */
export const IDENTITY_CORNER_RE =
  /^macrodetails\/(african|asian|caucasian)-(female|male)-(baby|child|young|old)\.target$/;

/** Pool names accepted by selectIdentityCorners(). */
export const POOL_IDENTITY_CORNERS = "identity-corners"; // default: the 24
export const POOL_ALL_MACRODETAILS = "all-macrodetails"; // literal: all 348

/**
 * Filters morph_id_map.json down to the ids eligible to be a character's
 * single identity.
 *
 * @param {Record<string, {category: string, target: string}>} morphIdMap
 *        parsed morph_id_map.json
 * @param {Iterable<string>} essentialsCategories
 *        the "essentials" pack's `categories` from manifest.json
 * @param {string} [pool]
 * @returns {{id: number, target: string}[]}
 */
export function selectIdentityCorners(
  morphIdMap,
  essentialsCategories,
  pool = POOL_IDENTITY_CORNERS,
) {
  if (pool !== POOL_IDENTITY_CORNERS && pool !== POOL_ALL_MACRODETAILS) {
    throw new Error(`unknown sampling pool "${pool}"`);
  }
  const cats = new Set(essentialsCategories);
  const out = [];
  for (const [idStr, entry] of Object.entries(morphIdMap)) {
    if (!cats.has(entry.category)) continue;
    if (pool === POOL_IDENTITY_CORNERS && !IDENTITY_CORNER_RE.test(entry.target)) continue;
    out.push({ id: Number(idStr), target: entry.target });
  }
  return out;
}

/**
 * One random CharacterDNA: exactly one macrodetails file at weight 1.0.
 * Seed and height/weight modifiers are drawn exactly as in the Phase 11
 * demo (seed in [0, 1e12), modifiers uniform in 0.5..1.5).
 *
 * @param {{id: number}[]} corners  from selectIdentityCorners()
 * @param {() => number} [rng]      defaults to Math.random; injectable so
 *                                  the verifier can run reproducible batches
 */
export function randomDna(corners, rng = Math.random) {
  if (corners.length === 0) throw new Error("randomDna: empty corner pool");
  const corner = corners[Math.floor(rng() * corners.length)];
  return {
    seed: BigInt(Math.floor(rng() * 1_000_000_000_000)),
    heightModifier: HEIGHT_MIN + rng() * (HEIGHT_MAX - HEIGHT_MIN),
    weightModifier: WEIGHT_MIN + rng() * (WEIGHT_MAX - WEIGHT_MIN),
    headId: HEAD_ID,
    torsoId: TORSO_ID,
    armsId: ARMS_ID,
    legsId: LEGS_ID,
    clothingIds: [],
    morphs: [{ id: corner.id, weight: 1.0 }],
  };
}
