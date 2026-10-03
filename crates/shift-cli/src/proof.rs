//! Renders text or source code set in a font to an image so a person or
//! agent can look at it.
//!
//! The font is compiled in memory and each proof line is shaped whole with
//! HarfRust at a user-space location. Plain text is drawn in one ink on one
//! background (black on white by default) as a waterfall of sizes. Code is
//! drawn like an editor at one size, each glyph colored by the syntax token
//! or `--spans` piece that contains it. Characters the font does not map are
//! shaped to `.notdef`, marked, and listed in the [`ProofReport`].

mod code;
mod code_layout;
mod font;
mod label;
mod layout;
mod raster;
mod report;
mod svg;
mod theme;

use std::path::Path;

use clap::ValueEnum;
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use serde::Serialize;
use syntect::parsing::SyntaxSet;

use crate::cli::ProofArgs;
use code::{ColoredLine, highlight, parse_spans, read_source, resolve_syntax};
use code_layout::{CodeLayout, layout_code};
use font::{ProofFont, compile_font};
use layout::{Guide, layout};
use report::{GuideReport, LineReport, ProofMode, RowReport, missing};
use theme::{DEFAULT_THEME, MARKER, MISSING_FILL, MissingStyle, Palette, Rgb, SIZE_LABEL};

pub use code::CodeSample;
pub use report::ProofReport;

/// Lines proofed when no `--text` is given: a spacing control string, the
/// classic round/straight check, a pangram, and confusable figures.
const DEFAULT_TEXT: [&str; 4] = [
    "Hamburgefonstiv",
    "HOHOHOnnnooo",
    "The quick brown fox jumps over the lazy dog",
    "0123456789 Il1| O0 {}[]()",
];
/// Text size used when `--sizes` is not given, in pixels per em.
const DEFAULT_SIZE: f32 = 72.0;
/// Code size used when `--sizes` is not given, in pixels per em.
const DEFAULT_CODE_SIZE: f32 = 20.0;
/// Columns per tab stop when `--tab-width` is not given.
const DEFAULT_TAB_WIDTH: usize = 4;
/// Largest accepted size, which keeps a single row within raster limits.
const MAX_SIZE: f32 = 1000.0;
/// How far line numbers fade from the foreground toward the background.
const LINE_NUMBER_FADE: f32 = 0.55;
/// Opacity of metric guides behind code, so they do not compete with tokens.
const CODE_GUIDE_OPACITY: f32 = 0.35;

/// Image encoding of a proof.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum ProofFormat {
    Png,
    Svg,
}

impl ProofFormat {
    /// Picks SVG for a `.svg` output path and PNG for anything else.
    fn infer(path: &Path) -> Self {
        let is_svg = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"));
        if is_svg { Self::Svg } else { Self::Png }
    }
}

/// The lines to set and the colors they come with, before CLI overrides.
struct Content {
    mode: ProofMode,
    lines: Vec<ColoredLine>,
    language: Option<String>,
    theme: Option<String>,
    background: Rgb,
    foreground: Rgb,
    styles_ignored: bool,
}

/// Compiles the font at `args.path`, renders the proof, and writes it to
/// `args.output`, replacing any existing file.
///
/// # Errors
///
/// Returns a diagnostic when the font cannot be read, compiled, or proofed
/// (for example because it maps no characters), when code, spans, a theme,
/// a language, or a color cannot be read or resolved, when an argument is
/// out of range, or when the image cannot be rendered or written.
pub fn proof(args: &ProofArgs) -> Result<ProofReport> {
    let tab_width = args.tab_width.map_or(DEFAULT_TAB_WIDTH, usize::from);
    let content = read_content(args, tab_width)?;
    let sizes = sizes(args, content.mode)?;
    if args.width == Some(0) {
        bail!("--width must be greater than 0");
    }
    let background = match &args.background {
        Some(hex) => Rgb::parse(hex)?,
        None => content.background,
    };
    let foreground = match &args.foreground {
        Some(hex) => Rgb::parse(hex)?,
        None => content.foreground,
    };

    let data = compile_font(&args.path)?;
    let font = ProofFont::new(&data, &args.location)?;
    let page = match content.mode {
        ProofMode::Text => {
            let palette = Palette {
                background,
                foreground,
                label: SIZE_LABEL,
                missing: MissingStyle::Fill(MISSING_FILL),
                guide_opacity: 1.0,
            };
            let lines: Vec<String> = content.lines.iter().map(|line| line.text.clone()).collect();
            layout(&font, &lines, &sizes, args.width, args.metrics, palette)
        }
        ProofMode::Code | ProofMode::Spans => {
            let palette = Palette {
                background,
                foreground,
                label: foreground.mix(background, LINE_NUMBER_FADE),
                missing: MissingStyle::Marker(MARKER),
                guide_opacity: CODE_GUIDE_OPACITY,
            };
            let options = CodeLayout {
                size: sizes[0],
                width: args.width,
                line_numbers: args.line_numbers,
                guides: args.metrics,
            };
            layout_code(&font, &content.lines, &options, palette)
        }
    };
    let format = args
        .format
        .unwrap_or_else(|| ProofFormat::infer(&args.output));
    let bytes = match format {
        ProofFormat::Png => raster::render_png(&page)?,
        ProofFormat::Svg => svg::render_svg(&page).into_bytes(),
    };
    std::fs::write(&args.output, bytes)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to write proof to {}", args.output.display()))?;

    let metrics = font.metrics();
    let line_reports: Vec<LineReport> = content
        .lines
        .iter()
        .map(|line| LineReport {
            text: line.text.clone(),
            missing_characters: missing(font.missing_characters(&line.text)),
        })
        .collect();
    let all_text: String = content
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect();
    let guides = if args.metrics {
        Guide::ALL
            .into_iter()
            .filter_map(|guide| {
                Some(GuideReport {
                    name: guide.name(),
                    color: guide.hex(),
                    font_units: guide.position(&metrics)?,
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    let code_mode = content.mode != ProofMode::Text;

    Ok(ProofReport {
        output: args.output.display().to_string(),
        format,
        mode: content.mode,
        width: page.width,
        height: page.height,
        sizes,
        language: content.language,
        theme: content.theme,
        background,
        foreground,
        line_count: content.lines.len(),
        tab_width: code_mode.then_some(tab_width),
        line_numbers: args.line_numbers,
        styles_ignored: content.styles_ignored,
        location: font.location(),
        metrics,
        guides,
        rows: page
            .rows
            .iter()
            .map(|row| RowReport {
                line: row.line,
                text: row.text.clone(),
                size: row.size,
                x: row.origin_x,
                baseline: row.baseline,
                width: row.run.advance * row.scale,
                clipped: row.clipped,
            })
            .collect(),
        missing_characters: missing(font.missing_characters(&all_text)),
        lines: line_reports,
    })
}

/// Validates `--sizes`; code is set at exactly one size.
fn sizes(args: &ProofArgs, mode: ProofMode) -> Result<Vec<f32>> {
    let sizes = match (args.sizes.is_empty(), mode) {
        (true, ProofMode::Text) => vec![DEFAULT_SIZE],
        (true, _) => vec![DEFAULT_CODE_SIZE],
        (false, _) => args.sizes.clone(),
    };
    if let Some(size) = sizes
        .iter()
        .find(|size| !size.is_finite() || **size <= 0.0 || **size > MAX_SIZE)
    {
        bail!("size {size} is out of range; sizes must be greater than 0 and at most {MAX_SIZE}");
    }
    if mode != ProofMode::Text && sizes.len() > 1 {
        bail!("code proofs are set at one size; pass a single --sizes value");
    }
    Ok(sizes)
}

fn read_content(args: &ProofArgs, tab_width: usize) -> Result<Content> {
    let themes = theme::Themes::load();
    let theme_name = args.theme.as_deref().unwrap_or(DEFAULT_THEME);
    let theme = themes.get(theme_name)?;
    let (theme_background, theme_foreground) = theme::theme_colors(theme);

    if let Some(spans) = &args.spans {
        let (document, lines) = parse_spans(&read_source(spans)?, tab_width)?;
        let background = document.background.as_deref().map(Rgb::parse).transpose()?;
        let foreground = document.foreground.as_deref().map(Rgb::parse).transpose()?;
        return Ok(Content {
            mode: ProofMode::Spans,
            lines,
            language: None,
            theme: (background.is_none() || foreground.is_none()).then(|| theme_name.to_string()),
            background: background.unwrap_or(theme_background),
            foreground: foreground.unwrap_or(theme_foreground),
            styles_ignored: false,
        });
    }

    let (source, language, path) = match (&args.code, args.code_sample) {
        (Some(path), _) => (read_source(path)?, args.lang.clone(), Some(path.as_path())),
        (None, Some(sample)) => (
            sample.source().to_string(),
            Some(sample.language().to_string()),
            None,
        ),
        (None, None) => {
            let lines: Vec<ColoredLine> = if args.text.is_empty() {
                DEFAULT_TEXT
                    .iter()
                    .map(|text| ColoredLine::plain(text))
                    .collect()
            } else {
                args.text
                    .iter()
                    .map(|text| ColoredLine::plain(text))
                    .collect()
            };
            let themed = args.theme.is_some();
            return Ok(Content {
                mode: ProofMode::Text,
                lines,
                language: None,
                theme: args.theme.clone(),
                background: if themed { theme_background } else { Rgb::WHITE },
                foreground: if themed { theme_foreground } else { Rgb::BLACK },
                styles_ignored: false,
            });
        }
    };

    let source_lines: Vec<&str> = source.lines().collect();
    if source_lines.is_empty() {
        bail!("the code is empty, so there is nothing to proof");
    }
    let syntaxes = SyntaxSet::load_defaults_newlines();
    let syntax = resolve_syntax(&syntaxes, language.as_deref(), path, source_lines[0])?;
    let highlighted = highlight(&syntaxes, syntax, theme, &source_lines, tab_width)?;
    Ok(Content {
        mode: ProofMode::Code,
        lines: highlighted.lines,
        language: Some(highlighted.language),
        theme: Some(theme_name.to_string()),
        background: theme_background,
        foreground: theme_foreground,
        styles_ignored: highlighted.styles_ignored,
    })
}
