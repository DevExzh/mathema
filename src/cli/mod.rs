pub mod detect;
pub mod error;

use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use mathema::Format;

use self::error::CliError;

#[derive(Parser)]
#[command(
    name = "mathema",
    version,
    about = "Convert math formulas between LaTeX, MathML, OMML, MTEF, and UnicodeMath"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Convert between math formats
    Convert(ConvertArgs),
    /// Validate input without producing output
    Validate(ValidateArgs),
    /// Render math to SVG, PNG, or JPEG
    #[cfg(feature = "render")]
    Render(RenderArgs),
}

#[derive(Args)]
pub struct ConvertArgs {
    /// Input format (auto-detected if omitted)
    #[arg(short, long, value_parser = parse_format)]
    pub from: Option<Format>,

    /// Output format (required)
    #[arg(short, long, value_parser = parse_format)]
    pub to: Format,

    /// Output file or directory (stdout if omitted)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Use lenient parsing (collect warnings instead of failing)
    #[arg(long)]
    pub lenient: bool,

    /// Input: file paths or an inline expression (reads stdin if omitted)
    pub input: Vec<String>,
}

#[derive(Args)]
pub struct ValidateArgs {
    /// Input format (auto-detected if omitted)
    #[arg(short, long, value_parser = parse_format)]
    pub from: Option<Format>,

    /// Use lenient parsing
    #[arg(long)]
    pub lenient: bool,

    /// Input: file paths or an inline expression (reads stdin if omitted)
    pub input: Vec<String>,
}

#[cfg(feature = "render")]
#[derive(Args)]
pub struct RenderArgs {
    /// Input format (auto-detected if omitted)
    #[arg(short, long, value_parser = parse_format)]
    pub from: Option<Format>,

    /// Output format: svg (default), png, jpeg
    #[arg(long, default_value = "svg", value_parser = parse_render_format)]
    pub format: Format,

    /// Font size in pixels
    #[arg(long, default_value = "16")]
    pub font_size: f64,

    /// Foreground color (default: currentColor)
    #[arg(long, default_value = "currentColor")]
    pub color: String,

    /// Output file or directory (stdout if omitted)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Use lenient parsing
    #[arg(long)]
    pub lenient: bool,

    /// Input: file paths or an inline expression (reads stdin if omitted)
    pub input: Vec<String>,
}

fn parse_format(s: &str) -> Result<Format, String> {
    match s.to_lowercase().as_str() {
        "latex" | "tex" => Ok(Format::LaTeX),
        "mathml" | "mml" => Ok(Format::MathML),
        "omml" => Ok(Format::Omml),
        "mtef" => Ok(Format::Mtef),
        "unicodemath" | "unicode-math" | "um" => Ok(Format::UnicodeMath),
        #[cfg(feature = "render")]
        "svg" => Ok(Format::Svg),
        #[cfg(feature = "raster")]
        "png" => Ok(Format::Png),
        #[cfg(feature = "raster")]
        "jpeg" | "jpg" => Ok(Format::Jpeg),
        _ => Err(format!(
            "unknown format '{s}'. Expected: latex, mathml, omml, mtef, unicodemath{}{}",
            if cfg!(feature = "render") { ", svg" } else { "" },
            if cfg!(feature = "raster") { ", png, jpeg" } else { "" },
        )),
    }
}

#[cfg(feature = "render")]
fn parse_render_format(s: &str) -> Result<Format, String> {
    match s.to_lowercase().as_str() {
        "svg" => Ok(Format::Svg),
        #[cfg(feature = "raster")]
        "png" => Ok(Format::Png),
        #[cfg(feature = "raster")]
        "jpeg" | "jpg" => Ok(Format::Jpeg),
        _ => Err(format!(
            "unknown render format '{s}'. Expected: svg{}",
            if cfg!(feature = "raster") { ", png, jpeg" } else { "" },
        )),
    }
}

/// Main entry point — parse args and dispatch.
pub fn run() -> Result<(), CliError> {
    let cli = Cli::parse();
    match cli.command {
        Command::Convert(args) => run_convert(args),
        Command::Validate(args) => run_validate(args),
        #[cfg(feature = "render")]
        Command::Render(args) => run_render(args),
    }
}

fn run_convert(_args: ConvertArgs) -> Result<(), CliError> {
    todo!("implemented in Task 5")
}

fn run_validate(_args: ValidateArgs) -> Result<(), CliError> {
    todo!("implemented in Task 6")
}

#[cfg(feature = "render")]
fn run_render(_args: RenderArgs) -> Result<(), CliError> {
    todo!("implemented in Task 7")
}
