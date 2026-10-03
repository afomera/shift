//! Unicode-derived glyph roles that relax advance and outline expectations.
//!
//! Encoded glyphs are classified from the Unicode general category of their
//! code points. Unencoded glyphs fall back to name heuristics: a `uniXXXX` or
//! `uXXXX[X]` name is classified like that code point, and a name whose base
//! (the part before the first `.`) ends in `comb` is a combining mark.

use icu_properties::CodePointMapData;
use icu_properties::props::GeneralCategory;
use shift_font::Glyph;

/// Advance and outline expectations for one glyph.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GlyphRole {
    /// Nonspacing or enclosing combining mark (`Mn`, `Me`): zero advance is
    /// expected and monospace width checks skip it.
    pub mark: bool,
    /// Separator, control, or format character (`Zs`, `Zl`, `Zp`, `Cc`, `Cf`):
    /// an empty outline is expected.
    pub space_like: bool,
    /// Control or format character (`Cc`, `Cf`), or a `.null`/`NULL` glyph:
    /// zero advance is acceptable.
    pub zero_width_allowed: bool,
}

impl GlyphRole {
    pub fn of(glyph: &Glyph) -> Self {
        let codepoints = if glyph.unicodes().is_empty() {
            codepoint_from_name(glyph.name()).into_iter().collect()
        } else {
            glyph.unicodes().to_vec()
        };
        let categories = codepoints
            .iter()
            .filter_map(|codepoint| char::from_u32(*codepoint))
            .map(|character| CodePointMapData::<GeneralCategory>::new().get(character))
            .collect::<Vec<_>>();
        let any =
            |predicate: fn(GeneralCategory) -> bool| categories.iter().copied().any(predicate);
        let all = |predicate: fn(GeneralCategory) -> bool| {
            !categories.is_empty() && categories.iter().copied().all(predicate)
        };

        let mark = any(is_mark) || (categories.is_empty() && has_comb_name(glyph.name()));
        let null_name = matches!(glyph.name(), ".null" | "NULL");

        Self {
            mark,
            space_like: all(is_space_like) || null_name,
            zero_width_allowed: mark || all(is_zero_width_control) || null_name,
        }
    }
}

fn is_mark(category: GeneralCategory) -> bool {
    matches!(
        category,
        GeneralCategory::NonspacingMark | GeneralCategory::EnclosingMark
    )
}

fn is_space_like(category: GeneralCategory) -> bool {
    matches!(
        category,
        GeneralCategory::SpaceSeparator
            | GeneralCategory::LineSeparator
            | GeneralCategory::ParagraphSeparator
            | GeneralCategory::Control
            | GeneralCategory::Format
    )
}

fn is_zero_width_control(category: GeneralCategory) -> bool {
    matches!(category, GeneralCategory::Control | GeneralCategory::Format)
}

fn base_name(name: &str) -> &str {
    name.split('.').next().unwrap_or(name)
}

fn has_comb_name(name: &str) -> bool {
    base_name(name).ends_with("comb")
}

fn codepoint_from_name(name: &str) -> Option<u32> {
    let base = base_name(name);
    let hex = match base.strip_prefix("uni") {
        Some(hex) if hex.len() == 4 => hex,
        _ => base
            .strip_prefix('u')
            .filter(|hex| (4..=6).contains(&hex.len()))?,
    };
    u32::from_str_radix(hex, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn role(name: &str, unicodes: &[u32]) -> GlyphRole {
        let mut glyph = Glyph::new(name);
        glyph.set_unicodes(unicodes.to_vec());
        GlyphRole::of(&glyph)
    }

    #[test]
    fn combining_marks_are_detected_by_category_and_name() {
        assert!(role("acutecomb", &[0x0301]).mark);
        assert!(role("gravecomb.case", &[]).mark);
        assert!(role("uni0308", &[]).mark);
        assert!(!role("acute", &[0x00B4]).mark);
        assert!(!role("A", &[0x41]).mark);
    }

    #[test]
    fn spaces_and_format_characters_relax_outline_and_advance_rules() {
        let space = role("space", &[0x20]);
        let zwj = role("uni200D", &[0x200D]);

        assert!(space.space_like);
        assert!(!space.zero_width_allowed);
        assert!(zwj.space_like);
        assert!(zwj.zero_width_allowed);
        assert!(role(".null", &[]).zero_width_allowed);
    }
}
