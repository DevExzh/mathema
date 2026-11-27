pub mod detect;
pub mod error;

#[cfg(feature = "raster")]
use std::io::Write as _;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand, ValueEnum};
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

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OmmlDialect {
    /// Pure OMML without host run properties.
    Pure,
    /// OMML embedded in WordprocessingML.
    #[value(alias = "wordprocessingml")]
    Word,
    /// OMML embedded in DrawingML, used by PowerPoint.
    #[value(alias = "drawingml")]
    Drawing,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum LatexOutputProfile {
    /// Historical KaTeX-oriented fragment spelling.
    Katex,
    /// A fragment intended for pdfLaTeX with amsmath, amssymb and xcolor.
    Pdflatex,
}

impl LatexOutputProfile {
    fn config(self, config: mathema::Config) -> mathema::Config {
        config.with_latex_profile(match self {
            Self::Katex => mathema::LatexProfile::KaTeX,
            Self::Pdflatex => mathema::LatexProfile::PdfLaTeX,
        })
    }
}

impl OmmlDialect {
    fn options(self) -> mathema::OmmlOptions {
        let run_properties = match self {
            Self::Pure => mathema::OmmlRunProperties::None,
            Self::Word => mathema::OmmlRunProperties::WordprocessingMl,
            Self::Drawing => mathema::OmmlRunProperties::DrawingMl,
        };
        mathema::OmmlOptions::default().with_run_properties(run_properties)
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum MathmlProfileArg {
    /// Presentation MathML with private round-trip attributes retained.
    RoundTrip,
    /// Schema-oriented Presentation MathML without private metadata.
    Standard,
}

impl MathmlProfileArg {
    fn options(self) -> mathema::MathmlOptions {
        let profile = match self {
            Self::RoundTrip => mathema::MathmlProfile::RoundTrip,
            Self::Standard => mathema::MathmlProfile::Standard,
        };
        mathema::MathmlOptions::default().with_profile(profile)
    }
}

#[derive(Args)]
pub struct ConvertArgs {
    /// Input format (auto-detected if omitted)
    #[arg(short, long, value_parser = parse_format)]
    pub from: Option<Format>,

    /// Output format (required)
    #[arg(short, long, value_parser = parse_format)]
    pub to: Format,

    /// SVG backend when the output format is SVG.
    #[cfg(feature = "render")]
    #[arg(long, default_value = "standalone", value_parser = parse_svg_mode)]
    pub svg_mode: SvgMode,

    /// Output file or directory (stdout if omitted)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Use lenient parsing (collect warnings instead of failing)
    #[arg(long)]
    pub lenient: bool,

    /// Also print informational diagnostics to stderr (warnings are always shown)
    #[arg(long)]
    pub diagnostics: bool,

    /// Select the OMML host dialect: pure, word, or drawing.
    #[arg(long, value_enum)]
    omml_dialect: Option<OmmlDialect>,

    /// Select the emitted LaTeX consumer profile.
    #[arg(long, value_enum)]
    latex_profile: Option<LatexOutputProfile>,

    /// Select the MathML output profile: round-trip or standard.
    #[arg(long, value_enum)]
    mathml_profile: Option<MathmlProfileArg>,

    /// Read input as raw binary MTEF from a file or stdin. Implies `--from mtef`.
    #[arg(long = "mtef-binary", visible_alias = "binary")]
    pub mtef_binary: bool,

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

    /// Also print informational diagnostics to stderr (warnings are always shown)
    #[arg(long)]
    pub diagnostics: bool,

    /// Read input as raw binary MTEF from a file or stdin. Implies `--from mtef`.
    #[arg(long = "mtef-binary", visible_alias = "binary")]
    pub mtef_binary: bool,

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

    /// SVG backend: embedded outlines (default) or client-font text.
    #[arg(long, default_value = "standalone", value_parser = parse_svg_mode)]
    pub svg_mode: SvgMode,

    /// Font size in pixels
    #[arg(long, default_value = "16")]
    pub font_size: f64,

    /// Foreground color (default: currentColor)
    #[arg(long, default_value = "currentColor")]
    pub color: String,

    /// Accessible SVG title.
    #[arg(long)]
    pub title: Option<String>,

    /// Accessible SVG label.
    #[arg(long = "aria-label")]
    pub aria_label: Option<String>,

    /// Prefix generated SVG IDs for document composition.
    #[arg(long)]
    pub id_prefix: Option<String>,

    /// Output file or directory (stdout if omitted)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Use lenient parsing
    #[arg(long)]
    pub lenient: bool,

    /// Also print informational diagnostics to stderr (warnings are always shown)
    #[arg(long)]
    pub diagnostics: bool,

    /// Read input as raw binary MTEF from a file or stdin. Implies `--from mtef`.
    #[arg(long = "mtef-binary", visible_alias = "binary")]
    pub mtef_binary: bool,

    /// Input: file paths or an inline expression (reads stdin if omitted)
    pub input: Vec<String>,
}

#[cfg(feature = "render")]
type SvgMode = mathema::SvgMode;

fn parse_format(s: &str) -> Result<Format, String> {
    s.parse().map_err(|_| {
        format!(
            "unknown format '{s}'. Expected: latex, mathml, omml, mtef, unicodemath{}{}",
            if cfg!(feature = "render") {
                ", svg"
            } else {
                ""
            },
            if cfg!(feature = "raster") {
                ", png, jpeg"
            } else {
                ""
            },
        )
    })
}

#[cfg(feature = "render")]
fn parse_render_format(s: &str) -> Result<Format, String> {
    let format = s.parse::<Format>().map_err(|_| {
        format!(
            "unknown render format '{s}'. Expected: svg{}",
            if cfg!(feature = "raster") {
                ", png, jpeg"
            } else {
                ""
            },
        )
    })?;
    match format {
        #[cfg(feature = "render")]
        Format::Svg => Ok(format),
        #[cfg(feature = "raster")]
        Format::Png | Format::Jpeg => Ok(format),
        _ => Err(format!("'{s}' is not a render output format")),
    }
}

#[cfg(feature = "render")]
fn parse_svg_mode(s: &str) -> Result<SvgMode, String> {
    match s.to_ascii_lowercase().as_str() {
        "standalone" | "embedded" | "outline" | "outlines" => Ok(mathema::SvgMode::Standalone),
        "text" => Ok(mathema::SvgMode::Text),
        _ => Err("unknown SVG mode. Expected: standalone or text".into()),
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
    let mut config = make_config(args.lenient);
    if let Some(profile) = args.latex_profile {
        if args.to != Format::LaTeX {
            return Err(CliError::Unsupported(
                "--latex-profile requires --to latex".into(),
            ));
        }
        config = profile.config(config);
    }
    let inputs = resolve_inputs(args.input)?;
    validate_mtef_binary(args.mtef_binary, args.from, &inputs)?;
    validate_omml_dialect(args.omml_dialect, args.to)?;
    validate_mathml_profile(args.mathml_profile, args.to)?;
    #[cfg(feature = "render")]
    validate_svg_mode(args.svg_mode, args.to)?;
    let is_batch = inputs.len() > 1;

    if is_batch {
        return run_convert_batch(
            inputs,
            args.output,
            &config,
            ConvertOptions {
                from: args.from,
                to: args.to,
                include_info: args.diagnostics,
                mtef_binary: args.mtef_binary,
                omml_dialect: args.omml_dialect,
                mathml_profile: args.mathml_profile,
                #[cfg(feature = "render")]
                svg_mode: args.svg_mode,
            },
        );
    }

    // Single input
    let input = &inputs[0];
    let source_name = input_source_name(input);
    let (result, diagnostics) = if args.mtef_binary {
        let bytes = read_binary_input(input, &config)?;
        convert_binary_input(
            &bytes,
            args.to,
            &config,
            &source_name,
            ConversionOutputOptions {
                omml_dialect: args.omml_dialect,
                mathml_profile: args.mathml_profile,
                #[cfg(feature = "render")]
                svg_mode: args.svg_mode,
            },
        )?
    } else {
        let (content, path) = read_text_input(input, &config)?;
        let from = resolve_format(args.from, path.as_deref(), &content)?;
        convert_text_input(
            input_for_format(&content, from),
            from,
            args.to,
            &config,
            &source_name,
            ConversionOutputOptions {
                omml_dialect: args.omml_dialect,
                mathml_profile: args.mathml_profile,
                #[cfg(feature = "render")]
                svg_mode: args.svg_mode,
            },
        )?
    };
    print_diagnostics(&source_name, &diagnostics, args.diagnostics);

    match &args.output {
        Some(out_path) => {
            std::fs::write(out_path, &result)
                .map_err(|e| CliError::Io(format!("{}: {e}", out_path.display())))?;
        },
        None => {
            println!("{result}");
        },
    }

    Ok(())
}

fn run_convert_batch(
    inputs: Vec<Input>,
    output: Option<PathBuf>,
    config: &mathema::Config,
    options: ConvertOptions,
) -> Result<(), CliError> {
    // If output is given, it must be a directory
    if let Some(ref out_dir) = output
        && !out_dir.is_dir()
    {
        std::fs::create_dir_all(out_dir)
            .map_err(|e| CliError::Io(format!("creating {}: {e}", out_dir.display())))?;
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
        let path = match input {
            Input::File(p) => p,
            _ => unreachable!("batch mode only has File inputs"),
        };
        pb.set_message(path.display().to_string());
        let source_name = path.display().to_string();
        let conversion = if options.mtef_binary {
            match read_binary_input(input, config) {
                Ok(bytes) => convert_binary_input(
                    &bytes,
                    options.to,
                    config,
                    &source_name,
                    options.output_options(),
                ),
                Err(error) => Err(error),
            }
        } else {
            match read_file(path, config) {
                Ok(content) => match resolve_format(options.from, Some(path), &content) {
                    Ok(fmt) => convert_text_input(
                        input_for_format(&content, fmt),
                        fmt,
                        options.to,
                        config,
                        &source_name,
                        options.output_options(),
                    ),
                    Err(error) => Err(error),
                },
                Err(error) => Err(error),
            }
        };

        match conversion {
            Ok((result, diagnostics)) => {
                print_diagnostics(&source_name, &diagnostics, options.include_info);
                let out_path = match &output {
                    Some(dir) => {
                        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                        dir.join(format!("{stem}.{}", format_extension(options.to)))
                    },
                    None => {
                        // Write next to input file with new extension
                        path.with_extension(format_extension(options.to))
                    },
                };
                if let Err(e) = std::fs::write(&out_path, &result) {
                    failures.push((
                        source_name,
                        CliError::Io(format!("{}: {e}", out_path.display())),
                    ));
                } else {
                    success_count += 1;
                }
            },
            Err(e) => {
                failures.push((source_name, e));
            },
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

fn run_validate(args: ValidateArgs) -> Result<(), CliError> {
    let config = make_config(args.lenient);
    let inputs = resolve_inputs(args.input)?;
    validate_mtef_binary(args.mtef_binary, args.from, &inputs)?;
    let is_batch = inputs.len() > 1;

    let pb = if is_batch {
        let bar = ProgressBar::new(inputs.len() as u64);
        bar.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
                .expect("valid template")
                .progress_chars("#>-"),
        );
        Some(bar)
    } else {
        None
    };

    let mut failures: Vec<(String, CliError)> = Vec::new();
    let mut success_count: u64 = 0;

    for input in &inputs {
        // Parse the actual input to validate it. Do not rewrite it to the same
        // format: validation should report reader diagnostics, not losses
        // caused solely by that round trip.
        let source_name = input_source_name(input);
        if let Input::File(path) = input
            && let Some(ref bar) = pb
        {
            bar.set_message(path.display().to_string());
        }
        let (from, parsed) = if args.mtef_binary {
            let bytes = match read_binary_input(input, &config) {
                Ok(bytes) => bytes,
                Err(error) => {
                    if !is_batch {
                        return Err(error);
                    }
                    failures.push((source_name, error));
                    if let Some(ref bar) = pb {
                        bar.inc(1);
                    }
                    continue;
                },
            };
            (
                Format::Mtef,
                parse_binary_input(&bytes, &config, &source_name, args.diagnostics),
            )
        } else {
            let (content, path) = match read_text_input(input, &config) {
                Ok(value) => value,
                Err(error) => {
                    if !is_batch {
                        return Err(error);
                    }
                    failures.push((source_name, error));
                    if let Some(ref bar) = pb {
                        bar.inc(1);
                    }
                    continue;
                },
            };
            let from = match resolve_format(args.from, path.as_deref(), &content) {
                Ok(format) => format,
                Err(error) => {
                    if !is_batch {
                        return Err(error);
                    }
                    failures.push((source_name, error));
                    if let Some(ref bar) = pb {
                        bar.inc(1);
                    }
                    continue;
                },
            };
            (
                from,
                parse_input(
                    input_for_format(&content, from),
                    from,
                    &config,
                    &source_name,
                    args.diagnostics,
                ),
            )
        };

        match parsed {
            Ok(_) => {
                if !is_batch {
                    println!("{source_name}: valid ({from:?})");
                }
                success_count += 1;
            },
            Err(e) => {
                if !is_batch {
                    return Err(e);
                }
                failures.push((source_name, e));
            },
        }
        if let Some(ref bar) = pb {
            bar.inc(1);
        }
    }

    if let Some(bar) = pb {
        bar.finish_and_clear();
    }

    if is_batch {
        let total = inputs.len();
        if failures.is_empty() {
            eprintln!("Validated {total} files: all valid.");
        } else {
            eprintln!(
                "Validated {total} files: {success_count} valid, {} invalid.",
                failures.len()
            );
            for (name, err) in &failures {
                eprintln!("  {name}: {err}");
            }
            return Err(failures.into_iter().next().unwrap().1);
        }
    }

    Ok(())
}

/// Print non-fatal diagnostics on stderr so conversion output on stdout stays
/// suitable for piping to another program or redirecting to an output file.
fn print_diagnostics(source_name: &str, diagnostics: &[mathema::Diagnostic], include_info: bool) {
    for diagnostic in diagnostics {
        if !include_info && diagnostic.kind == mathema::DiagnosticKind::Info {
            continue;
        }
        let severity = match diagnostic.kind {
            mathema::DiagnosticKind::Warning => "warning",
            mathema::DiagnosticKind::Info => "info",
            _ => "diagnostic",
        };
        if let Some(span) = diagnostic.span {
            eprintln!(
                "{source_name}: {severity}: {} (bytes {}..{})",
                diagnostic.message, span.start, span.end
            );
        } else {
            eprintln!("{source_name}: {severity}: {}", diagnostic.message);
        }
    }
}

#[cfg(feature = "render")]
fn run_render(args: RenderArgs) -> Result<(), CliError> {
    let config = make_config(args.lenient);
    let inputs = resolve_inputs(args.input)?;
    validate_mtef_binary(args.mtef_binary, args.from, &inputs)?;
    let is_batch = inputs.len() > 1;

    let render_opts = mathema::RenderOptions {
        font_size_px: args.font_size,
        fg_color: args.color.clone(),
        ..mathema::RenderOptions::default()
    };
    let mut svg_opts = mathema::SvgOptions::new(render_opts.clone()).with_mode(args.svg_mode);
    if let Some(title) = args.title {
        svg_opts = svg_opts.with_title(title);
    }
    if let Some(label) = args.aria_label {
        svg_opts = svg_opts.with_aria_label(label);
    }
    if let Some(prefix) = args.id_prefix {
        svg_opts = svg_opts
            .with_id_prefix(prefix)
            .map_err(|e| CliError::Render(e.to_string()))?;
    }

    if is_batch {
        return run_render_batch(
            inputs,
            args.format,
            args.output,
            &config,
            &svg_opts,
            args.svg_mode,
            InputOptions {
                from: args.from,
                include_info: args.diagnostics,
                mtef_binary: args.mtef_binary,
            },
        );
    }

    // Single input
    let input = &inputs[0];
    let source_name = input_source_name(input);
    let conv = if args.mtef_binary {
        let bytes = read_binary_input(input, &config)?;
        parse_binary_input(&bytes, &config, &source_name, args.diagnostics)?
    } else {
        let (content, path) = read_text_input(input, &config)?;
        let from = resolve_format(args.from, path.as_deref(), &content)?;
        parse_input(
            input_for_format(&content, from),
            from,
            &config,
            &source_name,
            args.diagnostics,
        )?
    };

    match args.format {
        Format::Svg => {
            let svg = if args.svg_mode == SvgMode::Text {
                if args.diagnostics {
                    let (svg, diagnostics) = conv
                        .to_svg_text_with_svg_options_and_diagnostics(&svg_opts)
                        .map_err(|e| CliError::Render(e.to_string()))?;
                    print_diagnostics(&source_name, &diagnostics, true);
                    svg
                } else {
                    conv.to_svg_text_with_svg_options(&svg_opts)
                        .map_err(|e| CliError::Render(e.to_string()))?
                }
            } else if args.diagnostics {
                let (svg, diagnostics) = conv
                    .to_svg_with_svg_options_and_diagnostics(&svg_opts)
                    .map_err(|e| CliError::Render(e.to_string()))?;
                print_diagnostics(&source_name, &diagnostics, true);
                svg
            } else {
                conv.to_svg_with_svg_options(&svg_opts)
                    .map_err(|e| CliError::Render(e.to_string()))?
            };
            match &args.output {
                Some(p) => std::fs::write(p, &svg)
                    .map_err(|e| CliError::Io(format!("{}: {e}", p.display())))?,
                None => print!("{svg}"),
            }
        },
        #[cfg(feature = "raster")]
        Format::Png => {
            let raster_opts = mathema::RasterOptions {
                render: render_opts,
                format: mathema::ImageFormat::Png,
                ..mathema::RasterOptions::default()
            };
            let bytes = if args.diagnostics {
                let (bytes, diagnostics) = conv
                    .to_png_with_options_and_diagnostics(&raster_opts)
                    .map_err(|e| CliError::Render(e.to_string()))?;
                print_diagnostics(&source_name, &diagnostics, true);
                bytes
            } else {
                conv.to_png_with(&raster_opts)
                    .map_err(|e| CliError::Render(e.to_string()))?
            };
            write_binary(&bytes, args.output.as_deref())?;
        },
        #[cfg(feature = "raster")]
        Format::Jpeg => {
            let raster_opts = mathema::RasterOptions {
                render: render_opts,
                format: mathema::ImageFormat::Jpeg,
                ..mathema::RasterOptions::default()
            };
            let bytes = if args.diagnostics {
                let (bytes, diagnostics) = conv
                    .to_jpeg_with_options_and_diagnostics(&raster_opts)
                    .map_err(|e| CliError::Render(e.to_string()))?;
                print_diagnostics(&source_name, &diagnostics, true);
                bytes
            } else {
                conv.to_jpeg_with(&raster_opts)
                    .map_err(|e| CliError::Render(e.to_string()))?
            };
            write_binary(&bytes, args.output.as_deref())?;
        },
        _ => {
            return Err(CliError::Unsupported(format!(
                "render format: {:?}",
                args.format
            )));
        },
    }

    Ok(())
}

/// Parse the input and report its reader diagnostics for validation and rendering.
fn parse_input(
    input: &str,
    from: Format,
    config: &mathema::Config,
    source_name: &str,
    include_info: bool,
) -> Result<mathema::ConvertResult, CliError> {
    let parsed = parse_input_result(input, from, config, source_name)?;
    print_diagnostics(source_name, parsed.diagnostics(), include_info);
    Ok(parsed)
}

fn parse_input_result(
    input: &str,
    from: Format,
    config: &mathema::Config,
    source_name: &str,
) -> Result<mathema::ConvertResult, CliError> {
    let result = match from {
        Format::LaTeX => mathema::from_latex_with(input, config),
        Format::MathML => mathema::from_mathml_with(input, config),
        Format::Omml => mathema::from_omml_with(input, config),
        Format::Mtef => mathema::from_mtef_with(input, config),
        Format::UnicodeMath => mathema::from_unicode_math_with(input, config),
        _ => {
            return Err(CliError::Unsupported(format!(
                "{from:?} is not a parseable format"
            )));
        },
    };
    result.map_err(|e| CliError::from_mathema(e, input, source_name))
}

fn convert_text_input(
    input: &str,
    from: Format,
    to: Format,
    config: &mathema::Config,
    source_name: &str,
    output_options: ConversionOutputOptions,
) -> Result<(String, Vec<mathema::Diagnostic>), CliError> {
    if let Some(profile) = output_options.mathml_profile {
        let parsed = parse_input_result(input, from, config, source_name)?;
        let mut diagnostics = parsed.diagnostics().to_vec();
        let (output, writer_diagnostics) = parsed
            .to_mathml_with_options_and_diagnostics(&profile.options())
            .map_err(|e| CliError::from_mathema(e, input, source_name))?;
        diagnostics.extend(writer_diagnostics);
        return Ok((output, diagnostics));
    }
    if let Some(dialect) = output_options.omml_dialect {
        let parsed = parse_input_result(input, from, config, source_name)?;
        let mut diagnostics = parsed.diagnostics().to_vec();
        let options = dialect.options();
        let (output, writer_diagnostics) = parsed
            .to_omml_with_options(&options)
            .map_err(|e| CliError::from_mathema(e, input, source_name))?;
        diagnostics.extend(writer_diagnostics);
        return Ok((output, diagnostics));
    }
    #[cfg(feature = "render")]
    if to == Format::Svg && output_options.svg_mode == SvgMode::Text {
        let parsed = parse_input_result(input, from, config, source_name)?;
        let diagnostics = parsed.diagnostics().to_vec();
        let (output, writer_diagnostics) = parsed
            .to_svg_text_with_diagnostics()
            .map_err(|e| CliError::from_mathema(e, input, source_name))?;
        let mut diagnostics = diagnostics;
        diagnostics.extend(writer_diagnostics);
        return Ok((output, diagnostics));
    }
    mathema::convert_with_config_and_diagnostics(input, from, to, config)
        .map_err(|e| CliError::from_mathema(e, input, source_name))
}

/// Parse raw MTEF bytes for validation or rendering.
fn parse_binary_input(
    input: &[u8],
    config: &mathema::Config,
    source_name: &str,
    include_info: bool,
) -> Result<mathema::ConvertResult, CliError> {
    let parsed = mathema::from_mtef_bytes_with(input, config)
        .map_err(|e| CliError::from_mathema(e, "", source_name))?;
    print_diagnostics(source_name, parsed.diagnostics(), include_info);
    Ok(parsed)
}

/// Convert raw MTEF bytes through the same reusable conversion result used by
/// the library's parse-once API. Text output remains suitable for stdout and
/// piping, while `--mtef-binary` only changes how the source is read.
fn convert_binary_input(
    input: &[u8],
    to: Format,
    config: &mathema::Config,
    source_name: &str,
    output_options: ConversionOutputOptions,
) -> Result<(String, Vec<mathema::Diagnostic>), CliError> {
    let parsed = mathema::from_mtef_bytes_with(input, config)
        .map_err(|e| CliError::from_mathema(e, "", source_name))?;
    let mut diagnostics = parsed.diagnostics().to_vec();
    let (output, writer_diagnostics) = match to {
        Format::LaTeX => parsed
            .to_latex_with_diagnostics()
            .map_err(|e| CliError::from_mathema(e, "", source_name))?,
        Format::MathML => match output_options.mathml_profile {
            Some(profile) => parsed
                .to_mathml_with_options_and_diagnostics(&profile.options())
                .map_err(|e| CliError::from_mathema(e, "", source_name))?,
            None => parsed
                .to_mathml_with_diagnostics()
                .map_err(|e| CliError::from_mathema(e, "", source_name))?,
        },
        Format::Omml => match output_options.omml_dialect {
            Some(dialect) => parsed
                .to_omml_with_options(&dialect.options())
                .map_err(|e| CliError::from_mathema(e, "", source_name))?,
            None => parsed
                .to_omml_with_diagnostics()
                .map_err(|e| CliError::from_mathema(e, "", source_name))?,
        },
        Format::Mtef => parsed
            .to_mtef_with_diagnostics()
            .map_err(|e| CliError::from_mathema(e, "", source_name))?,
        Format::UnicodeMath => parsed
            .to_unicode_math_with_diagnostics()
            .map_err(|e| CliError::from_mathema(e, "", source_name))?,
        #[cfg(feature = "render")]
        Format::Svg => {
            #[cfg(feature = "render")]
            if output_options.svg_mode == SvgMode::Text {
                parsed
                    .to_svg_text_with_diagnostics()
                    .map_err(|e| CliError::from_mathema(e, "", source_name))?
            } else {
                parsed
                    .to_svg_with_diagnostics()
                    .map_err(|e| CliError::from_mathema(e, "", source_name))?
            }
            #[cfg(not(feature = "render"))]
            {
                unreachable!("SVG output requires the render feature")
            }
        },
        #[cfg(feature = "raster")]
        Format::Png | Format::Jpeg => {
            return Err(CliError::Unsupported(
                "raster formats produce binary output; use the render subcommand".into(),
            ));
        },
        _ => {
            return Err(CliError::Unsupported(format!(
                "{to:?} is not a text conversion target"
            )));
        },
    };
    diagnostics.extend(writer_diagnostics);
    Ok((output, diagnostics))
}

#[cfg(feature = "raster")]
fn write_binary(bytes: &[u8], output: Option<&Path>) -> Result<(), CliError> {
    match output {
        Some(p) => {
            std::fs::write(p, bytes).map_err(|e| CliError::Io(format!("{}: {e}", p.display())))?;
        },
        None => {
            io::stdout()
                .write_all(bytes)
                .map_err(|e| CliError::Io(e.to_string()))?;
        },
    }
    Ok(())
}

#[cfg(feature = "render")]
fn run_render_batch(
    inputs: Vec<Input>,
    render_format: Format,
    output: Option<PathBuf>,
    config: &mathema::Config,
    svg_opts: &mathema::SvgOptions,
    svg_mode: SvgMode,
    input_options: InputOptions,
) -> Result<(), CliError> {
    if let Some(ref out_dir) = output
        && !out_dir.is_dir()
    {
        std::fs::create_dir_all(out_dir)
            .map_err(|e| CliError::Io(format!("creating {}: {e}", out_dir.display())))?;
    }

    let total = inputs.len() as u64;
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .expect("valid template")
            .progress_chars("#>-"),
    );

    let ext = format_extension(render_format);
    let mut failures: Vec<(String, CliError)> = Vec::new();
    let mut success_count: u64 = 0;

    for input in &inputs {
        let path = match input {
            Input::File(p) => p,
            _ => unreachable!("batch mode only has File inputs"),
        };
        pb.set_message(path.display().to_string());
        let source_name = path.display().to_string();
        let conv = if input_options.mtef_binary {
            match read_binary_input(input, config) {
                Ok(bytes) => {
                    parse_binary_input(&bytes, config, &source_name, input_options.include_info)
                },
                Err(error) => Err(error),
            }
        } else {
            match read_file(path, config) {
                Ok(content) => match resolve_format(input_options.from, Some(path), &content) {
                    Ok(fmt) => parse_input(
                        input_for_format(&content, fmt),
                        fmt,
                        config,
                        &source_name,
                        input_options.include_info,
                    ),
                    Err(error) => Err(error),
                },
                Err(error) => Err(error),
            }
        };
        let conv = match conv {
            Ok(c) => c,
            Err(e) => {
                failures.push((source_name, e));
                pb.inc(1);
                continue;
            },
        };

        let out_path = match &output {
            Some(dir) => {
                let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                dir.join(format!("{stem}.{ext}"))
            },
            None => path.with_extension(ext),
        };

        let rendered = match render_format {
            Format::Svg => {
                let svg = if svg_mode == SvgMode::Text {
                    if input_options.include_info {
                        conv.to_svg_text_with_svg_options_and_diagnostics(svg_opts)
                            .map_err(|e| CliError::Render(e.to_string()))
                            .map(|(svg, diagnostics)| {
                                print_diagnostics(&source_name, &diagnostics, true);
                                svg
                            })
                    } else {
                        conv.to_svg_text_with_svg_options(svg_opts)
                            .map_err(|e| CliError::Render(e.to_string()))
                    }
                } else if input_options.include_info {
                    conv.to_svg_with_svg_options_and_diagnostics(svg_opts)
                        .map_err(|e| CliError::Render(e.to_string()))
                        .map(|(svg, diagnostics)| {
                            print_diagnostics(&source_name, &diagnostics, true);
                            svg
                        })
                } else {
                    conv.to_svg_with_svg_options(svg_opts)
                        .map_err(|e| CliError::Render(e.to_string()))
                };
                svg.map(String::into_bytes)
            },
            #[cfg(feature = "raster")]
            _ => {
                let raster_fmt = match render_format {
                    Format::Jpeg => mathema::ImageFormat::Jpeg,
                    _ => mathema::ImageFormat::Png,
                };
                let raster_opts = mathema::RasterOptions {
                    render: svg_opts.render.clone(),
                    format: raster_fmt,
                    ..mathema::RasterOptions::default()
                };
                if input_options.include_info {
                    let result = match raster_fmt {
                        mathema::ImageFormat::Png => {
                            conv.to_png_with_options_and_diagnostics(&raster_opts)
                        },
                        mathema::ImageFormat::Jpeg => {
                            conv.to_jpeg_with_options_and_diagnostics(&raster_opts)
                        },
                    };
                    result.map_err(|e| CliError::Render(e.to_string())).map(
                        |(bytes, diagnostics)| {
                            print_diagnostics(&source_name, &diagnostics, true);
                            bytes
                        },
                    )
                } else {
                    let result = match raster_fmt {
                        mathema::ImageFormat::Png => conv.to_png_with(&raster_opts),
                        mathema::ImageFormat::Jpeg => conv.to_jpeg_with(&raster_opts),
                    };
                    result.map_err(|e| CliError::Render(e.to_string()))
                }
            },
            #[cfg(not(feature = "raster"))]
            _ => Err(CliError::Unsupported(format!(
                "{render_format:?} requires the 'raster' feature"
            ))),
        };

        let write_result = rendered.and_then(|bytes| {
            std::fs::write(&out_path, bytes)
                .map_err(|e| CliError::Io(format!("{}: {e}", out_path.display())))
        });
        match write_result {
            Ok(()) => success_count += 1,
            Err(e) => failures.push((source_name, e)),
        }
        pb.inc(1);
    }

    pb.finish_and_clear();

    let total = inputs.len();
    if failures.is_empty() {
        eprintln!("Rendered {total} files successfully.");
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

// --- Helpers ---

/// Prepare input for a reader without discarding a LaTeX control-space token.
///
/// `str::trim` is still appropriate for XML and linear binary/text formats,
/// where surrounding whitespace is framing. In LaTeX, however, a trailing
/// backslash followed by a space is an explicit `\ ` spacing command, so the
/// original source must reach the reader unchanged.
fn input_for_format(content: &str, format: Format) -> &str {
    match format {
        Format::LaTeX => content,
        _ => content.trim(),
    }
}

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
    let mut config = mathema::Config::default();
    config.strictness = if lenient {
        mathema::Strictness::Lenient
    } else {
        mathema::Strictness::Strict
    };
    config
}

/// Read content from a file path, enforcing the configured byte limit while
/// the file is being read.
fn read_file(path: &Path, config: &mathema::Config) -> Result<String, CliError> {
    let file = std::fs::File::open(path).map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            CliError::FileNotFound(path.display().to_string())
        } else {
            CliError::Io(format!("{}: {e}", path.display()))
        }
    })?;
    let bytes = read_limited(file, config)?;
    String::from_utf8(bytes).map_err(|_| {
        CliError::Io(format!(
            "{}: stream did not contain valid UTF-8",
            path.display()
        ))
    })
}

/// Read a text input and retain its path for format detection and diagnostics.
fn read_text_input(
    input: &Input,
    config: &mathema::Config,
) -> Result<(String, Option<PathBuf>), CliError> {
    match input {
        Input::Stdin => Ok((read_stdin(config)?, None)),
        Input::Inline(s) => Ok((s.clone(), None)),
        Input::File(path) => Ok((read_file(path, config)?, Some(path.clone()))),
    }
}

/// Read raw binary MTEF input from a file or stdin.
fn read_binary_input(input: &Input, config: &mathema::Config) -> Result<Vec<u8>, CliError> {
    match input {
        Input::Stdin => read_stdin_bytes(config),
        Input::File(path) => read_file_bytes(path, config),
        Input::Inline(_) => Err(CliError::Unsupported(
            "--mtef-binary accepts a file path or stdin, not an inline expression".into(),
        )),
    }
}

fn read_file_bytes(path: &Path, config: &mathema::Config) -> Result<Vec<u8>, CliError> {
    let file = std::fs::File::open(path).map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            CliError::FileNotFound(path.display().to_string())
        } else {
            CliError::Io(format!("{}: {e}", path.display()))
        }
    })?;
    read_limited(file, config)
}

fn read_limited<R: io::Read>(reader: R, config: &mathema::Config) -> Result<Vec<u8>, CliError> {
    let mut bytes = Vec::new();
    if let Some(limit) = config.max_input_bytes
        && limit < usize::MAX
    {
        let mut limited = reader.take((limit as u64).saturating_add(1));
        limited
            .read_to_end(&mut bytes)
            .map_err(|e| CliError::Io(e.to_string()))?;
        if bytes.len() > limit {
            return Err(input_too_large(bytes.len(), limit));
        }
        return Ok(bytes);
    }
    let mut reader = reader;
    reader
        .read_to_end(&mut bytes)
        .map_err(|e| CliError::Io(e.to_string()))?;
    Ok(bytes)
}

/// Read text content from stdin, enforcing the configured byte limit while it
/// is being read.
fn read_stdin(config: &mathema::Config) -> Result<String, CliError> {
    let bytes = read_limited(io::stdin(), config)?;
    String::from_utf8(bytes)
        .map_err(|_| CliError::Io("stdin: stream did not contain valid UTF-8".into()))
}

/// Read arbitrary bytes from stdin without attempting UTF-8 decoding.
fn read_stdin_bytes(config: &mathema::Config) -> Result<Vec<u8>, CliError> {
    read_limited(io::stdin(), config)
}

fn input_too_large(size: usize, limit: usize) -> CliError {
    CliError::Conversion(format!(
        "input size {size} bytes exceeds configured limit of {limit} bytes"
    ))
}

fn input_source_name(input: &Input) -> String {
    match input {
        Input::Stdin => "<stdin>".into(),
        Input::Inline(_) => "<inline>".into(),
        Input::File(path) => path.display().to_string(),
    }
}

fn validate_mtef_binary(
    enabled: bool,
    from: Option<Format>,
    inputs: &[Input],
) -> Result<(), CliError> {
    if !enabled {
        return Ok(());
    }
    if let Some(from) = from
        && from != Format::Mtef
    {
        return Err(CliError::Unsupported(
            "--mtef-binary cannot be combined with a non-MTEF --from format".into(),
        ));
    }
    if inputs.iter().any(|input| matches!(input, Input::Inline(_))) {
        return Err(CliError::Unsupported(
            "--mtef-binary accepts a file path or stdin, not an inline expression".into(),
        ));
    }
    Ok(())
}

fn validate_omml_dialect(dialect: Option<OmmlDialect>, target: Format) -> Result<(), CliError> {
    if dialect.is_some() && target != Format::Omml {
        return Err(CliError::Unsupported(
            "--omml-dialect requires --to omml".into(),
        ));
    }
    Ok(())
}

fn validate_mathml_profile(
    profile: Option<MathmlProfileArg>,
    target: Format,
) -> Result<(), CliError> {
    if profile.is_some() && target != Format::MathML {
        return Err(CliError::Unsupported(
            "--mathml-profile requires --to mathml".into(),
        ));
    }
    Ok(())
}

#[cfg(feature = "render")]
fn validate_svg_mode(mode: SvgMode, target: Format) -> Result<(), CliError> {
    if mode == SvgMode::Text && target != Format::Svg {
        return Err(CliError::Unsupported("--svg-mode requires --to svg".into()));
    }
    Ok(())
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
        // `Format` is `#[non_exhaustive]`; default extension for any new format.
        _ => "txt",
    }
}

/// A single input item — either an inline expression or a file path.
enum Input {
    Stdin,
    Inline(String),
    File(PathBuf),
}

#[cfg(feature = "render")]
#[derive(Clone, Copy)]
struct InputOptions {
    from: Option<Format>,
    include_info: bool,
    mtef_binary: bool,
}

#[derive(Clone, Copy)]
struct ConvertOptions {
    from: Option<Format>,
    to: Format,
    include_info: bool,
    mtef_binary: bool,
    omml_dialect: Option<OmmlDialect>,
    mathml_profile: Option<MathmlProfileArg>,
    #[cfg(feature = "render")]
    svg_mode: SvgMode,
}

#[derive(Clone, Copy)]
struct ConversionOutputOptions {
    omml_dialect: Option<OmmlDialect>,
    mathml_profile: Option<MathmlProfileArg>,
    #[cfg(feature = "render")]
    svg_mode: SvgMode,
}

impl ConvertOptions {
    fn output_options(self) -> ConversionOutputOptions {
        ConversionOutputOptions {
            omml_dialect: self.omml_dialect,
            mathml_profile: self.mathml_profile,
            #[cfg(feature = "render")]
            svg_mode: self.svg_mode,
        }
    }
}

/// Resolve the `input` positional args into a list of `Input` items.
fn resolve_inputs(input: Vec<String>) -> Result<Vec<Input>, CliError> {
    if input.is_empty() {
        return Ok(vec![Input::Stdin]);
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

#[cfg(test)]
mod tests {
    use super::input_for_format;
    use mathema::Format;

    #[test]
    fn only_latex_keeps_reader_input_whitespace() {
        let input = "  x\\ \n";
        assert_eq!(input_for_format(input, Format::LaTeX), input);
        assert_eq!(input_for_format(input, Format::MathML), "x\\");
    }
}
