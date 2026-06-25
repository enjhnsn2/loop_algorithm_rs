#![flux::no_panic]

use crate::types::{closest_prior, ScheduleEntry, Timestamp};

// ── Recommendation types ──────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Default)]
pub enum DoseRecommendationType {
    ManualBolus,
    AutomaticBolus,
    #[default]
    TempBasal,
}

impl DoseRecommendationType {
    pub fn is_automated(self) -> bool {
        matches!(self, Self::AutomaticBolus | Self::TempBasal)
    }
}

#[derive(Clone, Debug)]
pub struct TempBasalRecommendation {
    pub units_per_hour: f64,
    pub duration: f64, // seconds
}

#[derive(Clone, Copy, Debug)]
pub enum AutoDirection {
    Decrease,
    Neutral,
    Increase,
}

#[derive(Clone, Debug)]
pub struct AutomaticDoseRecommendation {
    pub basal_adjustment: TempBasalRecommendation,
    pub bolus_units: Option<f64>,
    pub direction: AutoDirection,
}

#[derive(Clone, Debug)]
pub enum BolusRecommendationNotice {
    GlucoseBelowSuspendThreshold {
        min_glucose_mgdl: f64,
        at: Timestamp,
    },
    CurrentGlucoseBelowTarget {
        glucose_mgdl: f64,
        at: Timestamp,
    },
    PredictedGlucoseBelowTarget {
        min_glucose_mgdl: f64,
        at: Timestamp,
    },
    PredictedGlucoseInRange,
    AllGlucoseBelowTarget {
        min_glucose_mgdl: f64,
        at: Timestamp,
    },
}

#[derive(Clone, Debug)]
pub struct ManualBolusRecommendation {
    pub amount_iu: f64,
    pub notice: Option<BolusRecommendationNotice>,
}

/// Combined recommendation (at most one of manual/automatic is populated).
#[derive(Clone, Debug)]
pub struct DoseRecommendation {
    pub manual: Option<ManualBolusRecommendation>,
    pub automatic: Option<AutomaticDoseRecommendation>,
}

impl DoseRecommendation {
    pub fn manual(rec: ManualBolusRecommendation) -> Self {
        Self {
            manual: Some(rec),
            automatic: None,
        }
    }
    pub fn automatic(rec: AutomaticDoseRecommendation) -> Self {
        Self {
            manual: None,
            automatic: Some(rec),
        }
    }
}

// ── Correction range (target) ─────────────────────────────────────────────────

/// `(min, max)` in mg/dL.
pub type GlucoseRange = (f64, f64);
pub type GlucoseRangeTimeline = Vec<ScheduleEntry<GlucoseRange>>;

fn range_average(r: GlucoseRange) -> f64 {
    (r.0 + r.1) / 2.0
}

// ── InsulinCorrection ─────────────────────────────────────────────────────────

/// Summary of the glucose correction needed, with associated insulin amount.
#[derive(Clone, Debug)]
pub enum InsulinCorrection {
    InRange,
    AboveRange {
        min_glucose: (Timestamp, f64),
        correcting_glucose: (Timestamp, f64),
        min_target_mgdl: f64,
        units: f64,
    },
    EntirelyBelowRange {
        min_glucose: (Timestamp, f64),
        min_target_mgdl: f64,
        units: f64,
    },
    Suspend {
        min_glucose: (Timestamp, f64),
    },
}

impl InsulinCorrection {
    fn units(&self) -> f64 {
        match self {
            Self::AboveRange { units, .. } | Self::EntirelyBelowRange { units, .. } => *units,
            _ => 0.0,
        }
    }

    /// Convert correction to a 30-minute temp basal recommendation.
    pub fn as_temp_basal(
        &self,
        neutral_rate: f64,
        max_rate: f64,
        duration: f64,
    ) -> TempBasalRecommendation {
        let mut rate = self.units() / (duration / 3600.0);
        match self {
            Self::AboveRange { .. } | Self::InRange | Self::EntirelyBelowRange { .. } => {
                rate += neutral_rate;
            }
            Self::Suspend { .. } => {}
        }
        rate = rate.clamp(0.0, max_rate);
        TempBasalRecommendation {
            units_per_hour: rate,
            duration,
        }
    }

    /// Partial bolus amount.
    pub fn as_partial_bolus(&self, factor: f64, max_units: f64) -> f64 {
        let partial = self.units() * factor;
        partial.clamp(0.0, max_units)
    }

    /// Manual bolus recommendation.
    pub fn as_manual_bolus(&self, max_units: f64) -> ManualBolusRecommendation {
        let notice = self.bolus_notice();
        ManualBolusRecommendation {
            amount_iu: self.units().clamp(0.0, max_units),
            notice,
        }
    }

    fn bolus_notice(&self) -> Option<BolusRecommendationNotice> {
        match self {
            Self::Suspend {
                min_glucose: (at, v),
            } => Some(BolusRecommendationNotice::GlucoseBelowSuspendThreshold {
                min_glucose_mgdl: *v,
                at: *at,
            }),
            Self::InRange => Some(BolusRecommendationNotice::PredictedGlucoseInRange),
            Self::EntirelyBelowRange {
                min_glucose: (at, v),
                ..
            } => Some(BolusRecommendationNotice::AllGlucoseBelowTarget {
                min_glucose_mgdl: *v,
                at: *at,
            }),
            Self::AboveRange {
                min_glucose: (at, v),
                min_target_mgdl,
                units,
                ..
            } => {
                if *units > 0.0 && v < min_target_mgdl {
                    Some(BolusRecommendationNotice::PredictedGlucoseBelowTarget {
                        min_glucose_mgdl: *v,
                        at: *at,
                    })
                } else {
                    None
                }
            }
        }
    }

    pub fn direction(&self) -> AutoDirection {
        match self {
            Self::InRange => AutoDirection::Neutral,
            Self::AboveRange { .. } => AutoDirection::Increase,
            Self::EntirelyBelowRange { .. } | Self::Suspend { .. } => AutoDirection::Decrease,
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn insulin_correction_units(from: f64, to: f64, sensitivity: f64) -> f64 {
    debug_assert!(sensitivity > 0.0, "Negative sensitivity: {sensitivity}");
    (from - to) / sensitivity
}

/// Target glucose at `percent_duration` through effect window; blends toward range center
/// after the 50% mark.
fn target_glucose_value(percent_duration: f64, suspend_threshold: f64, target_avg: f64) -> f64 {
    const INFLECTION: f64 = 0.5;
    if percent_duration <= INFLECTION {
        return suspend_threshold;
    }
    if percent_duration >= 1.0 {
        return target_avg;
    }
    let slope = (target_avg - suspend_threshold) / (1.0 - INFLECTION);
    suspend_threshold + slope * (percent_duration - INFLECTION)
}

// ── Main correction calculation ───────────────────────────────────────────────

use crate::insulin::ExponentialInsulinModel;

/// Determine the minimum insulin correction needed to bring predicted glucose into range.
///
/// `predicted` must extend at least `model.effect_duration()` beyond `delivery_date`.
#[flux_rs::trusted(reason = "ICE - Impossible case reached")]
pub fn insulin_correction(
    predicted: &[(Timestamp, f64)], // (time, mg/dL)
    delivery_date: Timestamp,
    target: &GlucoseRangeTimeline,
    suspend_threshold_mgdl: f64,
    isf_schedule: &[ScheduleEntry<f64>], // mg/dL/IU
    model: &ExponentialInsulinModel,
) -> InsulinCorrection {
    assert!(!predicted.is_empty(), "Empty glucose prediction");

    let end_of_absorption = delivery_date + model.effect_duration();
    let target_at_delivery =
        closest_prior(target, delivery_date).expect("Target must cover delivery date");

    let mut min_glucose: Option<(Timestamp, f64)> = None;
    let mut eventual_glucose: Option<(Timestamp, f64)> = None;
    let mut correcting_glucose: Option<(Timestamp, f64)> = None;
    let mut min_correction_units: Option<f64> = None;
    let mut sensitivity_at_min: Option<f64> = None;

    for &(t, g) in predicted {
        if t < delivery_date {
            continue;
        }

        // Suspend if any predicted value falls below threshold
        if g < suspend_threshold_mgdl {
            return InsulinCorrection::Suspend {
                min_glucose: (t, g),
            };
        }

        eventual_glucose = Some((t, g));

        let time = t - delivery_date;
        let target_avg = range_average(target_at_delivery.value);
        let target_val = target_glucose_value(
            time / model.effect_duration(),
            suspend_threshold_mgdl,
            target_avg,
        );

        // Effective sensitivity = sum of (percent_effect_delta * ISF) over ISF segments
        let isf_segments = isf_schedule
            .iter()
            .filter(|s| s.end >= delivery_date && s.start <= t);

        let mut isf_end: Option<f64> = None;
        let effected_sensitivity: f64 = isf_segments
            .map(|seg| {
                let seg_start = (delivery_date.max(seg.start) - delivery_date).max(0.0);
                let seg_end = (t.min(seg.end) - delivery_date).max(0.0);
                let pe = model.percent_effect_remaining(seg_start)
                    - model.percent_effect_remaining(seg_end);
                isf_end = Some(seg_end);
                pe * seg.value
            })
            .sum();

        // Ensure ISF covered the entire prediction window
        assert!(
            isf_end.map_or(false, |e| e >= t - delivery_date),
            "ISF schedule must cover prediction window up to {t}"
        );

        // Track minimum glucose
        if min_glucose.map_or(true, |(_, mv)| g < mv) {
            min_glucose = Some((t, g));
            sensitivity_at_min = Some(effected_sensitivity);
        }

        let correction_units =
            insulin_correction_units(g, target_val, effected_sensitivity.max(f64::EPSILON));
        if correction_units > 0.0 && min_correction_units.map_or(true, |m| correction_units < m) {
            correcting_glucose = Some((t, g));
            min_correction_units = Some(correction_units);
        }

        if t >= end_of_absorption {
            break;
        }
    }

    let (min_t, min_g) = min_glucose.expect("No predictions after delivery date");
    let (ev_t, ev_g) = eventual_glucose.expect("No predictions after delivery date");

    let min_target = closest_prior(target, min_t).expect("Target must cover min glucose time");
    let ev_target = closest_prior(target, ev_t).expect("Target must cover eventual glucose time");

    if min_g < min_target.value.0 && ev_g < ev_target.value.0 {
        // Both min and eventual are below range → negative correction
        let units = insulin_correction_units(
            min_g,
            range_average(min_target.value),
            sensitivity_at_min.unwrap_or(f64::EPSILON).max(f64::EPSILON),
        );
        return InsulinCorrection::EntirelyBelowRange {
            min_glucose: (min_t, min_g),
            min_target_mgdl: min_target.value.0,
            units,
        };
    }

    if ev_g > ev_target.value.1 {
        if let (Some(units), Some(corr)) = (min_correction_units, correcting_glucose) {
            return InsulinCorrection::AboveRange {
                min_glucose: (min_t, min_g),
                correcting_glucose: corr,
                min_target_mgdl: ev_target.value.0,
                units,
            };
        }
    }

    InsulinCorrection::InRange
}

// ── Dose recommendation helpers ───────────────────────────────────────────────

pub const TEMP_BASAL_DURATION: f64 = 30.0 * 60.0; // 30 minutes
pub const DEFAULT_BOLUS_FACTOR: f64 = 0.4;

pub fn recommend_temp_basal(
    correction: &InsulinCorrection,
    neutral_rate: f64,
    active_insulin: f64,
    _max_bolus: f64,
    max_basal: f64,
    max_active_insulin: f64,
) -> TempBasalRecommendation {
    let mut effective_max = max_basal;

    // If min glucose < high-basal threshold, cap to neutral rate
    if let InsulinCorrection::AboveRange {
        min_glucose: (_, min_g),
        min_target_mgdl,
        ..
    } = correction
    {
        if *min_g < *min_target_mgdl {
            effective_max = neutral_rate;
        }
    }

    // Enforce active-insulin cap
    let headroom = max_active_insulin - active_insulin;
    let iob_limited_rate = headroom * (3600.0 / TEMP_BASAL_DURATION) + neutral_rate;
    effective_max = effective_max.min(iob_limited_rate);

    correction.as_temp_basal(neutral_rate, effective_max, TEMP_BASAL_DURATION)
}

pub fn recommend_automatic_dose(
    correction: &InsulinCorrection,
    application_factor: f64,
    neutral_rate: f64,
    active_insulin: f64,
    max_bolus: f64,
    max_active_insulin: f64,
) -> AutomaticDoseRecommendation {
    let headroom = (max_active_insulin - active_insulin).max(0.0);
    let mut delivery_max = (max_bolus * application_factor).min(headroom);

    // If min glucose < target, don't deliver additional bolus
    if let InsulinCorrection::AboveRange {
        min_glucose: (_, min_g),
        min_target_mgdl,
        ..
    } = correction
    {
        if *min_g < *min_target_mgdl {
            delivery_max = 0.0;
        }
    }

    // Low temp basal (capped at neutral) for downward adjustments
    let temp = correction.as_temp_basal(neutral_rate, neutral_rate, TEMP_BASAL_DURATION);
    let bolus = correction.as_partial_bolus(application_factor, delivery_max);

    AutomaticDoseRecommendation {
        basal_adjustment: temp,
        bolus_units: if bolus > 0.0 { Some(bolus) } else { None },
        direction: correction.direction(),
    }
}

pub fn recommend_manual_bolus(
    correction: &InsulinCorrection,
    max_bolus: f64,
    current_glucose_time: Timestamp,
    current_glucose_mgdl: f64,
    target: &GlucoseRangeTimeline,
) -> ManualBolusRecommendation {
    let mut rec = correction.as_manual_bolus(max_bolus);

    if let Some(target_entry) = closest_prior(target, current_glucose_time) {
        if current_glucose_mgdl < target_entry.value.0 {
            rec.notice = Some(BolusRecommendationNotice::CurrentGlucoseBelowTarget {
                glucose_mgdl: current_glucose_mgdl,
                at: current_glucose_time,
            });
        }
    }

    rec
}
