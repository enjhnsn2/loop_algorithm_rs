use heapless::Vec;
use std::collections::BTreeMap;

use crate::{
    carbs::{
        dynamic_cob_at, dynamic_glucose_effects, map_carb_entries, CarbAbsorptionModel, CarbEntry,
        CarbStatus, DEFAULT_ABSORPTION_TIME, DEFAULT_EFFECT_DELAY,
    },
    dose::{
        insulin_correction, recommend_automatic_dose, recommend_manual_bolus, recommend_temp_basal,
        DoseRecommendation, DoseRecommendationType, GlucoseRangeTimeline,
    },
    glucose::{
        combined_sums, counteraction_effects, has_gradual_transitions, linear_momentum_effect,
        subtract_effects, GlucoseSample,
    },
    insulin::{
        annotated_doses, effects_interval, glucose_effects, glucose_effects_mid_absorption_isf,
        iob_at, BasalRelativeDose, ExponentialInsulinModel, InsulinDose,
        DEFAULT_INSULIN_ACTIVITY_DURATION, DELTA,
    },
    retrospective::{integral_rc_effect, standard_rc_effect, RCMode, RETROSPECTION_INTERVAL},
    types::{
        ceil_to, closest_prior, GlucoseChange, GlucoseEffect, GlucoseEffectVelocity,
        PredictedGlucose, ScheduleEntry, Timestamp,
    },
};

// ── Constants ─────────────────────────────────────────────────────────────────

pub const INPUT_DATA_RECENCY: f64 = 15.0 * 60.0; // 15 min
pub const DEFAULT_MAX_ACTIVE_INSULIN_MULTIPLIER: f64 = 2.0;
pub const RETROSPECTIVE_GROUPING_INTERVAL: f64 = 30.0 * 60.0; // 30 min
pub const RETROSPECTIVE_EFFECT_DURATION: f64 = 60.0 * 60.0; // 1 hour
pub const MOMENTUM_DATA_INTERVAL: f64 = 15.0 * 60.0;
pub const DEFAULT_GRADUAL_TRANSITION_THRESHOLD: f64 = 40.0;

// -- Vec capacities -------------------------------------------------------------------------------
pub const MAX_GLUCOSE_SAMPLES: usize = libm::ceil((10.0 * 60.0 * 60.0) / DELTA); // 10 hours of sensor data
pub const MAX_DOSES: usize = libm::ceil((16.0 * 60.0 * 60.0) / DELTA); // 16 hours of dose data
pub const MAX_CARB_ENTRIES: usize = 50; // 10 hours of carb entries
pub const MAX_SCHEDULE_ENTRIES: usize = 48; // daily profile entries (30-min granularity)

// (todo) MAX_EFFECT_TIMELINE, MAX_PREDICTION, MAX_VELOCITY, MAX_DISCREPANCIES, MAX_RC_EFFECT,
// MAX_MOMENTUM_EFFECT, MAX_EFFECT_DELTAS

// ── Option flags ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
pub struct AlgorithmEffectsOptions {
    pub carbs: bool,
    pub insulin: bool,
    pub momentum: bool,
    pub retrospection: bool,
}

impl Default for AlgorithmEffectsOptions {
    fn default() -> Self {
        Self {
            carbs: true,
            insulin: true,
            momentum: true,
            retrospection: true,
        }
    }
}

// ── Input / Output ────────────────────────────────────────────────────────────

/// All inputs required by the Loop dosing algorithm.
pub struct AlgorithmInput {
    pub prediction_start: Timestamp,
    pub glucose_history: Vec<GlucoseSample>,
    pub doses: Vec<InsulinDose>,
    pub carb_entries: Vec<CarbEntry>,
    /// IU/hr at each time segment.
    pub basal_schedule: Vec<ScheduleEntry<f64>>,
    /// mg/dL per IU.
    pub sensitivity_schedule: Vec<ScheduleEntry<f64>>,
    /// Grams per IU.
    pub carb_ratio_schedule: Vec<ScheduleEntry<f64>>,
    /// (min, max) mg/dL target range.
    pub target: GlucoseRangeTimeline,
    /// None → use the lower bound of `target`.
    pub suspend_threshold: Option<f64>,
    pub max_bolus: f64,
    pub max_basal_rate: f64,
    /// Multiplied by `max_bolus` to get the active-insulin cap.  Default 2.0.
    pub max_active_insulin_multiplier: f64,
    pub rc_mode: RCMode,
    /// If false, positive momentum and RC effects are suppressed.
    pub include_positive_velocity_and_rc: bool,
    pub use_mid_absorption_isf: bool,
    pub carb_absorption_model: CarbAbsorptionModel,
    pub recommendation_model: ExponentialInsulinModel,
    pub recommendation_type: DoseRecommendationType,
    /// Fraction of needed insulin delivered as automatic bolus.  Default 0.4.
    pub automatic_bolus_factor: f64,
    /// Maximum allowed consecutive glucose jump (mg/dL).  Default 40.
    pub gradual_transition_threshold: f64,
}

impl AlgorithmInput {
    pub fn new(
        prediction_start: Timestamp,
        glucose_history: Vec<GlucoseSample>,
        doses: Vec<InsulinDose>,
        carb_entries: Vec<CarbEntry>,
        basal_schedule: Vec<ScheduleEntry<f64>>,
        sensitivity_schedule: Vec<ScheduleEntry<f64>>,
        carb_ratio_schedule: Vec<ScheduleEntry<f64>>,
        target: GlucoseRangeTimeline,
        suspend_threshold: Option<f64>,
        max_bolus: f64,
        max_basal_rate: f64,
        recommendation_model: ExponentialInsulinModel,
        recommendation_type: DoseRecommendationType,
    ) -> Self {
        Self {
            prediction_start,
            glucose_history,
            doses,
            carb_entries,
            basal_schedule,
            sensitivity_schedule,
            carb_ratio_schedule,
            target,
            suspend_threshold,
            max_bolus,
            max_basal_rate,
            max_active_insulin_multiplier: DEFAULT_MAX_ACTIVE_INSULIN_MULTIPLIER,
            rc_mode: RCMode::default(),
            include_positive_velocity_and_rc: true,
            use_mid_absorption_isf: false,
            carb_absorption_model: CarbAbsorptionModel::default(),
            recommendation_model,
            recommendation_type,
            automatic_bolus_factor: crate::dose::DEFAULT_BOLUS_FACTOR,
            gradual_transition_threshold: DEFAULT_GRADUAL_TRANSITION_THRESHOLD,
        }
    }
}

/// Intermediate algorithm effects (for diagnostics / UI).
pub struct AlgorithmEffects {
    pub insulin: Vec<GlucoseEffect>,
    pub carbs: Vec<GlucoseEffect>,
    pub carb_status: Vec<CarbStatus>,
    pub retrospective_correction: Vec<GlucoseEffect>,
    pub momentum: Vec<GlucoseEffect>,
    pub insulin_counteraction: Vec<GlucoseEffectVelocity>,
    pub retrospective_glucose_discrepancies: Vec<GlucoseChange>,
    pub total_retrospective_correction_mgdl: Option<f64>,
}

pub struct LoopPrediction {
    pub glucose: Vec<PredictedGlucose>,
    pub effects: AlgorithmEffects,
    pub doses_relative_to_basal: Vec<BasalRelativeDose>,
    pub active_insulin: Option<f64>,
    pub active_carbs: Option<f64>,
}

pub struct AlgorithmOutput {
    pub recommendation: Result<DoseRecommendation, AlgorithmError>,
    pub predicted_glucose: Vec<PredictedGlucose>,
    pub effects: AlgorithmEffects,
    pub doses_relative_to_basal: Vec<BasalRelativeDose>,
    pub active_insulin: Option<f64>,
    pub active_carbs: Option<f64>,
}

#[derive(Clone, Debug)]
pub enum AlgorithmError {
    MissingGlucose,
    GlucoseTooOld,
    BasalTimelineIncomplete,
    MissingSuspendThreshold,
    SensitivityTimelineStartsTooLate,
    SensitivityTimelineEndsTooEarly,
    FutureBasalNotAllowed,
}

impl std::fmt::Display for AlgorithmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for AlgorithmError {}

// ── predict_glucose: combine effects into prediction ─────────────────────────

/// An `OrdF64` is a totally-ordered wrapper for finite f64 values.
/// Used as BTreeMap keys when accumulating glucose effects by timestamp.
///
/// SAFETY: We only insert timestamps produced by floor_to/ceil_to (always finite).
#[derive(Clone, Copy, PartialEq, PartialOrd)]
struct OrdF64(f64);
impl Eq for OrdF64 {}
impl Ord for OrdF64 {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // SAFETY: values are always finite (floor/ceil of finite timestamps).
        self.0.partial_cmp(&other.0).unwrap()
    }
}

/// Combine insulin, carb, RC, and momentum effects into a glucose prediction.
///
/// `starting_glucose` is the most recent real glucose reading.
/// `effects` are cumulative (each entry's value represents the total up to that time).
/// `momentum` is cumulative and blended linearly into the combined effect.
pub fn predict_glucose(
    starting_time: Timestamp,
    starting_mgdl: f64,
    momentum: &[GlucoseEffect],
    effects: &[&[GlucoseEffect]],
) -> Vec<PredictedGlucose> {
    // Accumulate deltas at each timestamp
    let mut effect_deltas: BTreeMap<OrdF64, f64> = BTreeMap::new();

    for timeline in effects {
        let mut prev = timeline.first().map(|e| e.value_mgdl).unwrap_or(0.0);
        for effect in *timeline {
            let delta = effect.value_mgdl - prev;
            *effect_deltas.entry(OrdF64(effect.start)).or_default() += delta;
            prev = effect.value_mgdl;
        }
    }

    // Blend momentum linearly (1.0 at first future step → 0.0 at last momentum point)
    if momentum.len() > 1 {
        let mut prev_mom = momentum[0].value_mgdl;
        let blend_count = (momentum.len() - 2) as f64;
        let time_delta = momentum[1].start - momentum[0].start;
        let momentum_offset = starting_time - momentum[0].start;
        let blend_slope = if blend_count > 0.0 {
            1.0 / blend_count
        } else {
            1.0
        };
        let blend_offset = (momentum_offset / time_delta) * blend_slope;

        for (i, effect) in momentum.iter().enumerate() {
            let mom_delta = effect.value_mgdl - prev_mom;
            let split = (1.0f64.min(
                0.0f64.max((momentum.len() - i) as f64 / blend_count - blend_slope + blend_offset),
            ))
            .clamp(0.0, 1.0);
            let existing = effect_deltas
                .get(&OrdF64(effect.start))
                .copied()
                .unwrap_or(0.0);
            let blended = (1.0 - split) * existing + split * mom_delta;
            effect_deltas.insert(OrdF64(effect.start), blended);
            prev_mom = effect.value_mgdl;
        }
    }

    // Accumulate deltas into predictions
    let mut out = vec![PredictedGlucose {
        start: starting_time,
        value_mgdl: starting_mgdl,
    }];
    let mut running = starting_mgdl;
    for (OrdF64(t), delta) in &effect_deltas {
        if *t > starting_time {
            running += delta;
            out.push(PredictedGlucose {
                start: *t,
                value_mgdl: running,
            });
        }
    }
    out
}

// ── generate_prediction ───────────────────────────────────────────────────────

/// Compute a glucose prediction from raw inputs.
///
/// Returns a partial result even if some effects cannot be computed; always fills in as
/// much as possible.
pub fn generate_prediction(
    start: Timestamp,
    glucose_history: &[GlucoseSample],
    doses: &[InsulinDose],
    carb_entries: &[CarbEntry],
    basal_schedule: &[ScheduleEntry<f64>],
    sensitivity_schedule: &[ScheduleEntry<f64>],
    carb_ratio_schedule: &[ScheduleEntry<f64>],
    options: AlgorithmEffectsOptions,
    rc_mode: RCMode,
    include_positive_velocity_and_rc: bool,
    use_mid_absorption_isf: bool,
    carb_model: CarbAbsorptionModel,
    gradual_threshold: f64,
) -> LoopPrediction {
    let mut insulin_effects: Vec<GlucoseEffect> = vec![];
    let carb_effects: Vec<GlucoseEffect>;
    let mut rc_effects: Vec<GlucoseEffect> = vec![];
    let mut momentum_effects: Vec<GlucoseEffect> = vec![];
    let mut ice: Vec<GlucoseEffectVelocity> = vec![];
    let discrepancies_summed: Vec<GlucoseChange>;
    let mut total_rc: Option<f64> = None;
    let active_insulin: Option<f64>;
    let active_carbs: Option<f64>;
    let mut doses_relative: Vec<BasalRelativeDose> = vec![];

    // ── Insulin effects ───────────────────────────────────────────────────────
    let dose_start = doses.first().map(|d| d.start).unwrap_or(start);
    if basal_schedule
        .first()
        .map_or(false, |e| e.start <= dose_start)
    {
        doses_relative = annotated_doses(doses, basal_schedule, false);
        active_insulin = Some(iob_at(&doses_relative, start));

        // Extend insulin effect range to cover glucose history
        let mut from = doses_relative.iter().map(|d| d.start).reduce(f64::min);
        let mut to = doses_relative
            .iter()
            .map(|d| d.end + d.model.effect_duration())
            .reduce(f64::max);

        if let Some(g_start) = glucose_history.first().map(|s| s.start) {
            from = Some(from.map_or(g_start, |f| f.min(g_start)));
        }
        if let Some(g_end) = glucose_history.last().map(|s| s.start) {
            to = Some(to.map_or(g_end, |t| t.max(g_end)));
        }

        if use_mid_absorption_isf {
            insulin_effects = glucose_effects_mid_absorption_isf(
                &doses_relative,
                sensitivity_schedule,
                from,
                to,
                DELTA,
            );
        } else {
            insulin_effects =
                glucose_effects(&doses_relative, sensitivity_schedule, from, to, DELTA);
        }

        ice = counteraction_effects(glucose_history, &insulin_effects);
    } else {
        active_insulin = Some(0.0);
    }

    // ── Carb effects ──────────────────────────────────────────────────────────
    let carb_status = map_carb_entries(
        carb_entries,
        &ice,
        carb_ratio_schedule,
        sensitivity_schedule,
        crate::carbs::DEFAULT_ABSORPTION_TIME_OVERRUN,
        DEFAULT_ABSORPTION_TIME,
        DEFAULT_EFFECT_DELAY,
        crate::carbs::DEFAULT_ABSORPTION_TIME_OVERRUN,
        carb_model,
        false,
        0.2,
    );

    let rc_start = start - RETROSPECTION_INTERVAL;
    carb_effects = dynamic_glucose_effects(
        &carb_status,
        Some(rc_start),
        None,
        carb_ratio_schedule,
        sensitivity_schedule,
        carb_model,
        DEFAULT_EFFECT_DELAY,
        DELTA,
    );

    active_carbs = Some(dynamic_cob_at(&carb_status, start, carb_model));

    // ── Retrospective correction ──────────────────────────────────────────────
    let raw_discrepancies = subtract_effects(&ice, &carb_effects, DELTA);
    discrepancies_summed =
        combined_sums(&raw_discrepancies, RETROSPECTIVE_GROUPING_INTERVAL * 1.01);

    let latest = glucose_history.last();
    let mut prediction: Vec<PredictedGlucose> = vec![];

    if let Some(latest_glucose) = latest {
        let (rc, total) = match rc_mode {
            RCMode::Standard => standard_rc_effect(
                latest_glucose.start,
                latest_glucose.value_mgdl,
                &discrepancies_summed,
                15.0 * 60.0,
                RETROSPECTIVE_GROUPING_INTERVAL,
                RETROSPECTIVE_EFFECT_DURATION,
                DELTA,
            ),
            RCMode::Integral => integral_rc_effect(
                latest_glucose.start,
                latest_glucose.value_mgdl,
                &discrepancies_summed,
                15.0 * 60.0,
                RETROSPECTIVE_GROUPING_INTERVAL,
                RETROSPECTIVE_EFFECT_DURATION,
                DELTA,
            ),
        };
        rc_effects = rc;
        total_rc = total;

        let mut combined: Vec<&[GlucoseEffect]> = vec![];

        if options.carbs {
            combined.push(&carb_effects);
        }
        if options.insulin {
            combined.push(&insulin_effects);
        }

        let use_rc = if options.retrospection {
            let rc_window_start = start - RETROSPECTIVE_GROUPING_INTERVAL;
            let rc_data: Vec<&GlucoseSample> = glucose_history
                .iter()
                .filter(|s| s.start >= rc_window_start && s.start <= start)
                .collect();
            let rc_samples: Vec<GlucoseSample> = rc_data.into_iter().cloned().collect();
            let smooth = has_gradual_transitions(&rc_samples, gradual_threshold);
            let positive_ok = include_positive_velocity_and_rc
                || rc_effects
                    .last()
                    .map(|e| e.value_mgdl - rc_effects.first().map(|f| f.value_mgdl).unwrap_or(0.0))
                    .unwrap_or(0.0)
                    <= 0.0;
            smooth && positive_ok
        } else {
            false
        };
        if use_rc {
            combined.push(&rc_effects);
        }

        // Momentum
        let use_momentum;
        if options.momentum {
            let mom_window_start = start - MOMENTUM_DATA_INTERVAL;
            let mom_data: Vec<GlucoseSample> = glucose_history
                .iter()
                .filter(|s| s.start >= mom_window_start && s.start <= start)
                .cloned()
                .collect();
            momentum_effects = linear_momentum_effect(&mom_data, MOMENTUM_DATA_INTERVAL, DELTA);
            let net_mom = momentum_effects.last().map(|e| e.value_mgdl).unwrap_or(0.0);
            use_momentum = include_positive_velocity_and_rc || net_mom <= 0.0;
        } else {
            use_momentum = false;
        }

        let mom_ref: &[GlucoseEffect] = if use_momentum { &momentum_effects } else { &[] };

        prediction = predict_glucose(
            latest_glucose.start,
            latest_glucose.value_mgdl,
            mom_ref,
            &combined,
        );

        // Extend to cover full insulin effect duration for dosing
        let final_date = start + DEFAULT_INSULIN_ACTIVITY_DURATION;
        if let Some(last) = prediction.last() {
            if last.start < final_date {
                prediction.push(PredictedGlucose {
                    start: final_date,
                    value_mgdl: last.value_mgdl,
                });
            }
        }
    }

    LoopPrediction {
        glucose: prediction,
        effects: AlgorithmEffects {
            insulin: insulin_effects,
            carbs: carb_effects,
            carb_status,
            retrospective_correction: rc_effects,
            momentum: momentum_effects,
            insulin_counteraction: ice,
            retrospective_glucose_discrepancies: discrepancies_summed,
            total_retrospective_correction_mgdl: total_rc,
        },
        doses_relative_to_basal: doses_relative,
        active_insulin,
        active_carbs,
    }
}

// ── run: full algorithm with validation and dosing ────────────────────────────

pub fn run(input: &AlgorithmInput) -> AlgorithmOutput {
    let empty_effects = AlgorithmEffects {
        insulin: vec![],
        carbs: vec![],
        carb_status: vec![],
        retrospective_correction: vec![],
        momentum: vec![],
        insulin_counteraction: vec![],
        retrospective_glucose_discrepancies: vec![],
        total_retrospective_correction_mgdl: None,
    };
    let mut prediction = LoopPrediction {
        glucose: vec![],
        effects: empty_effects,
        doses_relative_to_basal: vec![],
        active_insulin: None,
        active_carbs: None,
    };

    let result: Result<DoseRecommendation, AlgorithmError> = (|| {
        // ── Validation ────────────────────────────────────────────────────────
        let latest_glucose = input
            .glucose_history
            .last()
            .ok_or(AlgorithmError::MissingGlucose)?;

        if input.prediction_start - latest_glucose.start >= INPUT_DATA_RECENCY {
            return Err(AlgorithmError::GlucoseTooOld);
        }

        // No future basal allowed for automated dosing
        if input.recommendation_type.is_automated() {
            let latest_basal_end = input
                .doses
                .iter()
                .filter(|d| d.delivery_type == crate::insulin::InsulinDeliveryType::Basal)
                .map(|d| d.end)
                .reduce(f64::max);
            if let Some(end) = latest_basal_end {
                if end > input.prediction_start {
                    return Err(AlgorithmError::FutureBasalNotAllowed);
                }
            }
        }

        let forecast_end = ceil_to(
            input.prediction_start + input.recommendation_model.effect_duration(),
            DELTA,
        );
        let glucose_start = input
            .glucose_history
            .first()
            .map(|s| s.start)
            .unwrap_or(input.prediction_start);

        // Compute needed ISF interval
        let (eff_start, eff_end) = match effects_interval(&[]) {
            Some(i) => i,
            None => (glucose_start, glucose_start),
        };
        let isf_need_start = glucose_start.min(input.prediction_start).min(eff_start);
        let isf_need_end = forecast_end.max(eff_end);

        if input
            .sensitivity_schedule
            .first()
            .map_or(true, |e| e.start > isf_need_start)
        {
            return Err(AlgorithmError::SensitivityTimelineStartsTooLate);
        }
        if input
            .sensitivity_schedule
            .last()
            .map_or(true, |e| e.end < isf_need_end)
        {
            return Err(AlgorithmError::SensitivityTimelineEndsTooEarly);
        }
        if closest_prior(&input.basal_schedule, input.prediction_start).is_none() {
            return Err(AlgorithmError::BasalTimelineIncomplete);
        }

        let suspend_threshold = match input
            .suspend_threshold
            .or_else(|| closest_prior(&input.target, input.prediction_start).map(|t| t.value.0))
        {
            Some(v) => v,
            None => return Err(AlgorithmError::MissingSuspendThreshold),
        };

        // ── Generate prediction ───────────────────────────────────────────────
        prediction = generate_prediction(
            input.prediction_start,
            &input.glucose_history,
            &input.doses,
            &input.carb_entries,
            &input.basal_schedule,
            &input.sensitivity_schedule,
            &input.carb_ratio_schedule,
            AlgorithmEffectsOptions::default(),
            input.rc_mode,
            input.include_positive_velocity_and_rc,
            input.use_mid_absorption_isf,
            input.carb_absorption_model,
            input.gradual_transition_threshold,
        );

        // ── ISF for dosing recommendation ─────────────────────────────────────
        let dosing_isf: Vec<ScheduleEntry<f64>> = if input.use_mid_absorption_isf {
            input.sensitivity_schedule.to_vec()
        } else {
            // Single ISF entry at prediction_start covering the forecast window
            let isf_at_start = input
                .sensitivity_schedule
                .iter()
                .find(|e| e.start <= input.prediction_start && e.end >= input.prediction_start)
                .expect("ISF must cover prediction start");
            let sens_end = forecast_end.max(
                prediction
                    .effects
                    .insulin
                    .last()
                    .map(|e| e.start)
                    .unwrap_or(forecast_end),
            );
            vec![ScheduleEntry {
                start: isf_at_start.start,
                end: sens_end,
                value: isf_at_start.value,
            }]
        };

        let predicted_pairs: Vec<(Timestamp, f64)> = prediction
            .glucose
            .iter()
            .map(|p| (p.start, p.value_mgdl))
            .collect();

        let correction = insulin_correction(
            &predicted_pairs,
            input.prediction_start,
            &input.target,
            suspend_threshold,
            &dosing_isf,
            &input.recommendation_model,
        );

        let scheduled_basal = closest_prior(&input.basal_schedule, input.prediction_start)
            .expect("Basal must cover prediction start")
            .value;

        let active = prediction.active_insulin.unwrap_or(0.0);
        let max_active = input.max_bolus * input.max_active_insulin_multiplier;

        let rec = match input.recommendation_type {
            DoseRecommendationType::ManualBolus => {
                DoseRecommendation::manual(recommend_manual_bolus(
                    &correction,
                    input.max_bolus,
                    latest_glucose.start,
                    latest_glucose.value_mgdl,
                    &input.target,
                ))
            }
            DoseRecommendationType::AutomaticBolus => {
                DoseRecommendation::automatic(recommend_automatic_dose(
                    &correction,
                    input.automatic_bolus_factor,
                    scheduled_basal,
                    active,
                    input.max_bolus,
                    max_active,
                ))
            }
            DoseRecommendationType::TempBasal => {
                let temp = recommend_temp_basal(
                    &correction,
                    scheduled_basal,
                    active,
                    input.max_bolus,
                    input.max_basal_rate,
                    max_active,
                );
                DoseRecommendation::automatic(crate::dose::AutomaticDoseRecommendation {
                    basal_adjustment: temp,
                    bolus_units: None,
                    direction: correction.direction(),
                })
            }
        };

        Ok(rec)
    })();

    AlgorithmOutput {
        recommendation: result,
        predicted_glucose: prediction.glucose,
        effects: prediction.effects,
        doses_relative_to_basal: prediction.doses_relative_to_basal,
        active_insulin: prediction.active_insulin,
        active_carbs: prediction.active_carbs,
    }
}
