//! `bind_pose_fit` — CC0-Phase 13, Option 2: derives a per-character,
//! per-axis uniform scale + pivot from a character's own morph, so the
//! caller can scale the (fixed, global) skeleton's bind-pose bone
//! translations to approximately match a morphed character's body
//! proportions before building a `THREE.Skeleton`.
//!
//! Background (see CC0_PHASE_12_POSE_DIAGNOSIS_RESULTS.md and
//! CC0_PHASE_13_OPTION2_SCALED_BIND_POSE.md): every character shares one
//! fixed, global skeleton bind pose, but a macrodetail identity morph
//! (e.g. "baby") can shrink the mesh far more than the skeleton, so a
//! bone's fixed bind position ends up proportionally very far from the
//! morphed skin around it. Phase 12 found the morph is close to a
//! uniform, feet-anchored scale (R² ~0.93-0.99 across axes for the one
//! corner it measured). This module fits that same per-axis
//! `morphed = k * base + b` relationship in general, from *any*
//! character's own pre-morph vs. post-morph vertex positions -- not
//! special-cased to any particular morph.

/// Per-axis fit result: `morphed ≈ scale[axis] * (base[axis] - pivot[axis]) + pivot[axis]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScaleFit {
    pub scale: [f32; 3],
    pub pivot: [f32; 3],
}

impl ScaleFit {
    /// The no-op fit: no scaling, pivot irrelevant. Used when there is no
    /// morph to fit against (an unmorphed/adult-like character), so the
    /// per-call cost of fitting is skipped entirely rather than computing
    /// a fit that would come out as `scale ≈ [1, 1, 1]` anyway.
    pub const IDENTITY: ScaleFit = ScaleFit {
        scale: [1.0, 1.0, 1.0],
        pivot: [0.0, 0.0, 0.0],
    };
}

/// Fits `scale`/`pivot` per axis by ordinary least squares between
/// `base_positions` (pre-morph vertex positions) and `morphed_positions`
/// (the same vertex set's post-morph positions), matched strictly by
/// index -- callers must pass same-length, index-corresponding slices
/// (exactly what `generate_character`'s own pipeline already has: the
/// registry's unmorphed `part.vertices` concatenated in the same
/// head/torso/arms/legs order as the morphed, merged vertex buffer, so
/// vertex `i` in one slice is the same logical vertex as vertex `i` in
/// the other).
///
/// Per axis: fits `y = k*x + b` by the standard closed-form least-squares
/// formulas (accumulated in `f64` to avoid precision loss over ~50k
/// vertices), then recovers the pivot from `b = (1 - k) * pivot`, i.e.
/// `pivot = b / (1 - k)`. If an axis's `x` values have (numerically) zero
/// variance, or `k` comes out numerically indistinguishable from 1
/// (pivot undefined/irrelevant -- no scaling happening on that axis),
/// that axis falls back to `scale = 1.0, pivot = 0.0` (identity) rather
/// than dividing by ~0 and producing a wild pivot.
///
/// Returns `ScaleFit::IDENTITY` if either input is empty, or if the two
/// slices differ in length (a mismatch that should never happen given
/// the caller contract above, but this function does not panic on
/// mismatched input -- callers running unusual test data should get a
/// safe no-op instead of a slice index panic).
pub fn fit_scale_pivot(base_positions: &[[f32; 3]], morphed_positions: &[[f32; 3]]) -> ScaleFit {
    let n = base_positions.len();
    if n == 0 || morphed_positions.len() != n {
        return ScaleFit::IDENTITY;
    }
    let n_f = n as f64;

    let mut sum_x = [0.0f64; 3];
    let mut sum_y = [0.0f64; 3];
    let mut sum_xx = [0.0f64; 3];
    let mut sum_xy = [0.0f64; 3];

    for i in 0..n {
        for axis in 0..3 {
            let x = base_positions[i][axis] as f64;
            let y = morphed_positions[i][axis] as f64;
            sum_x[axis] += x;
            sum_y[axis] += y;
            sum_xx[axis] += x * x;
            sum_xy[axis] += x * y;
        }
    }

    let mut scale = [1.0f32; 3];
    let mut pivot = [0.0f32; 3];

    for axis in 0..3 {
        let denom = n_f * sum_xx[axis] - sum_x[axis] * sum_x[axis];
        // Degenerate: every base position on this axis is identical (or
        // near enough that the fit is numerically unstable) -- nothing to
        // fit a slope against, so leave this axis at identity.
        if denom.abs() < 1e-9 {
            continue;
        }
        let k = (n_f * sum_xy[axis] - sum_x[axis] * sum_y[axis]) / denom;
        let b = (sum_y[axis] - k * sum_x[axis]) / n_f;

        // k == 1 (no scale on this axis) makes `b / (1 - k)` blow up; the
        // pivot is meaningless anyway when there's no scaling to anchor,
        // so fall back to identity for this axis rather than propagate a
        // huge/garbage pivot value.
        if (k - 1.0).abs() < 1e-6 {
            continue;
        }

        scale[axis] = k as f32;
        pivot[axis] = (b / (1.0 - k)) as f32;
    }

    ScaleFit { scale, pivot }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_when_inputs_are_identical() {
        let base = vec![[1.0, 2.0, 3.0], [4.0, -5.0, 6.0], [0.0, 0.0, 0.0]];
        let fit = fit_scale_pivot(&base, &base);
        assert_eq!(fit.scale, [1.0, 1.0, 1.0]);
        // Pivot is irrelevant at k=1 (falls back to identity's 0.0).
        assert_eq!(fit.pivot, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn empty_input_is_identity() {
        let fit = fit_scale_pivot(&[], &[]);
        assert_eq!(fit, ScaleFit::IDENTITY);
    }

    #[test]
    fn mismatched_lengths_is_identity() {
        let base = vec![[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]];
        let morphed = vec![[0.0, 0.0, 0.0]];
        let fit = fit_scale_pivot(&base, &morphed);
        assert_eq!(fit, ScaleFit::IDENTITY);
    }

    #[test]
    fn recovers_exact_uniform_feet_anchored_scale() {
        // Construct base positions, then apply a KNOWN k/pivot per axis to
        // get "morphed" positions -- the fit must recover k/pivot exactly
        // (up to float error) for this noise-free case.
        let known_k = [0.35f32, 0.32, 0.34];
        let known_pivot = [0.0f32, -8.4, 0.0]; // feet-anchored: pivot near 0 on X/Z

        let base: Vec<[f32; 3]> = (0..200)
            .map(|i| {
                let t = i as f32;
                [t * 0.1 - 5.0, t * 0.2 - 8.0, (t * 0.05).sin() * 3.0]
            })
            .collect();

        let morphed: Vec<[f32; 3]> = base
            .iter()
            .map(|p| {
                let mut m = [0.0f32; 3];
                for axis in 0..3 {
                    m[axis] = known_k[axis] * (p[axis] - known_pivot[axis]) + known_pivot[axis];
                }
                m
            })
            .collect();

        let fit = fit_scale_pivot(&base, &morphed);
        for axis in 0..3 {
            assert!(
                (fit.scale[axis] - known_k[axis]).abs() < 1e-3,
                "axis {axis}: expected scale {}, got {}",
                known_k[axis],
                fit.scale[axis]
            );
            assert!(
                (fit.pivot[axis] - known_pivot[axis]).abs() < 1e-2,
                "axis {axis}: expected pivot {}, got {}",
                known_pivot[axis],
                fit.pivot[axis]
            );
        }
    }

    #[test]
    fn degenerate_constant_axis_falls_back_to_identity_on_that_axis() {
        // Every base X value is identical -- no variance to fit a slope
        // against on that axis.
        let base = vec![[5.0, 0.0, 0.0], [5.0, 1.0, 0.0], [5.0, 2.0, 0.0]];
        let morphed = vec![[5.0, 0.0, 0.0], [5.0, 0.5, 0.0], [5.0, 1.0, 0.0]];
        let fit = fit_scale_pivot(&base, &morphed);
        assert_eq!(fit.scale[0], 1.0, "degenerate X axis must fall back to identity");
        assert_eq!(fit.pivot[0], 0.0);
        // Y axis has real variance (k=0.5, pivot=0) and should fit normally.
        assert!((fit.scale[1] - 0.5).abs() < 1e-4);
    }
}

#[cfg(test)]
mod perf {
    use super::*;
    use std::time::Instant;

    /// Real measured cost of `fit_scale_pivot` alone (isolated from the
    /// rest of `generate_character` -- mesh merge, morph blend, I/O,
    /// etc.), on a vertex count matching the real essentials.afpp
    /// combined body (53,512 vertices per CC0-Phase 12 §3).
    #[test]
    fn measures_real_fit_cost_at_production_vertex_count() {
        let n = 53_512usize;
        // Synthetic but realistically-scaled data: not zeros/constants
        // (which would hit the degenerate-axis fast path and
        // under-measure the real cost), a plausible body-sized spread.
        let base: Vec<[f32; 3]> = (0..n)
            .map(|i| {
                let t = i as f32;
                [(t * 0.017).sin() * 0.4, (t * 0.0031) % 17.0 - 8.0, (t * 0.023).cos() * 0.3]
            })
            .collect();
        let morphed: Vec<[f32; 3]> = base
            .iter()
            .map(|p| [p[0] * 0.35, (p[1] + 8.4) * 0.32 - 8.4, p[2] * 0.42])
            .collect();

        let mut durations_us = Vec::new();
        for _ in 0..20 {
            let start = Instant::now();
            let fit = fit_scale_pivot(&base, &morphed);
            let elapsed = start.elapsed();
            std::hint::black_box(fit);
            durations_us.push(elapsed.as_secs_f64() * 1_000_000.0);
        }
        let mean: f64 = durations_us.iter().sum::<f64>() / durations_us.len() as f64;
        let min = durations_us.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = durations_us.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        println!(
            "fit_scale_pivot({n} vertices) over 20 runs: mean={mean:.1}us min={min:.1}us max={max:.1}us"
        );
    }
}
