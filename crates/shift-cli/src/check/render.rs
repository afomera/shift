use std::collections::BTreeMap;

use crate::inspect::RenderMode;

use super::types::{CheckReport, Finding, FindingCode, Severity};

/// Findings printed per code before the human summary elides the rest.
const FINDINGS_PER_CODE: usize = 12;

impl CheckReport {
    /// Renders a compact summary grouped by severity and code.
    ///
    /// Long groups are truncated; `--json` always carries every finding.
    pub fn render(&self, _mode: RenderMode) -> String {
        let mut lines = vec![format!(
            "{}  {}  {} errors, {} warnings, {} info",
            file_name(&self.path),
            if self.passed { "PASS" } else { "FAIL" },
            self.counts.error,
            self.counts.warning,
            self.counts.info
        )];
        lines.push(format!(
            "checked     {} glyphs on {}",
            self.glyph_count,
            self.sources.join(", ")
        ));
        for mono in &self.mono {
            let expected = mono
                .expected_width
                .map_or_else(|| "-".to_string(), format_number);
            let origin = if mono.expected_width_origin == "option" {
                "--width"
            } else {
                "most common"
            };
            lines.push(format!(
                "mono        {}: cell {expected} ({origin}), {} of {} glyphs differ",
                mono.source, mono.mismatch_count, mono.measured_glyph_count
            ));
        }
        if let Some(charset) = &self.charset {
            lines.push(format!(
                "charset     {}: {} characters, {} unmapped, {} without a layer",
                charset.name,
                charset.character_count,
                charset.missing_glyph_count,
                charset.missing_layer_count
            ));
        }

        let mut groups: BTreeMap<(Severity, FindingCode), Vec<&Finding>> = BTreeMap::new();
        for finding in &self.findings {
            groups
                .entry((finding.severity, finding.code))
                .or_default()
                .push(finding);
        }
        for ((severity, code), findings) in groups {
            lines.push(String::new());
            lines.push(format!(
                "{} {} ({})",
                severity.as_str(),
                code.as_str(),
                findings.len()
            ));
            for finding in findings.iter().take(FINDINGS_PER_CODE) {
                lines.push(format!("  {}{}", subject(finding), finding.message));
            }
            if findings.len() > FINDINGS_PER_CODE {
                lines.push(format!(
                    "  ... {} more (use --json for all)",
                    findings.len() - FINDINGS_PER_CODE
                ));
            }
        }

        lines.join("\n")
    }
}

fn subject(finding: &Finding) -> String {
    match (&finding.glyph, &finding.source) {
        (Some(glyph), Some(source)) => format!("{glyph} @ {source}: "),
        (Some(glyph), None) => format!("{glyph}: "),
        (None, Some(source)) => format!("@ {source}: "),
        (None, None) => String::new(),
    }
}

fn file_name(path: &str) -> &str {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
}

fn format_number(value: f64) -> String {
    if value.fract().abs() < f64::EPSILON {
        return format!("{value:.0}");
    }
    format!("{value}")
}
