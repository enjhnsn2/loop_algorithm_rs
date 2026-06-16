use loop_algorithm::{
    algorithm::{run, AlgorithmInput, AlgorithmError},
    carbs::CarbEntry,
    dose::DoseRecommendationType,
    glucose::GlucoseSample,
    insulin::{InsulinDeliveryType, InsulinDose, ModelPreset, DEFAULT_INSULIN_ACTIVITY_DURATION, DELTA},
    types::{ceil_to, ScheduleEntry},
};

// 2024-01-03T12:00:00Z
const NOW: f64 = 1704283200.0;
// 2020-03-11T19:13:14Z (= 2020-03-11T12:13:14-0700)
const NOW_2020: f64 = 1583953994.0;
// 2024-01-03T00:00:00Z
const NOW_2024_MIDNIGHT: f64 = 1704240000.0;

fn mock_input(now: f64) -> AlgorithmInput {
    let forecast_end = ceil_to(now + DEFAULT_INSULIN_ACTIVITY_DURATION, DELTA);
    AlgorithmInput::new(
        now,
        vec![
            GlucoseSample { start: now - 19.0 * 60.0, value_mgdl: 100.0, provenance: 0, is_display_only: false },
            GlucoseSample { start: now - 14.0 * 60.0, value_mgdl: 120.0, provenance: 0, is_display_only: false },
            GlucoseSample { start: now - 9.0 * 60.0,  value_mgdl: 140.0, provenance: 0, is_display_only: false },
            GlucoseSample { start: now - 4.0 * 60.0,  value_mgdl: 160.0, provenance: 0, is_display_only: false },
        ],
        vec![],
        vec![],
        vec![ScheduleEntry::new(now - 36000.0, now, 1.0)],
        vec![ScheduleEntry::new(now - 36000.0, forecast_end, 55.0)],
        vec![ScheduleEntry::new(now - 36000.0, now, 10.0)],
        vec![ScheduleEntry::new(now - 36000.0, now, (100.0, 110.0))],
        Some(65.0),
        6.0,
        8.0,
        ModelPreset::RapidActingAdult.model(),
        DoseRecommendationType::TempBasal,
    )
}

fn bolus_dose(now: f64, start_offset: f64, end_offset: f64, volume_iu: f64) -> InsulinDose {
    InsulinDose {
        delivery_type: InsulinDeliveryType::Bolus,
        start: now + start_offset,
        end: now + end_offset,
        volume_iu,
        model: ModelPreset::RapidActingAdult.model(),
    }
}

// ── testAlgorithmWithLongAbsorbingCarbs ──────────────────────────────────────

// Swift: Tests/LoopAlgorithmTests/LoopAlgorithmTests.swift:70 testAlgorithmWithLongAbsorbingCarbs
#[test]
fn test_algorithm_with_long_absorbing_carbs() {
    let mut input = mock_input(NOW);
    input.recommendation_type = DoseRecommendationType::ManualBolus;
    input.carb_entries.push(CarbEntry {
        start: NOW,
        grams: 50.0,
        absorption_time: Some(6.0 * 3600.0),
    });

    let output = run(&input);

    assert!((output.active_carbs.unwrap_or(0.0) - 50.0).abs() < 1.0);
    let manual = output.recommendation.unwrap().manual.unwrap();
    assert!((manual.amount_iu - 5.83).abs() < 0.01, "Expected 5.83, got {}", manual.amount_iu);
}

// ── testAlgorithmShouldBeDateIndependent ─────────────────────────────────────

// Swift: Tests/LoopAlgorithmTests/LoopAlgorithmTests.swift:83 testAlgorithmShouldBeDateIndependent
#[test]
fn test_algorithm_should_be_date_independent() {
    let now = NOW;
    let mut a = mock_input(now);
    let mut b = mock_input(now - 150.0); // now - 2.5 min

    // 10g carb 30min before prediction start
    a.carb_entries.push(CarbEntry {
        start: a.prediction_start - 30.0 * 60.0,
        grams: 10.0,
        absorption_time: None,
    });
    b.carb_entries.push(CarbEntry {
        start: b.prediction_start - 30.0 * 60.0,
        grams: 10.0,
        absorption_time: None,
    });

    let output_a = run(&a);
    let output_b = run(&b);

    assert!(
        (output_a.active_carbs.unwrap_or(0.0) - output_b.active_carbs.unwrap_or(0.0)).abs() < 1.0,
        "activeCarbs should be approximately equal"
    );
    assert!(
        (output_a.active_insulin.unwrap_or(0.0) - output_b.active_insulin.unwrap_or(0.0)).abs() < 0.01,
        "activeInsulin should be approximately equal"
    );

    // 10g carbs at ISF 55, carb ratio 10 → total carb glucose effect = 55 mg/dL
    let carbs_last_a = output_a.effects.carbs.last().map(|e| e.value_mgdl).unwrap_or(0.0);
    let carbs_last_b = output_b.effects.carbs.last().map(|e| e.value_mgdl).unwrap_or(0.0);
    assert!((carbs_last_a - 55.0).abs() < 1.0, "carbs effect last should be ~55, got {}", carbs_last_a);
    assert!((carbs_last_b - 55.0).abs() < 1.0, "carbs effect last should be ~55, got {}", carbs_last_b);

    let insulin_last_a = output_a.effects.insulin.last().map(|e| e.value_mgdl).unwrap_or(0.0);
    let insulin_last_b = output_b.effects.insulin.last().map(|e| e.value_mgdl).unwrap_or(0.0);
    assert!((insulin_last_a - 0.0).abs() < 0.5, "insulin effect last should be ~0, got {}", insulin_last_a);
    assert!((insulin_last_b - 0.0).abs() < 0.5, "insulin effect last should be ~0, got {}", insulin_last_b);

    let rc_last_a = output_a.effects.retrospective_correction.last().map(|e| e.value_mgdl).unwrap_or(0.0);
    let rc_last_b = output_b.effects.retrospective_correction.last().map(|e| e.value_mgdl).unwrap_or(0.0);
    assert!((rc_last_a - 165.0).abs() < 5.0, "RC last should be ~165, got {}", rc_last_a);
    assert!((rc_last_b - 165.0).abs() < 5.0, "RC last should be ~165, got {}", rc_last_b);
}

// ── testSpuriousReadingDisablesRCAndMomentum ─────────────────────────────────

// Swift: Tests/LoopAlgorithmTests/LoopAlgorithmTests.swift:193 testSpuriousReadingDisablesRCAndMomentum
#[test]
fn test_spurious_reading_disables_rc_and_momentum() {
    // base = 2024-01-03T12:00:00Z, prediction_start = base + 30min
    let base = NOW;
    let mut input = mock_input(base + 30.0 * 60.0);

    input.glucose_history = vec![
        GlucoseSample { start: base,                value_mgdl: 100.0, provenance: 0, is_display_only: false },
        GlucoseSample { start: base + 5.0 * 60.0,  value_mgdl: 115.0, provenance: 0, is_display_only: false },
        GlucoseSample { start: base + 10.0 * 60.0, value_mgdl: 125.0, provenance: 0, is_display_only: false },
        GlucoseSample { start: base + 15.0 * 60.0, value_mgdl: 179.0, provenance: 0, is_display_only: false }, // +54 jump
        GlucoseSample { start: base + 20.0 * 60.0, value_mgdl: 148.0, provenance: 0, is_display_only: false },
        GlucoseSample { start: base + 25.0 * 60.0, value_mgdl: 158.0, provenance: 0, is_display_only: false },
        GlucoseSample { start: base + 30.0 * 60.0, value_mgdl: 168.0, provenance: 0, is_display_only: false },
    ];

    // With spurious reading (>40 threshold), momentum and RC are suppressed → lower forecast
    let output = run(&input);
    let last_glucose = output.predicted_glucose.last().map(|g| g.value_mgdl).unwrap_or(0.0);
    assert!(
        (last_glucose - 164.5).abs() < 0.5,
        "Expected ~164.5 with spurious reading, got {}", last_glucose
    );

    // With higher threshold (60), spurious reading is treated as gradual → higher forecast
    input.gradual_transition_threshold = 60.0;
    let output2 = run(&input);
    let last_glucose2 = output2.predicted_glucose.last().map(|g| g.value_mgdl).unwrap_or(0.0);
    assert!(
        (last_glucose2 - 216.0).abs() < 1.0,
        "Expected ~216 with threshold=60, got {}", last_glucose2
    );
}

// ── testMidAbsorptionISFFlag ──────────────────────────────────────────────────

// Swift: Tests/LoopAlgorithmTests/LoopAlgorithmTests.swift:281 testMidAbsorptionISFFlag
#[test]
fn test_mid_absorption_isf_flag() {
    // 2024-01-03T00:00:00Z
    let now = NOW_2024_MIDNIGHT;
    let mut input = mock_input(now);
    let forecast_end = ceil_to(now + DEFAULT_INSULIN_ACTIVITY_DURATION, DELTA);

    input.doses = vec![InsulinDose {
        delivery_type: InsulinDeliveryType::Bolus,
        start: now,
        end: now + 20.0,
        volume_iu: 1.0,
        model: ModelPreset::RapidActingAdult.model(),
    }];

    let isf_mid = now + 1.5 * 3600.0;
    input.sensitivity_schedule = vec![
        ScheduleEntry::new(now - 86400.0, isf_mid, 50.0),
        ScheduleEntry::new(isf_mid, forecast_end, 100.0),
    ];

    // Without mid-absorption ISF: uses ISF at delivery time for the whole window
    input.use_mid_absorption_isf = false;
    let output = run(&input);
    let insulin_last = output.effects.insulin.last().map(|e| e.value_mgdl).unwrap_or(0.0);
    assert!((insulin_last - (-50.0)).abs() < 0.5, "Expected ~-50, got {}", insulin_last);

    // With mid-absorption ISF: blends ISF across the absorption window
    input.use_mid_absorption_isf = true;
    let output2 = run(&input);
    let insulin_last2 = output2.effects.insulin.last().map(|e| e.value_mgdl).unwrap_or(0.0);
    assert!((insulin_last2 - (-83.0)).abs() < 1.0, "Expected ~-83, got {}", insulin_last2);
}

// ── testAutoBolusMaxIOBClamping ───────────────────────────────────────────────

// Swift: Tests/LoopAlgorithmTests/LoopAlgorithmTests.swift:309 testAutoBolusMaxIOBClamping
#[test]
fn test_auto_bolus_max_iob_clamping() {
    let now = NOW_2020;
    let mut input = mock_input(now);
    input.recommendation_type = DoseRecommendationType::AutomaticBolus;

    input.doses = vec![bolus_dose(now, -5.0 * 60.0, -4.0 * 60.0, 8.0)];
    input.carb_entries = vec![CarbEntry {
        start: now - 5.0 * 60.0,
        grams: 100.0,
        absorption_time: None,
    }];

    // maxBolus = 8 → maxActiveInsulin = 16; should allow some additional bolus
    input.max_bolus = 8.0;
    let output = run(&input);
    let active_insulin = output.active_insulin.unwrap_or(0.0);
    assert!((active_insulin - 8.0).abs() < 0.5, "Expected ~8U IOB, got {}", active_insulin);
    let bolus = output.recommendation.unwrap().automatic.unwrap().bolus_units.unwrap_or(0.0);
    assert!((bolus - 1.66).abs() < 0.05, "Expected ~1.66U bolus with maxBolus=8, got {}", bolus);

    // maxBolus = 4 → maxActiveInsulin = 8; already at cap, no additional bolus
    input.max_bolus = 4.0;
    let output2 = run(&input);
    let active_insulin2 = output2.active_insulin.unwrap_or(0.0);
    assert!((active_insulin2 - 8.0).abs() < 0.5, "Expected ~8U IOB, got {}", active_insulin2);
    let bolus2 = output2.recommendation.unwrap().automatic.unwrap().bolus_units.unwrap_or(0.0);
    assert!((bolus2 - 0.0).abs() < 0.05, "Expected 0U bolus with maxBolus=4, got {}", bolus2);
}

// ── testTempBasalMaxIOBClamping ───────────────────────────────────────────────

// Swift: Tests/LoopAlgorithmTests/LoopAlgorithmTests.swift:343 testTempBasalMaxIOBClamping
#[test]
fn test_temp_basal_max_iob_clamping() {
    let now = NOW_2020;
    let mut input = mock_input(now);
    input.recommendation_type = DoseRecommendationType::TempBasal;

    input.doses = vec![bolus_dose(now, -5.0 * 60.0, -4.0 * 60.0, 8.0)];
    input.carb_entries = vec![CarbEntry {
        start: now - 5.0 * 60.0,
        grams: 100.0,
        absorption_time: None,
    }];

    // maxBolus = 8 → maxActiveInsulin = 16; IOB = 8 → headroom = 8 → high basal allowed
    input.max_bolus = 8.0;
    let output = run(&input);
    let active_insulin = output.active_insulin.unwrap_or(0.0);
    assert!((active_insulin - 8.0).abs() < 0.5, "Expected ~8U IOB, got {}", active_insulin);
    let rate = output.recommendation.unwrap().automatic.unwrap().basal_adjustment.units_per_hour;
    assert!((rate - 8.0).abs() < 0.05, "Expected 8 U/hr with maxBolus=8, got {}", rate);

    // maxBolus = 4 → maxActiveInsulin = 8; IOB = 8 → headroom = 0 → only scheduled basal
    input.max_bolus = 4.0;
    let output2 = run(&input);
    let active_insulin2 = output2.active_insulin.unwrap_or(0.0);
    assert!((active_insulin2 - 8.0).abs() < 0.5, "Expected ~8U IOB, got {}", active_insulin2);
    let rate2 = output2.recommendation.unwrap().automatic.unwrap().basal_adjustment.units_per_hour;
    assert!((rate2 - 1.0).abs() < 0.05, "Expected 1 U/hr with maxBolus=4, got {}", rate2);
}

// ── testRecommendationWithMidAbsorptionISF ────────────────────────────────────

// Swift: Tests/LoopAlgorithmTests/LoopAlgorithmTests.swift:378 testRecommendationWithMidAbsorptionISF
#[test]
fn test_recommendation_with_mid_absorption_isf() {
    let now = NOW_2020;
    let mut input = mock_input(now);
    input.recommendation_type = DoseRecommendationType::ManualBolus;
    input.max_bolus = 8.0;
    input.doses = vec![];
    input.carb_entries = vec![];

    let isf_change_time = now + 3600.0;
    input.sensitivity_schedule = vec![
        ScheduleEntry::new(now - 48.0 * 3600.0, isf_change_time, 50.0),
        ScheduleEntry::new(isf_change_time, now + 48.0 * 3600.0, 100.0),
    ];

    // Without mid-absorption ISF
    input.use_mid_absorption_isf = false;
    let output = run(&input);
    let amount = output.recommendation.unwrap().manual.unwrap().amount_iu;
    assert!((amount - 2.58).abs() < 0.05, "Expected 2.58, got {}", amount);

    // With mid-absorption ISF
    input.use_mid_absorption_isf = true;
    let output2 = run(&input);
    let amount2 = output2.recommendation.unwrap().manual.unwrap().amount_iu;
    assert!((amount2 - 1.41).abs() < 0.05, "Expected 1.41, got {}", amount2);
}

// ── testIncompleteISFTimelineDetected ─────────────────────────────────────────

// Swift: Tests/LoopAlgorithmTests/LoopAlgorithmTests.swift:408 testIncompleteISFTimelineDetected
#[test]
fn test_incomplete_isf_timeline_starts_too_late() {
    let now = NOW_2020;
    let mut input = mock_input(now);
    input.recommendation_type = DoseRecommendationType::ManualBolus;

    let basal_start = now - 5.0 * 60.0;
    let basal_end = now + 30.0 * 60.0;
    input.doses = vec![InsulinDose {
        delivery_type: InsulinDeliveryType::Basal,
        start: basal_start,
        end: basal_end,
        volume_iu: 4.0, // 8 U/hr
        model: ModelPreset::RapidActingAdult.model(),
    }];

    input.glucose_history = vec![
        GlucoseSample { start: now - 60.0, value_mgdl: 105.0, provenance: 0, is_display_only: false },
    ];

    // Sensitivity doesn't cover start of basal dose (starts at `now`, but dose starts at now-5min)
    let forecast_end = ceil_to(now + DEFAULT_INSULIN_ACTIVITY_DURATION, DELTA);
    input.sensitivity_schedule = vec![
        ScheduleEntry::new(now, forecast_end, 50.0),
    ];

    let output = run(&input);
    assert!(
        matches!(output.recommendation, Err(AlgorithmError::SensitivityTimelineStartsTooLate)),
        "Expected SensitivityTimelineStartsTooLate, got {:?}", output.recommendation
    );
}

