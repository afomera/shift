use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

/// Counter-clockwise (PostScript direction) rectangle spanning `x0..x1`, `y0..y1`.
fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64) -> Value {
    json!({
        "closed": true,
        "points": [
            {"x": x0, "y": y0},
            {"x": x1, "y": y0},
            {"x": x1, "y": y1},
            {"x": x0, "y": y1}
        ]
    })
}

fn shift(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_shift-cli"))
        .args(args)
        .output()
        .expect("shift CLI should run")
}

fn shift_with_stdin(args: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_shift-cli"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("shift CLI should run");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

/// A font authored entirely through the public CLI.
struct Lab {
    _temp: tempfile::TempDir,
    path: PathBuf,
}

impl Lab {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("Lab.shift");
        let lab = Self { _temp: temp, path };
        lab.run(&["font", "create", lab.path()]);
        lab
    }

    fn path(&self) -> &str {
        self.path.to_str().unwrap()
    }

    fn dir(&self) -> &Path {
        self.path.parent().unwrap()
    }

    fn run(&self, args: &[&str]) {
        let output = shift(args);
        assert!(
            output.status.success(),
            "{args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn glyph(&self, name: &str, unicode: Option<&str>) {
        let mut args = vec!["glyph", "add", self.path(), name];
        if let Some(unicode) = unicode {
            args.extend(["--unicode", unicode]);
        }
        self.run(&args);
    }

    fn layer(&self, glyph: &str, source: &str, payload: Value) {
        let output = shift_with_stdin(
            &[
                "layer",
                "add",
                self.path(),
                "--glyph",
                glyph,
                "--source",
                source,
                "--input",
                "-",
            ],
            &payload.to_string(),
        );
        assert!(
            output.status.success(),
            "layer {glyph}@{source} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn drawn(&self, name: &str, unicode: &str, advance: f64) {
        self.glyph(name, Some(unicode));
        self.layer(
            name,
            "Regular",
            json!({
                "advance": advance,
                "contours": [rectangle(50.0, 0.0, advance - 50.0, 700.0)]
            }),
        );
    }

    fn check(&self, extra: &[&str]) -> (Option<i32>, Value) {
        let mut args = vec!["check", self.path(), "--json"];
        args.extend(extra);
        let output = shift(&args);
        let report = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "check output is not JSON ({error}): {}",
                String::from_utf8_lossy(&output.stdout)
            )
        });
        (output.status.code(), report)
    }
}

fn findings<'a>(report: &'a Value, code: &str) -> Vec<&'a Value> {
    report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| finding["code"] == code)
        .collect()
}

/// A monospace Latin seed: two drawn letters, a space, and a zero-width mark.
fn mono_lab() -> Lab {
    let lab = Lab::new();
    lab.drawn("A", "U+0041", 600.0);
    lab.drawn("H", "U+0048", 600.0);
    lab.glyph("space", Some("U+0020"));
    lab.layer("space", "Regular", json!({ "advance": 600 }));
    lab.glyph("acutecomb", Some("U+0301"));
    lab.layer(
        "acutecomb",
        "Regular",
        json!({ "advance": 0, "contours": [rectangle(-200.0, 720.0, -100.0, 800.0)] }),
    );
    lab
}

#[test]
fn healthy_monospace_font_passes_and_excludes_zero_width_marks() {
    let lab = mono_lab();

    let (code, report) = lab.check(&["--mono"]);

    assert_eq!(code, Some(0), "{report:#}");
    assert_eq!(report["passed"], true);
    assert_eq!(report["counts"]["error"], 0);
    assert_eq!(report["mono"][0]["source"], "Regular");
    assert_eq!(report["mono"][0]["expectedWidth"], 600.0);
    assert_eq!(report["mono"][0]["expectedWidthOrigin"], "mostCommon");
    assert_eq!(report["mono"][0]["measuredGlyphCount"], 3);
    assert_eq!(report["mono"][0]["mismatchCount"], 0);
    assert!(findings(&report, "invalid-advance").is_empty());
    assert!(findings(&report, "empty-outline").is_empty());
}

#[test]
fn mismatched_monospace_advance_is_an_error_with_both_widths() {
    let lab = mono_lab();
    lab.drawn("W", "U+0057", 620.0);

    let (code, report) = lab.check(&["--mono"]);

    assert_eq!(code, Some(1));
    assert_eq!(report["passed"], false);
    let mismatches = findings(&report, "mono-width-mismatch");
    assert_eq!(mismatches.len(), 1);
    assert_eq!(mismatches[0]["severity"], "error");
    assert_eq!(mismatches[0]["glyph"], "W");
    assert_eq!(mismatches[0]["source"], "Regular");
    assert_eq!(mismatches[0]["details"]["advance"], 620.0);
    assert_eq!(mismatches[0]["details"]["expected"], 600.0);
    assert_eq!(
        report["mono"][0]["widths"][0],
        json!({"width": 600.0, "count": 3})
    );
    assert_eq!(
        report["mono"][0]["widths"][1],
        json!({"width": 620.0, "count": 1})
    );
}

#[test]
fn explicit_monospace_width_overrides_the_most_common_advance() {
    let lab = mono_lab();
    lab.drawn("W", "U+0057", 620.0);

    let (code, report) = lab.check(&["--mono", "--width", "620"]);

    assert_eq!(code, Some(1));
    assert_eq!(report["mono"][0]["expectedWidthOrigin"], "option");
    let mut flagged = findings(&report, "mono-width-mismatch")
        .iter()
        .map(|finding| finding["glyph"].as_str().unwrap())
        .collect::<Vec<_>>();
    flagged.sort_unstable();
    assert_eq!(flagged, vec!["A", "H", "space"]);
}

#[test]
fn ink_wider_than_a_monospace_cell_is_a_warning() {
    let lab = mono_lab();
    lab.glyph("M", Some("U+004D"));
    lab.layer(
        "M",
        "Regular",
        json!({ "advance": 600, "contours": [rectangle(-20.0, 0.0, 640.0, 700.0)] }),
    );

    let (code, report) = lab.check(&["--mono"]);

    assert_eq!(code, Some(0), "warnings must not fail: {report:#}");
    let overflow = findings(&report, "mono-ink-overflow");
    assert_eq!(overflow.len(), 1);
    assert_eq!(overflow[0]["severity"], "warning");
    assert_eq!(overflow[0]["glyph"], "M");
    assert_eq!(overflow[0]["details"]["inkWidth"], 660.0);
    assert_eq!(overflow[0]["details"]["leftSidebearing"], -20.0);
}

#[test]
fn charset_reports_unmapped_characters_and_mapped_glyphs_without_layers() {
    let lab = Lab::new();
    lab.drawn("A", "U+0041", 600.0);
    lab.glyph("B", Some("U+0042"));
    let charset = lab.dir().join("required.txt");
    std::fs::write(&charset, "# required\nAB\nU+0043\n").unwrap();

    let (code, report) = lab.check(&["--charset", charset.to_str().unwrap()]);

    assert_eq!(code, Some(1));
    assert_eq!(report["charset"]["characterCount"], 3);
    assert_eq!(report["charset"]["missingGlyphCount"], 1);
    assert_eq!(report["charset"]["missingLayerCount"], 1);
    let unmapped = findings(&report, "charset-missing-glyph");
    assert_eq!(unmapped.len(), 1);
    assert_eq!(unmapped[0]["glyph"], Value::Null);
    assert_eq!(unmapped[0]["details"]["codepoint"], "U+0043");
    assert_eq!(unmapped[0]["details"]["character"], "C");
    let layerless = findings(&report, "charset-missing-layer");
    assert_eq!(layerless.len(), 1);
    assert_eq!(layerless[0]["glyph"], "B");
    assert_eq!(layerless[0]["source"], "Regular");
    assert!(
        findings(&report, "empty-outline").is_empty(),
        "charset findings supersede the generic missing-layer finding"
    );
}

#[test]
fn built_in_ascii_charset_lists_every_missing_printable_character() {
    let lab = Lab::new();
    lab.drawn("A", "U+0041", 600.0);

    let (code, report) = lab.check(&["--charset", "ascii"]);

    assert_eq!(code, Some(1));
    assert_eq!(report["charset"]["name"], "ascii");
    assert_eq!(report["charset"]["characterCount"], 95);
    assert_eq!(report["charset"]["missingGlyphCount"], 94);
    assert_eq!(findings(&report, "charset-missing-glyph").len(), 94);
}

#[test]
fn open_and_degenerate_contours_are_errors() {
    let lab = Lab::new();
    lab.glyph("O", Some("U+004F"));
    lab.layer(
        "O",
        "Regular",
        json!({
            "advance": 600,
            "contours": [
                rectangle(50.0, 0.0, 550.0, 700.0),
                {"closed": false, "points": [{"x": 100, "y": 100}, {"x": 200, "y": 200}, {"x": 300, "y": 100}]},
                {"closed": true, "points": [{"x": 100, "y": 100}, {"x": 200, "y": 200}]}
            ]
        }),
    );

    let (code, report) = lab.check(&[]);

    assert_eq!(code, Some(1));
    let open = findings(&report, "open-contour");
    assert_eq!(open.len(), 1);
    assert_eq!(open[0]["glyph"], "O");
    assert_eq!(open[0]["details"]["contour"], 1);
    let degenerate = findings(&report, "degenerate-contour");
    assert_eq!(degenerate.len(), 1);
    assert_eq!(
        degenerate[0]["details"],
        json!({"contour": 2, "pointCount": 2})
    );
}

#[test]
fn outline_and_advance_rules_flag_empty_reversed_and_out_of_bounds_glyphs() {
    let lab = Lab::new();
    lab.glyph("E", Some("U+0045"));
    lab.layer("E", "Regular", json!({ "advance": 600 }));
    lab.glyph("Z", Some("U+005A"));
    lab.layer(
        "Z",
        "Regular",
        json!({ "advance": 0, "contours": [rectangle(50.0, 0.0, 550.0, 700.0)] }),
    );
    lab.glyph("T", Some("U+0054"));
    lab.layer(
        "T",
        "Regular",
        json!({ "advance": 600, "contours": [rectangle(50.0, 0.0, 550.0, 1200.0)] }),
    );
    lab.glyph("R", Some("U+0052"));
    lab.layer(
        "R",
        "Regular",
        json!({
            "advance": 600,
            "contours": [{"closed": true, "points": [
                {"x": 50, "y": 0}, {"x": 50, "y": 700}, {"x": 550, "y": 700}, {"x": 550, "y": 0}
            ]}]
        }),
    );

    let (code, report) = lab.check(&[]);

    assert_eq!(code, Some(1));
    let empty = findings(&report, "empty-outline");
    assert_eq!(empty.len(), 1);
    assert_eq!(empty[0]["glyph"], "E");
    assert_eq!(empty[0]["details"]["reason"], "noContours");
    let advance = findings(&report, "invalid-advance");
    assert_eq!(advance.len(), 1);
    assert_eq!(advance[0]["glyph"], "Z");
    assert_eq!(advance[0]["details"]["reason"], "zero");
    let bounds = findings(&report, "outline-out-of-bounds");
    assert_eq!(bounds.len(), 1);
    assert_eq!(bounds[0]["severity"], "warning");
    assert_eq!(bounds[0]["glyph"], "T");
    assert_eq!(bounds[0]["details"]["maxY"], 1200.0);
    assert_eq!(bounds[0]["details"]["ascender"], 800.0);
    let direction = findings(&report, "contour-direction");
    assert_eq!(direction.len(), 1);
    assert_eq!(direction[0]["glyph"], "R");
    assert_eq!(direction[0]["details"]["allReversed"], true);
}

#[test]
fn incompatible_masters_are_errors_with_structural_differences() {
    let lab = Lab::new();
    lab.run(&[
        "axis",
        "add",
        lab.path(),
        "--tag",
        "wght",
        "--name",
        "Weight",
        "--min",
        "100",
        "--default",
        "400",
        "--max",
        "900",
    ]);
    lab.run(&[
        "source",
        "add",
        lab.path(),
        "--name",
        "Bold",
        "--location",
        "wght=900",
    ]);
    lab.drawn("A", "U+0041", 600.0);
    lab.glyph("H", Some("U+0048"));
    lab.layer(
        "H",
        "Regular",
        json!({ "advance": 600, "contours": [rectangle(50.0, 0.0, 550.0, 700.0)] }),
    );
    lab.layer(
        "H",
        "Bold",
        json!({ "advance": 640, "contours": [
            {"closed": true, "points": [
                {"x": 40, "y": 0}, {"x": 600, "y": 0}, {"x": 600, "y": 700}, {"x": 320, "y": 750}, {"x": 40, "y": 700}
            ]}
        ]}),
    );

    let (code, report) = lab.check(&[]);

    assert_eq!(code, Some(1));
    assert_eq!(report["sources"], json!(["Regular", "Bold"]));
    let incompatible = findings(&report, "incompatible-masters");
    assert_eq!(incompatible.len(), 1);
    assert_eq!(incompatible[0]["glyph"], "H");
    assert_eq!(incompatible[0]["source"], "Bold");
    assert_eq!(incompatible[0]["details"]["referenceSource"], "Regular");
    assert_eq!(
        incompatible[0]["details"]["differences"],
        json!(["path 0 node count: reference 4, source 5"])
    );
    let sparse = findings(&report, "missing-master-layer");
    assert_eq!(sparse.len(), 1);
    assert_eq!(sparse[0]["severity"], "info");
    assert_eq!(sparse[0]["glyph"], "A");
    assert_eq!(sparse[0]["source"], "Bold");

    let (_, regular_only) = lab.check(&["--source", "Regular"]);
    assert_eq!(regular_only["sources"], json!(["Regular"]));
    assert!(findings(&regular_only, "incompatible-masters").is_empty());
}

#[test]
fn json_report_has_a_stable_shape_and_severity_counts() {
    let lab = mono_lab();
    lab.drawn("W", "U+0057", 620.0);

    let (_, report) = lab.check(&["--mono", "--charset", "ascii"]);

    let mut keys = report
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec![
            "charset",
            "counts",
            "findings",
            "format",
            "glyphCount",
            "mono",
            "options",
            "passed",
            "path",
            "sources",
        ]
    );
    assert_eq!(
        report["options"],
        json!({"mono": true, "width": null, "charset": "ascii", "source": null})
    );
    assert_eq!(report["format"], "shift");
    assert_eq!(report["glyphCount"], 5);
    let all = report["findings"].as_array().unwrap();
    let mut finding_keys = all[0]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    finding_keys.sort_unstable();
    assert_eq!(
        finding_keys,
        vec!["code", "details", "glyph", "message", "severity", "source"]
    );
    let errors = all.iter().filter(|f| f["severity"] == "error").count();
    let warnings = all.iter().filter(|f| f["severity"] == "warning").count();
    let info = all.iter().filter(|f| f["severity"] == "info").count();
    assert_eq!(report["counts"]["error"], errors);
    assert_eq!(report["counts"]["warning"], warnings);
    assert_eq!(report["counts"]["info"], info);
    assert_eq!(report["counts"]["total"], all.len());
    assert_eq!(
        errors,
        1 + 91,
        "one width mismatch and 91 unmapped ascii characters"
    );
    let severities = all
        .iter()
        .map(|finding| finding["severity"].as_str().unwrap())
        .collect::<Vec<_>>();
    let mut sorted = severities.clone();
    sorted.sort_by_key(|severity| {
        ["error", "warning", "info"]
            .iter()
            .position(|s| s == severity)
    });
    assert_eq!(severities, sorted, "findings are ordered by severity");
}

#[test]
fn human_output_groups_findings_by_severity_and_code() {
    let lab = mono_lab();
    lab.drawn("W", "U+0057", 620.0);

    let output = shift(&["check", lab.path(), "--mono"]);

    assert_eq!(output.status.code(), Some(1));
    let text = String::from_utf8(output.stdout).unwrap();
    let lines = text.lines().collect::<Vec<_>>();
    assert_eq!(lines[0], "Lab.shift  FAIL  1 errors, 0 warnings, 0 info");
    assert!(text.contains("mono        Regular: cell 600 (most common), 1 of 4 glyphs differ"));
    assert!(text.contains("error mono-width-mismatch (1)"));
    assert!(text.contains("  W @ Regular: advance 620 differs from cell width 600"));
}

#[test]
fn unreadable_input_and_unknown_sources_exit_with_status_two() {
    let lab = mono_lab();
    let missing = lab.dir().join("Missing.shift");

    let unreadable = shift(&["check", missing.to_str().unwrap()]);
    let unknown_source = shift(&["check", lab.path(), "--source", "Nope", "--json"]);
    let missing_charset = shift(&["check", lab.path(), "--charset", "no-such-charset.txt"]);

    assert_eq!(unreadable.status.code(), Some(2));
    assert_eq!(unknown_source.status.code(), Some(2));
    let error: Value = serde_json::from_slice(&unknown_source.stdout).unwrap();
    assert_eq!(error["valid"], false);
    assert!(
        error["error"]["summary"]
            .as_str()
            .unwrap()
            .contains("\"Nope\"")
    );
    assert_eq!(missing_charset.status.code(), Some(2));
}

#[test]
fn check_never_modifies_the_document() {
    let lab = mono_lab();
    lab.drawn("W", "U+0057", 620.0);
    let before = std::fs::read(&lab.path).unwrap();

    let (code, _) = lab.check(&["--mono", "--charset", "latin1"]);

    assert_eq!(code, Some(1));
    assert_eq!(std::fs::read(&lab.path).unwrap(), before);
}
