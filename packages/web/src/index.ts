// AnthroForge/Web TypeScript SDK core.
//
// Public API surface per the AnthroForge/Web product spec §5, with one
// explicit, deliberate deviation — see the doc comment on
// `GeneratedCharacter` below and the "Deviation from the frozen spec"
// section in the handoff notes.

import { WasmBridge, type RawCharacterDNA } from "./wasm-bridge.js";

/** One morph target to blend in, by id (as stored in the Part Pack). */
export interface MorphWeight {
  id: number;
  /** 0 = no effect, 1 = full. Weights scale the morph's displacement linearly. */
  weight: number;
}

export interface CharacterDNA {
  seed: bigint;
  heightModifier: number;
  weightModifier: number;
  headId: number;
  torsoId: number;
  /** Required: every character has all four body parts (no "skip" sentinel). */
  armsId: number;
  /** Required: see `armsId`. */
  legsId: number;
  clothingIds: number[];
  /**
   * Optional morph targets to blend in (applied before the height/weight
   * body scale). An id with no morph loaded in the pack is skipped, not an
   * error. Omit or pass `[]` for none.
   */
  morphs?: MorphWeight[];
}

/**
 * Deviation from the frozen spec: the spec's §5 API declares a single flat
 * `vertices: Float32Array` field with no documented interleaving
 * convention. The wasm side produces an array of 56-byte interleaved
 * `SkinnedVertex` structs (position/normal/uv/bone_indices/bone_weights).
 *
 * `bone_indices` is two `u16`s that must stay intact integers — packing
 * them into a `Float32Array` slot would require reinterpreting their bits
 * as a float, which is lossy and would corrupt real index values above
 * 2^24 nowhere near the ceiling, but more importantly is simply the wrong
 * representation for an index. So this field set is de-interleaved into
 * separate typed arrays instead of keeping the single flat `vertices`
 * field. This is a bug fix to the frozen shape (it was going to silently
 * corrupt bone indices), not a redesign — flagged explicitly here and in
 * the handoff notes so the merge context knows the frozen contract was
 * intentionally changed, and why.
 */
export interface GeneratedCharacter {
  /** 3 floats per vertex: x, y, z. */
  positions: Float32Array;
  /** 3 floats per vertex: x, y, z. */
  normals: Float32Array;
  /** 2 floats per vertex: u, v. */
  uvs: Float32Array;
  /** 4 uint16s per vertex, kept as true integers (not float-reinterpreted). */
  boneIndices: Uint16Array;
  /** 4 floats per vertex. */
  boneWeights: Float32Array;
  indices: Uint32Array;
  /**
   * Not produced by `generate_character` today — that call only returns
   * mesh geometry (`MeshOutputBuffer`). Texture-atlas generation
   * (`generate_runtime_atlas`) is a separate, independently-callable wasm
   * export that this call path does not invoke. Always empty for now;
   * kept in the type because the spec's API surface is frozen and callers
   * should not have this field silently disappear later.
   */
  atlasBytes: Uint8Array;
  /** Always 0 for now — see `atlasBytes`. */
  atlasWidth: number;
  /** Always 0 for now — see `atlasBytes`. */
  atlasHeight: number;
  /**
   * CC0-Phase 13, Option 2: this character's own per-axis bind-pose
   * scale + pivot, fit against its own pre-/post-morph vertex positions.
   * `[1, 1, 1]` / `[0, 0, 0]` for a character with no active morphs.
   * Feed straight to `@anthroforge/web-three`'s `toSkinnedMesh` (or
   * apply the same `restPivot + restScale ⊙ (worldPos - restPivot)`
   * transform yourself) to correct the fixed global skeleton's bind pose
   * for this character's body proportions before posing it. See
   * `RawMesh.restScale`/`restPivot` in `wasm-bridge.ts` for the full
   * contract.
   */
  restScale: [number, number, number];
  restPivot: [number, number, number];
}

export interface InitOptions {
  partPackUrl: string;
  /** Accepted and ignored today; reserved for a future license check. */
  licenseKey?: string;
}

export interface SkeletonJoint {
  parentIndex: number;
  translation: [number, number, number];
  rotation: [number, number, number, number];
  scale: [number, number, number];
}

let bridge: WasmBridge | null = null;

/**
 * Loads the SDK's own bundled wasm module (shipped as a sibling file to
 * this package's built JS — not fetched from a caller-supplied URL) and
 * the caller's Part Pack, then instantiates the module.
 */
export async function init(options: InitOptions): Promise<void> {
  // `import.meta.url`-relative resolution works in both browsers and
  // modern Node, and keeps the wasm binary bundled with the package rather
  // than depending on an arbitrary host path.
  const wasmUrl = new URL("./anthroforge_core.wasm", import.meta.url);

  const [wasmResponse, packResponse] = await Promise.all([
    fetch(wasmUrl),
    fetch(options.partPackUrl),
  ]);

  if (!wasmResponse.ok) {
    throw new Error(
      `AnthroForge/Web: failed to fetch bundled wasm module (${wasmUrl}): ${wasmResponse.status} ${wasmResponse.statusText}`,
    );
  }
  if (!packResponse.ok) {
    throw new Error(
      `AnthroForge/Web: failed to fetch Part Pack (${options.partPackUrl}): ${packResponse.status} ${packResponse.statusText}`,
    );
  }

  const wasmBytes = await wasmResponse.arrayBuffer();
  const packBytes = new Uint8Array(await packResponse.arrayBuffer());

  const newBridge = await WasmBridge.instantiate(wasmBytes);
  const ok = newBridge.initPartRegistryFromPack(packBytes);
  if (!ok) {
    const err = newBridge.getLastError();
    throw new Error(
      `AnthroForge/Web: init_part_registry_from_pack failed${err ? `: ${err}` : ""}`,
    );
  }

  bridge = newBridge;
}

function requireBridge(): WasmBridge {
  if (!bridge) {
    throw new Error("AnthroForge/Web: init() must be called and awaited before generate().");
  }
  return bridge;
}

/**
 * Synchronous per the frozen spec: the underlying wasm calls are
 * synchronous once the module is instantiated, so no `Promise` is needed
 * here — only `init()` is async, because it has to fetch two files and
 * instantiate the module.
 */
export function generate(dna: CharacterDNA): GeneratedCharacter | null {
  const b = requireBridge();

  const rawDna: RawCharacterDNA = {
    seed: dna.seed,
    heightModifier: dna.heightModifier,
    weightModifier: dna.weightModifier,
    headId: dna.headId,
    torsoId: dna.torsoId,
    armsId: dna.armsId,
    legsId: dna.legsId,
    clothingIds: dna.clothingIds,
    morphs: dna.morphs ?? [],
  };

  const mesh = b.generateCharacter(rawDna);
  if (mesh === null) {
    return null;
  }

  return {
    positions: mesh.positions,
    normals: mesh.normals,
    uvs: mesh.uvs,
    boneIndices: mesh.boneIndices,
    boneWeights: mesh.boneWeights,
    indices: mesh.indices,
    // Not wired into generate_character's path yet — see the doc comment
    // on GeneratedCharacter.atlasBytes above. Deliberately empty, not
    // fabricated placeholder pixel data.
    atlasBytes: new Uint8Array(0),
    atlasWidth: 0,
    atlasHeight: 0,
    restScale: mesh.restScale,
    restPivot: mesh.restPivot,
  };
}

export function getLastError(): string | null {
  return requireBridge().getLastError();
}

/**
 * Returns the fully-assembled global skeleton (`master_skeleton.json`'s
 * bone hierarchy plus every bone's real bind-pose local transform,
 * contributed across every loaded part), or `null` on failure (call
 * `getLastError()` for details). This data is read-only, global, and
 * per-process — unlike `generate()`'s per-call output — so call this once
 * after `init()` succeeds, not once per generated character.
 */
export function getSkeleton(): SkeletonJoint[] | null {
  return requireBridge().getSkeleton();
}

/**
 * No-op: `generate()` already copies all mesh data out of wasm linear
 * memory into fresh JS typed arrays and immediately calls
 * `free_mesh_buffer` on the wasm-side buffer before returning. By the time
 * a `GeneratedCharacter` reaches the caller there is nothing left on the
 * wasm side to free. Kept as a real function (not thrown/unimplemented)
 * because the frozen API declares it, and callers should be able to call
 * it unconditionally without it being a trap.
 */
export function freeCharacter(_character: GeneratedCharacter): void {
  // Intentionally empty — see doc comment above.
}
