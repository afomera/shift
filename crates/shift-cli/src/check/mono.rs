//! Monospace advance rules.

use serde_json::json;

use super::rules::AdvanceSample;
use super::types::{Finding, FindingCode, MonoSourceSummary, WidthCount};

/// Advances closer than this, in font units, count as equal.
const WIDTH_TOLERANCE: f64 = 1e-6;

/// Compares one source's measured advances with the expected cell width.
///
/// Without `width`, the expected width is the most common advance; ties pick
/// the narrowest width so the result is deterministic. Ink wider than its own
/// advance is reported separately because it cannot fit a monospace cell.
pub fn check_source(
    source: &str,
    samples: &[AdvanceSample],
    width: Option<f64>,
    findings: &mut Vec<Finding>,
) -> MonoSourceSummary {
    let widths = width_histogram(samples);
    let expected = width.or_else(|| widths.first().map(|entry| entry.width));
    let origin = if width.is_some() {
        "option"
    } else {
        "mostCommon"
    };

    let mut mismatch_count = 0;
    for sample in samples {
        if let Some(expected) = expected
            && (sample.advance - expected).abs() > WIDTH_TOLERANCE
        {
            mismatch_count += 1;
            findings.push(
                Finding::new(
                    FindingCode::MonoWidthMismatch,
                    format!(
                        "advance {} differs from cell width {expected}",
                        sample.advance
                    ),
                    json!({
                        "advance": sample.advance,
                        "expected": expected,
                        "expectedOrigin": origin,
                        "delta": sample.advance - expected,
                    }),
                )
                .glyph(&sample.glyph)
                .source(source),
            );
        }

        let Some(ink) = sample.ink else {
            continue;
        };
        let ink_width = ink.max_x - ink.min_x;
        if ink_width > sample.advance + WIDTH_TOLERANCE {
            findings.push(
                Finding::new(
                    FindingCode::MonoInkOverflow,
                    format!(
                        "ink is {ink_width} wide but the advance is {}",
                        sample.advance
                    ),
                    json!({
                        "inkWidth": ink_width,
                        "advance": sample.advance,
                        "leftSidebearing": ink.min_x,
                        "rightSidebearing": sample.advance - ink.max_x,
                    }),
                )
                .glyph(&sample.glyph)
                .source(source),
            );
        }
    }

    MonoSourceSummary {
        source: source.to_string(),
        expected_width: expected,
        expected_width_origin: origin.to_string(),
        measured_glyph_count: samples.len(),
        mismatch_count,
        widths,
    }
}

fn width_histogram(samples: &[AdvanceSample]) -> Vec<WidthCount> {
    let mut widths: Vec<WidthCount> = Vec::new();
    for sample in samples {
        match widths
            .iter_mut()
            .find(|entry| (entry.width - sample.advance).abs() <= WIDTH_TOLERANCE)
        {
            Some(entry) => entry.count += 1,
            None => widths.push(WidthCount {
                width: sample.advance,
                count: 1,
            }),
        }
    }
    widths.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then(left.width.total_cmp(&right.width))
    });
    widths
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(glyph: &str, advance: f64) -> AdvanceSample {
        AdvanceSample {
            glyph: glyph.to_string(),
            advance,
            ink: None,
        }
    }

    #[test]
    fn most_common_width_is_expected_and_ties_pick_the_narrowest() {
        let mut findings = Vec::new();
        let samples = [sample("a", 600.0), sample("b", 500.0)];

        let summary = check_source("Regular", &samples, None, &mut findings);

        assert_eq!(summary.expected_width, Some(500.0));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].glyph.as_deref(), Some("a"));
    }
}
