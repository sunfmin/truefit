//! Promises a repo's history says it owes: a file fixed often whose code no
//! registered promise's check covers (`fixed-often`), and a kept failing
//! seed whose invariant no promise names (`failing-seed-promise`).

use crate::contract::{self, CONFIG, Config};
use crate::promise::{MARKS, has_any, hash, same_name};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;

/// The words of a fix commit's subject, or of the issue's title it names.
const FIX: &str = "fix, fixes, fixed, fixing, bug, bugs, race, races, racy, duplicate, duplicates, duplicated, lost";

/// The tasks the history owes that no `not` mark answers, and notes on
/// what was not looked for.
pub fn tasks(root: &Path, config: &Config) -> (Vec<Value>, Vec<String>) {
    let marks = contract::read(root, MARKS);
    let (mut owed, notes) = found(root, config);
    owed.retain(|t| marks[t["evidence"]["hash"].as_str().unwrap_or("")]["mark"] != "not");
    (owed, notes)
}

/// Every task the history owes, marked or not, each with its `hash`.
pub fn found(root: &Path, config: &Config) -> (Vec<Value>, Vec<String>) {
    let mut notes = Vec::new();
    let mut owed = match fixed_often(root, config) {
        Some(tasks) => tasks,
        None => {
            notes.push(format!(
                "{} is in no git repo: files fixed often are not looked for",
                root.display()
            ));
            vec![]
        }
    };
    owed.extend(failing_seeds(root, config));
    (owed, notes)
}

/// The issues a message names (`#12`, `factum#30`).
fn issues(message: &str) -> Vec<String> {
    let re = regex_lite::Regex::new(r"[\w.-]*#\d+").unwrap();
    re.find_iter(message).map(|m| m.as_str().into()).collect()
}

/// Each issue's title, as the subject that lands it gives it (`Land factum#30: <title>`).
fn titles(root: &Path) -> BTreeMap<String, String> {
    let mut titles = BTreeMap::new();
    for subject in contract::git(root, &["log", "--format=%s"])
        .unwrap_or_default()
        .lines()
    {
        if let Some((head, title)) = subject.split_once(": ")
            && let Some(issue) = issues(head).pop()
        {
            titles.entry(issue).or_insert_with(|| title.to_string());
        }
    }
    titles
}

/// Whether `file` is the system's code: not under a dot or test directory,
/// nor Markdown, JSON, a lockfile or text.
fn code(file: &str) -> bool {
    let parts: Vec<&str> = file.split('/').collect();
    let ext = file.rsplit_once('.').map_or("", |(_, e)| e);
    !parts[..parts.len() - 1]
        .iter()
        .any(|p| p.starts_with('.') || ["tests", "test", "testdata", "__tests__"].contains(p))
        && !["md", "json", "lock", "txt"].contains(&ext)
}

/// The files a registered promise's check covers: a model's `covers`, the simulation's code.
fn covered(root: &Path, config: &Config) -> Vec<String> {
    let mut files = Vec::new();
    for (rung, arg) in config.promise.iter().flat_map(|p| p.checks()) {
        match (rung, &config.sim) {
            ("model", _) => files.extend(
                crate::model::files(&config.model, arg)
                    .iter()
                    .map(|f| f.to_string()),
            ),
            ("sim", Some(sim)) => {
                files.extend(sim.run.iter().filter(|f| root.join(f).is_file()).cloned());
                files.extend(sim.code.iter().flat_map(|g| crate::next::expand(root, g)));
            }
            _ => {}
        }
    }
    files
        .iter()
        .map(|f| f.trim_start_matches("./").to_string())
        .collect()
}

/// The `fixed-often` tasks, or None outside a git repo.
fn fixed_often(root: &Path, config: &Config) -> Option<Vec<Value>> {
    contract::git(root, &["rev-parse", "--show-toplevel"])?;
    let days = config.promises.history_days;
    let since = format!("--since={days}.days");
    let log = contract::git(
        root,
        &[
            "log",
            &since,
            "--no-merges",
            "--relative",
            "--format=%x1e%H%x1f%B%x1f",
            "--name-only",
        ],
    )?;
    let titles = titles(root);
    let covered = covered(root, config);
    // Each file: its newest commit, and each fix commit with the fix it serves.
    let mut files: BTreeMap<String, (String, Vec<Value>)> = BTreeMap::new();
    for commit in log.split('\x1e').skip(1) {
        let mut parts = commit.splitn(3, '\x1f');
        let (sha, message, names) = (parts.next()?, parts.next()?, parts.next()?);
        let subject = message.lines().next().unwrap_or("");
        let named = issues(message);
        let fix = match has_any(subject, FIX) {
            true => Some(named.first().cloned().unwrap_or(sha.into())),
            false => named
                .into_iter()
                .find(|i| titles.get(i).is_some_and(|t| has_any(t, FIX))),
        };
        for file in names.lines().filter(|f| !f.is_empty()) {
            let (_, fixes) = files.entry(file.into()).or_insert((sha.into(), vec![]));
            if let Some(fix) = &fix {
                fixes.push(json!({"commit": sha, "subject": subject, "fix": fix}));
            }
        }
    }
    let mut tasks: Vec<(usize, Value)> = Vec::new();
    for (file, (newest, commits)) in files {
        let mut fixes: Vec<&str> = commits.iter().filter_map(|c| c["fix"].as_str()).collect();
        fixes.sort();
        fixes.dedup();
        let n = fixes.len();
        if n < 2 || !code(&file) || !root.join(&file).is_file() || covered.contains(&file) {
            continue;
        }
        let hash = hash(&format!("fixed-often {file} {newest}"));
        tasks.push((n, json!({
            "kind": "fixed-often",
            "summary": format!("{file} was changed by {n} fixes in {days} days, and no registered promise's check covers it"),
            "files": [file, CONFIG],
            "evidence": {"file": file, "fixes": n, "commits": commits, "history_days": days, "hash": hash},
            "steps": format!("Read each fix's commits (`git show <commit>`) where they change {file}: what did the fixes make the system keep that it broke before (an order of steps, at most once, a state it must never reach)? When they keep a promise, find the sentence that makes it (SPEC, README, ADRs; write one where none does) and register it as a [[promise]] in {CONFIG}, its checks from the ladder (`truefit --help`), and tie the check to this file: a `model`'s `covers` naming the functions it stands for, or `[sim] code` holding the file. When the fixes keep no promise (unrelated slips, a build, a message), or the promise's check is a `test` or `contract` (whose code truefit cannot see), mark it: `truefit promise --mark {hash} not --why \"<why: the promise it is, or why none>\"`; it is asked again when new commits change {file}. Commit {CONFIG} and {MARKS}."),
            "accept": "truefit promise",
        })));
    }
    tasks.sort_by_key(|a| std::cmp::Reverse(a.0));
    Some(tasks.into_iter().map(|(_, t)| t).collect())
}

/// The `failing-seed-promise` tasks: one per invariant that kept seeds
/// break and no registered promise names.
fn failing_seeds(root: &Path, config: &Config) -> Vec<Value> {
    let mut by: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (seed, k) in crate::sim::all_kept(root) {
        if let Some(inv) = k["last"]["invariant"]
            .as_str()
            .filter(|_| k["last"]["verdict"] == "fail")
            && !config.promise.iter().any(|p| same_name(&p.name, inv))
        {
            by.entry(inv.into()).or_default().push(seed);
        }
    }
    by.into_iter().map(|(inv, seeds)| {
        let hash = hash(&format!("failing-seed-promise {inv}"));
        let mut files = vec![CONFIG.to_string()];
        files.extend(seeds.iter().map(|s| format!("{}/{s}.json", crate::sim::SEEDS)));
        json!({
            "kind": "failing-seed-promise",
            "summary": format!("kept seed {} breaks invariant {inv}, which no registered promise names", seeds.join(", ")),
            "files": files,
            "evidence": {"invariant": inv, "seeds": seeds, "hash": hash},
            "steps": format!("Invariant `{inv}` fails on a kept seed: which promise of the system does it check? Find the sentence that makes it (SPEC, README, ADRs; write one where none does) and register it as a [[promise]] in {CONFIG} named `{inv}`, its checks from the ladder (`truefit --help`); or, when a registered promise is the one it checks, name the invariant by that promise's name in the simulation (and the model). So the seed's fix is tied to the promise it keeps. When the invariant is no promise of the system (a check of the simulation's own), mark it: `truefit promise --mark {hash} not --why \"<why>\"`. Commit {CONFIG} and {MARKS}."),
            "accept": "truefit promise",
        })
    }).collect()
}
