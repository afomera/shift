//! Compiles a font source and shapes proof text against the binary.
//!
//! Every coordinate produced here is in font units with the y axis pointing
//! up and the run's pen origin at `(0, 0)`; renderers scale and flip.

use std::collections::HashSet;
use std::path::Path;

use harfrust::{ShapeOptions, ShaperData, ShaperInstance, UnicodeBuffer};
use miette::{IntoDiagnostic, Result, WrapErr, bail, miette};
use serde::Serialize;
use shift_backends::FontExporter;
use shift_backends::font_loader::FontLoader;
use skrifa::instance::{Location, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, GlyphId, MetadataProvider};

/// Width of the placeholder box drawn for a missing glyph with no ink, in ems.
const PLACEHOLDER_MIN_ADVANCE_EM: f32 = 0.5;
/// Height of the placeholder box, in ems.
const PLACEHOLDER_HEIGHT_EM: f32 = 0.7;
/// Stroke thickness of the placeholder box, in ems.
const PLACEHOLDER_STROKE_EM: f32 = 0.04;

/// Reads `path` and returns TrueType or OpenType bytes ready for shaping.
///
/// Binary `.ttf` and `.otf` files are used as-is; every other format that
/// `glyph inspect` accepts, including `.shift`, is loaded into the Shift
/// model and compiled in memory through the same pipeline as `compile`.
///
/// # Errors
///
/// Returns a diagnostic when the source cannot be read or compiled.
pub fn compile_font(path: &Path) -> Result<Vec<u8>> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "ttf" | "otf") {
        return std::fs::read(path)
            .into_diagnostic()
            .wrap_err_with(|| format!("failed to read font {}", path.display()));
    }

    let path_text = path
        .to_str()
        .ok_or_else(|| miette!("font path is not valid UTF-8"))?;
    let font = FontLoader::new()
        .read_font(path_text)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to load font {}", path.display()))?;
    FontExporter::new()
        .compile_ttf(&font)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to compile {} for proofing", path.display()))
}

/// One outline command in font units, y up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Segment {
    Move(f32, f32),
    Line(f32, f32),
    Quad(f32, f32, f32, f32),
    Cubic(f32, f32, f32, f32, f32, f32),
    Close,
}

/// A shaped glyph positioned relative to its run's pen origin.
#[derive(Clone, Debug)]
pub struct PlacedGlyph {
    /// Outline already offset by the glyph's pen position, in font units.
    pub segments: Vec<Segment>,
    /// Whether shaping fell back to `.notdef`; renderers draw these in a
    /// warning color, and an inkless `.notdef` becomes a hollow box.
    pub missing: bool,
}

/// Shaped text in font units.
#[derive(Clone, Debug)]
pub struct ShapedRun {
    pub glyphs: Vec<PlacedGlyph>,
    /// Total horizontal advance, including placeholder widths.
    pub advance: f32,
}

/// Vertical metrics at the proof location, in font units.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofMetrics {
    pub units_per_em: u16,
    pub ascender: f32,
    /// Negative below the baseline.
    pub descender: f32,
    pub x_height: Option<f32>,
    pub cap_height: Option<f32>,
}

/// One axis coordinate of the proof location, in user space.
#[derive(Clone, Debug, Serialize)]
pub struct AxisValue {
    pub tag: String,
    pub value: f32,
}

/// A compiled font bound to one variation location.
pub struct ProofFont<'a> {
    data: &'a [u8],
    font: FontRef<'a>,
    location: Location,
    variations: Vec<(String, f32)>,
}

impl<'a> ProofFont<'a> {
    /// Binds `data` to the user-space `coordinates`, each `TAG=VALUE`.
    ///
    /// Axes that are not named stay at their defaults.
    ///
    /// # Errors
    ///
    /// Returns a diagnostic when the bytes are not a font, the font maps no
    /// characters, or a coordinate names an unknown axis, repeats one, or
    /// lies outside the axis range.
    pub fn new(data: &'a [u8], coordinates: &[String]) -> Result<Self> {
        let font =
            FontRef::new(data).map_err(|error| miette!("compiled font is unreadable: {error}"))?;
        if font.charmap().mappings().next().is_none() {
            bail!(
                "the font maps no characters, so there is nothing to proof; \
                 add glyphs with `glyph add NAME --unicode U+XXXX` and draw them with `layer add`"
            );
        }

        let variations = parse_location(&font, coordinates)?;
        let location = font
            .axes()
            .location(variations.iter().map(|(tag, value)| (tag.as_str(), *value)));
        Ok(Self {
            data,
            font,
            location,
            variations,
        })
    }

    /// Returns every axis with the user-space value the proof uses.
    pub fn location(&self) -> Vec<AxisValue> {
        self.variations
            .iter()
            .map(|(tag, value)| AxisValue {
                tag: tag.clone(),
                value: *value,
            })
            .collect()
    }

    pub fn metrics(&self) -> ProofMetrics {
        let metrics = self.font.metrics(Size::unscaled(), &self.location);
        ProofMetrics {
            units_per_em: metrics.units_per_em,
            ascender: metrics.ascent,
            descender: metrics.descent,
            x_height: metrics.x_height,
            cap_height: metrics.cap_height,
        }
    }

    /// Returns characters of `text` the character map does not cover, in
    /// order of first appearance; control characters are ignored.
    pub fn missing_characters(&self, text: &str) -> Vec<char> {
        let charmap = self.font.charmap();
        let mut seen = HashSet::new();
        text.chars()
            .filter(|character| !character.is_control())
            .filter(|character| charmap.map(*character).is_none())
            .filter(|character| seen.insert(*character))
            .collect()
    }

    /// Shapes `text` with HarfRust at the bound location.
    pub fn shape(&self, text: &str) -> ShapedRun {
        let Ok(font) = harfrust::FontRef::new(self.data) else {
            return ShapedRun {
                glyphs: Vec::new(),
                advance: 0.0,
            };
        };
        let data = ShaperData::new(&font);
        let instance = ShaperInstance::from_variations(
            &font,
            self.variations
                .iter()
                .map(|(tag, value)| (tag.as_str(), *value)),
        );
        let shaper = data.shaper(&font).instance(Some(&instance)).build();
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.guess_segment_properties();
        let shaped = shaper.shape(buffer, ShapeOptions::new());

        let units_per_em = f32::from(self.metrics().units_per_em);
        let mut glyphs = Vec::new();
        let mut pen_x = 0.0;
        for (info, position) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
            let x = pen_x + position.x_offset as f32;
            let y = position.y_offset as f32;
            let mut advance = position.x_advance as f32;
            let missing = info.glyph_id == 0;
            let mut segments = self.outline(info.glyph_id, x, y);
            if missing && segments.is_empty() {
                advance = advance.max(units_per_em * PLACEHOLDER_MIN_ADVANCE_EM);
                segments = placeholder_box(x, advance, units_per_em);
            }
            glyphs.push(PlacedGlyph { segments, missing });
            pen_x += advance;
        }

        ShapedRun {
            glyphs,
            advance: pen_x,
        }
    }

    fn outline(&self, glyph_id: u32, x: f32, y: f32) -> Vec<Segment> {
        let mut pen = SegmentPen {
            x,
            y,
            segments: Vec::new(),
        };
        if let Some(glyph) = self.font.outline_glyphs().get(GlyphId::new(glyph_id)) {
            let settings = DrawSettings::unhinted(Size::unscaled(), &self.location);
            // A glyph that fails to draw contributes no ink, like an empty one.
            if glyph.draw(settings, &mut pen).is_err() {
                pen.segments.clear();
            }
        }
        pen.segments
    }
}

fn parse_location(font: &FontRef, coordinates: &[String]) -> Result<Vec<(String, f32)>> {
    let axes: Vec<_> = font.axes().iter().collect();
    let mut values: Vec<(String, f32)> = axes
        .iter()
        .map(|axis| (axis.tag().to_string(), axis.default_value()))
        .collect();

    let mut seen = HashSet::new();
    for coordinate in coordinates {
        let Some((tag, value)) = coordinate.split_once('=') else {
            bail!("invalid location {coordinate:?}; expected TAG=VALUE");
        };
        let tag = tag.trim();
        if tag.is_empty() || !seen.insert(tag.to_string()) {
            bail!("axis tag {tag:?} is blank or repeated in the location");
        }
        let value: f32 = value
            .trim()
            .parse()
            .map_err(|_| miette!("location value {value:?} for axis {tag:?} is not a number"))?;
        if !value.is_finite() {
            bail!("location value for axis tag {tag:?} must be finite");
        }

        let Some(index) = values.iter().position(|(axis_tag, _)| axis_tag == tag) else {
            let known: Vec<&str> = values.iter().map(|(tag, _)| tag.as_str()).collect();
            bail!(
                "the compiled font has no {tag:?} axis; available axes: [{}]",
                known.join(", ")
            );
        };
        let axis = &axes[index];
        if value < axis.min_value() || value > axis.max_value() {
            bail!(
                "{tag}={value} is outside the axis range {}..{}",
                axis.min_value(),
                axis.max_value()
            );
        }
        values[index].1 = value;
    }
    Ok(values)
}

/// Builds a hollow rectangle whose inner contour runs opposite the outer one,
/// so a nonzero fill leaves the middle empty.
fn placeholder_box(x: f32, advance: f32, units_per_em: f32) -> Vec<Segment> {
    let stroke = units_per_em * PLACEHOLDER_STROKE_EM;
    let inset = advance * 0.1;
    let (left, right) = (x + inset, x + advance - inset);
    let (bottom, top) = (0.0, units_per_em * PLACEHOLDER_HEIGHT_EM);
    vec![
        Segment::Move(left, bottom),
        Segment::Line(right, bottom),
        Segment::Line(right, top),
        Segment::Line(left, top),
        Segment::Close,
        Segment::Move(left + stroke, bottom + stroke),
        Segment::Line(left + stroke, top - stroke),
        Segment::Line(right - stroke, top - stroke),
        Segment::Line(right - stroke, bottom + stroke),
        Segment::Close,
    ]
}

/// Collects outline commands offset by a glyph's pen position.
struct SegmentPen {
    x: f32,
    y: f32,
    segments: Vec<Segment>,
}

impl OutlinePen for SegmentPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.segments.push(Segment::Move(self.x + x, self.y + y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.segments.push(Segment::Line(self.x + x, self.y + y));
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.segments.push(Segment::Quad(
            self.x + cx0,
            self.y + cy0,
            self.x + x,
            self.y + y,
        ));
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.segments.push(Segment::Cubic(
            self.x + cx0,
            self.y + cy0,
            self.x + cx1,
            self.y + cy1,
            self.x + x,
            self.y + y,
        ));
    }

    fn close(&mut self) {
        self.segments.push(Segment::Close);
    }
}
