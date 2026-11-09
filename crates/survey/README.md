# CADCraft Survey

`cadcraft-survey` is the surveying and civil-site analysis library in the CADCraft workspace.
It is intentionally independent of egui and the drawing database so calculation logic can be
tested separately and reused by desktop, CLI, MCP and other future clients.

> **Release status:** engineering prototype / supervised evaluation. This crate is not certified
> for legal cadastral determinations, unsupervised construction set-out or contractual quantity
> measurement. See [the production-readiness gate](../../docs/surveying-production-readiness.md).

## Capability reference

| Module / area | API and functionality |
|---|---|
| Point data | `import_points_csv`, `PointFormat`, `SurveyPoint`, `export_pnezd_csv`, `group_points_by_code` |
| COGO | `cogo`: direct/inverse calculations, azimuths, infinite-line intersection and traverse closure diagnostics |
| Traverse adjustment | `cogo::adjustment`: Bowditch/compass-rule adjustment |
| Units and CRS metadata | `civil::LinearUnit`, `SurveyCrsMetadata`, axis order and coordinate-use metadata, explicit scale-factor helper |
| Control fit | Existing three-control affine fit plus redundant unweighted 2D similarity fit with residuals, RMS and maximum residual |
| Parcels | `parcel_area`, `parcel_perimeter` |
| Terrain | `Tin`: bounded triangulation, scale-independent elevation queries, deduplicated contour segments, branch-aware contour polylines and regular-grid cut/fill |
| Alignment | `HorizontalAlignment`: stationing, offsets and station equations |
| Profiles | `VerticalProfile`: linear elevation interpolation and segment grade |
| Cross sections | `CrossSection`: cut/fill area to a datum; average-end-area volume between sections |
| Setting-out | `stakeout_deltas`: horizontal distance, whole-circle azimuth and optional elevation difference |
| CAD-neutral output | `SurveyDrawing` and survey drawing entities for points, parcel geometry, per-level contour polylines and unique TIN edges |

## Coordinate and measurement conventions

- `SurveyPoint.x` is easting, `y` is northing, and `elevation` is Z.
- PNEZD input order is point number, northing, easting, elevation, description.
- PENZD input order is point number, easting, northing, elevation, description.
- XYZ input order is easting, northing, elevation, with generated point numbers.
- COGO azimuths are whole-circle bearings measured clockwise from grid north.
- Alignment positive offset is to the right of increasing station.
- Linear, area and volume conversions distinguish the international foot (0.3048 m) from the US
  survey foot (1200/3937 m).
- Coordinates are not automatically transformed. All operands in a calculation must already use
  compatible coordinate systems and units.

`SurveyCrsMetadata` stores authority/code, datum, projection, geoid model, coordinate epoch,
horizontal and vertical units, axis order, grid/ground/local use and an optional combined scale
factor. Validation checks metadata consistency; it does **not** resolve EPSG definitions or perform
datum, projection, geoid or epoch transformations. A scale-factor helper does not decide the
grid-to-ground convention for the operator.

## Desktop point import

The desktop application exposes **Insert → Survey CSV...**. It supports PNEZD, PENZD and XYZ,
file selection or pasted data, an optional header, a parsed-point preview and validation feedback.
The dialog can optionally generate a TIN edge network and stitched contour polylines from the same
validated point set. These are imported as native CAD lines and 3D polylines on dedicated survey
layers. The engine adapter `cadcraft_engine::survey::import_csv` remains available for point-only
imports; `import_drawing` converts a complete `SurveyDrawing` into native points, 3D polylines
and lines. Survey point numbers and descriptions are stored on point entities as
`CADCRAFT_SURVEY` XDATA.

Example engine integration:

```rust
use cadcraft_engine::{survey, Session};
use cadcraft_survey::PointFormat;

fn import_control_points() -> Result<usize, Box<dyn std::error::Error>> {
    let mut session = Session::new();
    let csv = "Point,Northing,Easting,Elevation,Description\n101,5000,2000,125.5,CONTROL\n";
    let count = survey::import_csv(&mut session, csv, PointFormat::Pnezd, true)?;
    Ok(count)
}
```

## Calculation examples

```rust
use cadcraft_survey::civil::LinearUnit;

fn convert_distance() -> Result<f64, cadcraft_survey::SurveyError> {
    LinearUnit::UsSurveyFoot.convert(100.0, LinearUnit::Metre)
}
```

## Known limitations

- TIN construction is bounded to 5,000 points, uses floating-point predicates and rejects duplicate
  XY coordinates. Breakline constraints, surface boundaries and holes are not implemented.
- Contour stitching builds an endpoint graph, reverses segments as needed, preserves closed loops and splits paths at branch nodes. It is still tolerance-based and is not a robust-predicate terrain topology solver.
- Grid cut/fill uses four-corner cell averaging, skips incomplete cells and does not clip to
  project boundaries. It is not a TIN-to-TIN quantity engine.
- Similarity fitting is unweighted and 2D; it does not provide covariance, a stochastic observation
  model, vertical adjustment or robust outlier detection.
- The affine transformation is a three-control exact fit, not a datum transformation.
- Raw field observations, field-book reduction, instrument-specific formats and full import
  provenance/audit reports are not implemented.
- CRS metadata is not yet persisted and enforced through every project, import, export and report.
- World-file coordinate mapping is not a complete georeferenced raster workflow.
- No independent company control datasets or third-party reference calculations have been run.

## Tests and development

Run the focused crate tests:

```sh
cargo test -p cadcraft-survey
```

Run formatting and the wider relevant package tests:

```sh
cargo fmt --all -- --check
cargo test -p cadcraft-survey -p cadcraft-engine -p cadcraft-ui-egui
```

Before using results operationally, compare every relevant calculation against independent,
documented reference data and obtain sign-off from a qualified surveyor. See
[docs/surveying-production-readiness.md](../../docs/surveying-production-readiness.md) for the
release checklist.
