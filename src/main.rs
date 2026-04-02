use clap::Parser;
use mathema::{convert, Format};
use std::io::{self, Read};
#[cfg(feature = "raster")]
use std::io::Write;

#[derive(Parser)]
#[command(name = "mathema", version, about = "Convert math formulas between LaTeX, MathML, OMML, MTEF, and UnicodeMath")]
struct Cli {
    /// Input format
    #[arg(short, long, value_parser = parse_format)]
    from: Format,

    /// Output format
    #[arg(short, long, value_parser = parse_format)]
    to: Format,

    /// Input expression (reads from stdin if omitted)
    input: Option<String>,
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

fn main() {
    let cli = Cli::parse();

    let input = match cli.input {
        Some(expr) => expr,
        None => {
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .expect("failed to read stdin");
            buf
        }
    };

    let input = input.trim();

    // Raster formats produce binary output — write raw bytes to stdout
    #[cfg(feature = "raster")]
    if matches!(cli.to, Format::Png | Format::Jpeg) {
        match render_binary(input, cli.from, cli.to) {
            Ok(bytes) => {
                io::stdout().write_all(&bytes).expect("failed to write output");
            }
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    match convert(input, cli.from, cli.to) {
        Ok(output) => println!("{output}"),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(feature = "raster")]
fn render_binary(input: &str, from: Format, to: Format) -> mathema::Result<Vec<u8>> {
    let conv = match from {
        Format::LaTeX => mathema::from_latex(input)?,
        Format::MathML => mathema::from_mathml(input)?,
        Format::Omml => mathema::from_omml(input)?,
        Format::Mtef => mathema::from_mtef(input)?,
        Format::UnicodeMath => mathema::from_unicode_math(input)?,
        #[cfg(feature = "render")]
        Format::Svg => {
            return Err(mathema::Error::Unsupported(
                "SVG cannot be used as input format".into(),
            ))
        }
        Format::Png | Format::Jpeg => {
            return Err(mathema::Error::Unsupported(
                "raster formats cannot be used as input".into(),
            ))
        }
    };
    match to {
        Format::Png => conv.to_png(),
        Format::Jpeg => conv.to_jpeg(),
        _ => unreachable!(),
    }
}
