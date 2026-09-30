import assert from 'node:assert/strict';
import {
  footTarget,
  walkPose,
  idlePose,
  locomotionPose,
  stridePeriod,
} from './locomotion.js';

let failures = 0;

function check(name, fn) {
  try {
    fn();
    console.log(`PASS: ${name}`);
  } catch (err) {
    failures += 1;
    console.log(`FAIL: ${name}`);
    console.log(`  ${err.message}`);
  }
}

function isFiniteNumber(v) {
  return typeof v === 'number' && Number.isFinite(v);
}

function assertPoseFinite(pose, label) {
  const scalars = ['hipBob', 'hipSwayYaw', 'hipSwayRoll', 'spineCounterYaw'];
  for (const key of scalars) {
    assert.ok(isFiniteNumber(pose[key]), `${label}.${key} not finite: ${pose[key]}`);
  }
  const vecFields = ['leftFoot', 'rightFoot', 'leftHand', 'rightHand'];
  for (const key of vecFields) {
    const v = pose[key];
    assert.ok(isFiniteNumber(v.x), `${label}.${key}.x not finite: ${v.x}`);
    assert.ok(isFiniteNumber(v.y), `${label}.${key}.y not finite: ${v.y}`);
    assert.ok(isFiniteNumber(v.z), `${label}.${key}.z not finite: ${v.z}`);
  }
}

const LEG_LENGTH = 0.9;
const HIP_WIDTH = 0.15;
const SPEEDS = [0, 0.001, 0.05, 0.3, 0.5, 1.2, 2.5];

// ---------------------------------------------------------------------
// 1. No NaN sweep
// ---------------------------------------------------------------------
check('1. no NaN across speed/time sweep', () => {
  for (const speed of SPEEDS) {
    for (let i = 0; i < 50; i += 1) {
      const t = (i / 49) * 4;
      assertPoseFinite(locomotionPose(t, speed, LEG_LENGTH, HIP_WIDTH, 7), `locomotionPose(t=${t},speed=${speed})`);
      assertPoseFinite(walkPose(t, speed, LEG_LENGTH, HIP_WIDTH), `walkPose(t=${t},speed=${speed})`);
      assertPoseFinite(idlePose(t, 7), `idlePose(t=${t})`);
    }
  }
});

// ---------------------------------------------------------------------
// 2. Cycle continuity
// ---------------------------------------------------------------------
check('2. cycle continuity across a stride wrap', () => {
  const speed = 1.0;
  const period = stridePeriod(speed);
  const baseTimes = [0, 0.3, 1.7, 3.14];
  const EPS = 1e-4;
  const TOL = 0.01;

  function maxFieldDiff(a, b) {
    let maxDiff = 0;
    const scalars = ['hipBob', 'hipSwayYaw', 'hipSwayRoll', 'spineCounterYaw'];
    for (const key of scalars) {
      maxDiff = Math.max(maxDiff, Math.abs(a[key] - b[key]));
    }
    const vecFields = ['leftFoot', 'rightFoot', 'leftHand', 'rightHand'];
    for (const key of vecFields) {
      maxDiff = Math.max(
        maxDiff,
        Math.abs(a[key].x - b[key].x),
        Math.abs(a[key].y - b[key].y),
        Math.abs(a[key].z - b[key].z),
      );
    }
    return maxDiff;
  }

  for (const t0 of baseTimes) {
    const beforeWrap = walkPose(t0 + period - EPS, speed, LEG_LENGTH, HIP_WIDTH);
    const afterWrap = walkPose(t0 + period, speed, LEG_LENGTH, HIP_WIDTH);
    const oneEarlier = walkPose(t0, speed, LEG_LENGTH, HIP_WIDTH);
    // "just after the wrap" should match "one period earlier" closely.
    const diff = maxFieldDiff(afterWrap, oneEarlier);
    assert.ok(diff <= TOL, `pop at wrap (t0=${t0}): max field diff ${diff} > ${TOL}`);
    // and "just before" / "just after" the wrap should themselves be close
    // (no instantaneous jump right at the seam).
    const seamDiff = maxFieldDiff(beforeWrap, afterWrap);
    assert.ok(seamDiff <= TOL, `discontinuous seam (t0=${t0}): max field diff ${seamDiff} > ${TOL}`);
  }
});

// ---------------------------------------------------------------------
// 3. Single stance per leg per cycle
// ---------------------------------------------------------------------
check('3. exactly one contiguous stance run per leg per cycle', () => {
  const speed = 1.0;
  const period = stridePeriod(speed);
  const N = 500;
  const groundedSeq = [];
  for (let i = 0; i < N; i += 1) {
    const t = (i / N) * period;
    const pose = walkPose(t, speed, LEG_LENGTH, HIP_WIDTH);
    groundedSeq.push(pose.leftFoot.grounded);
  }
  // Count contiguous true-runs, treating the sequence as circular since
  // it spans exactly one full period.
  let runs = 0;
  for (let i = 0; i < N; i += 1) {
    const prev = groundedSeq[(i - 1 + N) % N];
    const cur = groundedSeq[i];
    if (cur && !prev) runs += 1;
  }
  const anyGrounded = groundedSeq.some(Boolean);
  assert.ok(anyGrounded, 'leg is never grounded across the cycle');
  assert.equal(runs, 1, `expected exactly one contiguous stance run, found ${runs}`);
});

// ---------------------------------------------------------------------
// 4. Idle has no footsteps
// ---------------------------------------------------------------------
check('4. idlePose keeps both feet grounded with y ~ 0', () => {
  for (let i = 0; i < 50; i += 1) {
    const t = (i / 49) * 10;
    const pose = idlePose(t, 3);
    assert.equal(pose.leftFoot.grounded, true, `leftFoot not grounded at t=${t}`);
    assert.equal(pose.rightFoot.grounded, true, `rightFoot not grounded at t=${t}`);
    assert.ok(Math.abs(pose.leftFoot.y) < 1e-6, `leftFoot.y not ~0 at t=${t}: ${pose.leftFoot.y}`);
    assert.ok(Math.abs(pose.rightFoot.y) < 1e-6, `rightFoot.y not ~0 at t=${t}: ${pose.rightFoot.y}`);
  }
});

// ---------------------------------------------------------------------
// 5. Blend endpoints
// ---------------------------------------------------------------------
check('5. blend endpoints match idle (0.05 m/s) and walk (0.3 m/s)', () => {
  // At 0.05 m/s the blend must be pure idle: feet grounded, y ~ 0.
  for (let i = 0; i < 20; i += 1) {
    const t = (i / 19) * 4;
    const pose = locomotionPose(t, 0.05, LEG_LENGTH, HIP_WIDTH, 5);
    assert.equal(pose.leftFoot.grounded, true, `leftFoot not grounded at idle speed, t=${t}`);
    assert.equal(pose.rightFoot.grounded, true, `rightFoot not grounded at idle speed, t=${t}`);
    assert.ok(Math.abs(pose.leftFoot.y) < 1e-6, `leftFoot.y not ~0 at idle speed, t=${t}: ${pose.leftFoot.y}`);
    assert.ok(Math.abs(pose.rightFoot.y) < 1e-6, `rightFoot.y not ~0 at idle speed, t=${t}: ${pose.rightFoot.y}`);
  }

  // At 0.3 m/s it must be a real walk: the foot actually lifts.
  const period = stridePeriod(0.3);
  let maxY = 0;
  for (let i = 0; i < 200; i += 1) {
    const t = (i / 200) * period;
    const pose = locomotionPose(t, 0.3, LEG_LENGTH, HIP_WIDTH, 5);
    maxY = Math.max(maxY, pose.leftFoot.y, pose.rightFoot.y);
  }
  const LIFT_THRESHOLD = 0.03; // metres — well below our ~0.09m step height at this speed
  assert.ok(maxY > LIFT_THRESHOLD, `max foot y at walk speed only ${maxY}, expected > ${LIFT_THRESHOLD}`);
});

// ---------------------------------------------------------------------
// 6. legLength scaling sanity
// ---------------------------------------------------------------------
check('6. footTarget scales its output with legLength', () => {
  const strideLength = 5; // deliberately oversized to force clamping
  const stepHeight = 5; // deliberately oversized to force clamping
  for (const legLength of [0.7, 1.1]) {
    let maxAbsZ = 0;
    let maxY = 0;
    const N = 200;
    for (let i = 0; i < N; i += 1) {
      const phase = i / N;
      const foot = footTarget(phase, strideLength, stepHeight, legLength);
      maxAbsZ = Math.max(maxAbsZ, Math.abs(foot.z));
      maxY = Math.max(maxY, foot.y);
    }
    assert.ok(maxAbsZ < legLength, `legLength=${legLength}: max|z| ${maxAbsZ} not < legLength`);
    assert.ok(maxY < legLength, `legLength=${legLength}: max y ${maxY} not < legLength`);
    // Also confirm it isn't just clamping to some fixed constant regardless
    // of legLength — the short and tall bodies should differ.
  }
  const shortFoot = footTarget(0.8, strideLength, stepHeight, 0.7);
  const tallFoot = footTarget(0.8, strideLength, stepHeight, 1.1);
  assert.notEqual(shortFoot.y, tallFoot.y, 'stepHeight clamp did not scale with legLength');
});

if (failures > 0) {
  console.log(`\n${failures} test(s) failed.`);
  process.exit(1);
} else {
  console.log('\nAll tests passed.');
  process.exit(0);
}
