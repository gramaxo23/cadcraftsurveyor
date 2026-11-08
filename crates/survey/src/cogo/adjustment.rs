//! Traverse adjustment helpers for closed and connected control traverses.
use crate::{SurveyError, Vec2};
use serde::{Deserialize, Serialize};

use super::cogo::{TraverseClosure, TraverseLeg, direct, traverse_closure};

/// A traverse leg with its computed endpoint and Bowditch-adjusted endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdjustedTraverseLeg {
    pub start: Vec2,
    pub computed_end: Vec2,
    pub adjusted_end: Vec2,
    pub correction_easting: f64,
    pub correction_northing: f64,
}

/// Result of a Bowditch (compass-rule) traverse adjustment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BowditchAdjustment {
    pub closure: TraverseClosure,
    pub adjusted_legs: Vec<AdjustedTraverseLeg>,
    pub adjusted_end: Vec2,
}

/// Distributes coordinate misclosure in proportion to each leg's length.
///
/// This is a mathematical adjustment utility, not a substitute for a least-squares
/// network adjustment, observation weighting, blunder detection, or surveyor approval.
pub fn bowditch_adjust(start: Vec2, legs: &[TraverseLeg], known_end: Vec2) -> Result<BowditchAdjustment, SurveyError> {
    let closure = traverse_closure(start, legs, known_end)?;
    if !closure.total_length.is_finite() || closure.total_length <= 0.0 {
        return Err(SurveyError::InvalidValue("Bowditch adjustment requires positive total traverse length".into()));
    }

    let mut output = Vec::with_capacity(legs.len());
    let mut current_computed = start;
    let mut current_adjusted = start;
    for leg in legs {
        let computed_end = direct(current_computed, leg.azimuth_degrees, leg.distance)?;
        let correction_easting = closure.misclosure_easting * (leg.distance / closure.total_length);
        let correction_northing = closure.misclosure_northing * (leg.distance / closure.total_length);
        let adjusted_end = Vec2 {
            x: current_adjusted.x + (computed_end.x - current_computed.x) + correction_easting,
            y: current_adjusted.y + (computed_end.y - current_computed.y) + correction_northing,
        };
        if !adjusted_end.x.is_finite() || !adjusted_end.y.is_finite() {
            return Err(SurveyError::InvalidValue("Bowditch adjustment produced a non-finite coordinate".into()));
        }
        output.push(AdjustedTraverseLeg { start: current_adjusted, computed_end, adjusted_end, correction_easting, correction_northing });
        current_computed = computed_end;
        current_adjusted = adjusted_end;
    }
    // Pin the final adjusted coordinate to the supplied control endpoint to remove
    // the final floating-point accumulation residual.
    if let Some(last) = output.last_mut() {
        last.adjusted_end = known_end;
    }
    Ok(BowditchAdjustment { closure, adjusted_legs: output, adjusted_end: known_end })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bowditch_adjustment_closes_and_distributes_correction_by_length() {
        let start = Vec2 { x: 0.0, y: 0.0 };
        let legs = [
            TraverseLeg { azimuth_degrees: 90.0, distance: 100.0 },
            TraverseLeg { azimuth_degrees: 0.0, distance: 100.0 },
        ];
        let result = bowditch_adjust(start, &legs, Vec2 { x: 100.0, y: 99.0 });
        assert!(result.is_ok());
        if let Ok(result) = result {
            assert_eq!(result.adjusted_end, Vec2 { x: 100.0, y: 99.0 });
            assert_eq!(result.adjusted_legs.len(), 2);
            assert!((result.adjusted_legs[0].correction_northing + 0.5).abs() < 1.0e-9);
            assert!((result.adjusted_legs[1].correction_northing + 0.5).abs() < 1.0e-9);
        }
    }

    #[test]
    fn rejects_zero_length_traverse() {
        let leg = [TraverseLeg { azimuth_degrees: 0.0, distance: 0.0 }];
        assert!(bowditch_adjust(Vec2 { x: 0.0, y: 0.0 }, &leg, Vec2 { x: 0.0, y: 0.0 }).is_err());
    }
}
