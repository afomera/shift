//! Arranges shaped proof lines on a page in pixel space.
//!
//! Page coordinates are pixels with the origin at the top-left corner and y
//! pointing down. Each row keeps its run in font units plus the scale and
//! baseline needed to place it.

use super::font::{ProofFont, ProofMetrics, ShapedRun};
use super::label::LABEL_ADVANCE;

/// Space around the content, in pixels.
pub const MARGIN: f32 = 24.0;
/// Space between the size label column and the text, in pixels.
const LABEL_GAP: f32 = 16.0;
/// Extra space between rows, as a fraction of the row's size.
const ROW_GAP_EM: f32 = 0.25;
/// Narrowest page, so tiny proofs still have room for their labels.
const MIN_WIDTH: f32 = 160.0;

/// One rendered row: a whole proof line, or one wrapped part of it.
pub struct Row {
    pub text: String,
    /// Index of the proof line this row came from.
    pub line: usize,
    /// Pixels per em.
    pub size: f32,
    /// Pixels per font unit.
    pub scale: f32,
    /// Left edge of the text's pen origin, in pixels.
    pub origin_x: f32,
    /// Baseline, in pixels from the top.
    pub baseline: f32,
    pub run: ShapedRun,
    /// Whether this row gets metric guides.
    pub guided: bool,
}

/// A horizontal metric reference line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Guide {
    Ascender,
    CapHeight,
    XHeight,
    Baseline,
    Descender,
}

impl Guide {
    /// RGB color used by both raster and vector output.
    pub fn color(self) -> [u8; 3] {
        match self {
            Self::Ascender => [148, 103, 189],
            Self::CapHeight => [23, 190, 207],
            Self::XHeight => [44, 160, 44],
            Self::Baseline => [31, 119, 180],
            Self::Descender => [255, 127, 14],
        }
    }

    /// Stable name shared by JSON reports and SVG classes.
    pub fn name(self) -> &'static str {
        match self {
            Self::Ascender => "ascender",
            Self::CapHeight => "capHeight",
            Self::XHeight => "xHeight",
            Self::Baseline => "baseline",
            Self::Descender => "descender",
        }
    }

    pub fn hex(self) -> String {
        let [red, green, blue] = self.color();
        format!("#{red:02x}{green:02x}{blue:02x}")
    }

    /// Height above the baseline in font units, when the font defines it.
    pub fn position(self, metrics: &ProofMetrics) -> Option<f32> {
        match self {
            Self::Ascender => Some(metrics.ascender),
            Self::CapHeight => metrics.cap_height,
            Self::XHeight => metrics.x_height,
            Self::Baseline => Some(0.0),
            Self::Descender => Some(metrics.descender),
        }
    }

    pub const ALL: [Guide; 5] = [
        Self::Ascender,
        Self::CapHeight,
        Self::XHeight,
        Self::Baseline,
        Self::Descender,
    ];
}

/// A guide placed on the page, in pixels.
pub struct GuideLine {
    pub guide: Guide,
    pub y: f32,
    pub x0: f32,
    pub x1: f32,
}

/// Everything a renderer needs to draw the proof.
pub struct Page {
    pub width: u32,
    pub height: u32,
    pub rows: Vec<Row>,
    pub guides: Vec<GuideLine>,
}

/// Lays out every line at every size, line-major, wrapping at word
/// boundaries when `width` is set.
pub fn layout(
    font: &ProofFont,
    lines: &[String],
    sizes: &[f32],
    width: Option<u32>,
    with_guides: bool,
) -> Page {
    let metrics = font.metrics();
    let units_per_em = f32::from(metrics.units_per_em);
    let largest = sizes.iter().copied().fold(0.0_f32, f32::max);
    let label_chars = sizes
        .iter()
        .map(|size| size_label(*size).len())
        .max()
        .unwrap_or(0);
    let origin_x = MARGIN + label_chars as f32 * LABEL_ADVANCE + LABEL_GAP;
    let available = width.map(|width| (width as f32 - origin_x - MARGIN).max(1.0));

    let mut rows = Vec::new();
    let mut cursor = MARGIN;
    for (line_index, line) in lines.iter().enumerate() {
        for size in sizes {
            let scale = size / units_per_em;
            let guided = with_guides && *size == largest;
            for (text, run) in wrap(font, line, available.map(|width| width / scale)) {
                let baseline = cursor + metrics.ascender * scale;
                cursor = baseline - metrics.descender * scale + size * ROW_GAP_EM;
                rows.push(Row {
                    text,
                    line: line_index,
                    size: *size,
                    scale,
                    origin_x,
                    baseline,
                    run,
                    guided,
                });
            }
        }
    }

    let content_right = rows
        .iter()
        .map(|row| row.origin_x + row.run.advance * row.scale)
        .fold(0.0_f32, f32::max);
    let page_width = width.map_or((content_right + MARGIN).max(MIN_WIDTH).ceil(), |width| {
        width as f32
    });
    let page_height = (cursor - ROW_GAP_EM * sizes.last().copied().unwrap_or(0.0) + MARGIN).ceil();

    let guides = rows
        .iter()
        .filter(|row| row.guided)
        .flat_map(|row| {
            Guide::ALL.into_iter().filter_map(|guide| {
                let height = guide.position(&metrics)?;
                Some(GuideLine {
                    guide,
                    y: row.baseline - height * row.scale,
                    x0: row.origin_x,
                    x1: page_width - MARGIN,
                })
            })
        })
        .collect();

    Page {
        width: page_width as u32,
        height: page_height as u32,
        rows,
        guides,
    }
}

/// Formats a size for its row label, without a fractional part when whole.
pub fn size_label(size: f32) -> String {
    if size.fract() == 0.0 {
        format!("{size:.0}")
    } else {
        format!("{size}")
    }
}

/// Splits `text` greedily at spaces so each part fits `max_advance` font
/// units; a single word wider than the limit keeps its own row.
fn wrap(font: &ProofFont, text: &str, max_advance: Option<f32>) -> Vec<(String, ShapedRun)> {
    let Some(max_advance) = max_advance else {
        return vec![(text.to_string(), font.shape(text))];
    };

    let mut parts = Vec::new();
    let mut current: Option<(String, ShapedRun)> = None;
    for word in text.split(' ') {
        let candidate = match &current {
            Some((line, _)) => format!("{line} {word}"),
            None => word.to_string(),
        };
        let run = font.shape(&candidate);
        if run.advance <= max_advance {
            current = Some((candidate, run));
            continue;
        }
        match current.take() {
            Some(fitted) => {
                parts.push(fitted);
                current = Some((word.to_string(), font.shape(word)));
            }
            None => parts.push((candidate, run)),
        }
    }
    parts.extend(current);
    parts
}
