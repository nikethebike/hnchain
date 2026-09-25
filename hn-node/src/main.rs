//! `hn-node`'s process entry point (ADR-0037, "Decided: `hn-node`
//! Process"; ADR-0038, "Decided: Node Config"). Parses CLI flags (or an
//! equivalent `--config` file) into [`hn_node::NodeConfig`] and runs
//! the node until killed; a config or runtime error prints to stderr
//! and exits non-zero rather than panicking, matching this workspace's
//! no-panic discipline all the way out to the process boundary. "Stop"
//! is OS-level process termination — see `hn_node`'s own crate
//! documentation for why no custom signal handler is installed.

fn main() {
    let config = match hn_node::NodeConfig::parse(std::env::args().skip(1)) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };

    if let Err(error) = hn_node::run(config) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
