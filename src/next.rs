//! `truefit next [--all]`: what to do about the repo's ties, as tasks with
//! evidence, steps and an accept command, in the shape of slimfit's `next`
//! (factorium's `[idle]` files them as issues).

use crate::config::{self, CONFIG};
use crate::contract::{self, Standin};
use crate::date;
use crate::host;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// The tasks the ties in `root/truefit.toml` call for: the first, or `all`.
pub fn run(root: &Path, all: bool) -> Result<Value, Vec<Value>> {
    let config = config::load(root)?;
    let globs = config.contract.standins.as_deref().unwrap_or_default();
    let mut notes = Vec::new();
    let mut tasks = Vec::new();
    match &config.contract.standins {
        None => notes.push(format!(
            "no `[contract] standins` in {}: stand-ins without a contract are not looked for; list where the repo's stand-ins live",
            CONFIG
        )),
        Some(globs) => {
            let declared: Vec<PathBuf> = config
                .standin
                .iter()
                .filter_map(|s| host::find(root, &s.programs(root, globs).0)?.canonicalize().ok())
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
    let today = date::days(&date::today()).unwrap_or(0);
    for s in &config.standin {
        let programs = s.programs(root, globs);
        tasks.extend(stale(
            root,
            s,
            programs.1,
            today,
            config.contract.fresh_days,
        ));
        tasks.extend(without_faults(root, s, (&programs.0, programs.1), globs));
        let differ = host::read(root, &contract::differ_file(&s.name));
        if !differ.is_null() {
            tasks.push(differing(s, (&programs.0, programs.1), differ));
        }
    }
    tasks.extend(crate::sim::tasks(root));
    let (stale, unseen) = crate::model::tasks(root, &config.model);
    tasks.extend(stale);
    notes.extend(unseen);
    tasks.extend(crate::promise::tasks(root, &config));
    let (owed, unseen) = crate::history::tasks(root, &config);
    tasks.extend(owed);
    notes.extend(unseen);
    let mut counts = serde_json::Map::new();
    for t in &tasks {
        let n = counts
            .entry(t["kind"].as_str().unwrap_or(""))
            .or_insert(json!(0));
        *n = json!(n.as_u64().unwrap_or(0) + 1);
    }
    let next = match (tasks.is_empty(), all) {
        (true, _) => {
            "nothing: every declared stand-in's contract is fresh and declares its faults, none is without one, no kept seed fails, no model's covered code changed since its commit, every promise is registered, checked, reached and backed, and no file fixed often nor failing seed owes one"
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
        "files": [path, CONFIG],
        "evidence": {"standin": path, "found_by": glob},
        "steps": format!("Read the system's code where it runs the real program `{path}` stands in for (not the stand-in), and declare a [[standin]] in {cfg} with run = [\"{path}\"] and that program as `real`: a case for each command line the system runs and each answer it tells apart (`truefit --help`, what a contract covers). Run `truefit contract` until every case runs, then the accept command where the real program runs; when its cases differ, make the stand-in answer as the real program does. Commit {cfg} and .truefit/.", cfg = CONFIG),
        "accept": format!("truefit contract --real {name}"),
    })
}

/// A `stale-contract` task, if `s`'s last passing real run is missing, older
/// than `fresh_days`, or against another version than the one installed.
fn stale(root: &Path, s: &Standin, real: &str, today: u64, fresh_days: u64) -> Option<Value> {
    let file = contract::record_file(&s.name);
    let record = host::read(root, &file);
    let installed = contract::version(root, s);
    let mut why = Vec::new();
    if record.is_null() {
        why.push("no real run of it has passed".to_string());
    }
    if let Some(passed) = record["date"].as_str() {
        let age = today.saturating_sub(date::days(passed).unwrap_or(0));
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
        "steps": format!("Run the accept command where `{0}` runs. When every case agrees, commit {file}: the contract is fresh. When a case differs, make the stand-in answer as `{0}` does (the differing case's answers are printed), never the case or a normalise rule to hide what the system reads, and run it again. When it is not run, give it what it `needs` here.", real),
        "accept": format!("truefit contract --real {}", s.name),
    }))
}

/// A `standin-without-faults` task, if `s` declares no fault and no
/// `no_faults` kept for the stand-in as it is now.
fn without_faults(
    root: &Path,
    s: &Standin,
    (program, real): (&str, &str),
    globs: &[String],
) -> Option<Value> {
    let (why, sha) = contract::without_faults(root, s, globs)?;
    let kept = s.no_faults.as_ref();
    Some(json!({
        "kind": "standin-without-faults",
        "summary": format!("stand-in {}: {why}", s.name),
        "files": [program, CONFIG],
        "evidence": {"standin": s.name, "run": program, "sha256": sha, "no_faults": kept.map(|k| json!({"why": k.why, "sha256": k.sha256})), "why": why},
        "steps": format!("Read the system's code at each call to `{real}` (not the stand-in). For each call whose error the system handles, or drops, after an earlier step took effect (a token issued, a file written, a message sent), name that failure as a fault: add it to the [[standin]]'s `faults` in {cfg}, make the stand-in answer it when its `fault_env` (default TRUEFIT_FAULT) names it, and declare a [[standin.case]] with `fault` set whose real side fails as `{real}` does (an edit it refuses, an unreachable network; one it cannot be made to fail here gets `ready` and `needs`). Then write the system's test that runs that fault through the stand-in and checks what the system does after it. If no call can fail usefully, set no_faults = {{ why = \"<why>\", sha256 = \"<the evidence's sha256>\" }} on the [[standin]]: it is not asked again until the stand-in changes. Commit {cfg}, the stand-in and the test.", cfg = CONFIG),
        "accept": format!("truefit contract --real {}", s.name),
    }))
}

fn differing(s: &Standin, (program, real): (&str, &str), run: Value) -> Value {
    let file = contract::differ_file(&s.name);
    let cases: Vec<&str> = run["cases"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["case"].as_str())
        .collect();
    json!({
        "kind": "differing-case",
        "summary": format!("stand-in {} answers unlike `{}` in {}", s.name, real, cases.join(", ")),
        "files": [program, file],
        "evidence": {"standin": s.name, "real_run": run},
        "steps": format!("For each case in the evidence, read which parts `differ` and both answers (`standin`, `real`). Fix the stand-in `{}` so it answers as `{}` does: the real program is right. Do not change the case, and add a normalise rule only for what differs from run to run (dates, paths, ids), never for what the system reads. Run the accept command; when every case agrees it removes {file}: commit .truefit/.", program, real),
        "accept": format!("truefit contract --real {}", s.name),
    })
}

/// The files `glob` names under `root`, relative to it and sorted: `*` and
/// `?` match within one path part, `**` any directories (none too); a
/// directory named stands for its files.
pub fn expand(root: &Path, glob: &str) -> Vec<String> {
    let mut paths = vec![String::new()];
    for part in glob.split('/').filter(|p| !p.is_empty() && *p != ".") {
        paths = paths
            .iter()
            .flat_map(|at| {
                if part == "**" {
                    return below(root, at);
                }
                let names = if part.contains(['*', '?']) {
                    entries(&root.join(at))
                        .into_iter()
                        .filter(|n| wild(part.as_bytes(), n.as_bytes()))
                        .filter(|n| !n.starts_with('.') || part.starts_with('.'))
                        .collect()
                } else {
                    vec![part.to_string()]
                };
                names.into_iter().map(|n| join(at, &n)).collect()
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

/// `at` and every directory under it, but those named with a dot.
fn below(root: &Path, at: &str) -> Vec<String> {
    let mut dirs = vec![at.to_string()];
    for n in entries(&root.join(at)) {
        let p = join(at, &n);
        if !n.starts_with('.') && root.join(&p).is_dir() {
            dirs.extend(below(root, &p));
        }
    }
    dirs
}

fn join(at: &str, name: &str) -> String {
    if at.is_empty() {
        name.to_string()
    } else {
        format!("{at}/{name}")
    }
}

pub fn entries(dir: &Path) -> Vec<String> {
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
