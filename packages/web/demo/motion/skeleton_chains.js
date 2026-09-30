// FROZEN CONTRACT — do not regenerate or guess these numbers.
// Verified directly against rust-core/packs/essentials.afpp's skeleton
// (packages/web/dist/index.js getSkeleton(), 163 joints) on 2026-09-21.
// If the engine build ever changes, re-derive with tools/dump_skeleton.mjs
// and update this file — nothing downstream should hardcode these indices itself.
//
// IMPORTANT: chains are NOT simple parent==previous-named-joint. There are
// unnamed single-child "twist" joints between every named pair (e.g. the
// left knee's actual parentIndex is 3, not 2). Anything that walks a chain
// must use the explicit index lists below, not assume adjacency between
// named joints.

// Each leg/arm chain: ordered joint indices from root-of-limb to tip,
// INCLUDING the twist joints, so FK/IK code can walk it exactly.
export const CHAINS = {
  legL: [2, 3, 4, 5, 6],      // hip, twist, knee, twist, ankle
  legR: [22, 23, 24, 25, 26], // hip, twist, knee, twist, ankle
  toeL: [6, 12],              // ankle -> toe base (direct child, no twist)
  toeR: [26, 32],
  armL: [48, 49, 50, 51, 52, 53, 54], // shoulder, twist, upperArm, twist, forearm, twist, hand
  armR: [74, 75, 76, 77, 78, 79, 80],
  spine: [42, 43, 44, 47],    // pelvis-side spine base -> chest (44 and 47 are named "spine_mid"/"spine_up"; 42/43 are lower)
  neck: [100],
  head: [102, 103],
};

// Single named joints referenced directly (no chain walk needed).
export const JOINTS = {
  root: 0,          // skeleton root; translation IS the pelvis/hip world position
  pelvis: 1,         // left leg hangs off this; right leg's chain (see legR[0]'s parent) hangs off `root` instead — asymmetric in this rig, harmless for our purposes
  hipL: 2, kneeL: 4, ankleL: 6, toeTipL: 8, toeBaseL: 12,
  hipR: 22, kneeR: 24, ankleR: 26, toeTipR: 28, toeBaseR: 32,
  spineLow: 43, spineMid: 44, spineUp: 47,
  shoulderL: 48, upperArmL: 50, forearmL: 52, handL: 54, midFinger1L: 63,
  shoulderR: 74, upperArmR: 76, forearmR: 78, handR: 80, midFinger1R: 89,
  neck: 100, head: 103, headTopA: 141, headTopB: 146,
};

// Bind-pose (default body, height=weight=1) bone lengths in engine units,
// i.e. the translation.y of each child joint along its chain (all these
// chains happen to run straight down local +Y in bind pose). Use these only
// as reference/fallback; PREFER computing actual lengths from a specific
// body's fitted skeleton (buildFitter(...).fit(positions).worldPos), since
// height/weight morphs change them per character — that's the whole reason
// skeleton_fit.js exists.
export const BIND_LENGTHS = {
  upperLeg: 0.81 + 3.42,   // hip->twist->knee, summed (twist joints carry no extra length here beyond translation.y)
  lowerLeg: 1.909 + 1.465, // knee->twist->ankle
  footLen: 0.27,           // ankle->toeTip (approx; toeBase branches off ankle directly)
  upperArm: 0.993 + 0.615, // shoulder->twist->upperArm... NOTE: verify against a live fit; see Part A spec for the required lookup helper instead of trusting this constant for anything visual.
  lowerArm: 1.132,
};
