//! Survey units, coordinate metadata, horizontal geometry, profiles and cross sections.
//!
//! This module deliberately stores CRS identifiers and metadata; it does not pretend
//! to perform datum, geoid, or map-projection transformations. Those require an
//! authoritative grid/model and a separately validated transformation engine.
use crate::{SurveyError, Vec2};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinearUnit {
    Metre,
    InternationalFoot,
    UsSurveyFoot,
    Millimetre,
    Centimetre,
    Kilometre,
}

impl LinearUnit {
    /// Exact metres per unit (US survey foot is intentionally distinct).
    pub const fn metres_per_unit(self) -> f64 {
        match self {
            Self::Metre => 1.0,
            Self::InternationalFoot => 0.3048,
            Self::UsSurveyFoot => 1200.0 / 3937.0,
            Self::Millimetre => 0.001,
            Self::Centimetre => 0.01,
            Self::Kilometre => 1000.0,
        }
    }

    pub fn convert(self, value: f64, to: Self) -> Result<f64, SurveyError> {
        if !value.is_finite() {
            return Err(SurveyError::InvalidValue("unit conversion input must be finite".into()));
        }
        let converted = value * self.metres_per_unit() / to.metres_per_unit();
        if converted.is_finite() {
            Ok(converted)
        } else {
            Err(SurveyError::InvalidValue("unit conversion overflowed".into()))
        }
    }

    pub fn convert_area(self, value: f64, to: Self) -> Result<f64, SurveyError> {
        if !value.is_finite() || value < 0.0 {
            return Err(SurveyError::InvalidValue("area must be finite and non-negative".into()));
        }
        let ratio = self.metres_per_unit() / to.metres_per_unit();
        let result = value * ratio * ratio;
        if result.is_finite() { Ok(result) } else { Err(SurveyError::InvalidValue("area conversion overflowed".into())) }
    }

    pub fn convert_volume(self, value: f64, to: Self) -> Result<f64, SurveyError> {
        if !value.is_finite() || value < 0.0 {
            return Err(SurveyError::InvalidValue("volume must be finite and non-negative".into()));
        }
        let ratio = self.metres_per_unit() / to.metres_per_unit();
        let result = value * ratio * ratio * ratio;
        if result.is_finite() { Ok(result) } else { Err(SurveyError::InvalidValue("volume conversion overflowed".into())) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AxisOrder {
    EastingNorthing,
    NorthingEasting,
    LongitudeLatitude,
    LatitudeLongitude,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoordinateUse {
    Grid,
    Ground,
    LocalEngineering,
    Unknown,
}

/// Explicit metadata attached to a survey dataset or project.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurveyCrsMetadata {
    pub authority: Option<String>,
    pub code: Option<String>,
    pub name: String,
    pub datum: Option<String>,
    pub projection: Option<String>,
    pub geoid_model: Option<String>,
    pub coordinate_epoch: Option<f64>,
    pub horizontal_unit: LinearUnit,
    pub vertical_unit: LinearUnit,
    pub axis_order: AxisOrder,
    pub coordinate_use: CoordinateUse,
    /// Combined scale factor (grid-to-ground convention must be documented by caller).
    pub combined_scale_factor: Option<f64>,
}

impl SurveyCrsMetadata {
    pub fn validate(&self) -> Result<(), SurveyError> {
        if self.name.trim().is_empty() {
            return Err(SurveyError::InvalidValue("CRS name is required".into()));
        }
        if self.authority.as_ref().is_some_and(|v| v.trim().is_empty())
            || self.code.as_ref().is_some_and(|v| v.trim().is_empty())
        {
            return Err(SurveyError::InvalidValue("CRS authority and code cannot be blank".into()));
        }
        if self.coordinate_epoch.is_some_and(|v| !v.is_finite() || v <= 0.0) {
            return Err(SurveyError::InvalidValue("coordinate epoch must be finite and positive".into()));
        }
        if self.combined_scale_factor.is_some_and(|v| !v.is_finite() || v <= 0.0) {
            return Err(SurveyError::InvalidValue("combined scale factor must be finite and positive".into()));
        }
        if self.coordinate_use == CoordinateUse::Unknown && self.combined_scale_factor.is_some() {
            return Err(SurveyError::InvalidValue("a grid/ground scale factor requires a declared coordinate use".into()));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct StationEquation {
    pub back_station: f64,
    pub ahead_station: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HorizontalAlignment {
    pub name: String,
    pub points: Vec<Vec2>,
    pub start_station: f64,
    pub station_equations: Vec<StationEquation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AlignmentPoint {
    pub coordinate: Vec2,
    pub station: f64,
    pub tangent_azimuth_degrees: f64,
    pub offset: f64,
}

impl HorizontalAlignment {
    pub fn validate(&self) -> Result<(), SurveyError> {
        if self.name.trim().is_empty() || self.points.len() < 2 || !self.start_station.is_finite() {
            return Err(SurveyError::InvalidValue("alignment needs a name, finite start station and at least two vertices".into()));
        }
        for p in &self.points {
            if !p.x.is_finite() || !p.y.is_finite() {
                return Err(SurveyError::InvalidValue("alignment coordinates must be finite".into()));
            }
        }
        for pair in self.points.windows(2) {
            if pair[0].x == pair[1].x && pair[0].y == pair[1].y {
                return Err(SurveyError::InvalidValue("alignment contains a zero-length segment".into()));
            }
        }
        let mut last = f64::NEG_INFINITY;
        for eq in &self.station_equations {
            if !eq.back_station.is_finite() || !eq.ahead_station.is_finite()
                || eq.back_station <= last || eq.ahead_station < 0.0
            {
                return Err(SurveyError::InvalidValue("station equations must be finite and ordered".into()));
            }
            last = eq.back_station;
        }
        Ok(())
    }

    pub fn length(&self) -> Result<f64, SurveyError> {
        self.validate()?;
        let total: f64 = self.points.windows(2).map(|p| (p[1].x-p[0].x).hypot(p[1].y-p[0].y)).sum();
        if total.is_finite() { Ok(total) } else { Err(SurveyError::InvalidValue("alignment length overflowed".into())) }
    }

    /// Returns a point at a distance along the alignment; station equations affect displayed station only.
    pub fn point_at_distance(&self, distance: f64, offset: f64) -> Result<AlignmentPoint, SurveyError> {
        self.validate()?;
        if !distance.is_finite() || !offset.is_finite() || distance < 0.0 {
            return Err(SurveyError::InvalidValue("alignment distance and offset must be finite; distance cannot be negative".into()));
        }
        let mut remaining = distance;
        let mut station = self.start_station;
        for segment in self.points.windows(2) {
            let dx = segment[1].x - segment[0].x;
            let dy = segment[1].y - segment[0].y;
            let length = dx.hypot(dy);
            if remaining <= length {
                let t = remaining / length;
                let azimuth = dx.atan2(dy).to_degrees().rem_euclid(360.0);
                let x = segment[0].x + dx * t;
                let y = segment[0].y + dy * t;
                // Positive offset is to the right of increasing station.
                let radians = azimuth.to_radians();
                let point = Vec2 { x: x + offset * radians.cos(), y: y - offset * radians.sin() };
                let mut display_station = station + remaining;
                for eq in &self.station_equations {
                    if display_station >= eq.back_station {
                        display_station = eq.ahead_station + (display_station - eq.back_station);
                    }
                }
                return Ok(AlignmentPoint { coordinate: point, station: display_station, tangent_azimuth_degrees: azimuth, offset });
            }
            remaining -= length;
            station += length;
        }
        Err(SurveyError::InvalidValue("requested distance lies beyond the alignment".into()))
    }

    pub fn point_at_station(&self, station: f64, offset: f64) -> Result<AlignmentPoint, SurveyError> {
        if !station.is_finite() {
            return Err(SurveyError::InvalidValue("station must be finite".into()));
        }
        // Station equations are inverted from the last applicable equation.
        let mut distance = station - self.start_station;
        for eq in self.station_equations.iter().rev() {
            if station >= eq.ahead_station {
                distance = eq.back_station - self.start_station + (station - eq.ahead_station);
                break;
            }
        }
        self.point_at_distance(distance, offset)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProfilePoint {
    pub station: f64,
    pub elevation: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VerticalProfile {
    pub name: String,
    pub points: Vec<ProfilePoint>,
}

impl VerticalProfile {
    pub fn validate(&self) -> Result<(), SurveyError> {
        if self.name.trim().is_empty() || self.points.len() < 2 {
            return Err(SurveyError::InvalidValue("profile needs a name and at least two points".into()));
        }
        let mut previous = f64::NEG_INFINITY;
        for p in &self.points {
            if !p.station.is_finite() || !p.elevation.is_finite() || p.station <= previous {
                return Err(SurveyError::InvalidValue("profile stations must be finite and strictly increasing; elevations finite".into()));
            }
            previous = p.station;
        }
        Ok(())
    }

    pub fn elevation_at(&self, station: f64) -> Result<f64, SurveyError> {
        self.validate()?;
        if !station.is_finite() {
            return Err(SurveyError::InvalidValue("profile query station must be finite".into()));
        }
        for pair in self.points.windows(2) {
            if station >= pair[0].station && station <= pair[1].station {
                let t = (station - pair[0].station) / (pair[1].station - pair[0].station);
                return Ok(pair[0].elevation + t * (pair[1].elevation - pair[0].elevation));
            }
        }
        Err(SurveyError::InvalidValue("profile query is outside the profile extents".into()))
    }

    pub fn grade_percent(&self, segment_index: usize) -> Result<f64, SurveyError> {
        self.validate()?;
        let a = self.points.get(segment_index).ok_or_else(|| SurveyError::InvalidValue("profile segment index is out of range".into()))?;
        let b = self.points.get(segment_index + 1).ok_or_else(|| SurveyError::InvalidValue("profile segment index is out of range".into()))?;
        let grade = (b.elevation - a.elevation) / (b.station - a.station) * 100.0;
        if grade.is_finite() { Ok(grade) } else { Err(SurveyError::InvalidValue("profile grade overflowed".into())) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SectionOffsetElevation {
    pub offset: f64,
    pub elevation: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CrossSection {
    pub station: f64,
    pub points: Vec<SectionOffsetElevation>,
}

impl CrossSection {
    pub fn validate(&self) -> Result<(), SurveyError> {
        if !self.station.is_finite() || self.points.len() < 2 {
            return Err(SurveyError::InvalidValue("cross section needs a finite station and at least two points".into()));
        }
        let mut previous = f64::NEG_INFINITY;
        for p in &self.points {
            if !p.offset.is_finite() || !p.elevation.is_finite() || p.offset <= previous {
                return Err(SurveyError::InvalidValue("cross-section offsets must be finite and strictly increasing".into()));
            }
            previous = p.offset;
        }
        Ok(())
    }

    /// Area between the section and a horizontal datum, by trapezoidal integration.
    pub fn area_to_datum(&self, datum: f64) -> Result<(f64, f64), SurveyError> {
        self.validate()?;
        if !datum.is_finite() {
            return Err(SurveyError::InvalidValue("section datum must be finite".into()));
        }
        let mut cut = 0.0;
        let mut fill = 0.0;
        for pair in self.points.windows(2) {
            let width = pair[1].offset - pair[0].offset;
            let a = pair[0].elevation - datum;
            let b = pair[1].elevation - datum;
            if a >= 0.0 && b >= 0.0 { cut += (a + b) * 0.5 * width; }
            else if a <= 0.0 && b <= 0.0 { fill += (-a - b) * 0.5 * width; }
            else {
                let fraction = a.abs() / (a.abs() + b.abs());
                let left = width * fraction;
                let right = width - left;
                if a > 0.0 { cut += a * left * 0.5; fill += -b * right * 0.5; }
                else { fill += -a * left * 0.5; cut += b * right * 0.5; }
            }
        }
        if cut.is_finite() && fill.is_finite() { Ok((cut, fill)) } else { Err(SurveyError::InvalidValue("cross-section area overflowed".into())) }
    }
}


/// Residual diagnostics for a redundant 2D similarity control fit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimilarityFit {
    pub scale: f64,
    pub rotation_radians: f64,
    pub translation: Vec2,
    pub residuals: Vec<Vec2>,
    pub rms: f64,
    pub maximum_residual: f64,
}

/// Fits a 2D similarity transformation by unweighted least squares.
/// At least two non-identical source controls are required. This is not a
/// substitute for a weighted geodetic network adjustment.
pub fn fit_similarity(source: &[Vec2], target: &[Vec2]) -> Result<SimilarityFit, SurveyError> {
    if source.len() != target.len() || source.len() < 2 {
        return Err(SurveyError::NotEnoughPoints(2));
    }
    if source.iter().chain(target.iter()).any(|p| !p.x.is_finite() || !p.y.is_finite()) {
        return Err(SurveyError::InvalidValue("control coordinates must be finite".into()));
    }
    let n = source.len() as f64;
    let sx = source.iter().map(|p| p.x).sum::<f64>() / n;
    let sy = source.iter().map(|p| p.y).sum::<f64>() / n;
    let tx = target.iter().map(|p| p.x).sum::<f64>() / n;
    let ty = target.iter().map(|p| p.y).sum::<f64>() / n;
    let mut dot = 0.0;
    let mut cross = 0.0;
    let mut denom = 0.0;
    for (s, t) in source.iter().zip(target.iter()) {
        let x = s.x - sx;
        let y = s.y - sy;
        let u = t.x - tx;
        let v = t.y - ty;
        dot += x * u + y * v;
        cross += x * v - y * u;
        denom += x * x + y * y;
    }
    if !denom.is_finite() || denom <= f64::EPSILON {
        return Err(SurveyError::DegenerateControls);
    }
    let scale = dot.hypot(cross) / denom;
    let rotation_radians = cross.atan2(dot);
    let (sin, cos) = rotation_radians.sin_cos();
    let translation = Vec2 { x: tx - scale * (cos * sx - sin * sy), y: ty - scale * (sin * sx + cos * sy) };
    if !scale.is_finite() || scale <= 0.0 || !translation.x.is_finite() || !translation.y.is_finite() {
        return Err(SurveyError::DegenerateControls);
    }
    let residuals: Vec<Vec2> = source.iter().zip(target.iter()).map(|(s, t)| {
        let predicted = Vec2 { x: scale * (cos * s.x - sin * s.y) + translation.x, y: scale * (sin * s.x + cos * s.y) + translation.y };
        Vec2 { x: t.x - predicted.x, y: t.y - predicted.y }
    }).collect();
    let sum_sq = residuals.iter().map(|r| r.x * r.x + r.y * r.y).sum::<f64>();
    let maximum_residual = residuals.iter().map(|r| r.x.hypot(r.y)).fold(0.0, f64::max);
    let rms = (sum_sq / n).sqrt();
    if !rms.is_finite() || !maximum_residual.is_finite() {
        return Err(SurveyError::InvalidValue("control residual computation overflowed".into()));
    }
    Ok(SimilarityFit { scale, rotation_radians, translation, residuals, rms, maximum_residual })
}

/// Average-end-area volume between two cross sections.
pub fn average_end_area_volume(a: &CrossSection, b: &CrossSection, datum: f64) -> Result<(f64, f64), SurveyError> {
    a.validate()?;
    b.validate()?;
    if !datum.is_finite() || b.station <= a.station {
        return Err(SurveyError::InvalidValue("cross-section stations must increase and datum must be finite".into()));
    }
    let (cut_a, fill_a) = a.area_to_datum(datum)?;
    let (cut_b, fill_b) = b.area_to_datum(datum)?;
    let length = b.station - a.station;
    let cut = (cut_a + cut_b) * 0.5 * length;
    let fill = (fill_a + fill_b) * 0.5 * length;
    if cut.is_finite() && fill.is_finite() { Ok((cut, fill)) } else { Err(SurveyError::InvalidValue("average-end-area volume overflowed".into())) }
}

/// Applies an explicitly supplied grid-to-ground factor to a horizontal distance.
/// The caller is responsible for documenting whether the factor is grid/ground or ground/grid.
pub fn apply_combined_scale_factor(distance: f64, factor: f64) -> Result<f64, SurveyError> {
    if !distance.is_finite() || distance < 0.0 || !factor.is_finite() || factor <= 0.0 {
        return Err(SurveyError::InvalidValue("distance and scale factor must be finite and non-negative/positive".into()));
    }
    let result = distance * factor;
    if result.is_finite() { Ok(result) } else { Err(SurveyError::InvalidValue("scaled distance overflowed".into())) }
}

#[cfg(test)]
mod additional_civil_tests {
    use super::*;

    #[test]
    fn redundant_similarity_fit_reports_residuals() {
        let source = [Vec2 { x: 0.0, y: 0.0 }, Vec2 { x: 10.0, y: 0.0 }, Vec2 { x: 0.0, y: 10.0 }, Vec2 { x: 10.0, y: 10.0 }];
        let target = [Vec2 { x: 100.0, y: 200.0 }, Vec2 { x: 110.0, y: 200.0 }, Vec2 { x: 100.0, y: 210.0 }, Vec2 { x: 110.0, y: 210.0 }];
        let fit = fit_similarity(&source, &target);
        assert!(fit.is_ok_and(|v| v.rms < 1.0e-9 && v.maximum_residual < 1.0e-9));
    }

    #[test]
    fn average_end_area_volume_uses_station_spacing() {
        let section = |station| CrossSection { station, points: vec![SectionOffsetElevation { offset: -1.0, elevation: 12.0 }, SectionOffsetElevation { offset: 1.0, elevation: 12.0 }] };
        assert_eq!(average_end_area_volume(&section(0.0), &section(10.0), 10.0), Ok((40.0, 0.0)));
    }

    #[test]
    fn scale_factor_is_explicit_and_checked() {
        assert_eq!(apply_combined_scale_factor(100.0, 0.9996), Ok(99.96));
        assert!(apply_combined_scale_factor(100.0, 0.0).is_err());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_international_and_us_survey_feet() {
        let converted = LinearUnit::UsSurveyFoot.convert(1000.0, LinearUnit::InternationalFoot);
        assert!(converted.is_ok());
        assert!(converted.is_ok_and(|v| (v - 999.998).abs() < 1.0e-6));
    }

    #[test]
    fn validates_crs_metadata_and_grid_ground_declaration() {
        let metadata = SurveyCrsMetadata {
            authority: Some("EPSG".into()), code: Some("26910".into()), name: "NAD83 / UTM zone 10N".into(),
            datum: Some("NAD83".into()), projection: Some("Transverse Mercator".into()), geoid_model: None,
            coordinate_epoch: None, horizontal_unit: LinearUnit::Metre, vertical_unit: LinearUnit::Metre,
            axis_order: AxisOrder::EastingNorthing, coordinate_use: CoordinateUse::Grid, combined_scale_factor: Some(0.9996),
        };
        assert!(metadata.validate().is_ok());
    }

    #[test]
    fn alignment_interpolates_and_offsets_to_right() {
        let alignment = HorizontalAlignment { name: "CL".into(), points: vec![Vec2 { x: 0.0, y: 0.0 }, Vec2 { x: 100.0, y: 0.0 }], start_station: 1000.0, station_equations: Vec::new() };
        let result = alignment.point_at_station(1050.0, 2.0);
        assert!(result.is_ok());
        assert!(result.is_ok_and(|p| (p.coordinate.x - 50.0).abs() < 1e-9 && (p.coordinate.y + 2.0).abs() < 1e-9));
    }

    #[test]
    fn profile_interpolates_and_computes_grade() {
        let profile = VerticalProfile { name: "FG".into(), points: vec![ProfilePoint { station: 0.0, elevation: 10.0 }, ProfilePoint { station: 100.0, elevation: 15.0 }] };
        assert_eq!(profile.elevation_at(50.0), Ok(12.5));
        assert_eq!(profile.grade_percent(0), Ok(5.0));
    }

    #[test]
    fn cross_section_integrates_cut_and_fill() {
        let section = CrossSection { station: 0.0, points: vec![SectionOffsetElevation { offset: -5.0, elevation: 12.0 }, SectionOffsetElevation { offset: 5.0, elevation: 8.0 }] };
        let areas = section.area_to_datum(10.0);
        assert!(areas.is_ok());
        assert!(areas.is_ok_and(|(cut, fill)| (cut - 2.5).abs() < 1e-9 && (fill - 2.5).abs() < 1e-9));
    }
}
