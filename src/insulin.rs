#![flux::no_panic]
use crate::types::{
    closest_prior, filter_date_range, floor_to, simulation_date_range, GlucoseEffect, InsulinValue,
    ScheduleEntry, Timestamp,
};

// ── Constants ─────────────────────────────────────────────────────────────────

pub const DEFAULT_INSULIN_ACTIVITY_DURATION: f64 = 6.0 * 3600.0 + 10.0 * 60.0; // 6h 10m
pub const DELTA: f64 = 300.0; // 5 minutes in seconds

// ── Exponential insulin model ─────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct ExponentialInsulinModel {
    pub action_duration: f64,    // seconds (excluding delay)
    pub peak_activity_time: f64, // seconds
    pub delay: f64,              // seconds
    // Precomputed
    tau: f64,
    a: f64,
    s: f64,
}

impl ExponentialInsulinModel {
    pub fn new(action_duration: f64, peak_activity_time: f64, delay: f64) -> Self {
        let tau = peak_activity_time * (1.0 - peak_activity_time / action_duration)
            / (1.0 - 2.0 * peak_activity_time / action_duration);
        let a = 2.0 * tau / action_duration;
        let s = 1.0 / (1.0 - a + (1.0 + a) * (-action_duration / tau).exp());
        Self {
            action_duration,
            peak_activity_time,
            delay,
            tau,
            a,
            s,
        }
    }

    /// Fraction of total insulin effect remaining at `time` seconds after delivery.
    /// Returns a value in [0, 1].
    pub fn percent_effect_remaining(&self, time: f64) -> f64 {
        let t = time - self.delay;
        if t <= 0.0 {
            return 1.0;
        }
        if t >= self.action_duration {
            return 0.0;
        }
        1.0 - self.s
            * (1.0 - self.a)
            * ((t * t / (self.tau * self.action_duration * (1.0 - self.a)) - t / self.tau - 1.0)
                * (-t / self.tau).exp()
                + 1.0)
    }

    /// Total duration including delay.
    pub fn effect_duration(&self) -> f64 {
        self.action_duration + self.delay
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ModelPreset {
    RapidActingAdult,
    RapidActingChild,
    Fiasp,
    Lyumjev,
    Afrezza,
}

impl ModelPreset {
    pub fn model(self) -> ExponentialInsulinModel {
        let (dur, peak, delay) = match self {
            ModelPreset::RapidActingAdult => (360.0 * 60.0, 75.0 * 60.0, 10.0 * 60.0),
            ModelPreset::RapidActingChild => (360.0 * 60.0, 65.0 * 60.0, 10.0 * 60.0),
            ModelPreset::Fiasp => (360.0 * 60.0, 55.0 * 60.0, 10.0 * 60.0),
            ModelPreset::Lyumjev => (360.0 * 60.0, 55.0 * 60.0, 10.0 * 60.0),
            ModelPreset::Afrezza => (300.0 * 60.0, 29.0 * 60.0, 10.0 * 60.0),
        };
        ExponentialInsulinModel::new(dur, peak, delay)
    }
}

// ── Dose types ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsulinDeliveryType {
    Bolus,
    Basal,
}

/// User-supplied insulin dose.
#[derive(Clone, Debug)]
pub struct InsulinDose {
    pub delivery_type: InsulinDeliveryType,
    pub start: Timestamp,
    pub end: Timestamp,
    pub volume_iu: f64,
    pub model: ExponentialInsulinModel,
}

impl InsulinDose {
    pub fn duration(&self) -> f64 {
        self.end - self.start
    }
}

/// Dose annotated with the scheduled basal rate for its time segment.
#[derive(Clone, Debug)]
pub struct BasalRelativeDose {
    pub dose_type: BasalDoseType,
    pub start: Timestamp,
    pub end: Timestamp,
    pub volume_iu: f64,
    pub model: ExponentialInsulinModel,
}

#[derive(Clone, Debug)]
pub enum BasalDoseType {
    Bolus,
    Basal { scheduled_rate_iuhr: f64 },
}

impl BasalRelativeDose {
    pub fn duration(&self) -> f64 {
        self.end - self.start
    }

    /// Insulin delivered above (+) or below (−) the scheduled basal.
    pub fn net_basal_units(&self) -> f64 {
        match self.dose_type {
            BasalDoseType::Bolus => self.volume_iu,
            BasalDoseType::Basal {
                scheduled_rate_iuhr,
            } => {
                let duration_hr = self.duration() / 3600.0;
                if duration_hr <= 0.0 {
                    return 0.0;
                }
                self.volume_iu - scheduled_rate_iuhr * duration_hr
            }
        }
    }

    // ── IOB helpers ───────────────────────────────────────────────────────────

    fn continuous_delivery_iob(&self, time: f64, delta: f64) -> f64 {
        let dose_duration = self.duration();
        let mut iob = 0.0;
        let mut dose_date = 0.0f64;
        loop {
            let segment = if dose_duration > 0.0 {
                ((dose_date + delta).min(dose_duration) - dose_date).max(0.0) / dose_duration
            } else {
                1.0
            };
            iob += segment * self.model.percent_effect_remaining(time - dose_date);
            dose_date += delta;
            let limit = floor_to(time + self.model.delay, delta).min(dose_duration);
            if dose_date > limit {
                break;
            }
        }
        iob
    }

    pub fn insulin_on_board(&self, at: Timestamp, delta: f64) -> f64 {
        let time = at - self.start;
        if time < 0.0 {
            return 0.0;
        }
        let net = self.net_basal_units();
        if self.duration() <= 1.05 * delta {
            net * self.model.percent_effect_remaining(time)
        } else {
            net * self.continuous_delivery_iob(time, delta)
        }
    }

    // ── Glucose effect helpers ────────────────────────────────────────────────

    fn continuous_delivery_percent_effect(&self, at: Timestamp, delta: f64) -> f64 {
        let dose_duration = self.duration();
        let time = at - self.start;
        let mut value = 0.0;
        let mut dose_date = 0.0f64;
        loop {
            let segment = if dose_duration > 0.0 {
                ((dose_date + delta).min(dose_duration) - dose_date).max(0.0) / dose_duration
            } else {
                1.0
            };
            value += segment * (1.0 - self.model.percent_effect_remaining(time - dose_date));
            dose_date += delta;
            let limit = floor_to(time + self.model.delay, delta).min(dose_duration);
            if dose_date > limit {
                break;
            }
        }
        value
    }

    /// Cumulative glucose effect at `at` given ISF in mg/dL/IU.
    pub fn glucose_effect(&self, at: Timestamp, isf: f64, delta: f64) -> f64 {
        let time = at - self.start;
        if time < 0.0 {
            return 0.0;
        }
        let net = self.net_basal_units();
        if self.duration() <= 1.05 * delta {
            net * -isf * (1.0 - self.model.percent_effect_remaining(time))
        } else {
            net * -isf * self.continuous_delivery_percent_effect(at, delta)
        }
    }

    /// Incremental glucose effect during `[interval_start, interval_end]`.
    pub fn glucose_effect_during(
        &self,
        interval_start: Timestamp,
        interval_end: Timestamp,
        isf: f64,
        delta: f64,
    ) -> f64 {
        let start_rel = interval_start - self.start;
        let end_rel = interval_end - self.start;
        if end_rel - start_rel < 0.0 {
            return 0.0;
        }
        let net = self.net_basal_units();
        let effect = if self.duration() <= 1.05 * delta {
            self.model.percent_effect_remaining(start_rel)
                - self.model.percent_effect_remaining(end_rel)
        } else {
            let start_remaining =
                1.0 - self.continuous_delivery_percent_effect(interval_start, delta);
            let end_remaining = 1.0 - self.continuous_delivery_percent_effect(interval_end, delta);
            start_remaining - end_remaining
        };
        net * -isf * effect
    }
}

// ── Annotation: doses → BasalRelativeDose ─────────────────────────────────────

fn annotate_basal_dose(
    dose: &InsulinDose,
    basal_history: &[ScheduleEntry<f64>],
) -> Vec<BasalRelativeDose> {
    let n = basal_history.len();
    let mut out = Vec::new();
    for (i, entry) in basal_history.iter().enumerate() {
        let seg_start = if i == 0 { dose.start } else { entry.start };
        let seg_end = if i == n - 1 {
            dose.end
        } else {
            basal_history[i + 1].start
        };
        let seg_start = seg_start.max(dose.start);
        let seg_end = seg_start.max(seg_end.min(dose.end));
        let seg_dur = seg_end - seg_start;
        let seg_vol = if dose.duration() > 0.0 {
            dose.volume_iu * (seg_dur / dose.duration())
        } else {
            0.0
        };
        out.push(BasalRelativeDose {
            dose_type: BasalDoseType::Basal {
                scheduled_rate_iuhr: entry.value,
            },
            start: seg_start,
            end: seg_end,
            volume_iu: seg_vol,
            model: dose.model.clone(),
        });
    }
    out
}

/// Annotate a slice of doses with basal history.  Basal doses are split across schedule
/// boundaries; boluses are wrapped as-is.
///
/// If `fill_basal_gaps` is true, gaps in the dose timeline are filled with scheduled basal.
pub fn annotated_doses(
    doses: &[InsulinDose],
    basal_history: &[ScheduleEntry<f64>],
    fill_basal_gaps: bool,
) -> Vec<BasalRelativeDose> {
    let mut out: Vec<BasalRelativeDose> = Vec::new();

    if !fill_basal_gaps {
        if doses.is_empty() {
            return out;
        }
        for dose in doses {
            if dose.delivery_type == InsulinDeliveryType::Basal {
                let items = filter_date_range(basal_history, Some(dose.start), Some(dose.end));
                let owned: Vec<ScheduleEntry<f64>> = items.into_iter().cloned().collect();
                out.extend(annotate_basal_dose(dose, &owned));
            } else {
                out.push(BasalRelativeDose {
                    dose_type: BasalDoseType::Bolus,
                    start: dose.start,
                    end: dose.end,
                    volume_iu: dose.volume_iu,
                    model: dose.model.clone(),
                });
            }
        }
        return out;
    }

    // fill_basal_gaps = true
    let basal_doses: Vec<&InsulinDose> = doses
        .iter()
        .filter(|d| d.delivery_type == InsulinDeliveryType::Basal)
        .collect();
    let first_date = [
        basal_history.first().map(|e| e.start),
        basal_doses.first().map(|d| d.start),
    ]
    .into_iter()
    .flatten()
    .reduce(f64::min);

    let Some(mut cursor) = first_date else {
        return out;
    };

    let fill_gap = |start: Timestamp, end: Timestamp, basal: &[ScheduleEntry<f64>]| {
        let trimmed = crate::types::trim_schedule(basal, Some(start), Some(end));
        trimmed
            .into_iter()
            .map(|e| BasalRelativeDose {
                dose_type: BasalDoseType::Basal {
                    scheduled_rate_iuhr: e.value,
                },
                start: e.start,
                end: e.end,
                volume_iu: e.value * (e.end - e.start) / 3600.0,
                model: ModelPreset::RapidActingAdult.model(),
            })
            .collect::<Vec<_>>()
    };

    for dose in doses {
        if dose.delivery_type != InsulinDeliveryType::Basal {
            out.push(BasalRelativeDose {
                dose_type: BasalDoseType::Bolus,
                start: dose.start,
                end: dose.end,
                volume_iu: dose.volume_iu,
                model: dose.model.clone(),
            });
            continue;
        }
        if cursor < dose.start {
            out.extend(fill_gap(cursor, dose.start, basal_history));
        }
        let items: Vec<ScheduleEntry<f64>> =
            filter_date_range(basal_history, Some(dose.start), Some(dose.end))
                .into_iter()
                .cloned()
                .collect();
        out.extend(annotate_basal_dose(dose, &items));
        cursor = dose.end;
    }

    let end_date = [
        basal_history.last().map(|e| e.end),
        basal_doses.last().map(|d| d.end),
    ]
    .into_iter()
    .flatten()
    .reduce(f64::max)
    .unwrap_or(cursor);

    if cursor < end_date {
        out.extend(fill_gap(cursor, end_date, basal_history));
    }

    out
}

// ── Insulin-on-board timeline ─────────────────────────────────────────────────

pub fn iob_timeline(
    doses: &[BasalRelativeDose],
    from: Option<Timestamp>,
    to: Option<Timestamp>,
    delta: f64,
) -> Vec<InsulinValue> {
    let starts: Vec<_> = doses.iter().map(|d| d.start).collect();
    let ends: Vec<_> = doses
        .iter()
        .map(|d| d.end + d.model.effect_duration())
        .collect();
    let Some((start, end)) = simulation_date_range(
        &starts,
        &ends,
        from,
        to,
        DEFAULT_INSULIN_ACTIVITY_DURATION,
        delta,
    ) else {
        return vec![];
    };
    let mut t = start;
    let mut out = Vec::new();
    while t <= end {
        let iob = doses.iter().map(|d| d.insulin_on_board(t, delta)).sum();
        out.push(InsulinValue {
            start: t,
            value_iu: iob,
        });
        t += delta;
    }
    out
}

pub fn iob_at(doses: &[BasalRelativeDose], at: Timestamp) -> f64 {
    doses.iter().map(|d| d.insulin_on_board(at, DELTA)).sum()
}

// ── Glucose-effect timeline (fixed ISF per dose) ──────────────────────────────
#[flux_rs::trusted(reason = "closest_prior")]
pub fn glucose_effects(
    doses: &[BasalRelativeDose],
    isf_schedule: &[ScheduleEntry<f64>], // mg/dL/IU
    from: Option<Timestamp>,
    to: Option<Timestamp>,
    delta: f64,
) -> Vec<GlucoseEffect> {
    let active: Vec<&BasalRelativeDose> = doses
        .iter()
        .filter(|d| d.net_basal_units() != 0.0)
        .collect();
    let starts: Vec<_> = active.iter().map(|d| d.start).collect();
    let ends: Vec<_> = active
        .iter()
        .map(|d| d.end + d.model.effect_duration())
        .collect();
    let Some((start, end)) = simulation_date_range(
        &starts,
        &ends,
        from,
        to,
        DEFAULT_INSULIN_ACTIVITY_DURATION,
        delta,
    ) else {
        return vec![];
    };

    let mut t = start;
    let mut out = Vec::new();
    while t <= end {
        let value: f64 = doses
            .iter()
            .map(|dose| {
                let isf = closest_prior(isf_schedule, dose.start)
                    .expect("ISF schedule must cover all dose start times")
                    .value;
                dose.glucose_effect(t, isf, delta)
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

// ── Glucose-effect timeline (mid-absorption ISF) ──────────────────────────────

pub fn glucose_effects_mid_absorption_isf(
    doses: &[BasalRelativeDose],
    isf_schedule: &[ScheduleEntry<f64>], // mg/dL/IU
    from: Option<Timestamp>,
    to: Option<Timestamp>,
    delta: f64,
) -> Vec<GlucoseEffect> {
    let active: Vec<&BasalRelativeDose> = doses
        .iter()
        .filter(|d| d.net_basal_units() != 0.0)
        .collect();
    let starts: Vec<_> = active.iter().map(|d| d.start).collect();
    let ends: Vec<_> = active
        .iter()
        .map(|d| d.end + d.model.effect_duration())
        .collect();
    let Some((start, end)) = simulation_date_range(
        &starts,
        &ends,
        from,
        to,
        DEFAULT_INSULIN_ACTIVITY_DURATION,
        delta,
    ) else {
        return vec![];
    };

    let mut last_t = start;
    let mut t = start;
    let mut cumulative = 0.0f64;
    let mut out = Vec::new();
    while t <= end {
        if t != last_t {
            let delta_val: f64 = doses
                .iter()
                .map(|dose| {
                    let segments = filter_date_range(isf_schedule, Some(last_t), Some(t));
                    segments
                        .iter()
                        .map(|seg| {
                            let seg_start = last_t.max(seg.start);
                            let seg_end = t.min(seg.end);
                            if seg_start < seg_end {
                                dose.glucose_effect_during(seg_start, seg_end, seg.value, delta)
                            } else {
                                0.0
                            }
                        })
                        .sum::<f64>()
                })
                .sum();
            cumulative += delta_val;
        }
        out.push(GlucoseEffect {
            start: t,
            value_mgdl: cumulative,
        });
        last_t = t;
        t += delta;
    }
    out
}

/// Effects-covering interval for a set of doses.
pub fn effects_interval(doses: &[BasalRelativeDose]) -> Option<(Timestamp, Timestamp)> {
    if doses.is_empty() {
        return None;
    }
    let min_start = doses.iter().map(|d| d.start).fold(f64::INFINITY, f64::min);
    let max_end = doses
        .iter()
        .map(|d| d.end + d.model.effect_duration())
        .fold(f64::NEG_INFINITY, f64::max);
    Some((min_start, max_end))
}
