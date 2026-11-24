//! `truefit model`: the repo's TLA+ models checked by TLC, and a run's records
//! checked by TLC as a behaviour of one of them.

use crate::contract::{self, CONFIG, fault};
use crate::date;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The one TLC truefit runs: its release and the SHA-256 of its `tla2tools.jar`.
pub const TLC_VERSION: &str = "1.7.4";
const TLC_SHA256: &str = "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88";

/// Each model's committed traces: `<name>/accept/`, runs it must allow, and
/// `<name>/reject/`, runs that break a promise, which it must not.
pub const TRACES: &str = ".truefit/traces";

/// Where each model's last `truefit model` is recorded, with the commit.
fn record_file(name: &str) -> String {
    format!(".truefit/models/{name}.json")
}

/// `[[model]]`: a spec, its TLC config, its time limit, and its coverage
/// record: the code each action stands for, as of `commit`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub name: String,
    spec: String,
    config: String,
    #[serde(default = "default_limit")]
    limit: u64,
    #[serde(default = "default_init")]
    init: String,
    #[serde(default = "default_next")]
    next: String,
    #[serde(default)]
    commit: String,
    #[serde(default)]
    covers: Vec<Cover>,
    /// Each mapped variable of the spec, as a TLA+ expression over a record `r`.
    #[serde(default)]
    trace: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cover {
    action: String,
    file: String,
    function: String,
}

fn default_limit() -> u64 {
    300
}
fn default_init() -> String {
    "Init".into()
}
fn default_next() -> String {
    "Next".into()
}

/// What is wrong in the `[[model]]` declarations.
pub fn faults(models: &[Model]) -> Vec<Value> {
    let mut faults = Vec::new();
    for (i, m) in models.iter().enumerate() {
        let n = &m.name;
        if models[..i].iter().any(|o| &o.name == n) {
            faults.push(fault(
                format!("model `{n}` is declared twice"),
                "give each [[model]] its own name",
            ));
        }
        if m.covers.is_empty() || m.commit.is_empty() {
            faults.push(fault(
                format!("model `{n}` has no coverage record"),
                "declare which code each action stands for, `covers = [{ action = \"Next\", file = \"src/x.rs\", function = \"step\" }]`, and the `commit` you confirmed it at",
            ));
        }
        if m.limit == 0 {
            faults.push(fault(
                format!("model `{n}` has a limit of 0 seconds"),
                "give TLC the seconds it may take on it, e.g. limit = 60",
            ));
        }
    }
    faults
}

/// Checks each model (or the one named); with `trace`, checks that file's
/// records as a behaviour of the model.
pub fn run(
    root: &Path,
    name: Option<&str>,
    trace: Option<&str>,
) -> Result<(Value, bool), Vec<Value>> {
    let models = contract::load(root)?.model;
    if models.is_empty() {
        return Err(vec![fault(
            format!("no [[model]] in {CONFIG}"),
            format!(
                "write a TLA+ model of the system's design and declare it in {CONFIG} (`truefit --help`, models)"
            ),
        )]);
    }
    let chosen: Vec<&Model> = models
        .iter()
        .filter(|m| name.is_none_or(|n| m.name == n))
        .filter(|m| trace.is_none() || !m.trace.is_empty())
        .collect();
    let chosen = match (name, trace, chosen.as_slice()) {
        (Some(n), _, []) => Err(fault(
            format!("no model `{n}` in {CONFIG}{}", if trace.is_some() { " with a [model.trace] mapping" } else { "" }),
            format!("name a [[model]] of {CONFIG}; a trace needs its [model.trace]"),
        )),
        (None, Some(_), []) => Err(fault(
            "no model declares a [model.trace] mapping",
            "map each of the model's variables to a record's fields: [model.trace] x = \"r.state.x\"",
        )),
        (None, Some(_), [_, _, ..]) => Err(fault(
            "more than one model maps traces",
            "name the model: truefit model <name> --trace <file>",
        )),
        _ => Ok(chosen),
    }
    .map_err(|f| vec![f])?;
    let (jar, installed) = tlc()?;
    let tlc = json!({"version": TLC_VERSION, "sha256": TLC_SHA256, "jar": jar, "installed_now": installed});
    if let Some(file) = trace {
        let report = check_trace(root, chosen[0], &jar, file, Against::Next)?;
        let ok = report["result"] == "allowed";
        let next = if ok {
            "nothing: the model allows this run".to_string()
        } else if report["result"] == "disallowed" {
            format!(
                "the run took a step the model does not allow: read `record` against `after`. When the code is right and the design changed, change the model `{}` and its coverage; when the design is right, the code has a bug there. Then `truefit model {} --trace {file}`",
                chosen[0].spec, chosen[0].name
            )
        } else {
            format!(
                "correct [model.trace] of `{}` so TLC can read each record (`output`), then run it again",
                chosen[0].name
            )
        };
        return Ok((json!({"trace": report, "tlc": tlc, "next": next}), ok));
    }
    let mut reports = Vec::new();
    for m in chosen {
        reports.push(check_backed(root, m, &jar)?);
    }
    let any = |r: &str| reports.iter().any(|x| x["result"] == r);
    let mut next = Vec::new();
    if any("fails") {
        next.push("read each failing model's `behaviour` to where its property breaks: a bug in the design (fix the code and the model alike) or a modelling error (fix the model, never weaken the property to pass)");
    }
    if any("timed out") {
        next.push("a model TLC cannot finish within its limit: make its bounds smaller (constants, a state constraint), not the limit larger");
    }
    if any("broken") {
        next.push("TLC could not check a model: correct the spec or config by its `output`");
    }
    if reports.iter().any(|r| r.get("wrong").is_some()) {
        next.push("a committed trace the model now answers otherwise (`wrong`): an accept trace disallowed is a guard too strict, a reject trace allowed or breaking no invariant is a model too permissive or a property too weak. Fix the model or the code; never weaken the property, or drop the trace, to pass");
    }
    let ok = next.is_empty();
    if ok {
        next.push("nothing: every model passes; check real runs against them with `truefit model --trace <file>`");
    }
    Ok((
        json!({"models": reports, "tlc": tlc, "next": next.join("; ")}),
        ok,
    ))
}

/// TLC's jar in `$HOME`, at the pinned release and checksum: downloaded once
/// (from `TRUEFIT_TLC_FROM` when set, else the official release), and whether
/// it was just now.
fn tlc() -> Result<(PathBuf, bool), Vec<Value>> {
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
        vec![fault(
            "HOME is not set",
            "run truefit with HOME set: TLC is installed under it",
        )]
    })?;
    let dir = home.join(".local/share/truefit/tlc").join(TLC_VERSION);
    let jar = dir.join("tla2tools.jar");
    if sha256(&jar).as_deref() == Some(TLC_SHA256) {
        return Ok((jar, false));
    }
    let from = std::env::var("TRUEFIT_TLC_FROM").unwrap_or_else(|_| {
        format!("https://github.com/tlaplus/tlaplus/releases/download/v{TLC_VERSION}/tla2tools.jar")
    });
    let part = dir.join(format!("tla2tools.jar.{}.part", std::process::id()));
    let cannot = |why: String| {
        vec![fault(
            format!("cannot install TLC {TLC_VERSION} from {from}: {why}"),
            "let truefit reach it (or set TRUEFIT_TLC_FROM to a copy of that release's tla2tools.jar), then run it again",
        )]
    };
    std::fs::create_dir_all(&dir).map_err(|e| cannot(e.to_string()))?;
    let got = Command::new("curl")
        .args(["-fsSL", "--retry", "2", "-o"])
        .arg(&part)
        .arg(&from)
        .status()
        .map_err(|e| cannot(format!("curl: {e}")))?;
    let sum = sha256(&part);
    if !got.success() || sum.as_deref() != Some(TLC_SHA256) {
        let _ = std::fs::remove_file(&part);
        return Err(cannot(match sum {
            Some(s) if got.success() => format!("its SHA-256 is {s}, not {TLC_SHA256}"),
            _ => format!("curl {got}"),
        }));
    }
    std::fs::rename(&part, &jar).map_err(|e| cannot(e.to_string()))?;
    Ok((jar, true))
}

pub fn sha256(path: &Path) -> Option<String> {
    Some(digest(&std::fs::read(path).ok()?))
}

fn digest(bytes: &[u8]) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// What one TLC run printed, what that says, whether it completed without
/// an error, and how long it took.
struct Tlc {
    output: String,
    read: Read,
    completed: bool,
    seconds: f64,
    timed_out: bool,
}

/// Runs TLC on `spec` (the model `m`'s modules beside it) in `work`, then removes `work`.
fn run_tlc(
    jar: &Path,
    root: &Path,
    m: &Model,
    work: PathBuf,
    cfg: &Path,
    spec: &Path,
) -> Result<Tlc, Vec<Value>> {
    let lib = root.join(&m.spec);
    let log = work.join("tlc.out");
    let cannot = |e: std::io::Error| {
        vec![fault(
            format!("cannot run TLC (java): {e}"),
            "put a Java 11 or later `java` on PATH: TLC runs on it",
        )]
    };
    let file = std::fs::File::create(&log).map_err(cannot)?;
    let start = Instant::now();
    let mut child = Command::new("java")
        .arg("-XX:+UseParallelGC")
        // Its own temporary files: TLC runs at once share none.
        .arg(format!("-Djava.io.tmpdir={}", work.display()))
        .arg(format!(
            "-DTLA-Library={}",
            lib.parent().unwrap_or(root).display()
        ))
        .arg("-cp")
        .arg(jar)
        .args(["tlc2.TLC", "-metadir"])
        .arg(work.join("states"))
        .arg("-config")
        .arg(cfg)
        .arg(spec)
        .current_dir(&work)
        .stdin(Stdio::null())
        .stdout(file.try_clone().map_err(cannot)?)
        .stderr(file)
        .spawn()
        .map_err(cannot)?;
    let timed_out = loop {
        if child.try_wait().map_err(cannot)?.is_some() {
            break false;
        }
        if start.elapsed() > Duration::from_secs(m.limit) {
            let _ = child.kill();
            let _ = child.wait();
            break true;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let output = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&work);
    Ok(Tlc {
        read: read(&output),
        completed: output.contains("Model checking completed. No error has been found."),
        output,
        seconds: (start.elapsed().as_secs_f64() * 100.0).round() / 100.0,
        timed_out,
    })
}

/// A fresh directory for one TLC run's files, outside the repo.
fn workdir() -> Result<PathBuf, Vec<Value>> {
    contract::scratch().map_err(|e| vec![fault(e, "free the temporary directory")])
}

fn paths(root: &Path, m: &Model) -> Result<(PathBuf, PathBuf), Vec<Value>> {
    let found = |p: &str| {
        Some(root.join(p))
            .filter(|p| p.is_file())
            .and_then(|p| p.canonicalize().ok())
    };
    match (found(&m.spec), found(&m.config)) {
        (Some(spec), Some(cfg)) => Ok((spec, cfg)),
        _ => Err(vec![fault(
            format!(
                "model `{}`: `{}` or `{}` is not found",
                m.name, m.spec, m.config
            ),
            format!("correct its spec and config in {CONFIG}: paths relative to it"),
        )]),
    }
}

/// The traces committed under `TRACES/<name>/<kind>/`, sorted.
fn traces(root: &Path, name: &str, kind: &str) -> Vec<String> {
    let dir = format!("{TRACES}/{name}/{kind}");
    let mut files: Vec<String> = crate::next::entries(&root.join(&dir))
        .into_iter()
        .filter(|n| !n.starts_with('.'))
        .map(|n| format!("{dir}/{n}"))
        .filter(|f| root.join(f).is_file())
        .collect();
    files.sort();
    files
}

/// Checks `m` with TLC and, at once, each committed trace: an accept trace
/// must be allowed; a reject trace disallowed, and its records must break an
/// invariant of the config. Any that is not fails the model. Records the
/// result, with the commit, in `record_file`.
fn check_backed(root: &Path, m: &Model, jar: &Path) -> Result<Value, Vec<Value>> {
    let (accept, reject) = (
        traces(root, &m.name, "accept"),
        traces(root, &m.name, "reject"),
    );
    let (r, accepted, rejected) = std::thread::scope(|s| {
        let tlc = s.spawn(|| check(root, m, jar));
        let run = |f, against| spawn_trace(s, root, m, jar, f, against);
        let a: Vec<_> = accept.iter().map(|f| run(f, Against::Next)).collect();
        let b: Vec<_> = reject
            .iter()
            .map(|f| (run(f, Against::Next), run(f, Against::Invariants)))
            .collect();
        let join = |h: std::thread::ScopedJoinHandle<'_, _>| h.join().unwrap();
        let a: Result<Vec<Value>, Vec<Value>> = a.into_iter().map(join).collect();
        let b: Result<Vec<(Value, Value)>, Vec<Value>> = b
            .into_iter()
            .map(|(n, i)| Ok((join(n)?, join(i)?)))
            .collect();
        (join(tlc), a, b)
    });
    let (mut r, accepted, rejected) = (r?, accepted?, rejected?);
    let mut wrong = Vec::new();
    for t in &accepted {
        if t["result"] != "allowed" {
            wrong.push(format!(
                "accept trace {} is {}{}: the model no longer allows this run",
                t["file"].as_str().unwrap_or(""),
                t["result"].as_str().unwrap_or(""),
                at(t)
            ));
        }
    }
    let mut reject_reports = Vec::new();
    for (mut t, inv) in rejected {
        let file = t["file"].as_str().unwrap_or("").to_string();
        if t["result"] == "allowed" {
            wrong.push(format!(
                "reject trace {file} is allowed: the model is too permissive"
            ));
        } else if t["result"] != "disallowed" {
            wrong.push(format!(
                "reject trace {file} is {}",
                t["result"].as_str().unwrap_or("")
            ));
        }
        match inv["result"].as_str() {
            Some("breaks") => {}
            Some("holds") => wrong.push(format!(
                "reject trace {file} breaks none of {}'s invariants: the property is too weak to catch it",
                m.config
            )),
            other => wrong.push(format!(
                "reject trace {file}: its invariants' check is {}{}",
                other.unwrap_or(""),
                inv["output"].as_str().map_or(String::new(), |o| format!(": {o}"))
            )),
        }
        t["breaks"] = inv["breaks"].clone();
        t["breaks_at"] = inv["step"].clone();
        reject_reports.push(t);
    }
    r["traces"] = json!({"accept": accepted, "reject": reject_reports});
    if !wrong.is_empty() {
        if r["result"] == "passes" {
            r["result"] = "fails".into();
            r["error"] = wrong.join("; ").into();
        }
        r["wrong"] = wrong.into();
    }
    let brief = |ts: &[Value]| -> Vec<Value> {
        ts.iter()
            .map(|t| json!({"file": t["file"], "result": t["result"], "step": t["step"], "breaks": t.get("breaks")}))
            .collect()
    };
    let record = json!({
        "commit": contract::git(root, &["rev-parse", "HEAD"]).map(|c| c.trim().to_string()),
        "date": date::today(),
        "inputs": inputs(root, m),
        "result": r["result"],
        "traces": {"accept": brief(&accepted), "reject": brief(&reject_reports)},
    });
    contract::save(root, &record_file(&m.name), &record)?;
    Ok(r)
}

/// A trace's check, on a thread of `s`.
fn spawn_trace<'s, 'e>(
    s: &'s std::thread::Scope<'s, 'e>,
    root: &'e Path,
    m: &'e Model,
    jar: &'e Path,
    file: &'e str,
    against: Against,
) -> std::thread::ScopedJoinHandle<'s, Result<Value, Vec<Value>>> {
    s.spawn(move || check_trace(root, m, jar, file, against))
}

/// ` at step N` when a trace report names the step it stopped at.
fn at(t: &Value) -> String {
    t.get("step")
        .map_or(String::new(), |s| format!(" at step {s}"))
}

/// The SHA-256 of what a model's check reads: its declaration, spec, config
/// and committed traces. A record of other inputs is not this model's.
fn inputs(root: &Path, m: &Model) -> String {
    let mut all = format!("{}\n{}\n{:?}\n", m.init, m.next, m.trace).into_bytes();
    let mut files = vec![m.spec.clone(), m.config.clone()];
    files.extend(traces(root, &m.name, "accept"));
    files.extend(traces(root, &m.name, "reject"));
    for f in files {
        all.extend(f.as_bytes());
        all.extend(std::fs::read(root.join(&f)).unwrap_or_default());
    }
    digest(&all)
}

/// Why model `model`'s record does not back promise `promise`, or None: its
/// last check, of these inputs, passed with a trace accepted and a trace
/// rejected that breaks the promise's invariant.
pub fn unbacked(root: &Path, models: &[Model], model: &str, promise: &str) -> Option<String> {
    let m = models.iter().find(|m| m.name == model)?;
    let file = record_file(model);
    let r = contract::read(root, &file);
    let mut why = Vec::new();
    if r.is_null() {
        return Some(format!(
            "model {model}: no `truefit model {model}` run is recorded ({file})"
        ));
    }
    if r["inputs"] != inputs(root, m).as_str() {
        why.push("its spec, config or traces changed since its last `truefit model`".to_string());
    }
    if r["result"] != "passes" {
        why.push(format!(
            "its last check {}",
            r["result"].as_str().unwrap_or("did not finish")
        ));
    }
    let traces = |kind: &str| r["traces"][kind].as_array().cloned().unwrap_or_default();
    if !traces("accept").iter().any(|t| t["result"] == "allowed") {
        why.push(format!(
            "no accept trace it allows ({TRACES}/{model}/accept/)"
        ));
    }
    let breaks = |t: &Value| {
        t["breaks"]
            .as_str()
            .is_some_and(|b| crate::promise::same_name(b, promise))
    };
    if !traces("reject")
        .iter()
        .any(|t| t["result"] == "disallowed" && breaks(t))
    {
        why.push(format!(
            "no reject trace it disallows that breaks `{promise}` ({TRACES}/{model}/reject/)"
        ));
    }
    (!why.is_empty()).then(|| format!("model {model}: {}", why.join("; ")))
}

fn check(root: &Path, m: &Model, jar: &Path) -> Result<Value, Vec<Value>> {
    let (spec, cfg) = paths(root, m)?;
    let t = run_tlc(jar, root, m, workdir()?, &cfg, &spec)?;
    let mut r = json!({"name": m.name, "spec": m.spec, "seconds": t.seconds, "limit": m.limit});
    let read = t.read;
    if t.timed_out {
        r["result"] = "timed out".into();
    } else if t.completed {
        r["result"] = "passes".into();
    } else if let Some(error) = read.error.filter(|_| !read.behaviour.is_empty()) {
        // An error with no behaviour reaching it (a parse error) is no failing property.
        r["result"] = "fails".into();
        r["error"] = error.into();
        r["behaviour"] = read.behaviour.into();
    } else {
        r["result"] = "broken".into();
    }
    r["states"] = read.states.into();
    r["depth"] = read.depth.into();
    if r["result"] == "broken" || r["result"] == "timed out" {
        r["output"] = tail(&t.output).into();
    }
    Ok(r)
}

/// What a TLC run's output says: its first error, the behaviour that reached
/// it (each state's action and variables), distinct states and depth.
struct Read {
    error: Option<String>,
    behaviour: Vec<Value>,
    states: Option<u64>,
    depth: Option<u64>,
}

fn read(output: &str) -> Read {
    // From TLC's final summary, a line of its own: never a `Progress(n)` line
    // before it. TLC writes large numbers with commas (29,705).
    let number = |pattern: &str| {
        regex_lite::Regex::new(&format!("(?m)^{pattern}"))
            .unwrap()
            .captures_iter(output)
            .last()
            .and_then(|c| c[1].replace(',', "").parse().ok())
    };
    let mut behaviour: Vec<Value> = Vec::new();
    // Within a state's block: from its `State n:` line to the blank line after.
    let (mut within, mut last) = (false, String::new());
    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("State ") {
            let action = rest.split_once(": ").map_or("", |(_, a)| a);
            behaviour.push(json!({"action": action.trim_matches(['<', '>']), "state": {}}));
            within = true;
        } else if line.trim().is_empty() {
            within = false;
        } else if let (true, Some(s)) = (within, behaviour.last_mut()) {
            let state = s["state"].as_object_mut().unwrap();
            let first = line.starts_with("/\\ ") || state.is_empty();
            match line.trim_start_matches("/\\ ").split_once(" = ") {
                Some((var, value)) if first => {
                    state.insert(var.into(), value.into());
                    last = var.into();
                }
                // A long value TLC wraps onto the lines after its variable's.
                _ => {
                    if let Some(Value::String(v)) = state.get_mut(&last) {
                        v.push(' ');
                        v.push_str(line.trim());
                    }
                }
            }
        }
    }
    Read {
        error: output
            .lines()
            .filter_map(|l| l.strip_prefix("Error: "))
            .find(|l| !l.starts_with("The behavior up to this point"))
            .map(String::from),
        behaviour,
        states: number(r"[\d,]+ states generated, ([\d,]+) distinct states found"),
        depth: number(r"The depth of the complete state graph search is ([\d,]+)"),
    }
}

/// What a run that did not finish says: from its first error, or its last lines.
fn tail(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    let from = lines
        .iter()
        .position(|l| l.starts_with("Error:"))
        .unwrap_or(lines.len().saturating_sub(30));
    lines[from..]
        .iter()
        .take(30)
        .copied()
        .collect::<Vec<_>>()
        .join("\n")
}

/// A run's records: each line of `file` that is a JSON object with a `step`
/// and no `verdict` (a simulation's last line), or the `lines` of a kept
/// seed (`.truefit/seeds/<seed>.json`).
fn records(root: &Path, file: &str) -> Result<Vec<Value>, Vec<Value>> {
    let text = std::fs::read_to_string(root.join(file)).map_err(|e| {
        vec![fault(
            format!("cannot read trace {file}: {e}"),
            "name a file of a run's records, one JSON line per step",
        )]
    })?;
    let lines: Vec<Value> = match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(kept)) if kept.contains_key("lines") => {
            kept["lines"].as_array().cloned().unwrap_or_default()
        }
        _ => text
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect(),
    };
    let records: Vec<Value> = lines
        .into_iter()
        .filter(|l| l.get("step").is_some() && l.get("verdict").is_none())
        .collect();
    if records.is_empty() {
        return Err(vec![fault(
            format!("trace {file} holds no record"),
            "give it the run's step lines: {\"step\": 1, \"event\": \"...\", \"state\": {...}}, one per line",
        )]);
    }
    Ok(records)
}

/// What a trace is checked against: the model's next-state action, or the
/// config's invariants on its records' states alone.
#[derive(Clone, Copy)]
enum Against {
    Next,
    Invariants,
}

/// Checks the records of `file` against `m`: from its initial state, each
/// record one step, the variables `[model.trace]` maps equal to the
/// record's. Against `Next`, each step is one of its next-state action; TLC
/// searches every way the unmapped variables may go, and the trace is
/// allowed when one reaches its end. Against `Invariants`, the unmapped
/// variables keep their initial values, and the trace `breaks` the first of
/// the config's invariants one of its states violates, or `holds`.
fn check_trace(
    root: &Path,
    m: &Model,
    jar: &Path,
    file: &str,
    against: Against,
) -> Result<Value, Vec<Value>> {
    let records = records(root, file)?;
    let (spec, cfg) = paths(root, m)?;
    let module = spec.file_stem().map_or("".into(), |s| s.to_string_lossy());
    let config = std::fs::read_to_string(&cfg).unwrap_or_default();
    let (step, invariants) = match against {
        Against::Next => (m.next.clone(), vec!["TruefitUnfinished".to_string()]),
        Against::Invariants => {
            let text = std::fs::read_to_string(&spec).unwrap_or_default();
            let free: Vec<String> = variables(&text)
                .into_iter()
                .filter(|v| !m.trace.contains_key(v))
                .collect();
            let invariants = checked(&config, &["INVARIANT", "INVARIANTS"]);
            (format!("UNCHANGED <<{}>>", free.join(", ")), invariants)
        }
    };
    let matches: Vec<String> = m
        .trace
        .iter()
        .map(|(var, e)| format!("{var}' = ({e})"))
        .collect();
    let tla = format!(
        "---- MODULE TruefitTrace ----\nEXTENDS {module}, Naturals, Sequences\nVARIABLE truefit_l\n\
         TruefitRecords == <<\n  {}\n>>\n\
         TruefitInit == truefit_l = 0 /\\ {}\n\
         TruefitNext == /\\ truefit_l < Len(TruefitRecords)\n  /\\ truefit_l' = truefit_l + 1\n  /\\ {step}\n  \
         /\\ LET r == TruefitRecords[truefit_l + 1] IN\n     /\\ {}\n\
         TruefitUnfinished == truefit_l < Len(TruefitRecords)\n====\n",
        records
            .iter()
            .map(tla_value)
            .collect::<Vec<_>>()
            .join(",\n  "),
        m.init,
        matches.join("\n     /\\ "),
    );
    let check = match invariants.is_empty() {
        true => String::new(),
        false => format!("INVARIANT {}\n", invariants.join(" ")),
    };
    let work = workdir()?;
    let (trace_tla, trace_cfg) = (work.join("TruefitTrace.tla"), work.join("TruefitTrace.cfg"));
    let written = std::fs::write(&trace_tla, tla).and_then(|()| {
        std::fs::write(
            &trace_cfg,
            format!(
                "{}\nINIT TruefitInit\nNEXT TruefitNext\n{check}CHECK_DEADLOCK FALSE\n",
                constants(&config)
            ),
        )
    });
    written.map_err(|e| {
        vec![fault(
            format!("cannot write the trace's module: {e}"),
            "free the temporary directory",
        )]
    })?;
    let t = run_tlc(jar, root, m, work, &trace_cfg, &trace_tla)?;
    let read = t.read;
    let mut r =
        json!({"file": file, "model": m.name, "records": records.len(), "seconds": t.seconds});
    let violated = read
        .error
        .as_deref()
        .and_then(|e| e.strip_prefix("Invariant "))
        .and_then(|e| e.strip_suffix(" is violated."));
    if t.timed_out {
        r["result"] = "timed out".into();
        r["output"] = tail(&t.output).into();
    } else if let (Against::Next, Some("TruefitUnfinished")) = (against, violated) {
        r["result"] = "allowed".into();
    } else if let (Against::Invariants, Some(name)) = (against, violated) {
        // The behaviour's first state is the initial one, before any record.
        r["result"] = "breaks".into();
        r["breaks"] = name.into();
        r["step"] = (read.behaviour.len().checked_sub(2))
            .and_then(|i| records[i].get("step").cloned())
            .into();
    } else if let (Against::Invariants, true) = (against, t.completed) {
        r["result"] = "holds".into();
    } else if t.completed {
        // The search ends one state past the last record it allowed.
        let at = read.depth.unwrap_or(1).max(1) as usize;
        r["result"] = "disallowed".into();
        r["step"] = records[at - 1].get("step").cloned().into();
        r["record"] = records[at - 1].clone();
        r["after"] = at
            .checked_sub(2)
            .map_or(Value::Null, |i| records[i].clone());
    } else {
        r["result"] = "broken".into();
        r["error"] = read.error.into();
        r["output"] = tail(&t.output).into();
    }
    Ok(r)
}

/// The variables a TLA+ module declares (`VARIABLE` or `VARIABLES`), outside comments.
fn variables(spec: &str) -> Vec<String> {
    let comments = regex_lite::Regex::new(r"(?s)\(\*.*?\*\)|\\\*[^\n]*").unwrap();
    let text = comments.replace_all(spec, " ");
    let declared =
        regex_lite::Regex::new(r"\bVARIABLES?\s+([A-Za-z_]\w*(?:\s*,\s*[A-Za-z_]\w*)*)").unwrap();
    declared
        .captures_iter(&text)
        .filter_map(|c| c.get(1))
        .flat_map(|names| names.as_str().split(','))
        .map(|v| v.trim().to_string())
        .collect()
}

/// A JSON value as a TLA+ value: objects as records (keys that are no TLA+
/// name left out), arrays as sequences; numbers that are no integer, and
/// null, as strings.
fn tla_value(v: &Value) -> String {
    match v {
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Value::Number(n) if n.is_i64() => n.to_string(),
        Value::Array(a) => format!(
            "<<{}>>",
            a.iter().map(tla_value).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(o) => {
            let fields: Vec<String> = o
                .iter()
                .filter(|(k, _)| {
                    k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                        && k.chars().any(|c| c.is_ascii_alphabetic())
                })
                .map(|(k, v)| format!("{k} |-> {}", tla_value(v)))
                .collect();
            if fields.is_empty() {
                "<<>>".into()
            } else {
                format!("[{}]", fields.join(", "))
            }
        }
        Value::String(s) => tla_string(s),
        other => tla_string(&other.to_string()),
    }
}

fn tla_string(s: &str) -> String {
    let escaped: String = s
        .chars()
        .map(|c| match c {
            '"' => "\\\"".into(),
            '\\' => "\\\\".into(),
            '\n' => "\\n".into(),
            '\t' => "\\t".into(),
            c if c.is_control() => String::new(),
            c => c.to_string(),
        })
        .collect();
    format!("\"{escaped}\"")
}

/// The words that open a section of a TLC config.
const KEYWORDS: &str = "CONSTANT CONSTANTS INIT NEXT SPECIFICATION INVARIANT INVARIANTS PROPERTY PROPERTIES SYMMETRY VIEW CONSTRAINT CONSTRAINTS ACTION_CONSTRAINT ACTION_CONSTRAINTS CHECK_DEADLOCK ALIAS POSTCONDITION TYPE TYPE_CONSTRAINT";

/// Why model `model`'s config checks no invariant (or property) named `name`, or None: it does.
pub fn unreached(root: &Path, models: &[Model], model: &str, name: &str) -> Option<String> {
    let m = models.iter().find(|m| m.name == model)?;
    let config = std::fs::read_to_string(root.join(&m.config)).unwrap_or_default();
    let sections = ["INVARIANT", "INVARIANTS", "PROPERTY", "PROPERTIES"];
    if checked(&config, &sections)
        .iter()
        .any(|c| crate::promise::same_name(c, name))
    {
        return None;
    }
    Some(format!(
        "model {model}: {} checks no invariant named `{name}`",
        m.config
    ))
}

/// The names a TLC config lists in its `sections` (INVARIANT, PROPERTY ...).
fn checked(config: &str, sections: &[&str]) -> Vec<String> {
    let mut checking = false;
    let mut names = Vec::new();
    for line in config.lines() {
        let line = line.split("\\*").next().unwrap_or("");
        for word in line.split_whitespace() {
            if KEYWORDS.split(' ').any(|k| k == word) {
                checking = sections.contains(&word);
            } else if checking {
                names.push(word.to_string());
            }
        }
    }
    names
}

/// The CONSTANT sections of a TLC config, which a trace's check keeps.
fn constants(config: &str) -> String {
    let mut keep = false;
    let mut out = Vec::new();
    for line in config.lines() {
        let line = line.split("\\*").next().unwrap_or("");
        if let Some(word) = line
            .split_whitespace()
            .next()
            .filter(|w| KEYWORDS.split(' ').any(|k| k == *w))
        {
            keep = word.starts_with("CONSTANT");
        }
        if keep {
            out.push(line);
        }
    }
    out.join("\n")
}

/// The files model `name`'s actions cover.
pub fn files<'a>(models: &'a [Model], name: &'a str) -> impl Iterator<Item = &'a str> {
    let model = models.iter().filter(move |m| m.name == name);
    model.flat_map(|m| m.covers.iter().map(|c| c.file.as_str()))
}

/// The `stale-model` tasks: a model whose covered files changed since the
/// commit its coverage record names; and notes on what was not compared.
pub fn tasks(root: &Path, models: &[Model]) -> (Vec<Value>, Vec<String>) {
    let git = |args: &[&str]| contract::git(root, args);
    if models.is_empty() {
        return (vec![], vec![]);
    }
    if git(&["rev-parse", "--show-toplevel"]).is_none() {
        return (
            vec![],
            vec![format!(
                "{} is in no git repo: models' coverage is not compared with their commits",
                root.display()
            )],
        );
    }
    let mut tasks = Vec::new();
    for m in models {
        let mut files: Vec<&str> = m.covers.iter().map(|c| c.file.as_str()).collect();
        files.dedup();
        let known = git(&[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{}^{{commit}}", m.commit),
        ])
        .is_some();
        let changed: Vec<String> = if known {
            let mut args = vec!["diff", "--relative", "--name-only", &m.commit, "--"];
            args.extend(&files);
            git(&args)
                .unwrap_or_default()
                .lines()
                .map(String::from)
                .collect()
        } else {
            files.iter().map(|f| f.to_string()).collect()
        };
        if changed.is_empty() {
            continue;
        }
        let covered: Vec<Value> = changed
            .iter()
            .map(|f| {
                let by: Vec<Value> = m
                    .covers
                    .iter()
                    .filter(|c| &c.file == f)
                    .map(|c| json!({"action": c.action, "function": c.function}))
                    .collect();
                json!({"file": f, "covers": by})
            })
            .collect();
        let why = if known {
            format!("{} changed since {}", changed.join(", "), m.commit)
        } else {
            format!("its commit {} is not in this repo's history", m.commit)
        };
        let mut files = vec![m.spec.clone(), CONFIG.to_string()];
        files.extend(changed);
        tasks.push(json!({
            "kind": "stale-model",
            "summary": format!("model {}: {why}", m.name),
            "files": files,
            "evidence": {"model": m.name, "commit": m.commit, "changed": covered},
            "steps": format!("Read what changed in the covered code (`git diff {} -- <file>`) against each action that covers it (`covers`). Where the code's steps changed, change the model `{}` to match, action for action, and its `covers`; where they did not, the model still holds. Run the accept command until it passes (a failure is a bug in the design or the model: read its behaviour, never weaken the property), then set the model's `commit` in {CONFIG} to the commit you confirmed it at and commit.", m.commit, m.spec),
            "accept": format!("truefit model {}", m.name),
        }));
    }
    (tasks, vec![])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_becomes_tla_values() {
        let v: Value = serde_json::from_str(
            r#"{"a":[1,-2],"b":true,"c":"x\"y","d-e":1,"f":1.5,"g":null,"h":{}}"#,
        )
        .unwrap();
        assert_eq!(
            tla_value(&v),
            r#"[a |-> <<1, -2>>, b |-> TRUE, c |-> "x\"y", f |-> "1.5", g |-> "null", h |-> <<>>]"#
        );
    }

    #[test]
    fn a_specs_variables_are_its_declarations_outside_comments() {
        let spec = "(* VARIABLE hidden *)\nVARIABLES\n    a,  \\* the a, b\n    b\nVARIABLE c\nvars == <<a, b, c>>\n";
        assert_eq!(variables(spec), ["a", "b", "c"]);
    }

    #[test]
    fn a_trace_keeps_only_the_configs_constants() {
        let cfg = "\\* c\nCONSTANT N = 3 \\* three\n  M = 2\nSPECIFICATION Spec\nINVARIANT A\n B\nCONSTANTS P = {p1}\n";
        assert_eq!(
            constants(cfg),
            "CONSTANT N = 3 \n  M = 2\nCONSTANTS P = {p1}"
        );
    }

    #[test]
    fn tlc_output_is_read_for_its_error_behaviour_states_and_depth() {
        let out = "Error: Invariant AppliedOnce is violated.\nError: The behavior up to this point is:\nState 1: <Initial predicate>\n/\\ head = 1\n/\\ applied = <<0>>\n\nState 2: <ApplyCrash line 3, col 1 to line 4, col 2 of module Queue>\nx = 2\n\n5 states generated, 4 distinct states found, 0 states left on queue.\nThe depth of the complete state graph search is 3.\n";
        let r = read(out);
        assert_eq!(
            r.error.as_deref(),
            Some("Invariant AppliedOnce is violated.")
        );
        assert_eq!(
            Value::from(r.behaviour),
            json!([{"action": "Initial predicate", "state": {"head": "1", "applied": "<<0>>"}},
                   {"action": "ApplyCrash line 3, col 1 to line 4, col 2 of module Queue", "state": {"x": "2"}}])
        );
        assert_eq!((r.states, r.depth), (Some(4), Some(3)));
    }

    #[test]
    fn states_come_from_a_final_summary_without_commas_after_a_progress_line_with_them() {
        // TLC 1.7.4 on guild's HubLease (SYMMETRY): truefit#13.
        let out = "Progress(21) at 2026-11-21 18:42:54: 626,801 states generated (626,801 s/min), 247,557 distinct states found (247,557 ds/min), 60,000 states left on queue.\nModel checking completed. No error has been found.\n2967722 states generated, 892402 distinct states found, 0 states left on queue.\nThe depth of the complete state graph search is 37.\n";
        let r = read(out);
        assert_eq!((r.states, r.depth), (Some(892402), Some(37)));
    }

    #[test]
    fn states_and_depth_come_from_tlc_final_summary_not_a_progress_line() {
        let out = "Progress(5) at 2026-11-20 10:00:00: 742 states generated (742 s/min), 742 distinct states found, 12 states left on queue.\nProgress(9) at 2026-11-20 10:01:00: 1,913 states generated (1,171 s/min), 913 distinct states found (171 ds/min), 40 states left on queue.\nModel checking completed. No error has been found.\n31,002 states generated, 29,705 distinct states found, 0 states left on queue.\nThe depth of the complete state graph search is 14.\n";
        let r = read(out);
        assert_eq!((r.states, r.depth), (Some(29705), Some(14)));
    }

    #[test]
    fn a_tlc_run_that_cannot_start_is_a_fault_naming_java_and_its_fix() {
        let m: Model =
            toml::from_str("name = \"m\"\nspec = \"m/M.tla\"\nconfig = \"m/M.cfg\"\n").unwrap();
        let root = Path::new("/nonexistent-truefit-root");
        let work = root.join("work");
        let faults = run_tlc(
            Path::new("tla2tools.jar"),
            root,
            &m,
            work,
            Path::new("M.cfg"),
            Path::new("M.tla"),
        )
        .err()
        .expect("a run without its work dir is a fault");
        assert_eq!(faults.len(), 1);
        assert!(
            faults[0]["fault"]
                .as_str()
                .unwrap()
                .starts_with("cannot run TLC (java): "),
            "{faults:?}"
        );
        assert_eq!(
            faults[0]["fix"],
            "put a Java 11 or later `java` on PATH: TLC runs on it"
        );
    }

    #[test]
    fn a_value_tlc_wraps_stays_its_variables_even_where_it_holds_an_equals_sign() {
        let out =
            "State 1: <Initial predicate>\n/\\ log = <<\"a = b\",\n   \"c = d\">>\n/\\ x = 1\n\n";
        assert_eq!(
            read(out).behaviour[0]["state"],
            json!({"log": "<<\"a = b\", \"c = d\">>", "x": "1"})
        );
    }
}
