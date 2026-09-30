import { qrot, qFromTo, solveTwoBoneIK } from './ik.js';

let failures = 0;

function vSub(a, b) {
  return [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
}
function vLen(a) {
  return Math.sqrt(a[0] * a[0] + a[1] * a[1] + a[2] * a[2]);
}
function vDot(a, b) {
  return a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
}
function vNorm(a) {
  const l = vLen(a);
  return [a[0] / l, a[1] / l, a[2] / l];
}
function isFiniteVec(a) {
  return a.every((c) => Number.isFinite(c));
}

function check(name, cond) {
  if (cond) {
    console.log(`PASS: ${name}`);
  } else {
    console.log(`FAIL: ${name}`);
    failures++;
  }
}

// Small seeded PRNG (mulberry32) so the random test is deterministic.
function mulberry32(seed) {
  let a = seed >>> 0;
  return function () {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function randomUnitVec(rng) {
  // Rejection sampling in a cube, normalized -> uniform-ish direction.
  while (true) {
    const v = [rng() * 2 - 1, rng() * 2 - 1, rng() * 2 - 1];
    const l = vLen(v);
    if (l > 1e-6 && l <= 1) {
      return [v[0] / l, v[1] / l, v[2] / l];
    }
  }
}

// ---- Test 1: in-reach target ----
{
  const root = [0, 0, 0];
  const dir = vNorm([0.3, 1, -0.4]);
  const dist = 1.6;
  const target = [dir[0] * dist, dir[1] * dist, dir[2] * dist];
  const pole = [0, 0, 1];
  const res = solveTwoBoneIK(root, 1, 1, target, pole);

  const endErr = vLen(vSub(res.endPos, target));
  check('1. in-reach endPos matches targetPos within 1e-6', endErr < 1e-6);
  check('1. in-reach reachFrac within 1e-6 of 1', Math.abs(res.reachFrac - 1) < 1e-6);
}

// ---- Test 2: fully extended (out of reach) ----
{
  const root = [0, 0, 0];
  const dir = vNorm([1, 0.5, 0.2]);
  const dist = 5;
  const target = [dir[0] * dist, dir[1] * dist, dir[2] * dist];
  const pole = [0, 1, 0];
  const res = solveTwoBoneIK(root, 1, 1, target, pole);

  const endDistFromRoot = vLen(vSub(res.endPos, root));
  const endDirUnit = vNorm(vSub(res.endPos, root));
  const targetDirUnit = vNorm(vSub(target, root));
  const dotDirs = vDot(endDirUnit, targetDirUnit);

  check('2. fully extended endPos distance == len1+len2', Math.abs(endDistFromRoot - 2) < 1e-6);
  check('2. fully extended endPos same direction as target', dotDirs > 0.999999);
  check('2. fully extended reachFrac < 1', res.reachFrac < 1);
}

// ---- Test 3: exact boundary ----
{
  const root = [0, 0, 0];
  const dir = vNorm([0.6, -0.3, 0.7]);
  const dist = 2; // == len1 + len2
  const target = [dir[0] * dist, dir[1] * dist, dir[2] * dist];
  const pole = [0, 0, 1];
  const res = solveTwoBoneIK(root, 1, 1, target, pole);

  const allVals = [
    ...res.midPos,
    ...res.endPos,
    res.reachFrac,
    ...res.rootToMidDir,
    ...res.midToEndDir,
  ];
  check('3. exact boundary produces no NaN', allVals.every((v) => Number.isFinite(v)));
}

// ---- Test 4: degenerate target at root ----
{
  const root = [1, 2, 3];
  const target = [1, 2, 3];
  const pole = [0, 0, 1];
  const len1 = 1, len2 = 1;
  const res = solveTwoBoneIK(root, len1, len2, target, pole);

  const allVals = [
    ...res.midPos,
    ...res.endPos,
    res.reachFrac,
    ...res.rootToMidDir,
    ...res.midToEndDir,
  ];
  check('4. degenerate target-at-root no NaN', allVals.every((v) => Number.isFinite(v)));

  const midDist = vLen(vSub(res.midPos, root));
  const endMidDist = vLen(vSub(res.endPos, res.midPos));
  check('4. |midPos-rootPos| within 1e-4 of len1', Math.abs(midDist - len1) < 1e-4);
  check('4. |endPos-midPos| within 1e-4 of len2', Math.abs(endMidDist - len2) < 1e-4);
}

// ---- Test 5: degenerate poleDir parallel to root->target ----
{
  const root = [0, 0, 0];
  const target = [0, 0, 2]; // along +Z
  const pole = [0, 0, 5]; // parallel to root->target direction
  const res = solveTwoBoneIK(root, 1, 1.2, target, pole);

  const allVals = [
    ...res.midPos,
    ...res.endPos,
    res.reachFrac,
    ...res.rootToMidDir,
    ...res.midToEndDir,
  ];
  check('5. poleDir parallel to root->target no NaN', allVals.every((v) => Number.isFinite(v)));
}

// ---- Test 6: symmetry / determinism ----
{
  const root = [0.5, -1, 2];
  const target = [1.2, 0.3, -0.7];
  const pole = [0, 1, 0];
  const res1 = solveTwoBoneIK(root, 1.1, 0.9, target, pole);
  const res2 = solveTwoBoneIK(root, 1.1, 0.9, target, pole);

  const same =
    JSON.stringify(res1.midPos) === JSON.stringify(res2.midPos) &&
    JSON.stringify(res1.endPos) === JSON.stringify(res2.endPos) &&
    res1.reachFrac === res2.reachFrac &&
    JSON.stringify(res1.rootToMidDir) === JSON.stringify(res2.rootToMidDir) &&
    JSON.stringify(res1.midToEndDir) === JSON.stringify(res2.midToEndDir);

  check('6. solving the same input twice gives identical output', same);
}

// ---- Test 7: qFromTo round-trip ----
{
  const rng = mulberry32(12345);
  let allOk = true;
  for (let i = 0; i < 20; i++) {
    let a, b;
    if (i === 0) {
      // Guaranteed antiparallel pair.
      a = randomUnitVec(rng);
      b = [-a[0], -a[1], -a[2]];
    } else {
      a = randomUnitVec(rng);
      b = randomUnitVec(rng);
    }
    const q = qFromTo(a, b);
    const rotated = qrot(q, a);
    const err = vLen(vSub(rotated, b));
    if (!(err < 1e-5) || !isFiniteVec(rotated)) {
      allOk = false;
      console.log(`  pair ${i}: a=${JSON.stringify(a)} b=${JSON.stringify(b)} err=${err}`);
    }
  }
  check('7. qFromTo round-trip for 20 random unit vector pairs (incl. antiparallel)', allOk);
}

if (failures > 0) {
  console.log(`\n${failures} test(s) FAILED`);
  process.exit(1);
} else {
  console.log('\nAll tests PASSED');
  process.exit(0);
}
