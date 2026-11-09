//! CSV survey import dialog: paste or open point data, preview it, then add CAD entities.

use cadcraft_survey::{
    MAX_TIN_POINTS, PointFormat, SurveyDrawing, SurveyDrawingEntity, SurveyError, SurveyPoint, Tin, import_points_csv, stitch_contour_polylines,
};

use crate::CadApp;

#[derive(Clone)]
struct Form {
    csv: String,
    format: PointFormat,
    has_header: bool,
    create_tin: bool,
    create_contours: bool,
    contour_interval: f64,
    file_error: Option<String>,
}

impl Default for Form {
    fn default() -> Self {
        Self {
            csv: String::new(),
            format: PointFormat::Pnezd,
            has_header: true,
            create_tin: false,
            create_contours: false,
            contour_interval: 1.0,
            file_error: None,
        }
    }
}

fn format_name(format: PointFormat) -> &'static str {
    match format {
        PointFormat::Pnezd => "PNEZD (Point, Northing, Easting, Elevation, Description)",
        PointFormat::PenZd => "PENZD (Point, Easting, Northing, Elevation, Description)",
        PointFormat::Xyz => "XYZ (Easting, Northing, Elevation)",
    }
}

fn contour_tolerances(points: &[SurveyPoint]) -> (f64, f64) {
    let max_xy = points.iter().fold(1.0_f64, |maximum, point| maximum.max(point.x.abs()).max(point.y.abs()));
    let max_elevation = points.iter().fold(1.0_f64, |maximum, point| maximum.max(point.elevation.abs()));
    let minimum = f64::EPSILON * 8.0;
    ((max_xy * f64::EPSILON * 8.0).max(minimum), (max_elevation * f64::EPSILON * 8.0).max(minimum))
}

fn build_survey_drawing(form: &Form) -> Result<SurveyDrawing, SurveyError> {
    let parsed = import_points_csv(&form.csv, form.format, form.has_header)?;
    let mut drawing = SurveyDrawing::from_points(&parsed.points);
    if form.create_tin || form.create_contours {
        let tin = Tin::from_points(&parsed.points)?;
        if form.create_tin {
            drawing.add_tin(&tin);
        }
        if form.create_contours {
            let segments = tin.contour_segments(form.contour_interval)?;
            let (xy_tolerance, elevation_tolerance) = contour_tolerances(&parsed.points);
            let contours = stitch_contour_polylines(&segments, xy_tolerance, elevation_tolerance)?;
            drawing.add_contour_polylines(&contours);
        }
    }
    Ok(drawing)
}

/// Show the import dialog. Parsing is previewed before the engine is allowed to modify the drawing.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let id = egui::Id::new("cadcraft_survey_csv_import");
    let mut form = ctx.data(|data| data.get_temp::<Form>(id)).unwrap_or_default();
    let mut do_import = false;
    let mut close_requested = false;
    let has_drawing = app.session.doc().is_ok();

    egui::Window::new("Import Survey CSV").id(id).open(open).resizable(true).default_width(620.0).show(ctx, |ui| {
        ui.label("Import survey points into the active CAD drawing.");
        ui.small("Optional terrain output is generated from the same validated point set.");
        ui.horizontal(|ui| {
            ui.label("CSV format");
            egui::ComboBox::from_id_salt("survey_csv_format")
                .selected_text(match form.format {
                    PointFormat::Pnezd => "PNEZD",
                    PointFormat::PenZd => "PENZD",
                    PointFormat::Xyz => "XYZ",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut form.format, PointFormat::Pnezd, "PNEZD — Northing before Easting");
                    ui.selectable_value(&mut form.format, PointFormat::PenZd, "PENZD — Easting before Northing");
                    ui.selectable_value(&mut form.format, PointFormat::Xyz, "XYZ — Easting, Northing, Elevation");
                });
            ui.checkbox(&mut form.has_header, "First row is a header");
        });

        #[cfg(not(target_arch = "wasm32"))]
        if !app.services.no_file_system && ui.button("Open CSV file...").clicked() {
            if let Some(path) = app.services.pick_open.as_ref().and_then(|pick| pick()) {
                match std::fs::read_to_string(&path) {
                    Ok(contents) => {
                        form.csv = contents;
                        form.file_error = None;
                    }
                    Err(error) => form.file_error = Some(format!("Could not read {path}: {error}")),
                }
            }
        }

        ui.label("Or paste CSV data below:");
        ui.add(
            egui::TextEdit::multiline(&mut form.csv)
                .desired_rows(12)
                .desired_width(f32::INFINITY)
                .hint_text("101, 5000.000, 2000.000, 125.50, Control point"),
        );

        if let Some(error) = &form.file_error {
            ui.colored_label(ui.visuals().error_fg_color, error.as_str());
        }

        ui.separator();
        ui.strong("Optional terrain output");
        ui.checkbox(&mut form.create_tin, "Create TIN edge lines");
        ui.checkbox(&mut form.create_contours, "Generate stitched contour polylines");
        if form.create_contours {
            ui.horizontal(|ui| {
                ui.label("Contour interval");
                ui.add(egui::DragValue::new(&mut form.contour_interval).speed(0.1));
            });
        }

        let preview = import_points_csv(&form.csv, form.format, form.has_header);
        let mut valid = false;
        match &preview {
            Ok(result) => {
                valid = true;
                if form.create_tin || form.create_contours {
                    if result.points.len() < 3 {
                        valid = false;
                        ui.colored_label(ui.visuals().error_fg_color, "TIN and contours require at least 3 points.");
                    } else if result.points.len() > MAX_TIN_POINTS {
                        valid = false;
                        ui.colored_label(ui.visuals().error_fg_color, format!("Terrain generation is limited to {MAX_TIN_POINTS} points."));
                    }
                    if form.create_contours && (!form.contour_interval.is_finite() || form.contour_interval <= 0.0) {
                        valid = false;
                        ui.colored_label(ui.visuals().error_fg_color, "Contour interval must be a positive finite number.");
                    }
                }
                ui.separator();
                ui.label(format!("Preview: {} points", result.points.len()));
                egui::Grid::new("survey_import_preview").striped(true).num_columns(5).show(ui, |ui| {
                    for heading in ["Point", "Easting", "Northing", "Elevation", "Description"] {
                        ui.strong(heading);
                    }
                    ui.end_row();
                    for point in result.points.iter().take(6) {
                        ui.label(point.number.as_str());
                        ui.label(format!("{:.4}", point.x));
                        ui.label(format!("{:.4}", point.y));
                        ui.label(format!("{:.4}", point.elevation));
                        ui.label(point.description.as_str());
                        ui.end_row();
                    }
                });
                if result.points.len() > 6 {
                    ui.small(format!("Showing 6 of {} points.", result.points.len()));
                }
                if !result.skipped_lines.is_empty() {
                    ui.small(format!("Skipped lines: {:?}", result.skipped_lines));
                }
            }
            Err(error) if !form.csv.trim().is_empty() => {
                ui.colored_label(ui.visuals().error_fg_color, error.to_string());
            }
            Err(_) => {
                ui.small("Choose a CSV file or paste point data to see a preview.");
            }
        }

        ui.separator();
        ui.horizontal(|ui| {
            if ui.add_enabled(has_drawing && valid, egui::Button::new("Import survey data")).clicked() {
                do_import = true;
            }
            if !has_drawing {
                ui.colored_label(ui.visuals().error_fg_color, "Open a drawing before importing.");
            }
            if ui.button("Close").clicked() {
                close_requested = true;
            }
        });
        ui.small(format!("Selected format: {}", format_name(form.format)));
    });

    ctx.data_mut(|data| data.insert_temp(id, form.clone()));
    if close_requested {
        *open = false;
    }

    if do_import {
        match build_survey_drawing(&form) {
            Ok(drawing) => {
                let point_count = drawing.entities.iter().filter(|entity| matches!(entity, SurveyDrawingEntity::Point { .. })).count();
                let terrain_count = drawing.entities.len().saturating_sub(point_count);
                match cadcraft_engine::survey::import_drawing(&mut app.session, &drawing) {
                    Ok(count) => {
                        app.session.echo(format!("Imported {point_count} survey points and {terrain_count} terrain entities ({count} total)."));
                        *open = false;
                        ctx.data_mut(|data| data.remove::<Form>(id));
                    }
                    Err(error) => app.session.echo(format!("Survey import failed: {error}")),
                }
            }
            Err(error) => app.session.echo(format!("Survey import failed: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_command_opens_the_import_dialog() {
        let mut app = crate::CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        let result = crate::menus::run_ui_command(&mut app, "ui.survey.import", &serde_json::Value::Null);
        assert!(result.is_some());
        assert_eq!(app.ui.dialog.as_deref(), Some("surveyimport"));
    }

    #[test]
    fn terrain_options_create_native_tin_and_contour_geometry() {
        let form = Form {
            csv: "1,0,0,0\n2,10,0,10\n3,0,10,20".into(),
            format: PointFormat::Xyz,
            has_header: false,
            create_tin: true,
            create_contours: true,
            contour_interval: 5.0,
            file_error: None,
        };
        let drawing = build_survey_drawing(&form);
        assert!(drawing.is_ok());
        if let Ok(drawing) = drawing {
            assert!(drawing.entities.iter().any(|entity| matches!(entity,
                SurveyDrawingEntity::Line { layer, .. } if layer == "SURVEY-TIN")));
            assert!(drawing.entities.iter().any(|entity| matches!(entity,
                SurveyDrawingEntity::Polyline { layer, elevation, .. } if layer == "SURVEY-CONTOURS" && elevation.is_finite())));
            let mut session = cadcraft_engine::Session::new();
            assert_eq!(cadcraft_engine::survey::import_drawing(&mut session, &drawing).ok(), Some(drawing.entities.len()));
        }
    }

    #[test]
    fn format_labels_cover_all_supported_csv_layouts() {
        assert!(format_name(PointFormat::Pnezd).starts_with("PNEZD"));
        assert!(format_name(PointFormat::PenZd).starts_with("PENZD"));
        assert!(format_name(PointFormat::Xyz).starts_with("XYZ"));
    }
}
