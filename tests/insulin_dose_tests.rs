use loop_algorithm::insulin::{
    annotated_doses, BasalDoseType, ExponentialInsulinModel, InsulinDeliveryType, InsulinDose,
    ModelPreset,
};
use loop_algorithm::types::ScheduleEntry;

fn default_model() -> ExponentialInsulinModel {
    ModelPreset::RapidActingAdult.model()
}

fn basal_dose(start: f64, end: f64, volume_iu: f64) -> InsulinDose {
    InsulinDose {
        delivery_type: InsulinDeliveryType::Basal,
        start,
        end,
        volume_iu,
        model: default_model(),
    }
}

// ── annotated_doses ───────────────────────────────────────────────────────────

// Swift: Tests/LoopAlgorithmTests/InsulinDoseTests.swift:14 testAnnotatedWithSingleBasalSchedule
#[test]
fn test_annotated_with_single_basal_schedule() {
    let start = 0.0f64;
    let end = start + 3600.0;
    let dose = basal_dose(start, end, 1.0);
    let basal_history = vec![ScheduleEntry::new(start, end, 1.0)];

    let annotated = annotated_doses(&[dose], &basal_history, false);

    assert_eq!(annotated.len(), 1);
    assert!(matches!(
        annotated[0].dose_type,
        BasalDoseType::Basal { scheduled_rate_iuhr } if (scheduled_rate_iuhr - 1.0).abs() < 1e-9
    ));
    assert!((annotated[0].start - start).abs() < 1e-9);
    assert!((annotated[0].end - end).abs() < 1e-9);
    assert!((annotated[0].volume_iu - 1.0).abs() < 1e-9);
}

// Swift: Tests/LoopAlgorithmTests/InsulinDoseTests.swift:38 testAnnotatedWithBasalEndingBeforeDose
#[test]
fn test_annotated_with_basal_ending_before_dose() {
    let start = 0.0f64;
    let middle = start + 1800.0;
    let end = start + 3600.0;
    let dose = basal_dose(start, end, 1.0);
    // Schedule only covers the first half
    let basal_history = vec![ScheduleEntry::new(start, middle, 1.0)];

    let annotated = annotated_doses(&[dose], &basal_history, false);

    assert_eq!(annotated.len(), 1);
    assert!(matches!(
        annotated[0].dose_type,
        BasalDoseType::Basal { scheduled_rate_iuhr } if (scheduled_rate_iuhr - 1.0).abs() < 1e-9
    ));
    assert!((annotated[0].start - start).abs() < 1e-9);
    assert!((annotated[0].end - end).abs() < 1e-9);
    assert!((annotated[0].volume_iu - 1.0).abs() < 1e-9);
}

// Swift: Tests/LoopAlgorithmTests/InsulinDoseTests.swift:64 testAnnotatedWithMultipleBasalSchedules
#[test]
fn test_annotated_with_multiple_basal_schedules() {
    let start = 0.0f64;
    let middle = start + 1800.0;
    let end = start + 3600.0;
    let dose = basal_dose(start, end, 2.0);
    let basal_history = vec![
        ScheduleEntry::new(start, middle, 1.0),
        ScheduleEntry::new(middle, end, 2.0),
    ];

    let annotated = annotated_doses(&[dose], &basal_history, false);

    assert_eq!(annotated.len(), 2);

    assert!(matches!(
        annotated[0].dose_type,
        BasalDoseType::Basal { scheduled_rate_iuhr } if (scheduled_rate_iuhr - 1.0).abs() < 1e-9
    ));
    assert!((annotated[0].start - start).abs() < 1e-9);
    assert!((annotated[0].end - middle).abs() < 1e-9);
    assert!((annotated[0].volume_iu - 1.0).abs() < 1e-9);

    assert!(matches!(
        annotated[1].dose_type,
        BasalDoseType::Basal { scheduled_rate_iuhr } if (scheduled_rate_iuhr - 2.0).abs() < 1e-9
    ));
    assert!((annotated[1].start - middle).abs() < 1e-9);
    assert!((annotated[1].end - end).abs() < 1e-9);
    assert!((annotated[1].volume_iu - 1.0).abs() < 1e-9);
}

// Swift: Tests/LoopAlgorithmTests/InsulinDoseTests.swift:96 testAnnotatedWithOverlappingBasalSchedules
#[test]
fn test_annotated_with_overlapping_basal_schedules() {
    let start = 0.0f64;
    let m1 = start + 1200.0; // 20 min
    let m2 = start + 2400.0; // 40 min
    let end = start + 3600.0; // 1 hour
    let dose = basal_dose(start, end, 3.0);
    let basal_history = vec![
        ScheduleEntry::new(start, m1, 1.0),
        ScheduleEntry::new(m1, m2, 1.5),
        ScheduleEntry::new(m2, end, 2.0),
    ];

    let annotated = annotated_doses(&[dose], &basal_history, false);

    assert_eq!(annotated.len(), 3);

    assert!(matches!(
        annotated[0].dose_type,
        BasalDoseType::Basal { scheduled_rate_iuhr } if (scheduled_rate_iuhr - 1.0).abs() < 1e-9
    ));
    assert!((annotated[0].start - start).abs() < 1e-9);
    assert!((annotated[0].end - m1).abs() < 1e-9);
    assert!((annotated[0].volume_iu - 1.0).abs() < 1e-9);

    assert!(matches!(
        annotated[1].dose_type,
        BasalDoseType::Basal { scheduled_rate_iuhr } if (scheduled_rate_iuhr - 1.5).abs() < 1e-9
    ));
    assert!((annotated[1].start - m1).abs() < 1e-9);
    assert!((annotated[1].end - m2).abs() < 1e-9);
    assert!((annotated[1].volume_iu - 1.0).abs() < 1e-9);

    assert!(matches!(
        annotated[2].dose_type,
        BasalDoseType::Basal { scheduled_rate_iuhr } if (scheduled_rate_iuhr - 2.0).abs() < 1e-9
    ));
    assert!((annotated[2].start - m2).abs() < 1e-9);
    assert!((annotated[2].end - end).abs() < 1e-9);
    assert!((annotated[2].volume_iu - 1.0).abs() < 1e-9);
}

// Swift: Tests/LoopAlgorithmTests/InsulinDoseTests.swift:135 testAnnotatedWithZeroDuration
#[test]
fn test_annotated_with_zero_duration() {
    let start = 0.0f64;
    let dose = basal_dose(start, start, 0.0);
    let basal_history = vec![ScheduleEntry::new(start, start + 3600.0, 1.0)];

    let annotated = annotated_doses(&[dose], &basal_history, false);

    assert_eq!(annotated.len(), 1);
    assert!(matches!(
        annotated[0].dose_type,
        BasalDoseType::Basal { scheduled_rate_iuhr } if (scheduled_rate_iuhr - 1.0).abs() < 1e-9
    ));
    assert!((annotated[0].start - start).abs() < 1e-9);
    assert!((annotated[0].end - start).abs() < 1e-9);
    assert!((annotated[0].volume_iu - 0.0).abs() < 1e-9);
}
