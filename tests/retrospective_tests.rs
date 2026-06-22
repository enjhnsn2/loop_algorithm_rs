use loop_algorithm::retrospective::integral_rc_effect;
use loop_algorithm::types::GlucoseChange;

// 2015-07-13T12:02:37Z
// 2015-07-13T00:00:00Z = 1436745600, then + 12*3600+2*60+37 = 43357 → 1436788957
const START_DATE: f64 = 1436788957.0;

fn d(offset_secs: f64) -> f64 {
    START_DATE + offset_secs
}

// LoopMath constants (from algorithm.rs)
const EFFECT_DURATION: f64 = 60.0 * 60.0; // 1 hour (retrospectiveCorrectionEffectDuration)
const GROUPING_INTERVAL: f64 = 30.0 * 60.0; // 30 min (retrospectiveCorrectionGroupingInterval)
const RECENCY_INTERVAL: f64 = 15.0 * 60.0; // 15 min
const DELTA: f64 = 300.0; // 5 min

// Swift: Tests/LoopAlgorithmTests/Mocks/IntegralRetrospectiveCorrectionTests.swift:22 testIntegralRestrospectiveCorrection
#[test]
fn test_integral_retrospective_correction() {
    // +10 mg/dL over 30 minutes
    let discrepancies = vec![GlucoseChange {
        start: d(-30.0 * 60.0),
        end: START_DATE,
        value_mgdl: 10.0,
    }];

    let (effects, _total) = integral_rc_effect(
        START_DATE,
        100.0,
        &discrepancies,
        RECENCY_INTERVAL,
        GROUPING_INTERVAL,
        EFFECT_DURATION,
        DELTA,
    );

    let last = effects.last().expect("effect timeline should not be empty");

    // Expected last value = 110 mg/dL at 2015-07-13T13:00:00Z (= START_DATE + 3443s → 1436792400)
    // floor_to(START_DATE, 300) = floor_to(1436788957, 300) = 1436788800
    // sim_end = ceil_to(1436788800 + 3600, 300) = ceil_to(1436792400, 300) = 1436792400
    assert!(
        (last.value_mgdl - 110.0).abs() < 1.0,
        "Last effect value should be ~110 mg/dL, got {}",
        last.value_mgdl
    );

    let expected_last_start = 1436792400.0; // 2015-07-13T13:00:00Z
    assert!(
        (last.start - expected_last_start).abs() < 1.0,
        "Last effect start should be 2015-07-13T13:00:00Z ({}), got {}",
        expected_last_start,
        last.start
    );
}
