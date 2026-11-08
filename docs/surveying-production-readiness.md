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

The surveying branch currently provides CSV PNEZD/PENZD/XYZ point import, basic affine fitting from exactly three controls, parcel area/perimeter, a regular-grid cut/fill calculation, world-file coordinate mapping, a bounded Bowyer-Watson TIN and contour segments, plus native CAD entity import.

The following are not yet sufficient for production surveying:

- No explicit coordinate reference system, datum, projection, geoid model, coordinate epoch, grid-to-ground settings or unit metadata is carried through survey data.
- The affine transform is a three-control-point exact fit; it does not provide a redundant least-squares adjustment, residuals, weights, covariance or outlier diagnostics.
- TIN predicates use floating-point tolerances and the implementation is capped at 5,000 points. Breakline constraints, surface boundaries and holes are not modelled.
- Contour stitching is a simple endpoint walk and is not topology-aware at branches or ambiguous junctions.
- Grid cut/fill uses four-corner cell averaging, skips incomplete cells and does not clip to a project boundary. It is not a validated TIN-to-TIN earthworks calculation.
- CSV import supports a deliberately narrow set of formats; no instrument-specific export, field-book workflow or comprehensive import audit trail is provided.
- World-file parsing is not a complete georeferenced raster workflow.
- No company control dataset or independent reference calculations have been supplied or run.

## Release checklist

- [ ] CI passes formatting, lint, unit tests, application build and supported platform builds on the exact release commit.
- [ ] Add deterministic fixtures with documented units, CRS/datum, control coordinates and independently verified expected outputs.
- [ ] Validate all five workstreams above with a qualified surveyor using company field/control data.
- [ ] Add explicit units and coordinate-system metadata to projects, imports, exports and reports; reject ambiguous inputs rather than guessing.
- [ ] Add redundant control adjustment with residual reporting before setting-out use.
- [ ] Upgrade TIN robustness and implement breaklines, boundaries and holes before relying on surfaces or derived contours.
- [ ] Implement independently verified surface-to-surface volumes and project-boundary clipping before quantity use.
- [ ] Verify DXF/DWG round trips preserve survey layers, point metadata and 3D geometry in the target CAD applications.
- [ ] Produce auditable reports recording source files, import options, coordinate settings, calculation method, software version and warnings.
- [ ] Run a supervised pilot; require sign-off before operational use.

## Current release decision

**Not approved for production surveying or unsupervised employee use.** The feature branch is an engineering foundation and should remain a supervised development pilot until the checklist has evidence. In particular, do not use its current parcel calculations for legal cadastral certification, its transformations for construction set-out, or its current volume/surface outputs for payment quantities.
