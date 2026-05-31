//! `until` — future-leaning front door. Identical thin shell to `since.rs`,
//! differing by exactly one thing: `CliConfig::until()`. Every bit of logic —
//! resolver, store, dispatch, anchor subcommands — comes from the shared `cli`
//! engine, so the two binaries can never drift.

use chrono::Local;
use since_until::cli::{self, CliConfig};
use since_until::AnchorStore;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cfg = CliConfig::until();

    let store_path = match AnchorStore::default_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    let now = Local::now().date_naive();
    let out = cli::run(&args, &cfg, &store_path, now);

    if let Some(s) = out.stdout {
        println!("{s}");
    }
    if let Some(s) = out.stderr {
        eprintln!("{s}");
    }
    std::process::exit(out.code);
}
