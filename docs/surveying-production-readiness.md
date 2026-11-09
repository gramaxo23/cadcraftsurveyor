# Surveying production-readiness gate

This checklist defines the minimum release evidence for using CADCraft on employee surveying work. A green Rust build is necessary, but it does not validate survey accuracy or make a cadastral/legal determination reliable.

## Supported workstreams and release gates

| Workstream | Required capability | Acceptance evidence before employee release |
|---|---|---|
| Topographic surveys | Import field observations; preserve point IDs, codes, elevations and units; build/edit a terrain surface; query elevations; generate contours and deliverable linework | Compare imported points, breaklines, spot levels, surface elevations and contours against an independently computed reference dataset. Verify boundary behavior, duplicate points, breaklines, voids and contour closure. |
| Boundary / cadastral | Parcel geometry, bearings/distances, area and perimeter, coordinate reference metadata, control adjustment and traceable reports | Test closure, orientation, units, coordinate epoch/datum, legal area conventions and adjustment residuals against licensed/authoritative control examples. Require qualified surveyor sign-off; do not treat CAD geometry as a legal boundary determination. |
| Construction setting-out | Design-to-ground coordinate transformations, inverse calculations, stakeout coordinates, elevations, offsets and point export | Validate independent control points, transformation residuals, grid-to-ground scale/rotation, units, axis order, rounding, instrument-compatible exports and field check shots. No setting-out release until a surveyor approves a signed test pack. |
| Earthworks / volumes | Existing/design surfaces, cut/fill, comparison, volume report and stated method | Cross-check benchmark surfaces and volumes against an independent survey/civil package. Test grid resolution, triangulation, voids, boundary clipping, datum, units and mass-haul assumptions. Current grid averaging is not a substitute for a validated surface-to-surface method. |
| Surface creation | TIN creation, breaklines, boundaries, holes, contours and native CAD entities | Validate triangulation against reference surfaces, including near-collinear/cocircular points, large coordinate magnitudes, duplicate XY, breaklines, holes and disconnected regions. Robust predicates and topology-aware contour stitching are release requirements. |

## Current implementation limits

The surveying branch currently provides CSV PNEZD/PENZD/XYZ point import, PNEZD export, description-code grouping, COGO direct/inverse calculations, infinite-line intersection, traverse closure diagnostics, Bowditch adjustment, unit conversion (including distinct international and US survey feet), CRS metadata validation, a 2D least-squares similarity fit with residual summaries, a three-control affine fit, validated parcel boundary creation/editing, area/perimeter and edge schedules, horizontal alignment station/offset calculations, linear vertical profiles, cross-section cut/fill areas and average-end-area volumes, stakeout deltas, a regular-grid cut/fill calculation, world-file coordinate mapping, a bounded Bowyer-Watson TIN, scale-independent elevation queries, deduplicated contour segments and branch-aware stitched contour polylines. The desktop CSV dialog can optionally generate native TIN edge lines and contour polylines from imported points.

The following are not yet sufficient for production surveying:\n\n- Parcel editing currently provides validated library operations and ordered-point boundary creation in the desktop import dialog; interactive grips/edit-in-place, legal boundary rules and parcel-to-parcel topology are not yet implemented.

- CRS/unit metadata structures and validation now exist, but metadata is not yet persisted end-to-end in drawing/project files or enforced by every import/export path. Datum, projection, geoid and epoch transformations are not implemented. A scale-factor helper is available, but grid/ground convention and field workflow still require explicit operator configuration.
- A redundant unweighted 2D similarity fit now reports residuals, RMS and maximum residual. It is not a weighted geodetic network adjustment and does not provide covariance, stochastic models, vertical adjustment, or robust outlier detection. The separate affine transform remains a three-control exact fit.
- TIN predicates use floating-point tolerances and the implementation is capped at 5,000 points. Breakline constraints, surface boundaries and holes are not modelled.
- Contour generation caps candidate triangle-levels at 1,000,000, rejects intervals below elevation floating-point resolution and deduplicates shared horizontal edges.
- Contour stitching caps input at 1,000,000 segments and builds a tolerance-based endpoint graph with separate XY/elevation tolerances. It supports reversed segments, closed loops and branch splitting, but does not replace robust-predicate topology or breakline/boundary enforcement.
- Grid cut/fill uses four-corner cell averaging, skips incomplete cells and does not clip to a project boundary. It is not a validated TIN-to-TIN earthworks calculation.
- CSV import supports a deliberately narrow set of formats; PNEZD export and description-code grouping are available, but instrument-specific export, field-book reduction, raw observation storage and comprehensive import audit trails are not provided.
- World-file parsing is not a complete georeferenced raster workflow.
- No company control dataset or independent reference calculations have been supplied or run.

## Release checklist

- [ ] CI passes formatting, lint, unit tests, application build and supported platform builds on the exact release commit.
- [ ] Add deterministic fixtures with documented units, CRS/datum, control coordinates and independently verified expected outputs.
- [ ] Validate all five workstreams above with a qualified surveyor using company field/control data.
- [ ] Persist and enforce explicit units and coordinate-system metadata across projects, imports, exports and reports; reject ambiguous inputs rather than guessing.
- [ ] Upgrade the unweighted similarity fit to a validated weighted 2D/3D network adjustment with covariance, residual tests and outlier diagnostics before setting-out use.
- [ ] Upgrade TIN robustness and implement breaklines, boundaries and holes before relying on surfaces or derived contours.
- [ ] Implement independently verified TIN-to-TIN surface volumes, project-boundary clipping and mass-haul analysis before quantity use.
- [ ] Verify DXF/DWG round trips preserve survey layers, point metadata and 3D geometry in the target CAD applications.
- [ ] Produce auditable reports recording source files, import options, coordinate settings, calculation method, software version and warnings.
- [ ] Run a supervised pilot; require sign-off before operational use.

## Current release decision

**Not approved for production surveying or unsupervised employee use.** The feature branch is an engineering foundation and should remain a supervised development pilot until the checklist has evidence. In particular, do not use its current parcel calculations for legal cadastral certification, its transformations for construction set-out, or its current volume/surface outputs for payment quantities.
