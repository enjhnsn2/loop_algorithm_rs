use loop_algorithm::glucose::{
    contains_calibrations, counteraction_effects, has_gradual_transitions, has_single_provenance,
    is_continuous, GlucoseSample,
};
use loop_algorithm::types::GlucoseEffect;

fn sample(start: f64, value_mgdl: f64) -> GlucoseSample {
    GlucoseSample {
        start,
        value_mgdl,
        provenance: 0,
        is_display_only: false,
    }
}

fn sample_with(
    start: f64,
    value_mgdl: f64,
    provenance: u32,
    is_display_only: bool,
) -> GlucoseSample {
    GlucoseSample {
        start,
        value_mgdl,
        provenance,
        is_display_only,
    }
}

// ── hasGradualTransitions ─────────────────────────────────────────────────────

// Swift: Tests/LoopAlgorithmTests/GlucoseMathTests.swift:74 testHasGradualTransitions_SingleSample_ReturnsFalse
#[test]
fn test_has_gradual_transitions_single_sample_returns_false() {
    let samples = vec![sample(0.0, 120.0)];
    assert!(!has_gradual_transitions(&samples, 40.0));
}

// Swift: Tests/LoopAlgorithmTests/GlucoseMathTests.swift:82 testHasGradualTransitions_TwoSamplesWithinThreshold_ReturnsTrue
#[test]
fn test_has_gradual_transitions_two_samples_within_threshold_returns_true() {
    let samples = vec![sample(0.0, 100.0), sample(300.0, 110.0)];
    assert!(has_gradual_transitions(&samples, 40.0));
}

// Swift: Tests/LoopAlgorithmTests/GlucoseMathTests.swift:92 testHasGradualTransitions_TwoSamplesExceedingThreshold_ReturnsFalse
#[test]
fn test_has_gradual_transitions_two_samples_exceeding_threshold_returns_false() {
    let samples = vec![sample(0.0, 100.0), sample(300.0, 150.0)];
    assert!(!has_gradual_transitions(&samples, 40.0));
}

// Swift: Tests/LoopAlgorithmTests/GlucoseMathTests.swift:102 testHasGradualTransitions_MultipleSamplesAllWithinThreshold_ReturnsTrue
#[test]
fn test_has_gradual_transitions_multiple_samples_all_within_threshold_returns_true() {
    let samples = vec![
        sample(0.0, 100.0),
        sample(300.0, 115.0),
        sample(600.0, 125.0),
        sample(900.0, 118.0),
    ];
    assert!(has_gradual_transitions(&samples, 40.0));
}

// Swift: Tests/LoopAlgorithmTests/GlucoseMathTests.swift:115 testHasGradualTransitions_OneJumpExceedsThreshold_ReturnsFalse
#[test]
fn test_has_gradual_transitions_one_jump_exceeds_threshold_returns_false() {
    let samples = vec![
        sample(0.0, 100.0),
        sample(300.0, 115.0),
        sample(600.0, 160.0), // +45 mg/dL jump
    ];
    assert!(!has_gradual_transitions(&samples, 40.0));
}

// Swift: Tests/LoopAlgorithmTests/GlucoseMathTests.swift:127 testHasGradualTransitions_CustomThreshold
#[test]
fn test_has_gradual_transitions_custom_threshold() {
    let samples = vec![sample(0.0, 100.0), sample(300.0, 150.0)]; // +50 mg/dL
    assert!(has_gradual_transitions(&samples, 55.0));
    assert!(!has_gradual_transitions(&samples, 45.0));
}

// ── isContinuous ─────────────────────────────────────────────────────────────

// Swift: Tests/LoopAlgorithmTests/GlucoseMathTests.swift:143 testIsContinuous_EmptyCollection_ReturnsFalse
#[test]
fn test_is_continuous_empty_collection_returns_false() {
    let samples: Vec<GlucoseSample> = vec![];
    assert!(!is_continuous(&samples, 5.5 * 60.0));
}

// Swift: Tests/LoopAlgorithmTests/GlucoseMathTests.swift:149 testIsContinuous_Regular5MinSpacing_ReturnsTrue
#[test]
fn test_is_continuous_regular_5min_spacing_returns_true() {
    // 6 samples at 5-min intervals; tolerance = 5.5 min per sample
    let samples: Vec<GlucoseSample> = (0..6)
        .map(|i| sample(i as f64 * 300.0, 100.0 + i as f64))
        .collect();
    assert!(is_continuous(&samples, 5.5 * 60.0));
}

// Swift: Tests/LoopAlgorithmTests/GlucoseMathTests.swift:159 testIsContinuous_GapLargerThanTolerance_ReturnsFalse
#[test]
fn test_is_continuous_gap_larger_than_tolerance_returns_false() {
    let samples = vec![
        sample(0.0, 100.0),
        sample(300.0, 105.0),
        sample(1200.0, 110.0), // 15-min gap
    ];
    assert!(!is_continuous(&samples, 6.0 * 60.0));
}

// ── containsCalibrations ──────────────────────────────────────────────────────

// Swift: Tests/LoopAlgorithmTests/GlucoseMathTests.swift:171 testContainsCalibrations_NoCalibrations_ReturnsFalse
#[test]
fn test_contains_calibrations_no_calibrations_returns_false() {
    let samples: Vec<GlucoseSample> = (0..3).map(|i| sample(i as f64 * 300.0, 100.0)).collect();
    assert!(!contains_calibrations(&samples));
}

// Swift: Tests/LoopAlgorithmTests/GlucoseMathTests.swift:177 testContainsCalibrations_HasCalibration_ReturnsTrue
#[test]
fn test_contains_calibrations_has_calibration_returns_true() {
    let samples = vec![
        sample(0.0, 100.0),
        sample_with(300.0, 105.0, 0, true), // display-only / calibration
    ];
    assert!(contains_calibrations(&samples));
}

// ── hasSingleProvenance ───────────────────────────────────────────────────────

// Swift: Tests/LoopAlgorithmTests/GlucoseMathTests.swift:188 testHasSingleProvenance_AllSame_ReturnsTrue
#[test]
fn test_has_single_provenance_all_same_returns_true() {
    let samples: Vec<GlucoseSample> = (0..4)
        .map(|i| sample_with(i as f64 * 300.0, 100.0, 1, false))
        .collect();
    assert!(has_single_provenance(&samples));
}

// Swift: Tests/LoopAlgorithmTests/GlucoseMathTests.swift:194 testHasSingleProvenance_DifferentProvenance_ReturnsFalse
#[test]
fn test_has_single_provenance_different_provenance_returns_false() {
    let samples = vec![
        sample_with(0.0, 100.0, 1, false),
        sample_with(300.0, 105.0, 2, false),
    ];
    assert!(!has_single_provenance(&samples));
}

// ── counteractionEffects (no-glucose case) ────────────────────────────────────

// Swift: Tests/LoopAlgorithmTests/GlucoseMathTests.swift:425 testCounteractionEffectsForNoGlucose
#[test]
fn test_counteraction_effects_for_no_glucose_returns_empty() {
    let glucose_samples: Vec<GlucoseSample> = vec![];
    let insulin_effects: Vec<GlucoseEffect> = vec![
        GlucoseEffect {
            start: 0.0,
            value_mgdl: 0.0,
        },
        GlucoseEffect {
            start: 300.0,
            value_mgdl: -5.0,
        },
    ];
    let effects = counteraction_effects(&glucose_samples, &insulin_effects);
    assert_eq!(effects.len(), 0);
}
