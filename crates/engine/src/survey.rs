//! Import CAD-neutral surveying geometry into the active CAD drawing.
//!
//! Survey calculations stay in `cadcraft-survey`; this module is the engine boundary that
//! converts their output into native document entities and creates the corresponding layers.

use cadcraft_doc::{EntityKind, Layer, Line, Point, Polyline3d};
use cadcraft_geom::Vec3;
use cadcraft_survey::{SurveyDrawing, SurveyDrawingEntity};

use crate::{Result, Session};

/// Imports a survey drawing into the active drawing and returns the number of entities created.
///
/// Each source entity is assigned to its declared survey layer. Missing layers are created.
/// The drawing's current layer is restored even if an entity cannot be added.
pub fn import_drawing(session: &mut Session, survey: &SurveyDrawing) -> Result<usize> {
    // Validate the entire batch before changing the drawing, so malformed geometry cannot
    // leave a partially imported survey behind.
    for entity in &survey.entities {
        let valid = match entity {
            SurveyDrawingEntity::Point { position, .. } =>
                position.iter().all(|value| value.is_finite()),
            SurveyDrawingEntity::Polyline { vertices, elevation, .. } =>
                vertices.len() >= 2 && elevation.is_finite()
                    && vertices.iter().flatten().all(|value| value.is_finite()),
            SurveyDrawingEntity::Line { start, end, .. } =>
                start.iter().chain(end.iter()).all(|value| value.is_finite()),
        };
        if !valid {
            return Err(crate::EngineError::Other("survey drawing contains invalid geometry".into()));
        }
    }

    let layers: Vec<String> = survey.entities.iter().map(|entity| match entity {
        SurveyDrawingEntity::Point { layer, .. }
        | SurveyDrawingEntity::Polyline { layer, .. }
        | SurveyDrawingEntity::Line { layer, .. } => layer.clone(),
    }).collect();

    {
        let drawing = session.doc_mut()?;
        for name in &layers {
            if !drawing.layers.iter().any(|layer| layer.name.eq_ignore_ascii_case(name)) {
                drawing.layers.push(Layer::new(name));
            }
        }
    }

    let original_layer = session.doc()?.header.str("CLAYER", "0");
    let result = (|| {
        let mut imported = 0usize;
        for entity in &survey.entities {
            let (layer, kind) = match entity {
                SurveyDrawingEntity::Point { layer, position, .. } => (
                    layer,
                    EntityKind::Point(Point { p: Vec3::new(position[0], position[1], position[2]), angle: 0.0 }),
                ),
                SurveyDrawingEntity::Polyline { layer, vertices, closed, .. } => (
                    layer,
                    EntityKind::Polyline3d(Polyline3d {
                        points: vertices.iter().map(|p| Vec3::new(p[0], p[1], p[2])).collect(),
                        closed: *closed,
                    }),
                ),
                SurveyDrawingEntity::Line { layer, start, end } => (
                    layer,
                    EntityKind::Line(Line {
                        a: Vec3::new(start[0], start[1], start[2]),
                        b: Vec3::new(end[0], end[1], end[2]),
                    }),
                ),
            };
            session.doc_mut()?.header.set_str("CLAYER", layer);
            session.add_entity(kind)?;
            imported += 1;
        }
        Ok(imported)
    })();

    if let Ok(drawing) = session.doc_mut() {
        drawing.header.set_str("CLAYER", &original_layer);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_survey::{SurveyDrawing, SurveyPoint};

    #[test]
    fn imports_native_entities_on_survey_layers_and_restores_current_layer() {
        let mut session = Session::new();
        let survey = SurveyDrawing::from_points(&[
            SurveyPoint {
                number: "101".into(),
                x: 12.0,
                y: 34.0,
                elevation: 5.6,
                description: "BM".into(),
            },
        ]);

        let imported = import_drawing(&mut session, &survey);
        assert_eq!(imported.unwrap_or_default(), 1);

        let drawing = session.doc();
        assert!(drawing.is_ok());
        if let Ok(drawing) = drawing {
            assert!(drawing.layer("SURVEY-POINTS").is_some());
            assert_eq!(drawing.header.str("CLAYER", "0"), "0");
            let point = drawing.model.iter().find(|entity| {
                matches!(&entity.kind, EntityKind::Point(_))
            });
            assert!(point.is_some());
        }
    }

    #[test]
    fn rejects_non_finite_geometry_before_import() {
        let mut session = Session::new();
        let survey = SurveyDrawing {
            entities: vec![SurveyDrawingEntity::Line {
                layer: "SURVEY-TIN".into(),
                start: [0.0, 0.0, 0.0],
                end: [f64::NAN, 1.0, 0.0],
            }],
        };
        assert!(import_drawing(&mut session, &survey).is_err());
        assert!(session.doc().is_ok_and(|drawing| drawing.layer("SURVEY-TIN").is_none()));
    }
}
