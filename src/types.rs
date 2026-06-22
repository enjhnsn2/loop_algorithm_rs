/// Seconds since Unix epoch.
pub type Timestamp = f64;

/// Floor `t` to the nearest multiple of `interval`.
pub fn floor_to(t: Timestamp, interval: f64) -> Timestamp {
    if interval == 0.0 {
        return t;
    }
    libm::floor(t / interval) * interval
}

/// Ceil `t` to the nearest multiple of `interval`.
pub fn ceil_to(t: Timestamp, interval: f64) -> Timestamp {
    if interval == 0.0 {
        return t;
    }
    libm::ceil(t / interval) * interval
}

// ── Schedule ─────────────────────────────────────────────────────────────────

/// A value valid over `[start, end)`.
#[derive(Clone, Debug)]
pub struct ScheduleEntry<T> {
    pub start: Timestamp,
    pub end: Timestamp,
    pub value: T,
}

impl<T> ScheduleEntry<T> {
    pub fn new(start: Timestamp, end: Timestamp, value: T) -> Self {
        Self { start, end, value }
    }

    pub fn duration(&self) -> f64 {
        self.end - self.start
    }
}

/// The entry whose `start <= date`, or `None`.  Assumes `entries` is sorted by start.
pub fn closest_prior<T>(
    entries: &[ScheduleEntry<T>],
    date: Timestamp,
) -> Option<&ScheduleEntry<T>> {
    entries.iter().rev().find(|e| e.start <= date)
}

/// All entries that overlap `[start, end]` (either bound may be `None` = open).
pub fn filter_date_range<T>(
    entries: &[ScheduleEntry<T>],
    start: Option<Timestamp>,
    end: Option<Timestamp>,
) -> Vec<&ScheduleEntry<T>> {
    entries
        .iter()
        .filter(|e| {
            if let Some(s) = start {
                if e.end < s {
                    return false;
                }
            }
            if let Some(e_end) = end {
                if e.start > e_end {
                    return false;
                }
            }
            true
        })
        .collect()
}

/// Trim schedule entries to `[start, end]`, clipping their own start/end as needed.
pub fn trim_schedule(
    entries: &[ScheduleEntry<f64>],
    start: Option<Timestamp>,
    end: Option<Timestamp>,
) -> Vec<ScheduleEntry<f64>> {
    entries
        .iter()
        .filter_map(|e| {
            if let Some(s) = start {
                if e.end < s {
                    return None;
                }
            }
            if let Some(en) = end {
                if e.start > en {
                    return None;
                }
            }
            Some(ScheduleEntry {
                start: start.map_or(e.start, |s| e.start.max(s)),
                end: end.map_or(e.end, |en| e.end.min(en)),
                value: e.value,
            })
        })
        .collect()
}

// ── Glucose types ─────────────────────────────────────────────────────────────

/// Cumulative glucose effect at a point in time (mg/dL).
#[derive(Clone, Debug, PartialEq)]
pub struct GlucoseEffect {
    pub start: Timestamp,
    /// Cumulative mg/dL value (or delta when produced by `subtracting()`).
    pub value_mgdl: f64,
}

/// Rate of glucose change over an interval (mg/dL/s).
#[derive(Clone, Debug)]
pub struct GlucoseEffectVelocity {
    pub start: Timestamp,
    pub end: Timestamp,
    pub value_mgdl_per_sec: f64,
}

impl GlucoseEffectVelocity {
    /// Total glucose change over the velocity's interval (mg/dL).
    pub fn integrated_mgdl(&self) -> f64 {
        self.value_mgdl_per_sec * (self.end - self.start)
    }
}

/// Aggregated glucose change over a date interval (mg/dL total).
#[derive(Clone, Debug)]
pub struct GlucoseChange {
    pub start: Timestamp,
    pub end: Timestamp,
    pub value_mgdl: f64,
}

impl GlucoseChange {
    pub fn append_effect(&mut self, effect: &GlucoseEffect) {
        self.start = self.start.min(effect.start);
        self.end = self.end.max(effect.start); // GlucoseEffect.endDate == startDate
        self.value_mgdl += effect.value_mgdl;
    }
}

/// Predicted glucose value at a future point.
#[derive(Clone, Debug)]
pub struct PredictedGlucose {
    pub start: Timestamp,
    pub value_mgdl: f64,
}

// ── Insulin / Carb value types ────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct InsulinValue {
    pub start: Timestamp,
    pub value_iu: f64,
}

#[derive(Clone, Debug)]
pub struct CarbValue {
    pub start: Timestamp,
    pub end: Timestamp,
    pub value_g: f64,
}

// ── Simulation date range ─────────────────────────────────────────────────────

/// Compute the `[start, end]` range for a simulation, given optional overrides, an effect
/// `duration`, and a time-step `delta`.  Returns `None` if `items` is empty and no overrides
/// are provided.
pub fn simulation_date_range(
    starts: &[Timestamp], // startDate of each sample
    ends: &[Timestamp],   // endDate of each sample (may equal startDate)
    from: Option<Timestamp>,
    to: Option<Timestamp>,
    duration: f64,
    delta: f64,
) -> Option<(Timestamp, Timestamp)> {
    match (from, to) {
        (Some(s), Some(e)) => Some((floor_to(s, delta), ceil_to(e, delta))),
        _ => {
            let min_start = starts.iter().copied().reduce(f64::min)?;
            let max_end = ends.iter().copied().reduce(f64::max)?;
            Some((
                floor_to(from.unwrap_or(min_start), delta),
                ceil_to(to.unwrap_or(max_end + duration), delta),
            ))
        }
    }
}
