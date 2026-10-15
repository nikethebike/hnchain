//! `hnchain`'s process entry point (ADR-0047, "Decided: v1 Command
//! Set"). Parses argv into an [`hn_cli::cli::Invocation`] and prints
//! `--help` directly; every other command's own real execution
//! (loading keys, calling the node, signing transactions) lands one
//! command at a time in later passes — this binary only parses and
//! reports usage errors so far.

use hn_cli::cli::{self, exit_code};

fn main() {
    let invocation = match cli::parse(std::env::args().skip(1)) {
        Ok(invocation) => invocation,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(exit_code::USAGE_ERROR);
        }
    };

    if matches!(invocation.command, cli::Command::Help) {
        print!("{}", cli::help_text());
        std::process::exit(exit_code::SUCCESS);
    }

    eprintln!("hnchain: this command is not implemented yet");
    std::process::exit(exit_code::USAGE_ERROR);
}
