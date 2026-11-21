use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn truefit_in(dir: &Path, args: &[&str]) -> (Option<i32>, Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_truefit"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    let json = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    (out.status.code(), json)
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
    let _ = std::fs::remove_dir_all(dir.join(".truefit"));
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

#[test]
fn the_sample_stand_ins_run_every_case() {
    let (code, out) = truefit_in(&sample("plutil", "plain"), &["contract"]);
    assert_eq!(code, Some(0), "{out:#}");
    assert_eq!(out["standins"][0]["cases"].as_array().unwrap().len(), 4);
    assert_eq!(out["standins"][1]["result"], "pass");
    assert!(out["next"].as_str().unwrap().contains("--real"));
}

#[test]
fn a_true_stand_in_agrees_with_plutil_and_its_real_run_is_recorded() {
    let dir = sample("plutil", "real");
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
    assert!(record["version"].is_string());
    let (_, plain) = truefit_in(&dir, &["contract"]);
    assert_eq!(plain["standins"][0]["real_run"], record);
}

#[test]
fn a_wrong_stand_in_differs_and_both_answers_are_printed() {
    let dir = sample("plutil", "wrong");
    let (code, out) = truefit_in(&dir, &["contract", "--real", "plutil-wrong"]);
    assert_eq!(code, Some(1), "{out:#}");
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
"#,
    );
    let (code, out) = truefit_in(&dir, &["contract"]);
    assert_eq!(code, Some(2), "{out:#}");
    let faults = out["error"].as_array().unwrap();
    assert_eq!(faults.len(), 6, "{out:#}");
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
}

#[test]
fn by_default_a_contract_compares_exit_and_stdout() {
    let dir = config(
        "default-compare",
        "[[standin]]\nname = \"e\"\nrun = [\"/bin/echo\", \"a\"]\nreal = [\"echo\"]\n[[standin.case]]\nname = \"c\"\nargs = [\"b\"]\n",
    );
    let (code, out) = truefit_in(&dir, &["contract", "--real"]);
    assert_eq!(code, Some(1), "{out:#}");
    assert_eq!(
        out["standins"][0]["cases"][0]["differ"],
        serde_json::json!(["stdout"])
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
        serde_json::json!({"no-contract": 1, "stale-contract": 1, "differing-case": 1})
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
    let (code, out) = truefit_in(&dir, &["next", "--all"]);
    assert_eq!(code, Some(0), "{out:#}");
    assert_eq!(out["tasks"], serde_json::json!([]), "{out:#}");
    assert_eq!(out["notes"], serde_json::json!([]));
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
