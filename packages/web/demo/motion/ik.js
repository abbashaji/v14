// Pure-geometry two-bone analytic IK solver + small quaternion helpers.
// No dependencies, no imports. Knows nothing about skeletons/legs/arms.

const EPS = 1e-9;

// ---- vec3 helpers (internal, not exported) ----

function vSub(a, b) {
  return [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
}

function vAdd(a, b) {
  return [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
}

function vScale(a, s) {
  return [a[0] * s, a[1] * s, a[2] * s];
}

function vDot(a, b) {
  return a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
}

function vCross(a, b) {
  return [
    a[1] * b[2] - a[2] * b[1],
    a[2] * b[0] - a[0] * b[2],
    a[0] * b[1] - a[1] * b[0],
  ];
}

function vLen(a) {
  return Math.sqrt(vDot(a, a));
}

// Normalizes `a`; if it is (near) zero-length, returns `fallback` (already
// assumed unit-length) instead of producing NaN.
function vNormalize(a, fallback) {
  const len = vLen(a);
  if (len < EPS) {
    return fallback ? fallback.slice() : [0, 0, 1];
  }
  return [a[0] / len, a[1] / len, a[2] / len];
}

function clamp(x, lo, hi) {
  return Math.max(lo, Math.min(hi, x));
}

// Returns some unit vector perpendicular to unit vector `a`, chosen
// deterministically (no reliance on randomness or external state).
function anyPerpendicular(a) {
  // Pick whichever of world +X / world +Y is least parallel to `a`.
  const candidate = Math.abs(a[0]) < 0.9 ? [1, 0, 0] : [0, 1, 0];
  const perp = vCross(a, candidate);
  return vNormalize(perp, [0, 1, 0]);
}

// ---- quaternion helpers (exported) ----

// Rotate vector v (length-3 array) by unit quaternion q ([x,y,z,w]).
export function qrot(q, v) {
  const qv = [q[0], q[1], q[2]];
  const w = q[3];
  const t = vScale(vCross(qv, v), 2);
  const cross2 = vCross(qv, t);
  return [
    v[0] + w * t[0] + cross2[0],
    v[1] + w * t[1] + cross2[1],
    v[2] + w * t[2] + cross2[2],
  ];
}

// Quaternion product, a*b, both [x,y,z,w], returns [x,y,z,w].
export function qmul(a, b) {
  const ax = a[0], ay = a[1], az = a[2], aw = a[3];
  const bx = b[0], by = b[1], bz = b[2], bw = b[3];
  return [
    aw * bx + ax * bw + ay * bz - az * by,
    aw * by - ax * bz + ay * bw + az * bx,
    aw * bz + ax * by - ay * bx + az * bw,
    aw * bw - ax * bx - ay * by - az * bz,
  ];
}

// Shortest-arc quaternion rotating unit vector `from` onto unit vector `to`.
// Handles the from == -to (180 degree) case without producing NaN.
export function qFromTo(from, to) {
  const f = vNormalize(from, [0, 0, 1]);
  const t = vNormalize(to, [0, 0, 1]);
  const dot = clamp(vDot(f, t), -1, 1);

  if (dot < -0.999999) {
    // 180 degree rotation: pick any axis perpendicular to `from`.
    const axis = anyPerpendicular(f);
    return [axis[0], axis[1], axis[2], 0];
  }

  const axis = vCross(f, t);
  const w = 1 + dot;
  const q = [axis[0], axis[1], axis[2], w];
  const qlen = Math.sqrt(q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]);
  if (qlen < EPS) {
    // f and t are (numerically) identical; identity rotation.
    return [0, 0, 0, 1];
  }
  return [q[0] / qlen, q[1] / qlen, q[2] / qlen, q[3] / qlen];
}

// Internal: quaternion for a rotation of `angle` radians about unit `axis`.
function qFromAxisAngle(axis, angle) {
  const half = angle * 0.5;
  const s = Math.sin(half);
  return [axis[0] * s, axis[1] * s, axis[2] * s, Math.cos(half)];
}

/**
 * Analytic two-bone IK (law of cosines), root fixed in place.
 * See PART_A_ik.md for the full contract.
 */
export function solveTwoBoneIK(rootPos, len1, len2, targetPos, poleDir) {
  // Degenerate lengths are clamped to >= 0; negative/zero bones degenerate
  // to zero-length segments rather than throwing.
  const l1 = Number.isFinite(len1) && len1 > 0 ? len1 : 0;
  const l2 = Number.isFinite(len2) && len2 > 0 ? len2 : 0;

  const toTarget = vSub(targetPos, rootPos);
  const d = vLen(toTarget);

  const poleN = vNormalize(poleDir, [0, 0, 1]);

  // Direction from root toward the target. If the target sits exactly on
  // the root (d ~ 0) there is no well-defined direction, so fall back to
  // poleDir to pick a stable fold-back direction.
  const dirToTarget = d < EPS ? poleN.slice() : vScale(toTarget, 1 / d);

  const maxReach = l1 + l2;
  const minReach = Math.abs(l1 - l2);

  // Distance used for the law-of-cosines triangle: clamped into the range
  // the two bones can actually span, so the triangle is always valid.
  const clampedD = maxReach < EPS ? 0 : clamp(d, minReach, maxReach);

  // reachFrac: 1 whenever the (unclamped) target is within reach, and the
  // fraction of the required distance the fully-extended chain achieves
  // when the target is farther than the chain can reach.
  let reachFrac;
  if (d <= maxReach + 1e-12) {
    reachFrac = 1;
  } else {
    reachFrac = maxReach > EPS ? clamp(maxReach / d, 1e-9, 1) : 1e-9;
  }

  // Interior angle at the root, between rootToMid and rootToTarget dirs.
  let theta1 = 0;
  if (l1 > EPS && clampedD > EPS) {
    const cosTheta1 =
      (l1 * l1 + clampedD * clampedD - l2 * l2) / (2 * l1 * clampedD);
    theta1 = Math.acos(clamp(cosTheta1, -1, 1));
  }

  // Bend-plane axis: perpendicular to both the root->target direction and
  // the pole direction. If they're parallel (or antiparallel), there's no
  // well-defined bend plane, so fall back to an arbitrary perpendicular axis.
  let axis = vCross(dirToTarget, poleN);
  const axisLen = vLen(axis);
  if (axisLen < 1e-7) {
    axis = anyPerpendicular(dirToTarget);
  } else {
    axis = vScale(axis, 1 / axisLen);
  }

  const bendQ = qFromAxisAngle(axis, theta1);
  const rootToMidDir = vNormalize(qrot(bendQ, dirToTarget), dirToTarget);

  const midPos = vAdd(rootPos, vScale(rootToMidDir, l1));
  const endPos = vAdd(rootPos, vScale(dirToTarget, clampedD));

  const midToEndDir = vNormalize(vSub(endPos, midPos), rootToMidDir);

  return {
    midPos,
    endPos,
    reachFrac,
    rootToMidDir,
    midToEndDir,
  };
}
