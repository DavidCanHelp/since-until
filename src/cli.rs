//! Shared CLI engine for the `since` and `until` front doors.
//!
//! All argument dispatch, anchor management, and output formatting live here so
//! both binaries are thin shells with no duplicated logic. The only thing that
//! differs between them is the [`Framing`] — past-leaning vs future-leaning —
//! which is passed in as a parameter, never forked into copy-pasted code.

use crate::{resolve_token, span_between, AnchorStore, Direction, Span};
use chrono::NaiveDate;
use std::path::Path;

/// How a binary frames its results. `since` reports plainly in either
/// direction; `until` adds a gentle note when the date has already passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    Since,
    Until,
}

/// The gentle note `until` adds when a target date is already in the past.
/// Shared so the CLI line and the MCP `note` field stay identically worded.
pub const ALREADY_PASSED_NOTE: &str = "heads up — that date has already passed";

/// Per-binary configuration: the program name (used in help/hints) and framing.
#[derive(Debug, Clone, Copy)]
pub struct CliConfig {
    pub program: &'static str,
    pub framing: Framing,
}

impl CliConfig {
    pub fn since() -> Self {
        Self { program: "since", framing: Framing::Since }
    }
    pub fn until() -> Self {
        Self { program: "until", framing: Framing::Until }
    }
}

/// The result of a CLI run: what to print where, and the process exit code.
/// Returning this (instead of printing/exiting directly) keeps dispatch pure
/// and unit-testable.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CliOutput {
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub code: i32,
}

impl CliOutput {
    fn ok(msg: impl Into<String>) -> Self {
        Self { stdout: Some(msg.into()), stderr: None, code: 0 }
    }
    fn fail(msg: impl Into<String>, code: i32) -> Self {
        Self { stdout: None, stderr: Some(msg.into()), code }
    }
}

/// Format one measurement line from a resolved token and its span.
///
/// ISO tokens show just the date; anchor nicknames show `name (date)` so you
/// can see what resolved. `until` appends a gentle past-tense note.
fn format_measurement(token: &str, date: NaiveDate, span: &Span, framing: Framing) -> String {
    let label = if NaiveDate::parse_from_str(token.trim(), "%Y-%m-%d").is_ok() {
        date.to_string()
    } else {
        format!("{} ({})", token.trim(), date)
    };
    let mut line = format!("{label}: {}", span.humanized);
    if framing == Framing::Until && span.direction == Direction::Past {
        line.push_str(&format!("  ({ALREADY_PASSED_NOTE})"));
    }
    line
}

/// Top-level dispatch. The first positional argument is the branch point:
/// the literal word `anchor` means store management, anything else is a token
/// to measure.
pub fn run(args: &[String], cfg: &CliConfig, store_path: &Path, now: NaiveDate) -> CliOutput {
    match args.first().map(String::as_str) {
        None => CliOutput::fail(
            format!(
                "usage: {p} <YYYY-MM-DD | anchor-name>\n       {p} anchor add <name> <YYYY-MM-DD>\n       {p} anchor list\n       {p} anchor remove <name>",
                p = cfg.program
            ),
            2,
        ),
        Some("anchor") => run_anchor(&args[1..], cfg, store_path),
        Some(_) => run_token(&args[0], cfg, store_path, now),
    }
}

/// Measure a single token (ISO date or anchor nickname).
fn run_token(token: &str, cfg: &CliConfig, store_path: &Path, now: NaiveDate) -> CliOutput {
    let store = match AnchorStore::load_from(store_path) {
        Ok(s) => s,
        Err(e) => return CliOutput::fail(e.to_string(), 1),
    };
    match resolve_token(token, &store) {
        Ok(date) => {
            let span = span_between(date, now);
            CliOutput::ok(format_measurement(token, date, &span, cfg.framing))
        }
        Err(e) => CliOutput::fail(e.to_string(), 1),
    }
}

/// Handle the `anchor` subcommands: add / list / remove.
fn run_anchor(args: &[String], cfg: &CliConfig, store_path: &Path) -> CliOutput {
    match args.first().map(String::as_str) {
        Some("add") => anchor_add(&args[1..], cfg, store_path),
        Some("list") => anchor_list(store_path, cfg),
        Some("remove") | Some("rm") => anchor_remove(&args[1..], store_path),
        Some(other) => CliOutput::fail(
            format!("unknown anchor subcommand: {other}\n  try: {p} anchor add|list|remove", p = cfg.program),
            2,
        ),
        None => CliOutput::fail(
            format!("usage: {p} anchor add|list|remove ...", p = cfg.program),
            2,
        ),
    }
}

fn anchor_add(args: &[String], cfg: &CliConfig, store_path: &Path) -> CliOutput {
    let (name, date_str) = match args {
        [name, date] => (name, date),
        _ => {
            return CliOutput::fail(
                format!("usage: {p} anchor add <name> <YYYY-MM-DD>", p = cfg.program),
                2,
            )
        }
    };
    // Validate the date parses as ISO *before* touching the store.
    let date = match NaiveDate::parse_from_str(date_str.trim(), "%Y-%m-%d") {
        Ok(d) => d,
        Err(_) => {
            return CliOutput::fail(
                format!("not a valid ISO date (YYYY-MM-DD): {date_str}"),
                1,
            )
        }
    };
    let mut store = match AnchorStore::load_from(store_path) {
        Ok(s) => s,
        Err(e) => return CliOutput::fail(e.to_string(), 1),
    };
    store.add(name, date);
    if let Err(e) = store.save_to(store_path) {
        return CliOutput::fail(e.to_string(), 1);
    }
    CliOutput::ok(format!("added anchor '{}' -> {}", name.trim().to_lowercase(), date))
}

fn anchor_list(store_path: &Path, cfg: &CliConfig) -> CliOutput {
    let store = match AnchorStore::load_from(store_path) {
        Ok(s) => s,
        Err(e) => return CliOutput::fail(e.to_string(), 1),
    };
    let anchors = store.list();
    if anchors.is_empty() {
        return CliOutput::ok(format!(
            "no anchors yet — add one with `{p} anchor add <name> <YYYY-MM-DD>`",
            p = cfg.program
        ));
    }
    // BTreeMap iterates sorted by nickname already.
    let lines: Vec<String> = anchors
        .iter()
        .map(|(name, date)| format!("{name} -> {date}"))
        .collect();
    CliOutput::ok(lines.join("\n"))
}

fn anchor_remove(args: &[String], store_path: &Path) -> CliOutput {
    let name = match args {
        [name] => name,
        _ => return CliOutput::fail("usage: anchor remove <name>", 2),
    };
    let mut store = match AnchorStore::load_from(store_path) {
        Ok(s) => s,
        Err(e) => return CliOutput::fail(e.to_string(), 1),
    };
    match store.remove(name) {
        Some(date) => {
            if let Err(e) = store.save_to(store_path) {
                return CliOutput::fail(e.to_string(), 1);
            }
            CliOutput::ok(format!("removed anchor '{}' (was {})", name.trim().to_lowercase(), date))
        }
        // Gentle: note it, exit 0, don't hard-error.
        None => CliOutput::ok(format!(
            "no anchor named '{}' — nothing to remove",
            name.trim().to_lowercase()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn temp_path() -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("since-until-cli-{}-{}.json", std::process::id(), n))
    }

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_args_is_usage_error() {
        let out = run(&[], &CliConfig::since(), &temp_path(), d(2026, 5, 31));
        assert_eq!(out.code, 2);
        assert!(out.stderr.unwrap().contains("usage:"));
    }

    #[test]
    fn token_branch_measures_iso_date() {
        let out = run(&argv(&["2020-03-01"]), &CliConfig::since(), &temp_path(), d(2026, 5, 31));
        assert_eq!(out.code, 0);
        assert_eq!(out.stdout.unwrap(), "2020-03-01: 6 years, 2 months, 30 days ago");
    }

    #[test]
    fn unknown_token_errors_nonzero() {
        let out = run(&argv(&["nope"]), &CliConfig::since(), &temp_path(), d(2026, 5, 31));
        assert_eq!(out.code, 1);
        assert_eq!(out.stderr.unwrap(), "unknown date or anchor: nope");
    }

    #[test]
    fn anchor_branch_add_then_resolve_roundtrip() {
        let path = temp_path();
        let cfg = CliConfig::since();

        let add = run(&argv(&["anchor", "add", "covid", "2020-03-01"]), &cfg, &path, d(2026, 5, 31));
        assert_eq!(add.code, 0);
        assert!(add.stdout.unwrap().contains("added anchor 'covid' -> 2020-03-01"));

        // Now the nickname resolves through the token branch.
        let measure = run(&argv(&["covid"]), &cfg, &path, d(2026, 5, 31));
        assert_eq!(measure.code, 0);
        assert_eq!(measure.stdout.unwrap(), "covid (2020-03-01): 6 years, 2 months, 30 days ago");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn anchor_add_rejects_bad_date() {
        let path = temp_path();
        let out = run(&argv(&["anchor", "add", "oops", "March 1st"]), &CliConfig::since(), &path, d(2026, 5, 31));
        assert_eq!(out.code, 1);
        assert!(out.stderr.unwrap().contains("not a valid ISO date"));
        // Nothing should have been written.
        assert!(!path.exists());
    }

    #[test]
    fn anchor_remove_missing_is_gentle() {
        let out = run(&argv(&["anchor", "remove", "ghost"]), &CliConfig::since(), &temp_path(), d(2026, 5, 31));
        assert_eq!(out.code, 0); // gentle: not a hard error
        assert!(out.stdout.unwrap().contains("no anchor named 'ghost'"));
    }

    #[test]
    fn anchor_list_friendly_when_empty() {
        let out = run(&argv(&["anchor", "list"]), &CliConfig::since(), &temp_path(), d(2026, 5, 31));
        assert_eq!(out.code, 0);
        assert!(out.stdout.unwrap().contains("no anchors yet"));
    }

    #[test]
    fn anchor_list_sorted() {
        let path = temp_path();
        let cfg = CliConfig::since();
        run(&argv(&["anchor", "add", "covid", "2020-03-01"]), &cfg, &path, d(2026, 5, 31));
        run(&argv(&["anchor", "add", "bella-birthday", "2019-07-14"]), &cfg, &path, d(2026, 5, 31));
        let out = run(&argv(&["anchor", "list"]), &cfg, &path, d(2026, 5, 31));
        assert_eq!(out.stdout.unwrap(), "bella-birthday -> 2019-07-14\ncovid -> 2020-03-01");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn until_framing_notes_past_dates() {
        let out = run(&argv(&["2020-03-01"]), &CliConfig::until(), &temp_path(), d(2026, 5, 31));
        assert_eq!(out.code, 0);
        assert!(out.stdout.unwrap().contains("already passed"));
    }

    #[test]
    fn until_framing_silent_on_future_dates() {
        let out = run(&argv(&["2026-12-25"]), &CliConfig::until(), &temp_path(), d(2026, 5, 31));
        assert_eq!(out.code, 0);
        let line = out.stdout.unwrap();
        assert_eq!(line, "2026-12-25: in 6 months, 25 days");
        assert!(!line.contains("already passed"));
    }

    #[test]
    fn until_anchor_subcommands_share_the_store() {
        // Add through `until`, read back through `since`: one shared file.
        let path = temp_path();
        let add = run(&argv(&["anchor", "add", "launch", "2026-09-01"]), &CliConfig::until(), &path, d(2026, 5, 31));
        assert_eq!(add.code, 0);

        let via_since = run(&argv(&["launch"]), &CliConfig::since(), &path, d(2026, 5, 31));
        assert_eq!(via_since.stdout.unwrap(), "launch (2026-09-01): in 3 months, 1 day");

        let via_until = run(&argv(&["launch"]), &CliConfig::until(), &path, d(2026, 5, 31));
        assert_eq!(via_until.stdout.unwrap(), "launch (2026-09-01): in 3 months, 1 day");
        let _ = std::fs::remove_file(&path);
    }
}
