//! Coordinate geometry (COGO) calculations for surveying workflows.
//!
//! Bearings are whole-circle azimuths measured clockwise from grid north, in degrees.
//! Coordinates use easting (X) and northing (Y), in the drawing's declared linear unit.
use crate::{SurveyError, Vec2};
use serde::{Deserialize, Serialize};

const DEGREES_PER_TURN: f64 = 360.0;
const ANGULAR_EPS: f64 = 1.0e-12;

fn validate_point(point: Vec2) -> Result<(), SurveyError> {
    if point.x.is_finite() && point.y.is_finite() { Ok(()) } else { Err(SurveyError::InvalidValue("COGO coordinate must be finite".into())) }
}

/// An inverse solution from two coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct InverseResult {
    /// Horizontal distance in drawing units.
    pub distance: f64,
    /// Whole-circle azimuth clockwise from north, in degrees [0, 360).
    pub azimuth_degrees: f64,
}

/// A bearing-distance traverse leg.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TraverseLeg {
    pub azimuth_degrees: f64,
    pub distance: f64,
}

/// Closure diagnostics for a traverse.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TraverseClosure {
    pub computed_end: Vec2,
    pub misclosure_easting: f64,
    pub misclosure_northing: f64,
    pub linear_misclosure: f64,
    pub total_length: f64,
    /// Total traverse length divided by linear misclosure. None means exact closure.
    pub precision_ratio: Option<f64>,
}

/// Computes the inverse between two points using whole-circle azimuth from grid north.
pub fn inverse(from: Vec2, to: Vec2) -> Result<InverseResult, SurveyError> {
    validate_point(from)?;
    validate_point(to)?;
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let distance = dx.hypot(dy);
    if !distance.is_finite() {
        return Err(SurveyError::InvalidValue("inverse distance overflowed".into()));
    }
    let azimuth_degrees = if distance == 0.0 { 0.0 } else { dx.atan2(dy).to_degrees().rem_euclid(DEGREES_PER_TURN) };
    Ok(InverseResult { distance, azimuth_degrees })
}

/// Computes a destination from a start coordinate, azimuth clockwise from north, and distance.
pub fn direct(start: Vec2, azimuth_degrees: f64, distance: f64) -> Result<Vec2, SurveyError> {
    validate_point(start)?;
    if !azimuth_degrees.is_finite() || !distance.is_finite() || distance < 0.0 {
        return Err(SurveyError::InvalidValue("azimuth must be finite and distance must be finite and non-negative".into()));
    }
    let azimuth = azimuth_degrees.to_radians();
    let result = Vec2 { x: start.x + distance * azimuth.sin(), y: start.y + distance * azimuth.cos() };
    validate_point(result)?;
    Ok(result)
}

/// Intersects two infinite lines defined by point pairs. Parallel or coincident lines are rejected.
pub fn line_intersection(a1: Vec2, a2: Vec2, b1: Vec2, b2: Vec2) -> Result<Vec2, SurveyError> {
    for point in [a1, a2, b1, b2] {
        validate_point(point)?;
    }
    let (rx, ry) = (a2.x - a1.x, a2.y - a1.y);
    let (sx, sy) = (b2.x - b1.x, b2.y - b1.y);
    let denominator = rx * sy - ry * sx;
    let scale = rx.hypot(ry) * sx.hypot(sy);
    if !denominator.is_finite() || !scale.is_finite() || scale == 0.0 || denominator.abs() <= ANGULAR_EPS * scale {
        return Err(SurveyError::InvalidValue("lines are parallel, coincident, or degenerate".into()));
    }
    let qpx = b1.x - a1.x;
    let qpy = b1.y - a1.y;
    let t = (qpx * sy - qpy * sx) / denominator;
    let result = Vec2 { x: a1.x + t * rx, y: a1.y + t * ry };
    validate_point(result)?;
    Ok(result)
}

/// Computes the coordinate closure of a sequence of bearing-distance legs.
pub fn traverse_closure(start: Vec2, legs: &[TraverseLeg], known_end: Vec2) -> Result<TraverseClosure, SurveyError> {
    validate_point(start)?;
    validate_point(known_end)?;
    if legs.is_empty() {
        return Err(SurveyError::NotEnoughPoints(1));
    }
    let mut computed = start;
    let mut total_length = 0.0;
    for leg in legs {
        computed = direct(computed, leg.azimuth_degrees, leg.distance)?;
        total_length += leg.distance;
        if !total_length.is_finite() {
            return Err(SurveyError::InvalidValue("traverse length overflowed".into()));
        }
    }
    let misclosure_easting = known_end.x - computed.x;
    let misclosure_northing = known_end.y - computed.y;
    let linear_misclosure = misclosure_easting.hypot(misclosure_northing);
    if !linear_misclosure.is_finite() {
        return Err(SurveyError::InvalidValue("traverse misclosure overflowed".into()));
    }
    let precision_ratio = if linear_misclosure <= f64::EPSILON * total_length.max(1.0) { None } else { Some(total_length / linear_misclosure) };
    Ok(TraverseClosure { computed_end: computed, misclosure_easting, misclosure_northing, linear_misclosure, total_length, precision_ratio })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_and_direct_are_consistent_for_cardinal_and_oblique_bearings() {
        let origin = Vec2 { x: 100.0, y: 200.0 };
        for azimuth in [0.0, 45.0, 90.0, 180.0, 270.0, 359.0] {
            let end = direct(origin, azimuth, 125.5);
            assert!(end.is_ok());
            if let Ok(end) = end {
                let result = inverse(origin, end);
                assert!(result.is_ok());
                if let Ok(result) = result {
                    assert!((result.distance - 125.5).abs() < 1.0e-9);
                    assert!((result.azimuth_degrees - azimuth).abs() < 1.0e-9);
                }
            }
        }
    }

    #[test]
    fn intersects_two_lines() {
        let hit = line_intersection(Vec2 { x: 0.0, y: 0.0 }, Vec2 { x: 10.0, y: 10.0 }, Vec2 { x: 0.0, y: 10.0 }, Vec2 { x: 10.0, y: 0.0 });
        assert_eq!(hit, Ok(Vec2 { x: 5.0, y: 5.0 }));
    }

    #[test]
    fn reports_traverse_misclosure_and_precision() {
        let start = Vec2 { x: 0.0, y: 0.0 };
        let legs = [TraverseLeg { azimuth_degrees: 90.0, distance: 100.0 }, TraverseLeg { azimuth_degrees: 0.0, distance: 100.0 }];
        let closure = traverse_closure(start, &legs, Vec2 { x: 100.0, y: 99.0 });
        assert!(closure.is_ok());
        if let Ok(closure) = closure {
            assert!((closure.linear_misclosure - 1.0).abs() < 1.0e-9);
            assert!((closure.precision_ratio.unwrap_or_default() - 200.0).abs() < 1.0e-8);
        }
    }

    #[test]
    fn rejects_invalid_distances_and_parallel_lines() {
        assert!(direct(Vec2 { x: 0.0, y: 0.0 }, 90.0, -1.0).is_err());
        assert!(line_intersection(Vec2 { x: 0.0, y: 0.0 }, Vec2 { x: 1.0, y: 0.0 }, Vec2 { x: 0.0, y: 1.0 }, Vec2 { x: 1.0, y: 1.0 },).is_err());
    }
}
