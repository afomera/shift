use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::Value;
use shift_font::test_support::sample_variable_font;
use shift_store::ShiftStore;
use tiny_skia::Pixmap;

/// Stem width shared by the box glyphs, in font units.
const STEM: i32 = 100;

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

fn rect(x0: i32, y0: i32, x1: i32, y1: i32) -> Value {
    serde_json::json!({
        "closed": true,
        "points": [
            {"x": x0, "y": y0}, {"x": x1, "y": y0}, {"x": x1, "y": y1}, {"x": x0, "y": y1}
        ]
    })
}

/// Authors a font with a solid `I` (a stem at x 100..200, y 0..700) and an
/// `H` (stems at 100..200 and 500..600 joined by a bar at y 300..400).
fn create_box_font(path: &Path) {
    let path = path.to_str().unwrap();
    assert!(shift(&["font", "create", path]).status.success());

    let glyphs = [
        ("I", "U+0049", 300, vec![rect(100, 0, 100 + STEM, 700)]),
        (
            "H",
            "U+0048",
            700,
            vec![
                rect(100, 0, 100 + STEM, 700),
                rect(500, 0, 500 + STEM, 700),
                rect(100, 300, 600, 400),
            ],
        ),
    ];
    for (name, unicode, advance, contours) in glyphs {
        let added = shift(&["glyph", "add", path, name, "--unicode", unicode]);
        assert!(added.status.success(), "{:?}", added.stderr);
        let payload = serde_json::json!({ "advance": advance, "contours": contours });
        let layer = shift_with_stdin(
            &[
                "layer", "add", path, "--glyph", name, "--source", "Regular", "--input", "-",
            ],
            &payload.to_string(),
        );
        assert!(layer.status.success(), "{:?}", layer.stderr);
    }
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

fn is_dark(pixmap: &Pixmap, x: f64, y: f64) -> bool {
    let pixel = pixmap.pixel(x as u32, y as u32).unwrap();
    pixel.red() < 64 && pixel.green() < 64 && pixel.blue() < 64
}

fn is_white(pixmap: &Pixmap, x: f64, y: f64) -> bool {
    let pixel = pixmap.pixel(x as u32, y as u32).unwrap();
    pixel.red() == 255 && pixel.green() == 255 && pixel.blue() == 255
}

fn count_pixels(pixmap: &Pixmap, rgb: [u8; 3]) -> usize {
    pixmap
        .pixels()
        .iter()
        .filter(|pixel| [pixel.red(), pixel.green(), pixel.blue()] == rgb)
        .count()
}

#[test]
fn renders_glyph_ink_where_the_outlines_are() {
    let temp = tempfile::tempdir().unwrap();
    let font = temp.path().join("Box.shift");
    let image = temp.path().join("proof.png");
    create_box_font(&font);

    let report = proof_json(&[
        font.to_str().unwrap(),
        "--output",
        image.to_str().unwrap(),
        "--text",
        "HI",
        "--sizes",
        "100",
    ]);

    assert_eq!(report["format"], "png");
    assert_eq!(report["missingCharacters"], serde_json::json!([]));
    assert_eq!(report["metrics"]["unitsPerEm"], 1000);
    let pixmap = Pixmap::load_png(&image).unwrap();
    assert_eq!(u64::from(pixmap.width()), report["width"].as_u64().unwrap());
    assert_eq!(
        u64::from(pixmap.height()),
        report["height"].as_u64().unwrap()
    );

    // At 100px per em one font unit is 0.1px.
    let row = &report["rows"][0];
    assert_eq!(row["text"], "HI");
    assert_eq!(row["width"], 100.0);
    let x = row["x"].as_f64().unwrap();
    let baseline = row["baseline"].as_f64().unwrap();
    assert!(is_dark(&pixmap, x + 15.0, baseline - 35.0), "H left stem");
    assert!(is_dark(&pixmap, x + 35.0, baseline - 35.0), "H crossbar");
    assert!(is_white(&pixmap, x + 35.0, baseline - 60.0), "H counter");
    assert!(is_dark(&pixmap, x + 85.0, baseline - 35.0), "I stem");
    assert!(is_white(&pixmap, x + 95.0, baseline - 35.0), "after I");
    assert!(
        is_white(&pixmap, x + 15.0, baseline + 5.0),
        "below baseline"
    );
}

#[test]
fn reports_and_marks_characters_the_font_does_not_map() {
    let temp = tempfile::tempdir().unwrap();
    let font = temp.path().join("Box.shift");
    let image = temp.path().join("missing.png");
    create_box_font(&font);

    let report = proof_json(&[
        font.to_str().unwrap(),
        "--output",
        image.to_str().unwrap(),
        "--text",
        "HIx",
        "--text",
        "I H",
    ]);

    assert_eq!(
        report["missingCharacters"],
        serde_json::json!([
            {"character": "x", "unicode": "U+0078"},
            {"character": " ", "unicode": "U+0020"},
        ])
    );
    assert_eq!(
        report["lines"][0]["missingCharacters"],
        serde_json::json!([{"character": "x", "unicode": "U+0078"}])
    );
    let pixmap = Pixmap::load_png(&image).unwrap();
    assert!(
        count_pixels(&pixmap, [220, 38, 38]) > 100,
        "missing glyphs are drawn as red placeholders"
    );

    let human = shift(&[
        "proof",
        font.to_str().unwrap(),
        "--output",
        image.to_str().unwrap(),
        "--text",
        "HIx",
    ]);
    assert!(human.status.success());
    let stdout = String::from_utf8(human.stdout).unwrap();
    assert!(stdout.contains("missing  1 characters (drawn red): x U+0078"));
}

#[test]
fn renders_a_waterfall_with_metric_guides() {
    let temp = tempfile::tempdir().unwrap();
    let font = temp.path().join("Box.shift");
    let image = temp.path().join("waterfall.png");
    create_box_font(&font);

    let report = proof_json(&[
        font.to_str().unwrap(),
        "--output",
        image.to_str().unwrap(),
        "--text",
        "HI",
        "--text",
        "IH",
        "--sizes",
        "24,64",
        "--metrics",
    ]);

    let rows: Vec<(String, f64)> = report["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["text"].as_str().unwrap().to_string(),
                row["size"].as_f64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("HI".to_string(), 24.0),
            ("HI".to_string(), 64.0),
            ("IH".to_string(), 24.0),
            ("IH".to_string(), 64.0),
        ]
    );
    let guides: Vec<&str> = report["guides"]
        .as_array()
        .unwrap()
        .iter()
        .map(|guide| guide["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        guides,
        ["ascender", "capHeight", "xHeight", "baseline", "descender"]
    );

    let pixmap = Pixmap::load_png(&image).unwrap();
    let large = &report["rows"][1];
    let x = large["x"].as_f64().unwrap();
    let baseline = large["baseline"].as_f64().unwrap();
    let baseline_color = [0x1f, 0x77, 0xb4];
    let past_ink = x + large["width"].as_f64().unwrap() + 4.0;
    let pixel = pixmap
        .pixel(past_ink as u32, baseline.round() as u32)
        .unwrap();
    assert_eq!([pixel.red(), pixel.green(), pixel.blue()], baseline_color);
    // Guides sit behind the largest size only.
    let small = &report["rows"][0];
    let small_baseline = small["baseline"].as_f64().unwrap().round() as u32;
    let small_pixel = pixmap.pixel(past_ink as u32, small_baseline).unwrap();
    assert_eq!(small_pixel.red(), 255);
}

#[test]
fn infers_svg_from_the_output_extension() {
    let temp = tempfile::tempdir().unwrap();
    let font = temp.path().join("Box.shift");
    let image = temp.path().join("proof.svg");
    create_box_font(&font);

    let report = proof_json(&[
        font.to_str().unwrap(),
        "--output",
        image.to_str().unwrap(),
        "--text",
        "Hx",
        "--metrics",
    ]);

    assert_eq!(report["format"], "svg");
    let svg = std::fs::read_to_string(&image).unwrap();
    assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
    assert!(svg.contains(&format!(
        "width=\"{}\" height=\"{}\"",
        report["width"], report["height"]
    )));
    assert!(svg.contains("<path class=\"ink\" fill=\"#000000\""));
    assert!(svg.contains("<path class=\"missing\""));
    assert!(svg.contains("class=\"guide-xHeight\""));
}

#[test]
fn shapes_at_a_user_space_location() {
    let temp = tempfile::tempdir().unwrap();
    let font = temp.path().join("Variable.shift");
    let image = temp.path().join("variable.png");
    drop(ShiftStore::create_document(&font, &sample_variable_font()).unwrap());

    let width_at = |weight: &str| {
        let report = proof_json(&[
            font.to_str().unwrap(),
            "--output",
            image.to_str().unwrap(),
            "--text",
            "A",
            "--sizes",
            "100",
            "--location",
            weight,
        ]);
        assert_eq!(report["location"][0]["tag"], "wght");
        report["rows"][0]["width"].as_f64().unwrap()
    };

    // The advance interpolates from 600 at the default to 700 at wght=700.
    assert_eq!(width_at("wght=400"), 60.0);
    assert_eq!(width_at("wght=700"), 70.0);

    let unknown = shift(&[
        "proof",
        font.to_str().unwrap(),
        "--output",
        image.to_str().unwrap(),
        "--location",
        "wdth=100",
    ]);
    assert!(!unknown.status.success());
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("no \"wdth\" axis"));
}

#[test]
fn refuses_to_proof_a_font_without_characters() {
    let temp = tempfile::tempdir().unwrap();
    let font = temp.path().join("Empty.shift");
    let image = temp.path().join("empty.png");
    assert!(
        shift(&["font", "create", font.to_str().unwrap()])
            .status
            .success()
    );

    let output = shift(&[
        "proof",
        font.to_str().unwrap(),
        "--output",
        image.to_str().unwrap(),
    ]);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("maps no characters"));
    assert!(!image.exists());
}
