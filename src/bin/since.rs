//! `since` — past-leaning front door. A thin shell: gather args, find the
//! anchors file, hand everything to the shared `cli` engine, print, exit.

use chrono::Local;
use since_until::cli::{self, CliConfig};
use since_until::AnchorStore;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cfg = CliConfig::since();

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
