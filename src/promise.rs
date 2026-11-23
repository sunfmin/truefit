//! `truefit promise`: each promise the system makes, tied to the check that
//! keeps it. A promise a named check never reached, or none checks, fails;
//! sentences in the system's docs that read as promises are candidates, marked
//! by the agent until each is registered or marked `not`.

use crate::contract::{self, CONFIG, Config, fault};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;

/// Where candidates' marks are kept, by the sentence's hash: committed.
pub const MARKS: &str = ".truefit/promises.json";

/// The checks a promise may name, from the cheapest.
const LADDER: [&str; 4] = ["test", "contract", "sim", "model"];

/// The words a sentence that makes a promise reads with.
const WORDS: &str = "never, always, at most, exactly once, only, within, every";

/// The words of a promise about what runs at once or in an order the system
/// does not choose: one that needs both `sim` and `model`.
const CONCURRENT: &str = "at once, concurrent, concurrently, in parallel, simultaneous, race, out of order, reorder, crash, crashes, restart, retry, retries, lease, queue, lost, twice, deliver, delivered, redelivered, exactly once, at least once";

/// `[promises]`: the files whose sentences are candidates, and how many
/// days of history the fixes are read from.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Sources {
    sources: Vec<String>,
    pub history_days: u64,
}

impl Default for Sources {
    fn default() -> Self {
        Sources {
            sources: ["SPEC.md", "README.md", "docs/adr/*.md"]
                .map(String::from)
                .into(),
            history_days: 180,
        }
    }
}

/// `[[promise]]`: one promise, the sentence that makes it, and its checks.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Promise {
    pub name: String,
    says: String,
    source: String,
    #[serde(default)]
    check: Vec<String>,
    #[serde(default)]
    pub kept: Option<toml::Spanned<String>>,
    #[serde(default)]
    pub kept_until: Option<String>,
    #[serde(default)]
    pub kept_while_absent: Vec<String>,
    #[serde(default)]
    pub kept_until_issue: Option<String>,
    #[serde(default)]
    pub kept_forever: bool,
    #[serde(default)]
    why: Option<String>,
    #[serde(default)]
    concurrent: bool,
}

impl Promise {
    pub fn checks(&self) -> Vec<(&str, &str)> {
        let mut c: Vec<(&str, &str)> = self
            .check
            .iter()
            .map(|c| c.split_once(' ').unwrap_or((c, "")))
            .collect();
        c.sort_by_key(|(rung, _)| LADDER.iter().position(|l| l == rung));
        c
    }

    fn concurrent(&self) -> bool {
        self.concurrent || has_any(&self.says, CONCURRENT)
    }
}

/// What is wrong in the `[[promise]]` declarations.
pub fn faults(config: &Config) -> Vec<Value> {
    let mut faults = Vec::new();
    for (i, p) in config.promise.iter().enumerate() {
        let n = &p.name;
        if config.promise[..i].iter().any(|q| &q.name == n) {
            faults.push(fault(
                format!("promise `{n}` is declared twice"),
                "give each [[promise]] its own name: its invariant's",
            ));
        }
        if p.kept.is_some() && !p.check.is_empty() {
            faults.push(fault(
                format!("promise `{n}` is both checked and kept"),
                "give it a `check`, or `kept = \"<why it is not checked>\"`, not both",
            ));
        }
        faults.extend(crate::kept::faults(p));
        for (rung, arg) in p.checks() {
            let known = match rung {
                "test" | "sim" => arg.is_empty(),
                "contract" => config.standin.iter().any(|s| s.name == arg),
                "model" => config.model.iter().any(|m| m.name == arg),
                _ => false,
            };
            if !known {
                faults.push(fault(
                    format!("promise `{n}`: check `{rung} {arg}` names no check here"),
                    "name `test`, `contract <a declared stand-in>`, `sim` or `model <a declared model>`",
                ));
            }
        }
    }
    faults
}

/// `truefit promise [--mark <hash> <promise|not> ... [--why <reason>]]`.
pub fn run(root: &Path, marks: &[&str], why: Option<&str>) -> Result<(Value, bool), Vec<Value>> {
    let config = contract::load(root)?;
    let found = sentences(root, &config);
    if !marks.is_empty() {
        let owed = crate::history::found(root, &config).0;
        mark(root, &found, &owed, marks, why)?;
    }
    let promises: Vec<Value> = config
        .promise
        .iter()
        .map(|p| status(root, &config, p).0)
        .collect();
    let (unmarked, unregistered) = candidates(root, &config, &found);
    let failing = |s: &str| promises.iter().filter(|p| p["status"] == s).count();
    let mut next = Vec::new();
    if failing("unreached") > 0 {
        next.push("make each `unreached` promise's named check reach it: a simulation step that checks it by name in a state it is about, a model whose config checks an invariant of its name");
    }
    if failing("unbacked") > 0 {
        next.push("back each `unbacked` promise's model (`why`): commit a run it allows under .truefit/traces/<model>/accept/ and one that breaks the promise under reject/, then `truefit model <model>`");
    }
    if failing("unchecked") > 0 {
        next.push("give each `unchecked` promise its checks from the ladder (`truefit --help`), or the reason it needs none; one whose `kept` ended, or has no end past its grace, needs its check now (`why` says which), or an end that still holds");
    }
    if !unmarked.is_empty() || !unregistered.is_empty() {
        next.push("mark each `unmarked` candidate: `truefit promise --mark <hash> promise|not`; register each marked promise as a [[promise]]");
    }
    let ok = next.is_empty();
    if promises
        .iter()
        .any(|p| p["status"] == "kept" && p["warning"].is_string())
    {
        next.push("give each `kept` promise with no end (`warning`) the end its reason names, before its grace runs out, as its `warning` and `suggest` say");
    }
    if next.is_empty() {
        next.push("nothing: every promise is checked and reached, or kept with its reason and its end; commit .truefit/");
    }
    let report = json!({
        "promises": promises,
        "candidates": {"found": found.len(), "unmarked": unmarked, "unregistered": unregistered},
        "next": next.join("; "),
    });
    Ok((report, ok))
}

/// One promise's status: `kept`, `unchecked`, `unreached`, `unbacked` (a
/// model reaches it, but its last check lacks a passing TLC run, an accept
/// trace or a reject trace that breaks it) or `checked`; and, when
/// `unbacked`, each model that does not back it, with why.
fn status<'p>(root: &Path, config: &Config, p: &'p Promise) -> (Value, Vec<(&'p str, String)>) {
    let checks = p.checks();
    let has = |rung: &str| checks.iter().any(|(r, _)| *r == rung);
    let mut report = json!({"name": p.name, "says": p.says, "source": p.source, "check": p.check});
    let mut models = Vec::new();
    let (status, why) = if let Some(kept) = &p.kept {
        let (status, why, extra) = crate::kept::judge(root, p, kept.get_ref());
        for (k, v) in extra {
            report[k] = v;
        }
        (status, why)
    } else if p.check.is_empty() {
        ("unchecked", "no check keeps it, and it is not kept".into())
    } else if p.concurrent()
        && !(has("sim") && has("model"))
        && p.why.as_deref().is_none_or(|w| w.trim().is_empty())
    {
        ("unchecked", "it is about what runs at once or in an order the system does not choose: it needs both `sim` and `model`, or a `why` for a cheaper check".into())
    } else {
        let unreached: Vec<String> = checks
            .iter()
            .filter_map(|&(rung, arg)| match rung {
                "sim" => crate::sim::unreached(root, &p.name, config.contract.fresh_days),
                "model" => crate::model::unreached(root, &config.model, arg, &p.name),
                // A failure the promise is about cannot be reached through a stand-in that only succeeds.
                "contract" => config
                    .standin
                    .iter()
                    .find(|s| s.name == arg)
                    .and_then(|s| contract::without_faults(root, s))
                    .map(|(why, _)| format!("`contract {arg}`: stand-in {arg}: {why}")),
                _ => None,
            })
            .collect();
        let unbacked: Vec<(&str, String)> = checks
            .iter()
            .filter(|(rung, _)| *rung == "model")
            .filter_map(|&(_, m)| {
                Some((m, crate::model::unbacked(root, &config.model, m, &p.name)?))
            })
            .collect();
        match (unreached.is_empty(), unbacked.is_empty()) {
            (false, _) => ("unreached", unreached.join("; ")),
            (true, false) => {
                let why = unbacked
                    .iter()
                    .map(|(_, w)| w.as_str())
                    .collect::<Vec<_>>()
                    .join("; ");
                models = unbacked;
                ("unbacked", why)
            }
            (true, true) => ("checked", String::new()),
        }
    };
    report["status"] = status.into();
    if !why.is_empty() {
        report["why"] = why.into();
    }
    (report, models)
}

/// Whether two names are one invariant's: alike but for case and punctuation,
/// so `applied-once` is `AppliedOnce`.
pub fn same_name(a: &str, b: &str) -> bool {
    let key = |s: &str| -> String {
        s.chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_lowercase())
            .collect()
    };
    key(a) == key(b)
}

/// Of the candidates `found`, those no mark and no registered promise
/// answers, and those marked `promise` that none registers.
fn candidates(root: &Path, config: &Config, found: &[Value]) -> (Vec<Value>, Vec<Value>) {
    let marks = contract::read(root, MARKS);
    let says: Vec<String> = config.promise.iter().map(|p| plain(&p.says)).collect();
    let (mut unmarked, mut unregistered) = (Vec::new(), Vec::new());
    for c in found {
        let text = plain(c["sentence"].as_str().unwrap_or(""));
        if says
            .iter()
            .any(|s| !s.is_empty() && text.contains(s.as_str()))
        {
            continue;
        }
        match marks[c["hash"].as_str().unwrap_or("")]["mark"].as_str() {
            Some("not") => {}
            Some(_) => unregistered.push(c.clone()),
            None => unmarked.push(c.clone()),
        }
    }
    (unmarked, unregistered)
}

/// Every sentence in `[promises] sources` with a promise word, as a candidate.
fn sentences(root: &Path, config: &Config) -> Vec<Value> {
    let mut found = Vec::new();
    for glob in &config.promises.sources {
        for file in crate::next::expand(root, glob) {
            let text = std::fs::read_to_string(root.join(&file)).unwrap_or_default();
            for (line, s) in split(&text) {
                if has_any(&s, WORDS) {
                    let rung = if has_any(&s, CONCURRENT) {
                        "sim and model"
                    } else {
                        "test"
                    };
                    found.push(json!({"source": format!("{file}:{line}"), "sentence": s, "hash": hash(&s), "rung": rung}));
                }
            }
        }
    }
    found
}

/// A Markdown text's sentences, each with the line it starts on: paragraphs,
/// list items and table cells split at `.`, `?` or `!` before a space; code
/// blocks and headings are none.
fn split(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut block: Vec<(usize, &str)> = Vec::new();
    let mut fenced = false;
    for (i, line) in text.lines().enumerate() {
        let t = line.trim();
        let item = t.starts_with(['-', '*', '+'])
            || t.split_once(". ")
                .is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        if t.starts_with("```") || fenced || t.is_empty() || t.starts_with(['#', '|']) || item {
            sentences_of(&block, &mut out);
            block.clear();
        }
        if t.starts_with("```") {
            fenced = !fenced;
        } else if t.starts_with('|') {
            for cell in t.split('|') {
                sentences_of(&[(i + 1, cell.trim())], &mut out);
            }
        } else if !fenced && !t.is_empty() && !t.starts_with('#') {
            let t = match item {
                true => t.trim_start_matches(|c: char| "-*+.".contains(c) || c.is_ascii_digit()),
                false => t,
            };
            block.push((i + 1, t.trim()));
        }
    }
    sentences_of(&block, &mut out);
    out
}

fn sentences_of(block: &[(usize, &str)], out: &mut Vec<(usize, String)>) {
    let mut s = String::new();
    let mut start = 0;
    for (line, text) in block {
        for word in text.split_whitespace() {
            if s.is_empty() {
                start = *line;
            } else {
                s.push(' ');
            }
            s.push_str(word);
            let body = word.trim_end_matches(['.', '?', '!', ')', '"', '*', '`']);
            if body.len() < word.len()
                && word[body.len()..].starts_with(['.', '?', '!'])
                && !body.contains('.')
            {
                out.push((start, std::mem::take(&mut s)));
            }
        }
    }
    if !s.is_empty() {
        out.push((start, s));
    }
}

/// Whether `text` has any of `words` (`, `-separated), each as whole words, in any case.
pub fn has_any(text: &str, words: &str) -> bool {
    let words_of = |s: &str| {
        let words: Vec<String> = s
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(String::from)
            .collect();
        format!(" {} ", words.join(" "))
    };
    let text = words_of(text);
    words.split(", ").any(|w| text.contains(&words_of(w)))
}

/// A sentence as compared: its words, without Markdown's `*` and backticks.
fn plain(s: &str) -> String {
    s.replace(['*', '`'], "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn hash(sentence: &str) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(plain(sentence));
    digest[..6].iter().map(|b| format!("{b:02x}")).collect()
}

/// Records each `<hash> <promise|not>` pair given to `--mark`: of a
/// candidate, or (`not`, with `why`) of a task the history owes.
fn mark(
    root: &Path,
    found: &[Value],
    owed: &[Value],
    pairs: &[&str],
    why: Option<&str>,
) -> Result<(), Vec<Value>> {
    let mut marks = contract::read(root, MARKS);
    if !marks.is_object() {
        marks = json!({});
    }
    let mut faults = Vec::new();
    for pair in pairs.chunks(2) {
        let (hash, mark) = (pair[0], pair.get(1).copied().unwrap_or(""));
        let candidate = found.iter().find(|c| c["hash"] == hash);
        let task = owed.iter().find(|t| t["evidence"]["hash"] == hash);
        match (candidate, task, mark, why) {
            (Some(c), _, "promise" | "not", _) => {
                marks[hash] =
                    json!({"mark": mark, "sentence": c["sentence"], "source": c["source"]});
                if let Some(why) = why {
                    marks[hash]["why"] = why.into();
                }
            }
            (_, Some(t), "not", Some(why)) if !why.trim().is_empty() => {
                marks[hash] = json!({"mark": mark, "why": why, "kind": t["kind"], "summary": t["summary"]});
            }
            (_, Some(t), _, _) => faults.push(fault(
                format!("`{hash}` is a {} task, marked only `not` with its `--why`", t["kind"].as_str().unwrap_or("")),
                "do the task's steps (register the promise), or mark it `not` with `--why \"<reason>\"`",
            )),
            (None, None, _, _) => faults.push(fault(
                format!("`{hash}` is no candidate's hash, nor a task's"),
                "mark a hash `truefit promise` or `truefit next` lists: the sentence or the history may have changed since",
            )),
            _ => faults.push(fault(
                format!("`{mark}` is no mark"),
                "mark each candidate `promise` or `not`",
            )),
        }
    }
    if !faults.is_empty() {
        return Err(faults);
    }
    contract::save(root, MARKS, &marks)
}

/// The `unregistered-promise`, `unchecked-promise`, `unreached-promise` and
/// `unbacked-model` tasks.
pub fn tasks(root: &Path, config: &Config) -> Vec<Value> {
    let mut tasks = Vec::new();
    let (unmarked, unregistered) = candidates(root, config, &sentences(root, config));
    if !unmarked.is_empty() || !unregistered.is_empty() {
        tasks.push(json!({
            "kind": "unregistered-promise",
            "summary": format!("{} candidate promises unmarked, {} marked and not registered", unmarked.len(), unregistered.len()),
            "files": [CONFIG, MARKS],
            "evidence": {"unmarked": unmarked, "unregistered": unregistered, "sources": config.promises.sources},
            "steps": format!("For each `unmarked` candidate, read its sentence where it stands (`source`) and mark it: `promise` when it is a promise the system makes about its own behaviour that a run could break (what it never, always, at most, only, within, exactly once does), `not` when it is not (a description, a promise of another system, advice to the reader). Mark them with `truefit promise --mark <hash> promise|not [<hash> <mark>]...`. For each marked `promise` (and each `unregistered`), declare a [[promise]] in {CONFIG}: `name` its invariant as the code, the simulation and the model name it, `says` the sentence (or its part that is the promise) quoted, `source` its file and line, and `check` from the cheapest rung that keeps it (`rung`: one about what runs at once or in an order the system does not choose needs `sim` and `model`); or `kept = \"<why it is not checked>\"`. Commit {CONFIG} and {MARKS}."),
            "accept": "truefit promise",
        }));
    }
    // Each unbacked model, with each promise it does not back and why.
    let mut unbacked: std::collections::BTreeMap<&str, Vec<Value>> = Default::default();
    for p in &config.promise {
        let (s, models) = status(root, config, p);
        for (m, why) in models {
            unbacked
                .entry(m)
                .or_default()
                .push(json!({"promise": p.name, "why": why}));
        }
        let kind = match s["status"].as_str() {
            Some("unchecked") => "unchecked-promise",
            Some("unreached") => "unreached-promise",
            _ => continue,
        };
        let steps = if kind == "unchecked-promise" {
            format!(
                "Give promise `{}` its checks in {CONFIG}, from the cheapest rung that keeps it: `test`, `contract <stand-in>`, `sim`, `model <model>`. One about what runs at once or in an order the system does not choose (`truefit --help`, what to model) needs `sim` and `model`: write the simulation's check and the model's invariant by its name; a cheaper check needs its reason in `why`. One no check can keep yet is `kept = \"<why>\"` with the end its reason names (`kept_until`, `kept_while_absent`, `kept_until_issue`), or `kept_forever = true` when no check can ever exist. One whose `kept` ended (its `why` says how) needs its check now; give it a new end only when its reason still holds.",
                p.name
            )
        } else {
            format!(
                "Make each check `why` names reach promise `{}`: the simulation checks it by that name in a step's `checked` (in a state it is about; run `truefit sim`), the model's config checks an invariant of that name. Then run the accept command.",
                p.name
            )
        };
        tasks.push(json!({
            "kind": kind,
            "summary": match kind {
                "unchecked-promise" => format!("give {} its check: {}", p.name, s["why"].as_str().unwrap_or("")),
                _ => format!("promise {}: {}", p.name, s["why"].as_str().unwrap_or("")),
            },
            "files": [CONFIG, p.source.split(':').next().unwrap_or("")],
            "evidence": s,
            "steps": steps,
            "accept": "truefit promise",
        }));
    }
    for (m, promises) in unbacked {
        let traces = format!("{}/{m}", crate::model::TRACES);
        tasks.push(json!({
            "kind": "unbacked-model",
            "summary": format!("model {m} does not back {}", promises.iter().filter_map(|p| p["promise"].as_str()).collect::<Vec<_>>().join(", ")),
            "files": [CONFIG, traces],
            "evidence": {"model": m, "promises": promises},
            "steps": format!("A model checks a promise only with TLC passing, a real run it allows and a run that breaks the promise it rejects. Take a real run's records, or a passing seed's (`truefit sim`), check them with `truefit model {m} --trace <file>` until `allowed`, and commit the file under {traces}/accept/. Take a run that breaks the promise (a failing seed's lines, `.truefit/seeds/<seed>.json`, or a run written by hand that does the wrong thing), see it `disallowed`, and commit it under {traces}/reject/. Run `truefit model {m}`: its reject trace must break the promise's invariant. A trace the model answers wrongly is a model too strict or too permissive, or a property too weak: fix the model or the code, never weaken the property. Commit .truefit/."),
            "accept": "truefit promise",
        }));
    }
    tasks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_finds_sentences_with_their_lines_but_not_code_or_headings() {
        let text = "# Never a heading\n\nOne line. And it\nwraps, e.g. here! Done\n- an item\n```\nnever code\n```\n| never | a cell. Two |\n";
        assert_eq!(
            split(text),
            vec![
                (3, "One line.".into()),
                (3, "And it wraps, e.g. here!".into()),
                (4, "Done".into()),
                (5, "an item".into()),
                (9, "never".into()),
                (9, "a cell.".into()),
                (9, "Two".into()),
            ]
        );
    }

    #[test]
    fn words_match_whole_and_in_any_case() {
        assert!(has_any("It is NEVER lost.", WORDS));
        assert!(has_any("applied exactly-once", "exactly once"));
        assert!(!has_any("Nevermore, everyone.", WORDS));
        assert!(same_name("applied-once", "AppliedOnce"));
        assert!(!same_name("applied-once", "AppliedTwice"));
    }
}
