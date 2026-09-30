# PROVENANCE.md

This file documents the origin, license, and intended handling of the CC0
body-mesh and morph-target data being adopted as this project's canonical
`head`/`torso` topology, replacing the earlier placeholder test asset.

## 1. Upstream source — confirmed

- **Project:** MakeHuman (MakeHuman Community)
- **Official code repository:** https://github.com/makehumancommunity/makehuman
- **Official project site:** https://static.makehumancommunity.org / http://www.makehumancommunity.org
- **Most recent stable release as of this writing (September 2026):** tag
  `v1.3.0`, commit `1f508f6` — listed as the latest release on
  https://github.com/makehumancommunity/makehuman/releases, described there
  as adding a body-shapes library and bug fixes.
- The same GitHub organization also maintains a repository named
  `makehumancommunity/makehuman-assets`, listed on
  https://github.com/orgs/makehumancommunity/repositories. This may be a
  more appropriate direct source for the mesh/target data than the main
  code repo's own `data/` directory. **This has not yet been resolved —
  see Open Items below.**

## 2. License — confirmed

MakeHuman deliberately separates its code license from its asset license:

- **Source code** (Python application logic, scripts, shaders) is
  **AGPL-3.0-or-later**.
- **Assets** — explicitly defined by the project to include the base mesh
  and proxies, targets and modifiers (morph data), textures, clothes, and
  poses/expressions — are released separately under **CC0 1.0 Universal**.

This is stated directly in the upstream repository's own license files:
- https://github.com/makehumancommunity/makehuman/blob/master/LICENSE.md
- https://github.com/makehumancommunity/makehuman/blob/master/LICENSE.ASSETS.md

and is echoed on the community's plain-language explanation page:
- http://www.makehumancommunity.org/content/license_explanation.html

Canonical CC0 1.0 Universal legal text (verbatim, upstream):
- https://creativecommons.org/publicdomain/zero/1.0/legalcode

## 3. No AGPL code is used — statement

No source code from the MakeHuman project (Python application logic,
build scripts, or shaders — the AGPL-licensed portion) is copied,
translated, vendored, or otherwise derived from anywhere in this
repository. Only the separately CC0-licensed data (base mesh geometry and
`.target` morph files) is consumed, and only in parsed/re-encoded numeric
form — see Section 5.

## 4. Intended local layout

- `assets/upstream/` — raw fetched upstream data, kept unmodified for
  auditability against the source named in Section 1. **Partially
  populated as of CC0-Phase 3**: `assets/upstream/base.obj` (the full
  19,158-vertex base mesh, fetched per the updated Section 6 item 1
  below) is checked in. Individual `.target` morph files are fetched
  on demand per morph, not bulk-vendored — see
  `tests/fixtures/cc0_phase3_real_morphs/` for the two currently
  checked in as real test fixtures (not the full 1,280-file corpus).
- `assets/packs/` — this project's own converted/packed output, in this
  project's sparse delta format (see Section 5), produced from
  `assets/upstream/` and consumed by the runtime pipeline. Not yet
  created — packs are currently built on demand by `pack_builder`/
  `morph_converter` from whatever asset directory is pointed at them,
  not pre-baked and checked into the repo.

## 5. What "converted" means here

"Converted" means the raw upstream base mesh and `.target` morph files
are parsed from their original MakeHuman file formats and re-encoded into
this project's own sparse delta format — a per-vertex offset representation
sized to only the vertices each morph actually displaces, rather than a
dense per-vertex array. The upstream files themselves are never shipped or
read at runtime in their original format; they exist only as the
`assets/upstream/` input to an offline conversion step whose output
(`assets/packs/`) is what the pipeline actually loads.

## 6. Open items — require human maintainer confirmation before this is legally final

1. **Exact pinned version — fetched, for real, as of CC0-Phase 3.**
   `assets/upstream/base.obj` in this repo is the literal, unmodified
   content of `makehuman/data/3dobjs/base.obj` at commit
   `a8bc2d54ff0ac92e78ff71431b1023eda42bf482` of
   `github.com/makehumancommunity/makehuman` — the same commit Phase 1's
   own merge notes already pinned for the derived `9001_head.obj`/
   `9002_torso.obj` fixtures, so this continues that decision rather than
   making a new one. Fetched via `git show <commit>:<path>` (not a single
   raw-HTTP GET — a `raw.githubusercontent.com` fetch of this exact path
   404'd first; `git show` against a real clone is the second method that
   actually worked, consistent with this project's standing "don't trust
   one failed method" discipline). Verified: 19,158 `v` lines (matches
   the vertex count Phase 1 already reported), file's own header confirms
   the September 2020 CC0 release. This is now a checked-in fact, not a
   candidate.

   One correction while fetching it: the real path is nested one level
   deeper than this file previously stated — `makehuman/data/3dobjs/base.obj`
   inside the repo, not `data/3dobjs/base.obj` at the repo root. Confirmed
   directly against the repo's own tree listing at the pinned commit.
2. **Source repository — resolved, by continuing Phase 1's choice.** Used
   the same repo Phase 1 already used (`makehumancommunity/makehuman`'s
   own `data/` directory, via `git clone`/`git show` against the pinned
   commit above), not the separate `makehuman-assets` repo or the
   static-site asset packs. Noting this as continuing an existing
   decision, not an independent legal determination — a maintainer should
   still sign off on this before treating it as final, same as before.
3. Item 3 (numbering kept for continuity): everything else in this
   document (project identity, the AGPL/CC0 license split, and the CC0
   legal text link) has been checked directly against the upstream
   project's own repository and pages, both originally and again as part
   of this update.
