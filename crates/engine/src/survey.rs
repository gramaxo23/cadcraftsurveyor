//! Import CAD-neutral surveying geometry into the active CAD drawing.
//!
//! Survey calculations stay in `cadcraft-survey`; this module converts their output into native
//! document entities while preserving survey point identifiers and descriptions as DXF XDATA.

use cadcraft_doc::{Common, EntityKind, Line, Point, Polyline3d, RawTag};
use cadcraft_geom::Vec3;
use cadcraft_survey::{import_points_csv, PointFormat, SurveyDrawing, SurveyDrawingEntity};

use crate::{EngineError, Result, Session};

const SURVEY_XDATA_APP: &str = "CADCRAFT_SURVEY";
const XDATA_CHUNK_BYTES: usize = 240;

/// Imports a survey drawing into the active drawing and returns the number of entities created.
///
/// Geometry is placed in the currently edited space, on each entity's declared layer.
/// Point numbers and descriptions are stored as CAD XDATA (and therefore survive DXF save/load).
/// All input is validated before the document is modified.
pub fn import_drawing(session: &mut Session, survey: &SurveyDrawing) -> Result<usize> {
    let space = session.space();
    if session.doc()?.space(&space).is_none() {
        return Err(EngineError::Other("the active drawing space does not exist".into()));
    }

    // Validate the entire batch before changing the drawing, so malformed input cannot leave a
    // partially imported survey or create empty layers.
    for entity in &survey.entities {
        let (layer, valid_geometry) = match entity {
            SurveyDrawingEntity::Point { layer, position, point_number, .. } =>
                (layer, !point_number.trim().is_empty() && position.iter().all(|value| value.is_finite())),
            SurveyDrawingEntity::Polyline { layer, vertices, elevation, .. } =>
                (layer, vertices.len() >= 2 && elevation.is_finite()
                    && vertices.iter().flatten().all(|value| value.is_finite())),
            SurveyDrawingEntity::Line { layer, start, end } =>
                (layer, start.iter().chain(end.iter()).all(|value| value.is_finite())),
        };
        if !valid_geometry {
            return Err(EngineError::Other("survey drawing contains invalid geometry".into()));
        }
        if layer.trim().is_empty() || layer.len() > 255
            || layer.chars().any(|ch| ch.is_control() || matches!(ch, '<' | '>' | '/' | '\\' | '"' | ':' | ';' | '?' | '*' | '|' | '=' | ','))
        {
            return Err(EngineError::Other(format!("invalid survey layer name: {layer:?}")));
        }
    }

    let mut imported = 0usize;
    let base_common = session.current_common();
    let drawing = session.doc_mut()?;
    for entity in &survey.entities {
        let (layer, kind, point_metadata) = match entity {
            SurveyDrawingEntity::Point { layer, position, point_number, description } => (
                layer,
                EntityKind::Point(Point { p: Vec3::new(position[0], position[1], position[2]), angle: 0.0 }),
                Some((point_number.as_str(), description.as_str())),
            ),
            SurveyDrawingEntity::Polyline { layer, vertices, closed, .. } => (
                layer,
                EntityKind::Polyline3d(Polyline3d {
                    points: vertices.iter().map(|p| Vec3::new(p[0], p[1], p[2])).collect(),
                    closed: *closed,
                }),
                None,
            ),
            SurveyDrawingEntity::Line { layer, start, end } => (
                layer,
                EntityKind::Line(Line {
                    a: Vec3::new(start[0], start[1], start[2]),
                    b: Vec3::new(end[0], end[1], end[2]),
                }),
                None,
            ),
        };

        let mut common = Common { layer: layer.clone(), ..base_common.clone() };
        if let Some((number, description)) = point_metadata {
            common.xdata = vec![
                RawTag { code: 1001, value: SURVEY_XDATA_APP.into() },
                RawTag { code: 1000, value: "POINT_NUMBER".into() },
            ];
            append_xdata_text(&mut common.xdata, number);
            common.xdata.push(RawTag { code: 1000, value: "DESCRIPTION".into() });
            append_xdata_text(&mut common.xdata, description);
        }
        drawing.add(&space, common, kind)?;
        imported += 1;
    }
    Ok(imported)
}

/// Parses a PNEZD, PENZD or XYZ CSV file and imports its points into the active drawing.
///
/// CSV parsing completes before the drawing is touched. The returned count is the number of
/// native CAD point entities created.
pub fn import_csv(session: &mut Session, input: &str, format: PointFormat, has_header: bool) -> Result<usize> {
    let parsed = import_points_csv(input, format, has_header)
        .map_err(|error| EngineError::Other(error.to_string()))?;
    let drawing = SurveyDrawing::from_points(&parsed.points);
    import_drawing(session, &drawing)
}

/// Store arbitrarily long metadata in legal-sized XDATA strings without splitting UTF-8.
fn append_xdata_text(tags: &mut Vec<RawTag>, value: &str) {
    if value.is_empty() {
        return;
    }
    let mut chunk = String::new();
    for ch in value.chars() {
        if chunk.len() + ch.len_utf8() > XDATA_CHUNK_BYTES {
            tags.push(RawTag { code: 1000, value: std::mem::take(&mut chunk) });
        }
        chunk.push(ch);
    }
    if !chunk.is_empty() {
        tags.push(RawTag { code: 1000, value: chunk });
    }
}

/// The CADCraft survey XDATA application identifier used for point metadata.
pub const fn xdata_application_name() -> &'static str {
    SURVEY_XDATA_APP
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_survey::{SurveyDrawing, SurveyPoint};

    #[test]
    fn imports_native_entities_with_point_metadata_and_declared_layers() {
        let mut session = Session::new();
        let survey = SurveyDrawing::from_points(&[
            SurveyPoint {
                number: "101".into(),
                x: 12.0,
                y: 34.0,
                elevation: 5.6,
                description: "BM / control".into(),
            },
        ]);

        assert_eq!(import_drawing(&mut session, &survey).ok(), Some(1));
        let drawing = session.doc();
        assert!(drawing.is_ok());
        if let Ok(drawing) = drawing {
            let point = drawing.model.iter().find(|entity| matches!(&entity.kind, EntityKind::Point(_)));
            assert!(point.is_some());
            if let Some(point) = point {
                assert_eq!(point.common.layer, "SURVEY-POINTS");
                assert!(point.common.xdata.iter().any(|tag| tag.code == 1001 && tag.value == SURVEY_XDATA_APP));
                assert!(point.common.xdata.iter().any(|tag| tag.code == 1000 && tag.value == "101"));
                assert!(point.common.xdata.iter().any(|tag| tag.code == 1000 && tag.value == "BM / control"));
            }
            assert!(drawing.layer("SURVEY-POINTS").is_some());
        }
    }

    #[test]
    fn csv_import_creates_native_points_and_keeps_number_and_description() {
        let mut session = Session::new();
        let csv = "Point,Northing,Easting,Elevation,Description\n101,34,12,5.6,Control\n";
        assert_eq!(import_csv(&mut session, csv, PointFormat::Pnezd, true).ok(), Some(1));
        assert!(session.doc().is_ok_and(|drawing| {
            drawing.model.iter().any(|entity| {
                matches!(&entity.kind, EntityKind::Point(point)
                    if point.p.x == 12.0 && point.p.y == 34.0 && point.p.z == 5.6)
                    && entity.common.xdata.iter().any(|tag| tag.value == "101")
                    && entity.common.xdata.iter().any(|tag| tag.value == "Control")
            })
        }));
    }

    #[test]
    fn imports_parcel_and_tin_geometry_as_native_3d_entities() {
        let mut session = Session::new();
        let survey = SurveyDrawing {
            entities: vec![
                SurveyDrawingEntity::Polyline {
                    layer: "SURVEY-PARCELS".into(),
                    vertices: vec![[0.0, 0.0, 2.0], [10.0, 0.0, 2.0], [10.0, 10.0, 2.0]],
                    closed: true,
                    elevation: 2.0,
                },
                SurveyDrawingEntity::Line {
                    layer: "SURVEY-TIN".into(),
                    start: [0.0, 0.0, 1.0],
                    end: [10.0, 10.0, 3.0],
                },
            ],
        };
        assert_eq!(import_drawing(&mut session, &survey).ok(), Some(2));
        assert!(session.doc().is_ok_and(|drawing| {
            drawing.model.iter().any(|e| matches!(&e.kind, EntityKind::Polyline3d(p) if p.closed))
                && drawing.model.iter().any(|e| matches!(&e.kind, EntityKind::Line(_)))
                && drawing.layer("SURVEY-PARCELS").is_some()
                && drawing.layer("SURVEY-TIN").is_some()
        }));
    }

    #[test]
    fn rejects_invalid_geometry_and_layer_names_before_import() {
        let mut session = Session::new();
        let invalid_geometry = SurveyDrawing {
            entities: vec![SurveyDrawingEntity::Line {
                layer: "SURVEY-TIN".into(),
                start: [0.0, 0.0, 0.0],
                end: [f64::NAN, 1.0, 0.0],
            }],
        };
        assert!(import_drawing(&mut session, &invalid_geometry).is_err());
        assert!(session.doc().is_ok_and(|drawing| drawing.layer("SURVEY-TIN").is_none()));

        let invalid_layer = SurveyDrawing {
            entities: vec![SurveyDrawingEntity::Point {
                layer: "BAD/LAYER".into(),
                position: [0.0, 0.0, 0.0],
                point_number: "1".into(),
                description: String::new(),
            }],
        };
        assert!(import_drawing(&mut session, &invalid_layer).is_err());
        assert!(session.doc().is_ok_and(|drawing| drawing.layer("BAD/LAYER").is_none()));
    }

    #[test]
    fn long_utf8_metadata_is_split_without_corruption() {
        let mut tags = Vec::new();
        let value = "測".repeat(200);
        append_xdata_text(&mut tags, &value);
        assert!(tags.iter().all(|tag| tag.value.len() <= XDATA_CHUNK_BYTES));
        assert_eq!(tags.iter().map(|tag| tag.value.as_str()).collect::<String>(), value);
    }
}
