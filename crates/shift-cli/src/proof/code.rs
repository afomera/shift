//! Turns source code into colored proof lines.
//!
//! Code comes from a file, standard input, or a built-in sample. Each source
//! line becomes one [`ColoredLine`] whose tabs are already expanded to spaces,
//! so shaping sees exactly the columns an editor would show. Colors come from
//! syntect's bundled grammars and themes.

use std::io::Read as _;
use std::path::Path;

use clap::ValueEnum;
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Theme};
use syntect::parsing::{SyntaxReference, SyntaxSet};

use super::theme::Rgb;

/// A run of bytes in a [`ColoredLine`] drawn in one color.
#[derive(Clone, Debug, PartialEq)]
pub struct ColorSpan {
    /// Byte range into [`ColoredLine::text`].
    pub start: usize,
    pub end: usize,
    /// `None` draws in the page's default foreground.
    pub color: Option<Rgb>,
}

/// One rendered source line with tabs expanded.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ColoredLine {
    pub text: String,
    /// Ordered, non-overlapping spans covering colored bytes; bytes outside
    /// every span use the default foreground.
    pub spans: Vec<ColorSpan>,
    /// Column of the next character, counted in Unicode scalar values.
    column: usize,
}

impl ColoredLine {
    /// Wraps `text` verbatim in the default foreground.
    pub fn plain(text: &str) -> Self {
        Self {
            text: text.to_string(),
            spans: Vec::new(),
            column: text.chars().count(),
        }
    }

    /// Appends `piece` in `color`, replacing each tab with spaces up to the
    /// next multiple of `tab_width` columns.
    pub fn push(&mut self, piece: &str, color: Option<Rgb>, tab_width: usize) {
        let start = self.text.len();
        for character in piece.chars() {
            if character == '\t' {
                let spaces = tab_width - self.column % tab_width;
                self.text.extend(std::iter::repeat_n(' ', spaces));
                self.column += spaces;
            } else {
                self.text.push(character);
                self.column += 1;
            }
        }
        let end = self.text.len();
        if end > start {
            self.spans.push(ColorSpan { start, end, color });
        }
    }

    /// Returns the color of the byte at `offset`.
    pub fn color_at(&self, offset: usize) -> Option<Rgb> {
        self.spans
            .iter()
            .find(|span| span.start <= offset && offset < span.end)
            .and_then(|span| span.color)
    }
}

/// A short realistic program that exercises code-font stress cases.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum CodeSample {
    Rust,
    Typescript,
    Python,
    Go,
    Ruby,
    Shell,
}

impl CodeSample {
    /// The sample's source text.
    pub fn source(self) -> &'static str {
        match self {
            Self::Rust => include_str!("samples/sample.rs"),
            Self::Typescript => include_str!("samples/sample.ts"),
            Self::Python => include_str!("samples/sample.py"),
            Self::Go => include_str!("samples/sample.go"),
            Self::Ruby => include_str!("samples/sample.rb"),
            Self::Shell => include_str!("samples/sample.sh"),
        }
    }

    /// The language token used to pick a grammar.
    pub fn language(self) -> &'static str {
        match self {
            Self::Rust => "rs",
            Self::Typescript => "ts",
            Self::Python => "py",
            Self::Go => "go",
            Self::Ruby => "rb",
            Self::Shell => "sh",
        }
    }
}

/// Highlighted code ready for layout.
pub struct HighlightedCode {
    pub lines: Vec<ColoredLine>,
    /// Name of the grammar used, such as `Rust` or `Plain Text`.
    pub language: String,
    /// Whether any token asked for bold, italic, or underline, which a
    /// single-style proof cannot show.
    pub styles_ignored: bool,
}

/// Reads code from `path`, or from standard input when `path` is `-`.
///
/// # Errors
///
/// Returns a diagnostic when the input cannot be read or is not UTF-8.
pub fn read_source(path: &Path) -> Result<String> {
    if path == Path::new("-") {
        let mut text = String::new();
        std::io::stdin()
            .read_to_string(&mut text)
            .into_diagnostic()
            .wrap_err("failed to read code from standard input")?;
        return Ok(text);
    }
    std::fs::read_to_string(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to read code from {}", path.display()))
}

/// Picks a grammar: `language` when given (a name or file extension), else
/// the extension of `path`, else the first line (such as a shebang), else
/// plain text.
///
/// TypeScript has no bundled grammar, so `ts`, `tsx`, and `typescript`
/// resolve to JavaScript, which highlights TypeScript's shared syntax.
///
/// # Errors
///
/// Returns a diagnostic listing the bundled languages when `language`
/// matches none.
pub fn resolve_syntax<'a>(
    syntaxes: &'a SyntaxSet,
    language: Option<&str>,
    path: Option<&Path>,
    first_line: &str,
) -> Result<&'a SyntaxReference> {
    if let Some(language) = language {
        if let Some(syntax) = find_by_token(syntaxes, language) {
            return Ok(syntax);
        }
        let mut names: Vec<&str> = syntaxes
            .syntaxes()
            .iter()
            .map(|syntax| syntax.name.as_str())
            .collect();
        names.sort_unstable_by_key(|name| name.to_ascii_lowercase());
        bail!(
            "unknown language {language:?}; use a file extension or one of: {}",
            names.join(", ")
        );
    }

    let by_extension = path
        .and_then(|path| path.extension())
        .and_then(|extension| extension.to_str())
        .and_then(|extension| find_by_token(syntaxes, extension));
    let by_first_line = || syntaxes.find_syntax_by_first_line(first_line);
    Ok(by_extension
        .or_else(by_first_line)
        .unwrap_or_else(|| syntaxes.find_syntax_plain_text()))
}

fn find_by_token<'a>(syntaxes: &'a SyntaxSet, token: &str) -> Option<&'a SyntaxReference> {
    let lower = token.trim_start_matches('.').to_ascii_lowercase();
    let token = match lower.as_str() {
        "ts" | "tsx" | "mts" | "cts" | "typescript" => "js",
        "shell" | "zsh" | "bash" => "sh",
        other => other,
    };
    syntaxes.find_syntax_by_token(token).or_else(|| {
        syntaxes
            .syntaxes()
            .iter()
            .find(|syntax| syntax.name.eq_ignore_ascii_case(token))
    })
}

/// Highlights `lines` with `syntax` and `theme`, expanding tabs to
/// `tab_width` columns.
///
/// Tokens in the theme's own foreground carry no color, so a
/// `--foreground` override recolors them; every other token keeps its
/// theme color.
///
/// # Errors
///
/// Returns a diagnostic when the grammar fails on a line.
pub fn highlight(
    syntaxes: &SyntaxSet,
    syntax: &SyntaxReference,
    theme: &Theme,
    lines: &[&str],
    tab_width: usize,
) -> Result<HighlightedCode> {
    let mut highlighter = HighlightLines::new(syntax, theme);
    let default_foreground = theme.settings.foreground;
    let mut styles_ignored = false;
    let mut colored = Vec::with_capacity(lines.len());
    for line in lines {
        // The bundled grammars expect each line to end with a newline.
        let with_newline = format!("{line}\n");
        let tokens = highlighter
            .highlight_line(&with_newline, syntaxes)
            .into_diagnostic()
            .wrap_err("syntax highlighting failed")?;
        let mut output = ColoredLine::default();
        for (style, piece) in tokens {
            let piece = piece.trim_end_matches(['\n', '\r']);
            if piece.is_empty() {
                continue;
            }
            styles_ignored |= style.font_style != FontStyle::empty();
            let color = (Some(style.foreground) != default_foreground)
                .then(|| Rgb::from_syntect(style.foreground));
            output.push(piece, color, tab_width);
        }
        colored.push(output);
    }
    Ok(HighlightedCode {
        lines: colored,
        language: syntax.name.clone(),
        styles_ignored,
    })
}

/// The `--spans` document: pre-colored lines that bypass highlighting.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpansDocument {
    pub background: Option<String>,
    pub foreground: Option<String>,
    pub lines: Vec<Vec<SpanInput>>,
}

/// One colored piece of a `--spans` line.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpanInput {
    pub text: String,
    /// Hex color; omitted pieces use the document foreground.
    pub color: Option<String>,
}

/// Parses a `--spans` document into colored lines with tabs expanded.
///
/// # Errors
///
/// Returns a diagnostic when the JSON is malformed, a color is not hex, or a
/// piece contains a line break.
pub fn parse_spans(json: &str, tab_width: usize) -> Result<(SpansDocument, Vec<ColoredLine>)> {
    let document: SpansDocument = serde_json::from_str(json)
        .into_diagnostic()
        .wrap_err("--spans input is not a valid spans document")?;
    let mut lines = Vec::with_capacity(document.lines.len());
    for (index, pieces) in document.lines.iter().enumerate() {
        let mut line = ColoredLine::default();
        for piece in pieces {
            if piece.text.contains(['\n', '\r']) {
                bail!(
                    "--spans line {} has a piece containing a line break; put each line in its own array",
                    index + 1
                );
            }
            let color = piece.color.as_deref().map(Rgb::parse).transpose()?;
            line.push(&piece.text, color, tab_width);
        }
        lines.push(line);
    }
    Ok((document, lines))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_tabs_to_the_next_stop_and_keeps_span_ranges() {
        let mut line = ColoredLine::default();
        line.push("ab\t", None, 4);
        line.push("c\td", Some(Rgb([1, 2, 3])), 4);
        assert_eq!(line.text, "ab  c   d");
        assert_eq!(line.color_at(0), None);
        assert_eq!(line.color_at(4), Some(Rgb([1, 2, 3])));
        assert_eq!(line.color_at(8), Some(Rgb([1, 2, 3])));
    }

    #[test]
    fn resolves_languages_by_name_extension_and_first_line() {
        let syntaxes = SyntaxSet::load_defaults_newlines();
        let name = |language: Option<&str>, path: Option<&str>, first: &str| {
            resolve_syntax(&syntaxes, language, path.map(Path::new), first)
                .unwrap()
                .name
                .clone()
        };
        assert_eq!(name(Some("rust"), None, ""), "Rust");
        assert_eq!(name(Some("py"), None, ""), "Python");
        assert_eq!(name(Some("typescript"), None, ""), "JavaScript");
        assert_eq!(name(None, Some("main.go"), ""), "Go");
        assert_eq!(
            name(None, Some("-"), "#!/bin/bash"),
            "Bourne Again Shell (bash)"
        );
        assert_eq!(name(None, None, "hello"), "Plain Text");
        assert!(resolve_syntax(&syntaxes, Some("klingon"), None, "").is_err());
    }
}
