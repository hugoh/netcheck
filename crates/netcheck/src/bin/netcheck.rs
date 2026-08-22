use clap::Parser;
use netcheck::Cli;

fn main() {
    let cli = Cli::parse();

    match cli.command {
        None => {
            if let Err(err) = netcheck::tui::run() {
                eprintln!("error: {err}");
                std::process::exit(1);
            }
        }
        Some(command) => netcheck::run_command(command),
    }
}
