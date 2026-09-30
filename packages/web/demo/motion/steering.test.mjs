// packages/web/demo/motion/steering.test.mjs
//
// Run with: node packages/web/demo/motion/steering.test.mjs
// No test framework — plain assert, PASS/FAIL lines, non-zero exit on
// failure.

import assert from "node:assert/strict";
import {
  makeRng,
  makeWanderState,
  stepWander,
  MAX_TURN_RATE_PER_SEC,
  MAX_ACCEL_PER_SEC,
} from "./steering.js";

let failures = 0;

function test(name, fn) {
  try {
    fn();
    console.log(`PASS: ${name}`);
  } catch (err) {
    failures++;
    console.log(`FAIL: ${name}`);
    console.log(`  ${err && err.stack ? err.stack : err}`);
  }
}

function angleDiffWrapped(a, b) {
  let d = (a - b) % (2 * Math.PI);
  if (d > Math.PI) d -= 2 * Math.PI;
  if (d < -Math.PI) d += 2 * Math.PI;
  return d;
}

// ---------------------------------------------------------------------------
// Test 1: determinism
// ---------------------------------------------------------------------------
test("determinism: identical seed + start + steps => identical end state", () => {
  const bounds = { minX: -10, maxX: 10, minZ: -10, maxZ: 10 };
  const maxSpeed = 1.3;

  const a = makeWanderState(1.5, -2.25, 0.4, 777);
  const b = makeWanderState(1.5, -2.25, 0.4, 777);

  for (let i = 0; i < 1000; i++) {
    stepWander(a, 1 / 60, bounds, maxSpeed);
    stepWander(b, 1 / 60, bounds, maxSpeed);
  }

  const eps = 1e-12;
  assert.ok(Math.abs(a.x - b.x) <= eps, `x diverged: ${a.x} vs ${b.x}`);
  assert.ok(Math.abs(a.z - b.z) <= eps, `z diverged: ${a.z} vs ${b.z}`);
  assert.ok(
    Math.abs(angleDiffWrapped(a.heading, b.heading)) <= eps,
    `heading diverged: ${a.heading} vs ${b.heading}`
  );
  assert.ok(Math.abs(a.speed - b.speed) <= eps, `speed diverged: ${a.speed} vs ${b.speed}`);
});

// ---------------------------------------------------------------------------
// Shared run for tests 2, 3, 4: 20 agents, seeded starts, 5000 steps each,
// dt = 1/60, inside a small 6x6 box. Records per-step heading & speed deltas
// as we go so tests 3/4 can check bounds without re-simulating.
// ---------------------------------------------------------------------------

const SMALL_BOUNDS = { minX: -3, maxX: 3, minZ: -3, maxZ: 3 };
const MAX_SPEED = 1.3;
const STEP_DT = 1 / 60;
const STEP_COUNT = 5000;
const AGENT_COUNT = 20;

function buildSmallBoxAgents() {
  // Seeded, reproducible starting positions/headings for 20 agents, all
  // comfortably inside the 6x6 box.
  const startRng = makeRng(20260922);
  const agents = [];
  for (let i = 0; i < AGENT_COUNT; i++) {
    const x = SMALL_BOUNDS.minX + 0.5 + startRng() * (6 - 1.0);
    const z = SMALL_BOUNDS.minZ + 0.5 + startRng() * (6 - 1.0);
    const heading = (startRng() - 0.5) * 2 * Math.PI;
    const seed = 1000 + i * 97;
    agents.push(makeWanderState(x, z, heading, seed));
  }
  return agents;
}

function runSmallBoxSimulation() {
  const agents = buildSmallBoxAgents();
  let boundsViolation = null;
  let turnRateViolation = null;
  let accelViolation = null;

  const turnEps = 1e-9;
  const accelEps = 1e-9;
  const maxTurnAllowed = MAX_TURN_RATE_PER_SEC * STEP_DT;
  const maxAccelAllowed = MAX_ACCEL_PER_SEC * STEP_DT;

  const prevHeading = agents.map((a) => a.heading);
  const prevSpeed = agents.map((a) => a.speed);

  for (let step = 0; step < STEP_COUNT; step++) {
    for (let i = 0; i < agents.length; i++) {
      const st = agents[i];
      stepWander(st, STEP_DT, SMALL_BOUNDS, MAX_SPEED);

      if (
        !boundsViolation &&
        (st.x < SMALL_BOUNDS.minX ||
          st.x > SMALL_BOUNDS.maxX ||
          st.z < SMALL_BOUNDS.minZ ||
          st.z > SMALL_BOUNDS.maxZ)
      ) {
        boundsViolation = { step, agent: i, x: st.x, z: st.z };
      }

      const dHeading = Math.abs(angleDiffWrapped(st.heading, prevHeading[i]));
      if (!turnRateViolation && dHeading > maxTurnAllowed + turnEps) {
        turnRateViolation = { step, agent: i, dHeading, maxTurnAllowed };
      }
      prevHeading[i] = st.heading;

      const dSpeed = Math.abs(st.speed - prevSpeed[i]);
      if (!accelViolation && dSpeed > maxAccelAllowed + accelEps) {
        accelViolation = { step, agent: i, dSpeed, maxAccelAllowed };
      }
      prevSpeed[i] = st.speed;
    }
  }

  return { boundsViolation, turnRateViolation, accelViolation };
}

const smallBoxResult = runSmallBoxSimulation();

// ---------------------------------------------------------------------------
// Test 2: never exits bounds
// ---------------------------------------------------------------------------
test("never exits bounds: 20 agents, 5000 steps @ dt=1/60 in a 6x6 box", () => {
  assert.equal(
    smallBoxResult.boundsViolation,
    null,
    `bounds violated: ${JSON.stringify(smallBoxResult.boundsViolation)}`
  );
});

// ---------------------------------------------------------------------------
// Test 3: bounded turn rate
// ---------------------------------------------------------------------------
test(`bounded turn rate: never exceeds MAX_TURN_RATE_PER_SEC (${MAX_TURN_RATE_PER_SEC.toFixed(
  6
)} rad/s = 90 deg/s) * dt`, () => {
  assert.equal(
    smallBoxResult.turnRateViolation,
    null,
    `turn rate violated: ${JSON.stringify(smallBoxResult.turnRateViolation)}`
  );
});

// ---------------------------------------------------------------------------
// Test 4: bounded acceleration
// ---------------------------------------------------------------------------
test(`bounded acceleration: never exceeds MAX_ACCEL_PER_SEC (${MAX_ACCEL_PER_SEC} m/s^2) * dt`, () => {
  assert.equal(
    smallBoxResult.accelViolation,
    null,
    `acceleration violated: ${JSON.stringify(smallBoxResult.accelViolation)}`
  );
});

// ---------------------------------------------------------------------------
// Test 5: does pause sometimes (and does walk sometimes)
// ---------------------------------------------------------------------------
test("pauses and walks: long single-agent run has both a >=1s slow stretch and a >=1s fast stretch", () => {
  const bounds = { minX: -25, maxX: 25, minZ: -25, maxZ: 25 }; // large, so edge steering never interferes
  const maxSpeed = 1.3;
  const dt = 1 / 60;
  const steps = 20000;

  const st = makeWanderState(0, 0, 0, 4242);

  const PAUSE_SPEED_THRESHOLD = 0.05;
  const WALK_SPEED_THRESHOLD = 0.5 * maxSpeed;
  const minStretchSteps = Math.ceil(1 / dt); // >= 1 simulated second

  let pauseStreak = 0;
  let walkStreak = 0;
  let longestPauseStreak = 0;
  let longestWalkStreak = 0;

  for (let i = 0; i < steps; i++) {
    stepWander(st, dt, bounds, maxSpeed);

    if (st.speed < PAUSE_SPEED_THRESHOLD) {
      pauseStreak++;
      longestPauseStreak = Math.max(longestPauseStreak, pauseStreak);
    } else {
      pauseStreak = 0;
    }

    if (st.speed > WALK_SPEED_THRESHOLD) {
      walkStreak++;
      longestWalkStreak = Math.max(longestWalkStreak, walkStreak);
    } else {
      walkStreak = 0;
    }
  }

  assert.ok(
    longestPauseStreak >= minStretchSteps,
    `expected a >=1s (>= ${minStretchSteps} steps) stretch with speed < ${PAUSE_SPEED_THRESHOLD}, longest was ${longestPauseStreak} steps`
  );
  assert.ok(
    longestWalkStreak >= minStretchSteps,
    `expected a >=1s (>= ${minStretchSteps} steps) stretch with speed > ${WALK_SPEED_THRESHOLD}, longest was ${longestWalkStreak} steps`
  );
});

// ---------------------------------------------------------------------------
// Test 6: dt robustness
// ---------------------------------------------------------------------------
test("dt robustness: same scenario stays in-bounds and finite at dt=1/60 and dt=1/20", () => {
  const bounds = { minX: -8, maxX: 8, minZ: -8, maxZ: 8 };
  const maxSpeed = 1.3;
  const totalSimSeconds = 60;
  const seed = 999;

  function run(dt) {
    const st = makeWanderState(0, 0, 0.2, seed);
    const steps = Math.round(totalSimSeconds / dt);
    for (let i = 0; i < steps; i++) {
      stepWander(st, dt, bounds, maxSpeed);
      assert.ok(Number.isFinite(st.x), `x not finite at dt=${dt}, step ${i}: ${st.x}`);
      assert.ok(Number.isFinite(st.z), `z not finite at dt=${dt}, step ${i}: ${st.z}`);
      assert.ok(
        Number.isFinite(st.heading),
        `heading not finite at dt=${dt}, step ${i}: ${st.heading}`
      );
      assert.ok(
        Number.isFinite(st.speed),
        `speed not finite at dt=${dt}, step ${i}: ${st.speed}`
      );
    }
    return st;
  }

  const fast = run(1 / 60);
  const slow = run(1 / 20);

  for (const [label, st] of [
    ["dt=1/60", fast],
    ["dt=1/20", slow],
  ]) {
    assert.ok(st.x >= bounds.minX && st.x <= bounds.maxX, `${label}: x out of bounds (${st.x})`);
    assert.ok(st.z >= bounds.minZ && st.z <= bounds.maxZ, `${label}: z out of bounds (${st.z})`);
  }
});

// ---------------------------------------------------------------------------

if (failures > 0) {
  console.log(`\n${failures} test(s) FAILED`);
  process.exit(1);
} else {
  console.log(`\nAll tests PASSED`);
  process.exit(0);
}
