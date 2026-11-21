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
    assert_eq!(record["date"].as_str().unwrap().len(), 10);
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
    assert!(out["next"].as_str().unwrap().contains("plutil-wrong"));
}

#[test]
fn a_real_program_missing_from_path_is_not_run() {
    let (code, out) = truefit_in(&sample("missing", "missing"), &["contract", "--real"]);
    assert_eq!(code, Some(1), "{out:#}");
    let s = &out["standins"][0];
    assert_eq!(s["result"], "not run");
    assert_eq!(s["needs"], "truefit-no-such-program 1.x on PATH, logged in");
    assert!(s.get("cases").is_none());
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
        "[[standin]]\nname = \"e\"\nrun = [\"/bin/echo\"]\nreal = [\"echo\"]\n[[standin.case]]\nname = \"c\"\nargs = [\"hi\"]\n",
    );
    std::fs::write(dir.join(".truefit"), "a file, not a directory").unwrap();
    let (code, out) = truefit_in(&dir, &["contract", "--real"]);
    assert_eq!(code, Some(1), "{out:#}");
    assert_eq!(out["standins"][0]["result"], "fail");
    assert!(
        out["standins"][0]["fault"]
            .as_str()
            .unwrap()
            .contains(".truefit/e.json")
    );
}
