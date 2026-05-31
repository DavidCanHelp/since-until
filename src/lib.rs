//! since-until engine.
//!
//! Milestone 1: a pure signed-duration engine. Given a target date and a
//! reference `now`, compute the calendar-honest difference in years, months,
//! and days, note the direction, and humanize it into one English line.
//!
//! Anchors, token resolution, the `until` framing, and the MCP server all build
//! on top of this — but the math comes first and is proven here.

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub mod cli;

/// Which side of `now` the target falls on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Past,
    Future,
    Today,
}

impl Direction {
    /// Lowercase wire form, shared by every face (CLI text, MCP JSON).
    pub fn as_str(&self) -> &'static str {
        match self {
            Direction::Past => "past",
            Direction::Future => "future",
            Direction::Today => "today",
        }
    }
}

/// A calendar-honest, signed difference between two dates.
///
/// `years`, `months`, and `days` are always non-negative magnitudes; the sign
/// lives in `direction`. This keeps the breakdown easy to read and lets each
/// front door choose its own framing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub years: i64,
    pub months: i64,
    pub days: i64,
    pub direction: Direction,
    pub humanized: String,
}

/// Days in a given (year, month), honoring leap years.
fn days_in_month(year: i32, month: u32) -> i64 {
    // The first day of the *next* month, minus one day, lands on the last day
    // of this month — chrono does the leap-year reasoning for us.
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let first_of_next = NaiveDate::from_ymd_opt(ny, nm, 1).expect("valid first-of-month");
    let last_of_this = first_of_next.pred_opt().expect("date has a predecessor");
    last_of_this.day() as i64
}

/// Add `n` whole months to `date`, clamping the day to the target month's
/// length (so Jan 31 + 1 month = Feb 28/29, not an invalid date).
fn add_months(date: NaiveDate, n: i64) -> NaiveDate {
    let total = date.year() as i64 * 12 + (date.month() as i64 - 1) + n;
    let year = total.div_euclid(12) as i32;
    let month = total.rem_euclid(12) as u32 + 1;
    let day = (date.day() as i64).min(days_in_month(year, month)) as u32;
    NaiveDate::from_ymd_opt(year, month, day).expect("clamped date is valid")
}

/// Break the gap between two ordered dates into years, months, days.
///
/// `earlier` must be <= `later`. Rather than borrow raw day counts (which
/// underflows when `earlier` is a month-end), we find the largest whole-month
/// step that doesn't overshoot `later`, then measure the leftover days
/// directly. This is the calendar-honest "relativedelta" semantics.
fn ymd_between(earlier: NaiveDate, later: NaiveDate) -> (i64, i64, i64) {
    let mut months_total = (later.year() as i64 - earlier.year() as i64) * 12
        + (later.month() as i64 - earlier.month() as i64);

    // The initial estimate can overshoot (e.g. day-of-month not yet reached);
    // step back until adding that many months lands on or before `later`.
    while add_months(earlier, months_total) > later {
        months_total -= 1;
    }

    let anchor = add_months(earlier, months_total);
    let days = (later - anchor).num_days();
    (months_total / 12, months_total % 12, days)
}

/// Join the non-zero components into "6 years, 2 months, 30 days".
fn join_components(years: i64, months: i64, days: i64) -> String {
    fn unit(n: i64, name: &str) -> Option<String> {
        if n == 0 {
            None
        } else if n == 1 {
            Some(format!("1 {name}"))
        } else {
            Some(format!("{n} {name}s"))
        }
    }
    let parts: Vec<String> = [
        unit(years, "year"),
        unit(months, "month"),
        unit(days, "day"),
    ]
    .into_iter()
    .flatten()
    .collect();

    if parts.is_empty() {
        // Same calendar day but not flagged Today (shouldn't happen) — be safe.
        "0 days".to_string()
    } else {
        parts.join(", ")
    }
}

/// Compute the signed difference between `target` and `now`.
///
/// Past reads "... ago", future reads "in ...", same day reads "today".
pub fn span_between(target: NaiveDate, now: NaiveDate) -> Span {
    use std::cmp::Ordering;
    match target.cmp(&now) {
        Ordering::Equal => Span {
            years: 0,
            months: 0,
            days: 0,
            direction: Direction::Today,
            humanized: "today".to_string(),
        },
        Ordering::Less => {
            let (years, months, days) = ymd_between(target, now);
            let body = join_components(years, months, days);
            Span {
                years,
                months,
                days,
                direction: Direction::Past,
                humanized: format!("{body} ago"),
            }
        }
        Ordering::Greater => {
            let (years, months, days) = ymd_between(now, target);
            let body = join_components(years, months, days);
            Span {
                years,
                months,
                days,
                direction: Direction::Future,
                humanized: format!("in {body}"),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Anchors: user-defined nicknames for dates ("covid", "bella-birthday").
// ---------------------------------------------------------------------------

/// Errors the store and resolver can produce.
#[derive(Debug)]
pub enum Error {
    /// Filesystem trouble reading or writing the anchors file.
    Io(std::io::Error),
    /// The anchors file exists but isn't valid JSON of the expected shape.
    Json(serde_json::Error),
    /// The platform gave us no config directory to put anchors in.
    NoConfigDir,
    /// A token was neither an ISO date nor a known anchor nickname.
    UnknownToken(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "anchors file i/o error: {e}"),
            Error::Json(e) => write!(f, "anchors file is not valid JSON: {e}"),
            Error::NoConfigDir => write!(f, "could not determine a config directory"),
            Error::UnknownToken(t) => write!(f, "unknown date or anchor: {t}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}

/// Normalize a nickname so lookups are case-insensitive and whitespace-tolerant.
/// "  Covid " and "covid" are the same anchor.
fn normalize(name: &str) -> String {
    name.trim().to_lowercase()
}

/// A flat nickname -> date map, persisted as JSON in the platform config dir.
///
/// The on-disk shape is exactly `{"covid": "2020-03-01", ...}` — a plain object
/// of ISO date strings — so it's hand-editable. chrono's serde renders
/// `NaiveDate` in that ISO form for free.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AnchorStore {
    map: BTreeMap<String, NaiveDate>,
}

impl AnchorStore {
    /// The default anchors file path: `<config dir>/since-until/anchors.json`.
    /// On Linux that's `~/.config/since-until/anchors.json`.
    pub fn default_path() -> Result<PathBuf, Error> {
        let dirs = directories::ProjectDirs::from("", "", "since-until")
            .ok_or(Error::NoConfigDir)?;
        Ok(dirs.config_dir().join("anchors.json"))
    }

    /// Load anchors from the default path. A missing file is an empty set,
    /// never an error — a brand-new user has simply added nothing yet.
    pub fn load() -> Result<Self, Error> {
        Self::load_from(Self::default_path()?)
    }

    /// Load anchors from a specific path. Missing file => empty set.
    pub fn load_from(path: impl AsRef<Path>) -> Result<Self, Error> {
        match std::fs::read_to_string(path.as_ref()) {
            Ok(text) if text.trim().is_empty() => Ok(Self::default()),
            Ok(text) => Ok(serde_json::from_str(&text)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::Io(e)),
        }
    }

    /// Save anchors to the default path, creating the dir/file on first write.
    pub fn save(&self) -> Result<(), Error> {
        self.save_to(Self::default_path()?)
    }

    /// Save anchors to a specific path, creating parent directories as needed.
    pub fn save_to(&self, path: impl AsRef<Path>) -> Result<(), Error> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json + "\n")?;
        Ok(())
    }

    /// Add (or overwrite) an anchor. Nickname is normalized before storing.
    pub fn add(&mut self, name: &str, date: NaiveDate) {
        self.map.insert(normalize(name), date);
    }

    /// Remove an anchor by nickname. Returns the date if one was removed.
    pub fn remove(&mut self, name: &str) -> Option<NaiveDate> {
        self.map.remove(&normalize(name))
    }

    /// Look up a single nickname (normalized).
    pub fn get(&self, name: &str) -> Option<NaiveDate> {
        self.map.get(&normalize(name)).copied()
    }

    /// The full nickname -> date map.
    pub fn list(&self) -> &BTreeMap<String, NaiveDate> {
        &self.map
    }
}

/// Resolve a token and measure it against `now` in one step.
///
/// This is the shared entry point every front door uses: CLIs and the MCP
/// server all call this so resolution + math never gets reimplemented per face.
/// Returns the resolved date alongside its span.
pub fn measure(
    token: &str,
    store: &AnchorStore,
    now: NaiveDate,
) -> Result<(NaiveDate, Span), Error> {
    let date = resolve_token(token, store)?;
    Ok((date, span_between(date, now)))
}

/// Resolve a token to a concrete date.
///
/// Order matters and is deliberate:
///   1. Try to parse it as an ISO date (`YYYY-MM-DD`).
///   2. Failing that, look it up as an anchor nickname.
///   3. Failing that, a clear "unknown date or anchor" error.
///
/// ISO-first means a literal date always wins, even if some prankster named an
/// anchor `2020-03-01`.
pub fn resolve_token(token: &str, store: &AnchorStore) -> Result<NaiveDate, Error> {
    let t = token.trim();
    if let Ok(date) = NaiveDate::parse_from_str(t, "%Y-%m-%d") {
        return Ok(date);
    }
    if let Some(date) = store.get(t) {
        return Ok(date);
    }
    Err(Error::UnknownToken(token.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn past_reads_ago() {
        let s = span_between(d(2020, 3, 1), d(2026, 5, 31));
        assert_eq!(s.direction, Direction::Past);
        assert_eq!((s.years, s.months, s.days), (6, 2, 30));
        assert_eq!(s.humanized, "6 years, 2 months, 30 days ago");
    }

    #[test]
    fn future_reads_in() {
        let s = span_between(d(2026, 12, 25), d(2026, 6, 1));
        assert_eq!(s.direction, Direction::Future);
        assert_eq!((s.years, s.months, s.days), (0, 6, 24));
        assert_eq!(s.humanized, "in 6 months, 24 days");
    }

    #[test]
    fn same_day_is_today() {
        let s = span_between(d(2026, 5, 31), d(2026, 5, 31));
        assert_eq!(s.direction, Direction::Today);
        assert_eq!((s.years, s.months, s.days), (0, 0, 0));
        assert_eq!(s.humanized, "today");
    }

    #[test]
    fn month_end_does_not_underflow() {
        // The pathological case: earlier date is a month-end. Jan 31 -> Mar 1 is
        // "1 month, 1 day" (Jan 31 + 1mo clamps to Feb 28, then 1 day to Mar 1),
        // NOT a day-borrow underflow. target is in the past here.
        let s = span_between(d(2026, 1, 31), d(2026, 3, 1));
        assert_eq!(s.direction, Direction::Past);
        assert_eq!((s.years, s.months, s.days), (0, 1, 1));
    }

    #[test]
    fn leap_month_end() {
        // 2024 is a leap year: Jan 31 + 1mo clamps to Feb 29, then 1 day to Mar 1.
        let s = span_between(d(2024, 1, 31), d(2024, 3, 1));
        assert_eq!((s.years, s.months, s.days), (0, 1, 1));
    }

    #[test]
    fn singular_units() {
        let s = span_between(d(2025, 4, 30), d(2026, 5, 31));
        assert_eq!((s.years, s.months, s.days), (1, 1, 1));
        assert_eq!(s.humanized, "1 year, 1 month, 1 day ago");
    }

    // --- anchors ---------------------------------------------------------

    use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};

    /// A unique temp path per test, so parallel test runs don't collide.
    fn temp_path() -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
        std::env::temp_dir().join(format!(
            "since-until-test-{}-{}.json",
            std::process::id(),
            n
        ))
    }

    #[test]
    fn missing_file_is_empty_not_error() {
        let path = temp_path(); // never created
        let store = AnchorStore::load_from(&path).expect("missing file must not error");
        assert!(store.list().is_empty());
    }

    #[test]
    fn add_save_load_roundtrip() {
        let path = temp_path();
        let mut store = AnchorStore::default();
        store.add("covid", d(2020, 3, 1));
        store.add("bella-birthday", d(2019, 7, 14));
        store.save_to(&path).unwrap();

        let reloaded = AnchorStore::load_from(&path).unwrap();
        assert_eq!(reloaded.get("covid"), Some(d(2020, 3, 1)));
        assert_eq!(reloaded.get("bella-birthday"), Some(d(2019, 7, 14)));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn on_disk_shape_is_flat_iso_map() {
        let path = temp_path();
        let mut store = AnchorStore::default();
        store.add("covid", d(2020, 3, 1));
        store.save_to(&path).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"covid\""));
        assert!(text.contains("\"2020-03-01\""));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn nicknames_are_case_insensitive_and_trimmed() {
        let mut store = AnchorStore::default();
        store.add("  Covid ", d(2020, 3, 1));
        assert_eq!(store.get("covid"), Some(d(2020, 3, 1)));
        assert_eq!(store.get("COVID"), Some(d(2020, 3, 1)));
        assert_eq!(store.get(" covid  "), Some(d(2020, 3, 1)));
        assert_eq!(store.remove("CoViD"), Some(d(2020, 3, 1)));
        assert!(store.get("covid").is_none());
    }

    #[test]
    fn resolve_iso_date() {
        let store = AnchorStore::default();
        assert_eq!(resolve_token("2020-03-01", &store).unwrap(), d(2020, 3, 1));
    }

    #[test]
    fn resolve_anchor_nickname() {
        let mut store = AnchorStore::default();
        store.add("covid", d(2020, 3, 1));
        assert_eq!(resolve_token("covid", &store).unwrap(), d(2020, 3, 1));
    }

    #[test]
    fn resolve_iso_wins_over_anchor() {
        // ISO parsing happens FIRST: a literal date beats a same-named anchor.
        let mut store = AnchorStore::default();
        store.add("2020-03-01", d(1999, 1, 1));
        assert_eq!(resolve_token("2020-03-01", &store).unwrap(), d(2020, 3, 1));
    }

    #[test]
    fn resolve_unknown_is_clear_error() {
        let store = AnchorStore::default();
        let err = resolve_token("nope", &store).unwrap_err();
        assert!(matches!(err, Error::UnknownToken(_)));
        assert_eq!(err.to_string(), "unknown date or anchor: nope");
    }
}
