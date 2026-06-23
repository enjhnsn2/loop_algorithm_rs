use crate::types::{simulation_date_range, GlucoseEffect, GlucoseEffectVelocity, Timestamp};
use crate::VEC_SIZE;
use heapless::Vec;

// ── Glucose sample ────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, Copy)]
pub struct GlucoseSample {
    pub start: Timestamp,
    pub value_mgdl: f64,
    /// Identifies the CGM source.  Samples from different sources aren't used together.
    pub provenance: u32,
    /// True for calibration / display-only entries (excluded from calculations).
    pub is_display_only: bool,
}

// ── Continuity checks ─────────────────────────────────────────────────────────

pub fn has_single_provenance(samples: &[GlucoseSample]) -> bool {
    match samples.first() {
        None => true,
        Some(first) => samples.iter().all(|s| s.provenance == first.provenance),
    }
}

pub fn contains_calibrations(samples: &[GlucoseSample]) -> bool {
    samples.iter().any(|s| s.is_display_only)
}

/// True when the samples span at most `count * interval` seconds (contiguous).
pub fn is_continuous(samples: &[GlucoseSample], interval: f64) -> bool {
    let (first, last) = match (samples.first(), samples.last()) {
        (Some(f), Some(l)) => (f, l),
        _ => return false,
    };
    libm::fabs(last.start - first.start) < interval * samples.len() as f64
}

/// True when no consecutive pair differs by more than `threshold` mg/dL.
pub fn has_gradual_transitions(samples: &[GlucoseSample], threshold: f64) -> bool {
    if samples.len() < 2 {
        return false;
    }
    samples
        .windows(2)
        .all(|w| libm::fabs(w[1].value_mgdl - w[0].value_mgdl) <= threshold)
}

// ── Linear regression ─────────────────────────────────────────────────────────

fn linear_regression(points: &[(f64, f64)]) -> (f64, f64) {
    let n = points.len() as f64;
    let (mut sx, mut sy, mut sxy, mut sx2) = (0.0, 0.0, 0.0, 0.0);
    for &(x, y) in points {
        sx += x;
        sy += y;
        sxy += x * y;
        sx2 += x * x;
    }
    let denom = n * sx2 - sx * sx;
    let slope = (n * sxy - sx * sy) / denom;
    let intercept = (sy * sx2 - sx * sxy) / denom;
    (slope, intercept)
}

// ── Momentum effect ───────────────────────────────────────────────────────────

const MOMENTUM_DATA_INTERVAL: f64 = 15.0 * 60.0; // 15 min
const VELOCITY_MAX_MGDL_PER_SEC: f64 = 4.0 / 60.0; // 4 mg/dL/min → per sec

/// Compute short-term momentum glucose effects via linear regression on `samples`.
///
/// `samples` should be the most recent 15 minutes of glucose readings.  Returns a cumulative
/// `[GlucoseEffect]` timeline starting at the last sample and extending for `duration` seconds.
pub fn linear_momentum_effect(
    samples: &[GlucoseSample],
    duration: f64,
    delta: f64,
) -> Vec<GlucoseEffect, VEC_SIZE> {
    if samples.len() <= 2
        || !has_single_provenance(samples)
        || contains_calibrations(samples)
        || !is_continuous(
            samples,
            MOMENTUM_DATA_INTERVAL / samples.len().max(1) as f64 + delta,
        )
        || !has_gradual_transitions(samples, 40.0)
    {
        return Vec::new();
    }

    let first = samples.first().unwrap();
    let last = samples.last().unwrap();

    let points: Vec<(f64, f64), VEC_SIZE> = samples
        .iter()
        .map(|s| (s.start - first.start, s.value_mgdl))
        .collect();
    let (slope, _) = linear_regression(&points);
    if !slope.is_finite() {
        return Vec::new();
    }
    let limited_slope = slope.min(VELOCITY_MAX_MGDL_PER_SEC);

    let starts = [last.start];
    let ends = [last.start];
    let Some((start, end)) = simulation_date_range(&starts, &ends, None, None, duration, delta)
    else {
        return Vec::new();
    };

    let mut t = start;
    let mut out: Vec<GlucoseEffect, VEC_SIZE> = Vec::new();
    while t <= end {
        let value = (t - last.start).max(0.0) * limited_slope;
        let _ = out.push(GlucoseEffect {
            start: t,
            value_mgdl: value,
        });
        t += delta;
    }
    out
}

// ── Counteraction effects (ICE) ───────────────────────────────────────────────

/// Compute the timeline of glucose-effect velocities that counteract the provided insulin effects.
///
/// Each returned velocity represents the "unexplained" portion of glucose change for a
/// consecutive pair of valid glucose samples — i.e., `observed_Δglucose − insulin_effect_Δ`.
pub fn counteraction_effects(
    samples: &[GlucoseSample],
    insulin_effects: &[GlucoseEffect],
) -> Vec<GlucoseEffectVelocity, VEC_SIZE> {
    if samples.is_empty() || insulin_effects.is_empty() {
        return Vec::new();
    }

    let first_effect_start = insulin_effects[0].start;
    let start_idx = match samples.iter().position(|s| s.start >= first_effect_start) {
        Some(i) => i,
        None => return Vec::new(),
    };

    let mut out: Vec<GlucoseEffectVelocity, VEC_SIZE> = Vec::new();
    let mut effect_idx = 0usize;
    let mut sg_start = start_idx;
    let mut sg_end = start_idx + 1;

    while sg_end < samples.len() {
        let gs = &samples[sg_start];
        let ge = &samples[sg_end];

        let dt = ge.start - gs.start;
        if dt <= 4.0 * 60.0 {
            // need at least 4-min gap
            sg_end += 1;
            continue;
        }

        // Advance regardless of whether we produce output
        let advance = sg_start;
        sg_start = sg_end;
        sg_end += 1;

        // Skip if provenance differs or either is display-only
        if gs.provenance != ge.provenance || gs.is_display_only || ge.is_display_only {
            continue;
        }

        let glucose_change = ge.value_mgdl - gs.value_mgdl;

        // Find matching effect bracket
        let mut start_effect: Option<f64> = None;
        let mut end_effect: Option<f64> = None;

        for eff in &insulin_effects[effect_idx..] {
            if start_effect.is_none() && eff.start >= gs.start {
                start_effect = Some(eff.value_mgdl);
            } else if end_effect.is_none() && eff.start >= ge.start {
                end_effect = Some(eff.value_mgdl);
                break;
            }
            effect_idx += 1;
        }

        let (Some(se), Some(ee)) = (start_effect, end_effect) else {
            break;
        };

        let effect_change = ee - se;
        let discrepancy = glucose_change - effect_change;
        let velocity = discrepancy / dt;

        let _ = out.push(GlucoseEffectVelocity {
            start: gs.start,
            end: ge.start,
            value_mgdl_per_sec: velocity,
        });

        let _ = advance; // suppress unused warning
    }
    out
}

// ── Subtracting carb effects from ICE to get discrepancies ────────────────────

/// Subtract uniform-interval carb glucose effects from counteraction-effect velocities.
///
/// Returns a `[GlucoseEffect]` where each entry is the glucose change NOT explained by carbs
/// in that time window.
pub fn subtract_effects(
    velocities: &[GlucoseEffectVelocity],
    carb_effects: &[GlucoseEffect],
    effect_interval: f64,
) -> Vec<GlucoseEffect, VEC_SIZE> {
    if velocities.is_empty() {
        return Vec::new();
    }
    // When no carb effects, discrepancy equals the ICE directly (nothing to subtract)
    if carb_effects.is_empty() {
        return velocities
            .iter()
            .map(|vel| GlucoseEffect {
                start: vel.end,
                value_mgdl: vel.value_mgdl_per_sec * effect_interval,
            })
            .collect();
    }

    // Trim carb_effects to start at or after velocities[0].end
    let first_vel_end = velocities[0].end;
    let carb_start_idx = match carb_effects.iter().position(|e| e.start >= first_vel_end) {
        Some(i) => i,
        None => return Vec::new(),
    };

    // Trim velocities to start at or after carb_effects[0].start
    let first_carb_start = carb_effects[carb_start_idx].start;
    let vel_start_idx = match velocities.iter().position(|v| v.end >= first_carb_start) {
        Some(i) => i,
        None => return Vec::new(),
    };

    let carb_slice = &carb_effects[carb_start_idx..];
    let vel_slice = &velocities[vel_start_idx..];

    let mut out: Vec<GlucoseEffect, VEC_SIZE> = Vec::new();
    let mut prev_carb_val = carb_slice[0].value_mgdl;
    let mut vel_idx = 0usize;

    for carb_eff in carb_slice.iter().skip(1) {
        if vel_idx >= vel_slice.len() {
            break;
        }
        let carb_change = carb_eff.value_mgdl - prev_carb_val;
        prev_carb_val = carb_eff.value_mgdl;

        let vel = &vel_slice[vel_idx];
        if vel.end > carb_eff.start {
            // vel not yet in this carb window — skip this carb step
            continue;
        }
        vel_idx += 1;

        let vel_contribution = vel.value_mgdl_per_sec * effect_interval;
        let _ = out.push(GlucoseEffect {
            start: vel.end,
            value_mgdl: vel_contribution - carb_change,
        });
    }

    // Remaining velocities with no carb counterpart (carb effect = 0)
    for vel in &vel_slice[vel_idx..] {
        let _ = out.push(GlucoseEffect {
            start: vel.end,
            value_mgdl: vel.value_mgdl_per_sec * effect_interval,
        });
    }

    out
}

// ── Combined sums (rolling window sum for RC) ─────────────────────────────────

/// For each effect, build a rolling sum of all effects within `duration` seconds before it.
///
/// Input must be sorted chronologically.  This is used to compute the retrospective glucose
/// discrepancy over the past 30 minutes at each point in time.
pub fn combined_sums(
    effects: &[GlucoseEffect],
    duration: f64,
) -> Vec<crate::types::GlucoseChange, VEC_SIZE> {
    use crate::types::GlucoseChange;
    let mut sums: Vec<GlucoseChange, VEC_SIZE> = Vec::new();
    let mut last_valid = 0usize;

    for effect in effects.iter().rev() {
        let n = sums.len();
        let mut i = last_valid;
        while i < n {
            // sums[i].end is the latest time in that sum (initialized from a more-recent effect)
            if sums[i].end > effect.start + duration {
                last_valid = i + 1;
            } else {
                sums[i].append_effect(effect);
            }
            i += 1;
        }
        let _ = sums.push(GlucoseChange {
            start: effect.start,
            end: effect.start, // GlucoseEffect.endDate == startDate
            value_mgdl: effect.value_mgdl,
        });
    }

    sums.reverse();
    sums
}

/// Net cumulative effect of a slice: last - first value.
pub fn net_effect(effects: &[GlucoseEffect]) -> Option<f64> {
    let first = effects.first()?;
    let last = effects.last()?;
    Some(last.value_mgdl - first.value_mgdl)
}
