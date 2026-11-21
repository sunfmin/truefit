//! truefit: keeps a system's tests and models true to the real system.
//! `truefit --help` is the manual, and its only copy.

mod contract;

use serde_json::json;
use std::process::ExitCode;

const MANUAL: &str = include_str!("manual.md");

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        [] | ["--help" | "-h" | "help", ..] => {
            print!("{MANUAL}");
            ExitCode::SUCCESS
        }
        ["--version" | "-V"] => {
            println!("truefit {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        ["contract", rest @ ..] => {
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
        [other, ..] => {
            eprintln!("truefit: no command `{other}`; `truefit --help` is the manual");
            ExitCode::from(2)
        }
    }
}
