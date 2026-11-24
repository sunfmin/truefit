//! A `kept` promise's end. A promise is kept without a check only while the
//! reason it gives still holds: until a date, while no file of a path
//! exists, until an issue closes, or (`kept_forever`) for good. A `kept`
//! with none of these is warned for `GRACE_DAYS` after its line last
//! changed, then reported `unchecked`.

use crate::contract::{self, CONFIG, fault};
use crate::date;
use crate::promise::Promise;
use serde_json::Value;
use std::path::Path;

/// How long a `kept` with no end is warned before it is `unchecked`.
pub const GRACE_DAYS: u64 = 14;

const ENDS: &str = "kept_until = \"<YYYY-MM-DD>\", kept_while_absent = [\"<a path or glob the work adds>\"], kept_until_issue = \"<checkout>#<n>\", or kept_forever = true with a `kept` that says why no check can exist";

/// What is wrong in a promise's `kept` and its end.
pub fn faults(p: &Promise) -> Vec<Value> {
    let n = &p.name;
    let mut faults = Vec::new();
    let has_end = has_end(p);
    match &p.kept {
        None if has_end || p.kept_forever => faults.push(fault(
            format!("promise `{n}`: an end of `kept` with no `kept`"),
            "give it `kept = \"<why it is not checked>\"` too, or remove the end",
        )),
        Some(_) if has_end && p.kept_forever => faults.push(fault(
            format!("promise `{n}` is both kept_forever and kept to an end"),
            "keep it to an end, or forever: not both",
        )),
        Some(k) if p.kept_forever && k.get_ref().trim().is_empty() => faults.push(fault(
            format!("promise `{n}`: kept_forever, and its `kept` does not say why no check can exist"),
            "say in `kept` why no check can exist (a legal promise, the repos' setup), or give it an end",
        )),
        _ => {}
    }
    if let Some(d) = &p.kept_until
        && date::days(d).is_none()
    {
        faults.push(fault(
            format!("promise `{n}`: kept_until \"{d}\" is no date"),
            "write the date the reason stops holding as YYYY-MM-DD",
        ));
    }
    if let Some(i) = &p.kept_until_issue
        && issue(i).is_none()
    {
        faults.push(fault(
            format!("promise `{n}`: kept_until_issue \"{i}\" is no issue"),
            "name the issue whose work adds the check as <checkout>#<n>, e.g. rollmind#80",
        ));
    }
    if p.kept_while_absent.iter().any(|g| g.trim().is_empty()) {
        faults.push(fault(
            format!("promise `{n}`: kept_while_absent has an empty path"),
            "name each path or glob (`App/**/*.swift`) the work that adds the check creates",
        ));
    }
    faults
}

/// A kept promise's status, `kept` or `unchecked` (its end passed, or it has
/// none past its grace), and its `why`; what else it says of it (`until`,
/// `note`, `warning`, `suggest`) it writes in its `report`.
pub fn judge(root: &Path, p: &Promise, reason: &str, report: &mut Value) -> (&'static str, String) {
    let today = date::days(&date::today()).unwrap_or(0);
    let ended = |end: String| format!("its `kept` ended: {end}; it was kept because: {reason}");
    if let Some(d) = &p.kept_until {
        if today > date::days(d).unwrap_or(0) {
            return (
                "unchecked",
                ended(format!("kept until {d}, which has passed")),
            );
        }
        report["until"] = d.as_str().into();
    }
    for glob in &p.kept_while_absent {
        if let Some(f) = crate::next::expand(root, glob).first() {
            return ("unchecked", ended(format!("{f} exists (`{glob}`)")));
        }
    }
    if let Some(i) = &p.kept_until_issue {
        match closed(root, i) {
            Ok(true) => return ("unchecked", ended(format!("{i} is closed"))),
            Ok(false) => {}
            Err(e) => report["note"] = format!("{e}: whether {i} is closed is not read").into(),
        }
    }
    if has_end(p) || p.kept_forever {
        return ("kept", reason.into());
    }
    let changed = p.kept.as_ref().and_then(|k| changed(root, k.span()));
    // The grace's last day, while it lasts.
    let grace = changed
        .map(|day| day + GRACE_DAYS)
        .filter(|last| today <= *last);
    let until = date::civil(grace.unwrap_or(today + GRACE_DAYS));
    report["suggest"] = format!("kept_until = \"{until}\"").into();
    match (changed, grace) {
        (None, _) => (
            "unchecked",
            format!(
                "kept with no end, and no git history dates its `kept` line in {CONFIG}, so no grace can be counted: give it its check, or an end ({ENDS}); it was kept because: {reason}"
            ),
        ),
        (Some(day), None) => (
            "unchecked",
            format!(
                "kept with no end, and its line last changed on {}, over {GRACE_DAYS} days ago: give it its check, or an end that still holds ({ENDS}); it was kept because: {reason}",
                date::civil(day)
            ),
        ),
        (Some(day), Some(last)) => {
            report["warning"] = format!("kept with no end: from {} ({GRACE_DAYS} days after its line last changed, {}) it is reported unchecked: give it the end its reason names, `suggest`'s or another ({ENDS})", date::civil(last + 1), date::civil(day)).into();
            ("kept", reason.into())
        }
    }
}

/// Whether a promise names an end of its `kept`.
fn has_end(p: &Promise) -> bool {
    p.kept_until.is_some() || !p.kept_while_absent.is_empty() || p.kept_until_issue.is_some()
}

/// `<checkout>#<n>` as its parts.
fn issue(i: &str) -> Option<(&str, &str)> {
    let (checkout, n) = i.split_once('#')?;
    let ok = !checkout.is_empty()
        && checkout
            .chars()
            .all(|c| c.is_alphanumeric() || "-_.".contains(c))
        && !n.is_empty()
        && n.chars().all(|c| c.is_ascii_digit());
    ok.then_some((checkout, n))
}

/// Whether factorium's tracker says issue `i` is closed, or why it cannot tell.
fn closed(root: &Path, i: &str) -> Result<bool, String> {
    let (checkout, n) = issue(i).unwrap_or((i, ""));
    let program = contract::find(root, "factorium").ok_or("factorium is not on PATH")?;
    let shown: Value = std::process::Command::new(program)
        .args(["issue", "show", checkout, n])
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice(&o.stdout).ok())
        .unwrap_or_default();
    let status = shown["status"].as_str();
    status
        .map(|s| s == "closed")
        .ok_or_else(|| format!("`factorium issue show {checkout} {n}` answered no status"))
}

/// The day (since 1970-01-01, UTC) the lines of `span` in truefit.toml last
/// changed, as git blame says; None outside git or for a file it does not track.
fn changed(root: &Path, span: std::ops::Range<usize>) -> Option<u64> {
    let text = std::fs::read_to_string(root.join(CONFIG)).ok()?;
    let line = |at: usize| text.get(..at).map_or(1, |t| t.matches('\n').count() + 1);
    let lines = format!("{},{}", line(span.start), line(span.end));
    let blame = contract::git(
        root,
        &["blame", "--line-porcelain", "-L", &lines, "--", CONFIG],
    )?;
    blame
        .lines()
        .filter_map(|l| l.strip_prefix("committer-time ")?.parse::<u64>().ok())
        .max()
        .map(|secs| secs / 86_400)
}
