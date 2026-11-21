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
    assert_eq!(next["counts"], serde_json::json!({}), "{next:#}");
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
        next["counts"],
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
        next["counts"],
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
    assert_eq!(
        out["new"],
        serde_json::json!({"ran": 3, "passed": 3, "failing": []})
    );
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
