# CADCraft Survey

This workspace crate provides the first surveying analysis layer for CADCraft. It is deliberately
independent of egui and the drawing document so that the algorithms can be tested and later exposed
through the command engine, CLI, MCP and desktop/web UI.

## Initial capabilities

- Import CSV survey points in PNEZD, PENZD and XYZ column order, including quoted descriptions.
- Validate point identifiers and finite eastings, northings and elevations; cap import size and point count.
- Fit a 2D affine coordinate transformation from three non-collinear control-point pairs.
- Compute closed parcel polygon area and perimeter.
- Represent regular elevation grids, calculate average-cell cut/fill against a design elevation, and
  extract contour segments at a requested interval.
- Read six-line world files and map raster pixel-centre coordinates into drawing/map coordinates.
- Produce a text survey summary with point count and coordinate/elevation ranges.
- Convert imported points, parcel boundaries and stitched contours into CAD-neutral, layer-tagged drawing primitives (`SurveyDrawing`) while preserving point metadata and contour elevations.

## Coordinate conventions

All coordinates and measurements use the drawing's linear units. In `SurveyPoint`, `x` is
easting, `y` is northing, and `elevation` is Z. PNEZD import maps the second CSV field to
northing and the third to easting. The XYZ format has no point-number column; generated point
numbers begin at 1.

The affine transform is a general six-parameter 2D transform, not a datum transformation or a
substitute for a validated national grid operation. World-file origin values refer to the centre
of the upper-left pixel, as specified by the world-file convention.

## Limitations / next integration work

The surveying algorithms are connected to the CAD engine through `cadcraft_engine::survey`. `import_csv` parses PNEZD, PENZD or XYZ text and creates native CAD point entities; `import_drawing` creates native 3D polylines and lines for parcel, contour and TIN geometry. Survey point numbers and descriptions are attached to point entities as `CADCRAFT_SURVEY` XDATA. The adapter is callable by engine clients, and the egui front end now exposes **Insert → Survey CSV...** with PNEZD/PENZD/XYZ selection, file-open or paste input, validation preview and import feedback. A dedicated CLI command and MCP workflow are not yet implemented. The TIN triangulator
uses bounded Bowyer-Watson insertion (5,000 point cap) with normalised XY predicates; duplicate
XY coordinates are rejected. Its floating-point predicates are not an exact-arithmetic guarantee
for near-degenerate survey networks. TIN contours are emitted per triangle and can be stitched
using an endpoint tolerance, but the current stitcher follows forward endpoints only and is not a
full branch-aware topology solver. Grid volumes use the four-corner average for each complete
cell; cells with missing corner elevations are skipped. Datum/geoid transformations, georeferenced raster image entities in a drawing, and a dedicated CLI/MCP workflow remain follow-on work.
Validate all terrain and coordinate operations against known survey control data before production use.

Run the crate tests with `cargo test -p cadcraft-survey`.


## Engine integration

The engine exposes the CSV-to-drawing path without requiring UI dependencies:

\`\`\`rust
use cadcraft_engine::{survey, Session};
use cadcraft_survey::PointFormat;

let mut session = Session::new();
let csv = "Point,Northing,Easting,Elevation,Description\n101,34,12,5.6,Control\n";
let imported = survey::import_csv(&mut session, csv, PointFormat::Pnezd, true)?;
assert_eq!(imported, 1);
\`\`\`

For generated terrain or parcel geometry, build a \`SurveyDrawing\` using its helpers and pass it to
\`survey::import_drawing\`. Imported entities are placed in the active editing space on their
declared survey layers. The adapter validates the complete input before modifying the drawing.
