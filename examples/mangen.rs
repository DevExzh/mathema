use clap::CommandFactory;
use clap_mangen::Man;
use std::fs;
use std::path::PathBuf;

// Replicate the top-level command definition for man page generation.
// This must stay in sync with src/cli/mod.rs.

#[derive(clap::Parser)]
#[command(
    name = "mathema",
    version,
    about = "Convert math formulas between LaTeX, MathML, OMML, MTEF, and UnicodeMath"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Convert between math formats
    Convert,
    /// Validate input without producing output
    Validate,
    /// Render math to SVG, PNG, or JPEG
    Render,
}

fn main() -> std::io::Result<()> {
    let out_dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/man"));

    fs::create_dir_all(&out_dir)?;

    let cmd = Cli::command();
    let man = Man::new(cmd);
    let mut buf = Vec::new();
    man.render(&mut buf)?;
    let path = out_dir.join("mathema.1");
    fs::write(&path, buf)?;
    eprintln!("Wrote {}", path.display());

    Ok(())
}
