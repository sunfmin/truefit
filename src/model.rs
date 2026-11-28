//! `truefit model`: the repo's TLA+ models checked by TLC, and a run's records
//! checked by TLC as a behaviour of one of them. TLC itself is `tlc`'s.

use crate::config::{self, CONFIG};
use crate::date;
use crate::fault;
use crate::host;
use crate::tlc;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
    /// MB of Java heap TLC may take on it.
    #[serde(default = "default_heap")]
    heap: u64,
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
fn default_heap() -> u64 {
    1024
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
        if m.heap == 0 {
            faults.push(fault(
                format!("model `{n}` has a heap of 0 MB"),
                "give TLC the MB of Java heap it may take on it, e.g. heap = 1024",
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
    let models = config::load(root)?.model;
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
    let (jar, installed) = tlc::install()?;
    let tlc = tlc::pinned(&jar, installed);
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

/// How TLC runs on `m`: the modules beside its spec, its limit and heap.
fn bounds(root: &Path, m: &Model) -> tlc::Bounds {
    let spec = root.join(&m.spec);
    tlc::Bounds {
        library: spec.parent().unwrap_or(root).to_path_buf(),
        limit: m.limit,
        heap: m.heap,
    }
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
    // TLC's own check first, as it takes the longest; then each trace's.
    let mut jobs: Vec<(String, Job<'_>)> = vec![(
        format!("model {}", m.name),
        Box::new(|| check(root, m, jar)),
    )];
    let runs = (accept.iter().map(|f| (f, Against::Next)))
        .chain((reject.iter()).flat_map(|f| [(f, Against::Next), (f, Against::Invariants)]));
    jobs.extend(runs.map(|(f, against)| {
        let job: Job<'_> = Box::new(move || check_trace(root, m, jar, f, against));
        (format!("trace {f}"), job)
    }));
    // One answer per job, in its place: TLC's, each accept trace's, then
    // each reject trace's pair.
    let mut answers = pooled(jobs, bound()).into_iter();
    let r = answers.next().unwrap_or(Ok(Value::Null));
    let accepted: Result<Vec<Value>, Vec<Value>> = answers.by_ref().take(accept.len()).collect();
    let rejected: Result<Vec<(Value, Value)>, Vec<Value>> =
        std::iter::from_fn(|| Some((answers.next()?, answers.next()?)))
            .map(|(n, i)| Ok((n?, i?)))
            .collect();
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
        "commit": host::git(root, &["rev-parse", "HEAD"]).map(|c| c.trim().to_string()),
        "date": date::today(),
        "inputs": inputs(root, m),
        "result": r["result"],
        "traces": {"accept": brief(&accepted), "reject": brief(&reject_reports)},
    });
    host::save(root, &record_file(&m.name), &record)?;
    Ok(r)
}

/// One check of a model's: TLC's, or a trace's.
type Job<'e> = Box<dyn FnOnce() -> Result<Value, Vec<Value>> + Send + 'e>;

/// How many checks (each its own JVM, with its own heap) run at once: four,
/// or the host's CPUs when fewer. TLC's own check takes the longest (17-92 s
/// on factorium's models, each trace's 0.6-1 s), so one slot runs it and three
/// the traces of a model of 14 within its time.
fn bound() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get().min(4))
}

/// Runs each job, at most `bound` at once, and answers each in its job's
/// place. A job that panics answers a fault naming what it checked.
fn pooled(jobs: Vec<(String, Job<'_>)>, bound: usize) -> Vec<Result<Value, Vec<Value>>> {
    let n = jobs.len();
    let queue = std::sync::Mutex::new(jobs.into_iter().enumerate());
    let mut done: Vec<(usize, Result<Value, Vec<Value>>)> = std::thread::scope(|s| {
        let workers: Vec<_> = (0..bound.clamp(1, n.max(1)))
            .map(|_| {
                s.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let next = queue.lock().unwrap_or_else(|e| e.into_inner()).next();
                        let Some((i, (what, job))) = next else {
                            break done;
                        };
                        let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job))
                            .unwrap_or_else(|p| Err(vec![panicked(&what, p.as_ref())]));
                        done.push((i, answer));
                    }
                })
            })
            .collect();
        // A job's panic is caught where it runs: a worker never panics.
        workers
            .into_iter()
            .flat_map(|w| w.join().unwrap_or_default())
            .collect()
    });
    done.sort_by_key(|(i, _)| *i);
    done.into_iter().map(|(_, a)| a).collect()
}

/// The fault of a check that panicked: what it checked, and the panic's message.
fn panicked(what: &str, payload: &(dyn std::any::Any + Send)) -> Value {
    let message = (payload.downcast_ref::<String>().map(String::as_str))
        .or(payload.downcast_ref::<&str>().copied())
        .unwrap_or_default();
    fault(
        format!("the check of {what} panicked: {message}"),
        "a bug of truefit's: report it with this trace and the model's spec and config",
    )
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
    host::digest(&all)
}

/// Why model `model`'s record does not back promise `promise`, or None: its
/// last check, of these inputs, passed with a trace accepted and a trace
/// rejected that breaks the promise's invariant.
pub fn unbacked(root: &Path, models: &[Model], model: &str, promise: &str) -> Option<String> {
    let m = models.iter().find(|m| m.name == model)?;
    let file = record_file(model);
    let r = host::read(root, &file);
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
    let t = tlc::run(jar, &bounds(root, m), tlc::workdir()?, &cfg, &spec)?;
    let mut r = json!({"name": m.name, "spec": m.spec, "seconds": t.seconds, "limit": m.limit});
    let read = t.read;
    if t.exit.is_none() {
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
        r["exit"] = t.exit.map(|e| e.to_string()).into();
    }
    r["states"] = read.states.into();
    r["depth"] = read.depth.into();
    if r["result"] == "broken" || r["result"] == "timed out" {
        r["output"] = tlc::tail(&t.output).into();
    }
    Ok(r)
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
            let free: Vec<String> = tlc::variables(&text)
                .into_iter()
                .filter(|v| !m.trace.contains_key(v))
                .collect();
            let invariants = tlc::checked(&config, &["INVARIANT", "INVARIANTS"]);
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
            .map(tlc::value)
            .collect::<Vec<_>>()
            .join(",\n  "),
        m.init,
        matches.join("\n     /\\ "),
    );
    let check = match invariants.is_empty() {
        true => String::new(),
        false => format!("INVARIANT {}\n", invariants.join(" ")),
    };
    let work = tlc::workdir()?;
    let (trace_tla, trace_cfg) = (work.join("TruefitTrace.tla"), work.join("TruefitTrace.cfg"));
    let written = std::fs::write(&trace_tla, tla).and_then(|()| {
        std::fs::write(
            &trace_cfg,
            format!(
                "{}\nINIT TruefitInit\nNEXT TruefitNext\n{check}CHECK_DEADLOCK FALSE\n",
                tlc::constants(&config)
            ),
        )
    });
    written.map_err(|e| {
        vec![fault(
            format!("cannot write the trace's module: {e}"),
            "free the temporary directory",
        )]
    })?;
    let t = tlc::run(jar, &bounds(root, m), work, &trace_cfg, &trace_tla)?;
    let read = t.read;
    let mut r =
        json!({"file": file, "model": m.name, "records": records.len(), "seconds": t.seconds});
    let violated = read
        .error
        .as_deref()
        .and_then(|e| e.strip_prefix("Invariant "))
        .and_then(|e| e.strip_suffix(" is violated."));
    if t.exit.is_none() {
        r["result"] = "timed out".into();
        r["output"] = tlc::tail(&t.output).into();
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
        r["exit"] = t.exit.map(|e| e.to_string()).into();
        r["error"] = read.error.into();
        r["output"] = tlc::tail(&t.output).into();
    }
    Ok(r)
}

/// Why model `model`'s config checks no invariant (or property) named `name`, or None: it does.
pub fn unreached(root: &Path, models: &[Model], model: &str, name: &str) -> Option<String> {
    let m = models.iter().find(|m| m.name == model)?;
    let config = std::fs::read_to_string(root.join(&m.config)).unwrap_or_default();
    let sections = ["INVARIANT", "INVARIANTS", "PROPERTY", "PROPERTIES"];
    if tlc::checked(&config, &sections)
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

/// The files model `name`'s actions cover.
pub fn files<'a>(models: &'a [Model], name: &'a str) -> impl Iterator<Item = &'a str> {
    let model = models.iter().filter(move |m| m.name == name);
    model.flat_map(|m| m.covers.iter().map(|c| c.file.as_str()))
}

/// The `stale-model` tasks: a model whose covered files changed since the
/// commit its coverage record names; and notes on what was not compared.
pub fn tasks(root: &Path, models: &[Model]) -> (Vec<Value>, Vec<String>) {
    let git = |args: &[&str]| host::git(root, args);
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
    fn a_models_heap_is_1024_mb_unless_declared() {
        let m: Model =
            toml::from_str("name = \"m\"\nspec = \"M.tla\"\nconfig = \"M.cfg\"\n").unwrap();
        assert_eq!(m.heap, 1024);
    }
}
