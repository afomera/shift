//! Per-glyph and per-layer health rules.

use serde_json::json;
use shift_font::{Font, FontProjection, Glyph, GlyphLayer, MetricKind, Point, Source};

use crate::glyph_inspect::format_difference;

use super::classify::GlyphRole;
use super::geometry::{Extent, reversed_contours};
use super::types::{Finding, FindingCode};

/// Fraction of units-per-em an outline may extend past the ascender or below
/// the descender before `outline-out-of-bounds` is reported.
pub const BOUNDS_MARGIN_RATIO: f64 = 0.2;

/// One measured advance on one checked source, collected for monospace rules.
pub struct AdvanceSample {
    pub glyph: String,
    pub advance: f64,
    pub ink: Option<Extent>,
}

/// Applies glyph rules across a fixed set of checked master sources.
pub struct GlyphRules<'a> {
    font: &'a Font,
    sources: Vec<&'a Source>,
    /// Source whose missing layers count as absent glyph outlines.
    primary_source: Option<&'a Source>,
    projections: Vec<FontProjection<'a>>,
    pub findings: Vec<Finding>,
    /// Advance samples per checked source, in `sources` order.
    pub samples: Vec<Vec<AdvanceSample>>,
}

impl<'a> GlyphRules<'a> {
    pub fn new(
        font: &'a Font,
        sources: Vec<&'a Source>,
        primary_source: Option<&'a Source>,
    ) -> Self {
        let projections = sources
            .iter()
            .map(|source| font.projection(source.location()))
            .collect();
        let samples = sources.iter().map(|_| Vec::new()).collect();
        Self {
            font,
            sources,
            primary_source,
            projections,
            findings: Vec::new(),
            samples,
        }
    }

    pub fn check_glyph(&mut self, glyph: &Glyph) {
        let role = GlyphRole::of(glyph);
        self.check_compatibility(glyph);

        let has_master_layer = self.font.master_layers(glyph).next().is_some();
        let master_count = self.font.masters().count();
        for index in 0..self.sources.len() {
            let source = self.sources[index];
            let Some(layer) = glyph.layer_for_source(source.id()) else {
                self.check_missing_layer(glyph, role, source, has_master_layer, master_count);
                continue;
            };
            self.check_layer(glyph, role, source, layer, index);
        }
    }

    fn check_missing_layer(
        &mut self,
        glyph: &Glyph,
        role: GlyphRole,
        source: &Source,
        has_master_layer: bool,
        master_count: usize,
    ) {
        let primary = self
            .primary_source
            .is_some_and(|primary| primary.id() == source.id());
        if primary && !glyph.unicodes().is_empty() && !role.space_like {
            self.push(
                Finding::new(
                    FindingCode::EmptyOutline,
                    format!("encoded glyph has no layer on {}", source.name()),
                    json!({ "reason": "noLayer", "unicodes": unicode_labels(glyph) }),
                )
                .glyph(glyph.name())
                .source(source.name()),
            );
        }
        if has_master_layer && master_count > 1 {
            self.push(
                Finding::new(
                    FindingCode::MissingMasterLayer,
                    format!(
                        "no layer on {}; the glyph is interpolated or falls back here",
                        source.name()
                    ),
                    json!({}),
                )
                .glyph(glyph.name())
                .source(source.name()),
            );
        }
    }

    fn check_layer(
        &mut self,
        glyph: &Glyph,
        role: GlyphRole,
        source: &Source,
        layer: &GlyphLayer,
        index: usize,
    ) {
        let finite = self.check_points(glyph, source, layer);
        self.check_advance(glyph, role, source, layer);
        if !finite {
            return;
        }
        self.check_direction(glyph, source, layer);

        let resolved = match self.projections[index].glyph(&glyph.id()) {
            Ok(Some(resolved)) => resolved,
            Ok(None) => return,
            Err(error) => {
                self.push(
                    Finding::new(
                        FindingCode::UnresolvedGlyph,
                        format!("glyph cannot be resolved: {error}"),
                        json!({ "error": error.to_string() }),
                    )
                    .glyph(glyph.name())
                    .source(source.name()),
                );
                return;
            }
        };
        let ink = Extent::of_outlines(
            resolved
                .contours()
                .iter()
                .map(|contour| (contour.points.as_slice(), contour.closed)),
        );

        if ink.is_none() && !glyph.unicodes().is_empty() && !role.space_like {
            self.push(
                Finding::new(
                    FindingCode::EmptyOutline,
                    "encoded glyph has no outline",
                    json!({ "reason": "noContours", "unicodes": unicode_labels(glyph) }),
                )
                .glyph(glyph.name())
                .source(source.name()),
            );
        }
        if let Some(ink) = ink {
            self.check_vertical_bounds(glyph, source, ink);
        }

        let advance = layer.width();
        let measurable = !role.mark && advance.is_finite() && advance > 0.0;
        if measurable {
            self.samples[index].push(AdvanceSample {
                glyph: glyph.name().to_string(),
                advance,
                ink,
            });
        }
    }

    /// Reports point-level defects and returns whether every coordinate is finite.
    fn check_points(&mut self, glyph: &Glyph, source: &Source, layer: &GlyphLayer) -> bool {
        let mut non_finite = Vec::new();
        for (contour_index, contour) in layer.contours_iter().enumerate() {
            let point_count = contour.points().len();
            if !contour.is_closed() {
                self.push(
                    Finding::new(
                        FindingCode::OpenContour,
                        format!(
                            "contour {contour_index} is open and will not render as a filled shape"
                        ),
                        json!({ "contour": contour_index, "pointCount": point_count }),
                    )
                    .glyph(glyph.name())
                    .source(source.name()),
                );
            }
            if point_count < 3 {
                self.push(
                    Finding::new(
                        FindingCode::DegenerateContour,
                        format!("contour {contour_index} has {point_count} points; at least 3 are needed"),
                        json!({ "contour": contour_index, "pointCount": point_count }),
                    )
                    .glyph(glyph.name())
                    .source(source.name()),
                );
            }
            for (point_index, point) in contour.points().iter().enumerate() {
                if !point.x().is_finite() || !point.y().is_finite() {
                    non_finite.push(json!({
                        "contour": contour_index,
                        "point": point_index,
                        "x": point.x().to_string(),
                        "y": point.y().to_string(),
                    }));
                }
            }
        }

        if non_finite.is_empty() {
            return true;
        }
        self.push(
            Finding::new(
                FindingCode::NonFinitePoint,
                format!("{} points have non-finite coordinates", non_finite.len()),
                json!({ "points": non_finite }),
            )
            .glyph(glyph.name())
            .source(source.name()),
        );
        false
    }

    fn check_advance(
        &mut self,
        glyph: &Glyph,
        role: GlyphRole,
        source: &Source,
        layer: &GlyphLayer,
    ) {
        let advance = layer.width();
        let reason = if !advance.is_finite() {
            "non-finite"
        } else if advance < 0.0 {
            "negative"
        } else if advance == 0.0 && !role.zero_width_allowed {
            "zero"
        } else {
            return;
        };
        self.push(
            Finding::new(
                FindingCode::InvalidAdvance,
                format!("advance {advance} is {reason}; only marks and format characters may be zero-width"),
                json!({ "advance": advance.to_string(), "reason": reason }),
            )
            .glyph(glyph.name())
            .source(source.name()),
        );
    }

    fn check_direction(&mut self, glyph: &Glyph, source: &Source, layer: &GlyphLayer) {
        let closed = layer
            .contours_iter()
            .enumerate()
            .filter(|(_, contour)| contour.is_closed())
            .collect::<Vec<_>>();
        let polygons = closed
            .iter()
            .map(|(_, contour)| contour.points())
            .collect::<Vec<&[Point]>>();
        let reversed = reversed_contours(&polygons)
            .into_iter()
            .map(|position| closed[position].0)
            .collect::<Vec<_>>();
        if reversed.is_empty() {
            return;
        }

        let all_reversed = reversed.len() == polygons.len();
        let message = if all_reversed {
            "every contour is reversed; outer contours must run counter-clockwise and counters clockwise (PostScript direction)".to_string()
        } else {
            format!(
                "contours {reversed:?} run the wrong way for their nesting; outer contours must run counter-clockwise and counters clockwise"
            )
        };
        self.push(
            Finding::new(
                FindingCode::ContourDirection,
                message,
                json!({ "contours": reversed, "allReversed": all_reversed }),
            )
            .glyph(glyph.name())
            .source(source.name()),
        );
    }

    fn check_vertical_bounds(&mut self, glyph: &Glyph, source: &Source, ink: Extent) {
        let ascender = self.font.metric_value(source.id(), MetricKind::Ascender);
        let descender = self.font.metric_value(source.id(), MetricKind::Descender);
        let (Some(ascender), Some(descender)) = (ascender, descender) else {
            return;
        };
        let margin = self.font.metrics().units_per_em * BOUNDS_MARGIN_RATIO;
        let above = ink.max_y > ascender.position + margin;
        let below = ink.min_y < descender.position - margin;
        if !above && !below {
            return;
        }

        self.push(
            Finding::new(
                FindingCode::OutlineOutOfBounds,
                format!(
                    "outline spans y {}..{}, beyond descender {} / ascender {} by more than {margin}",
                    ink.min_y, ink.max_y, descender.position, ascender.position
                ),
                json!({
                    "minY": ink.min_y,
                    "maxY": ink.max_y,
                    "ascender": ascender.position,
                    "descender": descender.position,
                    "margin": margin,
                }),
            )
            .glyph(glyph.name())
            .source(source.name()),
        );
    }

    fn check_compatibility(&mut self, glyph: &Glyph) {
        let master_layer_count = self.font.master_layers(glyph).count();
        if master_layer_count < 2 {
            return;
        }
        let projection = match self.font.glyph_projection(&glyph.id()) {
            Ok(Some(projection)) => projection,
            Ok(None) => return,
            Err(error) => {
                self.push(
                    Finding::new(
                        FindingCode::UnresolvedGlyph,
                        format!("glyph projection cannot be built: {error}"),
                        json!({ "error": error.to_string() }),
                    )
                    .glyph(glyph.name()),
                );
                return;
            }
        };
        let incompatible = incompatible_layers(
            self.font,
            glyph,
            projection.fallback(),
            self.sources.iter().copied(),
        );
        for layer in incompatible {
            self.push(
                Finding::new(
                    FindingCode::IncompatibleMasters,
                    layer.message(),
                    json!({
                        "referenceSource": layer.reference_source,
                        "differences": layer.differences,
                    }),
                )
                .glyph(glyph.name())
                .source(layer.source.name()),
            );
        }
    }

    fn push(&mut self, finding: Finding) {
        self.findings.push(finding);
    }
}

pub fn unicode_labels(glyph: &Glyph) -> Vec<String> {
    glyph
        .unicodes()
        .iter()
        .map(|unicode| format!("U+{unicode:04X}"))
        .collect()
}

/// One master layer whose structure cannot interpolate with its glyph's
/// reference layer, so variation silently excludes it.
pub struct IncompatibleLayer<'a> {
    pub source: &'a Source,
    /// Name of the source owning the reference layer.
    pub reference_source: String,
    /// Path, node, anchor, and component differences, reference first.
    pub differences: Vec<String>,
}

impl IncompatibleLayer<'_> {
    /// Describes the incompatibility the way `check` reports it.
    pub fn message(&self) -> String {
        format!(
            "layer cannot interpolate with {}: {}",
            self.reference_source,
            self.differences.join("; ")
        )
    }
}

/// Lists `glyph`'s layers on `sources` that cannot interpolate with `reference`.
///
/// `reference` is the glyph projection's structural fallback, the same layer
/// interpolation uses for topology. Sources without a layer, and the
/// reference layer itself, are skipped.
pub fn incompatible_layers<'a>(
    font: &Font,
    glyph: &Glyph,
    reference: &GlyphLayer,
    sources: impl IntoIterator<Item = &'a Source>,
) -> Vec<IncompatibleLayer<'a>> {
    let reference_source = font.source(&reference.source_id()).map_or_else(
        || reference.source_id().to_string(),
        |source| source.name().to_string(),
    );
    sources
        .into_iter()
        .filter_map(|source| {
            let layer = glyph.layer_for_source(source.id())?;
            if layer.id() == reference.id() {
                return None;
            }
            let layer_match = reference.match_with(layer);
            if layer_match.is_complete() {
                return None;
            }
            Some(IncompatibleLayer {
                source,
                reference_source: reference_source.clone(),
                differences: layer_match
                    .differences()
                    .iter()
                    .map(format_difference)
                    .collect(),
            })
        })
        .collect()
}
