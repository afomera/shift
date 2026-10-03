//! Writes a laid-out proof page as standalone SVG in pixel coordinates.

use std::fmt::Write as _;

use super::font::{PlacedGlyph, Segment};
use super::label::LABEL_HEIGHT;
use super::layout::{Label, Page, Row};
use super::theme::Rgb;

/// Renders `page` with a background rect and one filled `<path>` per run of
/// same-colored glyphs.
pub fn render_svg(page: &Page) -> String {
    let palette = &page.palette;
    let mut svg = String::new();
    let _ = writeln!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{0}" height="{1}" viewBox="0 0 {0} {1}">"#,
        page.width, page.height
    );
    let _ = writeln!(
        svg,
        r#"<rect class="background" width="100%" height="100%" fill="{}"/>"#,
        palette.background
    );

    for guide in &page.guides {
        let y = guide.y.round() + 0.5;
        let _ = writeln!(
            svg,
            r#"<line class="guide-{name}" x1="{x0}" y1="{y}" x2="{x1}" y2="{y}" stroke="{color}" stroke-opacity="{opacity}" stroke-width="1"/>"#,
            name = guide.guide.name(),
            x0 = guide.x0,
            x1 = guide.x1,
            color = guide.guide.hex(),
            opacity = round(palette.guide_opacity),
        );
    }

    let missing = palette.missing.color();
    for row in &page.rows {
        match &row.label {
            None => {}
            Some(Label::Bitmap { text, x }) => {
                let _ = writeln!(
                    svg,
                    r#"<text class="label" x="{x}" y="{y}" font-family="monospace" font-size="{LABEL_HEIGHT}" fill="{fill}">{text}</text>"#,
                    y = row.baseline,
                    fill = palette.label,
                );
            }
            Some(Label::Font { run, x }) => {
                write_path(&mut svg, row, *x, &run.glyphs, "label", palette.label);
            }
        }
        for (glyphs, color) in row.color_runs(missing) {
            let class = if glyphs[0].missing { "missing" } else { "ink" };
            write_path(&mut svg, row, row.origin_x, glyphs, class, color);
        }
    }

    svg.push_str("</svg>\n");
    svg
}

fn write_path(
    svg: &mut String,
    row: &Row,
    origin_x: f32,
    glyphs: &[PlacedGlyph],
    class: &str,
    fill: Rgb,
) {
    let mut data = String::new();
    let point = |data: &mut String, x: f32, y: f32| {
        let x = origin_x + x * row.scale;
        let y = row.baseline - y * row.scale;
        let _ = write!(data, "{} {} ", round(x), round(y));
    };
    for segment in glyphs.iter().flat_map(|glyph| &glyph.segments) {
        match *segment {
            Segment::Move(x, y) => {
                data.push('M');
                point(&mut data, x, y);
            }
            Segment::Line(x, y) => {
                data.push('L');
                point(&mut data, x, y);
            }
            Segment::Quad(cx, cy, x, y) => {
                data.push('Q');
                point(&mut data, cx, cy);
                point(&mut data, x, y);
            }
            Segment::Cubic(cx0, cy0, cx1, cy1, x, y) => {
                data.push('C');
                point(&mut data, cx0, cy0);
                point(&mut data, cx1, cy1);
                point(&mut data, x, y);
            }
            Segment::Close => data.push('Z'),
        }
    }
    if data.is_empty() {
        return;
    }

    let _ = writeln!(
        svg,
        r#"<path class="{class}" fill="{fill}" fill-rule="nonzero" d="{}"/>"#,
        data.trim_end()
    );
}

fn round(value: f32) -> f32 {
    (value * 100.0).round() / 100.0
}
