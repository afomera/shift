//! TrueType compilation of canonical `.shift` documents.
//!
//! Without a target the document compiles as authored, so a font with axes
//! becomes a variable font carrying its named instances in `fvar`. With
//! `--location`, `--instance`, or `--all-instances`, each target becomes an
//! in-memory single-master font built by [`static_font()`] and compiled through
//! the same fontir/fontc pipeline. The document itself is never modified.

mod static_font;

use std::path::{Path, PathBuf};

use miette::{IntoDiagnostic, Result, WrapErr, bail};
use shift_backends::{ExportFormat, FontExportRequest, FontExporter};
use shift_font::Font;
use shift_store::ShiftStore;

use crate::authoring::resolve_instance;
use crate::cli::CompileArgs;
use crate::glyph_inspect::parse_location;

pub use static_font::{StaticTarget, static_font};

/// Compiles the requested font files and returns their paths in write order.
///
/// `--all-instances` writes one static font per named instance into the
/// output directory (created when missing), named by [`StaticTarget::file_stem`].
///
/// # Errors
///
/// Returns an error when the document cannot be loaded, a location or
/// instance selector is invalid, a static target has glyphs that cannot
/// interpolate there, two instances would share a file name, compilation
/// fails, or an output cannot be written.
pub fn compile(args: CompileArgs) -> Result<Vec<PathBuf>> {
    let font = ShiftStore::open_document(&args.path)
        .and_then(|store| store.load_font_state())
        .into_diagnostic()
        .wrap_err("failed to load Shift font")?;

    if args.all_instances {
        let Some(directory) = args.output_dir else {
            bail!("--all-instances needs --output-dir");
        };
        return compile_all_instances(&font, &directory);
    }

    let Some(output) = args.output else {
        bail!("compile needs --output");
    };
    let target = match (&args.instance, args.location.is_empty()) {
        (Some(selector), _) => Some(StaticTarget::from_instance(resolve_instance(
            &font, selector,
        )?)),
        (None, false) => Some(StaticTarget::from_location(
            &font,
            parse_location(&font, &args.location)?,
        )),
        (None, true) => None,
    };

    match target {
        Some(target) => export(&static_font(&font, &target)?, &output)?,
        None => export(&font, &output)?,
    }
    Ok(vec![output])
}

fn compile_all_instances(font: &Font, directory: &Path) -> Result<Vec<PathBuf>> {
    if font.named_instances().is_empty() {
        bail!("the font has no named instances; add them with `shift-cli instance add`");
    }

    let targets = font
        .named_instances()
        .iter()
        .map(StaticTarget::from_instance)
        .collect::<Vec<_>>();
    let mut paths = Vec::with_capacity(targets.len());
    for target in &targets {
        let path = directory.join(format!("{}.ttf", target.file_stem(font)));
        if paths.contains(&path) {
            bail!(
                "two instances would both write {}; give them distinct names or PostScript names",
                path.display()
            );
        }
        paths.push(path);
    }

    // Build every static font before writing any file, so an incompatible
    // instance leaves the output directory untouched.
    let fonts = targets
        .iter()
        .map(|target| static_font(font, target))
        .collect::<Result<Vec<_>>>()?;
    std::fs::create_dir_all(directory)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to create {}", directory.display()))?;
    for (static_font, path) in fonts.iter().zip(&paths) {
        export(static_font, path)?;
    }
    Ok(paths)
}

fn export(font: &Font, path: &Path) -> Result<()> {
    FontExporter::new()
        .export(
            font,
            FontExportRequest {
                path: path.to_path_buf(),
                format: ExportFormat::Ttf,
            },
        )
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to compile {}", path.display()))?;
    Ok(())
}
