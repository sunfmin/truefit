//! truefit: keeps a system's tests and models true to the real system.
//! `truefit --help` is the manual, and its only copy.

use std::process::ExitCode;

const MANUAL: &str = include_str!("manual.md");

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match answer(args.first().map(String::as_str)) {
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
        Some("--version" | "-V") => Ok(format!("truefit {}\n", env!("CARGO_PKG_VERSION"))),
        Some(other) => Err(format!(
            "truefit: no command `{other}`; `truefit --help` is the manual"
        )),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_names_truefit_and_its_package_version() {
        let version = Ok(format!("truefit {}\n", env!("CARGO_PKG_VERSION")));
        assert_eq!(super::answer(Some("--version")), version);
        assert_eq!(super::answer(Some("-V")), version);
    }
}
