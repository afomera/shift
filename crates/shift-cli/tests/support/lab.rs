//! A two-master `wght` font authored through the CLI, plus TTF readers.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use shift_font::Font;
use shift_store::ShiftStore;
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::string::StringId;
use skrifa::{FontRef, MetadataProvider};

pub fn shift(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_shift-cli"))
        .args(args)
        .output()
        .expect("shift CLI should run")
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

pub fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Runs a command that must succeed and returns its stdout.
pub fn ok(args: &[&str]) -> String {
    let output = shift(args);
    assert!(
        output.status.success(),
        "{args:?} failed: {}",
        stderr(&output)
    );
    stdout(&output)
}

/// Runs a command that must fail and returns its stderr on one line.
pub fn fails(args: &[&str]) -> String {
    let output = shift(args);
    assert!(
        !output.status.success(),
        "{args:?} unexpectedly succeeded: {}",
        stdout(&output)
    );
    flat(&stderr(&output))
}

/// Joins a miette report's wrapped lines into one space-separated line.
pub fn flat(report: &str) -> String {
    report
        .replace(['│', '×'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn load_font(path: &str) -> Font {
    ShiftStore::open_document(path)
        .unwrap()
        .load_font_directory()
        .unwrap()
}

/// `Atlas Mono`: wght 100..400..900, Regular at 400 and Bold at 900.
///
/// `I` is one rectangle whose stem widens from 100 to 200 units; `H` is two
/// rectangles; `space` is empty. All layers are compatible.
pub struct Lab {
    _temp: tempfile::TempDir,
    pub dir: PathBuf,
    pub path: String,
}

impl Lab {
    pub fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().to_path_buf();
        let path = dir.join("Atlas.shift").to_str().unwrap().to_string();
        ok(&["font", "create", &path]);
        ok(&[
            "font",
            "set",
            &path,
            "--family-name",
            "Atlas Mono",
            "--ascender",
            "800",
        ]);
        ok(&[
            "axis",
            "add",
            &path,
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
        ok(&[
            "source",
            "add",
            &path,
            "--name",
            "Bold",
            "--location",
            "wght=900",
        ]);
        ok(&[
            "font",
            "set",
            &path,
            "--source",
            "Bold",
            "--ascender",
            "900",
        ]);
        let lab = Self {
            _temp: temp,
            dir,
            path,
        };
        lab.set_glyphs(
            r#"{"glyphs": [
              {"name": "space", "unicodes": ["U+0020"], "source": "Regular", "layer": {"advance": 600}},
              {"name": "space", "source": "Bold", "layer": {"advance": 600}},
              {"name": "I", "unicodes": ["U+0049"], "source": "Regular", "layer": {"advance": 600,
                "svgPath": "M 250 0 L 350 0 L 350 700 L 250 700 Z"}},
              {"name": "I", "source": "Bold", "layer": {"advance": 600,
                "svgPath": "M 200 0 L 400 0 L 400 700 L 200 700 Z"}},
              {"name": "H", "unicodes": ["U+0048"], "source": "Regular", "layer": {"advance": 600,
                "svgPath": "M 100 0 L 180 0 L 180 700 L 100 700 Z M 420 0 L 500 0 L 500 700 L 420 700 Z"}},
              {"name": "H", "source": "Bold", "layer": {"advance": 600,
                "svgPath": "M 60 0 L 220 0 L 220 700 L 60 700 Z M 380 0 L 540 0 L 540 700 L 380 700 Z"}}
            ]}"#,
        );
        lab
    }

    pub fn set_glyphs(&self, batch: &str) {
        let input = self.dir.join("batch.json");
        std::fs::write(&input, batch).unwrap();
        ok(&[
            "glyph",
            "set",
            &self.path,
            "--input",
            input.to_str().unwrap(),
        ]);
    }

    pub fn file(&self, name: &str) -> String {
        self.dir.join(name).to_str().unwrap().to_string()
    }
}

pub fn read(path: impl AsRef<Path>) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

pub fn name(font: &FontRef, id: StringId) -> Option<String> {
    font.localized_strings(id)
        .english_or_first()
        .map(|string| string.to_string())
}

/// Outline points per contour, rounded to font units and sorted so start
/// point and direction choices do not matter.
pub type Outline = Vec<Vec<(i64, i64)>>;

pub fn ttf_outline(font: &FontRef, glyph_name: &str) -> Outline {
    let glyph_id = glyph_id(font, glyph_name);
    let glyph = font.outline_glyphs().get(glyph_id).unwrap();
    let mut pen = PointPen::default();
    glyph
        .draw(
            DrawSettings::unhinted(Size::unscaled(), LocationRef::default()),
            &mut pen,
        )
        .unwrap();
    normalize(pen.contours)
}

pub fn glyph_id(font: &FontRef, glyph_name: &str) -> skrifa::GlyphId {
    let character = match glyph_name {
        "I" => 'I',
        "H" => 'H',
        "O" => 'O',
        "space" => ' ',
        other => panic!("no test character for {other}"),
    };
    font.charmap().map(character).unwrap()
}

/// Reads `glyph inspect --view resolved` points for comparison with a TTF.
pub fn resolved_outline(path: &str, glyph: &str, location: &str) -> Outline {
    let json = ok(&[
        "glyph",
        "inspect",
        path,
        glyph,
        "--location",
        location,
        "--json",
    ]);
    let report: Value = serde_json::from_str(&json).unwrap();
    let contours = report["resolved"]["contours"]
        .as_array()
        .unwrap()
        .iter()
        .map(|contour| {
            contour["points"]
                .as_array()
                .unwrap()
                .iter()
                .map(|point| {
                    (
                        point["x"].as_f64().unwrap().round() as i64,
                        point["y"].as_f64().unwrap().round() as i64,
                    )
                })
                .collect()
        })
        .collect();
    normalize(contours)
}

fn normalize(contours: Outline) -> Outline {
    let mut contours = contours
        .into_iter()
        .map(|mut points| {
            points.sort_unstable();
            points.dedup();
            points
        })
        .collect::<Vec<_>>();
    contours.sort();
    contours
}

#[derive(Default)]
struct PointPen {
    contours: Outline,
}

impl PointPen {
    fn push(&mut self, x: f32, y: f32) {
        self.contours
            .last_mut()
            .unwrap()
            .push((x.round() as i64, y.round() as i64));
    }
}

impl OutlinePen for PointPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.contours.push(Vec::new());
        self.push(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.push(x, y);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.push(cx0, cy0);
        self.push(x, y);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.push(cx0, cy0);
        self.push(cx1, cy1);
        self.push(x, y);
    }

    fn close(&mut self) {}
}
