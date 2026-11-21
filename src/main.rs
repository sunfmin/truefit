//! truefit: keeps a system's tests and models true to the real system.
//! `truefit --help` is the manual, and its only copy.

mod contract;
mod history;
mod model;
mod next;
mod promise;
mod sim;

use serde_json::json;
use std::process::ExitCode;

// The version build.rs stamps; its tests run here.
#[cfg(test)]
mod version;

const MANUAL: &str = include_str!("manual.md");

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    if let ["contract", rest @ ..] = args.as_slice() {
        return contract(rest);
    }
    if let ["sim", rest @ ..] = args.as_slice() {
        return sim(rest);
    }
    if let ["model", rest @ ..] = args.as_slice() {
        return model(rest);
    }
    if let ["promise", rest @ ..] = args.as_slice() {
        let (rest, why) = match rest {
            [r @ .., "--why", w] if !r.is_empty() => (r, Some(*w)),
            r => (r, None),
        };
        let marks = match rest {
            [] => Ok(&[][..]),
            ["--mark", pairs @ ..] if !pairs.is_empty() && pairs.len() % 2 == 0 => Ok(pairs),
            _ => Err(vec![contract::fault(
                format!(
                    "`truefit promise {}` is not a promise command",
                    rest.join(" ")
                ),
                "run `truefit promise` or `truefit promise --mark <hash> <promise|not> [<hash> <mark>]... [--why <reason>]`",
            )]),
        };
        let root = std::env::current_dir().unwrap_or_default();
        return print(marks.and_then(|m| promise::run(&root, m, why)));
    }
    if let ["next", rest @ ..] = args.as_slice() {
        let all = match rest {
            [] => Ok(false),
            ["--all"] => Ok(true),
            _ => Err(vec![contract::fault(
                format!("`truefit next {}` is not a next command", rest.join(" ")),
                "run `truefit next` or `truefit next --all`",
            )]),
        };
        let root = std::env::current_dir().unwrap_or_default();
        return print(all.and_then(|all| next::run(&root, all).map(|a| (a, true))));
    }
    match answer(args.first().copied()) {
        Ok(out) => {
            print!("{out}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(2)
        }
    }
}

/// What truefit prints for its first argument: stdout on success, stderr on a usage error.
fn answer(command: Option<&str>) -> Result<String, String> {
    match command {
        None | Some("--help" | "-h" | "help") => Ok(MANUAL.to_string()),
        Some("--version" | "-V") => Ok(format!("truefit {}\n", env!("TRUEFIT_VERSION"))),
        Some(other) => Err(format!(
            "truefit: no command `{other}`; `truefit --help` is the manual"
        )),
    }
}

/// `truefit contract [--real [<name>]]`, run where the current directory is.
fn contract(rest: &[&str]) -> ExitCode {
    let real = match rest {
        [] => Ok(None),
        ["--real"] => Ok(Some(None)),
        ["--real", name] => Ok(Some(Some(*name))),
        _ => Err(vec![contract::fault(
            format!(
                "`truefit contract {}` is not a contract command",
                rest.join(" ")
            ),
            "run `truefit contract` or `truefit contract --real [<name>]`",
        )]),
    };
    let root = std::env::current_dir().unwrap_or_default();
    print(real.and_then(|real| contract::run(&root, real)))
}

/// `truefit sim [--seeds N | --budget <seconds> | --seed <s> | --forget <s> --why <reason>]`.
fn sim(rest: &[&str]) -> ExitCode {
    let ask = match rest {
        [] => Some(sim::Ask::Run(None)),
        ["--seeds", n] => n.parse().ok().map(|n| sim::Ask::Run(Some(sim::New::Seeds(n)))),
        ["--budget", s] => std::time::Duration::try_from_secs_f64(s.parse().unwrap_or(-1.0))
            .ok()
            .map(|d| sim::Ask::Run(Some(sim::New::Budget(d)))),
        ["--seed", seed] => Some(sim::Ask::Seed(seed)),
        ["--forget", seed, "--why", why] => Some(sim::Ask::Forget(seed, why)),
        _ => None,
    }
    .ok_or_else(|| {
        vec![contract::fault(
            format!("`truefit sim {}` is not a sim command", rest.join(" ")),
            "run `truefit sim [--seeds N | --budget <seconds>]`, `truefit sim --seed <s>` or `truefit sim --forget <s> --why <reason>`",
        )]
    });
    let root = std::env::current_dir().unwrap_or_default();
    print(ask.and_then(|ask| sim::run(&root, ask)))
}

/// `truefit model [<name>] [--trace <file>]`.
fn model(rest: &[&str]) -> ExitCode {
    let ask = match rest {
        [] => Some((None, None)),
        ["--trace", file] => Some((None, Some(*file))),
        [name] if !name.starts_with('-') => Some((Some(*name), None)),
        [name, "--trace", file] if !name.starts_with('-') => Some((Some(*name), Some(*file))),
        _ => None,
    }
    .ok_or_else(|| {
        vec![contract::fault(
            format!("`truefit model {}` is not a model command", rest.join(" ")),
            "run `truefit model [<name>]` or `truefit model [<name>] --trace <file>`",
        )]
    });
    let root = std::env::current_dir().unwrap_or_default();
    print(ask.and_then(|(name, trace)| model::run(&root, name, trace)))
}

/// Prints a command's answer: exit 0 when it passed, 1 when not, 2 on faults.
fn print(answer: Result<(serde_json::Value, bool), Vec<serde_json::Value>>) -> ExitCode {
    match answer {
        Ok((report, passed)) => {
            println!("{report:#}");
            ExitCode::from(if passed { 0 } else { 1 })
        }
        Err(faults) => {
            println!("{:#}", json!({"error": faults}));
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_names_truefit_and_the_stamped_version() {
        let version = Ok(format!("truefit {}\n", env!("TRUEFIT_VERSION")));
        assert_eq!(super::answer(Some("--version")), version);
        assert_eq!(super::answer(Some("-V")), version);
    }
}
