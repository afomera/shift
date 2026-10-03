//! Character sets that `check --charset` requires a font to cover.

use std::path::Path;

use miette::{IntoDiagnostic, Result, WrapErr, bail, miette};

/// Ordered, duplicate-free Unicode scalar values a font must map.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Charset {
    pub name: String,
    pub codepoints: Vec<u32>,
}

impl Charset {
    /// Resolves a built-in name (`ascii`, `latin1`) or reads a charset file.
    ///
    /// A charset file is UTF-8 text read line by line. Blank lines and lines
    /// starting with `#` are ignored. A line starting with `U+` names one scalar
    /// (`U+0041`) or an inclusive range (`U+0041..U+005A`); any other line
    /// contributes each of its non-whitespace characters. Use `U+0020` and
    /// `U+0023` to require space and `#`.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read, a `U+` line is not a
    /// valid scalar or range, or the charset is empty.
    pub fn resolve(selector: &str) -> Result<Self> {
        match selector {
            "ascii" => Ok(Self::new("ascii", ascii().collect())),
            "latin1" => Ok(Self::new("latin1", ascii().chain(0xA0..=0xFF).collect())),
            path => Self::read(Path::new(path)),
        }
    }

    fn new(name: &str, mut codepoints: Vec<u32>) -> Self {
        let mut seen = std::collections::HashSet::new();
        codepoints.retain(|codepoint| seen.insert(*codepoint));
        Self {
            name: name.to_string(),
            codepoints,
        }
    }

    fn read(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .into_diagnostic()
            .wrap_err_with(|| {
                format!(
                    "failed to read charset {}; use ascii, latin1, or a readable file",
                    path.display()
                )
            })?;
        let charset = Self::new(&path.display().to_string(), parse(&text)?);
        if charset.codepoints.is_empty() {
            bail!("charset {} contains no characters", path.display());
        }
        Ok(charset)
    }
}

fn ascii() -> impl Iterator<Item = u32> {
    0x20..=0x7E
}

fn parse(text: &str) -> Result<Vec<u32>> {
    let mut codepoints = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let names_scalar = line
            .get(..2)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("U+"));
        if names_scalar && line.len() > 2 {
            let range = parse_range(line)
                .wrap_err_with(|| format!("invalid charset line {}: {line:?}", index + 1))?;
            codepoints.extend(range);
            continue;
        }
        codepoints.extend(
            line.chars()
                .filter(|character| !character.is_whitespace())
                .map(u32::from),
        );
    }
    Ok(codepoints)
}

fn parse_range(line: &str) -> Result<Vec<u32>> {
    let (start, end) = match line.split_once("..") {
        Some((start, end)) => (parse_scalar(start)?, parse_scalar(end)?),
        None => {
            let scalar = parse_scalar(line)?;
            (scalar, scalar)
        }
    };
    if end < start {
        bail!("range end precedes its start");
    }
    Ok((start..=end)
        .filter(|codepoint| char::from_u32(*codepoint).is_some())
        .collect())
}

fn parse_scalar(text: &str) -> Result<u32> {
    let text = text.trim();
    let hex = text
        .strip_prefix("U+")
        .or_else(|| text.strip_prefix("u+"))
        .ok_or_else(|| miette!("expected U+XXXX"))?;
    let value = u32::from_str_radix(hex, 16)
        .into_diagnostic()
        .wrap_err("expected hexadecimal digits after U+")?;
    char::from_u32(value).ok_or_else(|| miette!("U+{value:04X} is not a Unicode scalar value"))?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_charsets_cover_their_documented_ranges() {
        let ascii = Charset::resolve("ascii").unwrap();
        let latin1 = Charset::resolve("latin1").unwrap();

        assert_eq!(ascii.codepoints.len(), 95);
        assert_eq!(ascii.codepoints.first(), Some(&0x20));
        assert_eq!(ascii.codepoints.last(), Some(&0x7E));
        assert_eq!(latin1.codepoints.len(), 95 + 96);
        assert_eq!(latin1.codepoints.last(), Some(&0xFF));
    }

    #[test]
    fn file_lines_accept_characters_scalars_and_ranges() {
        let parsed = parse("# comment\nAB\nU+0020\nu+0030..U+0032\n\n é\n€").unwrap();

        assert_eq!(
            parsed,
            vec![0x41, 0x42, 0x20, 0x30, 0x31, 0x32, 0xE9, 0x20AC]
        );
    }

    #[test]
    fn surrogate_scalars_are_rejected() {
        assert!(parse("U+D800").is_err());
    }
}
