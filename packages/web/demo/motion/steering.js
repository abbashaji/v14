// packages/web/demo/motion/steering.js
//
// Skeleton-agnostic 2D crowd wander / steering. Pure math: takes/returns
// positions in metres and headings in radians. No imports, no DOM/browser
// APIs, no Math.random() — all randomness flows through a seeded PRNG so
// runs are reproducible.
//
// Heading convention (documented per spec, since it's our own choice):
//   heading = 0            -> facing +Z
//   heading increasing     -> rotates counter-clockwise viewed from above,
//                             i.e. the +Z ray sweeps toward -X as heading
//                             grows from 0.
//   direction vector: dx = -sin(heading), dz = cos(heading)
// This is internally consistent everywhere below; nothing outside this file
// needs to agree with it, since the module only exchanges plain x/z/heading
// numbers with callers.

// ---------------------------------------------------------------------------
// Tunable constants. Tests import these directly so the numbers can never
// drift out of sync between implementation and test assertions.
// ---------------------------------------------------------------------------

// Max heading turn rate: 90 degrees/second.
export const MAX_TURN_RATE_PER_SEC = Math.PI / 2;

// Max speed change rate (acceleration), applied both speeding up and
// slowing down.
export const MAX_ACCEL_PER_SEC = 1.0;

// Largest internal substep the integrator will ever take. Callers may pass
// any dt in [1/240, 1/15] (or beyond); stepWander splits dt into substeps
// no larger than this so turn-rate/accel clamping and edge steering stay
// stable regardless of the caller's frame rate.
const MAX_SUBSTEP_DT = 1 / 60;

// How far (in radians/second, scaled by dt) the "wander" desired heading is
// allowed to randomly drift. This does NOT bypass MAX_TURN_RATE_PER_SEC —
// it only changes the *target* the agent smoothly turns toward.
const WANDER_TURN_SPEED = 0.6;

// Pause/walk dwell time ranges (seconds). Randomized per agent, per episode,
// from that agent's own RNG stream, so many agents desync instead of
// pausing/walking on a shared clock.
const PAUSE_MIN_S = 2.5;
const PAUSE_MAX_S = 4.5;
const WALK_MIN_S = 2.5;
const WALK_MAX_S = 6.0;

// Edge-avoidance margin, as a fraction of the smaller bounds dimension,
// clamped to a sane absolute range. Inside this margin from a wall the
// agent's desired heading is smoothly blended toward the interior, and its
// target speed is reduced, well before it would ever reach the wall.
const EDGE_MARGIN_FRACTION = 0.3;
const EDGE_MARGIN_MIN = 0.6;
const EDGE_MARGIN_MAX = 2.5;

// Absolute last-resort safety clamp so position can never numerically land
// exactly on/outside a bound, even by floating point rounding. With the
// margin + steering above this should essentially never engage in practice;
// it exists purely as a hard guarantee, not as the primary steering
// mechanism (which is the smooth heading blend above).
const BOUNDARY_EPS = 1e-9;

// ---------------------------------------------------------------------------
// Seeded PRNG
// ---------------------------------------------------------------------------

/**
 * Deterministic seeded PRNG (mulberry32). Same seed -> same infinite
 * sequence, every time, in any JS environment (only relies on ordinary
 * 32-bit integer arithmetic, no Math.random()).
 * @param {number} seed unsigned 32-bit integer.
 * @returns {() => number} function returning uniform numbers in [0, 1).
 */
export function makeRng(seed) {
  let a = seed >>> 0;
  return function rng() {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// ---------------------------------------------------------------------------
// Angle helpers
// ---------------------------------------------------------------------------

/** Wrap an angle (radians) into (-pi, pi]. */
function normalizeAngle(a) {
  a = a % (2 * Math.PI);
  if (a > Math.PI) a -= 2 * Math.PI;
  if (a <= -Math.PI) a += 2 * Math.PI;
  return a;
}

/** Shortest signed angular difference a - b, wrapped into [-pi, pi]. */
function angleDiff(a, b) {
  return normalizeAngle(a - b);
}

function clamp(v, lo, hi) {
  return v < lo ? lo : v > hi ? hi : v;
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/**
 * Creates a fresh wander state for one agent.
 * @param {number} x starting position, metres.
 * @param {number} z starting position, metres.
 * @param {number} heading starting heading, radians (see convention above).
 * @param {number} seed integer, unique per agent; drives this agent's own
 *   independent RNG stream.
 * @returns {object} opaque-ish state object. Human-readable fields are kept
 *   for debuggability; callers should treat this as owned by stepWander.
 */
export function makeWanderState(x, z, heading, seed) {
  const rng = makeRng(seed >>> 0);

  // Prime the initial pause/walk mode and dwell timer from this agent's own
  // stream so agents don't all start in the same phase.
  const startsWalking = rng() < 0.5;
  const mode = startsWalking ? "walk" : "pause";
  const modeTimer = startsWalking
    ? WALK_MIN_S + rng() * (WALK_MAX_S - WALK_MIN_S)
    : PAUSE_MIN_S + rng() * (PAUSE_MAX_S - PAUSE_MIN_S);

  return {
    // Public / documented fields (see stepWander's @returns).
    x,
    z,
    heading: normalizeAngle(heading),
    speed: 0,

    // Internal, human-readable fields kept for debuggability.
    rng,
    seed: seed >>> 0,
    mode, // 'walk' | 'pause'
    modeTimer, // seconds remaining in current mode
    wanderHeading: normalizeAngle(heading), // slowly drifting desired heading
  };
}

// ---------------------------------------------------------------------------
// Core per-substep update (small, bounded dt only — see stepWander).
// ---------------------------------------------------------------------------

function subStep(state, dt, bounds, maxSpeed) {
  const rng = state.rng;

  // --- pause/walk dwell-time state machine, randomized per agent -----------
  state.modeTimer -= dt;
  if (state.modeTimer <= 0) {
    if (state.mode === "walk") {
      state.mode = "pause";
      state.modeTimer = PAUSE_MIN_S + rng() * (PAUSE_MAX_S - PAUSE_MIN_S);
    } else {
      state.mode = "walk";
      state.modeTimer = WALK_MIN_S + rng() * (WALK_MAX_S - WALK_MIN_S);
    }
  }

  // --- wander: desired heading slowly random-walks ------------------------
  state.wanderHeading = normalizeAngle(
    state.wanderHeading + (rng() - 0.5) * 2 * WANDER_TURN_SPEED * dt
  );
  let desiredHeading = state.wanderHeading;

  // --- edge avoidance: steer back well before reaching a wall -------------
  const width = bounds.maxX - bounds.minX;
  const depth = bounds.maxZ - bounds.minZ;
  const margin = clamp(
    EDGE_MARGIN_FRACTION * Math.min(width, depth),
    EDGE_MARGIN_MIN,
    Math.min(EDGE_MARGIN_MAX, 0.49 * Math.min(width, depth))
  );

  const dMinX = state.x - bounds.minX;
  const dMaxX = bounds.maxX - state.x;
  const dMinZ = state.z - bounds.minZ;
  const dMaxZ = bounds.maxZ - state.z;

  let pushX = 0;
  let pushZ = 0;
  let urgency = 0;
  if (dMinX < margin) {
    const u = 1 - dMinX / margin;
    pushX += u;
    urgency = Math.max(urgency, u);
  }
  if (dMaxX < margin) {
    const u = 1 - dMaxX / margin;
    pushX -= u;
    urgency = Math.max(urgency, u);
  }
  if (dMinZ < margin) {
    const u = 1 - dMinZ / margin;
    pushZ += u;
    urgency = Math.max(urgency, u);
  }
  if (dMaxZ < margin) {
    const u = 1 - dMaxZ / margin;
    pushZ -= u;
    urgency = Math.max(urgency, u);
  }

  if (urgency > 0) {
    // Convert the desired inward push (pushX, pushZ) into a heading, using
    // the same convention as everywhere else: dx = -sin(h), dz = cos(h)
    // => h = atan2(-dx, dz).
    const avoidHeading = Math.atan2(-pushX, pushZ);
    desiredHeading = normalizeAngle(
      desiredHeading + angleDiff(avoidHeading, desiredHeading) * urgency
    );
  }

  // --- turn heading toward desiredHeading, bounded rate --------------------
  const maxTurn = MAX_TURN_RATE_PER_SEC * dt;
  const turn = clamp(angleDiff(desiredHeading, state.heading), -maxTurn, maxTurn);
  state.heading = normalizeAngle(state.heading + turn);

  // --- speed: ramp toward target, bounded acceleration ---------------------
  // Near a wall, ease off the target speed too — gives more time to turn.
  let targetSpeed = state.mode === "walk" ? maxSpeed : 0;
  targetSpeed *= 1 - 0.7 * urgency;

  const maxDs = MAX_ACCEL_PER_SEC * dt;
  const dSpeed = clamp(targetSpeed - state.speed, -maxDs, maxDs);
  state.speed = Math.max(0, state.speed + dSpeed);

  // --- integrate position ---------------------------------------------------
  const dx = -Math.sin(state.heading);
  const dz = Math.cos(state.heading);
  let nx = state.x + dx * state.speed * dt;
  let nz = state.z + dz * state.speed * dt;

  // Hard failsafe clamp — see BOUNDARY_EPS comment above.
  nx = clamp(nx, bounds.minX + BOUNDARY_EPS, bounds.maxX - BOUNDARY_EPS);
  nz = clamp(nz, bounds.minZ + BOUNDARY_EPS, bounds.maxZ - BOUNDARY_EPS);

  state.x = nx;
  state.z = nz;
}

// ---------------------------------------------------------------------------
// Public step function
// ---------------------------------------------------------------------------

/**
 * Advances one agent's wander state by dt seconds, in place.
 * @param {object} state from makeWanderState (or a prior stepWander call).
 * @param {number} dt seconds since last call, > 0.
 * @param {{minX:number,maxX:number,minZ:number,maxZ:number}} bounds metres.
 * @param {number} maxSpeed metres/second.
 * @returns {object} the same state object, updated.
 */
export function stepWander(state, dt, bounds, maxSpeed) {
  if (!(dt > 0)) return state;

  // Substep so behavior stays stable regardless of caller frame rate
  // (spec requires correctness for dt in [1/240, 1/15], this handles a
  // wider range safely).
  const n = Math.max(1, Math.ceil(dt / MAX_SUBSTEP_DT));
  const subDt = dt / n;
  for (let i = 0; i < n; i++) {
    subStep(state, subDt, bounds, maxSpeed);
  }

  return state;
}
