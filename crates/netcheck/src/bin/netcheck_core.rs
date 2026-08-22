use clap::{CommandFactory, Parser};
use netcheck::Cli;

fn main() {
    let cli = Cli::parse();

    match cli.command {
        None => {
            let _ = Cli::command().print_help();
            println!();
        }
        Some(command) => netcheck::run_command(command),
    }
}
