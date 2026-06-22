use crate::types::{
    closest_prior, floor_to, GlucoseEffect, GlucoseEffectVelocity, ScheduleEntry, Timestamp,
};

// ── Constants ─────────────────────────────────────────────────────────────────

pub const MAX_ABSORPTION_TIME: f64 = 10.0 * 3600.0; // 10 hours
pub const DEFAULT_ABSORPTION_TIME: f64 = 3.0 * 3600.0; // 3 hours
pub const DEFAULT_ABSORPTION_TIME_OVERRUN: f64 = 1.5;
pub const DEFAULT_EFFECT_DELAY: f64 = 10.0 * 60.0; // 10 minutes
const DELTA: f64 = 300.0;

// ── Absorption models ─────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Default)]
pub enum CarbAbsorptionModel {
    Linear,
    #[default]
    PiecewiseLinear,
}

impl CarbAbsorptionModel {
    // ── PiecewiseLinear constants ─────────────────────────────────────────────
    const RISE_END: f64 = 0.15;
    const FALL_START: f64 = 0.5;
    // scale = 2 / (1 + fall_start - rise_end) = 2 / 1.35 ≈ 1.481
    const SCALE: f64 = 2.0 / (1.0 + Self::FALL_START - Self::RISE_END);

    /// Fraction absorbed at `percent_time` (0–1) through absorption.
    pub fn percent_absorption_at_percent_time(self, t: f64) -> f64 {
        if t <= 0.0 {
            return 0.0;
        }
        match self {
            Self::Linear => t.min(1.0),
            Self::PiecewiseLinear => {
                if t < Self::RISE_END {
                    0.5 * Self::SCALE * t * t / Self::RISE_END
                } else if t < Self::FALL_START {
                    Self::SCALE * (t - 0.5 * Self::RISE_END)
                } else if t < 1.0 {
                    Self::SCALE
                        * (Self::FALL_START - 0.5 * Self::RISE_END
                            + (t - Self::FALL_START)
                                * (1.0 - 0.5 * (t - Self::FALL_START) / (1.0 - Self::FALL_START)))
                } else {
                    1.0
                }
            }
        }
    }

    /// Inverse of `percent_absorption_at_percent_time`.
    pub fn percent_time_at_percent_absorption(self, a: f64) -> f64 {
        if a <= 0.0 {
            return 0.0;
        }
        match self {
            Self::Linear => a.min(1.0),
            Self::PiecewiseLinear => {
                let rise_threshold = 0.5 * Self::SCALE * Self::RISE_END;
                let mid_threshold = Self::SCALE * (Self::FALL_START - 0.5 * Self::RISE_END);
                if a < rise_threshold {
                    libm::sqrt(2.0 * Self::RISE_END * a / Self::SCALE)
                } else if a < mid_threshold {
                    0.5 * Self::RISE_END + a / Self::SCALE
                } else if a < 1.0 {
                    1.0 - libm::sqrt(
                        (1.0 - Self::FALL_START)
                            * (1.0 + Self::FALL_START - Self::RISE_END)
                            * (1.0 - a),
                    )
                } else {
                    1.0
                }
            }
        }
    }

    /// Normalised instantaneous absorption rate at `percent_time`.
    pub fn percent_rate_at_percent_time(self, t: f64) -> f64 {
        match self {
            Self::Linear => {
                if t > 0.0 && t <= 1.0 {
                    1.0
                } else {
                    0.0
                }
            }
            Self::PiecewiseLinear => {
                if t > 0.0 && t < Self::RISE_END {
                    Self::SCALE * t / Self::RISE_END
                } else if t >= Self::RISE_END && t < Self::FALL_START {
                    Self::SCALE
                } else if t >= Self::FALL_START && t < 1.0 {
                    Self::SCALE * (1.0 - t) / (1.0 - Self::FALL_START)
                } else {
                    0.0
                }
            }
        }
    }

    pub fn absorbed_carbs(self, total: f64, time: f64, absorption_time: f64) -> f64 {
        total * self.percent_absorption_at_percent_time(time / absorption_time)
    }

    pub fn unabsorbed_carbs(self, total: f64, time: f64, absorption_time: f64) -> f64 {
        total * (1.0 - self.percent_absorption_at_percent_time(time / absorption_time))
    }

    pub fn absorption_time_for_percent(self, percent: f64, elapsed: f64) -> f64 {
        let pt = self
            .percent_time_at_percent_absorption(percent)
            .max(f64::EPSILON);
        elapsed / pt
    }

    pub fn time_to_absorb(self, percent: f64, total_time: f64) -> f64 {
        self.percent_time_at_percent_absorption(percent) * total_time
    }
}

// ── Carb entry ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct CarbEntry {
    pub start: Timestamp,
    pub grams: f64,
    /// None = use `DEFAULT_ABSORPTION_TIME`.
    pub absorption_time: Option<f64>,
}

// ── Absorbed carb value ───────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct AbsorbedCarbValue {
    pub observed_g: f64,
    pub clamped_g: f64,
    pub total_g: f64,
    pub remaining_g: f64,
    pub observed_start: Timestamp,
    pub observed_end: Timestamp,
    pub estimated_time_remaining: f64,
    pub time_to_absorb_observed: f64,
}

impl AbsorbedCarbValue {
    /// Predicted end of absorption.
    pub fn estimated_end(&self) -> Timestamp {
        self.observed_start
            + (self.observed_end - self.observed_start)
            + self.estimated_time_remaining
    }

    pub fn estimated_absorption_time(&self) -> f64 {
        self.estimated_end() - self.observed_start
    }
}

// ── CarbValue timeline element ────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct CarbTimelineValue {
    pub start: Timestamp,
    pub end: Timestamp,
    pub value_g: f64,
}

// ── CarbStatus ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct CarbStatus {
    pub entry: CarbEntry,
    pub absorption: Option<AbsorbedCarbValue>,
    /// Observed absorption timeline; `None` if below modeled minimum.
    pub observed_timeline: Option<Vec<CarbTimelineValue>>,
}

impl CarbStatus {
    /// Effective absorption time (uses estimated if available, otherwise entry's).
    pub fn absorption_time(&self) -> f64 {
        self.absorption
            .as_ref()
            .map(|a| a.estimated_absorption_time())
            .unwrap_or_else(|| {
                self.entry
                    .absorption_time
                    .unwrap_or(DEFAULT_ABSORPTION_TIME)
            })
    }

    /// Dynamic carbs on board at `date`.
    pub fn dynamic_cob(
        &self,
        date: Timestamp,
        default_absorption_time: f64,
        delay: f64,
        delta: f64,
        model: CarbAbsorptionModel,
    ) -> f64 {
        if date < self.entry.start - delta {
            return 0.0;
        }
        let Some(abs) = &self.absorption else {
            return model.unabsorbed_carbs(
                self.entry.grams,
                date - self.entry.start - delay,
                default_absorption_time,
            );
        };

        let Some(timeline) = &self.observed_timeline else {
            // Below minimum observed — use modeled
            let time = date - self.entry.start - delay;
            let at = abs.estimated_absorption_time();
            return model.unabsorbed_carbs(abs.total_g, time, at);
        };

        let obs_end = match timeline.last() {
            Some(v) => v.end,
            // Empty observed timeline — no data yet, fall back to modeled
            None => {
                let time = date - self.entry.start - delay;
                let at = abs.estimated_absorption_time();
                return model.unabsorbed_carbs(abs.total_g, time, at);
            }
        };

        if date > obs_end {
            // Post-observation prediction
            let eff_time = date - obs_end + abs.time_to_absorb_observed;
            let eff_at = abs.time_to_absorb_observed + abs.estimated_time_remaining;
            model
                .unabsorbed_carbs(abs.total_g, eff_time, eff_at)
                .max(0.0)
        } else {
            // Within observed window
            let absorbed: f64 = timeline
                .iter()
                .filter(|v| v.end <= date)
                .map(|v| v.value_g)
                .sum();
            (self.entry.grams - absorbed).max(0.0)
        }
    }

    /// Dynamic absorbed carbs at `date`.
    pub fn dynamic_absorbed(
        &self,
        date: Timestamp,
        absorption_time: f64,
        delay: f64,
        delta: f64,
        model: CarbAbsorptionModel,
    ) -> f64 {
        if date < self.entry.start {
            return 0.0;
        }
        let Some(abs) = &self.absorption else {
            return model.absorbed_carbs(
                self.entry.grams,
                date - self.entry.start - delay,
                absorption_time,
            );
        };

        let Some(timeline) = &self.observed_timeline else {
            let time = date - self.entry.start - delay;
            let at = abs.estimated_absorption_time();
            return model.absorbed_carbs(abs.total_g, time, at);
        };

        let obs_end = match timeline.last() {
            Some(v) => v.end,
            // Empty observed timeline — no data yet, fall back to modeled
            None => {
                let time = date - self.entry.start - delay;
                let at = abs.estimated_absorption_time();
                return model.absorbed_carbs(abs.total_g, time, at);
            }
        };

        if date > obs_end {
            let eff_time = date - obs_end + abs.time_to_absorb_observed;
            let eff_at = abs.time_to_absorb_observed + abs.estimated_time_remaining;
            model
                .absorbed_carbs(abs.total_g, eff_time, eff_at)
                .min(abs.total_g)
        } else {
            // Sum observed values up to `date`
            let mut before: Vec<&CarbTimelineValue> = timeline
                .iter()
                .filter(|v| v.start + delta <= date)
                .collect();
            let mut sum = 0.0;
            if let Some(last) = before.pop() {
                let obs_dur = last.end - last.start;
                if obs_dur > 0.0 {
                    let clipped_end = date.min(last.end);
                    let calc_dur = (clipped_end - last.start).max(0.0).min(obs_dur);
                    sum += calc_dur / obs_dur * last.value_g;
                }
            }
            sum += before.iter().map(|v| v.value_g).sum::<f64>();
            sum.min(self.entry.grams)
        }
    }
}

// ── CarbStatusBuilder ─────────────────────────────────────────────────────────

struct CarbStatusBuilder {
    entry: CarbEntry,
    csf: f64, // carbohydrate sensitivity factor (mg/dL / g)
    entry_grams: f64,
    entry_effect: f64, // total expected glucose effect (mg/dL)
    initial_absorption_time: f64,
    max_absorption_time: f64,
    delay: f64,
    last_effect_date: Timestamp,
    max_end_date: Timestamp,
    model: CarbAbsorptionModel,
    adaptive: bool,
    adaptive_standby_fraction: f64,
    // mutable state
    observed_effect: f64,
    observed_timeline: Vec<CarbTimelineValue>,
    observed_completion_date: Option<Timestamp>,
}

impl CarbStatusBuilder {
    fn new(
        entry: CarbEntry,
        csf: f64,
        initial_absorption_time: f64,
        max_absorption_time: f64,
        delay: f64,
        last_effect_date: Option<Timestamp>,
        model: CarbAbsorptionModel,
        adaptive: bool,
        adaptive_standby_fraction: f64,
        initial_observed_effect: f64,
    ) -> Self {
        let entry_grams = entry.grams;
        let entry_effect = entry_grams * csf;
        let max_end_date = entry.start + max_absorption_time + delay;
        let last_effect_date = max_end_date.min(
            last_effect_date
                .map(|d| d.max(entry.start))
                .unwrap_or(entry.start),
        );
        Self {
            entry,
            csf,
            entry_grams,
            entry_effect,
            initial_absorption_time,
            max_absorption_time,
            delay,
            last_effect_date,
            max_end_date,
            model,
            adaptive,
            adaptive_standby_fraction,
            observed_effect: initial_observed_effect,
            observed_timeline: Vec::new(),
            observed_completion_date: None,
        }
    }

    fn min_predicted_grams(&self) -> f64 {
        let time = self.last_effect_date - self.entry.start - self.delay;
        self.model
            .absorbed_carbs(self.entry_grams, time, self.max_absorption_time)
    }

    fn observed_grams(&self) -> f64 {
        self.observed_effect / self.csf
    }

    fn clamped_grams(&self) -> f64 {
        self.entry_grams
            .min(self.min_predicted_grams().max(self.observed_grams()))
    }

    fn percent_absorbed(&self) -> f64 {
        self.clamped_grams() / self.entry_grams
    }

    fn adaptive_standby_interval(&self) -> f64 {
        self.initial_absorption_time * self.adaptive_standby_fraction
    }

    fn time_to_absorb_observed(&self) -> f64 {
        let time = self.last_effect_date - self.entry.start - self.delay;
        if time <= 0.0 {
            return 0.0;
        }
        let t = if self.adaptive && time > self.adaptive_standby_interval() {
            time
        } else {
            self.model
                .time_to_absorb(self.percent_absorbed(), self.initial_absorption_time)
        };
        t.min(self.max_absorption_time)
    }

    fn estimated_time_remaining(&self) -> f64 {
        let time = self.last_effect_date - self.entry.start - self.delay;
        if time <= 0.0 {
            return self.initial_absorption_time;
        }
        let max_remaining = (self.max_absorption_time - time).max(0.0);
        if max_remaining == 0.0 {
            return 0.0;
        }
        let dtr = if self.adaptive && time > self.adaptive_standby_interval() {
            let dyn_at = self
                .model
                .absorption_time_for_percent(self.percent_absorbed(), time);
            dyn_at - time
        } else {
            self.initial_absorption_time - self.time_to_absorb_observed()
        };
        dtr.min(max_remaining)
    }

    fn add_effect(&mut self, effect: f64, start: Timestamp, end: Timestamp) {
        if start < self.entry.start {
            return;
        }
        self.observed_effect += effect;
        if self.observed_completion_date.is_none() {
            self.observed_timeline.push(CarbTimelineValue {
                start,
                end,
                value_g: effect / self.csf,
            });
            // Mark completion when 100% observed (with float tolerance)
            if self.observed_effect + f32::EPSILON as f64 >= self.entry_effect {
                self.observed_completion_date = Some(end);
            }
        }
    }

    fn absorption_rate_at(&self, t: f64) -> f64 {
        let obs_dur = self
            .observed_completion_date
            .map(|d| d - self.entry.start)
            .unwrap_or(self.last_effect_date - self.entry.start);
        let dyn_at = (obs_dur + self.estimated_time_remaining()).min(self.max_absorption_time);
        if dyn_at <= 0.0 {
            return 0.0;
        }
        let percent_t = t / dyn_at;
        let avg_rate = self.entry_grams / dyn_at;
        avg_rate * self.model.percent_rate_at_percent_time(percent_t)
    }

    fn build(self) -> CarbStatus {
        let obs_dates_start = self.entry.start;
        let obs_dates_end = self
            .observed_completion_date
            .unwrap_or(self.last_effect_date);

        let clamped = self.clamped_grams();
        let absorption = AbsorbedCarbValue {
            observed_g: self.observed_grams(),
            clamped_g: clamped,
            total_g: self.entry_grams,
            remaining_g: (self.entry_grams - clamped).max(0.0),
            observed_start: obs_dates_start,
            observed_end: obs_dates_end,
            estimated_time_remaining: self.estimated_time_remaining(),
            time_to_absorb_observed: self.time_to_absorb_observed(),
        };
        let observed_timeline = if self.observed_grams() >= self.min_predicted_grams() {
            Some(self.observed_timeline)
        } else {
            None
        };
        CarbStatus {
            entry: self.entry,
            absorption: Some(absorption),
            observed_timeline,
        }
    }
}

// ── Public API: carb entry → CarbStatus ───────────────────────────────────────

/// Map carb entries to `CarbStatus` using observed counteraction effects.
pub fn map_carb_entries(
    entries: &[CarbEntry],
    effect_velocities: &[GlucoseEffectVelocity],
    carb_ratio_schedule: &[ScheduleEntry<f64>], // g/IU
    isf_schedule: &[ScheduleEntry<f64>],        // mg/dL/IU
    absorption_time_overrun: f64,
    default_absorption_time: f64,
    delay: f64,
    initial_overrun: f64,
    model: CarbAbsorptionModel,
    adaptive: bool,
    adaptive_standby_fraction: f64,
) -> Vec<CarbStatus> {
    if entries.is_empty() {
        return vec![];
    }

    let last_effect_date = effect_velocities.last().map(|v| v.end);

    let mut builders: Vec<CarbStatusBuilder> = entries
        .iter()
        .map(|entry| {
            let cr = closest_prior(carb_ratio_schedule, entry.start)
                .expect("Carb ratio schedule must cover all carb entry dates")
                .value;
            let isf = closest_prior(isf_schedule, entry.start)
                .expect("ISF schedule must cover all carb entry dates")
                .value;
            let csf = isf / cr; // mg/dL per gram
            let abs_time = entry.absorption_time.unwrap_or(default_absorption_time);
            CarbStatusBuilder::new(
                entry.clone(),
                csf,
                abs_time * initial_overrun,
                abs_time * absorption_time_overrun,
                delay,
                last_effect_date,
                model,
                adaptive,
                adaptive_standby_fraction,
                0.0,
            )
        })
        .collect();

    for vel in effect_velocities {
        if vel.end <= vel.start {
            continue;
        }

        let mut effect_value = vel.integrated_mgdl().max(0.0);

        // Active builder indices (whose window overlaps this velocity)
        let active_indices: Vec<usize> = builders
            .iter()
            .enumerate()
            .filter(|(_, b)| vel.start < b.max_end_date && vel.start >= b.entry.start)
            .map(|(i, _)| i)
            .collect();

        if active_indices.is_empty() {
            continue;
        }

        // Sum absorption rates to split the effect proportionally
        let total_rate: f64 = active_indices
            .iter()
            .map(|&i| {
                let t = vel.start - builders[i].entry.start;
                builders[i].absorption_rate_at(t)
            })
            .sum();

        let mut running_total_rate = total_rate;
        let last_active = *active_indices.last().unwrap();

        for &idx in &active_indices {
            let t = vel.start - builders[idx].entry.start;
            let rate = builders[idx].absorption_rate_at(t);
            let partial = if running_total_rate > 0.0 {
                let p = (rate / running_total_rate * effect_value)
                    .min(builders[idx].remaining_effect());
                running_total_rate -= rate;
                effect_value -= p;
                p
            } else {
                0.0
            };

            builders[idx].add_effect(partial, vel.start, vel.end);

            // Assign leftover to last active builder
            if effect_value > f32::EPSILON as f64 && idx == last_active {
                builders[idx].add_effect(effect_value, vel.start, vel.end);
            }
        }
    }

    builders.into_iter().map(|b| b.build()).collect()
}

// Need remaining_effect on builder (moved out to allow borrow)
impl CarbStatusBuilder {
    fn remaining_effect(&self) -> f64 {
        (self.entry_effect - self.observed_effect).max(0.0)
    }
}

// ── Dynamic COB and carb glucose effects ──────────────────────────────────────

fn carb_simulation_date_range(
    entries: &[CarbStatus],
    from: Option<Timestamp>,
    to: Option<Timestamp>,
    delay: f64,
    delta: f64,
) -> Option<(Timestamp, Timestamp)> {
    if entries.is_empty() {
        return None;
    }
    match (from, to) {
        (Some(s), Some(e)) => Some((floor_to(s, delta), crate::types::ceil_to(e, delta))),
        _ => {
            let min_start = entries
                .iter()
                .map(|e| e.entry.start)
                .fold(f64::INFINITY, f64::min);
            let max_end = entries
                .iter()
                .map(|s| s.entry.start + s.absorption_time() + delay)
                .fold(f64::NEG_INFINITY, f64::max);
            Some((
                floor_to(from.unwrap_or(min_start), delta),
                crate::types::ceil_to(to.unwrap_or(max_end), delta),
            ))
        }
    }
}

/// Dynamic carbs on board at a single point in time.
pub fn dynamic_cob_at(statuses: &[CarbStatus], at: Timestamp, model: CarbAbsorptionModel) -> f64 {
    statuses
        .iter()
        .map(|s| {
            s.dynamic_cob(
                at,
                DEFAULT_ABSORPTION_TIME,
                DEFAULT_EFFECT_DELAY,
                DELTA,
                model,
            )
        })
        .sum()
}

/// Dynamic glucose effects from carbs (cumulative mg/dL timeline).
pub fn dynamic_glucose_effects(
    statuses: &[CarbStatus],
    from: Option<Timestamp>,
    to: Option<Timestamp>,
    carb_ratio_schedule: &[ScheduleEntry<f64>],
    isf_schedule: &[ScheduleEntry<f64>],
    model: CarbAbsorptionModel,
    delay: f64,
    delta: f64,
) -> Vec<GlucoseEffect> {
    let Some((start, end)) = carb_simulation_date_range(statuses, from, to, delay, delta) else {
        return vec![];
    };

    let mut t = start;
    let mut out = Vec::new();
    while t <= end {
        let value: f64 = statuses
            .iter()
            .map(|s| {
                let isf = closest_prior(isf_schedule, s.entry.start)
                    .expect("ISF must cover carb entry dates")
                    .value;
                let cr = closest_prior(carb_ratio_schedule, s.entry.start)
                    .expect("CR must cover carb entry dates")
                    .value;
                let csf = isf / cr;
                let at = s.absorption_time();
                csf * s.dynamic_absorbed(t, at, delay, delta, model)
            })
            .sum();
        out.push(GlucoseEffect {
            start: t,
            value_mgdl: value,
        });
        t += delta;
    }
    out
}
