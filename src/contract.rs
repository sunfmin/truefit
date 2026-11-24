//! `truefit contract`: each declared stand-in and its real program answer the
//! same cases alike.

use crate::config::{CONFIG, load};
use crate::fault;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const PARTS: [&str; 4] = ["exit", "stdout", "stderr", "files"];

/// `[contract]`: where the repo's stand-ins live, and how long a real run stays fresh.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    #[serde(default)]
    pub standins: Option<Vec<String>>,
    #[serde(default = "default_fresh_days")]
    pub fresh_days: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            standins: None,
            fresh_days: default_fresh_days(),
        }
    }
}

fn default_fresh_days() -> u64 {
    30
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Standin {
    pub name: String,
    pub run: Vec<String>,
    pub real: Vec<String>,
    #[serde(default)]
    needs: String,
    #[serde(default)]
    ready: Vec<String>,
    #[serde(default)]
    pub version: Vec<String>,
    #[serde(default = "default_compare")]
    compare: Vec<String>,
    #[serde(default)]
    normalise: Vec<Normalise>,
    #[serde(default)]
    case: Vec<Case>,
    /// The failures it can answer, each held by a case whose `fault` names it.
    #[serde(default)]
    pub faults: Vec<String>,
    /// The variable that tells the stand-in which fault to answer.
    #[serde(default = "default_fault_env")]
    fault_env: String,
    /// Why it answers no fault, kept while the stand-in is the one hashed.
    pub no_faults: Option<NoFaults>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoFaults {
    pub why: String,
    pub sha256: String,
}

fn default_compare() -> Vec<String> {
    vec!["exit".into(), "stdout".into()]
}

fn default_fault_env() -> String {
    "TRUEFIT_FAULT".into()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Normalise {
    pattern: String,
    replace: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    stdin: String,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    dir: Option<String>,
    #[serde(default)]
    files: Vec<String>,
    /// The fault it holds: the stand-in runs told to answer it.
    fault: Option<String>,
    /// Must succeed for the real side to run it; else it is not run, with `needs`.
    #[serde(default)]
    ready: Vec<String>,
    #[serde(default)]
    needs: String,
}

/// What one side answered to one case, after normalisation.
#[derive(Serialize, PartialEq, Debug)]
struct Answer {
    exit: Option<i32>,
    stdout: String,
    stderr: String,
    files: BTreeMap<String, Option<String>>,
}

/// Runs the contracts declared in `root/truefit.toml`: against the stand-ins
/// alone, or with `real` against the stand-ins and their real programs.
/// Answers the report and whether every stand-in passed; `Err` is the faults.
pub fn run(root: &Path, real: Option<Option<&str>>) -> Result<(Value, bool), Vec<Value>> {
    let standins = load(root)?.standin;
    let chosen: Vec<&Standin> = match real {
        Some(Some(name)) => match standins.iter().find(|s| s.name == name) {
            Some(s) => vec![s],
            None => {
                return Err(vec![fault(
                    format!("no stand-in `{name}` in {CONFIG}"),
                    format!("declare it as a [[standin]] in {CONFIG}, or name one that is"),
                )]);
            }
        },
        _ => standins.iter().collect(),
    };
    let mut reports = Vec::new();
    let mut next = Vec::new();
    for s in chosen {
        let rules: Vec<(regex_lite::Regex, &str)> = s
            .normalise
            .iter()
            .map(|n| {
                (
                    regex_lite::Regex::new(&n.pattern).unwrap(),
                    n.replace.as_str(),
                )
            })
            .collect();
        let report = if real.is_some() {
            check_real(root, s, &rules)
        } else {
            check_standin(root, s, &rules)
        };
        match report["result"].as_str() {
            Some("pass") => {}
            Some("not run") => next.push(format!(
                "give `{}` what it needs ({}), then `truefit contract --real {}`",
                s.real[0], report["needs"].as_str().unwrap_or(""), s.name
            )),
            _ if real.is_some() => next.push(format!(
                "make stand-in `{0}` answer as `{1}` does in each differing case, then `truefit contract --real {0}`",
                s.name, s.real[0]
            )),
            _ => next.push(format!(
                "make stand-in `{0}` run each of its cases, then `truefit contract`",
                s.name
            )),
        }
        reports.push(report);
    }
    let passed = next.is_empty();
    if passed {
        next.push(if real.is_some() {
            "commit .truefit/: the real run's date and version".into()
        } else {
            "nothing; `truefit contract --real` where the real programs run".into()
        });
    }
    Ok((
        json!({"standins": reports, "next": next.join("; ")}),
        passed,
    ))
}

/// What is wrong in the declared stand-ins: each fault with its fix.
pub fn faults(standins: &[Standin]) -> Vec<Value> {
    let mut faults = Vec::new();
    for (i, s) in standins.iter().enumerate() {
        let n = &s.name;
        if standins[..i].iter().any(|t| &t.name == n) {
            faults.push(fault(
                format!("stand-in `{n}` is declared twice"),
                "give each [[standin]] its own name",
            ));
        }
        if s.run.is_empty() || s.real.is_empty() {
            faults.push(fault(
                format!("stand-in `{n}` lacks `run` or `real`"),
                "declare both as command lines, e.g. run = [\"tests/fake-git\"], real = [\"git\"]",
            ));
        }
        if s.case.is_empty() {
            faults.push(fault(format!("stand-in `{n}` has no case"), format!("declare its contract as [[standin.case]] entries: every answer of `{}` the system reads", s.real.first().map_or("", |r| r))));
        }
        for f in s
            .faults
            .iter()
            .filter(|f| !s.case.iter().any(|c| c.fault.as_ref() == Some(f)))
        {
            faults.push(fault(
                format!("stand-in `{n}` declares fault `{f}` that no case holds"),
                format!("declare a [[standin.case]] with fault = \"{f}\" whose real side fails as `{}` does", s.real.first().map_or("", |r| r)),
            ));
        }
        for (c, f) in s
            .case
            .iter()
            .filter_map(|c| Some((&c.name, c.fault.as_ref()?)))
            .filter(|(_, f)| !s.faults.contains(f))
        {
            faults.push(fault(
                format!("stand-in `{n}`: case `{c}` holds fault `{f}`, not in its `faults`"),
                format!("add \"{f}\" to its faults, or name one that is"),
            ));
        }
        if s.no_faults.is_some() && !s.faults.is_empty() {
            faults.push(fault(
                format!("stand-in `{n}` declares faults and no_faults"),
                "drop no_faults: it declares faults",
            ));
        }
        for p in s.compare.iter().filter(|p| !PARTS.contains(&p.as_str())) {
            faults.push(fault(
                format!("stand-in `{n}` compares `{p}`"),
                format!("compare only {}", PARTS.join(", ")),
            ));
        }
        for r in s
            .normalise
            .iter()
            .filter(|r| regex_lite::Regex::new(&r.pattern).is_err())
        {
            faults.push(fault(
                format!(
                    "stand-in `{n}`: normalise pattern `{}` is not a regex",
                    r.pattern
                ),
                "correct the pattern",
            ));
        }
    }
    faults
}

fn check_standin(root: &Path, s: &Standin, rules: &[(regex_lite::Regex, &str)]) -> Value {
    let mut ok = true;
    let cases: Vec<Value> = s
        .case
        .iter()
        .map(
            |c| match answer(root, &s.run, c, rules, Some(&s.fault_env)) {
                Ok(_) => json!({"case": c.name, "result": "ran"}),
                Err(e) => {
                    ok = false;
                    json!({"case": c.name, "result": "cannot run", "fault": e})
                }
            },
        )
        .collect();
    json!({"name": s.name, "result": if ok { "pass" } else { "fail" }, "cases": cases,
           "real_run": read(root, &record_file(&s.name))})
}

fn check_real(root: &Path, s: &Standin, rules: &[(regex_lite::Regex, &str)]) -> Value {
    let needs = if s.needs.is_empty() {
        format!("`{}` on PATH", s.real[0])
    } else {
        s.needs.clone()
    };
    let not_run =
        |why: String| json!({"name": s.name, "result": "not run", "why": why, "needs": needs});
    if find(root, &s.real[0]).is_none() {
        return not_run(format!("`{}` is not found here", s.real[0]));
    }
    if !ready(root, &s.ready) {
        return not_run(format!("`{}` did not succeed", s.ready.join(" ")));
    }
    let mut ok = true;
    let mut cases = Vec::new();
    let mut differing = Vec::new();
    let mut unrun = Vec::new();
    for c in &s.case {
        if !ready(root, &c.ready) {
            let why = format!("`{}` did not succeed", c.ready.join(" "));
            let needs = if c.needs.is_empty() {
                why.clone()
            } else {
                c.needs.clone()
            };
            cases.push(json!({"case": c.name, "result": "not run", "why": why, "needs": needs}));
            unrun.push(needs);
            continue;
        }
        let (stand, real) = (
            answer(root, &s.run, c, rules, Some(&s.fault_env)),
            answer(root, &s.real, c, rules, None),
        );
        let case = match (stand, real) {
            (Ok(a), Ok(b)) => {
                let differ = differ(&s.compare, &a, &b);
                ok &= differ.is_empty();
                if differ.is_empty() {
                    json!({"case": c.name, "result": "agree"})
                } else {
                    let case = json!({"case": c.name, "result": "differ", "differ": differ, "standin": a, "real": b});
                    differing.push(case.clone());
                    case
                }
            }
            (a, b) => {
                ok = false;
                json!({"case": c.name, "result": "cannot run", "standin": a.err(), "real": b.err()})
            }
        };
        cases.push(case);
    }
    let mut report =
        json!({"name": s.name, "result": if ok { "pass" } else { "fail" }, "cases": cases});
    // A case not run here is never a pass: the run is not run, and records nothing.
    if ok && !unrun.is_empty() {
        report["result"] = "not run".into();
        report["needs"] = unrun.join("; ").into();
        ok = false;
    }
    let record = json!({"date": crate::date::today(), "version": version(root, s)});
    // The last real run's differing cases, kept until a real run finds none.
    let differ = differ_file(&s.name);
    let mut written = if differing.is_empty() {
        let path = root.join(&differ);
        if path.exists() {
            std::fs::remove_file(path)
        } else {
            Ok(())
        }
    } else {
        let mut cases = record.clone();
        cases["cases"] = differing.into();
        write(root, &differ, &cases)
    }
    .map_err(|e| format!("cannot write {differ}: {e}"));
    if ok {
        let file = record_file(&s.name);
        written = written.and_then(|()| {
            write(root, &file, &record).map_err(|e| format!("cannot write {file}: {e}"))
        });
    }
    if let Err(e) = written {
        report["result"] = "fail".into();
        report["fault"] = e.into();
    }
    if ok {
        report["real_run"] = record;
    }
    report
}

/// Why every test through `s` can only succeed, with its stand-in's SHA-256
/// now; None when it declares faults, or `no_faults` for it as it is now.
pub fn without_faults(root: &Path, s: &Standin) -> Option<(&'static str, Option<String>)> {
    let sha = find(root, &s.run[0]).and_then(|p| crate::model::sha256(&p));
    let kept = s.no_faults.as_ref();
    if !s.faults.is_empty() || kept.is_some_and(|k| Some(&k.sha256) == sha.as_ref()) {
        return None;
    }
    let why = if kept.is_some() {
        "the stand-in changed since it was marked as answering no fault"
    } else {
        "it declares no fault: every test through it can only succeed"
    };
    Some((why, sha))
}

/// Where a stand-in's last passing real run is kept: its date and version.
pub fn record_file(name: &str) -> String {
    format!(".truefit/{name}.json")
}

/// Where the last real run's differing cases are kept, while there are any.
pub fn differ_file(name: &str) -> String {
    format!(".truefit/{name}.differ.json")
}

/// Writes `value` to `root/file`, or the fault that it cannot.
pub fn save(root: &Path, file: &str, value: &Value) -> Result<(), Vec<Value>> {
    write(root, file, value).map_err(|e| {
        vec![fault(
            format!("cannot write {file}: {e}"),
            "make .truefit/ a directory truefit can write",
        )]
    })
}

/// Writes `value` to `root/file`, making the directories it is in.
pub fn write(root: &Path, file: &str, value: &Value) -> std::io::Result<()> {
    let path = root.join(file);
    std::fs::create_dir_all(path.parent().unwrap_or(root))?;
    std::fs::write(root.join(file), format!("{value:#}\n"))
}

/// The first line its `version` command prints here, or None: none declared, or it cannot run.
pub fn version(root: &Path, s: &Standin) -> Option<String> {
    (!s.version.is_empty())
        .then(|| quiet(root, &s.version))
        .flatten()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        })
}

fn differ(compare: &[String], a: &Answer, b: &Answer) -> Vec<String> {
    compare
        .iter()
        .filter(|p| match p.as_str() {
            "exit" => a.exit != b.exit,
            "stdout" => a.stdout != b.stdout,
            "stderr" => a.stderr != b.stderr,
            _ => a.files != b.files,
        })
        .cloned()
        .collect()
}

/// What `git <args>` prints, run in `root`, or None: it failed.
pub fn git(root: &Path, args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .current_dir(root)
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
}

pub fn read(root: &Path, file: &str) -> Value {
    std::fs::read_to_string(root.join(file))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null)
}

/// Where `program` is: relative to `root` when it names a path, else on PATH.
pub fn find(root: &Path, program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        let p = root.join(program);
        return p.is_file().then_some(p);
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join(program))
        .find(|p| p.is_file())
}

/// Whether `argv` is empty or succeeds here: a real run (or one case of it) may go ahead.
fn ready(root: &Path, argv: &[String]) -> bool {
    argv.is_empty() || matches!(quiet(root, argv), Some(o) if o.status.success())
}

fn quiet(root: &Path, argv: &[String]) -> Option<std::process::Output> {
    Command::new(find(root, &argv[0])?)
        .args(&argv[1..])
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .ok()
}

/// Runs `prefix` + the case's args in a fresh directory holding a copy of the
/// case's `dir`, and answers what it said, normalised.
fn answer(
    root: &Path,
    prefix: &[String],
    c: &Case,
    rules: &[(regex_lite::Regex, &str)],
    // The variable that tells the stand-in a case's fault; None on the real side.
    fault_env: Option<&str>,
) -> Result<Answer, String> {
    let program = find(root, &prefix[0]).ok_or_else(|| format!("`{}` is not found", prefix[0]))?;
    let work = scratch()?;
    let result = (|| {
        if let Some(dir) = &c.dir {
            copy(&root.join(dir), &work)
                .map_err(|e| format!("cannot copy case dir `{dir}`: {e}"))?;
        }
        let mut child = Command::new(&program)
            .args(&prefix[1..])
            .args(&c.args)
            .envs(&c.env)
            .envs(fault_env.zip(c.fault.as_deref()))
            .current_dir(&work)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot run `{}`: {e}", program.display()))?;
        // A program that does not read its stdin closes the pipe: not a fault.
        let _ = child.stdin.take().unwrap().write_all(c.stdin.as_bytes());
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        let norm = |b: &[u8]| normalise(&String::from_utf8_lossy(b), &work, rules);
        let files = c
            .files
            .iter()
            .map(|f| {
                (
                    f.clone(),
                    std::fs::read(work.join(f)).ok().map(|b| norm(&b)),
                )
            })
            .collect();
        Ok(Answer {
            exit: out.status.code(),
            stdout: norm(&out.stdout),
            stderr: norm(&out.stderr),
            files,
        })
    })();
    let _ = std::fs::remove_dir_all(&work);
    result
}

fn normalise(text: &str, work: &Path, rules: &[(regex_lite::Regex, &str)]) -> String {
    let mut t = text.to_string();
    for w in [work.to_path_buf(), work.canonicalize().unwrap_or_default()] {
        if !w.as_os_str().is_empty() {
            t = t.replace(&*w.to_string_lossy(), "<dir>");
        }
    }
    for (re, with) in rules {
        t = re.replace_all(&t, *with).into_owned();
    }
    t
}

pub fn scratch() -> Result<PathBuf, String> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "truefit-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    Ok(dir)
}

fn copy(from: &Path, to: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            std::fs::create_dir_all(&target)?;
            copy(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_replaces_the_case_dir_then_the_rules() {
        let rules = [(
            regex_lite::Regex::new(r"\d{4}-\d\d-\d\d").unwrap(),
            "<date>",
        )];
        let work = Path::new("/no/such/work");
        assert_eq!(
            normalise("/no/such/work/a on 2026-11-21", work, &rules),
            "<dir>/a on <date>"
        );
    }

    #[test]
    fn differ_names_each_compared_part_that_differs() {
        let a = Answer {
            exit: Some(0),
            stdout: "x".into(),
            stderr: "e".into(),
            files: BTreeMap::new(),
        };
        let mut b = Answer {
            exit: Some(1),
            stdout: "x".into(),
            stderr: "f".into(),
            files: BTreeMap::new(),
        };
        let all: Vec<String> = PARTS.iter().map(|p| p.to_string()).collect();
        assert_eq!(differ(&all, &a, &b), ["exit", "stderr"]);
        assert!(differ(&["stdout".into()], &a, &b).is_empty());
        b.files.insert("f".into(), None);
        assert_eq!(differ(&["files".into()], &a, &b), ["files"]);
    }
}
