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

fn run_convert(args: ConvertArgs) -> Result<(), CliError> {
    let config = make_config(args.lenient);
    let inputs = resolve_inputs(args.input)?;
    let is_batch = inputs.len() > 1;

    if is_batch {
        return run_convert_batch(inputs, args.from, args.to, args.output, &config);
    }

    // Single input
    let input = &inputs[0];
    let (content, source_name, path) = match input {
        Input::Stdin(s) => (s.clone(), "<stdin>".to_owned(), None),
        Input::Inline(s) => (s.clone(), "<inline>".to_owned(), None),
        Input::File(p) => {
            let c = read_file(p)?;
            (c, p.display().to_string(), Some(p.as_path()))
        }
    };

    let from = resolve_format(args.from, path, &content)?;
    let result = mathema::convert_with(content.trim(), from, args.to, &config)
        .map_err(|e| CliError::from_mathema(e, &content, &source_name))?;

    match &args.output {
        Some(out_path) => {
            std::fs::write(out_path, &result)
                .map_err(|e| CliError::Io(format!("{}: {e}", out_path.display())))?;
        }
        None => {
            println!("{result}");
        }
    }

    Ok(())
}

fn run_convert_batch(
    inputs: Vec<Input>,
    from: Option<Format>,
    to: Format,
    output: Option<PathBuf>,
    config: &mathema::Config,
) -> Result<(), CliError> {
    // If output is given, it must be a directory
    if let Some(ref out_dir) = output {
        if !out_dir.is_dir() {
            std::fs::create_dir_all(out_dir)
                .map_err(|e| CliError::Io(format!("creating {}: {e}", out_dir.display())))?;
        }
    }

    let total = inputs.len() as u64;
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .expect("valid template")
            .progress_chars("#>-"),
    );

    let mut failures: Vec<(String, CliError)> = Vec::new();
    let mut success_count: u64 = 0;

    for input in &inputs {
        let (path, content, source_name) = match input {
            Input::File(p) => {
                pb.set_message(p.display().to_string());
                match read_file(p) {
                    Ok(c) => (Some(p.as_path()), c, p.display().to_string()),
                    Err(e) => {
                        failures.push((p.display().to_string(), e));
                        pb.inc(1);
                        continue;
                    }
                }
            }
            _ => unreachable!("batch mode only has File inputs"),
        };

        let fmt = match resolve_format(from, path, &content) {
            Ok(f) => f,
            Err(e) => {
                failures.push((source_name, e));
                pb.inc(1);
                continue;
            }
        };

        match mathema::convert_with(content.trim(), fmt, to, config) {
            Ok(result) => {
                let out_path = match &output {
                    Some(dir) => {
                        let stem = path
                            .unwrap()
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy();
                        dir.join(format!("{stem}.{}", format_extension(to)))
                    }
                    None => {
                        // Write next to input file with new extension
                        let p = path.unwrap();
                        p.with_extension(format_extension(to))
                    }
                };
                if let Err(e) = std::fs::write(&out_path, &result) {
                    failures.push((
                        source_name,
                        CliError::Io(format!("{}: {e}", out_path.display())),
                    ));
                } else {
                    success_count += 1;
                }
            }
            Err(e) => {
                failures.push((
                    source_name.clone(),
                    CliError::from_mathema(e, &content, &source_name),
                ));
            }
        }
        pb.inc(1);
    }

    pb.finish_and_clear();

    // Print summary
    let total = inputs.len();
    if failures.is_empty() {
        eprintln!("Converted {total} files successfully.");
        Ok(())
    } else {
        eprintln!(
            "Processed {total} files: {success_count} succeeded, {} failed.",
            failures.len()
        );
        for (name, err) in &failures {
            eprintln!("  {name}: {err}");
        }
        Err(failures.into_iter().next().unwrap().1)
    }
}

fn run_validate(_args: ValidateArgs) -> Result<(), CliError> {
    todo!("implemented in Task 6")
}

#[cfg(feature = "render")]
fn run_render(_args: RenderArgs) -> Result<(), CliError> {
    todo!("implemented in Task 7")
}

// --- Helpers ---

/// Resolve input format: use explicit --from, or auto-detect, or fail.
fn resolve_format(
    explicit: Option<Format>,
    path: Option<&Path>,
    content: &str,
) -> Result<Format, CliError> {
    if let Some(fmt) = explicit {
        return Ok(fmt);
    }
    detect::detect_format(path, content).ok_or_else(|| CliError::DetectionFailed {
        path: path
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<stdin>".into()),
    })
}

/// Build a `mathema::Config` from CLI flags.
fn make_config(lenient: bool) -> mathema::Config {
    mathema::Config {
        strictness: if lenient {
            mathema::Strictness::Lenient
        } else {
            mathema::Strictness::Strict
        },
        ..mathema::Config::default()
    }
}

/// Read content from a file path.
fn read_file(path: &Path) -> Result<String, CliError> {
    std::fs::read_to_string(path).map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            CliError::FileNotFound(path.display().to_string())
        } else {
            CliError::Io(format!("{}: {e}", path.display()))
        }
    })
}

/// Read content from stdin.
fn read_stdin() -> Result<String, CliError> {
    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| CliError::Io(e.to_string()))?;
    Ok(buf)
}

/// Determine output extension for a given format.
fn format_extension(fmt: Format) -> &'static str {
    match fmt {
        Format::LaTeX => "tex",
        Format::MathML => "mml",
        Format::Omml => "omml",
        Format::Mtef => "mtef",
        Format::UnicodeMath => "um",
        #[cfg(feature = "render")]
        Format::Svg => "svg",
        #[cfg(feature = "raster")]
        Format::Png => "png",
        #[cfg(feature = "raster")]
        Format::Jpeg => "jpeg",
    }
}

/// A single input item — either an inline expression or a file path.
enum Input {
    Stdin(String),
    Inline(String),
    File(PathBuf),
}

/// Resolve the `input` positional args into a list of `Input` items.
fn resolve_inputs(input: Vec<String>) -> Result<Vec<Input>, CliError> {
    if input.is_empty() {
        return Ok(vec![Input::Stdin(read_stdin()?)]);
    }
    if input.len() == 1 {
        let s = &input[0];
        let path = Path::new(s);
        if path.exists() {
            return Ok(vec![Input::File(path.to_owned())]);
        }
        // Not a file — treat as inline expression
        return Ok(vec![Input::Inline(s.clone())]);
    }
    // Multiple args: all must be files
    let mut items = Vec::with_capacity(input.len());
    for s in &input {
        let path = PathBuf::from(s);
        if !path.exists() {
            return Err(CliError::FileNotFound(s.clone()));
        }
        items.push(Input::File(path));
    }
    Ok(items)
}
