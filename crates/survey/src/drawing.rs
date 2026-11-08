//! CAD-neutral drawing primitives generated from survey data.
//!
//! This adapter keeps surveying calculations independent of the CAD document and UI crates.
//! The desktop, CLI and export layers can map these stable primitives to native CAD entities.

use serde::{Deserialize, Serialize};

use crate::{SurveyPoint, Vec2};

/// Conventional layer names used by the survey drawing adapter.
pub const POINT_LAYER: &str = "SURVEY-POINTS";
pub const PARCEL_LAYER: &str = "SURVEY-PARCELS";
pub const CONTOUR_LAYER: &str = "SURVEY-CONTOURS";
pub const TIN_LAYER: &str = "SURVEY-TIN";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SurveyDrawingEntity {
    Point {
        layer: String,
        position: [f64; 3],
        point_number: String,
        description: String,
    },
    Polyline {
        layer: String,
        vertices: Vec<[f64; 3]>,
        closed: bool,
        elevation: f64,
    },
    Line {
        layer: String,
        start: [f64; 3],
        end: [f64; 3],
    },
}

/// A collection of survey-derived geometry ready for conversion to CAD document entities.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SurveyDrawing {
    pub entities: Vec<SurveyDrawingEntity>,
}

impl SurveyDrawing {
    /// Creates point marker entities while preserving point numbers and descriptions.
    pub fn from_points(points: &[SurveyPoint]) -> Self {
        let entities = points.iter().map(|point| SurveyDrawingEntity::Point {
            layer: POINT_LAYER.to_string(),
            position: [point.x, point.y, point.elevation],
            point_number: point.number.clone(),
            description: point.description.clone(),
        }).collect();
        Self { entities }
    }

    /// Adds a closed parcel boundary. The input vertices are 2D and use the supplied Z.
    pub fn add_parcel(&mut self, vertices: &[Vec2], elevation: f64) {
        if vertices.len() < 3 || !elevation.is_finite()
            || vertices.iter().any(|p| !p.x.is_finite() || !p.y.is_finite())
        {
            return;
        }
        self.entities.push(SurveyDrawingEntity::Polyline {
            layer: PARCEL_LAYER.to_string(),
            vertices: vertices.iter().map(|p| [p.x, p.y, elevation]).collect(),
            closed: true,
            elevation,
        });
    }

    /// Adds open contour polylines, retaining their measured contour elevation.
    pub fn add_contours(&mut self, contours: &[Vec<Vec2>], elevation: f64) {
        if !elevation.is_finite() {
            return;
        }
        self.entities.extend(contours.iter().filter(|line| line.len() >= 2
            && line.iter().all(|p| p.x.is_finite() && p.y.is_finite()))
            .map(|line| SurveyDrawingEntity::Polyline {
                layer: CONTOUR_LAYER.to_string(),
                vertices: line.iter().map(|p| [p.x, p.y, elevation]).collect(),
                closed: line.first() == line.last(),
                elevation,
            }));
    }

    /// Adds the unique edges of a TIN as line entities.
    pub fn add_tin_edges(&mut self, triangles: &[[[f64; 3]; 3]]) {
        fn compare_point(a: &[f64; 3], b: &[f64; 3]) -> std::cmp::Ordering {
            a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])).then(a[2].total_cmp(&b[2]))
        }

        let mut edges: Vec<([f64; 3], [f64; 3])> = Vec::new();
        for triangle in triangles {
            for (mut a, mut b) in [(triangle[0], triangle[1]), (triangle[1], triangle[2]), (triangle[2], triangle[0])] {
                if a.iter().chain(b.iter()).any(|value| !value.is_finite()) {
                    continue;
                }
                if compare_point(&a, &b).is_gt() {
                    std::mem::swap(&mut a, &mut b);
                }
                edges.push((a, b));
            }
        }
        edges.sort_by(|left, right| compare_point(&left.0, &right.0).then(compare_point(&left.1, &right.1)));
        edges.dedup_by(|left, right| left.0 == right.0 && left.1 == right.1);
        self.entities.extend(edges.into_iter().map(|edge| SurveyDrawingEntity::Line {
            layer: TIN_LAYER.to_string(),
            start: edge.0,
            end: edge.1,
        }));
    }}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_import_becomes_layered_drawing_geometry() {
        let points = [SurveyPoint {
            number: "101".into(), x: 12.0, y: 34.0, elevation: 5.6,
            description: "BM".into(),
        }];
        let drawing = SurveyDrawing::from_points(&points);
        assert_eq!(drawing.entities.len(), 1);
        assert!(matches!(drawing.entities.first(),
            Some(SurveyDrawingEntity::Point { layer, point_number, description, .. })
                if layer == POINT_LAYER && point_number == "101" && description == "BM"));
    }

    #[test]
    fn parcel_and_contour_geometry_keep_their_semantics() {
        let mut drawing = SurveyDrawing::default();
        let boundary = [Vec2 { x: 0.0, y: 0.0 }, Vec2 { x: 10.0, y: 0.0 }, Vec2 { x: 10.0, y: 10.0 }];
        drawing.add_parcel(&boundary, 2.0);
        drawing.add_contours(&[vec![boundary[0], boundary[1]]], 4.0);
        assert!(matches!(drawing.entities.first(),
            Some(SurveyDrawingEntity::Polyline { layer, closed: true, .. }) if layer == PARCEL_LAYER));
        assert!(matches!(drawing.entities.get(1),
            Some(SurveyDrawingEntity::Polyline { layer, closed: false, elevation: 4.0, .. }) if layer == CONTOUR_LAYER));
    }

    #[test]
    fn invalid_geometry_is_not_emitted() {
        let mut drawing = SurveyDrawing::default();
        drawing.add_parcel(&[Vec2 { x: f64::NAN, y: 0.0 }; 3], 0.0);
        drawing.add_contours(&[vec![Vec2 { x: 0.0, y: 0.0 }]], 0.0);
        assert!(drawing.entities.is_empty());
    }
}
