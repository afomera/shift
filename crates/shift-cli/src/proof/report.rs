//! The structured description of a written proof, for terminals and JSON.

use serde::Serialize;

use super::ProofFormat;
use super::font::{AxisValue, ProofMetrics};
use super::theme::Rgb;

/// What a proof sets.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProofMode {
    /// `--text` lines, or the default proof set.
    Text,
    /// Highlighted source from `--code` or `--code-sample`.
    Code,
    /// Pre-colored lines from `--spans`.
    Spans,
}

/// Describes a written proof image.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofReport {
    pub output: String,
    pub format: ProofFormat,
    pub mode: ProofMode,
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Sizes in pixels per em, in the order each line was rendered.
    pub sizes: Vec<f32>,
    /// Grammar used to highlight code, such as `Rust`; `null` outside code
    /// mode.
    pub language: Option<String>,
    /// Syntax theme that supplied colors, when one did.
    pub theme: Option<String>,
    /// Page background as hex RGB.
    pub background: Rgb,
    /// Default ink as hex RGB.
    pub foreground: Rgb,
    /// Number of source lines set.
    pub line_count: usize,
    /// Columns per tab stop; `null` in text mode.
    pub tab_width: Option<usize>,
    pub line_numbers: bool,
    /// Whether the theme asked for bold, italic, or underline, which the
    /// single-style proof does not draw.
    pub styles_ignored: bool,
    /// Every font axis at the user-space value used for shaping.
    pub location: Vec<AxisValue>,
    /// Font-unit metrics at that location.
    pub metrics: ProofMetrics,
    /// Guides drawn behind the guided rows; empty without `--metrics`.
    pub guides: Vec<GuideReport>,
    pub lines: Vec<LineReport>,
    /// Rows drawn top to bottom: every line at every size, split further
    /// when `--width` wraps text.
    pub rows: Vec<RowReport>,
    /// Unmapped characters across all lines, in order of first appearance.
    pub missing_characters: Vec<MissingCharacter>,
}

/// One requested proof line, with tabs already expanded in code modes.
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
    /// Whether the text runs past the right padding of a fixed `--width`
    /// code proof and is cut off.
    pub clipped: bool,
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

/// Describes `characters` for the report.
pub fn missing(characters: Vec<char>) -> Vec<MissingCharacter> {
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
        if self.mode != ProofMode::Text {
            let mode = match self.mode {
                ProofMode::Code => "code",
                _ => "spans",
            };
            let language = self.language.as_deref().unwrap_or("-");
            lines.push(format!(
                "{mode}  {language}, tab width {}",
                self.tab_width.unwrap_or_default()
            ));
        }
        lines.push(format!(
            "colors  background {} foreground {}{}",
            self.background,
            self.foreground,
            self.theme
                .as_deref()
                .map(|theme| format!(" (theme {theme})"))
                .unwrap_or_default()
        ));
        if self.styles_ignored {
            lines.push("styles  bold/italic/underline from the theme are not drawn".to_string());
        }
        let clipped = self.rows.iter().filter(|row| row.clipped).count();
        if clipped > 0 {
            lines.push(format!("clipped  {clipped} rows run past --width"));
        }
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
            let marker = match self.mode {
                ProofMode::Text => "drawn red",
                _ => "drawn as magenta boxes",
            };
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
                "missing  {} characters ({marker}): {}",
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
