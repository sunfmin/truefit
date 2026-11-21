//! `truefit sim`: seeded runs of the system's own simulation. A failing seed
//! is kept in `.truefit/seeds/`, reruns first on every `truefit sim`, and a
//! rerun whose steps differ is the simulation's nondeterminism.

use crate::contract::{self, CONFIG, fault};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Where kept seeds live, one `<seed>.json` each: committed.
pub const SEEDS: &str = ".truefit/seeds";

/// Where the last `truefit sim` keeps which invariants its steps checked
/// (each step's `checked`), by name: committed.
pub const REACHED: &str = ".truefit/sim.reached.json";

/// Each invariant a run's steps checked: how many seeds did, and the first.
type Reach = BTreeMap<String, (u64, String)>;

/// `[sim]`: the command that runs the simulation (the seed is appended), and
/// how many new seeds a plain `truefit sim` runs.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sim {
    pub run: Vec<String>,
    #[serde(default = "default_seeds")]
    seeds: u64,
}

fn default_seeds() -> u64 {
    20
}

pub enum Ask<'a> {
    /// Kept seeds, then new ones: `[sim] seeds` of them, or as given.
    Run(Option<New>),
    Seed(&'a str),
    Forget(&'a str, &'a str),
}

pub enum New {
    Seeds(u64),
    Budget(Duration),
}

pub fn run(root: &Path, ask: Ask) -> Result<(Value, bool), Vec<Value>> {
    let config = contract::load(root)?;
    let seed = match &ask {
        Ask::Seed(s) | Ask::Forget(s, _) => Some(s),
        Ask::Run(_) => None,
    };
    if let Some(s) = seed.filter(|s| {
        s.is_empty()
            || !s
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    }) {
        return Err(vec![fault(
            format!("`{s}` is not a seed"),
            "name a seed by letters, digits, `-` and `_`, as `truefit sim` prints them",
        )]);
    }
    if let Ask::Forget(seed, why) = &ask {
        return forget(root, seed, why);
    }
    let Some(sim) = config.sim else {
        return Err(vec![fault(
            format!("no [sim] in {CONFIG}"),
            format!(
                "declare the simulation in {CONFIG}: [sim] run = [\"<the command that runs it from a seed>\"] (`truefit --help`)"
            ),
        )]);
    };
    let mut reach = Reach::new();
    if let Ask::Seed(seed) = &ask {
        let report = seed_run(root, &sim, seed, &mut reach)?;
        let next = advice(std::slice::from_ref(&report));
        let ok = report["result"] == "passes";
        return Ok((json!({"seed": report, "next": next}), ok));
    }
    let mut reports = Vec::new();
    for (seed, _) in all_kept(root) {
        reports.push(seed_run(root, &sim, &seed, &mut reach)?);
    }
    let kept_reports = reports.len();
    let (start, base) = (Instant::now(), entropy());
    let (mut ran, mut passed) = (0, 0);
    while match &ask {
        Ask::Run(Some(New::Budget(d))) => ran == 0 || start.elapsed() < *d,
        Ask::Run(Some(New::Seeds(n))) => ran < *n,
        _ => ran < sim.seeds,
    } {
        let r = seed_run(root, &sim, &new_seed(base, ran), &mut reach)?;
        ran += 1;
        if r["result"] == "passes" {
            passed += 1;
            continue;
        }
        let broken = r["result"] == "broken";
        reports.push(r);
        // A simulation that ends without a verdict breaks on every seed.
        if broken {
            break;
        }
    }
    let reached: serde_json::Map<String, Value> = reach
        .into_iter()
        .map(|(n, (seeds, seed))| (n, json!({"seeds": seeds, "seed": seed})))
        .collect();
    let record =
        json!({"date": contract::today(), "seeds": kept_reports as u64 + ran, "reached": reached});
    contract::write(root, REACHED, &record).map_err(|e| {
        vec![fault(
            format!("cannot write {REACHED}: {e}"),
            "make .truefit/ a directory truefit can write",
        )]
    })?;
    let next = advice(&reports);
    let ok = reports.iter().all(|r| r["result"] == "passes");
    let failing = reports.split_off(kept_reports);
    Ok((
        json!({"kept": reports, "new": {"ran": ran, "passed": passed, "failing": failing}, "reached": record["reached"], "next": next}),
        ok,
    ))
}

/// What one run of the simulation printed: its lines, each JSON (or the line
/// as a string when it is not), the last of them the verdict.
struct Run {
    lines: Vec<Value>,
    exit: Option<i32>,
    stderr: String,
}

impl Run {
    fn verdict(&self) -> Option<&Value> {
        self.lines
            .last()
            .filter(|v| matches!(v["verdict"].as_str(), Some("pass" | "fail")))
    }
}

fn once(root: &Path, sim: &Sim, seed: &str) -> Result<Run, Vec<Value>> {
    let program = contract::find(root, &sim.run[0]).ok_or_else(|| {
        vec![fault(
            format!("the simulation `{}` is not found", sim.run[0]),
            format!("correct [sim] run in {CONFIG}: a path relative to it, or a program on PATH"),
        )]
    })?;
    let out = Command::new(&program)
        .args(&sim.run[1..])
        .arg(seed)
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| {
            vec![fault(
                format!("cannot run `{}`: {e}", program.display()),
                "make it an executable program",
            )]
        })?;
    let lines = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|_| Value::String(l.into())))
        .collect();
    let stderr = String::from_utf8_lossy(&out.stderr);
    let tail: Vec<&str> = stderr.lines().rev().take(10).collect();
    Ok(Run {
        lines,
        exit: out.status.code(),
        stderr: tail.into_iter().rev().collect::<Vec<_>>().join("\n"),
    })
}

/// One seed's run, noted in `reach`: a kept one rerun, any other fresh.
fn seed_run(root: &Path, sim: &Sim, seed: &str, reach: &mut Reach) -> Result<Value, Vec<Value>> {
    let a = once(root, sim, seed)?;
    note(reach, seed, &a.lines);
    match kept(root, seed) {
        Some(k) => rerun(root, sim, seed, &k, a),
        None => fresh(root, sim, seed, a),
    }
}

/// A seed no kept record holds, run as `a`: when it fails, run it again to
/// see that it reruns alike, and keep it.
fn fresh(root: &Path, sim: &Sim, seed: &str, a: Run) -> Result<Value, Vec<Value>> {
    let Some(verdict) = a.verdict() else {
        return Ok(broken(seed, &a));
    };
    if verdict["verdict"] == "pass" {
        return Ok(
            json!({"seed": seed, "result": "passes", "verdict": verdict, "steps": a.lines.len() - 1, "rerun": rerun_command(seed)}),
        );
    }
    let b = once(root, sim, seed)?;
    let record = json!({
        "seed": seed,
        "found": contract::today(),
        "verdict": verdict,
        "last": verdict,
        "nondeterministic": difference(&a.lines, &b.lines),
        "lines": a.lines,
    });
    write(root, seed, &record)?;
    Ok(report(seed, &record, Value::Null))
}

/// A kept seed, run again as `a`. Its steps as kept: the run is the same. Otherwise
/// it runs once more: two runs that differ are nondeterminism; two alike are
/// the system's code changed since, and they replace the kept steps.
fn rerun(root: &Path, sim: &Sim, seed: &str, k: &Value, a: Run) -> Result<Value, Vec<Value>> {
    let then = k["lines"].as_array().cloned().unwrap_or_default();
    let Some(verdict) = a.verdict() else {
        return Ok(broken(seed, &a));
    };
    let mut record = k.clone();
    let mut changed = Value::Null;
    if a.lines != then || !k["nondeterministic"].is_null() {
        let b = once(root, sim, seed)?;
        record["nondeterministic"] = difference(&a.lines, &b.lines);
        if record["nondeterministic"].is_null() {
            changed = difference(&then, &a.lines);
            record["lines"] = a.lines.clone().into();
        }
    }
    if record["nondeterministic"].is_null() {
        record["last"] = verdict.clone();
    }
    if &record != k {
        write(root, seed, &record)?;
    }
    Ok(report(seed, &record, changed))
}

fn report(seed: &str, record: &Value, changed: Value) -> Value {
    let differs = &record["nondeterministic"];
    let result = if !differs.is_null() {
        "nondeterministic"
    } else if record["last"]["verdict"] == "pass" {
        "passes"
    } else {
        "fails"
    };
    let mut r = json!({"seed": seed, "result": result, "verdict": record["last"], "steps": steps(record),
                       "kept": file(seed), "rerun": rerun_command(seed)});
    if !differs.is_null() {
        r["differs"] = differs.clone();
    }
    if !changed.is_null() {
        r["changed"] = changed;
    }
    r
}

fn broken(seed: &str, run: &Run) -> Value {
    json!({"seed": seed, "result": "broken", "why": "its last line is no verdict",
           "last_line": run.lines.last(), "exit": run.exit, "stderr": run.stderr,
           "rerun": rerun_command(seed)})
}

/// The first line two runs differ at, numbered from 1, with each run's line
/// there (`then` the earlier run's, `now` the later's), or null: none.
fn difference(then: &[Value], now: &[Value]) -> Value {
    (0..then.len().max(now.len()))
        .find(|&i| then.get(i) != now.get(i))
        .map_or(
            Value::Null,
            |i| json!({"line": i + 1, "then": then.get(i), "now": now.get(i)}),
        )
}

fn advice(reports: &[Value]) -> String {
    let any = |result: &str| reports.iter().any(|r| r["result"] == result);
    let mut next = Vec::new();
    if any("nondeterministic") {
        next.push("make the simulation deterministic: all it chooses must come from the seed (one clock, one seeded random source; no wall time, process ids, threads' order or unordered iteration). `differs` is the first line two runs of one seed differ at; run its `rerun` command until it is gone");
    }
    if any("broken") {
        next.push("make the simulation end every run with a verdict line, a panic of the system's code caught as a broken invariant; then run its `rerun` command");
    }
    if any("fails") {
        next.push("fix the bug each failing seed reaches: run its `rerun` command and read its steps to where its invariant breaks; fix the system's code, not the invariant or the simulation; commit .truefit/seeds/");
    }
    if next.is_empty() {
        next.push("nothing: every seed passed; commit .truefit/seeds/ when it changed");
    }
    next.join("; ")
}

fn forget(root: &Path, seed: &str, why: &str) -> Result<(Value, bool), Vec<Value>> {
    let file = file(seed);
    let Some(k) = kept(root, seed) else {
        return Err(vec![fault(
            format!("seed {seed} is not kept ({file})"),
            "name a seed `truefit sim` keeps",
        )]);
    };
    if k["last"]["verdict"] != "pass" || !k["nondeterministic"].is_null() {
        return Err(vec![fault(
            format!("seed {seed} did not pass when last run"),
            format!(
                "a kept seed is forgotten only once it passes: fix what it reaches, run `{}`, then forget it",
                rerun_command(seed)
            ),
        )]);
    }
    if why.trim().is_empty() {
        return Err(vec![fault(
            "no reason given",
            "say why the seed no longer earns its run: --why \"<reason>\"",
        )]);
    }
    std::fs::remove_file(root.join(&file)).map_err(|e| {
        vec![fault(
            format!("cannot remove {file}: {e}"),
            "remove it by hand",
        )]
    })?;
    let next = format!(
        "commit {SEEDS}/ with the reason: git commit -m \"truefit sim: forget seed {seed}: {why}\""
    );
    Ok((json!({"forgot": seed, "why": why, "next": next}), true))
}

/// The tasks kept seeds call for: the simulation's nondeterminism, then each
/// kept seed that failed when last run.
pub fn tasks(root: &Path) -> Vec<Value> {
    let kept = all_kept(root);
    let mut tasks = Vec::new();
    let nondeterministic: Vec<Value> = kept
        .iter()
        .filter(|(_, k)| !k["nondeterministic"].is_null())
        .map(
            |(seed, k)| json!({"seed": seed, "differs": k["nondeterministic"], "kept": file(seed)}),
        )
        .collect();
    if !nondeterministic.is_empty() {
        let seeds: Vec<&str> = nondeterministic
            .iter()
            .filter_map(|n| n["seed"].as_str())
            .collect();
        tasks.push(json!({
            "kind": "nondeterministic-sim",
            "summary": format!("the simulation runs seed {} unalike twice", seeds.join(", ")),
            "files": [CONFIG, SEEDS],
            "evidence": {"seeds": nondeterministic},
            "steps": "For each seed in the evidence, `differs` is the first line two runs of it printed unalike (`then`, `now`). Find what the simulation chooses there not from the seed (wall time, a process id, threads' order, a hash map's order, a random source of its own) and make it come from the seed: one clock, one seeded random source. Run the accept command: it reruns every kept seed twice where their steps differ, and clears each one whose two runs agree. Commit .truefit/seeds/.",
            "accept": "truefit sim --seeds 0",
        }));
    }
    for (seed, k) in &kept {
        if k["last"]["verdict"] == "fail" && k["nondeterministic"].is_null() {
            let v = &k["last"];
            tasks.push(json!({
                "kind": "failing-seed",
                "summary": format!("seed {seed} breaks invariant {} at step {}", v["invariant"].as_str().unwrap_or("?"), v["step"]),
                "files": [file(seed)],
                "evidence": {"seed": seed, "found": k["found"], "verdict": v, "steps": steps(k)},
                "steps": format!("Run `{}` and read its steps (in {}) to where the invariant breaks: the system's bug is there. Fix the system's code, not the invariant or the simulation, so the seed passes. It stays kept, rerun first on every `truefit sim` as a regression. Commit the fix with .truefit/seeds/.", rerun_command(seed), file(seed)),
                "accept": rerun_command(seed),
            }));
        }
    }
    tasks
}

/// Notes each invariant a run's steps name in their `checked`.
fn note(reach: &mut Reach, seed: &str, lines: &[Value]) {
    let names: std::collections::BTreeSet<&str> = lines
        .iter()
        .flat_map(|l| l["checked"].as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .collect();
    for n in names {
        reach.entry(n.into()).or_insert((0, seed.into())).0 += 1;
    }
}

/// Why the last `truefit sim` did not reach invariant `name`, or None: it did.
pub fn unreached(root: &Path, name: &str, fresh_days: u64) -> Option<String> {
    let r = contract::read(root, REACHED);
    let Some(date) = r["date"].as_str() else {
        return Some(format!("sim: no `truefit sim` run is recorded ({REACHED})"));
    };
    let age = contract::days(&contract::today())
        .unwrap_or(0)
        .saturating_sub(contract::days(date).unwrap_or(0));
    if age > fresh_days {
        return Some(format!(
            "sim: the last `truefit sim` ran {age} days ago, on {date}: over fresh_days ({fresh_days})"
        ));
    }
    let reached = r["reached"].as_object().into_iter().flatten();
    if reached
        .into_iter()
        .any(|(n, _)| crate::promise::same_name(n, name))
    {
        return None;
    }
    Some(format!(
        "sim: no step of the last `truefit sim` ({} seeds, on {date}) checked `{name}`",
        r["seeds"]
    ))
}

/// The steps a kept record holds: its lines but the verdict.
fn steps(record: &Value) -> usize {
    record["lines"]
        .as_array()
        .map_or(0, |l| l.len().saturating_sub(1))
}

fn rerun_command(seed: &str) -> String {
    format!("truefit sim --seed {seed}")
}

fn file(seed: &str) -> String {
    format!("{SEEDS}/{seed}.json")
}

fn kept(root: &Path, seed: &str) -> Option<Value> {
    Some(contract::read(root, &file(seed))).filter(|k| !k.is_null())
}

/// Every kept seed, by name; a file in .truefit/seeds/ that is no record is none.
fn all_kept(root: &Path) -> Vec<(String, Value)> {
    let mut names = crate::next::entries(&root.join(SEEDS));
    names.sort();
    names
        .iter()
        .filter_map(|n| n.strip_suffix(".json"))
        .filter_map(|s| Some((s.to_string(), kept(root, s)?)))
        .collect()
}

fn write(root: &Path, seed: &str, record: &Value) -> Result<(), Vec<Value>> {
    let file = file(seed);
    contract::write(root, &file, record).map_err(|e| {
        vec![fault(
            format!("cannot write {file}: {e}"),
            format!("make {SEEDS}/ a directory truefit can write"),
        )]
    })
}

fn entropy() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    nanos ^ (u64::from(std::process::id()) << 32)
}

/// The `n`th new seed from `base`: splitmix64, cut to below 2^32 so any
/// simulation's random source takes it whole.
fn new_seed(base: u64, n: u64) -> String {
    let mut z = base.wrapping_add(n.wrapping_add(1).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    ((z ^ (z >> 31)) >> 32).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn difference_names_the_first_line_that_differs() {
        let (one, two) = (vec![json!(1)], vec![json!(1), json!(2)]);
        assert!(difference(&one, &one).is_null());
        assert_eq!(
            difference(&[json!(1), json!(1)], &two),
            json!({"line": 2, "then": 1, "now": 2})
        );
        assert_eq!(
            difference(&one, &[]),
            json!({"line": 1, "then": 1, "now": null})
        );
    }

    #[test]
    fn new_seeds_differ_and_stay_below_2_32() {
        let seeds: Vec<u64> = (0..50).map(|n| new_seed(7, n).parse().unwrap()).collect();
        assert!(seeds.iter().all(|s| *s < 1 << 32));
        let mut unique = seeds.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), seeds.len());
        assert_eq!(new_seed(7, 3), new_seed(7, 3));
    }
}
