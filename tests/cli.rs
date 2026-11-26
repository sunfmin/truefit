use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn truefit_in(dir: &Path, args: &[&str]) -> (Option<i32>, Value) {
    answer(
        Command::new(env!("CARGO_BIN_EXE_truefit"))
            .args(args)
            .current_dir(dir),
    )
}

/// The exit code and stdout's JSON object (null if none) of `command`'s run.
fn answer(command: &mut Command) -> (Option<i32>, Value) {
    let out = command.output().unwrap();
    let json = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    (out.status.code(), json)
}

/// JIT flags for the JVM of each TLC run in a test (JAVA_TOOL_OPTIONS): C1
/// only, as TLC's answers do not depend on the JIT tier and a sample's run is
/// over before C2's compiles pay off (half the CPU of each TLC run here).
const C1_ONLY: &str = "-XX:TieredStopAtLevel=1";

/// `truefit_in`, with each TLC run's JVM given `C1_ONLY`.
fn truefit_c1_in(dir: &Path, args: &[&str]) -> (Option<i32>, Value) {
    answer(
        Command::new(env!("CARGO_BIN_EXE_truefit"))
            .args(args)
            .current_dir(dir)
            .env("JAVA_TOOL_OPTIONS", C1_ONLY),
    )
}

fn truefit(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_truefit"))
        .args(args)
        .output()
        .unwrap()
}

/// A copy of `testdata/<name>` in a fresh directory, so a real run's record
/// lands there and not in the repo.
fn sample(name: &str, tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("truefit-test-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let from = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("testdata")
        .join(name);
    let ok = Command::new("cp")
        .arg("-R")
        .arg(format!("{}/.", from.display()))
        .arg(&dir)
        .status()
        .unwrap();
    assert!(ok.success());
    // What runs left there, but the committed traces.
    for e in std::fs::read_dir(dir.join(".truefit"))
        .into_iter()
        .flatten()
    {
        let e = e.unwrap();
        if e.file_name() != "traces" {
            let _ = std::fs::remove_dir_all(e.path());
            let _ = std::fs::remove_file(e.path());
        }
    }
    dir
}

fn config(name: &str, toml: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("truefit-test-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("truefit.toml"), toml).unwrap();
    dir
}

#[test]
fn help_is_the_manual() {
    let out = truefit(&["--help"]);
    assert!(out.status.success());
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .starts_with("# `truefit`")
    );
}

#[test]
fn an_unknown_command_exits_2_and_names_the_manual() {
    let out = truefit(&["frob"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        String::from_utf8(out.stderr)
            .unwrap()
            .contains("truefit --help")
    );
}

/// On one copy of the plutil sample: its stand-ins run every case; the true
/// one agrees with plutil and its real run is recorded; the wrong one differs
/// and both answers are printed.
#[test]
fn the_sample_stand_ins_run_every_case_the_true_one_agrees_and_the_wrong_one_differs() {
    let dir = sample("plutil", "real");
    let (code, out) = truefit_in(&dir, &["contract"]);
    assert_eq!(code, Some(0), "every case runs: {out:#}");
    assert_eq!(out["standins"][0]["cases"].as_array().unwrap().len(), 5);
    assert_eq!(out["standins"][1]["result"], "pass");
    assert!(out["next"].as_str().unwrap().contains("--real"));

    let (code, out) = truefit_in(&dir, &["contract", "--real", "plutil"]);
    assert_eq!(code, Some(0), "{out:#}");
    let s = &out["standins"][0];
    assert!(
        s["cases"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["result"] == "agree")
    );
    let record: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(".truefit/plutil.json")).unwrap())
            .unwrap();
    assert_eq!(record, s["real_run"]);
    let utc = Command::new("date")
        .args(["-u", "+%F"])
        .output()
        .unwrap()
        .stdout;
    assert_eq!(record["date"], String::from_utf8(utc).unwrap().trim());
    let version = Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .unwrap()
        .stdout;
    assert_eq!(
        record["version"],
        String::from_utf8(version).unwrap().trim(),
        "the recorded version is its `version` command's first line"
    );
    let (_, plain) = truefit_in(&dir, &["contract"]);
    assert_eq!(plain["standins"][0]["real_run"], record);

    let (code, out) = truefit_in(&dir, &["contract", "--real", "plutil-wrong"]);
    assert_eq!(code, Some(1), "the wrong stand-in differs: {out:#}");
    let lint = &out["standins"][0]["cases"][0];
    assert_eq!(lint["result"], "differ");
    assert_eq!(
        lint["differ"],
        serde_json::json!(["exit", "stdout", "stderr"])
    );
    assert_eq!(lint["standin"]["stdout"], "bad.plist: OK\n");
    assert_eq!(lint["real"]["stderr"], "bad.plist: (<why>)\n");
    assert_eq!(lint["real"]["exit"], 1);
    assert!(!dir.join(".truefit/plutil-wrong.json").exists());
    assert!(
        out["next"]
            .as_str()
            .unwrap()
            .contains("answer as `plutil` does")
    );
}

#[test]
fn a_real_program_missing_from_path_is_not_run() {
    let (code, out) = truefit_in(&sample("missing", "missing"), &["contract", "--real"]);
    assert_eq!(code, Some(1), "{out:#}");
    let s = &out["standins"][0];
    assert_eq!(s["result"], "not run");
    assert_eq!(s["needs"], "truefit-no-such-program 1.x on PATH, logged in");
    assert!(s.get("cases").is_none());
    assert!(
        out["next"]
            .as_str()
            .unwrap()
            .starts_with("give `truefit-no-such-program` what it needs")
    );
}

#[test]
fn a_real_program_not_ready_is_not_run_and_needs_defaults_to_path() {
    let dir = config(
        "unready",
        r#"[[standin]]
name = "t"
run = ["/bin/echo"]
real = ["echo"]
ready = ["false"]
[[standin.case]]
name = "c"
"#,
    );
    let (code, out) = truefit_in(&dir, &["contract", "--real", "t"]);
    assert_eq!(code, Some(1), "{out:#}");
    assert_eq!(out["standins"][0]["result"], "not run");
    assert_eq!(out["standins"][0]["needs"], "`echo` on PATH");
}

#[test]
fn a_stand_in_that_cannot_run_its_case_fails() {
    let dir = config(
        "cannot",
        r#"[[standin]]
name = "t"
run = ["./no-such-stand-in"]
real = ["echo"]
[[standin.case]]
name = "c"
"#,
    );
    let (code, out) = truefit_in(&dir, &["contract"]);
    assert_eq!(code, Some(1), "{out:#}");
    assert_eq!(out["standins"][0]["cases"][0]["result"], "cannot run");
    assert!(
        out["next"]
            .as_str()
            .unwrap()
            .contains("run each of its cases")
    );
    let (code, out) = truefit_in(&dir, &["contract", "--real"]);
    assert_eq!(code, Some(1), "{out:#}");
    assert_eq!(out["standins"][0]["cases"][0]["result"], "cannot run");
}

#[test]
fn a_bad_declaration_is_every_fault_with_its_fix_and_exit_2() {
    let dir = config(
        "bad",
        r#"[[standin]]
name = "t"
run = []
real = ["echo"]
compare = ["colour"]
normalise = [{ pattern = "(", replace = "" }]
[[standin]]
name = "t"
run = ["x"]
real = ["y"]
faults = ["gone"]
no_faults = { why = "x", sha256 = "y" }
[[standin.case]]
name = "c"
fault = "other"
"#,
    );
    let (code, out) = truefit_in(&dir, &["contract"]);
    assert_eq!(code, Some(2), "{out:#}");
    let faults = out["error"].as_array().unwrap();
    assert_eq!(faults.len(), 8, "{out:#}");
    for f in [
        "stand-in `t` declares fault `gone` that no case holds",
        "stand-in `t`: case `c` holds fault `other`, not in its `faults`",
        "stand-in `t` declares faults and no_faults",
    ] {
        assert!(faults.iter().any(|g| g["fault"] == f), "{f}: {out:#}");
    }
    assert!(faults.iter().all(|f| f["fix"].is_string()));
}

#[test]
fn faults_exit_2() {
    for (dir, args) in [
        (config("unknown", "foo = 1\n"), vec!["contract"]),
        (config("empty", ""), vec!["contract", "--real", "nobody"]),
        (config("args", ""), vec!["contract", "--frob"]),
        (config("next-args", ""), vec!["next", "--frob"]),
        (
            config("next-unknown", "[contract]\nfresh = 1\n"),
            vec!["next"],
        ),
        (
            std::env::temp_dir().join("truefit-no-such-dir-\u{2603}"),
            vec!["contract"],
        ),
    ] {
        std::fs::create_dir_all(&dir).unwrap();
        let (code, out) = truefit_in(&dir, &args);
        assert_eq!(code, Some(2), "{args:?}: {out:#}");
        assert!(out["error"][0]["fix"].is_string());
    }
}

#[test]
fn a_case_dir_is_copied_whole_and_one_missing_cannot_run() {
    let dir = config(
        "dirs",
        r#"[[standin]]
name = "cat"
run = ["/bin/cat"]
real = ["cat"]
compare = ["stdout", "files"]
[[standin.case]]
name = "nested"
dir = "fixture"
args = ["sub/x.txt"]
files = ["sub/x.txt", "absent"]
[[standin.case]]
name = "no-dir"
dir = "no-such-fixture"
[[standin]]
name = "not-executable"
run = ["fixture/sub/x.txt"]
real = ["cat"]
[[standin.case]]
name = "c"
"#,
    );
    std::fs::create_dir_all(dir.join("fixture/sub")).unwrap();
    std::fs::write(dir.join("fixture/sub/x.txt"), "hi\n").unwrap();
    let (code, out) = truefit_in(&dir, &["contract", "--real", "cat"]);
    assert_eq!(code, Some(1), "{out:#}");
    let cases = &out["standins"][0]["cases"];
    assert_eq!(cases[0]["result"], "agree");
    assert_eq!(cases[1]["result"], "cannot run");
    assert!(
        cases[1]["standin"]
            .as_str()
            .unwrap()
            .contains("no-such-fixture")
    );
    let (_, out) = truefit_in(&dir, &["contract"]);
    let fault = out["standins"][1]["cases"][0]["fault"].as_str().unwrap();
    assert!(fault.starts_with("cannot run"), "{out:#}");
}

#[test]
fn a_real_run_whose_record_cannot_be_written_fails() {
    let dir = config(
        "unwritable",
        "[[standin]]\nname = \"e\"\nrun = [\"/bin/echo\"]\nreal = [\"echo\"]\n[[standin.case]]\nname = \"c\"\nargs = [\"hi\"]\n\
         [[standin]]\nname = \"d\"\nrun = [\"/bin/echo\", \"x\"]\nreal = [\"echo\"]\n[[standin.case]]\nname = \"c\"\n",
    );
    std::fs::write(dir.join(".truefit"), "a file, not a directory").unwrap();
    let (code, out) = truefit_in(&dir, &["contract", "--real"]);
    assert_eq!(code, Some(1), "{out:#}");
    for (i, file) in [".truefit/e.json", ".truefit/d.differ.json"]
        .iter()
        .enumerate()
    {
        assert_eq!(out["standins"][i]["result"], "fail");
        let fault = out["standins"][i]["fault"].as_str().unwrap();
        assert!(fault.contains(file), "{out:#}");
    }
    // `d` declares no `compare`: by default a contract compares exit and stdout.
    assert_eq!(
        out["standins"][1]["cases"][0]["differ"],
        serde_json::json!(["stdout"]),
        "by default a contract compares exit and stdout: {out:#}"
    );
}

#[test]
fn next_gives_each_kind_of_task_once_on_the_sample_with_its_evidence() {
    let dir = sample("plutil", "next");
    let (code, _) = truefit_in(&dir, &["contract", "--real"]);
    assert_eq!(code, Some(1), "plutil-wrong differs");
    let (code, out) = truefit_in(&dir, &["next", "--all"]);
    assert_eq!(code, Some(0), "{out:#}");
    assert!(out["by"].as_str().unwrap().starts_with("truefit "));
    assert_eq!(
        out["counts"],
        serde_json::json!({"no-contract": 1, "stale-contract": 1, "standin-without-faults": 1, "differing-case": 1})
    );
    let tasks = out["tasks"].as_array().unwrap();
    let kind = |k: &str| tasks.iter().find(|t| t["kind"] == k).unwrap();
    let none = kind("no-contract");
    assert_eq!(none["evidence"]["standin"], "plutil-undeclared");
    assert_eq!(
        none["files"],
        serde_json::json!(["plutil-undeclared", "truefit.toml"])
    );
    assert_eq!(none["accept"], "truefit contract --real plutil-undeclared");
    let stale = kind("stale-contract");
    assert_eq!(stale["evidence"]["standin"], "plutil-wrong");
    assert!(stale["evidence"]["real_run"].is_null());
    let differ = kind("differing-case");
    assert_eq!(
        differ["files"],
        serde_json::json!(["./plutil-wrong", ".truefit/plutil-wrong.differ.json"])
    );
    let lint = &differ["evidence"]["real_run"]["cases"][0];
    assert_eq!(lint["case"], "lint-bad");
    assert_eq!(lint["real"]["exit"], 1);
    assert_eq!(differ["accept"], "truefit contract --real plutil-wrong");
    for t in tasks {
        assert!(t["steps"].is_string() && t["summary"].is_string(), "{t:#}");
    }
    let (_, first) = truefit_in(&dir, &["next"]);
    assert_eq!(first["task"], tasks[0]);
    assert!(first.get("tasks").is_none());

    // A passing run long ago, against another version: stale for both reasons.
    std::fs::write(
        dir.join(".truefit/plutil.json"),
        r#"{"date": "2000-01-01", "version": "0.0-other"}"#,
    )
    .unwrap();
    let (_, out) = truefit_in(&dir, &["next", "--all"]);
    let plutil = out["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["evidence"]["standin"] == "plutil")
        .unwrap();
    let why = plutil["evidence"]["why"].as_array().unwrap();
    assert_eq!(why.len(), 2, "{plutil:#}");
    assert!(why[0].as_str().unwrap().contains("over fresh_days (30)"));
    assert!(why[1].as_str().unwrap().contains("0.0-other"));
}

#[test]
fn a_stand_in_without_faults_is_asked_for_them_until_marked_and_again_once_it_changes() {
    let dir = sample("plutil", "faults");
    let without = |dir: &Path| {
        let (_, out) = truefit_in(dir, &["next", "--all"]);
        out["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|t| t["kind"] == "standin-without-faults")
            .cloned()
            .collect::<Vec<_>>()
    };
    let tasks = without(&dir);
    assert_eq!(tasks.len(), 1, "plutil declares its fault: {tasks:#?}");
    let task = &tasks[0];
    assert_eq!(task["evidence"]["standin"], "plutil-wrong");
    assert_eq!(
        task["files"],
        serde_json::json!(["./plutil-wrong", "truefit.toml"])
    );
    assert_eq!(task["accept"], "truefit contract --real plutil-wrong");
    assert!(task["evidence"]["no_faults"].is_null());
    let sha = task["evidence"]["sha256"].as_str().unwrap().to_string();
    assert_eq!(sha.len(), 64);
    let toml = std::fs::read_to_string(dir.join("truefit.toml")).unwrap();
    let marked = toml.replace(
        "run = [\"./plutil-wrong\"]\n",
        &format!("run = [\"./plutil-wrong\"]\nno_faults = {{ why = \"its system never fails\", sha256 = \"{sha}\" }}\n"),
    );
    std::fs::write(dir.join("truefit.toml"), marked).unwrap();
    assert_eq!(without(&dir), Vec::<Value>::new());
    std::fs::write(dir.join("plutil-wrong"), "#!/bin/sh\necho changed\n").unwrap();
    let tasks = without(&dir);
    assert_eq!(tasks.len(), 1);
    assert!(
        tasks[0]["summary"]
            .as_str()
            .unwrap()
            .contains("changed since")
    );
    assert_eq!(
        tasks[0]["evidence"]["no_faults"]["why"],
        "its system never fails"
    );
}

#[test]
fn a_fault_case_the_real_program_cannot_produce_here_is_not_run_with_what_it_needs() {
    let dir = config(
        "unreachable",
        r#"[[standin]]
name = "t"
run = ["/bin/sh", "-c", "[ \"$TRUEFIT_FAULT\" = gone ] && exit 7; echo \"$@\"", "sh"]
real = ["echo"]
faults = ["gone"]
[[standin.case]]
name = "ok"
args = ["hi"]
[[standin.case]]
name = "unreachable"
fault = "gone"
ready = ["false"]
needs = "a network that drops"
[[standin.case]]
name = "unready"
fault = "gone"
ready = ["false"]
"#,
    );
    let (code, out) = truefit_in(&dir, &["contract", "--real"]);
    assert_eq!(code, Some(1), "{out:#}");
    let s = &out["standins"][0];
    assert_eq!(s["result"], "not run");
    assert_eq!(s["needs"], "a network that drops; `false` did not succeed");
    assert_eq!(s["cases"][0]["result"], "agree");
    assert_eq!(s["cases"][1]["result"], "not run");
    assert!(!dir.join(".truefit/t.json").exists());
    // Told its fault, the stand-in exits 7; the real side, never told, would not.
    let toml = std::fs::read_to_string(dir.join("truefit.toml")).unwrap();
    std::fs::write(
        dir.join("truefit.toml"),
        toml.replace("ready = [\"false\"]\nneeds", "needs"),
    )
    .unwrap();
    let (_, out) = truefit_in(&dir, &["contract", "--real"]);
    let case = &out["standins"][0]["cases"][1];
    assert_eq!(
        case["differ"],
        serde_json::json!(["exit", "stdout"]),
        "{out:#}"
    );
    assert_eq!(case["standin"]["exit"], 7);
    assert_eq!(case["real"]["exit"], 0);
}

#[test]
fn next_gives_nothing_once_every_tie_is_fresh() {
    let dir = config(
        "fresh",
        r#"[contract]
standins = ["fakes", "./f*/ech?", "no-such-dir/*"]
[[standin]]
name = "echo"
run = ["fakes/echo"]
real = ["echo"]
[[standin.case]]
name = "c"
args = ["hi"]
"#,
    );
    std::fs::create_dir_all(dir.join("fakes")).unwrap();
    let fake = |says: &str| {
        let path = dir.join("fakes/echo");
        std::fs::write(&path, format!("#!/bin/sh\necho {says}\n")).unwrap();
        Command::new("chmod").arg("+x").arg(&path).status().unwrap();
    };
    fake("wrong");
    truefit_in(&dir, &["contract", "--real"]);
    let (_, out) = truefit_in(&dir, &["next", "--all"]);
    assert_eq!(out["counts"]["differing-case"], 1, "{out:#}");
    fake("\"$@\"");
    let (code, _) = truefit_in(&dir, &["contract", "--real"]);
    assert_eq!(code, Some(0));
    assert!(!dir.join(".truefit/echo.differ.json").exists());
    let (_, out) = truefit_in(&dir, &["next", "--all"]);
    let sha = &out["tasks"][0]["evidence"]["sha256"];
    assert_eq!(out["tasks"][0]["kind"], "standin-without-faults", "{out:#}");
    let toml = std::fs::read_to_string(dir.join("truefit.toml")).unwrap();
    let marked = toml.replace(
        "real = [\"echo\"]\n",
        &format!(
            "real = [\"echo\"]\nno_faults = {{ why = \"echo cannot fail\", sha256 = {sha} }}\n"
        ),
    );
    std::fs::write(dir.join("truefit.toml"), marked).unwrap();
    // Passed exactly the default fresh_days (30) ago: still fresh.
    let month = Command::new("date")
        .args(["-u", "-v-30d", "+%F"])
        .output()
        .unwrap()
        .stdout;
    let month = String::from_utf8(month).unwrap();
    std::fs::write(
        dir.join(".truefit/echo.json"),
        format!(r#"{{"date": "{}", "version": null}}"#, month.trim()),
    )
    .unwrap();
    let (code, out) = truefit_in(&dir, &["next", "--all"]);
    assert_eq!(code, Some(0), "{out:#}");
    assert_eq!(out["tasks"], serde_json::json!([]), "{out:#}");
    let notes = out["notes"].as_array().unwrap();
    assert_eq!(notes.len(), 1, "{out:#}");
    assert!(notes[0].as_str().unwrap().contains("in no git repo"));
    assert!(out["next"].as_str().unwrap().starts_with("nothing"));
    let (_, out) = truefit_in(&dir, &["next"]);
    assert!(out["task"].is_null());
}

#[test]
fn next_without_declared_stand_in_places_says_so() {
    let dir = config("no-places", "");
    let (code, out) = truefit_in(&dir, &["next"]);
    assert_eq!(code, Some(0), "{out:#}");
    assert!(
        out["notes"][0]
            .as_str()
            .unwrap()
            .contains("[contract] standins")
    );
}

/// `truefit next`'s counts but the sim sample's promise and unbacked-model tasks.
fn but_promises(next: &Value) -> Value {
    let mut counts = next["counts"].clone();
    counts
        .as_object_mut()
        .unwrap()
        .retain(|kind, _| !kind.ends_with("-promise") && kind != "unbacked-model");
    counts
}

fn kept_seeds(dir: &Path) -> Vec<String> {
    let mut seeds: Vec<String> = std::fs::read_dir(dir.join(".truefit/seeds"))
        .into_iter()
        .flatten()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    seeds.sort();
    seeds
}

fn failing(out: &Value) -> Vec<Value> {
    out["new"]["failing"].as_array().unwrap().clone()
}

#[test]
fn sim_finds_the_sample_bug_keeps_its_seed_and_reruns_it_step_for_step() {
    let dir = sample("sim", "sim");
    let (code, out) = truefit_in(&dir, &["sim", "--seeds", "10"]);
    assert_eq!(code, Some(1), "{out:#}");
    assert_eq!(out["new"]["ran"], 10);
    let found = failing(&out);
    assert_eq!(out["new"]["passed"], 10 - found.len(), "{out:#}");
    let named: Vec<&Value> = out["new"]["seeds"].as_array().unwrap().iter().collect();
    assert_eq!(named.len(), 10, "{out:#}");
    let fails: Vec<&Value> = named
        .iter()
        .filter(|s| s["result"] == "fails")
        .map(|s| &s["seed"])
        .collect();
    assert_eq!(fails, found.iter().map(|f| &f["seed"]).collect::<Vec<_>>());
    assert!(
        !found.is_empty(),
        "three in four of the sample's seeds fail"
    );
    let seed = found[0]["seed"].as_str().unwrap();
    for f in &found {
        assert_eq!(f["result"], "fails", "{f:#}");
        assert_eq!(f["verdict"]["invariant"], "applied-once");
        assert_eq!(f["steps"], f["verdict"]["step"]);
    }
    assert_eq!(kept_seeds(&dir).len(), found.len());
    assert_eq!(found[0]["rerun"], format!("truefit sim --seed {seed}"));
    let file = dir.join(format!(".truefit/seeds/{seed}.json"));
    let record = std::fs::read(&file).unwrap();
    let kept: Value = serde_json::from_slice(&record).unwrap();
    assert_eq!(kept["verdict"], found[0]["verdict"]);
    assert!(kept["nondeterministic"].is_null());

    // Rerun: step for step as kept, so the record is untouched.
    let (code, again) = truefit_in(&dir, &["sim", "--seed", seed]);
    assert_eq!(code, Some(1), "{again:#}");
    assert_eq!(again["seed"]["result"], "fails");
    assert!(again["seed"].get("changed").is_none() && again["seed"].get("differs").is_none());
    assert_eq!(std::fs::read(&file).unwrap(), record);
    assert!(again["next"].as_str().unwrap().starts_with("fix the bug"));
    let (_, next) = truefit_in(&dir, &["next", "--all"]);
    assert_eq!(next["counts"]["failing-seed"], found.len(), "{next:#}");
    let task = &next["tasks"][0];
    assert_eq!(
        task["accept"],
        format!(
            "truefit sim --seed {}",
            kept_seeds(&dir)[0].trim_end_matches(".json")
        )
    );
    assert!(
        task["summary"]
            .as_str()
            .unwrap()
            .contains("invariant applied-once at step")
    );
    let (code, refused) = truefit_in(&dir, &["sim", "--forget", seed, "--why", "x"]);
    assert_eq!(code, Some(2), "a failing seed stays: {refused:#}");

    // Fixed: every kept seed reruns first, passes, and its new steps are kept.
    let queue = dir.join("queue.awk");
    let fixed = std::fs::read_to_string(&queue).unwrap().replace(
        "if (!(m in done)) applied[m]++",
        "if (!(m in done)) { applied[m]++; done[m] = 1 }",
    );
    std::fs::write(&queue, fixed).unwrap();
    let (code, out) = truefit_in(&dir, &["sim", "--seeds", "0"]);
    assert_eq!(code, Some(0), "{out:#}");
    assert_eq!(out["new"]["ran"], 0);
    let kept = out["kept"].as_array().unwrap();
    assert_eq!(kept.len(), found.len());
    assert!(
        kept.iter()
            .all(|k| k["result"] == "passes" && k["changed"]["line"].is_u64()),
        "{out:#}"
    );
    let (_, next) = truefit_in(&dir, &["next", "--all"]);
    assert_eq!(but_promises(&next), serde_json::json!({}), "{next:#}");
    let (code, out) = truefit_in(&dir, &["sim", "--seed", seed]);
    assert_eq!(code, Some(0), "{out:#}");
    assert!(out["seed"].get("changed").is_none());
    let (code, _) = truefit_in(&dir, &["sim", "--forget", seed, "--why", " "]);
    assert_eq!(code, Some(2), "a reason is needed");
    let (code, out) = truefit_in(&dir, &["sim", "--forget", seed, "--why", "fixed"]);
    assert_eq!(code, Some(0), "{out:#}");
    assert!(
        out["next"]
            .as_str()
            .unwrap()
            .contains(&format!("forget seed {seed}: fixed"))
    );
    assert!(!file.exists());
    let (code, out) = truefit_in(&dir, &["sim", "--forget", seed, "--why", "fixed"]);
    assert_eq!(code, Some(2), "{out:#}");
}

#[test]
fn sim_catches_a_nondeterministic_simulation_at_the_first_step_that_differs() {
    let dir = sample("sim", "nondet");
    let toml = std::fs::read_to_string(dir.join("truefit.toml")).unwrap();
    std::fs::write(
        dir.join("truefit.toml"),
        toml.replace("\"./sim\"", "\"./sim-nondeterministic\""),
    )
    .unwrap();
    let (code, out) = truefit_in(&dir, &["sim", "--seeds", "10"]);
    assert_eq!(code, Some(1), "{out:#}");
    let found = failing(&out);
    assert!(!found.is_empty());
    for f in &found {
        assert_eq!(f["result"], "nondeterministic", "{f:#}");
        let d = &f["differs"];
        assert_ne!(
            d["then"]["state"]["worker"], d["now"]["state"]["worker"],
            "{f:#}"
        );
        assert_eq!(d["then"]["event"], d["now"]["event"]);
    }
    assert!(
        out["next"]
            .as_str()
            .unwrap()
            .starts_with("make the simulation deterministic")
    );
    let (_, next) = truefit_in(&dir, &["next", "--all"]);
    assert_eq!(
        but_promises(&next),
        serde_json::json!({"nondeterministic-sim": 1}),
        "{next:#}"
    );
    assert_eq!(
        next["tasks"][0]["evidence"]["seeds"]
            .as_array()
            .unwrap()
            .len(),
        found.len()
    );
    let seed = found[0]["seed"].as_str().unwrap();
    let (code, out) = truefit_in(&dir, &["sim", "--seed", seed]);
    assert_eq!(code, Some(1));
    assert_eq!(out["seed"]["result"], "nondeterministic", "{out:#}");

    // Deterministic again: two reruns agree, so the flag clears and the bug remains.
    std::fs::write(dir.join("truefit.toml"), toml).unwrap();
    let (code, out) = truefit_in(&dir, &["sim", "--seeds", "0"]);
    assert_eq!(code, Some(1));
    assert!(
        out["kept"]
            .as_array()
            .unwrap()
            .iter()
            .all(|k| k["result"] == "fails" && k.get("differs").is_none()),
        "{out:#}"
    );
    let (_, next) = truefit_in(&dir, &["next", "--all"]);
    assert_eq!(
        but_promises(&next),
        serde_json::json!({"failing-seed": found.len()}),
        "{next:#}"
    );
}

#[test]
fn sim_runs_new_seeds_by_count_or_budget_and_stops_at_a_run_without_a_verdict() {
    let dir = config(
        "sim-script",
        "[sim]\nrun = [\"./s\", \"--seed\"]\nseeds = 3\n",
    );
    let script = dir.join("s");
    std::fs::write(
        &script,
        "#!/bin/sh\n[ \"$1\" = --seed ] || exit 9\necho '{\"step\":1}'\ncat verdict\n",
    )
    .unwrap();
    Command::new("chmod")
        .arg("+x")
        .arg(&script)
        .status()
        .unwrap();
    let verdict = |v: &str| std::fs::write(dir.join("verdict"), v).unwrap();
    verdict("{\"verdict\":\"pass\"}\n");
    let (code, out) = truefit_in(&dir, &["sim"]);
    assert_eq!(code, Some(0), "{out:#}");
    let new = &out["new"];
    assert_eq!(
        (&new["ran"], &new["passed"], &new["failing"]),
        (&3.into(), &3.into(), &serde_json::json!([]))
    );
    let seeds = new["seeds"].as_array().unwrap();
    assert_eq!(seeds.len(), 3, "every new seed is named: {out:#}");
    for s in seeds {
        assert_eq!(s["result"], "passes");
        let (code, again) = truefit_in(&dir, &["sim", "--seed", s["seed"].as_str().unwrap()]);
        assert_eq!(
            (code, &again["seed"]["result"]),
            (Some(0), &"passes".into()),
            "{again:#}"
        );
    }
    let (_, out) = truefit_in(&dir, &["sim", "--seeds", "2"]);
    assert_eq!(out["new"]["seeds"].as_array().unwrap().len(), 2, "{out:#}");
    assert!(out["next"].as_str().unwrap().starts_with("nothing"));
    let (_, out) = truefit_in(&dir, &["sim", "--budget", "0"]);
    assert_eq!(out["new"]["ran"], 1, "a budget runs at least one seed");
    let (_, out) = truefit_in(&dir, &["sim", "--budget", "0.2"]);
    assert!(out["new"]["ran"].as_u64().unwrap() > 1, "{out:#}");
    let (code, out) = truefit_in(&dir, &["sim", "--seed", "a-1_B"]);
    assert_eq!(code, Some(0), "{out:#}");
    assert_eq!(out["seed"]["result"], "passes");
    assert_eq!(out["seed"]["steps"], 1);
    assert!(kept_seeds(&dir).is_empty(), "a passing seed is not kept");

    verdict("{\"verdict\":\"fail\",\"invariant\":\"x\",\"step\":1}\n");
    let (code, out) = truefit_in(&dir, &["sim", "--seed", "k"]);
    assert_eq!(code, Some(1), "{out:#}");
    assert_eq!(kept_seeds(&dir), ["k.json"]);
    // Flagged nondeterministic once, it is confirmed by two runs, not by one alike.
    let file = dir.join(".truefit/seeds/k.json");
    let mut record: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    record["nondeterministic"] = serde_json::json!({"line": 1, "then": 1, "now": 2});
    std::fs::write(&file, record.to_string()).unwrap();
    let (_, out) = truefit_in(&dir, &["sim", "--seed", "k"]);
    assert_eq!(out["seed"]["result"], "fails", "{out:#}");
    verdict("boom\n");
    for args in [&["sim", "--seed", "k"][..], &["sim", "--seed", "new"]] {
        let (code, out) = truefit_in(&dir, args);
        assert_eq!(code, Some(1), "{out:#}");
        assert_eq!(out["seed"]["result"], "broken", "{out:#}");
        assert_eq!(out["seed"]["last_line"], "boom");
        assert!(
            out["next"]
                .as_str()
                .unwrap()
                .starts_with("make the simulation end every run")
        );
    }
    let (code, out) = truefit_in(&dir, &["sim", "--seeds", "5"]);
    assert_eq!(code, Some(1), "{out:#}");
    assert_eq!(out["kept"][0]["result"], "broken");
    assert_eq!(
        out["new"]["ran"], 1,
        "a broken run stops new seeds: {out:#}"
    );
    assert_eq!(kept_seeds(&dir), ["k.json"]);

    // Each `truefit sim` draws new seeds of its own.
    verdict("{\"verdict\":\"fail\"}\n");
    for _ in 0..2 {
        truefit_in(&dir, &["sim", "--seeds", "1"]);
    }
    assert_eq!(kept_seeds(&dir).len(), 3, "{:?}", kept_seeds(&dir));
    for seed in kept_seeds(&dir).iter().filter(|s| *s != "k.json") {
        std::fs::remove_file(dir.join(".truefit/seeds").join(seed)).unwrap();
    }

    // A kept seed that passes again stays kept; forgotten only while it can be removed.
    verdict("{\"verdict\":\"pass\"}\n");
    truefit_in(&dir, &["sim", "--seeds", "0"]);
    let seeds = dir.join(".truefit/seeds");
    Command::new("chmod")
        .arg("555")
        .arg(&seeds)
        .status()
        .unwrap();
    let (code, out) = truefit_in(&dir, &["sim", "--forget", "k", "--why", "w"]);
    Command::new("chmod")
        .arg("755")
        .arg(&seeds)
        .status()
        .unwrap();
    assert_eq!(code, Some(2), "{out:#}");
    assert!(
        out["error"][0]["fault"]
            .as_str()
            .unwrap()
            .starts_with("cannot remove")
    );
    assert_eq!(kept_seeds(&dir), ["k.json"]);
}

#[test]
fn sim_runs_20_new_seeds_by_default() {
    let dir = config(
        "sim-default",
        "[sim]\nrun = [\"/bin/sh\", \"-c\", \"echo '{\\\"verdict\\\":\\\"pass\\\"}'\"]\n",
    );
    let (code, out) = truefit_in(&dir, &["sim"]);
    assert_eq!(code, Some(0), "{out:#}");
    assert_eq!(out["new"]["ran"], 20);
}

#[test]
fn sim_faults_exit_2() {
    let unwritable = config(
        "sim-unwritable",
        "[sim]\nrun = [\"/bin/sh\", \"-c\", \"echo '{\\\"verdict\\\":\\\"fail\\\"}'\"]\n",
    );
    std::fs::write(unwritable.join(".truefit"), "a file, not a directory").unwrap();
    let sim = config("sim-args", "[sim]\nrun = [\"/bin/echo\"]\n");
    for (dir, args, says) in [
        (config("sim-none", ""), vec!["sim"], "no [sim]"),
        (
            config("sim-empty", "[sim]\nrun = []\n"),
            vec!["sim"],
            "[sim] run is empty",
        ),
        (
            config("sim-missing", "[sim]\nrun = [\"./nope\"]\n"),
            vec!["sim"],
            "not found",
        ),
        (
            config("sim-plain-file", "[sim]\nrun = [\"./truefit.toml\"]\n"),
            vec!["sim"],
            "cannot run",
        ),
        (
            unwritable,
            vec!["sim", "--seed", "1"],
            "cannot write .truefit/seeds/1.json",
        ),
        (sim.clone(), vec!["sim", "--seed", "a/b"], "is not a seed"),
        (
            sim.clone(),
            vec!["sim", "--forget", "", "--why", "w"],
            "is not a seed",
        ),
        (
            sim.clone(),
            vec!["sim", "--forget", "x", "--why", "w"],
            "is not kept",
        ),
        (
            sim.clone(),
            vec!["sim", "--seeds", "many"],
            "not a sim command",
        ),
        (
            sim.clone(),
            vec!["sim", "--budget", "soon"],
            "not a sim command",
        ),
        (sim, vec!["sim", "--frob"], "not a sim command"),
    ] {
        let (code, out) = truefit_in(&dir, &args);
        assert_eq!(code, Some(2), "{args:?}: {out:#}");
        let fault = out["error"][0]["fault"].as_str().unwrap();
        assert!(fault.contains(says), "{args:?}: {out:#}");
        assert!(out["error"][0]["fix"].is_string());
    }
}

/// `testdata/sim` copied, with `./sim <seed>`'s records written to `<seed>.jsonl` there.
fn model_sample(tag: &str, seeds: &[&str]) -> PathBuf {
    let dir = sample("sim", tag);
    for seed in seeds {
        let out = Command::new("./sim")
            .arg(seed)
            .current_dir(&dir)
            .output()
            .unwrap();
        std::fs::write(dir.join(format!("{seed}.jsonl")), out.stdout).unwrap();
    }
    dir
}

/// truefit#13: under SYMMETRY, `states` is TLC's final count of distinct states.
#[test]
fn model_states_are_tlcs_distinct_states_under_symmetry() {
    let dir = sample("symmetry", "symmetry");
    let (code, out) = truefit_in(&dir, &["model"]);
    assert_eq!(code, Some(0), "{out:#}");
    let counters = &out["models"][0];
    // Up to the five workers' permutations: C(14,5) = 2002 multisets of counts 0..9.
    assert_eq!(
        (&counters["result"], &counters["states"], &counters["depth"]),
        (&"passes".into(), &2002.into(), &46.into()),
        "{out:#}"
    );
}

/// truefit#15: TLC unpacks each standard module it reads into java.io.tmpdir
/// (`Naturals.tla` there) and deletes it on exit, so two runs sharing one read
/// each other's half-written or deleted modules ("Module-Table lookup
/// failure"). Each run has its own: the sample's passing check runs twice at
/// once, both pass, and the host's shared temp dir (here a private stand-in
/// for it, set as JVM-wide default through JAVA_TOOL_OPTIONS) is never
/// written or emptied. The race itself is rare (3 of 50 runs here without the
/// fix), so the untouched module is what fails at once if a run shares the
/// temp dir again.
#[test]
fn model_checks_the_sample_with_the_pinned_tlc_two_at_once_each_in_its_own_temp_dir_and_names_a_broken_property()
 {
    let dir = model_sample("model", &[]);
    // Without its `limit`, a model has 300 seconds.
    let toml = std::fs::read_to_string(dir.join("truefit.toml")).unwrap();
    let toml: Vec<&str> = toml
        .lines()
        .filter(|l| !l.starts_with("limit = "))
        .collect();
    std::fs::write(dir.join("truefit.toml"), toml.join("\n")).unwrap();
    let shared = dir.join("shared-tmp");
    std::fs::create_dir_all(&shared).unwrap();
    let planted = shared.join("Naturals.tla");
    std::fs::write(&planted, "another run's module, half written").unwrap();
    let pair: Vec<_> = (0..2)
        .map(|_| {
            Command::new(env!("CARGO_BIN_EXE_truefit"))
                .arg("model")
                .current_dir(&dir)
                .env(
                    "JAVA_TOOL_OPTIONS",
                    format!("{C1_ONLY} -Djava.io.tmpdir={}", shared.display()),
                )
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    for child in pair {
        let run = child.wait_with_output().unwrap();
        let out: Value = serde_json::from_slice(&run.stdout).unwrap_or(Value::Null);
        assert_eq!(
            run.status.code(),
            Some(0),
            "both runs at once pass: {out:#}"
        );
        let queue = &out["models"][0];
        assert_eq!(
            (&queue["name"], &queue["result"]),
            (&"queue".into(), &"passes".into()),
            "both runs at once pass: {out:#}"
        );
        assert_eq!((&queue["states"], &queue["depth"]), (&7.into(), &4.into()));
        assert_eq!(queue["limit"], 300);
        assert_eq!(out["tlc"]["version"], "1.7.4");
        assert!(
            out["tlc"]["jar"]
                .as_str()
                .unwrap()
                .ends_with(".local/share/truefit/tlc/1.7.4/tla2tools.jar")
        );
        // Its committed traces held: the accept trace allowed, the reject trace breaking AppliedOnce.
        let traces = &queue["traces"];
        assert_eq!(
            traces["accept"][0]["file"],
            ".truefit/traces/queue/accept/1.jsonl"
        );
        assert_eq!(traces["accept"][0]["result"], "allowed");
        let reject = &traces["reject"][0];
        assert_eq!(
            (
                &reject["result"],
                &reject["step"],
                &reject["breaks"],
                &reject["breaks_at"]
            ),
            (
                &"disallowed".into(),
                &3.into(),
                &"AppliedOnce".into(),
                &3.into()
            ),
            "{out:#}"
        );
    }
    let record: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join(".truefit/models/queue.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(record["result"], "passes");
    assert!(record["commit"].is_null(), "in no git repo: {record:#}");
    assert_eq!(record["traces"]["reject"][0]["breaks"], "AppliedOnce");
    assert_eq!(
        std::fs::read_to_string(&planted).ok().as_deref(),
        Some("another run's module, half written"),
        "a TLC run wrote or deleted a module in the shared temp dir"
    );
    // The design's bug, put in the model: a redelivered message is applied again.
    let spec = std::fs::read_to_string(dir.join("Queue.tla")).unwrap();
    std::fs::write(
        dir.join("Queue.tla"),
        spec.replace("IF head \\in done", "IF FALSE"),
    )
    .unwrap();
    let (code, out) = truefit_c1_in(&dir, &["model", "queue"]);
    assert_eq!(code, Some(1), "{out:#}");
    let queue = &out["models"][0];
    assert_eq!(queue["result"], "fails");
    assert_eq!(queue["error"], "Invariant AppliedOnce is violated.");
    let behaviour = queue["behaviour"].as_array().unwrap();
    assert_eq!(behaviour[0]["action"], "Initial predicate");
    assert!(
        behaviour[1]["action"]
            .as_str()
            .unwrap()
            .starts_with("ApplyCrash")
    );
    assert_eq!(behaviour.last().unwrap()["state"]["applied"], "<<2, 0, 0>>");
    assert!(queue["output"].is_null());
    // Too permissive: it allows the committed run that applies a message twice.
    assert_eq!(queue["traces"]["reject"][0]["result"], "allowed", "{out:#}");
    assert!(
        queue["wrong"][0]
            .as_str()
            .unwrap()
            .contains("too permissive")
    );
    assert!(
        out["next"]
            .as_str()
            .unwrap()
            .contains("never weaken the property")
    );
    // A spec TLC cannot parse is broken, its output the fault to correct.
    std::fs::write(
        dir.join("Queue.tla"),
        "---- MODULE Queue ----\nVARIABLE\n====\n",
    )
    .unwrap();
    let (code, out) = truefit_c1_in(&dir, &["model", "queue"]);
    assert_eq!(code, Some(1), "{out:#}");
    let queue = &out["models"][0];
    assert_eq!(queue["result"], "broken", "{out:#}");
    assert!(
        queue["output"]
            .as_str()
            .unwrap()
            .contains("Parsing or semantic analysis failed")
    );
    // Its committed traces with it: broken, not disallowed, and breaking nothing.
    let file = ".truefit/traces/queue/reject/3.jsonl";
    let wrong = &queue["wrong"];
    assert_eq!(
        wrong[1],
        format!("reject trace {file} is broken"),
        "{out:#}"
    );
    let unread = format!("reject trace {file}: its invariants' check is broken: Error: Parsing");
    assert!(wrong[2].as_str().unwrap().starts_with(&unread), "{out:#}");
    assert!(
        out["next"]
            .as_str()
            .unwrap()
            .contains("TLC could not check a model: correct the spec or config"),
        "{out:#}"
    );
}

/// The sample with `accepts` copies of its accept trace and no reject trace,
/// and on PATH a stand-in `java` that answers `answer` to every TLC run, after
/// it has held its place in `alive/` for a moment. Each run appends to
/// `alive.log` how many were alive with it.
fn model_with_stand_in_java(tag: &str, accepts: usize, answer: &str) -> (PathBuf, String) {
    let dir = model_sample(tag, &[]);
    let traces = dir.join(".truefit/traces/queue");
    std::fs::remove_dir_all(traces.join("reject")).unwrap();
    let one = std::fs::read(traces.join("accept/1.jsonl")).unwrap();
    for i in 2..=accepts {
        std::fs::write(traces.join(format!("accept/{i:02}.jsonl")), &one).unwrap();
    }
    let (bin, alive) = (dir.join("bin"), dir.join("alive"));
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&alive).unwrap();
    let java = bin.join("java");
    std::fs::write(
        &java,
        format!(
            "#!/bin/sh\nmkdir {a}/$$\nls {a} | wc -l >> {a}.log\nsleep 0.3\nrmdir {a}/$$\nprintf '{answer}'\n",
            a = alive.display()
        ),
    )
    .unwrap();
    let ok = Command::new("chmod").arg("+x").arg(&java).status().unwrap();
    assert!(ok.success());
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    (dir, path)
}

/// A model's checks, one JVM each, run at most four (or the
/// host's CPUs) at once, each answered in its trace's place.
#[test]
fn a_models_checks_run_at_most_the_bound_at_once_and_answer_as_one_at_a_time() {
    let bound = std::thread::available_parallelism().unwrap().get().min(4);
    // TLC's check passes; each trace check reaches the trace's end.
    let (dir, path) = model_with_stand_in_java(
        "model-bound",
        bound + 3,
        "Model checking completed. No error has been found.\\nError: Invariant TruefitUnfinished is violated.\\n",
    );
    let (code, out) = answer(
        Command::new(env!("CARGO_BIN_EXE_truefit"))
            .arg("model")
            .current_dir(&dir)
            .env("PATH", path),
    );
    assert_eq!(code, Some(0), "{out:#}");
    let queue = &out["models"][0];
    assert_eq!(queue["result"], "passes", "{out:#}");
    let files: Vec<&str> = queue["traces"]["accept"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            assert_eq!(t["result"], "allowed", "{t:#}");
            t["file"].as_str().unwrap()
        })
        .collect();
    let mut sorted = files.clone();
    sorted.sort();
    assert_eq!((files.len(), &files), (bound + 3, &sorted));
    let log = std::fs::read_to_string(dir.join("alive.log")).unwrap();
    let alive: Vec<usize> = log.lines().map(|l| l.trim().parse().unwrap()).collect();
    assert_eq!(alive.len(), bound + 4, "TLC's run and each trace's: {log}");
    let most = *alive.iter().max().unwrap();
    assert!(most <= bound, "{most} JVMs at once, past {bound}: {log}");
    assert!(most > 1, "they ran one at a time: {log}");
}

/// A check that panics is a fault naming its trace, exit 2, not
/// a panic of truefit. Here TLC says the search ended past the trace's last
/// record, which reads past the trace's records.
#[test]
fn a_check_that_panics_is_a_fault_naming_its_trace() {
    let (dir, path) = model_with_stand_in_java(
        "model-panic",
        1,
        "Model checking completed. No error has been found.\\nThe depth of the complete state graph search is 99.\\n",
    );
    let (code, out) = answer(
        Command::new(env!("CARGO_BIN_EXE_truefit"))
            .arg("model")
            .current_dir(&dir)
            .env("PATH", path),
    );
    assert_eq!(code, Some(2), "{out:#}");
    let fault = out["error"][0]["fault"].as_str().unwrap();
    assert!(
        fault.starts_with("the check of trace .truefit/traces/queue/accept/1.jsonl panicked: "),
        "{out:#}"
    );
    assert!(out["error"][0]["fix"].is_string(), "{out:#}");
}

#[test]
fn a_trace_the_model_allows_passes_and_one_it_does_not_names_its_first_disallowed_step() {
    // Seed 1 runs every message once; seed 3 crashes after applying message
    // 2 and, redelivered, applies it again at step 3.
    let dir = model_sample("trace", &["1", "3"]);
    let (code, out) = truefit_in(&dir, &["model", "--trace", "1.jsonl"]);
    assert_eq!(code, Some(0), "{out:#}");
    assert_eq!(out["trace"]["result"], "allowed");
    assert_eq!(out["trace"]["records"], 3);
    let (code, out) = truefit_in(&dir, &["model", "queue", "--trace", "3.jsonl"]);
    assert_eq!(code, Some(1), "{out:#}");
    let t = &out["trace"];
    assert_eq!(
        (&t["result"], &t["step"], &t["records"]),
        (&"disallowed".into(), &3.into(), &3.into())
    );
    assert_eq!(t["record"]["event"], "apply 2, ack");
    assert_eq!(t["after"]["event"], "apply 2, crash");
    // A kept seed is a trace as it is.
    let (code, _) = truefit_in(&dir, &["sim", "--seed", "3"]);
    assert_eq!(code, Some(1));
    let (code, out) = truefit_in(&dir, &["model", "--trace", ".truefit/seeds/3.json"]);
    assert_eq!(code, Some(1), "{out:#}");
    assert_eq!(out["trace"]["step"], 3);
    // Its first record disallowed: nothing after; a quote, backslash or newline in a record is no TLA+ fault.
    std::fs::write(
        dir.join("first.jsonl"),
        r#"{"step":1,"state":{"applied":[0,1,0],"next":1},"note":"a \"quote\", a back\\slash\nand a line"}"#,
    )
    .unwrap();
    let (_, out) = truefit_in(&dir, &["model", "--trace", "first.jsonl"]);
    assert_eq!(
        (&out["trace"]["step"], &out["trace"]["after"]),
        (&1.into(), &Value::Null),
        "{out:#}"
    );
    // A mapping TLC cannot read a record by is broken, not disallowed.
    let toml = std::fs::read_to_string(dir.join("truefit.toml")).unwrap();
    std::fs::write(
        dir.join("truefit.toml"),
        toml.replace("r.state.next", "r.state.nope"),
    )
    .unwrap();
    let (code, out) = truefit_in(&dir, &["model", "--trace", "1.jsonl"]);
    assert_eq!(code, Some(1), "{out:#}");
    assert_eq!(out["trace"]["result"], "broken");
    assert!(
        out["trace"]["output"].as_str().unwrap().contains("nope"),
        "{out:#}"
    );
    assert!(
        out["next"]
            .as_str()
            .unwrap()
            .starts_with("correct [model.trace]")
    );
}

#[test]
fn a_model_holds_its_committed_traces_an_accept_allowed_and_a_reject_that_breaks_the_promise() {
    // Its passing run, the accept trace allowed and the reject trace breaking AppliedOnce: the at-once test.
    let dir = model_sample("backed", &[]);
    let spec = std::fs::read_to_string(dir.join("Queue.tla")).unwrap();
    let fails = |changed: &str, naming: &str| {
        std::fs::write(dir.join("Queue.tla"), changed).unwrap();
        let (code, out) = truefit_in(&dir, &["model", "queue"]);
        assert_eq!(code, Some(1), "{out:#}");
        let error = out["models"][0]["error"].as_str().unwrap();
        assert!(error.contains(naming), "{out:#}");
        assert!(
            out["next"]
                .as_str()
                .unwrap()
                .contains("never weaken the property")
        );
        error.to_string()
    };
    // The property weakened: TLC passes, but the run that breaks it is no longer caught.
    let weak = fails(
        &spec.replace("applied[m] <= 1", "applied[m] <= 2"),
        ".truefit/traces/queue/reject/3.jsonl",
    );
    assert!(weak.contains("too weak"), "{weak}");
    // A guard too strict: the real run's last ack is no step of it.
    let strict = fails(
        &spec.replace("ApplyAck == /\\ head <= N", "ApplyAck == /\\ head < N"),
        ".truefit/traces/queue/accept/1.jsonl is disallowed at step 3",
    );
    assert!(!strict.contains("reject"), "{strict}");
    truefit_in(&dir, &["sim", "--seeds", "20"]);
    let why = |dir: &Path| {
        let (_, out) = truefit_in(dir, &["promise"]);
        assert_eq!(out["promises"][0]["status"], "unbacked", "{out:#}");
        out["promises"][0]["why"].as_str().unwrap().to_string()
    };
    // A failed check backs nothing, nor a record with no accept trace allowed.
    let failed = why(&dir);
    assert!(failed.contains("its last check fails"), "{failed}");
    assert!(failed.contains("no accept trace it allows"), "{failed}");
    std::fs::write(dir.join("Queue.tla"), &spec).unwrap();
    // Without a reject trace the model still passes; its promises are unbacked.
    std::fs::remove_dir_all(dir.join(".truefit/traces/queue/reject")).unwrap();
    let (code, out) = truefit_in(&dir, &["model"]);
    assert_eq!(code, Some(0), "{out:#}");
    assert_eq!(out["models"][0]["traces"]["reject"], serde_json::json!([]));
    assert!(why(&dir).contains("no reject trace it disallows that breaks `applied-once`"));
    // A reject trace that breaks another invariant (TypeOK: a head past N + 1) backs not this promise.
    let reject = dir.join(".truefit/traces/queue/reject");
    std::fs::create_dir_all(&reject).unwrap();
    std::fs::write(
        reject.join("head.jsonl"),
        r#"{"step":1,"state":{"applied":[0,0,0],"next":9}}"#,
    )
    .unwrap();
    let (code, out) = truefit_in(&dir, &["model"]);
    assert_eq!(code, Some(0), "{out:#}");
    assert_eq!(out["models"][0]["traces"]["reject"][0]["breaks"], "TypeOK");
    assert!(why(&dir).contains("no reject trace it disallows that breaks `applied-once`"));
    // A record of other inputs backs nothing.
    std::fs::write(dir.join("Queue.tla"), spec + "\\* a change\n").unwrap();
    assert!(why(&dir).contains("changed since its last `truefit model`"));
}

#[test]
fn a_model_past_its_limit_is_timed_out() {
    let dir = config(
        "model-limit",
        "[[model]]\nname = \"count\"\nspec = \"Count.tla\"\nconfig = \"Count.cfg\"\nlimit = 1\ncommit = \"x\"\ncovers = [{ action = \"Next\", file = \"c\", function = \"f\" }]\n",
    );
    std::fs::write(dir.join("Count.tla"), "---- MODULE Count ----\nEXTENDS Naturals\nVARIABLE x\nInit == x = 0\nNext == x' = x + 1\n====\n").unwrap();
    std::fs::write(dir.join("Count.cfg"), "INIT Init\nNEXT Next\n").unwrap();
    let start = std::time::Instant::now();
    let (code, out) = truefit_in(&dir, &["model"]);
    assert_eq!(code, Some(1), "{out:#}");
    assert_eq!(out["models"][0]["result"], "timed out");
    assert!(out["models"][0]["output"].is_string(), "{out:#}");
    assert!(start.elapsed().as_secs() < 10);
    assert!(out["next"].as_str().unwrap().contains("bounds smaller"));
}

#[test]
fn tlc_whose_checksum_is_not_the_pinned_one_is_not_installed() {
    let home = config("model-home", "");
    let bad = home.join("bad.jar");
    std::fs::write(&bad, "not TLC").unwrap();
    let dir = model_sample("model-bad-tlc", &[]);
    let out = Command::new(env!("CARGO_BIN_EXE_truefit"))
        .arg("model")
        .current_dir(&dir)
        .env("HOME", &home)
        .env("TRUEFIT_TLC_FROM", format!("file://{}", bad.display()))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let out: Value = serde_json::from_slice(&out.stdout).unwrap();
    let fault = out["error"][0]["fault"].as_str().unwrap();
    assert!(
        fault.contains("its SHA-256 is") && fault.contains("936a2620"),
        "{out:#}"
    );
    let tlc = home.join(".local/share/truefit/tlc/1.7.4");
    assert_eq!(std::fs::read_dir(tlc).unwrap().count(), 0);
}

/// What TLC's install and run cannot reach is a fault naming it, before any
/// TLC runs: no HOME, no curl, a source curl cannot read, no java.
#[test]
fn tlc_installs_from_its_source_and_a_missing_home_curl_source_or_java_is_a_fault() {
    let dir = model_sample("model-reach", &[]);
    // The pinned TLC in this HOME, installed if it is not yet: a missing trace faults before TLC runs.
    let (code, out) = truefit_in(&dir, &["model", "--trace", "none.jsonl"]);
    assert_eq!(code, Some(2), "{out:#}");
    let pinned = PathBuf::from(std::env::var("HOME").unwrap())
        .join(".local/share/truefit/tlc/1.7.4/tla2tools.jar");
    let home = config("model-reach-home", "");
    let tlc = home.join(".local/share/truefit/tlc/1.7.4");
    let _ = std::fs::remove_dir_all(home.join(".local"));
    // A PATH with curl on it and nothing else.
    let bin = home.join("bin");
    let _ = std::fs::remove_dir_all(&bin);
    std::fs::create_dir_all(&bin).unwrap();
    let curl = Command::new("sh")
        .args(["-c", "command -v curl"])
        .output()
        .unwrap();
    std::os::unix::fs::symlink(
        String::from_utf8(curl.stdout).unwrap().trim(),
        bin.join("curl"),
    )
    .unwrap();
    let fault = |home: Option<&Path>, path: &Path, from: &Path| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_truefit"));
        c.arg("model")
            .current_dir(&dir)
            .env("PATH", path)
            .env("TRUEFIT_TLC_FROM", format!("file://{}", from.display()));
        match home {
            Some(h) => c.env("HOME", h),
            None => c.env_remove("HOME"),
        };
        let out = c.output().unwrap();
        assert_eq!(out.status.code(), Some(2));
        let out: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert!(out["error"][0]["fix"].is_string(), "{out:#}");
        out["error"][0]["fault"].as_str().unwrap().to_string()
    };
    let says = |fault: String, what: &str| assert!(fault.contains(what), "{fault}");
    says(fault(None, &bin, &pinned), "HOME is not set");
    says(
        fault(Some(&home), &home.join("none"), &pinned),
        "1.7.4 from file://",
    );
    says(fault(Some(&home), &home.join("none"), &pinned), ": curl: ");
    says(
        fault(Some(&home), &bin, &home.join("none.jar")),
        ": curl exit status",
    );
    assert_eq!(std::fs::read_dir(&tlc).unwrap().count(), 0);
    // Installed from its source; then there is no java to run it on.
    says(fault(Some(&home), &bin, &pinned), "cannot run TLC (java)");
    let installed: Vec<_> = std::fs::read_dir(&tlc)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(installed, ["tla2tools.jar"]);
    assert_eq!(
        std::fs::read(tlc.join("tla2tools.jar")).unwrap(),
        std::fs::read(&pinned).unwrap()
    );
}

#[test]
fn next_gives_a_stale_model_once_its_covered_code_changes_after_its_commit() {
    let dir = model_sample("stale", &[]);
    let (code, out) = truefit_in(&dir, &["next"]);
    assert_eq!(code, Some(0));
    assert!(
        out["notes"][1].as_str().unwrap().contains("in no git repo"),
        "{out:#}"
    );
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    };
    git(&["init", "-q"]);
    git(&["add", "."]);
    git(&["commit", "-qm", "one"]);
    let one = git(&["rev-parse", "HEAD"]);
    let toml = std::fs::read_to_string(dir.join("truefit.toml")).unwrap();
    let toml = with_commit(&toml, &one);
    std::fs::write(dir.join("truefit.toml"), &toml).unwrap();
    git(&["commit", "-qam", "confirm the model at one"]);
    let (_, out) = truefit_in(&dir, &["next", "--all"]);
    assert_eq!(but_promises(&out), serde_json::json!({}), "{out:#}");
    let awk = std::fs::read_to_string(dir.join("queue.awk")).unwrap();
    std::fs::write(dir.join("queue.awk"), awk + "# a change\n").unwrap();
    let (_, out) = truefit_in(&dir, &["next"]);
    let task = &out["task"];
    assert_eq!(task["kind"], "stale-model", "{out:#}");
    assert_eq!(
        task["summary"],
        format!("model queue: queue.awk changed since {one}")
    );
    assert_eq!(
        task["evidence"]["changed"][0]["covers"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(task["accept"], "truefit model queue");
    std::fs::write(dir.join("truefit.toml"), with_commit(&toml, "0000000")).unwrap();
    let (_, out) = truefit_in(&dir, &["next"]);
    assert!(
        out["task"]["summary"]
            .as_str()
            .unwrap()
            .contains("is not in this repo's history"),
        "{out:#}"
    );
}

/// `toml` with its model's `commit` set to `commit`.
fn with_commit(toml: &str, commit: &str) -> String {
    toml.lines()
        .map(|l| {
            if l.starts_with("commit = ") {
                format!("commit = \"{commit}\"")
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn model_faults_exit_2() {
    let covers = "commit = \"x\"\ncovers = [{ action = \"A\", file = \"f\", function = \"g\" }]\n";
    let model = |name: &str, extra: &str| {
        format!("[[model]]\nname = \"{name}\"\nspec = \"M.tla\"\nconfig = \"M.cfg\"\n{extra}")
    };
    let traced = config(
        "model-traced",
        &(model("a", covers)
            + "[model.trace]\nx = \"r.x\"\n"
            + &model("b", covers)
            + "[model.trace]\nx = \"r.x\"\n"),
    );
    std::fs::write(traced.join("t.jsonl"), "{\"step\":1}\n").unwrap();
    std::fs::write(traced.join("none.jsonl"), "{\"verdict\":\"pass\"}\n").unwrap();
    let plain = config("model-plain", &model("m", covers));
    for (dir, args, says) in [
        (config("model-none", ""), vec!["model"], "no [[model]]"),
        (
            config("model-uncovered", &model("m", "")),
            vec!["model"],
            "has no coverage record",
        ),
        (
            config("model-commit-only", &model("m", "commit = \"x\"\n")),
            vec!["model"],
            "has no coverage record",
        ),
        (
            config("model-twice", &(model("m", covers) + &model("m", covers))),
            vec!["model"],
            "declared twice",
        ),
        (
            config("model-zero", &model("m", &format!("{covers}limit = 0\n"))),
            vec!["model"],
            "limit of 0",
        ),
        (plain.clone(), vec!["model", "other"], "no model `other`"),
        (
            plain.clone(),
            vec!["model", "--trace", "t.jsonl"],
            "no model declares a [model.trace]",
        ),
        (plain.clone(), vec!["model", "m"], "is not found"),
        (
            plain.clone(),
            vec!["model", "--frob"],
            "not a model command",
        ),
        (
            plain.clone(),
            vec!["model", "--frob", "--trace", "t.jsonl"],
            "not a model command",
        ),
        (plain, vec!["model", "m", "--trace"], "not a model command"),
        (
            traced.clone(),
            vec!["model", "--trace", "t.jsonl"],
            "more than one model",
        ),
        (
            traced.clone(),
            vec!["model", "a", "--trace", "gone.jsonl"],
            "cannot read trace",
        ),
        (
            traced,
            vec!["model", "a", "--trace", "none.jsonl"],
            "holds no record",
        ),
    ] {
        let (code, out) = truefit_in(&dir, &args);
        assert_eq!(code, Some(2), "{args:?}: {out:#}");
        let fault = out["error"][0]["fault"].as_str().unwrap();
        assert!(fault.contains(says), "{args:?}: {out:#}");
        assert!(out["error"][0]["fix"].is_string());
    }
}

/// Each sample promise's status, by name.
fn statuses(out: &Value) -> Vec<(&str, &str)> {
    let promises = out["promises"].as_array().unwrap();
    promises
        .iter()
        .map(|p| (p["name"].as_str().unwrap(), p["status"].as_str().unwrap()))
        .collect()
}

#[test]
fn promises_are_checked_and_reached_unreached_unchecked_or_kept() {
    let dir = sample("sim", "promise");
    let (code, out) = truefit_in(&dir, &["promise"]);
    assert_eq!(code, Some(1), "{out:#}");
    assert!(
        out["promises"][0]["why"]
            .as_str()
            .unwrap()
            .contains("no `truefit sim` run is recorded"),
        "{out:#}"
    );
    // Seeds that crash redeliver a message: their steps check applied-once.
    let (_, sim) = truefit_in(&dir, &["sim", "--seeds", "20"]);
    assert!(
        sim["reached"]["applied-once"]["seeds"].as_u64().unwrap() > 0,
        "{sim:#}"
    );
    // Reached by the model, but not backed until `truefit model` records its traces.
    let (_, out) = truefit_in(&dir, &["promise"]);
    assert_eq!(out["promises"][0]["status"], "unbacked", "{out:#}");
    assert!(
        out["promises"][0]["why"]
            .as_str()
            .unwrap()
            .contains("no `truefit model queue` run is recorded"),
        "{out:#}"
    );
    let (_, next) = truefit_in(&dir, &["next", "--all"]);
    let task = next["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["kind"] == "unbacked-model")
        .unwrap();
    assert_eq!(task["evidence"]["promises"][0]["promise"], "applied-once");
    assert_eq!(task["summary"], "model queue does not back applied-once");
    let (code, _) = truefit_in(&dir, &["model"]);
    assert_eq!(code, Some(0));
    let (code, out) = truefit_in(&dir, &["promise"]);
    assert_eq!(code, Some(1));
    assert!(
        !out["next"].as_str().unwrap().contains("unbacked"),
        "{out:#}"
    );
    let want = [
        ("applied-once", "checked"),
        ("no-lost-message", "unreached"),
        ("head-in-range", "unchecked"),
        ("seeded", "kept"),
    ];
    assert_eq!(statuses(&out), want, "{out:#}");
    let why = out["promises"][1]["why"].as_str().unwrap();
    assert!(
        why.contains("no step of the last `truefit sim` (20 seeds"),
        "{why}"
    );
    assert!(
        why.contains("Queue.cfg checks no invariant named `no-lost-message`"),
        "{why}"
    );

    // The one candidate no promise registers is asked, then not again once marked.
    let unmarked = out["candidates"]["unmarked"].as_array().unwrap();
    assert_eq!(out["candidates"]["found"], 5);
    assert_eq!(unmarked.len(), 1, "{out:#}");
    assert_eq!(unmarked[0]["source"], "SPEC.md:11");
    let hash = unmarked[0]["hash"].as_str().unwrap().to_string();
    let (_, next) = truefit_in(&dir, &["next", "--all"]);
    let kinds = serde_json::json!({"unregistered-promise": 1, "unreached-promise": 1, "unchecked-promise": 1});
    let mut counts = next["counts"].clone();
    counts.as_object_mut().unwrap().remove("failing-seed");
    assert_eq!(counts, kinds, "{next:#}");
    let tasks = next["tasks"].as_array().unwrap();
    let task = tasks
        .iter()
        .find(|t| t["kind"] == "unregistered-promise")
        .unwrap();
    assert_eq!(task["accept"], "truefit promise");
    assert_eq!(task["evidence"]["unmarked"][0]["hash"], hash.as_str());
    let (code, out) = truefit_in(&dir, &["promise", "--mark", &hash, "maybe"]);
    assert_eq!(code, Some(2), "{out:#}");
    let (code, out) = truefit_in(&dir, &["promise", "--mark", "0000", "not"]);
    assert_eq!(code, Some(2), "{out:#}");
    assert!(
        out["error"][0]["fault"]
            .as_str()
            .unwrap()
            .contains("no candidate's hash")
    );
    // A mark is a hash and its mark: one without the other is refused before any is read.
    for marks in [&[][..], &[hash.as_str()]] {
        let (code, out) = truefit_in(&dir, &[&["promise", "--mark"][..], marks].concat());
        assert_eq!(code, Some(2), "{out:#}");
        assert!(
            out["error"][0]["fault"]
                .as_str()
                .unwrap()
                .contains("is not a promise command"),
            "{out:#}"
        );
    }
    let (_, out) = truefit_in(
        &dir,
        &["promise", "--mark", &hash, "not", "--why", "a description"],
    );
    let marks = std::fs::read_to_string(dir.join(".truefit/promises.json")).unwrap();
    let marks: Value = serde_json::from_str(&marks).unwrap();
    assert_eq!(marks[&hash]["why"], "a description", "{marks:#}");
    assert_eq!(
        out["candidates"]["unmarked"],
        serde_json::json!([]),
        "{out:#}"
    );
    let (_, next) = truefit_in(&dir, &["next", "--all"]);
    assert!(
        next["counts"].get("unregistered-promise").is_none(),
        "{next:#}"
    );

    // A sentence that changed is asked again; one marked a promise waits to be registered.
    let spec = std::fs::read_to_string(dir.join("SPEC.md")).unwrap();
    std::fs::write(dir.join("SPEC.md"), spec.replace("in awk", "in awk alone")).unwrap();
    let (_, out) = truefit_in(&dir, &["promise"]);
    let again = out["candidates"]["unmarked"][0]["hash"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(again, hash);
    let (_, out) = truefit_in(&dir, &["promise", "--mark", &again, "promise"]);
    assert_eq!(
        out["candidates"]["unregistered"][0]["hash"],
        again.as_str(),
        "{out:#}"
    );

    // A sim run fresh_days old is fresh; one older reaches nothing.
    let toml = std::fs::read_to_string(dir.join("truefit.toml")).unwrap();
    std::fs::write(
        dir.join("truefit.toml"),
        format!("[contract]\nfresh_days = 0\n{toml}"),
    )
    .unwrap();
    let (_, out) = truefit_in(&dir, &["promise"]);
    assert_eq!(out["promises"][0]["status"], "checked", "{out:#}");
    let reached = dir.join(".truefit/sim.reached.json");
    let old = std::fs::read_to_string(&reached).unwrap();
    let date = serde_json::from_str::<Value>(&old).unwrap()["date"]
        .as_str()
        .unwrap()
        .to_string();
    std::fs::write(&reached, old.replace(&date, "2000-01-01")).unwrap();
    let (_, out) = truefit_in(&dir, &["promise"]);
    assert_eq!(out["promises"][0]["status"], "unreached");
    assert!(
        out["promises"][0]["why"]
            .as_str()
            .unwrap()
            .contains("over fresh_days"),
        "{out:#}"
    );
}

#[test]
fn a_promise_of_what_runs_at_once_needs_sim_and_model_or_a_reason() {
    let promise = |extra: &str| {
        format!(
            "[promises]\nsources = []\n[[promise]]\nname = \"redelivered\"\nsays = \"A message lost in a crash is delivered again.\"\nsource = \"SPEC.md:1\"\ncheck = [\"test\"]\n{extra}"
        )
    };
    let dir = config("promise-concurrent", &promise(""));
    let (code, out) = truefit_in(&dir, &["promise"]);
    assert_eq!(code, Some(1), "{out:#}");
    assert_eq!(out["promises"][0]["status"], "unchecked");
    assert!(
        out["promises"][0]["why"]
            .as_str()
            .unwrap()
            .contains("needs both `sim` and `model`")
    );
    let (_, next) = truefit_in(&dir, &["next"]);
    assert_eq!(next["task"]["kind"], "unchecked-promise", "{next:#}");
    std::fs::write(
        dir.join("truefit.toml"),
        promise("why = \"its test drives every crash point\""),
    )
    .unwrap();
    let (code, out) = truefit_in(&dir, &["promise"]);
    assert_eq!(
        (code, &out["promises"][0]["status"]),
        (Some(0), &"checked".into()),
        "{out:#}"
    );
    // Said so where its words do not tell.
    let plain =
        promise("concurrent = true").replace("lost in a crash is delivered again", "is applied");
    std::fs::write(dir.join("truefit.toml"), plain).unwrap();
    let (code, _) = truefit_in(&dir, &["promise"]);
    assert_eq!(code, Some(1));
    // One of the two it needs is not enough.
    let sim_only = promise("").replace("check = [\"test\"]", "check = [\"sim\"]");
    std::fs::write(dir.join("truefit.toml"), sim_only).unwrap();
    let (_, out) = truefit_in(&dir, &["promise"]);
    assert_eq!(out["promises"][0]["status"], "unchecked", "{out:#}");
}

#[test]
fn chinese_and_japanese_promises_are_candidates_with_their_rungs_and_stable_hashes() {
    let dir = config("promise-cjk", "[promises]\nsources = [\"SPEC.md\"]\n");
    std::fs::write(
        dir.join("SPEC.md"),
        "# 规格\n\n这是一张照片。索引仅在空闲时执行。\n崩溃重启后，任务不会丢失！\n\n- 結果は必ず保存される。再試行しても一度だけ適用される。\n",
    )
    .unwrap();
    let (code, out) = truefit_in(&dir, &["promise"]);
    assert_eq!(code, Some(1), "{out:#}");
    let found: Vec<(&str, &str, &str)> = out["candidates"]["unmarked"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["source"].as_str().unwrap(),
                c["sentence"].as_str().unwrap(),
                c["rung"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        found,
        vec![
            ("SPEC.md:3", "索引仅在空闲时执行。", "test"),
            ("SPEC.md:4", "崩溃重启后，任务不会丢失！", "sim and model"),
            ("SPEC.md:6", "結果は必ず保存される。", "test"),
            (
                "SPEC.md:6",
                "再試行しても一度だけ適用される。",
                "sim and model"
            ),
        ],
        "{out:#}"
    );
    let (_, again) = truefit_in(&dir, &["promise"]);
    assert_eq!(again["candidates"], out["candidates"]);
    let hash = out["candidates"]["unmarked"][0]["hash"].as_str().unwrap();
    let (_, marked) = truefit_in(&dir, &["promise", "--mark", hash, "not"]);
    assert_eq!(
        marked["candidates"]["unmarked"].as_array().unwrap().len(),
        3
    );
}

#[test]
fn a_candidate_marked_a_promise_fails_until_registered_and_readmes_are_read_by_default() {
    let toml = "[[standin]]\nname = \"git\"\nrun = [\"true\"]\nreal = [\"true\"]\nfaults = [\"gone\"]\n[[standin.case]]\nname = \"c\"\nfault = \"gone\"\n";
    let dir = config("promise-default", toml);
    std::fs::write(dir.join("README.md"), "A ref is never moved backwards.\n").unwrap();
    let (code, out) = truefit_in(&dir, &["promise"]);
    assert_eq!(code, Some(1), "{out:#}");
    let hash = out["candidates"]["unmarked"][0]["hash"]
        .as_str()
        .unwrap()
        .to_string();
    let (code, out) = truefit_in(&dir, &["promise", "--mark", &hash, "promise"]);
    assert_eq!(code, Some(1), "{out:#}");
    assert_eq!(
        out["candidates"]["unregistered"][0]["source"],
        "README.md:1"
    );
    let registered = format!(
        "{toml}[[promise]]\nname = \"no-rewind\"\nsays = \"A ref is never moved backwards.\"\nsource = \"README.md:1\"\ncheck = [\"contract git\"]\n"
    );
    std::fs::write(dir.join("truefit.toml"), registered).unwrap();
    let (code, out) = truefit_in(&dir, &["promise"]);
    assert_eq!(code, Some(0), "{out:#}");
    assert_eq!(out["promises"][0]["status"], "checked");
}

#[test]
fn promise_faults_exit_2() {
    let dir = config(
        "promise-faults",
        "[[promise]]\nname = \"a\"\nsays = \"s\"\nsource = \"f:1\"\ncheck = [\"model nope\", \"contract x\", \"frob\", \"sim now\"]\n\
         [[promise]]\nname = \"a\"\nsays = \"s\"\nsource = \"f:1\"\ncheck = [\"test\"]\nkept = \"k\"\n",
    );
    let (code, out) = truefit_in(&dir, &["promise"]);
    assert_eq!(code, Some(2), "{out:#}");
    assert_eq!(out["error"].as_array().unwrap().len(), 6, "{out:#}");
    let (code, _) = truefit_in(&dir, &["promise", "--frob"]);
    assert_eq!(code, Some(2));
}

#[test]
fn a_file_fixed_twice_is_owed_a_promise_until_a_check_covers_it_or_it_is_marked_not() {
    let dir = sample("history", "fixed-often");
    let (_, out) = truefit_in(&dir, &["next", "--all"]);
    assert!(
        out["notes"][1]
            .as_str()
            .unwrap()
            .contains("files fixed often are not looked for"),
        "{out:#}"
    );
    assert!(
        Command::new("sh")
            .arg(dir.join("history"))
            .status()
            .unwrap()
            .success()
    );
    let (code, out) = truefit_in(&dir, &["next", "--all"]);
    assert_eq!(code, Some(0));
    assert_eq!(
        out["counts"],
        serde_json::json!({"fixed-often": 1}),
        "{out:#}"
    );
    let task = &out["tasks"][0];
    assert_eq!(
        task["summary"],
        "ledger.awk was changed by 2 fixes in 180 days, and no registered promise's check covers it"
    );
    let fixes: Vec<&str> = task["evidence"]["commits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["fix"].as_str().unwrap())
        .collect();
    assert_eq!(fixes[0], "ledger#2", "{task:#}");
    assert_eq!(
        fixes[1].len(),
        40,
        "a fix naming no issue is its own commit: {task:#}"
    );
    let hash = task["evidence"]["hash"].as_str().unwrap().to_string();
    for args in [
        &["not"][..],
        &["not", "--why", " "],
        &["promise", "--why", "x"],
    ] {
        let mut mark = vec!["promise", "--mark", &hash];
        mark.extend(args);
        let (code, out) = truefit_in(&dir, &mark);
        assert_eq!(code, Some(2), "{args:?}: {out:#}");
        let fault = out["error"][0]["fault"].as_str().unwrap();
        assert!(
            fault.contains("marked only `not` with its `--why`"),
            "{fault}"
        );
    }
    let (code, _) = truefit_in(
        &dir,
        &[
            "promise",
            "--mark",
            &hash,
            "not",
            "--why",
            "two slips, no promise",
        ],
    );
    assert_eq!(code, Some(0));
    let marks = std::fs::read_to_string(dir.join(".truefit/promises.json")).unwrap();
    assert!(marks.contains("two slips, no promise"), "{marks}");
    let (_, out) = truefit_in(&dir, &["next"]);
    assert_eq!(out["task"], Value::Null, "{out:#}");
    // A new commit to the file asks again; a promise whose check covers it answers.
    std::fs::write(dir.join("ledger.awk"), "{ charged[$1] += $2 }\n").unwrap();
    let git = |args: &[&str]| {
        let ok = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .current_dir(&dir)
            .status()
            .unwrap();
        assert!(ok.success());
    };
    git(&["commit", "-qam", "Simpler"]);
    let (_, out) = truefit_in(&dir, &["next"]);
    assert_eq!(out["task"]["kind"], "fixed-often", "{out:#}");
    assert_ne!(out["task"]["evidence"]["hash"], hash.as_str());
    let base = std::fs::read_to_string(dir.join("truefit.toml")).unwrap();
    let promise = |check: &str| {
        format!(
            "\n[[promise]]\nname = \"charged-once\"\nsays = \"An order is charged once.\"\nsource = \"README.md:1\"\ncheck = [\"{check}\"]\n"
        )
    };
    for toml in [
        "[sim]\nrun = [\"./sim\"]\ncode = [\"*.awk\"]\n".to_string() + &promise("sim"),
        // A model's `covers` naming the file answers as the simulation's code does.
        "[[model]]\nname = \"ledger\"\nspec = \"L.tla\"\nconfig = \"L.cfg\"\ncommit = \"x\"\ncovers = [{ action = \"Charge\", file = \"ledger.awk\", function = \"main\" }]\n".to_string()
            + &promise("model ledger"),
    ] {
        std::fs::write(dir.join("truefit.toml"), base.clone() + &toml).unwrap();
        let (_, out) = truefit_in(&dir, &["next", "--all"]);
        assert_eq!(out["counts"].get("fixed-often"), None, "{toml}: {out:#}");
    }
}

#[test]
fn a_failing_seed_whose_invariant_no_promise_names_is_owed_one() {
    let dir = sample("sim", "seed-promise");
    let (code, _) = truefit_in(&dir, &["sim", "--seeds", "10"]);
    assert_eq!(code, Some(1));
    let (_, out) = truefit_in(&dir, &["next", "--all"]);
    assert_eq!(
        out["counts"].get("failing-seed-promise"),
        None,
        "applied-once is registered: {out:#}"
    );
    let toml = std::fs::read_to_string(dir.join("truefit.toml")).unwrap();
    std::fs::write(
        dir.join("truefit.toml"),
        toml.replace("name = \"applied-once\"", "name = \"applied-at-most-once\""),
    )
    .unwrap();
    let (_, out) = truefit_in(&dir, &["next", "--all"]);
    assert_eq!(out["counts"]["failing-seed-promise"], 1, "{out:#}");
    let task = out["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["kind"] == "failing-seed-promise")
        .unwrap();
    assert_eq!(task["evidence"]["invariant"], "applied-once");
    assert_eq!(
        task["evidence"]["seeds"].as_array().unwrap().len(),
        kept_seeds(&dir).len()
    );
    let hash = task["evidence"]["hash"].as_str().unwrap();
    let (code, _) = truefit_in(
        &dir,
        &[
            "promise",
            "--mark",
            hash,
            "not",
            "--why",
            "the sample's own check",
        ],
    );
    assert_eq!(
        code,
        Some(1),
        "marked; the renamed promise is still unreached"
    );
    let (_, out) = truefit_in(&dir, &["next", "--all"]);
    assert_eq!(out["counts"].get("failing-seed-promise"), None, "{out:#}");
}

#[test]
fn a_promise_kept_by_a_contract_whose_stand_in_declares_no_fault_is_unreached() {
    let dir = config(
        "promise-no-fault",
        r#"[promises]
sources = []
[[promise]]
name = "saved"
says = "A token issued is never dropped."
source = "SPEC.md:1"
check = ["contract e"]
[[standin]]
name = "e"
run = ["/bin/echo"]
real = ["echo"]
[[standin.case]]
name = "c"
"#,
    );
    let (code, out) = truefit_in(&dir, &["promise"]);
    assert_eq!(code, Some(1), "{out:#}");
    assert_eq!(statuses(&out), [("saved", "unreached")]);
    let why = out["promises"][0]["why"].as_str().unwrap();
    assert!(why.contains("stand-in e: it declares no fault"), "{why}");
    let toml = std::fs::read_to_string(dir.join("truefit.toml")).unwrap();
    let with_fault = toml.replace(
        "real = [\"echo\"]\n",
        "real = [\"echo\"]\nfaults = [\"gone\"]\n",
    ) + "fault = \"gone\"\n";
    std::fs::write(dir.join("truefit.toml"), with_fault).unwrap();
    let (code, out) = truefit_in(&dir, &["promise"]);
    assert_eq!(statuses(&out), [("saved", "checked")], "{out:#}");
    assert_eq!(code, Some(0), "{out:#}");
}

/// truefit in `dir` with PATH only `bin` and the system's, where a fake
/// `factorium` may stand.
fn truefit_with_path(dir: &Path, bin: &Path, args: &[&str]) -> (Option<i32>, Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_truefit"))
        .args(args)
        .current_dir(dir)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();
    (
        out.status.code(),
        serde_json::from_slice(&out.stdout).unwrap_or(Value::Null),
    )
}

/// One promise, `kept` with its reason, and `end`.
fn kept(end: &str) -> String {
    format!(
        "[promises]\nsources = []\n[[promise]]\nname = \"offline\"\nsays = \"No photo is ever uploaded.\"\nsource = \"SPEC.md:5\"\nkept = \"not built yet: phase 3 adds the test\"\n{end}"
    )
}

fn why(out: &Value) -> &str {
    out["promises"][0]["why"].as_str().unwrap_or("")
}

#[test]
fn a_kept_promise_whose_date_passes_is_unchecked_and_next_asks_for_its_check() {
    let dir = config("kept-until", &kept("kept_until = \"2999-01-01\"\n"));
    let (code, out) = truefit_in(&dir, &["promise"]);
    assert_eq!(code, Some(0), "{out:#}");
    assert_eq!(out["promises"][0]["status"], "kept");
    assert_eq!(out["promises"][0]["until"], "2999-01-01", "{out:#}");
    // Kept on its day, unchecked the day after.
    let today = utc_day(0);
    std::fs::write(
        dir.join("truefit.toml"),
        kept(&format!("kept_until = \"{today}\"\n")),
    )
    .unwrap();
    let (code, out) = truefit_in(&dir, &["promise"]);
    assert_eq!(code, Some(0), "{out:#}");
    std::fs::write(
        dir.join("truefit.toml"),
        kept("kept_until = \"2000-01-01\"\n"),
    )
    .unwrap();
    let (code, out) = truefit_in(&dir, &["promise"]);
    assert_eq!(code, Some(1), "{out:#}");
    assert_eq!(out["promises"][0]["status"], "unchecked");
    assert!(
        why(&out).contains("kept until 2000-01-01, which has passed"),
        "{out:#}"
    );
    assert!(
        why(&out).contains("not built yet: phase 3 adds the test"),
        "{out:#}"
    );
    let (_, next) = truefit_in(&dir, &["next"]);
    assert_eq!(next["task"]["kind"], "unchecked-promise", "{next:#}");
    assert!(
        next["task"]["summary"]
            .as_str()
            .unwrap()
            .starts_with("give offline its check: "),
        "{next:#}"
    );
}

#[test]
fn a_kept_promise_is_unchecked_once_a_path_it_waits_for_appears() {
    let dir = config(
        "kept-absent",
        &kept("kept_while_absent = [\"App/**/*.swift\"]\n"),
    );
    std::fs::create_dir_all(dir.join("App/RollMind/People")).unwrap();
    std::fs::write(dir.join("App/README.md"), "").unwrap();
    std::fs::create_dir_all(dir.join("App/.build")).unwrap();
    std::fs::write(dir.join("App/.build/Gen.swift"), "").unwrap();
    let (code, out) = truefit_in(&dir, &["promise"]);
    assert_eq!(code, Some(0), "{out:#}");
    assert_eq!(out["promises"][0]["status"], "kept");
    std::fs::write(dir.join("App/RollMind/People/Album.swift"), "").unwrap();
    let (code, out) = truefit_in(&dir, &["promise"]);
    assert_eq!(code, Some(1), "{out:#}");
    assert_eq!(out["promises"][0]["status"], "unchecked");
    assert!(
        why(&out).contains("App/RollMind/People/Album.swift exists (`App/**/*.swift`)"),
        "{out:#}"
    );
    // `**` is also no directory at all.
    std::fs::remove_dir_all(dir.join("App/RollMind")).unwrap();
    let (code, _) = truefit_in(&dir, &["promise"]);
    assert_eq!(code, Some(0));
    std::fs::write(dir.join("App/Main.swift"), "").unwrap();
    let (code, _) = truefit_in(&dir, &["promise"]);
    assert_eq!(code, Some(1));
}

#[test]
fn a_kept_promise_is_unchecked_once_its_issue_closes_and_kept_while_it_cannot_be_read() {
    let dir = config("kept-issue", &kept("kept_until_issue = \"rollmind#80\"\n"));
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let (code, out) = truefit_with_path(&dir, &bin, &["promise"]);
    assert_eq!(code, Some(0), "{out:#}");
    assert!(
        out["promises"][0]["note"]
            .as_str()
            .unwrap_or("")
            .contains("whether rollmind#80 is closed is not read"),
        "{out:#}"
    );
    let fake = bin.join("factorium");
    let state = dir.join("state");
    std::fs::write(&state, "ready").unwrap();
    std::fs::write(
        &fake,
        format!(
            "#!/bin/sh\n[ \"$1 $2 $3 $4\" = \"issue show rollmind 80\" ] || exit 3\n[ \"$(cat {0})\" = gone ] && exit 1\nprintf '{{\"checkout\":\"rollmind\",\"number\":80,\"status\":\"%s\"}}\\n' \"$(cat {0})\"\n",
            state.display()
        ),
    )
    .unwrap();
    Command::new("chmod").arg("+x").arg(&fake).status().unwrap();
    let (code, out) = truefit_with_path(&dir, &bin, &["promise"]);
    assert_eq!(code, Some(0), "{out:#}");
    assert_eq!(out["promises"][0]["status"], "kept");
    // One it cannot read stays kept, and says so.
    std::fs::write(&state, "gone").unwrap();
    let (code, out) = truefit_with_path(&dir, &bin, &["promise"]);
    assert_eq!(code, Some(0), "{out:#}");
    assert!(
        out["promises"][0]["note"]
            .as_str()
            .unwrap_or("")
            .contains("answered no status: whether rollmind#80 is closed is not read"),
        "{out:#}"
    );
    std::fs::write(&state, "closed").unwrap();
    let (code, out) = truefit_with_path(&dir, &bin, &["promise"]);
    assert_eq!(code, Some(1), "{out:#}");
    assert_eq!(out["promises"][0]["status"], "unchecked");
    assert!(why(&out).contains("rollmind#80 is closed"), "{out:#}");
}

/// The UTC date `ago` days before today (after it, when negative), as YYYY-MM-DD.
fn utc_day(ago: i32) -> String {
    let out = Command::new("date")
        .args(["-u", &format!("-v{:+}d", -ago), "+%F"])
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// Makes `dir` a git repo whose one commit, of all its files, is dated `date` (YYYY-MM-DD).
fn commit_on(dir: &Path, date: &str) {
    let at = format!("{date}T12:00:00Z");
    for args in [
        &["init", "-q"][..],
        &["add", "."],
        &["commit", "-qm", "promises"],
    ] {
        let ok = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_DATE", &at)
            .env("GIT_COMMITTER_DATE", &at)
            .status()
            .unwrap();
        assert!(ok.success());
    }
}

#[test]
fn a_bare_kept_is_warned_for_14_days_after_its_line_changed_then_unchecked() {
    // Changed today: kept, warned, with the end to give it.
    let dir = config("kept-bare-new", &kept(""));
    let _ = std::fs::remove_dir_all(dir.join(".git"));
    commit_on(&dir, &utc_day(0));
    let (code, out) = truefit_in(&dir, &["promise"]);
    assert_eq!(code, Some(0), "{out:#}");
    let p = &out["promises"][0];
    assert_eq!(p["status"], "kept", "{out:#}");
    assert!(
        p["warning"].as_str().unwrap_or("").contains("no end"),
        "{out:#}"
    );
    assert!(
        p["suggest"]
            .as_str()
            .unwrap_or("")
            .starts_with("kept_until = \""),
        "{out:#}"
    );
    assert!(
        out["next"].as_str().unwrap().contains("`suggest`"),
        "{out:#}"
    );

    // Changed long ago: past the grace, unchecked, still with its suggestion.
    let dir = config("kept-bare-old", &kept(""));
    let _ = std::fs::remove_dir_all(dir.join(".git"));
    commit_on(&dir, "2000-01-01");
    let (code, out) = truefit_in(&dir, &["promise"]);
    assert_eq!(code, Some(1), "{out:#}");
    let p = &out["promises"][0];
    assert_eq!(p["status"], "unchecked", "{out:#}");
    assert!(why(&out).contains("last changed on 2000-01-01"), "{out:#}");
    assert!(
        why(&out).contains("not built yet: phase 3 adds the test"),
        "{out:#}"
    );
    // Past its grace, the end it suggests is counted from today.
    assert_eq!(
        p["suggest"],
        format!("kept_until = \"{}\"", utc_day(-14)),
        "{out:#}"
    );

    // The grace's last day is kept; the day after is not.
    for (ago, code) in [(14, Some(0)), (15, Some(1))] {
        let dir = config(&format!("kept-bare-{ago}"), &kept(""));
        let _ = std::fs::remove_dir_all(dir.join(".git"));
        commit_on(&dir, &utc_day(ago));
        let (got, out) = truefit_in(&dir, &["promise"]);
        assert_eq!(got, code, "{ago} days ago: {out:#}");
    }

    // Kept forever, with its reason: no end is asked.
    std::fs::write(dir.join("truefit.toml"), kept("kept_forever = true\n")).unwrap();
    let (code, out) = truefit_in(&dir, &["promise"]);
    assert_eq!(code, Some(0), "{out:#}");
    assert!(out["promises"][0]["warning"].is_null(), "{out:#}");

    // No history dates its line: there is no grace to count.
    let dir = config("kept-bare-nogit", &kept(""));
    let (code, out) = truefit_in(&dir, &["promise"]);
    assert_eq!(code, Some(1), "{out:#}");
    assert!(why(&out).contains("no git history dates"), "{out:#}");
}

#[test]
fn a_kept_end_is_declared_rightly_or_exits_2() {
    let promise =
        |lines: &str| format!("[[promise]]\nname = \"p\"\nsays = \"s\"\nsource = \"f:1\"\n{lines}");
    for (bad, said) in [
        (
            "kept_until = \"2026-12-01\"\ncheck = [\"test\"]\n",
            "an end of `kept`",
        ),
        ("kept = \"k\"\nkept_until = \"soon\"\n", "is no date"),
        ("kept = \"k\"\nkept_until_issue = \"80\"\n", "is no issue"),
        (
            "kept = \"k\"\nkept_until_issue = \"rollmind#x\"\n",
            "is no issue",
        ),
        ("kept_while_absent = [\"App\"]\n", "an end of `kept`"),
        ("kept_until_issue = \"rollmind#80\"\n", "an end of `kept`"),
        (
            "kept = \"k\"\nkept_forever = true\nkept_while_absent = [\"App\"]\n",
            "both",
        ),
        (
            "kept = \"k\"\nkept_while_absent = [\"\"]\n",
            "an empty path",
        ),
        (
            "kept = \"k\"\nkept_forever = true\nkept_until = \"2999-01-01\"\n",
            "both",
        ),
        (
            "kept = \" \"\nkept_forever = true\n",
            "why no check can exist",
        ),
    ] {
        let dir = config("kept-faults", &promise(bad));
        let (code, out) = truefit_in(&dir, &["promise"]);
        assert_eq!(code, Some(2), "{bad}: {out:#}");
        assert!(
            out["error"][0]["fault"]
                .as_str()
                .unwrap_or("")
                .contains(said),
            "{bad}: {out:#}"
        );
    }
}
