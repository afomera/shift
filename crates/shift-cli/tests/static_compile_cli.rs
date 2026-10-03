#[path = "support/lab.rs"]
mod support;

use skrifa::FontRef;
use skrifa::raw::TableProvider;
use skrifa::string::StringId;
use support::{Lab, fails, name, ok, read, resolved_outline, ttf_outline};

fn names(font: &FontRef) -> [Option<String>; 5] {
    [
        StringId::FAMILY_NAME,
        StringId::SUBFAMILY_NAME,
        StringId::TYPOGRAPHIC_FAMILY_NAME,
        StringId::TYPOGRAPHIC_SUBFAMILY_NAME,
        StringId::POSTSCRIPT_NAME,
    ]
    .map(|id| name(font, id))
}

fn some(values: [&str; 5]) -> [Option<String>; 5] {
    values.map(|value| Some(value.to_string()))
}

#[test]
fn static_location_matches_resolved_glyphs_and_drops_variations() {
    let lab = Lab::new();
    let ttf = lab.file("Atlas-wght700.ttf");

    ok(&[
        "compile",
        &lab.path,
        "--output",
        &ttf,
        "--location",
        "wght=700",
    ]);

    let bytes = read(&ttf);
    let font = FontRef::new(&bytes).unwrap();
    assert!(font.fvar().is_err(), "static fonts have no fvar");
    assert!(font.gvar().is_err(), "static fonts have no gvar");
    assert!(font.stat().is_err(), "static fonts have no STAT");
    assert_eq!(font.os2().unwrap().us_weight_class(), 700);
    assert_eq!(
        names(&font),
        some([
            "Atlas Mono wght700",
            "Regular",
            "Atlas Mono",
            "wght700",
            "AtlasMono-wght700"
        ])
    );

    for glyph in ["I", "H"] {
        assert_eq!(
            ttf_outline(&font, glyph),
            resolved_outline(&lab.path, glyph, "wght=700"),
            "{glyph} at wght=700"
        );
    }
    // 60% of the way from Regular to Bold: the I stem spans 220..380.
    assert_eq!(
        ttf_outline(&font, "I"),
        vec![vec![(220, 0), (220, 700), (380, 0), (380, 700)]]
    );
    // Source metrics interpolate the same way: ascender 800 -> 900.
    assert_eq!(font.os2().unwrap().s_typo_ascender(), 860);
}

#[test]
fn static_instance_takes_its_name_weight_class_and_postscript_name() {
    let lab = Lab::new();
    ok(&["instance", "add", &lab.path, "--standard-weights"]);
    ok(&[
        "instance",
        "set",
        &lab.path,
        "Bold",
        "--postscript-name",
        "AtlasMonoPro-Bold",
    ]);
    let semibold = lab.file("SemiBold.ttf");
    let bold = lab.file("Bold.ttf");

    ok(&[
        "compile",
        &lab.path,
        "-o",
        &semibold,
        "--instance",
        "SemiBold",
    ]);
    ok(&["compile", &lab.path, "-o", &bold, "--instance", "Bold"]);

    let bytes = read(&semibold);
    let font = FontRef::new(&bytes).unwrap();
    assert_eq!(font.os2().unwrap().us_weight_class(), 600);
    // Non-RIBBI styles move into the legacy family name, as fontmake does.
    assert_eq!(
        names(&font),
        some([
            "Atlas Mono SemiBold",
            "Regular",
            "Atlas Mono",
            "SemiBold",
            "AtlasMono-SemiBold"
        ])
    );
    assert_eq!(
        ttf_outline(&font, "I"),
        resolved_outline(&lab.path, "I", "wght=600")
    );

    let bytes = read(&bold);
    let font = FontRef::new(&bytes).unwrap();
    assert_eq!(font.os2().unwrap().us_weight_class(), 700);
    // RIBBI styles need no typographic names: IDs 16/17 would repeat 1/2.
    assert_eq!(
        names(&font),
        [
            Some("Atlas Mono".to_string()),
            Some("Bold".to_string()),
            None,
            None,
            Some("AtlasMonoPro-Bold".to_string())
        ]
    );
    let selection = font.os2().unwrap().fs_selection();
    assert!(selection.contains(skrifa::raw::tables::os2::SelectionFlags::BOLD));
}

#[test]
fn static_default_location_equals_the_default_master() {
    let lab = Lab::new();
    let variable = lab.file("Atlas.ttf");
    let regular = lab.file("Atlas-Regular.ttf");

    ok(&["compile", &lab.path, "--output", &variable]);
    ok(&[
        "compile",
        &lab.path,
        "--output",
        &regular,
        "--location",
        "wght=400",
    ]);

    let variable_bytes = read(&variable);
    let static_bytes = read(&regular);
    let variable = FontRef::new(&variable_bytes).unwrap();
    let regular = FontRef::new(&static_bytes).unwrap();
    for glyph in ["I", "H", "space"] {
        assert_eq!(
            ttf_outline(&regular, glyph),
            ttf_outline(&variable, glyph),
            "{glyph}"
        );
    }
    assert_eq!(
        ttf_outline(&regular, "I"),
        vec![vec![(250, 0), (250, 700), (350, 0), (350, 700)]]
    );
    assert_eq!(regular.os2().unwrap().s_typo_ascender(), 800);
    assert_eq!(
        regular.os2().unwrap().s_typo_ascender(),
        variable.os2().unwrap().s_typo_ascender()
    );
    assert_eq!(regular.os2().unwrap().us_weight_class(), 400);
}

#[test]
fn all_instances_write_one_postscript_named_static_font_each() {
    let lab = Lab::new();
    ok(&[
        "instance",
        "add",
        &lab.path,
        "--name",
        "Light",
        "--location",
        "wght=300",
    ]);
    ok(&[
        "instance",
        "add",
        &lab.path,
        "--name",
        "Extra Bold",
        "--location",
        "wght=800",
    ]);
    let build = lab.file("build");

    let output = ok(&[
        "compile",
        &lab.path,
        "--all-instances",
        "--output-dir",
        &build,
    ]);

    let expected = ["AtlasMono-Light.ttf", "AtlasMono-ExtraBold.ttf"];
    let printed = output.lines().collect::<Vec<_>>();
    assert_eq!(printed.len(), 2);
    for (line, file) in printed.iter().zip(expected) {
        assert!(line.ends_with(file), "{line}");
    }
    let bytes = read(lab.dir.join("build").join("AtlasMono-ExtraBold.ttf"));
    let font = FontRef::new(&bytes).unwrap();
    assert!(font.fvar().is_err());
    assert_eq!(font.os2().unwrap().us_weight_class(), 800);
    assert_eq!(
        name(&font, StringId::TYPOGRAPHIC_SUBFAMILY_NAME).as_deref(),
        Some("Extra Bold")
    );
}

#[test]
fn incompatible_masters_fail_between_masters_but_compile_at_a_master() {
    let lab = Lab::new();
    lab.set_glyphs(
        r#"{"glyphs": [
          {"name": "O", "unicodes": ["U+004F"], "source": "Regular", "layer": {"advance": 600,
            "svgPath": "M 100 0 L 500 0 L 500 700 L 100 700 Z"}},
          {"name": "O", "source": "Bold", "layer": {"advance": 600,
            "svgPath": "M 100 0 L 500 0 L 300 700 Z"}}
        ]}"#,
    );
    let between = lab.file("Between.ttf");

    let error = fails(&[
        "compile",
        &lab.path,
        "-o",
        &between,
        "--location",
        "wght=650",
    ]);

    assert!(error.contains("cannot compile \"wght650\""), "{error}");
    assert!(
        error.contains(
            "O on Bold: layer cannot interpolate with Regular: path 0 node count: reference 4, source 3"
        ),
        "{error}"
    );
    assert!(!std::path::Path::new(&between).exists());

    let at_master = lab.file("Black.ttf");
    ok(&[
        "compile",
        &lab.path,
        "-o",
        &at_master,
        "--location",
        "wght=900",
    ]);
    let bytes = read(&at_master);
    let font = FontRef::new(&bytes).unwrap();
    assert_eq!(
        ttf_outline(&font, "O"),
        vec![vec![(100, 0), (300, 700), (500, 0)]]
    );
}

#[test]
fn static_targets_are_validated() {
    let lab = Lab::new();
    let ttf = lab.file("X.ttf");

    let out_of_range = fails(&["compile", &lab.path, "-o", &ttf, "--location", "wght=950"]);
    assert!(
        out_of_range.contains("is outside 100..900"),
        "{out_of_range}"
    );

    let unknown = fails(&["compile", &lab.path, "-o", &ttf, "--instance", "Bold"]);
    assert!(
        unknown.contains("instance \"Bold\" does not exist"),
        "{unknown}"
    );

    let empty = fails(&[
        "compile",
        &lab.path,
        "--all-instances",
        "--output-dir",
        &ttf,
    ]);
    assert!(empty.contains("the font has no named instances"), "{empty}");
}
