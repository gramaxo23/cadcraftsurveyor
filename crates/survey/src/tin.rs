//! Delaunay TIN construction and terrain queries.
//!
//! Coordinates are projected drawing units. Input is bounded to keep worst-case work predictable.
use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};
use crate::{SurveyError, SurveyPoint, Vec2};

const MAX_TIN_POINTS: usize = 5_000;
const PREDICATE_EPS: f64 = 1.0e-12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Triangle { pub vertices: [usize; 3] }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tin { pub vertices: Vec<SurveyPoint>, pub triangles: Vec<Triangle> }

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TinContourSegment { pub elevation: f64, pub start: Vec2, pub end: Vec2 }

fn orient(a: Vec2, b: Vec2, c: Vec2) -> f64 {
    (b.x-a.x)*(c.y-a.y)-(b.y-a.y)*(c.x-a.x)
}

fn in_circumcircle(a: Vec2, b: Vec2, c: Vec2, p: Vec2) -> bool {
    let ax=a.x-p.x; let ay=a.y-p.y; let bx=b.x-p.x; let by=b.y-p.y; let cx=c.x-p.x; let cy=c.y-p.y;
    let det=(ax*ax+ay*ay)*(bx*cy-by*cx)-(bx*bx+by*by)*(ax*cy-ay*cx)+(cx*cx+cy*cy)*(ax*by-ay*bx);
    let direction=orient(a,b,c);
    if direction>0.0 { det > -PREDICATE_EPS } else { det < PREDICATE_EPS }
}

fn normalise(points: &[SurveyPoint]) -> Result<Vec<Vec2>,SurveyError> {
    let mut min_x=f64::INFINITY; let mut min_y=f64::INFINITY;
    let mut max_x=f64::NEG_INFINITY; let mut max_y=f64::NEG_INFINITY;
    for p in points {
        p.validate()?;
        min_x=min_x.min(p.x); min_y=min_y.min(p.y); max_x=max_x.max(p.x); max_y=max_y.max(p.y);
    }
    let span=(max_x-min_x).max(max_y-min_y);
    if !span.is_finite() || span<=f64::EPSILON { return Err(SurveyError::DegenerateControls); }
    Ok(points.iter().map(|p| Vec2{x:(p.x-min_x)/span,y:(p.y-min_y)/span}).collect())
}

impl Tin {
    /// Builds a 2D Delaunay triangulation using bounded Bowyer-Watson insertion.
    /// Duplicate XY positions are rejected rather than silently dropping survey records.
    pub fn from_points(points: &[SurveyPoint]) -> Result<Self,SurveyError> {
        if points.len()<3 { return Err(SurveyError::NotEnoughPoints(3)); }
        if points.len()>MAX_TIN_POINTS { return Err(SurveyError::InputTooLarge); }
        let mut xy=normalise(points)?;
        for i in 0..xy.len() {
            for j in 0..i {
                let a=xy.get(i).ok_or(SurveyError::DegenerateControls)?;
                let b=xy.get(j).ok_or(SurveyError::DegenerateControls)?;
                if (a.x-b.x).hypot(a.y-b.y)<=PREDICATE_EPS {
                    return Err(SurveyError::InvalidValue("TIN contains duplicate XY coordinates".into()));
                }
            }
        }
        let n=xy.len();
        xy.extend([Vec2{x:-32.0,y:-2.0},Vec2{x:32.0,y:-2.0},Vec2{x:0.5,y:32.0}]);
        let mut triangles=vec![[n,n+1,n+2]];
        let mut order:Vec<usize>=(0..n).collect();
        order.sort_by(|a,b| match (xy.get(*a),xy.get(*b)) {
            (Some(a),Some(b))=>a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)),
            _=>a.cmp(b),
        });
        for point_index in order {
            let p=*xy.get(point_index).ok_or(SurveyError::DegenerateControls)?;
            let mut bad=Vec::new();
            for (index,t) in triangles.iter().enumerate() {
                let a=*xy.get(t[0]).ok_or(SurveyError::DegenerateControls)?;
                let b=*xy.get(t[1]).ok_or(SurveyError::DegenerateControls)?;
                let c=*xy.get(t[2]).ok_or(SurveyError::DegenerateControls)?;
                if in_circumcircle(a,b,c,p) { bad.push(index); }
            }
            if bad.is_empty() { return Err(SurveyError::DegenerateControls); }
            let mut boundary:BTreeMap<(usize,usize),(usize,usize,usize)>=BTreeMap::new();
            for index in bad.iter().rev() {
                let t=triangles.remove(*index);
                for (a,b) in [(t[0],t[1]),(t[1],t[2]),(t[2],t[0])] {
                    let key=if a<b {(a,b)} else {(b,a)};
                    let entry=boundary.entry(key).or_insert((a,b,0));
                    entry.2+=1;
                }
            }
            for (_, (a,b,count)) in boundary {
                if count!=1 { continue; }
                let direction=orient(*xy.get(a).ok_or(SurveyError::DegenerateControls)?,*xy.get(b).ok_or(SurveyError::DegenerateControls)?,p);
                if direction.abs()<=PREDICATE_EPS { continue; }
                triangles.push(if direction>0.0 {[a,b,point_index]} else {[b,a,point_index]});
            }
        }
        let mut output=Vec::new();
        for t in triangles {
            if t.iter().any(|index|*index>=n) { continue; }
            let a=*xy.get(t[0]).ok_or(SurveyError::DegenerateControls)?;
            let b=*xy.get(t[1]).ok_or(SurveyError::DegenerateControls)?;
            let c=*xy.get(t[2]).ok_or(SurveyError::DegenerateControls)?;
            if orient(a,b,c).abs()>PREDICATE_EPS { output.push(Triangle{vertices:t}); }
        }
        if output.is_empty() { return Err(SurveyError::DegenerateControls); }
        Ok(Self{vertices:points.to_vec(),triangles:output})
    }

    /// Returns interpolated elevation if query lies inside a TIN triangle.
    pub fn elevation_at(&self, query:Vec2)->Result<Option<f64>,SurveyError> {
        if !query.x.is_finite() || !query.y.is_finite() {
            return Err(SurveyError::InvalidValue("TIN query coordinate is not finite".into()));
        }
        for t in &self.triangles {
            let a=self.vertices.get(t.vertices[0]).ok_or(SurveyError::DegenerateControls)?;
            let b=self.vertices.get(t.vertices[1]).ok_or(SurveyError::DegenerateControls)?;
            let c=self.vertices.get(t.vertices[2]).ok_or(SurveyError::DegenerateControls)?;
            let pa=Vec2{x:a.x,y:a.y}; let pb=Vec2{x:b.x,y:b.y}; let pc=Vec2{x:c.x,y:c.y};
            let area=orient(pa,pb,pc);
            if area.abs()<=f64::EPSILON { continue; }
            let wa=orient(pb,pc,query)/area; let wb=orient(pc,pa,query)/area; let wc=1.0-wa-wb;
            if wa>=-PREDICATE_EPS && wb>=-PREDICATE_EPS && wc>=-PREDICATE_EPS {
                let z=wa*a.elevation+wb*b.elevation+wc*c.elevation;
                if z.is_finite() { return Ok(Some(z)); }
                return Err(SurveyError::InvalidValue("TIN interpolation overflowed".into()));
            }
        }
        Ok(None)
    }

    /// Emits contour segments per triangle. Stitch adjacent segments for continuous polylines.
    pub fn contour_segments(&self,interval:f64)->Result<Vec<TinContourSegment>,SurveyError> {
        if !interval.is_finite() || interval<=0.0 {
            return Err(SurveyError::InvalidValue("contour interval must be positive".into()));
        }
        let mut result=Vec::new();
        for t in &self.triangles {
            let v=[
                self.vertices.get(t.vertices[0]).ok_or(SurveyError::DegenerateControls)?,
                self.vertices.get(t.vertices[1]).ok_or(SurveyError::DegenerateControls)?,
                self.vertices.get(t.vertices[2]).ok_or(SurveyError::DegenerateControls)?,
            ];
            let min=v.iter().map(|p|p.elevation).fold(f64::INFINITY,f64::min);
            let max=v.iter().map(|p|p.elevation).fold(f64::NEG_INFINITY,f64::max);
            let first=(min/interval).ceil()*interval;
            let raw_count=((max-first)/interval).floor();
            if !raw_count.is_finite() || raw_count>100_000.0 { return Err(SurveyError::InputTooLarge); }
            let count=if raw_count<0.0 {0} else {raw_count as usize+1};
            for k in 0..count {
                let level=first+k as f64*interval;
                let mut hits=Vec::new();
                for (i,j) in [(0usize,1usize),(1,2),(2,0)] {
                    let a=v[i]; let b=v[j];
                    if (a.elevation<level && b.elevation>level)||(a.elevation>level && b.elevation<level) {
                        let t=(level-a.elevation)/(b.elevation-a.elevation);
                        hits.push(Vec2{x:a.x+(b.x-a.x)*t,y:a.y+(b.y-a.y)*t});
                    }
                }
                if let (Some(start),Some(end))=(hits.first(),hits.get(1)) {
                    if hits.len()==2 { result.push(TinContourSegment{elevation:level,start:*start,end:*end}); }
                }
            }
        }
        Ok(result)
    }
}

/// Joins contour segments whose endpoints match within tolerance and whose elevations match.
pub fn stitch_contours(segments:&[TinContourSegment],tolerance:f64)->Result<Vec<Vec<Vec2>>,SurveyError> {
    if !tolerance.is_finite() || tolerance<=0.0 {
        return Err(SurveyError::InvalidValue("stitch tolerance must be positive".into()));
    }
    let mut unused=vec![true;segments.len()];
    let mut lines=Vec::new();
    for seed in 0..segments.len() {
        if !unused.get(seed).copied().unwrap_or(false) { continue; }
        let first=segments.get(seed).ok_or(SurveyError::DegenerateControls)?;
        let mut line=vec![first.start,first.end];
        if let Some(slot)=unused.get_mut(seed) {*slot=false;}
        loop {
            let Some(last)=line.last().copied() else {break;};
            let next=segments.iter().enumerate().find(|(index,s)| {
                unused.get(*index).copied().unwrap_or(false)
                    && (s.start.x-last.x).hypot(s.start.y-last.y)<=tolerance
                    && (s.elevation-first.elevation).abs()<=tolerance
            });
            if let Some((index,s))=next {
                line.push(s.end);
                if let Some(slot)=unused.get_mut(index) {*slot=false;}
            } else {break;}
        }
        lines.push(line);
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn p(n:&str,x:f64,y:f64,z:f64)->SurveyPoint {
        SurveyPoint{number:n.into(),x,y,elevation:z,description:String::new()}
    }
    #[test]
    fn triangulates_and_interpolates() {
        let points=[p("1",0.0,0.0,0.0),p("2",10.0,0.0,10.0),p("3",0.0,10.0,20.0),p("4",10.0,10.0,30.0)];
        let tin=Tin::from_points(&points);
        assert!(tin.is_ok());
        let model=tin.unwrap_or_else(|_|Tin{vertices:Vec::new(),triangles:Vec::new()});
        let z=model.elevation_at(Vec2{x:2.0,y:2.0});
        assert!(z.ok().flatten().is_some_and(|value|(value-8.0).abs()<1.0e-8));
    }
    #[test]
    fn rejects_duplicate_and_collinear_xy() {
        assert!(Tin::from_points(&[p("1",0.0,0.0,0.0),p("2",0.0,0.0,1.0),p("3",1.0,0.0,2.0)]).is_err());
        assert!(Tin::from_points(&[p("1",0.0,0.0,0.0),p("2",1.0,1.0,1.0),p("3",2.0,2.0,2.0)]).is_err());
    }
    #[test]
    fn joins_contour_segments() {
        let segments=[
            TinContourSegment{elevation:5.0,start:Vec2{x:0.0,y:0.0},end:Vec2{x:1.0,y:0.0}},
            TinContourSegment{elevation:5.0,start:Vec2{x:1.0,y:0.0},end:Vec2{x:2.0,y:0.0}},
        ];
        let lines=stitch_contours(&segments,1.0e-6);
        assert_eq!(lines.map(|v|v.len()),Ok(1));
        assert_eq!(lines.ok().and_then(|v|v.first().map(Vec::len)),Some(3));
    }
}
