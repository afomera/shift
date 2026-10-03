//! Lays out colored code like an editor: one row per source line at one
//! size, a fixed line height, an optional line-number gutter, and no
//! wrapping.
//!
//! Each line is shaped whole so kerning, ligatures, and contextual
//! alternates work across token boundaries; glyphs then take the color of
//! the span containing their cluster's first byte.

use super::code::ColoredLine;
use super::font::{ProofFont, ShapedRun, hollow_box};
use super::label::LABEL_ADVANCE;
use super::layout::{Label, MARGIN, MIN_WIDTH, Page, Row, guide_lines};
use super::theme::{MissingStyle, Palette};

/// Line height as a multiple of the size, unless the font's ascender to
/// descender span is taller.
const LINE_HEIGHT_EM: f32 = 1.5;
/// Space between the line-number gutter and the code, in ems.
const GUTTER_GAP_EM: f32 = 1.0;
/// Horizontal inset of a missing-glyph marker, as a fraction of its advance.
const MARKER_INSET: f32 = 0.08;
/// Marker outline thickness, in pixels.
const MARKER_STROKE_PX: f32 = 1.5;

/// Options for [`layout_code`].
pub struct CodeLayout {
    /// Pixels per em.
    pub size: f32,
    /// Fixed page width in pixels; longer lines are clipped, not wrapped.
    pub width: Option<u32>,
    pub line_numbers: bool,
    /// Draw metric guides behind the first line only.
    pub guides: bool,
}

/// Lays out `lines` top to bottom with `palette` colors.
pub fn layout_code(
    font: &ProofFont,
    lines: &[ColoredLine],
    options: &CodeLayout,
    palette: Palette,
) -> Page {
    let metrics = font.metrics();
    let scale = options.size / f32::from(metrics.units_per_em);
    let ink_height = (metrics.ascender - metrics.descender) * scale;
    let line_height = (options.size * LINE_HEIGHT_EM).max(ink_height).ceil();
    let baseline_offset = (line_height - ink_height) / 2.0 + metrics.ascender * scale;

    let gutter = options
        .line_numbers
        .then(|| Gutter::new(font, lines.len(), options.size, scale));
    let origin_x = gutter.as_ref().map_or(MARGIN, |gutter| {
        MARGIN + gutter.width + options.size * GUTTER_GAP_EM
    });

    let mut rows: Vec<Row> = lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let mut run = font.shape(&line.text);
            if let MissingStyle::Marker(_) = palette.missing {
                mark_missing(&mut run, &metrics, scale);
            }
            let colors = run
                .glyphs
                .iter()
                .map(|glyph| line.color_at(glyph.cluster).unwrap_or(palette.foreground))
                .collect();
            let baseline = MARGIN + index as f32 * line_height + baseline_offset;
            Row {
                text: line.text.clone(),
                line: index,
                size: options.size,
                scale,
                origin_x,
                baseline,
                colors,
                run,
                guided: options.guides && index == 0,
                label: gutter.as_ref().map(|gutter| gutter.label(index + 1)),
                clipped: false,
            }
        })
        .collect();

    let content_right = rows
        .iter()
        .map(|row| row.origin_x + row.run.advance * row.scale)
        .fold(0.0_f32, f32::max);
    let page_width = options
        .width
        .map_or((content_right + MARGIN).max(MIN_WIDTH).ceil(), |width| {
            width as f32
        });
    for row in &mut rows {
        row.clipped = row.origin_x + row.run.advance * row.scale > page_width - MARGIN;
    }
    let page_height = (MARGIN * 2.0 + lines.len() as f32 * line_height).ceil();

    Page {
        width: page_width as u32,
        height: page_height as u32,
        guides: guide_lines(&rows, &metrics, page_width - MARGIN),
        rows,
        palette,
    }
}

/// Replaces each `.notdef` outline with a hollow box spanning the glyph's
/// advance from descender to ascender, so it reads as a marker rather than
/// as a token color.
fn mark_missing(run: &mut ShapedRun, metrics: &super::font::ProofMetrics, scale: f32) {
    let stroke = (MARKER_STROKE_PX / scale).max(f32::from(metrics.units_per_em) * 0.04);
    for glyph in run.glyphs.iter_mut().filter(|glyph| glyph.missing) {
        let inset = glyph.advance * MARKER_INSET;
        glyph.segments = hollow_box(
            (glyph.x + inset, metrics.descender),
            (glyph.x + glyph.advance - inset, metrics.ascender),
            stroke,
        );
    }
}

/// The right-aligned line-number column.
struct Gutter {
    /// Digit glyphs come from the proofed font when it maps `0-9`, else
    /// from the bitmap label face.
    numbers: Option<Vec<ShapedRun>>,
    scale: f32,
    right: f32,
    width: f32,
}

impl Gutter {
    fn new(font: &ProofFont, count: usize, size: f32, scale: f32) -> Self {
        let numbers = font.maps_all("0123456789").then(|| {
            (1..=count)
                .map(|number| font.shape(&number.to_string()))
                .collect::<Vec<_>>()
        });
        let width = match &numbers {
            Some(runs) => runs
                .iter()
                .map(|run| run.advance * scale)
                .fold(0.0_f32, f32::max),
            None => count.to_string().len() as f32 * LABEL_ADVANCE,
        };
        // Keep at least two digits of room so short files still read as code.
        let width = width.max(size);
        Self {
            numbers,
            scale,
            right: MARGIN + width,
            width,
        }
    }

    fn label(&self, number: usize) -> Label {
        match &self.numbers {
            Some(runs) => {
                let run = runs[number - 1].clone();
                Label::Font {
                    x: self.right - run.advance * self.scale,
                    run,
                }
            }
            None => {
                let text = number.to_string();
                Label::Bitmap {
                    x: self.right - text.len() as f32 * LABEL_ADVANCE,
                    text,
                }
            }
        }
    }
}
