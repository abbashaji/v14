// Procedural walk / idle cycle generator.
//
// Coordinate frame (character-local walk frame, frozen contract):
//   origin at ground level directly below the pelvis, +Y up, +Z the
//   direction the character is walking/facing, +X to the character's left.
//   Units are metres. This module knows nothing about engine joint
//   indices, three.js, or the DOM — it only produces numbers in this frame.
//
// No imports, no engine coupling. Every length that matters (leg length,
// hip width) is taken as an explicit parameter rather than hardcoded, so
// bodies that are +/-15% off a "typical" 0.9 m leg / 0.5 m torso still get
// plausible motion.

// ---------------------------------------------------------------------
// Shared gait constants / helpers
// ---------------------------------------------------------------------

// Fraction of one leg's own cycle spent in stance (foot planted). Real
// human gait is close to 60% stance / 40% swing at a normal walking speed;
// we hold this fixed rather than varying it with speed, which is a
// simplification but keeps the contact/lift geometry easy to reason about.
const STANCE_FRACTION = 0.6;

// Stride *frequency* (full gait cycles per second, i.e. 1 / stridePeriod)
// as a function of forward speed. Real walkers increase both cadence and
// stride length as they speed up (not just stride length), so this must
// depend on speed. Chosen formula (documented, not the only valid choice):
//   frequency(speed) = BASE_FREQ_HZ + FREQ_PER_SPEED * speed
// At speed = 0 this still returns BASE_FREQ_HZ (never zero), which is what
// keeps walkPose free of divide-by-zero / NaN for very small speeds.
const BASE_FREQ_HZ = 0.8; // idle-ish cadence baseline if speed were 0
const FREQ_PER_SPEED = 0.6; // Hz gained per additional m/s of speed

function strideFrequency(speed) {
  const s = Number.isFinite(speed) && speed > 0 ? speed : 0;
  return BASE_FREQ_HZ + FREQ_PER_SPEED * s;
}

/**
 * Full stride period (seconds) for a given forward speed. Exported because
 * the test file needs it to check pose continuity across a stride wrap.
 * @param speed m/s, >= 0.
 */
export function stridePeriod(speed) {
  return 1 / strideFrequency(speed);
}

function lerp(a, b, t) {
  return a + (b - a) * t;
}

// Deterministic pseudo-random value in [0, 1) from two integers/numbers.
// Classic "fract(sin(x)*large)" hash — deterministic, no Math.random, so
// idlePose stays a pure function of (t, seed) as required.
function hash01(seed, salt) {
  const v = Math.sin(seed * 12.9898 + salt * 78.233) * 43758.5453;
  return v - Math.floor(v);
}

// ---------------------------------------------------------------------
// footTarget
// ---------------------------------------------------------------------

/**
 * One leg's target foot position and lift, for a walk cycle.
 * @param phase   number in [0,1), this leg's own phase within its stride.
 * @param strideLength  metres, front-to-back distance during stance.
 * @param stepHeight    metres, max foot lift during swing.
 * @param legLength     metres, hip-to-ankle length, used to clamp the two
 *                above to plausible fractions of leg length.
 * @returns { x, y, z, grounded }
 */
export function footTarget(phase, strideLength, stepHeight, legLength) {
  // Normalise phase defensively into [0, 1).
  let p = phase % 1;
  if (p < 0) p += 1;

  const safeLegLength = Number.isFinite(legLength) && legLength > 0 ? legLength : 0.9;

  // Clamp so short/tall bodies never over- or under-step relative to their
  // own leg length. These fractions are a deliberate design choice, not a
  // biomechanical constant: stride capped at ~0.9x leg length, lift capped
  // at ~0.25x leg length.
  const maxStride = safeLegLength * 0.9;
  const maxStepHeight = safeLegLength * 0.25;
  const clampedStride = Math.max(0, Math.min(strideLength, maxStride));
  const clampedStepHeight = Math.max(0, Math.min(stepHeight, maxStepHeight));
  const half = clampedStride / 2;

  let y;
  let z;
  let grounded;

  if (p <= STANCE_FRACTION) {
    // Stance: foot planted, moves linearly from forward (+half) to
    // backward (-half) under the body as the body passes over it.
    grounded = true;
    y = 0;
    const stanceT = STANCE_FRACTION === 0 ? 0 : p / STANCE_FRACTION;
    z = half - stanceT * clampedStride;
  } else {
    // Swing: foot lifts off and arcs forward again to plant ahead of the
    // body. sin(pi * swingT) is 0 at both ends of swing and peaks at the
    // midpoint, giving a smooth up-and-over arc with no discontinuity at
    // either the stance->swing or swing->stance seam (both ends already
    // agree with the adjoining stance z values, see below).
    grounded = false;
    const swingT = (p - STANCE_FRACTION) / (1 - STANCE_FRACTION);
    y = clampedStepHeight * Math.sin(Math.PI * swingT);
    z = -half + swingT * clampedStride;
  }

  return { x: 0, y, z, grounded };
}

// ---------------------------------------------------------------------
// walkPose
// ---------------------------------------------------------------------

/**
 * Full-body walk pose at time t (seconds) and speed (m/s).
 */
export function walkPose(t, speed, legLength, hipWidth) {
  const safeSpeed = Number.isFinite(speed) && speed > 0 ? speed : 0;
  const safeLegLength = Number.isFinite(legLength) && legLength > 0 ? legLength : 0.9;
  const safeHipWidth = Number.isFinite(hipWidth) ? hipWidth : 0.1;

  const freq = strideFrequency(safeSpeed); // Hz, always > 0, see strideFrequency()
  const continuousPhase = freq * t; // NOT wrapped — used for continuous sinusoids

  let leftPhase = continuousPhase % 1;
  if (leftPhase < 0) leftPhase += 1;
  let rightPhase = (continuousPhase + 0.5) % 1;
  if (rightPhase < 0) rightPhase += 1;

  // Stride length grows with speed: distance covered per stride period.
  // (distance = speed * period = speed / freq). This is one concrete,
  // physically-motivated choice, not the only valid one.
  const strideLength = safeSpeed / freq;

  // Step height also grows mildly with speed (higher clearance at faster
  // walks), clamped later inside footTarget relative to legLength anyway.
  const stepHeight = 0.08 + 0.05 * Math.min(safeSpeed, 1.5) / 1.5;

  const leftFootRaw = footTarget(leftPhase, strideLength, stepHeight, safeLegLength);
  const rightFootRaw = footTarget(rightPhase, strideLength, stepHeight, safeLegLength);

  const leftFoot = { ...leftFootRaw, x: safeHipWidth };
  const rightFoot = { ...rightFootRaw, x: -safeHipWidth };

  // legSine: a single continuous sine tied to the left leg's cycle,
  // reused (with sign flips) to drive hip sway/roll, spine counter-yaw,
  // and arm swing so everything stays phase-locked and continuous across
  // stride wraps (no modulo used here, so no seam).
  const legSine = Math.sin(2 * Math.PI * continuousPhase);

  // Pelvis bobs down at each foot strike — twice per full gait cycle (once
  // per step, either leg). abs(sin(2*pi*f*t)) has period 1/(2f), i.e.
  // exactly half a stride period, which is what gives the double-bob.
  const HIP_BOB_AMPLITUDE = 0.02; // metres, spec allows "up to ~0.03"
  const hipBob = HIP_BOB_AMPLITUDE * Math.abs(Math.sin(2 * Math.PI * continuousPhase));

  const HIP_YAW_AMPLITUDE = 0.05; // radians
  const hipSwayYaw = HIP_YAW_AMPLITUDE * legSine;

  // Roll tilts the pelvis down on the swing-leg side; tied to the same
  // sine so it stays synchronized with the stepping pattern.
  const HIP_ROLL_AMPLITUDE = 0.035; // radians
  const hipSwayRoll = HIP_ROLL_AMPLITUDE * legSine;

  // Upper body counter-rotates against the hip yaw to keep the shoulders
  // roughly forward-facing, at a fraction of the hip yaw's amplitude.
  const SPINE_COUNTER_FACTOR = 0.6;
  const spineCounterYaw = -SPINE_COUNTER_FACTOR * hipSwayYaw;

  // Arm swing: each hand swings opposite its same-side leg (contralateral
  // gait pattern), i.e. left hand tracks the *right* leg's forward/back
  // motion and vice versa. Amplitude is expressed relative to legLength
  // since no arm-length parameter is available at this call site.
  const HAND_SWING_FACTOR = 0.28; // fraction of legLength used as swing amplitude
  const handAmp = HAND_SWING_FACTOR * safeLegLength;
  const leftHand = { x: 0, y: hipBob * 0.5, z: -handAmp * legSine };
  const rightHand = { x: 0, y: hipBob * 0.5, z: handAmp * legSine };

  return {
    leftFoot,
    rightFoot,
    hipBob,
    hipSwayYaw,
    hipSwayRoll,
    spineCounterYaw,
    leftHand,
    rightHand,
  };
}

// ---------------------------------------------------------------------
// idlePose
// ---------------------------------------------------------------------

/**
 * Standing-still pose: small idle sway/breathing, no footsteps.
 * Pure function of (t, seed) — no Math.random — so callers can seed
 * different characters to idle out of sync deterministically.
 */
export function idlePose(t, seed) {
  const s = Number.isFinite(seed) ? seed : 0;

  const breathFreq = 0.25 + hash01(s, 2) * 0.15; // 0.25-0.4 Hz slow breathing
  const breathPhase = hash01(s, 1) * 2 * Math.PI;
  const swayFreq = 0.15 + hash01(s, 4) * 0.1; // slower left/right weight-shift
  const swayPhase = hash01(s, 3) * 2 * Math.PI;

  const breath = Math.sin(2 * Math.PI * breathFreq * t + breathPhase);
  const sway = Math.sin(2 * Math.PI * swayFreq * t + swayPhase);

  const IDLE_BOB_AMPLITUDE = 0.006; // metres
  const hipBob = IDLE_BOB_AMPLITUDE * (0.5 + 0.5 * breath); // stays >= 0

  const IDLE_YAW_AMPLITUDE = 0.015; // radians
  const hipSwayYaw = IDLE_YAW_AMPLITUDE * sway;

  const IDLE_ROLL_AMPLITUDE = 0.012; // radians
  const hipSwayRoll = IDLE_ROLL_AMPLITUDE * Math.sin(2 * Math.PI * swayFreq * t + swayPhase + Math.PI / 2);

  const spineCounterYaw = -0.5 * hipSwayYaw;

  // Small weight-shift in z, well under walking stride amplitude; feet
  // stay planted (y == 0, grounded true) at all times.
  const IDLE_FOOT_SWAY = 0.01; // metres
  const leftFoot = { x: 0, y: 0, z: IDLE_FOOT_SWAY * sway, grounded: true };
  const rightFoot = { x: 0, y: 0, z: -IDLE_FOOT_SWAY * sway, grounded: true };

  const IDLE_HAND_SWAY = 0.015; // metres, relaxed near sides — much smaller
  // than the walking arm-swing amplitude.
  const leftHand = {
    x: IDLE_HAND_SWAY * 0.4 * Math.sin(2 * Math.PI * breathFreq * t + breathPhase + hash01(s, 5)),
    y: IDLE_HAND_SWAY * 0.3 * breath,
    z: IDLE_HAND_SWAY * sway,
  };
  const rightHand = {
    x: -IDLE_HAND_SWAY * 0.4 * Math.sin(2 * Math.PI * breathFreq * t + breathPhase + hash01(s, 6)),
    y: IDLE_HAND_SWAY * 0.3 * breath,
    z: -IDLE_HAND_SWAY * sway,
  };

  return {
    leftFoot,
    rightFoot,
    hipBob,
    hipSwayYaw,
    hipSwayRoll,
    spineCounterYaw,
    leftHand,
    rightHand,
  };
}

// ---------------------------------------------------------------------
// locomotionPose
// ---------------------------------------------------------------------

// Smoothstep easing between the idle and walk blend bounds, so the
// crossfade has zero slope at both ends (no velocity pop as speed crosses
// the thresholds).
function speedBlend(speed) {
  const LOW = 0.05; // m/s: at/under this, pure idle
  const HIGH = 0.3; // m/s: at/over this, pure walk
  if (speed <= LOW) return 0;
  if (speed >= HIGH) return 1;
  const x = (speed - LOW) / (HIGH - LOW);
  return x * x * (3 - 2 * x); // smoothstep
}

function lerpFoot(idleFoot, walkFoot, blend) {
  const y = lerp(idleFoot.y, walkFoot.y, blend);
  return {
    x: lerp(idleFoot.x, walkFoot.x, blend),
    y,
    z: lerp(idleFoot.z, walkFoot.z, blend),
    // grounded is derived from the *lerped* y rather than carried from
    // either source pose: thresholding the blended height is what keeps
    // "grounded" consistent with the actual foot height the caller will
    // see, at every intermediate blend weight, not just at the two pure
    // endpoints.
    grounded: y < 1e-4,
  };
}

function lerpHand(idleHand, walkHand, blend) {
  return {
    x: lerp(idleHand.x, walkHand.x, blend),
    y: lerp(idleHand.y, walkHand.y, blend),
    z: lerp(idleHand.z, walkHand.z, blend),
  };
}

/**
 * Cross-fades walkPose and idlePose by speed.
 */
export function locomotionPose(t, speed, legLength, hipWidth, seed) {
  const safeSpeed = Number.isFinite(speed) && speed > 0 ? speed : 0;
  const blend = speedBlend(safeSpeed);

  const idle = idlePose(t, seed);
  // walkPose must never be called with speed exactly 0 (see its own
  // NaN/divide-by-zero contract note); feed it a tiny floor instead. Its
  // output is only used in proportion to `blend`, so this is inaudible
  // when blend is near 0.
  const walk = walkPose(t, Math.max(safeSpeed, 1e-4), legLength, hipWidth);

  return {
    leftFoot: lerpFoot(idle.leftFoot, walk.leftFoot, blend),
    rightFoot: lerpFoot(idle.rightFoot, walk.rightFoot, blend),
    hipBob: lerp(idle.hipBob, walk.hipBob, blend),
    hipSwayYaw: lerp(idle.hipSwayYaw, walk.hipSwayYaw, blend),
    hipSwayRoll: lerp(idle.hipSwayRoll, walk.hipSwayRoll, blend),
    spineCounterYaw: lerp(idle.spineCounterYaw, walk.spineCounterYaw, blend),
    leftHand: lerpHand(idle.leftHand, walk.leftHand, blend),
    rightHand: lerpHand(idle.rightHand, walk.rightHand, blend),
  };
}
