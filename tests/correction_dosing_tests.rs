use loop_algorithm::dose::{
    insulin_correction, recommend_automatic_dose, recommend_manual_bolus, recommend_temp_basal,
    BolusRecommendationNotice, GlucoseRangeTimeline, InsulinCorrection,
};
use loop_algorithm::insulin::ModelPreset;
use loop_algorithm::types::ScheduleEntry;

// 2024-01-03T12:00:00Z
const TEST_DATE: f64 = 1704283200.0;

fn h(hours: f64) -> f64 {
    TEST_DATE + hours * 3600.0
}

fn setup_target() -> GlucoseRangeTimeline {
    let mut v = GlucoseRangeTimeline::new();
    v.push(ScheduleEntry::new(h(-24.0), h(24.0), (90.0, 120.0)))
        .unwrap();
    v
}

fn setup_isf() -> Vec<ScheduleEntry<f64>> {
    // Must cover the entire prediction window (up to 6.2h past testDate)
    vec![ScheduleEntry::new(h(-24.0), h(24.0), 60.0)]
}

fn model() -> loop_algorithm::insulin::ExponentialInsulinModel {
    ModelPreset::RapidActingAdult.model()
}

const BASAL_RATE: f64 = 1.0;
const MAX_BASAL_RATE: f64 = 3.0;
const MAX_BOLUS: f64 = 6.0;
const MAX_ACTIVE_INSULIN: f64 = 12.0;
const SUSPEND_THRESHOLD: f64 = 55.0;
const CURRENT_GLUCOSE: f64 = 120.0;

fn correction(prediction: &[(f64, f64)]) -> InsulinCorrection {
    insulin_correction(
        prediction,
        TEST_DATE,
        &setup_target(),
        SUSPEND_THRESHOLD,
        &setup_isf(),
        &model(),
    )
}

// ── testNoChange ─────────────────────────────────────────────────────────────
// Swift: Tests/LoopAlgorithmTests/CorrectionDosingTests.swift:45 testNoChange

fn no_change_prediction() -> Vec<(f64, f64)> {
    vec![(h(0.0), 100.0), (h(6.2), 100.0)]
}

#[test]
fn test_no_change_temp_basal() {
    let corr = correction(&no_change_prediction());
    let rec = recommend_temp_basal(
        &corr,
        BASAL_RATE,
        0.0,
        MAX_BOLUS,
        MAX_BASAL_RATE,
        MAX_ACTIVE_INSULIN,
    );
    assert!((rec.units_per_hour - BASAL_RATE).abs() < 1e-9);
    assert!((rec.duration - 1800.0).abs() < 1e-9);
}

#[test]
fn test_no_change_auto_dose() {
    let corr = correction(&no_change_prediction());
    let auto = recommend_automatic_dose(&corr, 0.5, BASAL_RATE, 0.0, MAX_BOLUS, MAX_ACTIVE_INSULIN);
    assert_eq!(auto.bolus_units.unwrap_or(0.0), 0.0);
    assert!((auto.basal_adjustment.units_per_hour - BASAL_RATE).abs() < 1e-9);
}

#[test]
fn test_no_change_manual_bolus() {
    let corr = correction(&no_change_prediction());
    let rec = recommend_manual_bolus(
        &corr,
        MAX_BOLUS,
        TEST_DATE,
        CURRENT_GLUCOSE,
        &setup_target(),
    );
    assert_eq!(rec.amount_iu, 0.0);
    assert!(matches!(
        rec.notice,
        Some(BolusRecommendationNotice::PredictedGlucoseInRange)
    ));
}

// ── testStartHighEndInRange ──────────────────────────────────────────────────
// Swift: Tests/LoopAlgorithmTests/CorrectionDosingTests.swift:91 testStartHighEndInRange

fn start_high_end_in_range_prediction() -> Vec<(f64, f64)> {
    vec![
        (h(0.0), 200.0),
        (h(0.5), 180.0),
        (h(1.0), 150.0),
        (h(1.5), 120.0),
        (h(6.2), 100.0),
    ]
}

#[test]
fn test_start_high_end_in_range_temp_basal() {
    let corr = correction(&start_high_end_in_range_prediction());
    let rec = recommend_temp_basal(
        &corr,
        BASAL_RATE,
        0.0,
        MAX_BOLUS,
        MAX_BASAL_RATE,
        MAX_ACTIVE_INSULIN,
    );
    assert!((rec.units_per_hour - BASAL_RATE).abs() < 1e-9);
    assert!((rec.duration - 1800.0).abs() < 1e-9);
}

#[test]
fn test_start_high_end_in_range_auto_dose() {
    let corr = correction(&start_high_end_in_range_prediction());
    let auto = recommend_automatic_dose(&corr, 0.5, BASAL_RATE, 0.0, MAX_BOLUS, MAX_ACTIVE_INSULIN);
    assert_eq!(auto.bolus_units.unwrap_or(0.0), 0.0);
    assert!((auto.basal_adjustment.units_per_hour - BASAL_RATE).abs() < 1e-9);
}

#[test]
fn test_start_high_end_in_range_manual_bolus() {
    let corr = correction(&start_high_end_in_range_prediction());
    let rec = recommend_manual_bolus(
        &corr,
        MAX_BOLUS,
        TEST_DATE,
        CURRENT_GLUCOSE,
        &setup_target(),
    );
    assert_eq!(rec.amount_iu, 0.0);
    assert!(matches!(
        rec.notice,
        Some(BolusRecommendationNotice::PredictedGlucoseInRange)
    ));
}

// ── testStartLowEndInRange ───────────────────────────────────────────────────
// Swift: Tests/LoopAlgorithmTests/CorrectionDosingTests.swift:137 testStartLowEndInRange

fn start_low_end_in_range_prediction() -> Vec<(f64, f64)> {
    vec![
        (h(0.0), 60.0),
        (h(0.5), 70.0),
        (h(1.0), 80.0),
        (h(1.5), 90.0),
        (h(6.2), 100.0),
    ]
}

#[test]
fn test_start_low_end_in_range_temp_basal() {
    let corr = correction(&start_low_end_in_range_prediction());
    let rec = recommend_temp_basal(
        &corr,
        BASAL_RATE,
        0.0,
        MAX_BOLUS,
        MAX_BASAL_RATE,
        MAX_ACTIVE_INSULIN,
    );
    assert!((rec.units_per_hour - 1.0).abs() < 1e-9);
    assert!((rec.duration - 1800.0).abs() < 1e-9);
}

#[test]
fn test_start_low_end_in_range_auto_dose() {
    let corr = correction(&start_low_end_in_range_prediction());
    let auto = recommend_automatic_dose(&corr, 0.4, BASAL_RATE, 0.0, MAX_BOLUS, MAX_ACTIVE_INSULIN);
    assert_eq!(auto.bolus_units.unwrap_or(0.0), 0.0);
    assert!((auto.basal_adjustment.units_per_hour - BASAL_RATE).abs() < 1e-9);
    assert!((auto.basal_adjustment.duration - 1800.0).abs() < 1e-9);
}

#[test]
fn test_start_low_end_in_range_manual_bolus() {
    let corr = correction(&start_low_end_in_range_prediction());
    let rec = recommend_manual_bolus(
        &corr,
        MAX_BOLUS,
        TEST_DATE,
        CURRENT_GLUCOSE,
        &setup_target(),
    );
    assert_eq!(rec.amount_iu, 0.0);
    assert!(matches!(
        rec.notice,
        Some(BolusRecommendationNotice::PredictedGlucoseInRange)
    ));
}

// ── testCorrectLowAtMin ──────────────────────────────────────────────────────
// Swift: Tests/LoopAlgorithmTests/CorrectionDosingTests.swift:184 testCorrectLowAtMin

fn correct_low_at_min_prediction() -> Vec<(f64, f64)> {
    vec![
        (h(0.0), 100.0),
        (h(0.5), 90.0),
        (h(1.0), 85.0),
        (h(1.5), 90.0),
        (h(6.2), 100.0),
    ]
}

#[test]
fn test_correct_low_at_min_temp_basal() {
    let corr = correction(&correct_low_at_min_prediction());
    let rec = recommend_temp_basal(
        &corr,
        BASAL_RATE,
        0.0,
        MAX_BOLUS,
        MAX_BASAL_RATE,
        MAX_ACTIVE_INSULIN,
    );
    assert!((rec.units_per_hour - 1.0).abs() < 1e-9);
    assert!((rec.duration - 1800.0).abs() < 1e-9);
}

#[test]
fn test_correct_low_at_min_auto_dose() {
    let corr = correction(&correct_low_at_min_prediction());
    let auto = recommend_automatic_dose(&corr, 0.4, BASAL_RATE, 0.0, MAX_BOLUS, MAX_ACTIVE_INSULIN);
    assert_eq!(auto.bolus_units.unwrap_or(0.0), 0.0);
    assert!((auto.basal_adjustment.units_per_hour - BASAL_RATE).abs() < 1e-9);
    assert!((auto.basal_adjustment.duration - 1800.0).abs() < 1e-9);
}

#[test]
fn test_correct_low_at_min_manual_bolus() {
    let corr = correction(&correct_low_at_min_prediction());
    let rec = recommend_manual_bolus(
        &corr,
        MAX_BOLUS,
        TEST_DATE,
        CURRENT_GLUCOSE,
        &setup_target(),
    );
    assert_eq!(rec.amount_iu, 0.0);
    assert!(matches!(
        rec.notice,
        Some(BolusRecommendationNotice::PredictedGlucoseInRange)
    ));
}

// ── testStartHighEndLow ──────────────────────────────────────────────────────
// Swift: Tests/LoopAlgorithmTests/CorrectionDosingTests.swift:232 testStartHighEndLow

fn start_high_end_low_prediction() -> Vec<(f64, f64)> {
    vec![
        (h(0.0), 200.0),
        (h(0.5), 160.0),
        (h(1.0), 120.0),
        (h(1.5), 80.0),
        (h(6.2), 60.0),
    ]
}

#[test]
fn test_start_high_end_low_temp_basal() {
    let corr = correction(&start_high_end_low_prediction());
    let rec = recommend_temp_basal(
        &corr,
        BASAL_RATE,
        0.0,
        MAX_BOLUS,
        MAX_BASAL_RATE,
        MAX_ACTIVE_INSULIN,
    );
    assert!((rec.units_per_hour - 0.0).abs() < 1e-9);
    assert!((rec.duration - 1800.0).abs() < 1e-9);
}

#[test]
fn test_start_high_end_low_auto_dose() {
    let corr = correction(&start_high_end_low_prediction());
    let auto = recommend_automatic_dose(&corr, 0.4, BASAL_RATE, 0.0, MAX_BOLUS, MAX_ACTIVE_INSULIN);
    assert_eq!(auto.bolus_units.unwrap_or(0.0), 0.0);
    assert!((auto.basal_adjustment.units_per_hour - 0.0).abs() < 1e-9);
    assert!((auto.basal_adjustment.duration - 1800.0).abs() < 1e-9);
}

#[test]
fn test_start_high_end_low_manual_bolus() {
    let corr = correction(&start_high_end_low_prediction());
    let rec = recommend_manual_bolus(
        &corr,
        MAX_BOLUS,
        TEST_DATE,
        CURRENT_GLUCOSE,
        &setup_target(),
    );
    assert!((rec.amount_iu - 0.0).abs() < 1e-9);
    match &rec.notice {
        Some(BolusRecommendationNotice::AllGlucoseBelowTarget {
            min_glucose_mgdl, ..
        }) => {
            assert!((min_glucose_mgdl - 60.0).abs() < 1.0);
        }
        other => panic!("Expected AllGlucoseBelowTarget, got {:?}", other),
    }
}

// ── testStartLowEndHigh ──────────────────────────────────────────────────────
// Swift: Tests/LoopAlgorithmTests/CorrectionDosingTests.swift:283 testStartLowEndHigh

fn start_low_end_high_prediction() -> Vec<(f64, f64)> {
    vec![
        (h(0.0), 60.0),
        (h(0.5), 80.0),
        (h(1.0), 120.0),
        (h(1.5), 160.0),
        (h(6.2), 200.0),
    ]
}

#[test]
fn test_start_low_end_high_temp_basal() {
    let corr = correction(&start_low_end_high_prediction());
    let rec = recommend_temp_basal(
        &corr,
        BASAL_RATE,
        0.0,
        MAX_BOLUS,
        MAX_BASAL_RATE,
        MAX_ACTIVE_INSULIN,
    );
    assert!((rec.units_per_hour - 1.0).abs() < 0.05);
    assert!((rec.duration - 1800.0).abs() < 1e-9);
}

#[test]
fn test_start_low_end_high_auto_dose() {
    let corr = correction(&start_low_end_high_prediction());
    let auto = recommend_automatic_dose(&corr, 0.4, BASAL_RATE, 0.0, MAX_BOLUS, MAX_ACTIVE_INSULIN);
    assert_eq!(auto.bolus_units.unwrap_or(0.0), 0.0);
    assert!((auto.basal_adjustment.units_per_hour - BASAL_RATE).abs() < 0.05);
    assert!((auto.basal_adjustment.duration - 1800.0).abs() < 1e-9);
}

#[test]
fn test_start_low_end_high_manual_bolus() {
    let corr = correction(&start_low_end_high_prediction());
    let rec = recommend_manual_bolus(
        &corr,
        MAX_BOLUS,
        TEST_DATE,
        CURRENT_GLUCOSE,
        &setup_target(),
    );
    assert!((rec.amount_iu - 1.6).abs() < 0.05);
    match &rec.notice {
        Some(BolusRecommendationNotice::PredictedGlucoseBelowTarget {
            min_glucose_mgdl, ..
        }) => {
            assert!((min_glucose_mgdl - 60.0).abs() < 1.0);
        }
        other => panic!("Expected PredictedGlucoseBelowTarget, got {:?}", other),
    }
}

// ── testFlatAndHigh ──────────────────────────────────────────────────────────
// Swift: Tests/LoopAlgorithmTests/CorrectionDosingTests.swift:334 testFlatAndHigh

fn flat_and_high_prediction() -> Vec<(f64, f64)> {
    vec![(h(0.0), 200.0), (h(6.2), 200.0)]
}

#[test]
fn test_flat_and_high_temp_basal() {
    let corr = correction(&flat_and_high_prediction());
    let rec = recommend_temp_basal(
        &corr,
        BASAL_RATE,
        0.0,
        MAX_BOLUS,
        MAX_BASAL_RATE,
        MAX_ACTIVE_INSULIN,
    );
    assert!((rec.units_per_hour - 3.0).abs() < 0.05);
    assert!((rec.duration - 1800.0).abs() < 1e-9);
}

#[test]
fn test_flat_and_high_auto_dose() {
    let corr = correction(&flat_and_high_prediction());
    let auto = recommend_automatic_dose(&corr, 0.4, BASAL_RATE, 0.0, MAX_BOLUS, MAX_ACTIVE_INSULIN);
    assert!((auto.bolus_units.unwrap_or(0.0) - 0.65).abs() < 0.05);
    assert!((auto.basal_adjustment.units_per_hour - BASAL_RATE).abs() < 0.05);
    assert!((auto.basal_adjustment.duration - 1800.0).abs() < 1e-9);
}

#[test]
fn test_flat_and_high_manual_bolus() {
    let corr = correction(&flat_and_high_prediction());
    let rec = recommend_manual_bolus(
        &corr,
        MAX_BOLUS,
        TEST_DATE,
        CURRENT_GLUCOSE,
        &setup_target(),
    );
    assert!((rec.amount_iu - 1.6).abs() < 0.05);
    assert!(rec.notice.is_none());
}

// ── testHighAndFalling ───────────────────────────────────────────────────────
// Swift: Tests/LoopAlgorithmTests/CorrectionDosingTests.swift:382 testHighAndFalling

fn high_and_falling_prediction() -> Vec<(f64, f64)> {
    vec![
        (h(0.0), 240.0),
        (h(1.0), 220.0),
        (h(2.0), 200.0),
        (h(3.0), 160.0),
        (h(6.2), 124.0),
    ]
}

#[test]
fn test_high_and_falling_temp_basal() {
    let corr = correction(&high_and_falling_prediction());
    let rec = recommend_temp_basal(
        &corr,
        BASAL_RATE,
        0.0,
        MAX_BOLUS,
        MAX_BASAL_RATE,
        MAX_ACTIVE_INSULIN,
    );
    assert!((rec.units_per_hour - 1.63).abs() < 0.05);
    assert!((rec.duration - 1800.0).abs() < 1e-9);
}

#[test]
fn test_high_and_falling_auto_dose() {
    let corr = correction(&high_and_falling_prediction());
    let auto = recommend_automatic_dose(&corr, 0.4, BASAL_RATE, 0.0, MAX_BOLUS, MAX_ACTIVE_INSULIN);
    assert!((auto.bolus_units.unwrap_or(0.0) - 0.10).abs() < 0.05);
    assert!((auto.basal_adjustment.units_per_hour - BASAL_RATE).abs() < 0.05);
    assert!((auto.basal_adjustment.duration - 1800.0).abs() < 1e-9);
}

#[test]
fn test_high_and_falling_manual_bolus() {
    let corr = correction(&high_and_falling_prediction());
    let rec = recommend_manual_bolus(
        &corr,
        MAX_BOLUS,
        TEST_DATE,
        CURRENT_GLUCOSE,
        &setup_target(),
    );
    assert!((rec.amount_iu - 0.30).abs() < 0.05);
    assert!(rec.notice.is_none());
}

// ── testInRangeAndRising ─────────────────────────────────────────────────────
// Swift: Tests/LoopAlgorithmTests/CorrectionDosingTests.swift:429 testInRangeAndRising

fn in_range_and_rising_prediction() -> Vec<(f64, f64)> {
    vec![
        (h(0.0), 90.0),
        (h(1.0), 100.0),
        (h(2.0), 110.0),
        (h(3.0), 120.0),
        (h(6.2), 125.0),
    ]
}

#[test]
fn test_in_range_and_rising_temp_basal() {
    let corr = correction(&in_range_and_rising_prediction());
    let rec = recommend_temp_basal(
        &corr,
        BASAL_RATE,
        0.0,
        MAX_BOLUS,
        MAX_BASAL_RATE,
        MAX_ACTIVE_INSULIN,
    );
    assert!((rec.units_per_hour - 1.63).abs() < 0.05);
    assert!((rec.duration - 1800.0).abs() < 1e-9);
}

#[test]
fn test_in_range_and_rising_auto_dose() {
    let corr = correction(&in_range_and_rising_prediction());
    let auto = recommend_automatic_dose(&corr, 0.4, BASAL_RATE, 0.0, MAX_BOLUS, MAX_ACTIVE_INSULIN);
    assert!((auto.bolus_units.unwrap_or(0.0) - 0.10).abs() < 0.05);
    assert!((auto.basal_adjustment.units_per_hour - BASAL_RATE).abs() < 0.05);
    assert!((auto.basal_adjustment.duration - 1800.0).abs() < 1e-9);
}

#[test]
fn test_in_range_and_rising_manual_bolus() {
    let corr = correction(&in_range_and_rising_prediction());
    let rec = recommend_manual_bolus(
        &corr,
        MAX_BOLUS,
        TEST_DATE,
        CURRENT_GLUCOSE,
        &setup_target(),
    );
    assert!((rec.amount_iu - 0.30).abs() < 0.05);
    assert!(rec.notice.is_none());
}

// ── testHighAndRising ────────────────────────────────────────────────────────
// Swift: Tests/LoopAlgorithmTests/CorrectionDosingTests.swift:476 testHighAndRising

fn high_and_rising_prediction() -> Vec<(f64, f64)> {
    vec![
        (h(0.0), 140.0),
        (h(1.0), 150.0),
        (h(2.0), 160.0),
        (h(3.0), 170.0),
        (h(6.2), 180.0),
    ]
}

#[test]
fn test_high_and_rising_temp_basal() {
    let corr = correction(&high_and_rising_prediction());
    let rec = recommend_temp_basal(
        &corr,
        BASAL_RATE,
        0.0,
        MAX_BOLUS,
        MAX_BASAL_RATE,
        MAX_ACTIVE_INSULIN,
    );
    assert!((rec.units_per_hour - 3.0).abs() < 0.05);
    assert!((rec.duration - 1800.0).abs() < 1e-9);
}

#[test]
fn test_high_and_rising_auto_dose() {
    let corr = correction(&high_and_rising_prediction());
    let auto = recommend_automatic_dose(&corr, 0.4, BASAL_RATE, 0.0, MAX_BOLUS, MAX_ACTIVE_INSULIN);
    assert!((auto.bolus_units.unwrap_or(0.0) - 0.5).abs() < 0.05);
    assert!((auto.basal_adjustment.units_per_hour - BASAL_RATE).abs() < 0.05);
    assert!((auto.basal_adjustment.duration - 1800.0).abs() < 1e-9);
}

#[test]
fn test_high_and_rising_manual_bolus() {
    let corr = correction(&high_and_rising_prediction());
    let rec = recommend_manual_bolus(
        &corr,
        MAX_BOLUS,
        TEST_DATE,
        CURRENT_GLUCOSE,
        &setup_target(),
    );
    assert!((rec.amount_iu - 1.25).abs() < 0.05);
    assert!(rec.notice.is_none());
}

// ── testVeryLowAndRising ─────────────────────────────────────────────────────
// Swift: Tests/LoopAlgorithmTests/CorrectionDosingTests.swift:523 testVeryLowAndRising

fn very_low_and_rising_prediction() -> Vec<(f64, f64)> {
    vec![
        (h(0.0), 60.0),
        (h(1.0), 50.0),
        (h(2.0), 60.0),
        (h(3.0), 70.0),
        (h(6.2), 100.0),
    ]
}

#[test]
fn test_very_low_and_rising_temp_basal() {
    let corr = correction(&very_low_and_rising_prediction());
    let rec = recommend_temp_basal(
        &corr,
        BASAL_RATE,
        0.0,
        MAX_BOLUS,
        MAX_BASAL_RATE,
        MAX_ACTIVE_INSULIN,
    );
    assert!((rec.units_per_hour - 0.0).abs() < 0.05);
    assert!((rec.duration - 1800.0).abs() < 1e-9);
}

#[test]
fn test_very_low_and_rising_auto_dose() {
    let corr = correction(&very_low_and_rising_prediction());
    let auto = recommend_automatic_dose(&corr, 0.4, BASAL_RATE, 0.0, MAX_BOLUS, MAX_ACTIVE_INSULIN);
    assert!((auto.bolus_units.unwrap_or(0.0) - 0.0).abs() < 0.05);
    assert!((auto.basal_adjustment.units_per_hour - 0.0).abs() < 0.05);
    assert!((auto.basal_adjustment.duration - 1800.0).abs() < 1e-9);
}

#[test]
fn test_very_low_and_rising_manual_bolus() {
    let corr = correction(&very_low_and_rising_prediction());
    let rec = recommend_manual_bolus(
        &corr,
        MAX_BOLUS,
        TEST_DATE,
        CURRENT_GLUCOSE,
        &setup_target(),
    );
    assert!((rec.amount_iu - 0.0).abs() < 0.05);
    match &rec.notice {
        Some(BolusRecommendationNotice::GlucoseBelowSuspendThreshold {
            min_glucose_mgdl, ..
        }) => {
            assert!((min_glucose_mgdl - 50.0).abs() < 1.0);
        }
        other => panic!("Expected GlucoseBelowSuspendThreshold, got {:?}", other),
    }
}
