//! Renders text set in a font to an image so a person or agent can look at it.
//!
//! The font is compiled in memory, each proof line is shaped with HarfRust at
//! a user-space location, and outlines are drawn black on white. Characters
//! the font does not map are shaped to `.notdef`, drawn in red, and listed in
//! the [`ProofReport`].

mod font;
mod label;
mod layout;
mod raster;
mod svg;

use std::path::Path;

use clap::ValueEnum;
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use serde::Serialize;

use crate::cli::ProofArgs;
use font::{AxisValue, ProofFont, ProofMetrics, compile_font};
use layout::{Guide, layout};

/// Lines proofed when no `--text` is given: a spacing control string, the
/// classic round/straight check, a pangram, and confusable figures.
const DEFAULT_TEXT: [&str; 4] = [
    "Hamburgefonstiv",
    "HOHOHOnnnooo",
    "The quick brown fox jumps over the lazy dog",
    "0123456789 Il1| O0 {}[]()",
];
/// Size used when `--sizes` is not given, in pixels per em.
const DEFAULT_SIZE: f32 = 72.0;
/// Largest accepted size, which keeps a single row within raster limits.
const MAX_SIZE: f32 = 1000.0;

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

/// Describes a written proof image.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofReport {
    pub output: String,
    pub format: ProofFormat,
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Sizes in pixels per em, in the order each line was rendered.
    pub sizes: Vec<f32>,
    /// Every font axis at the user-space value used for shaping.
    pub location: Vec<AxisValue>,
    /// Font-unit metrics at that location.
    pub metrics: ProofMetrics,
    /// Guides drawn behind the largest-size rows; empty without `--metrics`.
    pub guides: Vec<GuideReport>,
    pub lines: Vec<LineReport>,
    /// Rows drawn top to bottom: every line at every size, split further
    /// when `--width` wraps it.
    pub rows: Vec<RowReport>,
    /// Unmapped characters across all lines, in order of first appearance.
    pub missing_characters: Vec<MissingCharacter>,
}

/// One requested proof line.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineReport {
    pub text: String,
    pub missing_characters: Vec<MissingCharacter>,
}

/// One drawn row of the proof image.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RowReport {
    /// Index into [`ProofReport::lines`].
    pub line: usize,
    pub text: String,
    /// Pixels per em.
    pub size: f32,
    /// Pen origin of the first glyph, in pixels from the left of the image.
    pub x: f32,
    /// Baseline, in pixels from the top of the image.
    pub baseline: f32,
    /// Advance width of the set text, in pixels.
    pub width: f32,
}

/// A character the font's character map does not cover.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MissingCharacter {
    pub character: String,
    /// Code point as `U+XXXX`.
    pub unicode: String,
}

/// A metric guide and where it sits above the baseline.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuideReport {
    pub name: &'static str,
    /// Hex RGB stroke color.
    pub color: String,
    pub font_units: f32,
}

/// Compiles the font at `args.path`, renders the proof, and writes it to
/// `args.output`, replacing any existing file.
///
/// # Errors
///
/// Returns a diagnostic when the font cannot be read, compiled, or proofed
/// (for example because it maps no characters), when an argument is out of
/// range, or when the image cannot be rendered or written.
pub fn proof(args: &ProofArgs) -> Result<ProofReport> {
    let lines: Vec<String> = if args.text.is_empty() {
        DEFAULT_TEXT.iter().map(ToString::to_string).collect()
    } else {
        args.text.clone()
    };
    let sizes = if args.sizes.is_empty() {
        vec![DEFAULT_SIZE]
    } else {
        args.sizes.clone()
    };
    if let Some(size) = sizes
        .iter()
        .find(|size| !size.is_finite() || **size <= 0.0 || **size > MAX_SIZE)
    {
        bail!("size {size} is out of range; sizes must be greater than 0 and at most {MAX_SIZE}");
    }
    if args.width == Some(0) {
        bail!("--width must be greater than 0");
    }

    let data = compile_font(&args.path)?;
    let font = ProofFont::new(&data, &args.location)?;
    let page = layout(&font, &lines, &sizes, args.width, args.metrics);
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
    let line_reports: Vec<LineReport> = lines
        .iter()
        .map(|text| LineReport {
            text: text.clone(),
            missing_characters: missing(font.missing_characters(text)),
        })
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

    Ok(ProofReport {
        output: args.output.display().to_string(),
        format,
        width: page.width,
        height: page.height,
        sizes,
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
            })
            .collect(),
        missing_characters: missing(font.missing_characters(&lines.concat())),
        lines: line_reports,
    })
}

fn missing(characters: Vec<char>) -> Vec<MissingCharacter> {
    characters
        .into_iter()
        .map(|character| MissingCharacter {
            character: character.to_string(),
            unicode: format!("U+{:04X}", u32::from(character)),
        })
        .collect()
}

impl ProofReport {
    /// Formats the report as plain text lines for terminals.
    pub fn render(&self) -> String {
        let format = match self.format {
            ProofFormat::Png => "png",
            ProofFormat::Svg => "svg",
        };
        let sizes: Vec<String> = self.sizes.iter().map(|size| format!("{size}px")).collect();
        let mut lines = vec![
            format!("{}  {}x{} {format}", self.output, self.width, self.height),
            format!(
                "rows  {} ({} {} at {})",
                self.rows.len(),
                self.lines.len(),
                if self.lines.len() == 1 {
                    "line"
                } else {
                    "lines"
                },
                sizes.join(", ")
            ),
        ];
        if !self.location.is_empty() {
            let location: Vec<String> = self
                .location
                .iter()
                .map(|axis| format!("{}={}", axis.tag, axis.value))
                .collect();
            lines.push(format!("location  {}", location.join(", ")));
        }
        for guide in &self.guides {
            lines.push(format!(
                "guide  {:<10} {}  {}",
                guide.name, guide.color, guide.font_units
            ));
        }
        if self.missing_characters.is_empty() {
            lines.push("missing  none".to_string());
        } else {
            let missing: Vec<String> = self
                .missing_characters
                .iter()
                .map(|missing| {
                    format!(
                        "{} {}",
                        display_character(&missing.character),
                        missing.unicode
                    )
                })
                .collect();
            lines.push(format!(
                "missing  {} characters (drawn red): {}",
                self.missing_characters.len(),
                missing.join(", ")
            ));
        }
        lines.join("\n")
    }
}

fn display_character(character: &str) -> String {
    if character == " " {
        "space".to_string()
    } else {
        character.to_string()
    }
}
