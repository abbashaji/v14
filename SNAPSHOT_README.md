# Snapshot — v13 (CC0-Phase 13) + animation branch merged

On top of everything in CC0-Phase 13 (body diversity, bind-pose fix —
see below), this snapshot merges in the animation branch
(`github.com/abbashaji/animation`, tested working in-browser by the
project owner): `crowd.html`, `crowd_dance.html`, `motion_crowd.html`,
`dance/`, `motion/`, `tools/`, `rust-core/dance_clips/`.

(Note: the skin/texture phase, CC0-Phase 14 and its UV sub-fixes, was
cancelled after CC0-Phase 13 and is not part of this or any later
snapshot — v13 is the confirmed base going forward.)

## What was verified before merging (real components, not assumed)
- **Engine swap is safe**: the animation branch shipped its own older
  (pre-Phase-13) wasm+JS. Confirmed directly: `getSkeleton()` bit-
  identical (163/163 joints) and `generate()` bit-identical across 41
  varied DNAs (positions/normals/UVs/bone data/indices) between the old
  and v13's engine — the animation code runs unchanged on v13's engine.
  v13's real `dist/` is used throughout; the animation branch's own old
  `dist/` was not carried over.
- **Every file reference in the 3 new HTML demos resolves** in this
  merged tree (25 relative references checked programmatically, all
  present — including `rust-core/dance_clips/` and `demo/motion/`,
  which needed copying in separately, not just the demo HTML files
  themselves).
- **A real, quantified conflict was found and fixed**: Phase 13's
  `restScale`/`restPivot` and the animation branch's own
  `skeleton_fit.js` independently solve the same bind-pose/morph
  mismatch. Stacking both is measurably *worse* than either alone (baby
  body, 60° elbow bend, displacement as % of height: Phase 13 alone
  7.6%, `skeleton_fit` alone 6.8%, **both stacked 28.6%**). Fixed by
  forcing `restScale:[1,1,1], restPivot:[0,0,0]` at the two call sites
  that use `skeleton_fit`'s fitted skeleton (`crowd_dance.html`,
  `motion_crowd.html`) — confirmed present in both files. `diversity.html`
  and `crowd.html` don't use `skeleton_fit` and are untouched — Phase
  13's correction still applies there as before.

## How to run
`start_demo.bat` (Windows, from the animation branch) or the existing
`python3 -m http.server` pattern — see `RUN_LOCALLY.md`.

## Still open
- No pack-picker UI.
- Skin/texture phase — cancelled.
- Clothing not re-fit to the CC0 body.
- "Live mix-and-match" of multiple packs — deferred.

---

# CC0-Phase 13 — prior state (bind-pose fix, real wasm, real tests)

This is the project with the bind-pose/morph mismatch fix (CC0-Phase
13, Option 2) fully closed — not just source-verified, but built and
tested end-to-end against a real, matching `.wasm` binary.

## What's in this snapshot
- `rust-core/src/lib.rs` + `bind_pose_fit.rs` — the real fix: each
  character's skeleton bind pose is now scaled to match its own morph,
  instead of every character sharing one fixed adult rig. Verified:
  60° elbow-bend displacement on a baby-corner character dropped from
  123.6% of body height to 4.6% (26.85x reduction), adult characters
  unaffected.
- `packages/web/dist/anthroforge_core.wasm` — **real, rebuilt on a
  full toolchain** (by the project owner, locally — no sandbox in this
  whole project could produce a wasm32 build).
- `packages/web/dist/index.js`/`index.d.ts` — rebuilt from the current
  TS source to match this wasm's actual ABI (the 40-byte
  `MeshOutputBuffer`, up from 24 — shipping the new wasm with old JS
  would have silently misread memory; caught and fixed before
  packaging this).
- `packages/web/demo/vendor/three/build/three.core.js` — was missing
  (a real, unrelated infra bug found independently), now present.

## Verified, for real, in this exact assembled state
- `packages/web`: **15/15** tests pass (`node --test`), against the
  real matching wasm.
- `packages/web-three`: **4/4** tests pass.
- Three real local browser screenshots (yours) confirm it visually:
  all 16 characters, including the small/baby-corner ones, now stand
  in normal, undistorted poses — the original hunched/bent artifact is
  gone.

## Still open
- No pack-picker UI (only `essentials.afpp` wired into the demo).
- No skin tone/texture system.
- Clothing not re-fit to the CC0 body.
- "Live mix-and-match" of multiple packs — deferred on purpose.

