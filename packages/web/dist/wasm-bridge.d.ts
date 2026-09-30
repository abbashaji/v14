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
    parentIndex: number;
    translation: [number, number, number];
    rotation: [number, number, number, number];
    scale: [number, number, number];
}
export declare class WasmBridge {
    private readonly exports;
    private constructor();
    static instantiate(wasmBytes: ArrayBuffer | Uint8Array): Promise<WasmBridge>;
    /** Loads a Part Pack (`.afpp`) into the module's part registry. */
    initPartRegistryFromPack(packBytes: Uint8Array): boolean;
    private alloc;
    /**
     * Calls `generate_character`, copies the resulting mesh out of wasm linear
     * memory into fresh JS typed arrays, and frees the wasm-side buffer before
     * returning -- per the task's memory-ownership requirement, callers never
     * receive a view directly over wasm memory.
     */
    generateCharacter(dna: RawCharacterDNA): RawMesh | null;
    private readMeshOutputBuffer;
    /**
     * Reads the fully-assembled global skeleton (`master_skeleton.json`'s
     * bone hierarchy plus every bone's real bind-pose local transform,
     * contributed across every loaded part) via `get_skeleton`. This data is
     * read-only, global, and per-process — unlike `generateCharacter`'s
     * per-call output — so call this once after `initPartRegistryFromPack`
     * succeeds, not once per generated character.
     */
    getSkeleton(): RawJoint[] | null;
    /**
     * Reads and copies out the last error string recorded by the module.
     * Per the ABI contract this must be read before any further call into the
     * module, since the string is only valid until the next call.
     */
    getLastError(): string | null;
}
