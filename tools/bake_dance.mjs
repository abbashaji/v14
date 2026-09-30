// Retargets Mixamo FBX clips onto the AnthroForge skeleton and writes
// dance_clips.json + dance_clips.bin (baked local quaternions, 30 fps).
//
//   node tools/bake_dance.mjs <dir with .fbx files> <skeleton.json> <out dir>
//   (make skeleton.json with: node tools/dump_skeleton.mjs skeleton.json)
//
// What it does, per clip and per frame:
//   * samples the Mixamo skeleton with three's AnimationMixer;
//   * for every mapped bone, rotates the engine bone so that its DIRECTION
//     matches the Mixamo bone (this absorbs the engine's A-pose vs Mixamo's
//     T-pose), keeping the source's twist;
//   * removes horizontal root drift (dancers stay on their spot);
//   * stores the pelvis motion as a body-independent OFFSET; the page scales
//     it per character and pivots the body around that character's pelvis;
//   * shifts each clip vertically so its lowest foot/hand touches the floor.
import fs from "fs";
import * as THREE from "three";
import { FBXLoader } from "three/examples/jsm/loaders/FBXLoader.js";

const [SRC_ARG, SKEL_ARG, OUT_ARG] = process.argv.slice(2);
if (!SRC_ARG || !SKEL_ARG || !OUT_ARG) { console.error("usage: node bake_dance.mjs <fbx dir> <skeleton.json> <out dir>"); process.exit(1); }
const SRC = SRC_ARG.replace(/\/?$/, "/");
const OUT = OUT_ARG.replace(/\/?$/, "/");
fs.mkdirSync(OUT, { recursive: true });
const FPS = 30;

// ---------------- engine skeleton (bind pose) ----------------
const SK = JSON.parse(fs.readFileSync(SKEL_ARG));
const N = SK.length;
const parent = SK.map((j) => j.parentIndex);
const bindLQ = SK.map((j) => new THREE.Quaternion().fromArray(j.rotation));
const bindLP = SK.map((j) => new THREE.Vector3().fromArray(j.translation));
const bindWQ = [], bindWP = [];
for (let i = 0; i < N; i++) {
  if (parent[i] < 0) { bindWQ[i] = bindLQ[i].clone(); bindWP[i] = bindLP[i].clone(); }
  else {
    bindWQ[i] = bindWQ[parent[i]].clone().multiply(bindLQ[i]);
    bindWP[i] = bindLP[i].clone().applyQuaternion(bindWQ[parent[i]]).add(bindWP[parent[i]]);
  }
}
const GROUND_Y = -8.0;                                  // toe tips at bind
const PELVIS = bindWP[1].clone();                       // pelvis centre (children of root sit here)
const ENG_HIP_H = PELVIS.y - GROUND_Y;

// ---------------- Mixamo -> engine joint mapping ----------------
const MAP = [];
const add = (mix, mixChild, eng, engChild) => MAP.push({ mix, mixChild, eng, engChild });
add("Hips", null, 0, null);
add("Spine", "Spine1", 43, 44); add("Spine1", "Spine2", 44, 47); add("Spine2", "Neck", 47, 100);
add("Neck", "Head", 100, 103); add("Head", "HeadTop_End", 103, [141, 146]);
const FINGERS = { Thumb: [55, 56, 57], Index: [59, 60, 61], Middle: [63, 64, 65], Ring: [67, 68, 69], Pinky: [71, 72, 73] };
for (const [S, a, l] of [["Left", 0, 0], ["Right", 26, 20]]) {   // a = arm offset, l = leg offset
  add(S + "Shoulder", S + "Arm", 48 + a, 50 + a);
  add(S + "Arm", S + "ForeArm", 50 + a, 52 + a);
  add(S + "ForeArm", S + "Hand", 52 + a, 54 + a);
  add(S + "Hand", S + "HandMiddle1", 54 + a, 63 + a);
  add(S + "UpLeg", S + "Leg", 2 + l, 4 + l);
  add(S + "Leg", S + "Foot", 4 + l, 6 + l);
  add(S + "Foot", S + "ToeBase", 6 + l, 12 + l);
  for (const [fn, ix] of Object.entries(FINGERS))
    for (let k = 0; k < 3; k++)
      add(S + "Hand" + fn + (k + 1), S + "Hand" + fn + (k + 2), ix[k] + a, k < 2 ? ix[k + 1] + a : null);
}
const JOINTS = MAP.map((m) => m.eng);
const M = JOINTS.length;
const mappedSet = new Map(MAP.map((m, i) => [m.eng, i]));

// engine-side bone direction (bind world) for each mapping
function engDir(m) {
  let v;
  if (m.engChild == null) v = bindWP[m.eng].clone().sub(bindWP[parent[m.eng]]);
  else if (Array.isArray(m.engChild)) {
    const avg = new THREE.Vector3(); m.engChild.forEach((c) => avg.add(bindWP[c])); avg.multiplyScalar(1 / m.engChild.length);
    v = avg.sub(bindWP[m.eng]);
  } else v = bindWP[m.engChild].clone().sub(bindWP[m.eng]);
  return v.normalize();
}

// support points for grounding: [joint, margin below the joint]
const SUPPORT = [];
for (const o of [0, 20]) {
  for (const t of [7, 9, 12, 15, 18]) SUPPORT.push([t + o, 0]);
  for (const t of [8, 11, 14, 17, 20]) SUPPORT.push([t + o, 0]);
  SUPPORT.push([6 + o, 0.55], [4 + o, 0.6], [1 + o, 0.9]);
}
for (const o of [0, 26]) SUPPORT.push([54 + o, 0.4], [52 + o, 0.5], [57 + o, 0.15], [61 + o, 0.15], [65 + o, 0.15], [50 + o, 0.6]);
SUPPORT.push([0, 0.9], [103, 1.4], [141, 0.15], [146, 0.15], [44, 0.9]);

// ---------------- helpers ----------------
const loader = new FBXLoader();
const loadFbx = (f) => { const b = fs.readFileSync(SRC + f); return loader.parse(b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength), ""); };
const strip = (n) => n.replace(/^mixamorig:?/, "");
const q = new THREE.Quaternion(), qi = new THREE.Quaternion(), tmp = new THREE.Vector3();

function bakeClip(file) {
  const obj = loadFbx(file);
  const clip = obj.animations[0];
  const bones = {};
  obj.traverse((o) => { if (o.isBone) bones[strip(o.name)] = o; });
  obj.updateMatrixWorld(true);
  const srcBindWQ = {}, srcBindWP = {};
  for (const [n, b] of Object.entries(bones)) { srcBindWQ[n] = b.getWorldQuaternion(new THREE.Quaternion()); srcBindWP[n] = b.getWorldPosition(new THREE.Vector3()); }
  const hipsBindPos = srcBindWP.Hips.clone();
  const ratio = ENG_HIP_H / hipsBindPos.y;

  // constant part per mapping: K = Rc^-1 * Qtb
  const K = MAP.map((m) => {
    if (m.mix === "Hips") return bindWQ[m.eng].clone();
    const ds = srcBindWP[m.mixChild].clone().sub(srcBindWP[m.mix]).normalize();
    const dt = engDir(m);
    const rc = new THREE.Quaternion().setFromUnitVectors(ds, dt);
    return rc.invert().multiply(bindWQ[m.eng]);
  });

  const mixer = new THREE.AnimationMixer(obj);
  mixer.clipAction(clip).play();
  const nF = Math.max(2, Math.round(clip.duration * FPS) + 1);

  // pass 1: raw hips positions + world deltas
  const deltas = [], hipsPos = [];
  for (let k = 0; k < nF; k++) {
    mixer.setTime(Math.min(k / FPS, clip.duration));
    obj.updateMatrixWorld(true);
    const d = MAP.map((m) => bones[m.mix].getWorldQuaternion(new THREE.Quaternion()).multiply(srcBindWQ[m.mix].clone().invert()));
    deltas.push(d);
    hipsPos.push(bones.Hips.getWorldPosition(new THREE.Vector3()));
  }
  // in-place: remove linear horizontal drift from start to end
  const p0 = hipsPos[0], p1 = hipsPos[nF - 1];
  const rootOffsets = hipsPos.map((p, k) => {
    const f = k / (nF - 1);
    return new THREE.Vector3((p.x - (p0.x + (p1.x - p0.x) * f)) * ratio, (p.y - hipsBindPos.y) * ratio, (p.z - (p0.z + (p1.z - p0.z) * f)) * ratio);
  });

  // pass 2: poses (local quats), FK for grounding
  const frames = [], supMin = [];
  for (let k = 0; k < nF; k++) {
    const wq = new Array(N), wp = new Array(N), lq = new Array(N);
    const dh = deltas[k][0];
    const rootPos = PELVIS.clone().add(rootOffsets[k]).add(bindWP[0].clone().sub(PELVIS).applyQuaternion(dh));   // (only used for grounding FK)
    for (let i = 0; i < N; i++) {
      const mi = mappedSet.get(i);
      const target = mi !== undefined ? deltas[k][mi].clone().multiply(K[mi]) : null;
      if (parent[i] < 0) { wq[i] = target || bindLQ[i].clone(); lq[i] = wq[i].clone(); wp[i] = rootPos.clone(); continue; }
      const pq = wq[parent[i]];
      if (target) { wq[i] = target; lq[i] = pq.clone().invert().multiply(target); }
      else { lq[i] = bindLQ[i].clone(); wq[i] = pq.clone().multiply(lq[i]); }
      wp[i] = bindLP[i].clone().applyQuaternion(pq).add(wp[parent[i]]);
    }
    let mn = 1e9; for (const [j, mg] of SUPPORT) mn = Math.min(mn, wp[j].y - mg);
    supMin.push(mn);
    frames.push({ rootPos, lq, off: rootOffsets[k] });
  }
  const lift = GROUND_Y - Math.min(...supMin);
  const data = new Float32Array(nF * (3 + 4 * M));
  frames.forEach((f, k) => {
    let o = k * (3 + 4 * M);
    data[o++] = f.off.x; data[o++] = f.off.y + lift; data[o++] = f.off.z;      // pelvis offset from bind, default-skeleton units
    for (const j of JOINTS) { const c = f.lq[j]; data[o++] = c.x; data[o++] = c.y; data[o++] = c.z; data[o++] = c.w; }
  });
  return { data, nF, duration: clip.duration, lift, drift: [p1.x - p0.x, p1.z - p0.z].map((v) => +(v * ratio).toFixed(2)) };
}

// ---------------- run ----------------
// groove = loops; move = one-shot power move then hold; pose = held pose
const KIND = {
  "Flair": ["move", 0.5], "Breakdance Freeze Var 2": ["move", 2.0], "Crossleg Freeze": ["move", 2.0],
  "Northern Soul Spin": ["move", 0.6], "Bboy Uprock Start": ["move", 0.8],
  "Male Dance Pose": ["pose", 2.6], "Female Dance Pose": ["pose", 2.6],
};
const files = fs.readdirSync(SRC).filter((f) => f.endsWith(".fbx")).sort();
const meta = [], chunks = []; let offset = 0;
for (const f of files) {
  const r = bakeClip(f);
  const nm = f.replace(/\.fbx$/, ""), [kind, hold] = KIND[nm] ?? ["groove", 0];
  meta.push({ file: f, name: nm, kind, hold, frames: r.nF, duration: +r.duration.toFixed(4), offset, lift: +r.lift.toFixed(3) });
  chunks.push(Buffer.from(r.data.buffer)); offset += r.data.length;
  console.log(f.padEnd(30), "frames", String(r.nF).padStart(4), "dur", r.duration.toFixed(2), "lift", r.lift.toFixed(2), "drift(eng)", r.drift.join(","));
}
fs.writeFileSync(OUT + "dance_clips.bin", Buffer.concat(chunks));
fs.writeFileSync(OUT + "dance_clips.json", JSON.stringify({ format: 2, fps: FPS, joints: JOINTS, floatsPerFrame: 3 + 4 * M, groundY: GROUND_Y, clips: meta }));
console.log("joints mapped:", M, " total floats:", offset, " bytes:", offset * 4);
