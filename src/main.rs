use clap::Parser;
use mathema::{convert, Format};
use std::io::{self, Read};

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
        _ => Err(format!(
            "unknown format '{s}'. Expected: latex, mathml, omml, mtef, unicodemath"
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

    match convert(input, cli.from, cli.to) {
        Ok(output) => println!("{output}"),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
