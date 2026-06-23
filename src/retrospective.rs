use crate::types::{GlucoseChange, GlucoseEffect, Timestamp};
use crate::VEC_SIZE;
use heapless::Vec;

const DELTA_MIN: f64 = 5.0;

// Precomputed IRC constants (matching Swift's static lets)
//   integralForget = exp(-delta_min / correctionTimeConstant_min)
const INTEGRAL_FORGET: f64 = {
    // exp(-5.0/60.0) — we approximate via a const since exp isn't const-stable.
    // Actual value ≈ 0.919941
    // Computed offline: e^(-1/12) ≈ 0.919941
    0.919_941_125_497_824_f64
};
const PERSISTENT_GAIN: f64 = 2.0;
const CURRENT_GAIN: f64 = 1.0;
const INTEGRAL_GAIN: f64 =
    (1.0 - INTEGRAL_FORGET) / INTEGRAL_FORGET * (PERSISTENT_GAIN - CURRENT_GAIN);
const PROPORTIONAL_GAIN: f64 = CURRENT_GAIN - INTEGRAL_GAIN;
const DIFFERENTIAL_GAIN: f64 = 2.0;

pub const RETROSPECTION_INTERVAL: f64 = 180.0 * 60.0; // 180 min
const MAX_CORRECTION_DURATION: f64 = 180.0 * 60.0; // 180 min

/// Which retrospective correction algorithm to use.
#[derive(Clone, Copy, Debug, Default)]
pub enum RCMode {
    #[default]
    Standard,
    Integral,
}

fn signum(x: f64) -> f64 {
    if x.is_nan() {
        f64::NAN
    } else if x.is_sign_positive() {
        1.0
    } else {
        -1.0
    }
}

/// Compute the Standard RC (proportional controller) effect timeline.
///
/// Returns cumulative `[GlucoseEffect]` starting at `starting_glucose`.
/// Returns empty if the most recent discrepancy is stale (older than `recency_interval`).
pub fn standard_rc_effect(
    starting_glucose_start: Timestamp,
    starting_glucose_value: f64, // mg/dL
    discrepancies: &[GlucoseChange],
    recency_interval: f64,
    grouping_interval: f64,
    effect_duration: f64,
    delta: f64,
) -> (Vec<GlucoseEffect, VEC_SIZE>, Option<f64>) {
    let current = match discrepancies.last() {
        Some(d) if starting_glucose_start - d.end <= recency_interval => d,
        _ => return (Vec::new(), None),
    };

    let discrepancy_value = current.value_mgdl;
    let retro_interval = (current.end - current.start).max(grouping_interval);
    let velocity = discrepancy_value / retro_interval; // mg/dL/s

    let effects = decay_effect(
        starting_glucose_start,
        starting_glucose_value,
        velocity,
        effect_duration,
        delta,
    );
    (effects, Some(discrepancy_value))
}

/// Compute the Integral RC (PID controller) effect timeline.
pub fn integral_rc_effect(
    starting_glucose_start: Timestamp,
    starting_glucose_value: f64,
    discrepancies: &[GlucoseChange],
    recency_interval: f64,
    grouping_interval: f64,
    effect_duration: f64,
    delta: f64,
) -> (Vec<GlucoseEffect, VEC_SIZE>, Option<f64>) {
    let current = match discrepancies.last() {
        Some(d) if starting_glucose_start - d.end <= recency_interval => d,
        _ => return (Vec::new(), None),
    };

    let current_value = current.value_mgdl;

    // Find contiguous same-sign recent discrepancies
    let cutoff = starting_glucose_start - RETROSPECTION_INTERVAL;
    let past: Vec<&GlucoseChange, VEC_SIZE> = discrepancies
        .iter()
        .filter(|d| d.start >= cutoff && d.end <= starting_glucose_start)
        .collect();

    let current_sign = signum(current_value);
    let mut recent_values: Vec<f64, VEC_SIZE> = Vec::new();
    let mut next = current;
    for d in past.iter().rev() {
        let v = d.value_mgdl;
        if signum(v) == current_sign && next.end - d.end <= recency_interval && libm::fabs(v) >= 0.1
        {
            let _ = recent_values.push(v);
            next = d;
        } else {
            break;
        }
    }
    recent_values.reverse();

    // PID calculation
    let mut integral = 0.0f64;
    let mut integral_duration_min = effect_duration / 60.0 - 2.0 * DELTA_MIN;
    for &v in &recent_values {
        integral = INTEGRAL_FORGET * integral + INTEGRAL_GAIN * v;
        integral_duration_min += 2.0 * DELTA_MIN;
    }
    integral_duration_min = integral_duration_min.min(MAX_CORRECTION_DURATION / 60.0);

    let differential = if recent_values.len() > 1 {
        let prev = recent_values[recent_values.len() - 2];
        current_value - prev
    } else {
        0.0
    };
    let differential_correction = if differential < 0.0 {
        DIFFERENTIAL_GAIN * differential
    } else {
        0.0
    };

    let proportional = PROPORTIONAL_GAIN * current_value;
    let total_correction = proportional + integral + differential_correction;

    let integral_effect_duration = integral_duration_min * 60.0;
    let scaled_correction = total_correction * effect_duration / integral_effect_duration;

    let retro_interval = (current.end - current.start).max(grouping_interval);
    let velocity = scaled_correction / retro_interval;

    let effects = decay_effect(
        starting_glucose_start,
        starting_glucose_value,
        velocity,
        integral_effect_duration,
        delta,
    );
    (effects, Some(total_correction))
}

// ── Decay effect ─────────────────────────────────────────────────────────────

/// Build a cumulative glucose effect that linearly decays from `velocity * delta` at `delta`
/// after `start_time` to 0 at `start_time + duration`.
///
/// The first entry equals the starting glucose value (velocity decay starts at `delta`).
pub fn decay_effect(
    start_time: Timestamp,
    start_value_mgdl: f64,
    velocity_mgdl_per_sec: f64,
    duration: f64,
    delta: f64,
) -> Vec<GlucoseEffect, VEC_SIZE> {
    use crate::types::simulation_date_range;

    let starts = [start_time];
    let ends = [start_time];
    let Some((sim_start, sim_end)) =
        simulation_date_range(&starts, &ends, None, None, duration, delta)
    else {
        return Vec::new();
    };

    // intercept = velocity at t=0 (start_time)
    let intercept = velocity_mgdl_per_sec;
    let decay_start = sim_start + delta;
    // slope decays to 0 at t = duration
    let slope = -intercept / (duration - delta);

    let mut out: Vec<GlucoseEffect, VEC_SIZE> = Vec::new();
    let _ = out.push(GlucoseEffect {
        start: sim_start,
        value_mgdl: start_value_mgdl,
    });
    let mut last_val = start_value_mgdl;
    let mut t = decay_start;
    while t < sim_end {
        let next_val = last_val + (intercept + slope * (t - decay_start)) * delta;
        let _ = out.push(GlucoseEffect {
            start: t,
            value_mgdl: next_val,
        });
        last_val = next_val;
        t += delta;
    }
    out
}
