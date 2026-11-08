//! Surveying and civil-site analysis primitives for CADCraft.
//!
//! Coordinates are expressed in drawing units. CSV point import uses the common
//! PNEZD convention (point, northing, easting, elevation, description) by default.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod cogo;
pub mod civil;
pub mod drawing;
pub mod tin;
pub use drawing::{SurveyDrawing, SurveyDrawingEntity};
pub use tin::{Tin, TinContourSegment, Triangle, stitch_contours};

use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

const MAX_IMPORT_BYTES: usize = 64 * 1024 * 1024;
const MAX_POINTS: usize = 2_000_000;
const EPS: f64 = 1.0e-10;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum SurveyError {
    #[error("input exceeds the supported size limit")]
    InputTooLarge,
    #[error("CSV is empty or contains no point records")]
    EmptyInput,
    #[error("invalid CSV at line {line}: {reason}")]
    Csv { line: usize, reason: String },
    #[error("invalid coordinate or measurement: {0}")]
    InvalidValue(String),
    #[error("at least {0} points are required")]
    NotEnoughPoints(usize),
    #[error("the control points do not define a stable transformation")]
    DegenerateControls,
    #[error("grid dimensions or spacing are invalid")]
    InvalidGrid,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurveyPoint {
    pub number: String,
    pub x: f64,
    pub y: f64,
    pub elevation: f64,
    pub description: String,
}

impl SurveyPoint {
    pub fn validate(&self) -> Result<(), SurveyError> {
        if self.number.trim().is_empty() {
            return Err(SurveyError::InvalidValue("point number is empty".into()));
        }
        if !(self.x.is_finite() && self.y.is_finite() && self.elevation.is_finite()) {
            return Err(SurveyError::InvalidValue(format!("point {} has a non-finite coordinate", self.number)));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum PointFormat {
    /// Point, Northing, Easting, Elevation, Description.
    Pnezd,
    /// Point, Easting, Northing, Elevation, Description.
    PenZd,
    /// Easting, Northing, Elevation, with an optional description.
    Xyz,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImportResult {
    pub points: Vec<SurveyPoint>,
    pub skipped_lines: Vec<usize>,
}

pub fn import_points_csv(input: &str, format: PointFormat, has_header: bool) -> Result<ImportResult, SurveyError> {
    if input.len() > MAX_IMPORT_BYTES {
        return Err(SurveyError::InputTooLarge);
    }
    let mut points = Vec::new();
    let mut skipped = Vec::new();
    let mut first_data = true;
    for (zero_line, raw) in input.lines().enumerate() {
        let line = zero_line + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if first_data && has_header {
            first_data = false;
            continue;
        }
        first_data = false;
        let fields = parse_csv_line(trimmed).map_err(|reason| SurveyError::Csv { line, reason })?;
        let minimum = if format == PointFormat::Xyz { 3 } else { 4 };
        if fields.len() < minimum {
            return Err(SurveyError::Csv { line, reason: format!("expected at least {minimum} fields, found {}", fields.len()) });
        }
        let (number, x_idx, y_idx, z_idx, desc_idx) = match format {
            PointFormat::Pnezd => (0, 2, 1, 3, 4),
            PointFormat::PenZd => (0, 1, 2, 3, 4),
            PointFormat::Xyz => (usize::MAX, 0, 1, 2, 3),
        };
        let parse_num = |idx: usize, label: &str| -> Result<f64, SurveyError> {
            let text = fields.get(idx).ok_or_else(|| SurveyError::Csv { line, reason: format!("missing {label}") })?;
            let value = text.trim().parse::<f64>().map_err(|_| SurveyError::Csv { line, reason: format!("invalid {label}: {text}") })?;
            if !value.is_finite() {
                return Err(SurveyError::Csv { line, reason: format!("non-finite {label}") });
            }
            Ok(value)
        };
        let point_number =
            if number == usize::MAX { format!("{}", points.len() + 1) } else { fields.get(number).map(|v| v.trim().to_string()).unwrap_or_default() };
        let description = fields.get(desc_idx).map(|v| v.trim().to_string()).unwrap_or_default();
        let point = SurveyPoint {
            number: point_number,
            x: parse_num(x_idx, "easting")?,
            y: parse_num(y_idx, "northing")?,
            elevation: parse_num(z_idx, "elevation")?,
            description,
        };
        point.validate()?;
        points.push(point);
        if points.len() > MAX_POINTS {
            return Err(SurveyError::InputTooLarge);
        }
    }
    if points.is_empty() {
        return Err(SurveyError::EmptyInput);
    }
    Ok(ImportResult { points, skipped_lines: skipped })
}

fn parse_csv_line(line: &str) -> Result<Vec<String>, String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();
    let mut quoted = false;
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => {
                current.push('"');
                let _ = chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => {
                fields.push(std::mem::take(&mut current));
            }
            _ => current.push(ch),
        }
    }
    if quoted {
        return Err("unterminated quoted field".into());
    }
    fields.push(current);
    Ok(fields)
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Affine2 {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Affine2 {
    pub fn apply(&self, p: Vec2) -> Vec2 {
        Vec2 { x: self.a * p.x + self.b * p.y + self.c, y: self.d * p.x + self.e * p.y + self.f }
    }
    pub fn from_three_controls(source: [Vec2; 3], target: [Vec2; 3]) -> Result<Self, SurveyError> {
        let p0 = source[0];
        let p1 = source[1];
        let p2 = source[2];
        let det = p0.x * (p1.y - p2.y) + p1.x * (p2.y - p0.y) + p2.x * (p0.y - p1.y);
        if !det.is_finite() || det.abs() <= EPS {
            return Err(SurveyError::DegenerateControls);
        }
        let solve = |v: [f64; 3]| -> [f64; 3] {
            let c0 = (v[0] * (p1.y - p2.y) + v[1] * (p2.y - p0.y) + v[2] * (p0.y - p1.y)) / det;
            let c1 = (v[0] * (p2.x - p1.x) + v[1] * (p0.x - p2.x) + v[2] * (p1.x - p0.x)) / det;
            let c2 = (v[0] * (p1.x * p2.y - p2.x * p1.y) + v[1] * (p2.x * p0.y - p0.x * p2.y) + v[2] * (p0.x * p1.y - p1.x * p0.y)) / det;
            [c0, c1, c2]
        };
        let x = solve([target[0].x, target[1].x, target[2].x]);
        let y = solve([target[0].y, target[1].y, target[2].y]);
        let result = Self { a: x[0], b: x[1], c: x[2], d: y[0], e: y[1], f: y[2] };
        if [result.a, result.b, result.c, result.d, result.e, result.f].iter().all(|v| v.is_finite()) {
            Ok(result)
        } else {
            Err(SurveyError::DegenerateControls)
        }
    }
}

pub fn parcel_area(points: &[Vec2]) -> Result<f64, SurveyError> {
    if points.len() < 3 {
        return Err(SurveyError::NotEnoughPoints(3));
    }
    let mut sum = 0.0;
    for i in 0..points.len() {
        let p = points.get(i).ok_or(SurveyError::NotEnoughPoints(3))?;
        let q = points.get((i + 1) % points.len()).ok_or(SurveyError::NotEnoughPoints(3))?;
        if !(p.x.is_finite() && p.y.is_finite() && q.x.is_finite() && q.y.is_finite()) {
            return Err(SurveyError::InvalidValue("parcel has non-finite coordinates".into()));
        }
        sum += p.x * q.y - q.x * p.y;
    }
    let area = sum.abs() * 0.5;
    if area.is_finite() { Ok(area) } else { Err(SurveyError::InvalidValue("parcel area overflowed".into())) }
}

pub fn parcel_perimeter(points: &[Vec2]) -> Result<f64, SurveyError> {
    if points.len() < 3 {
        return Err(SurveyError::NotEnoughPoints(3));
    }
    let mut sum = 0.0;
    for i in 0..points.len() {
        let p = points.get(i).ok_or(SurveyError::NotEnoughPoints(3))?;
        let q = points.get((i + 1) % points.len()).ok_or(SurveyError::NotEnoughPoints(3))?;
        if !(p.x.is_finite() && p.y.is_finite() && q.x.is_finite() && q.y.is_finite()) {
            return Err(SurveyError::InvalidValue("parcel has non-finite coordinates".into()));
        }
        sum += (q.x - p.x).hypot(q.y - p.y);
    }
    if sum.is_finite() { Ok(sum) } else { Err(SurveyError::InvalidValue("parcel perimeter overflowed".into())) }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GridSpec {
    pub columns: usize,
    pub rows: usize,
    pub origin: Vec2,
    pub spacing_x: f64,
    pub spacing_y: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ElevationGrid {
    pub spec: GridSpec,
    pub elevations: Vec<Option<f64>>,
}

impl ElevationGrid {
    pub fn validate(&self) -> Result<(), SurveyError> {
        let count = self.spec.columns.checked_mul(self.spec.rows).ok_or(SurveyError::InvalidGrid)?;
        if self.spec.columns < 2
            || self.spec.rows < 2
            || count > MAX_POINTS
            || self.elevations.len() != count
            || !self.spec.spacing_x.is_finite()
            || !self.spec.spacing_y.is_finite()
            || self.spec.spacing_x <= 0.0
            || self.spec.spacing_y <= 0.0
        {
            return Err(SurveyError::InvalidGrid);
        }
        if self.elevations.iter().flatten().any(|z| !z.is_finite()) {
            return Err(SurveyError::InvalidGrid);
        }
        Ok(())
    }
    fn at(&self, col: usize, row: usize) -> Option<f64> {
        let index = row.checked_mul(self.spec.columns)?.checked_add(col)?;
        self.elevations.get(index).copied().flatten()
    }
    fn xy(&self, col: usize, row: usize) -> Vec2 {
        Vec2 { x: self.spec.origin.x + col as f64 * self.spec.spacing_x, y: self.spec.origin.y + row as f64 * self.spec.spacing_y }
    }
}

/// Computes cut/fill volume against a constant design elevation using cell-average integration.
pub fn grid_volume(grid: &ElevationGrid, design_elevation: f64) -> Result<(f64, f64), SurveyError> {
    grid.validate()?;
    if !design_elevation.is_finite() {
        return Err(SurveyError::InvalidValue("design elevation must be finite".into()));
    }
    let mut cut = 0.0;
    let mut fill = 0.0;
    for row in 0..grid.spec.rows - 1 {
        for col in 0..grid.spec.columns - 1 {
            let corners = [grid.at(col, row), grid.at(col + 1, row), grid.at(col, row + 1), grid.at(col + 1, row + 1)];
            if corners.iter().any(Option::is_none) {
                continue;
            }
            let mut delta = 0.0;
            for z in corners.iter().flatten() {
                delta += *z - design_elevation;
            }
            let volume = delta * 0.25 * grid.spec.spacing_x * grid.spec.spacing_y;
            if volume > 0.0 {
                cut += volume;
            } else {
                fill -= volume;
            }
        }
    }
    if cut.is_finite() && fill.is_finite() { Ok((cut, fill)) } else { Err(SurveyError::InvalidGrid) }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContourLine {
    pub elevation: f64,
    pub points: Vec<Vec2>,
}

/// Extracts line segments for each contour level from the two triangles in every grid cell.
/// Segments are intentionally returned unstitched; callers can join endpoints within tolerance.
pub fn contours(grid: &ElevationGrid, interval: f64) -> Result<Vec<ContourLine>, SurveyError> {
    grid.validate()?;
    if !interval.is_finite() || interval <= 0.0 {
        return Err(SurveyError::InvalidValue("contour interval must be positive".into()));
    }
    let values: Vec<f64> = grid.elevations.iter().flatten().copied().collect();
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if !min.is_finite() || !max.is_finite() {
        return Ok(Vec::new());
    }
    let first = (min / interval).ceil() * interval;
    let levels = ((max - first) / interval).floor() as usize + 1;
    if levels > 100_000 {
        return Err(SurveyError::InputTooLarge);
    }
    let mut result = Vec::new();
    for level_index in 0..levels {
        let level = first + level_index as f64 * interval;
        for row in 0..grid.spec.rows - 1 {
            for col in 0..grid.spec.columns - 1 {
                let corners = [
                    (grid.xy(col, row), grid.at(col, row)),
                    (grid.xy(col + 1, row), grid.at(col + 1, row)),
                    (grid.xy(col, row + 1), grid.at(col, row + 1)),
                    (grid.xy(col + 1, row + 1), grid.at(col + 1, row + 1)),
                ];
                let triangles = [[0usize, 1, 3], [0, 3, 2]];
                for tri in triangles {
                    let mut hits = Vec::new();
                    for edge in [[0usize, 1usize], [1, 2], [2, 0]] {
                        let Some((ia, ib)) = Some((tri[edge[0]], tri[edge[1]])) else {
                            continue;
                        };
                        let (pa, za) = corners[ia];
                        let (pb, zb) = corners[ib];
                        if let (Some(za), Some(zb)) = (za, zb) {
                            if (za < level && zb > level) || (za > level && zb < level) {
                                let t = (level - za) / (zb - za);
                                hits.push(Vec2 { x: pa.x + (pb.x - pa.x) * t, y: pa.y + (pb.y - pa.y) * t });
                            }
                        }
                    }
                    if hits.len() == 2 {
                        let mut points = Vec::with_capacity(2);
                        if let Some(p) = hits.first() {
                            points.push(*p);
                        }
                        if let Some(p) = hits.get(1) {
                            points.push(*p);
                        }
                        result.push(ContourLine { elevation: level, points });
                    }
                }
            }
        }
    }
    Ok(result)
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldFile {
    pub pixel_x: f64,
    pub rotation_y: f64,
    pub rotation_x: f64,
    pub pixel_y: f64,
    pub origin_x: f64,
    pub origin_y: f64,
}

impl WorldFile {
    /// Parses the six standard world-file lines (A, D, B, E, C, F).
    pub fn parse(text: &str) -> Result<Self, SurveyError> {
        let values: Result<Vec<f64>, _> = text.lines().map(|line| line.trim().parse::<f64>()).collect();
        let values = values.map_err(|_| SurveyError::InvalidValue("world file must contain six numeric lines".into()))?;
        if values.len() != 6 || values.iter().any(|v| !v.is_finite()) {
            return Err(SurveyError::InvalidValue("world file must contain six finite values".into()));
        }
        let mut it = values.into_iter();
        let pixel_x = it.next().ok_or_else(|| SurveyError::InvalidValue("missing A".into()))?;
        let rotation_y = it.next().ok_or_else(|| SurveyError::InvalidValue("missing D".into()))?;
        let rotation_x = it.next().ok_or_else(|| SurveyError::InvalidValue("missing B".into()))?;
        let pixel_y = it.next().ok_or_else(|| SurveyError::InvalidValue("missing E".into()))?;
        let origin_x = it.next().ok_or_else(|| SurveyError::InvalidValue("missing C".into()))?;
        let origin_y = it.next().ok_or_else(|| SurveyError::InvalidValue("missing F".into()))?;
        Ok(Self { pixel_x, rotation_y, rotation_x, pixel_y, origin_x, origin_y })
    }
    /// Converts pixel-center coordinates (column, row) to map coordinates.
    pub fn pixel_to_map(&self, column: f64, row: f64) -> Vec2 {
        Vec2 { x: self.pixel_x * column + self.rotation_x * row + self.origin_x, y: self.rotation_y * column + self.pixel_y * row + self.origin_y }
    }
}

pub fn survey_report(points: &[SurveyPoint]) -> Result<String, SurveyError> {
    if points.is_empty() {
        return Err(SurveyError::EmptyInput);
    }
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut min_z = f64::INFINITY;
    let mut max_z = f64::NEG_INFINITY;
    for point in points {
        point.validate()?;
        min_x = min_x.min(point.x);
        min_y = min_y.min(point.y);
        max_x = max_x.max(point.x);
        max_y = max_y.max(point.y);
        min_z = min_z.min(point.elevation);
        max_z = max_z.max(point.elevation);
    }
    let mut output = String::from("CADCraft Survey Report\n======================\n");
    let _ = writeln!(output, "Points: {}", points.len());
    let _ = writeln!(output, "Easting range: {:.3} to {:.3}", min_x, max_x);
    let _ = writeln!(output, "Northing range: {:.3} to {:.3}", min_y, max_y);
    let _ = writeln!(output, "Elevation range: {:.3} to {:.3}", min_z, max_z);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imports_pnezd_and_preserves_quoted_descriptions() {
        let parsed = import_points_csv("P,N,E,Z,D\n1,200,100,12.5,\"edge, north\"\n", PointFormat::Pnezd, true);
        assert!(parsed.is_ok());
        let result = parsed.unwrap_or_else(|_| ImportResult { points: Vec::new(), skipped_lines: Vec::new() });
        assert_eq!(result.points.first().map(|p| (p.x, p.y, p.elevation)), Some((100.0, 200.0, 12.5)));
        assert_eq!(result.points.first().map(|p| p.description.as_str()), Some("edge, north"));
    }
    #[test]
    fn parcel_metrics_close_the_polygon() {
        let polygon = [Vec2 { x: 0.0, y: 0.0 }, Vec2 { x: 4.0, y: 0.0 }, Vec2 { x: 4.0, y: 3.0 }, Vec2 { x: 0.0, y: 3.0 }];
        assert_eq!(parcel_area(&polygon), Ok(12.0));
        assert_eq!(parcel_perimeter(&polygon), Ok(14.0));
    }
    #[test]
    fn affine_fit_maps_three_control_points() {
        let source = [Vec2 { x: 0.0, y: 0.0 }, Vec2 { x: 1.0, y: 0.0 }, Vec2 { x: 0.0, y: 1.0 }];
        let target = [Vec2 { x: 10.0, y: 20.0 }, Vec2 { x: 12.0, y: 20.0 }, Vec2 { x: 10.0, y: 23.0 }];
        let transform = Affine2::from_three_controls(source, target);
        assert!(transform.is_ok());
        let mapped = transform.map(|t| t.apply(Vec2 { x: 0.5, y: 0.5 }));
        assert_eq!(mapped, Some(Vec2 { x: 11.0, y: 21.5 }));
    }
    #[test]
    fn grid_volume_and_contours_are_computed() {
        let grid = ElevationGrid {
            spec: GridSpec { columns: 2, rows: 2, origin: Vec2 { x: 0.0, y: 0.0 }, spacing_x: 10.0, spacing_y: 10.0 },
            elevations: vec![Some(12.0); 4],
        };
        assert_eq!(grid_volume(&grid, 10.0), Ok((2000.0, 0.0)));
        let lines = contours(&grid, 1.0);
        assert!(lines.is_ok());
        assert_eq!(lines.map(|v| v.len()), Ok(0));
    }
    #[test]
    fn world_file_maps_pixel_centres() {
        let world = WorldFile::parse("2\n0\n0\n-2\n100\n200");
        assert_eq!(world.map(|w| w.pixel_to_map(1.0, 1.0)), Ok(Vec2 { x: 102.0, y: 198.0 }));
    }
}
