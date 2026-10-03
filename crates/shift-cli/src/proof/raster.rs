//! Rasterizes a laid-out proof page to PNG with tiny-skia.

use miette::{Result, miette};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Rect, Transform};

use super::font::Segment;
use super::label::{LABEL_ADVANCE, LABEL_HEIGHT, LABEL_PIXEL, cells};
use super::layout::{MARGIN, Page, Row, size_label};

/// Fill for glyphs the font draws.
const INK: [u8; 3] = [0, 0, 0];
/// Fill for `.notdef` glyphs shaped in place of missing characters.
pub const MISSING: [u8; 3] = [220, 38, 38];
/// Fill for row size labels.
pub const LABEL: [u8; 3] = [140, 140, 140];

/// Draws `page` black on white and returns encoded PNG bytes.
///
/// # Errors
///
/// Returns a diagnostic when the page is too large to allocate or the PNG
/// cannot be encoded.
pub fn render_png(page: &Page) -> Result<Vec<u8>> {
    let mut pixmap = Pixmap::new(page.width, page.height).ok_or_else(|| {
        miette!(
            "a {}x{} proof is too large to render; use fewer lines, smaller --sizes, or --width",
            page.width,
            page.height
        )
    })?;
    pixmap.fill(Color::WHITE);

    for guide in &page.guides {
        let y = guide.y.round();
        if let Some(rect) = Rect::from_xywh(guide.x0, y, guide.x1 - guide.x0, 1.0) {
            pixmap.fill_rect(
                rect,
                &paint(guide.guide.color()),
                Transform::identity(),
                None,
            );
        }
    }

    for row in &page.rows {
        draw_label(&mut pixmap, row);
        fill_glyphs(&mut pixmap, row, false, INK);
        fill_glyphs(&mut pixmap, row, true, MISSING);
    }

    pixmap
        .encode_png()
        .map_err(|error| miette!("failed to encode the proof PNG: {error}"))
}

fn paint(color: [u8; 3]) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], 255);
    paint.anti_alias = true;
    paint
}

fn fill_glyphs(pixmap: &mut Pixmap, row: &Row, missing: bool, color: [u8; 3]) {
    let mut builder = PathBuilder::new();
    let point = |x: f32, y: f32| (row.origin_x + x * row.scale, row.baseline - y * row.scale);
    let glyphs = row
        .run
        .glyphs
        .iter()
        .filter(|glyph| glyph.missing == missing);
    for segment in glyphs.flat_map(|glyph| &glyph.segments) {
        match *segment {
            Segment::Move(x, y) => {
                let (x, y) = point(x, y);
                builder.move_to(x, y);
            }
            Segment::Line(x, y) => {
                let (x, y) = point(x, y);
                builder.line_to(x, y);
            }
            Segment::Quad(cx, cy, x, y) => {
                let (cx, cy) = point(cx, cy);
                let (x, y) = point(x, y);
                builder.quad_to(cx, cy, x, y);
            }
            Segment::Cubic(cx0, cy0, cx1, cy1, x, y) => {
                let (cx0, cy0) = point(cx0, cy0);
                let (cx1, cy1) = point(cx1, cy1);
                let (x, y) = point(x, y);
                builder.cubic_to(cx0, cy0, cx1, cy1, x, y);
            }
            Segment::Close => builder.close(),
        }
    }

    if let Some(path) = builder.finish() {
        pixmap.fill_path(
            &path,
            &paint(color),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

fn draw_label(pixmap: &mut Pixmap, row: &Row) {
    let paint = paint(LABEL);
    let top = (row.baseline - LABEL_HEIGHT).round();
    for (index, character) in size_label(row.size).chars().enumerate() {
        let left = MARGIN + index as f32 * LABEL_ADVANCE;
        for (column, cell_row) in cells(character) {
            let rect = Rect::from_xywh(
                left + f32::from(column) * LABEL_PIXEL,
                top + f32::from(cell_row) * LABEL_PIXEL,
                LABEL_PIXEL,
                LABEL_PIXEL,
            );
            if let Some(rect) = rect {
                pixmap.fill_rect(rect, &paint, Transform::identity(), None);
            }
        }
    }
}
