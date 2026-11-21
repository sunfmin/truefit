//! truefit: keeps a system's tests and models true to the real system.
//! `truefit --help` is the manual, and its only copy.

mod contract;

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
    match real.and_then(|real| contract::run(&root, real)) {
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
