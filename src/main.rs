//! truefit: keeps a system's tests and models true to the real system.
//! `truefit --help` is the manual, and its only copy.

use std::process::ExitCode;

const MANUAL: &str = include_str!("manual.md");

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("--help" | "-h" | "help") => {
            print!("{MANUAL}");
            ExitCode::SUCCESS
        }
        Some("--version" | "-V") => {
            println!("truefit {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("truefit: no command `{other}`; `truefit --help` is the manual");
            ExitCode::from(2)
        }
    }
}
