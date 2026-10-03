//! Static (single-location) fonts derived from a variable font.
//!
//! A static font is an ordinary single-master [`Font`]: every glyph's layer
//! is resolved at one design location through Shift's glyph projections
//! (exact masters where they land, compatible interpolation between them),
//! source metrics are interpolated with the same source weights, and axes,
//! mappings, extra sources, and named instances are dropped. Components stay
//! components; their base glyphs are resolved at the same location, so the
//! compiled outlines equal the location's flattened glyphs.

use miette::{IntoDiagnostic, Result, WrapErr, bail, miette};
use shift_font::{
    AxisRole, DesignLocation, ExternalLocation, Font, GlyphId, GlyphLayer, GlyphProjectionSet,
    LibValue, NamedInstance,
};

use crate::check::incompatible_layers;
use crate::kerning::format_number;

/// UFO fontinfo key the compiler reads for `OS/2.usWeightClass`.
const WEIGHT_CLASS_KEY: &str = "openTypeOS2WeightClass";
/// UFO fontinfo key the compiler reads for name ID 6.
const POSTSCRIPT_NAME_KEY: &str = "postscriptFontName";

/// One static font to derive: a style name at a user-space location.
#[derive(Clone, Debug, PartialEq)]
pub struct StaticTarget {
    /// Style (subfamily) name of the static font.
    pub style_name: String,
    /// Complete external/user-space location; mapped to design space once.
    pub location: ExternalLocation,
    /// Published PostScript name, when the instance authored one.
    pub postscript_name: Option<String>,
}

impl StaticTarget {
    /// Targets a named instance, using its name and PostScript name.
    pub fn from_instance(instance: &NamedInstance) -> Self {
        Self {
            style_name: instance.name().to_string(),
            location: instance.location().clone(),
            postscript_name: instance.postscript_name().map(str::to_string),
        }
    }

    /// Targets a raw location, naming the style from its coordinates.
    ///
    /// The generated style name lists each external axis as tag and value,
    /// such as `wght700`, separated by spaces when there are several axes.
    pub fn from_location(font: &Font, location: ExternalLocation) -> Self {
        let style_name = font
            .axes()
            .iter()
            .filter(|axis| axis.role() == AxisRole::External)
            .map(|axis| {
                let value = location.get(&axis.id()).unwrap_or(axis.default());
                format!("{}{}", axis.tag(), format_number(value))
            })
            .collect::<Vec<_>>()
            .join(" ");
        Self {
            style_name,
            location,
            postscript_name: None,
        }
    }

    /// Returns the PostScript-safe file stem `<Family>-<Style>`.
    ///
    /// An authored PostScript name wins. Otherwise the family and style names
    /// keep only printable ASCII outside `[](){}<>/%` and spaces, the
    /// characters PostScript names allow.
    pub fn file_stem(&self, font: &Font) -> String {
        if let Some(name) = &self.postscript_name {
            return name.clone();
        }
        let family = font
            .metadata()
            .family_name
            .as_deref()
            .map(postscript_safe)
            .filter(|family| !family.is_empty())
            .unwrap_or_else(|| "Untitled".to_string());
        let style = postscript_safe(&self.style_name);
        if style.is_empty() {
            return family;
        }
        format!("{family}-{style}")
    }

    /// Returns `OS/2.usWeightClass` for the target's `wght` coordinate, if any.
    fn weight_class(&self, font: &Font) -> Option<i64> {
        let axis = font
            .axes()
            .iter()
            .find(|axis| axis.tag() == "wght" && axis.role() == AxisRole::External)?;
        let weight = self.location.get(&axis.id()).unwrap_or(axis.default());
        Some(weight.round().clamp(1.0, 1000.0) as i64)
    }
}

fn postscript_safe(name: &str) -> String {
    name.chars()
        .filter(|character| character.is_ascii_graphic() && !"[](){}<>/%".contains(*character))
        .collect()
}

/// Builds a single-master font equal to `font` at `target`'s location.
///
/// The default source survives as the only master, renamed to the style and
/// carrying the location's interpolated metrics. The style name becomes the
/// font's style name, the `wght` coordinate becomes `OS/2.usWeightClass`, and
/// an authored PostScript name becomes name ID 6. Kerning, features, and
/// font-level data are kept as authored.
///
/// # Errors
///
/// Returns an error when the location cannot be mapped to design space, the
/// location is not exactly a master and some glyph has master layers that
/// cannot interpolate (each is named with its differences), a glyph cannot be
/// resolved, or the font has no default source.
pub fn static_font(font: &Font, target: &StaticTarget) -> Result<Font> {
    let design = font
        .mapped_location(&target.location)
        .into_diagnostic()
        .wrap_err("failed to map the location into design space")?;
    let default_id = font
        .default_source_id()
        .ok_or_else(|| miette!("the font has no default source"))?;
    let glyph_ids = font.glyphs().map(|glyph| glyph.id()).collect::<Vec<_>>();
    let projections = font
        .glyph_projection_set(&glyph_ids)
        .into_diagnostic()
        .wrap_err("failed to prepare glyph projections")?;

    ensure_compatible(font, &projections, &design, target)?;
    let layers = resolve_layers(font, &projections, &glyph_ids, &design)?;
    let metrics = font
        .source_metric_interpolation()
        .map(|interpolation| interpolation.resolve(&design, font.axes()))
        .transpose()
        .into_diagnostic()
        .wrap_err("failed to interpolate source metrics")?;

    let mut output = font.clone();
    output.set_named_instances(Vec::new()).into_diagnostic()?;
    output.set_axis_mappings(Vec::new()).into_diagnostic()?;
    let layer_ids = output
        .glyphs()
        .flat_map(|glyph| glyph.layers().keys().cloned().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    for layer_id in layer_ids {
        output.remove_glyph_layer(layer_id).into_diagnostic()?;
    }
    let other_sources = output
        .sources()
        .iter()
        .map(|source| source.id())
        .filter(|source_id| *source_id != default_id)
        .collect::<Vec<_>>();
    for source_id in other_sources {
        output.remove_source(source_id);
    }
    let axis_ids = output
        .axes()
        .iter()
        .map(|axis| axis.id())
        .collect::<Vec<_>>();
    for axis_id in axis_ids {
        output.remove_axis(axis_id).into_diagnostic()?;
    }

    let mut source = output
        .default_source()
        .cloned()
        .ok_or_else(|| miette!("the font has no default source"))?;
    source.set_name(target.style_name.clone());
    if let Some(metrics) = metrics {
        source.set_metric_values(metrics.metric_values().clone());
        let technical = [
            (metrics.italic_angle(), source.italic_angle()),
            (metrics.line_gap(), source.line_gap()),
            (metrics.underline_position(), source.underline_position()),
            (metrics.underline_thickness(), source.underline_thickness()),
        ]
        .map(|(resolved, authored)| resolved.or(authored));
        source.set_italic_angle(technical[0]);
        source.set_line_gap(technical[1]);
        source.set_underline_position(technical[2]);
        source.set_underline_thickness(technical[3]);
    }
    output.replace_source(source).into_diagnostic()?;

    for (glyph_id, layer) in layers {
        let layer = layer.clone_with_identity(layer.id(), default_id.clone());
        output
            .insert_glyph_layer(glyph_id, layer)
            .into_diagnostic()?;
    }

    output.metadata_mut().style_name = Some(target.style_name.clone());
    let fontinfo = output.fontinfo_remainder_mut();
    match target.weight_class(font) {
        Some(weight) => fontinfo.set(WEIGHT_CLASS_KEY.to_string(), LibValue::Integer(weight)),
        None => {
            fontinfo.remove(WEIGHT_CLASS_KEY);
        }
    }
    // A family-level PostScript name belongs to the default style, not this one.
    match &target.postscript_name {
        Some(name) => fontinfo.set(
            POSTSCRIPT_NAME_KEY.to_string(),
            LibValue::String(name.clone()),
        ),
        None => {
            fontinfo.remove(POSTSCRIPT_NAME_KEY);
        }
    }

    Ok(output)
}

/// Refuses locations between masters when some glyph cannot interpolate.
///
/// Interpolation silently drops incompatible master layers, so a static font
/// between masters would quietly misdraw those glyphs. Exactly at a master,
/// each glyph uses that master's own drawing and no check is needed.
fn ensure_compatible(
    font: &Font,
    projections: &GlyphProjectionSet,
    design: &DesignLocation,
    target: &StaticTarget,
) -> Result<()> {
    let at_master = font
        .masters()
        .any(|source| source.location().is_equivalent_to(design, font.axes()));
    if at_master {
        return Ok(());
    }

    let mut problems = Vec::new();
    for glyph in font.glyphs() {
        if font.master_layers(glyph).count() < 2 {
            continue;
        }
        let Some(projection) = projections.projection(&glyph.id()) else {
            continue;
        };
        for layer in incompatible_layers(font, glyph, projection.fallback(), font.masters()) {
            problems.push(format!(
                "{} on {}: {}",
                glyph.name(),
                layer.source.name(),
                layer.message()
            ));
        }
    }

    if problems.is_empty() {
        return Ok(());
    }
    bail!(
        "cannot compile {:?}: {} master layer(s) cannot interpolate; make their paths, nodes, \
         anchors, and components match the reference master (see `shift-cli check`):\n  {}",
        target.style_name,
        problems.len(),
        problems.join("\n  ")
    )
}

fn resolve_layers(
    font: &Font,
    projections: &GlyphProjectionSet,
    glyph_ids: &[GlyphId],
    design: &DesignLocation,
) -> Result<Vec<(GlyphId, GlyphLayer)>> {
    let mut layers = Vec::with_capacity(glyph_ids.len());
    for glyph_id in glyph_ids {
        let Some(projection) = projections.projection(glyph_id) else {
            continue;
        };
        let layer = projection
            .resolve(design, font.axes(), font.sources())
            .into_diagnostic()
            .wrap_err_with(|| {
                let name = font
                    .glyph(glyph_id)
                    .map_or_else(|| glyph_id.to_string(), |glyph| glyph.name().to_string());
                format!("failed to resolve glyph {name:?}")
            })?;
        layers.push((glyph_id.clone(), layer));
    }
    Ok(layers)
}
