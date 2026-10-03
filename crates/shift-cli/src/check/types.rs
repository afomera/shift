use serde::Serialize;
use serde_json::Value;

/// How strongly a finding blocks a font from being considered healthy.
///
/// Only [`Severity::Error`] findings make `check` exit unsuccessfully.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

/// Stable identifier for one kind of finding.
///
/// Each code serializes as a kebab-case string and always carries the same
/// [`Severity`], so scripts may match on the code alone.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FindingCode {
    MonoWidthMismatch,
    MonoInkOverflow,
    CharsetMissingGlyph,
    CharsetMissingLayer,
    EmptyOutline,
    OpenContour,
    DegenerateContour,
    NonFinitePoint,
    InvalidAdvance,
    OutlineOutOfBounds,
    ContourDirection,
    IncompatibleMasters,
    MissingMasterLayer,
    UnresolvedGlyph,
}

impl FindingCode {
    pub fn severity(self) -> Severity {
        match self {
            Self::MonoWidthMismatch
            | Self::CharsetMissingGlyph
            | Self::CharsetMissingLayer
            | Self::EmptyOutline
            | Self::OpenContour
            | Self::DegenerateContour
            | Self::NonFinitePoint
            | Self::InvalidAdvance
            | Self::IncompatibleMasters
            | Self::UnresolvedGlyph => Severity::Error,
            Self::MonoInkOverflow | Self::OutlineOutOfBounds | Self::ContourDirection => {
                Severity::Warning
            }
            Self::MissingMasterLayer => Severity::Info,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::MonoWidthMismatch => "mono-width-mismatch",
            Self::MonoInkOverflow => "mono-ink-overflow",
            Self::CharsetMissingGlyph => "charset-missing-glyph",
            Self::CharsetMissingLayer => "charset-missing-layer",
            Self::EmptyOutline => "empty-outline",
            Self::OpenContour => "open-contour",
            Self::DegenerateContour => "degenerate-contour",
            Self::NonFinitePoint => "non-finite-point",
            Self::InvalidAdvance => "invalid-advance",
            Self::OutlineOutOfBounds => "outline-out-of-bounds",
            Self::ContourDirection => "contour-direction",
            Self::IncompatibleMasters => "incompatible-masters",
            Self::MissingMasterLayer => "missing-master-layer",
            Self::UnresolvedGlyph => "unresolved-glyph",
        }
    }
}

/// One objective health observation about a font, glyph, or glyph layer.
///
/// `glyph` and `source` are names usable as CLI selectors. `details` is a JSON
/// object whose keys are stable per [`FindingCode`]; values are in font units.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub severity: Severity,
    pub code: FindingCode,
    pub glyph: Option<String>,
    pub source: Option<String>,
    pub message: String,
    pub details: Value,
}

impl Finding {
    pub fn new(code: FindingCode, message: impl Into<String>, details: Value) -> Self {
        Self {
            severity: code.severity(),
            code,
            glyph: None,
            source: None,
            message: message.into(),
            details,
        }
    }

    pub fn glyph(mut self, name: &str) -> Self {
        self.glyph = Some(name.to_string());
        self
    }

    pub fn source(mut self, name: &str) -> Self {
        self.source = Some(name.to_string());
        self
    }
}

/// Complete machine-readable result of `shift-cli check`.
///
/// Findings are ordered by severity, then by discovery order: glyph rules in
/// glyph and source order, then monospace findings, then charset findings.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckReport {
    pub path: String,
    pub format: String,
    /// `true` when no error-severity finding exists.
    pub passed: bool,
    pub options: CheckOptionsSummary,
    pub counts: SeverityCounts,
    /// Master sources whose layers were checked, in source order.
    pub sources: Vec<String>,
    pub glyph_count: usize,
    pub mono: Vec<MonoSourceSummary>,
    pub charset: Option<CharsetSummary>,
    pub findings: Vec<Finding>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckOptionsSummary {
    pub mono: bool,
    pub width: Option<f64>,
    pub charset: Option<String>,
    pub source: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeverityCounts {
    pub error: usize,
    pub warning: usize,
    pub info: usize,
    pub total: usize,
}

impl SeverityCounts {
    pub fn from_findings(findings: &[Finding]) -> Self {
        let mut counts = Self::default();
        for finding in findings {
            match finding.severity {
                Severity::Error => counts.error += 1,
                Severity::Warning => counts.warning += 1,
                Severity::Info => counts.info += 1,
            }
        }
        counts.total = findings.len();
        counts
    }
}

/// Monospace advance analysis for one master source.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonoSourceSummary {
    pub source: String,
    /// Expected advance in font units, or `None` when no glyph was measured.
    pub expected_width: Option<f64>,
    /// `option` when supplied by `--width`, otherwise `mostCommon`.
    pub expected_width_origin: String,
    pub measured_glyph_count: usize,
    pub mismatch_count: usize,
    /// Distinct measured advances, most common first.
    pub widths: Vec<WidthCount>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WidthCount {
    pub width: f64,
    pub count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CharsetSummary {
    pub name: String,
    pub character_count: usize,
    pub missing_glyph_count: usize,
    pub missing_layer_count: usize,
}
