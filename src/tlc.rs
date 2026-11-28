//! TLC, the model checker truefit pins and runs: its install, its JVM, how one
//! run is bounded, what its output says, and the languages it reads (a TLC
//! config, TLA+ values). What a model means to truefit is `model`'s.

use crate::fault;
use crate::host;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The one TLC truefit runs: its release and the SHA-256 of its `tla2tools.jar`.
pub const VERSION: &str = "1.7.4";
const SHA256: &str = "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88";

/// TLC's jar in `$HOME`, at the pinned release and checksum: downloaded once
/// (from `TRUEFIT_TLC_FROM` when set, else the official release), and whether
/// it was just now.
pub fn install() -> Result<(PathBuf, bool), Vec<Value>> {
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
        vec![fault(
            "HOME is not set",
            "run truefit with HOME set: TLC is installed under it",
        )]
    })?;
    let dir = home.join(".local/share/truefit/tlc").join(VERSION);
    let jar = dir.join("tla2tools.jar");
    if host::sha256(&jar).as_deref() == Some(SHA256) {
        return Ok((jar, false));
    }
    let from = std::env::var("TRUEFIT_TLC_FROM").unwrap_or_else(|_| {
        format!("https://github.com/tlaplus/tlaplus/releases/download/v{VERSION}/tla2tools.jar")
    });
    let part = dir.join(format!("tla2tools.jar.{}.part", std::process::id()));
    let cannot = |why: String| {
        vec![fault(
            format!("cannot install TLC {VERSION} from {from}: {why}"),
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
    let sum = host::sha256(&part);
    if !got.success() || sum.as_deref() != Some(SHA256) {
        let _ = std::fs::remove_file(&part);
        return Err(cannot(match sum {
            Some(s) if got.success() => format!("its SHA-256 is {s}, not {SHA256}"),
            _ => format!("curl {got}"),
        }));
    }
    std::fs::rename(&part, &jar).map_err(|e| cannot(e.to_string()))?;
    Ok((jar, true))
}

/// What one TLC run printed, what that says, whether it completed without
/// an error, how long it took, and how its JVM ended (`None`: killed past
/// its limit).
pub struct Run {
    pub output: String,
    pub read: Read,
    pub completed: bool,
    pub seconds: f64,
    pub exit: Option<std::process::ExitStatus>,
}

/// The pinned release and its checksum, the jar installed, and whether it
/// was just now: what a check names as the TLC it ran.
pub fn pinned(jar: &Path, installed_now: bool) -> Value {
    json!({"version": VERSION, "sha256": SHA256, "jar": jar, "installed_now": installed_now})
}

/// The JVM's bounds for one TLC run: TLC checks with one worker (no
/// `-workers`), so the JVM is sized to one CPU, its GC and JIT threads with
/// it, and to `heap_mb` of heap. Unbounded, it sizes them to every core and
/// a quarter of the host's memory, and the runs at once of a busy host ask
/// for several times what they use.
fn jvm(heap_mb: u64) -> [String; 4] {
    [
        "-XX:ActiveProcessorCount=1".into(),
        "-XX:+UseSerialGC".into(),
        "-XX:CICompilerCount=2".into(),
        format!("-Xmx{heap_mb}m"),
    ]
}

/// How one TLC run is bounded: the directory of the modules a spec extends,
/// the seconds it may take, and the MB of heap.
pub struct Bounds {
    pub library: PathBuf,
    pub limit: u64,
    pub heap: u64,
}

/// Runs TLC on `spec` with `cfg` in `work`, within `bounds`, then removes `work`.
pub fn run(
    jar: &Path,
    bounds: &Bounds,
    work: PathBuf,
    cfg: &Path,
    spec: &Path,
) -> Result<Run, Vec<Value>> {
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
        .args(jvm(bounds.heap))
        // Its own temporary files: TLC runs at once share none.
        .arg(format!("-Djava.io.tmpdir={}", work.display()))
        .arg(format!("-DTLA-Library={}", bounds.library.display()))
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
    let exit = loop {
        if let Some(status) = child.try_wait().map_err(cannot)? {
            break Some(status);
        }
        if start.elapsed() > Duration::from_secs(bounds.limit) {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let output = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&work);
    Ok(Run {
        read: read(&output),
        completed: output.contains("Model checking completed. No error has been found."),
        output,
        seconds: (start.elapsed().as_secs_f64() * 100.0).round() / 100.0,
        exit,
    })
}

/// A fresh directory for one TLC run's files, outside the repo.
pub fn workdir() -> Result<PathBuf, Vec<Value>> {
    host::scratch().map_err(|e| vec![fault(e, "free the temporary directory")])
}

/// What a TLC run's output says: its first error, the behaviour that reached
/// it (each state's action and variables), distinct states and depth.
pub struct Read {
    pub error: Option<String>,
    pub behaviour: Vec<Value>,
    pub states: Option<u64>,
    pub depth: Option<u64>,
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
pub fn tail(output: &str) -> String {
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

/// The variables a TLA+ module declares (`VARIABLE` or `VARIABLES`), outside comments.
pub fn variables(spec: &str) -> Vec<String> {
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
pub fn value(v: &Value) -> String {
    match v {
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Value::Number(n) if n.is_i64() => n.to_string(),
        Value::Array(a) => format!("<<{}>>", a.iter().map(value).collect::<Vec<_>>().join(", ")),
        Value::Object(o) => {
            let fields: Vec<String> = o
                .iter()
                .filter(|(k, _)| {
                    k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                        && k.chars().any(|c| c.is_ascii_alphabetic())
                })
                .map(|(k, v)| format!("{k} |-> {}", value(v)))
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

/// The names a TLC config lists in its `sections` (INVARIANT, PROPERTY ...).
pub fn checked(config: &str, sections: &[&str]) -> Vec<String> {
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
pub fn constants(config: &str) -> String {
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
            value(&v),
            r#"[a |-> <<1, -2>>, b |-> TRUE, c |-> "x\"y", f |-> "1.5", g |-> "null", h |-> <<>>]"#
        );
        // TLA+ escapes a backslash, a newline and a tab; other control characters are dropped.
        assert_eq!(value(&json!("a\\b\n\t\u{7}c")), r#""a\\b\n\tc""#);
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
        // TLC 1.7.4's output on guild's HubLease (SYMMETRY).
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
    fn each_tlc_jvm_is_bounded_to_one_cpu_its_gc_and_jit_and_the_model_heap() {
        assert_eq!(
            jvm(1024),
            [
                "-XX:ActiveProcessorCount=1",
                "-XX:+UseSerialGC",
                "-XX:CICompilerCount=2",
                "-Xmx1024m",
            ]
        );
    }

    #[test]
    fn a_tlc_run_that_cannot_start_is_a_fault_naming_java_and_its_fix() {
        let root = Path::new("/nonexistent-truefit-root");
        let bounds = Bounds {
            library: root.join("m"),
            limit: 300,
            heap: 1024,
        };
        let faults = run(
            Path::new("tla2tools.jar"),
            &bounds,
            root.join("work"),
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
