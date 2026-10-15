//! `hnchain`'s process entry point (ADR-0047, "Decided: v1 Command
//! Set"). Parses argv into an [`hn_cli::cli::Invocation`] and prints
//! `--help` directly, or runs `key generate`/`key show` (ADR-0047,
//! "Decided: Key Files"); every other command's own real execution
//! (calling the node, signing transactions) lands one command at a
//! time in later passes.

use hn_cli::cli::{self, exit_code};
use hn_cli::{hex, keyfile, profile};
use hn_crypto::{Ed25519KeyPair, account_address_body};

fn main() {
    let invocation = match cli::parse(std::env::args().skip(1)) {
        Ok(invocation) => invocation,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(exit_code::USAGE_ERROR);
        }
    };

    match invocation.command {
        cli::Command::Help => {
            print!("{}", cli::help_text());
            std::process::exit(exit_code::SUCCESS);
        }
        cli::Command::KeyGenerate { out } => match keyfile::generate(&out) {
            Ok(keypair) => {
                print_key_summary(&keypair);
                if !keyfile::enforces_unix_permissions() {
                    eprintln!(
                        "hnchain: this platform has no Unix file permissions -- hnchain cannot enforce that this key file stays unreadable by other accounts"
                    );
                }
                println!(
                    "hnchain: this key file is NOT encrypted -- protect it with file permissions and your own backups"
                );
                std::process::exit(exit_code::SUCCESS);
            }
            Err(error) => {
                eprintln!("hnchain: {error}");
                std::process::exit(exit_code::USAGE_ERROR);
            }
        },
        cli::Command::KeyShow { key } => match keyfile::load(&key) {
            Ok(keypair) => {
                print_key_summary(&keypair);
                std::process::exit(exit_code::SUCCESS);
            }
            Err(error) => {
                eprintln!("hnchain: {error}");
                std::process::exit(exit_code::USAGE_ERROR);
            }
        },
        _ => {
            eprintln!("hnchain: this command is not implemented yet");
            std::process::exit(exit_code::USAGE_ERROR);
        }
    }
}

fn print_key_summary(keypair: &Ed25519KeyPair) {
    let descriptor = keypair.key_descriptor();
    let public_key = descriptor.public_key_bytes();
    match account_address_body(profile::NETWORK_ID, descriptor.algorithm_id(), &public_key) {
        Ok(address) => println!("address:    {}", hex::encode(&address)),
        Err(error) => println!("address:    <could not derive: {error}>"),
    }
    println!("public_key: {}", hex::encode(&public_key));
}
