use std::process::ExitCode;

use clap::Parser;

use vers::cli::Cli;
use vers::context::Context;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("error: cannot read the current directory: {e}");
            return ExitCode::from(2);
        }
    };
    let ctx = Context::new(vers::git::canonicalize(&cwd), &cli.global);
    match vers::commands::run(&ctx, &cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(e.exit_code() as u8)
        }
    }
}
