//! Validated parcel boundary geometry and editing operations.
//!
//! Vertices are stored in boundary order. The closing vertex is implicit; an exact repeated
//! copy of the first vertex is removed during construction. Measurements use drawing units.
use crate::{SurveyError, SurveyPoint, Vec2};
use std::fmt::Write as _;

/// Maximum number of vertices accepted for one parcel boundary.
pub const MAX_PARCEL_VERTICES: usize = 4_096;
const GEOMETRY_EPSILON: f64 = f64::EPSILON * 32.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointLocation {
    Outside,
    Boundary,
    Inside,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParcelMetrics {
    pub area: f64,
    pub signed_area: f64,
    pub perimeter: f64,
    pub centroid: Vec2,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParcelEdge {
    /// Zero-based index of the edge's start vertex.
    pub start_index: usize,
    /// Zero-based index of the edge's end vertex (wraps to zero for the closing edge).
    pub end_index: usize,
    /// Whole-circle azimuth clockwise from grid north, in degrees.
    pub azimuth_degrees: f64,
    pub distance: f64,
}

/// A simple, non-self-intersecting closed parcel boundary.
/// Mutating operations validate a candidate ring before replacing the current one.
#[derive(Clone, Debug, PartialEq)]
pub struct ParcelBoundary {
    vertices: Vec<Vec2>,
    metrics: ParcelMetrics,
}

impl ParcelBoundary {
    /// Creates a validated boundary from ordered vertices.
    pub fn new(mut vertices: Vec<Vec2>) -> Result<Self, SurveyError> {
        if vertices.len() > MAX_PARCEL_VERTICES.saturating_add(1) {
            return Err(SurveyError::InvalidParcel(format!("parcel exceeds the {MAX_PARCEL_VERTICES}-vertex limit")));
        }
        if vertices.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
            return Err(SurveyError::InvalidParcel("parcel contains a non-finite coordinate".into()));
        }
        if vertices.len() >= 2 {
            let first = vertices.first().copied().ok_or(SurveyError::NotEnoughPoints(3))?;
            let last = vertices.last().copied().ok_or(SurveyError::NotEnoughPoints(3))?;
            if first == last {
                let _ = vertices.pop();
            }
        }
        if vertices.len() < 3 {
            return Err(SurveyError::NotEnoughPoints(3));
        }
        if vertices.len() > MAX_PARCEL_VERTICES {
            return Err(SurveyError::InvalidParcel(format!("parcel exceeds the {MAX_PARCEL_VERTICES}-vertex limit")));
        }
        let (origin, scale, normalized) = normalize_vertices(&vertices)?;
        validate_simple_ring(&normalized)?;
        let metrics = calculate_metrics(origin, scale, &normalized)?;
        Ok(Self { vertices, metrics })
    }

    /// Builds a boundary from survey points in their existing order.
    pub fn from_points(points: &[SurveyPoint]) -> Result<Self, SurveyError> {
        if points.len() < 3 {
            return Err(SurveyError::NotEnoughPoints(3));
        }
        if points.len() > MAX_PARCEL_VERTICES.saturating_add(1) {
            return Err(SurveyError::InvalidParcel(format!("parcel exceeds the {MAX_PARCEL_VERTICES}-vertex limit")));
        }
        for point in points {
            point.validate()?;
        }
        Self::new(points.iter().map(|p| Vec2 { x: p.x, y: p.y }).collect())
    }

    /// Returns the open vertex ring; the closing edge is implicit.
    pub fn vertices(&self) -> &[Vec2] {
        &self.vertices
    }

    /// Returns area, signed area, perimeter and centroid in drawing units.
    pub const fn metrics(&self) -> ParcelMetrics {
        self.metrics
    }

    /// Returns an edge schedule with whole-circle azimuths clockwise from grid north.
    pub fn edge_table(&self) -> Vec<ParcelEdge> {
        let count = self.vertices.len();
        self.vertices
            .iter()
            .enumerate()
            .filter_map(|(index, start)| {
                let end_index = if index + 1 == count { 0 } else { index + 1 };
                self.vertices.get(end_index).map(|end| {
                    let dx = end.x - start.x;
                    let dy = end.y - start.y;
                    ParcelEdge { start_index: index, end_index, azimuth_degrees: dx.atan2(dy).to_degrees().rem_euclid(360.0), distance: dx.hypot(dy) }
                })
            })
            .collect()
    }

    /// Classifies a point relative to the parcel. Points on an edge or vertex are Boundary.
    pub fn locate(&self, point: Vec2) -> Result<PointLocation, SurveyError> {
        if !point.x.is_finite() || !point.y.is_finite() {
            return Err(SurveyError::InvalidValue("parcel query coordinate is not finite".into()));
        }
        let origin = self.vertices.first().copied().ok_or(SurveyError::NotEnoughPoints(3))?;
        let scale = self.vertices.iter().fold(0.0_f64, |m, p| m.max((p.x - origin.x).abs()).max((p.y - origin.y).abs()));
        if !scale.is_finite() || scale == 0.0 {
            return Err(SurveyError::InvalidParcel("parcel coordinate range is not stable".into()));
        }
        let dx = point.x - origin.x;
        let dy = point.y - origin.y;
        if !dx.is_finite() || !dy.is_finite() {
            return Ok(PointLocation::Outside);
        }
        let query = Vec2 { x: dx / scale, y: dy / scale };
        if !query.x.is_finite() || !query.y.is_finite() {
            return Ok(PointLocation::Outside);
        }
        let normalized = self.vertices.iter().map(|p| Vec2 { x: (p.x - origin.x) / scale, y: (p.y - origin.y) / scale }).collect::<Vec<_>>();
        let count = normalized.len();
        let mut inside = false;
        for i in 0..count {
            let j = if i + 1 == count { 0 } else { i + 1 };
            let a = normalized.get(i).copied().ok_or(SurveyError::DegenerateControls)?;
            let b = normalized.get(j).copied().ok_or(SurveyError::DegenerateControls)?;
            if point_on_segment(a, b, query) {
                return Ok(PointLocation::Boundary);
            }
            if (a.y > query.y) != (b.y > query.y) {
                let crossing_x = a.x + (query.y - a.y) * (b.x - a.x) / (b.y - a.y);
                if query.x < crossing_x {
                    inside = !inside;
                }
            }
        }
        Ok(if inside { PointLocation::Inside } else { PointLocation::Outside })
    }

    /// Moves one vertex; an invalid candidate leaves the original boundary unchanged.
    pub fn move_vertex(&mut self, index: usize, position: Vec2) -> Result<(), SurveyError> {
        let mut candidate = self.vertices.clone();
        let vertex = candidate.get_mut(index).ok_or_else(|| SurveyError::InvalidParcel("vertex index is out of range".into()))?;
        *vertex = position;
        *self = Self::new(candidate)?;
        Ok(())
    }

    /// Inserts a vertex after the selected edge; the final edge wraps to the first vertex.
    pub fn insert_vertex(&mut self, after_edge: usize, position: Vec2) -> Result<(), SurveyError> {
        let count = self.vertices.len();
        if after_edge >= count {
            return Err(SurveyError::InvalidParcel("edge index is out of range".into()));
        }
        let mut candidate = self.vertices.clone();
        if after_edge + 1 == count {
            candidate.push(position);
        } else {
            candidate.insert(after_edge + 1, position);
        }
        *self = Self::new(candidate)?;
        Ok(())
    }

    /// Removes one vertex. A boundary may never contain fewer than three vertices.
    pub fn remove_vertex(&mut self, index: usize) -> Result<(), SurveyError> {
        if index >= self.vertices.len() {
            return Err(SurveyError::InvalidParcel("vertex index is out of range".into()));
        }
        if self.vertices.len() <= 3 {
            return Err(SurveyError::InvalidParcel("a parcel must retain at least three vertices".into()));
        }
        let mut candidate = self.vertices.clone();
        let _ = candidate.remove(index);
        *self = Self::new(candidate)?;
        Ok(())
    }

    /// Formats a parcel summary and edge schedule suitable for text report export.
    /// Units are labelled as drawing units; no CRS or legal area convention is inferred.
    pub fn report(&self) -> String {
        let mut out = String::from("CADCraft Parcel Boundary Report\n================================\n");
        let _ = writeln!(out, "Vertices: {}", self.vertices.len());
        let _ = writeln!(out, "Area: {:.4} drawing units²", self.metrics.area);
        let _ = writeln!(out, "Perimeter: {:.4} drawing units", self.metrics.perimeter);
        let _ = writeln!(out, "Centroid: E {:.4}, N {:.4}", self.metrics.centroid.x, self.metrics.centroid.y);
        let _ = writeln!(out, "Orientation: {}", if self.metrics.signed_area > 0.0 { "counter-clockwise" } else { "clockwise" });
        out.push_str("\nEdge  From→To  Azimuth (°)  Distance\n");
        for edge in self.edge_table() {
            let _ = writeln!(
                out,
                "{:>4}  {:>4}→{:<4}  {:>11.6}  {:>10.4}",
                edge.start_index + 1,
                edge.start_index + 1,
                edge.end_index + 1,
                edge.azimuth_degrees,
                edge.distance
            );
        }
        out
    }
}

fn normalize_vertices(vertices: &[Vec2]) -> Result<(Vec2, f64, Vec<Vec2>), SurveyError> {
    let origin = vertices.first().copied().ok_or(SurveyError::NotEnoughPoints(3))?;
    let mut scale = 0.0_f64;
    for p in vertices {
        let dx = p.x - origin.x;
        let dy = p.y - origin.y;
        if !dx.is_finite() || !dy.is_finite() {
            return Err(SurveyError::InvalidParcel("parcel coordinate range is too large".into()));
        }
        scale = scale.max(dx.abs()).max(dy.abs());
    }
    if !scale.is_finite() || scale == 0.0 {
        return Err(SurveyError::InvalidParcel("parcel has no measurable extent".into()));
    }
    let normalized = vertices.iter().map(|p| Vec2 { x: (p.x - origin.x) / scale, y: (p.y - origin.y) / scale }).collect();
    Ok((origin, scale, normalized))
}
fn compensated_add(sum: &mut f64, correction: &mut f64, value: f64) {
    let adjusted = value - *correction;
    let next = *sum + adjusted;
    *correction = (next - *sum) - adjusted;
    *sum = next;
}
fn calculate_metrics(origin: Vec2, scale: f64, ring: &[Vec2]) -> Result<ParcelMetrics, SurveyError> {
    let count = ring.len();
    let mut twice_area = 0.0;
    let mut area_correction = 0.0;
    let mut cx_sum = 0.0;
    let mut cx_correction = 0.0;
    let mut cy_sum = 0.0;
    let mut cy_correction = 0.0;
    let mut perimeter_norm = 0.0;
    let mut perimeter_correction = 0.0;
    for i in 0..count {
        let j = if i + 1 == count { 0 } else { i + 1 };
        let a = ring.get(i).copied().ok_or(SurveyError::DegenerateControls)?;
        let b = ring.get(j).copied().ok_or(SurveyError::DegenerateControls)?;
        let cross = a.x * b.y - b.x * a.y;
        compensated_add(&mut twice_area, &mut area_correction, cross);
        compensated_add(&mut cx_sum, &mut cx_correction, (a.x + b.x) * cross);
        compensated_add(&mut cy_sum, &mut cy_correction, (a.y + b.y) * cross);
        let edge = (b.x - a.x).hypot(b.y - a.y);
        if !edge.is_finite() || edge <= GEOMETRY_EPSILON {
            return Err(SurveyError::InvalidParcel("parcel contains a zero-length or unstable edge".into()));
        }
        compensated_add(&mut perimeter_norm, &mut perimeter_correction, edge);
    }
    if !twice_area.is_finite() || twice_area.abs() <= GEOMETRY_EPSILON * 4.0 {
        return Err(SurveyError::InvalidParcel("parcel area is zero or numerically unstable".into()));
    }
    let signed_area = twice_area * 0.5 * scale * scale;
    let perimeter = perimeter_norm * scale;
    let centroid = Vec2 { x: origin.x + (cx_sum / (3.0 * twice_area)) * scale, y: origin.y + (cy_sum / (3.0 * twice_area)) * scale };
    if !signed_area.is_finite()
        || signed_area == 0.0
        || !perimeter.is_finite()
        || perimeter <= 0.0
        || !centroid.x.is_finite()
        || !centroid.y.is_finite()
    {
        return Err(SurveyError::InvalidParcel("parcel measurements exceed the supported numeric range".into()));
    }
    Ok(ParcelMetrics { area: signed_area.abs(), signed_area, perimeter, centroid })
}
fn orientation(a: Vec2, b: Vec2, c: Vec2) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}
fn orientation_sign(value: f64) -> i8 {
    if value > GEOMETRY_EPSILON {
        1
    } else if value < -GEOMETRY_EPSILON {
        -1
    } else {
        0
    }
}
fn point_on_segment(a: Vec2, b: Vec2, p: Vec2) -> bool {
    orientation_sign(orientation(a, b, p)) == 0
        && p.x >= a.x.min(b.x) - GEOMETRY_EPSILON
        && p.x <= a.x.max(b.x) + GEOMETRY_EPSILON
        && p.y >= a.y.min(b.y) - GEOMETRY_EPSILON
        && p.y <= a.y.max(b.y) + GEOMETRY_EPSILON
}
fn segments_intersect(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let o1 = orientation_sign(orientation(a, b, c));
    let o2 = orientation_sign(orientation(a, b, d));
    let o3 = orientation_sign(orientation(c, d, a));
    let o4 = orientation_sign(orientation(c, d, b));
    (o1 != 0 && o2 == -o1 && o3 != 0 && o4 == -o3)
        || (o1 == 0 && point_on_segment(a, b, c))
        || (o2 == 0 && point_on_segment(a, b, d))
        || (o3 == 0 && point_on_segment(c, d, a))
        || (o4 == 0 && point_on_segment(c, d, b))
}
fn validate_simple_ring(ring: &[Vec2]) -> Result<(), SurveyError> {
    let count = ring.len();
    for i in 0..count {
        let next = if i + 1 == count { 0 } else { i + 1 };
        let previous = if i == 0 { count - 1 } else { i - 1 };
        let a = ring.get(previous).copied().ok_or(SurveyError::DegenerateControls)?;
        let b = ring.get(i).copied().ok_or(SurveyError::DegenerateControls)?;
        let c = ring.get(next).copied().ok_or(SurveyError::DegenerateControls)?;
        if orientation_sign(orientation(a, b, c)) == 0 && (point_on_segment(a, b, c) || point_on_segment(b, c, a)) {
            return Err(SurveyError::InvalidParcel("adjacent parcel edges overlap or reverse direction".into()));
        }
    }
    for i in 0..count {
        let i_next = if i + 1 == count { 0 } else { i + 1 };
        let a = ring.get(i).copied().ok_or(SurveyError::DegenerateControls)?;
        let b = ring.get(i_next).copied().ok_or(SurveyError::DegenerateControls)?;
        for j in (i + 1)..count {
            let j_next = if j + 1 == count { 0 } else { j + 1 };
            if j == i_next || (i == 0 && j == count - 1) {
                continue;
            }
            let c = ring.get(j).copied().ok_or(SurveyError::DegenerateControls)?;
            let d = ring.get(j_next).copied().ok_or(SurveyError::DegenerateControls)?;
            if segments_intersect(a, b, c, d) {
                return Err(SurveyError::InvalidParcel("parcel boundary self-intersects or touches a non-adjacent edge".into()));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rectangle() -> ParcelBoundary {
        let result = ParcelBoundary::new(vec![Vec2 { x: 0.0, y: 0.0 }, Vec2 { x: 4.0, y: 0.0 }, Vec2 { x: 4.0, y: 3.0 }, Vec2 { x: 0.0, y: 3.0 }]);
        assert!(result.is_ok());
        result.unwrap_or_else(|_| ParcelBoundary {
            vertices: Vec::new(),
            metrics: ParcelMetrics { area: 0.0, signed_area: 0.0, perimeter: 0.0, centroid: Vec2 { x: 0.0, y: 0.0 } },
        })
    }
    #[test]
    fn computes_stable_metrics_and_grid_north_edge_schedule() {
        let p = rectangle();
        let m = p.metrics();
        assert!((m.area - 12.0).abs() < 1.0e-12);
        assert!((m.perimeter - 14.0).abs() < 1.0e-12);
        assert!((m.centroid.x - 2.0).abs() < 1.0e-12);
        assert!((m.centroid.y - 1.5).abs() < 1.0e-12);
        let edges = p.edge_table();
        assert_eq!(edges.len(), 4);
        assert!((edges.first().map(|e| e.azimuth_degrees).unwrap_or(-1.0) - 90.0).abs() < 1.0e-12);
        assert!(edges.get(1).is_some_and(|e| e.azimuth_degrees.abs() < 1.0e-12));
    }
    #[test]
    fn removes_repeated_closing_vertex_and_classifies_points() {
        let p = ParcelBoundary::new(vec![
            Vec2 { x: 0.0, y: 0.0 },
            Vec2 { x: 4.0, y: 0.0 },
            Vec2 { x: 4.0, y: 3.0 },
            Vec2 { x: 0.0, y: 3.0 },
            Vec2 { x: 0.0, y: 0.0 },
        ]);
        assert!(p.is_ok());
        if let Ok(p) = p {
            assert_eq!(p.vertices().len(), 4);
            assert_eq!(p.locate(Vec2 { x: 2.0, y: 1.0 }), Ok(PointLocation::Inside));
            assert_eq!(p.locate(Vec2 { x: 4.0, y: 1.0 }), Ok(PointLocation::Boundary));
            assert_eq!(p.locate(Vec2 { x: 5.0, y: 1.0 }), Ok(PointLocation::Outside));
        }
    }
    #[test]
    fn rejects_bow_ties_collinear_rings_and_non_finite_coordinates() {
        assert!(
            ParcelBoundary::new(vec![Vec2 { x: 0.0, y: 0.0 }, Vec2 { x: 4.0, y: 3.0 }, Vec2 { x: 0.0, y: 3.0 }, Vec2 { x: 4.0, y: 0.0 },]).is_err()
        );
        assert!(ParcelBoundary::new(vec![Vec2 { x: 0.0, y: 0.0 }, Vec2 { x: 1.0, y: 1.0 }, Vec2 { x: 2.0, y: 2.0 },]).is_err());
        assert!(ParcelBoundary::new(vec![Vec2 { x: 0.0, y: 0.0 }, Vec2 { x: f64::NAN, y: 1.0 }, Vec2 { x: 0.0, y: 1.0 },]).is_err());
    }
    #[test]
    fn edits_are_atomic_and_recalculate_metrics() {
        let mut p = rectangle();
        assert!(p.insert_vertex(0, Vec2 { x: 2.0, y: 0.0 }).is_ok());
        assert_eq!(p.vertices().len(), 5);
        assert!(p.remove_vertex(1).is_ok());
        assert_eq!(p.vertices().len(), 4);
        assert!(p.move_vertex(2, Vec2 { x: 5.0, y: 3.0 }).is_ok());
        assert!((p.metrics().area - 13.5).abs() < 1.0e-12);
        let before = p.clone();
        assert!(p.move_vertex(1, Vec2 { x: 0.0, y: 4.0 }).is_err());
        assert_eq!(p, before);
        assert!(p.remove_vertex(0).is_ok());
        assert!(p.remove_vertex(0).is_err());
    }
    #[test]
    fn large_projected_coordinates_do_not_destroy_small_parcel_area() {
        let p = ParcelBoundary::new(vec![
            Vec2 { x: 1.0e12, y: 1.0e12 },
            Vec2 { x: 1.0e12 + 100.0, y: 1.0e12 },
            Vec2 { x: 1.0e12 + 100.0, y: 1.0e12 + 50.0 },
            Vec2 { x: 1.0e12, y: 1.0e12 + 50.0 },
        ]);
        assert!(p.is_ok());
        if let Ok(p) = p {
            assert!((p.metrics().area - 5_000.0).abs() < 1.0e-6);
            assert!((p.metrics().perimeter - 300.0).abs() < 1.0e-6);
        }
    }
    #[test]
    fn accepts_clockwise_winding_and_rejects_adjacent_edge_backtracking() {
        let clockwise = ParcelBoundary::new(vec![Vec2 { x: 0.0, y: 0.0 }, Vec2 { x: 0.0, y: 3.0 }, Vec2 { x: 4.0, y: 3.0 }, Vec2 { x: 4.0, y: 0.0 }]);
        assert!(clockwise.is_ok());
        if let Ok(boundary) = clockwise {
            assert!(boundary.metrics().signed_area < 0.0);
            assert!((boundary.metrics().area - 12.0).abs() < 1.0e-12);
        }

        let backtracking = ParcelBoundary::new(vec![
            Vec2 { x: 0.0, y: 0.0 },
            Vec2 { x: 4.0, y: 0.0 },
            Vec2 { x: 2.0, y: 0.0 },
            Vec2 { x: 2.0, y: 2.0 },
            Vec2 { x: 0.0, y: 2.0 },
        ]);
        assert!(backtracking.is_err());
    }

    #[test]
    fn report_identifies_units_and_edge_schedule() {
        let report = rectangle().report();
        assert!(report.contains("drawing units"));
        assert!(report.contains("Azimuth"));
        assert!(report.contains("Area: 12.0000"));
    }
}
