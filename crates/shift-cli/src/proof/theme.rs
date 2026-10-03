//! Colors for proof pages: hex parsing, bundled syntax themes, and the
//! palette a renderer paints with.

use std::fmt;

use miette::{Result, bail, miette};
use serde::{Serialize, Serializer};
use syntect::highlighting::{Theme, ThemeSet};

/// Theme used for code proofs when `--theme` is not given.
pub const DEFAULT_THEME: &str = "base16-ocean.dark";
/// High-contrast outline for missing glyphs in colored code proofs.
pub const MARKER: Rgb = Rgb([255, 0, 255]);
/// Fill for `.notdef` glyphs in plain text proofs.
pub const MISSING_FILL: Rgb = Rgb([220, 38, 38]);
/// Fill for row size labels in plain text proofs.
pub const SIZE_LABEL: Rgb = Rgb([140, 140, 140]);

/// An opaque 8-bit sRGB color.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rgb(pub [u8; 3]);

impl Rgb {
    pub const WHITE: Self = Self([255, 255, 255]);
    pub const BLACK: Self = Self([0, 0, 0]);

    /// Parses `#rgb` or `#rrggbb`; the leading `#` is optional.
    ///
    /// # Errors
    ///
    /// Returns a diagnostic naming `text` when it is not a hex color.
    pub fn parse(text: &str) -> Result<Self> {
        let digits = text.trim().trim_start_matches('#');
        let expanded: String = match digits.len() {
            3 => digits.chars().flat_map(|digit| [digit, digit]).collect(),
            6 => digits.to_string(),
            _ => bail!("{text:?} is not a hex color; expected #rrggbb or #rgb"),
        };
        let channel = |index: usize| {
            u8::from_str_radix(&expanded[index..index + 2], 16)
                .map_err(|_| miette!("{text:?} is not a hex color; expected #rrggbb or #rgb"))
        };
        Ok(Self([channel(0)?, channel(2)?, channel(4)?]))
    }

    /// Mixes `self` toward `other`; `amount` 0 keeps `self`, 1 gives `other`.
    pub fn mix(self, other: Self, amount: f32) -> Self {
        let channel = |index: usize| {
            let from = f32::from(self.0[index]);
            let to = f32::from(other.0[index]);
            (from + (to - from) * amount).round().clamp(0.0, 255.0) as u8
        };
        Self([channel(0), channel(1), channel(2)])
    }

    /// Converts a syntect color, dropping alpha.
    pub fn from_syntect(color: syntect::highlighting::Color) -> Self {
        Self([color.r, color.g, color.b])
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [red, green, blue] = self.0;
        write!(formatter, "#{red:02x}{green:02x}{blue:02x}")
    }
}

impl Serialize for Rgb {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// How `.notdef` glyphs are drawn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MissingStyle {
    /// The glyph's own outline (or a hollow placeholder) filled in this color.
    Fill(Rgb),
    /// A hollow box spanning the glyph's advance, outlined in this color,
    /// so it cannot be mistaken for a syntax color.
    Marker(Rgb),
}

impl MissingStyle {
    /// The marker or fill color.
    pub fn color(self) -> Rgb {
        match self {
            Self::Fill(color) | Self::Marker(color) => color,
        }
    }
}

/// Every color a renderer needs for one page.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub background: Rgb,
    /// Ink for text without a token color.
    pub foreground: Rgb,
    /// Size labels or line numbers.
    pub label: Rgb,
    pub missing: MissingStyle,
    /// Opacity of metric guides, 0 to 1.
    pub guide_opacity: f32,
}

/// The bundled syntect themes, loaded once per proof.
pub struct Themes(ThemeSet);

impl Themes {
    pub fn load() -> Self {
        Self(ThemeSet::load_defaults())
    }

    /// Returns the theme named `name`, ignoring ASCII case.
    ///
    /// # Errors
    ///
    /// Returns a diagnostic listing every bundled theme when none matches.
    pub fn get(&self, name: &str) -> Result<&Theme> {
        let exact = self.0.themes.get(name);
        let folded = || {
            self.0
                .themes
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, theme)| theme)
        };
        exact.or_else(folded).ok_or_else(|| {
            miette!(
                "unknown theme {name:?}; available themes: {}",
                self.names().join(", ")
            )
        })
    }

    /// Bundled theme names in sorted order.
    pub fn names(&self) -> Vec<&str> {
        self.0.themes.keys().map(String::as_str).collect()
    }
}

/// The background and default foreground a theme declares, with black on
/// white for whichever it leaves out.
pub fn theme_colors(theme: &Theme) -> (Rgb, Rgb) {
    let background = theme
        .settings
        .background
        .map_or(Rgb::WHITE, Rgb::from_syntect);
    let foreground = theme
        .settings
        .foreground
        .map_or(Rgb::BLACK, Rgb::from_syntect);
    (background, foreground)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_short_and_long_hex() {
        assert_eq!(Rgb::parse("#1e1e1e").unwrap(), Rgb([30, 30, 30]));
        assert_eq!(Rgb::parse("fa0").unwrap(), Rgb([255, 170, 0]));
        assert!(Rgb::parse("#12345").is_err());
        assert!(Rgb::parse("#zzzzzz").is_err());
        assert_eq!(Rgb([1, 2, 255]).to_string(), "#0102ff");
    }
}
