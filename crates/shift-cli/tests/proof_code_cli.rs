use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::Value;
use tiny_skia::Pixmap;

/// Code size used by these tests: one 600-unit cell is 30px and each box
/// glyph's ink spans x 5..25px and y 0..35px above the baseline.
const SIZE: &str = "50";
const CELL: f64 = 30.0;

/// base16-ocean.dark, the default code theme.
const OCEAN_BACKGROUND: [u8; 3] = [0x2b, 0x30, 0x3b];
const OCEAN_FOREGROUND: [u8; 3] = [0xc0, 0xc5, 0xce];
const OCEAN_KEYWORD: [u8; 3] = [0xb4, 0x8e, 0xad];
const OCEAN_NUMBER: [u8; 3] = [0xd0, 0x87, 0x70];
const MARKER: [u8; 3] = [0xff, 0x00, 0xff];

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

/// Authors a monospace font whose every glyph is a solid box in a 600-unit
/// cell: `l e t x =`, the digits, and an inkless space.
fn create_code_font(path: &Path) {
    let path = path.to_str().unwrap();
    assert!(shift(&["font", "create", path]).status.success());

    let boxed = "M 100 0 L 500 0 L 500 700 L 100 700 Z";
    let mut glyphs = vec![serde_json::json!({
        "name": "space", "unicodes": ["U+0020"], "layer": {"advance": 600}
    })];
    let named = [
        ("l", 'l'),
        ("e", 'e'),
        ("t", 't'),
        ("x", 'x'),
        ("equal", '='),
    ];
    let digits = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
    ];
    let all = named.into_iter().chain(digits.into_iter().zip('0'..='9'));
    for (name, character) in all {
        glyphs.push(serde_json::json!({
            "name": name,
            "unicodes": [format!("U+{:04X}", u32::from(character))],
            "layer": {"advance": 600, "svgPath": boxed}
        }));
    }
    let payload = serde_json::json!({ "glyphs": glyphs });
    let set = shift_with_stdin(
        &["glyph", "set", path, "--input", "-"],
        &payload.to_string(),
    );
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );
}

fn proof_json(args: &[&str]) -> Value {
    let output = shift(&[&["proof"], args, &["--json"]].concat());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn rgb(pixmap: &Pixmap, x: f64, y: f64) -> [u8; 3] {
    let pixel = pixmap.pixel(x as u32, y as u32).unwrap();
    [pixel.red(), pixel.green(), pixel.blue()]
}

/// Color at the middle of the ink of the `column`th cell of `row`.
fn cell_ink(pixmap: &Pixmap, row: &Value, column: usize) -> [u8; 3] {
    let x = row["x"].as_f64().unwrap() + column as f64 * CELL + CELL / 2.0;
    let y = row["baseline"].as_f64().unwrap() - 17.0;
    rgb(pixmap, x, y)
}

fn count_pixels(pixmap: &Pixmap, color: [u8; 3]) -> usize {
    pixmap
        .pixels()
        .iter()
        .filter(|pixel| [pixel.red(), pixel.green(), pixel.blue()] == color)
        .count()
}

struct Fixture {
    _temp: tempfile::TempDir,
    font: String,
    dir: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let font = temp.path().join("Code.shift");
        create_code_font(&font);
        Self {
            dir: temp.path().to_path_buf(),
            font: font.to_str().unwrap().to_string(),
            _temp: temp,
        }
    }

    fn path(&self, name: &str) -> String {
        self.dir.join(name).to_str().unwrap().to_string()
    }

    fn write(&self, name: &str, contents: &str) -> String {
        let path = self.path(name);
        std::fs::write(&path, contents).unwrap();
        path
    }
}

#[test]
fn colors_code_tokens_from_the_theme() {
    let fixture = Fixture::new();
    let code = fixture.write("main.rs", "let x = 1\n");
    let image = fixture.path("code.png");

    let report = proof_json(&[
        &fixture.font,
        "-o",
        &image,
        "--code",
        &code,
        "--sizes",
        SIZE,
    ]);

    assert_eq!(report["mode"], "code");
    assert_eq!(report["language"], "Rust");
    assert_eq!(report["theme"], "base16-ocean.dark");
    assert_eq!(report["background"], "#2b303b");
    assert_eq!(report["foreground"], "#c0c5ce");
    assert_eq!(report["lineCount"], 1);
    assert_eq!(report["tabWidth"], 4);
    assert_eq!(report["missingCharacters"], serde_json::json!([]));
    let pixmap = Pixmap::load_png(&image).unwrap();
    let row = &report["rows"][0];
    assert_eq!(row["width"], 9.0 * CELL);
    assert_eq!(rgb(&pixmap, 2.0, 2.0), OCEAN_BACKGROUND);
    assert_eq!(cell_ink(&pixmap, row, 0), OCEAN_KEYWORD, "`let` keyword");
    assert_eq!(
        cell_ink(&pixmap, row, 4),
        OCEAN_FOREGROUND,
        "`x` identifier"
    );
    assert_eq!(cell_ink(&pixmap, row, 8), OCEAN_NUMBER, "`1` literal");
    assert_eq!(cell_ink(&pixmap, row, 3), OCEAN_BACKGROUND, "space");
}

#[test]
fn draws_spans_in_exactly_the_given_colors() {
    let fixture = Fixture::new();
    let spans = serde_json::json!({
        "background": "#000080",
        "foreground": "#00ff00",
        "lines": [
            [{"text": "let ", "color": "#ff0000"}, {"text": "x"}],
            [{"text": "1", "color": "#123456"}]
        ]
    });
    let image = fixture.path("spans.png");

    let output = shift_with_stdin(
        &[
            "proof",
            &fixture.font,
            "-o",
            &image,
            "--spans",
            "-",
            "--sizes",
            SIZE,
            "--json",
        ],
        &spans.to_string(),
    );

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["mode"], "spans");
    assert_eq!(report["theme"], Value::Null);
    let pixmap = Pixmap::load_png(&image).unwrap();
    assert_eq!(rgb(&pixmap, 2.0, 2.0), [0, 0, 0x80]);
    assert_eq!(cell_ink(&pixmap, &report["rows"][0], 1), [0xff, 0, 0]);
    assert_eq!(cell_ink(&pixmap, &report["rows"][0], 4), [0, 0xff, 0]);
    assert_eq!(cell_ink(&pixmap, &report["rows"][1], 0), [0x12, 0x34, 0x56]);
}

#[test]
fn expands_tabs_to_tab_stops_before_shaping() {
    let fixture = Fixture::new();
    let code = fixture.write("tabs.txt", "\tx\nxx\tx\n    x\n");
    let image = fixture.path("tabs.png");

    let report = proof_json(&[
        &fixture.font,
        "-o",
        &image,
        "--code",
        &code,
        "--sizes",
        SIZE,
    ]);
    let widths: Vec<f64> = report["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["width"].as_f64().unwrap())
        .collect();
    assert_eq!(widths, [5.0 * CELL, 5.0 * CELL, 5.0 * CELL]);
    assert_eq!(report["lines"][1]["text"], "xx  x");
    assert_eq!(report["language"], "Plain Text");

    let narrow = proof_json(&[
        &fixture.font,
        "-o",
        &image,
        "--code",
        &code,
        "--sizes",
        SIZE,
        "--tab-width",
        "2",
    ]);
    assert_eq!(narrow["tabWidth"], 2);
    assert_eq!(narrow["lines"][0]["text"], "  x");
    assert_eq!(narrow["lines"][1]["text"], "xx  x");
    assert_eq!(narrow["rows"][0]["width"], 3.0 * CELL);
}

#[test]
fn line_numbers_widen_the_left_margin_in_a_muted_color() {
    let fixture = Fixture::new();
    let code = fixture.write("numbers.rs", "x\nx\n");
    let image = fixture.path("numbers.png");
    let plain = proof_json(&[
        &fixture.font,
        "-o",
        &image,
        "--code",
        &code,
        "--sizes",
        SIZE,
    ]);

    let numbered = proof_json(&[
        &fixture.font,
        "-o",
        &image,
        "--code",
        &code,
        "--sizes",
        SIZE,
        "--line-numbers",
    ]);

    assert_eq!(numbered["lineNumbers"], true);
    let plain_x = plain["rows"][0]["x"].as_f64().unwrap();
    let numbered_x = numbered["rows"][0]["x"].as_f64().unwrap();
    assert!(numbered_x > plain_x + CELL, "{numbered_x} vs {plain_x}");
    // The digit `2` is a box drawn in the proofed font, right-aligned one em
    // before the code.
    let pixmap = Pixmap::load_png(&image).unwrap();
    let row = &numbered["rows"][1];
    let digit = rgb(
        &pixmap,
        numbered_x - 50.0 - CELL / 2.0,
        row["baseline"].as_f64().unwrap() - 17.0,
    );
    assert_eq!(digit, [110, 115, 125], "foreground faded toward background");
}

#[test]
fn proofs_a_builtin_sample_and_marks_missing_glyphs() {
    let fixture = Fixture::new();
    let image = fixture.path("sample.png");

    let report = proof_json(&[&fixture.font, "-o", &image, "--code-sample", "rust"]);

    assert_eq!(report["language"], "Rust");
    assert_eq!(report["sizes"], serde_json::json!([20.0]));
    assert!(report["lineCount"].as_u64().unwrap() >= 20);
    let missing: Vec<&str> = report["missingCharacters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|missing| missing["character"].as_str().unwrap())
        .collect();
    for character in ["{", "}", "|", "\"", "#", ":"] {
        assert!(missing.contains(&character), "{character} is reported");
    }
    assert!(!missing.contains(&"x") && !missing.contains(&"1"));
    let pixmap = Pixmap::load_png(&image).unwrap();
    assert!(count_pixels(&pixmap, MARKER) > 500, "magenta markers");

    let human = shift(&["proof", &fixture.font, "-o", &image, "--code-sample", "go"]);
    let stdout = String::from_utf8(human.stdout).unwrap();
    assert!(stdout.contains("code  Go, tab width 4"), "{stdout}");
    assert!(stdout.contains("(drawn as magenta boxes)"), "{stdout}");
}

#[test]
fn colors_plain_text_proofs_with_overrides() {
    let fixture = Fixture::new();
    let image = fixture.path("dark.png");

    let report = proof_json(&[
        &fixture.font,
        "-o",
        &image,
        "--text",
        "lex",
        "--sizes",
        SIZE,
        "--background",
        "#102030",
        "--foreground",
        "#f0e0d0",
    ]);

    assert_eq!(report["mode"], "text");
    assert_eq!(report["background"], "#102030");
    let pixmap = Pixmap::load_png(&image).unwrap();
    assert_eq!(rgb(&pixmap, 2.0, 2.0), [0x10, 0x20, 0x30]);
    assert_eq!(cell_ink(&pixmap, &report["rows"][0], 1), [0xf0, 0xe0, 0xd0]);

    // A light theme supplies the colors; --foreground still wins.
    let themed = proof_json(&[
        &fixture.font,
        "-o",
        &image,
        "--code-sample",
        "python",
        "--theme",
        "InspiredGitHub",
        "--background",
        "#fafafa",
    ]);
    assert_eq!(themed["theme"], "InspiredGitHub");
    assert_eq!(themed["background"], "#fafafa");
    let pixmap = Pixmap::load_png(&image).unwrap();
    assert_eq!(rgb(&pixmap, 2.0, 2.0), [0xfa, 0xfa, 0xfa]);
}

#[test]
fn writes_code_as_svg_with_a_background_and_colored_runs() {
    let fixture = Fixture::new();
    let code = fixture.write("main.rs", "let x = 1\nlet y\n");
    let image = fixture.path("code.svg");

    proof_json(&[&fixture.font, "-o", &image, "--code", &code]);

    let svg = std::fs::read_to_string(&image).unwrap();
    assert!(
        svg.contains(r##"<rect class="background" width="100%" height="100%" fill="#2b303b"/>"##)
    );
    assert!(svg.contains(r##"<path class="ink" fill="#b48ead""##));
    assert!(svg.contains(r##"<path class="ink" fill="#d08770""##));
    assert!(svg.contains(r##"<path class="missing" fill="#ff00ff""##));
}

#[test]
fn rejects_unknown_themes_and_conflicting_inputs() {
    let fixture = Fixture::new();
    let image = fixture.path("bad.png");

    let bogus = shift(&[
        "proof",
        &fixture.font,
        "-o",
        &image,
        "--code-sample",
        "rust",
        "--theme",
        "bogus",
    ]);
    assert!(!bogus.status.success());
    let stderr = String::from_utf8_lossy(&bogus.stderr);
    assert!(stderr.contains("unknown theme \"bogus\""), "{stderr}");
    // miette wraps long messages, so compare without layout characters.
    let flat: String = stderr
        .chars()
        .filter(|character| !character.is_whitespace() && *character != '│')
        .collect();
    assert!(flat.contains("InspiredGitHub") && flat.contains("base16-ocean.dark"));

    let both = shift(&[
        "proof",
        &fixture.font,
        "-o",
        &image,
        "--code-sample",
        "rust",
        "--text",
        "x",
    ]);
    assert!(!both.status.success());
    assert!(String::from_utf8_lossy(&both.stderr).contains("cannot be used with"));

    let sizes = shift(&[
        "proof",
        &fixture.font,
        "-o",
        &image,
        "--code-sample",
        "rust",
        "--sizes",
        "12,20",
    ]);
    assert!(!sizes.status.success());
    assert!(String::from_utf8_lossy(&sizes.stderr).contains("one size"));
    assert!(!image.is_empty() && !Path::new(&image).exists());
}

#[test]
fn clips_long_code_lines_at_a_fixed_width_instead_of_wrapping() {
    let fixture = Fixture::new();
    let code = fixture.write("long.txt", "x\nxxxxxxxxxxxxxxxxxxxx\n");
    let image = fixture.path("clip.png");

    let report = proof_json(&[
        &fixture.font,
        "-o",
        &image,
        "--code",
        &code,
        "--sizes",
        SIZE,
        "--width",
        "300",
    ]);

    assert_eq!(report["width"], 300);
    assert_eq!(report["rows"].as_array().unwrap().len(), 2);
    assert_eq!(report["rows"][0]["clipped"], false);
    assert_eq!(report["rows"][1]["clipped"], true);
    assert_eq!(report["rows"][1]["width"], 20.0 * CELL);
}
