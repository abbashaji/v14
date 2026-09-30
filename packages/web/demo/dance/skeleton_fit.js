// Fits the engine's single global skeleton to an individual generated body.
//
// Why: generate() morphs the MESH (height, weight, age, gender, ...) but the
// engine exposes only one fixed "master" skeleton. Without fitting, a knee
// joint sits at the default body's knee even when this body's knee is
// elsewhere, so any rotation swings the mesh around the wrong pivot.
//
// How: every body shares the same mesh topology. For each joint we record its
// nearest vertices on a REFERENCE body (defaults: no morphs, height = weight
// = 1) with inverse-distance weights. For a new body the joint is moved by the
// same weighted blend of how those vertices moved. Rotations are unchanged, so
// the baked dance data still applies; only bone translations differ.

export const qmul = (a, b) => [
  a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
  a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
  a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
  a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
];
export const qinv = (q) => [-q[0], -q[1], -q[2], q[3]];
export function qrot(q, v) {           // rotate vector v by unit quaternion q
  const [x, y, z, w] = q, [vx, vy, vz] = v;
  const tx = 2 * (y * vz - z * vy), ty = 2 * (z * vx - x * vz), tz = 2 * (x * vy - y * vx);
  return [vx + w * tx + (y * tz - z * ty), vy + w * ty + (z * tx - x * tz), vz + w * tz + (x * ty - y * tx)];
}

export function buildFitter(skeleton, refPositions, K = 24) {
  const N = skeleton.length, nV = refPositions.length / 3;
  const parent = skeleton.map((j) => j.parentIndex);
  // bind world rotation / position of the default skeleton
  const wq = [], wp = [];
  for (let i = 0; i < N; i++) {
    const j = skeleton[i];
    if (parent[i] < 0) { wq[i] = j.rotation.slice(); wp[i] = j.translation.slice(); continue; }
    wq[i] = qmul(wq[parent[i]], j.rotation);
    const r = qrot(wq[parent[i]], j.translation);
    wp[i] = [r[0] + wp[parent[i]][0], r[1] + wp[parent[i]][1], r[2] + wp[parent[i]][2]];
  }
  // K nearest reference vertices per joint, inverse-distance weights
  const nbr = [], wts = [];
  for (let j = 0; j < N; j++) {
    const [jx, jy, jz] = wp[j];
    const bi = new Int32Array(K).fill(-1), bd = new Float64Array(K).fill(Infinity);
    let worst = Infinity;
    for (let i = 0; i < nV; i++) {
      const dx = refPositions[i * 3] - jx, dy = refPositions[i * 3 + 1] - jy, dz = refPositions[i * 3 + 2] - jz;
      const d = dx * dx + dy * dy + dz * dz;
      if (d >= worst) continue;
      let k = K - 1;                                   // insertion into the sorted small list
      while (k > 0 && bd[k - 1] > d) { bd[k] = bd[k - 1]; bi[k] = bi[k - 1]; k--; }
      bd[k] = d; bi[k] = i; worst = bd[K - 1];
    }
    const w = new Float64Array(K); let s = 0;
    for (let k = 0; k < K; k++) { w[k] = 1 / (bd[k] + 0.05); s += w[k]; }
    for (let k = 0; k < K; k++) w[k] /= s;
    nbr.push(bi); wts.push(w);
  }
  return {
    bindWorldPos: wp, bindWorldQuat: wq,
    // positions: this body's vertex positions (Float32Array, same topology as refPositions)
    fit(positions) {
      if (positions.length !== refPositions.length) throw new Error("mesh topology differs from the reference body");
      const jp = new Array(N);
      for (let j = 0; j < N; j++) {
        let dx = 0, dy = 0, dz = 0; const bi = nbr[j], w = wts[j];
        for (let k = 0; k < K; k++) {
          const i = bi[k] * 3;
          dx += w[k] * (positions[i] - refPositions[i]); dy += w[k] * (positions[i + 1] - refPositions[i + 1]); dz += w[k] * (positions[i + 2] - refPositions[i + 2]);
        }
        jp[j] = [wp[j][0] + dx, wp[j][1] + dy, wp[j][2] + dz];
      }
      const joints = skeleton.map((j, i) => {
        let t;
        if (parent[i] < 0) t = jp[i];
        else { const p = parent[i]; t = qrot(qinv(wq[p]), [jp[i][0] - jp[p][0], jp[i][1] - jp[p][1], jp[i][2] - jp[p][2]]); }
        return { parentIndex: j.parentIndex, translation: t, rotation: j.rotation, scale: j.scale };
      });
      return { joints, worldPos: jp };
    },
  };
}
