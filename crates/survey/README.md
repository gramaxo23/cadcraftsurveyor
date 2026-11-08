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

## Coordinate conventions

All coordinates and measurements use the drawing's linear units. In `SurveyPoint`, `x` is
easting, `y` is northing, and `elevation` is Z. PNEZD import maps the second CSV field to
northing and the third to easting. The XYZ format has no point-number column; generated point
numbers begin at 1.

The affine transform is a general six-parameter 2D transform, not a datum transformation or a
substitute for a validated national grid operation. World-file origin values refer to the centre
of the upper-left pixel, as specified by the world-file convention.

## Limitations / next integration work

This is the algorithm and data foundation, not yet a finished surveying UI. Contours are emitted
as independent two-point segments and are not stitched into polylines. Grid volumes use the
four-corner average for each complete cell; cells with missing corner elevations are skipped.
The first release does not yet triangulate scattered points into a TIN, perform datum/geoid
transformations, georeference raster image entities in a drawing, or create native CAD entities
from imported points. Those are follow-on integration tasks and must be tested against known
survey control data before production use.

Run the crate tests with `cargo test -p cadcraft-survey`.
