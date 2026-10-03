//! Rasterizes a laid-out proof page to PNG with tiny-skia.

use miette::{Result, miette};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Rect, Transform};

use super::font::{PlacedGlyph, Segment};
use super::label::{LABEL_ADVANCE, LABEL_HEIGHT, LABEL_PIXEL, cells};
use super::layout::{Label, Page, Row};
use super::theme::Rgb;

/// Draws `page` in its palette and returns encoded PNG bytes.
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
    let [red, green, blue] = page.palette.background.0;
    pixmap.fill(tiny_skia::Color::from_rgba8(red, green, blue, 255));

    let guide_alpha = (page.palette.guide_opacity * 255.0).round() as u8;
    for guide in &page.guides {
        let y = guide.y.round();
        if let Some(rect) = Rect::from_xywh(guide.x0, y, guide.x1 - guide.x0, 1.0) {
            let paint = paint(Rgb(guide.guide.color()), guide_alpha);
            pixmap.fill_rect(rect, &paint, Transform::identity(), None);
        }
    }

    let missing = page.palette.missing.color();
    for row in &page.rows {
        draw_label(&mut pixmap, row, page.palette.label);
        for (glyphs, color) in row.color_runs(missing) {
            fill_glyphs(&mut pixmap, row, row.origin_x, glyphs, color);
        }
    }

    pixmap
        .encode_png()
        .map_err(|error| miette!("failed to encode the proof PNG: {error}"))
}

fn paint(color: Rgb, alpha: u8) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color.0[0], color.0[1], color.0[2], alpha);
    paint.anti_alias = true;
    paint
}

fn fill_glyphs(pixmap: &mut Pixmap, row: &Row, origin_x: f32, glyphs: &[PlacedGlyph], color: Rgb) {
    let mut builder = PathBuilder::new();
    let point = |x: f32, y: f32| (origin_x + x * row.scale, row.baseline - y * row.scale);
    for segment in glyphs.iter().flat_map(|glyph| &glyph.segments) {
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
            &paint(color, 255),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

fn draw_label(pixmap: &mut Pixmap, row: &Row, color: Rgb) {
    match &row.label {
        None => {}
        Some(Label::Font { run, x }) => fill_glyphs(pixmap, row, *x, &run.glyphs, color),
        Some(Label::Bitmap { text, x }) => {
            let paint = paint(color, 255);
            let top = (row.baseline - LABEL_HEIGHT).round();
            for (index, character) in text.chars().enumerate() {
                let left = x + index as f32 * LABEL_ADVANCE;
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
    }
}
