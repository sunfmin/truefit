//! `truefit next [--all]`: what to do about the repo's ties, as tasks with
//! evidence, steps and an accept command, in the shape of slimfit's `next`
//! (factorium's `[idle]` files them as issues).

use crate::contract::{self, Standin};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// The tasks the ties in `root/truefit.toml` call for: the first, or `all`.
pub fn run(root: &Path, all: bool) -> Result<Value, Vec<Value>> {
    let config = contract::load(root)?;
    let mut notes = Vec::new();
    let mut tasks = Vec::new();
    match &config.contract.standins {
        None => notes.push(format!(
            "no `[contract] standins` in {}: stand-ins without a contract are not looked for; list where the repo's stand-ins live",
            contract::CONFIG
        )),
        Some(globs) => {
            let declared: Vec<PathBuf> = config
                .standin
                .iter()
                .filter_map(|s| contract::find(root, &s.run[0])?.canonicalize().ok())
                .collect();
            let mut found = Vec::new();
            for glob in globs {
                for path in expand(root, glob) {
                    let real = root.join(&path).canonicalize().ok();
                    if !real.is_some_and(|r| declared.contains(&r)) && !found.contains(&path) {
                        tasks.push(no_contract(&path, glob));
                        found.push(path);
                    }
                }
            }
        }
    }
    let today = contract::days(&contract::today()).unwrap_or(0);
    for s in &config.standin {
        tasks.extend(stale(root, s, today, config.contract.fresh_days));
        let differ = contract::read(root, &contract::differ_file(&s.name));
        if !differ.is_null() {
            tasks.push(differing(s, differ));
        }
    }
    let mut counts = serde_json::Map::new();
    for t in &tasks {
        let n = counts
            .entry(t["kind"].as_str().unwrap_or(""))
            .or_insert(json!(0));
        *n = json!(n.as_u64().unwrap_or(0) + 1);
    }
    let next = match (tasks.is_empty(), all) {
        (true, _) => {
            "nothing: every declared stand-in's contract is fresh, and none is without one"
        }
        (false, true) => "do each task by its steps, then run its accept command and commit",
        (false, false) => "do the task by its steps, then run its accept command and commit",
    };
    let mut answer = json!({
        "by": format!("truefit {}", env!("TRUEFIT_VERSION")),
        "counts": counts,
        "next": next,
        "notes": notes,
    });
    if all {
        answer["tasks"] = tasks.into();
    } else {
        answer["task"] = tasks.into_iter().next().unwrap_or(Value::Null);
    }
    Ok(answer)
}

fn no_contract(path: &str, glob: &str) -> Value {
    let name = Path::new(path)
        .file_name()
        .map_or(path.into(), |n| n.to_string_lossy());
    json!({
        "kind": "no-contract",
        "summary": format!("stand-in {path} has no contract"),
        "files": [path, contract::CONFIG],
        "evidence": {"standin": path, "found_by": glob},
        "steps": format!("Read the system's code where it runs the real program `{path}` stands in for (not the stand-in), and declare a [[standin]] in {cfg} with run = [\"{path}\"] and that program as `real`: a case for each command line the system runs and each answer it tells apart (`truefit --help`, what a contract covers). Run `truefit contract` until every case runs, then the accept command where the real program runs; when its cases differ, make the stand-in answer as the real program does. Commit {cfg} and .truefit/.", cfg = contract::CONFIG),
        "accept": format!("truefit contract --real {name}"),
    })
}

/// A `stale-contract` task, if `s`'s last passing real run is missing, older
/// than `fresh_days`, or against another version than the one installed.
fn stale(root: &Path, s: &Standin, today: u64, fresh_days: u64) -> Option<Value> {
    let file = contract::record_file(&s.name);
    let record = contract::read(root, &file);
    let installed = contract::version(root, s);
    let mut why = Vec::new();
    if record.is_null() {
        why.push("no real run of it has passed".to_string());
    }
    if let Some(passed) = record["date"].as_str() {
        let age = today.saturating_sub(contract::days(passed).unwrap_or(0));
        if age > fresh_days {
            why.push(format!(
                "its last real run passed {age} days ago, on {passed}: over fresh_days ({fresh_days})"
            ));
        }
    }
    if let (Some(then), Some(now)) = (record["version"].as_str(), &installed)
        && then != now
    {
        why.push(format!(
            "it passed against version {then}; {now} is installed"
        ));
    }
    if why.is_empty() {
        return None;
    }
    Some(json!({
        "kind": "stale-contract",
        "summary": format!("stand-in {}: {}", s.name, why.join("; ")),
        "files": [file],
        "evidence": {"standin": s.name, "real_run": record, "installed": installed, "fresh_days": fresh_days, "why": why},
        "steps": format!("Run the accept command where `{0}` runs. When every case agrees, commit {file}: the contract is fresh. When a case differs, make the stand-in answer as `{0}` does (the differing case's answers are printed), never the case or a normalise rule to hide what the system reads, and run it again. When it is not run, give it what it `needs` here.", s.real[0]),
        "accept": format!("truefit contract --real {}", s.name),
    }))
}

fn differing(s: &Standin, run: Value) -> Value {
    let file = contract::differ_file(&s.name);
    let cases: Vec<&str> = run["cases"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["case"].as_str())
        .collect();
    let mut files = vec![file.clone()];
    if s.run[0].contains('/') {
        files.insert(0, s.run[0].clone());
    }
    json!({
        "kind": "differing-case",
        "summary": format!("stand-in {} answers unlike `{}` in {}", s.name, s.real[0], cases.join(", ")),
        "files": files,
        "evidence": {"standin": s.name, "real_run": run},
        "steps": format!("For each case in the evidence, read which parts `differ` and both answers (`standin`, `real`). Fix the stand-in `{}` so it answers as `{}` does: the real program is right. Do not change the case, and add a normalise rule only for what differs from run to run (dates, paths, ids), never for what the system reads. Run the accept command; when every case agrees it removes {file}: commit .truefit/.", s.run[0], s.real[0]),
        "accept": format!("truefit contract --real {}", s.name),
    })
}

/// The files `glob` names under `root`, relative to it and sorted: `*` and
/// `?` match within one path part; a directory named stands for its files.
fn expand(root: &Path, glob: &str) -> Vec<String> {
    let mut paths = vec![String::new()];
    for part in glob.split('/').filter(|p| !p.is_empty() && *p != ".") {
        paths = paths
            .iter()
            .flat_map(|at| {
                let names = if part.contains(['*', '?']) {
                    entries(&root.join(at))
                        .into_iter()
                        .filter(|n| wild(part.as_bytes(), n.as_bytes()))
                        .filter(|n| !n.starts_with('.') || part.starts_with('.'))
                        .collect()
                } else {
                    vec![part.to_string()]
                };
                names.into_iter().map(move |n| join(at, &n))
            })
            .collect();
    }
    let mut files = Vec::new();
    for p in paths {
        if root.join(&p).is_dir() {
            files.extend(entries(&root.join(&p)).iter().map(|n| join(&p, n)));
        } else {
            files.push(p);
        }
    }
    files.retain(|f| root.join(f).is_file());
    files.sort();
    files.dedup();
    files
}

fn join(at: &str, name: &str) -> String {
    if at.is_empty() {
        name.to_string()
    } else {
        format!("{at}/{name}")
    }
}

fn entries(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| Some(e.ok()?.file_name().to_string_lossy().into_owned()))
        .collect()
}

/// Whether `name` matches `pattern`: `*` any run of bytes, `?` any one.
fn wild(pattern: &[u8], name: &[u8]) -> bool {
    match (pattern.first(), name.first()) {
        (None, None) => true,
        (Some(b'*'), _) => {
            wild(&pattern[1..], name) || (!name.is_empty() && wild(pattern, &name[1..]))
        }
        (Some(p), Some(n)) if *p == b'?' || p == n => wild(&pattern[1..], &name[1..]),
        _ => false,
    }
}
