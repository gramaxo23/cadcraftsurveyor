//! Delaunay TIN construction and terrain queries.
//!
//! Coordinates are projected drawing units. Input is bounded to keep worst-case work predictable.
use crate::{SurveyError, SurveyPoint, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Maximum number of scattered survey points accepted by the current TIN builder.
pub const MAX_TIN_POINTS: usize = 5_000;
const MAX_CONTOUR_LEVELS: usize = 1_000_000;
const MAX_STITCH_SEGMENTS: usize = 1_000_000;
const PREDICATE_EPS: f64 = 1.0e-12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Triangle {
    pub vertices: [usize; 3],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tin {
    pub vertices: Vec<SurveyPoint>,
    pub triangles: Vec<Triangle>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TinContourSegment {
    pub elevation: f64,
    pub start: Vec2,
    pub end: Vec2,
}

/// A stitched contour component at one elevation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TinContourPolyline {
    pub elevation: f64,
    pub vertices: Vec<Vec2>,
}

fn orient(a: Vec2, b: Vec2, c: Vec2) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

fn in_circumcircle(a: Vec2, b: Vec2, c: Vec2, p: Vec2) -> bool {
    let ax = a.x - p.x;
    let ay = a.y - p.y;
    let bx = b.x - p.x;
    let by = b.y - p.y;
    let cx = c.x - p.x;
    let cy = c.y - p.y;
    let det = (ax * ax + ay * ay) * (bx * cy - by * cx) - (bx * bx + by * by) * (ax * cy - ay * cx) + (cx * cx + cy * cy) * (ax * by - ay * bx);
    let direction = orient(a, b, c);
    if direction > 0.0 { det > -PREDICATE_EPS } else { det < PREDICATE_EPS }
}

fn normalise(points: &[SurveyPoint]) -> Result<Vec<Vec2>, SurveyError> {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for p in points {
        p.validate()?;
        min_x = min_x.min(p.x);
        min_y = min_y.min(p.y);
        max_x = max_x.max(p.x);
        max_y = max_y.max(p.y);
    }
    let span = (max_x - min_x).max(max_y - min_y);
    if !span.is_finite() || span <= f64::EPSILON {
        return Err(SurveyError::DegenerateControls);
    }
    Ok(points.iter().map(|p| Vec2 { x: (p.x - min_x) / span, y: (p.y - min_y) / span }).collect())
}


#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum ContourPointKey {
    Vertex(usize),
    Edge(usize, usize),
}

#[derive(Clone, Copy)]
struct ContourIntersection {
    key: ContourPointKey,
    position: Vec2,
}

fn triangle_points<'a>(vertices: &'a [SurveyPoint], triangle: &Triangle) -> Result<[&'a SurveyPoint; 3], SurveyError> {
    let a = vertices.get(triangle.vertices[0]).ok_or(SurveyError::DegenerateControls)?;
    let b = vertices.get(triangle.vertices[1]).ok_or(SurveyError::DegenerateControls)?;
    let c = vertices.get(triangle.vertices[2]).ok_or(SurveyError::DegenerateControls)?;
    Ok([a, b, c])
}

fn elevation_side(value: f64, level: f64, tolerance: f64) -> i8 {
    let difference = value - level;
    if difference.is_infinite() {
        return if value > level { 1 } else { -1 };
    }
    if difference < -tolerance {
        -1
    } else if difference > tolerance {
        1
    } else {
        0
    }
}

fn contour_intersection(
    first_index: usize,
    second_index: usize,
    vertices: &[SurveyPoint],
    level: f64,
    level_tolerance: f64,
) -> Result<ContourIntersection, SurveyError> {
    let first = vertices.get(first_index).ok_or(SurveyError::DegenerateControls)?;
    let second = vertices.get(second_index).ok_or(SurveyError::DegenerateControls)?;
    if elevation_side(first.elevation, level, level_tolerance) == 0 {
        return Ok(ContourIntersection {
            key: ContourPointKey::Vertex(first_index),
            position: Vec2 { x: first.x, y: first.y },
        });
    }
    if elevation_side(second.elevation, level, level_tolerance) == 0 {
        return Ok(ContourIntersection {
            key: ContourPointKey::Vertex(second_index),
            position: Vec2 { x: second.x, y: second.y },
        });
    }

    let (low_index, high_index) = if first_index <= second_index { (first_index, second_index) } else { (second_index, first_index) };
    let low = vertices.get(low_index).ok_or(SurveyError::DegenerateControls)?;
    let high = vertices.get(high_index).ok_or(SurveyError::DegenerateControls)?;
    let z_scale = low.elevation.abs().max(high.elevation.abs()).max(level.abs()).max(f64::MIN_POSITIVE);
    let low_z = low.elevation / z_scale;
    let high_z = high.elevation / z_scale;
    let level_z = level / z_scale;
    let denominator = high_z - low_z;
    if !denominator.is_finite() || denominator == 0.0 {
        return Err(SurveyError::InvalidValue("contour edge interpolation is numerically unstable".into()));
    }
    let t = ((level_z - low_z) / denominator).clamp(0.0, 1.0);
    if !t.is_finite() {
        return Err(SurveyError::InvalidValue("contour edge interpolation overflowed".into()));
    }
    let position = Vec2 {
        x: low.x * (1.0 - t) + high.x * t,
        y: low.y * (1.0 - t) + high.y * t,
    };
    if !position.x.is_finite() || !position.y.is_finite() {
        return Err(SurveyError::InvalidValue("contour coordinate interpolation overflowed".into()));
    }
    Ok(ContourIntersection { key: ContourPointKey::Edge(low_index, high_index), position })
}

fn push_contour_segment(
    level_index: i64,
    elevation: f64,
    first: ContourIntersection,
    second: ContourIntersection,
    seen: &mut HashSet<(i64, ContourPointKey, ContourPointKey)>,
    output: &mut Vec<TinContourSegment>,
) {
    if first.key == second.key {
        return;
    }
    let (start, end, first_key, second_key) = if first.key <= second.key {
        (first.position, second.position, first.key, second.key)
    } else {
        (second.position, first.position, second.key, first.key)
    };
    if seen.insert((level_index, first_key, second_key)) {
        output.push(TinContourSegment { elevation, start, end });
    }
}

fn contour_level_ranges(tin: &Tin, interval: f64) -> Result<Vec<Option<(i64, usize)>>, SurveyError> {
    if tin.triangles.len() > MAX_STITCH_SEGMENTS {
        return Err(SurveyError::InputTooLarge);
    }
    let mut ranges = Vec::with_capacity(tin.triangles.len());
    let mut total_levels = 0usize;
    for triangle in &tin.triangles {
        let points = triangle_points(&tin.vertices, triangle)?;
        for point in points {
            point.validate()?;
        }
        let min = points.iter().map(|point| point.elevation).fold(f64::INFINITY, f64::min);
        let max = points.iter().map(|point| point.elevation).fold(f64::NEG_INFINITY, f64::max);
        if min == max {
            ranges.push(None);
            continue;
        }
        let level_tolerance = min.abs().max(max.abs()).max(1.0) * f64::EPSILON * 8.0;
        if interval <= level_tolerance {
            return Err(SurveyError::InvalidValue("contour interval is below the floating-point resolution of the elevations".into()));
        }
        let first_level = (min / interval).ceil();
        let last_level = (max / interval).floor();
        if !first_level.is_finite() || !last_level.is_finite() {
            return Err(SurveyError::InputTooLarge);
        }
        if first_level > last_level {
            ranges.push(None);
            continue;
        }
        const MIN_LEVEL_INDEX: f64 = -9_223_372_036_854_775_808.0;
        const MAX_LEVEL_INDEX_EXCLUSIVE: f64 = 9_223_372_036_854_775_808.0;
        if first_level < MIN_LEVEL_INDEX
            || first_level >= MAX_LEVEL_INDEX_EXCLUSIVE
            || last_level < MIN_LEVEL_INDEX
            || last_level >= MAX_LEVEL_INDEX_EXCLUSIVE
        {
            return Err(SurveyError::InputTooLarge);
        }
        let candidate_count = last_level - first_level + 1.0;
        if !candidate_count.is_finite() || candidate_count > MAX_CONTOUR_LEVELS as f64 {
            return Err(SurveyError::InputTooLarge);
        }
        let count = candidate_count as usize;
        total_levels = total_levels.checked_add(count).ok_or(SurveyError::InputTooLarge)?;
        if total_levels > MAX_CONTOUR_LEVELS {
            return Err(SurveyError::InputTooLarge);
        }
        ranges.push(Some((first_level as i64, count)));
    }
    Ok(ranges)
}

fn append_triangle_contour(
    tin: &Tin,
    triangle: &Triangle,
    level_index: i64,
    level: f64,
    seen: &mut HashSet<(i64, ContourPointKey, ContourPointKey)>,
    output: &mut Vec<TinContourSegment>,
) -> Result<(), SurveyError> {
    let points = triangle_points(&tin.vertices, triangle)?;
    let min = points.iter().map(|point| point.elevation).fold(f64::INFINITY, f64::min);
    let max = points.iter().map(|point| point.elevation).fold(f64::NEG_INFINITY, f64::max);
    if level < min || level > max {
        return Ok(());
    }
    let level_tolerance = min.abs().max(max.abs()).max(1.0) * f64::EPSILON * 8.0;
    let on_level: Vec<usize> = points
        .iter()
        .enumerate()
        .filter_map(|(index, point)| (elevation_side(point.elevation, level, level_tolerance) == 0).then_some(index))
        .collect();
    if on_level.len() == 3 {
        return Ok(());
    }
    if on_level.len() == 2 {
        let first_local = *on_level.first().ok_or(SurveyError::DegenerateControls)?;
        let second_local = *on_level.get(1).ok_or(SurveyError::DegenerateControls)?;
        let first_vertex = *triangle.vertices.get(first_local).ok_or(SurveyError::DegenerateControls)?;
        let second_vertex = *triangle.vertices.get(second_local).ok_or(SurveyError::DegenerateControls)?;
        let first_point = tin.vertices.get(first_vertex).ok_or(SurveyError::DegenerateControls)?;
        let second_point = tin.vertices.get(second_vertex).ok_or(SurveyError::DegenerateControls)?;
        push_contour_segment(
            level_index,
            level,
            ContourIntersection {
                key: ContourPointKey::Vertex(first_vertex),
                position: Vec2 { x: first_point.x, y: first_point.y },
            },
            ContourIntersection {
                key: ContourPointKey::Vertex(second_vertex),
                position: Vec2 { x: second_point.x, y: second_point.y },
            },
            seen,
            output,
        );
        return Ok(());
    }

    let mut hits = Vec::with_capacity(2);
    for (first_local, second_local) in [(0usize, 1usize), (1, 2), (2, 0)] {
        let first_point = points.get(first_local).ok_or(SurveyError::DegenerateControls)?;
        let second_point = points.get(second_local).ok_or(SurveyError::DegenerateControls)?;
        let first_side = elevation_side(first_point.elevation, level, level_tolerance);
        let second_side = elevation_side(second_point.elevation, level, level_tolerance);
        let crosses = (first_side <= 0 && second_side > 0) || (second_side <= 0 && first_side > 0);
        if !crosses {
            continue;
        }
        let first_vertex = *triangle.vertices.get(first_local).ok_or(SurveyError::DegenerateControls)?;
        let second_vertex = *triangle.vertices.get(second_local).ok_or(SurveyError::DegenerateControls)?;
        let hit = contour_intersection(first_vertex, second_vertex, &tin.vertices, level, level_tolerance)?;
        if !hits.iter().any(|existing: &ContourIntersection| existing.key == hit.key) {
            hits.push(hit);
        }
    }
    if hits.len() == 2 {
        let first = *hits.first().ok_or(SurveyError::DegenerateControls)?;
        let second = *hits.get(1).ok_or(SurveyError::DegenerateControls)?;
        push_contour_segment(level_index, level, first, second, seen, output);
    }
    Ok(())
}

impl Tin {
    /// Builds a 2D Delaunay triangulation using bounded Bowyer-Watson insertion.
    /// Duplicate XY positions are rejected rather than silently dropping survey records.
    pub fn from_points(points: &[SurveyPoint]) -> Result<Self, SurveyError> {
        if points.len() < 3 {
            return Err(SurveyError::NotEnoughPoints(3));
        }
        if points.len() > MAX_TIN_POINTS {
            return Err(SurveyError::InputTooLarge);
        }
        let mut xy = normalise(points)?;
        for i in 0..xy.len() {
            for j in 0..i {
                let a = xy.get(i).ok_or(SurveyError::DegenerateControls)?;
                let b = xy.get(j).ok_or(SurveyError::DegenerateControls)?;
                if (a.x - b.x).hypot(a.y - b.y) <= PREDICATE_EPS {
                    return Err(SurveyError::InvalidValue("TIN contains duplicate XY coordinates".into()));
                }
            }
        }
        let n = xy.len();
        xy.extend([Vec2 { x: -32.0, y: -2.0 }, Vec2 { x: 32.0, y: -2.0 }, Vec2 { x: 0.5, y: 32.0 }]);
        let mut triangles = vec![[n, n + 1, n + 2]];
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by(|a, b| match (xy.get(*a), xy.get(*b)) {
            (Some(a), Some(b)) => a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)),
            _ => a.cmp(b),
        });
        for point_index in order {
            let p = *xy.get(point_index).ok_or(SurveyError::DegenerateControls)?;
            let mut bad = Vec::new();
            for (index, t) in triangles.iter().enumerate() {
                let a = *xy.get(t[0]).ok_or(SurveyError::DegenerateControls)?;
                let b = *xy.get(t[1]).ok_or(SurveyError::DegenerateControls)?;
                let c = *xy.get(t[2]).ok_or(SurveyError::DegenerateControls)?;
                if in_circumcircle(a, b, c, p) {
                    bad.push(index);
                }
            }
            if bad.is_empty() {
                return Err(SurveyError::DegenerateControls);
            }
            let mut boundary: BTreeMap<(usize, usize), (usize, usize, usize)> = BTreeMap::new();
            for index in bad.iter().rev() {
                let t = triangles.remove(*index);
                for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                    let key = if a < b { (a, b) } else { (b, a) };
                    let entry = boundary.entry(key).or_insert((a, b, 0));
                    entry.2 += 1;
                }
            }
            for (_, (a, b, count)) in boundary {
                if count != 1 {
                    continue;
                }
                let direction = orient(*xy.get(a).ok_or(SurveyError::DegenerateControls)?, *xy.get(b).ok_or(SurveyError::DegenerateControls)?, p);
                if direction.abs() <= PREDICATE_EPS {
                    continue;
                }
                triangles.push(if direction > 0.0 { [a, b, point_index] } else { [b, a, point_index] });
            }
        }
        let mut output = Vec::new();
        for t in triangles {
            if t.iter().any(|index| *index >= n) {
                continue;
            }
            let a = *xy.get(t[0]).ok_or(SurveyError::DegenerateControls)?;
            let b = *xy.get(t[1]).ok_or(SurveyError::DegenerateControls)?;
            let c = *xy.get(t[2]).ok_or(SurveyError::DegenerateControls)?;
            if orient(a, b, c).abs() > PREDICATE_EPS {
                output.push(Triangle { vertices: t });
            }
        }
        if output.is_empty() {
            return Err(SurveyError::DegenerateControls);
        }
        Ok(Self { vertices: points.to_vec(), triangles: output })
    }

    /// Returns interpolated elevation if query lies inside a TIN triangle.
    pub fn elevation_at(&self, query: Vec2) -> Result<Option<f64>, SurveyError> {
        if !query.x.is_finite() || !query.y.is_finite() {
            return Err(SurveyError::InvalidValue("TIN query coordinate is not finite".into()));
        }
        if self.vertices.is_empty() {
            return Ok(None);
        }
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for vertex in &self.vertices {
            vertex.validate()?;
            min_x = min_x.min(vertex.x);
            min_y = min_y.min(vertex.y);
            max_x = max_x.max(vertex.x);
            max_y = max_y.max(vertex.y);
        }
        if !(max_x - min_x).is_finite() || !(max_y - min_y).is_finite() {
            return Err(SurveyError::InvalidValue("TIN coordinate range is too large for stable interpolation".into()));
        }
        if query.x < min_x || query.x > max_x || query.y < min_y || query.y > max_y {
            return Ok(None);
        }
        for triangle in &self.triangles {
            let [a, b, c] = triangle_points(&self.vertices, triangle)?;
            let ab_x = b.x - a.x;
            let ab_y = b.y - a.y;
            let ac_x = c.x - a.x;
            let ac_y = c.y - a.y;
            let aq_x = query.x - a.x;
            let aq_y = query.y - a.y;
            let scale = ab_x.abs().max(ab_y.abs()).max(ac_x.abs()).max(ac_y.abs());
            if !scale.is_finite() || scale == 0.0 {
                continue;
            }
            let ab_x = ab_x / scale;
            let ab_y = ab_y / scale;
            let ac_x = ac_x / scale;
            let ac_y = ac_y / scale;
            let aq_x = aq_x / scale;
            let aq_y = aq_y / scale;
            let area = ab_x * ac_y - ab_y * ac_x;
            if !area.is_finite() || area.abs() <= f64::EPSILON {
                continue;
            }
            let weight_b = (aq_x * ac_y - aq_y * ac_x) / area;
            let weight_c = (ab_x * aq_y - ab_y * aq_x) / area;
            let weight_a = 1.0 - weight_b - weight_c;
            if weight_a >= -PREDICATE_EPS && weight_b >= -PREDICATE_EPS && weight_c >= -PREDICATE_EPS {
                let elevation = weight_a * a.elevation + weight_b * b.elevation + weight_c * c.elevation;
                if elevation.is_finite() {
                    return Ok(Some(elevation));
                }
                return Err(SurveyError::InvalidValue("TIN interpolation overflowed".into()));
            }
        }
        Ok(None)
    }

    /// Emits contour segments per triangle using a half-open edge rule.
    ///
    /// Vertices and horizontal edges exactly on a contour level are handled explicitly, and
    /// shared edges are emitted only once. Candidate levels are capped before allocation.
    pub fn contour_segments(&self, interval: f64) -> Result<Vec<TinContourSegment>, SurveyError> {
        if !interval.is_finite() || interval <= 0.0 {
            return Err(SurveyError::InvalidValue("contour interval must be positive".into()));
        }
        let ranges = contour_level_ranges(self, interval)?;
        let mut output = Vec::new();
        let mut seen = HashSet::new();
        for (triangle, range) in self.triangles.iter().zip(ranges) {
            let Some((first_level_index, count)) = range else {
                continue;
            };
            for offset in 0..count {
                let offset = i64::try_from(offset).map_err(|_| SurveyError::InputTooLarge)?;
                let level_index = first_level_index.checked_add(offset).ok_or(SurveyError::InputTooLarge)?;
                let level = level_index as f64 * interval;
                if !level.is_finite() {
                    return Err(SurveyError::InputTooLarge);
                }
                append_triangle_contour(self, triangle, level_index, level, &mut seen, &mut output)?;
            }
        }
        Ok(output)
    }

}

/// A graph-based contour stitcher that preserves branches and closed loops.
///
/// XY and elevation tolerances are separate because horizontal and vertical units can differ.
/// At junctions with degree other than two, each branch is emitted as a separate component.
pub fn stitch_contour_polylines(
    segments: &[TinContourSegment],
    xy_tolerance: f64,
    elevation_tolerance: f64,
) -> Result<Vec<TinContourPolyline>, SurveyError> {
    if !xy_tolerance.is_finite() || xy_tolerance <= 0.0 || !elevation_tolerance.is_finite() || elevation_tolerance <= 0.0 {
        return Err(SurveyError::InvalidValue("contour stitching tolerances must be positive".into()));
    }
    if segments.len() > MAX_STITCH_SEGMENTS {
        return Err(SurveyError::InputTooLarge);
    }
    if segments.is_empty() {
        return Ok(Vec::new());
    }
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut min_elevation = f64::INFINITY;
    for segment in segments {
        if !segment.elevation.is_finite()
            || !segment.start.x.is_finite()
            || !segment.start.y.is_finite()
            || !segment.end.x.is_finite()
            || !segment.end.y.is_finite()
        {
            return Err(SurveyError::InvalidValue("contour segment contains a non-finite coordinate".into()));
        }
        min_x = min_x.min(segment.start.x).min(segment.end.x);
        min_y = min_y.min(segment.start.y).min(segment.end.y);
        min_elevation = min_elevation.min(segment.elevation);
    }
    let mut builder = ContourGraphBuilder::new(min_x, min_y, min_elevation, xy_tolerance, elevation_tolerance);
    for segment in segments {
        builder.add_segment(segment)?;
    }
    let graph = builder.finish();
    let mut visited = vec![false; graph.edges.len()];
    let mut output = Vec::new();
    for node_index in 0..graph.nodes.len() {
        let incident_edges = graph.nodes.get(node_index).ok_or(SurveyError::DegenerateControls)?.edges.clone();
        if incident_edges.len() == 2 {
            continue;
        }
        for edge_index in incident_edges {
            if !visited.get(edge_index).copied().unwrap_or(true) {
                output.push(trace_contour(node_index, edge_index, &graph, &mut visited)?);
            }
        }
    }
    for edge_index in 0..graph.edges.len() {
        if visited.get(edge_index).copied().unwrap_or(true) {
            continue;
        }
        let start_node = graph.edges.get(edge_index).ok_or(SurveyError::DegenerateControls)?.start;
        output.push(trace_contour(start_node, edge_index, &graph, &mut visited)?);
    }
    Ok(output)
}

/// Compatibility helper using the same tolerance horizontally and vertically.
pub fn stitch_contours(segments: &[TinContourSegment], tolerance: f64) -> Result<Vec<Vec<Vec2>>, SurveyError> {
    stitch_contour_polylines(segments, tolerance, tolerance).map(|lines| lines.into_iter().map(|line| line.vertices).collect())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct QuantizedKey {
    x: i64,
    y: i64,
    elevation: i64,
}

struct ContourNode {
    position: Vec2,
    elevation: f64,
    edges: Vec<usize>,
}

struct ContourEdge {
    start: usize,
    end: usize,
}

struct ContourGraph {
    nodes: Vec<ContourNode>,
    edges: Vec<ContourEdge>,
}

struct ContourGraphBuilder {
    origin_x: f64,
    origin_y: f64,
    origin_elevation: f64,
    xy_tolerance: f64,
    elevation_tolerance: f64,
    buckets: HashMap<QuantizedKey, Vec<usize>>,
    nodes: Vec<ContourNode>,
    edges: Vec<ContourEdge>,
    unique_edges: HashSet<(usize, usize)>,
}

impl ContourGraphBuilder {
    fn new(origin_x: f64, origin_y: f64, origin_elevation: f64, xy_tolerance: f64, elevation_tolerance: f64) -> Self {
        Self {
            origin_x,
            origin_y,
            origin_elevation,
            xy_tolerance,
            elevation_tolerance,
            buckets: HashMap::new(),
            nodes: Vec::new(),
            edges: Vec::new(),
            unique_edges: HashSet::new(),
        }
    }

    fn add_segment(&mut self, segment: &TinContourSegment) -> Result<(), SurveyError> {
        let start = self.find_or_insert_node(segment.start, segment.elevation)?;
        let end = self.find_or_insert_node(segment.end, segment.elevation)?;
        if start == end {
            return Ok(());
        }
        let edge_key = if start < end { (start, end) } else { (end, start) };
        if !self.unique_edges.insert(edge_key) {
            return Ok(());
        }
        let edge_index = self.edges.len();
        self.edges.push(ContourEdge { start, end });
        self.nodes.get_mut(start).ok_or(SurveyError::DegenerateControls)?.edges.push(edge_index);
        self.nodes.get_mut(end).ok_or(SurveyError::DegenerateControls)?.edges.push(edge_index);
        Ok(())
    }

    fn find_or_insert_node(&mut self, position: Vec2, elevation: f64) -> Result<usize, SurveyError> {
        let key = QuantizedKey {
            x: quantize_coordinate(position.x, self.origin_x, self.xy_tolerance)?,
            y: quantize_coordinate(position.y, self.origin_y, self.xy_tolerance)?,
            elevation: quantize_coordinate(elevation, self.origin_elevation, self.elevation_tolerance)?,
        };
        let mut matched = None;
        for dx in -1i64..=1 {
            for dy in -1i64..=1 {
                for dz in -1i64..=1 {
                    let Some(x) = key.x.checked_add(dx) else { continue };
                    let Some(y) = key.y.checked_add(dy) else { continue };
                    let Some(elevation_key) = key.elevation.checked_add(dz) else { continue };
                    let neighbor = QuantizedKey { x, y, elevation: elevation_key };
                    if let Some(candidates) = self.buckets.get(&neighbor) {
                        for candidate in candidates {
                            let node = self.nodes.get(*candidate).ok_or(SurveyError::DegenerateControls)?;
                            let distance = (node.position.x - position.x).hypot(node.position.y - position.y);
                            let elevation_distance = (node.elevation - elevation).abs();
                            if distance <= self.xy_tolerance
                                && elevation_distance <= self.elevation_tolerance
                                && matched.map_or(true, |current| *candidate < current)
                            {
                                matched = Some(*candidate);
                            }
                        }
                    }
                }
            }
        }
        if let Some(index) = matched {
            return Ok(index);
        }
        let index = self.nodes.len();
        self.nodes.push(ContourNode { position, elevation, edges: Vec::new() });
        self.buckets.entry(key).or_default().push(index);
        Ok(index)
    }

    fn finish(self) -> ContourGraph {
        ContourGraph { nodes: self.nodes, edges: self.edges }
    }
}

fn quantize_coordinate(value: f64, origin: f64, tolerance: f64) -> Result<i64, SurveyError> {
    let offset = value - origin;
    let scaled = offset / tolerance;
    const MAX_BUCKET_COORDINATE: f64 = 9_223_372_036_854_770_000.0;
    if !scaled.is_finite() || scaled < -MAX_BUCKET_COORDINATE || scaled > MAX_BUCKET_COORDINATE {
        return Err(SurveyError::InvalidValue("contour coordinates exceed the selected stitching tolerance range".into()));
    }
    Ok(scaled.floor() as i64)
}

fn trace_contour(
    start_node: usize,
    first_edge: usize,
    graph: &ContourGraph,
    visited: &mut [bool],
) -> Result<TinContourPolyline, SurveyError> {
    let mut path = vec![start_node];
    let mut current_node = start_node;
    let mut current_edge = first_edge;
    loop {
        if visited.get(current_edge).copied().unwrap_or(true) {
            break;
        }
        *visited.get_mut(current_edge).ok_or(SurveyError::DegenerateControls)? = true;
        let edge = graph.edges.get(current_edge).ok_or(SurveyError::DegenerateControls)?;
        let next_node = if edge.start == current_node {
            edge.end
        } else if edge.end == current_node {
            edge.start
        } else {
            return Err(SurveyError::DegenerateControls);
        };
        path.push(next_node);
        current_node = next_node;
        let node = graph.nodes.get(current_node).ok_or(SurveyError::DegenerateControls)?;
        if node.edges.len() != 2 {
            break;
        }
        let next_edge = node.edges.iter().copied().find(|candidate| !visited.get(*candidate).copied().unwrap_or(true));
        let Some(next_edge) = next_edge else { break };
        current_edge = next_edge;
    }
    let start = graph.nodes.get(start_node).ok_or(SurveyError::DegenerateControls)?;
    let mut vertices = Vec::with_capacity(path.len());
    for node_index in path {
        vertices.push(graph.nodes.get(node_index).ok_or(SurveyError::DegenerateControls)?.position);
    }
    Ok(TinContourPolyline { elevation: start.elevation, vertices })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(number: &str, x: f64, y: f64, elevation: f64) -> SurveyPoint {
        SurveyPoint { number: number.into(), x, y, elevation, description: String::new() }
    }

    fn segment(start: [f64; 2], end: [f64; 2], elevation: f64) -> TinContourSegment {
        TinContourSegment {
            elevation,
            start: Vec2 { x: start[0], y: start[1] },
            end: Vec2 { x: end[0], y: end[1] },
        }
    }

    #[test]
    fn triangulates_and_interpolates() {
        let points = [
            p("1", 0.0, 0.0, 0.0),
            p("2", 10.0, 0.0, 10.0),
            p("3", 0.0, 10.0, 20.0),
            p("4", 10.0, 10.0, 30.0),
        ];
        let model = Tin::from_points(&points).unwrap_or_else(|_| Tin { vertices: Vec::new(), triangles: Vec::new() });
        let z = model.elevation_at(Vec2 { x: 2.0, y: 2.0 });
        assert!(z.ok().flatten().is_some_and(|value| (value - 8.0).abs() < 1.0e-8));
    }

    #[test]
    fn interpolation_is_scale_independent_for_small_and_large_coordinates() {
        let small = [
            p("1", 0.0, 0.0, 0.0),
            p("2", 1.0e-8, 0.0, 10.0),
            p("3", 0.0, 1.0e-8, 20.0),
        ];
        let small_tin = Tin::from_points(&small).unwrap_or_else(|_| Tin { vertices: Vec::new(), triangles: Vec::new() });
        let small_z = small_tin.elevation_at(Vec2 { x: 2.0e-9, y: 3.0e-9 });
        assert!(small_z.ok().flatten().is_some_and(|value| (value - 8.0).abs() < 1.0e-8));

        let large = [
            p("1", 1.0e9, 1.0e9, 0.0),
            p("2", 1.0e9 + 10.0, 1.0e9, 10.0),
            p("3", 1.0e9, 1.0e9 + 10.0, 20.0),
        ];
        let large_tin = Tin::from_points(&large).unwrap_or_else(|_| Tin { vertices: Vec::new(), triangles: Vec::new() });
        let large_z = large_tin.elevation_at(Vec2 { x: 1.0e9 + 2.0, y: 1.0e9 + 3.0 });
        assert!(large_z.ok().flatten().is_some_and(|value| (value - 8.0).abs() < 1.0e-8));
    }

    #[test]
    fn rejects_duplicate_and_collinear_xy() {
        assert!(Tin::from_points(&[p("1", 0.0, 0.0, 0.0), p("2", 0.0, 0.0, 1.0), p("3", 1.0, 0.0, 2.0)]).is_err());
        assert!(Tin::from_points(&[p("1", 0.0, 0.0, 0.0), p("2", 1.0, 1.0, 1.0), p("3", 2.0, 2.0, 2.0)]).is_err());
    }

    #[test]
    fn contour_generation_deduplicates_a_shared_horizontal_edge() {
        let tin = Tin {
            vertices: vec![
                p("1", 0.0, 0.0, 5.0),
                p("2", 1.0, 0.0, 10.0),
                p("3", 1.0, 1.0, 5.0),
                p("4", 0.0, 1.0, 0.0),
            ],
            triangles: vec![Triangle { vertices: [0, 1, 2] }, Triangle { vertices: [0, 2, 3] }],
        };
        let contours = tin.contour_segments(5.0);
        assert_eq!(contours.as_ref().map(Vec::len), Ok(1));
        if let Ok(contours) = contours {
            let contour = contours.first();
            assert!(contour.is_some_and(|item| {
                item.elevation == 5.0
                    && ((item.start.x == 0.0 && item.start.y == 0.0 && item.end.x == 1.0 && item.end.y == 1.0)
                        || (item.end.x == 0.0 && item.end.y == 0.0 && item.start.x == 1.0 && item.start.y == 1.0))
            }));
        }
    }

    #[test]
    fn contour_levels_treat_decimal_vertices_as_on_level_within_roundoff() {
        let tin = Tin {
            vertices: vec![p("1", 0.0, 0.0, 0.3), p("2", 1.0, 0.0, 0.0), p("3", 0.0, 1.0, 0.6)],
            triangles: vec![Triangle { vertices: [0, 1, 2] }],
        };
        let contours = tin.contour_segments(0.1);
        assert!(contours.is_ok());
        assert!(contours.ok().is_some_and(|segments| {
            segments.iter().any(|item| {
                (item.elevation - 0.3).abs() < 1.0e-12
                    && ((item.start.x == 0.0 && item.start.y == 0.0) || (item.end.x == 0.0 && item.end.y == 0.0))
            })
        }));
    }

    #[test]
    fn contour_generation_bounds_extreme_interval_counts() {
        let tin = Tin {
            vertices: vec![p("1", 0.0, 0.0, 0.0), p("2", 1.0, 0.0, 1.0), p("3", 0.0, 1.0, 1.0)],
            triangles: vec![Triangle { vertices: [0, 1, 2] }],
        };
        assert_eq!(tin.contour_segments(1.0e-8), Err(SurveyError::InputTooLarge));
    }

    #[test]
    fn stitches_reversed_segments_and_preserves_closed_loops() {
        let segments = [
            segment([1.0, 0.0], [0.0, 0.0], 5.0),
            segment([1.0, 1.0], [1.0, 0.0], 5.0),
            segment([0.0, 1.0], [1.0, 1.0], 5.0),
            segment([0.0, 0.0], [0.0, 1.0], 5.0),
        ];
        let lines = stitch_contour_polylines(&segments, 1.0e-8, 1.0e-9).unwrap_or_default();
        assert_eq!(lines.len(), 1);
        assert!(lines.first().is_some_and(|line| line.vertices.len() == 5 && line.vertices.first() == line.vertices.last()));
    }

    #[test]
    fn branches_are_split_into_distinct_components() {
        let segments = [
            segment([0.0, 0.0], [1.0, 0.0], 5.0),
            segment([0.0, 0.0], [-1.0, 0.0], 5.0),
            segment([0.0, 0.0], [0.0, 1.0], 5.0),
        ];
        let lines = stitch_contour_polylines(&segments, 1.0e-8, 1.0e-9).unwrap_or_default();
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|line| line.vertices.len() == 2));
    }

    #[test]
    fn elevation_tolerance_keeps_nearby_contour_levels_separate() {
        let segments = [segment([0.0, 0.0], [1.0, 0.0], 5.0), segment([1.0, 0.0], [2.0, 0.0], 6.0)];
        let lines = stitch_contour_polylines(&segments, 1.0e-6, 0.01).unwrap_or_default();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn rejects_non_finite_contour_segments_and_invalid_tolerances() {
        let invalid = [TinContourSegment {
            elevation: 5.0,
            start: Vec2 { x: f64::NAN, y: 0.0 },
            end: Vec2 { x: 1.0, y: 0.0 },
        }];
        assert!(stitch_contours(&invalid, 1.0e-6).is_err());
        assert!(stitch_contours(&[], 0.0).is_err());
    }
}
