use std::path::PathBuf;

use anstyle::{AnsiColor, Effects};
use clap::builder::styling::Styles;
use clap::{ArgGroup, Args, ColorChoice, Parser, Subcommand, ValueHint};

use crate::glyph_inspect::GlyphInspectView;
use crate::inspect::InspectView;
use crate::proof::{CodeSample, ProofFormat};

mod font;
pub use font::{FontInfoArgs, SetFontArgs};
mod kerning;
pub use kerning::{
    AssignKerningGroupArgs, CreateKerningGroupArgs, DeleteKerningGroupArgs, GetKerningArgs,
    KerningCommand, KerningGroupCommand, KerningGroupsArgs, KerningPositionArg, ListKerningArgs,
    RemoveKerningArgs, RenameKerningGroupArgs, SetKerningArgs, UnassignKerningGroupArgs,
};
mod skill;
pub use skill::{
    InstallSkillArgs, ShowSkillArgs, SkillAgent, SkillCommand, SkillIdentityArgs, SkillStatusArgs,
    SkillTopic,
};
mod variation;
pub use variation::{
    AddInstanceArgs, InstanceCommand, RemoveInstanceArgs, SetAxisArgs, SetInstanceArgs,
};

const CLAP_STYLES: Styles = Styles::styled()
    .header(AnsiColor::BrightCyan.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::BrightCyan.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Green.on_default())
    .placeholder(AnsiColor::BrightBlue.on_default())
    .error(AnsiColor::Red.on_default().effects(Effects::BOLD))
    .valid(AnsiColor::Green.on_default())
    .invalid(AnsiColor::Red.on_default());

#[derive(Debug, Parser)]
#[command(
    name = "shift",
    version = env!("SHIFT_PRODUCT_VERSION"),
    about = "Command-line tools for Shift documents",
    color = ColorChoice::Auto,
    styles = CLAP_STYLES
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Inspect a SQLite .shift document without modifying it.
    Inspect(InspectArgs),

    /// Compile a SQLite .shift document to a TrueType font.
    Compile(CompileArgs),

    /// Check font health without modifying it; exits 1 when errors are found.
    Check(CheckArgs),

    /// Render text or syntax-highlighted code set in a font to a PNG or SVG proof image.
    Proof(ProofArgs),

    /// Inspect or author font metadata and source metrics.
    Font {
        #[command(subcommand)]
        command: FontCommand,
    },

    /// Author variable-font axes.
    Axis {
        #[command(subcommand)]
        command: AxisCommand,
    },

    /// Author named product presets in external/user coordinates.
    Instance {
        #[command(subcommand)]
        command: InstanceCommand,
    },

    /// Author master sources.
    Source {
        #[command(subcommand)]
        command: SourceCommand,
    },

    /// Author glyph identities.
    Glyph {
        #[command(subcommand)]
        command: GlyphCommand,
    },

    /// Author sparse glyph layers.
    Layer {
        #[command(subcommand)]
        command: LayerCommand,
    },

    /// Inspect or author kerning groups and per-master pair values.
    Kerning {
        #[command(subcommand)]
        command: KerningCommand,
    },

    /// Install or print the agent skill that matches this command.
    Skill {
        #[command(subcommand)]
        command: SkillCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum FontCommand {
    /// Create a new .shift document with its default Regular source.
    Create(CreateFontArgs),

    /// Read authored metadata and one master's metrics without loading outlines.
    Info(FontInfoArgs),

    /// Set naming, attribution, license metadata, and one master's metrics atomically.
    Set(Box<SetFontArgs>),
}

#[derive(Debug, Subcommand)]
pub enum AxisCommand {
    /// Add a continuous axis.
    Add(AddAxisArgs),

    /// Edit axis naming or range without relocating masters or product presets.
    Set(SetAxisArgs),
}

#[derive(Debug, Subcommand)]
pub enum SourceCommand {
    /// Add a master source at a design-space location.
    Add(AddSourceArgs),
}

#[derive(Debug, Subcommand)]
pub enum GlyphCommand {
    /// Add glyph identity and Unicode assignments without creating layers.
    Add(AddGlyphArgs),

    /// Create or update glyphs and their layers atomically from JSON.
    Set(SetGlyphsArgs),

    /// Inspect one glyph from any supported font source.
    Inspect(InspectGlyphArgs),

    /// Write glyphs from any font source as the JSON `glyph set` reads.
    Export(ExportGlyphsArgs),
}

#[derive(Debug, Subcommand)]
pub enum LayerCommand {
    /// Add one authored layer from a semantic JSON payload.
    Add(LayerPayloadArgs),

    /// Create or replace one layer's drawing content from JSON.
    Set(LayerPayloadArgs),

    /// Copy one glyph's authored layer to another source with fresh internal ids.
    Copy(CopyLayerArgs),
}

#[derive(Debug, Args)]
pub struct CreateFontArgs {
    /// Path for the new canonical SQLite .shift document.
    #[arg(value_hint = ValueHint::FilePath)]
    pub path: PathBuf,

    /// Start from another font's structure (.shift, UFO, Designspace, Glyphs, TTF,
    /// or OTF): its names, units per em, metrics, axes, mappings, masters, and
    /// named instances, without glyphs, kerning, or feature code.
    #[arg(long, value_hint = ValueHint::FilePath)]
    pub from: Option<PathBuf>,

    /// Validate and describe the creation without writing.
    #[arg(long)]
    pub dry_run: bool,

    /// Emit a structured result for scripts and agents.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct AddAxisArgs {
    /// Path to the canonical SQLite .shift document.
    #[arg(value_hint = ValueHint::FilePath)]
    pub path: PathBuf,

    /// Four-character OpenType axis tag.
    #[arg(long)]
    pub tag: String,

    /// Human-readable axis name.
    #[arg(long)]
    pub name: String,

    /// Minimum external/design value.
    #[arg(long = "min")]
    pub minimum: f64,

    /// Default external/design value.
    #[arg(long)]
    pub default: f64,

    /// Maximum external/design value.
    #[arg(long = "max")]
    pub maximum: f64,

    #[command(flatten)]
    pub mutation: MutationArgs,
}

#[derive(Debug, Args)]
pub struct AddSourceArgs {
    /// Path to the canonical SQLite .shift document.
    #[arg(value_hint = ValueHint::FilePath)]
    pub path: PathBuf,

    /// Human-readable source name.
    #[arg(long)]
    pub name: String,

    /// Design-space coordinate as TAG=VALUE; repeat or comma-separate values.
    #[arg(long, value_name = "TAG=VALUE", value_delimiter = ',')]
    pub location: Vec<String>,

    #[command(flatten)]
    pub mutation: MutationArgs,
}

#[derive(Debug, Args)]
pub struct ExportGlyphsArgs {
    /// Path to a .shift, UFO, Designspace, Glyphs, TTF, or OTF font source.
    #[arg(value_hint = ValueHint::FilePath)]
    pub path: PathBuf,

    /// Glyph names to export; repeat or comma-separate. Glyphs their components
    /// use are exported too.
    #[arg(long, value_delimiter = ',', required = true)]
    pub glyphs: Vec<String>,

    /// Source to read: a source name, or `all` for every source. Defaults to the
    /// default source.
    #[arg(long)]
    pub source: Option<String>,

    /// Write the JSON here instead of to stdout.
    #[arg(long, value_hint = ValueHint::FilePath)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct InspectGlyphArgs {
    /// Path to a .shift, UFO, Designspace, Glyphs, TTF, or OTF font source.
    #[arg(value_hint = ValueHint::FilePath)]
    pub path: PathBuf,

    /// Glyph name or full stable glyph id.
    pub glyph: String,

    /// External user-space coordinate as TAG=VALUE; repeat or comma-separate values.
    #[arg(long, value_name = "TAG=VALUE", value_delimiter = ',')]
    pub location: Vec<String>,

    /// Select the human-readable section to print.
    #[arg(long, value_enum, default_value_t = GlyphInspectView::Summary)]
    pub view: GlyphInspectView,

    /// Emit the complete stable inspection as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct AddGlyphArgs {
    /// Path to the canonical SQLite .shift document.
    #[arg(value_hint = ValueHint::FilePath)]
    pub path: PathBuf,

    /// Glyph name, such as A or a.alt.
    pub name: String,

    /// Unicode scalar in U+XXXX form; repeat or comma-separate values.
    #[arg(long, value_name = "U+XXXX", value_delimiter = ',')]
    pub unicode: Vec<String>,

    #[command(flatten)]
    pub mutation: MutationArgs,
}

#[derive(Debug, Args)]
pub struct LayerPayloadArgs {
    /// Path to the canonical SQLite .shift document.
    #[arg(value_hint = ValueHint::FilePath)]
    pub path: PathBuf,

    /// Glyph name or full stable glyph id.
    #[arg(long)]
    pub glyph: String,

    /// Source name or full stable source id.
    #[arg(long)]
    pub source: String,

    /// JSON layer payload path, or - to read it from stdin.
    #[arg(long, value_hint = ValueHint::FilePath)]
    pub input: PathBuf,

    #[command(flatten)]
    pub mutation: MutationArgs,
}

#[derive(Debug, Args)]
pub struct SetGlyphsArgs {
    /// Path to the canonical SQLite .shift document.
    #[arg(value_hint = ValueHint::FilePath)]
    pub path: PathBuf,

    /// JSON glyph batch path, or - to read it from stdin.
    #[arg(long, value_hint = ValueHint::FilePath)]
    pub input: PathBuf,

    #[command(flatten)]
    pub mutation: MutationArgs,
}

#[derive(Debug, Args)]
pub struct CopyLayerArgs {
    /// Path to the canonical SQLite .shift document.
    #[arg(value_hint = ValueHint::FilePath)]
    pub path: PathBuf,

    /// Glyph name or full stable glyph id.
    #[arg(long)]
    pub glyph: String,

    /// Source owning the layer to copy, by name or full stable id.
    #[arg(long = "from-source")]
    pub from_source: String,

    /// Destination source, by name or full stable id.
    #[arg(long)]
    pub source: String,

    #[command(flatten)]
    pub mutation: MutationArgs,
}

#[derive(Debug, Args)]
pub struct MutationArgs {
    /// Write a new independent document instead of modifying the input.
    #[arg(long, value_hint = ValueHint::FilePath)]
    pub output: Option<PathBuf>,

    /// Validate and describe the mutation without writing.
    #[arg(long)]
    pub dry_run: bool,

    /// Emit a structured result for scripts and agents.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Parser)]
pub struct CompileArgs {
    /// Path to the canonical SQLite .shift document.
    #[arg(value_hint = ValueHint::FilePath)]
    pub path: PathBuf,

    /// Path for the compiled .ttf output.
    #[arg(short, long, value_hint = ValueHint::FilePath)]
    pub output: PathBuf,
}

/// Exit status: 0 when no error findings exist, 1 when any error finding
/// exists (warnings and info never fail), 2 when the check cannot run.
#[derive(Debug, Parser)]
pub struct CheckArgs {
    /// Path to a .shift, UFO, Designspace, Glyphs, TTF, or OTF font source.
    #[arg(value_hint = ValueHint::FilePath)]
    pub path: PathBuf,

    /// Require every non-mark glyph to share one advance width per source.
    #[arg(long)]
    pub mono: bool,

    /// Expected monospace advance in font units; defaults to the most common advance.
    #[arg(long, value_name = "UNITS", requires = "mono")]
    pub width: Option<f64>,

    /// Required characters: ascii, latin1, or a file of characters and U+XXXX lines.
    #[arg(long, value_name = "ascii|latin1|FILE")]
    pub charset: Option<String>,

    /// Check only this master source, by name or full stable source id.
    #[arg(long)]
    pub source: Option<String>,

    /// Emit the complete stable report as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Parser)]
#[command(group(ArgGroup::new("code_input").multiple(false)))]
pub struct ProofArgs {
    /// Path to a .shift, UFO, Designspace, Glyphs, TTF, or OTF font source.
    #[arg(value_hint = ValueHint::FilePath)]
    pub path: PathBuf,

    /// Path for the proof image; an existing file is replaced.
    #[arg(short, long, value_hint = ValueHint::FilePath)]
    pub output: PathBuf,

    /// Line of text to set; repeat for more lines. Defaults to a proof set.
    #[arg(long, value_name = "TEXT", conflicts_with = "code_input")]
    pub text: Vec<String>,

    /// Source file to set as syntax-highlighted code, or `-` for standard
    /// input; one row per line, whitespace kept, never wrapped.
    #[arg(long, value_name = "FILE|-", group = "code_input", value_hint = ValueHint::FilePath)]
    pub code: Option<PathBuf>,

    /// Set a built-in code sample that stresses code-font confusables.
    #[arg(long, value_name = "LANG", value_enum, group = "code_input")]
    pub code_sample: Option<CodeSample>,

    /// Pre-colored lines as JSON, or `-` for standard input:
    /// {"background":"#1e1e1e","foreground":"#d4d4d4","lines":[[{"text":"fn ","color":"#c678dd"},{"text":"main"}]]}
    #[arg(long, value_name = "FILE|-", group = "code_input", value_hint = ValueHint::FilePath)]
    pub spans: Option<PathBuf>,

    /// Grammar for --code as a language name or file extension (rust, py, ts,
    /// go, rb, sh, ...). Defaults to the file extension, then the first
    /// line, then plain text; an unknown name lists every language.
    #[arg(long, value_name = "NAME|EXT", requires = "code")]
    pub lang: Option<String>,

    /// Syntax theme for code colors, or for text background and ink:
    /// base16-ocean.dark (default for code), base16-eighties.dark,
    /// base16-mocha.dark, base16-ocean.light, InspiredGitHub,
    /// "Solarized (dark)", "Solarized (light)".
    #[arg(long, value_name = "NAME")]
    pub theme: Option<String>,

    /// Columns per tab stop in code and spans. Defaults to 4.
    #[arg(long, value_name = "N", requires = "code_input", value_parser = clap::value_parser!(u8).range(1..=16))]
    pub tab_width: Option<u8>,

    /// Number code lines in a muted gutter.
    #[arg(long, requires = "code_input")]
    pub line_numbers: bool,

    /// Page background as #rrggbb; overrides the theme and spans document.
    #[arg(long, value_name = "HEX")]
    pub background: Option<String>,

    /// Default ink as #rrggbb; overrides the theme and spans document.
    #[arg(long, value_name = "HEX")]
    pub foreground: Option<String>,

    /// Sizes in pixels per em; each text line is set at every size. Defaults
    /// to 72 for text; code takes a single size and defaults to 20.
    #[arg(long, value_name = "PX", value_delimiter = ',')]
    pub sizes: Vec<f32>,

    /// External user-space coordinate as TAG=VALUE; repeat or comma-separate values.
    #[arg(long, value_name = "TAG=VALUE", value_delimiter = ',')]
    pub location: Vec<String>,

    /// Draw ascender, cap-height, x-height, baseline, and descender guides
    /// behind the largest-size rows, or faintly behind the first code line.
    #[arg(long)]
    pub metrics: bool,

    /// Fixed image width in pixels; text wraps at spaces to fit, code is
    /// clipped at the image edge.
    #[arg(long, value_name = "PX")]
    pub width: Option<u32>,

    /// Image format; inferred from the output extension when omitted.
    #[arg(long, value_enum)]
    pub format: Option<ProofFormat>,

    /// Emit a structured report for scripts and agents.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Parser)]
pub struct InspectArgs {
    /// Path to the SQLite .shift document to inspect.
    #[arg(value_hint = ValueHint::FilePath)]
    pub path: PathBuf,

    /// Select the human-readable section to print.
    #[arg(long, value_enum, default_value_t = InspectView::Summary)]
    pub view: InspectView,

    /// Emit stable JSON for scripts and CI.
    #[arg(long)]
    pub json: bool,
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn command_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
