// Runtime for the baked dance clips (rust-core/dance_clips/dance_clips.{json,bin}).
// Pure math, no DOM / three.js dependency, so it can be unit-tested in Node.
//
// A baked frame is: [pelvis offset x,y,z] + one local quaternion per driven joint
// (lib.joints, engine joint indices). Everything else keeps its bind rotation.
import { qmul, qinv, qrot } from "./skeleton_fit.js";

export async function loadDanceClips(baseUrl) {
  const [meta, buf] = await Promise.all([
    fetch(new URL("dance_clips.json", baseUrl)).then((r) => { if (!r.ok) throw new Error("dance_clips.json: " + r.status); return r.json(); }),
    fetch(new URL("dance_clips.bin", baseUrl)).then((r) => { if (!r.ok) throw new Error("dance_clips.bin: " + r.status); return r.arrayBuffer(); }),
  ]);
  if (meta.format !== 2) throw new Error("dance_clips.json is an old format; re-run tools/bake_dance.mjs");
  const all = new Float32Array(buf);
  const clips = meta.clips.map((c) => ({ ...c, data: all.subarray(c.offset, c.offset + c.frames * meta.floatsPerFrame) }));
  return { fps: meta.fps, joints: meta.joints, stride: meta.floatsPerFrame, clips, byName: Object.fromEntries(clips.map((c) => [c.name, c])) };
}

export class Pose {
  constructor(jointCount) { this.q = new Float32Array(jointCount * 4); this.p = new Float32Array(3); }
}

// Sample a clip at time t (seconds, clamped to the clip) into `out`.
export function samplePose(lib, clip, t, out) {
  const S = lib.stride, M = lib.joints.length;
  const last = clip.frames - 1;
  let f = Math.min(Math.max(0, t) * lib.fps, last);
  let i0 = Math.floor(f); if (i0 >= last) i0 = Math.max(0, last - 1);
  const a = last === 0 ? 0 : f - i0;
  const d = clip.data, o0 = i0 * S, o1 = Math.min(i0 + 1, last) * S;
  for (let k = 0; k < 3; k++) out.p[k] = d[o0 + k] + (d[o1 + k] - d[o0 + k]) * a;
  for (let j = 0; j < M; j++) {
    const b0 = o0 + 3 + j * 4, b1 = o1 + 3 + j * 4, r = j * 4;
    const dot = d[b0] * d[b1] + d[b0 + 1] * d[b1 + 1] + d[b0 + 2] * d[b1 + 2] + d[b0 + 3] * d[b1 + 3];
    const s = dot < 0 ? -1 : 1;
    const x = d[b0] + (s * d[b1] - d[b0]) * a, y = d[b0 + 1] + (s * d[b1 + 1] - d[b0 + 1]) * a;
    const z = d[b0 + 2] + (s * d[b1 + 2] - d[b0 + 2]) * a, w = d[b0 + 3] + (s * d[b1 + 3] - d[b0 + 3]) * a;
    const n = 1 / Math.hypot(x, y, z, w);
    out.q[r] = x * n; out.q[r + 1] = y * n; out.q[r + 2] = z * n; out.q[r + 3] = w * n;
  }
  return out;
}

// out = lerp(a, b, w) (normalised lerp, shortest arc). `out` may alias `a` or `b`.
export function blendPose(a, b, w, out) {
  const M = a.q.length / 4;
  for (let k = 0; k < 3; k++) out.p[k] = a.p[k] + (b.p[k] - a.p[k]) * w;
  for (let j = 0; j < M; j++) {
    const r = j * 4;
    const dot = a.q[r] * b.q[r] + a.q[r + 1] * b.q[r + 1] + a.q[r + 2] * b.q[r + 2] + a.q[r + 3] * b.q[r + 3];
    const s = dot < 0 ? -1 : 1;
    const x = a.q[r] + (s * b.q[r] - a.q[r]) * w, y = a.q[r + 1] + (s * b.q[r + 1] - a.q[r + 1]) * w;
    const z = a.q[r + 2] + (s * b.q[r + 2] - a.q[r + 2]) * w, ww = a.q[r + 3] + (s * b.q[r + 3] - a.q[r + 3]) * w;
    const n = 1 / Math.hypot(x, y, z, ww);
    out.q[r] = x * n; out.q[r + 1] = y * n; out.q[r + 2] = z * n; out.q[r + 3] = ww * n;
  }
  return out;
}

// Per-character root rig: where this body's pelvis is, and how big its legs are
// relative to the default body the clips were baked on.
//   fitter    = buildFitter(...) result (default skeleton bind data)
//   fitWorld  = fitter.fit(positions).worldPos  (this body's joint positions)
export function makeRootRig(fitter, fitWorld) {
  const D = fitter.bindWorldPos, C = fitWorld[1], P0 = fitWorld[0];
  const toe = (w) => Math.min(w[8][1], w[28][1]);
  const s = (C[1] - toe(fitWorld)) / (D[1][1] - toe(D));           // pelvis height above the toes, vs default
  return { C, rel: [P0[0] - C[0], P0[1] - C[1], P0[2] - C[2]], s, qb0: fitter.bindWorldQuat[0] };
}

// Write a pose onto a three.js bone array (bones[i] == engine skeleton joint i).
// The pelvis rotates about ITS OWN centre, and the baked pelvis offset is scaled to this body.
export function applyPose(lib, bones, pose, rig) {
  const J = lib.joints;
  for (let j = 0; j < J.length; j++) bones[J[j]].quaternion.set(pose.q[j * 4], pose.q[j * 4 + 1], pose.q[j * 4 + 2], pose.q[j * 4 + 3]);
  const dh = qmul([pose.q[0], pose.q[1], pose.q[2], pose.q[3]], qinv(rig.qb0));    // pelvis rotation relative to bind
  const r = qrot(dh, rig.rel);
  bones[0].position.set(rig.C[0] + rig.s * pose.p[0] + r[0], rig.C[1] + rig.s * pose.p[1] + r[1], rig.C[2] + rig.s * pose.p[2] + r[2]);
}

// ---------------------------------------------------------------------------
// Per-body grounding.
// The bake only knows the default body, so contact with the floor is approximate for
// other proportions. For each (body, clip) we skin a sparse sample of THIS body's own
// vertices through the whole clip once, and lift/lower the clip so its true lowest point
// touches the floor (same rule the bake uses: lowest point of the clip = floor).
// ---------------------------------------------------------------------------

// character: engine mesh; fitWorld: fitted joint positions; fitter: buildFitter() result.
export function buildSkinSamples(character, fitWorld, fitter, step = 30) {
  const P = character.positions, bi = character.boneIndices, bw = character.boneWeights, wq = fitter.bindWorldQuat;
  const verts = [];
  for (let v = 0; v < P.length / 3; v += step) {
    const infl = [];
    for (let k = 0; k < 4; k++) {
      const w = bw[v * 4 + k]; if (w <= 1e-4) continue;
      const j = bi[v * 4 + k];
      const o = qrot(qinv(wq[j]), [P[v * 3] - fitWorld[j][0], P[v * 3 + 1] - fitWorld[j][1], P[v * 3 + 2] - fitWorld[j][2]]);
      infl.push(j, w, o[0], o[1], o[2]);
    }
    verts.push(infl);
  }
  let bindMin = Infinity; for (let v = 1; v < P.length; v += 3) bindMin = Math.min(bindMin, P[v]);
  return { verts, bindMin };
}

// Extra vertical shift (engine units, applied via pose.p[1] / rig.s) for this clip on this body.
export function groundShift(lib, clip, skelJoints, rig, samples, scratch) {
  const N = skelJoints.length, wq = new Array(N), wp = new Array(N);
  if (!lib._driven) { lib._driven = new Int16Array(N).fill(-1); lib.joints.forEach((j, i) => (lib._driven[j] = i)); }
  const step = Math.max(1, Math.ceil(clip.frames / 90));
  let lowest = Infinity;
  for (let f = 0; f < clip.frames; f += step) {
    samplePose(lib, clip, f / lib.fps, scratch);
    const q = scratch.q, dh = qmul([q[0], q[1], q[2], q[3]], qinv(rig.qb0)), r = qrot(dh, rig.rel);
    const root = [rig.C[0] + rig.s * scratch.p[0] + r[0], rig.C[1] + rig.s * scratch.p[1] + r[1], rig.C[2] + rig.s * scratch.p[2] + r[2]];
    for (let j = 0; j < N; j++) {
      const d = lib._driven[j], jt = skelJoints[j];
      const lq = d >= 0 ? [q[d * 4], q[d * 4 + 1], q[d * 4 + 2], q[d * 4 + 3]] : jt.rotation;
      if (jt.parentIndex < 0) { wq[j] = lq; wp[j] = root; continue; }
      const pq = wq[jt.parentIndex], pp = wp[jt.parentIndex], t = qrot(pq, jt.translation);
      wq[j] = qmul(pq, lq); wp[j] = [pp[0] + t[0], pp[1] + t[1], pp[2] + t[2]];
    }
    for (const infl of samples.verts) {
      let y = 0;
      for (let k = 0; k < infl.length; k += 5) {
        const j = infl[k], w = infl[k + 1], o = qrot(wq[j], [infl[k + 2], infl[k + 3], infl[k + 4]]);
        y += w * (wp[j][1] + o[1]);
      }
      if (y < lowest) lowest = y;
    }
  }
  return samples.bindMin - lowest;
}
