//! Read-only font health checks with stable, machine-readable findings.
//!
//! `check` loads any font source Shift can read, applies outline, advance,
//! compatibility, and optional monospace and character-set rules to every
//! master source (or one selected source), and reports [`Finding`]s. Each
//! [`FindingCode`] has a fixed [`types::Severity`]; only errors fail the check.

mod charset;
mod classify;
mod geometry;
mod mono;
mod render;
mod rules;
mod types;

use std::path::Path;

use miette::{IntoDiagnostic, Result, WrapErr, miette};
use serde_json::json;
use shift_backends::font_loader::FontLoader;
use shift_font::{Font, Source};

pub use rules::incompatible_layers;
pub use types::CheckReport;

use charset::Charset;
use rules::{GlyphRules, unicode_labels};
use types::{CharsetSummary, CheckOptionsSummary, Finding, FindingCode, SeverityCounts};

/// Process exit status when the check ran and found no errors.
pub const EXIT_PASSED: i32 = 0;
/// Process exit status when at least one error-severity finding exists.
pub const EXIT_FAILED: i32 = 1;
/// Process exit status when the check could not run: unreadable font,
/// invalid options, unknown source, or an unreadable charset.
pub const EXIT_UNAVAILABLE: i32 = 2;

/// Rules selected for one check run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CheckOptions {
    /// Require every non-mark glyph to share one advance per source.
    pub mono: bool,
    /// Expected monospace advance in font units; defaults to the most common.
    pub width: Option<f64>,
    /// `ascii`, `latin1`, or a charset file path.
    pub charset: Option<String>,
    /// Restrict layer rules to one master, by name or full source ID.
    pub source: Option<String>,
}

impl CheckReport {
    /// Loads a font from `path` and checks it without modifying the file.
    ///
    /// # Errors
    ///
    /// Returns an error when the font cannot be read or the options cannot be
    /// applied to it; see [`Self::from_font`].
    pub fn load(path: &Path, options: &CheckOptions) -> Result<Self> {
        let path_text = path
            .to_str()
            .ok_or_else(|| miette!("font path is not valid UTF-8"))?;
        let font = FontLoader::new()
            .read_font(path_text)
            .into_diagnostic()
            .wrap_err_with(|| format!("failed to load font {}", path.display()))?;
        Self::from_font(path, &font, options)
    }

    /// Checks an in-memory font.
    ///
    /// # Errors
    ///
    /// Returns an error when `--source` names no master, the font has no
    /// master source, `width` is not a positive finite value, or the charset
    /// cannot be resolved.
    pub fn from_font(path: &Path, font: &Font, options: &CheckOptions) -> Result<Self> {
        if let Some(width) = options.width
            && (!width.is_finite() || width <= 0.0)
        {
            return Err(miette!("--width must be a positive number of font units"));
        }
        let charset = options
            .charset
            .as_deref()
            .map(Charset::resolve)
            .transpose()?;
        let sources = checked_sources(font, options.source.as_deref())?;
        let primary_source = match options.source {
            Some(_) => sources.first().copied(),
            None => font
                .default_source()
                .filter(|source| source.is_master())
                .or_else(|| sources.first().copied()),
        };

        let mut rules = GlyphRules::new(font, sources.clone(), primary_source);
        for glyph in font.glyphs() {
            rules.check_glyph(glyph);
        }
        let mut findings = std::mem::take(&mut rules.findings);

        let mono = if options.mono {
            sources
                .iter()
                .zip(&rules.samples)
                .map(|(source, samples)| {
                    mono::check_source(source.name(), samples, options.width, &mut findings)
                })
                .collect()
        } else {
            Vec::new()
        };
        let charset =
            charset.map(|charset| check_charset(font, &charset, primary_source, &mut findings));

        // Stable sort keeps discovery order within each severity.
        findings.sort_by_key(|finding| finding.severity);
        let counts = SeverityCounts::from_findings(&findings);

        Ok(Self {
            path: path.display().to_string(),
            format: path
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("unknown")
                .to_ascii_lowercase(),
            passed: counts.error == 0,
            options: CheckOptionsSummary {
                mono: options.mono,
                width: options.width,
                charset: options.charset.clone(),
                source: options.source.clone(),
            },
            counts,
            sources: sources
                .iter()
                .map(|source| source.name().to_string())
                .collect(),
            glyph_count: font.glyph_count(),
            mono,
            charset,
            findings,
        })
    }

    /// Returns the documented process exit status for this report.
    pub fn exit_code(&self) -> i32 {
        if self.passed {
            EXIT_PASSED
        } else {
            EXIT_FAILED
        }
    }
}

fn checked_sources<'a>(font: &'a Font, selector: Option<&str>) -> Result<Vec<&'a Source>> {
    let masters = font.masters().collect::<Vec<_>>();
    if masters.is_empty() {
        return Err(miette!("font has no master sources to check"));
    }
    let Some(selector) = selector else {
        return Ok(masters);
    };

    masters
        .into_iter()
        .find(|source| source.id().to_string() == selector || source.name() == selector)
        .map(|source| vec![source])
        .ok_or_else(|| {
            miette!("source {selector:?} is not a master; use its name or full source_ id")
        })
}

fn check_charset(
    font: &Font,
    charset: &Charset,
    source: Option<&Source>,
    findings: &mut Vec<Finding>,
) -> CharsetSummary {
    let mut missing_glyph_count = 0;
    let mut missing_layer_count = 0;
    for &codepoint in &charset.codepoints {
        let label = format!("U+{codepoint:04X}");
        let character = char::from_u32(codepoint)
            .map(String::from)
            .unwrap_or_default();
        let Some(glyph) = font.glyphs_by_unicode(codepoint).next() else {
            missing_glyph_count += 1;
            findings.push(Finding::new(
                FindingCode::CharsetMissingGlyph,
                format!("no glyph maps {label} {character:?}"),
                json!({ "codepoint": label, "character": character }),
            ));
            continue;
        };
        let Some(source) = source else {
            continue;
        };
        if glyph.layer_for_source(source.id()).is_some() {
            continue;
        }
        missing_layer_count += 1;
        // The charset finding supersedes the general missing-layer outline finding.
        findings.retain(|finding| {
            !(finding.code == FindingCode::EmptyOutline
                && finding.glyph.as_deref() == Some(glyph.name())
                && finding.details["reason"] == "noLayer")
        });
        findings.push(
            Finding::new(
                FindingCode::CharsetMissingLayer,
                format!("{label} maps to a glyph with no layer on {}", source.name()),
                json!({
                    "codepoint": label,
                    "character": character,
                    "unicodes": unicode_labels(glyph),
                }),
            )
            .glyph(glyph.name())
            .source(source.name()),
        );
    }

    CharsetSummary {
        name: charset.name.clone(),
        character_count: charset.codepoints.len(),
        missing_glyph_count,
        missing_layer_count,
    }
}
