//! CSV survey import dialog: paste or open point data, preview it, then add CAD entities.

use cadcraft_survey::{import_points_csv, PointFormat};

use crate::CadApp;

#[derive(Clone)]
struct Form {
    csv: String,
    format: PointFormat,
    has_header: bool,
    file_error: Option<String>,
}

impl Default for Form {
    fn default() -> Self {
        Self {
            csv: String::new(),
            format: PointFormat::Pnezd,
            has_header: true,
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

/// Show the import dialog. Parsing is previewed before the engine is allowed to modify the drawing.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let id = egui::Id::new("cadcraft_survey_csv_import");
    let mut form = ctx.data(|data| data.get_temp::<Form>(id)).unwrap_or_default();
    let mut do_import = false;
    let has_drawing = app.session.doc().is_ok();

    egui::Window::new("Import Survey CSV")
        .id(id)
        .open(open)
        .resizable(true)
        .default_width(620.0)
        .show(ctx, |ui| {
            ui.label("Import survey points into the active CAD drawing.");
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
                ui.colored_label(ui.visuals().error_fg_color, error);
            }

            let preview = import_points_csv(&form.csv, form.format, form.has_header);
            let mut valid = false;
            match &preview {
                Ok(result) => {
                    valid = true;
                    ui.separator();
                    ui.label(format!("Preview: {} points", result.points.len()));
                    egui::Grid::new("survey_import_preview")
                        .striped(true)
                        .num_columns(5)
                        .show(ui, |ui| {
                            for heading in ["Point", "Easting", "Northing", "Elevation", "Description"] {
                                ui.strong(heading);
                            }
                            ui.end_row();
                            for point in result.points.iter().take(6) {
                                ui.label(&point.number);
                                ui.label(format!("{:.4}", point.x));
                                ui.label(format!("{:.4}", point.y));
                                ui.label(format!("{:.4}", point.elevation));
                                ui.label(&point.description);
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
                if ui.add_enabled(has_drawing && valid, egui::Button::new("Import points")).clicked() {
                    do_import = true;
                }
                if !has_drawing {
                    ui.colored_label(ui.visuals().error_fg_color, "Open a drawing before importing.");
                }
                if ui.button("Close").clicked() {
                    *open = false;
                }
            });
            ui.small(format!("Selected format: {}", format_name(form.format)));
        });

    ctx.data_mut(|data| data.insert_temp(id, form.clone()));

    if do_import {
        match cadcraft_engine::survey::import_csv(&mut app.session, &form.csv, form.format, form.has_header) {
            Ok(count) => {
                app.session.echo(format!("Imported {count} survey points on the SURVEY-POINTS layer."));
                *open = false;
                ctx.data_mut(|data| data.remove::<Form>(id));
            }
            Err(error) => {
                app.session.echo(format!("Survey import failed: {error}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_labels_cover_all_supported_csv_layouts() {
        assert!(format_name(PointFormat::Pnezd).starts_with("PNEZD"));
        assert!(format_name(PointFormat::PenZd).starts_with("PENZD"));
        assert!(format_name(PointFormat::Xyz).starts_with("XYZ"));
    }
}
