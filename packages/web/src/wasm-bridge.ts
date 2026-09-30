// Low-level bridge to the anthroforge_core wasm32 module.
//
// This file owns every byte-offset detail of the ABI. Nothing above this
// module (index.ts) should know about pointers, offsets, or little-endian
// layout.
//
// ABI history: through CC0-Phase 2 `CharacterDNA` was 32 bytes on wasm32
// (seed, height, weight, head_id, torso_id, clothing ptr/count). CC0-Phase 3
// appended `arms_id`, `legs_id` (both REQUIRED -- there is no "0 means skip"
// sentinel) and three morph fields, so the struct is now larger and the old
// 32-byte layout would leave arms/legs/morphs reading uninitialised memory.
//
// ABI verification note: these offsets were derived from the Rust
// `#[repr(C)]` struct for wasm32 (4-byte pointers, 8-byte u64 alignment),
// then verified empirically against a real wasm32 build of the core --
// not by a compile-time `size_of` assert (the crate only asserts the native
// size). Method, per field, against the real module + a real v2 Part Pack:
//   - `arms_id` (32) / `legs_id` (36): setting a bogus id produced the exact
//     error "no part loaded for arms_id 999999" / "... legs_id 888888",
//     i.e. the id echoed back is the one written at that offset.
//   - `height_modifier` (8): 0 produced "DNA mutation failed: scale[1] = 0 is
//     invalid ...".
//   - `active_morph_ids_ptr` (40) / `active_morph_weights_ptr` (44) /
//     `active_morph_count` (48): morph 5001 at weight 1.0 displaced vertices
//     by 0.0480 (identical to the native run) and at 0.5 by exactly half.
// The struct size itself (56) is the allocation used here; Rust reads only
// through offset 52. If the crate's struct layout changes again, re-verify
// every offset the same way. This is the same method that verified the
// original 32-byte layout.
//
// Alignment note: `wasm_alloc` is a plain byte-aligned `Vec<u8>` allocation.
// In practice the wasm32 allocator returns 8-byte-aligned blocks, which is
// what the u64 `seed` field and the u16/f32 morph arrays rely on; this bridge
// has always depended on that.

const CHARACTER_DNA_SIZE = 56;
// 16 bytes original (2 pointers + 2 counts) + 24 bytes CC0-Phase 13 added
// (rest_scale + rest_pivot, each [f32; 3]) = 40. Matches the Rust-side
// `MeshOutputBuffer`'s wasm32 layout exactly -- see that struct's own
// field-order comment; the two new fields are appended after the
// original four, never interleaved earlier.
const MESH_OUTPUT_BUFFER_SIZE = 40;
const SKINNED_VERTEX_SIZE = 56;

export interface RawCharacterDNA {
  seed: bigint;
  heightModifier: number;
  weightModifier: number;
  headId: number;
  torsoId: number;
  armsId: number;
  legsId: number;
  clothingIds: number[];
  morphs: RawMorphWeight[];
}

export interface RawMorphWeight {
  /** Morph id as stored in the Part Pack's morph table (u16). */
  id: number;
  /** Blend weight; 0 = none, 1 = full. Validated Rust-side. */
  weight: number;
}

export interface RawMesh {
  positions: Float32Array;
  normals: Float32Array;
  uvs: Float32Array;
  boneIndices: Uint16Array;
  boneWeights: Float32Array;
  indices: Uint32Array;
  /**
   * CC0-Phase 13, Option 2: per-axis uniform scale + pivot fit between
   * this character's own pre-morph and post-morph vertex positions (see
   * `MeshOutputBuffer.rest_scale`/`rest_pivot` on the Rust side). `[1, 1,
   * 1]` scale with a `[0, 0, 0]` pivot for an unmorphed character (no
   * active morphs) -- not necessarily `[1, 1, 1]` for every "adult-ish"
   * morph, since an identity-corner morph like the "young adult" corner
   * is itself a real (if modest) morph away from the unmorphed base
   * mesh, not a no-op. A consumer scales the fixed bind-pose skeleton's
   * WORLD bind positions by `restPivot + restScale ⊙ (worldPos -
   * restPivot)` before building a `THREE.Skeleton` -- see
   * `@anthroforge/web-three`'s `toSkinnedMesh` for the reference
   * implementation, including why this must be done in WORLD space, not
   * applied directly to each bone's LOCAL translation.
   */
  restScale: [number, number, number];
  restPivot: [number, number, number];
}

export interface RawJoint {
  parentIndex: number; // -1 = root
  translation: [number, number, number];
  rotation: [number, number, number, number];
  scale: [number, number, number];
}

interface WasmExports {
  memory: WebAssembly.Memory;
  wasm_alloc(size: number): number;
  wasm_dealloc(ptr: number, size: number): void;
  init_part_registry_from_pack(packPtr: number, packLen: number): number;
  generate_character(dnaPtr: number): number;
  free_mesh_buffer(bufferPtr: number): void;
  anthroforge_last_error(): number;
  get_skeleton(): number; // returns a pointer, 0 = failure
  free_skeleton_buffer(ptr: number): void;
}

function getCrypto(): Crypto {
  // Use the standard Web Crypto API so this works in both browsers and
  // modern Node (available on globalThis since Node 19+) — the SDK must
  // not depend on Node's `crypto` module directly, since it ships to
  // browsers too.
  const c = globalThis.crypto;
  if (!c || typeof c.getRandomValues !== "function") {
    throw new Error(
      "AnthroForge/Web: no Web Crypto API (globalThis.crypto.getRandomValues) " +
        "available in this environment.",
    );
  }
  return c;
}

export class WasmBridge {
  private readonly exports: WasmExports;

  private constructor(exports: WasmExports) {
    this.exports = exports;
  }

  static async instantiate(wasmBytes: ArrayBuffer | Uint8Array): Promise<WasmBridge> {
    let bridge: WasmBridge;
    const importObject: WebAssembly.Imports = {
      anthroforge_host: {
        // fill_random(ptr: u32, len: u32) -> void, required by the module's
        // getrandom backend. Fill `len` random bytes into the module's own
        // linear memory starting at `ptr`.
        fill_random: (ptr: number, len: number) => {
          const view = new Uint8Array(bridge.exports.memory.buffer, ptr, len);
          getCrypto().getRandomValues(view);
        },
      },
    };

    const source: BufferSource =
      wasmBytes instanceof Uint8Array
        ? (wasmBytes.buffer.slice(
            wasmBytes.byteOffset,
            wasmBytes.byteOffset + wasmBytes.byteLength,
          ) as ArrayBuffer)
        : wasmBytes;
    const { instance } = await WebAssembly.instantiate(source, importObject);
    const exports = instance.exports as unknown as WasmExports;
    bridge = new WasmBridge(exports);
    return bridge;
  }

  /** Loads a Part Pack (`.afpp`) into the module's part registry. */
  initPartRegistryFromPack(packBytes: Uint8Array): boolean {
    const packPtr = this.exports.wasm_alloc(packBytes.length);
    if (packPtr === 0) {
      throw new Error("AnthroForge/Web: wasm_alloc failed while loading the Part Pack.");
    }
    try {
      new Uint8Array(this.exports.memory.buffer, packPtr, packBytes.length).set(packBytes);
      const result = this.exports.init_part_registry_from_pack(packPtr, packBytes.length);
      return result !== 0;
    } finally {
      this.exports.wasm_dealloc(packPtr, packBytes.length);
    }
  }

  private alloc(size: number, what: string): number {
    const ptr = this.exports.wasm_alloc(size);
    if (ptr === 0) {
      throw new Error(`AnthroForge/Web: wasm_alloc failed while building ${what}.`);
    }
    return ptr;
  }

  /**
   * Calls `generate_character`, copies the resulting mesh out of wasm linear
   * memory into fresh JS typed arrays, and frees the wasm-side buffer before
   * returning -- per the task's memory-ownership requirement, callers never
   * receive a view directly over wasm memory.
   */
  generateCharacter(dna: RawCharacterDNA): RawMesh | null {
    for (const m of dna.morphs) {
      if (!Number.isInteger(m.id) || m.id < 0 || m.id > 0xffff) {
        throw new RangeError(
          `AnthroForge/Web: morph id ${m.id} is not a valid u16 (integer 0..65535).`,
        );
      }
    }

    const clothingByteLen = dna.clothingIds.length * 4;
    const morphIdsByteLen = dna.morphs.length * 2;
    const morphWeightsByteLen = dna.morphs.length * 4;

    let dnaPtr = 0;
    let clothingPtr = 0;
    let morphIdsPtr = 0;
    let morphWeightsPtr = 0;

    try {
      // Allocate EVERYTHING before creating any view: each wasm_alloc can
      // grow linear memory, which detaches any ArrayBuffer/DataView made
      // over the old `memory.buffer`.
      dnaPtr = this.alloc(CHARACTER_DNA_SIZE, "CharacterDNA");
      if (dna.clothingIds.length > 0) {
        clothingPtr = this.alloc(clothingByteLen, "the clothing id list");
      }
      if (dna.morphs.length > 0) {
        morphIdsPtr = this.alloc(morphIdsByteLen, "the morph id list");
        morphWeightsPtr = this.alloc(morphWeightsByteLen, "the morph weight list");
      }

      const buffer = this.exports.memory.buffer;
      // wasm_alloc does not zero memory; zero the struct so padding is never
      // uninitialised.
      new Uint8Array(buffer, dnaPtr, CHARACTER_DNA_SIZE).fill(0);

      if (clothingPtr !== 0) {
        const v = new DataView(buffer, clothingPtr, clothingByteLen);
        dna.clothingIds.forEach((id, i) => v.setUint32(i * 4, id >>> 0, true));
      }
      if (morphIdsPtr !== 0) {
        const ids = new DataView(buffer, morphIdsPtr, morphIdsByteLen);
        const weights = new DataView(buffer, morphWeightsPtr, morphWeightsByteLen);
        dna.morphs.forEach((m, i) => {
          ids.setUint16(i * 2, m.id, true);
          weights.setFloat32(i * 4, m.weight, true);
        });
      }

      // CharacterDNA, 56 bytes, little-endian (wasm32):
      //   0  seed                      u64
      //   8  height_modifier           f32
      //  12  weight_modifier           f32
      //  16  head_id                   u32
      //  20  torso_id                  u32
      //  24  equipped_clothing_ids_ptr u32 (wasm32 address, 0 = null)
      //  28  equipped_clothing_count   u32
      //  32  arms_id                   u32   (required)
      //  36  legs_id                   u32   (required)
      //  40  active_morph_ids_ptr      u32   (wasm32 address of u16[], 0 = null)
      //  44  active_morph_weights_ptr  u32   (wasm32 address of f32[], 0 = null)
      //  48  active_morph_count        u32
      //  52  (4 bytes tail padding; struct alignment is 8)
      const v = new DataView(buffer, dnaPtr, CHARACTER_DNA_SIZE);
      v.setBigUint64(0, dna.seed, true);
      v.setFloat32(8, dna.heightModifier, true);
      v.setFloat32(12, dna.weightModifier, true);
      v.setUint32(16, dna.headId >>> 0, true);
      v.setUint32(20, dna.torsoId >>> 0, true);
      v.setUint32(24, clothingPtr, true);
      v.setUint32(28, dna.clothingIds.length, true);
      v.setUint32(32, dna.armsId >>> 0, true);
      v.setUint32(36, dna.legsId >>> 0, true);
      v.setUint32(40, morphIdsPtr, true);
      v.setUint32(44, morphWeightsPtr, true);
      v.setUint32(48, dna.morphs.length, true);

      const bufferPtr = this.exports.generate_character(dnaPtr);
      if (bufferPtr === 0) {
        return null;
      }

      try {
        return this.readMeshOutputBuffer(bufferPtr);
      } finally {
        this.exports.free_mesh_buffer(bufferPtr);
      }
    } finally {
      if (dnaPtr !== 0) this.exports.wasm_dealloc(dnaPtr, CHARACTER_DNA_SIZE);
      if (clothingPtr !== 0) this.exports.wasm_dealloc(clothingPtr, clothingByteLen);
      if (morphIdsPtr !== 0) this.exports.wasm_dealloc(morphIdsPtr, morphIdsByteLen);
      if (morphWeightsPtr !== 0) this.exports.wasm_dealloc(morphWeightsPtr, morphWeightsByteLen);
    }
  }

  private readMeshOutputBuffer(bufferPtr: number): RawMesh {
    // MeshOutputBuffer, 40 bytes, little-endian:
    //   0  vertices_ptr    u32
    //   4  indices_ptr     u32
    //   8  vertices_count  u32
    //  12  indices_count   u32
    //  16  rest_scale      [f32; 3]  (12 bytes) -- CC0-Phase 13
    //  28  rest_pivot      [f32; 3]  (12 bytes) -- CC0-Phase 13
    const headerView = new DataView(this.exports.memory.buffer, bufferPtr, MESH_OUTPUT_BUFFER_SIZE);
    const verticesPtr = headerView.getUint32(0, true);
    const indicesPtr = headerView.getUint32(4, true);
    const verticesCount = headerView.getUint32(8, true);
    const indicesCount = headerView.getUint32(12, true);
    const restScale: [number, number, number] = [
      headerView.getFloat32(16, true),
      headerView.getFloat32(20, true),
      headerView.getFloat32(24, true),
    ];
    const restPivot: [number, number, number] = [
      headerView.getFloat32(28, true),
      headerView.getFloat32(32, true),
      headerView.getFloat32(36, true),
    ];

    const positions = new Float32Array(verticesCount * 3);
    const normals = new Float32Array(verticesCount * 3);
    const uvs = new Float32Array(verticesCount * 2);
    const boneIndices = new Uint16Array(verticesCount * 4);
    const boneWeights = new Float32Array(verticesCount * 4);

    // SkinnedVertex, 56 bytes each, little-endian, identical on every
    // target (no pointers in this struct):
    //   0  position      [f32; 3]  (12 bytes)
    //  12  normal        [f32; 3]  (12 bytes)
    //  24  uv            [f32; 2]  ( 8 bytes)
    //  32  bone_indices  [u16; 4]  ( 8 bytes)
    //  40  bone_weights  [f32; 4]  (16 bytes)
    for (let i = 0; i < verticesCount; i++) {
      const vertexBase = verticesPtr + i * SKINNED_VERTEX_SIZE;
      const v = new DataView(this.exports.memory.buffer, vertexBase, SKINNED_VERTEX_SIZE);

      for (let c = 0; c < 3; c++) {
        positions[i * 3 + c] = v.getFloat32(c * 4, true);
        normals[i * 3 + c] = v.getFloat32(12 + c * 4, true);
      }
      for (let c = 0; c < 2; c++) {
        uvs[i * 2 + c] = v.getFloat32(24 + c * 4, true);
      }
      for (let c = 0; c < 4; c++) {
        boneIndices[i * 4 + c] = v.getUint16(32 + c * 2, true);
        boneWeights[i * 4 + c] = v.getFloat32(40 + c * 4, true);
      }
    }

    // Flat u32 index array, `indices_count` entries, copied out before the
    // wasm-side buffer is freed by the caller.
    const indices = new Uint32Array(
      this.exports.memory.buffer.slice(indicesPtr, indicesPtr + indicesCount * 4),
    );

    return { positions, normals, uvs, boneIndices, boneWeights, indices, restScale, restPivot };
  }

  /**
   * Reads the fully-assembled global skeleton (`master_skeleton.json`'s
   * bone hierarchy plus every bone's real bind-pose local transform,
   * contributed across every loaded part) via `get_skeleton`. This data is
   * read-only, global, and per-process — unlike `generateCharacter`'s
   * per-call output — so call this once after `initPartRegistryFromPack`
   * succeeds, not once per generated character.
   */
  getSkeleton(): RawJoint[] | null {
    const ptr = this.exports.get_skeleton();
    if (ptr === 0) {
      return null;
    }
    try {
      // SkeletonBuffer, 8 bytes on wasm32, little-endian:
      //   0  joints_ptr    u32 (wasm32 address)
      //   4  joint_count   u32
      const headerView = new DataView(this.exports.memory.buffer, ptr, 8);
      const jointsPtr = headerView.getUint32(0, true);
      const jointCount = headerView.getUint32(4, true);

      const joints: RawJoint[] = [];
      for (let i = 0; i < jointCount; i++) {
        // FfiJoint, 44 bytes, little-endian, identical on every target
        // (no pointers):
        //   0   parent_index  i32   (-1 = no parent / this is a root)
        //   4   translation   [f32; 3]  (12 bytes)
        //  16   rotation      [f32; 4]  (16 bytes) -- quaternion, (x, y, z, w)
        //  32   scale         [f32; 3]  (12 bytes)
        const base = jointsPtr + i * 44;
        const v = new DataView(this.exports.memory.buffer, base, 44);
        joints.push({
          parentIndex: v.getInt32(0, true),
          translation: [v.getFloat32(4, true), v.getFloat32(8, true), v.getFloat32(12, true)],
          rotation: [
            v.getFloat32(16, true),
            v.getFloat32(20, true),
            v.getFloat32(24, true),
            v.getFloat32(28, true),
          ],
          scale: [v.getFloat32(32, true), v.getFloat32(36, true), v.getFloat32(40, true)],
        });
      }
      return joints;
    } finally {
      this.exports.free_skeleton_buffer(ptr);
    }
  }

  /**
   * Reads and copies out the last error string recorded by the module.
   * Per the ABI contract this must be read before any further call into the
   * module, since the string is only valid until the next call.
   */
  getLastError(): string | null {
    const ptr = this.exports.anthroforge_last_error();
    if (ptr === 0) {
      return null;
    }
    const bytes = new Uint8Array(this.exports.memory.buffer);
    let end = ptr;
    while (bytes[end] !== 0) {
      end++;
    }
    return new TextDecoder("utf-8").decode(bytes.slice(ptr, end));
  }
}
