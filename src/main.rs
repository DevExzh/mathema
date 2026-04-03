mod cli;

fn main() {
    if let Err(err) = cli::run() {
        let code = err.exit_code();
        eprintln!("{:?}", miette::Report::new(err));
        std::process::exit(code);
    }
}
